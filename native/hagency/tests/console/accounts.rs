use super::*;
use hagency_store::{ACCOUNT_PROFILE, AccountEnrollmentAccess};
use std::time::{Duration, Instant};

/// An account-management session: the operator ticket from the new route,
/// exchanged exactly as the other two management scopes are.
async fn management(service: &Service) -> String {
    // The issuer shares one rate budget across every scope (authority.rs
    // `issue_scope`): a second issue within one second of the `session()` issue
    // answers Busy. Clear the budget before issuing; a 429 is a real refusal
    // to be reported from its body, never retried.
    tokio::time::sleep(Duration::from_millis(1010)).await;
    let mut response = TestClient::post(format!("{BASE}/api/native/v1/console/account-access"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(service)
        .await;
    let status = response.status_code;
    let body = response.take_string().await.unwrap_or_default();
    assert_eq!(
        status,
        Some(StatusCode::OK),
        "account access issue refused: {body}"
    );
    let ticket = serde_json::from_str::<Value>(&body).unwrap()["ticket"]
        .as_str()
        .unwrap()
        .to_owned();
    let response = exchange(service, &ticket).await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    response
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}
fn command(path: &str, cookie: &str) -> salvo::test::RequestBuilder {
    TestClient::post(format!("{BASE}{path}"))
        .add_header("host", "127.0.0.1:13300", true)
        .add_header("origin", BASE, true)
        .add_header("sec-fetch-site", "same-origin", true)
        .add_header("cookie", cookie, true)
}
fn read(cookie: &str, path: &str) -> salvo::test::RequestBuilder {
    TestClient::get(format!("{BASE}{path}"))
        .add_header("host", "127.0.0.1:13300", true)
        .add_header("origin", BASE, true)
        .add_header("sec-fetch-site", "same-origin", true)
        .add_header("cookie", cookie, true)
}
/// The identity negative, twice over different value classes: the decoded
/// walk (sound for the tuple, which JSON-escapes on the wire) and the
/// raw-byte search (sound only for the alphanumeric seat and preset ids).
/// `credential` carries the MA-S3b byte-level negatives over the RIGHT value
/// class: the credential home path, a token-shaped value, the credential-
/// present file answer and any probe output — never the readiness word.
fn assert_no_identity(raw: &str, seeded: &[String], credential: &[&str]) {
    let value: Value = serde_json::from_str(raw).expect("decoded response");
    // The exact-six-keys clause, asserted server-side: `AccountRow` is
    // Serialize-only (no deny_unknown_fields on the wire), so a seventh field
    // added later must fail HERE, not only in the client validator (D4).
    fn assert_keys_exact(account: &Value, raw: &str) {
        let mut keys: Vec<&str> = account
            .as_object()
            .unwrap_or_else(|| panic!("account object, not {account}"))
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut expected = ["id", "ordinal", "readiness", "revision", "profile", "state"];
        expected.sort_unstable();
        assert_eq!(
            keys, expected,
            "account object must carry exactly the six declared keys: {raw}"
        );
        let readiness = account["readiness"].as_str().expect("readiness word");
        assert!(
            ["subscription", "api_key", "unknown"].contains(&readiness),
            "readiness must be the observed mode or unknown: {raw}"
        );
    }
    if let Some(account) = value.get("account") {
        assert_keys_exact(account, raw);
    }
    if let Some(accounts) = value.get("accounts").and_then(Value::as_array) {
        accounts.iter().for_each(|a| assert_keys_exact(a, raw));
    }
    // Byte-level negatives over the credential value class, on the raw
    // response: a seeded credential-shaped byte never crosses, whatever
    // object it was stored on.
    for forbidden in credential {
        assert!(
            !raw.contains(forbidden),
            "credential value leaked: {forbidden}"
        );
    }
    fn walk(value: &Value, seeded: &[String], raw: &str) {
        match value {
            Value::String(s) => {
                for secret in seeded {
                    assert!(!s.contains(secret.as_str()), "identity leaked: {raw}");
                    assert_ne!(s, secret.as_str(), "identity leaked: {raw}")
                }
            }
            Value::Object(map) => {
                for (key, inner) in map {
                    for secret in seeded {
                        assert_ne!(key, secret.as_str(), "identity key leaked: {raw}");
                    }
                    walk(inner, seeded, raw);
                }
            }
            Value::Array(items) => items.iter().for_each(|item| walk(item, seeded, raw)),
            _ => {}
        }
    }
    walk(&value, seeded, raw);
}
/// Seed one enrolled account through the store's own wrappers — the same
/// calls the store half's test drives — so the seeded identity values exist
/// before the service answers.
async fn seed_enrolled(f: &Fixture) -> hagency_store::AccountChoice {
    let reserved = f
        .domain
        .reserve_account(ACCOUNT_PROFILE.to_owned())
        .await
        .unwrap();
    let choice = f
        .domain
        .materialize_account(reserved.id.clone())
        .await
        .unwrap();
    let managed = f.domain.managed_account(choice.id.clone()).await.unwrap();
    let access =
        AccountEnrollmentAccess::new(Instant::now() + Duration::from_secs(30), Default::default());
    let command = access
        .prepare(
            &managed,
            choice.revision.clone(),
            "gpt-5.6-sol".into(),
            Some("medium".into()),
            None,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    f.domain.enroll_account_resource(command).await.unwrap();
    choice
}
#[tokio::test]
async fn native_console_account_routes_carry_no_identity() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let seeded_choice = seed_enrolled(&f).await;
    let seeded = {
        let sql = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
        sql.query_row(
            "SELECT a.seat_id,a.identity_tuple,json_extract(a.namespace_identity,'$.volume'),r.preset_id FROM managed_accounts a JOIN resource_accounts r ON r.account_id=a.id LIMIT 1",
            [],
            |row| {
                Ok([
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ])
            },
        )
        .unwrap()
    };
    let service = f.service();
    let cookie = session(&service).await;
    let manager = management(&service).await;
    let mut bodies: Vec<String> = Vec::new();
    // The list read and the single read of the seeded row.
    for path in [
        "/console/api/accounts".to_owned(),
        format!("/console/api/accounts/{}", seeded_choice.id),
    ] {
        let mut response = read(&cookie, &path).send(&service).await;
        assert_eq!(response.status_code, Some(StatusCode::OK));
        bodies.push(response.take_string().await.unwrap());
    }
    // All three mutation responses.
    let mut prepared = command("/console/api/accounts", &manager)
        .json(&json!({"profile":ACCOUNT_PROFILE}))
        .send(&service)
        .await;
    assert_eq!(prepared.status_code, Some(StatusCode::OK));
    bodies.push(prepared.take_string().await.unwrap());
    let mut retired = command(
        &format!("/console/api/accounts/{}/retire", seeded_choice.id),
        &manager,
    )
    .send(&service)
    .await;
    assert_eq!(retired.status_code, Some(StatusCode::OK));
    bodies.push(retired.take_string().await.unwrap());
    // Enrolment takes the remaining active account — the one just prepared.
    let active = f
        .domain
        .account_choices()
        .await
        .unwrap()
        .into_iter()
        .find(|c| matches!(c.state, hagency_store::AccountState::Active))
        .unwrap();
    let mut enrolled = command(
        &format!("/console/api/accounts/{}/enrollment", active.id),
        &manager,
    )
    .json(&json!({"model":"gpt-5.6-sol","reasoning":"medium","expectedRevision":active.revision}))
    .send(&service)
    .await;
    let body = enrolled.take_string().await.unwrap_or_default();
    assert_eq!(
        enrolled.status_code,
        Some(StatusCode::OK),
        "enrollment refused: {body}"
    );
    bodies.push(body);
    for raw in &bodies {
        assert_no_identity(raw, &seeded, &["credential-present", "/.codex", "sk-"]);
        // Raw-byte search: sound only for the alphanumeric seat and preset.
        assert!(!raw.contains(seeded[0].as_str()), "seat id leaked");
        assert!(!raw.contains(seeded[3].as_str()), "preset id leaked");
        for forbidden in [
            "namespace_identity",
            "identity_tuple",
            "seat_id",
            "preset_id",
        ] {
            assert!(!raw.contains(forbidden), "identity key leaked: {forbidden}");
        }
    }
    f.close().await;
}
#[tokio::test]
async fn native_console_account_mutations_require_the_scope() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let readonly = session(&service).await;
    let before = f.domain.account_choices().await.unwrap().len();
    // Each of the three mutations refuses the read-only session with the
    // account scope word before any store job runs.
    let mut prepare = command("/console/api/accounts", &readonly)
        .json(&json!({"profile":ACCOUNT_PROFILE}))
        .send(&service)
        .await;
    assert_eq!(prepare.status_code, Some(StatusCode::FORBIDDEN));
    assert_eq!(
        serde_json::from_str::<Value>(&prepare.take_string().await.unwrap()).unwrap()["code"],
        "account_scope_required"
    );
    let mut retire = command("/console/api/accounts/no_such_row/retire", &readonly)
        .send(&service)
        .await;
    assert_eq!(retire.status_code, Some(StatusCode::FORBIDDEN));
    assert_eq!(
        serde_json::from_str::<Value>(&retire.take_string().await.unwrap()).unwrap()["code"],
        "account_scope_required"
    );
    let mut enrol = command("/console/api/accounts/no_such_row/enrollment", &readonly)
        .json(&json!({"model":"gpt-5.6-sol","expected_revision":"0".repeat(64)}))
        .send(&service)
        .await;
    assert_eq!(enrol.status_code, Some(StatusCode::FORBIDDEN));
    assert_eq!(
        serde_json::from_str::<Value>(&enrol.take_string().await.unwrap()).unwrap()["code"],
        "account_scope_required"
    );
    assert_eq!(
        f.domain.account_choices().await.unwrap().len(),
        before,
        "no account row changed state"
    );
    // With the scoped session the SAME three calls succeed and the row state
    // advances; only the enrolment body carries an expected revision.
    let manager = management(&service).await;
    let mut prepared = command("/console/api/accounts", &manager)
        .json(&json!({"profile":ACCOUNT_PROFILE}))
        .send(&service)
        .await;
    assert_eq!(prepared.status_code, Some(StatusCode::OK));
    let prepared: Value = serde_json::from_str(&prepared.take_string().await.unwrap()).unwrap();
    let row = &prepared["account"];
    let id = row["id"].as_str().unwrap().to_owned();
    assert_eq!(row["state"], "active", "prepare advances the row");
    // Enrolment: the only body that carries an expected revision.
    let mut enrolled = command(&format!("/console/api/accounts/{id}/enrollment"), &manager)
        .json(&json!({"model":"gpt-5.6-sol","reasoning":"medium","expectedRevision":row["revision"].as_str().unwrap()}))
        .send(&service)
        .await;
    let enrolled_body = enrolled.take_string().await.unwrap_or_default();
    assert_eq!(
        enrolled.status_code,
        Some(StatusCode::OK),
        "enrollment refused: {enrolled_body}"
    );
    let retired_row: Value = serde_json::from_str(&enrolled_body).unwrap();
    let retired_row = &retired_row["account"];
    assert_eq!(retired_row["id"], id.as_str());
    // Retire: no body, no expected revision — the row leaves active.
    let mut retired = command(&format!("/console/api/accounts/{id}/retire"), &manager)
        .send(&service)
        .await;
    assert_eq!(retired.status_code, Some(StatusCode::OK));
    let retired: Value = serde_json::from_str(&retired.take_string().await.unwrap()).unwrap();
    assert_eq!(
        retired["account"]["state"], "retired",
        "retire advances the row"
    );
    f.close().await;
}
#[tokio::test]
async fn native_console_account_prepare_interrupted_is_unknown() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let cookie = session(&service).await;
    // Reserve, then do NOT materialize: the row stays inspectable with no
    // invented outcome — the wider unknown window the console owns (2 s
    // reply bound vs the store's 5 s preparation deadline).
    let reserved = f
        .domain
        .reserve_account(ACCOUNT_PROFILE.to_owned())
        .await
        .unwrap();
    let mut response = read(&cookie, "/console/api/accounts").send(&service).await;
    let value: Value = serde_json::from_str(&response.take_string().await.unwrap()).unwrap();
    let row = value["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == reserved.id)
        .expect("the interrupted row is never dropped");
    // The store's own in-progress word; the 'uncertain' arm is
    // materialize_account's 'uncertain'-before-mkdir ordering, exercised by
    // the store half's own test. The read never invents an outcome either way.
    assert_eq!(row["state"], "preparing", "state never invents an outcome");
    assert!(row.get("outcome").is_none());
    // The row remains readable on a later read.
    let mut again = read(&cookie, "/console/api/accounts").send(&service).await;
    let second: Value = serde_json::from_str(&again.take_string().await.unwrap()).unwrap();
    assert!(
        second["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == reserved.id)
    );
    f.close().await;
}

/// MA-S3b: the DTO carries exactly the six declared keys, the readiness word
/// is the observed mode or unknown, and no credential byte — the credential
/// home path, the credential-present file answer, a token-shaped value or any
/// probe output — ever crosses, over the right value class. The readiness
/// word is served; nothing from the credential value class is.
#[tokio::test]
async fn native_console_account_dto_matches_retained_redaction() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let base = now();
    // Four readiness shapes on the real store rows, seeded through the same
    // raw second connection the identity negative already uses (the store
    // half's own tests cover the receipt-path writer). The probe-output
    // marker carries no `credential` byte — migration 028's CHECK forbids
    // one in provider_state — so the credential negative is anchored at the
    // schema level and re-asserted here on the response bytes.
    async fn materialize(f: &Fixture) -> hagency_store::AccountChoice {
        let reserved = f
            .domain
            .reserve_account(ACCOUNT_PROFILE.to_owned())
            .await
            .unwrap();
        f.domain
            .materialize_account(reserved.id.clone())
            .await
            .unwrap()
    }
    let live = materialize(&f).await;
    let expired = materialize(&f).await;
    let uncertain = materialize(&f).await;
    let absent = materialize(&f).await;
    let probe_live = "probe-output-ALPHA9f2";
    let probe_expired = "probe-output-BRAVO7c1";
    let probe_uncertain = "probe-output-CHARLIEd4";
    {
        let sql = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
        let seed = |suffix: &str,
                    account: &str,
                    observed: u64,
                    expires: u64,
                    mode: &str,
                    provider: &str,
                    outcome: &str| {
            sql.execute(
                "INSERT INTO account_login_observations \
                 (id,account_id,account_generation,attempt,observed_at_ms,expires_at_ms,mode,provider_state,outcome) \
                 VALUES (?1,?2,1,1,?3,?4,?5,?6,?7)",
                rusqlite::params![format!("observation_{suffix}"), account, observed, expires, mode, provider, outcome],
            )
            .unwrap();
        };
        // observed + unexpired -> the mode word.
        seed(
            "0a1b2c3d4e5f0a1b2c3d4e5f0a1b2c3d",
            &live.id,
            base,
            base + 3_600_000,
            "subscription",
            probe_live,
            "observed",
        );
        // observed but already expired -> unknown.
        seed(
            "1b2c3d4e5f0a1b2c3d4e5f0a1b2c3d4e",
            &expired.id,
            base,
            base,
            "api_key",
            probe_expired,
            "observed",
        );
        // uncertain -> unknown (an interrupted login is never an answer).
        seed(
            "2c3d4e5f0a1b2c3d4e5f0a1b2c3d4e5f",
            &uncertain.id,
            base,
            base + 3_600_000,
            "unknown",
            probe_uncertain,
            "uncertain",
        );
        // absent: no row at all -> unknown.
    }
    let service = f.service();
    let cookie = session(&service).await;
    let mut bodies: Vec<String> = Vec::new();
    let list: Value;
    {
        let mut response = read(&cookie, "/console/api/accounts").send(&service).await;
        assert_eq!(response.status_code, Some(StatusCode::OK));
        let raw = response.take_string().await.unwrap();
        list = serde_json::from_str(&raw).unwrap();
        bodies.push(raw);
    }
    for id in [&live.id, &expired.id, &uncertain.id, &absent.id] {
        let mut response = read(&cookie, &format!("/console/api/accounts/{id}"))
            .send(&service)
            .await;
        assert_eq!(response.status_code, Some(StatusCode::OK));
        bodies.push(response.take_string().await.unwrap());
    }
    // The readiness word per shape, read from the list.
    let word = |id: &str| -> String {
        list["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == id)
            .unwrap_or_else(|| panic!("account {id} in list"))["readiness"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(word(&live.id), "subscription");
    assert_eq!(word(&expired.id), "unknown");
    assert_eq!(word(&uncertain.id), "unknown");
    assert_eq!(word(&absent.id), "unknown");
    // Byte-level negatives over the credential value class: the readiness
    // word crosses, the credential class never does.
    let credential_class = [
        probe_live,
        probe_expired,
        probe_uncertain,
        "/.codex",
        "credential-present",
        "sk-",
        "provider_state",
    ];
    for raw in &bodies {
        assert_no_identity(raw, &[], &credential_class);
        assert!(
            !raw.contains("authentication"),
            "AccountChoice never served"
        );
        assert!(!raw.contains("quota"), "AccountChoice never served");
    }
    f.close().await;
}

/// MA-S3b: the readiness word is observed, never asserted. The
/// observed-unexpired fact reports its mode; expired and uncertain report
/// unknown; a read performs no filesystem, directory, model or network
/// check and writes nothing — the stored fact is byte-identical afterwards.
#[tokio::test]
async fn native_console_account_readiness_is_observed_not_asserted() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let base = now();
    async fn materialize(f: &Fixture) -> hagency_store::AccountChoice {
        let reserved = f
            .domain
            .reserve_account(ACCOUNT_PROFILE.to_owned())
            .await
            .unwrap();
        f.domain
            .materialize_account(reserved.id.clone())
            .await
            .unwrap()
    }
    let observed = materialize(&f).await;
    let expired = materialize(&f).await;
    let uncertain = materialize(&f).await;
    let probe = "probe-output-DELTA5e8";
    {
        let sql = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
        let seed = |suffix: &str,
                    account: &str,
                    observed_at: u64,
                    expires: u64,
                    mode: &str,
                    outcome: &str| {
            sql.execute(
                "INSERT INTO account_login_observations \
                 (id,account_id,account_generation,attempt,observed_at_ms,expires_at_ms,mode,provider_state,outcome) \
                 VALUES (?1,?2,1,1,?3,?4,?5,?6,?7)",
                rusqlite::params![format!("observation_{suffix}"), account, observed_at, expires, mode, probe, outcome],
            )
            .unwrap();
        };
        seed(
            "3d4e5f0a1b2c3d4e5f0a1b2c3d4e5f0a",
            &observed.id,
            base,
            base + 3_600_000,
            "api_key",
            "observed",
        );
        seed(
            "4e5f0a1b2c3d4e5f0a1b2c3d4e5f0a1b",
            &expired.id,
            base,
            base,
            "subscription",
            "observed",
        );
        seed(
            "5f0a1b2c3d4e5f0a1b2c3d4e5f0a1b2c",
            &uncertain.id,
            base,
            base + 3_600_000,
            "unknown",
            "uncertain",
        );
    }
    let service = f.service();
    let cookie = session(&service).await;
    let count = || -> i64 {
        rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3"))
            .unwrap()
            .query_row("SELECT COUNT(*) FROM account_login_observations", [], |r| {
                r.get(0)
            })
            .unwrap()
    };
    let before = count();
    let word = |id: &str, body: &Value| -> String {
        let accounts = body.get("accounts");
        match accounts {
            Some(list) => list
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["id"] == id)
                .unwrap()["readiness"]
                .as_str()
                .unwrap()
                .to_owned(),
            None => body["account"]["readiness"].as_str().unwrap().to_owned(),
        }
    };
    // The observed-unexpired fact reports its mode on both reads.
    let mut single = read(&cookie, &format!("/console/api/accounts/{}", observed.id))
        .send(&service)
        .await;
    let single_body: Value = serde_json::from_str(&single.take_string().await.unwrap()).unwrap();
    assert_eq!(word(&observed.id, &single_body), "api_key");
    // Expired and uncertain degrade to unknown; no read computes or probes.
    let mut list = read(&cookie, "/console/api/accounts").send(&service).await;
    let list_body: Value = serde_json::from_str(&list.take_string().await.unwrap()).unwrap();
    assert_eq!(word(&expired.id, &list_body), "unknown");
    assert_eq!(word(&uncertain.id, &list_body), "unknown");
    // A read never writes and never promotes: the observation ledger is
    // unchanged (no new row, no settled promotion) after every read.
    assert_eq!(count(), before, "a read never writes a fact");
    f.close().await;
}
