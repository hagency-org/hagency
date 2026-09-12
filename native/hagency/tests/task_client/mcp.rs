use super::*;
use hagency::mcp::Session;
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::Value;

async fn handle(s: &mut Session, v: Value) -> Option<Value> {
    s.handle(&serde_json::to_vec(&v).unwrap()).await.unwrap()
}
async fn initialized(s: &mut Session) {
    let v = handle(s, json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).await.unwrap();
    assert_eq!(v["result"]["capabilities"], json!({"tools":{}}));
    assert!(
        handle(
            s,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .await
        .is_none()
    );
}
async fn call(s: &mut Session, id: u64, name: &str, args: Value) -> Value {
    handle(s,json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}})).await.unwrap()["result"].clone()
}
#[tokio::test]
async fn native_mcp_task_lifecycle() {
    let f = Fixture::new(false).await;
    let mut s = Session::new(f.context());
    initialized(&mut s).await;
    let read = call(&mut s, 1, "get_task", json!({"id":"task"})).await;
    assert_eq!(read["structuredContent"]["task"]["status"], "in_progress");
    let heartbeat = call(
        &mut s,
        2,
        "update_task_execution",
        json!({"id":"task","call_id":"beat","heartbeat":true}),
    )
    .await;
    assert_eq!(heartbeat["isError"], false);
    let retry = call(
        &mut s,
        3,
        "update_task_execution",
        json!({"id":"task","call_id":"beat","heartbeat":true}),
    )
    .await;
    assert_eq!(retry["structuredContent"]["replayed"], true);
    assert_eq!(
        retry["structuredContent"]["task"],
        heartbeat["structuredContent"]["task"]
    );
    let mut reconnected = Session::new(f.context());
    initialized(&mut reconnected).await;
    let replay = call(
        &mut reconnected,
        1,
        "update_task_execution",
        json!({"id":"task","call_id":"beat","heartbeat":true}),
    )
    .await;
    assert_eq!(replay["structuredContent"]["replayed"], true);
    assert_eq!(
        replay["structuredContent"]["task"],
        heartbeat["structuredContent"]["task"]
    );
    assert_eq!(
        call(
            &mut reconnected,
            2,
            "comment_task",
            json!({"id":"task","call_id":"beat","text":"changed after reconnect"})
        )
        .await["isError"],
        true
    );
    for (id, name, args) in [
        (
            4,
            "comment_task",
            json!({"id":"task","call_id":"beat","text":"conflicts"}),
        ),
        (
            5,
            "comment_task",
            json!({"id":"other","call_id":"cross","text":"wrong task"}),
        ),
        (6, "comment_task", json!({"id":"task","text":"no call id"})),
        (
            7,
            "comment_task",
            json!({"id":"task","call_id":"inject","text":"body","action":"accept"}),
        ),
        (
            8,
            "get_task",
            json!({"id":"task","capability":{"secret":"untrusted"}}),
        ),
    ] {
        assert_eq!(call(&mut s, id, name, args).await["isError"], true);
    }
    let blocked = call(&mut s,9,"transition_task",json!({"id":"task","call_id":"wait","status":"blocked","waiting_reason":"review","waiting_until":"2030-01-01T00:00:00Z"})).await;
    assert_eq!(blocked["structuredContent"]["task"]["status"], "blocked");
    assert_eq!(
        call(
            &mut s,
            10,
            "transition_task",
            json!({"id":"task","call_id":"resume","status":"in_progress"})
        )
        .await["isError"],
        false
    );
    assert_eq!(
        call(
            &mut s,
            11,
            "comment_task",
            json!({"id":"task","call_id":"comment","text":"检查通过"})
        )
        .await["isError"],
        false
    );
    let done = call(
        &mut s,
        12,
        "transition_task",
        json!({"id":"task","call_id":"done","status":"done"}),
    )
    .await;
    assert_eq!(done["structuredContent"]["task"]["status"], "done");
    let replay = call(
        &mut s,
        13,
        "transition_task",
        json!({"id":"task","call_id":"done","status":"done"}),
    )
    .await;
    assert_eq!(replay["structuredContent"]["replayed"], true);
    assert_eq!(
        call(
            &mut s,
            14,
            "update_task_execution",
            json!({"id":"task","call_id":"after","heartbeat":true})
        )
        .await["isError"],
        true
    );
    let result = f
        .domain
        .runner_command(f.cap.clone(), RunnerCommand::Task { id: "task".into() })
        .await
        .unwrap();
    assert_eq!(result["status"], "done");
    f.close().await;

    for parked in [false, true] {
        let f = Fixture::new(parked).await;
        let mut cap = f.cap.clone();
        if !parked {
            cap.secret = "f".repeat(64);
        }
        let mut s = Session::new(Context::new(f.address, cap, "task".into()).unwrap());
        initialized(&mut s).await;
        let result = call(
            &mut s,
            1,
            "update_task_execution",
            json!({"id":"task","call_id":"refused","heartbeat":true}),
        )
        .await;
        assert_eq!(result["isError"], true);
        assert!(!result.to_string().contains(&f.cap.secret));
        f.close().await;
    }
}

#[tokio::test]
async fn native_mcp_sdk() {
    let f = Fixture::new(false).await;
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_hagency"));
    command
        .arg("mcp")
        .env_clear()
        .env("HAGENCY_RUNNER_API_ADDR", f.address.to_string())
        .env(
            "HAGENCY_RUNNER_CAPABILITY",
            serde_json::to_string(&f.cap).unwrap(),
        )
        .env("HAGENCY_TASK_ID", "task")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    let mut child = command.spawn().unwrap();
    let transport = (child.stdout.take().unwrap(), child.stdin.take().unwrap());
    tokio::time::timeout(Duration::from_secs(15), async {
        let client = ().serve(transport).await.unwrap();
        let tools = client.list_all_tools().await.unwrap();
        assert_eq!(tools.len(), 20);
        assert!(tools.iter().all(|v| !v.name.contains("approve")));
        let read = client
            .call_tool(
                CallToolRequestParams::new("get_task")
                    .with_arguments(json!({"id":"task"}).as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        assert_eq!(read.is_error, Some(false));
        assert_eq!(read.structured_content.unwrap()["task"]["id"], "task");
        let beat = client
            .call_tool(
                CallToolRequestParams::new("update_task_execution").with_arguments(
                    json!({"id":"task","call_id":"sdk","heartbeat":true})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .unwrap();
        assert_eq!(beat.is_error, Some(false));
        client.cancel().await.unwrap();
    })
    .await
    .unwrap();
    let output = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "native helper failed: {:?}",
        output.stderr
    );
    assert!(output.stderr.is_empty());
    f.close().await;
}

fn file_helper(
    address: SocketAddr,
    cap: &RunnerCapability,
    receive: bool,
) -> tokio::process::Child {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_hagency"));
    command
        .arg("mcp")
        .env_clear()
        .env("HAGENCY_RUNNER_API_ADDR", address.to_string())
        .env(
            "HAGENCY_RUNNER_CAPABILITY",
            serde_json::to_string(cap).unwrap(),
        )
        .env("HAGENCY_TASK_ID", "task")
        .env(
            if receive {
                "HAGENCY_RECEIVE_FILE_TOOLS"
            } else {
                "HAGENCY_FILE_TOOLS"
            },
            "1",
        )
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    command.spawn().unwrap()
}

async fn file_call(
    address: SocketAddr,
    cap: &RunnerCapability,
    name: &str,
    args: Value,
) -> rmcp::model::CallToolResult {
    let mut child = file_helper(
        address,
        cap,
        matches!(name, "receive_file" | "list_received_files"),
    );
    let transport = (child.stdout.take().unwrap(), child.stdin.take().unwrap());
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let client = ().serve(transport).await.unwrap();
        let result = client
            .call_tool(
                CallToolRequestParams::new(name.to_owned())
                    .with_arguments(args.as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        client.cancel().await.unwrap();
        result
    })
    .await
    .unwrap();
    let output = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    result
}

async fn file_exchange(
    name: &str,
    args: Value,
    response: String,
) -> (rmcp::model::CallToolResult, String) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            let n = stream.read(&mut buffer).await.unwrap();
            assert_ne!(n, 0);
            request.extend_from_slice(&buffer[..n]);
            assert!(request.len() <= 32 * 1024);
            if let Some(end) = request.windows(4).position(|v| v == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        let _ = stream.write_all(response.as_bytes()).await;
        drop(stream);
        // A refused, malformed or unknown answer must not reconnect or retry.
        assert!(
            tokio::time::timeout(Duration::from_millis(30), listener.accept())
                .await
                .is_err()
        );
        String::from_utf8(request).unwrap()
    });
    let result = file_call(address, &capability(), name, args).await;
    (result, server.await.unwrap())
}

fn file_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn native_mcp_receive_transport() {
    // This actual MCP child and counted HTTP peer validate the adapter only.
    // Response fixtures do not prove SDK verification or a physical Ready file.
    let view = json!({"event_id":"$original","filename":"报告.txt","mime_type":"text/plain",
        "size":4,"sha256":"a".repeat(64),"path":format!(".hagency-received-{}.bin", "b".repeat(32)),"replayed":false});
    let item = json!({"event_id":"$original","sequence":3,"metadata":{"filename":"报告.txt","mime_type":"text/plain","declared_size":4}});
    let page = json!({"items":[item],"next":3});
    for (name, args, value, line) in [
        (
            "receive_file",
            json!({"event_id":"$original"}),
            view.clone(),
            "POST /api/native/v1/runner/received-files HTTP/1.1",
        ),
        (
            "list_received_files",
            json!({"after":2,"limit":1}),
            page.clone(),
            "GET /api/native/v1/runner/received-files?after=2&limit=1 HTTP/1.1",
        ),
        (
            "list_received_files",
            json!({}),
            json!({"items":[],"next":null}),
            "GET /api/native/v1/runner/received-files?after=0&limit=16 HTTP/1.1",
        ),
    ] {
        let (result, request) =
            file_exchange(name, args.clone(), file_response(&value.to_string())).await;
        assert_eq!(result.is_error, Some(false));
        assert_eq!(result.structured_content.unwrap(), value);
        assert!(request.starts_with(line));
        assert!(
            request.contains(&format!("Authorization: Bearer {}", capability().secret))
                || request.contains(&format!("authorization: Bearer {}", capability().secret))
        );
        assert!(request.contains("x-hagency-dispatch: dispatch"));
        assert!(request.contains("x-hagency-runner: runner"));
        assert!(request.contains("x-hagency-fence: 1"));
        let (_, body) = request.split_once("\r\n\r\n").unwrap();
        if name == "receive_file" {
            assert_eq!(serde_json::from_str::<Value>(body).unwrap(), args);
        } else {
            assert!(body.is_empty());
        }
    }
    let mut invalid = Vec::new();
    for (field, value) in [
        ("event_id", json!("$other")),
        ("path", json!("../private_response_canary")),
        ("path", json!("/private_response_canary")),
        ("path", json!(".hagency-received-ABC.bin")),
        (
            "path",
            json!(format!(".hagency-received-{}.bin/child", "a".repeat(32))),
        ),
        ("filename", json!("../private_response_canary")),
        ("sha256", json!("A".repeat(64))),
        ("size", json!(4 * 1024 * 1024 + 1)),
        ("size", json!(-1)),
        ("replayed", json!("true")),
    ] {
        let mut bad = view.clone();
        bad[field] = value;
        invalid.push(bad.to_string());
    }
    for field in [
        "room_id",
        "thread_root",
        "mxc",
        "url",
        "descriptor",
        "key",
        "capability",
        "root",
    ] {
        let mut bad = view.clone();
        bad[field] = "private_response_canary".into();
        invalid.push(bad.to_string());
    }
    invalid.push(format!(
        "{{\"event_id\":\"$original\",{}",
        &view.to_string()[1..]
    ));
    invalid.push(format!("{view}{}", " ".repeat(4097)));
    let mut responses: Vec<_> = invalid.iter().map(|body| file_response(body)).collect();
    responses.extend([
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:1/private_response_canary\r\nContent-Length: 0\r\n\r\n".into(),
        "HTTP/1.1 503 Unavailable\r\nContent-Length: 23\r\n\r\nprivate_response_canary".into(),
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 200\r\n\r\n{".into(),
    ]);
    for response in responses {
        let (result, _) =
            file_exchange("receive_file", json!({"event_id":"$original"}), response).await;
        assert_eq!(result.is_error, Some(true));
        assert!(result.structured_content.is_none());
        let encoded = serde_json::to_string(&result).unwrap();
        assert!(!encoded.contains("private_response_canary"));
        assert!(!encoded.contains(&capability().secret));
    }
    let mut invalid_pages = vec![
        json!({"items":[],"next":3}),
        json!({"items":[item.clone()],"next":4}),
        json!({"items":[item.clone(),item.clone()],"next":3}),
    ];
    for (field, value) in [
        ("sequence", json!(2)),
        ("sequence", json!(9007199254740992_u64)),
        ("event_id", json!("bad")),
        ("key", json!("private_response_canary")),
    ] {
        let mut bad = item.clone();
        bad[field] = value;
        invalid_pages.push(json!({"items":[bad],"next":null}));
    }
    let mut later = item.clone();
    later["sequence"] = 4.into();
    invalid_pages.push(json!({"items":[item,later],"next":4})); // Duplicate event with distinct sequence.
    for page in invalid_pages {
        let (result, _) = file_exchange(
            "list_received_files",
            json!({"after":2,"limit":2}),
            file_response(&page.to_string()),
        )
        .await;
        assert_eq!(result.is_error, Some(true));
        assert!(result.structured_content.is_none());
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("private_response_canary")
        );
    }
    let (result, _) = file_exchange(
        "list_received_files",
        json!({}),
        file_response(&format!("{page}{}", " ".repeat(16 * 1024))),
    )
    .await;
    assert_eq!(result.is_error, Some(true));
    let f = Fixture::new(false).await;
    for (name, args) in [
        ("receive_file", json!({"event_id":"$original"})),
        ("list_received_files", json!({})),
    ] {
        let result = file_call(f.address, &f.cap, name, args).await;
        assert_eq!(result.is_error, Some(true));
        assert!(result.structured_content.is_none());
    }
    f.close().await;
}

#[tokio::test]
async fn native_mcp_file_transport() {
    // This peer proves closed transport and response handling only. It supplies
    // no FileService, source authority, SDK acceptance or delivery evidence.
    let delivery_id = format!("file_{}", "a".repeat(32));
    let view = json!({"delivery_id":delivery_id,"filename":"report.txt","status":"queued","replayed":false});
    for (name, args, line) in [
        (
            "send_file",
            json!({"call_id":"stable_file","path":"report.txt","caption":"private_caption_canary"}),
            "POST /api/native/v1/runner/file-deliveries HTTP/1.1".to_owned(),
        ),
        (
            "get_file_delivery",
            json!({"delivery_id":delivery_id}),
            format!("GET /api/native/v1/runner/file-deliveries/{delivery_id} HTTP/1.1"),
        ),
    ] {
        let (result, request) = file_exchange(name, args, file_response(&view.to_string())).await;
        assert_eq!(result.is_error, Some(false));
        let structured = result.structured_content.unwrap();
        assert_eq!(structured["delivery_id"], delivery_id);
        assert_eq!(structured["status"], "queued");
        assert_eq!(structured["replayed"], false);
        assert!(request.starts_with(&line));
        assert!(request.contains("x-hagency-dispatch: dispatch"));
        assert!(request.contains("x-hagency-runner: runner"));
        assert!(request.contains("x-hagency-fence: 1"));
        assert!(!structured.to_string().contains("private_caption_canary"));
        if name == "send_file" {
            let (_, body) = request.split_once("\r\n\r\n").unwrap();
            let body: Value = serde_json::from_str(body).unwrap();
            assert_eq!(body["call_id"], "stable_file");
            assert_eq!(body["path"], "report.txt");
            assert_eq!(body["caption"], "private_caption_canary");
            assert!(body.get("capability").is_none());
        } else {
            assert!(request.ends_with("\r\n\r\n"));
        }
    }
    let mut unsafe_bodies = vec![
        format!("{{\"delivery_id\":\"{delivery_id}\",\"delivery_id\":\"other\",\"filename\":\"report.txt\",\"status\":\"queued\",\"replayed\":false}}"),
        json!({"delivery_id":delivery_id,"filename":"../private_response_canary","status":"queued","replayed":false}).to_string(),
        json!({"delivery_id":delivery_id,"filename":"report.txt","status":"delivered","replayed":false,"error_code":"outcome_unknown"}).to_string(),
        json!({"delivery_id":delivery_id,"filename":"report.txt","status":"failed","replayed":false,"error_code":"private_response_canary"}).to_string(),
        json!({"delivery_id":delivery_id,"filename":"report.txt","status":"failed","replayed":false,"error_code":null}).to_string(),
        json!({"delivery_id":delivery_id,"filename":"report.txt","status":"outcome_unknown","replayed":false}).to_string(),
        json!({"delivery_id":delivery_id,"filename":"report.txt","status":"upload_accepted","replayed":false}).to_string(),
        format!("{}{}", view, " ".repeat(4097)),
    ];
    for field in [
        "path",
        "caption",
        "room_id",
        "thread_root",
        "mxc",
        "key",
        "descriptor",
        "sha256",
    ] {
        let mut unsafe_view = view.clone();
        unsafe_view[field] = "private_response_canary".into();
        unsafe_bodies.push(unsafe_view.to_string());
    }
    let mut responses: Vec<String> = unsafe_bodies
        .iter()
        .map(|body| file_response(body))
        .collect();
    responses.extend([
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:1/private_response_canary\r\nContent-Length: 0\r\n\r\n".into(),
        "HTTP/1.1 503 Unavailable\r\nContent-Length: 23\r\n\r\nprivate_response_canary".into(),
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 200\r\n\r\n{".into(),
    ]);
    for response in responses {
        let (result, _) = file_exchange(
            "send_file",
            json!({"call_id":"stable_file","path":"report.txt"}),
            response,
        )
        .await;
        assert_eq!(result.is_error, Some(true));
        assert!(result.structured_content.is_none());
        let result = serde_json::to_string(&result).unwrap();
        assert!(!result.contains("private_response_canary"));
        assert!(!result.contains(&capability().secret));
    }
    let mut other = view;
    other["delivery_id"] = "file_other".into();
    let (result, _) = file_exchange(
        "get_file_delivery",
        json!({"delivery_id":delivery_id}),
        file_response(&other.to_string()),
    )
    .await;
    assert_eq!(result.is_error, Some(true));
    assert!(result.structured_content.is_none());

    // A forged presentation marker and a real current task credential do not
    // create the absent service or original retained workspace admission.
    let f = Fixture::new(false).await;
    for (name, args) in [
        ("send_file", json!({"call_id":"file","path":"report.txt"})),
        ("get_file_delivery", json!({"delivery_id":delivery_id})),
    ] {
        let result = file_call(f.address, &f.cap, name, args).await;
        assert_eq!(result.is_error, Some(true));
        assert!(result.structured_content.is_none());
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains(&f.cap.secret)
        );
    }
    f.close().await;
}
