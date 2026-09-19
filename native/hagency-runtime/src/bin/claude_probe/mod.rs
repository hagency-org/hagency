//! Explicit offline peer mode; never invokes Claude or another provider.
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, Write},
    path::Path,
};

fn emit(value: Value) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &value)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}
pub(super) fn run(mode: &str, marker: &Path) -> io::Result<()> {
    if !matches!(
        mode,
        "normal"
            | "malformed"
            | "stall"
            | "permission-allow"
            | "permission-deny"
            | "permission-cancel"
            | "permission-hold"
            | "usage"
            | "usage-conflict"
    ) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    fs::write(marker.with_extension("entered"), b"offline")?;
    let mut stdin = io::stdin().lock();
    let mut line = String::new();
    stdin.read_line(&mut line)?;
    let request: Value = serde_json::from_str(&line)?;
    if request["type"] != "control_request" || request["request"]["subtype"] != "initialize" {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    if mode == "stall" {
        drop(stdin);
        return super::pulse(marker);
    }
    if mode == "malformed" {
        io::stdout().write_all(b"{\"type\":\"unknown\"}\n")?;
        io::stdout().flush()?;
        drop(stdin);
        return super::pulse(marker);
    }
    emit(
        json!({"type":"control_response","response":{"subtype":"success","request_id":request["request_id"],"response":{}}}),
    )?;
    line.clear();
    stdin.read_line(&mut line)?;
    let prompt: Value = serde_json::from_str(&line)?;
    if prompt["type"] != "user" || prompt["message"]["role"] != "user" || prompt["session_id"] != ""
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    fs::write(
        marker.with_extension("prompt"),
        serde_json::to_vec(&prompt)?,
    )?;
    emit(json!({"type":"system","subtype":"init","session_id":"owned-claude"}))?;
    if mode.starts_with("permission-") {
        let input = json!({"command":"offline literal $(no shell) 中文"});
        emit(
            json!({"type":"control_request","request_id":"owned-permission","request":{
            "subtype":"can_use_tool","tool_name":"Bash","tool_use_id":"owned-tool","input":input}}),
        )?;
        if mode == "permission-cancel" {
            emit(json!({"type":"control_cancel_request","request_id":"owned-permission"}))?;
            fs::write(marker.with_extension("cancelled"), b"offline")?;
            drop(stdin);
            return super::pulse(marker);
        }
        if mode == "permission-hold" {
            drop(stdin);
            return super::pulse(marker);
        }
        line.clear();
        stdin.read_line(&mut line)?;
        let response: Value = serde_json::from_str(&line)?;
        let expected = if mode == "permission-allow" {
            json!({"behavior":"allow","updatedInput":input})
        } else {
            json!({"behavior":"deny","message":"Permission denied by Hagency.","interrupt":true})
        };
        if response
            != json!({"type":"control_response","response":{
            "subtype":"success","request_id":"owned-permission","response":expected}})
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        fs::write(
            marker.with_extension("response"),
            serde_json::to_vec(&response)?,
        )?;
    }
    if mode.starts_with("usage") {
        let mut message = json!({"type":"assistant","session_id":"owned-claude","uuid":"one","parent_tool_use_id":null,
            "message":{"id":"step-one","usage":{"input_tokens":10,"output_tokens":999,"cache_read_input_tokens":20,"cache_creation_input_tokens":30}}});
        emit(message.clone())?;
        message["uuid"] = json!("two");
        if mode == "usage-conflict" {
            message["message"]["usage"]["input_tokens"] = json!(11);
        }
        emit(message.clone())?;
        message["parent_tool_use_id"] = json!("parent");
        emit(message.clone())?;
        message["parent_tool_use_id"] = Value::Null;
        message["message"]["id"] = json!("step-two");
        emit(message)?;
        emit(
            json!({"type":"result","subtype":"success","session_id":"owned-claude","is_error":true,
            "modelUsage":{"private-model-one":{"inputTokens":100,"outputTokens":200,"cacheReadInputTokens":300,"cacheCreationInputTokens":400},
                "private-model-two":{"inputTokens":1,"outputTokens":2,"cacheReadInputTokens":3,"cacheCreationInputTokens":4}}}),
        )?;
    } else {
        emit(
            json!({"type":"assistant","session_id":"owned-claude","message":{"content":[{"type":"text","text":"offline"}]}}),
        )?;
        emit(
            json!({"type":"result","subtype":"success","session_id":"owned-claude","is_error":false,"result":"offline"}),
        )?;
    }
    // Deliberately remains alive after result AND after stdin closes. Only the
    // original guardian's stop or the bounded fixture fuse ends the pulse.
    drop(stdin);
    super::pulse(marker)
}
