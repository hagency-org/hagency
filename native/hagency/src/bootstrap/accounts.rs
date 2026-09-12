//! Offline account commands acquire the same exclusive owners as serve.
use hagency_store::{ACCOUNT_PROFILE, AccountChoice, DomainRepository, Repository, private};
use std::path::Path;

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
        Command::Retire { id } => Ok(vec![domain.retire_account(&id)?]),
    }
}
