//! Explicit operator-only catalog compatibility check; never prompts a model.
//! No authentic task/dispatch/account exists and no API accepts the dummy scope.
use hagency_platform::Launch;
use hagency_runtime::{
    claude::{
        self, TaskMcp,
        session::{Error, Limits},
    },
    owned::{Cleanup, OwnedClaudeSession},
};
use serde_json::json;
use std::{
    collections::BTreeMap, ffi::OsString, net::TcpListener, path::PathBuf, process::ExitCode,
    time::Duration,
};

const TASK: &str = "diagnostic_catalog_only";
fn environment(
    directory: PathBuf,
    operator_home: OsString,
    operator_user: OsString,
    address: std::net::SocketAddr,
) -> BTreeMap<OsString, OsString> {
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
        ("HAGENCY_RUNNER_API_ADDR".into(), address.to_string().into()),
        ("HAGENCY_TASK_ID".into(), TASK.into()),
        // Not a credential issued by Hagency. The held socket has no service.
        (
            "HAGENCY_RUNNER_CAPABILITY".into(),
            json!({"dispatch_id":"diagnostic_dispatch","runner_id":"diagnostic_runner",
            "fence":1,"secret":"0".repeat(64)})
            .to_string()
            .into(),
        ),
    ])
}
#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [confirm, executable, guardian, helper, directory] = args.as_slice() else {
        eprintln!(
            "operator only: --run-local-catalog-probe CLAUDE_EXECUTABLE GUARDIAN NATIVE_HELPER PRIVATE_EMPTY_WORKSPACE"
        );
        return ExitCode::from(2);
    };
    if confirm != "--run-local-catalog-probe" {
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
    if [executable, guardian, helper]
        .iter()
        .any(|p| !PathBuf::from(p).is_absolute() || !PathBuf::from(p).is_file())
    {
        return ExitCode::from(2);
    }
    let Some(operator_home) = std::env::var_os("HOME").filter(|v| PathBuf::from(v).is_absolute())
    else {
        return ExitCode::from(2);
    };
    let Some(operator_user) = std::env::var_os("USER").filter(|v| !v.is_empty()) else {
        return ExitCode::from(2);
    };
    // Keeping this owned socket open prevents its address being reused by a real
    // task service during the diagnostic. No accept/read/write API is offered.
    let Ok(listener) = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)) else {
        return ExitCode::FAILURE;
    };
    let Ok(address) = listener.local_addr() else {
        return ExitCode::FAILURE;
    };
    if listener.set_nonblocking(true).is_err() {
        return ExitCode::FAILURE;
    }
    let mut environment = environment(directory.clone(), operator_home, operator_user, address);
    if let Some(root) = std::env::var_os("SystemRoot") {
        environment.insert("SystemRoot".into(), root);
    }
    let Ok(mut arguments) = claude::task_arguments("sonnet", false) else {
        return ExitCode::FAILURE;
    };
    // Builtins are unnecessary. Never use safe-mode here: it disables MCP too.
    arguments.push("--tools=".into());
    let launch = Launch {
        executable: executable.into(),
        arguments: arguments.into_iter().map(Into::into).collect(),
        directory,
        environment,
        require_crash_containment: false,
    };
    let Ok(helper) = TaskMcp::new(helper.into(), TASK.into()) else {
        return ExitCode::FAILURE;
    };
    let mut runner = match OwnedClaudeSession::spawn(
        &PathBuf::from(guardian),
        &launch,
        Limits {
            write_timeout_ms: 3000,
            event_wait_ms: 15_000,
            lifetime_ms: 40_000,
        },
    ) {
        Ok(runner) => runner,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut initialized = false;
    let outcome = tokio::time::timeout(Duration::from_secs(30), async {
        runner.initialize().await?;
        initialized = true;
        runner.bind_task_mcp(helper).await?;
        Ok::<_, Error>(())
    })
    .await;
    let error = match outcome {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error.to_string()),
        Err(_) => Some("operator diagnostic deadline".into()),
    };
    let bound = error.is_none();
    let (leader, signals, whole) = match runner.stop() {
        Cleanup::Observed(report) => (
            report.scope.leader_exited,
            report.scope.signals_accepted,
            report.scope.whole_tree_stopped,
        ),
        _ => (false, false, false),
    };
    let api_connection = match listener.accept() {
        Ok(_) => Some(true),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Some(false),
        Err(_) => None,
    };
    let passed = bound && api_connection == Some(false) && leader;
    println!(
        "{}",
        json!({"diagnostic_only":true,"native_host_qualified":false,"managed_account_bound":false,
        "synthetic_non_authoritative_context":true,"initialized":initialized,"task_mcp_bound":bound,"error":error,
        "prompt_sent":false,"tool_calls_sent":0,"api_connection_observed":api_connection,
        "leader_exited":leader,"signals_accepted":signals,"whole_tree_stopped":whole,"catalog_compatible":passed})
    );
    if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[test]
fn native_claude_task_probe_environment() {
    let e = environment(
        PathBuf::from("/private/probe"),
        "/private/operator".into(),
        "offline-user".into(),
        "127.0.0.1:19222".parse().unwrap(),
    );
    assert_eq!(e.len(), 9);
    for (key, value) in [
        ("HOME", "/private/operator"),
        ("USER", "offline-user"),
        ("HAGENCY_TASK_ID", TASK),
        ("HAGENCY_RUNNER_API_ADDR", "127.0.0.1:19222"),
    ] {
        assert_eq!(
            e.get(std::ffi::OsStr::new(key)),
            Some(&OsString::from(value))
        );
    }
    for key in [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CONFIG_DIR",
        "OPENAI_API_KEY",
        "HAGENCY_FILE_TOOLS",
        "HAGENCY_RECEIVE_FILE_TOOLS",
    ] {
        assert!(!e.contains_key(std::ffi::OsStr::new(key)));
    }
    let cap: serde_json::Value = serde_json::from_str(
        e[std::ffi::OsStr::new("HAGENCY_RUNNER_CAPABILITY")]
            .to_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(cap["secret"], "0".repeat(64));
    assert_eq!(cap["dispatch_id"], "diagnostic_dispatch");
}
