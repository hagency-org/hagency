use super::*;
#[cfg(unix)]
#[tokio::test]
async fn native_bootstrap_local_codex() {
    use std::os::unix::fs::PermissionsExt;
    for kind in [
        "valid", "profile", "extra", "managed", "factory", "missing", "writable",
    ] {
        let f = Fixture::new(false).await;
        let root = f.root.path().canonicalize().unwrap();
        let home = root.join("provider-home");
        let codex = root.join("provider-codex");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&codex).unwrap();
        let path = f.state_dir.join("development-driver.json");
        let mut config: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        config["local_codex"] = json!({"profile":"provider_owned_codex_v1","preset":"pool","seat":"seat","home":home,"codex_home":codex});
        match kind {
            "profile" => config["local_codex"]["profile"] = json!("implicit"),
            "extra" => config["local_codex"]["ready"] = json!(true),
            "managed" => config["managed_account"] = json!("account_cannot_fallback"),
            "factory" => {
                config["factory_service"] =
                    json!({"profile":"inline_factory_service_checkpoint_v1","idle_ms":1000})
            }
            "missing" => config["local_codex"]["codex_home"] = json!(root.join("missing")),
            "writable" => {
                std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o777)).unwrap()
            }
            _ => {}
        }
        std::fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
        let result = hagency::bootstrap::Bootstrap::open(&f.state_dir, f.address, 16, true);
        if kind == "valid" {
            result.unwrap().close().await.unwrap();
        } else {
            assert!(
                matches!(result, Err(hagency::bootstrap::Failure::Config)),
                "{kind}"
            );
        }
        assert!(!f.state_dir.join("runtime-home").exists());
        assert_eq!(f.attempts(), 0);
        assert_eq!(f.fake.requests(), 0);
        assert_eq!(std::fs::read_dir(&home).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(&codex).unwrap().count(), 0);
        f.fake.close().await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn native_bootstrap_local_codex_dispatch() {
    let mut f = Fixture::new(false).await;
    let root = f.root.path().canonicalize().unwrap();
    let home = root.join("provider-home");
    let codex = root.join("provider-codex");
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir(&codex).unwrap();
    std::fs::write(codex.join("fixture-account-marker"), b"bootstrap-local").unwrap();
    std::fs::write(f.work.join("local-account-probe.required"), b"required").unwrap();
    let path = f.state_dir.join("development-driver.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    config["local_codex"] = json!({"profile":"provider_owned_codex_v1","preset":"pool","seat":"seat","home":home,"codex_home":codex});
    std::fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
    let mut command = f.command(true);
    command
        .env("OPENAI_API_KEY", "offline-forbidden-key")
        .env("HAGENCY_DASHBOARD_TOKEN", "offline-coordinator-secret");
    let child = fixture::Running::from_child(command.spawn().unwrap());
    f.fake.next().await.json(200, common::who());
    f.fake
        .next()
        .await
        .json(200, common::sync("local-bootstrap"));
    f.fake.next().await.json(200, common::state());
    f.fake.next().await.json(200, common::state());
    let status = f.wait_result().await;
    assert_eq!(status["protocol"], "completed", "{status}");
    assert_eq!(status["cleanup"], "whole_tree_stopped", "{status}");
    assert_eq!(f.attempts(), 1);
    let observed: serde_json::Value =
        serde_json::from_slice(&std::fs::read(f.work.join("local-account-observed.json")).unwrap())
            .unwrap();
    assert_eq!(
        observed,
        json!({"selected":true,"same_home":false,"ambient_key":false})
    );
    assert!(!f.state_dir.join("runtime-home").exists());
    let sql = rusqlite::Connection::open(f.state_dir.join("domain.sqlite3")).unwrap();
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM managed_accounts", [], |row| row
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM account_login_observations",
            [],
            |row| row.get::<_, u64>(0)
        )
        .unwrap(),
        0
    );
    drop(child);
}

#[tokio::test]
async fn native_account_bootstrap() {
    let mut f = Fixture::with_account(false, true).await;
    let mut command = f.command(true);
    command
        .env("PATH", "")
        .env("HOME", "/untrusted-ambient-home")
        .env("CODEX_HOME", "/untrusted-ambient-codex")
        .env("OPENAI_API_KEY", "isolated-test-ambient-key");
    let child = command.spawn().unwrap();
    let child = fixture::Running::from_child(child);
    f.fake.next().await.json(200, common::who());
    f.fake
        .next()
        .await
        .json(200, common::sync("managed-bootstrap"));
    f.fake.next().await.json(200, common::state());
    f.fake.next().await.json(200, common::state());
    let status = f.wait_result().await;
    assert_eq!(status["protocol"], "completed", "{status}");
    assert_eq!(f.attempts(), 1);
    let actual: serde_json::Value =
        serde_json::from_slice(&std::fs::read(f.work.join("account-observed.json")).unwrap())
            .unwrap();
    assert_eq!(
        actual,
        json!({"marker":"bootstrap-selected","same_home":true,"ambient_key":false})
    );
    drop(child);
}
