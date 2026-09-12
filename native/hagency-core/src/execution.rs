//! Reusable permission descriptions, never an owner verdict or runtime approval.
mod paths;
mod profile;
pub use paths::PathFlavor;

use crate::{InvalidInput, canonical};
use serde::Serialize;
use serde_json::{Value, json};

const MAX_FIELD_UNITS: usize = 8192;
const MAX_METADATA_BYTES: usize = 64 * 1024;
const MAX_DESCRIPTION_UNITS: usize = 12_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExecutionPolicy {
    pub yolo: bool,
}
pub fn normalize_policy(
    value: Option<&Value>,
    framework: &str,
) -> Result<ExecutionPolicy, InvalidInput> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(ExecutionPolicy { yolo: false });
    };
    let yolo = value.get("yolo").and_then(Value::as_bool);
    if !keys_within(value, &["yolo"])
        || yolo.is_none()
        || (yolo == Some(true) && framework != "codex")
    {
        return Err(InvalidInput("invalid execution policy"));
    }
    Ok(ExecutionPolicy {
        yolo: yolo == Some(true),
    })
}

/// Constructed by the host that owns the scoped upstream runner connection.
/// No Deserialize implementation or Agent HTTP input can establish that origin.
pub struct HostRequest<'a> {
    pub agent_id: &'a str,
    pub workspace: &'a str,
    pub task_id: Option<&'a str>,
    pub may_write: bool,
    pub method: &'a str,
    pub params: &'a Value,
    pub path_flavor: PathFlavor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    ExactCommand,
    NetworkHost,
    PermissionProfile,
}
#[derive(Serialize)]
pub struct Scope {
    pub kind: ScopeKind,
    pub key: String,
    pub description: String,
}
/// Private host metadata. Deliberately not Debug or Serialize: descriptions may
/// be projected into a private owner card; workspace is not a console DTO.
pub struct Authorization {
    pub agent_id: String,
    pub task_id: Option<String>,
    pub workspace: String,
    pub may_write: bool,
    pub environment_id: Option<String>,
    pub scope: Scope,
}

fn bounded(value: &str) -> bool {
    !value.is_empty() && !value.contains('\0') && value.encode_utf16().count() <= MAX_FIELD_UNITS
}
fn text(value: &Value) -> Option<&str> {
    value.as_str().filter(|s| bounded(s))
}
fn keys_within(value: &Value, keys: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|map| map.keys().all(|k| keys.contains(&k.as_str())))
}
fn present<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|v| !v.is_null())
}

// Check nesting/aggregate allocation before serialization. Values are already
// bounded at ingress; this additional bound keeps pure callers honest too.
fn metadata(value: &Value, depth: usize, remaining: &mut usize) -> Option<()> {
    if depth > 16 {
        return None;
    }
    *remaining = remaining.checked_sub(1)?;
    match value {
        Value::String(s) => {
            *remaining = remaining.checked_sub(s.len())?;
        }
        Value::Array(a) => {
            for v in a {
                metadata(v, depth + 1, remaining)?;
            }
        }
        Value::Object(m) => {
            for (k, v) in m {
                *remaining = remaining.checked_sub(k.len())?;
                metadata(v, depth + 1, remaining)?;
            }
        }
        _ => {}
    }
    Some(())
}
struct EncodedBound(usize);
impl std::io::Write for EncodedBound {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(bytes.len())
            .ok_or_else(|| std::io::Error::other("metadata limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// None means the reusable scope is not representable; it never means allow.
pub fn derive(request: HostRequest<'_>) -> Option<Authorization> {
    if !bounded(request.agent_id) || !request.params.is_object() {
        return None;
    }
    let mut remaining = MAX_METADATA_BYTES;
    metadata(request.params, 0, &mut remaining)?;
    serde_json::to_writer(EncodedBound(MAX_METADATA_BYTES), request.params).ok()?;
    let workspace = request.path_flavor.normalize(request.workspace)?;
    let p = request.params;
    let environment = present(p, "environmentId").map(text).transpose_option()?;
    let common = [
        "threadId",
        "turnId",
        "itemId",
        "approvalId",
        "startedAtMs",
        "reason",
        "environmentId",
    ];
    let mut allowed = common.to_vec();
    let (kind, value, description) = match request.method {
        "item/commandExecution/requestApproval" => {
            allowed.extend([
                "command",
                "cwd",
                "commandActions",
                "kind",
                "networkApprovalContext",
                "proposedExecpolicyAmendment",
                "proposedNetworkPolicyAmendments",
                "availableDecisions",
                "additionalPermissions",
            ]);
            if !keys_within(p, &allowed) || present(p, "kind").is_some_and(|v| v != "command") {
                return None;
            }
            if let Some(net) = present(p, "networkApprovalContext") {
                if !keys_within(net, &["host", "protocol"])
                    || present(p, "additionalPermissions").is_some()
                {
                    return None;
                }
                let host = text(net.get("host")?)?;
                let protocol = net.get("protocol")?.as_str()?;
                if !network_host(host)
                    || !["http", "https", "socks5Tcp", "socks5Udp"].contains(&protocol)
                {
                    return None;
                }
                let host = host.to_ascii_lowercase();
                (
                    ScopeKind::NetworkHost,
                    json!({"host":host,"protocol":protocol}),
                    format!("Network host: {host}\nProtocol: {protocol}"),
                )
            } else {
                let command = text(p.get("command")?)?;
                let cwd = request.path_flavor.normalize(text(p.get("cwd")?)?)?;
                let additional = present(p, "additionalPermissions")
                    .map(|v| profile::derive(v, request.path_flavor))
                    .transpose_option()?;
                let mut description = format!(
                    "Exact command (including requested sandbox escalation):\n{command}\nWorking directory: {cwd}"
                );
                if let Some((_, summary)) = &additional {
                    description.push('\n');
                    description.push_str(summary);
                }
                (
                    ScopeKind::ExactCommand,
                    json!({"command":command,"cwd":cwd,"additionalPermissions":additional.map(|(v,_)|v)}),
                    description,
                )
            }
        }
        "item/permissions/requestApproval" => {
            allowed.extend(["cwd", "permissions"]);
            if !keys_within(p, &allowed) {
                return None;
            }
            let cwd = request.path_flavor.normalize(text(p.get("cwd")?)?)?;
            let (permissions, description) =
                profile::derive(p.get("permissions")?, request.path_flavor)?;
            (
                ScopeKind::PermissionProfile,
                json!({"cwd":cwd,"permissions":permissions}),
                description,
            )
        }
        _ => return None,
    };
    if description.encode_utf16().count() > MAX_DESCRIPTION_UNITS {
        return None;
    }
    let key = canonical::digest(&json!({"workspace":workspace,"mayWrite":request.may_write,"environmentId":environment,"kind":kind,"value":value})).ok()?;
    Some(Authorization {
        agent_id: request.agent_id.into(),
        task_id: request.task_id.filter(|s| bounded(s)).map(str::to_owned),
        workspace,
        may_write: request.may_write,
        environment_id: environment.map(str::to_owned),
        scope: Scope {
            kind,
            key,
            description,
        },
    })
}

fn network_host(value: &str) -> bool {
    let (name, port) = value
        .split_once(':')
        .map_or((value, None), |(n, p)| (n, Some(p)));
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        && port
            .is_none_or(|p| !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()))
}
trait TransposeOption<T> {
    fn transpose_option(self) -> Option<Option<T>>;
}
impl<T> TransposeOption<T> for Option<Option<T>> {
    fn transpose_option(self) -> Option<Option<T>> {
        match self {
            None => Some(None),
            Some(v) => v.map(Some),
        }
    }
}
