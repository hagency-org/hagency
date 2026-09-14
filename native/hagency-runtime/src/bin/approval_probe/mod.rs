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
/// Announce that the probe reached a stage, by writing the marker file the
/// test polls for.
pub(super) fn announce(marker: &Path, extension: &str) -> io::Result<()> {
    fs::write(marker.with_extension(extension), b"ready")
}

/// Wait until the test writes the release marker, bounded by the derived
/// harness wait (one tenth of the operation budget, doubled where the test
/// side must first observe something of its own) — never a literal, which
/// is what let the probe give up while a loaded host was still inside its
/// operation budget. Expiry names what it was waiting for.
pub(super) fn await_release(marker: &Path, extension: &str) -> io::Result<()> {
    let until = Instant::now() + harness_wait() * 2;
    while !marker.with_extension(extension).is_file() {
        if Instant::now() >= until {
            return Err(io::Error::other(format!(
                "probe timed out waiting for the host to write owned-dispatch.{extension}"
            )));
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

/// One half of the original gate: announce readiness, then wait.
pub(super) fn gate(marker: &Path) -> io::Result<()> {
    announce(marker, "approval-ready")?;
    await_release(marker, "approval-release")
}

/// Append one raw response frame to the probe-read stream.
pub(super) fn append_bytes(marker: &Path, response: &Value) -> io::Result<()> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(marker.with_extension("approval-bytes"))?
        .write_all(&serde_json::to_vec(response)?)
}

/// Bounded probe for one more inbound frame: `Ok(line)` when a frame arrives
/// inside the bound, `Err(TimedOut)` when none did. The caller's buffered
/// reader cannot be bounded, so a fresh stdin lock runs on a reader thread.
pub(super) fn timeout_read(duration: Duration) -> io::Result<String> {
    let (sent, received) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        let mut line = String::new();
        let _ = stdin.read_line(&mut line);
        let _ = sent.send(line);
    });
    received
        .recv_timeout(duration)
        .map_err(|_| io::ErrorKind::TimedOut.into())
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
    if mode == "owned-approval-eof-gated" {
        // The peer-gone-before-first-byte case, ordered: the host must have
        // recorded the owner's verdict and be holding the armed frame at its
        // send gate BEFORE this peer leaves, or the verdict races the host's
        // own observation of the exit (a loaded host observed the exit first
        // and refused the verdict with `RunnerAuthority`). So: hold an
        // exclusive lock on the `alive` marker for the life of this process
        // (the OS releases it at exit — the test's proof that the peer is
        // gone before it releases the host), announce the callback is
        // delivered, hold for the test's release, then exit with nothing on
        // the wire. The lock handle is forgotten, never dropped, so the lock
        // outlives every early return and ends with the process itself.
        let alive = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(marker.with_extension("alive"))?;
        alive.lock()?;
        std::mem::forget(alive);
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
    if mode == "owned-approval-resolve-first" {
        // The pre-send resolution case: the resolution is emitted while the
        // host is held at the recheck gate — before the first byte of the
        // frame. The retained ADR-046 rule is the quiet path: the host drops
        // the armed frame without sending (never `Closed`, never a named
        // failure) and the drive completes. No frame may reach the wire.
        gate(marker)?;
        resolved("approval-1")?;
        // Handshake: the resolution bytes are on the wire. The test waits for
        // this marker before releasing the host's gate, so the resolution can
        // never be emitted before the host is in flight nor after its send.
        fs::write(marker.with_extension("approval-resolving"), b"resolving")?;
        // A frame arriving after the resolution is a REAL defect, so the
        // watch is bounded by the budget — never a short literal that can
        // pass vacuously when the frame is merely late.
        if let Ok(extra) = timeout_read(harness_wait() * 2) {
            return Err(io::Error::other(format!(
                "frame after pre-first-byte resolution: {extra:?}"
            )));
        }
        // Return into the parent's terminal turn: turn/completed ends the
        // drive promptly, quietly, with no approval frame on the wire.
        return Ok(true);
    }
    if mode == "owned-approval-turn-untransmitted" {
        // The final verdict.s rule, untransmitted arm: end the turn while
        // the host is held at the recheck gate — in flight, armed, but not
        // one byte on the wire. The host parses this turn end during the
        // hold (the pump polls the wire while the gate future is awaited),
        // so the rule decides with zero accepted bytes and the operation
        // must never complete silently. No frame may reach the wire.
        gate(marker)?;
        note(
            "turn/completed",
            json!({ "threadId": "owned-thread", "turn": { "id": "owned-turn", "status": "completed", "items": [] } }),
        )?;
        announce(marker, "approval-resolving")?;
        return Ok(false);
    }
    if mode == "owned-approval-turn-midwrite" {
        // The final verdict's rule, uncertain arm. No gate: the host's send
        // is free to run. The probe stays silent long enough for the whole
        // frame to be accepted (the write completes into the OS buffer the
        // moment the choice round-trip lands), then ends the turn and exits
        // with the frame UNREAD — no read at all, so no buffered reader can
        // drain the pipe and complete the flush. On Windows the transport's
        // flush (`FlushFileBuffers`) is still pending over the unread bytes
        // when the read handle closes, so the send fails mid-write with
        // bytes accepted — transmitted, receipt-less, uncertain:
        // `SettlementUnknown`, never a silent completion. On POSIX the
        // 51-byte pipe write is atomic and the flush is a no-op, so the
        // write is accepted and recorded before the turn end arrives: the
        // clean control arm, asserted as such by the scenario.
        std::thread::sleep(Duration::from_millis(1200));
        note(
            "turn/completed",
            json!({ "threadId": "owned-thread", "turn": { "id": "owned-turn", "status": "completed", "items": [] } }),
        )?;
        announce(marker, "approval-midwrite-exit")?;
        return Ok(false);
    }
    if mode == "owned-approval-gate-resolve" {
        // Host is held at the recheck gate: in_flight is set and the frame is
        // armed but not yet committed to the OS. Emit the resolution for the
        // in-flight id and say so; the test releases the host only after this
        // marker, so the resolution is parsed before the first byte and the
        // host takes ADR-046's quiet path for a pre-send resolution: the
        // armed frame is dropped, the entry stays in flight, nothing is
        // cancelled and no frame is written. The probe therefore reads no
        // frame here (a read would wait for bytes the rule never sends).
        // It must not end its turn either until the host has left the gate
        // and retired the frame: a turn end parsed during the hold reaches
        // the pump before the quiet arm and the drive ends on it. So the
        // probe holds for the test's continue marker — written once the
        // pump's own `resolved-before-send` mark is observed — and only
        // then returns into the parent's terminal turn.
        announce(marker, "approval-ready")?;
        await_release(marker, "approval-release")?;
        resolved("approval-1")?;
        announce(marker, "approval-resolved")?;
        await_release(marker, "approval-continue")?;
        return Ok(true);
    }
    if mode == "owned-approval-admitted-resolve-count" {
        // Frame 1 is written and read, then the resolution arrives (the legal
        // order). Any further response frame for this id must never arrive.
        let response = read(reader, marker)?;
        append_bytes(marker, &response)?;
        resolved("approval-1")?;
        if let Ok(extra) = timeout_read(Duration::from_millis(300)) {
            return Err(io::Error::other(format!(
                "second frame after resolution: {extra:?}"
            )));
        }
        // Return into the parent's terminal turn: turn/completed ends the
        // drive promptly without another approval frame.
        return Ok(true);
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
