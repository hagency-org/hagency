use super::*;
use hagency_store::DomainStore;
use std::{sync::Arc, time::Duration};

#[test]
fn native_approval_wire_corpus() {
    let mut cards = Vec::new();
    for (name, workspace, reusable, rpc, padding) in [
        ("posix", "/work/a", true, ApprovalRpcId::Number(0), 0),
        (
            "drive",
            r"C:\work\Agent 中文",
            true,
            ApprovalRpcId::Number(hagency_core::JSON_SAFE_MAX),
            0,
        ),
        (
            "unc",
            r"\\server\share\work",
            true,
            ApprovalRpcId::String("001".into()),
            0,
        ),
        (
            "unknown",
            "/work/a",
            false,
            ApprovalRpcId::String("opaque".into()),
            0,
        ),
        (
            "long_preview",
            "/work/a",
            false,
            ApprovalRpcId::Number(7),
            9000,
        ),
    ] {
        let mut f = Fixture::new(true);
        f.contexts[0].id = format!("corpus_{name}");
        f.contexts[0].workspace = workspace.into();
        f.contexts[0].windows_paths = matches!(name, "drive" | "unc");
        f.db.bind_approval_context(&f.caps[0], &f.contexts[0], 1009)
            .unwrap();
        let mut input = f.input(0, 1);
        input.upstream_id = rpc;
        if !reusable {
            input.method = "unknown/requestApproval".into();
        }
        if padding > 0 {
            input.params["opaque"] = json!("x".repeat(padding));
        }
        let request =
            f.db.request_owner_approval(&f.caps[0], &input, 1010)
                .unwrap();
        let card =
            f.db.private_approval_card(&request.id, 11000, 1011)
                .unwrap();
        assert_eq!(card.target().reusable_scope, reusable);
        assert!(serde_json::to_vec(card.content()).unwrap().len() <= 48 * 1024);
        if reusable {
            assert_eq!(
                card.content()["com.agentchat.approval"]["reusable_scope"]["workspace"],
                workspace
            );
        }
        cards.push(json!({"name":name,"content":card.content()}));
    }
    let corpus = json!({"version":1,"producer":"native_approval_wire_corpus","cards":cards});
    // Explicit fixture-export mode is a developer tool, not normal qualification.
    // The normal selector compares every byte-bearing field against real output.
    if std::env::var("HAGENCY_TEST_EXPORT_APPROVAL_WIRE").as_deref() == Ok("1") {
        println!("HAGENCY_APPROVAL_WIRE_CORPUS={corpus}");
    } else {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/native-approval-wire.json");
        let expected: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(corpus, expected);
    }
}

#[test]
fn native_private_approval_card_content() {
    let mut f = Fixture::new(true);
    for (index, reusable) in [true, false].into_iter().enumerate() {
        let mut input = f.input(0, index as u64 + 1);
        if !reusable {
            input.method = "unknown/requestApproval".into();
            input.upstream_id = ApprovalRpcId::String("001".into());
        }
        let request =
            f.db.request_owner_approval(&f.caps[0], &input, 1010)
                .unwrap();
        let card =
            f.db.private_approval_card(&request.id, 11000, 1011)
                .unwrap();
        let target = card.target();
        assert_eq!(target.request_id, request.id);
        assert_eq!(target.authority.owner_mxid, "@owner:example.test");
        assert_eq!(target.authority.bot_mxid, "@approval:example.test");
        assert_eq!(target.authority.room_id, "!private:example.test");
        assert_eq!(target.device_id, "BOT_DEVICE");
        assert_eq!(target.expires_at, 12000);
        assert_eq!(card.owner_expires_at(), 11000);
        assert_eq!(
            card.content()["msgtype"],
            "com.agentchat.approval.request.v1"
        );
        assert!(card.content().get("m.relates_to").is_none());
        let detail = &card.content()["com.agentchat.approval"];
        assert_eq!(detail["request_id"], request.id);
        assert_eq!(detail["input_digest"], target.request_digest);
        assert_eq!(detail["agent"], f.agents[0]);
        assert_eq!(detail["project"], target.authority.project_id);
        assert_eq!(detail["project_room_id"], "!project:example.test");
        assert_eq!(detail["upstream_rpc_id"], json!(input.upstream_id));
        assert_eq!(
            detail["upstream_request_id"],
            if reusable { "1" } else { "001" }
        );
        assert_eq!(detail["tool_name"], input.method);
        assert_eq!(detail["expires_at"], 11000);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(detail["input_preview"].as_str().unwrap())
                .unwrap(),
            input.params
        );
        let actions: Vec<_> = detail["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            actions,
            if reusable {
                vec!["approve_once", "approve_task", "approve_always", "deny"]
            } else {
                vec!["approve_once", "deny"]
            }
        );
        if reusable {
            let old = f.db.private_approval(&request.id, 1011).unwrap();
            assert_eq!(
                detail["reusable_scope"]["description"],
                old.description.unwrap()
            );
            assert_eq!(detail["reusable_scope"]["workspace"], "/work/a");
            assert_eq!(detail["reusable_scope"]["task_id"], "task_a");
        } else {
            assert!(detail.get("reusable_scope").is_none());
        }
        f.db.check_private_approval_card(&card, 1012).unwrap();
        assert_eq!(f.db.approval_summary(&request.id).unwrap().state, "pending");
        assert_eq!(f.state(0), "parked");
    }
}

#[test]
fn native_private_approval_card_authority() {
    for change in [
        "decided", "promoted", "device", "retired", "expired", "lease",
    ] {
        let mut f = Fixture::new(true);
        let request = f.admit(0, 1);
        let card =
            f.db.private_approval_card(&request.id, 11000, 1011)
                .unwrap();
        let at = match change {
            "decided" => {
                f.choose(&request.id, ApprovalChoice::Deny);
                1013
            }
            "promoted" => {
                let mut room = f.rooms[0].clone();
                room.joined.insert("@outsider:example.test".into());
                f.db.observe_approval_room(&room, 1012).unwrap();
                1013
            }
            "device" => {
                let mut room = f.rooms[0].clone();
                room.generation = 2;
                room.device_id = "NEW_BOT".into();
                f.db.observe_approval_room(&room, 1012).unwrap();
                1013
            }
            "retired" => {
                f.db.revoke("retire", &f.agents[0]).unwrap();
                1013
            }
            "expired" => 11000,
            "lease" => {
                f.db.renew_dispatch(&f.caps[0], 1012, 100).unwrap();
                1112
            }
            _ => unreachable!(),
        };
        assert!(
            f.db.check_private_approval_card(&card, at).is_err(),
            "{change}"
        );
        assert!(
            f.db.private_approval_card(&request.id, 11000, at).is_err(),
            "{change}"
        );
        assert!(f.grants(0).is_empty());
        let responses: u64 = f
            .sql()
            .query_row("SELECT COUNT(*) FROM approval_responses", [], |r| r.get(0))
            .unwrap();
        assert_eq!(responses, 0);
    }
}

#[test]
fn native_private_approval_card_capacity() {
    let mut f = Fixture::new(true);
    let request = f.admit(0, 1);
    for cutoff in [0, 1011, 12001, hagency_core::JSON_SAFE_MAX + 1] {
        assert!(
            f.db.private_approval_card(&request.id, cutoff, 1011)
                .is_err()
        );
    }
    assert!(f.db.private_approval_card("unknown", 11000, 1011).is_err());
    let mut input = f.input(0, 2);
    input.method = "unknown/requestApproval".into();
    // Legal stored input, but representing its exact preview twice exceeds the
    // smaller encrypted-card budget. No truncation or replacement request.
    input.params["opaque"] = json!("中".repeat(9000));
    let large =
        f.db.request_owner_approval(&f.caps[0], &input, 1010)
            .unwrap();
    assert!(matches!(
        f.db.private_approval_card(&large.id, 11000, 1011),
        Err(Error::Capacity)
    ));
    assert_eq!(
        f.db.private_approval(&large.id, 1011).unwrap().params,
        input.params
    );
    assert_eq!(f.db.approval_summary(&large.id).unwrap().state, "pending");
    assert!(f.grants(0).is_empty());
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[tokio::test]
async fn native_private_approval_card_clock() {
    let mut f = Fixture::new_at(true, now().saturating_sub(20), 60000);
    let mut sql = f.sql();
    let mut input = f.input(0, 1);
    input.expires_at = now() + 10000;
    let request =
        f.db.request_owner_approval(&f.caps[0], &input, now())
            .unwrap();
    let cutoff = now() + 1000;
    let card = Arc::new(
        f.db.private_approval_card(&request.id, cutoff, now())
            .unwrap(),
    );
    let store = DomainStore::start(f.db, 16).unwrap();
    // Both public host APIs also succeed through the real original writer.
    store
        .check_private_approval_card(card.clone())
        .await
        .unwrap();
    store
        .private_approval_card(request.id.clone(), cutoff)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(
        cutoff.saturating_sub(now()).saturating_sub(60),
    ))
    .await;
    let lock = sql
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    let first = store.private_approval_card(request.id.clone(), cutoff);
    tokio::pin!(first);
    assert!(
        tokio::time::timeout(Duration::from_millis(5), &mut first)
            .await
            .is_err()
    );
    let second = store.check_private_approval_card(card);
    tokio::pin!(second);
    assert!(
        tokio::time::timeout(Duration::from_millis(5), &mut second)
            .await
            .is_err()
    );
    assert!(
        now() < cutoff,
        "both original operations must enter before cutoff"
    );
    tokio::time::sleep(Duration::from_millis(cutoff.saturating_sub(now()) + 5)).await;
    lock.commit().unwrap();
    assert!(matches!(first.await, Err(Error::RunnerAuthority)));
    assert!(matches!(second.await, Err(Error::RunnerAuthority)));
    assert_eq!(
        store.approval_summary(request.id).await.unwrap().state,
        "pending"
    );
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM approval_grants", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
    store.shutdown().await.unwrap();
}
