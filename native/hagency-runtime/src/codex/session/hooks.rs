//! Pinned 0.154.0 hook summaries are diagnostics, never permission or completion.
use super::{Error, id, object, string};
use serde_json::Value;

pub(super) fn validate(method: &str, params: &Value) -> Result<(), Error> {
    let run = object(params, "run")?;
    id(run, "id")?;
    if !matches!(
        string(run, "eventName")?,
        "preToolUse"
            | "permissionRequest"
            | "postToolUse"
            | "preCompact"
            | "postCompact"
            | "sessionStart"
            | "sessionEnd"
            | "userPromptSubmit"
            | "subagentStart"
            | "subagentStop"
            | "stop"
            | "interrupt"
    ) || string(run, "executionMode")? != "sync"
        || !matches!(
            string(run, "handlerType")?,
            "command" | "mcpTool" | "prompt" | "agent"
        )
    {
        return Err(Error::Malformed);
    }
    match string(run, "scope")? {
        "thread" => {}
        "turn" => {
            id(params, "turnId")?;
        }
        _ => return Err(Error::Malformed),
    }
    let status = string(run, "status")?;
    if (method == "hook/started" && status != "running")
        || (method == "hook/completed"
            && !matches!(status, "completed" | "failed" | "blocked" | "stopped"))
    {
        return Err(Error::Malformed);
    }
    for key in ["startedAt", "displayOrder"] {
        if run.get(key).and_then(Value::as_i64).is_none() {
            return Err(Error::Malformed);
        }
    }
    for key in ["completedAt", "durationMs"] {
        if run
            .get(key)
            .is_some_and(|v| !v.is_null() && v.as_i64().is_none())
        {
            return Err(Error::Malformed);
        }
    }
    let path = string(run, "sourcePath")?;
    if path.len() > 4096
        || path.chars().any(char::is_control)
        || !std::path::Path::new(path).is_absolute()
    {
        return Err(Error::Malformed);
    }
    if let Some(source) = run.get("source")
        && !matches!(
            source.as_str(),
            Some(
                "system"
                    | "user"
                    | "project"
                    | "mdm"
                    | "sessionFlags"
                    | "plugin"
                    | "cloudRequirements"
                    | "cloudManagedConfig"
                    | "legacyManagedConfigFile"
                    | "legacyManagedConfigMdm"
                    | "unknown"
            )
        )
    {
        return Err(Error::Malformed);
    }
    if run
        .get("statusMessage")
        .is_some_and(|v| !v.is_null() && v.as_str().is_none_or(|s| s.len() > 4096))
    {
        return Err(Error::Malformed);
    }
    let entries = run
        .get("entries")
        .and_then(Value::as_array)
        .ok_or(Error::Malformed)?;
    if entries.len() > 64 {
        return Err(Error::Capacity);
    }
    let mut bytes = 0usize;
    for entry in entries {
        if !matches!(
            string(entry, "kind")?,
            "warning" | "stop" | "feedback" | "context" | "error"
        ) {
            return Err(Error::Malformed);
        }
        bytes = bytes
            .checked_add(string(entry, "text")?.len())
            .ok_or(Error::Capacity)?;
        if bytes > 65536 {
            return Err(Error::Capacity);
        }
    }
    Ok(())
}
