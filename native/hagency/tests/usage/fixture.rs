use hagency::App;
use hagency_core::tasks::*;
use hagency_metering::{Framework, observation::UsageObservation};
use hagency_store::*;
use salvo::prelude::*;
use serde_json::json;

#[path = "../../../hagency-store/tests/common/mod.rs"]
mod common;
use common::*;

pub const TOKEN: &str = "fixture_operator_token_32_bytes_minimum";
pub const BASE: &str = "http://127.0.0.1:13300";
pub struct Fixture {
    pub service: Service,
    pub domain: DomainStore,
    pub custody: Store,
    pub engagement: String,
    _root: tempfile::TempDir,
}
impl Fixture {
    pub fn new(snapshots: &[(&str, u64)], source: bool, connected: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let custody = Store::start(Repository::open(&state).unwrap(), 16).unwrap();
        let mut db = DomainRepository::open(&state).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("private_usage_pool", "private_usage_seat", 1000);
        db.put_resource(&pool).unwrap();
        let proof = proof(&request("usage_request", "Worker", &pool, 100));
        let engagement = db.admit(&proof, 1000).unwrap().id;
        db.approve("approve", &proof, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "synthetic fixture".into(),
            },
        )
        .unwrap();
        if source {
            db.register_session(&SessionBinding {
                id: "private_session".into(),
                engagement_id: engagement.clone(),
                room_id: "!project:example.test".into(),
                thread_root: Some("$private_thread".into()),
            })
            .unwrap();
            db.create_canonical_task("private_task", "private_session", "Usage", 1001)
                .unwrap();
            db.register_workspace("private_workspace").unwrap();
            db.enqueue_dispatch(&DispatchInput {
                id: "private_dispatch".into(),
                session_id: "private_session".into(),
                task_id: Some("private_task".into()),
                resources: vec![ResourceLease {
                    id: "private_workspace".into(),
                    exclusive: true,
                }],
                payload: json!({}),
            })
            .unwrap();
            let cap = db
                .claim_dispatch("private_runner", 1002, 60000, 120000, 128)
                .unwrap()
                .unwrap();
            let scope = db.owned_dispatch_scope(&cap, 1003).unwrap();
            let started = db
                .start_owned_dispatch(&cap, scope.fingerprint(), 1004)
                .unwrap();
            let source = db.bind_usage_source(&cap, &started, 1005).unwrap();
            for (index, (text, at)) in snapshots.iter().enumerate() {
                let observation = UsageObservation::parse(Framework::Codex, text).unwrap();
                db.record_usage_observation(
                    &source,
                    &format!("observation_{index}"),
                    &observation,
                    *at,
                )
                .unwrap();
            }
        }
        let domain = DomainStore::start(db, 16).unwrap();
        let mut app = App::new(
            custody.clone(),
            TOKEN.as_bytes(),
            "127.0.0.1:13300".parse().unwrap(),
        )
        .unwrap();
        if connected {
            app = app.with_domain(domain.clone());
        }
        Self {
            service: Service::new(app.router()),
            domain,
            custody,
            engagement,
            _root: root,
        }
    }
    pub fn url(&self) -> String {
        format!("{BASE}/api/native/v1/engagements/{}/usage", self.engagement)
    }
    pub async fn close(self) {
        self.domain.shutdown().await.unwrap();
        self.custody.shutdown().await.unwrap();
    }
}
pub fn snapshot(input: u64, output: u64, cached: u64) -> String {
    json!({"payload":{"info":{"total_token_usage":{"input_tokens":input+cached,"output_tokens":output,"cached_input_tokens":cached,"reasoning_output_tokens":0,"total_tokens":input+cached+output}}}}).to_string()
}
