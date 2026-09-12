mod common;

#[test]
fn native_owner_approval_recovery_crlf_fixture() {
    let schemas = [
        include_str!("../src/domain.sql"),
        include_str!("../src/migrations/002-role-publication.sql"),
        include_str!("../src/migrations/003-task-dispatch.sql"),
        include_str!("../src/migrations/004-message-inputs.sql"),
        include_str!("../src/migrations/005-task-intents.sql"),
        include_str!("../src/migrations/006-internal-conversations.sql"),
        include_str!("../src/migrations/007-peer-inputs.sql"),
        include_str!("../src/migrations/008-recovery-reports.sql"),
        include_str!("../src/migrations/009-conversation-lifecycle.sql"),
        include_str!("../src/migrations/010-task-graphs.sql"),
        include_str!("../src/migrations/011-final-replies.sql"),
    ];
    for crlf in [false, true] {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        for schema in schemas {
            db.execute_batch(schema).unwrap();
        }
        let source = schemas.last().unwrap().replace("\r\n", "\n");
        let source = if crlf {
            source.replace('\n', "\r\n")
        } else {
            source
        };
        let view = common::reply_route_view(&source);
        assert!(view.trim_end().ends_with(';'));
        assert!(!view.contains("CREATE TABLE final_replies"));
        db.execute_batch("DROP VIEW current_matrix_routes;")
            .unwrap();
        db.execute_batch(view).unwrap();
        db.prepare("SELECT session_id FROM current_matrix_routes LIMIT 0")
            .unwrap();
    }
}

#[test]
fn native_account_schema22() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut domain = hagency_store::DomainRepository::open(&state).unwrap();
    let resource = common::resource("schema22-resource", "schema22-seat", 1000);
    domain.put_resource(&resource).unwrap();
    domain.put_seat(&serde_json::from_value(serde_json::json!({"id":"schema22-seat","declaration":{"quotaTokens":5000,"period":"monthly"}})).unwrap()).unwrap();
    domain.register(&common::registration()).unwrap();
    let proof = common::proof(&common::request(
        "schema22-request",
        "Worker",
        &resource,
        100,
    ));
    domain.admit(&proof, 1000).unwrap();
    domain.approve("schema22-approval", &proof, 1000).unwrap();
    drop(domain);
    let sql = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    sql.execute_batch("DROP TABLE resource_accounts; DROP TABLE managed_accounts; DROP TABLE account_identity_key; PRAGMA user_version=22;").unwrap();
    let snapshot = |sql: &rusqlite::Connection| -> Vec<(String, String, String)> {
        sql.prepare("SELECT 'resources',id,config FROM resources UNION ALL SELECT 'seats',id,config FROM seats UNION ALL SELECT 'engagements',id,context FROM engagements ORDER BY 1,2").unwrap().query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap().collect::<Result<_,_>>().unwrap()
    };
    let before = snapshot(&sql);
    drop(sql);
    let domain = hagency_store::DomainRepository::open(&state).unwrap();
    assert!(domain.account_choices().unwrap().is_empty());
    drop(domain);
    let sql = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    assert_eq!(
        sql.query_row("PRAGMA user_version", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        23
    );
    assert_eq!(before, snapshot(&sql));
    drop(sql);
    drop(hagency_store::DomainRepository::open(&state).unwrap());
}
