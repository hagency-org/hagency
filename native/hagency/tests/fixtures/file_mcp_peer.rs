//! Disposable file-tool app-server peer. Every file operation uses the real native MCP and loopback service.
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
    "HAGENCY_FILE_TOOLS",
];
fn invalid() -> io::Error {
    io::Error::other("offline file MCP fixture refused or incomplete")
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
fn receipt(stage: &str, value: Value) -> io::Result<()> {
    let target = format!("file-mcp.{stage}");
    let temporary = format!("{target}.tmp");
    fs::write(&temporary, serde_json::to_vec(&value)?)?;
    fs::rename(temporary, target)
}
fn helper(params: &Value) -> io::Result<()> {
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
                "send_file",
                "get_file_delivery"
            ])
        || table.get("env").is_some()
        || table.get("url").is_some()
        || table.get("default_tools_approval_mode").is_some()
        || config["shell_environment_policy.inherit"] != "none"
        || config["shell_environment_policy.experimental_use_profile"] != false
        || std::env::var("HAGENCY_FILE_TOOLS").ok().as_deref() != Some("1")
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
    let mut output = child.0.stdin.take().ok_or_else(invalid)?;
    let mut input = BufReader::new(child.0.stdout.take().ok_or_else(invalid)?);
    let mut stderr = child.0.stderr.take().ok_or_else(invalid)?;
    rpc(
        &mut input,
        &mut output,
        0,
        "initialize",
        json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"file-offline-peer","version":"1"}}),
    )?;
    send(
        &mut output,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )?;

    // First actual tool request is the file mutation. This cannot work by racing
    // a later fixture registration. Arguments contain only fixed source selection.
    let admission = rpc(
        &mut input,
        &mut output,
        1,
        "tools/call",
        json!({
            "name":"send_file", "arguments":{"call_id":"file_service_send",
            "path":"sample.bin", "filename":"原始文件.bin",
            "caption":"Native file from the original workspace"}
        }),
    )?;
    receipt("admission", admission.clone())?;
    if admission["isError"] != false {
        return Err(invalid());
    }
    // Preserve only this disposable child's actually inherited context for a
    // fresh historical-read fixture. No authority is created or printed, and
    // the file is private and removed with the isolated test workspace.
    let _original = hagency::task_client::Context::from_env().map_err(|_| invalid())?;
    let inherited = ENV
        .into_iter()
        .map(|key| {
            std::env::var(key)
                .map(|value| (key, value))
                .map_err(|_| invalid())
        })
        .collect::<io::Result<std::collections::BTreeMap<_, _>>>()?;
    let inherited = serde_json::to_vec(&inherited)?;
    if inherited.len() > 8192 {
        return Err(invalid());
    }
    hagency_store::private::write_new(
        &std::env::current_dir()?.join("file-mcp.context"),
        &inherited,
    )
    .map_err(|_| invalid())?;
    let id = admission["structuredContent"]["delivery_id"]
        .as_str()
        .ok_or_else(invalid)?
        .to_owned();
    let mut observed = admission["structuredContent"].clone();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut next = 2;
    while !matches!(observed["status"].as_str(), Some("delivered" | "failed"))
        && Instant::now() < deadline
        && next < 128
    {
        let result = rpc(
            &mut input,
            &mut output,
            next,
            "tools/call",
            json!({"name":"get_file_delivery","arguments":{"delivery_id":id}}),
        )?;
        if result["isError"] != false {
            receipt("read-error", result)?;
            return Err(invalid());
        }
        observed = result["structuredContent"].clone();
        if observed["delivery_id"] != id {
            return Err(invalid());
        }
        next += 1;
        // This is read-only bounded observation, never another send_file call.
        std::thread::sleep(Duration::from_millis(50));
    }
    receipt("delivery", observed.clone())?;
    let task = std::env::var("HAGENCY_TASK_ID").map_err(|_| invalid())?;
    let result = rpc(
        &mut input,
        &mut output,
        next,
        "tools/call",
        json!({"name":"get_task","arguments":{"id":task}}),
    )?;
    if result["isError"] != false
        || result["structuredContent"]["task"]["id"] != task
        || result["structuredContent"]["task"]["status"] != "in_progress"
    {
        return Err(invalid());
    }
    receipt("task", result["structuredContent"]["task"].clone())?;
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
    let mut diagnostic = Vec::new();
    (&mut stderr).take(1025).read_to_end(&mut diagnostic)?;
    if !diagnostic.is_empty() {
        return Err(invalid());
    }
    // A negative/unknown status is recorded as observed, not promoted to success.
    // Positive executable tests must independently assert Delivered and decrypt.
    receipt("receipt", json!({"delivery":observed,"helper_exit":true}))?;
    Ok(())
}
fn fake() -> io::Result<()> {
    let mut log = fs::File::create("file-mcp.requests")?;
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
    helper(params)?;
    {
        send(
            &mut output,
            json!({"method":"item/started","params":{"startedAtMs":1,"threadId":"owned-thread","turnId":"owned-turn","item":{"id":"answer","type":"agentMessage","phase":"final_answer","text":""}}}),
        )?;
        send(
            &mut output,
            json!({"method":"item/completed","params":{"completedAtMs":2,"threadId":"owned-thread","turnId":"owned-turn","item":{"id":"answer","type":"agentMessage","phase":"final_answer","text":"Native file status inspected"}}}),
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
