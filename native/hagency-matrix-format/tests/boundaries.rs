use hagency_matrix_format::{
    Error, MAX_BODY_BYTES, MAX_EDIT_DEPTH, MAX_INPUT_BYTES, MAX_JSON_DEPTH, MatrixContent,
};
use serde_json::{Value, json};

fn format(body: &str) -> Value {
    MatrixContent::new(json!({"msgtype":"m.text","body":body}))
        .unwrap()
        .formatted()
        .unwrap()
        .into_value()
}

#[test]
fn native_matrix_format_preservation() {
    for relation in [
        Value::Null,
        json!({"rel_type":"m.thread","event_id":"$root","m.in_reply_to":{"event_id":"$parent"},"is_falling_back":false}),
        json!({"rel_type":"m.replace","event_id":"$old"}),
    ] {
        let value = json!({"msgtype":"m.text","body":"原文 **bold**\r\n下一行", "m.relates_to":relation, "m.mentions":{"user_ids":["@edison:example.org"]}, "room_id":"inert-content-not-a-route", "io.hagency.extra":{"id":"unchanged"}});
        let input = MatrixContent::new(value.clone()).unwrap();
        let result = input.formatted().unwrap();
        assert_eq!(input.as_value(), &value);
        for key in [
            "msgtype",
            "body",
            "m.relates_to",
            "m.mentions",
            "room_id",
            "io.hagency.extra",
        ] {
            assert_eq!(result.as_value()[key], value[key]);
        }
        assert!(result.formatted().unwrap() == result);
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            result.as_value().clone()
        );
    }
    // Supplied HTML remains caller-trusted input, not certified safe by this API.
    let html = json!({"msgtype":"m.text","body":"body","formatted_body":"<script>already supplied</script>","format":"custom"});
    assert_eq!(
        MatrixContent::new(html.clone())
            .unwrap()
            .formatted()
            .unwrap()
            .into_value(),
        html
    );
    let encrypted = json!({"algorithm":"m.megolm.v1.aes-sha2","ciphertext":"**opaque**","session_id":"not-a-domain-session","device_id":"not-a-host-device"});
    assert_eq!(
        MatrixContent::new(encrypted.clone())
            .unwrap()
            .formatted()
            .unwrap()
            .into_value(),
        encrypted
    );
}

#[test]
fn native_matrix_format_bounds() {
    for value in [Value::Null, json!("text"), json!([])] {
        assert!(matches!(MatrixContent::new(value), Err(Error::Shape)));
    }
    assert!(matches!(
        MatrixContent::new(json!({"x":"x".repeat(MAX_INPUT_BYTES)})),
        Err(Error::Capacity)
    ));
    let input =
        MatrixContent::new(json!({"msgtype":"m.text","body":"中".repeat(MAX_BODY_BYTES / 3 + 1)}))
            .unwrap();
    assert!(matches!(input.formatted(), Err(Error::Capacity)));
    let valid = "中".repeat(MAX_BODY_BYTES / 3);
    assert_eq!(format(&valid)["body"], valid);
    for edit in [json!(true), json!("text"), json!([]), json!(42)] {
        let input =
            MatrixContent::new(json!({"msgtype":"m.text","body":"valid","m.new_content":edit}))
                .unwrap();
        let before = input.as_value().clone();
        assert!(matches!(input.formatted(), Err(Error::Shape)));
        assert_eq!(input.as_value(), &before);
    }
    for depth in [MAX_EDIT_DEPTH, MAX_EDIT_DEPTH + 1] {
        let mut value = json!({"msgtype":"m.text","body":"leaf"});
        for _ in 0..depth {
            value = json!({"m.new_content":value});
        }
        let input = MatrixContent::new(value).unwrap();
        assert_eq!(input.formatted().is_ok(), depth == MAX_EDIT_DEPTH);
    }
    for depth in [MAX_JSON_DEPTH, MAX_JSON_DEPTH + 1] {
        let mut value = json!(null);
        for _ in 0..depth {
            value = json!({"next":value});
        }
        assert_eq!(MatrixContent::new(value).is_ok(), depth == MAX_JSON_DEPTH);
    }
    // Individually valid UTF8 bodies can exceed the aggregate output budget.
    let body = "&".repeat(MAX_BODY_BYTES);
    let input = MatrixContent::new(
        json!({"msgtype":"m.text","body":body,"m.new_content":{"msgtype":"m.text","body":body}}),
    )
    .unwrap();
    let before = input.as_value().clone();
    assert!(matches!(input.formatted(), Err(Error::Capacity)));
    assert_eq!(input.as_value(), &before);
}

#[test]
fn native_matrix_format_security() {
    for body in [
        "<script>alert(1)</script><svg onload=evil><img src=x onerror=evil>",
        "<!-- </p><iframe src=evil> --> **bold**",
        "[x](javascript:evil) [x](vbscript:evil) [x](data:text/html,evil) [x](file:///secret)",
        "[x](//evil.example) [x](\\\\evil.example) [x](ftp://evil.example)",
        "![secret alt](https://example.org/file)\n\n```\n</code><script>bad</script>\n```",
        "[title](https://example.org \"&quot; onclick=&quot;evil\")",
        "<div>**Markdown still parses**</div>",
        "> *a*\n> [", // Confirmed ordinary-input panic in the rejected alternative parser.
        "\u{2028}b[tp:/// and w://*\u{2029}",
    ] {
        let output = format(body);
        let html = output["formatted_body"].as_str().unwrap();
        for forbidden in [
            "<script",
            "<svg",
            "<img",
            "<iframe",
            "href=\"javascript:",
            "href=\"data:",
            "href=\"file:",
            "href=\"//",
            "href=\"ftp:",
        ] {
            assert!(!html.contains(forbidden), "{html}");
        }
        assert_eq!(output["body"], body);
    }
    assert_eq!(
        format("![hidden](https://example.org/file)")["formatted_body"],
        "<p></p>\n"
    );
    assert_eq!(
        format("`https://example.org` [https://inner.test](https://outer.test)")["formatted_body"],
        "<p><code>https://example.org</code> <a href=\"https://outer.test\">https://inner.test</a></p>\n"
    );
}

#[test]
fn native_matrix_format_bounds_parser_complexity() {
    for body in [
        "> ".repeat(101) + "deep",
        "*".repeat(hagency_matrix_format::MAX_MARKDOWN_MARKERS + 1),
    ] {
        let input = MatrixContent::new(json!({"msgtype":"m.text","body":body})).unwrap();
        assert!(matches!(input.formatted(), Err(Error::Capacity)));
    }
    let tokens = [
        "*", "_", "[", "]", "<", ">", "(", ")", "中", "🙂", "\n", " ", "&", "#", "`", "~", "\\",
    ];
    let mut seed = 19u64;
    for _ in 0..128 {
        let mut body = String::new();
        for _ in 0..96 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            body.push_str(tokens[(seed >> 32) as usize % tokens.len()]);
        }
        let input = MatrixContent::new(json!({"msgtype":"m.text","body":body})).unwrap();
        let result = input.formatted().unwrap();
        assert_eq!(result.as_value()["body"], body);
        assert!(result.formatted().unwrap() == result);
    }
}
