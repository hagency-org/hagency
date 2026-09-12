mod common;
use hagency_store::*;
use std::time::{Duration, Instant};
fn prepared(db: &mut DomainRepository) -> AccountChoice {
    let original = db.reserve_account(ACCOUNT_PROFILE).unwrap();
    db.materialize_account(&original.id).unwrap()
}
fn enroll(db: &mut DomainRepository, choice: &AccountChoice) -> hagency_core::project::Resource {
    let account = db.managed_account(&choice.id).unwrap();
    let access =
        AccountEnrollmentAccess::new(Instant::now() + Duration::from_secs(30), Default::default());
    let command = access
        .prepare(
            &account,
            choice.revision.clone(),
            "gpt-5.6-sol".into(),
            Some("medium".into()),
            None,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let result = db.enroll_account_resource(command).unwrap();
    db.resource_configuration(&result.resource_id).unwrap()
}
#[test]
fn native_account_preparation() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut db = DomainRepository::open(&state).unwrap();
    assert!(db.reserve_account("subscription-only").is_err());
    let first = db.reserve_account(ACCOUNT_PROFILE).unwrap();
    let collision = state.join(&first.id);
    private::create_directory_new(&collision).unwrap();
    std::fs::write(collision.join("original"), b"retain original").unwrap();
    assert!(db.materialize_account(&first.id).is_err());
    assert!(db.materialize_account(&first.id).is_err());
    assert_eq!(
        std::fs::read(collision.join("original")).unwrap(),
        b"retain original"
    );
    assert!(matches!(
        db.account_choices().unwrap()[0].state,
        AccountState::Uncertain
    ));
    let lost = db.reserve_account(ACCOUNT_PROFILE).unwrap();
    let actual = prepared(&mut db);
    assert!(matches!(actual.state, AccountState::Active));
    assert_eq!(actual.authentication, "unknown");
    assert_eq!(actual.quota, None);
    let handle = db.managed_account(&actual.id).unwrap();
    assert!(matches!(DomainRepository::open(&state), Err(Error::Locked)));
    drop(db);
    let mut db = DomainRepository::open(&state).unwrap();
    assert!(db.materialize_account(&lost.id).is_err());
    assert!(matches!(
        db.account_choices().unwrap()[1].state,
        AccountState::Uncertain
    ));
    let access =
        AccountEnrollmentAccess::new(Instant::now() + Duration::from_secs(30), Default::default());
    let command = access
        .prepare(
            &handle,
            actual.revision,
            "gpt-5.6-sol".into(),
            Some("medium".into()),
            None,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    assert!(matches!(
        db.enroll_account_resource(command),
        Err(Error::LocalAuthority)
    ));
    let sql = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    sql.execute("DELETE FROM account_identity_key", []).unwrap();
    assert!(db.managed_account(&actual.id).is_err());
    drop(db);
    assert!(DomainRepository::open(&state).is_err());
}
#[test]
fn native_account_association() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut db = DomainRepository::open(&state).unwrap();
    let choice = prepared(&mut db);
    let first = enroll(&mut db, &choice);
    let access = ResourceConfigurationAccess::new(
        Instant::now() + Duration::from_secs(30),
        Default::default(),
    );
    let command = access
        .prepare(
            first.id(),
            resource_publication_revision(&first).unwrap(),
            true,
            ProfileChange::Preserve {},
            CeilingChange::Preserve {},
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let result = db.configure_resource(command).unwrap();
    let second = db.resource_configuration(&result.resource_id).unwrap();
    assert_ne!(first.id(), second.id());
    assert_eq!(first.seat_id, second.seat_id);
    let sql = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    let mappings: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM resource_accounts WHERE account_id=?1",
            [&choice.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mappings, 2);
    let seat: String = sql
        .query_row(
            "SELECT config FROM seats WHERE id=?1",
            [&first.seat_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(&seat).unwrap()["declaration"].is_null());
    let mut forged = first.clone();
    forged.preset_id = "forged_preset".into();
    assert!(db.put_resource(&forged).is_err());
    let mut rebound = first.clone();
    rebound.seat_id = "other-seat".into();
    assert!(db.put_resource(&rebound).is_err());
    assert!(
        db.put_seat(&hagency_core::project::Seat {
            id: first.seat_id.clone(),
            declaration: None
        })
        .is_err()
    );
    let command = access
        .prepare(
            second.id(),
            result.revision,
            false,
            ProfileChange::Preserve {},
            CeilingChange::Monthly {
                tokens: 123u64.try_into().unwrap(),
            },
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    db.configure_resource(command).unwrap();
    assert_eq!(
        db.resource_configuration(&second.id()).unwrap().seat_id,
        first.seat_id
    );
    assert!(
        sql.execute("UPDATE resource_accounts SET account_id='forged'", [])
            .is_err()
    );
    let before: usize = sql
        .query_row("SELECT COUNT(*) FROM resources", [], |r| r.get(0))
        .unwrap();
    let account = db.managed_account(&choice.id).unwrap();
    let enroll =
        AccountEnrollmentAccess::new(Instant::now() + Duration::from_secs(30), Default::default());
    let cmd = enroll
        .prepare(
            &account,
            choice.revision,
            "unsupported".into(),
            None,
            None,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    assert!(db.enroll_account_resource(cmd).is_err());
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM resources", [], |r| r
            .get::<_, usize>(0))
            .unwrap(),
        before
    );
}
#[test]
fn native_account_retirement() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut db = DomainRepository::open(&state).unwrap();
    let choice = prepared(&mut db);
    let mut resource = enroll(&mut db, &choice);
    resource.ceiling = Some(
        serde_json::from_value(serde_json::json!({"tokens":1000,"period":"monthly"})).unwrap(),
    );
    db.put_resource(&resource).unwrap();
    db.register(&common::registration()).unwrap();
    let proof = common::proof(&common::request(
        "account-commitment",
        "Worker",
        &resource,
        100,
    ));
    db.admit(&proof, 1000).unwrap();
    db.approve("approval", &proof, 1000).unwrap();
    let sql = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    let before: String = sql
        .query_row("SELECT json_group_array(config) FROM resources", [], |r| {
            r.get(0)
        })
        .unwrap();
    let obligations: Vec<(String, String)> = sql
        .prepare("SELECT id,context FROM engagements")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let account = db.managed_account(&choice.id).unwrap();
    assert!(!db.catalog("", 100).unwrap().is_empty());
    account.retire();
    assert!(db.catalog("", 100).unwrap().is_empty());
    let access = ResourcePublicationAccess::new(
        Instant::now() + Duration::from_secs(30),
        Default::default(),
    );
    let cmd = access
        .prepare(
            resource.id(),
            resource_publication_revision(&resource).unwrap(),
            true,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    assert!(db.publish_resource(cmd).is_err());
    db.retire_account(&choice.id).unwrap();
    assert!(!db.resource_configuration(&resource.id()).unwrap().published);
    assert!(db.managed_account(&choice.id).is_err());
    assert_ne!(
        sql.query_row("SELECT json_group_array(config) FROM resources", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        before
    );
    let after: Vec<(String, String)> = sql
        .prepare("SELECT id,context FROM engagements")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(after, obligations);
    drop(db);
    let db = DomainRepository::open(&state).unwrap();
    assert!(matches!(
        db.account_choices().unwrap()[0].state,
        AccountState::Retired
    ));
}
