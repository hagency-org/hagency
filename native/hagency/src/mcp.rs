//! Dedicated native MCP task helper. Task authority stays in the loopback API.
mod catalog;
mod coordination_catalog;
mod file_catalog;
pub(crate) mod json;
mod receive_catalog;
use crate::task_client::{self, Context, coordination, files, received};
use hagency_core::{JSON_SAFE_MAX, project::identifier, tasks::TaskMutation};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub const FRAME_LIMIT: usize = 32 * 1024;
pub const OUTPUT_LIMIT: usize = 256 * 1024;
const REQUEST_LIMIT: usize = 4096;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("native MCP protocol is invalid or exceeds its bound")]
    Protocol,
    #[error("native MCP IO failed")]
    Io,
    #[error("native MCP runner context is missing or invalid")]
    Context,
}
#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Initialize,
    Initialized,
    Ready,
}
/// One context and one connection. No public credential projection or store.
pub struct Session {
    context: Context,
    phase: Phase,
    ids: BTreeSet<String>,
    closed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}
impl Session {
    pub fn new(context: Context) -> Self {
        Self {
            context,
            phase: Phase::Initialize,
            ids: BTreeSet::new(),
            closed: false,
        }
    }
    /// One complete frame, without its newline. A fatal frame closes this session.
    pub async fn handle(&mut self, bytes: &[u8]) -> Result<Option<Value>, Error> {
        if self.closed {
            return Err(Error::Protocol);
        }
        let result = self.handle_inner(bytes).await;
        if result.is_err() {
            self.closed = true;
        }
        result
    }
    async fn handle_inner(&mut self, bytes: &[u8]) -> Result<Option<Value>, Error> {
        if bytes.is_empty() || bytes.len() > FRAME_LIMIT || bytes.contains(&b'\n') {
            return Err(Error::Protocol);
        }
        let value = json::json(bytes)?;
        // null is not a request ID and is not a notification with omitted ID.
        if value.get("id").is_some_and(Value::is_null) {
            return Err(Error::Protocol);
        }
        let request: Request = serde_json::from_value(value).map_err(|_| Error::Protocol)?;
        if request.jsonrpc != "2.0" || request.method.len() > 128 {
            return Err(Error::Protocol);
        }
        let Some(id) = request.id else {
            match request.method.as_str() {
                "notifications/initialized"
                    if self.phase == Phase::Initialized && empty(&request.params) =>
                {
                    self.phase = Phase::Ready
                }
                // Sequential operations finish before a later notification is read.
                // Cancellation does not undo a mutation or authorize a retry.
                "notifications/cancelled" if self.phase == Phase::Ready => {}
                _ => return Err(Error::Protocol),
            }
            return Ok(None);
        };
        let key = match &id {
            Value::String(s)
                if !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control) =>
            {
                format!("s:{s}")
            }
            Value::Number(n)
                if n.as_i64()
                    .is_some_and(|v| v.unsigned_abs() <= JSON_SAFE_MAX) =>
            {
                format!("n:{n}")
            }
            _ => return Err(Error::Protocol),
        };
        if self.ids.len() >= REQUEST_LIMIT || !self.ids.insert(key) {
            return Err(Error::Protocol);
        }
        let result = match request.method.as_str() {
            "initialize" if self.phase == Phase::Initialize => {
                let p = request.params.as_ref().ok_or(Error::Protocol)?;
                if p.get("protocolVersion")
                    .and_then(Value::as_str)
                    .is_none_or(|v| v.is_empty() || v.len() > 64)
                    || !p.get("capabilities").is_some_and(Value::is_object)
                    || !p.get("clientInfo").is_some_and(|v| {
                        v.get("name").is_some_and(Value::is_string)
                            && v.get("version").is_some_and(Value::is_string)
                    })
                {
                    return Err(Error::Protocol);
                }
                self.phase = Phase::Initialized;
                json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"hagency","version":env!("CARGO_PKG_VERSION")},"instructions":"Maintain the assigned task and coordinate only within current runner authority. Graph assignees are exact internal participant session IDs returned by conversations. Frames are limited to 32 KiB; page reads at most 32 items. Every mutation requires a stable call_id. A lost response is uncertain; inspect or retry the identical call_id and content."})
            }
            "ping" if empty(&request.params) => json!({}),
            "tools/list" if self.phase == Phase::Ready && empty(&request.params) => {
                catalog::list(self.context.file_tools(), self.context.receive_tools())
            }
            "tools/call" if self.phase == Phase::Ready => {
                if !valid_call(
                    request.params.as_ref(),
                    self.context.file_tools(),
                    self.context.receive_tools(),
                ) {
                    return Ok(Some(
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"Unknown tool or invalid tool request schema"}}),
                    ));
                }
                self.call(request.params).await?
            }
            _ => {
                return Ok(Some(
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method is unavailable in this MCP lifecycle"}}),
                ));
            }
        };
        let response = json!({"jsonrpc":"2.0","id":id,"result":result});
        if serde_json::to_vec(&response)
            .map_err(|_| Error::Protocol)?
            .len()
            > OUTPUT_LIMIT
        {
            return Err(Error::Protocol);
        }
        Ok(Some(response))
    }
    async fn call(&self, params: Option<Value>) -> Result<Value, Error> {
        let params = params.ok_or(Error::Protocol)?;
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or(Error::Protocol)?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if params.as_object().is_none_or(|p| {
            p.keys()
                .any(|k| !matches!(k.as_str(), "name" | "arguments" | "_meta"))
        }) {
            return Ok(tool_error("Unsupported tool request fields"));
        }
        if coordination::NAMES.contains(&name) {
            let command = match coordination::Command::parse(name, args) {
                Ok(command) => command,
                Err(error) => return Ok(tool_error(&error.to_string())),
            };
            return Ok(
                match coordination::run(&self.context, &command, task_client::DEFAULT_DEADLINE)
                    .await
                {
                    Ok(structured) => {
                        json!({"content":[{"type":"text","text":structured.to_string()}],"structuredContent":structured,"isError":false})
                    }
                    Err(error) => tool_error(&error.to_string()),
                },
            );
        }
        if received::NAMES.contains(&name) {
            let command = match received::Command::parse(name, args) {
                Ok(command) => command,
                Err(error) => return Ok(tool_error(&error.to_string())),
            };
            return Ok(
                match received::run(&self.context, &command, task_client::DEFAULT_DEADLINE).await {
                    Ok(value) => {
                        json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false})
                    }
                    Err(error) => tool_error(&error.to_string()),
                },
            );
        }
        if files::NAMES.contains(&name) {
            let command = match files::Command::parse(name, args) {
                Ok(command) => command,
                Err(error) => return Ok(tool_error(&error.to_string())),
            };
            return Ok(
                match files::run(&self.context, &command, task_client::DEFAULT_DEADLINE).await {
                    Ok(view) => {
                        let structured = serde_json::to_value(view).map_err(|_| Error::Protocol)?;
                        json!({"content":[{"type":"text","text":structured.to_string()}],"structuredContent":structured,"isError":false})
                    }
                    Err(error) => tool_error(&error.to_string()),
                },
            );
        }
        let Some(mut args) = args.as_object().cloned() else {
            return Ok(tool_error("Tool arguments must be an object"));
        };
        if args.remove("id").as_ref().and_then(Value::as_str) != Some(self.context.task_id()) {
            return Ok(tool_error("Task ID differs from the assigned task"));
        }
        let call_id = args.remove("call_id");
        let call_id = match &call_id {
            Some(Value::String(v)) if identifier(v, 512).is_ok() => Some(v.as_str()),
            Some(_) => return Ok(tool_error("Invalid stable call_id")),
            None => None,
        };
        if name == "complete_task_with_reply" {
            let Some(call_id) = call_id else {
                return Ok(tool_error("Missing stable call_id"));
            };
            let input =
                serde_json::from_value::<hagency_core::completions::CompleteTaskWithReply>(json!({
                    "id":self.context.task_id(),"call_id":call_id,"body":args.remove("body")
                }));
            if !args.is_empty() {
                return Ok(tool_error("Unsupported completion fields"));
            }
            let Ok(input) = input else {
                return Ok(tool_error("Invalid completion content"));
            };
            return Ok(
                match task_client::completion::run(
                    &self.context,
                    &input,
                    task_client::DEFAULT_DEADLINE,
                )
                .await
                {
                    Ok(v) => {
                        let structured = serde_json::to_value(v).map_err(|_| Error::Protocol)?;
                        json!({"content":[{"type":"text","text":structured.to_string()}],"structuredContent":structured,"isError":false})
                    }
                    Err(e) => tool_error(&e.to_string()),
                },
            );
        }
        let action = match name {
            "get_task" if args.is_empty() && call_id.is_none() => None,
            "accept_task" => Some("accept"),
            "transition_task" => Some("transition"),
            "comment_task" => Some("comment"),
            "update_task_execution" => {
                if !args.contains_key("heartbeat") {
                    args.insert("heartbeat".into(), false.into());
                }
                Some("execution")
            }
            _ => return Ok(tool_error("Unknown tool or unsupported arguments")),
        };
        let operation = if let Some(action) = action {
            if call_id.is_none() || args.contains_key("action") {
                return Ok(tool_error(
                    "Mutation requires call_id and exact tool arguments",
                ));
            }
            args.insert("action".into(), action.into());
            match serde_json::from_value::<TaskMutation>(Value::Object(args)) {
                Ok(v) => Some(v),
                Err(_) => return Ok(tool_error("Invalid task operation arguments")),
            }
        } else {
            None
        };
        match task_client::run_operation(
            &self.context,
            operation,
            call_id,
            task_client::DEFAULT_DEADLINE,
        )
        .await
        {
            Ok(v) => {
                let structured = serde_json::to_value(v).map_err(|_| Error::Protocol)?;
                Ok(
                    json!({"content":[{"type":"text","text":structured.to_string()}],"structuredContent":structured,"isError":false}),
                )
            }
            Err(e) => Ok(tool_error(&e.to_string())),
        }
    }
}
fn empty(value: &Option<Value>) -> bool {
    value.as_ref().is_none_or(|v| {
        v.as_object()
            .is_some_and(|o| o.iter().all(|(k, v)| k == "_meta" && v.is_object()))
    })
}
fn tool_error(message: &str) -> Value {
    json!({"content":[{"type":"text","text":message}],"isError":true})
}

fn valid_call(params: Option<&Value>, file_tools: bool, receive_tools: bool) -> bool {
    let Some(p) = params.and_then(Value::as_object) else {
        return false;
    };
    (p.get("name").and_then(Value::as_str).is_some_and(|name| {
        coordination::NAMES.contains(&name)
            || (file_tools && files::NAMES.contains(&name))
            || (receive_tools && received::NAMES.contains(&name))
    }) || matches!(
        p.get("name").and_then(Value::as_str),
        Some(
            "get_task"
                | "accept_task"
                | "transition_task"
                | "comment_task"
                | "update_task_execution"
                | "complete_task_with_reply"
        )
    )) && p
        .keys()
        .all(|k| matches!(k.as_str(), "name" | "arguments" | "_meta"))
        && p.get("arguments").is_none_or(Value::is_object)
        && p.get("_meta").is_none_or(Value::is_object)
}
