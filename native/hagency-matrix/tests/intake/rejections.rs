use super::*;

async fn stop_sdk(c: Collector) {
    c.inner
        .owner
        .lock()
        .await
        .take()
        .unwrap()
        .close()
        .await
        .unwrap();
    drop(c);
}
async fn ready(private: bool) -> (common::Fixture, common::Fake, Collector) {
    let f = common::Fixture::new();
    let mut fake = common::Fake::start(true).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), 1, private),
        f.store.clone(),
    )
    .unwrap();
    prime(&c, &f, &mut fake, private).await;
    (f, fake, c)
}
fn malformed() -> Vec<Value> {
    let mut values = vec![];
    for kind in [
        "body", "sender", "id", "relation", "mentions", "media", "oversize",
    ] {
        let mut value = event(kind, "Rejected", &["@worker:example.test"], None);
        match kind {
            "body" => {
                value["content"].as_object_mut().unwrap().remove("body");
            }
            "sender" => value["sender"] = json!(123),
            "id" => {
                value.as_object_mut().unwrap().remove("event_id");
            }
            "relation" => {
                value["content"]["m.relates_to"] =
                    json!({"rel_type":"m.replace","event_id":"$root"})
            }
            "mentions" => value["content"]["m.mentions"] = json!({"user_ids":[123]}),
            "media" => {
                value["content"]["msgtype"] = json!("m.file");
                value["content"]["url"] = json!("mxc://example.test/file");
            }
            _ => value["content"]["body"] = json!("x".repeat(32769)),
        }
        values.push(value);
    }
    values
}
#[tokio::test]
async fn native_matrix_rejection_continuation_actual_mixed_batch() {
    let (f, mut fake, c) = ready(false).await;
    let mut values = malformed();
    let rejected = values.len();
    values.push(event(
        "outside",
        "Other thread",
        &["@worker:example.test"],
        Some("$unselected"),
    ));
    values.push(event(
        "good",
        "Eligible later in same batch",
        &["@worker:example.test"],
        None,
    ));
    c.inner
        .handoff_fault
        .store(1, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        run(&c, &mut fake, sync("mixed", values), false).await,
        Err(Error::Busy)
    );
    assert!(f.available().await);
    let batch = c
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .batch()
        .await
        .unwrap()
        .unwrap();
    assert!(batch.phase == Phase::Derived);
    assert_eq!(batch.rejected(), rejected);
    assert_eq!(batch.filtered, 1);
    assert_eq!(batch.events.len(), 1);
    assert_eq!(
        rows(&f, "admitted_messages"),
        0,
        "dispositions exist before first eligible domain admission"
    );
    let result = resume(&c, &mut fake, false).await.unwrap();
    assert_eq!(
        (result.admitted, result.rejected, result.filtered),
        (1, rejected, 1)
    );
    assert_eq!(
        run(
            &c,
            &mut fake,
            sync("later", vec![event("later", "New valid event", &[], None)]),
            false
        )
        .await
        .unwrap()
        .admitted,
        1
    );
    assert!(f.available().await);
    assert_eq!(rows(&f, "admitted_messages"), 2);
    assert_eq!(status(&c, &mut fake).await.stage, "idle");
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_rejection_crypto_missing_keys_and_later_verified_message() {
    let (f, mut fake, c) = ready(true).await;
    let mut encrypted = c
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .crypto_messages(true, 2)
        .await;
    let mut no_keys = encrypted.clone();
    no_keys["next_batch"] = json!("missing_keys");
    no_keys["to_device"]["events"] = json!([]);
    let events = no_keys["rooms"]["join"]["!project:example.test"]["timeline"]["events"]
        .as_array_mut()
        .unwrap();
    events.truncate(1);
    let mut plain = event("plain", "Not encrypted", &[], None);
    plain["encryption_info"] = json!({"verification_state":"verified"});
    events.insert(0, plain);
    let result = run(&c, &mut fake, no_keys, true).await.unwrap();
    assert_eq!((result.admitted, result.rejected), (0, 2));
    assert!(f.available().await);
    // Same original ciphertext is now genuinely decryptable through actual key
    // sharing, but its first refusal is terminal. The independently encrypted
    // next message has a fresh ID/index and must still be admitted.
    encrypted["next_batch"] = json!("keys_arrived");
    let result = run(&c, &mut fake, encrypted, true).await.unwrap();
    assert_eq!((result.admitted, result.rejected), (1, 1));
    let inbox = f.store.inbox("root".into(), 0, 10, None).await.unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].message.event_id, "$encrypted_new");
    assert!(inbox[0].wake);
    assert!(f.available().await);
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_rejection_crypto_trust_upgrade_cannot_reinterpret_source() {
    let (f, mut fake, c) = ready(true).await;
    let mut encrypted = c
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .crypto_messages(false, 2)
        .await;
    let mut unverified = encrypted.clone();
    unverified["next_batch"] = json!("unverified");
    unverified["to_device"]["events"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    unverified["rooms"]["join"]["!project:example.test"]["timeline"]["events"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    assert_eq!(
        run(&c, &mut fake, unverified, true).await.unwrap().rejected,
        1
    );
    c.inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .crypto_trust_human()
        .await;
    encrypted["next_batch"] = json!("verified");
    encrypted["to_device"]["events"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    let result = run(&c, &mut fake, encrypted, true).await.unwrap();
    assert_eq!((result.admitted, result.rejected), (1, 1));
    assert!(f.available().await);
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_rejection_replay_restart_changed_plan_and_content() {
    let (f, mut fake, c) = ready(false).await;
    let off = event(
        "unselected",
        "Original non-target",
        &["@worker:example.test"],
        Some("$thread"),
    );
    let bad = malformed().remove(0);
    let no_id = malformed().remove(2);
    let first = run(
        &c,
        &mut fake,
        sync("first", vec![off.clone(), bad.clone(), no_id.clone()]),
        false,
    )
    .await
    .unwrap();
    assert_eq!((first.filtered, first.rejected), (1, 2));
    stop_sdk(c).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), 1, false),
        f.store.clone(),
    )
    .unwrap();
    f.store
        .resolve_verified_matrix_session(SessionBinding {
            id: "thread".into(),
            engagement_id: f.identity.transport.engagement_id.clone(),
            room_id: "!project:example.test".into(),
            thread_root: Some("$thread".into()),
        })
        .await
        .unwrap();
    let mut changed = off.clone();
    changed["content"]["body"] = json!("Replacement under same event ID");
    let mut aged = off.clone();
    aged["unsigned"] = json!({"age":1234});
    for (index, event) in [aged, changed, off].into_iter().enumerate() {
        let cancel = CancellationToken::new();
        let (result, ()) = common::scripted(
            c.intake(HostIntakePlan::new(vec!["thread".into()]).unwrap(), &cancel),
            async {
                fake.next().await.json(200, common::who());
                fake.next()
                    .await
                    .json(200, sync(&format!("replay{index}"), vec![event]));
                fake.next().await.json(200, state(false));
            },
        )
        .await;
        let result = result.unwrap();
        assert_eq!(result.admitted, 0);
        assert_eq!(
            (result.filtered, result.rejected),
            if index == 1 { (0, 1) } else { (1, 0) }
        );
    }
    let result = run(&c, &mut fake, sync("bad_again", vec![bad, no_id]), false)
        .await
        .unwrap();
    assert_eq!((result.admitted, result.rejected), (0, 2));
    assert!(f.available().await);
    assert_eq!(rows(&f, "admitted_messages"), 0);
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_rejection_custody_actual_receipt_rollback_and_sdk_uncertainty() {
    let (f, fake, c) = ready(false).await;
    let targets = vec![f.store.matrix_intake_route("root".into()).await.unwrap()];
    let guard = c.inner.owner.lock().await;
    let owner = guard.as_ref().unwrap();
    owner
        .intake_start(sync("rejected", malformed()), targets.clone())
        .await
        .unwrap();
    let batch = owner.batch().await.unwrap().unwrap();
    assert_eq!(batch.rejected(), 7);
    let sql =
        rusqlite::Connection::open(f.root.path().join("sdk/matrix-sdk-state.sqlite3")).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_receipt BEFORE INSERT ON kv_blob BEGIN SELECT RAISE(ABORT,'controlled rollback'); END;").unwrap();
    assert_eq!(
        owner.intake_finish(batch.digest.clone()).await,
        Err(Error::OutcomeUnknown)
    );
    assert_eq!(owner.batch().await.unwrap().unwrap().rejected(), 7);
    assert_eq!(owner.cursor().await.unwrap().as_deref(), Some("bootstrap"));
    sql.execute_batch("DROP TRIGGER reject_receipt").unwrap();
    drop(sql);
    owner.intake_finish(batch.digest).await.unwrap();
    owner.apply_fault().await;
    assert_eq!(
        owner
            .intake_start(
                sync(
                    "unknown",
                    vec![event("valid", "Unobserved derive", &[], None)]
                ),
                targets
            )
            .await,
        Err(Error::OutcomeUnknown)
    );
    assert!(owner.batch().await.unwrap().unwrap().phase == Phase::Applying);
    assert_eq!(rows(&f, "admitted_messages"), 0);
    drop(guard);
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_rejection_custody_missing_sdk_coverage_and_identity_failure() {
    let (f, mut fake, c) = ready(false).await;
    let targets = vec![f.store.matrix_intake_route("root".into()).await.unwrap()];
    let mut batch = Batch::new(
        sync("coverage", malformed()),
        targets,
        "synthetic identity".into(),
    )
    .unwrap();
    assert_eq!(batch.derive(Default::default(), &[]), Err(Error::Conflict));
    assert!(batch.phase == Phase::Prepared);
    assert_eq!(batch.rejected(), 0);
    let cancel = CancellationToken::new();
    let (result, ()) = common::scripted(c.intake(plan(), &cancel), async {
        fake.next().await.json(
            200,
            json!({"user_id":"@other:example.test","device_id":"DEVICE_1"}),
        );
    })
    .await;
    assert_eq!(result, Err(Error::Identity));
    assert!(!f.available().await);
    assert_eq!(rows(&f, "admitted_messages"), 0);
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

async fn alter_journal(f: &common::Fixture, alter: impl FnOnce(&mut Value)) {
    use matrix_sdk_base::StateStore;
    use matrix_sdk_sqlite::{SqliteStateStore, SqliteStoreConfig};
    use matrix_sdk_store_encryption::StoreCipher;
    let root = f.root.path().join("sdk");
    let key = [42u8; 32];
    let cipher =
        StoreCipher::import_with_key(&key, &std::fs::read(root.join("journal.key")).unwrap())
            .unwrap();
    let store = SqliteStateStore::open_with_config(
        &SqliteStoreConfig::new(&root)
            .key(Some(&key))
            .pool_max_size(2),
    )
    .await
    .unwrap();
    let name = b"hagency.observer.sync.v1";
    let mut value: Value = cipher
        .decrypt_value(&store.get_custom_value(name).await.unwrap().unwrap())
        .unwrap();
    alter(&mut value);
    store
        .set_custom_value(name, cipher.encrypt_value(&value).unwrap())
        .await
        .unwrap();
    store.close().await.unwrap();
}
#[tokio::test]
async fn native_matrix_rejection_bounds_legacy_filter_history_is_inspectable_only() {
    let (f, mut fake, c) = ready(false).await;
    assert_eq!(
        run(
            &c,
            &mut fake,
            sync(
                "old",
                vec![event("outside", "Not selected", &[], Some("$else"))]
            ),
            false
        )
        .await
        .unwrap()
        .filtered,
        1
    );
    stop_sdk(c).await;
    alter_journal(&f, |value| {
        value["intake_receipts"][0]
            .as_object_mut()
            .unwrap()
            .remove("dispositions");
    })
    .await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), 1, false),
        f.store.clone(),
    )
    .unwrap();
    assert_eq!(status(&c, &mut fake).await.stage, "idle");
    let cancel = CancellationToken::new();
    let (result, ()) = common::scripted(c.intake(plan(), &cancel), async {
        fake.next().await.json(200, common::who());
        fake.next().await.json(200, sync("new", vec![]));
    })
    .await;
    assert_eq!(result, Err(Error::Unsupported));
    assert_eq!(rows(&f, "admitted_messages"), 0);
    assert!(f.root.path().join("sdk/journal.key").is_file());
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
#[tokio::test]
async fn native_matrix_rejection_bounds_restore_refuses_changed_dispositions() {
    for kind in [
        "raw",
        "proof",
        "count",
        "candidate",
        "body",
        "prepared",
        "applying",
    ] {
        let (f, mut fake, c) = ready(false).await;
        c.inner
            .handoff_fault
            .store(1, std::sync::atomic::Ordering::SeqCst);
        let mut values = malformed();
        values.push(event("candidate", "Bound original body", &[], None));
        assert_eq!(
            run(&c, &mut fake, sync("pending", values), false).await,
            Err(Error::Busy)
        );
        c.close().await.unwrap();
        alter_journal(&f, |value| match kind {
            "raw" => value["intake"]["dispositions"][0]["source"]["raw"] = json!("a".repeat(64)),
            "proof" => value["intake"]["dispositions"][0]["sdk_observation"] = json!("invalid"),
            "candidate" => value["intake"]["events"][0]["input"]["event_id"] = json!("$substitute"),
            "body" => value["intake"]["events"][0]["input"]["body"] = json!("Substituted text"),
            "prepared" | "applying" => value["intake"]["phase"] = json!(kind),
            _ => {
                value["intake"]["dispositions"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            }
        })
        .await;
        let c = Collector::new(
            config(&f, &fake.endpoint, f.identity.clone(), 1, false),
            f.store.clone(),
        )
        .unwrap();
        let cancel = CancellationToken::new();
        let (result, ()) = common::scripted(c.intake_status(&cancel), async {
            fake.next().await.json(200, common::who());
        })
        .await;
        assert_eq!(result, Err(Error::Storage));
        assert_eq!(rows(&f, "admitted_messages"), 0);
        c.close().await.unwrap();
        f.store.shutdown().await.unwrap();
        fake.close().await;
    }
}

#[tokio::test]
async fn native_matrix_rejection_bounds_actual_terminal_receipts_never_evict() {
    let (f, fake, c) = ready(false).await;
    let targets = vec![f.store.matrix_intake_route("root".into()).await.unwrap()];
    let guard = c.inner.owner.lock().await;
    let owner = guard.as_ref().unwrap();
    for i in 1..64 {
        let events = if i == 1 {
            (0..100)
                .map(|n| event(&format!("filtered{n}"), "Not selected", &[], Some("$other")))
                .collect()
        } else {
            vec![malformed().remove(0)]
        };
        owner
            .intake_start(sync(&format!("terminal{i}"), events), targets.clone())
            .await
            .unwrap();
        let batch = owner.batch().await.unwrap().unwrap();
        if i == 1 {
            assert_eq!(batch.filtered, 100)
        } else {
            assert_eq!(batch.rejected(), 1)
        }
        owner.intake_finish(batch.digest).await.unwrap();
    }
    assert_eq!(
        owner.intake_start(sync("overflow", vec![]), targets).await,
        Err(Error::Capacity)
    );
    assert!(owner.batch().await.unwrap().is_none());
    assert_eq!(owner.cursor().await.unwrap().as_deref(), Some("terminal63"));
    assert_eq!(rows(&f, "admitted_messages"), 0);
    drop(guard);
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
