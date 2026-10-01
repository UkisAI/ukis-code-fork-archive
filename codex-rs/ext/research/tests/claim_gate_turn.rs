#![allow(clippy::expect_used)]

//! End-to-end checks that the research claim gate keeps a turn running when the final
//! answer states an unbacked performance number.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use codex_extension_api::ExtensionEventSink;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ExtensionWarning;
use codex_protocol::protocol::Event;
use core_test_support::responses::ResponseMock;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;

const UNBACKED: &str = "Swift-7B scores 87.3% on GPQA Diamond.";
const UNKNOWN: &str = "The GPQA Diamond accuracy of Swift-7B is unknown: no validated run exists.";
const BACKED: &str = "Swift-7B scores 61.2% on GPQA Diamond [run:gpqa-swift7b-s0].";

#[derive(Default)]
struct RecordingSink {
    warnings: Mutex<Vec<String>>,
}

impl ExtensionEventSink for RecordingSink {
    fn emit(&self, _event: Event) {}

    fn emit_warning(&self, warning: ExtensionWarning) {
        self.warnings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(warning.message);
    }
}

fn answer(response_id: &str, text: &str) -> String {
    sse(vec![
        ev_response_created(response_id),
        ev_assistant_message(&format!("msg-{response_id}"), text),
        ev_completed(response_id),
    ])
}

/// Runs one user turn against `answers` with the gate installed; returns the mock and
/// the warnings the gate emitted.
async fn run_gated_turn(answers: &[&str]) -> anyhow::Result<(ResponseMock, Vec<String>)> {
    let server = start_mock_server().await;
    let bodies = answers
        .iter()
        .enumerate()
        .map(|(index, text)| answer(&format!("resp-{index}"), text))
        .collect();
    let mock = mount_sse_sequence(&server, bodies).await;
    let sink = Arc::new(RecordingSink::default());
    let event_sink: Arc<dyn ExtensionEventSink> = sink.clone();
    let registry = ExtensionRegistryBuilder::with_event_sink(event_sink).build();
    let test = test_codex()
        .with_extensions(Arc::new(registry))
        .with_extension_installer(codex_research_extension::install)
        .build_with_auto_env(&server)
        .await?;

    test.submit_turn("What GPQA Diamond accuracy does Swift-7B get?")
        .await?;

    let warnings = sink
        .warnings
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    Ok((mock, warnings))
}

fn has_correction(texts: &[String]) -> bool {
    texts
        .iter()
        .any(|text| text.contains("Research gate") && text.contains("87.3%"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unbacked_final_claim_continues_turn_with_correction() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let (mock, warnings) = run_gated_turn(&[UNBACKED, UNKNOWN]).await?;

    let requests = mock.requests();
    assert_eq!(2, requests.len());
    assert!(!has_correction(&requests[0].message_input_texts("user")));
    assert!(has_correction(&requests[1].message_input_texts("user")));
    // The rejected answer stays in history, immediately followed by the correction.
    let roles_and_texts = requests[1]
        .input()
        .iter()
        .filter(|item| item["type"] == "message")
        .map(|item| {
            let text = item["content"][0]["text"].as_str().unwrap_or_default();
            (
                item["role"].as_str().unwrap_or_default().to_string(),
                text.starts_with(UNBACKED) || text.contains("Research gate"),
            )
        })
        .filter(|(_, relevant)| *relevant)
        .map(|(role, _)| role)
        .collect::<Vec<_>>();
    assert_eq!(
        vec!["assistant".to_string(), "user".to_string()],
        roles_and_texts
    );
    assert_eq!(Vec::<String>::new(), warnings);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backed_final_claim_ends_turn() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let (mock, warnings) = run_gated_turn(&[BACKED]).await?;

    assert_eq!(1, mock.requests().len());
    assert_eq!(Vec::<String>::new(), warnings);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gate_gives_up_visibly_after_two_corrections() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let (mock, warnings) = run_gated_turn(&[UNBACKED, UNBACKED, UNBACKED]).await?;

    assert_eq!(3, mock.requests().len());
    assert_eq!(
        vec![
            "Research gate gave up after 2 corrections: the final answer still has 1 unbacked performance number(s). Treat them as UNVERIFIED."
                .to_string()
        ],
        warnings
    );
    Ok(())
}
