use super::{crypto, matrix};
use hagency_core::{canonical, replies::*, tasks::SessionBinding};
use hagency_execution::{ApprovalHost, Limits, WarmHostPlan, WarmLimits, WarmTaskBridge};
use hagency_matrix::{
    ApprovalCollector, CancellationToken, Collector, HostApprovalConfig, HostConfig, HostIdentity,
    HostIntakePlan, HostRoom, TokenProvisioningHost,
};
use hagency_store::agent_home::{HomeProject, ManagedHomePlan, ProjectMode};
use salvo::Listener;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
pub const OWNER: &str = "@owner:example.test";
pub const PROJECT: &str = "!factory_project:example.test";
pub const DM: &str = "!factory_owner_dm:example.test";
const ROOT_ROOM: &str = "!bootstrap:example.test";
const REP_TOKEN: &str = "synthetic-separate-representative-token";
const HUMAN_TOKEN: &str = "synthetic-independent-owner-token";
const AGENT_TOKEN: &str = "actual-returned-synthetic-token";
const AS_TOKEN: &str = "synthetic-fixed-side-application-service-token";
const APPROVAL_TOKEN: &str = "synthetic-independent-approval-bot-token";
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
pub fn execution_limits() -> Limits {
    Limits {
        operation_ms: 30_000,
        response_ms: 2000,
    }
}
fn reg() -> hagency_core::authority::Registration {
    matrix::domain::registration()
}
fn engagement() -> String {
    format!(
        "en_{}",
        &format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&[reg().fleet_id, "factory_target".into()]).unwrap())
        )[..32]
    )
}
fn member(user: &str) -> Value {
    json!({"type":"m.room.member","state_key":user,"content":{"membership":"join"}})
}
fn rules(users: Value) -> Vec<Value> {
    vec![
        json!({"type":"m.room.join_rules","state_key":"","content":{"join_rule":"invite"}}),
        json!({"type":"m.room.power_levels","state_key":"","content":{"users":users,"users_default":0,"invite":0}}),
    ]
}
fn approval_config(base: &matrix::Fixture, endpoint: &str, anchor: String) -> HostApprovalConfig {
    let mut identity = base.identity.clone();
    identity.transport.sender_mxid = reg().approval_bot_mxid;
    identity.transport.device_id = "APPROVAL_DEVICE".into();
    HostApprovalConfig::new(
        HostConfig::new(
            identity,
            endpoint,
            APPROVAL_TOKEN,
            base.root.path().join("approval-sdk"),
            [85; 32],
            vec![HostRoom {
                room_id: "!private:example.test".into(),
                generation: 1,
                privacy: RoomPrivacy::Direct {
                    human_mxid: OWNER.into(),
                },
            }],
            matrix::factory_limits(),
        )
        .unwrap()
        .with_root_pem(include_bytes!(
            "../../../hagency-matrix/tests/fixtures/ca.pem"
        ))
        .unwrap(),
        vec![base.identity.transport.engagement_id.clone()],
    )
    .unwrap()
    .with_fresh_account_enrollment(vec![(OWNER.into(), anchor)])
    .unwrap()
}
pub struct Fixture {
    pub base: matrix::Fixture,
    pub fake: matrix::Fake,
    pub collector: Arc<Collector>,
    pub peer: Peer,
    pub fleet: Option<hagency::bootstrap::fleet::Service>,
    pub approvals: Arc<ApprovalCollector>,
    foreign: Option<hagency_store::DomainStore>,
    custody: hagency_store::Store,
    server: tokio::task::JoinHandle<()>,
    handle: salvo::server::ServerHandle,
    application_service: bool,
    service_mode: bool,
    pub address: std::net::SocketAddr,
}
impl Fixture {
    pub async fn new(application_service: bool) -> Self {
        Self::new_configured(application_service, false, false).await
    }
    pub async fn new_service(application_service: bool) -> Self {
        Self::new_configured(application_service, false, true).await
    }
    pub async fn foreign_approval_writer() -> Self {
        Self::new_configured(false, true, false).await
    }
    async fn new_configured(
        application_service: bool,
        foreign_writer: bool,
        service_mode: bool,
    ) -> Self {
        // An already-managed project and unrelated external bootstrap agent.
        // ONLY that external agent uses fixture activation. The target factory
        // never receives seeded agent/effect/session/current/approval rows.
        let root = tempfile::tempdir().unwrap();
        let mut repository =
            hagency_store::DomainRepository::open(&root.path().join("domain")).unwrap();
        repository.register(&reg()).unwrap();
        let pool = matrix::domain::resource("pool", "seat", 1000);
        repository.put_resource(&pool).unwrap();
        let mut request = matrix::domain::request("worker", "Worker", &pool, 100);
        request.target_project_id = "factory_project".into();
        request.target_room_id = PROJECT.into();
        let proof = matrix::domain::proof(&request);
        let external = repository.admit(&proof, 1000).unwrap();
        repository.approve("approve", &proof, 1000).unwrap();
        let effect = repository.claim_effect().unwrap().unwrap();
        repository
            .observe_effect(
                &effect.id,
                effect.fence,
                &hagency_store::EffectOutcome::Applied {
                    receipt: "preexisting external bootstrap fixture only".into(),
                },
            )
            .unwrap();
        let base = matrix::Fixture {
            root,
            store: hagency_store::DomainStore::start(repository, 32).unwrap(),
            identity: HostIdentity {
                server_name: "example.test".into(),
                registration_fingerprint: canonical::digest(&json!(reg())).unwrap(),
                transport: MatrixTransportObservation {
                    engagement_id: external.id,
                    registration_generation: 1,
                    generation: 1,
                    sender_mxid: "@worker:example.test".into(),
                    device_id: "DEVICE_1".into(),
                },
            },
        };
        for name in ["homes", "source-project", "contexts", "accounts"] {
            hagency_store::private::directory(&base.root.path().join(name)).unwrap();
        }
        fs::write(
            base.root.path().join("source-project/source.txt"),
            b"genuine offline factory source",
        )
        .unwrap();
        let fake = matrix::Fake::start(true).await;
        let peer = Peer::new(application_service, fake.endpoint.clone()).await;
        Self::assemble(
            base,
            fake,
            peer,
            application_service,
            foreign_writer,
            service_mode,
        )
        .await
    }
    /// Every in-memory owner over an existing state: the approval bot, the
    /// coordinator with its provisioner, the app and the fleet. A fresh fixture
    /// and a restarted one build the same way.
    async fn assemble(
        base: matrix::Fixture,
        fake: matrix::Fake,
        peer: Peer,
        application_service: bool,
        foreign_writer: bool,
        service_mode: bool,
    ) -> Self {
        let custody = hagency_store::Store::start(
            hagency_store::Repository::open(&base.root.path().join("runtime")).unwrap(),
            16,
        )
        .unwrap();
        let acceptor = salvo::conn::TcpListener::new("127.0.0.1:0")
            .try_bind()
            .await
            .unwrap();
        let address = acceptor.local_addr().unwrap();
        let mut app = hagency::App::new(
            custody.clone(),
            b"fixture_operator_token_32_bytes_minimum",
            address,
        )
        .unwrap()
        .with_domain(base.store.clone());
        let service = salvo::Server::new(acceptor);
        let handle = service.handle();
        let mut peer = peer;
        let approvals = Arc::new(
            ApprovalCollector::new(
                approval_config(&base, &fake.endpoint, peer.peer.anchor()),
                base.store.clone(),
            )
            .unwrap(),
        );
        let mut fake = fake;
        {
            let cancel = CancellationToken::new();
            let observe = approvals.observe(&cancel);
            tokio::pin!(observe);
            loop {
                tokio::select! {result=&mut observe=>{result.unwrap();break;},request=fake.next()=>peer.respond(request,&base).await}
            }
        }
        let foreign = foreign_writer.then(|| {
            hagency_store::DomainStore::start(
                hagency_store::DomainRepository::open(&base.root.path().join("foreign-domain"))
                    .unwrap(),
                16,
            )
            .unwrap()
        });
        let factory_approvals = if let Some(writer) = &foreign {
            Arc::new(
                ApprovalCollector::new(
                    approval_config(&base, &fake.endpoint, peer.peer.anchor()),
                    writer.clone(),
                )
                .unwrap(),
            )
        } else {
            approvals.clone()
        };
        let helper = PathBuf::from(env!("CARGO_BIN_EXE_hagency"))
            .canonicalize()
            .unwrap();
        let binary = PathBuf::from(env!("CARGO_BIN_EXE_hagency-owned-mcp-probe"))
            .canonicalize()
            .unwrap();
        let home = ManagedHomePlan::new(
            base.root.path().join("homes").canonicalize().unwrap(),
            vec![HomeProject {
                project_id: "factory_project".into(),
                source: base
                    .root
                    .path()
                    .join("source-project")
                    .canonicalize()
                    .unwrap(),
                mode: ProjectMode::Copy,
            }],
            helper.clone(),
        )
        .unwrap();
        let environment = BTreeMap::from([
            ("PATH".into(), "".into()),
            ("HAGENCY_OFFLINE_MODE".into(), "warm".into()),
        ]);
        #[cfg(windows)]
        let environment = {
            let mut environment = environment;
            environment.insert("SystemRoot".into(), std::env::var_os("SystemRoot").unwrap());
            environment
        };
        let bridge = WarmTaskBridge::new(
            helper,
            address,
            base.root.path().join("contexts").canonicalize().unwrap(),
        )
        .unwrap();
        let warm = WarmHostPlan::new(
            binary.clone(),
            binary,
            environment,
            bridge,
            ApprovalHost::new(2, 1, 1000, 2000).unwrap(),
            WarmLimits {
                initialize: Limits {
                    operation_ms: 3000,
                    response_ms: 2000,
                },
                idle_ms: 10_000,
            },
        )
        .unwrap();
        let warm = if service_mode {
            warm.with_file_access(1024, true, true).unwrap()
        } else {
            warm
        };
        let limits = matrix::factory_limits();
        let host = if application_service {
            TokenProvisioningHost::application_service(
                reg(),
                &fake.endpoint,
                hagency_matrix::ApplicationServiceCredential::new(
                    AS_TOKEN,
                    &format!("{}_", reg().fleet_id),
                )
                .unwrap(),
                base.root.path().join("accounts"),
                [73; 32],
                limits.clone(),
            )
        } else {
            TokenProvisioningHost::new(
                reg(),
                &fake.endpoint,
                "synthetic-registration-token",
                base.root.path().join("accounts"),
                [73; 32],
                limits.clone(),
            )
        }
        .unwrap()
        .with_root_pem(include_bytes!(
            "../../../hagency-matrix/tests/fixtures/ca.pem"
        ))
        .unwrap()
        .with_agent_rooms_enrollment(REP_TOKEN, vec![(OWNER.into(), peer.peer.anchor())])
        .unwrap()
        .with_managed_homes(home)
        .unwrap()
        .with_warm_runtime(warm, factory_approvals)
        .unwrap();
        let mut config = HostConfig::new(
            base.identity.clone(),
            &fake.endpoint,
            matrix::TOKEN,
            base.root.path().join("sdk"),
            [42; 32],
            vec![
                // The external bootstrap agent's existing owner DM is separate
                // from the target project whose members change during creation.
                HostRoom {
                    room_id: ROOT_ROOM.into(),
                    generation: 1,
                    privacy: RoomPrivacy::Direct {
                        human_mxid: OWNER.into(),
                    },
                },
            ],
            limits,
        )
        .unwrap()
        .with_root_pem(include_bytes!(
            "../../../hagency-matrix/tests/fixtures/ca.pem"
        ))
        .unwrap();
        config
            .with_reception_room(HostRoom {
                room_id: reg().reception_room_id,
                generation: 1,
                privacy: RoomPrivacy::Group {},
            })
            .unwrap();
        let config = config.with_token_account_provisioning(host).unwrap();
        let collector = Arc::new(Collector::new(config, base.store.clone()).unwrap());
        let fleet = service_mode.then(|| {
            hagency::bootstrap::fleet::Service::new(
                base.store.clone(),
                collector.clone(),
                hagency::bootstrap::fleet::Setup {
                    state: base.root.path().join("service-state"),
                    limit: 1024,
                    send: true,
                    receive: true,
                    limits: execution_limits(),
                },
            )
            .unwrap()
        });
        if let Some(fleet) = &fleet {
            app = app.with_fleet(fleet);
        }
        let server = tokio::spawn(async move {
            service.try_serve(app.router()).await.unwrap();
        });
        let mut f = Self {
            base,
            fake,
            collector,
            peer,
            approvals,
            foreign,
            custody,
            server,
            handle,
            fleet,
            application_service,
            service_mode,
            address,
        };
        {
            let cancel = CancellationToken::new();
            let operation = f.collector.collect(&cancel);
            tokio::pin!(operation);
            loop {
                tokio::select! {result=&mut operation=>{result.unwrap();break;},request=f.fake.next()=>f.peer.respond(request,&f.base).await}
            }
        }
        f.base
            .store
            .resolve_verified_matrix_session(SessionBinding {
                id: "root".into(),
                engagement_id: f.base.identity.transport.engagement_id.clone(),
                room_id: ROOT_ROOM.into(),
                thread_root: None,
            })
            .await
            .unwrap();
        f
    }
    /// The process restarts: every in-memory owner is closed and rebuilt over
    /// the same state, and the domain repository is reopened the way a real
    /// start reopens it. The fake homeserver and the owner's device keep their
    /// state, as the real ones would.
    pub async fn restart(mut self) -> Self {
        if let Some(fleet) = &mut self.fleet {
            fleet.close().await.unwrap();
        }
        if let Some(owner) = self.peer.owner_job.take() {
            owner.await.unwrap();
        }
        self.collector.close_provisioned_agents().await.unwrap();
        self.collector.close().await.unwrap();
        self.approvals.close().await.unwrap();
        self.handle.stop_graceful(Some(Duration::from_secs(1)));
        self.server.await.unwrap();
        self.base.store.shutdown().await.unwrap();
        self.custody.shutdown().await.unwrap();
        assert!(self.foreign.is_none());
        let Self {
            mut base,
            fake,
            peer,
            application_service,
            service_mode,
            ..
        } = self;
        base.store = hagency_store::DomainStore::start(
            hagency_store::DomainRepository::open(&base.root.path().join("domain")).unwrap(),
            32,
        )
        .unwrap();
        Self::assemble(base, fake, peer, application_service, false, service_mode).await
    }
    pub async fn provision(&mut self) {
        let plan = HostIntakePlan::new(vec!["root".into()]).unwrap();
        let cancel = CancellationToken::new();
        let operation = self.collector.intake(plan, &cancel);
        tokio::pin!(operation);
        loop {
            tokio::select! {result=&mut operation=>{
                match result {
                    Ok(summary)=>assert_eq!(summary.admitted,2),
                    Err(error)=>panic!("original factory intake failed: {error:?}; state={:?}; home_progress={}; native_entered={}; native_initialized={}; account_posts={}; agent_key_writes={}; approval_observations={}",
                        self.target_state(),self.home_progress(),self.work().join("owned-mcp.warm-entered").exists(),self.work().join("owned-mcp.warm-initialized").exists(),self.peer.account_posts,self.peer.peer.writes.len(),self.peer.approval_observations),
                }
                break;
            },request=self.fake.next()=>self.peer.respond(request,&self.base).await}
        }
    }
    pub async fn approval_card(
        &self,
        agent: &hagency_matrix::ProvisionedAgent,
    ) -> Arc<hagency_store::PrivateApprovalCard> {
        use hagency_core::{approvals::*, tasks::*};
        let session = agent.session().id.clone();
        let workspace = agent.workspace_id().to_owned();
        self.base
            .store
            .create_canonical_task(
                "approval_task".into(),
                session.clone(),
                "Factory private approval".into(),
                now(),
            )
            .await
            .unwrap();
        self.base
            .store
            .enqueue_dispatch(DispatchInput {
                id: "approval_dispatch".into(),
                session_id: session,
                task_id: Some("approval_task".into()),
                resources: vec![ResourceLease {
                    id: workspace.clone(),
                    exclusive: true,
                }],
                payload: json!({"instruction":"typed approval delivery fixture"}),
            })
            .await
            .unwrap();
        let cap = self
            .base
            .store
            .claim_owned_dispatch_for_host(
                agent.claim_profile().await.unwrap(),
                "approval_fixture".into(),
                60_000,
                60_000,
                2,
            )
            .await
            .unwrap()
            .unwrap();
        self.base
            .store
            .start_dispatch(cap.clone(), now())
            .await
            .unwrap();
        self.base
            .store
            .bind_approval_context(
                cap.clone(),
                HostApprovalContext {
                    id: "factory_context".into(),
                    connection_id: "factory_connection".into(),
                    thread_id: "factory_thread".into(),
                    turn_id: "factory_turn".into(),
                    workspace_resource: workspace,
                    workspace: self.work().to_str().unwrap().into(),
                    windows_paths: cfg!(windows),
                    environment_id: None,
                    may_write: true,
                    yolo: false,
                },
            )
            .await
            .unwrap();
        let request=self.base.store.request_owner_approval(cap,HostApprovalRequest {context_id:"factory_context".into(),upstream_id:ApprovalRpcId::Number(1),
            item_id:"factory_command".into(),method:"item/commandExecution/requestApproval".into(),
            params:json!({"threadId":"factory_thread","turnId":"factory_turn","itemId":"factory_command","command":"echo private factory request","cwd":self.work()}),
            expires_at:now()+50_000}).await.unwrap();
        Arc::new(
            self.base
                .store
                .private_approval_card(request.id, now() + 40_000)
                .await
                .unwrap(),
        )
    }
    pub async fn enroll_approvals(&mut self) {
        let approvals = self.approvals.clone();
        let cancel = CancellationToken::new();
        let enrollment = approvals.enroll_fresh_account(&cancel);
        tokio::pin!(enrollment);
        loop {
            tokio::select! {result=&mut enrollment=>{result.unwrap();break;},request=self.fake.next()=>self.peer.respond(request,&self.base).await}
        }
    }
    pub async fn deliver_approval(
        &mut self,
        card: Arc<hagency_store::PrivateApprovalCard>,
    ) -> hagency_matrix::PrivateApprovalDeliverySummary {
        self.enroll_approvals().await;
        let approvals = self.approvals.clone();
        let cancel = CancellationToken::new();
        let send = approvals.send_private_approval_card(card, &cancel);
        tokio::pin!(send);
        loop {
            tokio::select! {result=&mut send=>break result.unwrap(),request=self.fake.next()=>self.peer.respond(request,&self.base).await}
        }
    }
    pub fn engagement(&self) -> String {
        engagement()
    }
    pub fn work(&self) -> PathBuf {
        self.base
            .root
            .path()
            .join(format!("homes/agents/agent_{}/workdir", engagement()))
    }
    fn home_progress(&self) -> Value {
        // Observation-only fixture diagnostics: no credentials, file content,
        // replacement readiness or retry authority is emitted.
        let work = self.work();
        let home = work.parent().unwrap();
        json!({"workdir":work.exists(),"binding":home.join("state/home-binding").exists(),
            "project_copy_bytes":fs::metadata(work.join("projects/factory_project/source.txt")).ok().map(|value|value.len()),
            "first_entry":work.join("CLAUDE.md").exists(),"entry":work.join("AGENTS.md").exists(),"manifest":home.join("agent.json").exists(),
            "complete":self.base.root.path().join(format!("homes/custody/home-{}/complete",engagement())).exists()})
    }
    pub fn count(&self, sql: &str) -> u64 {
        rusqlite::Connection::open(self.base.root.path().join("domain/domain.sqlite3"))
            .unwrap()
            .query_row(sql, [], |r| r.get(0))
            .unwrap()
    }
    pub fn target_state(&self) -> (String, String) {
        rusqlite::Connection::open(self.base.root.path().join("domain/domain.sqlite3")).unwrap().query_row(
        "SELECT f.state,e.state FROM effects f JOIN engagements e ON e.id=f.engagement_id WHERE e.request_id='factory_target' AND f.kind='provision'",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap()
    }
    pub fn provision_receipt(&self) -> (u64, String) {
        rusqlite::Connection::open(self.base.root.path().join("domain/domain.sqlite3")).unwrap().query_row(
        "SELECT f.fence,f.outcome_digest FROM effects f JOIN engagements e ON e.id=f.engagement_id WHERE e.request_id='factory_target' AND f.kind='provision'",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap()
    }
    pub fn task_status(&self) -> String {
        rusqlite::Connection::open(self.base.root.path().join("domain/domain.sqlite3")).unwrap().query_row("SELECT json_extract(config,'$.status') FROM canonical_tasks WHERE id='factory_task'",[],|r|r.get(0)).unwrap()
    }
    pub fn authority_snapshot(&self) -> Value {
        let sql = rusqlite::Connection::open(self.base.root.path().join("domain/domain.sqlite3"))
            .unwrap();
        // A terminal dispatch clears these nullable deadlines. Preserve null
        // in diagnostics rather than panicking before the original report is
        // observed or treating missing custody as positively current.
        let (state,lease,expiry):(String,Option<bool>,Option<bool>)=sql.query_row("SELECT state,lease_until>?1,capability_until>?1 FROM runner_dispatches WHERE id='factory_dispatch'",[now()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
        let quarantined: bool = sql
            .query_row(
                "SELECT quarantined FROM runner_sessions WHERE id=?1",
                [format!("session_{}", engagement())],
                |r| r.get(0),
            )
            .unwrap();
        let current: bool = sql
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM current_matrix_routes WHERE session_id=?1)",
                [format!("session_{}", engagement())],
                |r| r.get(0),
            )
            .unwrap();
        json!({"dispatch":state,"lease_current":lease,"capability_current":expiry,"quarantined":quarantined,"route_current":current})
    }
    pub fn requests(&self) -> Vec<Value> {
        fs::read_to_string(self.work().join("owned-mcp.requests"))
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }
    pub fn receipt(&self, stage: &str) -> Value {
        serde_json::from_slice(&fs::read(self.work().join(format!("owned-mcp.{stage}"))).unwrap())
            .unwrap()
    }
    pub async fn original_owner(&self) -> Value {
        tokio::time::timeout(Duration::from_secs(4), async {
            loop {
                match fs::read(self.work().join("owned-mcp.warm-initialized")) {
                    Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                        Ok(value) => {
                            assert!(value["pid"].as_u64().is_some_and(|pid| pid > 1));
                            break value;
                        }
                        Err(error) if error.is_eof() => {}
                        Err(_) => panic!("original initialization metadata malformed"),
                    },
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => panic!("original initialization metadata unreadable"),
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap()
    }
    pub async fn close(mut self) {
        if let Some(fleet) = &mut self.fleet {
            fleet.close().await.unwrap();
        }
        if let Some(owner) = self.peer.owner_job {
            owner.await.unwrap();
        }
        self.collector.close_provisioned_agents().await.unwrap();
        self.collector.close().await.unwrap();
        self.approvals.close().await.unwrap();
        self.handle.stop_graceful(Some(Duration::from_secs(1)));
        self.server.await.unwrap();
        self.base.store.shutdown().await.unwrap();
        self.custody.shutdown().await.unwrap();
        self.fake.close().await;
        if let Some(foreign) = self.foreign {
            foreign.shutdown().await.unwrap();
        }
    }
}
pub struct Peer {
    user: String,
    device: String,
    endpoint: String,
    pub peer: crypto::Peer,
    pub approval_peer: crypto::Peer,
    root_sync: usize,
    pub created: bool,
    pub invited: bool,
    pub joined: bool,
    pub owner: bool,
    pub posts: usize,
    pub approval_owner: bool,
    pub approval_observations: usize,
    application_service: bool,
    as_created: bool,
    as_logged: bool,
    pub account_posts: usize,
    owner_job: Option<tokio::task::JoinHandle<()>>,
    /// Hold the owner's join back, to exercise the wait for a slow human.
    pub owner_join_delay: Option<Duration>,
}
impl Peer {
    async fn new(application_service: bool, endpoint: String) -> Self {
        let user = format!("@{}_{}:example.test", reg().fleet_id, engagement());
        let device = format!("DEVICE_{}", engagement());
        let mut peer = crypto::Peer::for_sender(&user, &device).await;
        let approval_peer = peer.additional_sender(&reg().approval_bot_mxid, "APPROVAL_DEVICE");
        Self {
            user,
            device,
            endpoint,
            peer,
            approval_peer,
            root_sync: 0,
            created: false,
            invited: false,
            joined: false,
            owner: false,
            posts: 0,
            approval_owner: true,
            approval_observations: 0,
            application_service,
            as_created: false,
            as_logged: false,
            account_posts: 0,
            owner_job: None,
            owner_join_delay: None,
        }
    }
    fn project(&self) -> Value {
        let mut events = vec![
            member(OWNER),
            member(&reg().representative_mxid),
            member("@worker:example.test"),
        ];
        events.extend(rules(json!({OWNER:100,reg().representative_mxid:50})));
        events.push(json!({"type":"com.hagency.project.binding.v1","state_key":"","content":{"v":1,"purpose":"project","authVersion":1,
            "fleetId":reg().fleet_id,"projectId":"factory_project","ownerMxid":OWNER}}));
        if self.invited {
            events.push(json!({"type":"m.room.member","state_key":self.user,"content":{"membership":if self.joined {"join"} else {"invite"}}}));
        }
        json!(events)
    }
    fn dm(&self) -> Value {
        assert!(self.created);
        json!([
            member(&self.user),{"type":"m.room.member","state_key":OWNER,"content":{"membership":if self.owner {"join"} else {"invite"}}},
            {"type":"m.room.join_rules","state_key":"","content":{"join_rule":"invite"}},
            {"type":"m.room.encryption","state_key":"","content":{"algorithm":"m.megolm.v1.aes-sha2"}},
            {"type":"m.room.create","state_key":"","sender":self.user,"content":{"creator":self.user,"m.federate":false}},
            {"type":"m.room.history_visibility","state_key":"","content":{"history_visibility":"invited"}}
        ])
    }
    pub async fn respond(&mut self, request: matrix::Request, base: &matrix::Fixture) {
        let actor = request.headers.get("authorization").cloned();
        let body = if request.body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&request.body).unwrap()
        };
        let url =
            reqwest::Url::parse(&format!("https://synthetic.test{}", request.target)).unwrap();
        let response = if actor == Some(format!("Bearer {APPROVAL_TOKEN}")) {
            if url.path().ends_with("/whoami") {
                (
                    200,
                    json!({"user_id":reg().approval_bot_mxid,"device_id":"APPROVAL_DEVICE","is_guest":false}),
                )
            } else if url.path().ends_with("/state") {
                assert_eq!(request.method, "GET");
                assert!(request.target.contains("private"));
                self.approval_observations += 1;
                (
                    200,
                    json!([
                        {"type":"m.room.member","state_key":OWNER,"content":{"membership":if self.approval_owner {"join"} else {"invite"}}},
                        member(&reg().approval_bot_mxid),
                        {"type":"m.room.join_rules","state_key":"","content":{"join_rule":"invite"}},
                        {"type":"m.room.encryption","state_key":"","content":{"algorithm":"m.megolm.v1.aes-sha2"}}
                    ]),
                )
            } else if url.path().ends_with("/sync") {
                (
                    200,
                    json!({"next_batch":"factory-approval-sync","rooms":{"join":{"!private:example.test":{"timeline":{"events":[],"limited":false},"state":{"events":[]}}}},"to_device":{"events":[]}}),
                )
            } else if request.method == "PUT" && url.path().contains("/sendToDevice/") {
                self.approval_peer.share(body).await;
                (200, json!({}))
            } else if request.method == "PUT" && url.path().contains("/send/") {
                assert!(
                    request.target.contains("private") && url.path().contains("/m.room.encrypted/")
                );
                self.approval_peer
                    .decrypt(body, "!private:example.test".try_into().unwrap())
                    .await;
                (200, json!({"event_id":"$factory_private_card"}))
            } else {
                self.approval_peer
                    .protocol(&request.method, &request.target, &body)
                    .await
                    .expect("original approval SDK request")
            }
        } else if actor == Some(format!("Bearer {}", matrix::TOKEN)) {
            if url.path().ends_with("/whoami") {
                (200, matrix::who())
            } else if url.path().ends_with("/sync") {
                let events = if self.root_sync == 0 {
                    vec![]
                } else {
                    vec![
                        json!({"event_id":"$factory_request","sender":OWNER,"type":"m.room.message","origin_server_ts":now(),"content":{"msgtype":"com.hagency.engagement.request.v1","body":json!({
                        "requestId":"factory_target","requester":OWNER,"project":"factory_project","projectRoomId":PROJECT,"role":"coding","requestedTokens":250,
                        "ratePerDay":null,"agent":"FactoryAgent","context":{"agentDefinition":{"resourceId":"resource_27cac5503836765cd10751d2"}}}).to_string()}}),
                        json!({"event_id":"$factory_approve","sender":reg().representative_mxid,"type":"m.room.message","origin_server_ts":now(),"content":{"msgtype":"com.hagency.engagement.approval.v1","body":json!({"requestId":"factory_target","decision":"approve"}).to_string()}}),
                    ]
                };
                self.root_sync += 1;
                (
                    200,
                    json!({"next_batch":format!("factory-root-{}",self.root_sync),"rooms":{"join":{
                    ROOT_ROOM:{"timeline":{"events":[],"limited":false},"state":{"events":[]}},
                    "!reception:example.test":{"timeline":{"events":events,"limited":false},"state":{"events":[]}}}},"to_device":{"events":[]}}),
                )
            } else {
                assert!(url.path().ends_with("/state"));
                if request.target.contains("factory_project") {
                    (200, self.project())
                } else {
                    let mut events = vec![member("@worker:example.test"), member(OWNER)];
                    if request.target.contains("reception") {
                        events.push(member(&reg().representative_mxid));
                    }
                    if request.target.contains("bootstrap") {
                        events.push(json!({"type":"m.room.encryption","state_key":"","content":{"algorithm":"m.megolm.v1.aes-sha2"}}));
                    }
                    events.extend(rules(json!({})));
                    (200, json!(events))
                }
            }
        } else if actor == Some(format!("Bearer {AS_TOKEN}")) {
            assert!(self.application_service);
            if url.path().ends_with("/whoami") {
                match url
                    .query_pairs()
                    .find(|(k, _)| k == "user_id")
                    .map(|(_, v)| v.into_owned())
                {
                    None => (
                        200,
                        json!({"user_id":reg().representative_mxid,"is_guest":false}),
                    ),
                    Some(user) if user == self.user => {
                        assert!(self.as_created);
                        (200, json!({"user_id":self.user,"is_guest":false}))
                    }
                    Some(user) => {
                        assert!(user.starts_with("@hagency_namespace_probe_"));
                        (403, json!({"errcode":"M_EXCLUSIVE"}))
                    }
                }
            } else {
                assert_eq!(request.method, "POST");
                assert_eq!(body["type"], "m.login.application_service");
                self.account_posts += 1;
                if url.path().ends_with("/register") {
                    assert!(!self.as_created);
                    assert_eq!(body["inhibit_login"], true);
                    self.as_created = true;
                    (200, json!({"user_id":self.user}))
                } else {
                    assert!(url.path().ends_with("/login") && self.as_created && !self.as_logged);
                    assert_eq!(body["device_id"], self.device);
                    self.as_logged = true;
                    (
                        200,
                        json!({"user_id":self.user,"device_id":self.device,"access_token":AGENT_TOKEN}),
                    )
                }
            }
        } else if actor.is_none() {
            assert!(!self.application_service && self.account_posts == 0);
            assert_eq!(request.method, "POST");
            assert!(url.path().ends_with("/register"));
            assert_eq!(
                body["username"],
                self.user.trim_start_matches('@').split_once(':').unwrap().0
            );
            assert_eq!(body["device_id"], self.device);
            assert_eq!(body["auth"]["type"], "m.login.registration_token");
            self.account_posts += 1;
            assert!(
                base.root
                    .path()
                    .join(format!("homes/agents/agent_{}/agent.json", engagement()))
                    .exists()
            );
            (
                200,
                json!({"user_id":self.user,"device_id":self.device,"access_token":AGENT_TOKEN}),
            )
        } else {
            let rep = actor == Some(format!("Bearer {REP_TOKEN}"));
            let human = actor == Some(format!("Bearer {HUMAN_TOKEN}"));
            assert!(rep || human || actor == Some(format!("Bearer {AGENT_TOKEN}")));
            if url.path().ends_with("/whoami") {
                assert!(!human);
                (
                    200,
                    if rep {
                        json!({"user_id":reg().representative_mxid,"device_id":"REP_DEVICE","is_guest":false})
                    } else {
                        json!({"user_id":self.user,"device_id":self.device,"is_guest":false})
                    },
                )
            } else if url.path().ends_with("/state") {
                if request.target.contains("factory_project") {
                    (200, self.project())
                } else {
                    assert!(self.created && !rep && !human);
                    (200, self.dm())
                }
            } else if url.path().ends_with("/createRoom") {
                assert!(!rep && !human && !self.created);
                assert_eq!(body["invite"], json!([OWNER]));
                self.created = true;
                self.posts += 1;
                (200, json!({"room_id":DM}))
            } else if url.path().ends_with("/invite") {
                assert!(rep && self.created && !self.invited);
                assert_eq!(body["user_id"], self.user);
                self.invited = true;
                self.posts += 1;
                (200, json!({}))
            } else if url.path().contains("/join/") {
                if human {
                    assert!(self.created && !self.owner);
                    self.owner = true;
                    (200, json!({"room_id":DM}))
                } else {
                    assert!(!rep && self.invited && !self.joined);
                    self.joined = true;
                    self.posts += 1;
                    (200, json!({"room_id":PROJECT}))
                }
            } else if url.path().ends_with("/sync") {
                assert!(!rep && !human && self.owner && self.joined);
                (
                    200,
                    json!({"next_batch":"factory-agent-active","rooms":{"join":{}},"to_device":{"events":[]}}),
                )
            } else {
                assert!(!rep && !human && self.created && self.owner && self.joined);
                self.peer
                    .protocol(&request.method, &request.target, &body)
                    .await
                    .expect("original SDK request")
            }
        };
        // The owner joins the new DM once; after a restart they are already in it.
        let invite_owner = request.target.ends_with("/state")
            && request.target.contains("factory_owner_dm")
            && !self.owner
            && self.owner_job.is_none();
        request.json(response.0, response.1);
        if invite_owner {
            let endpoint = self.endpoint.clone();
            let delay = self.owner_join_delay;
            self.owner_job = Some(tokio::spawn(async move {
                if let Some(delay) = delay {
                    tokio::time::sleep(delay).await;
                }
                join_owner(endpoint).await
            }));
        }
    }
}
async fn join_owner(endpoint: String) {
    // Independent actual owner HTTP client, never a factory membership setter.
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .add_root_certificate(
            reqwest::Certificate::from_pem(include_bytes!(
                "../../../hagency-matrix/tests/fixtures/ca.pem"
            ))
            .unwrap(),
        )
        .timeout(Duration::from_secs(4))
        .build()
        .unwrap();
    let mut url = reqwest::Url::parse(&endpoint).unwrap();
    url.path_segments_mut()
        .unwrap()
        .extend(["_matrix", "client", "v3", "join", DM]);
    let response = client
        .post(url)
        .bearer_auth(HUMAN_TOKEN)
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}
