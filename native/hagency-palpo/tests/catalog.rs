#[allow(dead_code)] // Other transport fixtures use the remaining shared helpers.
mod common;
use common::*;
use hagency_core::{authority::Registration, canonical, project::Resource};
use hagency_palpo::{Adapter, CancellationToken, Error, HostConfig, Step};
use hagency_store::{DomainRepository, DomainStore, Store, outbound::RegistrationIdentity};
use serde_json::{Value, json};

fn domain_registration() -> Registration {
    Registration {
        fleet_id: FLEET.into(),
        generation: 7,
        server_name: "matrix.example.test".into(),
        representative_mxid: format!("@{FLEET}_representative:matrix.example.test"),
        approval_bot_mxid: "@approval:matrix.example.test".into(),
        reception_room_id: "!reception:matrix.example.test".into(),
    }
}
fn resource() -> Resource {
    serde_json::from_value(json!({"presetId":"private_preset","seatId":"private_seat",
        "framework":"codex","model":"gpt-5.6-sol","reasoning":"medium",
        "ceiling":{"tokens":1000,"period":"monthly"}}))
    .unwrap()
}
struct Context {
    _custody_dir: tempfile::TempDir,
    _domain_dir: tempfile::TempDir,
    store: Store,
    domain: DomainStore,
    adapter: Adapter,
}
impl Context {
    async fn new(endpoint: &str) -> Self {
        let (custody_dir, store) = store();
        let domain_dir = tempfile::tempdir().unwrap();
        let domain = DomainStore::start(
            DomainRepository::open(&domain_dir.path().join("private")).unwrap(),
            32,
        )
        .unwrap();
        let registration = domain_registration();
        domain.register(registration.clone()).await.unwrap();
        let identity = RegistrationIdentity {
            registration_fingerprint: canonical::digest(&json!(registration)).unwrap(),
            ..common::registration()
        };
        let config = HostConfig::new(identity, endpoint, TOKEN, 31, limits())
            .unwrap()
            .with_root_pem(include_bytes!("fixtures/ca.pem"))
            .unwrap();
        let adapter = Adapter::attach(config, store.clone()).await.unwrap();
        Self {
            _custody_dir: custody_dir,
            _domain_dir: domain_dir,
            store,
            domain,
            adapter,
        }
    }
    async fn close(self) {
        self.domain.shutdown().await.unwrap();
        self.store.shutdown().await.unwrap();
    }
}
fn offers(v: &Value) -> &[Value] {
    v["capabilities"]["offers"].as_array().unwrap()
}
fn check(request: &Request, sequence: u64) -> Value {
    assert!(request.target.ends_with("/updates"));
    assert_eq!(request.method, "POST");
    assert_eq!(request.headers["authorization"], format!("Bearer {TOKEN}"));
    let value = request.value();
    assert_eq!(value["generation"], 31);
    assert_eq!(value["sequence"], sequence);
    assert_eq!(value["capabilities"]["fleetId"], FLEET);
    assert_eq!(
        value["capabilities"]["representativeMxid"],
        domain_registration().representative_mxid
    );
    assert!(!value.to_string().contains("private_preset"));
    assert!(!value.to_string().contains("private_seat"));
    value
}

#[tokio::test]
async fn native_catalog_outbound_dynamic() {
    let mut fake = Fake::start(true).await;
    let ctx = Context::new(&fake.endpoint).await;
    let cancel = CancellationToken::new();
    let (result, ()) = tokio::join!(
        ctx.adapter.run_with_resources(&ctx.domain, &cancel),
        async {
            let mut sequence = 0;
            loop {
                let request = fake.next().await;
                if request.target.contains("/poll?") {
                    request.json(200, empty(31));
                    continue;
                }
                sequence += 1;
                let value = check(&request, sequence);
                match sequence {
                    1 => {
                        assert!(offers(&value).is_empty());
                        ctx.domain.put_resource(resource()).await.unwrap();
                    }
                    2 => {
                        assert!(!offers(&value).is_empty());
                        for offer in offers(&value) {
                            assert_eq!(offer["resources"][0]["id"], resource().id());
                        }
                        let mut withdrawn = resource();
                        withdrawn.published = false;
                        ctx.domain.put_resource(withdrawn).await.unwrap();
                    }
                    3 => assert!(offers(&value).is_empty()),
                    _ => panic!("unexpected publication"),
                }
                request.json(200, json!({"ok":true}));
                if sequence == 3 {
                    cancel.cancel();
                    break;
                }
            }
        }
    );
    assert_eq!(result, Ok(()));
    ctx.close().await;
    fake.close().await;
}

#[tokio::test]
async fn native_catalog_outbound_pending() {
    let mut fake = Fake::start(true).await;
    let ctx = Context::new(&fake.endpoint).await;
    ctx.domain.put_resource(resource()).await.unwrap();
    let cancel = CancellationToken::new();
    let mut original = Vec::new();
    let (first, ()) = tokio::join!(
        ctx.adapter.publish_resources_once(&ctx.domain, &cancel),
        async {
            let request = fake.next().await;
            assert!(!offers(&check(&request, 1)).is_empty());
            original = request.body.clone();
            drop(request); // Actual accepted bytes, missing HTTP acknowledgment.
        }
    );
    assert_eq!(first, Err(Error::Transport));
    let mut withdrawn = resource();
    withdrawn.published = false;
    ctx.domain.put_resource(withdrawn).await.unwrap();
    let (retry, ()) = tokio::join!(
        ctx.adapter.publish_resources_once(&ctx.domain, &cancel),
        async {
            let request = fake.next().await;
            assert_eq!(request.body, original);
            check(&request, 1);
            request.json(200, json!({"ok":true}));
        }
    );
    assert_eq!(retry, Ok(Step::Published));
    let (next, ()) = tokio::join!(
        ctx.adapter.publish_resources_once(&ctx.domain, &cancel),
        async {
            let request = fake.next().await;
            assert!(offers(&check(&request, 2)).is_empty());
            request.json(200, json!({"ok":true}));
        }
    );
    assert_eq!(next, Ok(Step::Published));
    ctx.close().await;
    fake.close().await;
}

#[tokio::test]
async fn native_catalog_outbound_retirement() {
    let mut fake = Fake::start(true).await;
    let ctx = Context::new(&fake.endpoint).await;
    ctx.domain.put_resource(resource()).await.unwrap();
    let cancel = CancellationToken::new();
    let (first, ()) = tokio::join!(
        ctx.adapter.publish_resources_once(&ctx.domain, &cancel),
        async {
            let request = fake.next().await;
            check(&request, 1);
            let mut rotated = domain_registration();
            rotated.generation += 1;
            ctx.domain.register(rotated).await.unwrap();
            drop(request); // Already admitted bytes cannot be recalled by rotation.
        }
    );
    assert_eq!(first, Err(Error::Transport));
    assert_eq!(
        ctx.adapter
            .publish_resources_once(&ctx.domain, &cancel)
            .await,
        Err(Error::Generation)
    );
    fake.no_request().await;
    cancel.cancel();
    assert_eq!(
        ctx.adapter
            .publish_resources_once(&ctx.domain, &cancel)
            .await,
        Err(Error::Cancelled)
    );
    ctx.close().await;
    fake.close().await;
}

#[tokio::test]
async fn native_catalog_outbound_queued_rotation() {
    use hagency_store::outbound::{Command, Reply};
    use std::time::Duration;
    let mut fake = Fake::start(true).await;
    let ctx = Context::new(&fake.endpoint).await;
    ctx.domain.put_resource(resource()).await.unwrap();
    // Preserve a real frozen update before blocking the original writer. This
    // separate connection only owns a fixture lock; it never writes authority.
    let identity = RegistrationIdentity {
        registration_fingerprint: canonical::digest(&json!(domain_registration())).unwrap(),
        ..common::registration()
    };
    let snapshot = ctx.domain.published_catalog(identity).await.unwrap();
    ctx.adapter
        .freeze_update(snapshot.into_update())
        .await
        .unwrap();
    let lock = rusqlite::Connection::open(ctx._custody_dir.path().join("private/custody.sqlite3"))
        .unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut blocker = std::pin::pin!(
        ctx.store
            .outbound(Command::PendingPublication(ctx.adapter.scope()), now())
    );
    // Poll the original command once to enqueue it, then observe that the
    // exclusive writer consumed it. It cannot complete while this lock is held.
    tokio::select! {
        biased;
        result = &mut blocker => panic!("custody lock was not held: {}", result.is_ok()),
        () = tokio::task::yield_now() => {}
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        while ctx.store.queue_remaining() != 32 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let cancel = CancellationToken::new();
    let mut publication = std::pin::pin!(ctx.adapter.publish_resources_once(&ctx.domain, &cancel));
    tokio::select! {
        result = &mut publication => panic!("publication did not queue: {result:?}"),
        result = tokio::time::timeout(Duration::from_secs(1), async {
            while ctx.store.queue_remaining() == 32 { tokio::task::yield_now().await; }
        }) => result.unwrap(),
    }
    // The publication itself has now enqueued a custody command, proving its
    // first current-domain observation completed before this actual rotation.
    let mut rotated = domain_registration();
    rotated.generation += 1;
    ctx.domain.register(rotated).await.unwrap();
    lock.execute_batch("ROLLBACK").unwrap();
    let (blocked, result) = tokio::join!(&mut blocker, &mut publication);
    assert!(matches!(blocked.unwrap(), Reply::Publication(Some(_))));
    assert_eq!(result, Err(Error::Generation));
    fake.no_request().await;
    assert!(matches!(
        ctx.store
            .outbound(Command::PendingPublication(ctx.adapter.scope()), now())
            .await
            .unwrap(),
        Reply::Publication(Some(_))
    ));
    ctx.domain.shutdown().await.unwrap();
    ctx.store.shutdown().await.unwrap();
    fake.close().await;
}
