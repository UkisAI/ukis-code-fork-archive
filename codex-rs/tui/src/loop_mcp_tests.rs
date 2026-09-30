use super::*;
use pretty_assertions::assert_eq;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn loop_mcp_rejects_unauthenticated_calls_before_they_reach_the_ui() {
    let directory = tempfile::tempdir().unwrap();
    let config = Config::load_default_with_cli_overrides_for_codex_home(
        directory.path().to_path_buf(),
        Vec::new(),
    )
    .await
    .unwrap();
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let server = LoopMcpServer::start(&config, AppEventSender::new(tx))
        .await
        .unwrap();
    let url = url::Url::parse(server.config["url"].as_str().unwrap()).unwrap();
    let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", url.port().unwrap()))
        .await
        .unwrap();
    socket.write_all(b"POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
    let mut response = String::new();
    tokio::time::timeout(Duration::from_secs(5), socket.read_to_string(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(response.starts_with("HTTP/1.1 401"), "{response}");
    assert!(events.try_recv().is_err());
    let mut overrides = Some(HashMap::from([(
        "model_reasoning_effort".into(),
        json!("high"),
    )]));
    server.configure(&mut overrides);
    let overrides = overrides.unwrap();
    assert_eq!(overrides["model_reasoning_effort"], json!("high"));
    assert_eq!(overrides["mcp_servers.ukis_loop"], server.config);
}
