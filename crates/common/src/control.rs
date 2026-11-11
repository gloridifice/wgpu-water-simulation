//! Optional MCP adapter for a single application-owned scene. No simulation types live here.
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::Deserialize;
use serde_json::Value;

/// Strict, transactional patch against an application's complete validated settings.
/// Traversal knows only JSON containers; policy and ranges stay with `Settings`.
pub fn patch_settings<T: crate::settings::Settings>(
    current: &T,
    changes: Value,
) -> Result<T, String> {
    let changes = changes.as_object().ok_or("changes must be a JSON object")?;
    if changes.is_empty() {
        return Err("changes must not be empty".into());
    }
    let mut document = serde_json::to_value(current).map_err(|e| e.to_string())?;
    for (path, value) in changes {
        let mut cursor = &mut document;
        let mut segments = path.split('.').peekable();
        while let Some(segment) = segments.next() {
            if segment.is_empty() {
                return Err(format!("invalid path: {path}"));
            }
            cursor = match cursor {
                Value::Object(map) => map.get_mut(segment),
                Value::Array(items) => segment.parse::<usize>().ok().and_then(|i| items.get_mut(i)),
                _ => None,
            }
            .ok_or_else(|| format!("unknown path: {path}"))?;
            if segments.peek().is_none() {
                if cursor.is_object()
                    || value.is_object()
                    || cursor.is_array() && !value.is_null()
                    || value.is_array() && !cursor.is_null()
                {
                    return Err(format!("set scalar fields individually: {path}"));
                }
                // Nullable array fields may switch between null and a whole array.
                // Typed deserialization and the application's validation still decide validity.
                *cursor = value.clone();
            }
        }
    }
    let next: T = serde_json::from_value(document).map_err(|e| e.to_string())?;
    next.validate()?;
    Ok(next)
}
use std::sync::mpsc::SyncSender;
use tokio::sync::oneshot;

pub enum Command {
    Describe,
    Get,
    Patch(Value),
    Action(String),
    Step(u32),
    Capture(String),
}

pub struct Request {
    pub command: Command,
    pub reply: oneshot::Sender<Result<Value, String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PatchArgs {
    /// JSON object mapping dotted configuration paths to values; applied as one transaction.
    changes: Value,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ActionArgs {
    /// Stable action ID from scene_describe.
    id: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct StepArgs {
    /// Fixed 1/60-second steps (1..=600); does not use wall-clock time.
    frames: u32,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CaptureArgs {
    /// New PNG file path; existing files are never replaced.
    path: String,
}

#[derive(Clone)]
struct SceneServer {
    tx: SyncSender<Request>,
}
#[tool_router]
impl SceneServer {
    fn new(tx: SyncSender<Request>) -> Self {
        Self { tx }
    }
    async fn request(&self, command: Command) -> CallToolResult {
        let (reply, rx) = oneshot::channel();
        if self.tx.try_send(Request { command, reply }).is_err() {
            return CallToolResult::error(vec![ContentBlock::text(
                "scene command queue full or closed",
            )]);
        }
        match rx.await {
            Ok(Ok(value)) => {
                let mut result =
                    CallToolResult::success(vec![ContentBlock::text(value.to_string())]);
                result.structured_content = Some(value);
                result
            }
            Ok(Err(error)) => CallToolResult::error(vec![ContentBlock::text(error)]),
            Err(_) => {
                CallToolResult::error(vec![ContentBlock::text("scene stopped before responding")])
            }
        }
    }
    #[tool(description = "Describe this scene, editable parameter paths and available actions")]
    async fn scene_describe(&self) -> CallToolResult {
        self.request(Command::Describe).await
    }
    #[tool(description = "Read the live scene configuration and simulation clock")]
    async fn parameters_get(&self) -> CallToolResult {
        self.request(Command::Get).await
    }
    #[tool(
        description = "Atomically change one or more existing configuration fields; does not save to disk"
    )]
    async fn parameters_patch(&self, Parameters(args): Parameters<PatchArgs>) -> CallToolResult {
        self.request(Command::Patch(args.changes)).await
    }
    #[tool(
        description = "Execute a named scene action (apply draft, reset, lake emission, or save)"
    )]
    async fn action_call(&self, Parameters(args): Parameters<ActionArgs>) -> CallToolResult {
        self.request(Command::Action(args.id)).await
    }
    #[tool(
        description = "Advance the current scene by fixed 1/60-second increments; no wall-clock dependence"
    )]
    async fn simulation_step(&self, Parameters(args): Parameters<StepArgs>) -> CallToolResult {
        self.request(Command::Step(args.frames)).await
    }
    #[tool(description = "Render the current live scene to a new PNG file")]
    async fn frame_capture(&self, Parameters(args): Parameters<CaptureArgs>) -> CallToolResult {
        self.request(Command::Capture(args.path)).await
    }
}
#[tool_handler]
impl ServerHandler for SceneServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
    }
}

/// stdio belongs exclusively to MCP; applications log to stderr.
pub async fn serve_stdio(tx: SyncSender<Request>) -> Result<(), crate::Error> {
    SceneServer::new(tx).serve(stdio()).await?.waiting().await?;
    Ok(())
}
