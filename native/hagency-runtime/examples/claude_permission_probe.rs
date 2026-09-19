//! Operator-only live compatibility diagnostic, never an ordinary test.
//! Always denies. Does not bind a Hagency account/task or qualify production.
use hagency_platform::Launch;
use hagency_runtime::{
    claude::{
        self, EventKind, Message,
        session::{ApprovalControlPolicy, Error, Limits, PermissionDecision, PreparedUpdate},
    },
    owned::{Cleanup, OwnedClaudeSession},
};
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf, process::ExitCode, time::Duration};

fn operator_environment(
    directory: PathBuf,
    operator_home: std::ffi::OsString,
    operator_user: std::ffi::OsString,
) -> BTreeMap<std::ffi::OsString, std::ffi::OsString> {
    // With this installed CLI, preserving USER makes its existing local login
    // available. It is OS identity metadata, not an imported credential.
    BTreeMap::from([
        ("HOME".into(), operator_home),
        ("USER".into(), operator_user),
        ("PATH".into(), "/opt/homebrew/bin:/usr/bin:/bin".into()),
        ("TMPDIR".into(), directory.into_os_string()),
        (
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".into(),
            "1".into(),
        ),
        ("CLAUDE_CODE_DISABLE_BACKGROUND_TASKS".into(), "1".into()),
    ])
}

#[derive(Default)]
struct Evidence {
    initialized: bool,
    prompt_flushed: bool,
    auto_observed: bool,
    permissions: usize,
    denied: usize,
    assistant: usize,
    result: bool,
    result_error: bool,
    api_status: Option<u64>,
}
async fn drive(runner: &mut OwnedClaudeSession, e: &mut Evidence) -> Result<(), Error> {
    runner.initialize().await?;
    e.initialized = true;
    runner.prompt("Call Bash exactly once with command printf 'HAGENCY_CLAUDE_DENY_PROBE'. If permission is denied, do not retry or use another tool. Return a brief acknowledgement.").await?;
    e.prompt_flushed = true;
    for _ in 0..64 {
        match runner.next_message().await? {
            Message::Event {
                kind: EventKind::System,
                payload,
                ..
            } if payload["subtype"] == "init" => {
                e.auto_observed = payload["permissionMode"] == "auto";
                if !e.auto_observed {
                    return Err(Error::State);
                }
                runner.enable_approval_control(ApprovalControlPolicy {
                    owner_wait_ms: 3000,
                    response_reserve_ms: 5000,
                })?;
            }
            Message::Permission { request_id, .. } => {
                e.permissions += 1;
                if e.permissions > 4 {
                    return Err(Error::Capacity);
                }
                let mut response =
                    runner.prepare_approval(&request_id, PermissionDecision::Deny)?;
                loop {
                    match runner.send_prepared_approval(&mut response).await? {
                        PreparedUpdate::WriteAccepted(_) => {
                            e.denied += 1;
                            break;
                        }
                        PreparedUpdate::Message(Message::Event {
                            kind: EventKind::System | EventKind::ToolProgress | EventKind::RateLimit,
                            ..
                        }) => {}
                        // Do not discard a cancellation, result or second request
                        // while sending. Stop with original byte progress instead.
                        PreparedUpdate::Message(_) => return Err(Error::PermissionUnavailable),
                    }
                }
            }
            Message::Event {
                kind: EventKind::Assistant,
                ..
            } => e.assistant += 1,
            Message::Event {
                kind: EventKind::Result,
                payload,
                ..
            } => {
                e.result = true;
                e.result_error = payload["is_error"].as_bool().unwrap_or(true);
                e.api_status = payload["api_error_status"].as_u64();
                return Ok(());
            }
            Message::Event { .. } => {}
            _ => return Err(Error::PermissionUnavailable),
        }
    }
    Err(Error::Capacity)
}
#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [confirm, executable, guardian, directory] = args.as_slice() else {
        eprintln!(
            "operator only: --run-local-deny-probe CLAUDE_EXECUTABLE GUARDIAN PRIVATE_EMPTY_WORKSPACE"
        );
        return ExitCode::from(2);
    };
    if confirm != "--run-local-deny-probe" {
        return ExitCode::from(2);
    }
    let directory = PathBuf::from(directory);
    if !directory.is_absolute()
        || directory.canonicalize().ok().as_ref() != Some(&directory)
        || !std::fs::read_dir(&directory).is_ok_and(|mut entries| entries.next().is_none())
    {
        return ExitCode::from(2);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !std::fs::metadata(&directory).is_ok_and(|m| m.permissions().mode() & 0o077 == 0) {
            return ExitCode::from(2);
        }
    }
    let Some(operator_home) =
        std::env::var_os("HOME").filter(|value| PathBuf::from(value).is_absolute())
    else {
        return ExitCode::from(2);
    };
    let Some(operator_user) = std::env::var_os("USER").filter(|value| !value.is_empty()) else {
        return ExitCode::from(2);
    };
    // This is the provider's own operator login, NOT a managed-account launch.
    // No API key, auth token, credential file or ambient settings are imported.
    let mut environment = operator_environment(directory.clone(), operator_home, operator_user);
    if let Some(root) = std::env::var_os("SystemRoot") {
        environment.insert("SystemRoot".into(), root);
    }
    let Ok(mut arguments) = claude::arguments("sonnet") else {
        return ExitCode::FAILURE;
    };
    arguments.extend(["--safe-mode","--restricted","--tools=Bash","--disable-slash-commands","--no-chrome",
        "--setting-sources=","--strict-mcp-config","--mcp-config={\"mcpServers\":{}}","--no-session-persistence",
        "--settings={\"permissions\":{\"ask\":[\"Bash\"]}}","--max-budget-usd=0.25",
        "--system-prompt=Test permission rejection. Attempt the one requested Bash call, then stop. Do not read files or use other tools."]
        .into_iter().map(str::to_owned));
    let launch = Launch {
        executable: executable.into(),
        arguments: arguments.into_iter().map(Into::into).collect(),
        directory,
        environment,
        require_crash_containment: false,
    };
    let mut runner = match OwnedClaudeSession::spawn(
        &PathBuf::from(guardian),
        &launch,
        Limits {
            write_timeout_ms: 3000,
            event_wait_ms: 60_000,
            lifetime_ms: 90_000,
        },
    ) {
        Ok(runner) => runner,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut e = Evidence::default();
    let outcome = tokio::time::timeout(Duration::from_secs(75), drive(&mut runner, &mut e)).await;
    let error = match outcome {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error.to_string()),
        Err(_) => Some("operator diagnostic deadline".into()),
    };
    let progress = runner.termination().and_then(|t| t.unconfirmed_write);
    let (leader, signals, whole) = match runner.stop() {
        Cleanup::Observed(report) => (
            report.scope.leader_exited,
            report.scope.signals_accepted,
            report.scope.whole_tree_stopped,
        ),
        _ => (false, false, false),
    };
    let passed = error.is_none()
        && e.auto_observed
        && e.permissions > 0
        && e.permissions == e.denied
        && e.result
        && !e.result_error;
    println!(
        "{}",
        json!({"diagnostic_only":true,"native_host_qualified":false,"managed_account_bound":false,
        "initialized":e.initialized,"prompt_flushed":e.prompt_flushed,"auto_observed":e.auto_observed,
        "permission_requests":e.permissions,"deny_writes":e.denied,"allow_writes":0,"assistant_messages":e.assistant,
        "result_observed":e.result,"result_error":e.result_error,"api_status":e.api_status,"error":error,
        "unconfirmed_write":progress.map(|p|json!({"accepted":p.accepted_bytes,"total":p.total_bytes,"flushed":p.flushed})),
        "leader_exited":leader,"signals_accepted":signals,"whole_tree_stopped":whole,"deny_roundtrip_observed":passed})
    );
    if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[test]
fn native_claude_probe_environment() {
    let environment = operator_environment(
        PathBuf::from("/private/probe"),
        "/private/operator".into(),
        "offline-user".into(),
    );
    assert_eq!(environment.len(), 6);
    assert_eq!(
        environment.get(std::ffi::OsStr::new("USER")),
        Some(&std::ffi::OsString::from("offline-user"))
    );
    assert_eq!(
        environment.get(std::ffi::OsStr::new("HOME")),
        Some(&std::ffi::OsString::from("/private/operator"))
    );
    for name in [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CONFIG_DIR",
        "OPENAI_API_KEY",
    ] {
        assert!(!environment.contains_key(std::ffi::OsStr::new(name)));
    }
}
