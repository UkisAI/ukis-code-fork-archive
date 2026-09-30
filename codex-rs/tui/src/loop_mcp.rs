//! Authenticated, local-only control of already-authorized adaptive loops.
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::legacy_core::config::Config;
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::Request;
use axum::http::StatusCode;
use axum::http::header::AUTHORIZATION;
use axum::middleware;
use axum::middleware::Next;
use axum::response::Response;
use codex_config::McpServerConfig;
use codex_config::RawMcpServerConfig;
use rmcp::ErrorData as McpError;
use rmcp::handler::server::ServerHandler;
use rmcp::model::CallToolRequestParams;
use rmcp::model::CallToolResult;
use rmcp::model::ContentBlock;
use rmcp::model::ListToolsResult;
use rmcp::model::PaginatedRequestParams;
use rmcp::model::ServerCapabilities;
use rmcp::model::ServerInfo;
use rmcp::model::Tool;
use rmcp::service::RequestContext;
use rmcp::service::RoleServer;
use rmcp::transport::StreamableHttpServerConfig;
use rmcp::transport::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::PoisonError;
use std::sync::RwLock;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;

pub(crate) const NAMESPACE: &str = "ukis_loop";

pub(crate) struct LoopMcpServer {
    sender: Arc<RwLock<AppEventSender>>,
    config: Value,
    task: JoinHandle<()>,
}

impl LoopMcpServer {
    pub(crate) async fn start(config: &Config, sender: AppEventSender) -> std::io::Result<Self> {
        if config.mcp_servers.get().contains_key(NAMESPACE) {
            return Err(std::io::Error::other(
                "The ukis_loop MCP namespace is already configured.",
            ));
        }
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let authorization = Arc::new(format!("Bearer {}", Uuid::new_v4()));
        let server_config = json!({
            "url": format!("http://{address}/mcp"),
            "http_headers": {"Authorization": authorization.as_str()},
            "default_tools_approval_mode": "approve"
        });
        if let Some(requirements) = config
            .config_layer_stack
            .requirements()
            .mcp_servers
            .as_ref()
        {
            let requirement = requirements.value.get(NAMESPACE).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "Managed MCP requirements do not permit adaptive loop controls.",
                )
            })?;
            let raw: RawMcpServerConfig =
                serde_json::from_value(server_config.clone()).map_err(std::io::Error::other)?;
            let configured = McpServerConfig::try_from(raw).map_err(std::io::Error::other)?;
            if !configured.matches_requirement(requirement) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "Adaptive loop controls do not match managed MCP requirements.",
                ));
            }
        }
        let sender = Arc::new(RwLock::new(sender));
        let handler = LoopHandler {
            sender: Arc::clone(&sender),
        };
        let service = StreamableHttpService::new(
            move || Ok(handler.clone()),
            Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig::default(),
        );
        let router =
            Router::new()
                .nest_service("/mcp", service)
                .layer(middleware::from_fn_with_state(
                    authorization,
                    require_authorization,
                ));
        let task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, router).await {
                tracing::warn!(%error, "Adaptive loop control server stopped");
            }
        });
        Ok(Self {
            sender,
            config: server_config,
            task,
        })
    }

    pub(crate) fn configure(&self, config: &mut Option<HashMap<String, Value>>) {
        config
            .get_or_insert_default()
            .insert(format!("mcp_servers.{NAMESPACE}"), self.config.clone());
    }

    pub(crate) fn reconnect(&self, sender: AppEventSender) {
        *self.sender.write().unwrap_or_else(PoisonError::into_inner) = sender;
    }
}

impl Drop for LoopMcpServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn require_authorization(
    State(expected): State<Arc<String>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if request
        .headers()
        .get(AUTHORIZATION)
        .is_some_and(|value| value.as_bytes() == expected.as_bytes())
    {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

#[derive(Clone)]
struct LoopHandler {
    sender: Arc<RwLock<AppEventSender>>,
}

impl ServerHandler for LoopHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let schema = json!({
            "type": "object", "additionalProperties": false,
            "properties": {
                "loop_id": {"type": "integer", "minimum": 1},
                "run_id": {"type": "string"},
                "delay_seconds": {"type": "integer", "minimum": 60, "maximum": 3600},
                "stop": {"type": "boolean"},
                "reason": {"type": "string", "minLength": 1, "maxLength": 500}
            },
            "required": ["loop_id", "run_id", "reason"]
        });
        let schema = serde_json::from_value(schema)
            .map_err(|error| McpError::internal_error(error.to_string(), None))?;
        Ok(ListToolsResult::with_all_items(vec![Tool::new(
            "schedule_wakeup",
            "Only during an active adaptive /loop iteration: choose the next delay (60-3600 seconds), or stop:true when complete. Use the loop_id and run_id from this iteration. Supply a brief reason. This cannot create loops, change permissions, or control another conversation.",
            Arc::new(schema),
        )]))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, McpError> {
        if request.name != "schedule_wakeup" {
            return Err(McpError::invalid_params("Unknown loop tool.", None));
        }
        let metadata = &context.meta.0.0;
        let turn_metadata = metadata
            .get("x-codex-turn-metadata")
            .and_then(|value| match value {
                Value::Object(_) => Some(value.clone()),
                Value::String(value) => serde_json::from_str(value).ok(),
                _ => None,
            });
        let thread_id = metadata
            .get("threadId")
            .and_then(Value::as_str)
            .or_else(|| turn_metadata.as_ref()?.get("thread_id")?.as_str())
            .ok_or_else(|| McpError::invalid_params("Missing conversation metadata.", None))?;
        let turn_id = metadata
            .get("turnId")
            .and_then(Value::as_str)
            .or_else(|| turn_metadata.as_ref()?.get("turn_id")?.as_str())
            .ok_or_else(|| McpError::invalid_params("Missing turn metadata.", None))?;
        let (reply, result) = oneshot::channel();
        self.sender
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .send(AppEvent::LoopToolCall {
                thread_id: thread_id.to_owned(),
                turn_id: turn_id.to_owned(),
                arguments: Value::Object(request.arguments.unwrap_or_default()),
                reply,
            });
        let result = tokio::time::timeout(Duration::from_secs(10), result)
            .await
            .map_err(|_| McpError::internal_error("Loop controller timed out.", None))?
            .map_err(|_| McpError::internal_error("Loop controller disconnected.", None))?;
        Ok(match result {
            Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
            Err(text) => CallToolResult::error(vec![ContentBlock::text(text)]),
        }
        .into())
    }
}

#[cfg(test)]
#[path = "loop_mcp_tests.rs"]
mod tests;
