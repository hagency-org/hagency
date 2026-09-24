//! Offline Claude-shaped parent; only its native MCP child contacts loopback.
use super::*;
fn control(input: &mut impl BufRead, subtype: &str) -> io::Result<Value> {
    let value = read(input)?;
    if value["type"] != "control_request" || value["request"]["subtype"] != subtype {
        return Err(invalid());
    }
    Ok(value)
}
fn respond(out: &mut impl Write, request: &Value, value: Value) -> io::Result<()> {
    send(
        out,
        json!({"type":"control_response","response":{"subtype":"success","request_id":request["request_id"],"response":value}}),
    )
}
pub(super) fn run() -> io::Result<()> {
    let (release, wait) = mpsc::sync_channel::<()>(1);
    let watchdog = std::thread::spawn(move || {
        if wait.recv_timeout(Duration::from_secs(20)).is_err() {
            std::process::exit(74)
        }
    });
    let result = inner();
    let _ = release.send(());
    let _ = watchdog.join();
    result.map_err(|_| invalid())
}
fn inner() -> io::Result<()> {
    let mut input = io::stdin().lock();
    let mut out = io::stdout().lock();
    let init = control(&mut input, "initialize")?;
    if let Some(reference) = context_reference()?
        && reference.path.exists()
    {
        return Err(invalid());
    }
    respond(&mut out, &init, json!({}))?;
    let before = control(&mut input, "mcp_status")?;
    respond(&mut out, &before, json!({"mcpServers":[]}))?;
    let binding = control(&mut input, "mcp_set_servers")?;
    let server = &binding["request"]["servers"]["hagency_task_writer"];
    if binding["request"]["servers"]
        .as_object()
        .is_none_or(|v| v.len() != 1)
        || server["type"] != "stdio"
        || server["args"] != json!(["mcp", "--owned-task-profile"])
        || server["timeout"] != 5000
        || server["alwaysLoad"] != true
    {
        return Err(invalid());
    }
    let command = server["command"].as_str().ok_or_else(invalid)?;
    if !Path::new(command).is_absolute()
        || binding.to_string().contains("HAGENCY_RUNNER_CAPABILITY")
    {
        return Err(invalid());
    }
    let flags = server["env"].as_object().ok_or_else(invalid)?;
    if flags.iter().any(|(key, v)| {
        !["HAGENCY_FILE_TOOLS", "HAGENCY_RECEIVE_FILE_TOOLS"].contains(&key.as_str()) || v != "1"
    }) {
        return Err(invalid());
    }
    let mut cmd = Command::new(command);
    cmd.args(["mcp", "--owned-task-profile"])
        .env_clear()
        .current_dir(std::env::current_dir()?);
    for name in ENV {
        cmd.env(name, std::env::var_os(name).ok_or_else(invalid)?);
    }
    for (name, value) in flags {
        cmd.env(name, value.as_str().ok_or_else(invalid)?);
    }
    if let Some(root) = std::env::var_os("SystemRoot") {
        cmd.env("SystemRoot", root);
    }
    let mut child = Helper(
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let mut write = child.0.stdin.take().ok_or_else(invalid)?;
    let mut read_helper = BufReader::new(child.0.stdout.take().ok_or_else(invalid)?);
    rpc(
        &mut read_helper,
        &mut write,
        1,
        "initialize",
        json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"offline-claude","version":"1"}}),
    )?;
    send(
        &mut write,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )?;
    let listed = rpc(&mut read_helper, &mut write, 2, "tools/list", json!({}))?;
    let tools = listed["tools"]
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .map(|v| json!({"name":v["name"]}))
        .collect::<Vec<_>>();
    respond(
        &mut out,
        &binding,
        json!({"added":["hagency_task_writer"],"removed":[],"errors":{}}),
    )?;
    let after = control(&mut input, "mcp_status")?;
    respond(
        &mut out,
        &after,
        json!({"mcpServers":[{"name":"hagency_task_writer","status":"connected","tools":tools}]}),
    )?;
    let prompt = read(&mut input)?;
    if prompt["type"] != "user"
        || !prompt["message"]["content"]
            .as_str()
            .is_some_and(|t| t.contains("canonical task ID is task."))
    {
        return Err(invalid());
    }
    // Native helper must refuse hidden tools too, before any backend mutation.
    for (n, name) in [
        "accept_task",
        "comment_task",
        "get_approval",
        "consume_approval",
    ]
    .iter()
    .enumerate()
    {
        send(
            &mut write,
            json!({"jsonrpc":"2.0","id":10+n,"method":"tools/call","params":{"name":name,"arguments":{"id":"task","call_id":"forbidden","text":"must not write"}}}),
        )?;
        let response = read(&mut read_helper)?;
        if response["error"]["code"] != -32602 {
            return Err(invalid());
        }
    }
    let wrong = rpc(
        &mut read_helper,
        &mut write,
        20,
        "tools/call",
        json!({"name":"get_task","arguments":{"id":"foreign_task"}}),
    )?;
    if wrong["isError"] != true {
        return Err(invalid());
    }
    let before = tool(
        &mut read_helper,
        &mut write,
        21,
        "get_task",
        json!({"id":"task"}),
    )?;
    if before["status"] != "in_progress" {
        return Err(invalid());
    }
    let beat = tool(
        &mut read_helper,
        &mut write,
        22,
        "update_task_execution",
        json!({"id":"task","call_id":"claude_native_heartbeat","heartbeat":true}),
    )?;
    let after = tool(
        &mut read_helper,
        &mut write,
        23,
        "get_task",
        json!({"id":"task"}),
    )?;
    if after["status"] != "in_progress"
        || !beat["heartbeat_at"].is_u64()
        || after["heartbeat_at"] != beat["heartbeat_at"]
    {
        return Err(invalid());
    }
    // The list is served too (ADR-158 names it among the task tools): here
    // it holds exactly the assigned task.
    let listed = rpc(
        &mut read_helper,
        &mut write,
        30,
        "tools/call",
        json!({"name":"list_tasks","arguments":{}}),
    )?;
    if listed["isError"] != false
        || listed["structuredContent"]["tasks"]
            .as_array()
            .map(Vec::len)
            != Some(1)
        || listed["structuredContent"]["tasks"][0]["id"] != "task"
    {
        return Err(invalid());
    }
    drop(write);
    if !child.0.wait()?.success() {
        return Err(invalid());
    }
    receipt(
        "claude-task",
        json!({"heartbeat":true,"readback":true,"helper_exit":true,"tools":tools.len(),"outside_profile_refused":4,"foreign_task_refused":true}),
    )?;
    send(
        &mut out,
        json!({"type":"system","subtype":"init","session_id":"offline-claude-task"}),
    )?;
    send(
        &mut out,
        json!({"type":"result","subtype":"success","session_id":"offline-claude-task","is_error":false,"result":"not canonical Done"}),
    )?;
    // Remain owned after the result until the original parent stream closes.
    let mut rest = String::new();
    input.read_line(&mut rest)?;
    Ok(())
}
