use super::*;
use sha2::Digest;

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

/// The provider's decision event: a separate pre-project admission carrying
/// the request id it approves (ADR-095: the verdict is the separate `approve`
/// write, never folded into the mint).
fn approval_event(event_id: &str, request_id: &str) -> Value {
    approval_event_from(event_id, request_id, &representative())
}

/// The approval event with an explicit sender — the fail-closed refusal tests
/// drive verdicts from a sender that is not the fleet's representative.
fn approval_event_from(event_id: &str, request_id: &str, sender: &str) -> Value {
    json!({
        "event_id": event_id,
        "sender": sender,
        "type": "m.room.message",
        "origin_server_ts": now(),
        "content": {
            "msgtype": "com.hagency.engagement.approval.v1",
            "body": json!({"requestId": request_id, "decision": "approve"}).to_string()
        }
    })
}

/// The provision effect row for the request_one engagement (the fixture's own
/// engagement already carries a completed provision effect, so key on ours).
fn effect_row(f: &common::Fixture) -> Option<(String, String)> {
    rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3"))
        .unwrap()
        .query_row(
            "SELECT f.kind,f.state FROM effects f JOIN engagements e ON e.id=f.engagement_id WHERE f.kind='provision' AND e.request_id='request_one'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()
}

/// The number of session-route rows bound to the request_one engagement's
/// project room (0 before the verdict, exactly 1 after; a refused or replayed
/// verdict must never add a second).
fn route_rows(f: &common::Fixture) -> u64 {
    rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3"))
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM matrix_session_routes r JOIN runner_sessions s ON s.id=r.session_id JOIN engagements e ON e.id=s.engagement_id WHERE e.request_id='request_one'",
            [],
            |r| r.get(0),
        )
        .unwrap()
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
    // The retained product joins the provisioned agent's own MXID to the
    // project room (backend-v2 createAgent): the engagement id is en_ + the
    // sha256 of the [fleet_id, request_id] pair (authority.rs:120-124), so the
    // member event is derived deterministically for request_one.
    let agent_mxid = format!(
        "@en_{}:example.test",
        &format!(
            "{:x}",
            sha2::Sha256::digest(
                serde_json::to_vec(&[&fleet_id(), &"request_one".to_string()]).unwrap()
            )
        )[..32]
    );
    json!([
        member("@worker:example.test"),
        member(OWNER),
        member(&representative()),
        member(&agent_mxid),
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

/// The approval-only variant of `run_provisioning`: a verdict for an unknown
/// request id is refused at the evidence read, before any target-room /state
/// fetch, so the script answers only the session and reception rooms.
async fn run_approval_only(
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
        fake.next().await.json(200, session_state());
        fake.next().await.json(200, reception_state());
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
async fn native_provisioning_ingress_replays_an_identical_duplicate() {
    let (f, mut fake, c) = ready_provisioning().await;
    let before = rows(&f, "engagements");
    let sync = provisioning_sync(
        "provision",
        vec![request_event(
            "$request_one",
            request_body("request_one", 250),
        )],
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
async fn native_provisioning_ingress_refuses_a_conflicting_request_by_the_same_key() {
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

/// A provisioning request whose target-room authority does not verify (the
/// binding names a different fleet than the registration) fails closed:
/// `verify_request` refuses before admission, the handoff quarantines the
/// intake, and nothing is minted.
#[tokio::test]
async fn native_provisioning_ingress_refuses_unverified_before_admit() {
    let (f, mut fake, c) = ready_provisioning().await;
    let before = rows(&f, "engagements");
    let mut forged = project_state();
    for event in forged.as_array_mut().unwrap() {
        if event["type"] == "com.hagency.project.binding.v1" {
            event["content"]["fleetId"] = json!(format!("hf_{}", "b".repeat(32)));
        }
    }
    let cancel = CancellationToken::new();
    let intake = c.intake(plan(), &cancel);
    let (result, ()) = common::scripted(intake, async {
        fake.next().await.json(200, common::who());
        let request = fake.next().await;
        assert!(request.target.contains("sync?"));
        request.json(
            200,
            provisioning_sync(
                "provision",
                vec![request_event(
                    "$request_one",
                    request_body("request_one", 250),
                )],
            ),
        );
        fake.next().await.json(200, session_state());
        fake.next().await.json(200, reception_state());
        fake.next().await.json(200, forged);
    })
    .await;
    assert_eq!(result, Err(Error::Generation));
    assert_eq!(rows(&f, "engagements"), before);
    assert!(f.available().await);
    assert_eq!(status(&c, &mut fake).await.stage, "quarantined");
    c.close().await.unwrap();
}

/// The provider approval is observed after the mint as the separate `approve`
/// write: the intake admits the request, then the decision event drives
/// `approve`, which reserves the engagement and records the provision effect.
#[tokio::test]
async fn native_provisioning_effect_produced() {
    let (f, mut fake, c) = ready_provisioning().await;
    let summary = run_provisioning(
        &c,
        &mut fake,
        provisioning_sync(
            "provision",
            vec![
                request_event("$request_one", request_body("request_one", 250)),
                approval_event("$approval_one", "request_one"),
            ],
        ),
    )
    .await
    .unwrap_or_else(|e| panic!("intake failed: {e:?}"));
    assert_eq!(summary.admitted, 2);
    assert_eq!(summary.replayed, 0);
    // The verdict produces the provision effect row; because the provision
    // completes synchronously in the same handoff (ADR-022 createAgent inline),
    // the produced row is already complete here. test_completed asserts the
    // full claim->complete lifecycle.
    assert_eq!(
        effect_row(&f),
        Some(("provision".into(), "complete".into()))
    );
    c.close().await.unwrap();
}

/// The approved engagement's provision effect is claimed and observed complete
/// by the wired effect path: after the approval verdict, the intake claims the
/// pending effect and reports the provision outcome, so the effects row
/// carries kind=provision state=complete and the engagement goes active.
#[tokio::test]
async fn native_provisioning_effect_completed() {
    let (f, mut fake, c) = ready_provisioning().await;
    let summary = run_provisioning(
        &c,
        &mut fake,
        provisioning_sync(
            "provision",
            vec![
                request_event("$request_one", request_body("request_one", 250)),
                approval_event("$approval_one", "request_one"),
            ],
        ),
    )
    .await
    .unwrap_or_else(|e| panic!("intake failed: {e:?}"));
    assert_eq!(summary.admitted, 2);
    assert_eq!(summary.replayed, 0);
    assert_eq!(
        effect_row(&f),
        Some(("provision".into(), "complete".into()))
    );
    c.close().await.unwrap();
}

/// The admitted engagement's project room binds a session route: after the
/// verdict completes the provision, the intake resolves a verified Matrix
/// session for the new engagement, so matrix_session_routes binds the project
/// room and the intake plan's session id resolves to it.
#[tokio::test]
async fn native_provisioning_session_route() {
    let (f, mut fake, c) = ready_provisioning().await;
    let summary = run_provisioning(
        &c,
        &mut fake,
        provisioning_sync(
            "provision",
            vec![
                request_event("$request_one", request_body("request_one", 250)),
                approval_event("$approval_one", "request_one"),
            ],
        ),
    )
    .await
    .unwrap_or_else(|e| panic!("intake failed: {e:?}"));
    assert_eq!(summary.admitted, 2);
    // The new engagement's project room is bound to a session route.
    let bound: Option<(String, String)> = rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3"))
        .unwrap()
        .query_row(
            "SELECT r.session_id,r.room_id FROM matrix_session_routes r JOIN runner_sessions s ON s.id=r.session_id JOIN engagements e ON e.id=s.engagement_id WHERE e.request_id='request_one'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    assert_eq!(
        bound.as_ref().map(|(_, room)| room.as_str()),
        Some(PROJECT),
        "expected a session route binding the provisioned project room"
    );
    c.close().await.unwrap();
}

/// A verdict from anyone but the fleet's representative is refused fail-closed:
/// approve_provision's sender check (intake.rs:301-304) rejects the event, the
/// handoff quarantines with the verification reason, and nothing is reserved.
#[tokio::test]
async fn native_provisioning_approval_refuses_a_non_representative_verdict() {
    let (f, mut fake, c) = ready_provisioning().await;
    let result = run_provisioning(
        &c,
        &mut fake,
        provisioning_sync(
            "provision",
            vec![
                request_event("$request_one", request_body("request_one", 250)),
                approval_event_from("$approval_one", "request_one", "@intruder:example.test"),
            ],
        ),
    )
    .await;
    assert_eq!(result, Err(Error::Generation));
    assert_eq!(status(&c, &mut fake).await.stage, "quarantined");
    assert_eq!(effect_row(&f), None, "no effect row for the refused verdict");
    assert_eq!(route_rows(&f), 0, "no session route for the refused verdict");
    assert!(f.available().await);
    c.close().await.unwrap();
}

/// A verdict naming a request id with no admitted engagement is refused
/// fail-closed: approve_provision's evidence read finds nothing
/// (.ok_or(Error::Wire) at intake.rs:307-310), the handoff quarantines, and no
/// engagement/effect/route row is created for it.
#[tokio::test]
async fn native_provisioning_approval_refuses_an_unknown_request_id() {
    let (f, mut fake, c) = ready_provisioning().await;
    let before_effects = rows(&f, "effects");
    let before_routes = rows(&f, "matrix_session_routes");
    let result = run_approval_only(
        &c,
        &mut fake,
        provisioning_sync(
            "provision",
            vec![approval_event("$approval_one", "no_such_request")],
        ),
    )
    .await;
    assert_eq!(result, Err(Error::Generation));
    assert_eq!(status(&c, &mut fake).await.stage, "quarantined");
    assert_eq!(rows(&f, "effects"), before_effects);
    assert_eq!(rows(&f, "matrix_session_routes"), before_routes);
    assert!(f.available().await);
    c.close().await.unwrap();
}

/// A second verdict for the same request id replays the recorded decision:
/// exactly one effect row remains in ('provision','complete'), no duplicate
/// engagement/effect/route is created, and the handoff counts it as a replay.
#[tokio::test]
async fn native_provisioning_approval_replays_a_second_verdict() {
    let (f, mut fake, c) = ready_provisioning().await;
    let before_effects = rows(&f, "effects");
    let before_routes = rows(&f, "matrix_session_routes");
    let before_engagements = rows(&f, "engagements");
    let summary = run_provisioning(
        &c,
        &mut fake,
        provisioning_sync(
            "provision",
            vec![
                request_event("$request_one", request_body("request_one", 250)),
                approval_event("$approval_one", "request_one"),
                approval_event("$approval_two", "request_one"),
            ],
        ),
    )
    .await
    .unwrap_or_else(|e| panic!("intake failed: {e:?}"));
    assert_eq!(summary.admitted, 2);
    assert_eq!(summary.replayed, 1);
    assert_eq!(rows(&f, "engagements"), before_engagements + 1);
    assert_eq!(rows(&f, "effects"), before_effects + 1);
    assert_eq!(rows(&f, "matrix_session_routes"), before_routes + 1);
    assert_eq!(effect_row(&f), Some(("provision".into(), "complete".into())));
    assert_eq!(route_rows(&f), 1);
    c.close().await.unwrap();
}
