use super::fixture::Fixture;
use matrix_sdk_base::store::StateStore;
use matrix_sdk_sqlite::{SqliteStateStore, SqliteStoreConfig};
use matrix_sdk_store_encryption::StoreCipher;
use serde_json::Value;
use std::{fs, time::Duration};

// A fresh inspector reads only the original encrypted journal after every
// original service SDK owner has died. Store open performs SDK SQLite housekeeping;
// this fixture never creates an OlmMachine or sets a proof, key or journal value.
async fn original_complete(f: &Fixture, id: &str) -> Value {
    let root = f.state_dir.join("sdk");
    let journal: Value = tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let journal = runtime.block_on(async move {
            let key = [42; 32];
            let cipher =
                StoreCipher::import_with_key(&key, &fs::read(root.join("journal.key")).unwrap())
                    .unwrap();
            let store = SqliteStateStore::open_with_config(
                &SqliteStoreConfig::new(root)
                    .key(Some(&key))
                    .pool_max_size(2),
            )
            .await
            .unwrap();
            let bytes = store
                .get_custom_value(b"hagency.observer.sync.v1")
                .await
                .unwrap()
                .unwrap();
            assert!(bytes.len() <= 16 * 1024 * 1024);
            let journal: Value = cipher.decrypt_value(&bytes).unwrap();
            store.close().await.unwrap();
            drop(store);
            journal
        });
        // SDK pool close can schedule final Connection drops in the background.
        // Destroy this inspector's runtime before another process opens the SDK.
        drop(runtime);
        journal
    })
    .await
    .unwrap();
    let attempt = &journal["outgoing"];
    assert_eq!(attempt["kind"], "File");
    assert_eq!(attempt["phase"], "Complete");
    assert_eq!(attempt["id"], id);
    assert_eq!(attempt["route"]["room_id"], f.room);
    assert_eq!(attempt["content"], f.peer.events[0]["content"]);
    assert_eq!(attempt["file"]["locator"]["delivery_id"], id);
    let writes = attempt["writes"].as_array().unwrap();
    let index = attempt["index"].as_u64().unwrap() as usize;
    assert_eq!(index + 1, writes.len());
    assert_eq!(writes[index]["room"], true);
    assert_eq!(
        writes[index]["response"]["event_id"],
        "$native_file_accepted"
    );
    assert!(
        !journal["outgoing_receipts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == id)
    );
    attempt.clone()
}

#[tokio::test]
async fn native_file_service_restart() {
    let mut f = Fixture::new(false).await;
    f.sql().execute_batch(
        "CREATE TRIGGER fixture_first_delivered_abort BEFORE UPDATE OF event_state ON file_deliveries
         WHEN NEW.event_state='delivered'
         BEGIN SELECT RAISE(ABORT, 'fixture first domain settlement refused'); END;",
    ).unwrap();
    let child = f.launch(true, "restart.original");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut requests = 0;
    loop {
        if let Ok(request) =
            tokio::time::timeout(Duration::from_millis(50), f.next("restart.original.http")).await
        {
            requests += 1;
            assert!(requests <= 96);
            f.respond(request).await;
            continue;
        }
        let status = f.capabilities().await["development_execution"].clone();
        if matches!(
            status["state"].as_str(),
            Some("outcome_unknown" | "unavailable")
        ) {
            // The real domain writer refuses after this injected transaction
            // failure. A failed MCP read/runtime is expected, never delivery proof.
            assert_eq!(status["settlement"], "negative");
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "injected domain failure was not observed"
        );
    }
    let admission: Value =
        serde_json::from_slice(&fs::read(f.work.join("file-mcp.admission")).unwrap()).unwrap();
    assert_eq!(admission["isError"], false);
    let id = admission["structuredContent"]["delivery_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(f.delivered_count(), 0);
    assert_eq!(f.peer.events.len(), 1);
    assert_eq!(f.peer.writes.len(), 5);
    assert_eq!(f.peer.claims, 1);
    assert_eq!(f.peer.shares, 1);
    assert_eq!(f.attempts(), 1);
    child.stop_and_reap(); // Checked original PID death; no in-process SDK owner survives.
    let original = original_complete(&f, &id).await;
    let before: (String, Option<String>) = f
        .sql()
        .query_row(
            "SELECT event_state, acceptance FROM file_deliveries WHERE id=?1",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(before, ("write_possible".into(), None));
    f.sql()
        .execute_batch("DROP TRIGGER fixture_first_delivered_abort")
        .unwrap();
    // Historical settlement cannot depend on reopening the original source.
    fs::remove_file(f.work.join("sample.bin")).unwrap();
    fs::rename(
        f.root.path().join("native.stderr"),
        f.root.path().join("native-first.stderr"),
    )
    .unwrap();
    let child = f.launch(true, "restart.reopened");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut requests = 0;
    loop {
        if let Ok(request) =
            tokio::time::timeout(Duration::from_millis(50), f.next("restart.reopened.http")).await
        {
            requests += 1;
            assert!(requests <= 24);
            assert_eq!(
                request.method, "GET",
                "historical delivery must not replay writes or key claims"
            );
            // An invalid current token denies new readiness. The same original
            // SDK receipt can still establish historical delivery independently.
            assert_eq!(request.target, "/_matrix/client/v3/account/whoami");
            request.json(
                401,
                serde_json::json!({"errcode":"M_UNKNOWN_TOKEN","error":"fixture token revoked"}),
            );
        }
        if f.delivered_count() == 1 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "fresh native process did not recover first Delivered"
        );
    }
    let acceptance: String = f
        .sql()
        .query_row(
            "SELECT acceptance FROM file_deliveries WHERE id=?1 AND event_state='delivered'",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    let acceptance: Value = serde_json::from_str(&acceptance).unwrap();
    assert_eq!(acceptance["event_id"], "$native_file_accepted");
    assert_eq!(acceptance["transaction_id"], original["transaction_id"]);
    assert_eq!(acceptance["content_digest"], original["domain_digest"]);
    let status = f.wait_result().await;
    assert_eq!(status["state"], "unavailable");
    assert_eq!(status["error"], "refresh");
    assert_eq!(status["workspace_registered"], false);
    let historical = f.historical_file(&id).await;
    assert_eq!(historical["isError"], false);
    assert_eq!(historical["structuredContent"]["delivery_id"], id);
    assert_eq!(historical["structuredContent"]["status"], "delivered");
    assert!(historical["structuredContent"]["error_code"].is_null());
    let foreign = f.historical_file("unrelated_delivery").await;
    assert_eq!(foreign["isError"], true);
    assert_eq!(f.attempts(), 1);
    assert_eq!(f.peer.writes.len(), 5);
    assert_eq!(f.peer.claims, 1);
    assert_eq!(f.peer.shares, 1);
    assert_eq!(f.peer.events.len(), 1);
    child.stop_and_reap();
    f.fake.no_request().await;
    f.fake.close().await;
}
