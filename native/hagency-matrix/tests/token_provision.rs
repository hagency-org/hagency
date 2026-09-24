//! Offline account-step fixtures only, not inline provisioning/SDK/runtime proof.
mod common;
use common::{Fake, TOKEN};
use hagency_core::{JSON_SAFE_MAX, authority::Registration};
use hagency_matrix::{
    ApplicationServiceCredential, CancellationToken, Error, Limits, TokenAccountProvision,
};
use hagency_store::{Effect, EffectState, private};
use matrix_sdk_store_encryption::StoreCipher;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    future::Future,
    path::{Path, PathBuf},
    time::Duration,
};

const KEY: [u8; 32] = [31; 32];
const REGISTRATION_TOKEN: &str = "synthetic_registration_credential";
const AS_TOKEN: &str = "synthetic-side-application-service-token";
struct PrivateState {
    _temp: tempfile::TempDir,
    path: PathBuf,
}
impl PrivateState {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("private");
        private::directory(&path).unwrap();
        Self { _temp: temp, path }
    }
    fn path(&self) -> &Path {
        &self.path
    }
}
fn registration() -> Registration {
    common::domain::registration()
}
fn effect() -> Effect {
    Effect {
        id: "provision_target".into(),
        engagement_id: format!("en_{}", "a".repeat(32)),
        kind: "provision".into(),
        state: EffectState::Started,
        fence: 1,
        payload: json!({"resource":{"presetId":"offline"}}),
    }
}
fn identity(effect: &Effect) -> (String, String) {
    (
        format!(
            "@{}_{}:{}",
            registration().fleet_id,
            effect.engagement_id,
            registration().server_name
        ),
        format!("DEVICE_{}", effect.engagement_id),
    )
}
fn operation(fake: &Fake, state: &Path, effect: &Effect) -> TokenAccountProvision {
    configured(fake, state, effect, common::load_limits())
}
fn configured(fake: &Fake, state: &Path, effect: &Effect, limits: Limits) -> TokenAccountProvision {
    TokenAccountProvision::new(
        &registration(),
        effect,
        &fake.endpoint,
        REGISTRATION_TOKEN,
        state.to_owned(),
        KEY,
        limits,
    )
    .unwrap()
    .with_root_pem(include_bytes!("fixtures/ca.pem"))
    .unwrap()
}
fn root(state: &Path, effect: &Effect) -> PathBuf {
    state.join(format!("agent-matrix-{}", effect.id))
}
fn response(effect: &Effect) -> Value {
    let (user, device) = identity(effect);
    json!({"user_id":user,"device_id":device,"access_token":TOKEN})
}
fn whoami(effect: &Effect) -> Value {
    let (user, device) = identity(effect);
    json!({"user_id":user,"device_id":device,"is_guest":false})
}
fn challenge() -> Value {
    json!({"session":"original_session","flows":[{"stages":["m.login.registration_token"]}]})
}
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (entry.file_name().into_string().unwrap(), entry.path())
        })
        .filter(|(_, path)| path.is_file())
        .map(|(name, path)| (name, fs::read(path).unwrap()))
        .collect()
}
fn error<T>(result: Result<T, Error>) -> Error {
    match result {
        Ok(_) => panic!("unexpected account-step success"),
        Err(error) => error,
    }
}
async fn driven<T>(
    work: impl Future<Output = Result<hagency_matrix::ProvisionedTokenAccount, Error>>,
    script: impl Future<Output = T>,
) -> (Result<hagency_matrix::ProvisionedTokenAccount, Error>, T) {
    tokio::pin!(work, script);
    tokio::select! {
        biased;
        output = &mut script => (work.await, output),
        result = &mut work => match result {
            Err(error) => panic!("account step ended before its local peer script: {error}"),
            Ok(_) => panic!("account step succeeded before its local peer script"),
        },
    }
}
async fn wait_unlocked(root: &Path) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let lock = private::open(&root.join("registration.lock"), false).unwrap();
            if lock.try_lock().is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("original accepted owner retained beyond fixture budget");
}

fn as_operation(fake: &Fake, state: &Path, effect: &Effect) -> TokenAccountProvision {
    TokenAccountProvision::application_service(
        &registration(),
        effect,
        &fake.endpoint,
        ApplicationServiceCredential::new(AS_TOKEN, &format!("{}_", registration().fleet_id))
            .unwrap(),
        state.to_owned(),
        KEY,
        common::load_limits(),
    )
    .unwrap()
    .with_root_pem(include_bytes!("fixtures/ca.pem"))
    .unwrap()
}
fn as_reply(request: &common::Request, state: &Path, effect: &Effect) -> (u16, Value) {
    let url = reqwest::Url::parse(&format!("https://fixture.test{}", request.target)).unwrap();
    assert!(!url.query_pairs().any(|(key, _)| key == "access_token"));
    let actor = &request.headers["authorization"];
    if request.method == "GET" {
        assert_eq!(url.path(), "/_matrix/client/v3/account/whoami");
        if actor == &format!("Bearer {AS_TOKEN}") {
            if let Some((_, user)) = url.query_pairs().find(|(key, _)| key == "user_id") {
                if user.starts_with("@hagency_namespace_probe_") {
                    return (403, json!({"errcode":"M_EXCLUSIVE"}));
                }
                assert_eq!(user, identity(effect).0);
                assert!(root(state, effect).join("initial").exists());
                return (200, json!({"user_id":user,"is_guest":false}));
            }
            return (
                200,
                json!({"user_id":registration().representative_mxid,"is_guest":false}),
            );
        }
        assert_eq!(actor, &format!("Bearer {TOKEN}"));
        assert!(url.query().is_none());
        return (200, whoami(effect));
    }
    assert_eq!(request.method, "POST");
    assert_eq!(actor, &format!("Bearer {AS_TOKEN}"));
    assert!(url.query().is_none());
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(body["type"], "m.login.application_service");
    assert!(
        body.get("password").is_none() && body.get("token").is_none() && body.get("auth").is_none()
    );
    assert!(
        !request
            .body
            .windows(AS_TOKEN.len())
            .any(|bytes| bytes == AS_TOKEN.as_bytes())
    );
    if url.path().ends_with("/register") {
        assert_eq!(
            body["username"],
            identity(effect)
                .0
                .split_once(':')
                .unwrap()
                .0
                .trim_start_matches('@')
        );
        assert_eq!(body["inhibit_login"], true);
        assert!(body.get("device_id").is_none());
        assert!(root(state, effect).join("possible").exists());
        return (200, json!({"user_id":identity(effect).0}));
    }
    assert!(url.path().ends_with("/login"));
    assert_eq!(
        body["identifier"],
        json!({"type":"m.id.user","user":identity(effect).0})
    );
    assert_eq!(body["device_id"], identity(effect).1);
    assert_eq!(body["refresh_token"], false);
    assert!(root(state, effect).join("login-possible").exists());
    assert!(!root(state, effect).join("auth-possible").exists());
    (200, response(effect))
}
async fn as_drive(
    work: impl Future<Output = Result<hagency_matrix::ProvisionedTokenAccount, Error>>,
    fake: &mut Fake,
    state: &Path,
    effect: &Effect,
    mut change: impl FnMut(&common::Request, &mut (u16, Value)),
) -> Result<hagency_matrix::ProvisionedTokenAccount, Error> {
    tokio::pin!(work);
    loop {
        tokio::select! {
            result=&mut work=>return result,
            request=fake.next()=>{let mut response=as_reply(&request,state,effect);change(&request,&mut response);request.json(response.0,response.1);},
        }
    }
}

#[tokio::test]
async fn native_appservice_account_provision() {
    let state = PrivateState::new();
    let effect = effect();
    let mut fake = Fake::start(true).await;
    let cancel = CancellationToken::new();
    let mut posts = 0;
    let account = as_drive(
        as_operation(&fake, state.path(), &effect).execute(&cancel),
        &mut fake,
        state.path(),
        &effect,
        |request, _| {
            if request.method == "POST" {
                posts += 1;
            }
        },
    )
    .await
    .unwrap();
    assert_eq!(posts, 2);
    assert_eq!(account.sender_mxid(), identity(&effect).0);
    assert_eq!(account.device_id(), identity(&effect).1);
    let files = snapshot(&root(state.path(), &effect));
    assert_eq!(files.len(), 8);
    for bytes in files.values() {
        for secret in [AS_TOKEN, TOKEN] {
            assert!(
                !bytes
                    .windows(secret.len())
                    .any(|value| value == secret.as_bytes())
            );
        }
    }
    let cipher = StoreCipher::import_with_key(&KEY, &files["cipher.key"]).unwrap();
    let register: Value = cipher.decrypt_value(&files["initial"]).unwrap();
    let login: Value = cipher.decrypt_value(&files["login"]).unwrap();
    assert_eq!(register["stage"], "initial");
    assert!(register["value"]["value"].get("access_token").is_none());
    assert_eq!(login["stage"], "login");
    assert_eq!(login["value"]["value"]["access_token"], TOKEN);
    drop(account);
    fake.close().await;
}

#[tokio::test]
async fn native_appservice_account_refusals() {
    for variant in [
        "sender",
        "broad",
        "bad_probe",
        "registration_user",
        "registration_token",
        "login_refused",
        "login_unsupported",
        "login_user",
        "login_device",
        "login_origin",
        "login_token",
        "login_refresh",
        "device_whoami",
        "side_revoked",
    ] {
        let state = PrivateState::new();
        let effect = effect();
        let mut fake = Fake::start(true).await;
        let cancel = CancellationToken::new();
        let mut posts = 0;
        let result = as_drive(
            as_operation(&fake, state.path(), &effect).execute(&cancel),
            &mut fake,
            state.path(),
            &effect,
            |request, response| {
                if request.method == "POST" {
                    posts += 1;
                }
                let master = request.headers["authorization"] == format!("Bearer {AS_TOKEN}");
                if request.method == "GET" && master {
                    if variant == "sender" && !request.target.contains('?') {
                        response.1["user_id"] = json!(identity(&effect).0);
                    }
                    if request.target.contains("hagency_namespace_probe_") {
                        if variant == "broad" {
                            *response = (200, json!({"user_id":"@outside:example.test"}));
                        }
                        if variant == "bad_probe" {
                            *response = (403, json!({"errcode":"arbitrary"}));
                        }
                    }
                    if variant == "side_revoked"
                        && root(state.path(), &effect).join("login").exists()
                    {
                        *response = (401, json!({"errcode":"M_UNKNOWN_TOKEN"}));
                    }
                }
                if request.target.ends_with("/register") {
                    if variant == "registration_user" {
                        response.1["user_id"] = json!("@other:example.test");
                    }
                    if variant == "registration_token" {
                        response.1["access_token"] = json!(TOKEN);
                    }
                }
                if request.target.ends_with("/login") {
                    match variant {
                        "login_refused" => *response = (403, json!({"errcode":"M_EXCLUSIVE"})),
                        "login_unsupported" => {
                            *response = (400, json!({"errcode":"M_APPSERVICE_LOGIN_UNSUPPORTED"}))
                        }
                        "login_user" => response.1["user_id"] = json!("@other:example.test"),
                        "login_device" => response.1["device_id"] = json!("other-device"),
                        "login_origin" => response.1["home_server"] = json!("other.test"),
                        "login_token" => response.1["access_token"] = json!("short"),
                        "login_refresh" => response.1["refresh_token"] = json!("forbidden"),
                        _ => {}
                    }
                }
                if variant == "device_whoami" && !master {
                    response.1["device_id"] = json!("other-device");
                }
            },
        )
        .await;
        assert!(result.is_err(), "{variant}");
        assert!(posts <= 2);
        assert!(!root(state.path(), &effect).join("complete").exists());
        if matches!(variant, "sender" | "broad" | "bad_probe") {
            assert_eq!(posts, 0);
        }
        let before = fake.requests();
        let retry = as_operation(&fake, state.path(), &effect).execute(&cancel);
        tokio::pin!(retry);
        loop {
            tokio::select! {result=&mut retry=>{assert!(result.is_err(),"{variant}");break;},request=fake.next()=>{
                assert_eq!(request.method,"GET","refused original AS account cannot rearm POST");
                request.json(401,json!({"errcode":"M_UNKNOWN_TOKEN"}));
            }}
        }
        assert!(fake.requests() >= before);
        fake.close().await;
    }
    let state = PrivateState::new();
    let effect = effect();
    let fake = Fake::start(true).await;
    assert!(
        TokenAccountProvision::application_service(
            &registration(),
            &effect,
            &fake.endpoint,
            ApplicationServiceCredential::new(AS_TOKEN, "other_fleet_").unwrap(),
            state.path().to_owned(),
            KEY,
            common::load_limits()
        )
        .is_err()
    );
    assert_eq!(fake.requests(), 0);
    fake.close().await;
}

#[tokio::test]
async fn native_appservice_account_reopen() {
    for variant in [
        "complete",
        "torn",
        "missing_login",
        "swapped",
        "foreign",
        "ordinary_profile",
        "extra",
    ] {
        let state = PrivateState::new();
        let effect = effect();
        let mut fake = Fake::start(true).await;
        let cancel = CancellationToken::new();
        let account = as_drive(
            as_operation(&fake, state.path(), &effect).execute(&cancel),
            &mut fake,
            state.path(),
            &effect,
            |_, _| {},
        )
        .await
        .unwrap();
        drop(account);
        let path = root(state.path(), &effect);
        if variant == "torn" {
            use std::io::Write;
            let mut file = private::open(&path.join("cipher.key"), false).unwrap();
            file.set_len(0).unwrap();
            file.write_all(b"torn").unwrap();
        }
        if variant == "missing_login" {
            fs::remove_file(path.join("login")).unwrap();
        }
        if variant == "swapped" {
            use std::io::Write;
            let bytes = fs::read(path.join("login")).unwrap();
            let mut file = private::open(&path.join("initial"), false).unwrap();
            file.set_len(0).unwrap();
            file.write_all(&bytes).unwrap();
        }
        if variant == "extra" {
            private::write_new(&path.join("unrecognized"), b"fixture").unwrap();
        }
        let original = snapshot(&path);
        let mut claim = effect.clone();
        if variant == "foreign" {
            claim.fence += 1;
        }
        let operation = if variant == "ordinary_profile" {
            TokenAccountProvision::new(
                &registration(),
                &claim,
                &fake.endpoint,
                AS_TOKEN,
                state.path().to_owned(),
                KEY,
                common::load_limits(),
            )
            .unwrap()
        } else {
            as_operation(&fake, state.path(), &claim)
        };
        let result = as_drive(
            operation.execute(&cancel),
            &mut fake,
            state.path(),
            &claim,
            |request, _| assert_eq!(request.method, "GET"),
        )
        .await;
        assert_eq!(result.is_ok(), variant == "complete", "{variant}");
        assert_eq!(snapshot(&path), original);
        drop(result);
        fake.close().await;
    }
}

#[tokio::test]
async fn native_appservice_account_custody() {
    for (boundary, lost) in [
        ("register", false),
        ("register", true),
        ("login", false),
        ("login", true),
    ] {
        let state = PrivateState::new();
        let effect = effect();
        let mut fake = Fake::start(true).await;
        let operation = as_operation(&fake, state.path(), &effect);
        let waiter =
            tokio::spawn(async move { operation.execute(&CancellationToken::new()).await });
        let held = loop {
            let request = fake.next().await;
            if request.target.ends_with(boundary) {
                break request;
            }
            let response = as_reply(&request, state.path(), &effect);
            request.json(response.0, response.1);
        };
        let lock = private::open(
            &root(state.path(), &effect).join("registration.lock"),
            false,
        )
        .unwrap();
        assert!(lock.try_lock().is_err());
        drop(lock);
        let before = fake.requests();
        let retry = as_operation(&fake, state.path(), &effect)
            .execute(&CancellationToken::new())
            .await;
        assert!(matches!(retry, Err(Error::Busy)));
        assert_eq!(fake.requests(), before);
        if lost {
            drop(held);
            assert!(waiter.await.unwrap().is_err());
        } else {
            waiter.abort();
            assert!(matches!(waiter.await,Err(error) if error.is_cancelled()));
            let response = as_reply(&held, state.path(), &effect);
            held.json(response.0, response.1);
            let deadline = tokio::time::sleep(Duration::from_secs(3));
            tokio::pin!(deadline);
            loop {
                if root(state.path(), &effect).join("complete").exists() {
                    break;
                }
                tokio::select! {_=&mut deadline=>panic!("retained AS account did not finish"),request=fake.next()=>{
                    let response=as_reply(&request,state.path(),&effect);request.json(response.0,response.1);
                },_=tokio::time::sleep(Duration::from_millis(5))=>{}}
            }
        }
        wait_unlocked(&root(state.path(), &effect)).await;
        if lost {
            let before = fake.requests();
            assert!(
                as_operation(&fake, state.path(), &effect)
                    .execute(&CancellationToken::new())
                    .await
                    .is_err()
            );
            assert_eq!(fake.requests(), before);
        }
        fake.close().await;
    }
}

#[tokio::test]
async fn native_token_account_provision_observes_registration() {
    let state = PrivateState::new();
    let effect = effect();
    let mut fake = Fake::start(true).await;
    let operation = operation(&fake, state.path(), &effect);
    let cancel = CancellationToken::new();
    let script = async {
        let request = fake.next().await;
        assert_eq!(request.method, "POST");
        assert_eq!(request.target, "/_matrix/client/v3/register");
        assert!(!request.headers.contains_key("authorization"));
        let original: Value = serde_json::from_slice(&request.body).unwrap();
        let (user, device) = identity(&effect);
        assert_eq!(
            original["username"],
            user.split_once(':').unwrap().0.trim_start_matches('@')
        );
        assert_eq!(original["device_id"], device);
        assert_eq!(original["auth"]["type"], "m.login.registration_token");
        assert_eq!(original["auth"]["token"], REGISTRATION_TOKEN);
        assert!(original["auth"].get("session").is_none());
        let password = original["password"].as_str().unwrap().to_owned();
        assert_eq!(password.len(), 68);
        assert!(password.starts_with("Aa1!"));
        assert!(password[4..].bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(original["inhibit_login"], false);
        assert_eq!(original["refresh_token"], false);
        assert!(root(state.path(), &effect).join("possible").is_file());
        request.json(401, challenge());
        let request = fake.next().await;
        assert!(!request.headers.contains_key("authorization"));
        assert_eq!(request.target, "/_matrix/client/v3/register");
        let next: Value = serde_json::from_slice(&request.body).unwrap();
        for name in [
            "username",
            "password",
            "device_id",
            "inhibit_login",
            "refresh_token",
        ] {
            assert_eq!(original[name], next[name]);
        }
        assert_eq!(
            next["auth"],
            json!({"type":"m.login.registration_token","token":REGISTRATION_TOKEN,"session":"original_session"})
        );
        assert!(root(state.path(), &effect).join("auth-possible").is_file());
        request.json(200, response(&effect));
        let request = fake.next().await;
        assert_eq!(request.method, "GET");
        assert_eq!(request.target, "/_matrix/client/v3/account/whoami");
        assert_eq!(request.headers["authorization"], format!("Bearer {TOKEN}"));
        assert!(root(state.path(), &effect).join("auth").is_file());
        request.json(200, whoami(&effect));
        password
    };
    let (result, password) = driven(operation.execute(&cancel), script).await;
    let account = match result {
        Ok(account) => account,
        Err(error) => panic!("original account step refused: {error}"),
    };
    assert_eq!(account.sender_mxid(), identity(&effect).0);
    assert_eq!(account.device_id(), identity(&effect).1);
    assert_eq!(fake.requests(), 3);
    assert!(
        matches!(
            account
                .enroll_before_activation(1, [33; 32], vec![], vec![], &cancel)
                .await,
            Err(Error::Config)
        ),
        "a standalone observed account has no original inline writer/claim custody"
    );
    assert_eq!(fake.requests(), 3);
    assert!(!root(state.path(), &effect).join("sdk").exists());
    let files = snapshot(&root(state.path(), &effect));
    let cipher = StoreCipher::import_with_key(&KEY, &files["cipher.key"]).unwrap();
    for stage in ["possible", "initial", "auth-possible", "auth", "complete"] {
        let plain: Value = cipher.decrypt_value(&files[stage]).unwrap();
        assert!(
            !serde_json::to_string(&plain).unwrap().contains(&password),
            "password must not enter private persistent custody"
        );
        let encoded = String::from_utf8_lossy(&files[stage]);
        assert!(!encoded.contains(TOKEN));
        assert!(!encoded.contains(REGISTRATION_TOKEN));
    }
    assert!(!root(state.path(), &effect).join("domain.sqlite3").exists());
    let config = account
        .into_host_config(
            1,
            [33; 32],
            vec![hagency_matrix::HostRoom {
                room_id: format!("!target:{}", registration().server_name),
                generation: 1,
                privacy: hagency_core::replies::RoomPrivacy::Group {},
            }],
        )
        .unwrap();
    assert_eq!(config.engagement_id(), effect.engagement_id);
    drop(config);
    // A later physical SDK handoff must not make original account inspection
    // fail just because its known, private SDK child directory now exists.
    private::directory(&root(state.path(), &effect).join("sdk")).unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_token_account_reattach_only_reads() {
    let state = PrivateState::new();
    let effect = effect();
    let mut fake = Fake::start(true).await;
    let cancel = CancellationToken::new();
    // Nothing was ever completed here: a re-attach refuses before any request.
    let empty = operation(&fake, state.path(), &effect).for_reattach();
    assert_eq!(error(empty.execute(&cancel).await), Error::Storage);
    assert_eq!(fake.requests(), 0);
    // The original provision: one register, one whoami.
    let original = operation(&fake, state.path(), &effect);
    let (result, ()) = tokio::join!(original.execute(&cancel), async {
        fake.next().await.json(200, response(&effect));
        fake.next().await.json(200, whoami(&effect));
    });
    assert!(result.is_ok());
    let settled = snapshot(&root(state.path(), &effect));
    // After a restart: the stored token, one GET whoami, and nothing written.
    for _ in 0..2 {
        let reattached = operation(&fake, state.path(), &effect).for_reattach();
        let (result, ()) = tokio::join!(reattached.execute(&cancel), async {
            let request = fake.next().await;
            assert_eq!(request.method, "GET");
            assert_eq!(request.target, "/_matrix/client/v3/account/whoami");
            assert_eq!(request.headers["authorization"], format!("Bearer {TOKEN}"));
            request.json(200, whoami(&effect));
        });
        assert!(result.is_ok());
        assert_eq!(settled, snapshot(&root(state.path(), &effect)));
    }
    assert_eq!(fake.requests(), 4);
    // The homeserver no longer knows the token: refused, still nothing written
    // and nothing registered again.
    let reattached = operation(&fake, state.path(), &effect).for_reattach();
    let (result, ()) = tokio::join!(reattached.execute(&cancel), async {
        let request = fake.next().await;
        assert_eq!(request.method, "GET");
        request.json(401, json!({"errcode":"M_UNKNOWN_TOKEN"}));
    });
    assert_eq!(error(result), Error::Unauthorized);
    assert_eq!(settled, snapshot(&root(state.path(), &effect)));
    assert_eq!(fake.requests(), 5);
    fake.close().await;
}
#[tokio::test]
async fn native_token_account_provision_reopens_original_response() {
    let state = PrivateState::new();
    let effect = effect();
    let mut fake = Fake::start(true).await;
    let original = operation(&fake, state.path(), &effect);
    let cancel = CancellationToken::new();
    let (result, ()) = tokio::join!(original.execute(&cancel), async {
        fake.next().await.json(200, response(&effect));
        fake.next()
            .await
            .json(401, json!({"errcode":"M_UNKNOWN_TOKEN"}));
    });
    assert_eq!(error(result), Error::Unauthorized);
    let original_files = snapshot(&root(state.path(), &effect));
    for _ in 0..2 {
        let reopened = operation(&fake, state.path(), &effect);
        let (result, ()) = tokio::join!(reopened.execute(&cancel), async {
            let request = fake.next().await;
            assert_eq!(request.method, "GET");
            assert_eq!(request.target, "/_matrix/client/v3/account/whoami");
            assert_eq!(request.headers["authorization"], format!("Bearer {TOKEN}"));
            request.json(200, whoami(&effect));
        });
        assert!(result.is_ok());
        for (name, bytes) in &original_files {
            assert_eq!(
                &fs::read(root(state.path(), &effect).join(name)).unwrap(),
                bytes
            );
        }
    }
    assert_eq!(fake.requests(), 4);
    let settled = snapshot(&root(state.path(), &effect));
    private::directory(&root(state.path(), &effect).join("sdk")).unwrap();
    let reopened = operation(&fake, state.path(), &effect);
    let (result, ()) = tokio::join!(reopened.execute(&cancel), async {
        let request = fake.next().await;
        assert_eq!(request.method, "GET");
        request.json(200, whoami(&effect));
    });
    assert!(result.is_ok());
    assert_eq!(settled, snapshot(&root(state.path(), &effect)));
    for mutation in 0..4 {
        let mut registration = registration();
        let mut changed = effect.clone();
        let mut token = REGISTRATION_TOKEN;
        match mutation {
            0 => changed.fence += 1,
            1 => registration.generation += 1,
            2 => changed.payload["different"] = true.into(),
            _ => token = "different_registration_credential",
        }
        let reopened = TokenAccountProvision::new(
            &registration,
            &changed,
            &fake.endpoint,
            token,
            state.path().to_owned(),
            KEY,
            common::load_limits(),
        )
        .unwrap();
        assert_eq!(error(reopened.execute(&cancel).await), Error::Conflict);
        assert_eq!(settled, snapshot(&root(state.path(), &effect)));
    }
    let path = root(state.path(), &effect).join("initial");
    let bytes = fs::read(&path).unwrap();
    let mut corrupt = bytes.clone();
    corrupt[0] ^= 1;
    fs::write(&path, &corrupt).unwrap();
    assert_eq!(
        error(
            operation(&fake, state.path(), &effect)
                .execute(&cancel)
                .await
        ),
        Error::Storage
    );
    assert_eq!(fs::read(&path).unwrap(), corrupt);
    fs::write(&path, bytes).unwrap();
    assert_eq!(fake.requests(), 5);
    fake.close().await;
}

#[tokio::test]
async fn native_token_account_provision_retains_uncertainty() {
    let state = PrivateState::new();
    let effect = effect();
    let mut fake = Fake::start(true).await;
    let original = operation(&fake, state.path(), &effect);
    let cancel = CancellationToken::new();
    let original_cancel = cancel.clone();
    let caller = tokio::spawn(async move { original.execute(&original_cancel).await });
    let request = fake.next().await;
    assert_eq!(
        error(
            operation(&fake, state.path(), &effect)
                .execute(&cancel)
                .await
        ),
        Error::Busy
    );
    caller.abort();
    assert!(caller.await.err().is_some_and(|error| error.is_cancelled()));
    assert_eq!(
        error(
            operation(&fake, state.path(), &effect)
                .execute(&cancel)
                .await
        ),
        Error::Busy
    );
    request.json(200, response(&effect));
    let request = fake.next().await;
    assert_eq!(request.method, "GET");
    request.json(200, whoami(&effect));
    wait_unlocked(&root(state.path(), &effect)).await;
    assert!(root(state.path(), &effect).join("complete").is_file());
    assert_eq!(fake.requests(), 2);
    let mut lost = effect.clone();
    lost.id = "lost_response".into();
    let original = operation(&fake, state.path(), &lost);
    let (result, ()) = tokio::join!(original.execute(&cancel), async {
        let request = fake.next().await;
        // A real admitted POST followed by a truncated JSON response, not a
        // fake completed phase or a pre-request connection refusal.
        request.raw(b"HTTP/1.1 200 Fixture\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{".to_vec());
    });
    assert_eq!(error(result), Error::OutcomeUnknown);
    let before = snapshot(&root(state.path(), &lost));
    assert!(before.contains_key("possible"));
    assert!(!before.contains_key("initial"));
    assert_eq!(
        error(operation(&fake, state.path(), &lost).execute(&cancel).await),
        Error::OutcomeUnknown
    );
    assert_eq!(before, snapshot(&root(state.path(), &lost)));
    assert_eq!(fake.requests(), 3);
    fake.close().await;
}

#[tokio::test]
async fn native_token_account_provision_refuses_unsafe_profiles() {
    let state = PrivateState::new();
    let effect = effect();
    let mut fake = Fake::start(true).await;
    for mutation in 0..10 {
        let mut changed = effect.clone();
        let mut token = REGISTRATION_TOKEN.to_owned();
        let mut endpoint = fake.endpoint.clone();
        match mutation {
            0 => changed.state = EffectState::Pending,
            1 => changed.kind = "retire".into(),
            2 => changed.fence = 0,
            3 => changed.fence = JSON_SAFE_MAX + 1,
            4 => changed.engagement_id = "en_unbound".into(),
            5 => changed.id = "../outside".into(),
            6 => token.clear(),
            7 => token = "a".repeat(65),
            8 => token = "bad token".into(),
            _ => endpoint.push_str("outside"),
        }
        assert_eq!(
            error(TokenAccountProvision::new(
                &registration(),
                &changed,
                &endpoint,
                &token,
                state.path().to_owned(),
                KEY,
                common::load_limits()
            )),
            Error::Config
        );
    }
    assert_eq!(fake.requests(), 0);
    for variant in 0..12 {
        let mut selected = effect.clone();
        selected.id = format!("unsafe_{variant}");
        let operation = operation(&fake, state.path(), &selected);
        let cancel = CancellationToken::new();
        let (result, ()) = tokio::join!(operation.execute(&cancel), async {
            let request = fake.next().await;
            match variant {
                0 => request.json(401, json!({"flows":[{"stages":["m.login.registration_token"]}]})),
                1 => request.json(401, json!({"session":"s","flows":[{"stages":["m.login.dummy"]}]})),
                2 => request.json(401, json!({"session":"s","flows":[{"stages":["m.login.registration_token","m.login.dummy"]}]})),
                3 => request.json(401, json!({"session":"s","completed":["m.login.registration_token"],"flows":[{"stages":["m.login.registration_token"]}]})),
                4 => { let mut value=response(&selected); value["user_id"]="@substituted:example.test".into(); request.json(200,value); }
                5 => { let mut value=response(&selected); value["device_id"]="SUBSTITUTED".into(); request.json(200,value); }
                6 => { let mut value=response(&selected); value["refresh_token"]="unsupported".into(); request.json(200,value); }
                7 => request.raw(b"HTTP/1.1 302 Fixture\r\nLocation: https://example.invalid/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()),
                8 => request.raw(b"HTTP/1.1 200 Fixture\r\nContent-Type: application/json\r\nContent-Length: 17408\r\nConnection: close\r\n\r\n".to_vec()),
                9 => request.raw(common::response(200, b"{\"user_id\":\"first\",\"user_id\":\"second\"}")),
                _ => {
                    request.json(200, response(&selected));
                    let request = fake.next().await;
                    assert_eq!(request.method, "GET");
                    let mut value = whoami(&selected);
                    if variant == 10 { value["device_id"]="FOREIGN_DEVICE".into(); }
                    else { value["is_guest"]=true.into(); }
                    request.json(200,value);
                }
            }
        });
        let expected = match variant {
            0 => Error::Wire,
            1..=3 | 6 => Error::Unsupported,
            4..=5 | 10..=11 => Error::Identity,
            _ => Error::OutcomeUnknown,
        };
        assert_eq!(error(result), expected);
        let before = snapshot(&root(state.path(), &selected));
        assert!(!before.contains_key("complete"));
        // A retained refusal is not a fallback/second registration opportunity.
        let retry = operation_for_retry(&fake, state.path(), &selected);
        if variant >= 10 {
            let (result, ()) = driven(retry.execute(&cancel), async {
                let request = fake.next().await;
                assert_eq!(request.method, "GET");
                request.json(401, json!({"errcode":"M_UNKNOWN_TOKEN"}));
            })
            .await;
            assert_eq!(error(result), Error::Unauthorized);
        } else {
            assert!(retry.execute(&cancel).await.is_err());
        }
        assert_eq!(before, snapshot(&root(state.path(), &selected)));
    }
    assert_eq!(fake.requests(), 16);
    let mut partial = effect.clone();
    partial.id = "partial_bootstrap".into();
    private::directory(&root(state.path(), &partial)).unwrap();
    private::write_new(
        &root(state.path(), &partial).join("binding"),
        b"retained-original-context",
    )
    .unwrap();
    assert!(
        operation(&fake, state.path(), &partial)
            .execute(&CancellationToken::new())
            .await
            .is_err()
    );
    assert!(!root(state.path(), &partial).join("cipher.key").exists());
    assert_eq!(fake.requests(), 16);
    fake.close().await;
}
fn operation_for_retry(fake: &Fake, state: &Path, effect: &Effect) -> TokenAccountProvision {
    operation(fake, state, effect)
}

#[tokio::test]
async fn native_token_account_provision_original_deadline() {
    let state = PrivateState::new();
    let effect = effect();
    let mut fake = Fake::start(true).await;
    let mut limits = common::load_limits();
    limits.sdk = Duration::from_millis(750);
    let original = configured(&fake, state.path(), &effect, limits);
    let cancel = CancellationToken::new();
    // Real fsync/TLS scheduling must not decide WHICH of three deliberately
    // delayed responses crosses this cumulative deadline. Only the clock is
    // controlled: the original transport, owner, encrypted records and reopen
    // remain real. This is deadline semantics, not wall-clock latency evidence.
    tokio::time::pause();
    let start = tokio::time::Instant::now();
    let work = driven(original.execute(&cancel), async {
        let request = fake.next().await;
        tokio::time::advance(Duration::from_millis(300)).await;
        request.json(401, challenge());
        let request = fake.next().await;
        tokio::time::advance(Duration::from_millis(300)).await;
        request.json(200, response(&effect));
        let request = fake.next().await;
        assert_eq!(request.method, "GET");
        tokio::time::advance(Duration::from_millis(300)).await;
        request.json(200, whoami(&effect));
    });
    let real_start = std::time::Instant::now();
    // A ready waker prevents automatic virtual-time jumps while real sockets
    // or blocking custody writes are pending. No extra worker/task is created.
    let stationary = std::future::poll_fn(|cx| {
        assert!(
            real_start.elapsed() < Duration::from_secs(3),
            "account deadline fixture stalled"
        );
        cx.waker().wake_by_ref();
        std::task::Poll::<()>::Pending
    });
    let (result, ()) = tokio::select! {value=work=>value,()=stationary=>unreachable!()};
    assert_eq!(start.elapsed(), Duration::from_millis(900));
    tokio::time::resume();
    assert_eq!(error(result), Error::OutcomeUnknown);
    wait_unlocked(&root(state.path(), &effect)).await;
    let accepted = snapshot(&root(state.path(), &effect));
    assert!(accepted.contains_key("auth"));
    assert!(!accepted.contains_key("complete"));
    let reopen = operation(&fake, state.path(), &effect);
    let (result, ()) = driven(reopen.execute(&cancel), async {
        let request = fake.next().await;
        assert_eq!(request.method, "GET");
        request.json(200, whoami(&effect));
    })
    .await;
    assert!(result.is_ok());
    assert_eq!(fake.requests(), 4);
    fake.close().await;
}
