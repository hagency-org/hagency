//! The two-engagement fixture (MA-M8a, ADR-144), beside the single-
//! engagement `Fixture::new()` every existing harness depends on. Two
//! admitted+approved engagements on two resources whose SEATS differ
//! (unmanaged, exactly as `new()` runs today), two transport identities,
//! ONE shared delivery room that is never an approval room, and one direct
//! room per engagement.
use super::*;

pub const SHARED_ROOM: &str = "!shared:example.test";
pub const DM_A: &str = "!dm-a:example.test";
pub const DM_B: &str = "!dm-b:example.test";
pub const OWNER: &str = "@owner:example.test";

pub struct PairFixture {
    pub root: tempfile::TempDir,
    pub store: DomainStore,
    pub a: HostIdentity,
    pub b: HostIdentity,
}
impl PairFixture {
    /// Two engagements, two seats, two identities. The shared room is a
    /// DELIVERY room only — never an approval room — and each engagement
    /// owns one direct room for its DMs with the owner.
    pub fn new_pair() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("domain")).unwrap();
        db.register(&domain::registration()).unwrap();
        let pool_a = domain::resource("pool-a", "seat-a", 1000);
        let pool_b = domain::resource("pool-b", "seat-b", 1000);
        db.put_resource(&pool_a).unwrap();
        db.put_resource(&pool_b).unwrap();
        let proof_a = domain::proof(&domain::request("worker", "Worker A", &pool_a, 100));
        let proof_b = domain::proof(&domain::request("helper", "Worker B", &pool_b, 100));
        // The two admissions must be distinct under the fleet: `admit`
        // refuses a collision on (fleet_id, project_id, name), so a shared
        // name with the same fleet/project returns Conflict on the second
        // admit. Distinct names + distinct request ids are the pair's own
        // identity requirement; assert the resulting engagement ids differ
        // before any further setup.
        let e_a = db.admit(&proof_a, 1000).unwrap();
        let e_b = db.admit(&proof_b, 1000).unwrap();
        assert_ne!(
            e_a.id, e_b.id,
            "the two engagements must resolve to distinct ids"
        );
        db.approve("approve_a", &proof_a, 1000).unwrap();
        db.approve("approve_b", &proof_b, 1000).unwrap();
        // Drive each engagement's provision effect explicitly — claim, assert
        // the state we just drove (Started) and that it belongs to one of
        // our two engagements, then settle it by its own id. `claim_effect`
        // returns effects in HASH order (`ORDER BY f.id`), never admit order,
        // so the identity check is by membership + set coverage, not loop
        // position. The hosted evidence (probe/lane-c run 34761248905)
        // showed a blind loop asserting a state the leg never reached; here
        // every state is asserted only after being driven, and a leg that
        // cannot produce an effect for both engagements fails by name, not
        // by order.
        let mut settled = std::collections::BTreeSet::new();
        for _ in 0..2 {
            let effect = db
                .claim_effect()
                .unwrap()
                .unwrap_or_else(|| panic!("no pending provision effect remains"));
            assert!(
                effect.engagement_id == e_a.id || effect.engagement_id == e_b.id,
                "the claimed effect must belong to one of the two engagements"
            );
            assert_eq!(effect.state, EffectState::Started);
            db.observe_effect(
                &effect.id,
                effect.fence,
                &EffectOutcome::Applied {
                    receipt: "fixture account".into(),
                },
            )
            .unwrap();
            assert!(
                settled.insert(effect.engagement_id.clone()),
                "each engagement must contribute exactly one provision effect"
            );
        }
        assert_eq!(settled.len(), 2, "both engagements' effects must settle");
        let identity = |engagement: String, mxid: &str, device: &str| HostIdentity {
            server_name: "example.test".into(),
            registration_fingerprint: "a".repeat(64),
            transport: MatrixTransportObservation {
                engagement_id: engagement,
                registration_generation: 1,
                generation: 1,
                sender_mxid: mxid.into(),
                device_id: device.into(),
            },
        };
        Self {
            store: DomainStore::start(db, 32).unwrap(),
            a: identity(e_a.id, "@worker:example.test", "DEVICE_1"),
            b: identity(e_b.id, "@helper:example.test", "DEVICE_2"),
            root,
        }
    }
    /// One host configuration per agent: the shared delivery room plus that
    /// agent's own direct room, on its own SDK store path.
    pub fn config(&self, agent: &HostIdentity, direct_room: &str, endpoint: &str) -> HostConfig {
        HostConfig::new(
            agent.clone(),
            endpoint,
            TOKEN,
            self.root
                .path()
                .join(format!("sdk-{}", agent.transport.device_id.to_lowercase())),
            [42; 32],
            vec![
                HostRoom {
                    room_id: SHARED_ROOM.into(),
                    generation: 1,
                    privacy: RoomPrivacy::Group {},
                },
                HostRoom {
                    room_id: direct_room.into(),
                    generation: 1,
                    privacy: RoomPrivacy::Direct {
                        human_mxid: OWNER.into(),
                    },
                },
            ],
            limits(),
        )
        .unwrap()
    }
}
/// The whoami answer for one agent — the collector enforces the EXACT
/// sender mxid and device, so an answer naming the other agent is the
/// ambiguous-sender refusal.
pub fn who(agent: &HostIdentity) -> Value {
    json!({
        "user_id": agent.transport.sender_mxid,
        "device_id": agent.transport.device_id,
        "is_guest": false
    })
}
/// Room state for one agent's view of a room: itself, the owner, invite
/// rules and encryption — the same shape `state()` pins for one agent.
pub fn state_for(agent: &HostIdentity) -> Value {
    json!([
     {"type":"m.room.member","state_key":agent.transport.sender_mxid,"content":{"membership":"join"}},
     {"type":"m.room.member","state_key":OWNER,"content":{"membership":"join"}},
     {"type":"m.room.join_rules","state_key":"","content":{"join_rule":"invite"}},
     {"type":"m.room.encryption","state_key":"","content":{"algorithm":"m.megolm.v1.aes-sha2"}}
    ])
}
/// The unencrypted variant, for plain shared-room legs.
pub fn state_plain_for(agent: &HostIdentity) -> Value {
    state_for(agent)
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] != "m.room.encryption")
        .cloned()
        .collect::<Vec<_>>()
        .into()
}
