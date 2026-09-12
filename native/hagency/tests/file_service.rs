#[path = "file_service/fixture.rs"]
mod fixture;
#[path = "file_service/recovery.rs"]
mod recovery;
use fixture::{DATA, Fixture};
use serde_json::json;

#[tokio::test]
async fn native_file_service_executable() {
    for direct in [false, true] {
        let mut f = Fixture::new(direct).await;
        let child = f.launch(
            true,
            if direct {
                "executable.dm"
            } else {
                "executable.group"
            },
        );
        let first = f.next("executable.initial_whoami").await;
        assert_eq!(first.target, "/_matrix/client/v3/account/whoami");
        assert_eq!(f.state(), "queued");
        assert_eq!(f.attempts(), 0);
        assert!(!f.work.join("file-mcp.requests").exists());
        first.json(200, fixture::common::who());
        let receipt = f.deliver().await;
        assert_eq!(receipt["delivery"]["status"], "delivered");
        let task: serde_json::Value =
            serde_json::from_slice(&std::fs::read(f.work.join("file-mcp.task")).unwrap()).unwrap();
        assert_eq!(task["id"], "task");
        assert_eq!(task["status"], "in_progress");
        assert_eq!(receipt["helper_exit"], true);
        assert_eq!(f.attempts(), 1);
        assert_eq!(f.peer.claims, 1);
        assert_eq!(f.peer.writes.len(), 5);
        assert_eq!(f.peer.shares, 1);
        assert_eq!(f.peer.events.len(), 1);
        let content = &f.peer.events[0]["content"];
        assert_eq!(content["msgtype"], "m.file");
        assert_eq!(content["body"], "Native file from the original workspace");
        assert_eq!(content["filename"], "原始文件.bin");
        assert_eq!(content["info"]["size"], DATA.len());
        assert_eq!(content["info"]["mimetype"], "application/octet-stream");
        if direct {
            assert!(content.get("m.relates_to").is_none());
        } else {
            assert_eq!(content["m.relates_to"]["rel_type"], "m.thread");
            assert_eq!(content["m.relates_to"]["event_id"], "$task_thread");
        }
        let mut descriptor = content["file"].clone();
        assert_eq!(
            descriptor.as_object_mut().unwrap().remove("url"),
            Some(json!("mxc://example.test/native-file"))
        );
        let descriptor = hagency_media::Descriptor::from_private_event_json(
            &serde_json::to_vec(&descriptor).unwrap(),
        )
        .unwrap();
        let codec = hagency_media::Codec::new(hagency_media::Limits::new(4096, 2).unwrap());
        let bytes = f.ciphertext.as_ref().unwrap();
        assert_eq!(codec.decrypt(&descriptor, bytes).unwrap().bytes(), DATA);
        let status = f.wait_result().await;
        assert_eq!(status["workspace_registered"], true);
        assert_eq!(status["protocol"], "completed");
        let capabilities = f.capabilities().await;
        assert_eq!(capabilities["agent_execution"], false);
        assert_eq!(capabilities["production_api_parity"], false);
        f.fake.no_request().await;
        // Explicit process teardown: this assertion does not claim qualified
        // shutdown on platforms whose process-tree census is still unknown.
        child.stop_and_reap();
        f.fake.close().await;
    }
}

/// Actual native MCP outcome after the original TLS request may have reached
/// the peer. A decrypted recipient event still needs a complete server ACK.
#[tokio::test]
async fn native_file_service_uncertainty() {
    for event in [false, true] {
        let mut f = Fixture::new(true).await;
        let child = f.launch(
            true,
            if event {
                "uncertainty.event"
            } else {
                "uncertainty.upload"
            },
        );
        let held = f.pause_write(event).await;
        assert_eq!(f.delivered_count(), 0);
        assert_eq!(f.attempts(), 1);
        let status = f.capabilities().await["development_execution"].clone();
        assert_eq!(status["workspace_registered"], true);
        let states: (String, String) = f.sql().query_row(
            "SELECT u.upload_state,f.event_state FROM file_deliveries f JOIN file_uploads u ON u.id=f.upload_id",
            [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!(
            states,
            if event {
                ("accepted".into(), "write_possible".into())
            } else {
                ("write_possible".into(), "pending".into())
            }
        );
        if event {
            assert_eq!(f.peer.events.len(), 1);
            assert!(f.peer.events[0]["content"].get("m.relates_to").is_none());
        }
        // A deliberately longer Content-Length and no clean TLS EOF mean no
        // complete upload/event response can be accepted. Never retry this write.
        let partial = if event {
            "{\"event_id\":"
        } else {
            "{\"content_uri\":"
        };
        held.unclean(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 80\r\nConnection: close\r\n\r\n{partial}").into_bytes());
        // The ordinary offline peer continues its bounded status wait. Its
        // original runtime may time out; that negative result is not helper
        // success or proof that a possible external write did not happen.
        let status = f.wait_result().await;
        assert_eq!(status["state"], "outcome_unknown");
        assert_eq!(status["settlement"], "negative");
        let admission: serde_json::Value =
            serde_json::from_slice(&std::fs::read(f.work.join("file-mcp.admission")).unwrap())
                .unwrap();
        assert_eq!(admission["isError"], false);
        let id = admission["structuredContent"]["delivery_id"]
            .as_str()
            .unwrap();
        let after: (String, String, bool, bool) = f.sql().query_row(
            "SELECT u.upload_state,f.event_state,f.cancel_requested,u.cancel_requested FROM file_deliveries f JOIN file_uploads u ON u.id=f.upload_id WHERE f.id=?1",
            [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
        assert_eq!((&after.0, &after.1), (&states.0, &states.1));
        assert!(
            after.2 && after.3,
            "the original failed operation's negative observation must be committed"
        );
        // The same privately captured inherited context reaches a fresh real
        // native MCP process. Historical authority cannot restart the runtime.
        let receipt = f.historical_file(id).await;
        assert_eq!(receipt["isError"], false);
        assert_eq!(receipt["structuredContent"]["delivery_id"], id);
        assert_eq!(receipt["structuredContent"]["status"], "outcome_unknown");
        assert_eq!(
            receipt["structuredContent"]["error_code"],
            "outcome_unknown"
        );
        assert_eq!(f.delivered_count(), 0);
        assert_eq!(f.uploads, 1);
        assert_eq!(f.event_puts, usize::from(event));
        assert_eq!(f.peer.events.len(), usize::from(event));
        assert_eq!(f.peer.shares, usize::from(event));
        assert_eq!(f.peer.claims, 1);
        assert_eq!(f.peer.writes.len(), 5);
        let row: (u64, Option<String>) = f
            .sql()
            .query_row("SELECT COUNT(*),acceptance FROM file_deliveries", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(row, (1, None));
        let task: String = f
            .sql()
            .query_row(
                "SELECT json_extract(config,'$.status') FROM canonical_tasks WHERE id='task'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(task, "in_progress");
        f.fake.no_request().await;
        child.stop_and_reap();
        f.fake.close().await;
    }
    // The configured DM is invalidated through its actual initial Matrix state,
    // before any claim or source handoff. No DB availability/capability setter.
    let mut f = Fixture::new(true).await;
    let child = f.launch(true, "uncertainty.negative_room");
    let until = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut observed = false;
    for _ in 0..16 {
        let request = tokio::time::timeout_at(until, f.next("uncertainty.negative_room"))
            .await
            .unwrap();
        assert_eq!(
            request.method, "GET",
            "negative room must precede key or file writes"
        );
        if request.target.starts_with("/_matrix/client/v3/rooms/")
            && request.target.ends_with("/state")
        {
            assert_eq!(
                request.headers["authorization"],
                format!("Bearer {}", fixture::common::TOKEN)
            );
            let mut state = fixture::common::state();
            let member = state
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|v| v["type"] == "m.room.member" && v["state_key"] == "@owner:example.test")
                .unwrap();
            member["content"]["membership"] = json!("leave");
            request.json(200, state);
            observed = true;
            break;
        }
        f.respond(request).await;
    }
    assert!(observed);
    let status = f.wait_result().await;
    assert_eq!(status["state"], "unavailable");
    assert_eq!(status["error"], "refresh");
    assert_eq!(status["workspace_registered"], false);
    let available: bool = f
        .sql()
        .query_row(
            "SELECT available FROM matrix_room_scopes WHERE room_id=?1",
            [&f.room],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        !available,
        "actual negative room observation must be committed"
    );
    // Actual room reconciliation supersedes unstarted work; it never issues a claim.
    assert_eq!(f.state(), "superseded");
    assert_eq!(f.attempts(), 0);
    assert!(!f.work.join("file-mcp.requests").exists());
    assert_eq!(f.uploads, 0);
    assert_eq!(f.event_puts, 0);
    assert_eq!(f.delivered_count(), 0);
    assert!(f.peer.writes.is_empty() && f.peer.events.is_empty());
    f.fake.no_request().await;
    child.stop_and_reap();
    f.fake.close().await;
}
