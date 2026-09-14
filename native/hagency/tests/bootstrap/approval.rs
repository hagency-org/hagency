//! PC-C0 selectors (specs/task-rust-private-approval-wiring.spec.md).
//!
//! Both scenarios drive the REAL service composition through the actual
//! `hagency` binary with the fake TLS Matrix peer — no live homeserver — by
//! injecting the approval bot's own second credential section into the
//! development-driver configuration the bootstrap fixture already writes.
use super::*;
use serde_json::Value;
use std::io::{Seek, Write};

/// Inject the approval bot's own credential set (config.rs `approval`): a
/// SECOND identity/token/device/SDK root and a DM room, never the pooled
/// ordinary `HostConfig`. `anchors` empty models the absent enrollment.
async fn with_approval(f: &Fixture, anchors: bool) {
    let path = f.state_dir.join("development-driver.json");
    let mut config: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    config["approval"] = json!({
        "origin": f.fake.endpoint,
        "server_name": "example.test",
        "registration_fingerprint": "a".repeat(64),
        "engagement_id": config["matrix"]["engagement_id"],
        "registration_generation": 1,
        "transport_generation": 1,
        "sender_mxid": "@approval:example.test",
        "device_id": "APPROVAL_DEVICE",
        "rooms": [{"id": "!private:example.test", "generation": 1,
                   "privacy": {"kind": "direct", "human_mxid": "@owner:example.test"}}],
        "peer_masters": if anchors {
            json!([{"user_id": "@owner:example.test",
                    // base64(0x41 * 32): a valid, roundtripping Ed25519 public key.
                    "master_key": "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE="}])
        } else {
            json!([])
        }
    });
    // The fixture's own `write_new` created this file already; rewrite it in
    // place (the `scope.rs` shape) instead of re-creating it, and let the
    // write itself fail loudly rather than swallowing `AlreadyExists`.
    let mut file = hagency_store::private::open(&path, false).unwrap();
    file.set_len(0).unwrap();
    file.rewind().unwrap();
    file.write_all(&serde_json::to_vec(&config).unwrap())
        .unwrap();
    drop(file);
    hagency_store::private::write_new(
        &f.state_dir.join("approval.access_token"),
        common::TOKEN.as_bytes(),
    )
    .unwrap();
    hagency_store::private::write_new(&f.state_dir.join("approval.sdk_key"), &[42; 32]).unwrap();
}

/// Scenario: A service-composed run delivers a request end to end.
///
/// PC-C0b: the delivery leg is observable through the test-only fixture.
/// The callback-capable probe — pinned by this test's OWN executable path
/// and sha, never a production switch — raises one real
/// `item/commandExecution/requestApproval` during the turn. The
/// composition's pump (PC-C0's wiring, untouched) takes the
/// single-consumer receiver, re-reads the card from the admitted domain
/// request and drives `send_private_approval_card` against the shared fake
/// peer. The scripted second-identity enrollment (test-side, pre-launch:
/// the operator's own host act under D-ADR114 observe) leaves the bot's
/// SDK root enrolled so the composition's lazy collector opens it at send
/// time. No production path is altered.
#[tokio::test]
async fn native_private_approval_delivery_is_wired() {
    use sha2::{Digest, Sha256};
    let mut f = Fixture::new(false).await;
    // The composition's engagement (the driver config's matrix section).
    let driver: Value = serde_json::from_slice(
        &std::fs::read(f.state_dir.join("development-driver.json")).unwrap(),
    )
    .unwrap();
    let engagement = driver["matrix"]["engagement_id"]
        .as_str()
        .unwrap()
        .to_owned();
    // The scripted host act: enroll the approval bot's second identity
    // against the shared fake peer, into the very state the composition
    // will run on. The anchor it returns is the fresh-account enrollment
    // key the driver config must carry.
    let (anchor, mut peer) = support::enroll(&f.state_dir, &mut f.fake, &engagement).await;
    // The approval section plus the probe pin: the executable and its sha
    // are the test's own choice — the composition's configured executable
    // is whatever the pinned path says (config.rs never changes).
    let probe = std::path::PathBuf::from(env!("CARGO_BIN_EXE_hagency-approval-mcp-probe"))
        .canonicalize()
        .unwrap();
    let probe_sha256: String = Sha256::digest(std::fs::read(&probe).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let path = f.state_dir.join("development-driver.json");
    let mut config: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    config["executable"] = json!(probe);
    config["executable_sha256"] = json!(probe_sha256);
    config["approval"] = json!({
        "origin": f.fake.endpoint,
        "server_name": "example.test",
        "registration_fingerprint": "a".repeat(64),
        "engagement_id": engagement,
        "registration_generation": 1,
        "transport_generation": 1,
        "sender_mxid": support::BOT,
        "device_id": support::DEVICE,
        "rooms": [{"id": support::ROOM, "generation": 2,
                   "privacy": {"kind": "direct", "human_mxid": "@owner:example.test"}}],
        "peer_masters": [{"user_id": "@owner:example.test", "master_key": anchor}]
    });
    {
        let mut file = hagency_store::private::open(&path, false).unwrap();
        file.set_len(0).unwrap();
        file.rewind().unwrap();
        file.write_all(&serde_json::to_vec(&config).unwrap())
            .unwrap();
    }
    hagency_store::private::write_new(
        &f.state_dir.join("approval.access_token"),
        support::APPROVAL_TOKEN.as_bytes(),
    )
    .unwrap();
    hagency_store::private::write_new(&f.state_dir.join("approval.sdk_key"), &[42; 32]).unwrap();

    let _child = f.launch(true);
    // Route the two interleaved legs of the shared fake peer until the run
    // reports its result: the ordinary worker leg (who/sync/state on the
    // worker token) and the approval leg (room refresh, key protocol,
    // secret share and the encrypted card itself on the bot's own token).
    // The two borrows (fake mutable, capabilities immutable) alternate
    // rather than share one select — requests buffer in the peer's channel
    // during the brief status polls.
    let status = loop {
        let request = tokio::select! {
            request = f.fake.next() => Some(request),
            _ = tokio::time::sleep(std::time::Duration::from_millis(25)) => None,
        };
        if let Some(request) = request {
            if support::is_approval(&request) {
                support::respond(request, &mut peer).await;
            } else if request.target.ends_with("/whoami") {
                request.json(200, common::who());
            } else if request.target.contains("/sync?") {
                request.json(200, common::sync("bootstrap"));
            } else if request.target.ends_with("/state") {
                request.json(200, common::state());
            } else {
                panic!("unexpected ordinary-leg request: {}", request.target);
            }
            continue;
        }
        let status = f.capabilities().await["development_execution"].clone();
        if matches!(
            status["state"].as_str(),
            Some("completed" | "unavailable" | "outcome_unknown" | "no_work")
        ) {
            break status;
        }
    };
    // The pump's send is still in flight when the run settles — the probe
    // holds the turn open with no terminal event, so the transport timeout
    // settles outcome_unknown BEFORE the card's HTTP round trip completes.
    // Keep servicing both legs until the event lands, bounded by the
    // fixture's own STARTUP_WATCHDOG (the same bound capabilities() polls
    // under — a fixed 10 s here raced a loaded host's round trip and the
    // card arrived after the window, leaving peer.events empty).
    let drain = tokio::time::Instant::now() + STARTUP_WATCHDOG;
    while peer.events.is_empty() && tokio::time::Instant::now() < drain {
        let request = tokio::select! {
            request = f.fake.next() => Some(request),
            _ = tokio::time::sleep(std::time::Duration::from_millis(25)) => None,
        };
        if let Some(request) = request {
            if support::is_approval(&request) {
                support::respond(request, &mut peer).await;
            } else if request.target.ends_with("/whoami") {
                request.json(200, common::who());
            } else if request.target.contains("/sync?") {
                request.json(200, common::sync("bootstrap"));
            } else if request.target.ends_with("/state") {
                request.json(200, common::state());
            } else {
                panic!("unexpected ordinary-leg request: {}", request.target);
            }
        }
    }
    // Named failure: the card must be observed before the fixture watchdog
    // expires. Dump the child's stderr so a timeout names the pump's own
    // send outcome (a denied-at-deadline card is a different root cause
    // than a merely slow one).
    assert!(
        !peer.events.is_empty(),
        "approval card not delivered within the fixture watchdog\n--- native.stderr ---\n{}",
        String::from_utf8_lossy(
            &std::fs::read(f.root.path().join("native.stderr")).unwrap_or_default()
        )
    );
    // The scenario observes the DELIVERY, never the verdict: the probe
    // raises the requestApproval and then holds the turn open with no
    // terminal event (the owned probe's own discipline — "Done
    // intentionally has no turn terminal event"), so the designed end is
    // the transport timeout settling outcome_unknown, never protocol
    // completion. What must hold: one attempt, the card delivered, and the
    // pump gone when the worker dropped the sender at run end.
    assert_eq!(
        status["state"],
        "outcome_unknown",
        "the wired run: {status}\n--- native.stderr ---\n{}\n--- probe requests (exists={}) ---\n{}\n--- store ---\n{}",
        String::from_utf8_lossy(
            &std::fs::read(f.root.path().join("native.stderr")).unwrap_or_default()
        ),
        f.work.join("approval-mcp.requests").exists(),
        String::from_utf8_lossy(
            &std::fs::read(f.work.join("approval-mcp.requests")).unwrap_or_default()
        ),
        {
            let sql = rusqlite::Connection::open(f.state_dir.join("domain.sqlite3")).unwrap();
            let dispatch: String = sql.query_row("SELECT state||' lease='||COALESCE(lease_until,0)||' cap='||COALESCE(capability_until,0)||' fence='||fence FROM runner_dispatches WHERE id='dispatch'", [], |r| r.get(0)).unwrap();
            let task: String = sql
                .query_row(
                    "SELECT config FROM canonical_tasks WHERE id='task'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            let approvals: i64 = sql
                .query_row("SELECT COUNT(*) FROM owner_approvals", [], |r| r.get(0))
                .unwrap();
            let contexts: i64 = sql
                .query_row("SELECT COUNT(*) FROM approval_contexts", [], |r| r.get(0))
                .unwrap();
            format!(
                "dispatch: {dispatch}\ntask: {task}\nowner_approvals: {approvals}\napproval_contexts: {contexts}"
            )
        }
    );
    assert_eq!(f.attempts(), 1);
    // The delivery observation: the approval leg decrypted exactly one
    // room event — the card built from the admitted domain request — and
    // the send completed under the cancellation token (an errored or
    // cancelled send would have left no decrypted event and the pump's
    // warning would name it).
    assert_eq!(peer.events.len(), 1, "the card was delivered");
    assert!(peer.events[0]["content"].is_object());
    assert_eq!(peer.shares, 1, "the room key was shared exactly once");
    // The callback-capable probe is what ran (its own request log), and it
    // was driven by the ordinary dispatch — one attempt, no retries.
    assert!(f.work.join("approval-mcp.requests").exists());
    // Process cleanup only: `Running`'s Drop kills and reaps the child so
    // the fixture never leaks a zombie. It deliberately proves nothing
    // about the pump's self-termination — the pump task ends its
    // `while let` loop silently (no terminal log line) and its handle is
    // aborted, not joined, at shutdown, so the receiver's `None` is not
    // observable from outside the child; the delivery evidence above is
    // what this slice observes.
    drop(_child);
}

/// Scenario: The pump refuses without a fresh approval enrollment.
///
/// The same fixture with the enrollment anchors absent: the composition
/// refuses at startup with the named configuration failure — the pump is
/// never constructed, no collector is built from a config the ordinary
/// `Collector::new` would refuse, and no card is sent (the peer stays idle
/// because the process exits before any Matrix work).
#[tokio::test]
async fn native_private_approval_delivery_wiring_refuses_without_enrollment() {
    let mut f = Fixture::new(false).await;
    with_approval(&f, false).await;
    // The fixture's command() nulls stderr; re-capture it for the assertion.
    let mut command = f.command(true);
    command.stderr(std::process::Stdio::piped());
    let output = command.output().unwrap();
    assert!(
        !output.status.success(),
        "the composition accepted an approval section without enrollment anchors"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The named refusal (r1 item 4), not the shared `invalid or unavailable`
    // config label every `Failure::Config` emits: the enrollment leg of the
    // approval construction is what this scenario refuses.
    assert!(
        stderr.contains("approval enrollment refused"),
        "refusal was not the named enrollment failure: {stderr}"
    );
    f.fake.no_request().await;
}
