#[path = "bootstrap/accounts.rs"]
mod accounts;
#[path = "bootstrap/fixture.rs"]
mod fixture;
#[path = "bootstrap/scope.rs"]
mod scope;
use fixture::*;
use serde_json::json;

#[tokio::test]
async fn native_bootstrap_executable() {
    let mut f = Fixture::new(false).await;
    let child = f.launch(true);
    let first = f.fake.next().await;
    assert_eq!(f.state(), "queued");
    assert_eq!(f.attempts(), 0);
    assert!(!f.work.join("owned-mcp.requests").exists());
    first.json(200, common::who());
    f.fake.next().await.json(200, common::sync("bootstrap"));
    f.fake.next().await.json(200, common::state());
    let status = f.wait_result().await;
    assert_eq!(status["mode"], "one_attempt");
    assert_eq!(status["workspace_registered"], true);
    assert_eq!(status["protocol"], "completed");
    assert_eq!(f.attempts(), 1);
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(f.work.join("owned-mcp.receipt")).unwrap()).unwrap();
    assert_eq!(receipt["task"]["id"], "task");
    assert_eq!(receipt["task"]["status"], "in_progress");
    assert_eq!(receipt["helper_exit"], true);
    let capability = f.capabilities().await;
    assert_eq!(capability["agent_execution"], false);
    assert_eq!(capability["production_api_parity"], false);
    let safe = capability.to_string();
    assert!(!safe.contains(common::TOKEN));
    assert!(!safe.contains(f.work.to_str().unwrap()));
    // No automatic second claim even though this peer returned its terminal update.
    assert_eq!(f.attempts(), 1);
    drop(child);
}

#[tokio::test]
async fn native_bootstrap_refresh_refusal() {
    for failure in ["identity", "room", "fenced"] {
        let mut f = Fixture::new(failure == "fenced").await;
        let child = f.launch(true);
        if failure != "fenced" {
            f.fake.next().await.json(
                200,
                if failure == "identity" {
                    json!({"user_id":"@other:example.test","device_id":"DEVICE_1"})
                } else {
                    common::who()
                },
            );
            if failure == "room" {
                f.fake.next().await.json(200, common::sync("bootstrap"));
                f.fake.next().await.json(200, json!([]));
            }
        }
        let status = f.wait_result().await;
        assert_eq!(status["state"], "unavailable");
        assert_eq!(status["error"], "refresh");
        assert_eq!(status["workspace_registered"], false);
        assert_eq!(f.attempts(), 0);
        assert!(!f.work.join("owned-mcp.requests").exists());
        drop(child);
    }
}

#[tokio::test]
async fn native_bootstrap_config_disabled() {
    let f = Fixture::new(false).await;
    std::fs::remove_file(f.state_dir.join("development-driver.json")).unwrap();
    let child = f.launch(false);
    let value = f.capabilities().await;
    assert_eq!(value["development_execution"]["state"], "disabled");
    assert_eq!(value["agent_execution"], false);
    assert_eq!(f.attempts(), 0);
    drop(child);
}

#[cfg(unix)]
#[tokio::test]
async fn native_bootstrap_custody_shutdown() {
    let mut f = Fixture::new(false).await;
    let mut child = f.launch(true);
    common::success(&mut f.fake, "shutdown").await;
    let status = f.wait_result().await;
    assert_eq!(status["protocol"], "completed");
    child.request_shutdown();
    if cfg!(target_os = "macos") {
        assert_eq!(status["cleanup"], "unknown");
        let until = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let status = f.capabilities().await["development_execution"].clone();
            if status["error"] == "outcome_unknown" {
                break;
            }
            assert!(
                tokio::time::Instant::now() < until,
                "unresolved close not reported"
            );
            tokio::task::yield_now().await;
        }
        assert!(child.still_owned());
        assert!(matches!(
            hagency_store::DomainRepository::open(&f.state_dir),
            Err(hagency_store::Error::Locked)
        ));
        assert!(matches!(
            hagency_store::Repository::open(&f.state_dir),
            Err(hagency_store::Error::Locked)
        ));
        assert_eq!(f.attempts(), 1);
        // Test teardown abandons this known-unknown process; it never calls it
        // a clean shutdown or releases its persisted workspace lease.
    } else {
        assert_eq!(status["cleanup"], "whole_tree_stopped");
        child.exited().await;
        drop(hagency_store::DomainRepository::open(&f.state_dir).unwrap());
        drop(hagency_store::Repository::open(&f.state_dir).unwrap());
    }
}
