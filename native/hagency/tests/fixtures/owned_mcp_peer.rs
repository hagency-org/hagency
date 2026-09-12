//! Disposable offline app-server peer. Never a live model or daemon launcher.
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
const ENV: [&str; 3] = [
    "HAGENCY_RUNNER_API_ADDR",
    "HAGENCY_RUNNER_CAPABILITY",
    "HAGENCY_TASK_ID",
];
fn invalid() -> io::Error {
    io::Error::other("offline MCP fixture refused or incomplete")
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
fn helper(params: &Value, mode: &str) -> io::Result<()> {
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
                "complete_task_with_reply"
            ])
        || table.get("env").is_some()
        || table.get("url").is_some()
        || table.get("default_tools_approval_mode").is_some()
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
    let task = std::env::var("HAGENCY_TASK_ID").map_err(|_| invalid())?;
    let before = tool(&mut input, &mut output, 1, "get_task", json!({"id":task}))?;
    if before["id"] != task || before["status"] != "in_progress" {
        return Err(invalid());
    }
    let done = mode == "done";
    let after = if mode == "finish" {
        let response = rpc(
            &mut input,
            &mut output,
            2,
            "tools/call",
            json!({"name":"complete_task_with_reply","arguments":{"id":task,"call_id":"owned_finish","body":"Verified **native MCP final result**"}}),
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
    if mode == "finish" {
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
    let mode = std::env::var("HAGENCY_OFFLINE_MODE").unwrap_or_else(|_| "heartbeat".into());
    if !["heartbeat", "done", "finish"].contains(&mode.as_str()) {
        return Err(invalid());
    }
    let mut log = fs::File::create("owned-mcp.requests")?;
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
    helper(params, &mode)?;
    if mode == "heartbeat" {
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
    // Disposable peer-wide bound: even a broken synchronous pipe cannot hang a
    // fixture forever. No detached daemon cleanup and no successful exit claim.
    let (release, wait) = mpsc::sync_channel::<()>(1);
    let watchdog = std::thread::spawn(move || {
        if wait.recv_timeout(Duration::from_secs(15)).is_err() {
            std::process::exit(74);
        }
    });
    let result = fake();
    let _ = release.send(());
    let _ = watchdog.join();
    result.map_err(|_| invalid())
}
