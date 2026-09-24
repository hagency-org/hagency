//! Genuine target factory activation on offline TLS + actual native pipes/API.
//! The preexisting external bootstrap fixture is not the newly provisioned agent.
#[path = "fixtures/matrix_crypto_peer.rs"]
mod crypto;
#[path = "../../hagency-matrix/tests/common/mod.rs"]
mod matrix;
#[path = "inline_factory/mod.rs"]
mod support;
use hagency_core::tasks::*;
use hagency_matrix::{CancellationToken, HostIntakePlan};
use std::{fs, future::Future, task::Poll, time::Duration};
use support::*;

#[tokio::test]
async fn native_configured_fleet_original_handoff() {
    for application_service in [false, true] {
        let mut f = Fixture::new(application_service).await;
        assert!(f.collector.take_next_provisioned_agent().unwrap().is_none());
        f.provision().await;
        let original = f.original_owner().await;
        assert_eq!(
            original["home"],
            serde_json::json!(f.work().parent().unwrap().canonicalize().unwrap())
        );
        assert_eq!(original["codex_home"], original["home"]);
        let agent = f.collector.take_next_provisioned_agent().unwrap().unwrap();
        let shared = agent.shared_collector();
        assert!(
            std::ptr::eq(shared.as_ref(), agent.collector()),
            "same original Collector wrapper/SDK"
        );
        assert_eq!(agent.session().engagement_id, f.engagement());
        assert!(f.collector.take_next_provisioned_agent().unwrap().is_none());
        assert!(f.collector.take_provisioned_agent(&f.engagement()).is_err());
        assert_eq!(f.original_owner().await["pid"], original["pid"]);
        agent.close().await.unwrap();
        assert!(f.collector.take_next_provisioned_agent().unwrap().is_none());
        f.collector.close_provisioned_agents().await.unwrap();
        assert!(f.collector.take_next_provisioned_agent().is_err());
        f.close().await;
    }
}

#[tokio::test]
async fn native_configured_fleet_recurring_driver() {
    for application_service in [false, true] {
        let mut f = Fixture::new_service(application_service).await;
        f.provision().await;
        let original = f.original_owner().await;
        // Queue canonical work only. The actual service, not this test,
        // discovers the agent, selects its profile and claims/starts dispatch.
        for n in 1..=2 {
            let task = format!("service_task_{n}");
            f.base
                .store
                .create_canonical_task(
                    task.clone(),
                    format!("session_{}", f.engagement()),
                    format!("Service task {n}"),
                    now(),
                )
                .await
                .unwrap();
            f.base.store.enqueue_dispatch(DispatchInput {id:format!("service_dispatch_{n}"),session_id:format!("session_{}",f.engagement()),task_id:Some(task),
                resources:vec![ResourceLease {id:format!("work_{}",f.engagement()),exclusive:true}],payload:serde_json::json!({"instruction":"offline actual helper heartbeat/readback"})}).await.unwrap();
        }
        fs::write(f.work().join("owned-mcp.sequential"), b"offline probe only").unwrap();
        fs::write(
            f.work().join("owned-mcp.fleet-files"),
            b"offline probe only",
        )
        .unwrap();
        let mut fleet = f.fleet.take().unwrap();
        let cancel = CancellationToken::new();
        let (notices, mut receiver) = tokio::sync::mpsc::channel(1);
        let mut runner = Box::pin(fleet.run(notices, &cancel));
        let mut sources = Vec::new();
        let mut first_pid = None;
        tokio::time::timeout(Duration::from_secs(12), async {
            loop {
                tokio::select! {
                    result=&mut runner=>panic!("service stopped early: {result:?}"),
                    request=f.fake.next()=>f.peer.respond(request,&f.base).await,
                    Some(notices)=receiver.recv()=>sources.push(notices),
                    _=tokio::time::sleep(Duration::from_millis(10))=>{},
                }
                // Capture before servicing the next iteration's authenticated
                // refresh. The sequential fixture intentionally overwrites
                // its latest-stage receipt when the second runtime starts.
                if first_pid.is_none() && f.work().join("owned-mcp.warm-thread").exists() {
                    first_pid = Some(f.receipt("warm-thread")["pid"].clone());
                }
                #[cfg(any(target_os = "linux", target_os = "macos"))]
                if f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='completed'") == 2 {
                    break;
                }
                #[cfg(not(any(target_os = "linux", target_os = "macos")))]
                if f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='outcome_unknown'")
                    == 1
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
        cancel.cancel();
        runner.await.unwrap();
        assert_eq!(first_pid.unwrap(), original["pid"]);
        assert!(f.collector.take_next_provisioned_agent().unwrap().is_none());
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            assert_eq!(f.receipt("readback")["task"]["id"], "service_task_2");
            assert_eq!(
                f.requests()
                    .iter()
                    .filter(|r| r["method"] == "initialize")
                    .count(),
                2
            );
            assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            assert_eq!(f.receipt("readback")["task"]["id"], "service_task_1");
            assert_eq!(
                f.requests()
                    .iter()
                    .filter(|r| r["method"] == "initialize")
                    .count(),
                1
            );
            assert_eq!(
                f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='queued'"),
                1
            );
            assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 1);
        }
        assert_eq!(
            f.count("SELECT COUNT(*) FROM final_replies"),
            0,
            "heartbeat is not canonical Done"
        );
        assert_eq!(f.receipt("fleet-files")["delivery"]["status"], "failed");
        assert_eq!(
            f.receipt("fleet-files")["delivery"]["error_code"],
            "source_refused"
        );
        assert_eq!(
            f.receipt("fleet-files")["list"]["items"],
            serde_json::json!([])
        );
        drop(sources);
        // Continue serving any original refresh already admitted before cancel.
        {
            let closing = fleet.close();
            tokio::pin!(closing);
            loop {
                tokio::select! {result=&mut closing=>{
                    #[cfg(any(target_os="linux",target_os="macos"))] assert_eq!(result,Ok(()));
                    #[cfg(not(any(target_os="linux",target_os="macos")))] assert_eq!(result,Err(hagency::bootstrap::Failure::OutcomeUnknown));
                    break;
                },request=f.fake.next()=>f.peer.respond(request,&f.base).await}
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            assert_eq!(
                fleet.close().await,
                Err(hagency::bootstrap::Failure::OutcomeUnknown)
            );
            assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 1);
            assert_eq!(
                serde_json::to_value(fleet.statuses()).unwrap()[0]["state"],
                "outcome_unknown"
            );
        }
        // Test teardown may abandon the negative owner; this is not a
        // successful service close or authority to clear its retained lease.
        drop(fleet);
        f.close().await;
    }
}
use serde_json::json;

async fn queue_service_task(f: &Fixture, n: u32) {
    let task = format!("service_task_{n}");
    f.base
        .store
        .create_canonical_task(
            task.clone(),
            format!("session_{}", f.engagement()),
            format!("Service task {n}"),
            now(),
        )
        .await
        .unwrap();
    f.base
        .store
        .enqueue_dispatch(DispatchInput {
            id: format!("service_dispatch_{n}"),
            session_id: format!("session_{}", f.engagement()),
            task_id: Some(task),
            resources: vec![ResourceLease {
                id: format!("work_{}", f.engagement()),
                exclusive: true,
            }],
            payload: json!({"instruction":"offline actual helper heartbeat/readback"}),
        })
        .await
        .unwrap();
}
/// Run the fleet until `done(completed dispatches, this agent's fleet row)`,
/// then stop and close it cleanly. Returns the last fleet snapshot.
async fn run_fleet_until(
    f: &mut Fixture,
    label: &str,
    engagement: &str,
    done: impl Fn(u64, &serde_json::Value) -> bool,
) -> serde_json::Value {
    let mut fleet = f.fleet.take().unwrap();
    let cancel = CancellationToken::new();
    let (notices, mut receiver) = tokio::sync::mpsc::channel(1);
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let url = format!("http://{}/api/native/v1/capabilities", f.address);
    let last = {
        let mut runner = Box::pin(fleet.run(notices, &cancel));
        let mut seen = serde_json::Value::Null;
        let last = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                tokio::select! {
                    result=&mut runner=>panic!("service stopped early: {result:?}"),
                    request=f.fake.next()=>f.peer.respond(request,&f.base).await,
                    Some(_)=receiver.recv()=>{},
                    _=tokio::time::sleep(Duration::from_millis(10))=>{},
                }
                let response = client
                    .get(&url)
                    .bearer_auth("fixture_operator_token_32_bytes_minimum")
                    .send()
                    .await
                    .unwrap();
                let value: serde_json::Value =
                    serde_json::from_str(&response.text().await.unwrap()).unwrap();
                let snapshot = value["factory_service"].clone();
                seen = snapshot.clone();
                let agent = snapshot["agents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|a| a["engagement_id"] == engagement)
                    .cloned()
                    .unwrap_or_default();
                if done(
                    f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='completed'"),
                    &agent,
                ) {
                    break snapshot;
                }
            }
        })
        .await;
        let Ok(last) = last else {
            let states = f.base.store.clone();
            drop(states);
            let receipts: Vec<String> = fs::read_dir(f.work())
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("owned-mcp."))
                .collect();
            panic!(
                "{label}: timed out; snapshot={seen}; completed={} queued={} leased={} started={} unknown={}; \
                 account_posts={} key_writes={} probe_requests={} receipts={receipts:?}",
                f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='completed'"),
                f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='queued'"),
                f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='leased'"),
                f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='started'"),
                f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='outcome_unknown'"),
                f.peer.account_posts,
                f.peer.peer.writes.len(),
                f.requests().len(),
            );
        };
        cancel.cancel();
        runner.await.unwrap();
        last
    };
    // A clean close, serving any refresh already admitted before the cancel.
    {
        let closing = fleet.close();
        tokio::pin!(closing);
        loop {
            tokio::select! {result=&mut closing=>{result.unwrap();break;},request=f.fake.next()=>f.peer.respond(request,&f.base).await}
        }
    }
    last
}
async fn fleet_snapshot(client: &reqwest::Client, url: &str) -> serde_json::Value {
    let response = client
        .get(url)
        .bearer_auth("fixture_operator_token_32_bytes_minimum")
        .send()
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
    value["factory_service"].clone()
}
fn agent_row(snapshot: &serde_json::Value, engagement: &str) -> serde_json::Value {
    snapshot["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["engagement_id"] == engagement)
        .cloned()
        .unwrap_or_default()
}
/// The owner's join has no deadline (task rust-owner-join-wait). A provision
/// whose rooms exist waits as `awaiting_owner`; the coordinator keeps taking
/// turns and looks again on each; when the owner joins, a later turn finishes
/// the agent, which takes the waiting row's place and runs its first task.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn native_provisioning_waits_for_the_owner_without_a_deadline() {
    let mut f = Fixture::new_service(false).await;
    // Longer than the first attempt's whole budget (the fixture's SDK limit).
    f.peer.owner_join_delay = Some(Duration::from_secs(24));
    let started = std::time::Instant::now();
    f.provision().await;
    let engagement = f.engagement();
    assert_eq!(f.target_state(), ("started".into(), "reserved".into()));
    assert!(!f.peer.owner, "the owner has not joined yet");
    assert_eq!(f.peer.account_posts, 1);
    let waiting = f.collector.awaiting_owner_engagements();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].0, engagement);
    let since = waiting[0].1;
    fs::write(
        f.work().join("owned-mcp.fleet-files"),
        b"offline probe only",
    )
    .unwrap();
    let mut fleet = f.fleet.take().unwrap();
    let cancel = CancellationToken::new();
    let (notices, mut receiver) = tokio::sync::mpsc::channel(1);
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let url = format!("http://{}/api/native/v1/capabilities", f.address);
    {
        let mut runner = Box::pin(fleet.run(notices, &cancel));
        tokio::time::timeout(Duration::from_secs(90), async {
            // 1. The fleet shows the wait, with its start, and is not failed.
            loop {
                tokio::select! {
                    result=&mut runner=>panic!("service stopped early: {result:?}"),
                    request=f.fake.next()=>f.peer.respond(request,&f.base).await,
                    Some(_)=receiver.recv()=>{},
                    _=tokio::time::sleep(Duration::from_millis(10))=>{},
                }
                let snapshot = fleet_snapshot(&client, &url).await;
                let row = agent_row(&snapshot, &engagement);
                if row["status"]["state"] == "awaiting_owner" {
                    assert_eq!(row["status"]["awaiting_owner_since_ms"], since);
                    assert_eq!(snapshot["failed"], false);
                    break;
                }
            }
            // 2. Coordinator turns look again until the owner has joined.
            while !f.collector.awaiting_owner_engagements().is_empty() {
                let turn = f
                    .collector
                    .intake(HostIntakePlan::new(vec!["root".into()]).unwrap(), &cancel);
                tokio::pin!(turn);
                loop {
                    tokio::select! {
                        result=&mut runner=>panic!("service stopped early: {result:?}"),
                        result=&mut turn=>{result.unwrap();break;},
                        request=f.fake.next()=>f.peer.respond(request,&f.base).await,
                        Some(_)=receiver.recv()=>{},
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            assert!(f.peer.owner);
            assert!(started.elapsed() >= Duration::from_secs(24));
            assert_eq!(f.target_state(), ("complete".into(), "active".into()));
            // 3. The agent takes the waiting row's place and runs its first task.
            queue_service_task(&f, 1).await;
            loop {
                tokio::select! {
                    result=&mut runner=>panic!("service stopped early: {result:?}"),
                    request=f.fake.next()=>f.peer.respond(request,&f.base).await,
                    Some(_)=receiver.recv()=>{},
                    _=tokio::time::sleep(Duration::from_millis(10))=>{},
                }
                if f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='completed'") == 1 {
                    break;
                }
            }
            let snapshot = fleet_snapshot(&client, &url).await;
            let row = agent_row(&snapshot, &engagement);
            assert_ne!(row["status"]["state"], "awaiting_owner");
            assert_eq!(snapshot["failed"], false);
        })
        .await
        .unwrap();
        cancel.cancel();
        runner.await.unwrap();
    }
    assert_eq!(f.receipt("readback")["task"]["id"], "service_task_1");
    assert_eq!(f.peer.account_posts, 1, "the account is registered once");
    {
        let closing = fleet.close();
        tokio::pin!(closing);
        loop {
            tokio::select! {result=&mut closing=>{result.unwrap();break;},request=f.fake.next()=>f.peer.respond(request,&f.base).await}
        }
    }
    f.close().await;
}
/// After a restart the service brings back the agents its inline factory
/// completed, from what their provision left on disk (task
/// rust-factory-agent-reattach). It registers nothing and uploads no keys.
/// An agent that cannot come back is shown as `not_attached` and fails
/// neither the fleet nor readiness.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn native_configured_fleet_reattaches_after_restart() {
    for case in ["reattached", "home_tampered"] {
        let mut f = Fixture::new_service(false).await;
        f.provision().await;
        f.original_owner().await;
        let engagement = f.engagement();
        // The restarted process launches no warm child: its first launch for
        // this agent is a follow-up, which the offline probe accepts with this marker.
        fs::write(f.work().join("owned-mcp.sequential"), b"offline probe only").unwrap();
        fs::write(
            f.work().join("owned-mcp.fleet-files"),
            b"offline probe only",
        )
        .unwrap();
        queue_service_task(&f, 1).await;
        run_fleet_until(&mut f, "before restart", &engagement, |completed, _| {
            completed == 1
        })
        .await;
        assert_eq!(f.receipt("readback")["task"]["id"], "service_task_1");
        let account_posts = f.peer.account_posts;
        let key_writes = f.peer.peer.writes.len();
        if case == "home_tampered" {
            fs::write(
                f.work().parent().unwrap().join("state/home-binding"),
                "0".repeat(64),
            )
            .unwrap();
        }
        let mut f = f.restart().await;
        queue_service_task(&f, 2).await;
        if case == "reattached" {
            let snapshot = run_fleet_until(&mut f, "after restart", &engagement, |completed, _| {
                completed == 2
            })
            .await;
            assert_eq!(snapshot["failed"], false);
            let agent = snapshot["agents"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["engagement_id"] == engagement)
                .expect("the re-attached agent is listed");
            assert_ne!(agent["status"]["state"], "not_attached");
            assert_eq!(f.receipt("readback")["task"]["id"], "service_task_2");
            assert_eq!(
                f.requests()
                    .iter()
                    .filter(|r| r["method"] == "initialize")
                    .count(),
                2,
                "one warm launch before the restart, one follow-up after it"
            );
            assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
        } else {
            let snapshot = run_fleet_until(
                &mut f,
                "after restart, tampered",
                &engagement,
                |_, agent| agent["status"]["state"] == "not_attached",
            )
            .await;
            assert_eq!(
                snapshot["failed"], false,
                "one missing agent does not fail the fleet"
            );
            assert_eq!(
                f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='queued'"),
                1,
                "its work waits; nothing else takes it"
            );
        }
        assert_eq!(
            f.peer.account_posts, account_posts,
            "a restart registers no account"
        );
        assert_eq!(
            f.peer.peer.writes.len(),
            key_writes,
            "a restart uploads no keys"
        );
        f.close().await;
    }
}

/// ADR-182 decision 3: the fence is a row, so it survives a restart. An
/// agent whose stop the guardian could not prove is fenced by the product
/// itself; the restarted service re-attaches it, shows it `fenced`, and
/// claims nothing for it; the operator's settlement clears the fence and the
/// next restart re-attaches a working agent.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn native_reattach_honours_the_fence() {
    let mut f = Fixture::new_service(false).await;
    f.provision().await;
    f.original_owner().await;
    let engagement = f.engagement();
    fs::write(f.work().join("owned-mcp.sequential"), b"offline probe only").unwrap();
    fs::write(
        f.work().join("owned-mcp.fleet-files"),
        b"offline probe only",
    )
    .unwrap();
    // The diagnostics pin: this turn's stop verdict is `Unknown`.
    fs::write(f.work().join("owned-mcp.unproven-stop"), b"offline fixture").unwrap();
    queue_service_task(&f, 1).await;
    let snapshot = run_fleet_until(&mut f, "fenced before restart", &engagement, |_, agent| {
        agent["status"]["state"] == "fenced"
    })
    .await;
    assert_eq!(
        snapshot["failed"], false,
        "a fenced agent does not fail the fleet"
    );
    assert_eq!(
        f.count("SELECT COUNT(*) FROM agent_fences WHERE dispatch_id='service_dispatch_1' AND reason='cleanup_unknown' AND cleared_at IS NULL"),
        1
    );
    assert_eq!(
        f.count("SELECT COUNT(*) FROM runner_dispatches WHERE id='service_dispatch_1' AND state='outcome_unknown'"),
        1
    );
    fs::remove_file(f.work().join("owned-mcp.unproven-stop")).unwrap();
    let account_posts = f.peer.account_posts;
    // Across the restart the fence stands: the re-attached agent reads it
    // from the store and says so. (Its session is quarantined by the same
    // settlement, so no new task can even be queued into it; the claim gate
    // itself is pinned at the store and in the bootstrap scenario.)
    let mut f = f.restart().await;
    let snapshot = run_fleet_until(&mut f, "fenced after restart", &engagement, |_, agent| {
        agent["status"]["state"] == "fenced"
    })
    .await;
    assert_eq!(snapshot["failed"], false);
    let agent = agent_row(&snapshot, &engagement);
    assert_eq!(agent["status"]["fenced"], "service_dispatch_1");
    assert_eq!(
        f.count("SELECT COUNT(*) FROM runner_attempts"),
        1,
        "nothing was claimed across the restart"
    );
    assert_eq!(
        f.peer.account_posts, account_posts,
        "a restart registers no account"
    );
    // The operator's settlement (the console route's store operation) is
    // what clears the fence: the inspection stands on the attempt's own stop
    // evidence, since an unproven stop left no host receipt.
    let inspection = f
        .base
        .store
        .begin_outcome_inspection(engagement.clone(), "service_dispatch_1".into(), 60_000)
        .await
        .unwrap();
    assert_eq!(inspection["snapshot"]["fenced"], true, "{inspection}");
    f.base
        .store
        .resolve_stopped_dispatch(
            engagement.clone(),
            hagency_store::OutcomeResolution {
                original: "service_dispatch_1".into(),
                request_id: "reviewed_settlement".into(),
                inspection_id: inspection["inspectionId"].as_str().unwrap().into(),
                inspection_token: inspection["inspectionToken"].as_str().unwrap().into(),
                action: hagency_store::OutcomeAction::KeepBlocked,
                operator_note: "Reviewed the unproven stop; the task stays blocked".into(),
                replacement: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        f.count("SELECT COUNT(*) FROM agent_fences WHERE cleared_at IS NULL"),
        0
    );
    // The next restart re-attaches a working agent: its next task runs.
    let mut f = f.restart().await;
    queue_service_task(&f, 2).await;
    let snapshot = run_fleet_until(
        &mut f,
        "working after the clearing",
        &engagement,
        |completed, agent| completed == 1 && agent["status"]["fenced"].is_null(),
    )
    .await;
    assert_eq!(snapshot["failed"], false);
    assert_eq!(f.receipt("readback")["task"]["id"], "service_task_2");
    f.close().await;
}

#[tokio::test]
async fn native_provisioning_effect_completed() {
    for application_service in [false, true] {
        let mut f = Fixture::new(application_service).await;
        f.provision().await;
        assert_eq!(f.target_state(), ("complete".into(), "active".into()));
        f.original_owner().await;
        assert_eq!(f.peer.posts, 3);
        assert_eq!(f.peer.peer.claims, 1);
        assert_eq!(f.peer.peer.writes.len(), 5);
        assert_eq!(
            f.peer.account_posts,
            if application_service { 2 } else { 1 }
        );
        assert_eq!(
            f.peer.approval_observations, 2,
            "original bot observes the target after its own ACK"
        );
        assert_eq!(f.count("SELECT COUNT(*) FROM current_approval_bindings c JOIN engagements e ON e.id=c.engagement_id WHERE e.request_id='factory_target'"),1);
        assert_eq!(f.count("SELECT COUNT(*) FROM canonical_tasks"), 0);
        assert_eq!(
            f.requests()
                .iter()
                .filter(|r| r["method"] == "initialize")
                .count(),
            1
        );
        assert!(
            !f.requests()
                .iter()
                .any(|r| r["method"] == "thread/start" || r["method"] == "turn/start")
        );
        assert!(
            fs::read_dir(f.base.root.path().join("contexts"))
                .unwrap()
                .next()
                .is_none()
        );
        f.close().await;
    }
}
#[tokio::test]
async fn native_provisioning_session_route() {
    for application_service in [false, true] {
        let mut f = Fixture::new(application_service).await;
        f.provision().await;
        let agent = f.collector.take_provisioned_agent(&f.engagement()).unwrap();
        assert_eq!(agent.session().id, format!("session_{}", f.engagement()));
        assert_eq!(agent.session().room_id, DM);
        assert!(agent.session().thread_root.is_none());
        assert_eq!(agent.workspace_id(), format!("work_{}", f.engagement()));
        assert!(f.collector.take_provisioned_agent(&f.engagement()).is_err());
        assert_eq!(f.count("SELECT COUNT(*) FROM matrix_session_routes r JOIN runner_sessions s ON s.id=r.session_id JOIN engagements e ON e.id=s.engagement_id WHERE e.request_id='factory_target'"),1);
        let profile = agent.claim_profile().await.unwrap();
        drop(profile);
        agent.close().await.unwrap();
        f.close().await;
    }
}
#[tokio::test]
async fn native_provisioning_factory_first_dispatch() {
    for application_service in [false, true] {
        factory_first_dispatch(application_service).await;
    }
}

async fn next_task(
    f: &Fixture,
    agent: &hagency_matrix::ProvisionedAgent,
    n: u64,
) -> hagency_core::tasks::RunnerCapability {
    let task = format!("factory_task_{n}");
    f.base
        .store
        .create_canonical_task(
            task.clone(),
            agent.session().id.clone(),
            format!("Sequential task {n}"),
            now(),
        )
        .await
        .unwrap();
    f.base
        .store
        .enqueue_dispatch(DispatchInput {
            id: format!("factory_dispatch_{n}"),
            session_id: agent.session().id.clone(),
            task_id: Some(task),
            resources: vec![ResourceLease {
                id: agent.workspace_id().into(),
                exclusive: true,
            }],
            payload: json!({"instruction":"offline actual helper heartbeat/readback"}),
        })
        .await
        .unwrap();
    let profile = agent.claim_profile().await.unwrap();
    f.base
        .store
        .claim_owned_dispatch_for_host(profile, "factory_host".into(), 30_000, 30_000, 2)
        .await
        .unwrap()
        .unwrap()
}

async fn acknowledge(
    operation: &mut hagency_execution::Operation,
    cap: &hagency_core::tasks::RunnerCapability,
) -> hagency_execution::StartedWorkspace {
    let registration = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(registration) = operation.take_workspace_registration() {
                break registration;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let (binding, ack) = registration.into_parts();
    binding.validate_current(cap).await.unwrap();
    ack.registered().unwrap();
    binding
}

#[tokio::test]
async fn native_provisioning_factory_sequential_dispatch() {
    for application_service in [false, true] {
        let mut f = Fixture::new(application_service).await;
        f.provision().await;
        let original = f.original_owner().await;
        let mut agent = f.collector.take_provisioned_agent(&f.engagement()).unwrap();
        let cap = next_task(&f, &agent, 1).await;
        let mut first = agent
            .dispatch(cap.clone(), execution_limits())
            .await
            .unwrap();
        // Before the original result is observed there is no second admission,
        // even if a caller supplies a different-looking capability value.
        let mut other = cap.clone();
        other.dispatch_id = "not_an_admitted_second_task".into();
        assert!(
            agent
                .dispatch(other.clone(), execution_limits())
                .await
                .is_err()
        );
        let binding = acknowledge(&mut first, &cap).await;
        let mut report = first.wait().await.unwrap();
        assert_eq!(
            report.protocol,
            hagency_execution::Protocol::Completed,
            "first: {:?}",
            report.failure
        );
        assert_eq!(f.receipt("warm-thread")["pid"], original["pid"]);
        assert_eq!(f.receipt("readback")["task"]["id"], "factory_task_1");
        assert_eq!(
            f.requests()
                .iter()
                .filter(|r| r["method"] == "initialize")
                .count(),
            1
        );
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            assert_eq!(
                report.failure,
                Some(hagency_execution::Failure::CleanupUnknown)
            );
            assert!(agent.dispatch(other, execution_limits()).await.is_err());
            assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 1);
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            assert_eq!(report.failure, None);
            assert_eq!(report.settlement, hagency_execution::Settlement::Completed);
            assert!(!report.retains_process_custody());
            assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
            assert!(
                agent.dispatch(cap, execution_limits()).await.is_err(),
                "same capability is not a new turn"
            );
            // The returned diagnostic can be made to look negative, but only
            // the internal original worker observation governs continuation.
            report.failure = Some(hagency_execution::Failure::Worker);
            drop(report);
            drop(first);
            drop(binding);
            fs::write(f.work().join("owned-mcp.sequential"), b"offline probe only").unwrap();
            let cap = next_task(&f, &agent, 2).await;
            let mut second = agent
                .dispatch(cap.clone(), execution_limits())
                .await
                .unwrap();
            let binding = acknowledge(&mut second, &cap).await;
            let report = second.wait().await.unwrap();
            assert_eq!(
                report.protocol,
                hagency_execution::Protocol::Completed,
                "second: {:?}",
                report.failure
            );
            assert_eq!(report.failure, None);
            assert_eq!(report.settlement, hagency_execution::Settlement::Completed);
            assert!(!report.retains_process_custody());
            assert_eq!(f.receipt("direct-parent")["task_id"], "factory_task_2");
            assert_ne!(f.receipt("direct-parent")["pid"], original["pid"]);
            assert_eq!(f.receipt("readback")["task"]["id"], "factory_task_2");
            assert_eq!(
                f.requests()
                    .iter()
                    .filter(|r| r["method"] == "initialize")
                    .count(),
                2
            );
            assert_eq!(
                fs::read_dir(f.base.root.path().join("contexts"))
                    .unwrap()
                    .count(),
                1
            );
            assert_eq!(
                f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='completed'"),
                2
            );
            assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
            assert_eq!(
                f.count("SELECT COUNT(*) FROM final_replies"),
                0,
                "heartbeat is not canonical Done"
            );
            drop(report);
            drop(second);
            drop(binding);
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            report.retry_stop();
            drop(report);
            drop(first);
            drop(binding);
        }
        assert_eq!(f.peer.peer.writes.len(), 5);
        assert_eq!(f.peer.peer.claims, 1);
        agent.close().await.unwrap();
        f.close().await;
    }
}

#[tokio::test]
async fn native_provisioning_factory_report_is_not_authority() {
    let mut f = Fixture::new(false).await;
    f.provision().await;
    f.original_owner().await;
    let mut agent = f.collector.take_provisioned_agent(&f.engagement()).unwrap();
    let cap = next_task(&f, &agent, 1).await;
    let mut operation = agent
        .dispatch(cap.clone(), execution_limits())
        .await
        .unwrap();
    // Lose the real Started workspace acknowledgment. No helper/thread is
    // authorized; the warm child remains under the actual negative teardown.
    let registration = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(value) = operation.take_workspace_registration() {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    drop(registration);
    let mut report = operation.wait().await.unwrap();
    assert!(report.failure.is_some());
    report.failure = None;
    report.protocol = hagency_execution::Protocol::Completed;
    report.settlement = hagency_execution::Settlement::Completed;
    if let hagency_runtime::owned::Cleanup::Observed(mut observation) = report.cleanup {
        observation.scope.whole_tree_stopped = true;
        observation.scope.leader_exited = true;
        observation.scope.signals_accepted = true;
        report.cleanup = hagency_runtime::owned::Cleanup::Observed(observation);
    }
    let mut other = cap;
    other.dispatch_id = "forged_second_dispatch".into();
    assert!(agent.dispatch(other, execution_limits()).await.is_err());
    assert_eq!(
        f.requests()
            .iter()
            .filter(|r| r["method"] == "initialize")
            .count(),
        1
    );
    assert!(!f.requests().iter().any(|r| r["method"] == "thread/start"));
    report.retry_stop();
    drop(report);
    drop(operation);
    agent.close().await.unwrap();
    f.close().await;
}
async fn factory_first_dispatch(application_service: bool) {
    let mut f = Fixture::new(application_service).await;
    f.provision().await;
    let original = f.original_owner().await;
    let mut agent = f.collector.take_provisioned_agent(&f.engagement()).unwrap();
    let session = agent.session().id.clone();
    let workspace = agent.workspace_id().to_owned();
    f.base
        .store
        .create_canonical_task(
            "factory_task".into(),
            session.clone(),
            "Original factory task".into(),
            now(),
        )
        .await
        .unwrap();
    f.base.store.enqueue_dispatch(DispatchInput {id:"factory_dispatch".into(),session_id:session,task_id:Some("factory_task".into()),
        resources:vec![ResourceLease {id:workspace,exclusive:true}],payload:json!({"instruction":"offline heartbeat/readback","model":"impostor","cwd":"/impostor","done":true})}).await.unwrap();
    let profile = agent.claim_profile().await.unwrap();
    let cap = f
        .base
        .store
        .claim_owned_dispatch_for_host(profile, "factory_host".into(), 30_000, 30_000, 2)
        .await
        .unwrap()
        .unwrap();
    let expected = f
        .base
        .store
        .owned_dispatch_scope(cap.clone())
        .await
        .unwrap()
        .fingerprint()
        .to_owned();
    let mut operation = agent
        .dispatch(cap.clone(), execution_limits())
        .await
        .unwrap();
    let registration = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(registration) = operation.take_workspace_registration() {
                break registration;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let (binding, ack) = registration.into_parts();
    binding.validate_current(&cap).await.unwrap();
    ack.registered().unwrap();
    let mut first_refusal = None;
    let mut report = {
        let wait = operation.wait();
        tokio::pin!(wait);
        let mut tick = tokio::time::interval(Duration::from_millis(20));
        loop {
            tokio::select! {
                result=&mut wait=>break result.unwrap(),
                _=tick.tick()=>{if first_refusal.is_none() && let Err(error)=f.base.store.check_owned_dispatch(cap.clone(),expected.clone()).await {first_refusal=Some((format!("{error:?}"),f.authority_snapshot()));}},
            }
        }
    };
    assert_eq!(
        report.protocol,
        hagency_execution::Protocol::Completed,
        "original factory dispatch failed: {:?}; observation={:?}; canonical={:?}; first_refusal={:?}; helper_spawned={}; cached={}; ack={}; readback={}; exit={}",
        report.failure,
        report.runtime_observation(),
        report.canonical_status,
        first_refusal,
        f.work().join("owned-mcp.spawned").exists(),
        f.work().join("owned-mcp.context-cached").exists(),
        f.work().join("owned-mcp.ack").exists(),
        f.work().join("owned-mcp.readback").exists(),
        f.work().join("owned-mcp.receipt").exists()
    );
    let hagency_runtime::owned::Cleanup::Observed(cleanup) = report.cleanup else {
        panic!("actual cleanup observation required");
    };
    if !cleanup.scope.whole_tree_stopped {
        assert_eq!(
            report.failure,
            Some(hagency_execution::Failure::CleanupUnknown)
        );
        assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 1);
    }
    assert_eq!(f.task_status(), "in_progress");
    assert_eq!(f.count("SELECT COUNT(*) FROM final_replies"), 0);
    assert_eq!(
        f.requests()
            .iter()
            .filter(|r| r["method"] == "initialize")
            .count(),
        1
    );
    assert_eq!(f.receipt("warm-thread")["pid"], original["pid"]);
    assert_eq!(f.receipt("readback")["task"]["status"], "in_progress");
    assert!(
        agent.dispatch(cap, execution_limits()).await.is_err(),
        "no cold fallback/second consumption"
    );
    report.retry_stop();
    drop(report);
    drop(operation);
    drop(binding);
    agent.close().await.unwrap();
    f.close().await;
}
#[tokio::test]
async fn native_provisioning_factory_refusals() {
    let mut f = Fixture::new(false).await;
    // Hold the original final SDK read AFTER native initialize, then remove
    // actual joined owner membership. No target fixture Applied is ever written.
    let plan = HostIntakePlan::new(vec!["root".into()]).unwrap();
    let cancel = CancellationToken::new();
    let mut refused = false;
    let result = {
        let operation = f.collector.intake(plan, &cancel);
        tokio::pin!(operation);
        loop {
            tokio::select! {
            result=&mut operation=>break result,
            request=f.fake.next()=>{
                if request.target.ends_with("/state") && request.target.contains("factory_owner_dm")
                    && f.work().join("owned-mcp.warm-initialized").exists() {
                    refused=true;f.peer.owner=false;
                }
                f.peer.respond(request,&f.base).await;
            },
            }
        }
    };
    assert!(refused);
    assert!(result.is_err());
    assert_eq!(f.target_state(), ("uncertain".into(), "reserved".into()));
    assert_eq!(f.count("SELECT COUNT(*) FROM matrix_session_routes r JOIN runner_sessions s ON s.id=r.session_id JOIN engagements e ON e.id=s.engagement_id WHERE e.request_id='factory_target'"),0);
    assert!(f.collector.take_provisioned_agent(&f.engagement()).is_err());
    assert!(
        f.collector.take_next_provisioned_agent().unwrap().is_none(),
        "uncertain original factory cannot be discovered as ready"
    );
    assert_eq!(
        f.requests()
            .iter()
            .filter(|r| r["method"] == "initialize")
            .count(),
        1
    );
    f.close().await;
}

#[tokio::test]
async fn native_provisioning_factory_approval_delivery() {
    for application_service in [false, true] {
        let mut f = Fixture::new(application_service).await;
        // The bot already owns its actual Complete SDK before this target
        // exists. Later membership must not regenerate signing/session state.
        f.enroll_approvals().await;
        let original = f.peer.approval_peer.query["device_keys"].clone();
        assert_eq!(f.peer.approval_peer.writes.len(), 5);
        f.provision().await;
        let agent = f.collector.take_provisioned_agent(&f.engagement()).unwrap();
        let card = f.approval_card(&agent).await;
        let expected = card.content().clone();
        let id = card.target().request_id.clone();
        let approvals = f.approvals.clone();
        let cancel = CancellationToken::new();
        let sent = f.deliver_approval(card.clone()).await;
        assert_eq!(f.peer.approval_peer.claims, 1);
        assert_eq!(f.peer.approval_peer.writes.len(), 5);
        assert_eq!(f.peer.approval_peer.query["device_keys"], original);
        assert_eq!(
            sent.state,
            hagency_matrix::PrivateApprovalDeliveryState::Accepted
        );
        assert!(!sent.replayed);
        assert_eq!(f.peer.approval_peer.events.len(), 1);
        assert_eq!(f.peer.approval_peer.events[0]["content"], expected);
        assert_eq!(f.peer.approval_peer.shares, 1);
        assert_eq!(
            f.base.store.approval_summary(id).await.unwrap().state,
            "pending",
            "delivery must not grant owner permission"
        );
        let replay = approvals
            .send_private_approval_card(card, &cancel)
            .await
            .unwrap();
        assert!(replay.replayed);
        assert_eq!(
            replay.state,
            hagency_matrix::PrivateApprovalDeliveryState::Accepted
        );
        assert_eq!(
            f.requests()
                .iter()
                .filter(|r| r["method"] == "initialize")
                .count(),
            1
        );
        agent.close().await.unwrap();
        f.close().await;
    }
}

#[tokio::test]
async fn native_provisioning_factory_approval_refusal() {
    let mut f = Fixture::new(false).await;
    let plan = HostIntakePlan::new(vec!["root".into()]).unwrap();
    let cancel = CancellationToken::new();
    let mut refused = false;
    let result = {
        let operation = f.collector.intake(plan, &cancel);
        tokio::pin!(operation);
        loop {
            tokio::select! {
                result=&mut operation=>break result,
                request=f.fake.next()=>{
                    if request.target.ends_with("/state") && request.target.contains("private") && f.work().join("owned-mcp.warm-initialized").exists() {
                        assert_eq!(f.target_state(),("complete".into(),"active".into()));
                        refused=true;f.peer.approval_owner=false;
                    }
                    f.peer.respond(request,&f.base).await;
                },
            }
        }
    };
    assert!(refused);
    assert!(result.is_err());
    assert_eq!(
        f.target_state(),
        ("complete".into(), "active".into()),
        "no backward Unknown after activation ACK"
    );
    assert_eq!(f.count("SELECT COUNT(*) FROM current_approval_bindings c JOIN engagements e ON e.id=c.engagement_id WHERE e.request_id='factory_target'"),0);
    assert_eq!(f.count("SELECT COUNT(*) FROM matrix_session_routes r JOIN runner_sessions s ON s.id=r.session_id JOIN engagements e ON e.id=s.engagement_id WHERE e.request_id='factory_target'"),0);
    assert!(f.collector.take_provisioned_agent(&f.engagement()).is_err());
    assert_eq!(
        f.requests()
            .iter()
            .filter(|r| r["method"] == "initialize")
            .count(),
        1
    );
    f.close().await;
}

#[tokio::test]
async fn native_provisioning_factory_waiter_loss() {
    let mut f = Fixture::new(false).await;
    let plan = HostIntakePlan::new(vec!["root".into()]).unwrap();
    let cancel = CancellationToken::new();
    let held = {
        let mut operation = Box::pin(f.collector.intake(plan, &cancel));
        loop {
            tokio::select! {
                result=&mut operation=>panic!("original intake finished before retained SDK read: {result:?}"),
                request=f.fake.next()=>{
                    if request.target.ends_with("/state") && request.target.contains("factory_owner_dm") && f.work().join("owned-mcp.warm-initialized").exists() {
                        drop(operation);break request;
                    }
                    f.peer.respond(request,&f.base).await;
                },
            }
        }
    };
    let original = f.original_owner().await;
    assert_eq!(f.target_state(), ("started".into(), "reserved".into()));
    f.peer.respond(held, &f.base).await;
    // The original owned intake job retains the busy permit through actual
    // SDK/domain settlement. Inspect its original idle journal, not elapsed
    // time or a replacement intake, to establish that it has finished.
    tokio::time::timeout(Duration::from_secs(4),async {
        loop {
            let status={
                let read=f.collector.intake_status(&cancel);tokio::pin!(read);
                loop {tokio::select! {result=&mut read=>break result,request=f.fake.next()=>f.peer.respond(request,&f.base).await}}
            };
            match status {
                Ok(status)=>{assert_eq!(status.stage,"idle");break;},
                Err(hagency_matrix::Error::Busy)=>tokio::time::sleep(Duration::from_millis(5)).await,
                Err(error)=>panic!("original intake custody failed: {error:?}"),
            }
        }
    }).await.unwrap();
    assert_eq!(f.target_state(), ("complete".into(), "active".into()));
    let agent = f.collector.take_provisioned_agent(&f.engagement()).unwrap();
    assert_eq!(agent.session().room_id, DM);
    assert_eq!(f.original_owner().await["pid"], original["pid"]);
    assert_eq!(
        f.requests()
            .iter()
            .filter(|r| r["method"] == "initialize")
            .count(),
        1
    );
    assert_eq!(f.peer.account_posts, 1);
    assert_eq!(f.peer.peer.claims, 1);
    let card = f.approval_card(&agent).await;
    let expected = card.content().clone();
    assert_eq!(
        f.deliver_approval(card).await.state,
        hagency_matrix::PrivateApprovalDeliveryState::Accepted
    );
    assert_eq!(f.peer.approval_peer.events.len(), 1);
    assert_eq!(f.peer.approval_peer.events[0]["content"], expected);
    agent.close().await.unwrap();
    f.close().await;
}

#[tokio::test]
async fn native_provisioning_factory_approval_revoked() {
    let mut f = Fixture::new(false).await;
    let cancel = CancellationToken::new();
    let mut revoked = false;
    let mut activated = None;
    let result = {
        let operation = f
            .collector
            .intake(HostIntakePlan::new(vec!["root".into()]).unwrap(), &cancel);
        tokio::pin!(operation);
        loop {
            tokio::select! {
                result=&mut operation=>break result,
                request=f.fake.next()=>{
                    if request.target.ends_with("/state") && request.target.contains("private") && f.work().join("owned-mcp.warm-initialized").exists() {
                        assert!(!revoked);assert_eq!(f.target_state(),("complete".into(),"active".into()));
                        activated=Some(f.provision_receipt());
                        f.base.store.revoke("revoke_during_factory_private_read".into(),f.engagement()).await.unwrap();
                        revoked=true;
                    }
                    f.peer.respond(request,&f.base).await;
                },
            }
        }
    };
    assert!(revoked);
    assert!(result.is_err());
    // Canonical revoke cancels the effect and advances its fence; the original
    // Applied digest remains historical evidence, never rewritten to Unknown.
    assert_eq!(f.target_state(), ("cancelled".into(), "revoked".into()));
    let (fence, digest) = activated.unwrap();
    assert_eq!(f.provision_receipt(), (fence + 1, digest));
    assert_eq!(f.count("SELECT COUNT(*) FROM current_approval_bindings c JOIN engagements e ON e.id=c.engagement_id WHERE e.request_id='factory_target'"),0);
    assert_eq!(f.count("SELECT COUNT(*) FROM matrix_session_routes r JOIN runner_sessions s ON s.id=r.session_id JOIN engagements e ON e.id=s.engagement_id WHERE e.request_id='factory_target'"),0);
    assert!(f.collector.take_provisioned_agent(&f.engagement()).is_err());
    // If the revoked candidate had been admitted, the original bot's close
    // would include its retired authority and retain that refusal.
    f.approvals.close().await.unwrap();
    assert_eq!(
        f.requests()
            .iter()
            .filter(|r| r["method"] == "initialize")
            .count(),
        1
    );
    f.close().await;
}

#[tokio::test]
async fn native_provisioning_factory_foreign_approval_writer() {
    let mut f = Fixture::foreign_approval_writer().await;
    let plan = HostIntakePlan::new(vec!["root".into()]).unwrap();
    let cancel = CancellationToken::new();
    let result = {
        let operation = f.collector.intake(plan, &cancel);
        tokio::pin!(operation);
        loop {
            tokio::select! {result=&mut operation=>break result,request=f.fake.next()=>f.peer.respond(request,&f.base).await}
        }
    };
    assert!(result.is_err());
    assert_eq!(f.target_state(), ("uncertain".into(), "reserved".into()));
    assert_eq!(f.peer.account_posts, 1);
    assert_eq!(f.peer.peer.claims, 1);
    assert!(!f.work().join("owned-mcp.warm-initialized").exists());
    assert!(
        !f.work().join("owned-mcp.requests").exists(),
        "foreign producer must refuse before native initialize"
    );
    assert_eq!(f.count("SELECT COUNT(*) FROM current_approval_bindings c JOIN engagements e ON e.id=c.engagement_id WHERE e.request_id='factory_target'"),0);
    assert_eq!(f.count("SELECT COUNT(*) FROM matrix_session_routes r JOIN runner_sessions s ON s.id=r.session_id JOIN engagements e ON e.id=s.engagement_id WHERE e.request_id='factory_target'"),0);
    assert!(f.collector.take_provisioned_agent(&f.engagement()).is_err());
    f.close().await;
}

#[tokio::test]
async fn native_provisioning_factory_close_waiter_loss() {
    let mut f = Fixture::new(false).await;
    f.provision().await;
    f.original_owner().await;
    let agent = f.collector.take_provisioned_agent(&f.engagement()).unwrap();
    let mut closing = Box::pin(f.collector.close_provisioned_agents());
    // One poll admits the real owned closure with the coordinator permit. On
    // this current-thread fixture runtime its spawned job has not run yet.
    std::future::poll_fn(|cx| {
        assert!(closing.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(closing);
    let cancel = CancellationToken::new();
    assert_eq!(
        f.collector.intake_status(&cancel).await.unwrap_err(),
        hagency_matrix::Error::Busy,
        "lost waiter must not release its retained permit"
    );
    tokio::time::timeout(Duration::from_secs(4),async {
        loop {
            let status={let read=f.collector.intake_status(&cancel);tokio::pin!(read);
                loop {tokio::select! {result=&mut read=>break result,request=f.fake.next()=>f.peer.respond(request,&f.base).await}}
            };
            match status {
                Ok(status)=>{assert_eq!(status.stage,"idle");break;},
                Err(hagency_matrix::Error::Busy)=>tokio::time::sleep(Duration::from_millis(5)).await,
                Err(error)=>panic!("original shutdown custody failed: {error:?}"),
            }
        }
    }).await.unwrap();
    assert!(matches!(
        agent.claim_profile().await,
        Err(hagency_matrix::Error::Generation)
    ));
    // The closed handle refuses, but a clean close retires nothing durable:
    // the agent's incarnation stays available for the next start.
    assert!(
        f.base
            .store
            .matrix_transport_state(f.engagement())
            .await
            .unwrap()
            .unwrap()
            .available
    );
    assert_eq!(
        f.requests()
            .iter()
            .filter(|r| r["method"] == "initialize")
            .count(),
        1
    );
    agent.close().await.unwrap();
    f.close().await;
}

#[tokio::test]
async fn native_provisioning_factory_close_busy() {
    let mut f = Fixture::new(false).await;
    f.provision().await;
    f.original_owner().await;
    let agent = f.collector.take_provisioned_agent(&f.engagement()).unwrap();
    let cancel = CancellationToken::new();
    {
        let read = f.collector.collect(&cancel);
        tokio::pin!(read);
        let held = tokio::select! {result=&mut read=>panic!("read finished before authenticated request: {result:?}"),request=f.fake.next()=>request};
        assert!(held.target.ends_with("/whoami"));
        assert_eq!(
            f.collector.close_provisioned_agents().await.unwrap_err(),
            hagency_matrix::Error::Busy
        );
        agent.claim_profile().await.unwrap();
        f.peer.respond(held, &f.base).await;
        loop {
            tokio::select! {result=&mut read=>{result.unwrap();break;},request=f.fake.next()=>f.peer.respond(request,&f.base).await}
        }
    }
    assert_eq!(
        f.requests()
            .iter()
            .filter(|r| r["method"] == "initialize")
            .count(),
        1
    );
    agent.claim_profile().await.unwrap();
    agent.close().await.unwrap();
    f.close().await;
}

#[tokio::test]
async fn native_provisioning_factory_close_agent_busy() {
    let mut f = Fixture::new(false).await;
    f.provision().await;
    f.original_owner().await;
    let agent = f.collector.take_provisioned_agent(&f.engagement()).unwrap();
    let cancel = CancellationToken::new();
    {
        let read = agent.collector().collect(&cancel);
        tokio::pin!(read);
        let held = tokio::select! {result=&mut read=>panic!("agent read finished before authenticated request: {result:?}"),request=f.fake.next()=>request};
        assert!(held.target.ends_with("/whoami"));
        assert_eq!(
            f.collector.close_provisioned_agents().await.unwrap_err(),
            hagency_matrix::Error::OutcomeUnknown,
            "accepted partial drain is not effect-free Busy"
        );
        assert!(matches!(
            agent.claim_profile().await,
            Err(hagency_matrix::Error::Generation)
        ));
        assert!(f.collector.take_provisioned_agent(&f.engagement()).is_err());
        f.peer.respond(held, &f.base).await;
        loop {
            tokio::select! {result=&mut read=>{result.unwrap();break;},request=f.fake.next()=>f.peer.respond(request,&f.base).await}
        }
    }
    assert!(matches!(
        agent.claim_profile().await,
        Err(hagency_matrix::Error::Generation)
    ));
    assert_eq!(
        f.requests()
            .iter()
            .filter(|r| r["method"] == "initialize")
            .count(),
        1
    );
    assert_eq!(f.peer.peer.writes.len(), 5);
    assert_eq!(f.peer.peer.claims, 1);
    agent.close().await.unwrap();
    f.close().await;
}

#[test]
fn native_provisioning_factory_dispatch_custody() {
    // Deliberately controlled fixture capacity, not serialization of existing
    // acceptance tests or a change to any product IO/deadline/process budget.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        for lost in [false, true] {
            let mut f = Fixture::new(false).await;
            f.provision().await;
            f.original_owner().await;
            let mut agent = f.collector.take_provisioned_agent(&f.engagement()).unwrap();
            f.base
                .store
                .create_canonical_task(
                    "factory_task".into(),
                    agent.session().id.clone(),
                    "Failed original handoff".into(),
                    now(),
                )
                .await
                .unwrap();
            f.base
                .store
                .enqueue_dispatch(DispatchInput {
                    id: "factory_dispatch".into(),
                    session_id: agent.session().id.clone(),
                    task_id: Some("factory_task".into()),
                    resources: vec![ResourceLease {
                        id: agent.workspace_id().into(),
                        exclusive: true,
                    }],
                    payload: json!({"instruction":"no IO from failed admission"}),
                })
                .await
                .unwrap();
            let profile = agent.claim_profile().await.unwrap();
            let cap = f
                .base
                .store
                .claim_owned_dispatch_for_host(profile, "factory_host".into(), 30_000, 30_000, 2)
                .await
                .unwrap()
                .unwrap();
            let before = f.task_status();
            let (entered, ready) = tokio::sync::oneshot::channel();
            let (release, released) = std::sync::mpsc::sync_channel(0);
            let holder = tokio::task::spawn_blocking(move || {
                entered.send(()).unwrap();
                let _ = released.recv();
            });
            ready.await.unwrap();
            let mut dispatch = Box::pin(agent.dispatch(
                cap.clone(),
                hagency_execution::Limits {
                    operation_ms: 0,
                    response_ms: 2000,
                },
            ));
            std::future::poll_fn(|cx| {
                assert!(
                    dispatch.as_mut().poll(cx).is_pending(),
                    "failed handoff must never join its warm owner on the async caller"
                );
                Poll::Ready(())
            })
            .await;
            if lost {
                drop(dispatch);
                assert!(
                    matches!(
                        agent.dispatch(cap, execution_limits()).await,
                        Err(hagency_execution::Failure::Admission)
                    ),
                    "lost queued waiter cannot select a second handoff"
                );
                release.send(()).unwrap();
                holder.await.unwrap();
            } else {
                release.send(()).unwrap();
                holder.await.unwrap();
                assert!(matches!(
                    dispatch.await,
                    Err(hagency_execution::Failure::Admission)
                ));
                assert!(matches!(
                    agent.dispatch(cap, execution_limits()).await,
                    Err(hagency_execution::Failure::Admission)
                ));
            }
            agent.close().await.unwrap();
            assert_eq!(f.task_status(), before);
            assert_eq!(
                f.count("SELECT COUNT(*) FROM runner_dispatches WHERE state='started'"),
                0
            );
            let requests = f.requests();
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
            assert!(!f.work().join("owned-mcp.spawned").exists());
            f.close().await;
        }
    });
}
