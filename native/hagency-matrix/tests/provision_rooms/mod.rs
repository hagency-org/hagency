//! Original inline room operations, with independent actors on actual local TLS.
use super::*;
use crate::enrollment::{crypto_fixture as crypto, state::View};
use crate::sdk::enrollment::Command;
use std::sync::Arc;

const DM: &str = "!physically_created_agent_dm:example.test";
const REP_TOKEN: &str = "synthetic-separate-representative-token";
const HUMAN_TOKEN: &str = "synthetic-independent-owner-token";
const AGENT_TOKEN: &str = "actual-returned-synthetic-token";
const AS_TOKEN: &str = "synthetic-fixed-side-application-service-token";

struct Server {
    user: String,
    device: String,
    peer: crypto::Peer,
    created: bool,
    invited: bool,
    joined: bool,
    owner: bool,
    posts: usize,
    owner_reads: usize,
    as_created: bool,
    as_logged: bool,
    as_posts: usize,
}
impl Server {
    async fn new() -> Self {
        let engagement = format!(
            "en_{}",
            &format!(
                "{:x}",
                sha2::Sha256::digest(
                    serde_json::to_vec(&[fleet_id(), "request_one".into()]).unwrap()
                )
            )[..32]
        );
        let user = format!("@{}_{}:example.test", fleet_id(), engagement);
        let device = format!("DEVICE_{engagement}");
        let peer = crypto::Peer::for_sender(&user, &device).await;
        Self {
            user,
            device,
            peer,
            created: false,
            invited: false,
            joined: false,
            owner: false,
            posts: 0,
            owner_reads: 0,
            as_created: false,
            as_logged: false,
            as_posts: 0,
        }
    }
    fn project(&self) -> Value {
        let mut state = json!([
            member(OWNER),member(&representative()),
            {"type":"m.room.join_rules","state_key":"","content":{"join_rule":"invite"}},
            power_levels(json!({OWNER:100,representative():50})),
            {"type":"com.hagency.project.binding.v1","state_key":"","content":{
                "v":1,"purpose":"project","authVersion":1,"fleetId":fleet_id(),
                "projectId":"project_provision","ownerMxid":OWNER}}
        ]);
        if self.invited {
            state
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"m.room.member","state_key":self.user,
                "content":{"membership":if self.joined {"join"} else {"invite"}}}));
        }
        state
    }
    fn dm(&self) -> Value {
        assert!(self.created);
        json!([
            member(&self.user),
            {"type":"m.room.member","state_key":OWNER,"content":{"membership":if self.owner {"join"} else {"invite"}}},
            {"type":"m.room.join_rules","state_key":"","content":{"join_rule":"invite"}},
            {"type":"m.room.encryption","state_key":"","content":{"algorithm":"m.megolm.v1.aes-sha2"}},
            {"type":"m.room.create","state_key":"","sender":self.user,"content":{"creator":self.user,"m.federate":false}},
            {"type":"m.room.history_visibility","state_key":"","content":{"history_visibility":"invited"}}
        ])
    }
    async fn reply(&mut self, request: &common::Request) -> (u16, Value) {
        let actor = request.headers.get("authorization").unwrap();
        let url =
            reqwest::Url::parse(&format!("https://synthetic.test{}", request.target)).unwrap();
        assert!(!url.query_pairs().any(|(k, _)| k == "access_token"));
        if actor == &format!("Bearer {AS_TOKEN}") {
            if url.path().ends_with("/account/whoami") {
                assert_eq!(request.method, "GET");
                assert!(request.body.is_empty());
                let query = url
                    .query_pairs()
                    .find(|(k, _)| k == "user_id")
                    .map(|(_, v)| v.into_owned());
                return match query {
                    None => (200, json!({"user_id":representative(),"is_guest":false})),
                    Some(user) if user == self.user => {
                        assert!(
                            self.as_created,
                            "a virtual identity is not available before actual registration"
                        );
                        (200, json!({"user_id":self.user,"is_guest":false}))
                    }
                    Some(user) => {
                        assert!(
                            user.starts_with("@hagency_namespace_probe_")
                                && !user.starts_with(&format!("@{}_", fleet_id()))
                        );
                        (
                            403,
                            json!({"errcode":"M_EXCLUSIVE","error":"Outside fixed side namespace"}),
                        )
                    }
                };
            }
            assert_eq!(request.method, "POST");
            assert!(url.query().is_none());
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(body["type"], "m.login.application_service");
            assert!(
                ["password", "token", "access_token", "auth"]
                    .iter()
                    .all(|k| body.get(k).is_none())
            );
            assert!(!String::from_utf8_lossy(&request.body).contains(AS_TOKEN));
            self.as_posts += 1;
            if url.path().ends_with("/register") {
                assert!(!self.as_created && !self.as_logged);
                assert_eq!(
                    body["username"],
                    self.user.trim_start_matches('@').split_once(':').unwrap().0
                );
                assert_eq!(body["inhibit_login"], true);
                assert!(body.get("device_id").is_none());
                self.as_created = true;
                return (200, json!({"user_id":self.user}));
            }
            assert!(url.path().ends_with("/login") && self.as_created && !self.as_logged);
            assert_eq!(
                body["identifier"],
                json!({"type":"m.id.user","user":self.user})
            );
            assert_eq!(body["device_id"], self.device);
            assert_eq!(body["refresh_token"], false);
            self.as_logged = true;
            return (
                200,
                json!({"user_id":self.user,"device_id":self.device,"access_token":AGENT_TOKEN}),
            );
        }
        let rep = actor == &format!("Bearer {REP_TOKEN}");
        let human = actor == &format!("Bearer {HUMAN_TOKEN}");
        assert!(rep || human || actor == &format!("Bearer {AGENT_TOKEN}"));
        let body = if request.body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&request.body).unwrap()
        };
        if request.target.starts_with("/_matrix/client/v3/sync?") {
            assert_eq!(request.method, "GET");
            assert!(!rep && !human && self.as_logged && self.owner && self.joined);
            return (
                200,
                json!({"next_batch":"active-as-original-sdk","rooms":{"join":{}},"to_device":{"events":[]}}),
            );
        }
        if request.target.ends_with("/account/whoami") {
            assert!(!human);
            return (
                200,
                if rep {
                    json!({"user_id":representative(),"device_id":"REP_DEVICE","is_guest":false})
                } else {
                    json!({"user_id":self.user,"device_id":self.device,"is_guest":false})
                },
            );
        }
        if request.target.ends_with("/state") {
            assert!(!human);
            if request.target.contains("project_provision") {
                return (200, self.project());
            }
            assert!(request.target.contains("physically_created_agent_dm") && !rep);
            self.owner_reads += 1;
            return (200, self.dm());
        }
        if request.target.ends_with("/createRoom") {
            assert!(!rep && !human && !self.created);
            assert_eq!(body["invite"], json!([OWNER]));
            assert_eq!(body["preset"], "private_chat");
            assert_eq!(body["is_direct"], true);
            assert_eq!(body["creation_content"]["m.federate"], false);
            assert_eq!(
                body["initial_state"][0]["content"]["algorithm"],
                "m.megolm.v1.aes-sha2"
            );
            assert_eq!(
                body["initial_state"][1]["content"]["history_visibility"],
                "invited"
            );
            self.created = true;
            self.posts += 1;
            return (200, json!({"room_id":DM}));
        }
        if request.target.ends_with("/invite") {
            assert!(
                rep && self.created
                    && !self.invited
                    && request.target.contains("project_provision")
            );
            assert_eq!(body, json!({"user_id":self.user}));
            self.invited = true;
            self.posts += 1;
            return (200, json!({}));
        }
        if request.target.contains("/join/") {
            if human {
                assert!(
                    self.created
                        && !self.owner
                        && request.target.contains("physically_created_agent_dm")
                );
                self.owner = true;
                return (200, json!({"room_id":DM}));
            }
            assert!(
                !rep && self.invited
                    && !self.joined
                    && request.target.contains("project_provision")
            );
            self.joined = true;
            self.posts += 1;
            return (200, json!({"room_id":PROJECT}));
        }
        assert!(!rep && !human && self.created && self.joined && self.owner);
        self.peer
            .protocol(&request.method, &request.target, &body)
            .await
            .expect("original SDK protocol")
    }
}
async fn owner_join(endpoint: String) {
    // This is a separate client acting as the owner, not the factory joining
    // with another participant's credential or inventing owner membership.
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .add_root_certificate(
            reqwest::Certificate::from_pem(include_bytes!("../fixtures/ca.pem")).unwrap(),
        )
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .unwrap();
    let mut url = reqwest::Url::parse(&endpoint).unwrap();
    url.path_segments_mut()
        .unwrap()
        .pop_if_empty()
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
    assert_eq!(
        serde_json::from_slice::<Value>(&response.bytes().await.unwrap()).unwrap(),
        json!({"room_id":DM})
    );
}
async fn drive(
    f: &common::Fixture,
    fake: &mut common::Fake,
    c: &Collector,
    server: &mut Server,
    join_owner: bool,
    mut change: impl FnMut(&common::Request, &mut (u16, Value)),
) -> Result<IntakeSummary, Error> {
    let cancel = CancellationToken::new();
    let operation = c.intake(plan(), &cancel);
    tokio::pin!(operation);
    {
        let script = async {
            inline_input(fake, "physical_rooms").await;
            if !c
                .inner
                .config
                .provisioning
                .as_ref()
                .unwrap()
                .application_service_profile()
            {
                let request = fake.next().await;
                if let Some(home) = c
                    .inner
                    .config
                    .provisioning
                    .as_ref()
                    .unwrap()
                    .observed_home_handle(&admitted_id(f))
                {
                    assert!(
                        home.home_path().unwrap().join("agent.json").exists(),
                        "actual home complete before register acknowledgement"
                    );
                }
                complete_account_request(f, request, fake).await;
            }
        };
        tokio::pin!(script);
        tokio::select! { _=&mut script=>{}, result=&mut operation=>panic!("intake ended before account: {result:?}") }
    }
    let mut owner = None;
    let result = loop {
        tokio::select! {
            result=&mut operation=>break result,
            request=fake.next()=>{
                if request.headers.get("authorization")==Some(&format!("Bearer {AS_TOKEN}")) && request.method=="POST" {
                    assert!(actual_home(f,c).home_path().unwrap().join("agent.json").exists(),"original home precedes either AS POST");
                    assert_eq!(effect_row(f),Some(("provision".into(),"started".into())));
                    let root=account_root(f);assert!(root.join("possible").exists());
                    if request.target.ends_with("/login") {assert!(root.join("initial").exists() && root.join("login-possible").exists());}
                }
                let mut response = server.reply(&request).await;
                change(&request,&mut response);
                let first_dm = request.target.ends_with("/state") && request.target.contains("physically_created_agent_dm");
                request.json(response.0,response.1);
                if first_dm && join_owner && owner.is_none() {owner=Some(tokio::spawn(owner_join(fake.endpoint.clone())));}
            }
        }
    };
    if let Some(mut owner) = owner {
        // An unsafe DM snapshot can finish intake before the independently
        // spawned owner request reaches the peer. Keep serving that original
        // actor until it finishes; awaiting it without the peer deadlocks until
        // its unchanged HTTP deadline. The intake refusal remains the result.
        loop {
            tokio::select! {
                result=&mut owner=>{result.unwrap();break;},
                request=fake.next()=>{
                    assert_eq!(request.headers.get("authorization"),Some(&format!("Bearer {HUMAN_TOKEN}")));
                    let response=server.reply(&request).await;
                    request.json(response.0,response.1);
                }
            }
        }
    }
    result
}
fn observed_account(f: &common::Fixture, c: &Collector) -> Arc<crate::ProvisionedTokenAccount> {
    c.inner
        .config
        .provisioning
        .as_ref()
        .unwrap()
        .observed_account_handle(&admitted_id(f))
}
fn room_root(f: &common::Fixture) -> std::path::PathBuf {
    f.root
        .path()
        .join("accounts")
        .join(format!("agent-rooms-provision_{}", admitted_id(f)))
}
async fn finish(f: common::Fixture, fake: common::Fake, c: Collector) {
    observed_account(&f, &c)
        .close_enrollment_sdk()
        .await
        .unwrap();
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
#[tokio::test]
async fn native_provisioning_inline_rooms_enrollment() {
    let mut server = Server::new().await;
    let (f, mut fake, c) = ready_inline_plan(Some((
        REP_TOKEN,
        vec![(OWNER.into(), server.peer.anchor())],
    )))
    .await;
    let result = drive(&f, &mut fake, &c, &mut server, true, |_, _| {})
        .await
        .unwrap();
    assert_eq!(result.admitted, 2);
    assert_eq!(server.posts, 3);
    assert!(server.owner && server.owner_reads >= 2);
    assert_eq!(server.peer.writes.len(), 5);
    assert_eq!(server.peer.claims, 1);
    assert_account_only(&f, &c);
    let account = observed_account(&f, &c);
    assert_eq!(account.created_agent_dm().unwrap().as_deref(), Some(DM));
    let enrolled = account.observed_enrollment().unwrap().unwrap();
    let owner = enrolled.inner.owner.lock().await;
    assert!(matches!(
        owner.as_ref().unwrap().enrollment(Command::Status).await,
        Ok(View::Complete)
    ));
    drop(owner);
    assert!(room_root(&f).join("complete").exists());
    let encrypted = std::fs::read(room_root(&f).join("dm-response")).unwrap();
    assert!(!String::from_utf8_lossy(&encrypted).contains(DM));
    assert!(!String::from_utf8_lossy(&encrypted).contains(REP_TOKEN));
    super::account_enrollment::decrypt_in_room(&account, &enrolled, &mut server.peer, DM).await;
    finish(f, fake, c).await;
}
#[tokio::test]
async fn native_provisioning_inline_rooms_refusals() {
    for variant in 0..9 {
        let mut server = Server::new().await;
        let (f, mut fake, c) = ready_inline_plan(Some((
            REP_TOKEN,
            vec![(OWNER.into(), server.peer.anchor())],
        )))
        .await;
        let user = server.user.clone();
        let result = drive(&f, &mut fake, &c, &mut server, true, |request, response| {
            if variant == 0
                && request.headers["authorization"] == format!("Bearer {REP_TOKEN}")
                && request.target.ends_with("/whoami")
            {
                response.1["user_id"] = json!(OWNER);
            }
            if request.target.ends_with("/state") && request.target.contains("project_provision") {
                if variant == 1 {
                    response.1[4]["content"]["ownerMxid"] = json!(representative());
                }
                if variant == 2 {
                    response.1[3]["content"]["users"][representative()] = json!(-1);
                }
                if variant == 3 {
                    response
                        .1
                        .as_array_mut()
                        .unwrap()
                        .retain(|e| e["state_key"] != user);
                }
                if variant == 4 {
                    for e in response.1.as_array_mut().unwrap() {
                        if e["state_key"] == user {
                            e["content"]["membership"] = json!("invite");
                        }
                    }
                }
            }
            if variant == 5 && request.target.ends_with("/invite") {
                response.0 = 403;
            }
            if variant == 6
                && request.target.contains("/join/")
                && request.target.contains("project_provision")
            {
                response.1["room_id"] = json!(PRIVATE);
            }
            if request.target.ends_with("/state")
                && request.target.contains("physically_created_agent_dm")
            {
                if variant == 7 {
                    response
                        .1
                        .as_array_mut()
                        .unwrap()
                        .push(member(&representative()));
                }
                if variant == 8 {
                    response.1[3]["content"]["algorithm"] = json!("unsupported");
                }
            }
        })
        .await;
        assert!(result.is_err(), "variant {variant}");
        assert!(server.peer.writes.is_empty());
        assert_eq!(server.peer.claims, 0);
        assert_eq!(
            server.posts,
            match variant {
                0..=2 => 0,
                3 | 5 => 2,
                _ => 3,
            }
        );
        assert_eq!(
            effect_row(&f),
            Some(("provision".into(), "uncertain".into()))
        );
        assert_eq!(route_rows(&f), 0);
        finish(f, fake, c).await;
    }
    // The owner does not join within the attempt's own budget. That is not a
    // failure: the rooms exist, the effect stays Started, the intake succeeds,
    // and a later turn looks again; once the owner has joined, the provision
    // continues from the rooms without creating, inviting or joining again.
    let mut server = Server::new().await;
    let mut limits = common::load_limits();
    limits.sdk = std::time::Duration::from_secs(4);
    let (f, mut fake, c) = ready_inline_limits(
        Some((REP_TOKEN, vec![(OWNER.into(), server.peer.anchor())])),
        limits,
    )
    .await;
    drive(&f, &mut fake, &c, &mut server, false, |_, _| {})
        .await
        .unwrap();
    assert_eq!(server.posts, 3);
    assert!(server.owner_reads > 0 && !server.owner);
    assert!(server.peer.writes.is_empty());
    assert_eq!(effect_row(&f), Some(("provision".into(), "started".into())));
    assert_eq!(route_rows(&f), 0);
    let reads = server.owner_reads;
    // A turn with the owner still absent looks once and keeps waiting.
    turn(&mut fake, &c, &mut server).await.unwrap();
    assert!(server.owner_reads > reads && !server.owner);
    assert!(server.peer.writes.is_empty());
    assert_eq!(effect_row(&f), Some(("provision".into(), "started".into())));
    // The owner joins; the next turn finishes the rooms and the enrollment.
    let mut owner = tokio::spawn(owner_join(fake.endpoint.clone()));
    loop {
        tokio::select! {
            result=&mut owner=>{result.unwrap();break;},
            request=fake.next()=>{let response=server.reply(&request).await;request.json(response.0,response.1);}
        }
    }
    assert!(server.owner);
    turn(&mut fake, &c, &mut server).await.unwrap();
    assert_eq!(
        server.posts, 3,
        "nothing is created, invited or joined again"
    );
    assert!(!server.peer.writes.is_empty(), "the enrollment followed");
    assert_eq!(effect_row(&f), Some(("provision".into(), "started".into())));
    finish(f, fake, c).await;
}
/// One coordinator turn with nothing new to admit: the intake resumes every
/// provision waiting for its owner before it reads the room. The coordinator's
/// own reads (whoami, an empty sync, its room states) are answered here; the
/// agent's, the representative's and the owner's go to the rooms peer.
async fn turn(
    fake: &mut common::Fake,
    c: &Collector,
    server: &mut Server,
) -> Result<IntakeSummary, Error> {
    let cancel = CancellationToken::new();
    let operation = c.intake(plan(), &cancel);
    tokio::pin!(operation);
    let coordinator = format!("Bearer {}", common::TOKEN);
    let mut states = [
        super::session_state(),
        super::reception_state(),
        super::project_state(),
    ]
    .into_iter()
    .cycle();
    loop {
        tokio::select! {
            result=&mut operation=>break result,
            request=fake.next()=>{
                if request.headers.get("authorization") == Some(&coordinator) {
                    if request.target.ends_with("/whoami") {
                        request.json(200, common::who());
                    } else if request.target.contains("/sync?") {
                        request.json(200, super::provisioning_sync("idle_turn", vec![]));
                    } else if request.target.ends_with("/state") {
                        request.json(200, states.next().unwrap());
                    } else {
                        panic!("unexpected coordinator request in an idle turn: {}", request.target);
                    }
                } else {
                    let response=server.reply(&request).await;
                    request.json(response.0,response.1);
                }
            }
        }
    }
}
#[tokio::test]
async fn native_provisioning_inline_rooms_replay() {
    let mut server = Server::new().await;
    let (f, mut fake, c) = ready_inline_plan(Some((
        REP_TOKEN,
        vec![(OWNER.into(), server.peer.anchor())],
    )))
    .await;
    drive(&f, &mut fake, &c, &mut server, true, |_, _| {})
        .await
        .unwrap();
    let account = observed_account(&f, &c);
    let before = fake.requests();
    assert!(matches!(
        account
            .create_agent_rooms(
                "another-distinct-representative-token",
                &CancellationToken::new()
            )
            .await,
        Err(Error::Conflict)
    ));
    assert!(matches!(
        account
            .enroll_before_activation(
                1,
                [73; 32],
                vec![HostRoom {
                    room_id: PRIVATE.into(),
                    generation: 1,
                    privacy: RoomPrivacy::Group {}
                }],
                vec![(OWNER.into(), server.peer.anchor())],
                &CancellationToken::new()
            )
            .await,
        Err(Error::Conflict)
    ));
    assert_eq!(fake.requests(), before);
    let protected = std::fs::read(room_root(&f).join("complete")).unwrap();
    {
        let cancel = CancellationToken::new();
        let replay = account.create_agent_rooms(REP_TOKEN, &cancel);
        tokio::pin!(replay);
        loop {
            tokio::select! {result=&mut replay=>{result.unwrap();break;},request=fake.next()=>{
                assert_eq!(request.method,"GET");let response=server.reply(&request).await;request.json(response.0,response.1);
            }}
        }
    }
    assert_eq!(
        std::fs::read(room_root(&f).join("complete")).unwrap(),
        protected
    );
    assert_eq!(server.posts, 3);
    assert_eq!(server.peer.writes.len(), 5);
    assert_account_only(&f, &c);
    finish(f, fake, c).await;
}
#[tokio::test]
async fn native_provisioning_inline_rooms_custody() {
    for (boundary, lost) in [
        ("createRoom", false),
        ("createRoom", true),
        ("/invite", true),
        ("/join/!project_provision", true),
    ] {
        let mut server = Server::new().await;
        let (f, mut fake, c) = ready_inline_plan(Some((
            REP_TOKEN,
            vec![(OWNER.into(), server.peer.anchor())],
        )))
        .await;
        let c = Arc::new(c);
        let owner = c.clone();
        let waiter =
            tokio::spawn(async move { owner.intake(plan(), &CancellationToken::new()).await });
        inline_input(&mut fake, "rooms_custody").await;
        complete_account_step(&f, &mut fake).await;
        let request = loop {
            let request = fake.next().await;
            if request.target.contains(boundary) {
                break request;
            }
            let response = server.reply(&request).await;
            request.json(response.0, response.1);
        };
        assert!(matches!(
            c.intake(plan(), &CancellationToken::new()).await,
            Err(Error::Busy)
        ));
        waiter.abort();
        assert!(matches!(waiter.await,Err(error) if error.is_cancelled()));
        let response = server.reply(&request).await;
        if lost {
            request.raw(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{".to_vec());
        } else {
            request.json(response.0, response.1);
        }
        let mut human = None;
        {
            let settled = c.inner.busy.clone().acquire_owned();
            tokio::pin!(settled);
            let watchdog = tokio::time::sleep(std::time::Duration::from_secs(5));
            tokio::pin!(watchdog);
            loop {
                tokio::select! {
                    permit=&mut settled=>{drop(permit.unwrap());break;},
                    _=&mut watchdog=>panic!("original owned room job did not settle"),
                    request=fake.next()=>{
                        let response=server.reply(&request).await;
                        let dm=request.target.ends_with("/state") && request.target.contains("physically_created_agent_dm");
                        request.json(response.0,response.1);
                        if dm && human.is_none() {human=Some(tokio::spawn(owner_join(fake.endpoint.clone())));}
                    }
                }
            }
        }
        if let Some(human) = human {
            human.await.unwrap();
        }
        if lost {
            assert_eq!(
                effect_row(&f),
                Some(("provision".into(), "uncertain".into()))
            );
            assert!(server.peer.writes.is_empty());
            let before = fake.requests();
            let account = observed_account(&f, &c);
            assert!(
                account
                    .create_agent_rooms(REP_TOKEN, &CancellationToken::new())
                    .await
                    .is_err()
            );
            assert_eq!(fake.requests(), before);
            if boundary != "createRoom" {
                assert_eq!(account.created_agent_dm().unwrap().as_deref(), Some(DM));
            }
        } else {
            assert_account_only(&f, &c);
            assert_eq!(server.peer.writes.len(), 5);
        }
        let c = Arc::try_unwrap(c).ok().unwrap();
        finish(f, fake, c).await;
    }
}
#[tokio::test]
async fn native_provisioning_inline_rooms_scope_change() {
    let mut server = Server::new().await;
    let (f, mut fake, c) = ready_inline_plan(Some((
        REP_TOKEN,
        vec![(OWNER.into(), server.peer.anchor())],
    )))
    .await;
    {
        let cancel = CancellationToken::new();
        let operation = c.intake(plan(), &cancel);
        tokio::pin!(operation);
        {
            let script = async {
                inline_input(&mut fake, "rooms_revoke").await;
                complete_account_step(&f, &mut fake).await;
            };
            tokio::pin!(script);
            tokio::select! {_=&mut script=>{},result=&mut operation=>panic!("early intake: {result:?}")}
        }
        let mut revoked = false;
        loop {
            tokio::select! {result=&mut operation=>{assert!(result.is_err());break;},request=fake.next()=>{
                let response=server.reply(&request).await;
                if server.created && !revoked && request.target.ends_with("/state") && request.target.contains("project_provision") {
                    f.store.revoke("during_room_project_get".into(),admitted_id(&f)).await.unwrap();revoked=true;
                }
                request.json(response.0,response.1);
            }}
        }
        assert!(revoked);
    }
    assert_eq!(server.posts, 1);
    assert!(server.peer.writes.is_empty());
    assert_eq!(
        observed_account(&f, &c)
            .created_agent_dm()
            .unwrap()
            .as_deref(),
        Some(DM)
    );
    assert_eq!(route_rows(&f), 0);
    finish(f, fake, c).await;
}

#[tokio::test]
async fn native_provisioning_inline_rooms_custody_refusals() {
    use std::io::Write;
    for stage in [
        "binding",
        "cipher.key",
        "dm-response",
        "complete",
        "swapped",
        "missing",
        "extra",
    ] {
        let mut server = Server::new().await;
        let (f, mut fake, c) = ready_inline_plan(Some((
            REP_TOKEN,
            vec![(OWNER.into(), server.peer.anchor())],
        )))
        .await;
        drive(&f, &mut fake, &c, &mut server, true, |_, _| {})
            .await
            .unwrap();
        let root = room_root(&f);
        if stage == "extra" {
            hagency_store::private::write_new(&root.join("unrecognized"), b"fixed-fixture-record")
                .unwrap();
        } else if stage == "missing" {
            std::fs::remove_file(root.join("complete")).unwrap();
        } else {
            let bytes = if stage == "swapped" {
                std::fs::read(root.join("dm-response")).unwrap()
            } else {
                b"torn".to_vec()
            };
            let path = root.join(if stage == "swapped" {
                "complete"
            } else {
                stage
            });
            let mut file = hagency_store::private::open(&path, false).unwrap();
            file.set_len(0).unwrap();
            file.write_all(&bytes).unwrap();
            drop(file);
        }
        let account = observed_account(&f, &c);
        let before = fake.requests();
        assert!(
            account
                .create_agent_rooms(REP_TOKEN, &CancellationToken::new())
                .await
                .is_err(),
            "{stage}"
        );
        assert_eq!(fake.requests(), before);
        assert_eq!(account.created_agent_dm().unwrap().as_deref(), Some(DM));
        assert!(
            account
                .create_agent_rooms(REP_TOKEN, &CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(fake.requests(), before);
        assert_eq!(
            effect_row(&f),
            Some(("provision".into(), "uncertain".into()))
        );
        assert_eq!(server.posts, 3);
        assert_eq!(server.peer.writes.len(), 5);
        finish(f, fake, c).await;
    }
}

struct HomeFixture {
    root: tempfile::TempDir,
    homes: std::path::PathBuf,
    source: std::path::PathBuf,
    binary: std::path::PathBuf,
}
impl HomeFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().canonicalize().unwrap();
        let homes = base.join("homes");
        let source = base.join("source");
        let binary = base.join("hagency");
        hagency_store::private::directory(&homes).unwrap();
        hagency_store::private::directory(&source).unwrap();
        hagency_store::private::directory(&source.join("nested")).unwrap();
        hagency_store::private::write_new(
            &source.join("nested/文件.md"),
            b"original project bytes",
        )
        .unwrap();
        hagency_store::private::write_new(&binary, b"#!/bin/sh\nexit 2\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self {
            root,
            homes,
            source,
            binary,
        }
    }
    fn plan(
        &self,
        mode: hagency_store::agent_home::ProjectMode,
        project: &str,
    ) -> hagency_store::agent_home::ManagedHomePlan {
        hagency_store::agent_home::ManagedHomePlan::new(
            self.homes.clone(),
            vec![hagency_store::agent_home::HomeProject {
                project_id: project.into(),
                source: self.source.clone(),
                mode,
            }],
            self.binary.clone(),
        )
        .unwrap()
    }
}
fn actual_home(
    f: &common::Fixture,
    c: &Collector,
) -> Arc<hagency_store::agent_home::ManagedAgentHome> {
    c.inner
        .config
        .provisioning
        .as_ref()
        .unwrap()
        .observed_home_handle(&admitted_id(f))
        .unwrap()
}
async fn ready_appservice(
    home: &HomeFixture,
    server: &Server,
) -> (common::Fixture, common::Fake, Collector) {
    ready_inline_credentials(
        Some((REP_TOKEN, vec![(OWNER.into(), server.peer.anchor())])),
        common::load_limits(),
        Some(home.plan(
            hagency_store::agent_home::ProjectMode::Copy,
            "project_provision",
        )),
        Some(
            crate::ApplicationServiceCredential::new(AS_TOKEN, &format!("{}_", fleet_id()))
                .unwrap(),
        ),
    )
    .await
}
async fn device_identity(fake: &mut common::Fake, server: &mut Server) {
    let endpoint = format!(
        "{}/_matrix/client/v3/account/whoami",
        fake.endpoint.trim_end_matches('/')
    );
    let operation = async {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .add_root_certificate(
                reqwest::Certificate::from_pem(include_bytes!("../fixtures/ca.pem")).unwrap(),
            )
            .timeout(std::time::Duration::from_secs(4))
            .build()
            .unwrap();
        let response = client
            .get(endpoint)
            .bearer_auth(AGENT_TOKEN)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        serde_json::from_slice::<Value>(&response.bytes().await.unwrap()).unwrap()
    };
    let (identity, ()) = common::scripted(operation, async {
        let request = fake.next().await;
        assert_eq!(request.method, "GET");
        assert_eq!(
            request.headers["authorization"],
            format!("Bearer {AGENT_TOKEN}")
        );
        let response = server.reply(&request).await;
        request.json(response.0, response.1);
    })
    .await;
    assert_eq!(
        identity,
        json!({"user_id":server.user,"device_id":server.device,"is_guest":false})
    );
}
async fn retained_as_ledger(f: &common::Fixture) -> crate::enrollment::state::Ledger {
    // Inspect only the already-created SDK after its real owner has closed.
    // These reads cannot seed keys, trust, completion or a replacement owner.
    use matrix_sdk_base::store::StateStore;
    use matrix_sdk_sqlite::{SqliteStateStore, SqliteStoreConfig};
    use matrix_sdk_store_encryption::StoreCipher;
    let root = account_root(f).join("sdk");
    assert!(root.join("journal.key").exists());
    tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let ledger = runtime.block_on(async move {
            let store = SqliteStateStore::open_with_config(
                &SqliteStoreConfig::new(&root)
                    .key(Some(&[73; 32]))
                    .pool_max_size(2),
            )
            .await
            .unwrap();
            let cipher = StoreCipher::import_with_key(
                &[73; 32],
                &std::fs::read(root.join("journal.key")).unwrap(),
            )
            .unwrap();
            let bytes = store
                .get_custom_value(crate::enrollment::state::KEY)
                .await
                .unwrap()
                .unwrap();
            let ledger = cipher.decrypt_value(&bytes).unwrap();
            store.close().await.unwrap();
            drop(store);
            ledger
        });
        drop(runtime);
        ledger
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn native_provisioning_inline_appservice() {
    let home = HomeFixture::new();
    let mut server = Server::new().await;
    let (f, mut fake, c) = ready_appservice(&home, &server).await;
    let result = drive(&f, &mut fake, &c, &mut server, true, |_, _| {})
        .await
        .unwrap();
    assert_eq!(result.admitted, 2);
    assert!(server.as_created && server.as_logged);
    assert_eq!(server.as_posts, 2);
    assert_eq!(server.posts, 3);
    assert!(server.owner && server.owner_reads >= 2);
    assert_eq!(server.peer.writes.len(), 5);
    assert_eq!(server.peer.claims, 1);
    assert_account_only(&f, &c);
    let actual = actual_home(&f, &c);
    let path = actual.home_path().unwrap();
    let workdir = actual.workdir_path().unwrap();
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(path.join("agent.json")).unwrap()).unwrap();
    assert_eq!(manifest["id"], format!("agent_{}", admitted_id(&f)));
    assert_eq!(manifest["human"]["owner"], OWNER);
    assert!(
        !manifest.to_string().contains(AS_TOKEN) && !manifest.to_string().contains(AGENT_TOKEN)
    );
    assert_eq!(
        std::fs::read(workdir.join("projects/project_provision/nested/文件.md")).unwrap(),
        b"original project bytes"
    );
    let account = observed_account(&f, &c);
    assert_eq!(account.created_agent_dm().unwrap().as_deref(), Some(DM));
    for stage in ["possible", "initial", "login-possible", "login", "complete"] {
        let bytes = std::fs::read(account_root(&f).join(stage)).unwrap();
        assert!(
            !String::from_utf8_lossy(&bytes).contains(AS_TOKEN)
                && !String::from_utf8_lossy(&bytes).contains(AGENT_TOKEN)
        );
    }
    assert!(!account_root(&f).join("auth-possible").exists());
    assert!(room_root(&f).join("complete").exists());
    let enrolled = account.observed_enrollment().unwrap().unwrap();
    let owner = enrolled.inner.owner.lock().await;
    assert!(matches!(
        owner.as_ref().unwrap().enrollment(Command::Status).await,
        Ok(View::Complete)
    ));
    drop(owner);
    super::account_enrollment::decrypt_in_room(&account, &enrolled, &mut server.peer, DM).await;
    finish(f, fake, c).await;
}
#[tokio::test]
async fn native_provisioning_sdk_active_appservice() {
    for boundary in ["active", "side_revoked", "namespace_broad"] {
        let home = HomeFixture::new();
        let mut server = Server::new().await;
        let (f, mut fake, c) = ready_appservice(&home, &server).await;
        drive(&f, &mut fake, &c, &mut server, true, |_, _| {})
            .await
            .unwrap();
        assert_account_only(&f, &c);
        let account = observed_account(&f, &c);
        let enrolled = account.observed_enrollment().unwrap().unwrap();
        let identity = enrolled
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .owner_identity();
        let binding = enrolled.inner.config.binding().unwrap();
        let id = format!("provision_{}", admitted_id(&f));
        let sql = rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3")).unwrap();
        let fence = sql
            .query_row("SELECT fence FROM effects WHERE id=?1", [&id], |r| {
                r.get::<_, u64>(0)
            })
            .unwrap();
        drop(sql);
        f.store
            .observe_effect(
                id,
                fence,
                hagency_store::EffectOutcome::Applied {
                    receipt: "explicit offline AS activation, not physical factory proof".into(),
                },
            )
            .await
            .unwrap();
        for _ in 0..if boundary == "active" { 2 } else { 1 } {
            let cancel = CancellationToken::new();
            let operation = account.active_collector(&cancel);
            tokio::pin!(operation);
            let result = loop {
                tokio::select! {
                    result=&mut operation=>break result,
                    request=fake.next()=>{
                        let mut response=server.reply(&request).await;
                        if request.headers.get("authorization")==Some(&format!("Bearer {AS_TOKEN}")) {
                            if boundary=="side_revoked" {response=(401,json!({"errcode":"M_UNKNOWN_TOKEN"}));}
                            if boundary=="namespace_broad" {
                                let url=reqwest::Url::parse(&format!("https://synthetic.test{}",request.target)).unwrap();
                                if let Some((_,user))=url.query_pairs().find(|(k,v)|k=="user_id" && v.starts_with("@hagency_namespace_probe_")) {
                                    response=(200,json!({"user_id":user,"is_guest":false}));
                                }
                            }
                        }
                        request.json(response.0,response.1);
                    },
                }
            };
            assert_eq!(
                result.is_ok(),
                boundary == "active",
                "AS Active boundary {boundary}"
            );
            if let Ok(active) = result {
                assert!(Arc::ptr_eq(&active.inner, &enrolled.inner));
            }
            assert!(
                enrolled
                    .inner
                    .owner
                    .lock()
                    .await
                    .as_ref()
                    .unwrap()
                    .same_owner(&identity)
            );
            assert_eq!(enrolled.inner.config.binding().unwrap(), binding);
        }
        device_identity(&mut fake, &mut server).await;
        if boundary != "active" {
            let before = fake.requests();
            assert!(
                account
                    .active_collector(&CancellationToken::new())
                    .await
                    .is_err()
            );
            assert_eq!(
                fake.requests(),
                before,
                "failed AS Active owner cannot rearm"
            );
        }
        assert_eq!(server.as_posts, 2);
        assert_eq!(server.posts, 3);
        assert_eq!(server.peer.writes.len(), 5);
        assert_eq!(server.peer.claims, 1);
        assert_eq!(route_rows(&f), 0);
        finish(f, fake, c).await;
    }
}
#[tokio::test]
async fn native_provisioning_inline_appservice_scope_change() {
    use crate::enrollment::state::{Phase, WritePhase};
    for boundary in ["room", "sdk", "sdk_broad"] {
        let home = HomeFixture::new();
        let mut server = Server::new().await;
        let (f, mut fake, c) = ready_appservice(&home, &server).await;
        let mut revoked = false;
        let mut sdk_reads = 0;
        let result = drive(&f, &mut fake, &c, &mut server, true, |request, response| {
            if !revoked && request.target.ends_with("/state") {
                if boundary == "room"
                    && request.target.contains("project_provision")
                    && room_root(&f).join("dm-response").exists()
                {
                    revoked = true;
                }
                if boundary != "room"
                    && request.target.contains("physically_created_agent_dm")
                    && room_root(&f).join("complete").exists()
                {
                    sdk_reads += 1;
                    if sdk_reads == 3 {
                        revoked = true;
                    }
                }
            }
            if revoked
                && request.headers.get("authorization") == Some(&format!("Bearer {AS_TOKEN}"))
            {
                if boundary == "sdk_broad" {
                    let url =
                        reqwest::Url::parse(&format!("https://synthetic.test{}", request.target))
                            .unwrap();
                    if let Some((_, user)) = url
                        .query_pairs()
                        .find(|(k, v)| k == "user_id" && v.starts_with("@hagency_namespace_probe_"))
                    {
                        *response = (200, json!({"user_id":user,"is_guest":false}));
                    }
                } else {
                    *response = (
                        401,
                        json!({"errcode":"M_UNKNOWN_TOKEN","error":"Side master revoked"}),
                    );
                }
            }
        })
        .await;
        assert!(revoked && result.is_err(), "boundary {boundary}");
        assert_eq!(server.as_posts, 2);
        assert_eq!(server.posts, if boundary == "room" { 1 } else { 3 });
        assert!(server.peer.writes.is_empty());
        assert_eq!(server.peer.claims, 0);
        assert_eq!(
            effect_row(&f),
            Some(("provision".into(), "uncertain".into()))
        );
        assert_eq!(route_rows(&f), 0);
        let account = observed_account(&f, &c);
        assert_eq!(account.created_agent_dm().unwrap().as_deref(), Some(DM));
        device_identity(&mut fake, &mut server).await;
        let before = fake.requests();
        if boundary != "room" {
            assert_eq!(sdk_reads, 3);
            assert!(
                account
                    .enroll_created_rooms(
                        1,
                        [73; 32],
                        vec![(OWNER.into(), server.peer.anchor())],
                        &CancellationToken::new()
                    )
                    .await
                    .is_err()
            );
            assert_eq!(fake.requests(), before, "failed SDK ownership cannot rearm");
            account.close_enrollment_sdk().await.unwrap();
            let ledger = retained_as_ledger(&f).await;
            assert!(
                ledger.phase == Phase::Writing && ledger.writes[0].phase == WritePhase::Possible
            );
            assert!(
                ledger
                    .writes
                    .iter()
                    .skip(1)
                    .all(|w| w.phase == WritePhase::Prepared && w.response.is_none())
            );
            assert!(ledger.writes[0].response.is_none());
            assert_eq!(ledger.context.user, server.user);
            assert_eq!(ledger.context.device, server.device);
        } else {
            assert!(!account_root(&f).join("sdk").exists());
            assert!(
                account
                    .create_agent_rooms(REP_TOKEN, &CancellationToken::new())
                    .await
                    .is_err()
            );
            assert_eq!(
                fake.requests(),
                before,
                "failed original room owner cannot rearm"
            );
        }
        finish(f, fake, c).await;
    }
}
#[tokio::test]
async fn native_provisioning_inline_home() {
    let home = HomeFixture::new();
    let mut server = Server::new().await;
    let (f, mut fake, c) = ready_inline_home(
        Some((REP_TOKEN, vec![(OWNER.into(), server.peer.anchor())])),
        common::load_limits(),
        Some(home.plan(
            hagency_store::agent_home::ProjectMode::Copy,
            "project_provision",
        )),
    )
    .await;
    drive(&f, &mut fake, &c, &mut server, true, |_, _| {})
        .await
        .unwrap();
    let actual = actual_home(&f, &c);
    let workdir = actual.workdir_path().unwrap();
    let path = actual.home_path().unwrap();
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(path.join("agent.json")).unwrap()).unwrap();
    assert_eq!(manifest["id"], format!("agent_{}", admitted_id(&f)));
    assert_eq!(manifest["name"], "Provisioned");
    assert_eq!(manifest["layoutVersion"], 1);
    assert_eq!(manifest["type"], "codex");
    assert_eq!(manifest["human"]["owner"], OWNER);
    assert!(manifest["task"].is_null());
    let resource = f
        .store
        .resource_configuration("resource_27cac5503836765cd10751d2".into())
        .await
        .unwrap();
    assert_eq!(
        manifest["runtimeProfile"]["primary"]["model"],
        resource.model
    );
    assert_eq!(
        manifest["runtimeProfile"]["primary"]["reasoning"],
        json!(resource.reasoning)
    );
    assert_eq!(manifest["managedProjects"][0]["source"], "copy");
    assert_eq!(
        std::fs::read(workdir.join("projects/project_provision/nested/文件.md")).unwrap(),
        b"original project bytes"
    );
    for file in ["CLAUDE.md", "AGENTS.md"] {
        let content = std::fs::read_to_string(workdir.join(file)).unwrap();
        assert!(!content.contains("{{"));
        assert!(
            content.contains("Provisioned")
                && content.contains(&admitted_id(&f))
                && content.contains("native DomainStore")
        );
        assert_eq!(
            std::fs::read(workdir.join("docs").join(file)).unwrap(),
            content.as_bytes()
        );
        assert!(path.join("supervisor").join(file).exists());
    }
    for dir in [
        "state/locks",
        "state/history",
        "state/tmp",
        "workdir/data",
        "supervisor/docs",
    ] {
        assert!(path.join(dir).is_dir());
    }
    let projects = std::fs::read_to_string(workdir.join("docs/projects.md")).unwrap();
    assert!(projects.contains("hagency-managed-projects:start"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let wrapper = std::fs::read_to_string(workdir.join("task-writer")).unwrap();
        assert!(!wrapper.contains("exec node"));
        assert!(
            wrapper.contains("task \"$@\"")
                && !wrapper.contains(AGENT_TOKEN)
                && !wrapper.contains(REP_TOKEN)
        );
        assert_eq!(
            std::fs::metadata(workdir.join("task-writer"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    assert_account_only(&f, &c);
    assert_eq!(server.posts, 3);
    assert_eq!(server.peer.writes.len(), 5);
    let weak = Arc::downgrade(&actual);
    drop(actual);
    finish(f, fake, c).await;
    assert!(weak.upgrade().is_none());
}
#[tokio::test]
async fn native_provisioning_inline_home_projects() {
    for mode in [
        hagency_store::agent_home::ProjectMode::Copy,
        hagency_store::agent_home::ProjectMode::Symlink,
    ] {
        let home = HomeFixture::new();
        #[cfg(unix)]
        std::os::unix::fs::symlink("nested/文件.md", home.source.join("safe-link")).unwrap();
        let mut server = Server::new().await;
        let (f, mut fake, c) = ready_inline_home(
            Some((REP_TOKEN, vec![(OWNER.into(), server.peer.anchor())])),
            common::load_limits(),
            Some(home.plan(mode, "project_provision")),
        )
        .await;
        drive(&f, &mut fake, &c, &mut server, true, |_, _| {})
            .await
            .unwrap();
        let actual = actual_home(&f, &c);
        let project = actual
            .workdir_path()
            .unwrap()
            .join("projects/project_provision");
        #[cfg(unix)]
        assert_eq!(
            std::fs::read(project.join("safe-link")).unwrap(),
            b"original project bytes"
        );
        let mut file =
            hagency_store::private::open(&project.join("nested/文件.md"), false).unwrap();
        use std::io::Write;
        file.set_len(0).unwrap();
        file.write_all(b"managed edit").unwrap();
        drop(file);
        assert_eq!(
            std::fs::read(home.source.join("nested/文件.md")).unwrap(),
            match mode {
                hagency_store::agent_home::ProjectMode::Copy =>
                    b"original project bytes".as_slice(),
                hagency_store::agent_home::ProjectMode::Symlink => b"managed edit".as_slice(),
            }
        );
        assert_account_only(&f, &c);
        std::fs::rename(&home.source, home.root.path().join("held-original-source")).unwrap();
        hagency_store::private::directory(&home.source).unwrap();
        assert!(
            actual.workdir_path().is_err(),
            "a replacement source cannot inherit original home configuration"
        );
        finish(f, fake, c).await;
    }
}
#[tokio::test]
async fn native_provisioning_inline_home_refusals() {
    for variant in [
        "partial_home",
        "partial_custody",
        "missing_mapping",
        "replaced_source",
        "external_link",
        "large_file",
        "depth",
    ] {
        let home = HomeFixture::new();
        let server = Server::new().await;
        let home_plan = home.plan(
            hagency_store::agent_home::ProjectMode::Copy,
            if variant == "missing_mapping" {
                "other_project"
            } else {
                "project_provision"
            },
        );
        let engagement = server.device.strip_prefix("DEVICE_").unwrap();
        let marker = home
            .homes
            .join(format!("agents/agent_{engagement}/untouched"));
        match variant {
            "partial_home" => {
                hagency_store::private::directory(&home.homes.join("agents")).unwrap();
                hagency_store::private::directory(marker.parent().unwrap()).unwrap();
                hagency_store::private::write_new(&marker, b"preserved original bytes").unwrap();
            }
            "partial_custody" => {
                let parent = home.homes.join("custody");
                hagency_store::private::directory(&parent).unwrap();
                let partial = parent.join(format!("home-{engagement}"));
                hagency_store::private::directory(&partial).unwrap();
                hagency_store::private::write_new(
                    &partial.join("possible"),
                    b"original partial record",
                )
                .unwrap();
            }
            "replaced_source" => {
                std::fs::rename(&home.source, home.root.path().join("held-original")).unwrap();
                hagency_store::private::directory(&home.source).unwrap();
            }
            "external_link" => {
                #[cfg(unix)]
                {
                    let external = home.root.path().join("external");
                    hagency_store::private::write_new(&external, b"outside declared project")
                        .unwrap();
                    std::os::unix::fs::symlink(&external, home.source.join("unsafe-link")).unwrap();
                }
                #[cfg(not(unix))]
                {
                    continue;
                }
            }
            "large_file" => hagency_store::private::write_new(
                &home.source.join("too-large"),
                &vec![b'A'; 4 * 1024 * 1024 + 1],
            )
            .unwrap(),
            "depth" => {
                let mut directory = home.source.clone();
                for _ in 0..18 {
                    directory = directory.join("nested");
                    hagency_store::private::directory(&directory).unwrap();
                }
            }
            _ => {}
        }
        let (f, mut fake, c) = ready_inline_home(
            Some((REP_TOKEN, vec![(OWNER.into(), server.peer.anchor())])),
            common::load_limits(),
            Some(home_plan),
        )
        .await;
        {
            let cancel = CancellationToken::new();
            let operation = c.intake(plan(), &cancel);
            tokio::pin!(operation);
            {
                let script = inline_input(&mut fake, "home_refusal");
                tokio::pin!(script);
                tokio::select! {_=&mut script=>{},result=&mut operation=>panic!("ended before original approval: {result:?}")}
            }
            tokio::select! {result=&mut operation=>assert!(result.is_err(),"{variant}"),request=fake.next()=>panic!("unexpected post-home HTTP method {}",request.method)}
        }
        assert_eq!(fake.requests(), 10);
        assert_eq!(
            effect_row(&f),
            Some(("provision".into(), "uncertain".into()))
        );
        assert_eq!(route_rows(&f), 0);
        assert!(server.peer.writes.is_empty());
        assert!(!account_root(&f).exists());
        if variant == "partial_home" {
            assert_eq!(std::fs::read(&marker).unwrap(), b"preserved original bytes");
        }
        {
            let cancel = CancellationToken::new();
            let replay = c.intake(plan(), &cancel);
            tokio::pin!(replay);
            loop {
                tokio::select! {result=&mut replay=>{assert!(result.is_err());break;},request=fake.next()=>{
                    assert_eq!(request.method,"GET","unknown home cannot rearm a network write");
                    let value=if request.target.ends_with("/whoami") {common::who()}
                        else if request.target.contains("project_provision") {project_state()}
                        else if request.target.contains("reception") {reception_state()} else {session_state()};
                    request.json(200,value);
                }}
            }
        }
        assert!(!account_root(&f).exists());
        c.close().await.unwrap();
        f.store.shutdown().await.unwrap();
        fake.close().await;
    }
}

#[tokio::test]
async fn native_provisioning_inline_home_custody() {
    let home = HomeFixture::new();
    let bytes = vec![b'B'; 4 * 1024 * 1024];
    for index in 0..15 {
        hagency_store::private::write_new(&home.source.join(format!("bounded-{index}")), &bytes)
            .unwrap();
    }
    let mut server = Server::new().await;
    let (f, mut fake, c) = ready_inline_home(
        Some((REP_TOKEN, vec![(OWNER.into(), server.peer.anchor())])),
        common::load_limits(),
        Some(home.plan(
            hagency_store::agent_home::ProjectMode::Copy,
            "project_provision",
        )),
    )
    .await;
    let c = Arc::new(c);
    let caller = c.clone();
    let waiter =
        tokio::spawn(async move { caller.intake(plan(), &CancellationToken::new()).await });
    inline_input(&mut fake, "home_caller_loss").await;
    let custody = home.homes.join(format!("custody/home-{}", admitted_id(&f)));
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(4);
    while !custody.join("possible").try_exists().unwrap() {
        assert!(!waiter.is_finished());
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert!(
        !custody.join("complete").try_exists().unwrap(),
        "outer caller drops during actual physical copy"
    );
    assert!(!account_root(&f).exists());
    assert!(matches!(
        c.intake(plan(), &CancellationToken::new()).await,
        Err(Error::Busy)
    ));
    waiter.abort();
    assert!(matches!(waiter.await,Err(error) if error.is_cancelled()));
    let mut human = None;
    {
        let finished = c.inner.busy.clone().acquire_owned();
        tokio::pin!(finished);
        let watchdog = tokio::time::sleep(std::time::Duration::from_secs(15));
        tokio::pin!(watchdog);
        loop {
            tokio::select! {
                permit=&mut finished=>{drop(permit.unwrap());break;},
                _=&mut watchdog=>panic!("original retained home/copy operation did not settle"),
                request=fake.next()=>{
                    if request.target.ends_with("/register") {
                        assert!(actual_home(&f,&c).home_path().unwrap().join("agent.json").exists());
                        complete_account_request(&f,request,&mut fake).await;
                    } else {
                        let response=server.reply(&request).await;
                        let dm=request.target.ends_with("/state") && request.target.contains("physically_created_agent_dm");
                        request.json(response.0,response.1);
                        if dm && human.is_none() {human=Some(tokio::spawn(owner_join(fake.endpoint.clone())));}
                    }
                }
            }
        }
    }
    if let Some(human) = human {
        human.await.unwrap();
    }
    assert_account_only(&f, &c);
    assert_eq!(server.posts, 3);
    assert_eq!(server.peer.writes.len(), 5);
    assert!(custody.join("complete").exists());
    assert_eq!(
        std::fs::read_dir(home.homes.join("agents"))
            .unwrap()
            .count(),
        1
    );
    let c = Arc::try_unwrap(c).ok().unwrap();
    finish(f, fake, c).await;
}

#[tokio::test]
async fn native_provisioning_inline_home_scope_change() {
    let home = HomeFixture::new();
    let bytes = vec![b'C'; 512 * 1024];
    for index in 0..120 {
        hagency_store::private::write_new(&home.source.join(format!("bounded-{index}")), &bytes)
            .unwrap();
    }
    let server = Server::new().await;
    let (f, mut fake, c) = ready_inline_home(
        Some((REP_TOKEN, vec![(OWNER.into(), server.peer.anchor())])),
        common::load_limits(),
        Some(home.plan(
            hagency_store::agent_home::ProjectMode::Copy,
            "project_provision",
        )),
    )
    .await;
    let c = Arc::new(c);
    let caller = c.clone();
    let mut waiter =
        tokio::spawn(async move { caller.intake(plan(), &CancellationToken::new()).await });
    inline_input(&mut fake, "home_revocation").await;
    let engagement = admitted_id(&f);
    let custody = home.homes.join(format!("custody/home-{engagement}"));
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(4);
    while !custody.join("possible").try_exists().unwrap() {
        assert!(!waiter.is_finished());
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert!(
        !custody.join("complete").try_exists().unwrap(),
        "revocation begins during the actual physical copy"
    );
    f.store
        .revoke("during_original_home_copy".into(), engagement.clone())
        .await
        .unwrap();
    let result = tokio::select! {
        result=&mut waiter=>result.unwrap(),
        request=fake.next()=>panic!("revoked home attempted HTTP method {}",request.method),
        _=tokio::time::sleep(std::time::Duration::from_secs(15))=>panic!("original revoked home did not settle"),
    };
    assert!(result.is_err());
    assert!(
        custody.join("complete").exists(),
        "completed physical custody is retained after the final scope refusal"
    );
    assert!(!account_root(&f).exists());
    assert_eq!(route_rows(&f), 0);
    assert_eq!(fake.requests(), 10);
    assert!(server.peer.writes.is_empty());
    assert!(
        c.inner
            .config
            .provisioning
            .as_ref()
            .unwrap()
            .observed_home_handle(&engagement)
            .is_none()
    );
    let original = std::fs::read(custody.join("complete")).unwrap();
    {
        let cancel = CancellationToken::new();
        let replay = c.intake(plan(), &cancel);
        tokio::pin!(replay);
        loop {
            tokio::select! {result=&mut replay=>{assert!(result.is_err());break;},request=fake.next()=>{
                assert_eq!(request.method,"GET","revoked original home cannot rearm a write");
                let value=if request.target.ends_with("/whoami") {common::who()}
                    else if request.target.contains("project_provision") {project_state()}
                    else if request.target.contains("reception") {reception_state()} else {session_state()};
                request.json(200,value);
            }}
        }
    }
    assert_eq!(std::fs::read(custody.join("complete")).unwrap(), original);
    assert!(!account_root(&f).exists());
    assert_eq!(route_rows(&f), 0);
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
