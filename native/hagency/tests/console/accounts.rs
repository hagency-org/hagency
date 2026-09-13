use super::*;
use hagency_store::{ACCOUNT_PROFILE, AccountEnrollmentAccess};
use std::time::{Duration, Instant};

/// An account-management session: the operator ticket from the new route,
/// exchanged exactly as the other two management scopes are.
async fn management(service: &Service) -> String {
    let mut response = TestClient::post(format!("{BASE}/api/native/v1/console/account-access"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let ticket = response.take_json::<Value>().await.unwrap()["ticket"]
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
fn assert_no_identity(raw: &str, seeded: &[String]) {
    let value: Value = serde_json::from_str(raw).expect("decoded response");
    fn walk(value: &Value, seeded: &[String], raw: &str) {
        match value {
            Value::String(s) => {
                for secret in seeded {
                    assert!(!s.contains(secret.as_str()), "identity leaked: {raw}");
                    assert_ne!(s, secret.as_str(), "identity leaked: {raw}");
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
    .json(&json!({"model":"gpt-5.6-sol","reasoning":"medium","expected_revision":active.revision}))
    .send(&service)
    .await;
    assert_eq!(enrolled.status_code, Some(StatusCode::OK));
    bodies.push(enrolled.take_string().await.unwrap());
    for raw in &bodies {
        assert_no_identity(raw, &seeded);
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
    let retire = command("/console/api/accounts/no_such_row/retire", &readonly)
        .send(&service)
        .await;
    assert_eq!(retire.status_code, Some(StatusCode::FORBIDDEN));
    let enrol = command("/console/api/accounts/no_such_row/enrollment", &readonly)
        .json(&json!({"model":"gpt-5.6-sol","expected_revision":"0".repeat(64)}))
        .send(&service)
        .await;
    assert_eq!(enrol.status_code, Some(StatusCode::FORBIDDEN));
    assert_eq!(
        f.domain.account_choices().await.unwrap().len(),
        before,
        "no account row changed state"
    );
    // With the scoped session the same prepare succeeds and the row advances;
    // only the enrolment body carries an expected revision.
    let manager = management(&service).await;
    let prepared = command("/console/api/accounts", &manager)
        .json(&json!({"profile":ACCOUNT_PROFILE}))
        .send(&service)
        .await;
    assert_eq!(prepared.status_code, Some(StatusCode::OK));
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
