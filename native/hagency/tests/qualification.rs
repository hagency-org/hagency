//! MA-M8b (ADR-144): the two-agent qualification record gate. This test
//! reads the tracked evidence file and FAILS — never skips — while it is a
//! placeholder, missing, stale, or carries no verdict for any claim (the
//! ADR-140 evidence class: `native/hagency-execution/tests/
//! qualification.rs` is the exemplar). Hosted runners have no real Palpo
//! homeserver and no real owner client, so the workflow skips this ONE
//! selector by name; the operator runs it on a qualified host per the
//! runbook — and until then this refusal IS the record's state.
use serde_json::{Value, json};
use std::path::Path;

const PINNED_NATIVE: &str = "0.1.0";
const EVIDENCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/qualification/two-agent.json");
/// The three claims an in-process fixture cannot honestly prove (ADR-144):
/// foreign homeserver admission, E2EE to a second real device, and the
/// approval round-trip through a real owner client.
const CLAIMS: [&str; 3] = [
    "homeserver_admission",
    "e2ee_second_device",
    "approval_round_trip",
];

fn load_evidence() -> Value {
    let path = Path::new(EVIDENCE);
    let text = std::fs::read_to_string(path).unwrap_or_else(|error| {
        panic!(
            "two-agent qualification evidence file is missing at {EVIDENCE} \
             (refusal: unreadable, {error}); an operator must follow \
             docs/design/two-agent-qualification-runbook.md against a real Palpo \
             homeserver and a real owner client, then commit the rewritten record \
             in the same commit"
        )
    });
    serde_json::from_str(&text).unwrap_or_else(|error| {
        panic!("two-agent qualification evidence is not valid JSON: {error}")
    })
}

fn refuse_placeholder(evidence: &Value) {
    assert_eq!(
        evidence["schema"], "hagency-two-agent-qualification-v1",
        "qualification evidence schema is not the recorded shape"
    );
    if evidence["placeholder"] == json!(true) {
        panic!(
            "two-agent qualification evidence is a checked-in placeholder \
             (refusal: no operator qualification run recorded); reason recorded in \
             the file: {}",
            evidence["placeholder_reason"].as_str().unwrap_or("unnamed")
        )
    }
}

/// The homeserver identity is recorded as a DIGEST, never a name: no
/// homeserver hostname or room alias survives into the tracked record.
fn assert_digest(value: &Value, what: &str) {
    let text = value.as_str().unwrap_or_default();
    assert_eq!(text.len(), 64, "{what} is not a sha256 hex digest");
    assert!(
        text.chars().all(|c| c.is_ascii_hexdigit()),
        "{what} is not hex"
    );
}

#[test]
fn native_two_agent_qualification_records_its_evidence() {
    let evidence = load_evidence();
    refuse_placeholder(&evidence);
    // The pinned artifact and version: the native binary the operator ran.
    let pin = evidence["pinned_native_version"]
        .as_str()
        .unwrap_or_default();
    assert_eq!(
        pin, PINNED_NATIVE,
        "evidence pin moved without re-qualification"
    );
    let native_version = evidence["native_version"].as_str().unwrap_or_default();
    assert!(
        native_version.contains(PINNED_NATIVE),
        "qualified native version {native_version:?} does not match the pin {PINNED_NATIVE:?}"
    );
    let commit = evidence["commit"].as_str().unwrap_or_default();
    assert_eq!(
        commit.len(),
        40,
        "evidence records no full commit hash; freshness is unprovable"
    );
    assert!(
        commit.chars().all(|c| c.is_ascii_hexdigit()),
        "evidence commit {commit:?} is not a hex object name"
    );
    let recorded = evidence["recorded_at_ms"].as_u64().unwrap_or_default();
    assert!(recorded > 0, "evidence records no capture timestamp");
    assert!(
        recorded < 4_102_444_800_000,
        "evidence capture timestamp is not a plausible millisecond epoch"
    );
    // The environment: the real homeserver, redacted to a digest.
    let homeserver = &evidence["environment"]["homeserver"];
    assert_digest(&homeserver["identity_digest"], "homeserver identity");
    assert!(
        homeserver["software"]
            .as_str()
            .is_some_and(|v| !v.is_empty()),
        "evidence records no homeserver software"
    );
    // Both engagements, the shared room, and the DM in each direction.
    let engagements = evidence["engagements"]
        .as_array()
        .expect("evidence records no engagements array");
    assert_eq!(
        engagements.len(),
        2,
        "the record must carry both engagements"
    );
    let room = &evidence["rooms"]["shared"];
    assert!(
        room["delivery_room"].as_bool() == Some(true)
            && room["approval_room"].as_bool() == Some(false),
        "the shared room must be recorded as a delivery room, never an approval room"
    );
    assert_digest(&room["id_digest"], "shared room id");
    let directions = evidence["direct_messages"]
        .as_array()
        .expect("evidence records no direct_messages array");
    assert_eq!(
        directions.len(),
        2,
        "the record must carry the DM in BOTH directions"
    );
    for (index, dm) in directions.iter().enumerate() {
        assert_digest(&dm["room_id_digest"], "direct message room id");
        let from = dm["from"].as_str().unwrap_or_default();
        assert!(
            ["owner", "agent"].contains(&from),
            "direct message {index} names no sender role"
        );
        assert!(
            dm["plaintext_verified"].as_bool() == Some(true),
            "direct message {index} was not verified end-to-end as plaintext"
        );
    }
    // The usage rows: the spend attributed to each engagement.
    let usage = evidence["usage"]
        .as_array()
        .expect("evidence records no usage rows");
    assert_eq!(
        usage.len(),
        2,
        "the record must carry usage for both engagements"
    );
    for (index, row) in usage.iter().enumerate() {
        assert!(
            row["engagement_index"].as_u64() == Some(index as u64),
            "usage row {index} is not attributed to engagement {index}"
        );
        assert!(
            row["tokens"].is_object(),
            "usage row {index} records no token counts"
        );
    }
    // One verdict per claim, each explicitly a pass with its evidence named.
    for claim in CLAIMS {
        let verdict = &evidence["verdicts"][claim];
        assert!(
            verdict["pass"] == json!(true),
            "claim {claim} has no passing verdict: {verdict}"
        );
        let detail = verdict["evidence"].as_str().unwrap_or_default();
        assert!(
            !detail.is_empty(),
            "claim {claim} records no evidence pointer"
        );
    }
    // What remains unproven, stated so the M8 exit gate is never read green
    // from this record.
    let unproven = evidence["unproven"]
        .as_array()
        .expect("evidence records no unproven list");
    assert!(
        !unproven.is_empty(),
        "a qualification record must state what it does not prove"
    );
}
