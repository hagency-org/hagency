//! Pinned Codex 0.153.4 request shapes. Parsed data is never owner authority.
use super::{RequestId, session::Error, text};
use serde::Deserialize;
use serde_json::{Value, json};

pub const MAX_APPROVAL_BYTES: usize = 64 * 1024;

#[derive(Clone, PartialEq)]
pub struct ApprovalRequest {
    id: RequestId,
    method: String,
    params: Value,
    common: Common,
    kind: Kind,
}
#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Command,
    File,
    Permissions(Value),
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Common {
    thread_id: String,
    turn_id: String,
    item_id: String,
    started_at_ms: u64,
}

// Do not expose a generic result Value or Deserialize constructor. The host
// coordinator creates this only after the repository persists Applying.
pub struct ApprovalResponse {
    pub(super) request: ApprovalRequest,
    pub(super) result: Value,
}

impl ApprovalRequest {
    pub fn id(&self) -> &RequestId {
        &self.id
    }
    pub fn method(&self) -> &str {
        &self.method
    }
    pub fn params(&self) -> &Value {
        &self.params
    }
    pub fn thread_id(&self) -> &str {
        &self.common.thread_id
    }
    pub fn turn_id(&self) -> &str {
        &self.common.turn_id
    }
    pub fn item_id(&self) -> &str {
        &self.common.item_id
    }

    pub(super) fn parse(id: RequestId, method: String, params: Value) -> Result<Self, Error> {
        if !id.valid()
            || serde_json::to_vec(&params)
                .map_err(|_| Error::Malformed)?
                .len()
                > MAX_APPROVAL_BYTES
        {
            return Err(Error::Capacity);
        }
        let common: Common =
            serde_json::from_value(params.clone()).map_err(|_| Error::Malformed)?;
        if [&common.thread_id, &common.turn_id, &common.item_id]
            .iter()
            .any(|v| !text(v, 255))
            || common.started_at_ms > 9_007_199_254_740_991
        {
            return Err(Error::Malformed);
        }
        let shared = ["threadId", "turnId", "itemId", "startedAtMs", "reason"];
        optional_text(&params, "reason", 8192)?;
        let kind = match method.as_str() {
            "item/commandExecution/requestApproval" => {
                fields(
                    &params,
                    &shared,
                    &[
                        "kind",
                        "approvalId",
                        "environmentId",
                        "command",
                        "cwd",
                        "commandActions",
                        "additionalPermissions",
                        "networkApprovalContext",
                        "proposedExecpolicyAmendment",
                        "proposedNetworkPolicyAmendments",
                        "availableDecisions",
                    ],
                )?;
                if params.get("kind").is_some_and(|v| v != "command") {
                    return Err(Error::Policy);
                }
                for field in ["approvalId", "environmentId", "command", "cwd"] {
                    optional_text(&params, field, 8192)?;
                }
                // Alternate decision sets must still offer exactly the two
                // responses this adapter can issue. Never apply amendments.
                if let Some(v) = present(&params, "availableDecisions") {
                    let values = v
                        .as_array()
                        .filter(|a| a.len() <= 16)
                        .ok_or(Error::Malformed)?;
                    if !values.contains(&json!("accept")) || !values.contains(&json!("decline")) {
                        return Err(Error::Policy);
                    }
                }
                if let Some(v) = present(&params, "commandActions")
                    && !v
                        .as_array()
                        .is_some_and(|a| a.len() <= 128 && a.iter().all(Value::is_object))
                {
                    return Err(Error::Malformed);
                }
                if let Some(v) = present(&params, "additionalPermissions") {
                    profile(v)?;
                }
                if let Some(net) = present(&params, "networkApprovalContext") {
                    fields(net, &["host", "protocol"], &[])?;
                    required_text(net, "host", 8192)?;
                    if !matches!(
                        net.get("protocol").and_then(Value::as_str),
                        Some("http" | "https" | "socks5Tcp" | "socks5Udp")
                    ) {
                        return Err(Error::Malformed);
                    }
                    // Network callbacks are not arbitrary command callbacks.
                    if present(&params, "command").is_some()
                        || present(&params, "cwd").is_some()
                        || present(&params, "additionalPermissions").is_some()
                    {
                        return Err(Error::Policy);
                    }
                } else {
                    required_text(&params, "command", 8192)?;
                    required_text(&params, "cwd", 8192)?;
                }
                Kind::Command
            }
            "item/fileChange/requestApproval" => {
                fields(&params, &shared, &["grantRoot"])?;
                // The grant-root UI asks for session-wide write authority.
                if present(&params, "grantRoot").is_some() {
                    return Err(Error::Policy);
                }
                Kind::File
            }
            "item/permissions/requestApproval" => {
                fields(&params, &shared, &["environmentId", "cwd", "permissions"])?;
                optional_text(&params, "environmentId", 8192)?;
                required_text(&params, "cwd", 8192)?;
                let permissions = params.get("permissions").ok_or(Error::Malformed)?;
                profile(permissions)?;
                Kind::Permissions(permissions.clone())
            }
            _ => return Err(Error::UnsupportedRequest),
        };
        Ok(Self {
            id,
            method,
            params,
            common,
            kind,
        })
    }

    /// A host wire primitive, not a verdict setter. Callers must first consume
    /// durable approval authority; the coordinator owns that sequence. These
    /// are exact once/decline responses, never session or policy amendments.
    pub fn response(&self, allow: bool) -> ApprovalResponse {
        let result = match &self.kind {
            Kind::Command | Kind::File => {
                json!({"decision": if allow { "accept" } else { "decline" }})
            }
            Kind::Permissions(profile) => {
                json!({"permissions": if allow { profile.clone() } else { json!({}) }, "scope":"turn"})
            }
        };
        ApprovalResponse {
            request: self.clone(),
            result,
        }
    }
}
fn fields(value: &Value, common: &[&str], extra: &[&str]) -> Result<(), Error> {
    if value.as_object().is_some_and(|o| {
        o.keys()
            .all(|k| common.contains(&k.as_str()) || extra.contains(&k.as_str()))
    }) {
        Ok(())
    } else {
        Err(Error::Malformed)
    }
}
fn present<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|v| !v.is_null())
}
fn required_text(value: &Value, key: &str, limit: usize) -> Result<(), Error> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| text(s, limit))
        .map(|_| ())
        .ok_or(Error::Malformed)
}
fn optional_text(value: &Value, key: &str, limit: usize) -> Result<(), Error> {
    if present(value, key).is_some() {
        required_text(value, key, limit)
    } else {
        Ok(())
    }
}
// Supported literal-path subset of the pinned profile schema. This validates
// wire shape only; reusable scope and lease authority remain in core/store.
fn profile(value: &Value) -> Result<(), Error> {
    fields(value, &["network", "fileSystem"], &[])?;
    if let Some(net) = present(value, "network") {
        fields(net, &["enabled"], &[])?;
        if !net.get("enabled").is_some_and(Value::is_boolean) {
            return Err(Error::Malformed);
        }
    }
    if let Some(fs) = present(value, "fileSystem") {
        fields(fs, &["read", "write", "entries", "globScanMaxDepth"], &[])?;
        if present(fs, "globScanMaxDepth").is_some() {
            return Err(Error::Policy);
        }
        for key in ["read", "write"] {
            if let Some(paths) = present(fs, key)
                && !paths.as_array().is_some_and(|a| {
                    a.len() <= 64 && a.iter().all(|p| p.as_str().is_some_and(|s| text(s, 8192)))
                })
            {
                return Err(Error::Malformed);
            }
        }
        if let Some(entries) = present(fs, "entries") {
            let entries = entries
                .as_array()
                .filter(|a| a.len() <= 64)
                .ok_or(Error::Malformed)?;
            for entry in entries {
                fields(entry, &["access", "path"], &[])?;
                if !matches!(
                    entry.get("access").and_then(Value::as_str),
                    Some("read" | "write")
                ) {
                    return Err(Error::Policy);
                }
                let path = entry.get("path").ok_or(Error::Malformed)?;
                fields(path, &["type", "path"], &[])?;
                if path.get("type") != Some(&json!("path")) {
                    return Err(Error::Policy);
                }
                required_text(path, "path", 8192)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::{Connection, Message, encode};
    fn params() -> Value {
        json!({"threadId":"thread","turnId":"turn","itemId":"item","startedAtMs":1,"command":"echo x","cwd":"/tmp"})
    }
    fn parse(p: Value) -> Result<ApprovalRequest, Error> {
        ApprovalRequest::parse(
            RequestId::Number(7),
            "item/commandExecution/requestApproval".into(),
            p,
        )
    }
    #[test]
    fn native_codex_approval_mapping() {
        trait Ambiguous<A> {
            fn check() {}
        }
        impl<T: ?Sized> Ambiguous<()> for T {}
        impl<T: serde::de::DeserializeOwned> Ambiguous<u8> for T {}
        let _ = <ApprovalRequest as Ambiguous<_>>::check;
        let _ = <ApprovalResponse as Ambiguous<_>>::check;
        let request = parse(params()).unwrap();
        assert_eq!(request.response(true).result, json!({"decision":"accept"}));
        assert_eq!(
            request.response(false).result,
            json!({"decision":"decline"})
        );
        for (key, value) in [
            ("threadId", json!(null)),
            ("itemId", json!("")),
            ("startedAtMs", json!(-1)),
            ("kind", json!("writeStdin")),
            ("kind", json!(null)),
            ("command", json!(3)),
            ("environmentId", json!({})),
            ("owner", json!("fake")),
            ("availableDecisions", json!(["acceptForSession", "decline"])),
            ("commandActions", json!(true)),
        ] {
            let mut p = params();
            p[key] = value;
            assert!(parse(p).is_err(), "{key}");
        }
        let mut p = params();
        p["reason"] = json!("x".repeat(MAX_APPROVAL_BYTES));
        assert!(parse(p).is_err());
        let common = json!({"threadId":"thread","turnId":"turn","itemId":"patch","startedAtMs":1});
        let file = ApprovalRequest::parse(
            RequestId::String("7".into()),
            "item/fileChange/requestApproval".into(),
            common.clone(),
        )
        .unwrap();
        assert_ne!(request.id(), file.id());
        assert_eq!(file.response(true).result, json!({"decision":"accept"}));
        let mut p = common.clone();
        p["grantRoot"] = json!("/work");
        assert!(ApprovalRequest::parse(RequestId::Number(7), file.method().into(), p).is_err());
        let mut p = common;
        p["cwd"] = json!("/work");
        p["permissions"] = json!({"fileSystem":{"read":["/one"],"write":["/two"],"entries":[{"access":"write","path":{"type":"path","path":"/three"}}]},"network":{"enabled":true}});
        let permission = ApprovalRequest::parse(
            RequestId::Number(7),
            "item/permissions/requestApproval".into(),
            p.clone(),
        )
        .unwrap();
        assert_eq!(
            permission.response(true).result,
            json!({"permissions":p["permissions"],"scope":"turn"})
        );
        assert_eq!(
            permission.response(false).result,
            json!({"permissions":{},"scope":"turn"})
        );
        for bad in [
            json!({"extra":true}),
            json!({"network":{"enabled":true,"host":"fake"}}),
            json!({"fileSystem":{"globScanMaxDepth":1}}),
            json!({"fileSystem":{"entries":[{"access":"write","path":{"type":"special","value":"root"}}]}}),
        ] {
            p["permissions"] = bad;
            assert!(
                ApprovalRequest::parse(RequestId::Number(7), permission.method().into(), p.clone())
                    .is_err()
            );
        }
        assert!(
            ApprovalRequest::parse(
                RequestId::Number(7),
                "unknown/requestApproval".into(),
                params()
            )
            .is_err()
        );
    }
    fn ready() -> Connection {
        let mut c = Connection::default();
        let (id, _) = c.initialize("test", 0, 1000).unwrap();
        let line=encode(Message::Response{id,result:json!({"userAgent":"fixture","platformFamily":"fixture","platformOs":"fixture","codexHome":"/fixture"})}).unwrap();
        c.receive(&line, 1).unwrap();
        c.initialized(2).unwrap();
        c
    }
    #[test]
    fn native_codex_approval_mapping_one_response_until_resolution() {
        let request = parse(params()).unwrap();
        let mut c = ready();
        let line = encode(Message::Request {
            id: request.id().clone(),
            method: request.method().into(),
            params: Some(params()),
            trace: None,
        })
        .unwrap();
        c.receive(&line, 3).unwrap();
        let response: Value =
            serde_json::from_slice(&c.respond_approval(request.response(true), 4).unwrap())
                .unwrap();
        assert_eq!(response, json!({"id":7,"result":{"decision":"accept"}}));
        assert!(matches!(
            c.reject_server_request(request.id(), 5),
            Err(crate::codex::Error::Identity)
        ));
        assert!(matches!(
            c.respond_approval(request.response(false), 6),
            Err(crate::codex::Error::Identity)
        ));
        assert_eq!(c.pending_server_count(), 1);
        let resolved = encode(Message::Notification {
            method: "serverRequest/resolved".into(),
            params: Some(json!({"threadId":"thread","requestId":7})),
        })
        .unwrap();
        assert!(c.receive(&resolved, 7).unwrap().1.is_some());
        assert_eq!(c.pending_server_count(), 0);
    }
    #[test]
    fn native_codex_approval_mapping_connection_fences() {
        let request = parse(params()).unwrap();
        let mut c = ready();
        let line = encode(Message::Request {
            id: request.id().clone(),
            method: request.method().into(),
            params: Some(params()),
            trace: None,
        })
        .unwrap();
        c.receive(&line, 3).unwrap();
        let mut substituted = request.clone();
        substituted.params["command"] = json!("evil");
        assert!(c.respond_approval(substituted.response(true), 4).is_err());
        let response: Value =
            serde_json::from_slice(&c.respond_approval(request.response(false), 5).unwrap())
                .unwrap();
        assert_eq!(response, json!({"id":7,"result":{"decision":"decline"}}));
        assert!(c.respond_approval(request.response(true), 6).is_err());
        assert_eq!(c.pending_server_count(), 1);
        let resolved = encode(Message::Notification {
            method: "serverRequest/resolved".into(),
            params: Some(json!({"threadId":"wrong","requestId":7})),
        })
        .unwrap();
        assert!(c.receive(&resolved, 7).is_err());
        for mode in ["eof", "timeout"] {
            let mut c = ready();
            c.receive(&line, 3).unwrap();
            c.respond_approval(request.response(true), 4).unwrap();
            if mode == "eof" {
                assert!(c.eof().is_err());
            } else {
                assert!(c.tick(3 + crate::codex::MAX_REQUEST_MS).is_err());
            }
        }
    }
}
