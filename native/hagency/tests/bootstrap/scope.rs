use super::*;
use hagency_store::private;
use std::io::{Seek, Write};
use std::process::Stdio;

#[tokio::test]
async fn native_bootstrap_config() {
    let cases = [
        "unknown",
        "duplicate",
        "workspace_duplicate",
        "oversized",
        "digest",
        "empty_executable",
        "large_executable",
        "missing",
        "special",
    ];
    #[cfg(unix)]
    let cases = cases.into_iter().chain(["nonprivate"]);
    for kind in cases {
        let f = Fixture::new(false).await;
        let path = f.state_dir.join("development-driver.json");
        let bytes = std::fs::read(&path).unwrap();
        let mut config: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let bytes = match kind {
            "nonprivate" | "missing" => bytes,
            "unknown" => {
                config["runner_capability"] = json!({"secret":"model"});
                serde_json::to_vec(&config).unwrap()
            }
            "duplicate" => {
                let s = String::from_utf8(bytes).unwrap();
                s.replacen("{", "{\"profile\":\"duplicate\",", 1)
                    .into_bytes()
            }
            "workspace_duplicate" => {
                let s = String::from_utf8(bytes).unwrap();
                s.replacen(
                    "\"workspaces\":{",
                    "\"workspaces\":{\"work\":\"/duplicate\",",
                    1,
                )
                .into_bytes()
            }
            "oversized" => vec![b' '; 16 * 1024 + 1],
            "empty_executable" | "large_executable" => {
                let executable = f.root.path().join("invalid-executable");
                let file = private::open(&executable, true).unwrap();
                file.set_len(if kind == "empty_executable" {
                    0
                } else {
                    512 * 1024 * 1024 + 1
                })
                .unwrap();
                config["executable"] = json!(executable.canonicalize().unwrap());
                serde_json::to_vec(&config).unwrap()
            }
            "digest" => {
                config["executable_sha256"] = json!("0".repeat(64));
                serde_json::to_vec(&config).unwrap()
            }
            _ => {
                config["executable"] = json!(f.root.path());
                serde_json::to_vec(&config).unwrap()
            }
        };
        let mut file = private::open(&path, false).unwrap();
        file.set_len(0).unwrap();
        file.rewind().unwrap();
        file.write_all(&bytes).unwrap();
        drop(file);
        #[cfg(unix)]
        if kind == "nonprivate" {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        if kind == "missing" {
            std::fs::remove_file(&path).unwrap();
        }
        let result = f.command(true).output().unwrap();
        assert!(
            !result.status.success(),
            "invalid profile unexpectedly started"
        );
        assert_eq!(f.attempts(), 0);
        assert!(!f.work.join("owned-mcp.requests").exists());
    }
}

/// G6 refusal (bootstrap/config.rs:241): a receive-inbox plan naming a
/// workspace absent from the configured workspace map must refuse with the
/// named `Failure::Config` — the plan's `validate()` passes (the id is a
/// well-formed identifier), so the refusal is the map-membership check, not a
/// plan-shape error. Assert the variant through the process's `Error: Config`
/// termination line, not merely that the service did not start.
#[tokio::test]
async fn native_bootstrap_config_receive_inbox_absent_workspace() {
    let f = Fixture::new(false).await;
    let path = f.state_dir.join("development-driver.json");
    let bytes = std::fs::read(&path).unwrap();
    let mut config: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    config["receive_inbox"] = json!({
        "dispatch_id": "dispatch",
        "session_id": "session",
        "task_id": "task",
        "workspace_id": "absent-workspace"
    });
    let mut file = private::open(&path, false).unwrap();
    file.set_len(0).unwrap();
    file.rewind().unwrap();
    file.write_all(&serde_json::to_vec(&config).unwrap())
        .unwrap();
    drop(file);
    let result = f.command(true).stderr(Stdio::piped()).output().unwrap();
    assert!(
        !result.status.success(),
        "absent workspace unexpectedly started"
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("Error: Config"),
        "expected Failure::Config, got stderr: {stderr}"
    );
    assert_eq!(f.attempts(), 0);
    assert!(!f.work.join("owned-mcp.requests").exists());
}
