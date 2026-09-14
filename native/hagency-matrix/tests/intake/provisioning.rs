use super::*;

/// The session-route room belongs to the fixture engagement's project; the
/// provisioning request targets a fresh project room (the projects table keys
/// (fleet, room), so a new engagement cannot reuse an existing project room).
/// The reception room is observed but never routed: the provisioning event
/// arrives there, not in a room any session route binds.
const SESSION_ROOM: &str = "!project:example.test";
const PROJECT: &str = "!project_provision:example.test";
const RECEPTION: &str = "!reception:example.test";
const PRIVATE: &str = "!private:example.test";
const OWNER: &str = "@owner:example.test";
const APPROVAL_BOT: &str = "@approval:example.test";

fn fleet_id() -> String {
    format!("hf_{}", "a".repeat(32))
}
fn representative() -> String {
    format!("@{}_representative:example.test", fleet_id())
}

fn provisioning_config(f: &common::Fixture, endpoint: &str) -> HostConfig {
    // Only the session-route room is a `rooms` member (it publishes). The
    // request's target project room is named by the event body, not the
    // config: its /state is fetched on demand at provision time and its
    // authority facts stay in memory (observe_matrix_room would refuse it).
    let mut config = config(f, endpoint, f.identity.clone(), 1, false);
    config
        .with_reception_room(HostRoom {
            room_id: RECEPTION.into(),
            generation: 1,
            privacy: RoomPrivacy::Group {},
        })
        .unwrap();
    config
}

/// Seed the enrolled owner-DM room the provisioning ingress reads fail-closed
/// before admit. Written directly (the same pattern as `rows()`); the store
/// writer path requires an engagement that already names the room.
fn enroll_owner_room(f: &common::Fixture) {
    rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3"))
        .unwrap()
        .execute(
            "INSERT INTO approval_rooms(server_name,room_id,generation,fleet_id,project_id,registration_generation,owner_mxid,bot_mxid,device_id,available,digest,config) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,1,?10,?11)",
            rusqlite::params![
                "example.test",
                PRIVATE,
                1,
                fleet_id(),
                "project_one",
                1,
                OWNER,
                APPROVAL_BOT,
                "APPROVAL_DEVICE",
                "e".repeat(64),
                serde_json::json!({
                    "joined": [OWNER, APPROVAL_BOT],
                    "invite_only": true,
                    "encrypted": true,
                    "available": true
                })
                .to_string()
            ],
        )
        .unwrap();
}

fn request_body(request_id: &str, tokens: u64) -> String {
    serde_json::json!({
        "requestId": request_id,
        "requester": OWNER,
        "project": "project_provision",
        "projectRoomId": PROJECT,
        "role": "coding",
        "requestedTokens": tokens,
        "ratePerDay": null,
        "agent": "Provisioned",
        "context": {"agentDefinition": {"resourceId": "resource_27cac5503836765cd10751d2"}}
    })
    .to_string()
}

fn request_event(event_id: &str, body: String) -> Value {
    json!({
        "event_id": event_id,
        "sender": OWNER,
        "type": "m.room.message",
        "origin_server_ts": now(),
        "content": {
            "msgtype": "com.hagency.engagement.request.v1",
            "body": body
        }
    })
}

fn provisioning_sync(token: &str, events: Vec<Value>) -> Value {
    // The target project room never appears in the sync: sync_bounds only
    // accepts observed rooms (config.rooms + reception). The request event
    // names it; its /state is fetched on demand during the handoff.
    json!({
        "next_batch": token,
        "rooms": {
            "join": {
                SESSION_ROOM: {"timeline": {"events": [], "limited": false}, "state": {"events": []}},
                RECEPTION: {"timeline": {"events": events, "limited": false}, "state": {"events": []}}
            }
        },
        "to_device": {"events": []}
    })
}

fn member(mxid: &str) -> Value {
    json!({"type": "m.room.member", "state_key": mxid, "content": {"membership": "join"}})
}

fn power_levels(users: Value) -> Value {
    json!({
        "type": "m.room.power_levels",
        "state_key": "",
        "content": {
            "users": users,
            "users_default": 0,
            "invite": 0
        }
    })
}

/// The session room only needs to publish: no authority facts are read from
/// it. Same shape as the shared intake fixture state, unencrypted.
fn session_state() -> Value {
    json!([
        member("@worker:example.test"),
        member(OWNER),
        {"type": "m.room.join_rules", "state_key": "", "content": {"join_rule": "invite"}},
        power_levels(json!({}))
    ])
}

fn reception_state() -> Value {
    json!([
        member("@worker:example.test"),
        member(OWNER),
        member(&representative()),
        {"type": "m.room.join_rules", "state_key": "", "content": {"join_rule": "invite"}},
        power_levels(json!({}))
    ])
}

fn project_state() -> Value {
    json!([
        member("@worker:example.test"),
        member(OWNER),
        member(&representative()),
        {"type": "m.room.join_rules", "state_key": "", "content": {"join_rule": "invite"}},
        power_levels(json!({OWNER: 100, representative(): 50})),
        {
            "type": "m.room.name",
            "state_key": "",
            "content": {"name": "实际项目名称"}
        },
        {
            "type": "com.hagency.project.binding.v1",
            "state_key": "",
            "content": {
                "v": 1,
                "fleetId": fleet_id(),
                "purpose": "project",
                "projectId": "project_provision",
                "ownerMxid": OWNER,
                "authVersion": 1
            }
        }
    ])
}

/// Prime the collector like `prime()`, but answer both observed room-state
/// requests: the session room first, then the reception room.
async fn prime_provisioning(c: &Collector, f: &common::Fixture, fake: &mut common::Fake) {
    let cancel = CancellationToken::new();
    let (result, _) = common::scripted(c.collect(&cancel), async {
        fake.next().await.json(200, common::who());
        fake.next().await.json(200, common::sync("bootstrap"));
        fake.next().await.json(200, session_state());
        fake.next().await.json(200, reception_state());
    })
    .await;
    result.unwrap();
    f.store
        .resolve_verified_matrix_session(SessionBinding {
            id: "root".into(),
            engagement_id: f.identity.transport.engagement_id.clone(),
            room_id: SESSION_ROOM.into(),
            thread_root: None,
        })
        .await
        .unwrap();
}

async fn ready_provisioning() -> (common::Fixture, common::Fake, Collector) {
    let f = common::Fixture::new();
    enroll_owner_room(&f);
    let mut fake = common::Fake::start(true).await;
    let c = Collector::new(provisioning_config(&f, &fake.endpoint), f.store.clone()).unwrap();
    prime_provisioning(&c, &f, &mut fake).await;
    (f, fake, c)
}

async fn run_provisioning(
    c: &Collector,
    fake: &mut common::Fake,
    value: Value,
) -> Result<IntakeSummary, Error> {
    let cancel = CancellationToken::new();
    let intake = c.intake(plan(), &cancel);
    let (result, ()) = common::scripted(intake, async {
        fake.next().await.json(200, common::who());
        let request = fake.next().await;
        assert!(request.target.contains("sync?"));
        request.json(200, value);
        // Room-state requests: the session room and the reception room during
        // collection, then the request's target project room on demand when
        // provision() finds it missing from the in-memory facts.
        fake.next().await.json(200, session_state());
        fake.next().await.json(200, reception_state());
        fake.next().await.json(200, project_state());
    })
    .await;
    result
}

#[tokio::test]
async fn native_provisioning_ingress_admits_a_provider_approved_request() {
    let (f, mut fake, c) = ready_provisioning().await;
    let before = rows(&f, "engagements");
    let result = run_provisioning(
        &c,
        &mut fake,
        provisioning_sync(
            "provision",
            vec![request_event(
                "$request_one",
                request_body("request_one", 250),
            )],
        ),
    )
    .await;
    let stage = status(&c, &mut fake).await.stage;
    let summary = result.unwrap_or_else(|e| panic!("intake failed: {e:?}, stage={stage}"));
    assert_eq!(summary.admitted, 1);
    assert_eq!(summary.replayed, 0);
    assert_eq!(rows(&f, "engagements"), before + 1);
    c.close().await.unwrap();
}

/// A lost writer response after a successful provision leaves the batch
/// pending; the restored handoff replays the admission (idempotent on
/// `request_id`) instead of minting a second engagement.
#[tokio::test]
async fn native_provisioning_ingress_replays_an_already_admitted_request() {
    let (f, mut fake, c) = ready_provisioning().await;
    let before = rows(&f, "engagements");
    let sync = provisioning_sync(
        "provision",
        vec![request_event("$request_one", request_body("request_one", 250))],
    );
    c.inner
        .handoff_fault
        .store(6, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        run_provisioning(&c, &mut fake, sync).await,
        Err(Error::OutcomeUnknown)
    );
    assert_eq!(rows(&f, "engagements"), before + 1);
    assert_eq!(status(&c, &mut fake).await.stage, "domain_handoff");
    // The writer response was lost; the transport itself is still healthy, so
    // the restored handoff resumes the pending batch under the same owner.
    let second = resume_provisioning(&c, &mut fake)
        .await
        .unwrap_or_else(|e| panic!("replay intake failed: {e:?}"));
    assert_eq!(second.admitted, 0);
    assert_eq!(second.replayed, 1);
    assert_eq!(rows(&f, "engagements"), before + 1);
    c.close().await.unwrap();
}

/// Resume a pending provisioning batch: the handoff re-reads the observed
/// rooms' /state (session, then reception) before replaying the admission.
async fn resume_provisioning(
    c: &Collector,
    fake: &mut common::Fake,
) -> Result<IntakeSummary, Error> {
    let cancel = CancellationToken::new();
    let (result, _) = tokio::join!(c.intake(plan(), &cancel), async {
        fake.next().await.json(200, common::who());
        fake.next().await.json(200, session_state());
        fake.next().await.json(200, reception_state());
    });
    result
}

/// A second provisioning request that reuses an admitted `request_id` with
/// different content digests differently: `admit` refuses the conflict, the
/// handoff quarantines the intake, and no second engagement is minted.
#[tokio::test]
async fn native_provisioning_ingress_refuses_a_conflicting_request_id() {
    let (f, mut fake, c) = ready_provisioning().await;
    let before = rows(&f, "engagements");
    let result = run_provisioning(
        &c,
        &mut fake,
        provisioning_sync(
            "provision",
            vec![
                request_event("$request_one", request_body("request_one", 250)),
                request_event("$request_two", request_body("request_one", 500)),
            ],
        ),
    )
    .await;
    assert_eq!(result, Err(Error::Generation));
    assert_eq!(rows(&f, "engagements"), before + 1);
    assert!(f.available().await);
    assert_eq!(status(&c, &mut fake).await.stage, "quarantined");
    c.close().await.unwrap();
}
