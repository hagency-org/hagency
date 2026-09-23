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
    let f = Fixture::profile(false, false, true).await;
    project_mentions(f).await;
}
/// ADR178 superseded-plan amendment. The later agent's join advances the shared
/// project's generation while the earlier agent's poll sits between resolving
/// its inbox plan and selecting it. That plan is superseded, not refused: the
/// earlier agent must keep running, re-resolve at the new generation and then
/// serve its own project mention like the later one.
#[cfg(unix)]
#[tokio::test]
async fn native_configured_fleet_earlier_agent_survives_later_join() {
    let mut f = Fixture::profile(false, false, true).await;
    f.peer.interleave_later_join_with_first_agent_poll();
    f.until(
        "the later join advanced the project generation under the first agent's held poll",
        |f| f.peer.holding_first_agent_poll() && f.project_generation() >= 3,
    )
    .await;
    f.peer.release_first_agent_poll().await;
    project_mentions(f).await;
}
#[cfg(unix)]
async fn project_mentions(mut f: Fixture) {
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
    // Each agent is ASKED only what addressed it: the other participant's
    // near-identical request is not in its inbox, where live it got carried
    // out. It still LISTENS to the whole room — the discussion is frozen for
    // the dispatch and the agent read it back through read_conversation, with
    // the speaker named.
    for index in 0..2 {
        let receipt = f.receipt(index, "fleet-ready");
        let input: Value = serde_json::from_str(receipt["input"].as_str().unwrap()).unwrap();
        let inbox = input["inbox"].as_array().unwrap();
        assert_eq!(inbox.len(), 1, "only the addressed request is asked of it");
        assert_eq!(
            inbox[0]["message"]["body"],
            format!("PROJECT_ADDRESSED_{index}")
        );
        assert_eq!(inbox[0]["wake"], true);
        let page = &receipt["discussion"];
        assert_eq!(page["total_messages"], input["discussion"]["message_count"]);
        assert_eq!(page["next"], Value::Null);
        let bodies: Vec<&str> = page["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|part| part["body"].as_str().unwrap())
            .collect();
        assert!(
            bodies.contains(&"PROJECT_UNADDRESSED")
                && bodies.contains(&format!("PROJECT_ADDRESSED_{index}").as_str()),
            "the frozen room is reachable: {bodies:?}"
        );
        for part in page["messages"].as_array().unwrap() {
            assert_eq!(part["sender"], OWNER);
            assert_eq!(part["sender_name"], "project owner");
        }
    }
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
/// ADR180 delivery of a delegated task. The owner asks one agent; that agent
/// hands the work to its colleague with `delegate_task`, the owner approves the
/// one card the call raises, and the ASSIGNEE — not the delegator — announces
/// the delegated task in its own identity, is dispatched by its own driver and
/// answers in the delegator's thread. Nothing is hidden from either agent: the
/// unaddressed participant sees the question exactly as everyone else does.
#[cfg(unix)]
#[tokio::test]
async fn native_configured_fleet_delegated_task_delivery() {
    let mut f = Fixture::delegating().await;
    f.until("both original private and project inboxes",|f|f.count("SELECT COUNT(*) FROM current_matrix_routes r JOIN runner_sessions s ON s.id=r.session_id JOIN engagements e ON e.id=s.engagement_id WHERE e.request_id LIKE 'fleet_target_%'")==4
        && (0..2).all(|i|f.work(i).join("owned-mcp.warm-initialized").is_file())).await;
    f.wait_for_registered_agents().await;
    f.assert_project_scope();
    // Two disposable fixture channels, neither one domain scope: the marker is
    // the peer's own independent expectation of the offered tool group, and no
    // tool tells an agent a colleague's engagement ID (ADR180), so the test
    // names the assignee the way it names every other scripted choice.
    for index in 0..2 {
        fs::write(
            f.work(index).join("owned-mcp.coordination"),
            b"ADR180 group offered",
        )
        .unwrap();
    }
    fs::write(f.work(1).join("owned-mcp.delegate-to"), engagement(0)).unwrap();
    f.peer.queue_delegation_request();
    f.until("the question reached both participants and woke one",|f|f.count("SELECT COUNT(*) FROM session_inputs WHERE json_extract(config,'$.body')='PROJECT_DELEGATION'")==2
        && f.try_receipt(1,"fleet-ready").is_some_and(|r|f.task_started(r["task_id"].as_str().unwrap()))).await;
    let source = f.receipt(1, "fleet-ready")["task_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(f.count("SELECT COUNT(*) FROM runner_dispatches"), 1);
    assert!(
        f.try_receipt(0, "fleet-ready").is_none(),
        "an unaddressed participant admits the question without being woken"
    );
    assert_eq!(f.count("SELECT COUNT(*) FROM session_inputs WHERE json_extract(config,'$.body')='PROJECT_DELEGATION' AND wake=0"),1);
    fs::write(f.work(1).join("owned-mcp.fleet-release"), b"delegate now").unwrap();
    f.until("the owner approved the one delegation card", |f| {
        f.work(1).join("owned-mcp.fleet-delegated").is_file()
    })
    .await;
    let created = f.receipt(1, "fleet-delegated");
    let task = created["task_id"].as_str().unwrap().to_owned();
    let session = created["session_id"].as_str().unwrap().to_owned();
    assert_eq!(created["activation"], "pending");
    assert_ne!(task, source);
    assert_eq!(f.peer.approval.events.len(), 1);
    assert_eq!(f.peer.verdicts, 1);
    f.until("the assignee posted its own task notice", |f| {
        !f.peer.agents[0].project_events.is_empty()
    })
    .await;
    f.until("Matrix acceptance activated the intent", |f| {
        f.intent_state(&task) == "active"
    })
    .await;
    f.until(
        "the assignee's own driver dispatched the delegated task",
        |f| {
            f.try_receipt(0, "fleet-ready")
                .is_some_and(|r| r["task_id"].as_str() == Some(task.as_str()))
                && f.task_started(&task)
        },
    )
    .await;
    // The assignee is told who it is and shown the request as the room saw it.
    let input: Value =
        serde_json::from_str(f.receipt(0, "fleet-ready")["input"].as_str().unwrap()).unwrap();
    assert_eq!(input["agent"]["mxid"], f.peer.agents[0].user);
    assert_eq!(input["agent"]["name"], "FleetAgent0");
    assert_eq!(input["inbox"][0]["message"]["event_id"], DELEGATION_EVENT);
    assert_eq!(input["inbox"][0]["message"]["body"], "PROJECT_DELEGATION");
    assert_eq!(input["inbox"][0]["message"]["sender_mxid"], OWNER);
    // Like a human assignee it has the task card and knows who handed it over.
    assert_eq!(input["task"]["id"], task.as_str());
    assert_eq!(input["task"]["title"], "Delegated fleet report");
    assert_eq!(
        input["task"]["description"],
        "Draft the report and reply with it."
    );
    assert_eq!(input["delegated_by"]["mxid"], f.peer.agents[1].user);
    assert_eq!(input["delegated_by"]["name"], "FleetAgent1");
    assert!(
        input["instruction"]
            .as_str()
            .unwrap()
            .contains("The participant named in delegated_by handed you the work in task")
    );
    fs::write(
        f.work(0).join("owned-mcp.fleet-release"),
        b"answer the delegated task",
    )
    .unwrap();
    f.until("both original replies delivered", |f| {
        f.count("SELECT COUNT(*) FROM final_replies WHERE state='delivered'") == 2
    })
    .await;
    // The notice is the assignee's own post, in the delegator's thread, and it
    // addresses nobody: the delegation, not a mention, is its authority. The
    // fake files a project event under the agent whose access token sent it, so
    // reading it from the assignee's log is the identity evidence.
    let notice = &f.peer.agents[0].project_events[0];
    assert_eq!(notice["sender"], f.peer.agents[0].user);
    assert_eq!(notice["content"]["msgtype"], "m.notice");
    assert_eq!(
        notice["content"]["body"],
        "Task created: Delegated fleet report"
    );
    let relation = &notice["content"]["m.relates_to"];
    assert_eq!(relation["rel_type"], "m.thread");
    assert_eq!(relation["event_id"], DELEGATION_EVENT);
    assert_eq!(relation["m.in_reply_to"]["event_id"], DELEGATION_EVENT);
    assert!(notice["content"].get("m.mentions").is_none());
    assert_eq!(
        f.peer
            .agents
            .iter()
            .flat_map(|agent| &agent.project_events)
            .filter(|event| event["content"]["msgtype"] == "m.notice")
            .count(),
        1,
        "one claim is never sent twice"
    );
    assert_eq!(f.peer.agents[1].project_events.len(), 1);
    assert_eq!(
        f.peer.agents[1].project_events[0]["content"]["msgtype"],
        "m.text"
    );
    assert_eq!(f.intent_state(&task), "active");
    assert_eq!(f.task_status(&task), "done");
    assert!(f.task_reply_delivered(&task));
    // Exactly one dispatch, on the assignee's own delegated session; the
    // delegator's project session never held work for this task.
    assert_eq!(f.task_session(&task), session);
    assert_eq!(f.task_dispatch_sessions(&task), vec![session.clone()]);
    let delegator = f.task_session(&source);
    assert!(delegator.starts_with(&format!("project_{}_1_", engagement(1))));
    assert_ne!(delegator, session);
    assert_eq!(f.count("SELECT COUNT(*) FROM runner_dispatches"), 2);
    assert_eq!(f.count("SELECT COUNT(*) FROM canonical_tasks"), 2);
    let route = f.reply_route(&task);
    assert_eq!(route["engagement_id"], engagement(0));
    assert_eq!(route["room_id"], PROJECT);
    assert_eq!(route["thread_root"], DELEGATION_EVENT);
    let reply = &f.peer.agents[0].project_events[1];
    assert_eq!(
        reply["content"]["body"],
        format!("Verified factory task {task}")
    );
    assert_eq!(
        reply["content"]["m.relates_to"]["event_id"],
        DELEGATION_EVENT
    );
    assert!(
        f.peer
            .agents
            .iter()
            .all(|agent| agent.crypto.events.is_empty())
    );
    assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
    assert_eq!(f.count("SELECT COUNT(*) FROM runner_attempts"), 2);
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
    // ADR-181: the refusal is kept with each attempt, uncollapsed — the check
    // that lost the authority and what it said, not just "lost_authority".
    assert_eq!(
        f.count(
            "SELECT COUNT(*) FROM runner_attempt_events WHERE phase='failed' \
             AND json_extract(detail,'$.status.owned_failure')='lost_authority' \
             AND json_extract(detail,'$.status.authority_site')='local_codex_check' \
             AND json_extract(detail,'$.status.authority_cause')='io'"
        ),
        2,
        "both refusals name their site and cause"
    );
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
