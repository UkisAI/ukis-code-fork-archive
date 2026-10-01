//! Turn-lifecycle wiring for the claim gate.
//!
//! Mechanism: `on_item_completed` is awaited inline by core while the sampling response
//! is still being processed, before the turn loop computes
//! `needs_follow_up = model_needs_follow_up || has_pending_input`. Injecting the
//! correction with `CodexThread::inject_if_running` puts it in the pending input queue,
//! so the loop sees pending input, records the correction and samples again instead of
//! ending the turn. No core lock is held across `on_item_completed`, so taking the
//! active-turn lock inside `inject_if_running` cannot deadlock.

use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

use codex_core::ThreadManager;
use codex_core::context::ContextualUserFragment;
use codex_core::context::InternalContextSource;
use codex_core::context::InternalModelContextFragment;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionEventSink;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ExtensionWarning;
use codex_extension_api::TurnLifecycleContributor;
use codex_protocol::ThreadId;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::TurnItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::models::ResponseItem;

use crate::claim_gate::MAX_GATE_CORRECTIONS_PER_TURN;
use crate::claim_gate::correction_text;
use crate::claim_gate::unbacked_claims;

/// Installs the research claim gate into a thread extension registry.
pub fn install<C>(registry: &mut ExtensionRegistryBuilder<C>, thread_manager: Weak<ThreadManager>)
where
    C: Send + Sync + 'static,
{
    let extension = Arc::new(ResearchExtension {
        thread_manager,
        event_sink: registry.event_sink(),
    });
    registry.turn_lifecycle_contributor(extension);
}

struct ResearchExtension {
    thread_manager: Weak<ThreadManager>,
    event_sink: Arc<dyn ExtensionEventSink>,
}

/// Per-turn gate bookkeeping, dropped with the turn store.
#[derive(Default)]
struct GateTurnState {
    corrections: AtomicU32,
    gave_up: AtomicBool,
}

impl TurnLifecycleContributor for ResearchExtension {
    fn on_item_completed<'a>(
        &'a self,
        thread_store: &'a ExtensionData,
        turn_store: &'a ExtensionData,
        item: &'a TurnItem,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let TurnItem::AgentMessage(message) = item else {
                return;
            };
            // Phase is optional; providers that omit it are treated as final answers.
            if matches!(message.phase, Some(MessagePhase::Commentary)) {
                return;
            }
            let text = message
                .content
                .iter()
                .map(|AgentMessageContent::Text { text }| text.as_str())
                .collect::<String>();
            let claims = unbacked_claims(&text);
            if claims.is_empty() {
                return;
            }
            let thread_id = thread_store.level_id();
            let turn_id = turn_store.level_id();
            let state = turn_store.get_or_init::<GateTurnState>(GateTurnState::default);
            let attempt = state.corrections.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt > MAX_GATE_CORRECTIONS_PER_TURN {
                if !state.gave_up.swap(true, Ordering::SeqCst) {
                    let count = claims.len();
                    let message = format!(
                        "Research gate gave up after {MAX_GATE_CORRECTIONS_PER_TURN} corrections: the final answer still has {count} unbacked performance number(s). Treat them as UNVERIFIED."
                    );
                    tracing::warn!(%thread_id, %turn_id, "{message}");
                    self.event_sink.emit_warning(ExtensionWarning {
                        thread_id: thread_id.to_string(),
                        turn_id: Some(turn_id.to_string()),
                        message,
                    });
                }
                return;
            }
            let correction: ResponseItem =
                ContextualUserFragment::into(InternalModelContextFragment::new(
                    InternalContextSource::from_static("research_gate"),
                    correction_text(&claims),
                ));
            let Some(thread_manager) = self.thread_manager.upgrade() else {
                tracing::warn!("research gate skipped: thread manager is unavailable");
                return;
            };
            let Ok(parsed_thread_id) = ThreadId::from_string(thread_id) else {
                tracing::warn!(%thread_id, "research gate skipped: invalid thread id");
                return;
            };
            let Ok(thread) = thread_manager.get_thread(parsed_thread_id).await else {
                tracing::warn!(%thread_id, "research gate skipped: live thread is unavailable");
                return;
            };
            match thread.inject_if_running(vec![correction]).await {
                Ok(()) => tracing::info!(
                    %thread_id,
                    %turn_id,
                    attempt,
                    "research gate injected a correction for unbacked claims"
                ),
                Err(_) => tracing::warn!(
                    %thread_id,
                    %turn_id,
                    "research gate could not inject: no active turn"
                ),
            }
        })
    }
}
