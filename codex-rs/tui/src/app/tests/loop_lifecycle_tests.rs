use super::*;
use crate::app_server_session::ResumeModelSettings;
use crate::app_server_session::StartupThreadOverrides;
use crate::app_server_session::ThreadParamsMode;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn loop_controls_are_registered_on_initial_new_resumed_and_forked_threads() -> Result<()> {
    let (app, _codex_home) = make_history_test_app().await?;
    let controller = Arc::new(
        crate::loop_mcp::LoopMcpServer::start(&app.config, app.app_event_tx.clone()).await?,
    );
    let (mut server, requests, proxy) = Box::pin(start_recording_app_server(
        &app.config,
        /*blocked_thread_list*/ None,
        /*failed_thread_name*/ None,
    ))
    .await?;
    server.loop_mcp = Some(controller.clone());
    let started = server.start_thread(&app.config).await?;
    crate::app_server_session::start_thread_with_request_handle(
        server.request_handle(),
        &app.local_settings,
        app.config.clone(),
        ThreadParamsMode::Embedded,
        /*remote_cwd_override*/ None,
        server.thread_tool_transport(),
        StartupThreadOverrides {
            model_provider: None,
            loop_mcp: Some(controller.clone()),
        },
    )
    .await?;
    server
        .resume_thread(
            &app.local_settings,
            app.config.clone(),
            started.session.thread_id,
            ResumeModelSettings::PreserveExistingThread,
        )
        .await?;
    server
        .fork_thread(
            &app.local_settings,
            app.config.clone(),
            started.session.thread_id,
        )
        .await?;
    let mut expected = None;
    controller.configure(&mut expected);
    let expected = expected.unwrap().remove("mcp_servers.ukis_loop").unwrap();
    for (method, count) in [
        ("thread/start", 2),
        ("thread/resume", 1),
        ("thread/fork", 1),
    ] {
        let calls = recorded_params(&requests, method);
        assert_eq!(calls.len(), count, "{method}");
        for params in calls {
            assert_eq!(
                params["config"]["mcp_servers.ukis_loop"], expected,
                "{method}"
            );
        }
    }
    server.shutdown().await?;
    proxy.await??;
    Ok(())
}

#[tokio::test]
async fn loop_cancel_reaches_the_controller_through_disconnected_input() -> Result<()> {
    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    let id = ThreadId::new();
    app.active_thread_id = Some(id);
    app.chat_widget
        .handle_thread_session(test_thread_session(id, app.config.cwd.to_path_buf()));
    app.chat_widget.handle_loop_command(
        "1m check",
        Instant::now(),
        crate::chatwidget::loops::LoopAvailability::FixedOnly,
    );
    assert!(app.chat_widget.has_loop_tasks());
    let mut server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    app.reconnect.offline = true;
    app.chat_widget
        .restore_user_message_to_composer("/loop stop".into());
    app.handle_tui_event(
        &mut tui,
        &mut server,
        TuiEvent::Key(KeyEvent::from(KeyCode::Enter)),
    )
    .await?;
    let mut dispatched = false;
    while let Ok(event) = events.try_recv() {
        if matches!(event, AppEvent::LoopCommand { .. }) {
            dispatched = true;
            app.handle_event(&mut tui, &mut server, event).await?;
        }
    }
    assert!(dispatched);
    assert!(!app.chat_widget.has_loop_tasks());
    app.handle_event(
        &mut tui,
        &mut server,
        AppEvent::LoopCommand {
            thread_id: Some(id),
            args: "1m another task".into(),
        },
    )
    .await?;
    assert!(!app.chat_widget.has_loop_tasks());
    let (reply, result) = tokio::sync::oneshot::channel();
    app.handle_event(
        &mut tui,
        &mut server,
        AppEvent::LoopToolCall {
            thread_id: id.to_string(),
            turn_id: "old-turn".into(),
            arguments: serde_json::json!({}),
            reply,
        },
    )
    .await?;
    assert!(result.await?.unwrap_err().contains("disconnected"));
    server.shutdown().await?;
    Ok(())
}
