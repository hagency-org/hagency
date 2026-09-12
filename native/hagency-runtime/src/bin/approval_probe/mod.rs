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
    let until = Instant::now() + Duration::from_secs(6);
    while !marker.with_extension("approval-release").is_file() {
        if Instant::now() >= until {
            return Err(io::ErrorKind::TimedOut.into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
pub(super) fn run(mode: &str, reader: &mut impl BufRead, marker: &Path) -> io::Result<bool> {
    callback("approval-1")?;
    if mode == "owned-approval-eof" {
        return Ok(false);
    }
    if mode == "owned-approval-resolve" {
        gate(marker)?;
        resolved("approval-1")?;
        pulse(marker)?;
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
