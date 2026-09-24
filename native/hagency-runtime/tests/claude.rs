use hagency_runtime::claude::{
    self, ControlOutcome, Decoder, Error, EventKind, MAX_FRAME_BYTES, MAX_TEXT_BYTES, Message,
    PARTIAL_FRAME_MS,
};
use serde_json::{Value, json};
fn line(value: Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    bytes
}
fn parse(value: Value) -> Message {
    Decoder::default().feed(&line(value), 1).unwrap().1.unwrap()
}
fn malformed(bytes: &[u8], error: Error) {
    let mut decoder = Decoder::default();
    assert!(matches!(decoder.feed(bytes,1),Err(actual) if actual==error));
    assert!(matches!(decoder.feed(b"{}\n", 2), Err(Error::Closed)));
    assert_eq!(decoder.buffered_bytes(), 0);
}
#[test]
fn native_claude_stream_framing() {
    let original = line(
        json!({"type":"assistant","session_id":"session-1","message":{"content":[{"type":"text","text":"测试"}]}}),
    );
    let mut decoder = Decoder::default();
    for (index, byte) in original.iter().enumerate() {
        let (count, message) = decoder.feed(&[*byte], index as u64).unwrap();
        assert_eq!(count, 1);
        assert_eq!(message.is_some(), index == original.len() - 1);
    }
    assert_eq!(decoder.buffered_bytes(), 0);
    decoder.eof().unwrap();
    assert!(matches!(decoder.feed(&original, 1000), Err(Error::Closed)));
    let mut crlf = original[..original.len() - 1].to_vec();
    crlf.extend(b"\r\n");
    crlf.extend(&original);
    let mut decoder = Decoder::default();
    let (count, message) = decoder.feed(&crlf, 1).unwrap();
    assert_eq!(count, original.len() + 1);
    assert!(message.is_some());
    assert!(decoder.feed(&crlf[count..], 1).unwrap().1.is_some());
    malformed(
        b"{\"type\":\"result\",\"type\":\"assistant\"}\n",
        Error::Envelope,
    );
    malformed(
        b"{\"type\":\"result\",\"\\u0074ype\":\"assistant\"}\n",
        Error::Envelope,
    );
    malformed(
        b"{\"type\":\"assistant\",\"session_id\":\"s\",\"message\":{\"x\":1,\"x\":2}}\n",
        Error::Envelope,
    );
    malformed(b"\xff\n", Error::Envelope);
    malformed(b"\n", Error::Envelope);
    malformed(&vec![b' '; MAX_FRAME_BYTES + 1], Error::Capacity);
    let mut nested = json!(null);
    for _ in 0..66 {
        nested = json!([nested]);
    }
    malformed(
        &line(json!({"type":"assistant","session_id":"s","message":{"deep":nested}})),
        Error::Capacity,
    );
    let mut exact = original[..original.len() - 1].to_vec();
    exact.resize(MAX_FRAME_BYTES, b' ');
    exact.push(b'\n');
    assert!(Decoder::default().feed(&exact, 1).unwrap().1.is_some());
    exact.insert(MAX_FRAME_BYTES, b'\r');
    malformed(&exact, Error::Capacity);
    let mut decoder = Decoder::default();
    decoder.feed(b"{", 1).unwrap();
    decoder.check_deadline(PARTIAL_FRAME_MS).unwrap();
    assert_eq!(
        decoder.check_deadline(PARTIAL_FRAME_MS + 1),
        Err(Error::Timeout)
    );
    assert_eq!(
        decoder.check_deadline(PARTIAL_FRAME_MS + 2),
        Err(Error::Closed)
    );
    let mut decoder = Decoder::default();
    decoder.feed(b"{", 9).unwrap();
    assert_eq!(decoder.check_deadline(8), Err(Error::Clock));
    let mut decoder = Decoder::default();
    decoder.feed(b"{", 1).unwrap();
    assert_eq!(decoder.eof(), Err(Error::UnexpectedEof));
    assert_eq!(decoder.eof(), Err(Error::Closed));
}
#[test]
fn native_claude_stream_envelopes() {
    match parse(
        json!({"type":"control_request","request_id":"request-1","request":{"subtype":"can_use_tool","tool_name":"Bash","tool_use_id":"tool-1","input":{"command":"synthetic only"}}}),
    ) {
        Message::Permission {
            request_id,
            tool_name,
            tool_use_id,
            input,
        } => {
            assert_eq!(request_id, "request-1");
            assert_eq!(tool_name, "Bash");
            assert_eq!(tool_use_id.as_deref(), Some("tool-1"));
            assert_eq!(input["command"], "synthetic only");
        }
        _ => panic!("permission shape"),
    }
    match parse(
        json!({"type":"control_response","response":{"subtype":"success","request_id":"initialize-1","response":{"commands":[]}}}),
    ) {
        Message::ControlResponse {
            request_id,
            outcome: ControlOutcome::Success(body),
        } => {
            assert_eq!(request_id, "initialize-1");
            assert_eq!(body["commands"], json!([]));
        }
        _ => panic!("response shape"),
    }
    assert!(matches!(
        parse(
            json!({"type":"control_response","response":{"subtype":"error","request_id":"i","error":"private upstream text"}})
        ),
        Message::ControlResponse {
            outcome: ControlOutcome::Refused,
            ..
        }
    ));
    assert!(
        matches!(parse(json!({"type":"control_cancel_request","request_id":"request-1"})),Message::ControlCancel {request_id} if request_id=="request-1")
    );
    for (name, kind, extra) in [
        (
            "system",
            EventKind::System,
            json!({"subtype":"init","permissionMode":"auto"}),
        ),
        (
            "assistant",
            EventKind::Assistant,
            json!({"message":{"role":"assistant","content":[]}}),
        ),
        (
            "user",
            EventKind::User,
            json!({"message":{"role":"user","content":[]}}),
        ),
        (
            "result",
            EventKind::Result,
            json!({"subtype":"success","is_error":false,"result":"Done is only text"}),
        ),
        (
            "stream_event",
            EventKind::Stream,
            json!({"event":{"type":"message_start"}}),
        ),
        ("tool_progress", EventKind::ToolProgress, json!({})),
        ("tool_use_summary", EventKind::ToolSummary, json!({})),
        ("auth_status", EventKind::AuthStatus, json!({})),
        ("rate_limit_event", EventKind::RateLimit, json!({})),
    ] {
        let mut value = extra;
        value["type"] = json!(name);
        value["session_id"] = json!("original-session");
        value["parent_tool_use_id"] = json!(null);
        match parse(value.clone()) {
            Message::Event {
                session_id,
                kind: actual,
                payload,
            } => {
                assert_eq!(session_id, "original-session");
                assert_eq!(actual, kind);
                assert_eq!(payload, value);
            }
            _ => panic!("event shape"),
        }
    }
    // A misleading success subtype with is_error=true stays an error-shaped
    // upstream observation, never a completion or permission grant.
    match parse(
        json!({"type":"result","subtype":"success","is_error":true,"session_id":"s","result":"API failure","usage":{}}),
    ) {
        Message::Event {
            kind: EventKind::Result,
            payload,
            ..
        } => assert_eq!(payload["is_error"], true),
        _ => panic!("result shape"),
    }
    for value in [
        json!({"type":"control_cancel_request","request_id":1}),
        json!({"type":"control_cancel_request","request_id":"x\n"}),
        json!({"type":"control_response","response":{"subtype":"success","request_id":"i","response":{},"error":"conflicting"}}),
        json!({"type":"control_request","request_id":"i","request":{"subtype":"can_use_tool","tool_name":"Bash","input":[]}}),
        json!({"type":"result","subtype":"success","session_id":"s","result":"not enough"}),
        json!({"type":"assistant","session_id":"","message":{}}),
    ] {
        malformed(&line(value), Error::Envelope);
    }
    malformed(
        &line(json!({"type":"control_request","request_id":"i","request":{"subtype":"unknown"}})),
        Error::Unsupported,
    );
    malformed(
        &line(json!({"type":"future_unhandled","session_id":"s"})),
        Error::Unsupported,
    );
    // Real installed CLI initialize metadata, values sanitized to empty arrays.
    assert!(matches!(
        parse(
            json!({"type":"control_response","response":{"subtype":"success","request_id":"init",
        "response":{},"pending_permission_requests":[],"pending_user_dialog_requests":[]}})
        ),
        Message::ControlResponse { .. }
    ));
    for name in [
        "pending_permission_requests",
        "pending_user_dialog_requests",
    ] {
        for (pending, expected) in [
            (json!(null), Error::Envelope),
            (json!({}), Error::Envelope),
            (json!([{"request_id":"preexisting"}]), Error::Unsupported),
        ] {
            let mut value = json!({"type":"control_response","response":{"subtype":"success","request_id":"init","response":{}}});
            value["response"][name] = pending;
            malformed(&line(value), expected);
        }
    }
}
#[test]
fn native_claude_stream_host_input() {
    let args = claude::arguments("claude-sonnet-4-6").unwrap();
    assert_eq!(
        args,
        vec![
            "--print",
            "--verbose",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--permission-mode",
            "auto",
            "--permission-prompt-tool",
            "stdio",
            "--model=claude-sonnet-4-6"
        ]
    );
    for model in [
        "",
        "--dangerously-skip-permissions",
        "model --permission-mode bypassPermissions",
        "x\0y",
    ] {
        assert!(claude::arguments(model).is_err());
    }
    for (bytes, subtype) in [
        (claude::initialize("original-1").unwrap(), "initialize"),
        (claude::interrupt("original-2").unwrap(), "interrupt"),
    ] {
        assert_eq!(bytes.last(), Some(&b'\n'));
        assert_eq!(bytes.iter().filter(|&&b| b == b'\n').count(), 1);
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["request"]["subtype"], subtype);
        assert_eq!(value["type"], "control_request");
    }
    let first: Value =
        serde_json::from_slice(&claude::prompt("hello\nworld", None).unwrap()).unwrap();
    assert_eq!(first["session_id"], "");
    assert_eq!(first["message"]["content"], "hello\nworld");
    assert!(first["parent_tool_use_id"].is_null());
    let next: Value =
        serde_json::from_slice(&claude::prompt("next", Some("session-1")).unwrap()).unwrap();
    assert_eq!(next["session_id"], "session-1");
    assert!(claude::prompt(&"x".repeat(MAX_TEXT_BYTES), None).is_ok());
    assert!(claude::prompt(&"x".repeat(MAX_TEXT_BYTES + 1), None).is_err());
    assert!(claude::prompt("", None).is_err());
    assert!(claude::prompt("next", Some("")).is_err());
    assert!(claude::initialize("bad id").is_err());
    assert!(claude::interrupt(&"x".repeat(257)).is_err());
}
