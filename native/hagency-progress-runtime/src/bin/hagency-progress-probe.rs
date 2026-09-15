//! Offline protocol fixture. No model, credential, network or arbitrary command.
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, Write},
    path::Path,
    time::{Duration, Instant},
};
fn read(reader: &mut impl BufRead) -> io::Result<Value> {
    let mut bytes = Vec::new();
    loop {
        let part = reader.fill_buf()?;
        if part.is_empty() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let n = part
            .iter()
            .position(|&x| x == b'\n')
            .map_or(part.len(), |n| n + 1);
        if bytes.len() + n > 1_048_577 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        bytes.extend_from_slice(&part[..n]);
        reader.consume(n);
        if bytes.last() == Some(&b'\n') {
            return serde_json::from_slice(&bytes).map_err(io::Error::other);
        }
    }
}
fn send(value: Value) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(&value)?;
    bytes.push(b'\n');
    let mut out = io::stdout().lock();
    out.write_all(&bytes)?;
    out.flush()
}
fn reply(request: &Value, value: Value) -> io::Result<()> {
    send(json!({"id":request["id"],"result":value}))
}
fn expected(input: &mut impl BufRead, method: &str) -> io::Result<Value> {
    let v = read(input)?;
    if v["method"] != method {
        return Err(io::Error::other("offline fixture method mismatch"));
    }
    Ok(v)
}
fn pulse(marker: &Path) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(marker)?;
    let until = Instant::now() + Duration::from_secs(8);
    while Instant::now() < until {
        file.write_all(b"x")?;
        file.flush()?;
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
fn serve(mode: &str, marker: &Path) -> io::Result<()> {
    let mut input = io::stdin().lock();
    reply(
        &expected(&mut input, "initialize")?,
        json!({"userAgent":"fixture/0.153.4","platformFamily":"fixture","platformOs":"fixture","codexHome":"/fixture"}),
    )?;
    expected(&mut input, "initialized")?;
    let req = expected(&mut input, "thread/start")?;
    let p = &req["params"];
    if p["approvalPolicy"] != "on-request" || p["sandbox"] != "workspace-write" {
        return Err(io::Error::other("offline fixture launch policy mismatch"));
    }
    reply(
        &req,
        json!({"thread":{"id":"owned-thread","cwd":p["cwd"],"status":{"type":"idle"},"turns":[]},"cwd":p["cwd"],"model":p["model"],"modelProvider":"fixture","approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"workspaceWrite","networkAccess":false}}),
    )?;
    reply(
        &expected(&mut input, "turn/start")?,
        json!({"turn":{"id":"owned-turn","status":"inProgress","items":[]}}),
    )?;
    if mode == "silent" {
        return pulse(marker);
    }
    for (id, kind, status, exit) in [
        ("command", "commandExecution", "completed", json!(0)),
        ("edit", "fileChange", "failed", Value::Null),
        ("unknown", "commandExecution", "completed", Value::Null),
        ("mcp", "mcpToolCall", "completed", Value::Null),
    ] {
        for done in [false, true] {
            let mut item = json!({"id":id,"type":kind,"status":if done {status} else {"inProgress"},"command":"SECRET /private/.env","cwd":"/private/SECRET","error":"SECRET credential","result":"SECRET token","changes":[]});
            if done && !exit.is_null() {
                item["exitCode"] = exit.clone();
            }
            send(
                json!({"method":if done {"item/completed"}else{"item/started"},"params":{"threadId":"owned-thread","turnId":"owned-turn","startedAtMs":1,"completedAtMs":2,"item":item}}),
            )?;
        }
    }
    for done in [false, true] {
        send(
            json!({"method":if done {"item/completed"}else{"item/started"},"params":{"threadId":"owned-thread","turnId":"owned-turn","startedAtMs":1,"completedAtMs":2,"item":{"id":"answer","type":"agentMessage","phase":"final_answer","text":if done {"SECRET model answer"}else{""}}}}),
        )?;
    }
    send(
        json!({"method":"turn/completed","params":{"threadId":"owned-thread","turn":{"id":"owned-turn","status":"completed","items":[]}}}),
    )?;
    // Protocol end does not exit the process: retained host ownership must stop it.
    pulse(marker)
}
fn main() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    #[cfg(unix)]
    if args.first().is_some_and(|v| v == "guardian") {
        return hagency_platform::run_guardian();
    }
    match args.as_slice() {
        [mode, marker] if mode == "normal" || mode == "silent" => serve(
            mode.to_str().ok_or(io::ErrorKind::InvalidInput)?,
            Path::new(marker),
        ),
        _ => Err(io::Error::other("offline fixture mode required")),
    }
}
