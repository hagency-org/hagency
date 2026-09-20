use hagency_runtime::{
    claude::{TaskMcp, session::*, task_arguments},
    task_mcp::owned_task_tools,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};
type Driver = SessionDriver<DuplexStream, DuplexStream, DuplexStream>;
struct Peer {
    input: BufReader<DuplexStream>,
    output: DuplexStream,
    _stderr: DuplexStream,
}
fn pair() -> (Driver, Peer) {
    let (stdin, input) = tokio::io::duplex(16 * 1024);
    let (stdout, output) = tokio::io::duplex(16 * 1024);
    let (stderr, error) = tokio::io::duplex(1024);
    (
        Driver::new(
            stdout,
            stdin,
            stderr,
            Limits {
                write_timeout_ms: 1000,
                event_wait_ms: 2000,
                lifetime_ms: 60_000,
            },
        )
        .unwrap(),
        Peer {
            input: BufReader::new(input),
            output,
            _stderr: error,
        },
    )
}
async fn read(p: &mut Peer) -> Value {
    let mut line = String::new();
    p.input.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}
async fn reply(p: &mut Peer, id: &Value, value: Value) {
    let mut bytes=serde_json::to_vec(&json!({"type":"control_response","response":{"subtype":"success","request_id":id,"response":value}})).unwrap();
    bytes.push(b'\n');
    p.output.write_all(&bytes).await.unwrap();
}
async fn ready() -> (Driver, Peer) {
    let (mut d, mut p) = pair();
    let (result, ()) = tokio::join!(d.initialize(), async {
        let v = read(&mut p).await;
        reply(&mut p, &v["request_id"], json!({})).await;
    });
    result.unwrap();
    (d, p)
}
fn helper(send: bool, receive: bool) -> TaskMcp {
    let mut h = TaskMcp::new(
        std::env::temp_dir().join("native helper 中文"),
        "original_task".into(),
    )
    .unwrap();
    if send {
        h = h.with_file_tools();
    }
    if receive {
        h = h.with_receive_tools();
    }
    h
}
fn status(send: bool, receive: bool) -> Value {
    json!({"mcpServers":[{"name":"hagency_task_writer","status":"connected",
    "tools":owned_task_tools(send,receive).into_iter().map(|name|json!({"name":name})).collect::<Vec<_>>()}]})
}
async fn bind_peer(p: &mut Peer, send: bool, receive: bool, mode: &str) {
    let before = read(p).await;
    assert_eq!(before["request"], json!({"subtype":"mcp_status"}));
    if mode == "before" {
        reply(p, &before["request_id"], status(false, false)).await;
        return;
    }
    reply(p, &before["request_id"], json!({"mcpServers":[]})).await;
    let set = read(p).await;
    assert_eq!(set["request"]["subtype"], "mcp_set_servers");
    let server = &set["request"]["servers"]["hagency_task_writer"];
    assert_eq!(set["request"]["servers"].as_object().unwrap().len(), 1);
    assert_eq!(server["type"], "stdio");
    assert_eq!(server["args"], json!(["mcp", "--owned-task-profile"]));
    assert_eq!(server["alwaysLoad"], true);
    assert_eq!(server["timeout"], 5000);
    assert_eq!(server["env"].get("HAGENCY_FILE_TOOLS").is_some(), send);
    assert_eq!(
        server["env"].get("HAGENCY_RECEIVE_FILE_TOOLS").is_some(),
        receive
    );
    assert_eq!(
        server["env"].as_object().unwrap().len(),
        usize::from(send) + usize::from(receive)
    );
    assert!(!set.to_string().contains("HAGENCY_RUNNER_CAPABILITY"));
    let mut ack = json!({"added":["hagency_task_writer"],"removed":[],"errors":{}});
    match mode {
        "errors" => ack["errors"] = json!({"hagency_task_writer":"private failure sentinel"}),
        "removed" => ack["removed"] = json!(["foreign"]),
        "added" => ack["added"] = json!([]),
        "extra-ack" => ack["other"] = json!(true),
        "wrong-id" => {
            reply(p, &json!("wrong"), ack).await;
            return;
        }
        _ => {}
    }
    reply(p, &set["request_id"], ack).await;
    if ["errors", "removed", "added", "extra-ack"].contains(&mode) {
        return;
    }
    let after = read(p).await;
    assert_eq!(after["request"], json!({"subtype":"mcp_status"}));
    let mut s = status(send, receive);
    match mode {
        "pending" => s["mcpServers"][0]["status"] = json!("pending"),
        "missing" => {
            s["mcpServers"][0].as_object_mut().unwrap().remove("tools");
        }
        "extra-tool" => s["mcpServers"][0]["tools"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"other"})),
        "duplicate" => s["mcpServers"][0]["tools"][1] = s["mcpServers"][0]["tools"][0].clone(),
        "extra-server" => {
            let copy = s["mcpServers"][0].clone();
            s["mcpServers"].as_array_mut().unwrap().push(copy);
        }
        _ => {}
    }
    reply(p, &after["request_id"], s).await;
}
#[test]
fn native_claude_task_mcp_profile() {
    let args = task_arguments("sonnet", true).unwrap();
    assert!(args.windows(2).any(|a| a == ["--permission-mode", "auto"]));
    assert!(args.contains(&"--strict-mcp-config".into()));
    assert!(args.contains(&"--mcp-config={\"mcpServers\":{}}".into()));
    assert!(args.contains(&"--setting-sources=".into()));
    let settings: Value = serde_json::from_str(
        args.iter()
            .find_map(|s| s.strip_prefix("--settings="))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        settings,
        json!({"disableAllHooks":true,"permissions":{"ask":["Bash(gh *)","Bash(git push *)"]}})
    );
    let rules = args
        .iter()
        .find_map(|s| s.strip_prefix("--allowedTools="))
        .unwrap()
        .split(',')
        .collect::<Vec<_>>();
    assert_eq!(
        rules,
        vec![
            "mcp__hagency_task_writer__get_task",
            "mcp__hagency_task_writer__update_task_execution",
            "mcp__hagency_task_writer__transition_task",
            "mcp__hagency_task_writer__complete_task_with_reply",
            "mcp__hagency_task_writer__read_conversation"
        ]
    );
    // The TS ask patterns contain '*'; only the allow rules must be exact.
    assert!(!rules.iter().any(|s| s.contains('*')));
    assert!(!args.iter().any(|s| s.contains("bypass")
        || s.contains("skip-permissions")
        || s.contains("send_file")));
    for id in ["", "task\nignore", "../task", &"x".repeat(129)] {
        assert!(TaskMcp::new(std::env::temp_dir().join("helper"), id.into()).is_err());
    }
    for path in [
        std::path::PathBuf::from("relative"),
        std::env::temp_dir().join("../helper"),
        std::env::temp_dir().join("helper\nother"),
    ] {
        assert!(TaskMcp::new(path, "task".into()).is_err());
    }
    assert!(task_arguments("--dangerously-skip-permissions", true).is_err());
}
#[test]
fn native_claude_launch_ts_parity() {
    // References: backend-v2.js::claudeThreadSessionArgs and
    // tests/claude-thread-runtime.test.js's model/configuration cases.
    for may_write in [false, true] {
        let args = task_arguments("claude-sonnet-4-6", may_write).unwrap();
        let mode = args.iter().position(|s| s == "--permission-mode").unwrap();
        assert_eq!(args[mode + 1], if may_write { "auto" } else { "plan" });
        assert_eq!(args.iter().filter(|s| *s == "--permission-mode").count(), 1);
        let settings: Value = serde_json::from_str(
            args.iter()
                .find_map(|s| s.strip_prefix("--settings="))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            settings["permissions"],
            json!({"ask":["Bash(gh *)","Bash(git push *)"]})
        );
        for model in [
            "",
            "--no-permissions",
            "opus;touch",
            "claude opus",
            "../other",
            "模型",
            &"x".repeat(65),
        ] {
            assert!(task_arguments(model, may_write).is_err(), "{model}");
        }
        assert!(task_arguments(&"x".repeat(64), may_write).is_ok());
    }
}
#[tokio::test]
async fn native_claude_task_mcp_binding() {
    for (send, receive) in [(false, false), (true, false), (false, true), (true, true)] {
        let (mut d, mut p) = ready().await;
        let (result, ()) = tokio::join!(
            d.bind_task_mcp(helper(send, receive)),
            bind_peer(&mut p, send, receive, "ok")
        );
        result.unwrap();
        assert_eq!(d.phase(), Phase::Ready);
        assert!(d.last_observation().is_none());
        let (result, prompt) = tokio::join!(d.prompt("original bounded input"), read(&mut p));
        result.unwrap();
        let text = prompt["message"]["content"].as_str().unwrap();
        assert!(text.contains("canonical task ID is original_task"));
        assert!(text.ends_with("original bounded input"));
        assert_eq!(text.contains("Use send_file"), send);
        assert_eq!(text.contains("Use list_received_files"), receive);
        assert!(text.contains("a final answer does not complete"));
        assert_eq!(d.phase(), Phase::Running);
        assert!(d.bind_task_mcp(helper(false, false)).await.is_err());
        assert_eq!(d.phase(), Phase::Closed);
    }
}
#[tokio::test(start_paused = true)]
async fn native_claude_task_mcp_refusals() {
    for mode in [
        "before",
        "errors",
        "removed",
        "added",
        "extra-ack",
        "wrong-id",
        "pending",
        "missing",
        "extra-tool",
        "duplicate",
        "extra-server",
    ] {
        let (mut d, mut p) = ready().await;
        let (result, ()) = tokio::join!(
            d.bind_task_mcp(helper(false, false)),
            bind_peer(&mut p, false, false, mode)
        );
        assert_eq!(
            result,
            Err(if mode == "wrong-id" {
                Error::Identity
            } else {
                Error::TaskWriterStartup
            }),
            "{mode}"
        );
        assert_eq!(d.phase(), Phase::Closed);
        assert!(d.prompt("must not execute").await.is_err());
        assert!(!format!("{:?}", d.termination()).contains("private failure"));
    }
    let (mut d, _p) = pair();
    assert_eq!(
        d.bind_task_mcp(helper(false, false)).await,
        Err(Error::State)
    );
    let (mut d, mut p) = ready().await;
    let (r, ()) = tokio::join!(
        d.bind_task_mcp(helper(false, false)),
        bind_peer(&mut p, false, false, "ok")
    );
    r.unwrap();
    assert_eq!(
        d.bind_task_mcp(helper(false, false)).await,
        Err(Error::State)
    );
    let (mut d, mut p) = ready().await;
    let (r, ()) = tokio::join!(
        d.bind_task_mcp(helper(false, false)),
        bind_peer(&mut p, false, false, "ok")
    );
    r.unwrap();
    assert!(d.prompt(&"x".repeat(64 * 1024)).await.is_err());
    assert_eq!(d.phase(), Phase::Closed);
    let (mut d, _p) = ready().await;
    drop(d.bind_task_mcp(helper(false, false)));
    assert_eq!(d.phase(), Phase::Ready);
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(10),
            d.bind_task_mcp(helper(false, false))
        )
        .await
        .is_err()
    );
    assert_eq!(d.termination().unwrap().cause, Error::Cancelled);
    assert_eq!(d.phase(), Phase::Closed);
    let (mut d, mut p) = ready().await;
    let start = tokio::time::Instant::now();
    let (result, ()) = tokio::join!(d.bind_task_mcp(helper(false, false)), async {
        let v = read(&mut p).await;
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        reply(&mut p, &v["request_id"], json!({"mcpServers":[]})).await;
        let _ = read(&mut p).await;
    });
    assert_eq!(result, Err(Error::Timeout));
    // One 2-second deadline for all stages, not a new wait after the first ACK.
    assert_eq!(start.elapsed(), std::time::Duration::from_millis(2000));
    assert_eq!(d.phase(), Phase::Closed);
}
