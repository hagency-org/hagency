//! Disposable offline app-server peer. Never a live model or daemon launcher.
mod claude_mcp_peer;
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
const ENV: [&str; 3] = [
    "HAGENCY_RUNNER_API_ADDR",
    "HAGENCY_RUNNER_CAPABILITY",
    "HAGENCY_TASK_ID",
];
/// ADR180's eight names, spelled out here rather than imported: this peer is
/// the independent observer of what the host actually offered.
const COORDINATION: [&str; 8] = [
    "comment_task",
    "delegate_task",
    "open_conversation",
    "get_conversation",
    "update_conversation_members",
    "close_conversation",
    "send_peer_message",
    "read_peer_inbox",
];
fn invalid() -> io::Error {
    io::Error::other("offline MCP fixture refused or incomplete")
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextReference {
    profile: String,
    path: PathBuf,
    context_id: String,
}
fn context_reference() -> io::Result<Option<ContextReference>> {
    let inherited = std::env::var("HAGENCY_RUNNER_CAPABILITY").map_err(|_| invalid())?;
    let value: Value = serde_json::from_str(&inherited).map_err(|_| invalid())?;
    if value.get("profile").is_none() {
        return Ok(None);
    }
    let reference: ContextReference = serde_json::from_str(&inherited).map_err(|_| invalid())?;
    if reference.profile != "retained_task_context_v1"
        || reference.context_id.len() != 64
        || !reference
            .context_id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || !reference.path.is_absolute()
        || reference.path.file_name().and_then(|v| v.to_str())
            != Some(format!("context-{}.json", reference.context_id).as_str())
        || std::env::var("HAGENCY_TASK_ID").map_err(|_| invalid())?
            != format!("context_{}", reference.context_id)
    {
        return Err(invalid());
    }
    Ok(Some(reference))
}
fn read(reader: &mut impl BufRead) -> io::Result<Value> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Err(invalid());
        }
        let count = available
            .iter()
            .position(|&c| c == b'\n')
            .map_or(available.len(), |v| v + 1);
        if bytes.len() + count > 65536 {
            return Err(invalid());
        }
        bytes.extend_from_slice(&available[..count]);
        reader.consume(count);
        if bytes.last() == Some(&b'\n') {
            break;
        }
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}
fn send(out: &mut impl Write, value: Value) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(&value).map_err(|_| invalid())?;
    if bytes.len() > 65535 {
        return Err(invalid());
    }
    bytes.push(b'\n');
    out.write_all(&bytes)?;
    out.flush()
}
fn request(input: &mut impl BufRead, method: &str, log: &mut fs::File) -> io::Result<Value> {
    let value = read(input)?;
    if value["method"] != method {
        return Err(invalid());
    }
    send(log, value.clone())?;
    Ok(value)
}
// Ordinary fixture child only, not an additional production launcher. It is
// inside the retained guardian/job and emulates Codex's separate Unix MCP group.
struct Helper(Child);
impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn rpc(
    input: &mut impl BufRead,
    out: &mut impl Write,
    id: u64,
    method: &str,
    params: Value,
) -> io::Result<Value> {
    send(
        out,
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
    )?;
    let response = read(input)?;
    if response["id"] != id || response.get("error").is_some() {
        return Err(invalid());
    }
    Ok(response["result"].clone())
}
fn tool(
    input: &mut impl BufRead,
    out: &mut impl Write,
    id: u64,
    name: &str,
    args: Value,
) -> io::Result<Value> {
    let result = rpc(
        input,
        out,
        id,
        "tools/call",
        json!({"name":name,"arguments":args}),
    )?;
    if result["isError"] != false {
        return Err(invalid());
    }
    Ok(result["structuredContent"]["task"].clone())
}
fn receipt(stage: &str, value: Value) -> io::Result<()> {
    let target = format!("owned-mcp.{stage}");
    let temporary = format!("{target}.tmp");
    fs::write(&temporary, serde_json::to_vec(&value)?)?;
    fs::rename(temporary, target)
}
/// Codex's own approval gate for a tool it may not pre-approve (ADR180). The
/// item is announced first, because the elicitation correlates to it by server
/// and arguments alone, and only an accepted decision lets the call through.
fn elicit(host_in: &mut impl BufRead, host_out: &mut impl Write, args: &Value) -> io::Result<()> {
    let item = |status: &str| {
        json!({"type":"mcpToolCall","id":"delegate-item","server":"hagency_task_writer",
            "tool":"delegate_task","arguments":args,"status":status})
    };
    send(
        host_out,
        json!({"method":"item/started","params":{"threadId":"owned-thread","turnId":"owned-turn",
            "startedAtMs":1,"item":item("inProgress")}}),
    )?;
    send(
        host_out,
        json!({"id":31,"method":"mcpServer/elicitation/request","params":{
            "threadId":"owned-thread","turnId":"owned-turn","serverName":"hagency_task_writer",
            "mode":"form","message":"Hand this work to the named colleague?",
            "requestedSchema":{"type":"object","properties":{}},
            "_meta":{"codex_approval_kind":"mcp_tool_call","tool_params":args}}}),
    )?;
    let response = read(host_in)?;
    if response["id"] != 31
        || response["result"] != json!({"action":"accept","content":null,"_meta":null})
    {
        return Err(invalid());
    }
    send(
        host_out,
        json!({"method":"serverRequest/resolved","params":{"threadId":"owned-thread","requestId":31}}),
    )?;
    send(
        host_out,
        json!({"method":"item/completed","params":{"threadId":"owned-thread","turnId":"owned-turn",
            "completedAtMs":2,"item":item("completed")}}),
    )
}
fn helper(
    params: &Value,
    turn: &Value,
    mode: &str,
    host_in: &mut impl BufRead,
    host_out: &mut impl Write,
) -> io::Result<()> {
    let config = &params["config"];
    let table = &config["mcp_servers.hagency_task_writer"];
    let executable = table["command"].as_str().ok_or_else(invalid)?;
    let fleet_files = Path::new("owned-mcp.fleet-files").exists();
    let fleet_media = Path::new("projects/factory_project/configured-fleet-media").is_file();
    let coordination = Path::new("owned-mcp.coordination").exists();
    let mut environment = ENV.to_vec();
    let mut tools = vec![
        "get_task",
        "update_task_execution",
        "transition_task",
        "complete_task_with_reply",
    ];
    let mut approved = json!({
        "get_task":{"approval_mode":"approve"},
        "update_task_execution":{"approval_mode":"approve"},
        "transition_task":{"approval_mode":"approve"},
        "complete_task_with_reply":{"approval_mode":"approve"}
    });
    if fleet_files || fleet_media {
        environment.extend(["HAGENCY_FILE_TOOLS", "HAGENCY_RECEIVE_FILE_TOOLS"]);
        tools.extend([
            "send_file",
            "get_file_delivery",
            "list_received_files",
            "receive_file",
        ]);
    }
    if coordination {
        // No environment marker: the helper has always served these. Only
        // comment_task joins the pre-approved set (ADR-021).
        tools.extend(COORDINATION);
        approved["comment_task"] = json!({"approval_mode":"approve"});
    }
    if !Path::new(executable).is_absolute()
        || table["args"] != json!(["mcp"])
        || table["cwd"] != params["cwd"]
        || table["env_vars"] != json!(environment)
        || table["enabled_tools"] != json!(tools)
        || table.get("env").is_some()
        || table.get("url").is_some()
        || table.get("default_tools_approval_mode").is_some()
        || table["tools"] != approved
        || config["shell_environment_policy.inherit"] != "none"
        || config["shell_environment_policy.experimental_use_profile"] != false
    {
        return Err(invalid());
    }
    let mut command = Command::new(executable);
    command
        .args(["mcp"])
        .current_dir(table["cwd"].as_str().ok_or_else(invalid)?)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in environment {
        command.env(key, std::env::var_os(key).ok_or_else(invalid)?);
    }
    #[cfg(windows)]
    command.env(
        "SystemRoot",
        std::env::var_os("SystemRoot").ok_or_else(invalid)?,
    );
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = Helper(command.spawn()?);
    // The earliest fact this fixture can publish about the helper: it exists and
    // its stdio is retained. Written BEFORE any MCP exchange, so an absent
    // `ack`/`readback`/`receipt` can be attributed to the helper rather than to a
    // probe that never reached this line.
    receipt("spawned", json!({"helper":"spawned"}))?;
    let mut output = child.0.stdin.take().ok_or_else(invalid)?;
    let mut input = BufReader::new(child.0.stdout.take().ok_or_else(invalid)?);
    let mut stderr = child.0.stderr.take().ok_or_else(invalid)?;
    rpc(
        &mut input,
        &mut output,
        0,
        "initialize",
        json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"owned-offline-peer","version":"1"}}),
    )?;
    send(
        &mut output,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )?;
    let task = if let Some(reference) = context_reference()? {
        // The actual helper has already cached its private original context.
        // Invalidating the startup record cannot retarget that context later.
        if !["retained", "warm"].contains(&mode) {
            return Err(invalid());
        }
        let mut file =
            hagency_store::private::open(&reference.path, false).map_err(|_| invalid())?;
        file.set_len(0)?;
        file.write_all(b"invalid after native helper initialize")?;
        file.sync_all()?;
        receipt(
            "context-cached",
            json!({"cached_before_record_change":true}),
        )?;
        params["developerInstructions"]
            .as_str()
            .and_then(|v| v.strip_prefix("The assigned canonical task ID is "))
            .and_then(|v| v.split_once('.'))
            .map(|(id, _)| id.to_owned())
            .ok_or_else(invalid)?
    } else {
        std::env::var("HAGENCY_TASK_ID").map_err(|_| invalid())?
    };
    let before = tool(&mut input, &mut output, 1, "get_task", json!({"id":task}))?;
    if before["id"] != task || before["status"] != "in_progress" {
        return Err(invalid());
    }
    let fleet_finish = Path::new("projects/factory_project/configured-fleet-probe").is_file();
    if fleet_finish {
        // A disposable peer gate, not production readiness or task authority.
        // The independent test observer waits for BOTH real helpers and the
        // service's actual Started rows before allowing either completion.
        receipt(
            "fleet-ready",
            json!({"task_id":task,"pid":std::process::id(),
                "input":turn["params"]["input"][0]["text"]}),
        )?;
        // The release comes only after the OTHER agent's helper is in flight too.
        // On a small hosted runner that took longer than the 5 s this gate used
        // to allow: the first helper gave up, the host saw peer_eof, and "both
        // in flight" could then never become true. The host's operation budget
        // is the real bound on this peer; stay under it, not under a guess.
        let until = Instant::now() + Duration::from_secs(20);
        while !Path::new("owned-mcp.fleet-release").is_file() {
            if Instant::now() >= until {
                return Err(invalid());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    // No tool tells an agent a colleague's engagement ID (ADR180), so the
    // fixture names the assignee the way it names every other scripted
    // choice: a file in this peer's own disposable workspace.
    if let Ok(assignee) = fs::read_to_string("owned-mcp.delegate-to") {
        let args = json!({"call_id":"fleet_delegate","assignee_engagement":assignee.trim(),
            "definition":{"title":"Delegated fleet report","description":"Draft the report and reply with it."}});
        elicit(host_in, host_out, &args)?;
        let delegated = rpc(
            &mut input,
            &mut output,
            30,
            "tools/call",
            json!({"name":"delegate_task","arguments":args}),
        )?;
        if delegated["isError"] != false {
            return Err(invalid());
        }
        receipt("fleet-delegated", delegated["structuredContent"].clone())?;
    }
    if fleet_files {
        let listed = rpc(
            &mut input,
            &mut output,
            10,
            "tools/call",
            json!({"name":"list_received_files","arguments":{}}),
        )?;
        if listed["isError"] != false || listed["structuredContent"]["items"] != json!([]) {
            return Err(invalid());
        }
        let admitted = rpc(
            &mut input,
            &mut output,
            11,
            "tools/call",
            json!({"name":"send_file","arguments":{
            "call_id":"fleet_missing_source","path":"missing.txt","filename":"missing.txt"}}),
        )?;
        if admitted["isError"] != false {
            return Err(invalid());
        }
        let id = admitted["structuredContent"]["delivery_id"]
            .as_str()
            .ok_or_else(invalid)?;
        // 128 polls at 100 ms: a loaded runner needs more than the 0.6 s that
        // 128 polls at 5 ms allowed for the refusal to be recorded.
        let until = Instant::now() + Duration::from_secs(12);
        let mut next = 12;
        loop {
            let inspected = rpc(
                &mut input,
                &mut output,
                next,
                "tools/call",
                json!({"name":"get_file_delivery","arguments":{"delivery_id":id}}),
            )?;
            if inspected["isError"] != false {
                return Err(invalid());
            }
            if inspected["structuredContent"]["status"] == "failed" {
                if inspected["structuredContent"]["error_code"] != "source_refused" {
                    return Err(invalid());
                }
                receipt(
                    "fleet-files",
                    json!({"task_id":task,"list":listed["structuredContent"],"delivery":inspected["structuredContent"]}),
                )?;
                break;
            }
            if Instant::now() >= until || next >= 128 {
                return Err(invalid());
            }
            next += 1;
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    if fleet_media {
        let listing = rpc(
            &mut input,
            &mut output,
            14,
            "tools/call",
            json!({"name":"list_received_files","arguments":{}}),
        )?;
        if listing["isError"] != false {
            return Err(invalid());
        }
        let items = listing["structuredContent"]["items"]
            .as_array()
            .ok_or_else(invalid)?;
        let latest = items
            .iter()
            .max_by_key(|item| item["sequence"].as_u64().unwrap_or(0))
            .ok_or_else(invalid)?;
        let event = latest["event_id"].as_str().ok_or_else(invalid)?;
        let foreign = if event.starts_with("$fleet_input_0_") {
            event.replacen("_0_", "_1_", 1)
        } else if event.starts_with("$fleet_input_1_") {
            event.replacen("_1_", "_0_", 1)
        } else {
            return Err(invalid());
        };
        if items.iter().any(|item| item["event_id"] == foreign) {
            return Err(invalid());
        }
        let refused = rpc(
            &mut input,
            &mut output,
            15,
            "tools/call",
            json!({"name":"receive_file","arguments":{"event_id":foreign}}),
        )?;
        if refused["isError"] != true {
            return Err(invalid());
        }
        let received = rpc(
            &mut input,
            &mut output,
            16,
            "tools/call",
            json!({"name":"receive_file","arguments":{"event_id":event}}),
        )?;
        if received["isError"] != false || received["structuredContent"]["event_id"] != event {
            return Err(invalid());
        }
        let path = received["structuredContent"]["path"]
            .as_str()
            .ok_or_else(invalid)?;
        if !path.starts_with(".hagency-received-") || !path.ends_with(".bin") || path.contains('/')
        {
            return Err(invalid());
        }
        let bytes = fs::read(path)?;
        if bytes != format!("independent owner bytes for {event}\0\u{fffd}\n").as_bytes() {
            return Err(invalid());
        }
        receipt(
            "fleet-receive",
            json!({"task_id":task,"event_id":event,"path":path}),
        )?;
        // The original child writes the SAME relative name in each private
        // workspace. Only the real task-bound MCP selects and sends its bytes.
        let mut result = format!("factory binary for {task}\0\u{fffd}\n").into_bytes();
        result.extend(bytes);
        fs::write("same.bin", result)?;
        let admission = rpc(
            &mut input,
            &mut output,
            20,
            "tools/call",
            json!({"name":"send_file","arguments":{
            "call_id":"fleet_media","path":"same.bin","filename":"任务文件.bin","caption":format!("Factory file for {task}")}}),
        )?;
        if admission["isError"] != false {
            return Err(invalid());
        }
        let id = admission["structuredContent"]["delivery_id"]
            .as_str()
            .ok_or_else(invalid)?;
        // Encrypt, upload and publish under two concurrent agents outran 5 s on a
        // hosted runner; the helper gave up and the round died as peer_eof.
        let until = Instant::now() + Duration::from_secs(15);
        let mut next = 21;
        loop {
            let status = rpc(
                &mut input,
                &mut output,
                next,
                "tools/call",
                json!({"name":"get_file_delivery","arguments":{"delivery_id":id}}),
            )?;
            if status["isError"] != false || status["structuredContent"]["delivery_id"] != id {
                return Err(invalid());
            }
            if status["structuredContent"]["status"] == "delivered" {
                let current = tool(
                    &mut input,
                    &mut output,
                    next + 1,
                    "get_task",
                    json!({"id":task}),
                )?;
                if current["id"] != task || current["status"] != "in_progress" {
                    return Err(invalid());
                }
                receipt(
                    "fleet-media",
                    json!({"task_id":task,"task_status":current["status"],"delivery":status["structuredContent"]}),
                )?;
                break;
            }
            if status["structuredContent"]["status"] == "failed"
                || Instant::now() >= until
                || next >= 128
            {
                return Err(invalid());
            }
            next += 1;
            std::thread::sleep(Duration::from_millis(150));
        }
    }
    let done = mode == "done";
    let after = if mode == "finish" || fleet_finish {
        let response = rpc(
            &mut input,
            &mut output,
            2,
            "tools/call",
            json!({"name":"complete_task_with_reply","arguments":{"id":task,"call_id":"owned_finish",
                "body":if fleet_finish {format!("Verified factory task {task}")} else {"Verified **native MCP final result**".into()}}}),
        )?;
        if response["isError"] != false {
            return Err(invalid());
        }
        let completion = response["structuredContent"].clone();
        if completion["task_id"] != task
            || completion["execution_epoch"] != 1
            || completion["state"] != "held"
        {
            return Err(invalid());
        }
        receipt("finish-ack", json!({"completion":completion}))?;
        completion
    } else {
        let updated = if done {
            tool(
                &mut input,
                &mut output,
                2,
                "transition_task",
                json!({"id":task,"call_id":"owned_done","status":"done"}),
            )?
        } else {
            tool(
                &mut input,
                &mut output,
                2,
                "update_task_execution",
                json!({"id":task,"call_id":"owned_heartbeat","heartbeat":true}),
            )?
        };
        receipt("ack", json!({"task": updated}))?;
        let after = tool(&mut input, &mut output, 3, "get_task", json!({"id":task}))?;
        let epoch = before["execution_epoch"].as_u64().ok_or_else(invalid)?;
        if after != updated
            || after["id"] != task
            || after["status"] != if done { "done" } else { "in_progress" }
            || after["execution_epoch"] != epoch + u64::from(done)
            || (!done && !after["heartbeat_at"].is_u64())
        {
            return Err(invalid());
        }
        receipt("readback", json!({"task": after}))?;
        after
    };
    drop(output);
    drop(input);
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.0.try_wait()? {
            if !status.success() {
                return Err(invalid());
            }
            break;
        }
        if Instant::now() >= until {
            return Err(invalid());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    // No helper descendants or log flood are expected; cap and watchdog also
    // cover a broken helper which never produces EOF or drains its stderr.
    let mut diagnostic = Vec::new();
    (&mut stderr).take(1025).read_to_end(&mut diagnostic)?;
    if !diagnostic.is_empty() {
        return Err(invalid());
    }
    if mode == "finish" || fleet_finish {
        receipt(
            "finish-exit",
            json!({"completion":after,"helper_exit":true}),
        )?;
    } else {
        receipt("receipt", json!({"task": after, "helper_exit": true}))?;
    }
    Ok(())
}
fn fake() -> io::Result<()> {
    let mode = std::env::var("HAGENCY_OFFLINE_MODE").unwrap_or_else(|_| {
        if Path::new("projects/factory_project/configured-fleet-probe").is_file() {
            "warm".into()
        } else {
            "heartbeat".into()
        }
    });
    if !["heartbeat", "done", "finish", "retained", "warm"].contains(&mode.as_str()) {
        return Err(invalid());
    }
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("owned-mcp.requests")?;
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    if ["retained", "warm"].contains(&mode.as_str()) {
        if let Some(reference) = context_reference()? {
            match fs::symlink_metadata(&reference.path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                _ => return Err(invalid()),
            }
            receipt(
                "context-parent",
                json!({"reference_only":true,"record_absent_at_initialize":true}),
            )?;
        } else if mode == "warm"
            && (Path::new("owned-mcp.sequential").exists()
                || Path::new("projects/factory_project/configured-fleet-probe").is_file())
        {
            receipt(
                "direct-parent",
                json!({"task_id":std::env::var("HAGENCY_TASK_ID").map_err(|_|invalid())?,"pid":std::process::id()}),
            )?;
        } else {
            return Err(invalid());
        }
    }
    let init = request(&mut input, "initialize", &mut log)?;
    if mode == "warm" {
        receipt("warm-entered", json!({"pid":std::process::id()}))?;
        if Path::new("owned-mcp.warm-hold").exists() {
            let until = Instant::now() + Duration::from_secs(5);
            while !Path::new("owned-mcp.warm-release").exists() {
                if Instant::now() >= until {
                    return Err(invalid());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
    send(
        &mut output,
        json!({"id":init["id"],"result":{"userAgent":"offline/0.153.4","platformFamily":"fixture","platformOs":"fixture","codexHome":"/fixture"}}),
    )?;
    request(&mut input, "initialized", &mut log)?;
    if mode == "warm" {
        receipt(
            "warm-initialized",
            json!({"pid":std::process::id(),"home":std::env::var("HOME").ok(),"codex_home":std::env::var("CODEX_HOME").ok(),"ambient_key":std::env::var_os("OPENAI_API_KEY").is_some()}),
        )?;
        if Path::new("owned-mcp.warm-idle-gate").exists() {
            let until = Instant::now() + Duration::from_secs(5);
            while !Path::new("owned-mcp.warm-idle-exit").exists() {
                if Instant::now() >= until {
                    return Err(invalid());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            return Ok(());
        }
    }
    let thread = request(&mut input, "thread/start", &mut log)?;
    if mode == "warm" {
        receipt("warm-thread", json!({"pid":std::process::id()}))?;
    }
    let params = &thread["params"];
    if params["approvalPolicy"] != "on-request"
        || params["sandbox"] != "workspace-write"
        || params["approvalsReviewer"] != "user"
    {
        return Err(invalid());
    }
    send(
        &mut output,
        json!({"id":thread["id"],"result":{"thread":{"id":"owned-thread","cwd":params["cwd"],"status":{"type":"idle"},"turns":[]},"cwd":params["cwd"],"model":params["model"],"modelProvider":"offline","approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"workspaceWrite"}}}),
    )?;
    let turn = request(&mut input, "turn/start", &mut log)?;
    if turn["params"]["threadId"] != "owned-thread"
        || turn["params"]["sandboxPolicy"]["networkAccess"] != false
    {
        return Err(invalid());
    }
    send(
        &mut output,
        json!({"id":turn["id"],"result":{"turn":{"id":"owned-turn","status":"inProgress","items":[]}}}),
    )?;
    if Path::new("owned-mcp.fail-notification").is_file() {
        send(
            &mut output,
            json!({"method":"offlinePrivateNotification","params":{"threadId":"owned-thread","turnId":"owned-turn"}}),
        )?;
        std::thread::sleep(Duration::from_secs(8));
        return Ok(());
    }
    helper(params, &turn, &mode, &mut input, &mut output)?;
    if ["heartbeat", "retained", "warm"].contains(&mode.as_str())
        && !Path::new("projects/factory_project/configured-fleet-probe").is_file()
    {
        send(
            &mut output,
            json!({"method":"item/started","params":{"startedAtMs":1,"threadId":"owned-thread","turnId":"owned-turn","item":{"id":"answer","type":"agentMessage","phase":"final_answer","text":""}}}),
        )?;
        send(
            &mut output,
            json!({"method":"item/completed","params":{"completedAtMs":2,"threadId":"owned-thread","turnId":"owned-turn","item":{"id":"answer","type":"agentMessage","phase":"final_answer","text":"Native helper heartbeat confirmed"}}}),
        )?;
        send(
            &mut output,
            json!({"method":"turn/completed","params":{"threadId":"owned-thread","turn":{"id":"owned-turn","status":"completed","items":[]}}}),
        )?;
    }
    // Neither helper exit nor terminal output exits this owned runtime. Done
    // intentionally has no turn terminal event: fresh renewal must fence it.
    std::thread::sleep(Duration::from_secs(8));
    Ok(())
}
fn main() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    #[cfg(unix)]
    if args.first().is_some_and(|v| v == "guardian") {
        return hagency_platform::run_guardian();
    }
    if args.as_slice() == [std::ffi::OsString::from("claude-task-peer")] {
        return claude_mcp_peer::run();
    }
    if args.as_slice() != [std::ffi::OsString::from("app-server")] {
        return Err(invalid());
    }
    if Path::new("account-probe.required").exists() {
        let home = std::env::var_os("HOME").ok_or_else(invalid)?;
        let codex = std::env::var_os("CODEX_HOME").ok_or_else(invalid)?;
        if home != codex
            || std::env::var_os("OPENAI_API_KEY").is_some()
            || std::env::var_os("CODEX_API_KEY").is_some()
        {
            return Err(invalid());
        }
        let marker = fs::read_to_string(Path::new(&home).join("fixture-account-marker"))?;
        if marker != "bootstrap-selected" {
            return Err(invalid());
        }
        fs::write(
            "account-observed.json",
            serde_json::to_vec(&json!({"marker":marker,"same_home":true,"ambient_key":false}))?,
        )?;
    }
    if Path::new("local-account-probe.required").exists() {
        let home = std::env::var_os("HOME").ok_or_else(invalid)?;
        let codex = std::env::var_os("CODEX_HOME").ok_or_else(invalid)?;
        if home == codex
            || ["OPENAI_API_KEY", "CODEX_API_KEY", "HAGENCY_DASHBOARD_TOKEN"]
                .iter()
                .any(|key| std::env::var_os(key).is_some())
            || fs::read_to_string(Path::new(&codex).join("fixture-account-marker"))?
                != "bootstrap-local"
        {
            return Err(invalid());
        }
        fs::write(
            "local-account-observed.json",
            serde_json::to_vec(&json!({"selected":true,"same_home":false,"ambient_key":false}))?,
        )?;
    }
    // Disposable peer-wide bound: even a broken synchronous pipe cannot hang a
    // fixture forever. No detached daemon cleanup and no successful exit claim.
    //
    // It bounds the WHOLE peer, and a warm peer lives from its agent's
    // provisioning, through the wait for every other agent, to the end of its own
    // task. At 15 s that was shorter than a two-agent fleet needs on a hosted
    // runner: the first agent's peer exited 74 while idle, the guardian reported
    // the leader gone, and the handoff was refused as lost_authority; when it
    // fired mid-task the host saw peer_eof instead. It must outlast what a
    // fixture may legitimately ask of it (120 s warm idle plus a 60 s operation);
    // the host's own budgets stop a peer long before this does.
    let (release, wait) = mpsc::sync_channel::<()>(1);
    let watchdog = std::thread::spawn(move || {
        if wait.recv_timeout(Duration::from_secs(240)).is_err() {
            std::process::exit(74);
        }
    });
    let result = fake();
    let _ = release.send(());
    let _ = watchdog.join();
    result.map_err(|_| invalid())
}
