//! Callback-capable offline app-server peer (PC-C0b). The composition's
//! configured executable is whatever the test pins, so this fixture is a
//! test-built binary whose path + sha256 the test pins into
//! `development-driver.json` — never a production switch, never an
//! environment variable the production launch would read. The wired delivery
//! leg (PC-C0's pump) is exercised end to end by emitting one real
//! `item/commandExecution/requestApproval` so the pump takes the
//! single-consumer receiver, rebuilds the card from the admitted domain
//! request, and drives `send_private_approval_card` against the shared fake
//! peer. No production path changes.
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, Write},
    time::Duration,
};
fn invalid() -> io::Error {
    io::Error::other("callback-capable approval fixture refused or incomplete")
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
fn fake() -> io::Result<()> {
    let mut log = fs::File::create("approval-mcp.requests")?;
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
        json!({"id":thread["id"],"result":{"thread":{"id":"approval-thread","cwd":params["cwd"],"status":{"type":"idle"},"turns":[]},"cwd":params["cwd"],"model":params["model"],"modelProvider":"offline","approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"workspaceWrite"}}}),
    )?;
    let turn = request(&mut input, "turn/start", &mut log)?;
    if turn["params"]["threadId"] != "approval-thread"
        || turn["params"]["sandboxPolicy"]["networkAccess"] != false
    {
        return Err(invalid());
    }
    send(
        &mut output,
        json!({"id":turn["id"],"result":{"turn":{"id":"approval-turn","status":"inProgress","items":[]}}}),
    )?;
    // The delivery trigger: one real command-execution approval request. The
    // execution layer acknowledges it and emits the ApprovalNotice the pump
    // consumes. The runtime's parse requires the shared fields plus
    // `kind:"command"`; the command/cwd are bounded text.
    send(
        &mut output,
        json!({"id":7,"method":"item/commandExecution/requestApproval","params":{"threadId":"approval-thread","turnId":"approval-turn","itemId":"item-approval","startedAtMs":1,"kind":"command","command":"echo approval-delivery","cwd":params["cwd"]}}),
    )?;
    // Only this pinned fixture reads this marker. Record the actual callback
    // wire response, not a test-side injected permission or an assumed ACK.
    if std::path::Path::new("approval-mcp.roundtrip").exists() {
        let response = read(&mut input)?;
        if response["id"] != 7
            || !matches!(
                response["result"]["decision"].as_str(),
                Some("accept" | "decline")
            )
        {
            return Err(invalid());
        }
        send(&mut log, response.clone())?;
        let mut receipt = fs::File::create("approval-mcp.response.pending")?;
        send(&mut receipt, response)?;
        drop(receipt);
        fs::rename("approval-mcp.response.pending", "approval-mcp.response")?;
        send(
            &mut output,
            json!({"method":"turn/completed","params":{"threadId":"approval-thread","turn":{"id":"approval-turn","status":"completed","items":[]}}}),
        )?;
        // Protocol completion is not canonical task completion or an upstream
        // permission-applied acknowledgment. The test asserts neither.
        return Ok(());
    }
    // The runtime's response to id 7 arrives only when the OWNER resolves
    // the approval (the console verdict) — which this fixture never does,
    // because the scenario observes the DELIVERY, not the verdict. So do
    // not block on it: keep the turn open (no terminal event, the model
    // probe's discipline) and hold the process alive through the send
    // window so the pump can drain the notice and drive the card.
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
    let (release, wait) = std::sync::mpsc::sync_channel::<()>(1);
    let watchdog = std::thread::spawn(move || {
        // A last resort against a hung fixture, so it must outlast the longest
        // operation budget a fixture gives this peer (20 s). At 15 s it was
        // shorter than that, and on a slow runner it would end a healthy peer;
        // owned_mcp_peer did exactly that in the hosted fleet fixtures.
        if wait.recv_timeout(Duration::from_secs(90)).is_err() {
            std::process::exit(74);
        }
    });
    let result = fake();
    let _ = release.send(());
    let _ = watchdog.join();
    result.map_err(|_| invalid())
}
