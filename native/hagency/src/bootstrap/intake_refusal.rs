//! Explicit operator refusal, separate from orphaned-dispatch resume.
use super::{DriverMode, Failure, config::Prepared};
use hagency_matrix::Collector;
use hagency_store::{DomainRepository, DomainStore};
use std::path::Path;

pub async fn run(state: &Path, digest: String) -> Result<usize, Failure> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(Failure::Config);
    }
    // Loading the production-shaped host/profile validates private paths and
    // profile binding, but no Driver/Operation or model is started.
    let mut prepared = Prepared::load(
        state,
        "127.0.0.1:13300".parse().unwrap(),
        DriverMode::Continuous,
    )?;
    let repository = DomainRepository::open(state).map_err(|_| Failure::Startup)?;
    let domain = DomainStore::start(repository, 4).map_err(|_| Failure::Startup)?;
    let collector = Collector::new(
        prepared.matrix.take().ok_or(Failure::Config)?,
        domain.clone(),
    )
    .map_err(|_| Failure::Config)?;
    let result = collector
        .refuse_stale_session_batch(digest)
        .await
        .map_err(|_| Failure::OutcomeUnknown);
    collector
        .close_refusal_owner()
        .await
        .map_err(|_| Failure::OutcomeUnknown)?;
    domain
        .shutdown()
        .await
        .map_err(|_| Failure::OutcomeUnknown)?;
    result
}
