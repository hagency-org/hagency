//! Approval wire + client-origin oracle pins (brief 29, PC-C4 lane).
//!
//! `native/scripts/approval-vectors.mjs` EXECUTES the retained sources
//! (`bridge-matrix.js`, `mockup/app/api/hagency/[...path]/route.js`) by
//! slicing them at asserted line anchors and writes
//! `tests/fixtures/approval-vectors.json` carrying the sha256 of every
//! retained source it executed. `native_approval_oracle_pins_drift` re-checks
//! those pins from the Rust side: a drifted retained source must fail the
//! oracle naming the file, never silently re-bless the native rules.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/approval-vectors.json"
    );
    serde_json::from_str(std::fs::read_to_string(path).unwrap().as_str()).unwrap()
}

fn sha256(path: &PathBuf) -> String {
    Sha256::digest(std::fs::read(path).unwrap_or_default())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The oracle pins: every retained source the vectors executed is hash-pinned
/// in the fixture. When either source drifts without regenerating the fixture
/// this fails naming the drifted file, so no native rule is silently
/// re-blessed against the new source.
#[test]
fn native_approval_oracle_pins_drift() {
    let fixture = fixture();
    assert_eq!(
        fixture["source"].as_str().unwrap(),
        "bridge-matrix.js + mockup/app/api/hagency/[...path]/route.js"
    );
    for (file_key, hash_key) in [("bridgeFile", "bridgeSha256"), ("routeFile", "routeSha256")] {
        let file = fixture[file_key].as_str().unwrap();
        let pinned = fixture[hash_key].as_str().unwrap();
        let actual = sha256(&root().join(file));
        assert_eq!(
            pinned, actual,
            "retained source {file} drifted from the checked-in oracle vectors; \
             regenerate with `node native/scripts/approval-vectors.mjs` instead of \
             re-blessing the native rules"
        );
    }
    // The corpus the other selector replays is the regenerated one.
    assert_eq!(fixture["requests"].as_array().unwrap().len(), 5);
    assert_eq!(fixture["notices"].as_array().unwrap().len(), 2);
    assert_eq!(fixture["verdicts"].as_array().unwrap().len(), 16);
    assert_eq!(fixture["origin"].as_array().unwrap().len(), 6);
}
