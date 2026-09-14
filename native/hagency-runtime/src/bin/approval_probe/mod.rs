//! Offline callbacks use the pinned native shape and read real response bytes.
use super::*;

fn callback(id: &str) -> io::Result<()> {
    send(
        json!({"id":id,"method":"item/commandExecution/requestApproval","params":{
            "threadId":"owned-thread","turnId":"owned-turn","itemId":id,
            "startedAtMs":1,"command":"echo approved","cwd":std::env::current_dir()?.to_string_lossy(),
            "availableDecisions":["accept","decline"]
        }}),
    )
}
fn resolved(id: &str) -> io::Result<()> {
    note(
        "serverRequest/resolved",
        json!({"threadId":"owned-thread","requestId":id}),
    )
}
pub(super) fn gate(marker: &Path) -> io::Result<()> {
    fs::write(marker.with_extension("approval-ready"), b"ready")?;
    // Derived from the operation budget (one tenth, doubled: the test side
    // must first observe something of its own before releasing) — never a
    // literal, which let the probe give up while a loaded host was still
    // inside its operation budget. Expiry names what it was waiting for.
    let until = Instant::now() + harness_wait() * 2;
    while !marker.with_extension("approval-release").is_file() {
        if Instant::now() >= until {
            return Err(io::Error::other(
                "probe timed out waiting for the host to write owned-dispatch.approval-release",
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
/// Hold until the HOST stops ownership, observed as EOF on the response
/// stream we were already given (the parent's own `StdinLock`, borrowed as
/// `reader` — never a second lock, which would deadlock on the guard).
/// Pulses for evidence while polling; the budget is the outer ceiling. Used
/// by the cancellation mode, whose subject ends before the host does.
fn hold_reader_to_eof(reader: &mut impl BufRead, marker: &Path) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(marker.with_extension("pulse"))?;
    let until = Instant::now() + Duration::from_millis(operation_budget_ms() * 3 / 2);
    loop {
        file.write_all(b"x")?;
        file.flush()?;
        // An empty fill_buf is EOF: the host closed our stdin (ownership
        // stop). The bytes are left unconsumed for any later reader.
        if reader.fill_buf()?.is_empty() {
            return Ok(());
        }
        if Instant::now() >= until {
            return Err(io::Error::other(
                "probe held past its derived ceiling without the host closing stdin",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
pub(super) fn run(mode: &str, reader: &mut impl BufRead, marker: &Path) -> io::Result<bool> {
    callback("approval-1")?;
    if mode == "owned-approval-eof" {
        // EOF is this mode's cancellation subject, but it must never overtake
        // the callback that authorizes it: this probe used to exit right after
        // writing the callback, and on a loaded host the teardown closed
        // stdout before the host's first read had consumed the callback line,
        // stranding a parsed server request (termination snapshot:
        // pending_server_requests 1) so the notice never arrived and the run
        // reported Protocol before the approval drive ever ran. Gate on the
        // host-side release — the test writes it only once the notice proves
        // the callback was retained — then exit; teardown closes stdout and
        // the EOF arrives ordered, still while the approval is pending.
        gate(marker)?;
        return Ok(false);
    }
    if mode == "owned-approval-resolve" {
        // Cancellation: the probe must resolve without reading a response —
        // that is its subject — so the handshake is explicit both ways and
        // the probe never exits before the host is done with it: announce
        // the resolution (the test can release the host on THIS marker),
        // then hold to the host's stdin close.
        gate(marker)?;
        resolved("approval-1")?;
        fs::write(marker.with_extension("approval-resolving"), b"resolving")?;
        hold_reader_to_eof(reader, marker)?;
        return Ok(false);
    }
    if mode == "owned-approval-barriers" {
        callback("approval-2")?;
    }
    if mode == "owned-approval-queued" {
        gate(marker)?;
        callback("approval-2")?;
    }
    if matches!(mode, "owned-approval-usage" | "owned-approval-queued-usage") {
        gate(marker)?;
        for n in 1..=3 {
            note(
                "thread/tokenUsage/updated",
                json!({"threadId":"owned-thread","turnId":"owned-turn","tokenUsage":{
                    "total":{"totalTokens":n*11,"inputTokens":n*10,"cachedInputTokens":0,"outputTokens":n,"reasoningOutputTokens":0},
                    "last":{"totalTokens":11,"inputTokens":10,"cachedInputTokens":0,"outputTokens":1,"reasoningOutputTokens":0},"modelContextWindow":200000
                }}),
            )?;
        }
    }
    if mode == "owned-approval-queued-usage" {
        callback("approval-2")?;
    }
    let count = if mode == "owned-approval-barriers" {
        3
    } else if matches!(
        mode,
        "owned-approval-reuse" | "owned-approval-queued" | "owned-approval-queued-usage"
    ) {
        2
    } else {
        1
    };
    let mut ids = std::collections::BTreeSet::new();
    for index in 0..count {
        let response = read(reader, marker)?;
        let id = response["id"].as_str().ok_or(io::ErrorKind::InvalidData)?;
        if !id.starts_with("approval-")
            || !ids.insert(id.to_owned())
            || response.get("method").is_some()
            || !matches!(
                response["result"]["decision"].as_str(),
                Some("accept" | "decline")
            )
        {
            return Err(io::Error::other("invalid or duplicated approval response"));
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(marker.with_extension("approval-bytes"))?
            .write_all(&serde_json::to_vec(&response)?)?;
        if mode == "owned-approval-barriers" && index == 0 {
            callback("approval-3")?;
        }
        resolved(id)?;
        if mode == "owned-approval-reuse" && index == 0 {
            callback("approval-2")?;
        }
    }
    fs::write(
        marker.with_extension("approval-continued"),
        b"actual responses received",
    )?;
    Ok(true)
}
