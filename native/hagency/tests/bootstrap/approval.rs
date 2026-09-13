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
