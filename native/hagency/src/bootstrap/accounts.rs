//! Offline account commands acquire the same exclusive owners as serve.
use hagency_store::{
    ACCOUNT_PROFILE, AccountChoice, AccountReadinessMode, DomainRepository, LoginOutcome,
    LoginVerdict, LogoutObservation, Repository, private,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command as StdCommand,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(clap::Subcommand)]
pub enum Command {
    /// Create a fresh namespace; effective authentication and quota stay unknown.
    Prepare {
        #[arg(long, default_value = ACCOUNT_PROFILE)]
        profile: String,
    },
    /// Inspect original preparation state without repeating filesystem effects.
    Inspect,
    /// Permanently fence a binding and withdraw its local resources.
    Retire {
        #[arg(long)]
        id: String,
    },
    /// Observe a provider login inside the retained namespace (ADR-114 D-ADR114
    /// observe). Drives `<login-binary> login` with HOME/CODEX_HOME set exactly
    /// as `apply_codex_environment` sets them, classifies the child's exit, and
    /// records the derived readiness fact — never a credential byte.
    Login {
        #[arg(long)]
        id: String,
        /// The provider login binary; `<binary> login` is spawned. Its stdout is
        /// never captured and it inherits the operator's terminal.
        #[arg(long)]
        login_binary: PathBuf,
    },
}
pub fn run(state: &Path, command: Command) -> Result<Vec<AccountChoice>, hagency_store::Error> {
    // Require an initialized private state; never manufacture a replacement key
    // or import an ambient credential home. Token bytes never leave this scope.
    private::read_secret(&state.join("operator.token"))?;
    let _custody = Repository::open(state)?;
    let mut domain = DomainRepository::open(state)?;
    match command {
        Command::Prepare { profile } => {
            let prepared = domain.reserve_account(&profile)?;
            // Even on error the original ID is discoverable through inspect.
            // Do not retry with another identity or erase the possible directory.
            Ok(vec![domain.materialize_account(&prepared.id)?])
        }
        Command::Inspect => domain.account_choices(),
        Command::Retire { id } => Ok(vec![
            domain.retire_account(&id, LogoutObservation::unobserved())?,
        ]),
        Command::Login { id, login_binary } => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| hagency_store::Error::OutcomeUnknown)?
                .as_millis() as u64;
            // Allocate the attempt BEFORE the effect, so an interrupted login
            // is reconciled as `uncertain` on reopen — never promoted.
            let attempt = domain.begin_account_login(&id, now)?;
            let account = domain.managed_account(&id)?;
            let launch = account.prepare_login()?;
            let mut environment: BTreeMap<std::ffi::OsString, std::ffi::OsString> = BTreeMap::new();
            // Refuses ambient provider keys and sets HOME/CODEX_HOME from the
            // retained binding only. Nothing else reaches the child.
            launch.apply_codex_environment(&mut environment)?;
            // The login child inherits the operator's terminal; stdout is never
            // captured, only the exit status is read.
            let status = StdCommand::new(&login_binary)
                .arg("login")
                .env_clear()
                .envs(&environment)
                .status();
            let verdict = match status {
                Ok(status) if status.success() => LoginVerdict {
                    mode: AccountReadinessMode::Subscription,
                    provider_state: "logged-in-subscription".into(),
                    outcome: LoginOutcome::Observed,
                    expires_at_ms: None,
                },
                Ok(status) if status.code() == Some(1) => LoginVerdict {
                    mode: AccountReadinessMode::Unknown,
                    provider_state: "login-refused".into(),
                    outcome: LoginOutcome::Refused,
                    expires_at_ms: None,
                },
                // Fail closed: a spawn failure, signal or any unclassifiable
                // exit records `uncertain` — never `observed`, never ready.
                _ => LoginVerdict {
                    mode: AccountReadinessMode::Unknown,
                    provider_state: "login-unknown".into(),
                    outcome: LoginOutcome::Uncertain,
                    expires_at_ms: None,
                },
            };
            domain.settle_account_login(attempt, verdict, now)?;
            domain.account_choices()
        }
    }
}
