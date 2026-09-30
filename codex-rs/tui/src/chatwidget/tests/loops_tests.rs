use super::*;
use crate::chatwidget::loops::LoopAvailability;
use crate::loop_scheduler::Cadence;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn loop_list_renders_schedule_and_stop_cancels_it() {
    let (mut chat, mut events, _operations) = make_chatwidget_manual(None).await;
    chat.thread_id = Some(ThreadId::new());
    let now = Instant::now();
    chat.handle_loop_command("5m check CI", now, LoopAvailability::Adaptive);
    while events.try_recv().is_ok() {}
    chat.handle_loop_command("list", now, LoopAvailability::Adaptive);
    let rendered = drain_insert_history(&mut events)
        .into_iter()
        .flatten()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(rendered, @r"
    Loops in this conversation
    1: every 5m; due in 300s; check CI
    /loop stop <id> or /loop stop all
    ");
    chat.handle_loop_command("stop 1", now, LoopAvailability::Adaptive);
    assert!(!chat.has_loop_tasks());
}

#[tokio::test]
async fn loop_waits_for_user_input_and_submits_literal_prompt_once() {
    let (mut chat, _events, mut operations) = make_chatwidget_manual(None).await;
    chat.thread_id = Some(ThreadId::new());
    let now = Instant::now();
    chat.handle_loop_command(
        "1m !echo treat as model input",
        now,
        LoopAvailability::Adaptive,
    );
    chat.bottom_pane
        .set_composer_text("my draft".into(), Vec::new(), Vec::new());
    chat.poll_loops(now + Duration::from_secs(60));
    assert!(operations.try_recv().is_err());
    chat.bottom_pane
        .set_composer_text(String::new(), Vec::new(), Vec::new());
    chat.poll_loops(now + Duration::from_secs(600));
    let Op::UserTurn {
        items,
        cwd,
        approval_policy,
        model,
        ..
    } = next_submit_op(&mut operations)
    else {
        panic!("expected ordinary user turn");
    };
    assert_eq!(
        items,
        vec![UserInput::Text {
            text: "!echo treat as model input".into(),
            text_elements: Vec::new(),
        }]
    );
    assert_eq!(cwd, chat.config.cwd.as_path());
    assert_eq!(
        approval_policy,
        chat.config.permissions.approval_policy.value().into()
    );
    assert_eq!(model, chat.current_model());
    chat.poll_loops(now + Duration::from_secs(1200));
    assert!(operations.try_recv().is_err());
    handle_turn_started(&mut chat, "loop-turn");
    handle_turn_interrupted(&mut chat, "loop-turn");
    assert!(!chat.has_loop_tasks());
}

#[tokio::test]
async fn loop_preserves_queue_priority_and_busy_turns() {
    let (mut chat, _events, mut operations) = make_chatwidget_manual(None).await;
    chat.thread_id = Some(ThreadId::new());
    let now = Instant::now();
    chat.handle_loop_command("1m check", now, LoopAvailability::Adaptive);
    chat.input_queue.user_turn_pending_start = true;
    chat.poll_loops(now + Duration::from_secs(60));
    assert!(operations.try_recv().is_err());
    chat.input_queue.user_turn_pending_start = false;
    chat.input_queue
        .queued_user_messages
        .push_back(UserMessage::from("user first").into());
    chat.poll_loops(now + Duration::from_secs(60));
    assert!(operations.try_recv().is_err());
    assert_eq!(chat.queued_user_message_texts(), vec!["user first"]);
}

#[tokio::test]
async fn loop_adaptive_tool_is_bound_to_its_conversation_turn_and_iteration() {
    let (mut chat, _events, _operations) = make_chatwidget_manual(None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    let now = Instant::now();
    let id = chat
        .loop_scheduler
        .add(Some("check".into()), Cadence::Adaptive, now)
        .unwrap();
    let fire = chat.loop_scheduler.take_due(now).unwrap();
    chat.loop_scheduler.bind_turn("turn");
    let args = serde_json::json!({
        "loop_id": id, "run_id": fire.run_id.to_string(), "delay_seconds": 60, "reason": "Build is still running"
    });
    assert!(
        chat.handle_loop_tool(&ThreadId::new().to_string(), "turn", args.clone())
            .is_err()
    );
    assert!(
        chat.handle_loop_tool(&thread_id.to_string(), "wrong-turn", args.clone())
            .is_err()
    );
    assert!(
        chat.handle_loop_tool(&thread_id.to_string(), "turn", args.clone())
            .is_ok()
    );
    chat.handle_loop_command("stop", now, LoopAvailability::Adaptive);
    assert!(
        chat.handle_loop_tool(&thread_id.to_string(), "turn", args)
            .is_err()
    );
    assert_eq!(chat.loop_scheduler.finish(now), None);
    assert!(!chat.has_loop_tasks());
}

#[tokio::test]
async fn loop_default_file_reloads_and_explicit_prompt_ignores_it() {
    let (mut chat, _events, mut operations) = make_chatwidget_manual(None).await;
    chat.thread_id = Some(ThreadId::new());
    let project = tempfile::tempdir().unwrap();
    chat.config.cwd = project.path().to_path_buf().abs();
    std::fs::create_dir(project.path().join(".ukis")).unwrap();
    let path = project.path().join(".ukis/loop.md");
    std::fs::write(&path, "old default").unwrap();
    let now = Instant::now();
    chat.handle_loop_command("1m", now, LoopAvailability::Adaptive);
    std::fs::write(&path, "new default").unwrap();
    chat.poll_loops(now + Duration::from_secs(60));
    let Op::UserTurn { items, .. } = next_submit_op(&mut operations) else {
        panic!("expected turn")
    };
    assert_eq!(
        items,
        vec![UserInput::Text {
            text: "new default".into(),
            text_elements: Vec::new()
        }]
    );
    handle_turn_started(&mut chat, "turn");
    handle_turn_completed(&mut chat, "turn", None);
    chat.handle_loop_command("stop", now, LoopAvailability::Adaptive);
    std::fs::write(&path, "").unwrap();
    chat.handle_loop_command("1m explicit task", now, LoopAvailability::Adaptive);
    assert!(chat.has_loop_tasks());
}

#[tokio::test]
async fn loop_escape_cancels_adaptive_without_touching_fixed_schedule() {
    let (mut chat, _events, _operations) = make_chatwidget_manual(None).await;
    chat.thread_id = Some(ThreadId::new());
    let now = Instant::now();
    chat.handle_loop_command("check", now, LoopAvailability::Adaptive);
    chat.handle_loop_command("5m check", now, LoopAvailability::Adaptive);
    chat.handle_key_event(KeyEvent::from(KeyCode::Esc));
    assert_eq!(chat.loop_scheduler.tasks.len(), 1);
    assert_eq!(
        chat.loop_scheduler.tasks[0].cadence,
        Cadence::Fixed(Duration::from_secs(300))
    );
}

#[tokio::test]
async fn loop_adaptive_is_rejected_without_its_control_transport() {
    let (mut chat, _events, _operations) = make_chatwidget_manual(None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.handle_loop_command("check", Instant::now(), LoopAvailability::FixedOnly);
    assert!(!chat.has_loop_tasks());
    chat.handle_loop_command("1m check", Instant::now(), LoopAvailability::FixedOnly);
    assert!(chat.has_loop_tasks());
}
