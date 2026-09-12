use super::*;
// This independent suite reuses the existing executable peer source without
// exposing the ordinary enrollment test module as production crate API.
#[allow(clippy::duplicate_mod)]
#[path = "../../../hagency/tests/fixtures/matrix_crypto_peer.rs"]
pub mod crypto;
pub const BOT: &str = "@approval:example.test";
pub const DEVICE: &str = "BOT_DEVICE";
pub const ROOM: &str = "!private:example.test";
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
/// Hosted runners execute the whole matrix crate in parallel; these deliveries
/// then exceeded the shared 900 ms request budget while passing ten of ten
/// serial probe iterations on the same runner. No delivery scenario expects an
/// HTTP timeout, so this suite keeps its own fixture budgets. Production
/// limits are unchanged.
pub fn limits() -> hagency_matrix::Limits {
    hagency_matrix::Limits {
        connect: std::time::Duration::from_secs(2),
        headers: std::time::Duration::from_secs(2),
        request: std::time::Duration::from_secs(4),
        body_idle: std::time::Duration::from_secs(1),
        sdk: std::time::Duration::from_secs(20),
        ..hagency_matrix::Limits::default()
    }
}
pub fn who() -> Value {
    json!({"user_id":BOT,"device_id":DEVICE,"is_guest":false})
}
pub fn room() -> Value {
    json!([
    {"type":"m.room.member","state_key":crypto::HUMAN,"content":{"membership":"join"}},
    {"type":"m.room.member","state_key":BOT,"content":{"membership":"join"}},
    {"type":"m.room.join_rules","state_key":"","content":{"join_rule":"invite"}},
    {"type":"m.room.encryption","state_key":"","content":{"algorithm":"m.megolm.v1.aes-sha2"}}
    ])
}
pub fn config(
    base: &common::Fixture,
    fake: &common::Fake,
    anchor: Option<String>,
) -> HostApprovalConfig {
    let config = HostConfig::new(
        HostIdentity {
            server_name: "example.test".into(),
            registration_fingerprint: "a".repeat(64),
            transport: MatrixTransportObservation {
                engagement_id: base.identity.transport.engagement_id.clone(),
                registration_generation: 1,
                generation: 1,
                sender_mxid: BOT.into(),
                device_id: DEVICE.into(),
            },
        },
        &fake.endpoint,
        common::TOKEN,
        base.root.path().join("approval-sdk"),
        [42; 32],
        vec![HostRoom {
            room_id: ROOM.into(),
            generation: 1,
            privacy: RoomPrivacy::Direct {
                human_mxid: crypto::HUMAN.into(),
            },
        }],
        limits(),
    )
    .unwrap()
    .with_root_pem(include_bytes!("../fixtures/ca.pem"))
    .unwrap();
    let c = HostApprovalConfig::new(config, vec![base.identity.transport.engagement_id.clone()])
        .unwrap();
    match anchor {
        Some(key) => c
            .with_fresh_account_enrollment(vec![(crypto::HUMAN.into(), key)])
            .unwrap(),
        None => c,
    }
}
pub struct Fixture {
    pub base: common::Fixture,
    pub fake: common::Fake,
    pub peer: crypto::Peer,
    pub collector: ApprovalCollector,
    pub cap: RunnerCapability,
}
impl Fixture {
    pub async fn new() -> Self {
        let base = common::Fixture::new();
        // Legitimate Agent task scope is independent from the approval bot SDK.
        // The bot never observes or adopts this Agent transport as its own.
        base.store
            .observe_matrix_transport(base.identity.transport.clone())
            .await
            .unwrap();
        base.store
            .observe_matrix_room(MatrixRoomObservation {
                engagement_id: base.identity.transport.engagement_id.clone(),
                registration_generation: 1,
                transport_generation: 1,
                room_id: "!project:example.test".into(),
                generation: 1,
                privacy: RoomPrivacy::Group {},
                joined: std::collections::BTreeSet::from([
                    "@worker:example.test".into(),
                    crypto::HUMAN.into(),
                ]),
                invite_only: true,
                encrypted: false,
            })
            .await
            .unwrap();
        base.store
            .resolve_verified_matrix_session(SessionBinding {
                id: "root".into(),
                engagement_id: base.identity.transport.engagement_id.clone(),
                room_id: "!project:example.test".into(),
                thread_root: None,
            })
            .await
            .unwrap();
        base.store
            .create_canonical_task(
                "task".into(),
                "root".into(),
                "Private approval fixture".into(),
                now(),
            )
            .await
            .unwrap();
        base.store
            .register_workspace("workspace".into())
            .await
            .unwrap();
        base.store
            .enqueue_dispatch(DispatchInput {
                id: "dispatch".into(),
                session_id: "root".into(),
                task_id: Some("task".into()),
                resources: vec![ResourceLease {
                    id: "workspace".into(),
                    exclusive: true,
                }],
                payload: json!({"instruction":"fixture"}),
            })
            .await
            .unwrap();
        let cap = base
            .store
            .claim_dispatch("runner".into(), now(), 60_000, 120_000, 1)
            .await
            .unwrap()
            .unwrap();
        base.store.start_dispatch(cap.clone(), now()).await.unwrap();
        let fake = common::Fake::start(true).await;
        let peer = crypto::Peer::for_sender(BOT, DEVICE).await;
        let collector = ApprovalCollector::new(
            config(&base, &fake, Some(peer.anchor())),
            base.store.clone(),
        )
        .unwrap();
        let mut f = Self {
            base,
            fake,
            peer,
            collector,
            cap,
        };
        drive(
            f.collector.observe(&CancellationToken::new()),
            &mut f.fake,
            &mut f.peer,
        )
        .await
        .unwrap();
        f.base
            .store
            .bind_approval_context(
                f.cap.clone(),
                HostApprovalContext {
                    id: "context".into(),
                    connection_id: "connection".into(),
                    thread_id: "thread".into(),
                    turn_id: "turn".into(),
                    workspace_resource: "workspace".into(),
                    workspace: "/fixture/workspace".into(),
                    windows_paths: false,
                    environment_id: None,
                    may_write: true,
                    yolo: false,
                },
            )
            .await
            .unwrap();
        f
    }
    pub async fn enroll(&mut self) -> Result<(), Error> {
        let result = drive(
            self.collector
                .enroll_fresh_account(&CancellationToken::new()),
            &mut self.fake,
            &mut self.peer,
        )
        .await;
        if let Err(error) = result {
            eprintln!(
                "original approval enrollment: error={error:?} key_writes={} claims={} shares={} room_events={}",
                self.peer.writes.len(),
                self.peer.claims,
                self.peer.shares,
                self.peer.events.len()
            );
        }
        result
    }
    pub async fn card(&self, id: u64, reusable: bool) -> Arc<PrivateApprovalCard> {
        let item = format!("item{id}");
        let input = HostApprovalRequest {
            context_id: "context".into(),
            upstream_id: ApprovalRpcId::Number(id),
            item_id: item.clone(),
            method: if reusable {
                "item/commandExecution/requestApproval"
            } else {
                "unknown/requestApproval"
            }
            .into(),
            params: json!({"threadId":"thread","turnId":"turn","itemId":item,"command":"echo private approval","cwd":"/fixture/workspace"}),
            expires_at: now() + 50_000,
        };
        let approval = self
            .base
            .store
            .request_owner_approval(self.cap.clone(), input)
            .await
            .unwrap();
        Arc::new(
            self.base
                .store
                .private_approval_card(approval.id, now() + 40_000)
                .await
                .unwrap(),
        )
    }
    pub async fn send(
        &mut self,
        card: Arc<PrivateApprovalCard>,
    ) -> Result<PrivateApprovalDeliverySummary, Error> {
        drive(
            self.collector
                .send_private_approval_card(card, &CancellationToken::new()),
            &mut self.fake,
            &mut self.peer,
        )
        .await
    }
    pub async fn close(self) {
        let result = self.collector.close().await;
        assert!(result.is_ok(), "original close: {result:?}");
        common::shutdown_domain(&self.base.store, "private-card-final").await;
        self.fake.close().await;
    }
}
pub async fn respond(request: common::Request, peer: &mut crypto::Peer) {
    respond_with(request, peer, &mut |_, _, _| {}).await
}
pub async fn respond_with(
    request: common::Request,
    peer: &mut crypto::Peer,
    change: &mut impl FnMut(&common::Request, &mut crypto::Peer, &mut (u16, Value)),
) {
    assert_eq!(
        request.headers.get("authorization"),
        Some(&format!("Bearer {}", common::TOKEN))
    );
    let body = if request.body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&request.body).unwrap()
    };
    let mut response = if request.target.ends_with("/whoami") {
        (200, who())
    } else if request.target.ends_with("/state") {
        (200, room())
    } else if request.target.contains("/sync?") {
        (
            200,
            json!({"next_batch":"approval-empty-original","rooms":{"join":{ROOM:{"timeline":{"events":[],"limited":false},"state":{"events":[]}}}},"to_device":{"events":[]}}),
        )
    } else if request.method == "PUT" && request.target.contains("/sendToDevice/") {
        peer.share(body).await;
        (200, json!({}))
    } else if request.method == "PUT" && request.target.contains("/send/") {
        peer.decrypt(body, ROOM.try_into().unwrap()).await;
        (200, json!({"event_id":"$card_accepted"}))
    } else {
        peer.protocol(&request.method, &request.target, &body)
            .await
            .expect("fixed original approval protocol")
    };
    change(&request, peer, &mut response);
    request.json(response.0, response.1);
}
pub async fn drive<T>(
    future: impl std::future::Future<Output = T>,
    fake: &mut common::Fake,
    peer: &mut crypto::Peer,
) -> T {
    drive_with(future, fake, peer, |_, _, _| {}).await
}
pub async fn drive_with<T>(
    future: impl std::future::Future<Output = T>,
    fake: &mut common::Fake,
    peer: &mut crypto::Peer,
    mut change: impl FnMut(&common::Request, &mut crypto::Peer, &mut (u16, Value)),
) -> T {
    tokio::pin!(future);
    let mut count = 0;
    loop {
        tokio::select! {result=&mut future=>return result,request=fake.next()=>{count+=1;assert!(count<=128);respond_with(request,peer,&mut change).await;}}
    }
}
