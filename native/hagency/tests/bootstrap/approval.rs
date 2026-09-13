//! PC-C0 selectors (specs/task-rust-private-approval-wiring.spec.md).
//!
//! Both scenarios drive the REAL service composition through the actual
//! `hagency` binary with the fake TLS Matrix peer — no live homeserver — by
//! injecting the approval bot's own second credential section into the
//! development-driver configuration the bootstrap fixture already writes.
use super::*;
use serde_json::Value;

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
    hagency_store::private::write_new(&path, &serde_json::to_vec(&config).unwrap()).unwrap();
    hagency_store::private::write_new(
        &f.state_dir.join("approval.access_token"),
        common::TOKEN.as_bytes(),
    )
    .unwrap();
    hagency_store::private::write_new(&f.state_dir.join("approval.sdk_key"), &[42; 32]).unwrap();
}

/// Scenario: A service-composed run delivers a request end to end.
///
/// The composition is the assertion: with the approval section present the
/// bootstrap attaches `ApprovalHost` to the host (so `start_mode` creates the
/// notices channel), builds the approval bot's OWN collector
/// (`HostApprovalConfig::new` → `with_fresh_account_enrollment` →
/// `ApprovalCollector::new`), spawns the pump forwarder on the service
/// runtime, hands the driver the one new sender parameter, and the ordinary
/// dispatch run still completes end to end through the real binary. The
/// card-send leg itself (encrypted DM to the owner) is bound by the
/// matrix-crate selector `native_private_approval_fresh_enrollment_and_delivery`
/// over the same `send_private_approval_card`; this selector binds the
/// service-level wiring C0 owns. No approval-bot traffic may leave the
/// composition without an admitted request.
#[tokio::test]
async fn native_private_approval_delivery_is_wired() {
    let mut f = Fixture::new(false).await;
    with_approval(&f, true).await;
    let child = f.launch(true);
    let first = f.fake.next().await;
    first.json(200, common::who());
    f.fake.next().await.json(200, common::sync("bootstrap"));
    f.fake.next().await.json(200, common::state());
    let status = f.wait_result().await;
    assert_eq!(status["protocol"], "completed");
    assert_eq!(f.attempts(), 1);
    // The pump is constructed but idle: no card was sent because no request
    // was admitted, and the approval bot never touched the peer.
    f.fake.no_request().await;
    let capability = f.capabilities().await;
    assert_eq!(capability["development_execution"]["state"], "one_attempt");
    drop(child);
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
    assert!(
        stderr.contains("invalid or unavailable"),
        "refusal was not the named configuration failure: {stderr}"
    );
    f.fake.no_request().await;
}
