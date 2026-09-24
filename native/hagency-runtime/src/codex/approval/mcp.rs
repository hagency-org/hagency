//! TS correlateMcp parity. Only structured, scoped item facts identify a tool.
use super::{
    ApprovalRequest, Common, Error, Kind, MAX_APPROVAL_BYTES, RequestId, Value, fields,
    required_text,
};
use std::collections::BTreeMap;

const MAX_RETAINED_BYTES: usize = 256 * 1024;
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Call {
    pub server: String,
    pub tool: String,
    pub arguments: Value,
}
struct Candidate {
    call: Call,
    active: bool,
    consumed: bool,
}
#[derive(Default)]
pub(crate) struct McpTracker {
    items: BTreeMap<String, Candidate>,
    bytes: usize,
}
impl McpTracker {
    // Called only after the session validates scope, timestamp and lifecycle.
    pub(crate) fn observe(&mut self, method: &str, params: &Value) -> Result<(), Error> {
        if !matches!(method, "item/started" | "item/completed")
            || params["item"]["type"] != "mcpToolCall"
        {
            return Ok(());
        }
        let item = &params["item"];
        let id = item["id"].as_str().ok_or(Error::Malformed)?;
        if method == "item/completed" {
            if let Some(candidate) = self.items.get_mut(id) {
                candidate.active = false;
            }
            return Ok(());
        }
        if self.items.contains_key(id) {
            return Err(Error::Scope);
        }
        if self.items.len() >= super::super::session::MAX_ITEMS {
            return Err(Error::Capacity);
        }
        // Ordinary tool activity need not be an approval candidate. Match the
        // TS predicate: an absent/unsupported argument shape cannot correlate,
        // but does not itself turn an otherwise valid item into an approval.
        if required_text(item, "server", 255).is_err()
            || required_text(item, "tool", 255).is_err()
            || item["status"] != "inProgress"
            || !item["arguments"].is_object()
        {
            return Ok(());
        }
        let size = serde_json::to_vec(item)
            .map_err(|_| Error::Malformed)?
            .len();
        if size > MAX_APPROVAL_BYTES {
            return Err(Error::Capacity);
        }
        self.bytes = self
            .bytes
            .checked_add(size)
            .filter(|n| *n <= MAX_RETAINED_BYTES)
            .ok_or(Error::Capacity)?;
        self.items.insert(
            id.into(),
            Candidate {
                call: Call {
                    server: item["server"].as_str().ok_or(Error::Malformed)?.into(),
                    tool: item["tool"].as_str().ok_or(Error::Malformed)?.into(),
                    arguments: item["arguments"].clone(),
                },
                active: true,
                consumed: false,
            },
        );
        Ok(())
    }
    pub(crate) fn active(&self, id: &str) -> bool {
        self.items.get(id).is_some_and(|c| c.active && c.consumed)
    }
    pub(crate) fn request(
        &mut self,
        id: RequestId,
        params: Value,
    ) -> Result<ApprovalRequest, Error> {
        if !id.valid()
            || serde_json::to_vec(&params)
                .map_err(|_| Error::Malformed)?
                .len()
                > MAX_APPROVAL_BYTES
        {
            return Err(Error::Capacity);
        }
        fields(
            &params,
            &[
                "threadId",
                "turnId",
                "serverName",
                "mode",
                "message",
                "requestedSchema",
                "_meta",
            ],
            &[],
        )?;
        for field in ["threadId", "turnId", "serverName"] {
            required_text(&params, field, 255)?;
        }
        required_text(&params, "message", 8192)?;
        let schema = &params["requestedSchema"];
        fields(schema, &["type", "properties", "required"], &[])?;
        if params["mode"] != "form"
            || schema["type"] != "object"
            || !schema["properties"]
                .as_object()
                .is_some_and(|v| v.is_empty())
            || schema
                .get("required")
                .is_some_and(|v| !v.as_array().is_some_and(|a| a.is_empty()))
            || params["_meta"]["codex_approval_kind"] != "mcp_tool_call"
            || !params["_meta"]["tool_params"].is_object()
        {
            return Err(Error::Policy);
        }
        let mut matches = self.items.iter().filter(|(_, c)| {
            c.active
                && !c.consumed
                && c.call.server == params["serverName"]
                && c.call.arguments == params["_meta"]["tool_params"]
        });
        let item_id = matches
            .next()
            .map(|(key, _)| key.clone())
            .ok_or(Error::Scope)?;
        if matches.next().is_some() {
            return Err(Error::Scope);
        }
        let candidate = self.items.get_mut(&item_id).ok_or(Error::Scope)?;
        candidate.consumed = true;
        Ok(ApprovalRequest {
            id,
            method: "mcpServer/elicitation/request".into(),
            common: Common {
                thread_id: params["threadId"].as_str().ok_or(Error::Malformed)?.into(),
                turn_id: params["turnId"].as_str().ok_or(Error::Malformed)?.into(),
                item_id,
            },
            params,
            kind: Kind::Mcp(Box::new(candidate.call.clone())),
        })
    }
}
