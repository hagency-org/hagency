//! Refusal attribution: each helper refusal class carries its own process exit
//! code, so a hosted load failure is attributable from the status alone (stderr
//! is only drained after the exit, and the sandbox spawn wall eats neither).
use super::fixture::*;
use hagency_core::tasks::RunnerCapability;
use std::{net::SocketAddr, time::Duration};
use tokio::io::AsyncWriteExt;

/// Synthetic inherited context for the pre-runner refusal path. The helper
/// validates this at startup and does not contact the address before it answers
/// a frame, so no service is needed and no live capability is fabricated.
fn synthetic() -> (SocketAddr, RunnerCapability, String) {
    (
        "127.0.0.1:9".parse().unwrap(),
        RunnerCapability {
            dispatch_id: "dispatch".into(),
            runner_id: "runner".into(),
            fence: 1,
            secret: "0".repeat(64),
        },
        "task".into(),
    )
}

/// Drive the raw helper (not `Client::new`, which hands stdin to `serve`) with
/// `payload`, then close the write half, and report its exit code and stderr.
async fn refuse(payload: &[u8]) -> (Option<i32>, String) {
    let (address, cap, task) = synthetic();
    let mut child = raw_helper(address, &cap, &task);
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(payload).await.unwrap();
    drop(stdin);
    let out = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .expect("the helper did not exit")
        .unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// EOF with a partial frame is a pipe/transport fault, not a protocol refusal:
/// it names `Framing` and reports both the bound and the observed size, so the
/// hosted failure is readable without stderr.
#[tokio::test]
async fn native_mcp_helper_framing_is_named() {
    let partial = br#"{"jsonrpc":"2.0","id""#;
    let (code, stderr) = refuse(partial).await;
    assert_eq!(code, Some(70), "expected the Framing exit; stderr={stderr}");
    assert!(stderr.contains("framing refused"), "{stderr}");
    assert!(
        stderr.contains("stdin reached EOF with a partial frame"),
        "{stderr}"
    );
    assert!(stderr.contains("bound 32768 bytes"), "{stderr}");
    let observed = stderr
        .split("observed ")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .and_then(|size| size.parse::<usize>().ok())
        .expect("the observed size is reported");
    assert_eq!(observed, partial.len());
}

/// A well-formed frame outside the current lifecycle is a real protocol
/// refusal and keeps its own code, distinct from a framing fault.
#[tokio::test]
async fn native_mcp_helper_protocol_is_named() {
    let frame = b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n";
    let (code, stderr) = refuse(frame).await;
    assert_eq!(
        code,
        Some(71),
        "expected the Protocol exit; stderr={stderr}"
    );
    assert!(stderr.contains("protocol refused"), "{stderr}");
    assert!(
        stderr.contains("notification is outside the current lifecycle"),
        "{stderr}"
    );
}
