mod common;
use common::*;
use hagency_store::DomainRepository;
use serde_json::json;

fn open_raw(dir: &tempfile::TempDir) -> rusqlite::Connection {
    rusqlite::Connection::open(dir.path().join("state/domain.sqlite3")).unwrap()
}

/// The list read this slice owns (ADR-132): one row per fleet
/// registration — the id IS the server name (ADR-016) — LEFT JOINed to
/// its projects, which `admit` writes. The projection carries exactly
/// `{id, room_id}` per project: `owner_mxid` and the owner's DM room are
/// withheld, so neither MXID nor room id can reach the read.
#[test]
fn project_sides_list_read_joins_registrations_to_projects() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&dir.path().join("state")).unwrap();
    db.register(&registration()).unwrap();
    // No engagement yet: the side exists (the registration row is the
    // side) with an empty project list — a LEFT JOIN, not an inner one.
    let sides = db.project_sides().unwrap();
    assert_eq!(sides.len(), 1);
    assert_eq!(sides[0].id, "example.test", "the id is the server name");
    assert_eq!(sides[0].representative, registration().representative_mxid);
    assert_eq!(sides[0].generation, 1);
    assert_eq!(sides[0].reception_room_id, "!reception:example.test");
    assert!(sides[0].registered, "row and config generations agree");
    assert!(sides[0].projects.is_empty());
    // Admitting an engagement writes the project row the join reads.
    let pool = resource("preset", "seat", 1000);
    db.put_resource(&pool).unwrap();
    db.admit(
        &proof(&request("side_request", "SideWorker", &pool, 100)),
        1000,
    )
    .unwrap();
    let sides = db.project_sides().unwrap();
    assert_eq!(sides.len(), 1);
    assert_eq!(sides[0].projects.len(), 1);
    let project = &sides[0].projects[0];
    assert_eq!(project.id, "project_one");
    assert_eq!(project.room_id, "!project:example.test");
    let text = serde_json::to_string(&sides).unwrap();
    assert!(!text.contains("@owner:example.test"), "owner mxid withheld");
    assert!(!text.contains("!private:example.test"), "owner DM withheld");
}

/// The credential forward guard (ADR-132): a registration config seeded
/// with token-shaped values — the only place native could ever grow one —
/// cannot reach the projection in any byte, because the read extracts the
/// named paths with `json_extract`, never a parse-and-strip of the whole
/// config. A corrupt (non-integer) config generation must surface as
/// `Error::Schema`, never as a silent value.
#[test]
fn project_sides_projection_omits_credential_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&dir.path().join("state")).unwrap();
    db.register(&registration()).unwrap();
    let pool = resource("preset", "seat", 1000);
    db.put_resource(&pool).unwrap();
    db.admit(
        &proof(&request("side_request", "SideWorker", &pool, 100)),
        1000,
    )
    .unwrap();
    let as_token = "as_token_a3f9c2e18d7b4605";
    let hs_token = "hs_token_5d1e9f02b7a8c3416";
    let raw = open_raw(&dir);
    raw.execute(
        "UPDATE registrations SET config=json_set(config,'$.as_token',?1,'$.hs_token',?2)",
        rusqlite::params![as_token, hs_token],
    )
    .unwrap();
    drop(raw);
    let text = serde_json::to_string(&db.project_sides().unwrap()).unwrap();
    assert!(!text.contains(as_token), "no as_token value in any byte");
    assert!(!text.contains(hs_token), "no hs_token value in any byte");
    assert!(!text.contains("as_token") && !text.contains("hs_token"));
    assert!(!text.contains("asToken") && !text.contains("hsToken"));
    // The forward guard is not a safety proof: a corrupt generation path
    // fails the read (Error::Schema) rather than inventing a figure.
    let raw = open_raw(&dir);
    raw.execute(
        "UPDATE registrations SET config=json_set(config,'$.generation','corrupt')",
        [],
    )
    .unwrap();
    drop(raw);
    assert!(matches!(
        db.project_sides(),
        Err(hagency_store::Error::Schema)
    ));
}

/// The bounded read: `registered` is COMPUTED against the config's own
/// generation field (drift is visible, not assumed away), the project
/// list is capped at 64 per side, and the fleet cap mirrors the 1024-row
/// bound `register()` enforces.
#[test]
fn project_sides_registered_is_computed_and_projects_are_capped() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&dir.path().join("state")).unwrap();
    db.register(&registration()).unwrap();
    let raw = open_raw(&dir);
    raw.execute(
        "UPDATE registrations SET config=json_set(config,'$.generation',7)",
        [],
    )
    .unwrap();
    for index in 0..70 {
        raw.execute(
            "INSERT INTO projects(fleet_id,id,generation,room_id,owner_mxid,owner_room_id) \
             VALUES(?1,?2,1,?3,'@owner:example.test','!private:example.test')",
            rusqlite::params![
                registration().fleet_id,
                format!("p{index:02}"),
                format!("!p{index:02}:example.test")
            ],
        )
        .unwrap();
    }
    drop(raw);
    let sides = db.project_sides().unwrap();
    assert_eq!(sides.len(), 1);
    assert!(!sides[0].registered, "row generation 1, config claims 7");
    assert_eq!(sides[0].generation, 1, "the row's column, not the config's");
    assert_eq!(sides[0].projects.len(), 64, "the per-side project cap");
    assert_eq!(sides[0].projects[0].id, "p00");
    assert_eq!(sides[0].projects[63].id, "p63");
    assert_eq!(
        serde_json::to_value(&sides[0]).unwrap(),
        json!({
            "id": "example.test",
            "representative": registration().representative_mxid,
            "generation": 1,
            "reception_room_id": "!reception:example.test",
            "registered": false,
            "projects": (0..64)
                .map(|i| json!({"id": format!("p{i:02}"), "room_id": format!("!p{i:02}:example.test")}))
                .collect::<Vec<_>>()
        }),
        "exactly the six keys; every project is exactly id and room_id"
    );
}
