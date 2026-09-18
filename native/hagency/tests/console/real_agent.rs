//! The console observes the same store written by the original inline factory.
//! Only the unrelated external bootstrap agent is fixture-activated. The target
//! is admitted by Matrix ingress, physically provisioned and activated by its
//! retained native owner before it can appear as active in the roster.
use super::*;

#[path = "../../../hagency-matrix/tests/common/mod.rs"]
pub mod matrix_common;
use matrix_common as matrix;
#[path = "../fixtures/matrix_crypto_peer.rs"]
mod crypto;
// Other factory tests exercise the support module's refusal/dispatch helpers.
#[allow(dead_code)]
#[path = "../inline_factory/mod.rs"]
mod factory;

use hagency::{App, console::Console};

async fn roster(service: &Service, cookie: &str) -> Value {
    let mut response = get("/console/api/agents", cookie).send(service).await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    response.take_json().await.unwrap()
}

#[tokio::test]
async fn native_console_roster_shows_an_ingress_provisioned_agent() {
    for application_service in [false, true] {
        let mut f = factory::Fixture::new(application_service).await;
        let custody = hagency_store::Store::start(
            hagency_store::Repository::open(&f.base.root.path().join("console-custody")).unwrap(),
            16,
        )
        .unwrap();
        let asset_dir = f.base.root.path().join("assets");
        assets(&asset_dir);
        let console = Console::load(&asset_dir.canonicalize().unwrap()).unwrap();
        let app = App::new(
            custody.clone(),
            TOKEN.as_bytes(),
            "127.0.0.1:13300".parse().unwrap(),
        )
        .unwrap()
        .with_domain(f.base.store.clone())
        .with_console(console);
        let service = Service::new(app.router());
        let cookie = session(&service).await;

        let before = roster(&service, &cookie).await;
        assert!(
            !before["agents"]
                .as_array()
                .unwrap()
                .iter()
                .any(|agent| agent["engagement_id"] == f.engagement())
        );
        assert_eq!(
            f.count("SELECT COUNT(*) FROM engagements WHERE request_id='factory_target'"),
            0
        );

        f.provision().await;
        f.original_owner().await;
        // Read the ID actually minted by ingress; the test must not derive a
        // substitute ID and then seed the rows it is meant to verify.
        let engagement: String =
            rusqlite::Connection::open(f.base.root.path().join("domain/domain.sqlite3"))
                .unwrap()
                .query_row(
                    "SELECT id FROM engagements WHERE request_id='factory_target'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
        assert_eq!(f.target_state(), ("complete".into(), "active".into()));
        assert_eq!(f.count("SELECT COUNT(*) FROM matrix_session_routes r JOIN runner_sessions s ON s.id=r.session_id JOIN engagements e ON e.id=s.engagement_id WHERE e.request_id='factory_target'"), 1);
        let agent = f.collector.take_provisioned_agent(&engagement).unwrap();
        assert_eq!(agent.session().id, format!("session_{engagement}"));
        assert_eq!(agent.session().room_id, factory::DM);
        assert!(agent.session().thread_root.is_none());

        let after = roster(&service, &cookie).await;
        let matching: Vec<&Value> = after["agents"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|agent| agent["engagement_id"] == engagement)
            .collect();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0]["state"], "active");
        assert!(
            matching[0]["last_activity_ms"].is_null(),
            "initialization is not a dispatch"
        );
        let mut keys: Vec<&str> = matching[0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "engagement_id",
                "framework",
                "last_activity_ms",
                "name",
                "requested_tokens",
                "role",
                "state"
            ]
        );
        assert!(
            matching[0]
                .as_object()
                .unwrap()
                .values()
                .all(|value| !value.is_object() && !value.is_array())
        );
        assert_eq!(
            f.requests()
                .iter()
                .filter(|request| request["method"] == "initialize")
                .count(),
            1
        );

        agent.close().await.unwrap();
        custody.shutdown().await.unwrap();
        f.close().await;
    }
}
