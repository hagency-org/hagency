use super::fixture::*;
use serde_json::json;
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

async fn request(stream: &mut TcpStream) -> Vec<u8> {
    let mut bytes = vec![];
    let mut buffer = [0; 4096];
    loop {
        let n = stream.read(&mut buffer).await.unwrap();
        assert_ne!(n, 0);
        bytes.extend_from_slice(&buffer[..n]);
        assert!(bytes.len() <= 40 * 1024);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                        .map(|(_, v)| v.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            if bytes.len() == end + 4 + length {
                return bytes;
            }
        }
    }
}
fn response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
async fn fake(reply: String, delay: Duration) -> (SocketAddr, JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let bytes = request(&mut stream).await;
        tokio::time::sleep(delay).await;
        let _ = stream.write_all(reply.as_bytes()).await;
        bytes
    });
    (address, server)
}
pub(super) async fn lost_proxy(actual: SocketAddr, corrupt: bool) -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut client, _) = listener.accept().await.unwrap();
        let bytes = request(&mut client).await;
        // The real API pins Host to its own listener. The fixture forwards only
        // the Host address; runner capability and mutation bytes stay identical.
        let bytes = String::from_utf8(bytes)
            .unwrap()
            .replacen(
                &format!("host: {address}\r\n"),
                &format!("host: {actual}\r\n"),
                1,
            )
            .into_bytes();
        let mut backend = TcpStream::connect(actual).await.unwrap();
        backend.write_all(&bytes).await.unwrap();
        let mut reply = vec![];
        backend.read_to_end(&mut reply).await.unwrap();
        assert!(reply.starts_with(b"HTTP/1.1 200"));
        assert!(reply.len() < 64 * 1024);
        if corrupt {
            client
                .write_all(response("{\"conversation\":{},\"replayed\":false}").as_bytes())
                .await
                .unwrap();
        }
    });
    (address, server)
}
#[tokio::test]
async fn native_mcp_coordination_recovery() {
    let f = Fixture::new().await;
    for corrupt in [false, true] {
        let key = if corrupt { "corrupt" } else { "lost" };
        let args = json!({"call_id":key,"label":"Durable conversation","participant_engagements":[f.agents[1]]});
        let (address, proxy) = lost_proxy(f.address, corrupt).await;
        let client = Client::new(address, &f.cap, &f.parent.task_id).await;
        let lost = client.call("open_conversation", args.clone()).await;
        assert_eq!(lost["isError"], true);
        assert!(lost.to_string().contains("outcome unknown"));
        assert!(!lost.to_string().contains(&f.cap.secret));
        proxy.await.unwrap();
        client.close().await;
        let before = f.count("internal_conversations");
        let c = f.client().await;
        let replay = c.ok("open_conversation", args.clone()).await;
        assert_eq!(replay["replayed"], true);
        assert_eq!(f.count("internal_conversations"), before);
        let mut changed = args.clone();
        changed["label"] = json!("Changed after lost response");
        c.refused("open_conversation", changed).await;
        let mut stale = f.cap.clone();
        stale.fence += 1;
        let stale = Client::new(f.address, &stale, &f.parent.task_id).await;
        stale.refused("open_conversation", args).await;
        stale.close().await;
        c.close().await;
    }
    assert_eq!(f.count("internal_conversations"), 2);
    f.close().await;
}
#[tokio::test]
async fn native_mcp_coordination_catalog() {
    let f = Fixture::new().await;
    let mut c = f.client().await;
    // Diagnostic only: rmcp's `TransportClosed` means the helper's stdio
    // closed, not a deadline, so the deciding evidence is the child's own exit
    // state, which is otherwise never queried before `close()`.
    let tools = match c.service.list_all_tools().await {
        Ok(tools) => tools,
        Err(error) => {
            let evidence = c.exit_evidence().await;
            panic!("tools/list failed: {error:?}; {evidence}");
        }
    };
    let names: std::collections::BTreeSet<_> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(
        names,
        std::collections::BTreeSet::from([
            "get_task",
            "accept_task",
            "transition_task",
            "comment_task",
            "update_task_execution",
            "complete_task_with_reply",
            "delegate_task",
            "open_conversation",
            "get_conversation",
            "update_conversation_members",
            "close_conversation",
            "send_peer_message",
            "read_peer_inbox",
            "create_graph",
            "get_graph",
            "list_graphs",
            "cancel_graph",
            "report_graph_result",
            "read_graph_dependencies",
            "read_graph_dependency"
        ])
    );
    for tool in tools {
        assert_eq!(tool.input_schema["additionalProperties"], false);
        assert!(
            !serde_json::to_string(&*tool.input_schema)
                .unwrap()
                .contains("secret")
        );
        for field in [
            "actor",
            "capability",
            "method",
            "path",
            "url",
            "registration",
            "report_grant",
        ] {
            let args = json!({field:"untrusted"});
            c.refused(&tool.name, args).await;
        }
    }
    // Typed endpoint references and all nested authority fields fail before routing.
    for (name, args) in [
        ("get_graph", json!({"graph_id":"../operator"})),
        (
            "get_conversation",
            json!({"conversation_id":"x?capability=forged"}),
        ),
        (
            "delegate_task",
            json!({"call_id":"d","assignee_engagement":f.agents[1],"definition":{"title":"Work","creator_session_id":"forged"}}),
        ),
        (
            "create_graph",
            json!({"call_id":"g","conversation_id":"group","definition":{"label":"Work","nodes":[{"id":"n","assignee":"session","description":"Work","report_grant":"forged"}]}}),
        ),
        (
            "report_graph_result",
            json!({"graph_id":"g","call_id":"r","node_id":"n","outcome":{"kind":"complete","result":null,"execution_epoch":1}}),
        ),
        ("read_peer_inbox", json!({"limit":33})),
        ("list_graphs", json!({"limit":0})),
        (
            "read_graph_dependencies",
            json!({"graph_id":"g","after":9007199254740992_u64}),
        ),
    ] {
        c.refused(name, args).await;
    }
    let mut s = direct(f.address, f.cap.clone(), &f.parent.task_id).await;
    for (i, name) in [json!(42), json!(null), json!(["delegate_task"])]
        .into_iter()
        .enumerate()
    {
        let v=s.handle(&serde_json::to_vec(&json!({"jsonrpc":"2.0","id":i+1,"method":"tools/call","params":{"name":name,"arguments":{}}})).unwrap()).await.unwrap().unwrap();
        assert_eq!(v["error"]["code"], -32602);
    }
    assert_eq!(f.count("internal_conversations"), 0);
    assert_eq!(f.count("canonical_tasks"), 1);
    c.close().await;
    f.close().await;
}
#[tokio::test]
async fn native_mcp_coordination_bounds() {
    let f = Fixture::new().await;
    let c = f.client().await;
    let g = group(&c, &f).await;
    let member = participant(&g, &f.agents[1]);
    // Coordination may exceed the unchanged task budget, within its own body/frame cap.
    c.ok("send_peer_message",json!({"call_id":"large","conversation_id":g["id"],"recipient_session_ids":[member],"kind":"notification","summary":"Bounded body","body":"z".repeat(17*1024)})).await;
    c.refused(
        "comment_task",
        json!({"id":f.parent.task_id,"call_id":"escaped","text":"\n".repeat(8192)}),
    )
    .await;
    c.close().await;
    let summary = |id: &str| json!({"id":id,"conversation_id":"conversation","label":"Graph","state":"active","created_at":1,"node_count":1});
    let good_dependency = json!({"dependency":{"sequence":1,"node_id":"n","task_id":"task","execution_epoch":1,"digest":"digest"},"result":{"value":0.25}});
    let malformed=[
  response("[]{}"),response("[{\"id\":\"a\",\"id\":\"b\"}]"),
  response(&json!([summary("a"),summary("a")]).to_string()),
  response(&json!([summary("b"),summary("a")]).to_string()),
  "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 65537\r\n\r\n".into(),
  response(&" ".repeat(65537)),
  "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/stolen\r\nContent-Length: 0\r\n\r\n".into(),
  "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n[]".into(),
  "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\nContent-Length: 2\r\n\r\n[]".into(),
 ];
    for reply in malformed {
        let (address, server) = fake(reply, Duration::ZERO).await;
        let client = Client::new(address, &f.cap, &f.parent.task_id).await;
        let v = client.call("list_graphs", json!({"limit":2})).await;
        assert_eq!(v["isError"], true);
        assert!(!v.to_string().contains("outcome unknown"));
        assert!(!v.to_string().contains(&f.cap.secret));
        server.await.unwrap();
        client.close().await;
    }
    // Dependency hydration uses POST but has read semantics, including a lost response.
    let (address, server) = fake(String::new(), Duration::ZERO).await;
    let client = Client::new(address, &f.cap, &f.parent.task_id).await;
    let v = client
        .call(
            "read_graph_dependency",
            json!({"graph_id":"g","node_id":"n"}),
        )
        .await;
    assert_eq!(v["isError"], true);
    assert!(!v.to_string().contains("outcome unknown"));
    assert!(
        server
            .await
            .unwrap()
            .starts_with(b"POST /api/native/v1/runner/graphs/g/dependencies ")
    );
    client.close().await;
    // Projection accepts a fractional result, removes unknown top-level fields, and
    // refuses a wrong node rather than letting a successful HTTP status assert scope.
    let mut projected = good_dependency.clone();
    projected["private_credential"] = json!("not forwarded");
    let (address, server) = fake(response(&projected.to_string()), Duration::ZERO).await;
    let client = Client::new(address, &f.cap, &f.parent.task_id).await;
    assert_eq!(
        client
            .ok(
                "read_graph_dependency",
                json!({"graph_id":"g","node_id":"n"})
            )
            .await,
        good_dependency
    );
    server.await.unwrap();
    client.close().await;
    let (address, server) = fake(response(&good_dependency.to_string()), Duration::ZERO).await;
    let client = Client::new(address, &f.cap, &f.parent.task_id).await;
    client
        .refused(
            "read_graph_dependency",
            json!({"graph_id":"g","node_id":"other"}),
        )
        .await;
    server.await.unwrap();
    client.close().await;
    // A stalled mutation has exactly one attempt and never claims the absence of a commit.
    let (address, server) = fake(response("{}"), Duration::from_millis(5100)).await;
    let client = Client::new(address, &f.cap, &f.parent.task_id).await;
    let v = client
        .call(
            "open_conversation",
            json!({"call_id":"slow","label":"Slow","participant_engagements":[f.agents[1]]}),
        )
        .await;
    assert_eq!(v["isError"], true);
    assert!(v.to_string().contains("outcome unknown"));
    server.await.unwrap();
    client.close().await;
    f.close().await;
}
