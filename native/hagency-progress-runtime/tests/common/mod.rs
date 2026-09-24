#![allow(dead_code)]
use hagency_runtime::codex::session::{Error, Phase, SessionDriver, Settings};
use hagency_runtime::codex::transport::Limits;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::time::timeout;
pub type Session = SessionDriver<DuplexStream, DuplexStream, DuplexStream>;
pub struct Peer {
    stdin: DuplexStream,
    stdout: DuplexStream,
    _stderr: DuplexStream,
}
fn cwd() -> String {
    std::env::temp_dir()
        .join("hagency-session-fixture")
        .to_str()
        .unwrap()
        .into()
}
fn settings(read_only: bool) -> Settings {
    let value = Settings::new(cwd().into(), "fixture-model".into(), "medium".into()).unwrap();
    if read_only { value.read_only() } else { value }
}
pub fn fixture(read_only: bool) -> (Session, Peer) {
    let (stdin, peer_in) = tokio::io::duplex(131072);
    let (stdout, peer_out) = tokio::io::duplex(131072);
    let (stderr, peer_err) = tokio::io::duplex(1024);
    (
        Session::new(
            stdout,
            stdin,
            stderr,
            settings(read_only),
            Limits {
                write_timeout_ms: 500,
                event_wait_ms: 1000,
                lifetime_ms: 30_000,
            },
            1000,
        )
        .unwrap(),
        Peer {
            stdin: peer_in,
            stdout: peer_out,
            _stderr: peer_err,
        },
    )
}
async fn read(reader: &mut DuplexStream) -> Value {
    timeout(Duration::from_secs(3), async {
        let mut bytes = Vec::new();
        loop {
            let byte = reader.read_u8().await.unwrap();
            bytes.push(byte);
            assert!(bytes.len() <= 1_048_577);
            if byte == b'\n' {
                return serde_json::from_slice(&bytes).unwrap();
            }
        }
    })
    .await
    .unwrap()
}
pub async fn write(peer: &mut Peer, value: Value) {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    timeout(Duration::from_secs(3), peer.stdout.write_all(&bytes))
        .await
        .unwrap()
        .unwrap();
}
async fn exchange(peer: &mut Peer, result: Value, before: Vec<Value>) -> Value {
    let request = read(&mut peer.stdin).await;
    for event in before {
        write(peer, event).await;
    }
    write(peer, json!({ "id": request["id"], "result": result })).await;
    request
}
fn thread_result(read_only: bool) -> Value {
    json!({ "thread": { "id": "thread-one", "cwd": cwd(), "status": { "type": "idle" }, "turns": [] },
        "cwd": cwd(), "model": "fixture-model", "modelProvider": "fixture", "approvalPolicy": "on-request", "approvalsReviewer": "user",
        "sandbox": { "type": if read_only { "readOnly" } else { "workspaceWrite" }, "networkAccess": false } })
}
fn turn(status: &str) -> Value {
    json!({ "id": "turn-one", "items": [], "status": status })
}
pub fn note(method: &str, params: Value) -> Value {
    json!({ "method": method, "params": params })
}
pub fn end(status: &str) -> Value {
    note(
        "turn/completed",
        json!({ "threadId": "thread-one", "turn": turn(status) }),
    )
}
pub fn item_event(id: &str, text: &str, complete: bool) -> Value {
    let key = if complete {
        "completedAtMs"
    } else {
        "startedAtMs"
    };
    note(
        if complete {
            "item/completed"
        } else {
            "item/started"
        },
        json!({
            "threadId": "thread-one", "turnId": "turn-one", key: 10,
            "item": { "type": "agentMessage", "id": id, "text": text, "phase": "final_answer" }
        }),
    )
}
fn delta(id: &str, text: &str) -> Value {
    note(
        "item/agentMessage/delta",
        json!({ "threadId": "thread-one", "turnId": "turn-one", "itemId": id, "delta": text }),
    )
}
pub async fn initialize(session: &mut Session, peer: &mut Peer) {
    let (result, ()) = tokio::join!(session.initialize(), async {
        let request = read(&mut peer.stdin).await;
        assert_eq!(request["method"], "initialize");
        write(peer, json!({ "id": request["id"], "result": { "userAgent": "fixture/0.153.4", "platformFamily": "unix", "platformOs": "fixture", "codexHome": "/fixture" } })).await;
        assert_eq!(read(&mut peer.stdin).await["method"], "initialized");
    });
    result.unwrap();
    assert_eq!(session.phase(), Phase::Ready);
}
pub async fn open(session: &mut Session, peer: &mut Peer, read_only: bool) -> Value {
    let (result, request) = tokio::join!(
        session.start_thread(),
        exchange(peer, thread_result(read_only), vec![])
    );
    assert_eq!(result.unwrap(), "thread-one");
    request
}
pub async fn start(
    session: &mut Session,
    peer: &mut Peer,
    before: Vec<Value>,
) -> Result<String, Error> {
    let (result, _) = tokio::join!(
        session.start_turn("fixture input".into()),
        exchange(peer, json!({ "turn": turn("inProgress") }), before)
    );
    result
}
pub async fn running() -> (Session, Peer) {
    let (mut session, mut peer) = fixture(false);
    initialize(&mut session, &mut peer).await;
    open(&mut session, &mut peer, false).await;
    start(&mut session, &mut peer, vec![]).await.unwrap();
    (session, peer)
}
pub fn tool(id: &str, kind: &str, status: &str, complete: bool, exit: Value) -> Value {
    let mut item = json!({"id":id,"type":kind,"status":status,"command":"SECRET /private/.env","cwd":"/private/SECRET","changes":[],"arguments":{"password":"SECRET"},"error":"SECRET","result":"SECRET"});
    if !exit.is_null() {
        item["exitCode"] = exit;
    }
    note(
        if complete {
            "item/completed"
        } else {
            "item/started"
        },
        json!({"threadId":"thread-one","turnId":"turn-one","startedAtMs":1,"completedAtMs":2,"item":item}),
    )
}
