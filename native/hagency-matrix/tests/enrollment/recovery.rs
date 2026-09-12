//! Readers open after the actual SDK owner closes. Negative mutations never
//! create passing identity, trust, session or enrollment evidence.
use super::*;
use crate::enrollment::state::{KEY, Ledger, Phase, View, WritePhase};
use matrix_sdk_base::store::StateStore;
use matrix_sdk_crypto::{CollectStrategy, EncryptionSettings, OlmMachine, store::CryptoStore};
use matrix_sdk_sqlite::{SqliteCryptoStore, SqliteStateStore, SqliteStoreConfig};
use matrix_sdk_store_encryption::StoreCipher;
use ruma::{device_id, room_id, user_id};
use std::{ops::Deref, sync::Arc};

pub(super) struct CryptoInspection {
    store: SqliteCryptoStore,
}
impl Deref for CryptoInspection {
    type Target = SqliteCryptoStore;

    fn deref(&self) -> &Self::Target {
        &self.store
    }
}
impl CryptoInspection {
    // All three callers drop their original machine before this close. Transfer
    // closing the SAME shared pool into a dedicated runtime; this clone neither
    // reopens SQLite nor creates an SDK owner. Deadpool schedules the contained
    // connection drops there, and dropping that runtime joins those jobs before
    // another fixture owner opens the original files. Earlier background work on
    // the ambient runtime is not covered by this close-generated-work barrier.
    // This lifetime correction does not establish the original hidden error.
    pub(super) async fn close(&self) -> Result<(), <SqliteCryptoStore as CryptoStore>::Error> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let result = runtime.block_on(async move {
                let result = store.close().await;
                drop(store);
                result
            });
            drop(runtime);
            result
        })
        .await
        .unwrap()
    }
}

pub(super) async fn open_crypto(config: &crate::HostConfig) -> (OlmMachine, CryptoInspection) {
    let store = SqliteCryptoStore::open_with_config(
        &SqliteStoreConfig::new(&config.root)
            .key(Some(&config.key))
            .pool_max_size(2),
    )
    .await
    .unwrap();
    assert!(
        store.load_account().await.unwrap().is_some(),
        "only the actual original SDK account is opened"
    );
    let machine = OlmMachine::with_store(
        user_id!("@worker:example.test"),
        device_id!("DEVICE_1"),
        store.clone(),
        None,
    )
    .await
    .unwrap();
    (machine, CryptoInspection { store })
}

pub async fn decrypt_from_original_sessions(f: &mut Fixture) {
    stop_sdk(&f.collector).await;
    let (machine, store) = open_crypto(&f.collector.inner.config).await;
    let room = room_id!("!direct:example.test");
    let settings = EncryptionSettings {
        sharing_strategy: CollectStrategy::OnlyTrustedDevices,
        ..Default::default()
    };
    let shares = machine
        .share_room_key(
            room,
            [user_id!("@owner:example.test")].into_iter(),
            settings,
        )
        .await
        .unwrap();
    assert_eq!(shares.len(), 1);
    for share in shares {
        f.peer.share(json!({"messages":share.messages})).await;
        machine
            .mark_request_as_sent(
                &share.txn_id,
                &ruma::api::client::to_device::send_event_to_device::v3::Response::new(),
            )
            .await
            .unwrap();
    }
    let content = json!({"msgtype":"m.text","body":"actual original signed session"});
    let raw = ruma::serde::Raw::from_json_string(content.to_string()).unwrap();
    let event = machine
        .encrypt_room_event_raw(room, "m.room.message", &raw)
        .await
        .unwrap();
    let plain = f
        .peer
        .decrypt(serde_json::to_value(event.content).unwrap(), room)
        .await;
    assert_eq!(plain["content"], content);
    drop(machine);
    store.close().await.unwrap();
    drop(store);
}

async fn inspect<T, F, Fut>(config: &crate::HostConfig, operation: F) -> T
where
    T: Send + 'static,
    F: FnOnce(SqliteStateStore, StoreCipher) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = T>,
{
    let root = config.root.clone();
    let key = config.key;
    tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(async move {
            let store = SqliteStateStore::open_with_config(
                &SqliteStoreConfig::new(&root)
                    .key(Some(&key))
                    .pool_max_size(2),
            )
            .await
            .unwrap();
            let cipher = StoreCipher::import_with_key(
                &key,
                &std::fs::read(root.join("journal.key")).unwrap(),
            )
            .unwrap();
            let result = operation(store.clone(), cipher).await;
            store.close().await.unwrap();
            drop(store);
            result
        });
        // Deadpool schedules read-connection destruction as blocking work.
        // Join it before the next SDK owner can examine these original files.
        // This closes a fixture lifetime gap; it does not identify the earlier
        // hidden-error failure or add a production wait/retry.
        drop(runtime);
        result
    })
    .await
    .unwrap()
}
async fn read_ledger(config: &crate::HostConfig) -> Ledger {
    inspect(config, |store, cipher| async move {
        let bytes = store.get_custom_value(KEY).await.unwrap().unwrap();
        cipher.decrypt_value(&bytes).unwrap()
    })
    .await
}

#[tokio::test]
async fn native_matrix_enrollment_custody() {
    for variant in 0..3 {
        let mut f = Fixture::new().await;
        let cancel = CancellationToken::new();
        let mut operation = Box::pin(f.collector.enroll_fresh_account(&cancel));
        let request = until_write(
            operation.as_mut(),
            &mut f.fake,
            &mut f.peer,
            "/_matrix/client/v3/keys/upload",
        )
        .await;
        assert_eq!(
            f.collector
                .enroll_fresh_account(&CancellationToken::new())
                .await,
            Err(Error::Busy)
        );
        assert_eq!(f.collector.close().await, Err(Error::Busy));
        let original_body = std::str::from_utf8(&request.body).unwrap().to_owned();
        let body = serde_json::from_slice(&request.body).unwrap();
        let (status, value) = f
            .peer
            .protocol(&request.method, &request.target, &body)
            .await
            .unwrap();
        assert_eq!(f.peer.writes.len(), 1);
        if variant == 0 {
            // Dropping the real caller future does not abort its owned coordinator.
            drop(operation);
            request.json(status, value);
            assert_eq!(finish_owned(&mut f).await, Ok(()));
            assert_eq!(f.peer.writes.len(), 5);
            assert_eq!(f.peer.claims, 1);
        } else {
            if variant == 1 {
                cancel.cancel();
            }
            let result = operation.await;
            assert_eq!(
                result,
                Err(if variant == 1 {
                    Error::Cancelled
                } else {
                    Error::Timeout
                })
            );
            // The server had the actual upload, but its acknowledgement was lost.
            drop(request);
            assert!(!f.base.available().await);
        }
        stop_sdk(&f.collector).await;
        let record = read_ledger(&f.collector.inner.config).await;
        assert_eq!(record.writes[0].body, original_body);
        if variant == 0 {
            assert!(matches!(sdk_status(&f.collector).await, Ok(View::Complete)));
            // An unexplained lost result in the original registry is uncertainty.
            // This is a negative consistency fixture, not simulated crash proof.
            *f.collector
                .inner
                .enrollment_jobs
                .0
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .result
                .lock()
                .unwrap() = None;
            assert_eq!(f.run().await, Err(Error::OutcomeUnknown));
        } else {
            assert!(record.writes[0].phase == WritePhase::Possible);
            assert!(record.writes[0].response.is_none());
            assert!(matches!(
                sdk_status(&f.collector).await,
                Err(Error::OutcomeUnknown)
            ));
        }
        f.fake.no_request().await;
        let weak = Arc::downgrade(&f.collector.inner);
        f.close().await;
        assert!(
            weak.upgrade().is_none(),
            "original enrollment registry must not retain its Collector"
        );
    }
}

#[tokio::test]
async fn native_matrix_enrollment_unknown() {
    for fault in [1, 2] {
        let mut f = Fixture::new().await;
        {
            let guard = f.collector.inner.owner.lock().await;
            assert!(matches!(
                guard
                    .as_ref()
                    .unwrap()
                    .enrollment(crate::sdk::enrollment::Command::Fault(fault))
                    .await,
                Ok(View::Unit)
            ));
        }
        assert_eq!(f.run().await, Err(Error::OutcomeUnknown));
        assert_eq!(f.peer.writes.len(), usize::from(fault == 2));
        stop_sdk(&f.collector).await;
        let record = read_ledger(&f.collector.inner.config).await;
        if fault == 1 {
            assert!(record.phase == Phase::Preparing);
            assert!(record.writes.is_empty());
            let (machine, store) = open_crypto(&f.collector.inner.config).await;
            assert!(store.load_identity().await.unwrap().is_none());
            assert!(!machine.cross_signing_status().await.is_complete());
            drop(machine);
            store.close().await.unwrap();
            drop(store);
        } else {
            assert!(record.writes[0].phase == WritePhase::Applying);
            assert!(record.writes[0].response.is_some());
        }
        let trace = crate::collector::observation::Trace::new(
            "enrollment.unknown.original-status",
            Some(if fault == 1 { "preparing" } else { "applying" }),
            None,
        );
        let reopened =
            crate::collector::observation::observed(trace, sdk_status(&f.collector)).await;
        assert!(
            matches!(reopened, Err(Error::OutcomeUnknown)),
            "fault {fault} original reopened enrollment status: {:?}",
            reopened.as_ref().map(|view| match view {
                View::Absent => "absent",
                View::Complete => "complete",
                View::Query(_) => "query",
                View::Write(_) => "write",
                View::Verify => "verify",
                View::Ready => "ready",
                View::Unit => "unit",
            })
        );
        f.fake.no_request().await;
        f.close().await;
    }

    for target in [1, 2, 3] {
        let mut f = Fixture::new().await;
        let (reached, mut observed) = tokio::sync::oneshot::channel();
        let (release, hold) = tokio::sync::oneshot::channel();
        {
            let guard = f.collector.inner.owner.lock().await;
            let hook = crate::sdk::enrollment::ReplyHold {
                target,
                reached,
                release: hold,
                lose_reply: true,
            };
            assert!(matches!(
                guard
                    .as_ref()
                    .unwrap()
                    .enrollment(crate::sdk::enrollment::Command::HoldReply(hook))
                    .await,
                Ok(View::Unit)
            ));
        }
        let cancel = CancellationToken::new();
        let mut operation = Box::pin(f.collector.enroll_fresh_account(&cancel));
        for count in 0..=96 {
            assert!(count < 96);
            tokio::select! {
                result=&mut operation => panic!("original enrollment returned before actual SDK hold: {result:?}"),
                result=&mut observed => {result.unwrap();break;},
                request=f.fake.next()=>respond(request,&mut f.peer).await,
            }
        }
        assert_eq!(
            f.peer.writes.len(),
            match target {
                1 => 0,
                2 => 1,
                _ => 5,
            }
        );
        assert!(matches!(
            crate::sdk::Owner::open_existing(&f.collector.inner.config).await,
            Err(Error::Busy)
        ));
        assert_eq!(f.collector.close().await, Err(Error::Busy));
        drop(operation);
        release.send(()).unwrap();
        assert_eq!(finish_owned(&mut f).await, Err(Error::OutcomeUnknown));
        assert!(!f.base.available().await);
        stop_sdk(&f.collector).await;
        let record = read_ledger(&f.collector.inner.config).await;
        match target {
            1 => {
                assert!(record.phase == Phase::Writing);
                assert!(
                    record
                        .writes
                        .iter()
                        .all(|w| w.phase == WritePhase::Prepared)
                );
            }
            2 => {
                assert!(record.writes[0].phase == WritePhase::Applied);
                assert!(record.writes[0].response.is_some());
            }
            3 => {
                assert!(record.phase == Phase::Complete);
            }
            _ => unreachable!(),
        }
        let reopened = sdk_status(&f.collector).await;
        if target == 3 {
            assert!(matches!(reopened, Ok(View::Complete)));
            // Historical completion cannot undo the original failed readiness.
            assert_eq!(f.run().await, Err(Error::OutcomeUnknown));
        } else {
            assert!(matches!(reopened, Err(Error::OutcomeUnknown)));
        }
        f.fake.no_request().await;
        f.close().await;
    }

    let mut f = Fixture::new().await;
    let sql = rusqlite::Connection::open(
        f.collector
            .inner
            .config
            .root
            .join("matrix-sdk-state.sqlite3"),
    )
    .unwrap();
    let mut injected = false;
    let result = drive_with(&f.collector, &mut f.fake, &mut f.peer, &CancellationToken::new(), |request, _, _| {
        if request.target == "/_matrix/client/v3/keys/upload" {
            sql.execute_batch("CREATE TRIGGER enrollment_abort BEFORE INSERT ON kv_blob BEGIN SELECT RAISE(ABORT,'fixture enrollment rollback'); END;").unwrap();
            injected = true;
        }
    }).await;
    assert!(injected);
    assert_eq!(result, Err(Error::Storage));
    assert_eq!(f.peer.writes.len(), 1);
    sql.execute_batch("DROP TRIGGER enrollment_abort;").unwrap();
    drop(sql);
    stop_sdk(&f.collector).await;
    let record = read_ledger(&f.collector.inner.config).await;
    assert!(record.writes[0].phase == WritePhase::Possible);
    assert!(record.writes[0].response.is_none());
    assert!(matches!(
        sdk_status(&f.collector).await,
        Err(Error::OutcomeUnknown)
    ));
    f.fake.no_request().await;
    f.close().await;
}

#[tokio::test]
async fn native_matrix_enrollment_restore() {
    let mut f = Fixture::new().await;
    assert_eq!(f.run().await, Ok(()));
    stop_sdk(&f.collector).await;
    let record = read_ledger(&f.collector.inner.config).await;
    assert!(record.phase == Phase::Complete);
    assert_eq!(record.writes.len(), 5);
    assert_eq!(record.sessions.len(), 1);
    let trace =
        crate::collector::observation::Trace::new("enrollment.restore.original-status", None, None);
    let status = crate::collector::observation::observed(trace, sdk_status(&f.collector)).await;
    assert!(
        matches!(status, Ok(View::Complete)),
        "original reopened SDK status: {:?}",
        status.err()
    );
    assert_eq!(f.run().await, Ok(()));
    assert_eq!(f.peer.writes.len(), 5);
    stop_sdk(&f.collector).await;
    let mut wrong = config(&f.base, &f.fake, &f.peer);
    wrong.enrollment = Some(
        crate::enrollment::state::Profile::new(
            vec![(crypto::HUMAN.into(), crypto::Peer::new().await.anchor())],
            crypto::SENDER,
            "example.test",
        )
        .unwrap(),
    );
    let owner = crate::sdk::Owner::open_existing(&wrong).await.unwrap();
    assert!(matches!(
        owner
            .enrollment(crate::sdk::enrollment::Command::Status)
            .await,
        Err(Error::Conflict)
    ));
    owner.close().await.unwrap();
    wrong.key = [43; 32];
    assert!(matches!(
        crate::sdk::Owner::open_existing(&wrong).await,
        Err(Error::Storage)
    ));

    let main_key = b"hagency.observer.sync.v1";
    let (original, main, cipher) =
        inspect(&f.collector.inner.config, move |store, cipher| async move {
            (
                store.get_custom_value(KEY).await.unwrap().unwrap(),
                store.get_custom_value(main_key).await.unwrap().unwrap(),
                cipher,
            )
        })
        .await;
    for variant in 0..14 {
        let mut value: serde_json::Value = cipher.decrypt_value(&original).unwrap();
        let mut changed_main = None;
        match variant {
            0 => {
                value["context"]["binding"] = json!("0".repeat(64));
            }
            1 => {
                value["sessions"] = json!([]);
            }
            2 => {
                value["sessions"][0]["ids"] = json!(["nonexistent-session"]);
            }
            3 => {
                value["verified"]["body"] = json!("x".repeat(crate::enrollment::state::QUERY));
            }
            4 => {
                value["writes"][0]["digest"] = json!("0".repeat(64));
            }
            5 => {
                let mut main_value: serde_json::Value = cipher.decrypt_value(&main).unwrap();
                main_value["enrollment"] = serde_json::Value::Null;
                changed_main = Some(cipher.encrypt_value(&main_value).unwrap());
            }
            6 => {
                value["unknown_field"] = json!(true);
            }
            7 => {
                let body = json!({"device_keys":{},"one_time_keys":{}}).to_string();
                value["writes"][0]["digest"] = json!(crate::outgoing::state::hash(body.as_bytes()));
                value["writes"][0]["body"] = json!(body);
            }
            8 => {
                value["writes"][2]["response"] =
                    json!({"failures":{"example.test":{"errcode":"M_UNKNOWN"}}});
            }
            9 => {
                value["writes"][0]["kind"] = json!("Signature");
            }
            10 => {
                value["sessions"][0]["before_ids"] = value["sessions"][0]["ids"].clone();
            }
            11 => {
                value["sessions"][0]["before_ids"] = json!(["unrelated-before-session"]);
            }
            12 | 13 => {
                let index = if variant == 12 { 3 } else { 2 };
                let mut body: serde_json::Value =
                    serde_json::from_str(value["writes"][index]["body"].as_str().unwrap()).unwrap();
                if variant == 12 {
                    body[crypto::HUMAN]
                        .as_object_mut()
                        .unwrap()
                        .values_mut()
                        .next()
                        .unwrap()
                        .as_object_mut()
                        .unwrap()
                        .remove("usage");
                } else {
                    body[crypto::SENDER][crypto::DEVICE]
                        .as_object_mut()
                        .unwrap()
                        .remove("algorithms");
                }
                let body = body.to_string();
                value["writes"][index]["digest"] =
                    json!(crate::outgoing::state::hash(body.as_bytes()));
                value["writes"][index]["body"] = json!(body);
            }
            _ => unreachable!(),
        }
        let bytes = cipher.encrypt_value(&value).unwrap();
        inspect(&f.collector.inner.config, move |store, _| async move {
            if let Some(main) = changed_main {
                store.set_custom_value(main_key, main).await.unwrap();
            }
            store.set_custom_value(KEY, bytes).await.unwrap();
        })
        .await;
        assert!(
            sdk_status(&f.collector).await.is_err(),
            "protected corruption {variant}"
        );
        let (original, main) = (original.clone(), main.clone());
        inspect(&f.collector.inner.config, move |store, _| async move {
            store.set_custom_value(KEY, original).await.unwrap();
            store.set_custom_value(main_key, main).await.unwrap();
        })
        .await;
    }
    assert!(matches!(sdk_status(&f.collector).await, Ok(View::Complete)));
    let key = f.collector.inner.config.root.join("journal.key");
    let saved = f.base.root.path().join("journal.key.fixture-original");
    std::fs::rename(&key, &saved).unwrap();
    assert!(matches!(
        sdk_status(&f.collector).await,
        Err(Error::Storage)
    ));
    assert!(
        !key.exists(),
        "missing original key must never be regenerated"
    );
    std::fs::rename(saved, key).unwrap();
    inspect(&f.collector.inner.config, |store, _| async move {
        store.remove_custom_value(KEY).await.unwrap();
    })
    .await;
    assert!(matches!(
        sdk_status(&f.collector).await,
        Err(Error::Storage)
    ));
    inspect(&f.collector.inner.config, move |store, _| async move {
        assert!(store.get_custom_value(KEY).await.unwrap().is_none());
        store.set_custom_value(KEY, original).await.unwrap();
    })
    .await;
    f.fake.no_request().await;
    f.close().await;
}
