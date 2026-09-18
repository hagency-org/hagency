//! Actual configured service process and two genuine inline factories. All
//! network traffic stays on the local TLS fixture; no live accounts or models.
#[path = "configured_fleet/mod.rs"]
mod configured_fleet;
#[path = "fixtures/matrix_crypto_peer.rs"]
mod crypto;
#[path = "../../hagency-matrix/tests/common/mod.rs"]
mod matrix;
use configured_fleet::*;
use serde_json::{Value, json};
use std::fs;

#[tokio::test]
async fn native_configured_paced_startup() {
    for budget in [None, Some(60_000)] {
        let mut f = Fixture::paced_startup(budget).await;
        let started = tokio::time::Instant::now();
        assert_eq!(f.startup_result().await, budget.is_some());
        assert!(started.elapsed() >= std::time::Duration::from_secs(20));
        assert_eq!(f.count("SELECT COUNT(*) FROM runner_dispatches"), 0);
        assert_eq!(f.count("SELECT COUNT(*) FROM canonical_tasks"), 0);
        assert!(f.peer.agents.iter().all(|agent| agent.account_posts == 0));
        if budget.is_some() {
            assert_eq!(
                f.peer.approval.writes.len(),
                5,
                "one original enrollment, no replay"
            );
            f.stop().await;
        } else {
            assert!(!f.peer.approval.writes.is_empty());
            assert!(f.peer.approval.writes.len() < 5);
        }
        f.fake.close().await;
    }
}

#[tokio::test]
#[cfg_attr(
    not(any(target_os = "linux", target_os = "macos")),
    ignore = "positive qualification requires supported whole-tree observations, not compilation or leader-only cleanup"
)]
async fn native_configured_fleet_executable_two_agents() {
    qualify(false).await;
}
#[tokio::test]
#[cfg_attr(
    not(any(target_os = "linux", target_os = "macos")),
    ignore = "positive qualification requires supported whole-tree observations, not compilation or leader-only cleanup"
)]
async fn native_configured_fleet_media_two_agents() {
    qualify(true).await;
}
#[cfg(unix)]
#[tokio::test]
async fn native_configured_local_codex_fleet() {
    qualify_profile(true, true).await;
}
#[cfg(unix)]
#[tokio::test]
async fn native_configured_fleet_project_mentions() {
    let mut f = Fixture::profile(false, false, true).await;
    f.until("both original private and project inboxes",|f|f.count("SELECT COUNT(*) FROM current_matrix_routes r JOIN runner_sessions s ON s.id=r.session_id JOIN engagements e ON e.id=s.engagement_id WHERE e.request_id LIKE 'fleet_target_%'")==4
        && (0..2).all(|i|f.work(i).join("owned-mcp.warm-initialized").is_file())).await;
    f.wait_for_registered_agents().await;
    f.assert_project_scope();
    assert_eq!(f.count("SELECT COUNT(*) FROM canonical_tasks"), 0);
    f.peer.queue_project_mentions(false);
    f.until("unaddressed project input admitted without wake",|f|f.count("SELECT COUNT(*) FROM session_inputs WHERE json_extract(config,'$.body')='PROJECT_UNADDRESSED' AND wake=0")==2).await;
    assert_eq!(f.count("SELECT COUNT(*) FROM canonical_tasks"), 0);
    f.peer.queue_project_mentions(true);
    f.until("both exact project mentions executing", |f| {
        (0..2).all(|i| {
            f.try_receipt(i, "fleet-ready")
                .is_some_and(|r| f.task_started(r["task_id"].as_str().unwrap()))
        })
    })
    .await;
    assert_eq!(f.count("SELECT COUNT(*) FROM runner_dispatches"), 2);
    assert_eq!(f.count("SELECT COUNT(*) FROM session_inputs WHERE json_extract(config,'$.body') LIKE 'PROJECT_ADDRESSED_%' AND wake=1"),2);
    assert_eq!(f.count("SELECT COUNT(*) FROM session_inputs WHERE json_extract(config,'$.body') LIKE 'PROJECT_ADDRESSED_%' AND wake=0"),2);
    let tasks: Vec<String> = (0..2)
        .map(|i| {
            f.receipt(i, "fleet-ready")["task_id"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_ne!(tasks[0], tasks[1]);
    for index in 0..2 {
        fs::write(
            f.work(index).join("owned-mcp.fleet-release"),
            b"original project task observed",
        )
        .unwrap();
    }
    f.until("project replies delivered", |f| {
        f.count("SELECT COUNT(*) FROM final_replies WHERE state='delivered'") == 2
            && f.peer
                .agents
                .iter()
                .all(|agent| agent.project_events.len() == 1)
    })
    .await;
    for (index, task) in tasks.iter().enumerate() {
        f.assert_project_task(index, task);
        assert_eq!(f.task_status(task), "done");
        assert!(f.task_reply_delivered(task));
        let event = &f.peer.agents[index].project_events[0];
        assert_eq!(event["sender"], f.peer.agents[index].user);
        assert_eq!(
            event["content"]["body"],
            format!("Verified factory task {task}")
        );
        assert!(event["content"].get("m.relates_to").is_none());
        assert!(f.peer.agents[index].crypto.events.is_empty());
        for name in ["owned-mcp.fleet-release", "owned-mcp.fleet-ready"] {
            fs::remove_file(f.work(index).join(name)).unwrap();
        }
    }
    assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
    f.peer.queue_owner_round(1).await;
    f.until("private DM tasks after project work", |f| {
        (0..2).all(|i| {
            f.try_receipt(i, "fleet-ready")
                .is_some_and(|r| f.task_started(r["task_id"].as_str().unwrap()))
        })
    })
    .await;
    let private: Vec<String> = (0..2)
        .map(|i| {
            f.receipt(i, "fleet-ready")["task_id"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    for (index, task) in private.iter().enumerate() {
        assert_eq!(
            f.task_session(task),
            format!("session_{}", engagement(index))
        );
        assert!(!tasks.contains(task));
        fs::write(
            f.work(index).join("owned-mcp.fleet-release"),
            b"original private task observed",
        )
        .unwrap();
    }
    f.until("private replies independently decrypted", |f| {
        f.count("SELECT COUNT(*) FROM final_replies WHERE state='delivered'") == 4
            && f.peer
                .agents
                .iter()
                .all(|agent| agent.crypto.events.len() == 1)
    })
    .await;
    for (index, task) in private.iter().enumerate() {
        assert_eq!(f.task_status(task), "done");
        assert!(f.task_reply_delivered(task));
        assert_eq!(
            f.peer.agents[index].crypto.events[0]["content"]["body"],
            format!("Verified factory task {task}")
        );
        assert_eq!(
            f.peer.agents[index].project_events.len(),
            1,
            "private reply must not reach shared project"
        );
    }
    assert_eq!(f.count("SELECT COUNT(*) FROM runner_attempts"), 4);
    assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
    assert_eq!(
        f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='outcome_unknown'"),
        0
    );
    f.assert_ready().await;
    f.stop().await;
    f.fake.close().await;
}
#[cfg(unix)]
#[tokio::test]
async fn native_configured_fleet_handoff_diagnostics() {
    let mut f = Fixture::profile(false, false, true).await;
    f.until("both original warm owners admitted",|f|f.count("SELECT COUNT(*) FROM current_matrix_routes r JOIN runner_sessions s ON s.id=r.session_id JOIN engagements e ON e.id=s.engagement_id WHERE e.request_id LIKE 'fleet_target_%' AND json_extract(s.binding,'$.room_id') LIKE '!fleet_dm_%'")==2
        && (0..2).all(|i|f.work(i).join("owned-mcp.warm-initialized").is_file())).await;
    f.wait_for_registered_agents().await;
    // Real original provider-directory revocation, never an injected diagnostic.
    f.revoke_local_provider_permissions();
    f.peer.queue_owner_round(1).await;
    f.until("both original handoffs refused", Fixture::handoffs_refused)
        .await;
    f.assert_handoff_failures().await;
    assert_eq!(f.count("SELECT COUNT(*) FROM runner_dispatches"), 2);
    assert_eq!(
        f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='started'"),
        0
    );
    assert_eq!(f.count("SELECT COUNT(*) FROM final_replies"), 0);
    assert_eq!(f.count("SELECT COUNT(*) FROM owned_stop_inspections"), 0);
    for index in 0..2 {
        let requests: Vec<Value> = fs::read_to_string(f.work(index).join("owned-mcp.requests"))
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r["method"] == "initialize")
                .count(),
            1
        );
        assert!(
            !requests
                .iter()
                .any(|r| r["method"] == "thread/start" || r["method"] == "turn/start")
        );
    }
    // Fixture Drop tears down the deliberately refused disposable service;
    // there is no successful product shutdown or cleanup claim here.
}
async fn qualify(media: bool) {
    qualify_profile(media, false).await;
}
async fn qualify_profile(media: bool, local: bool) {
    for application_service in if local {
        vec![false]
    } else {
        vec![false, true]
    } {
        let mut f = if local {
            Fixture::profile(application_service, media, true).await
        } else {
            Fixture::new(application_service, media).await
        };
        f.until("two genuine Active factories",|f|f.count("SELECT COUNT(*) FROM engagements WHERE request_id LIKE 'fleet_target_%' AND state='active'")==2
            && f.count("SELECT COUNT(*) FROM current_approval_bindings b JOIN engagements e ON e.id=b.engagement_id WHERE e.request_id LIKE 'fleet_target_%'")==2
            && f.count("SELECT COUNT(*) FROM current_matrix_routes r JOIN runner_sessions s ON s.id=r.session_id JOIN engagements e ON e.id=s.engagement_id WHERE e.request_id LIKE 'fleet_target_%' AND json_extract(s.binding,'$.room_id') LIKE '!fleet_dm_%'")==2
            && (0..2).all(|i|f.work(i).join("owned-mcp.warm-initialized").is_file())).await;
        assert_eq!(f.count("SELECT COUNT(*) FROM canonical_tasks"), 0);
        assert_eq!(
            f.count("SELECT COUNT(*) FROM effects WHERE state='complete'"),
            3,
            "one external coordinator plus two physical targets"
        );
        assert_eq!(f.count("SELECT COUNT(*) FROM current_approval_bindings"), 3);
        f.assert_project_scope();
        let mut original = Vec::new();
        for index in 0..2 {
            let value = f.receipt(index, "warm-initialized");
            if local {
                f.assert_local_provider(&value);
            } else {
                assert_eq!(value["home"], json!(f.work(index).parent().unwrap()));
                assert_eq!(value["codex_home"], value["home"]);
            }
            original.push(value["pid"].clone());
        }
        assert_ne!(original[0], original[1]);
        assert_ne!(f.work(0), f.work(1));
        for round in 1..=2 {
            // Genuine owner encryption enters the real SDK/intake path. No
            // task/session/claim/Started/completion API is called by the test.
            f.peer.queue_owner_round(round).await;
            f.until("both original native helpers are in flight", |f| {
                (0..2).all(|i| {
                    f.work(i).join("owned-mcp.fleet-ready").is_file()
                        && f.try_receipt(i, "fleet-ready").is_some_and(|r| {
                            let task = r["task_id"].as_str().unwrap();
                            f.task_started(task)
                        })
                })
            })
            .await;
            assert_eq!(
                f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='started'"),
                2
            );
            assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 2);
            let tasks: Vec<String> = (0..2)
                .map(|i| {
                    f.receipt(i, "fleet-ready")["task_id"]
                        .as_str()
                        .unwrap()
                        .to_owned()
                })
                .collect();
            assert_ne!(tasks[0], tasks[1]);
            for (index, task) in tasks.iter().enumerate() {
                if local {
                    f.assert_local_provider(&f.receipt(index, "warm-initialized"));
                }
                let pid = f.receipt(index, "fleet-ready")["pid"].clone();
                if round == 1 {
                    assert_eq!(
                        pid, original[index],
                        "first dispatch must use original warm process"
                    );
                } else {
                    assert_ne!(
                        pid, original[index],
                        "second turn is admitted by the retained original factory after cleanup"
                    );
                }
                assert_eq!(
                    f.task_session(task),
                    format!("session_{}", engagement(index))
                );
                fs::write(
                    f.work(index).join("owned-mcp.fleet-release"),
                    b"both original helpers observed",
                )
                .unwrap();
            }
            f.until(
                "both canonical results delivered and independently decrypted",
                |f| {
                    f.count("SELECT COUNT(*) FROM final_replies WHERE state='delivered'")
                        == round * 2
                        && f.peer.agents.iter().all(|agent| {
                            agent.crypto.events.len() == round as usize * if media { 2 } else { 1 }
                        })
                },
            )
            .await;
            for (index, task) in tasks.iter().enumerate() {
                let position = round as usize * if media { 2 } else { 1 } - 1;
                let event = &f.peer.agents[index].crypto.events[position];
                assert_eq!(
                    event["content"]["body"],
                    format!("Verified factory task {task}")
                );
                assert_eq!(event["sender"], f.peer.agents[index].user);
                assert_eq!(
                    event["content"].get("m.relates_to"),
                    None,
                    "owner DM is not another agent's thread"
                );
                assert_eq!(f.task_status(task), "done");
                // Canonical completion can retire the process before its local
                // ACK/exit diagnostic is written. A previous round's file is
                // not evidence for this round; inspect the exact writer chain.
                assert!(f.task_reply_delivered(task));
                if media {
                    f.assert_file_delivery(index, round as usize - 1, task);
                }
                if round == 1 {
                    // These two exact files belong only to the disposable
                    // fixture protocol. Removing them grants no domain scope.
                    fs::remove_file(f.work(index).join("owned-mcp.fleet-release")).unwrap();
                    fs::remove_file(f.work(index).join("owned-mcp.fleet-ready")).unwrap();
                }
            }
            assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
            assert_eq!(
                f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='outcome_unknown'"),
                0
            );
        }
        for index in 0..2 {
            let agent = &f.peer.agents[index];
            assert_eq!(agent.account_posts, if application_service { 2 } else { 1 });
            assert_eq!(agent.room_posts, 3);
            assert_eq!(
                agent.crypto.writes.len(),
                5,
                "no replacement SDK enrollment"
            );
            let requests: Vec<Value> = fs::read_to_string(f.work(index).join("owned-mcp.requests"))
                .unwrap()
                .lines()
                .map(|s| serde_json::from_str(s).unwrap())
                .collect();
            assert_eq!(
                requests
                    .iter()
                    .filter(|r| r["method"] == "initialize")
                    .count(),
                2
            );
        }
        assert_eq!(f.count("SELECT COUNT(*) FROM runner_attempts"), 4);
        assert_eq!(f.count("SELECT COUNT(*) FROM owned_task_completions"), 4);
        assert_eq!(f.count("SELECT COUNT(*) FROM canonical_tasks"), 4);
        f.assert_project_scope();
        f.assert_ready().await;
        f.stop().await;
        assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
        assert_eq!(
            f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='outcome_unknown'"),
            0
        );
        f.fake.close().await;
    }
}
