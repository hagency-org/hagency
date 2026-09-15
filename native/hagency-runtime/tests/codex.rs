use hagency_runtime::codex::{
    Connection, Decoder, Error, Event, MAX_FRAME_BYTES, MAX_PENDING, MAX_REQUEST_MS,
    MAX_SERVER_IDS, Message, PARTIAL_FRAME_MS, Phase, RequestId, TurnScope, encode,
};
use serde_json::{Value, json};

fn line(value: Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    bytes
}

fn accept(connection: &mut Connection, value: Value, now: u64) -> Result<Event, Error> {
    let bytes = line(value);
    let (consumed, event) = connection.receive(&bytes, now)?;
    assert_eq!(consumed, bytes.len());
    event.ok_or(Error::Envelope)
}

fn init_response(id: RequestId) -> Value {
    json!({ "id": id, "result": {
        "userAgent": "codex-fixture/0.153.4", "platformFamily": "unix",
        "platformOs": "macos", "codexHome": "/fixture/codex"
    } })
}

fn ready() -> Connection {
    let mut connection = Connection::default();
    let (id, bytes) = connection.initialize("0.1.0", 0, 1000).unwrap();
    let request: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(request["params"]["clientInfo"]["name"], "hagency");
    assert!(request.get("jsonrpc").is_none());
    assert!(matches!(
        accept(&mut connection, init_response(id), 1),
        Ok(Event::Initialized { .. })
    ));
    assert_eq!(connection.phase(), Phase::AwaitingInitialized);
    let initialized: Value = serde_json::from_slice(&connection.initialized(2).unwrap()).unwrap();
    assert_eq!(
        initialized,
        json!({ "method": "initialized", "params": {} })
    );
    assert_eq!(connection.phase(), Phase::Ready);
    connection
}

#[test]
fn native_codex_frames_versioned_wire_vectors() {
    let vectors: Value = serde_json::from_str(include_str!("fixtures/codex-0.153.4.json")).unwrap();
    assert_eq!(vectors["codexVersion"], "0.153.4");
    for vector in vectors["messages"].as_array().unwrap() {
        let bytes = line(vector["message"].clone());
        let (_, message) = Decoder::default().feed(&bytes, 0).unwrap();
        let message = message.unwrap();
        let kind = match &message {
            Message::Request { .. } => "request",
            Message::Notification { .. } => "notification",
            Message::Response { .. } => "response",
            Message::Error { .. } => "error",
        };
        assert_eq!(kind, vector["kind"]);
        let encoded: Value = serde_json::from_slice(&encode(message).unwrap()).unwrap();
        assert_eq!(encoded, vector["message"]);
    }
}

#[test]
fn native_codex_frames_fragmented_and_coalesced() {
    let bytes =
        line(json!({ "method": "item/agentMessage/delta", "params": { "delta": "你好🦀\nnext" } }));
    for split in 0..bytes.len() {
        let mut decoder = Decoder::default();
        let (consumed, first) = decoder.feed(&bytes[..split], 0).unwrap();
        assert_eq!(consumed, split);
        assert!(first.is_none());
        assert_eq!(decoder.buffered_bytes(), split);
        let (consumed, second) = decoder.feed(&bytes[split..], 1).unwrap();
        assert_eq!(consumed, bytes.len() - split);
        assert!(
            matches!(second, Some(Message::Notification { params: Some(ref p), .. }) if p["delta"] == "你好🦀\nnext")
        );
        assert_eq!(decoder.buffered_bytes(), 0);
        assert_eq!(decoder.eof(), Ok(()));
        assert_eq!(decoder.feed(&bytes, 2).err(), Some(Error::Closed));
    }
    let mut decoder = Decoder::default();
    let mut joined = bytes.clone();
    joined.extend_from_slice(b"{\"id\":-9223372036854775808,\"result\":null}\r\n");
    joined.extend_from_slice(b"{\"id\":\"-9223372036854775808\",\"result\":false}\n");
    let (used, event) = decoder.feed(&joined, 0).unwrap();
    assert_eq!(used, bytes.len());
    assert!(matches!(event, Some(Message::Notification { .. })));
    let (next, event) = decoder.feed(&joined[used..], 0).unwrap();
    assert!(matches!(
        event,
        Some(Message::Response {
            id: RequestId::Number(i64::MIN),
            result: Value::Null
        })
    ));
    let (_, event) = decoder.feed(&joined[used + next..], 0).unwrap();
    assert!(matches!(
        event,
        Some(Message::Response {
            id: RequestId::String(_),
            result: Value::Bool(false)
        })
    ));
    let mut connection = ready();
    let valid = line(json!({ "method": "warning", "params": { "message": "fixture" } }));
    let mut mixed = valid.clone();
    mixed.extend_from_slice(b"private malformed suffix\n");
    let (consumed, event) = connection.receive(&mixed, 3).unwrap();
    assert_eq!(consumed, valid.len());
    assert!(matches!(event, Some(Event::Notification { .. })));
    assert_eq!(
        connection.receive(&mixed[consumed..], 3).err(),
        Some(Error::Envelope)
    );
    assert_eq!(connection.phase(), Phase::Closed);
}

#[test]
fn native_codex_frames_reject_ambiguous_and_oversized_data() {
    let malformed = [
        "\n",
        "[]\n",
        "true\n",
        "{\"id\":null,\"result\":{}}\n",
        "{\"id\":1.0,\"result\":{}}\n",
        "{\"id\":9223372036854775808,\"result\":{}}\n",
        "{\"id\":true,\"result\":{}}\n",
        "{\"id\":\"\",\"result\":{}}\n",
        "{\"id\":0,\"result\":null,\"error\":{\"code\":1,\"message\":\"x\"}}\n",
        "{\"id\":0,\"id\":1,\"result\":null}\n",
        "{\"method\":\"notice\",\"params\":{\"owner\":1,\"owner\":2}}\n",
        "{\"method\":\"notice\",\"params\":{\"x\":0,\"\\u0078\":1}}\n",
        "{\"id\":0,\"method\":\"notice\",\"result\":null}\n",
        "{\"id\":0}\n",
        "{\"method\":null}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":0,\"result\":null}\n",
        "{\"id\":0,\"error\":{\"code\":1,\"message\":\"x\",\"grant\":true}}\n",
        "{\"method\":\"notice\"} {\"method\":\"notice\"}\n",
        "{\"method\":\"notice\",\"params\":NaN}\n",
        "{\"id\":0,\"method\":\"notice\",\"trace\":{\"authority\":true}}\n",
    ];
    for input in malformed {
        let mut decoder = Decoder::default();
        assert!(
            decoder.feed(input.as_bytes(), 0).is_err(),
            "accepted {input}"
        );
        assert_eq!(decoder.buffered_bytes(), 0);
        assert_eq!(
            decoder.feed(b"{\"id\":1,\"result\":null}\n", 1).err(),
            Some(Error::Closed)
        );
    }
    let mut decoder = Decoder::default();
    assert_eq!(
        decoder
            .feed(b"{\"method\":\"notice\",\"params\":\"\xff\"}\n", 0)
            .err(),
        Some(Error::Envelope)
    );
    let too_deep = format!(
        "{{\"method\":\"n\",\"params\":{}{}}}\n",
        "[".repeat(65),
        "]".repeat(65)
    );
    assert!(Decoder::default().feed(too_deep.as_bytes(), 0).is_err());
    let prefix = b"{\"method\":\"n\",\"params\":\"";
    let mut exact = prefix.to_vec();
    exact.resize(MAX_FRAME_BYTES - 2, b'x');
    exact.extend_from_slice(b"\"}\n");
    assert_eq!(exact.len(), MAX_FRAME_BYTES + 1);
    assert!(Decoder::default().feed(&exact, 0).is_ok());
    exact.insert(prefix.len(), b'x');
    assert_eq!(
        Decoder::default().feed(&exact, 0).err(),
        Some(Error::Capacity)
    );
    let too_big = Message::Notification {
        method: "n".into(),
        params: Some(Value::String("x".repeat(MAX_FRAME_BYTES))),
    };
    assert_eq!(encode(too_big).err(), Some(Error::Capacity));
    let mut nested = Value::Null;
    for _ in 0..65 {
        nested = Value::Array(vec![nested]);
    }
    assert_eq!(
        encode(Message::Notification {
            method: "n".into(),
            params: Some(nested)
        })
        .err(),
        Some(Error::Capacity)
    );
    // Trace and primitive/null params are accepted by the generated envelope schema.
    assert!(Decoder::default().feed(b"{\"id\":\"a\",\"method\":\"n\",\"params\":null,\"trace\":{\"traceparent\":null}}\n", 0).is_ok());
}

#[test]
fn native_codex_correlation_handshake_and_concurrent_requests() {
    let mut connection = Connection::default();
    assert_eq!(
        connection.request("thread/start", json!({}), 0, 100).err(),
        Some(Error::State)
    );
    assert_eq!(connection.initialized(0).err(), Some(Error::State));
    let (init, _) = connection.initialize("0.1.0", 0, 100).unwrap();
    assert_eq!(
        connection.initialize("0.1.0", 0, 100).err(),
        Some(Error::State)
    );
    assert!(matches!(
        accept(
            &mut connection,
            json!({ "method": "configWarning", "params": { "summary": "fixture" }}),
            1
        ),
        Ok(Event::Notification { .. })
    ));
    assert_eq!(connection.pending_count(), 1);
    assert!(accept(&mut connection, init_response(init), 2).is_ok());
    assert_eq!(
        connection.request("thread/start", json!({}), 2, 100).err(),
        Some(Error::State)
    );
    connection.initialized(3).unwrap();
    let (a, _) = connection
        .request("thread/start", json!({}), 3, 100)
        .unwrap();
    let (b, _) = connection.request("model/list", json!({}), 3, 100).unwrap();
    assert!(
        matches!(accept(&mut connection, json!({ "id": b, "error": { "code": -32001, "message": "fixture", "data": { "retryAfter": 5 } } }), 4), Ok(Event::Response { method, result: Err(_), .. }) if method == "model/list")
    );
    assert!(
        matches!(accept(&mut connection, json!({ "id": a, "result": null }), 5), Ok(Event::Response { method, result: Ok(Value::Null), .. }) if method == "thread/start")
    );
    assert_eq!(connection.pending_count(), 0);
    // The RPC error did not trigger any automatic retry or close a healthy transport.
    assert_eq!(connection.phase(), Phase::Ready);
    assert_eq!(
        accept(&mut connection, json!({ "id": a, "result": null }), 6).err(),
        Some(Error::Identity)
    );
    assert_eq!(connection.phase(), Phase::Closed);
}

#[test]
fn native_codex_correlation_wrong_types_limits_and_invalid_initialize() {
    let mut connection = Connection::default();
    let (id, _) = connection.initialize("0.1", 0, 100).unwrap();
    assert_eq!(accept(&mut connection, json!({ "id": id, "error": { "code": -32000, "message": "private initialize failure" } }), 1).err(), Some(Error::State));
    assert_eq!(connection.phase(), Phase::Closed);
    assert_eq!(
        connection.initialize("0.1", 2, 100).err(),
        Some(Error::Closed)
    );
    for id in [json!("1"), json!(999), json!(-1)] {
        let mut connection = ready();
        connection.request("model/list", json!({}), 3, 100).unwrap();
        assert_eq!(
            accept(&mut connection, json!({ "id": id, "result": {} }), 4).err(),
            Some(Error::Identity)
        );
        assert_eq!(connection.phase(), Phase::Closed);
    }
    for result in [
        Value::Null,
        json!({}),
        json!({ "userAgent": "fixture", "platformFamily": "unix", "platformOs": "linux" }),
    ] {
        let mut connection = Connection::default();
        let (id, _) = connection.initialize("0.1", 0, 100).unwrap();
        assert_eq!(
            accept(&mut connection, json!({ "id": id, "result": result }), 1).err(),
            Some(Error::Envelope)
        );
    }
    let mut connection = ready();
    let mut ids = Vec::new();
    for _ in 0..MAX_PENDING {
        ids.push(
            connection
                .request("model/list", json!({}), 3, 100)
                .unwrap()
                .0,
        );
    }
    assert_eq!(
        connection.request("model/list", json!({}), 3, 100).err(),
        Some(Error::Capacity)
    );
    assert_eq!(connection.pending_count(), MAX_PENDING);
    assert!(accept(&mut connection, json!({ "id": ids[0], "result": {} }), 4).is_ok());
    let next = connection
        .request("model/list", json!({}), 4, 100)
        .unwrap()
        .0;
    assert!(!ids.contains(&next));
    assert_eq!(
        connection
            .request("turn/interrupt", json!({ "threadId": "forged" }), 4, 100)
            .err(),
        Some(Error::State)
    );
}

#[test]
fn native_codex_server_requests_tombstones_and_exact_rejections() {
    let mut connection = ready();
    let (host_id, _) = connection.request("model/list", json!({}), 3, 100).unwrap();
    assert_eq!(host_id, RequestId::Number(1));
    for id in [json!(1), json!("1")] {
        assert!(matches!(
            accept(
                &mut connection,
                json!({ "id": id, "method": "item/commandExecution/requestApproval", "params": { "threadId": "t", "turnId": "u", "itemId": "i", "command": "display only" } }),
                3
            ),
            Ok(Event::ServerRequest { .. })
        ));
    }
    assert_eq!(connection.pending_server_count(), 2);
    // A peer request may share the exact ID of a host request. Direction and
    // envelope shape keep the two independent until their respective replies.
    assert!(matches!(
        accept(&mut connection, json!({ "id": host_id, "result": {} }), 4),
        Ok(Event::Response { .. })
    ));
    assert_eq!(connection.pending_server_count(), 2);
    let rejection: Value = serde_json::from_slice(
        &connection
            .reject_server_request(&RequestId::Number(1), 4)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(rejection["id"], 1);
    assert_eq!(rejection["error"]["code"], -32601);
    assert!(rejection.get("result").is_none());
    assert_eq!(
        connection
            .reject_server_request(&RequestId::Number(1), 4)
            .err(),
        Some(Error::Identity)
    );
    assert!(accept(&mut connection, json!({ "method": "serverRequest/resolved", "params": { "threadId": "t", "requestId": "1" } }), 4).is_ok());
    assert_eq!(connection.pending_server_count(), 0);
    assert_eq!(
        connection
            .reject_server_request(&RequestId::String("1".into()), 5)
            .err(),
        Some(Error::Identity)
    );
    assert_eq!(
        accept(
            &mut connection,
            json!({ "id": "1", "method": "item/fileChange/requestApproval", "params": {} }),
            5
        )
        .err(),
        Some(Error::Identity)
    );
    let mut connection = ready();
    assert!(accept(&mut connection, json!({ "method": "serverRequest/resolved", "params": { "threadId": "t", "requestId": 99 } }), 3).is_ok());
    assert_eq!(
        accept(
            &mut connection,
            json!({ "id": 99, "method": "newApproval", "params": {} }),
            4
        )
        .err(),
        Some(Error::Identity)
    );
    let mut connection = ready();
    accept(&mut connection, json!({ "id": 10, "method": "item/fileChange/requestApproval", "params": { "threadId": "t" } }), 3).ok().unwrap();
    assert_eq!(accept(&mut connection, json!({ "method": "serverRequest/resolved", "params": { "threadId": "other", "requestId": 10 } }), 4).err(), Some(Error::Identity));
}

#[test]
fn native_codex_server_requests_capacity_and_timeout() {
    let mut connection = ready();
    for id in 0..MAX_PENDING {
        accept(
            &mut connection,
            json!({ "id": id, "method": "request", "params": {} }),
            3,
        )
        .ok()
        .unwrap();
    }
    assert_eq!(
        accept(
            &mut connection,
            json!({ "id": 10000, "method": "request", "params": {} }),
            3
        )
        .err(),
        Some(Error::Capacity)
    );
    assert_eq!(connection.phase(), Phase::Closed);
    let mut connection = ready();
    for id in 0..MAX_SERVER_IDS {
        accept(
            &mut connection,
            json!({ "id": id, "method": "request", "params": {} }),
            3,
        )
        .ok()
        .unwrap();
        connection
            .reject_server_request(&RequestId::Number(id as i64), 3)
            .unwrap();
    }
    assert_eq!(connection.pending_server_count(), 0);
    assert_eq!(
        accept(
            &mut connection,
            json!({ "id": 10000, "method": "request", "params": {} }),
            3
        )
        .err(),
        Some(Error::Capacity)
    );
    let mut connection = ready();
    accept(&mut connection, json!({ "id": 10, "method": "request" }), 3)
        .ok()
        .unwrap();
    assert_eq!(connection.tick(3 + MAX_REQUEST_MS), Err(Error::Timeout));
    assert_eq!(
        connection
            .reject_server_request(&RequestId::Number(10), 3 + MAX_REQUEST_MS)
            .err(),
        Some(Error::Closed)
    );
}

#[test]
fn native_codex_failure_and_interrupt_acceptance_is_not_completion() {
    let mut connection = ready();
    let scope = TurnScope::new("thread-fixture".into(), "turn-fixture".into()).unwrap();
    let (id, bytes) = connection.interrupt(scope.clone(), 3, 100).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap(),
        json!({ "id": id, "method": "turn/interrupt", "params": { "threadId": scope.thread_id(), "turnId": scope.turn_id() } })
    );
    assert_eq!(
        connection.interrupt(scope.clone(), 3, 100).err(),
        Some(Error::State)
    );
    assert!(
        matches!(accept(&mut connection, json!({ "id": id, "result": {} }), 4), Ok(Event::InterruptAcknowledged { scope: actual }) if actual == scope)
    );
    // Notification order can race with cancellation. Both remain observations;
    // neither implies process termination, lease release or canonical task done.
    for status in ["completed", "interrupted", "failed"] {
        assert!(matches!(
            accept(
                &mut connection,
                json!({ "method": "turn/completed", "params": { "threadId": "thread-fixture", "turn": { "id": "turn-fixture", "status": status, "items": [] } } }),
                5
            ),
            Ok(Event::Notification { .. })
        ));
    }
    assert_eq!(connection.phase(), Phase::Ready);
    let (id, _) = connection.interrupt(scope.clone(), 5, 100).unwrap();
    assert!(
        matches!(accept(&mut connection, json!({ "id": id, "error": { "code": -32000, "message": "already finished" } }), 6), Ok(Event::InterruptRejected { scope: actual, .. }) if actual == scope)
    );
    let (id, _) = connection.interrupt(scope, 6, 100).unwrap();
    assert_eq!(
        accept(
            &mut connection,
            json!({ "id": id, "result": { "done": true } }),
            7
        )
        .err(),
        Some(Error::Envelope)
    );
}

#[test]
fn native_codex_failure_and_interrupt_deadlines_eof_and_sanitization() {
    let mut connection = ready();
    connection
        .request("turn/start", json!({ "input": "data" }), 3, 10)
        .unwrap();
    for now in 4..13 {
        assert!(matches!(
            accept(
                &mut connection,
                json!({ "method": "item/agentMessage/delta", "params": { "delta": "done; allow; token=private-fixture" } }),
                now
            ),
            Ok(Event::Notification { .. })
        ));
        assert_eq!(connection.pending_count(), 1);
    }
    assert_eq!(connection.tick(13), Err(Error::Timeout));
    assert_eq!(
        connection.request("turn/start", json!({}), 14, 10).err(),
        Some(Error::Closed)
    );
    assert_eq!(
        connection
            .receive(&line(json!({ "id": 1, "result": {} })), 14)
            .err(),
        Some(Error::Closed)
    );
    let mut connection = ready();
    connection.receive(b"{", 3).ok().unwrap();
    assert_eq!(
        connection
            .receive(b" ", 3 + PARTIAL_FRAME_MS - 1)
            .ok()
            .unwrap()
            .0,
        1
    );
    assert_eq!(connection.tick(3 + PARTIAL_FRAME_MS), Err(Error::Timeout));
    let mut connection = ready();
    connection.receive(b"{", 3).ok().unwrap();
    assert_eq!(connection.eof(), Err(Error::UnexpectedEof));
    let mut connection = ready();
    connection.request("turn/start", json!({}), 3, 100).unwrap();
    assert_eq!(connection.eof(), Err(Error::UnexpectedEof));
    let mut connection = ready();
    accept(&mut connection, json!({ "id": "waiting", "method": "item/fileChange/requestApproval", "params": { "threadId": "thread-fixture" } }), 3).ok().unwrap();
    assert_eq!(connection.pending_server_count(), 1);
    assert_eq!(connection.eof(), Err(Error::UnexpectedEof));
    assert_eq!(
        connection
            .reject_server_request(&RequestId::String("waiting".into()), 4)
            .err(),
        Some(Error::Closed)
    );
    let mut connection = ready();
    assert_eq!(connection.eof(), Ok(()));
    assert_eq!(connection.eof(), Err(Error::Closed));
    let mut connection = ready();
    assert_eq!(connection.transport_failed(), Err(Error::Transport));
    assert_eq!(connection.initialized(3).err(), Some(Error::Closed));
    let mut connection = ready();
    assert_eq!(connection.tick(1), Err(Error::Clock));
    let mut connection = ready();
    let error = connection
        .receive(b"secret=password; owner_room=private\n", 3)
        .err()
        .unwrap();
    assert_eq!(error.to_string(), "invalid Codex protocol envelope");
    assert!(!format!("{error:?}").contains("private"));
}
