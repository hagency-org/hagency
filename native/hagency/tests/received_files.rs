#[path = "received_files/fixture.rs"]
mod fixture;
#[path = "received_files/recovery.rs"]
mod recovery;
use fixture::{DATA, Fixture};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[tokio::test]
async fn native_receive_executable() {
    for direct in [true, false] {
        let mut f = Fixture::new(direct, "normal").await;
        assert_eq!(f.count("runner_dispatches"), 0);
        assert_eq!(f.count("runner_attempts"), 0);
        let child = f.launch(if direct {
            "receive.direct"
        } else {
            "receive.group"
        });
        let receipt = f.drive().await;
        assert_eq!(receipt["helper_exit"], true);
        assert_eq!(receipt["receive"]["isError"], false);
        let received = &receipt["receive"]["structuredContent"];
        assert_eq!(received["event_id"], "$incoming");
        assert_eq!(received["filename"], "原始文件.bin");
        assert_eq!(received["size"], DATA.len());
        assert_eq!(received["sha256"], format!("{:x}", Sha256::digest(DATA)));
        assert_eq!(received["replayed"], false);
        let actual = f.receipt("bytes").unwrap();
        assert_eq!(
            serde_json::from_value::<Vec<u8>>(actual["bytes"].clone()).unwrap(),
            DATA
        );
        // The runner reports the ordinary projection of the retained root
        // (ADR-116 amendment), not the verbatim canonical fixture path.
        assert_eq!(
            actual["cwd"],
            hagency_execution::ordinary_launch_path(&f.work).unwrap()
        );
        assert_eq!(
            std::fs::read(f.work.join(received["path"].as_str().unwrap())).unwrap(),
            DATA
        );
        assert_eq!(f.receipt("task").unwrap()["status"], "in_progress");
        assert_eq!((f.gets, f.intakes), (1, 1));
        assert_eq!(f.count("runner_dispatches"), 1);
        assert_eq!(f.count("runner_attempts"), 1);
        assert_eq!(f.count("matrix_attachments"), 1);
        assert_eq!(f.count("received_files"), 1);
        let (state, facts): (String, String) = f
            .sql()
            .query_row("SELECT state,facts FROM received_files", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(state, "ready");
        assert_eq!(
            serde_json::from_str::<Value>(&facts).unwrap()["sha256"],
            received["sha256"]
        );
        // The agent is asked only what addresses it and hears the rest (ADR178).
        // In a direct room the upload is the request. In a group thread it
        // addressed nobody: the prompt asks only the mention that followed, and
        // the upload comes back from the room read, which hides nothing and
        // leaks nothing either.
        let prompt = f.receipt("prompt").unwrap().to_string();
        let heard = f.receipt("discussion").unwrap().to_string();
        if direct {
            assert!(prompt.contains("$incoming"));
        } else {
            assert!(prompt.contains("$wake"));
            assert!(!prompt.contains("$incoming"));
            assert!(heard.contains("$wake"));
        }
        assert!(heard.contains("$incoming"));
        for secret in ["mxc://", "key_ops", "A256CTR", "remote.media"] {
            assert!(!prompt.contains(secret));
            assert!(!heard.contains(secret));
        }
        let status = f.capabilities().await;
        assert_eq!(
            status["development_execution"]["workspace_registered"],
            true
        );
        assert_eq!(status["agent_execution"], false);
        assert_eq!(status["production_api_parity"], false);
        // G6: the production start registered the receive-inbox plan's
        // workspace before the first claim (bootstrap::open_with_options →
        // register_workspace), never this fixture. Assert the workspace_resources
        // row the plan's dispatch FK-references actually exists.
        assert_eq!(
            f.sql()
                .query_row(
                    "SELECT COUNT(*) FROM workspace_resources WHERE id='work'",
                    [],
                    |r| r.get::<_, u64>(0)
                )
                .unwrap(),
            1,
            "production start must register the receive-inbox plan's workspace"
        );
        f.fake.no_request().await;
        child.stop_and_reap();
        f.fake.close().await;
    }
}

#[tokio::test]
async fn native_receive_replay_bounds() {
    let mut f = Fixture::new(true, "replay").await;
    let child = f.launch("receive.replay");
    f.drive().await;
    let first = f.receipt("first").unwrap();
    let replay = f.receipt("replay").unwrap();
    let changed = f.receipt("changed").unwrap();
    assert_eq!(first["isError"], false);
    assert_eq!(replay["isError"], false);
    assert_eq!(replay["structuredContent"]["replayed"], true);
    assert_eq!(
        replay["structuredContent"]["path"],
        first["structuredContent"]["path"]
    );
    assert_eq!(changed["isError"], true);
    assert_eq!(
        std::fs::read(
            f.work
                .join(first["structuredContent"]["path"].as_str().unwrap())
        )
        .unwrap(),
        b"mutated original received destination"
    );
    assert_eq!(f.gets, 1);
    assert_eq!(f.count("received_files"), 1);
    let state: String = f
        .sql()
        .query_row("SELECT state FROM received_files", [], |r| r.get(0))
        .unwrap();
    assert_eq!(state, "ready");
    f.fake.no_request().await;
    child.stop_and_reap();
    f.fake.close().await;
}

#[tokio::test]
async fn native_receive_uncertainty() {
    let mut f = Fixture::new(true, "negative").await;
    let child = f.launch("receive.truncated");
    let receipt = f.drive().await;
    assert_eq!(receipt["helper_exit"], true);
    assert_eq!(receipt["receive"]["isError"], true);
    assert_eq!(f.gets, 1);
    assert_eq!(f.count("received_files"), 1);
    let state: String = f
        .sql()
        .query_row("SELECT state FROM received_files", [], |r| r.get(0))
        .unwrap();
    assert_eq!(state, "failed");
    assert!(std::fs::read_dir(&f.work).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".hagency-received-")
    }));
    assert!(f.receipt("bytes").is_none());
    f.fake.no_request().await;
    child.stop_and_reap();
    f.fake.close().await;
}
