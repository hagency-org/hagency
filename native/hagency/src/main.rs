#[path = "mcp/stdio.rs"]
mod mcp_stdio;
use clap::{Parser, Subcommand};
use hagency_store::{DomainRepository, Repository, private};
use std::{net::SocketAddr, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Hagency native migration runtime (isolated development state)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve scoped task tools over MCP stdio using inherited runner context.
    Mcp,
    /// Maintain the canonical task from the host-provisioned runner environment.
    Task {
        /// Stable identifier for a mutation; reuse only with identical content.
        #[arg(long, global = true)]
        call_id: Option<String>,
        #[command(subcommand)]
        command: hagency::task_client::Command,
    },
    /// Internal native guardian. Requires a private inherited channel on stdin.
    #[cfg(unix)]
    #[command(hide = true)]
    Guardian,
    /// Initialize fresh state. Requires an empty, private directory or a new path.
    Init {
        #[arg(long)]
        state_dir: PathBuf,
    },
    /// Prepare or inspect fresh host-owned Codex credential namespaces offline.
    Account {
        // clap forbids required global arguments; the offline account commands
        // accept --state-dir before or after their verb and refuse without it.
        #[arg(long, global = true)]
        state_dir: Option<PathBuf>,
        #[command(subcommand)]
        command: hagency::bootstrap::accounts::Command,
    },
    /// Print a short-lived read-only console link using local operator authority.
    ConsoleAccess {
        #[arg(long)]
        state_dir: PathBuf,
        #[arg(long, default_value = "127.0.0.1:13300")]
        listen: SocketAddr,
        /// Grant only finite native resource catalog publication management.
        #[arg(long, conflicts_with = "manage_resource_configuration")]
        manage_resource_publication: bool,
        /// Grant finite additional resource configuration creation and editing.
        #[arg(long)]
        manage_resource_configuration: bool,
    },
    /// Run the isolated native API. Does not load .env or any existing Hagency state.
    Serve {
        #[arg(long)]
        state_dir: PathBuf,
        #[arg(long, default_value = "127.0.0.1:13300")]
        listen: SocketAddr,
        #[arg(long, default_value_t = 16)]
        queue_capacity: usize,
        /// Execute one configured development attempt after authenticated Matrix refresh.
        #[arg(long)]
        development_driver: bool,
        /// Run configured Palpo v2 custody lanes and native resource publication.
        #[arg(long)]
        palpo_transport: bool,
        /// Private validated static build of the retained native usage console.
        #[arg(long)]
        console_assets: Option<PathBuf>,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let command = Cli::parse().command;
    #[cfg(unix)]
    if matches!(command, Command::Guardian) {
        hagency_platform::run_guardian()?;
        return Ok(());
    }
    if matches!(command, Command::Mcp) {
        mcp_stdio::run_stdio()?;
        return Ok(());
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    tracing::trace!(target: "hagency_startup_observation", "native startup boundary: runtime_entered");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    tracing::trace!(target: "hagency_startup_observation", "native startup boundary: runtime_ready");
    runtime.block_on(run(command))
}
async fn run(command: Command) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Command::Mcp => unreachable!("MCP runs on the dedicated main thread"),
        Command::Task { call_id, command } => {
            let context = hagency::task_client::Context::from_env()?;
            let output = hagency::task_client::run(
                &context,
                &command,
                call_id.as_deref(),
                hagency::task_client::DEFAULT_DEADLINE,
            )
            .await?;
            println!("{}", serde_json::to_string(&output)?);
        }
        #[cfg(unix)]
        Command::Guardian => return Err("guardian requires isolated synchronous startup".into()),
        Command::Init { state_dir } => {
            private::directory(&state_dir)?;
            if std::fs::read_dir(&state_dir)?.next().is_some() {
                return Err("init requires empty state; no existing data will be imported".into());
            }
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).map_err(|_| "secure randomness unavailable")?;
            let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            private::write_new(&state_dir.join("operator.token"), token.as_bytes())?;
            drop(Repository::open(&state_dir)?);
            drop(DomainRepository::open(&state_dir)?);
            println!(
                "Initialized native state. Operator token is in operator.token; keep it private."
            );
        }
        Command::Account { state_dir, command } => {
            let state_dir = state_dir.ok_or("account commands require --state-dir")?;
            let result = hagency::bootstrap::accounts::run(&state_dir, command)?;
            println!("{}", serde_json::to_string(&result)?);
        }
        Command::Serve {
            state_dir,
            listen,
            queue_capacity,
            development_driver,
            palpo_transport,
            console_assets,
        } => {
            let console = console_assets
                .as_deref()
                .map(hagency::console::Console::load)
                .transpose()?;
            let mut bootstrap = hagency::bootstrap::Bootstrap::open_with_options(
                &state_dir,
                listen,
                queue_capacity,
                hagency::bootstrap::Options {
                    development_driver,
                    palpo_transport,
                },
            )?;
            if let Some(console) = console {
                bootstrap = bootstrap.with_console(console);
            }
            let cancel = hagency_matrix::CancellationToken::new();
            let signal = cancel.clone();
            tokio::spawn(async move {
                #[cfg(unix)]
                {
                    let mut termination =
                        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                            .expect("install SIGTERM handler");
                    tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = termination.recv() => {} }
                }
                #[cfg(not(unix))]
                let _ = tokio::signal::ctrl_c().await;
                signal.cancel();
            });
            if let Err(error) = bootstrap.serve(&cancel).await {
                tracing::error!("native server stopped; closing retained owners");
                if bootstrap.close().await.is_err() {
                    tracing::error!("native shutdown remains unknown; retaining original owners");
                    std::future::pending::<()>().await;
                }
                return Err(error.into());
            }
        }
        Command::ConsoleAccess {
            state_dir,
            listen,
            manage_resource_publication,
            manage_resource_configuration,
        } => {
            println!(
                "{}",
                if manage_resource_configuration {
                    hagency::console::client::configuration_access(&state_dir, listen).await?
                } else if manage_resource_publication {
                    hagency::console::client::publication_access(&state_dir, listen).await?
                } else {
                    hagency::console::client::access(&state_dir, listen).await?
                }
            );
        }
    }
    Ok(())
}
