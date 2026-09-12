use hagency::{
    mcp::{FRAME_LIMIT, Session},
    task_client::Context,
};
use hagency_core::tasks::RunnerCapability;
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};
fn capability() -> RunnerCapability {
    RunnerCapability {
        dispatch_id: "dispatch".into(),
        runner_id: "runner".into(),
        fence: 1,
        secret: "a".repeat(64),
    }
}
fn session() -> Session {
    Session::new(Context::new("127.0.0.1:9".parse().unwrap(), capability(), "task".into()).unwrap())
}
fn init(id: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}})
}
async fn request(s: &mut Session, v: Value) -> Option<Value> {
    s.handle(&serde_json::to_vec(&v).unwrap()).await.unwrap()
}
#[tokio::test]
async fn native_mcp_protocol() {
    let mut s = session();
    let before = request(
        &mut s,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
    )
    .await
    .unwrap();
    assert_eq!(before["error"]["code"], -32601);
    request(&mut s, init(json!("1"))).await.unwrap(); // Numeric and string IDs differ.
    assert!(
        request(
            &mut s,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .await
        .is_none()
    );
    let catalog = request(
        &mut s,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    )
    .await
    .unwrap();
    assert_eq!(catalog["result"]["tools"].as_array().unwrap().len(), 20);
    for tool in catalog["result"]["tools"].as_array().unwrap() {
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        assert!(!tool.to_string().contains("secret"));
    }
    for (id, params) in [
        (3, json!({"name":"missing","arguments":{"id":"task"}})),
        (4, json!({"name":"get_task","arguments":[]})),
        (5, json!({"arguments":{"id":"task"}})),
        (
            6,
            json!({"name":"get_task","arguments":{"id":"task"},"task":{}}),
        ),
        (
            8,
            json!({"name":"send_file","arguments":{"call_id":"file","path":"report.txt"}}),
        ),
        (
            9,
            json!({"name":"get_file_delivery","arguments":{"delivery_id":"file_original"}}),
        ),
    ] {
        let response = request(
            &mut s,
            json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":params}),
        )
        .await
        .unwrap();
        assert_eq!(response["error"]["code"], -32602);
        assert!(response.get("result").is_none());
    }
    assert_eq!(
        request(&mut s, json!({"jsonrpc":"2.0","id":7,"method":"ping"}))
            .await
            .unwrap()["result"],
        json!({})
    );
    assert!(
        s.handle(br#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#)
            .await
            .is_err()
    );
    assert!(
        s.handle(br#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#)
            .await
            .is_err()
    );
    for input in [
        br#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#.to_vec(),
        br#"{"jsonrpc":"2.0","id":1.5,"method":"ping"}"#.to_vec(),
        br#"{"jsonrpc":"2.0","id":9007199254740992,"method":"ping"}"#.to_vec(),
        br#"{"jsonrpc":"2.0","id":1,"id":2,"method":"ping"}"#.to_vec(),
        br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"arguments":{"id":"task","id":"other"}}}"#.to_vec(),
        br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_vec(),
        b"[]".to_vec(), b"{}{}".to_vec(), vec![b' ';FRAME_LIMIT+1],
        format!("{}0{}","[".repeat(80),"]".repeat(80)).into_bytes(),
    ] { assert!(session().handle(&input).await.is_err()); }
    let mut s = session();
    for n in 0..4096 {
        request(&mut s, json!({"jsonrpc":"2.0","id":n,"method":"ping"}))
            .await
            .unwrap();
    }
    assert!(
        s.handle(br#"{"jsonrpc":"2.0","id":4096,"method":"ping"}"#)
            .await
            .is_err()
    );
}
fn command() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_hagency"));
    cmd.arg("mcp")
        .env_clear()
        .env("HAGENCY_RUNNER_API_ADDR", "127.0.0.1:9")
        .env(
            "HAGENCY_RUNNER_CAPABILITY",
            serde_json::to_string(&capability()).unwrap(),
        )
        .env("HAGENCY_TASK_ID", "task")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        cmd.env("SystemRoot", root);
    }
    cmd
}

#[tokio::test]
async fn native_mcp_receive_presentation() {
    for flag in ["", "0", "true", " 1", "1\n", "private_receive_marker"] {
        let output = command()
            .env("HAGENCY_RECEIVE_FILE_TOOLS", flag)
            .output()
            .await
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private_receive_marker"));
    }
    for (send, receive) in [(false, false), (true, false), (false, true), (true, true)] {
        // Retain a real listener: malformed or disabled calls must never connect.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut cmd = command();
        cmd.env(
            "HAGENCY_RUNNER_API_ADDR",
            listener.local_addr().unwrap().to_string(),
        );
        if send {
            cmd.env("HAGENCY_FILE_TOOLS", "1");
        }
        if receive {
            cmd.env("HAGENCY_RECEIVE_FILE_TOOLS", "1");
        }
        let mut child = cmd.spawn().unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut frames = vec![
            init(json!(0)),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":"catalog","method":"tools/list"}),
        ];
        let mut invalid = vec![
            ("receive_file", json!({"event_id":"missing_prefix"})),
            ("receive_file", json!({"event_id":""})),
            (
                "receive_file",
                json!({"event_id":format!("${}", "a".repeat(512))}),
            ),
            ("receive_file", json!({})),
            ("list_received_files", json!({"limit":0})),
            ("list_received_files", json!({"limit":17})),
            ("list_received_files", json!({"after":-1})),
            ("list_received_files", json!({"after":1.5})),
            ("list_received_files", json!({"after":9007199254740992_u64})),
        ];
        for field in [
            "path",
            "room_id",
            "url",
            "key",
            "descriptor",
            "capability",
            "verified",
            "sdk",
            "thread_root",
        ] {
            let mut args = json!({"event_id":"$original"});
            args[field] = "private_argument_canary".into();
            invalid.push(("receive_file", args));
            let mut args = json!({"limit":1});
            args[field] = "private_argument_canary".into();
            invalid.push(("list_received_files", args));
        }
        if !receive {
            invalid.push(("receive_file", json!({"event_id":"$original"})));
            invalid.push(("list_received_files", json!({})));
        }
        let count = invalid.len();
        for (id, (name, arguments)) in invalid.into_iter().enumerate() {
            frames.push(json!({"jsonrpc":"2.0","id":id+1,"method":"tools/call","params":{"name":name,"arguments":arguments}}));
        }
        let writer = tokio::spawn(async move {
            for frame in frames {
                let mut bytes = serde_json::to_vec(&frame).unwrap();
                bytes.push(b'\n');
                input.write_all(&bytes).await.unwrap();
            }
        });
        let output = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        writer.await.unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains("private_argument_canary"));
        let replies: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(replies.len(), 2 + count);
        let tools = replies[1]["result"]["tools"].as_array().unwrap();
        assert_eq!(
            tools.len(),
            20 + 2 * usize::from(send) + 2 * usize::from(receive)
        );
        for name in ["list_received_files", "receive_file"] {
            let tool = tools.iter().find(|tool| tool["name"] == name);
            assert_eq!(tool.is_some(), receive);
            if let Some(tool) = tool {
                assert_eq!(tool["inputSchema"]["additionalProperties"], false);
                assert!(
                    tool["description"]
                        .as_str()
                        .unwrap()
                        .contains("untrusted user")
                );
            }
        }
        for reply in &replies[2..] {
            if receive {
                assert_eq!(reply["result"]["isError"], true);
            } else {
                assert_eq!(reply["error"]["code"], -32602);
            }
            assert!(reply["result"].get("structuredContent").is_none());
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(30), listener.accept())
                .await
                .is_err()
        );
    }
}
#[tokio::test]
async fn native_mcp_stdio() {
    let mut missing = command();
    missing
        .env_remove("HAGENCY_RUNNER_CAPABILITY")
        .env("API_TOKEN", "operator_must_not_work");
    let output = missing.output().await.unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("operator_must_not_work"));
    let mut child = command().spawn().unwrap();
    drop(child.stdin.take());
    assert!(
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    let mut child = command().spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(b"{").await.unwrap();
    let status = tokio::time::timeout(Duration::from_secs(15), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.code(), Some(74));
    drop(input);
    let mut child = command().spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    // Consumer retains but never drains stdout; helper must terminate its own
    // blocked write. Keep sender joined and its buffer finite as well.
    let mut payload = serde_json::to_vec(&init(json!(0))).unwrap();
    payload.push(b'\n');
    payload.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n");
    for n in 1..1000 {
        payload.extend_from_slice(
            format!("{{\"jsonrpc\":\"2.0\",\"id\":{n},\"method\":\"tools/list\"}}\n").as_bytes(),
        );
    }
    let sender = tokio::spawn(async move {
        let _ = input.write_all(&payload).await;
    });
    let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.code(), Some(74));
    sender.await.unwrap();
}

#[tokio::test]
async fn native_mcp_file_presentation() {
    for flag in ["", "0", "true", " 1", "1\n", "private_flag_canary"] {
        let output = command()
            .env("HAGENCY_FILE_TOOLS", flag)
            .output()
            .await
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private_flag_canary"));
    }
    let mut child = command().env("HAGENCY_FILE_TOOLS", "1").spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut frames = vec![
        init(json!(0)),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":"catalog","method":"tools/list"}),
    ];
    let mut invalid = vec![
        (
            "send_file",
            json!({"call_id":"file","path":"report.txt","room_id":"private_argument_canary"}),
        ),
        (
            "send_file",
            json!({"call_id":"file","path":"../private_argument_canary"}),
        ),
        (
            "send_file",
            json!({"call_id":"file","path":"report.txt","filename":"文".repeat(86)}),
        ),
        (
            "send_file",
            json!({"call_id":"file","path":"report.txt","caption":"文".repeat(334)}),
        ),
        (
            "send_file",
            json!({"call_id":"f".repeat(129),"path":"report.txt"}),
        ),
        (
            "send_file",
            json!({"call_id":"file","path":vec!["part";33].join("/")}),
        ),
        (
            "send_file",
            json!({"call_id":"file","path":"p".repeat(4097)}),
        ),
        (
            "get_file_delivery",
            json!({"delivery_id":"file_original","path":"private_argument_canary"}),
        ),
        (
            "get_file_delivery",
            json!({"delivery_id":"../private_argument_canary"}),
        ),
    ];
    for field in [
        "root",
        "url",
        "owner",
        "key",
        "descriptor",
        "mime_type",
        "capability",
        "verified",
        "sdk",
        "thread_root",
    ] {
        let mut args = json!({"call_id":"file","path":"report.txt"});
        args[field] = "private_argument_canary".into();
        invalid.push(("send_file", args));
    }
    let invalid_count = invalid.len();
    for (id, (name, args)) in invalid.into_iter().enumerate() {
        frames.push(json!({"jsonrpc":"2.0","id":id+1,"method":"tools/call","params":{"name":name,"arguments":args}}));
    }
    let writer = tokio::spawn(async move {
        for frame in frames {
            let mut bytes = serde_json::to_vec(&frame).unwrap();
            assert!(bytes.len() <= FRAME_LIMIT);
            bytes.push(b'\n');
            input.write_all(&bytes).await.unwrap();
        }
    });
    let output = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    writer.await.unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let output = String::from_utf8(output.stdout).unwrap();
    assert!(!output.contains("private_argument_canary"));
    let replies: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(replies.len(), 2 + invalid_count);
    assert_eq!(replies[1]["id"], "catalog");
    let tools = replies[1]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 22);
    let send = tools
        .iter()
        .find(|tool| tool["name"] == "send_file")
        .unwrap();
    assert_eq!(send["inputSchema"]["required"], json!(["call_id", "path"]));
    assert_eq!(send["inputSchema"]["additionalProperties"], false);
    assert_eq!(
        send["inputSchema"]["properties"].as_object().unwrap().len(),
        4
    );
    let inspect = tools
        .iter()
        .find(|tool| tool["name"] == "get_file_delivery")
        .unwrap();
    assert_eq!(inspect["inputSchema"]["required"], json!(["delivery_id"]));
    assert_eq!(inspect["inputSchema"]["additionalProperties"], false);
    assert_eq!(
        inspect["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
    for (index, reply) in replies[2..].iter().enumerate() {
        assert_eq!(reply["id"], index + 1);
        assert_eq!(reply["result"]["isError"], true);
        assert!(reply["result"].get("structuredContent").is_none());
        assert!(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("invalid native command")
        );
    }
}
