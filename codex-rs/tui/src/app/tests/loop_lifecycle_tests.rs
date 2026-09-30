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
