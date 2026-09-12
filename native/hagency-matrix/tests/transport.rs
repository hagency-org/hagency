mod common;
use common::*;
use hagency_core::tasks::SessionBinding;
use hagency_matrix::{CancellationToken, Collector, Error};
use serde_json::json;
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn native_matrix_transport_identity_authenticated_https_and_sdk_restart() {
    let mut fake = Fake::start(true).await;
    let mut f = Fixture::new();
    let cancel = CancellationToken::new();
    let untrusted = Collector::new(f.config(&fake.endpoint), f.store.clone()).unwrap();
    assert_eq!(untrusted.collect(&cancel).await, Err(Error::Transport));
    assert!(!f.available().await);
    assert!(!f.root.path().join("sdk").exists());
    drop(untrusted);
    f.identity.transport.generation = 2;
    let c = Collector::new(
        f.config(&fake.endpoint)
            .with_root_pem(include_bytes!("fixtures/ca.pem"))
            .unwrap(),
        f.store.clone(),
    )
    .unwrap();
    let (result, _) = scripted(c.collect(&cancel), success(&mut fake, "batch1")).await;
    assert_eq!(result.unwrap().rooms, 1);
    assert!(f.available().await);
    let binding = SessionBinding {
        id: "session".into(),
        engagement_id: f.identity.transport.engagement_id.clone(),
        room_id: "!direct:example.test".into(),
        thread_root: None,
    };
    f.store
        .resolve_verified_matrix_session(binding.clone())
        .await
        .unwrap();
    let identity = std::fs::read(f.root.path().join("sdk/identity")).unwrap();
    c.close().await.unwrap();
    assert!(!f.available().await);
    f.identity.transport.generation = 3;
    let c = Collector::new(
        f.config(&fake.endpoint)
            .with_root_pem(include_bytes!("fixtures/ca.pem"))
            .unwrap(),
        f.store.clone(),
    )
    .unwrap();
    let (result, _) = scripted(c.collect(&cancel), async {
        fake.next().await.json(200, who());
        let req = fake.next().await;
        assert!(req.target.ends_with("&since=batch1"));
        req.json(200, sync("batch1"));
        fake.next().await.json(200, state());
    })
    .await;
    result.unwrap();
    assert_eq!(
        std::fs::read(f.root.path().join("sdk/identity")).unwrap(),
        identity
    );
    assert!(
        f.store
            .resolve_verified_matrix_session(binding)
            .await
            .is_err()
    );
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
#[tokio::test]
async fn native_matrix_transport_identity_wrong_account_device_or_missing_device() {
    for body in [
        json!({"user_id":"@other:example.test","device_id":"DEVICE_1"}),
        json!({"user_id":"@worker:other.test","device_id":"DEVICE_1"}),
        json!({"user_id":"@worker:example.test","device_id":"OTHER"}),
        json!({"user_id":"@worker:example.test"}),
        json!({"user_id":"@worker:example.test","device_id":"DEVICE_1","is_guest":true}),
    ] {
        let mut fake = Fake::start(false).await;
        let f = Fixture::new();
        f.store
            .observe_matrix_transport(f.identity.transport.clone())
            .await
            .unwrap();
        let c = Collector::new(f.config(&fake.endpoint), f.store.clone()).unwrap();
        let cancel = CancellationToken::new();
        let (result, _) = tokio::join!(c.collect(&cancel), async {
            fake.next().await.json(200, body);
        });
        assert_eq!(result, Err(Error::Identity));
        assert!(!f.available().await);
        assert!(!f.root.path().join("sdk").exists());
        fake.no_request().await;
        f.store.shutdown().await.unwrap();
        fake.close().await;
    }
}
#[tokio::test]
async fn native_matrix_transport_bounds_json_framing_status_and_deadlines() {
    for (body, expected) in [
        (b"{}{}".to_vec(), Error::InvalidJson),
        (
            b"{\"user_id\":1,\"user_id\":2}".to_vec(),
            Error::InvalidJson,
        ),
        (vec![b'x'; 1024 * 1024 + 1], Error::BodyTooLarge),
    ] {
        let mut fake = Fake::start(false).await;
        let f = Fixture::new();
        let c = Collector::new(f.config(&fake.endpoint), f.store.clone()).unwrap();
        let cancel = CancellationToken::new();
        let (result, _) = tokio::join!(c.collect(&cancel), async {
            fake.next().await.raw(response(200, &body));
        });
        assert_eq!(result, Err(expected));
        assert!(!f.available().await);
        f.store.shutdown().await.unwrap();
        fake.close().await;
    }
    for (status, expected) in [
        (302, Error::Redirect),
        (401, Error::Unauthorized),
        (429, Error::Remote(429)),
        (503, Error::Remote(503)),
    ] {
        let mut fake = Fake::start(false).await;
        let f = Fixture::new();
        let c = Collector::new(f.config(&fake.endpoint), f.store.clone()).unwrap();
        let cancel = CancellationToken::new();
        let (result, _) = tokio::join!(c.collect(&cancel), async {
            fake.next().await.json(status, json!({"secret":TOKEN}));
        });
        assert_eq!(result, Err(expected));
        assert!(!format!("{result:?}").contains(TOKEN));
        fake.no_request().await;
        f.store.shutdown().await.unwrap();
        fake.close().await;
    }
    for body in [false, true] {
        let mut fake = Fake::start(false).await;
        let f = Fixture::new();
        f.store
            .observe_matrix_transport(f.identity.transport.clone())
            .await
            .unwrap();
        let c = Arc::new(Collector::new(f.config(&fake.endpoint), f.store.clone()).unwrap());
        let owner = c.clone();
        let task = tokio::spawn(async move { owner.collect(&CancellationToken::new()).await });
        let req = fake.next().await;
        assert_eq!(c.collect(&CancellationToken::new()).await, Err(Error::Busy));
        if body {
            req.chunks(vec![(Duration::ZERO,b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n{".to_vec()),(Duration::from_secs(1),b"}".to_vec())]);
        } else {
            req.chunks(vec![(
                Duration::from_secs(1),
                response(200, &serde_json::to_vec(&who()).unwrap()),
            )]);
        }
        task.abort(); // The bounded owned operation must still settle negative authority.
        tokio::time::timeout(Duration::from_secs(2), async {
            while f.available().await {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(!f.available().await);
        f.store.shutdown().await.unwrap();
        fake.close().await;
    }
}
#[tokio::test]
async fn native_matrix_transport_rooms_full_snapshots_refuse_unsafe_or_conflicting_state() {
    for variant in [
        "third",
        "missing",
        "duplicate",
        "plaintext",
        "public",
        "algorithm",
        "wrong_room",
    ] {
        let mut fake = Fake::start(false).await;
        let f = Fixture::new();
        let c = Collector::new(f.config(&fake.endpoint), f.store.clone()).unwrap();
        let cancel = CancellationToken::new();
        let (result, _) = scripted(c.collect(&cancel), async {
            fake.next().await.json(200, who());
            fake.next().await.json(200, sync("batch"));
            let mut v = state();
            let a = v.as_array_mut().unwrap();
            match variant {
 "third"=>a.push(json!({"type":"m.room.member","state_key":"@third:example.test","content":{"membership":"join"}})),
 "missing"=>{a.remove(1);},"duplicate"=>a.push(a[0].clone()),"plaintext"=>{a.pop();},"public"=>a[2]["content"]["join_rule"]=json!("public"),"algorithm"=>a[3]["content"]["algorithm"]=json!("unknown"),_=>a[0]["room_id"]=json!("!other:example.test")};
            fake.next().await.json(200, v);
        }).await;
        assert!(result.is_err(), "{variant}");
        assert!(!f.available().await);
        c.close().await.unwrap();
        f.store.shutdown().await.unwrap();
        fake.close().await;
    }
}

#[test]
fn native_matrix_transport_identity_host_configuration_rejects_ambiguous_endpoints() {
    let f = Fixture::new();
    for url in [
        "http://example.test/",
        "http://localhost/",
        "https://user:secret@example.test/",
        "https://example.test/?token=secret",
        "https://example.test/#fragment",
        "https://example.test/../",
        "https://EXAMPLE.test/",
        "https://example.test:0/",
        "https://example.test/%2f",
        "ftp://example.test/",
        " https://example.test/",
    ] {
        let result = hagency_matrix::HostConfig::new(
            f.identity.clone(),
            url,
            TOKEN,
            f.root.path().join("sdk"),
            [42; 32],
            vec![hagency_matrix::HostRoom {
                room_id: "!room:example.test".into(),
                generation: 1,
                privacy: hagency_core::replies::RoomPrivacy::Group {},
            }],
            limits(),
        );
        assert!(result.is_err(), "{url}");
    }
}
#[tokio::test]
async fn native_matrix_transport_bounds_sync_scopes_events_and_cancel() {
    for variant in ["foreign", "events", "depth", "cancel", "headers"] {
        let mut fake = Fake::start(false).await;
        let f = Fixture::new();
        let c = Collector::new(f.config(&fake.endpoint), f.store.clone()).unwrap();
        let cancel = CancellationToken::new();
        let (result, _) = scripted(c.collect(&cancel), async {
            let req = fake.next().await;
            if variant == "headers" {
                let header = format!(
                    "HTTP/1.1 200 OK\r\nX-Large: {}\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{{}}",
                    "x".repeat(17000)
                );
                req.raw(header.into_bytes());
                return;
            }
            req.json(200, who());
            let req = fake.next().await;
            match variant {
   "foreign"=>req.json(200,json!({"next_batch":"one","rooms":{"join":{"!foreign:example.test":{}}}})),
   "events"=>req.json(200,json!({"next_batch":"one","to_device":{"events":vec![json!({"type":"fixture","content":{}});1001]}})),
   "depth"=>{let mut v=json!({});for _ in 0..66{v=json!({"a":v});}req.json(200,json!({"next_batch":"one","extension":v}));},
   _=>{cancel.cancel();req.json(200,sync("cancelled"));}
  }
        }).await;
        assert!(result.is_err(), "{variant}");
        assert!(!f.available().await);
        c.close().await.unwrap();
        f.store.shutdown().await.unwrap();
        fake.close().await;
    }
}

#[tokio::test]
async fn native_matrix_transport_rooms_unsafe_shared_state_retires_other_agent_route() {
    use common::domain;
    use hagency_core::replies::*;
    use hagency_store::{DomainRepository, DomainStore, EffectOutcome};
    use std::collections::BTreeSet;
    let root = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&root.path().join("domain")).unwrap();
    db.register(&domain::registration()).unwrap();
    let pool = domain::resource("pool", "seat", 1000);
    db.put_resource(&pool).unwrap();
    let mut agents = vec![];
    for name in ["worker", "peer"] {
        let p = domain::proof(&domain::request(name, name, &pool, 100));
        let e = db.admit(&p, 1000).unwrap();
        db.approve(&format!("approve_{name}"), &p, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "fixture".into(),
            },
        )
        .unwrap();
        let t = MatrixTransportObservation {
            engagement_id: e.id,
            registration_generation: 1,
            generation: 1,
            sender_mxid: format!("@{name}:example.test"),
            device_id: if name == "worker" {
                "DEVICE_1"
            } else {
                "DEVICE_PEER"
            }
            .into(),
        };
        db.observe_matrix_transport(&t, 1001).unwrap();
        db.observe_matrix_room(
            &MatrixRoomObservation {
                engagement_id: t.engagement_id.clone(),
                registration_generation: 1,
                transport_generation: 1,
                room_id: "!project:example.test".into(),
                generation: 1,
                privacy: RoomPrivacy::Group {},
                joined: BTreeSet::from([
                    "@worker:example.test".into(),
                    "@peer:example.test".into(),
                    "@owner:example.test".into(),
                ]),
                invite_only: true,
                encrypted: false,
            },
            1002,
        )
        .unwrap();
        db.resolve_verified_matrix_session(
            &SessionBinding {
                id: name.into(),
                engagement_id: t.engagement_id.clone(),
                room_id: "!project:example.test".into(),
                thread_root: None,
            },
            1003,
        )
        .unwrap();
        agents.push(t);
    }
    let store = DomainStore::start(db, 32).unwrap();
    let mut fake = Fake::start(false).await;
    let config = hagency_matrix::HostConfig::new(
        hagency_matrix::HostIdentity {
            server_name: "example.test".into(),
            registration_fingerprint: "a".repeat(64),
            transport: agents[0].clone(),
        },
        &fake.endpoint,
        TOKEN,
        root.path().join("sdk"),
        [42; 32],
        vec![hagency_matrix::HostRoom {
            room_id: "!project:example.test".into(),
            generation: 1,
            privacy: RoomPrivacy::Group {},
        }],
        limits(),
    )
    .unwrap();
    let c = Collector::new(config, store.clone()).unwrap();
    let cancel = CancellationToken::new();
    let (result, _) = scripted(c.collect(&cancel), async {
        fake.next().await.json(200, who());
        fake.next().await.json(200, sync("unsafe"));
        fake.next().await.json(200,json!([
  {"type":"m.room.member","state_key":"@worker:example.test","content":{"membership":"join"}},
  {"type":"m.room.member","state_key":"@peer:example.test","content":{"membership":"join"}},
  {"type":"m.room.member","state_key":"@owner:example.test","content":{"membership":"leave"}}
 ]));
    })
    .await;
    assert!(result.is_err());
    let sql = rusqlite::Connection::open(root.path().join("domain/domain.sqlite3")).unwrap();
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM current_matrix_routes", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
    assert!(
        store
            .matrix_transport_state(agents[1].engagement_id.clone())
            .await
            .unwrap()
            .unwrap()
            .available
    );
    let room = store
        .matrix_room_state(
            agents[1].engagement_id.clone(),
            "!project:example.test".into(),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(!room.available);
    assert_eq!(room.generation, 2);
    c.close().await.unwrap();
    store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_transport_rooms_fixture_accepts_sdk_sized_gap_between_requests() {
    let mut fake = Fake::start(false).await;
    let endpoint = fake.endpoint.clone();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let (result, ()) = scripted(
        async {
            client
                .get(format!("{endpoint}whoami"))
                .send()
                .await
                .unwrap();
            // Deterministically reproduce a legal SDK interval longer than the
            // old three-second fake-peer deadline. No production timeout changes.
            tokio::time::sleep(Duration::from_millis(3100)).await;
            client.get(format!("{endpoint}sync")).send().await.unwrap();
            Ok::<_, Error>(())
        },
        async {
            let request = fake.next().await;
            assert_eq!(request.target, "/whoami");
            request.json(200, who());
            let request = fake.next().await;
            assert_eq!(request.target, "/sync");
            request.json(200, sync("fixture"));
        },
    )
    .await;
    assert_eq!(result, Ok(()));
    fake.close().await;
}

#[tokio::test]
#[should_panic(expected = "collector completed before its HTTP script: Err(Identity)")]
async fn native_matrix_transport_rooms_fixture_reports_early_refusal() {
    let mut fake = Fake::start(false).await;
    let f = Fixture::new();
    let c = Collector::new(f.config(&fake.endpoint), f.store.clone()).unwrap();
    let _ = scripted(c.collect(&CancellationToken::new()), async {
        fake.next().await.json(
            200,
            json!({"user_id":"@other:example.test","device_id":"OTHER"}),
        );
        // Refused whoami must never produce this request; report Identity.
        fake.next().await;
    })
    .await;
}
