//! Offline fleet registration command (G11): the trusted-local writer of the
//! `registrations` row the provisioning ingress requires before serve.
//!
//! Parity with the retained product's `POST /api/project-sides` →
//! `projectSideStore.upsertSide` (spec `task-rust-project-side-registration`),
//! but as a pre-serve CLI act — the port precedent for a trusted local store
//! write exposed to an operator (`hagency account …`, `accounts.rs`). The write
//! is `DomainRepository::register` (`hagency-store/src/domain.rs:730`), the sole
//! writer of the table; this module adds no second INSERT and no guard of its
//! own — the store's own contract is kept whole: shape validation
//! (`authority.rs:36-58`), the identical-content no-op, the stale-generation
//! refusal (`Error::Generation`), and the rotate-and-reconcile on advance.
use hagency_store::{DomainRepository, Repository, private};
use std::path::{Path, PathBuf};

#[derive(clap::Subcommand)]
pub enum Command {
    /// Write the fleet registration before first serve; refuses without an
    /// initialized private state. The record is the six-field JSON document:
    /// fleetId, generation, serverName, receptionRoomId, representativeMxid,
    /// approvalBotMxid.
    Register {
        /// Path to the registration JSON file (`-` reads stdin).
        #[arg(long)]
        file: PathBuf,
    },
}

pub fn run(state: &Path, command: Command) -> Result<(), hagency_store::Error> {
    // Require an initialized private state; never manufacture a replacement key
    // or import an ambient credential home. Token bytes never leave this scope.
    private::read_secret(&state.join("operator.token"))?;
    let _custody = Repository::open(state)?;
    let mut domain = DomainRepository::open(state)?;
    match command {
        Command::Register { file } => {
            let raw = if file.as_os_str() == "-" {
                let mut buf = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
                    .map_err(|_| hagency_store::Error::OutcomeUnknown)?;
                buf
            } else {
                std::fs::read_to_string(&file).map_err(|_| {
                    hagency_store::Error::Invalid(hagency_core::InvalidInput(
                        "registration file unreadable",
                    ))
                })?
            };
            let registration: hagency_core::authority::Registration = serde_json::from_str(&raw)
                .map_err(|_| {
                    hagency_store::Error::Invalid(hagency_core::InvalidInput(
                        "invalid registration document",
                    ))
                })?;
            // The store's own contract runs unmodified: validate, refuse a stale
            // generation, no-op on identical content, rotate and reconcile on an
            // advance. Nothing here softens or pre-empts it.
            domain.register(&registration)
        }
    }
}
