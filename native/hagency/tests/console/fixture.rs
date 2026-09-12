#![allow(dead_code)] // Shared by HTTP, process and real-browser selectors.
use hagency::{App, console::Console};
use hagency_core::tasks::*;
use hagency_metering::{Framework, observation::UsageObservation};
use hagency_store::*;
use salvo::prelude::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    net::SocketAddr,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[path = "../../../hagency-store/tests/common/mod.rs"]
pub mod common;
pub const TOKEN: &str = "fixture_operator_token_32_bytes_minimum";
pub const BASE: &str = "http://127.0.0.1:13300";
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
pub fn assets(path: &Path) {
    private::directory(path).unwrap();
    std::fs::create_dir(path.join("usage")).unwrap();
    let bytes = b"<!doctype html><html><body>retained asset fixture</body></html>";
    private::write_new(&path.join("usage/index.html"), bytes).unwrap();
    let digest: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    private::write_new(&path.join("manifest.json"), json!({"version":1,"assets":[{"path":"usage/index.html","size":bytes.len(),"sha256":digest,"mime":"text/html; charset=utf-8"}]}).to_string().as_bytes()).unwrap();
}
pub fn native_resource(preset: &str) -> hagency_core::project::Resource {
    serde_json::from_value(json!({"presetId":preset,"seatId":"private_resource_account","framework":"codex","model":"gpt-5.6-sol","reasoning":"medium","ceiling":{"tokens":5000,"period":"monthly"}})).unwrap()
}
pub fn seed(state: &Path) -> (DomainRepository, String) {
    let mut db = DomainRepository::open(state).unwrap();
    db.register(&common::registration()).unwrap();
    let pool = common::resource("private_usage_pool", "private_usage_seat", 1000);
    db.put_resource(&pool).unwrap();
    let proof = common::proof(&common::request("usage_request", "UsageWorker", &pool, 100));
    let engagement = db.admit(&proof, 1000).unwrap().id;
    db.approve("approve", &proof, 1000).unwrap();
    let effect = db.claim_effect().unwrap().unwrap();
    db.observe_effect(
        &effect.id,
        effect.fence,
        &EffectOutcome::Applied {
            receipt: "synthetic canonical fixture".into(),
        },
    )
    .unwrap();
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
    for (index, text) in [snapshot(7, 2, 3), String::new(), snapshot(4, 1, 1)]
        .iter()
        .enumerate()
    {
        let observation = UsageObservation::parse(Framework::Codex, text).unwrap();
        db.record_usage_observation(
            &source,
            &format!("observation_{index}"),
            &observation,
            now() - 10 + index as u64,
        )
        .unwrap();
    }
    (db, engagement)
}
fn snapshot(input: u64, output: u64, cached: u64) -> String {
    json!({"payload":{"info":{"total_token_usage":{"input_tokens":input+cached,"output_tokens":output,"cached_input_tokens":cached,"reasoning_output_tokens":0,"total_tokens":input+cached+output}}}}).to_string()
}
pub struct Fixture {
    pub app: App,
    pub console: Console,
    pub domain: DomainStore,
    pub custody: Store,
    pub engagement: String,
    pub root: tempfile::TempDir,
}
impl Fixture {
    pub fn new(address: SocketAddr, built: Option<&Path>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let asset_dir = root.path().join("assets");
        if built.is_none() {
            assets(&asset_dir);
        }
        // macOS's temporary-directory spelling may traverse the /var alias.
        // Select its actual host path; production correctly refuses that alias.
        let actual_assets = built.unwrap_or(&asset_dir).canonicalize().unwrap();
        let console = Console::load(&actual_assets).unwrap();
        let state = root.path().join("state");
        let custody = Store::start(Repository::open(&state).unwrap(), 16).unwrap();
        let (db, engagement) = seed(&state);
        let domain = DomainStore::start(db, 16).unwrap();
        let app = App::new(custody.clone(), TOKEN.as_bytes(), address)
            .unwrap()
            .with_domain(domain.clone())
            .with_console(console.clone());
        Self {
            app,
            console,
            domain,
            custody,
            engagement,
            root,
        }
    }
    pub fn service(&self) -> Service {
        Service::new(self.app.clone().router())
    }
    pub async fn close(self) {
        self.console.retire();
        self.domain.shutdown().await.unwrap();
        self.custody.shutdown().await.unwrap();
    }
    pub async fn new_engagement(&self) -> String {
        let pool = common::resource("private_usage_pool", "private_usage_seat", 1000);
        self.domain
            .admit(
                common::proof(&common::request(
                    "new_after_browser_build",
                    "NewUsageWorker",
                    &pool,
                    100,
                )),
                1000,
            )
            .await
            .unwrap()
            .id
    }
}
pub fn assert_private(value: &Value) {
    let text = value.to_string();
    for private in [
        TOKEN,
        "private_session",
        "private_task",
        "private_runner",
        "private_workspace",
        "private_usage",
        "roomId",
        "RoomId",
        "source_id",
        "snapshot_digest",
        "!private:example.test",
    ] {
        assert!(
            !text.contains(private),
            "unexpected private output {private}"
        );
    }
}

pub fn configuration_resource() -> hagency_core::project::Resource {
    native_resource("private_configuration_browser_source")
}
