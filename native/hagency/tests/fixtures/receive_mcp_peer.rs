//! Disposable receive-tool app-server peer. Every file operation uses the real native MCP and loopback service.
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
const ENV: [&str; 4] = [
    "HAGENCY_RUNNER_API_ADDR",
    "HAGENCY_RUNNER_CAPABILITY",
    "HAGENCY_TASK_ID",
    "HAGENCY_RECEIVE_FILE_TOOLS",
];
fn invalid() -> io::Error {
    io::Error::other("offline receive MCP fixture refused or incomplete")
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
    receipt("phase", json!({"stage":"runtime_wait","method":method}))?;
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
    receipt("phase", json!({"rpc_id":id,"method":method}))?;
    send(
        out,
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
    )?;
    // Diagnostic only: prove the request was flushed, so a later host-side
    // timeout can be attributed to delivery rather than to this peer stalling.
    receipt("sent", json!({"rpc_id":id,"method":method}))?;
    let response = read(input)?;
    if response["id"] != id || response.get("error").is_some() {
        return Err(invalid());
    }
    Ok(response["result"].clone())
}
fn receipt(stage: &str, value: Value) -> io::Result<()> {
    let target = format!("receive-mcp.{stage}");
    let temporary = format!("{target}.tmp");
    fs::write(&temporary, serde_json::to_vec(&value)?)?;
    fs::rename(temporary, target)
}
fn helper(params: &Value, turn: &Value, progress: &mut impl Write) -> io::Result<()> {
    receipt("phase", json!({"stage":"check_helper_config"}))?;
    use sha2::{Digest, Sha256};
    let config = &params["config"];
    let table = &config["mcp_servers.hagency_task_writer"];
    let executable = table["command"].as_str().ok_or_else(invalid)?;
    if !Path::new(executable).is_absolute()
        || table["args"] != json!(["mcp"])
        || table["cwd"] != params["cwd"]
        || table["env_vars"] != json!(ENV)
        || table["enabled_tools"]
            != json!([
                "get_task",
                "update_task_execution",
                "transition_task",
                "complete_task_with_reply",
                "list_received_files",
                "receive_file"
            ])
        || table.get("env").is_some()
        || table.get("url").is_some()
        || config["shell_environment_policy.inherit"] != "none"
        || config["shell_environment_policy.experimental_use_profile"] != false
        || std::env::var("HAGENCY_RECEIVE_FILE_TOOLS").ok().as_deref() != Some("1")
        || std::env::var_os("HAGENCY_FILE_TOOLS").is_some()
    {
        return Err(invalid());
    }
    receipt("phase", json!({"stage":"check_selected_prompt"}))?;
    let prompt = serde_json::to_string(&turn["input"])?;
    if !prompt.contains("$incoming") || prompt.contains("mxc://") || prompt.contains("key_ops") {
        return Err(invalid());
    }
    receipt("prompt", turn["input"].clone())?;
    let _original = hagency::task_client::Context::from_env().map_err(|_| invalid())?;
    let inherited = ENV
        .into_iter()
        .map(|key| Ok((key, std::env::var(key).map_err(|_| invalid())?)))
        .collect::<io::Result<std::collections::BTreeMap<_, _>>>()?;
    hagency_store::private::write_new(
        &std::env::current_dir()?.join("receive-mcp.context"),
        &serde_json::to_vec(&inherited)?,
    )
    .map_err(|_| invalid())?;
    let mut command = Command::new(executable);
    command
        .arg("mcp")
        .current_dir(table["cwd"].as_str().ok_or_else(invalid)?)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in ENV {
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
    let mut output = child.0.stdin.take().ok_or_else(invalid)?;
    let mut input = BufReader::new(child.0.stdout.take().ok_or_else(invalid)?);
    let mut stderr = child.0.stderr.take().ok_or_else(invalid)?;
    rpc(
        &mut input,
        &mut output,
        0,
        "initialize",
        json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"receive-offline-peer","version":"1"}}),
    )?;
    send(
        &mut output,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )?;
    let catalog = rpc(&mut input, &mut output, 1, "tools/list", json!({}))?;
    let names: Vec<_> = catalog["tools"]
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    if !names.contains(&"list_received_files")
        || !names.contains(&"receive_file")
        || names.contains(&"send_file")
    {
        return Err(invalid());
    }
    let list = rpc(
        &mut input,
        &mut output,
        2,
        "tools/call",
        json!({"name":"list_received_files","arguments":{}}),
    )?;
    receipt("list", list.clone())?;
    if list["isError"] != false {
        return Err(invalid());
    }
    let items = list["structuredContent"]["items"]
        .as_array()
        .ok_or_else(invalid)?;
    if items.len() != 1
        || items[0]["event_id"] != "$incoming"
        || items[0]["metadata"]["filename"] != "原始文件.bin"
    {
        return Err(invalid());
    }
    let event = items[0]["event_id"].as_str().ok_or_else(invalid)?;
    let call = json!({"name":"receive_file","arguments":{"event_id":event}});
    let first = rpc(&mut input, &mut output, 3, "tools/call", call.clone())?;
    receipt("first", first.clone())?;
    let mode = fs::read_to_string("receive-fixture.mode")?;
    if mode == "negative" {
        if first["isError"] != true {
            return Err(invalid());
        }
    } else {
        if first["isError"] != false {
            return Err(invalid());
        }
        let value = &first["structuredContent"];
        let path = value["path"].as_str().ok_or_else(invalid)?;
        let suffix = path
            .strip_prefix(".hagency-received-")
            .and_then(|v| v.strip_suffix(".bin"))
            .ok_or_else(invalid)?;
        if suffix.len() != 32
            || !suffix
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || value["event_id"] != event
            || value["replayed"] != false
        {
            return Err(invalid());
        }
        let bytes = fs::read(path)?;
        if bytes.len() > 4194304
            || value["size"] != bytes.len()
            || value["sha256"] != format!("{:x}", Sha256::digest(&bytes))
        {
            return Err(invalid());
        }
        receipt(
            "bytes",
            json!({"bytes":bytes,"cwd":std::env::current_dir()?.to_str().ok_or_else(invalid)?,"size":value["size"],"sha256":value["sha256"],"path":path}),
        )?;
        if mode == "replay" {
            // The original five-second write deadline has elapsed. A retained
            // Ready owner must use its fresh read-only response deadline.
            send(
                progress,
                json!({"method":"item/started","params":{"startedAtMs":1,"threadId":"owned-thread","turnId":"owned-turn","item":{"id":"replay_wait","type":"agentMessage","phase":"commentary","text":""}}}),
            )?;
            let mut text = String::new();
            for _ in 0..12 {
                // Real scripted runtime progress keeps the unchanged protocol
                // response deadline live while the service write deadline ages.
                std::thread::sleep(Duration::from_millis(500));
                let delta = "Waiting to verify the retained original file. ";
                text.push_str(delta);
                send(
                    progress,
                    json!({"method":"item/agentMessage/delta","params":{"threadId":"owned-thread","turnId":"owned-turn","itemId":"replay_wait","delta":delta}}),
                )?;
            }
            send(
                progress,
                json!({"method":"item/completed","params":{"completedAtMs":6001,"threadId":"owned-thread","turnId":"owned-turn","item":{"id":"replay_wait","type":"agentMessage","phase":"commentary","text":text}}}),
            )?;
            let repeated = rpc(&mut input, &mut output, 4, "tools/call", call.clone())?;
            receipt("replay", repeated.clone())?;
            if repeated["isError"] != false
                || repeated["structuredContent"]["replayed"] != true
                || repeated["structuredContent"]["path"] != path
            {
                return Err(invalid());
            }
            fs::write(path, b"mutated original received destination")?;
            let changed = rpc(&mut input, &mut output, 5, "tools/call", call)?;
            receipt("changed", changed.clone())?;
            if changed["isError"] != true {
                return Err(invalid());
            }
        }
    }
    let task = std::env::var("HAGENCY_TASK_ID").map_err(|_| invalid())?;
    let task = rpc(
        &mut input,
        &mut output,
        6,
        "tools/call",
        json!({"name":"get_task","arguments":{"id":task}}),
    )?;
    if task["isError"] != false || task["structuredContent"]["task"]["status"] != "in_progress" {
        return Err(invalid());
    }
    receipt("task", task["structuredContent"]["task"].clone())?;
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
    let mut errors = Vec::new();
    (&mut stderr).take(1025).read_to_end(&mut errors)?;
    if !errors.is_empty() {
        return Err(invalid());
    }
    receipt(
        "receipt",
        json!({"mode":mode,"helper_exit":true,"receive":first}),
    )?;
    Ok(())
}
fn fake() -> io::Result<()> {
    let mut log = fs::File::create("receive-mcp.requests")?;
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let init = request(&mut input, "initialize", &mut log)?;
    send(
        &mut output,
        json!({"id":init["id"],"result":{"userAgent":"offline/0.153.4","platformFamily":"fixture","platformOs":"fixture","codexHome":"/fixture"}}),
    )?;
    request(&mut input, "initialized", &mut log)?;
    let thread = request(&mut input, "thread/start", &mut log)?;
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
    // Diagnostic only: the host's response budget once expired while this
    // peer was inside helper(). This stage separates "reply emitted, delivery
    // slow" from "reply never emitted" without changing any budget.
    receipt("stage", json!({"stage":"turn_start_replied"}))?;
    helper(params, &turn["params"], &mut output)?;
    {
        send(
            &mut output,
            json!({"method":"item/started","params":{"startedAtMs":1,"threadId":"owned-thread","turnId":"owned-turn","item":{"id":"answer","type":"agentMessage","phase":"final_answer","text":""}}}),
        )?;
        send(
            &mut output,
            json!({"method":"item/completed","params":{"completedAtMs":2,"threadId":"owned-thread","turnId":"owned-turn","item":{"id":"answer","type":"agentMessage","phase":"final_answer","text":"Native received bytes inspected"}}}),
        )?;
        send(
            &mut output,
            json!({"method":"turn/completed","params":{"threadId":"owned-thread","turn":{"id":"owned-turn","status":"completed","items":[]}}}),
        )?;
    }
    // Neither helper exit nor terminal output exits this owned runtime. The
    // original driver must perform its ordinary owned process cleanup.
    std::thread::sleep(Duration::from_secs(8));
    Ok(())
}
fn main() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    #[cfg(unix)]
    if args.first().is_some_and(|v| v == "guardian") {
        return hagency_platform::run_guardian();
    }
    if args.as_slice() != [std::ffi::OsString::from("app-server")] {
        return Err(invalid());
    }
    receipt(
        "entry",
        json!({"stage":"runtime_main","pid":std::process::id()}),
    )?;
    // Disposable peer-wide bound: even a broken synchronous pipe cannot hang a
    // fixture forever. No detached daemon cleanup and no successful exit claim.
    let (release, wait) = mpsc::sync_channel::<()>(1);
    let watchdog = std::thread::spawn(move || {
        if wait.recv_timeout(Duration::from_secs(25)).is_err() {
            std::process::exit(74);
        }
    });
    let result = fake();
    let _ = release.send(());
    let _ = watchdog.join();
    result.map_err(|_| invalid())
}
