//! Offline login fixture for the CLI `account login --id` production route.
//! It proves the login child inherited the retained namespace environment
//! exactly as `apply_codex_environment` sets it (HOME == CODEX_HOME, no
//! ambient provider key), then returns a closed exit status the parent
//! classifies. stdout is never captured; the parent reads only the exit code.
use std::fs;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args != ["login"] && args != ["login", "--device-auth"] {
        std::process::exit(2);
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    let codex = std::env::var_os("CODEX_HOME").unwrap_or_default();
    // A login child must carry the retained namespace home and nothing else
    // provider-shaped: any ambient key proves the parent leaked its own env.
    if home.is_empty() || codex.is_empty() || home != codex {
        std::process::exit(2);
    }
    if std::env::var_os("OPENAI_API_KEY").is_some()
        || std::env::var_os("CODEX_API_KEY").is_some()
        || std::env::var_os("AZURE_OPENAI_API_KEY").is_some()
        || std::env::var_os("OPENAI_BASE_URL").is_some()
    {
        std::process::exit(2);
    }
    fs::write(
        Path::new(&home).join("login-arguments.json"),
        serde_json::to_vec(&args).unwrap(),
    )
    .unwrap();
    // The parent classifies by exit status only. The test plants this marker
    // in the retained namespace to make the child refuse, exactly as a
    // provider that declines the login would.
    let outcome = fs::read_to_string(Path::new(&home).join("login-outcome"))
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();
    if outcome == "refuse" {
        std::process::exit(1);
    }
    // An unclassifiable exit: a status that is neither success nor the
    // terminal `1` refusal code, so the parent's fail-closed arm records
    // `uncertain` — never `observed`, never ready.
    if outcome == "uncertain" {
        std::process::exit(17);
    }
    // Success proof: an observed login leaves a derived marker in the
    // namespace. This is the fixture's own observation, not a credential.
    let _ = fs::write(
        Path::new(&home).join("login-observed.json"),
        br#"{"login":"observed"}"#,
    );
}
