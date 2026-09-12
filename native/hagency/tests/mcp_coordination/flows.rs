use super::fixture::*;
use serde_json::{Value, json};

#[tokio::test]
async fn native_mcp_coordination_delegation() {
    let f = Fixture::new().await;
    let c = f.client().await;
    let args = json!({"call_id":"delegate","assignee_engagement":f.agents[1],"input_sequences":[f.seq],"definition":{"title":"Implement sum","parent_id":f.parent.task_id}});
    let result = c.ok("delegate_task", args.clone()).await;
    assert_eq!(result["activation"], "pending");
    assert_eq!(f.count("canonical_tasks"), 2);
    assert_eq!(c.ok("delegate_task", args.clone()).await["replayed"], true);
    let mut changed = args.clone();
    changed["definition"]["title"] = json!("Changed content");
    c.refused("delegate_task", changed).await;
    let mut foreign = args.clone();
    foreign["call_id"] = json!("foreign");
    foreign["assignee_engagement"] = json!(f.agents[2]);
    c.refused("delegate_task", foreign).await;
    let mut wrong = args.clone();
    wrong["definition"]["parent_id"] = json!("other_parent");
    c.refused("delegate_task", wrong).await;
    let mut unowned = args.clone();
    unowned["call_id"] = json!("unowned");
    unowned["root_sequence"] = json!(99999);
    c.refused("delegate_task", unowned).await;
    c.close().await;
    let c = f.client().await;
    assert_eq!(
        c.ok("delegate_task", args.clone()).await["task_id"],
        result["task_id"]
    );
    assert_eq!(f.count("canonical_tasks"), 2);
    f.domain
        .park_dispatch(f.cap.clone(), true, now())
        .await
        .unwrap();
    c.refused("delegate_task", args).await;
    c.close().await;
    f.close().await;
}

#[tokio::test]
async fn native_mcp_coordination_conversation() {
    let f = Fixture::new().await;
    let c = f.client().await;
    c.refused("open_conversation",json!({"call_id":"foreign","label":"Wrong project","participant_engagements":[f.agents[2]]})).await;
    let g = group(&c, &f).await;
    let id = g["id"].as_str().unwrap();
    let member = participant(&g, &f.agents[1]);
    assert_eq!(
        c.ok("get_conversation", json!({"conversation_id":id}))
            .await["id"],
        id
    );
    let sent=c.ok("send_peer_message",json!({"call_id":"message","conversation_id":id,"recipient_session_ids":[member],"kind":"request","summary":"计算结果","body":"Check fractions","data":{"score":0.25}})).await;
    let task = "peer_task";
    f.domain
        .create_canonical_task(
            task.into(),
            member.clone(),
            "Process peer input".into(),
            now(),
        )
        .await
        .unwrap();
    let cap = f
        .start(
            "member",
            &member,
            task,
            Some(sent["sequence"].as_u64().unwrap()),
        )
        .await;
    let peer = Client::new(f.address, &cap, task).await;
    let page = peer.ok("read_peer_inbox", json!({"limit":1})).await;
    assert_eq!(page["messages"].as_array().unwrap().len(), 1);
    assert_eq!(
        page["messages"][0]["message"]["data"],
        json!({"score":0.25})
    );
    assert_eq!(
        peer.ok(
            "read_peer_inbox",
            json!({"after":sent["sequence"],"limit":1})
        )
        .await["messages"],
        json!([])
    );
    peer.ok("send_peer_message",json!({"call_id":"response","conversation_id":id,"recipient_session_ids":[f.parent.session_id],"kind":"response","summary":"Verified"})).await;
    peer.refused("update_conversation_members",json!({"conversation_id":id,"call_id":"not_creator","expected_revision":0,"participant_engagements":[f.agents[0]]})).await;
    peer.refused(
        "close_conversation",
        json!({"conversation_id":id,"call_id":"not_creator_close","expected_revision":0}),
    )
    .await;
    let (foreign_cap, foreign_task) = f.independent("foreign", "c").await;
    let foreign = Client::new(f.address, &foreign_cap, &foreign_task).await;
    foreign
        .refused("get_conversation", json!({"conversation_id":id}))
        .await;
    foreign.refused("send_peer_message",json!({"call_id":"foreign_send","conversation_id":id,"recipient_session_ids":[member],"kind":"request","summary":"Wrong project"})).await;
    let (same_cap, same_task) = f.independent("same_agent_wrong_session", "b").await;
    let same = Client::new(f.address, &same_cap, &same_task).await;
    same.refused("get_conversation", json!({"conversation_id":id}))
        .await;
    same.close().await;
    let other = group(&c, &f).await;
    assert_eq!(other["id"], id);
    let changed=c.ok("update_conversation_members",json!({"conversation_id":id,"call_id":"remove","expected_revision":0,"participant_engagements":[f.agents[0]]})).await;
    assert_eq!(changed["conversation"]["revision"], 1);
    peer.refused("get_conversation", json!({"conversation_id":id}))
        .await;
    peer.refused("read_peer_inbox", json!({})).await;
    c.refused("send_peer_message",json!({"call_id":"removed","conversation_id":id,"recipient_session_ids":[member],"kind":"request","summary":"No longer current"})).await;
    c.refused(
        "close_conversation",
        json!({"conversation_id":id,"call_id":"stale_revision","expected_revision":0}),
    )
    .await;
    assert_eq!(
        c.ok(
            "close_conversation",
            json!({"conversation_id":id,"call_id":"close","expected_revision":1})
        )
        .await["conversation"]["state"],
        "closed"
    );
    foreign.close().await;
    peer.close().await;
    c.close().await;
    f.close().await;
}
fn node<'a>(graph: &'a Value, id: &str) -> &'a Value {
    graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["node_id"] == id)
        .unwrap()
}
#[tokio::test]
async fn native_mcp_coordination_graph() {
    let f = Fixture::new().await;
    let c = f.client().await;
    let g = group(&c, &f).await;
    let req = json!({"call_id":"graph","conversation_id":g["id"],"definition":{"label":"Implement and verify","nodes":[{"id":"实现/一步","assignee":participant(&g,&f.agents[1]),"description":"Implement"},{"id":"verify","assignee":participant(&g,&f.agents[0]),"description":"Verify","depends_on":["实现/一步"]}]}});
    let graph = c.ok("create_graph", req.clone()).await["workflow"].clone();
    let id = graph["id"].as_str().unwrap();
    assert_eq!(node(&graph, "verify")["state"], "pending");
    assert_eq!(c.ok("create_graph", req.clone()).await["replayed"], true);
    let mut changed = req.clone();
    changed["definition"]["label"] = json!("Different");
    c.refused("create_graph", changed).await;
    let mut wrong = req.clone();
    wrong["call_id"] = json!("wrong_assignee");
    wrong["definition"]["nodes"][0]["assignee"] = json!(f.agents[1]);
    c.refused("create_graph", wrong).await;
    let page = c.ok("list_graphs", json!({"limit":1})).await;
    assert_eq!(page["graphs"][0]["id"], id);
    assert_eq!(
        c.ok("list_graphs", json!({"after":id,"limit":1})).await["graphs"],
        json!([])
    );
    // An ordinary peer input cannot make an unready graph node runnable.
    let pending = &node(&graph, "verify")["binding"];
    let extra = c.ok("send_peer_message", json!({"call_id":"early_peer","conversation_id":g["id"],"recipient_session_ids":[pending["session_id"]],"kind":"request","summary":"Attempt early work"})).await;
    assert!(
        f.domain
            .enqueue_peer_dispatch(
                dispatch(
                    "bypass",
                    pending["session_id"].as_str().unwrap(),
                    pending["task_id"].as_str().unwrap()
                ),
                vec![extra["sequence"].as_u64().unwrap()]
            )
            .await
            .is_err()
    );
    let (foreign_cap, foreign_task) = f.independent("foreign_graph", "c").await;
    let foreign = Client::new(f.address, &foreign_cap, &foreign_task).await;
    foreign.refused("get_graph", json!({"graph_id":id})).await;
    foreign
        .refused(
            "read_graph_dependency",
            json!({"graph_id":id,"node_id":"实现/一步"}),
        )
        .await;
    foreign
        .refused(
            "cancel_graph",
            json!({"graph_id":id,"call_id":"foreign_cancel"}),
        )
        .await;
    assert_eq!(
        foreign.ok("list_graphs", json!({})).await["graphs"],
        json!([])
    );
    foreign.close().await;
    let n = &node(&graph, "实现/一步")["binding"];
    let task = n["task_id"].as_str().unwrap();
    let cap = f
        .start(
            "worker",
            n["session_id"].as_str().unwrap(),
            task,
            Some(n["message_sequence"].as_u64().unwrap()),
        )
        .await;
    let worker = Client::new(f.address, &cap, task).await;
    let report = json!({"graph_id":id,"call_id":"result","node_id":"实现/一步","outcome":{"kind":"complete","result":{"score":0.25,"answer":3}}});
    worker.refused("report_graph_result", report.clone()).await;
    worker
        .refused(
            "cancel_graph",
            json!({"graph_id":id,"call_id":"not_creator"}),
        )
        .await;
    worker.refused("get_graph", json!({"graph_id":id})).await;
    worker
        .refused(
            "read_graph_dependency",
            json!({"graph_id":id,"node_id":"verify"}),
        )
        .await;
    worker
        .ok(
            "transition_task",
            json!({"id":task,"call_id":"done","status":"done"}),
        )
        .await;
    let mut wrong = report.clone();
    wrong["node_id"] = json!("verify");
    worker.refused("report_graph_result", wrong).await;
    let (address, proxy) = super::recovery::lost_proxy(f.address, false).await;
    let disconnected = Client::new(address, &cap, task).await;
    let lost = disconnected
        .call("report_graph_result", report.clone())
        .await;
    assert_eq!(lost["isError"], true);
    assert!(lost.to_string().contains("outcome unknown"));
    proxy.await.unwrap();
    disconnected.close().await;
    let receipt = worker.ok("report_graph_result", report.clone()).await;
    assert_eq!(receipt["replayed"], true);
    assert_eq!(receipt["execution_epoch"], 1);
    assert_eq!(
        worker.ok("report_graph_result", report.clone()).await["replayed"],
        true
    );
    let mut changed = report;
    changed["outcome"]["result"] = json!({"score":1});
    worker.refused("report_graph_result", changed).await;
    f.domain
        .complete_dispatch(cap, json!({"reported":true}), now())
        .await
        .unwrap();
    worker.close().await;
    let graph = c.ok("get_graph", json!({"graph_id":id})).await;
    let n = &node(&graph, "verify")["binding"];
    let task = n["task_id"].as_str().unwrap();
    let cap = f
        .start(
            "verify",
            n["session_id"].as_str().unwrap(),
            task,
            Some(n["message_sequence"].as_u64().unwrap()),
        )
        .await;
    let verifier = Client::new(f.address, &cap, task).await;
    let refs = verifier
        .ok("read_graph_dependencies", json!({"graph_id":id,"limit":1}))
        .await;
    assert_eq!(refs["dependencies"][0]["node_id"], "实现/一步");
    assert_eq!(
        verifier
            .ok(
                "read_graph_dependency",
                json!({"graph_id":id,"node_id":"实现/一步"})
            )
            .await["result"],
        json!({"score":0.25,"answer":3})
    );
    assert_eq!(
        verifier
            .ok(
                "read_graph_dependencies",
                json!({"graph_id":id,"after":refs["dependencies"][0]["sequence"],"limit":1})
            )
            .await["dependencies"],
        json!([])
    );
    let cancellation = c
        .ok("cancel_graph", json!({"graph_id":id,"call_id":"cancel"}))
        .await;
    assert_eq!(cancellation["workflow"]["state"], "cancelled");
    verifier
        .refused(
            "read_graph_dependency",
            json!({"graph_id":id,"node_id":"实现/一步"}),
        )
        .await;
    verifier
        .refused(
            "transition_task",
            json!({"id":task,"call_id":"done_after_cancel","status":"done"}),
        )
        .await;
    assert_eq!(
        c.ok("get_task", json!({"id":f.parent.task_id})).await["task"]["status"],
        "in_progress"
    );
    verifier.close().await;
    c.close().await;
    f.close().await;
}
