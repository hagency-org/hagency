//! One historical host source per fresh native execution session. Observations
//! never confer current execution, provider authenticity or quota authority.
use super::{
    DomainRepository, OwnedDispatchScope, execution, owned_dispatch, read_engagement,
    read_resource, serialize,
};
use crate::Error;
use hagency_core::{canonical, project::identifier, tasks::RunnerCapability};
use hagency_metering::{Framework, TokenCounts, observation::UsageObservation};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::json;
mod reads;
mod types;
pub(super) use reads::ceiling_report;
pub use types::*;
use types::{add, grow, period_keys, unknown};

fn framework(value: &str) -> Result<Framework, Error> {
    match value {
        "claude" => Ok(Framework::Claude),
        "codex" => Ok(Framework::Codex),
        _ => Err(Error::Schema),
    }
}
fn source_binding(db: &Connection, source: &UsageSource) -> Result<(String, String), Error> {
    let row: Option<(String, String, String)> = db
        .query_row(
            "SELECT engagement_id,framework,identity_digest FROM usage_sources WHERE id=?1",
            [&source.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (engagement, runtime, digest) = row.ok_or(Error::RunnerAuthority)?;
    if digest != source.identity_digest {
        return Err(Error::RunnerAuthority);
    }
    framework(&runtime)?;
    Ok((engagement, runtime))
}
impl DomainRepository {
    /// Initial binding requires the exact current Started scope. Existing source
    /// replay is historical identity only; it never renews a retired capability.
    pub fn bind_usage_source(
        &mut self,
        cap: &RunnerCapability,
        scope: &OwnedDispatchScope,
        now: u64,
    ) -> Result<UsageSource, Error> {
        self.bind_usage_clock(cap, scope, || Ok(now))
    }
    pub(crate) fn bind_usage_clock(
        &mut self,
        cap: &RunnerCapability,
        scope: &OwnedDispatchScope,
        clock: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<UsageSource, Error> {
        identifier(&cap.dispatch_id, 128)?;
        identifier(&cap.runner_id, 128)?;
        if cap.secret.len() != 64 {
            return Err(Error::RunnerAuthority);
        }
        scope.check_started(cap)?;
        let id = format!(
            "usage_{}",
            &canonical::digest(&json!([cap.dispatch_id, cap.fence]))?[..32]
        );
        let identity_digest = canonical::digest(&json!([
            "usage_source",
            scope.fingerprint(),
            cap.runner_id,
            cap.fence,
            canonical::digest(&json!(cap.secret))?
        ]))?;
        let source = UsageSource {
            id,
            identity_digest,
        };
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM usage_sources WHERE id=?1)",
            [&source.id],
            |r| r.get(0),
        )?;
        if exists {
            source_binding(&tx, &source)?;
            return Ok(source);
        }
        let now = clock()?;
        period_keys(now)?;
        let current = owned_dispatch::scope(&tx, cap, now, &["started"])?;
        if current.fingerprint() != scope.fingerprint() {
            return Err(Error::RunnerAuthority);
        }
        let session = execution::session(&tx, &current.input().session_id)?;
        let engagement = read_engagement(&tx, session.engagement_id())?;
        framework(&current.resource().framework)?;
        let global: u64 = tx.query_row("SELECT COUNT(*) FROM usage_sources", [], |r| r.get(0))?;
        let own: u64 = tx.query_row(
            "SELECT COUNT(*) FROM usage_sources WHERE engagement_id=?1",
            [&engagement.id],
            |r| r.get(0),
        )?;
        if global >= MAX_USAGE_SOURCES || own >= MAX_ENGAGEMENT_USAGE_SOURCES {
            return Err(Error::Capacity);
        }
        let (fleet, generation): (String, u64) = tx.query_row(
            "SELECT fleet_id,generation FROM engagements WHERE id=?1",
            [&engagement.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let attribution = json!({"engagement_id":engagement.id,"fleet_id":fleet,"registration_generation":generation,"project_id":engagement.project_id,
            "resource_id":engagement.resource_id,"runtime_name":engagement.runtime_name,"session_id":current.input().session_id,
            "task_id":current.task().id,"task_epoch":current.task().execution_epoch,"scope_fingerprint":current.fingerprint()});
        tx.execute("INSERT INTO usage_sources(id,dispatch_id,fence,engagement_id,identity_digest,framework,attribution,high_water,latest_counts) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'null')",
            params![source.id,cap.dispatch_id,cap.fence,engagement.id,source.identity_digest,current.resource().framework,serialize(&attribution)?,serialize(&unknown())?])?;
        tx.commit()?;
        Ok(source)
    }
    /// Host-only restoration of an admitted historical identity. Never proves
    /// that bytes selected by a caller actually came from the retained execution.
    pub fn restore_usage_source(&self, id: &str) -> Result<UsageSource, Error> {
        identifier(id, 128)?;
        let identity_digest = self
            .db
            .query_row(
                "SELECT identity_digest FROM usage_sources WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        Ok(UsageSource {
            id: id.into(),
            identity_digest,
        })
    }
    pub fn record_usage_observation(
        &mut self,
        source: &UsageSource,
        call_id: &str,
        observation: &UsageObservation,
        now: u64,
    ) -> Result<UsageReceipt, Error> {
        self.record_usage_clock(source, call_id, observation, || Ok(now))
    }
    pub(crate) fn record_usage_clock(
        &mut self,
        source: &UsageSource,
        call_id: &str,
        observation: &UsageObservation,
        clock: impl FnOnce() -> Result<u64, Error>,
    ) -> Result<UsageReceipt, Error> {
        identifier(call_id, 256)?;
        let digest = canonical::digest(&json!(["usage_observation", observation]))?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (engagement, runtime) = source_binding(&tx, source)?;
        if observation.framework() != framework(&runtime)? {
            return Err(Error::RunnerAuthority);
        }
        let previous: Option<(String, String)> = tx
            .query_row(
                "SELECT digest,response FROM usage_receipts WHERE source_id=?1 AND call_id=?2",
                params![source.id, call_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((previous, response)) = previous {
            if previous != digest {
                return Err(Error::Conflict);
            }
            let mut receipt: UsageReceipt = serde_json::from_str(&response)?;
            receipt.replayed = true;
            return Ok(receipt);
        }
        let now = clock()?; // Fresh only after queue and immediate DB lock.
        let (daily, monthly) = period_keys(now)?;
        let last: Option<u64> = tx
            .query_row(
                "SELECT observed_at FROM usage_clock WHERE singleton=1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if last.is_some_and(|last| now < last) {
            return Err(
                hagency_core::InvalidInput("usage observation clock moved backwards").into(),
            );
        }
        let global: u64 = tx.query_row("SELECT COUNT(*) FROM usage_receipts", [], |r| r.get(0))?;
        let own: u64 = tx.query_row(
            "SELECT COUNT(*) FROM usage_receipts WHERE source_id=?1",
            [&source.id],
            |r| r.get(0),
        )?;
        if global >= MAX_USAGE_RECEIPTS || own >= MAX_SOURCE_USAGE_RECEIPTS {
            return Err(Error::Capacity);
        }
        let (water,regressions,count,history):(String,u64,u64,bool)=tx.query_row("SELECT high_water,regressions,observations,historical_incomplete FROM usage_sources WHERE id=?1",[&source.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        let (water, delta, regressed) = grow(serde_json::from_str(&water)?, observation.counts())?;
        // Sum known historical values independently of optional latest totals:
        // missing evidence in another source cannot conceal arithmetic overflow.
        let mut known = KnownTokens::default().adding(water)?;
        let mut query =
            tx.prepare("SELECT high_water FROM usage_sources WHERE engagement_id=?1 AND id<>?2")?;
        for old in query.query_map(params![engagement, source.id], |r| r.get::<_, String>(0))? {
            known = known.adding(serde_json::from_str(&old?)?)?;
        }
        drop(query);
        let incomplete = observation.incomplete() || regressed;
        reads::credit_period(
            &tx,
            &engagement,
            UsagePeriodKind::Daily,
            &daily,
            delta,
            incomplete,
        )?;
        reads::credit_period(
            &tx,
            &engagement,
            UsagePeriodKind::Monthly,
            &monthly,
            delta,
            incomplete,
        )?;
        let receipt = UsageReceipt {
            source_id: source.id.clone(),
            observed_at: now,
            daily_key: daily,
            monthly_key: monthly,
            delta,
            regressed,
            incomplete,
            replayed: false,
        };
        tx.execute("UPDATE usage_sources SET high_water=?2,latest_counts=?3,latest_observation=?4,historical_incomplete=?5,regressions=?6,observations=?7,observed_at=?8,latest_incomplete=?9,latest_regressed=?10 WHERE id=?1",params![source.id,serialize(&water)?,serialize(&observation.counts())?,serialize(observation)?,history||incomplete,add(regressions,u64::from(regressed))?,add(count,1)?,now,incomplete,regressed])?;
        tx.execute("INSERT INTO usage_receipts(source_id,call_id,digest,observation,response) VALUES(?1,?2,?3,?4,?5)",params![source.id,call_id,digest,serialize(observation)?,serialize(&receipt)?])?;
        tx.execute("INSERT INTO usage_clock(singleton,observed_at) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET observed_at=excluded.observed_at",[now])?;
        tx.commit()?;
        Ok(receipt)
    }
}
