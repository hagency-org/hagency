use super::*;
use hagency_core::JSON_SAFE_MAX;
use rusqlite::Transaction;
use types::{optional_add, zero};
pub(super) fn credit_period(
    tx: &Transaction<'_>,
    engagement: &str,
    kind: UsagePeriodKind,
    key: &str,
    delta: TokenCounts,
    incomplete: bool,
) -> Result<(), Error> {
    let old:Option<(String,String,bool,u64)>=tx.query_row("SELECT observed_growth,known_growth,incomplete,observations FROM usage_periods WHERE engagement_id=?1 AND granularity=?2 AND period_key=?3",params![engagement,kind.name(),key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let (old, known, was_incomplete, observations) = if let Some((old, known, incomplete, count)) =
        old
    {
        (
            serde_json::from_str(&old)?,
            serde_json::from_str::<KnownTokens>(&known)?,
            incomplete,
            count,
        )
    } else {
        let global: u64 = tx.query_row("SELECT COUNT(*) FROM usage_periods", [], |r| r.get(0))?;
        let own: u64 = tx.query_row(
            "SELECT COUNT(*) FROM usage_periods WHERE engagement_id=?1",
            [engagement],
            |r| r.get(0),
        )?;
        if global >= MAX_USAGE_PERIODS || own >= MAX_ENGAGEMENT_USAGE_PERIODS {
            return Err(Error::Capacity);
        }
        (zero(), KnownTokens::default(), false, 0)
    };
    let known = known.adding(delta)?;
    let observed = optional_add(old, delta)?;
    tx.execute("INSERT INTO usage_periods(engagement_id,granularity,period_key,observed_growth,known_growth,incomplete,observations) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(engagement_id,granularity,period_key) DO UPDATE SET observed_growth=excluded.observed_growth,known_growth=excluded.known_growth,incomplete=excluded.incomplete,observations=excluded.observations",params![engagement,kind.name(),key,serialize(&observed)?,serialize(&known)?,was_incomplete||incomplete,add(observations,1)?])?;
    Ok(())
}
impl DomainRepository {
    pub fn usage_report(&self, engagement: &str, at: u64) -> Result<UsageReport, Error> {
        identifier(engagement, 128)?;
        period_keys(at)?;
        let record = read_engagement(&self.db, engagement)?;
        // The same figures the admission decision uses (slice 3), published
        // instead of re-derived: the resource's drawn ceiling, its display
        // total, and the minimum of the non-null limits. No exclusion here —
        // publication states current headroom including this engagement's own
        // commitment, mirroring the retained `remainingFor(agent)` reads.
        let report = ceiling_report(&self.db, &record.resource_id, at)?;
        let limits = super::super::budget(
            &self.db,
            &read_resource(&self.db, &record.resource_id)?,
            None,
            false,
        )?;
        let by_ceiling = report
            .ceiling_tokens
            .map(|c| c.saturating_sub(report.drawn));
        let remaining = [by_ceiling, limits.remaining_tokens.map(u64::from)]
            .into_iter()
            .flatten()
            .min();
        Ok(UsageReport {
            engagement_id: engagement.into(),
            at_ms: at,
            summary: self.usage_summary(engagement)?,
            daily: self.usage_period(engagement, UsagePeriodKind::Daily, at)?,
            monthly: self.usage_period(engagement, UsagePeriodKind::Monthly, at)?,
            ceiling: UsageCeiling {
                tokens_drawn: report.drawn,
                tokens_used: report.consumed,
                remaining_tokens: remaining,
            },
        })
    }
    pub fn usage_source(&self, source: &UsageSource) -> Result<SourceUsage, Error> {
        source_binding(&self.db, source)?;
        let (runtime,water,latest,observation,history,regressions,count,at):(String,String,String,Option<String>,bool,u64,u64,Option<u64>)=self.db.query_row("SELECT framework,high_water,latest_counts,latest_observation,historical_incomplete,regressions,observations,observed_at FROM usage_sources WHERE id=?1",[&source.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?)))?;
        let (latest_incomplete, latest_regressed) = self.db.query_row(
            "SELECT latest_incomplete,latest_regressed FROM usage_sources WHERE id=?1",
            [&source.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(SourceUsage {
            source_id: source.id.clone(),
            framework: runtime,
            high_water: serde_json::from_str(&water)?,
            latest_counts: serde_json::from_str(&latest)?,
            latest_observation: observation.map(|s| serde_json::from_str(&s)).transpose()?,
            latest_incomplete,
            latest_regressed,
            historical_incomplete: history,
            regressions,
            observations: count,
            observed_at: at,
        })
    }
    pub fn usage_summary(&self, engagement: &str) -> Result<UsageSummary, Error> {
        identifier(engagement, 128)?;
        let mut result = UsageSummary {
            sources: 0,
            latest_counts: None,
            known_high_water_lower_bound: None,
            latest_incomplete_sources: 0,
            historically_incomplete_sources: 0,
            regression_observations: 0,
            evidence: UsageEvidence::HostAttributedUntrustedUsage,
        };
        let mut latest = zero();
        let mut known = KnownTokens::default();
        let mut known_latest = KnownTokens::default();
        let mut query=self.db.prepare("SELECT high_water,latest_counts,historical_incomplete,regressions,latest_incomplete FROM usage_sources WHERE engagement_id=?1 ORDER BY id")?;
        for row in query.query_map([engagement], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, bool>(2)?,
                r.get::<_, u64>(3)?,
                r.get::<_, bool>(4)?,
            ))
        })? {
            let (water, next, history, regressions, latest_incomplete) = row?;
            let next: Option<TokenCounts> = serde_json::from_str(&next)?;
            let next = next.unwrap_or_else(unknown);
            known = known.adding(serde_json::from_str(&water)?)?;
            known_latest = known_latest.adding(next)?;
            latest = optional_add(latest, next)?;
            result.sources = add(result.sources, 1)?;
            result.latest_incomplete_sources = add(
                result.latest_incomplete_sources,
                u64::from(latest_incomplete),
            )?;
            result.historically_incomplete_sources =
                add(result.historically_incomplete_sources, u64::from(history))?;
            result.regression_observations = add(result.regression_observations, regressions)?;
        }
        if result.sources > 0 {
            result.latest_counts = Some(latest);
            result.known_high_water_lower_bound = Some(known);
        }
        Ok(result)
    }
    pub fn usage_period(
        &self,
        engagement: &str,
        kind: UsagePeriodKind,
        at: u64,
    ) -> Result<Option<UsagePeriod>, Error> {
        identifier(engagement, 128)?;
        let (day, month) = period_keys(at)?;
        let key = match kind {
            UsagePeriodKind::Daily => day,
            UsagePeriodKind::Monthly => month,
        };
        let row:Option<(String,String,bool,u64)>=self.db.query_row("SELECT observed_growth,known_growth,incomplete,observations FROM usage_periods WHERE engagement_id=?1 AND granularity=?2 AND period_key=?3",params![engagement,kind.name(),key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        row.map(|(observed, known, incomplete, count)| {
            Ok(UsagePeriod {
                kind,
                key,
                observed_growth: serde_json::from_str(&observed)?,
                known_growth_lower_bound: serde_json::from_str(&known)?,
                incomplete,
                observations: count,
                evidence: UsageEvidence::HostAttributedUntrustedUsage,
            })
        })
        .transpose()
    }
    /// Read-side ceiling draw for one resource (slice 1 of the ceiling
    /// enforcement plan). This is the counterpart of the retained JavaScript
    /// `ceilingSpendFor` + `remainingFor` drawn rule (`backend-v2.js:14012-14060`):
    /// fresh tokens only (never cache reads), period-scoped, combined with
    /// commitments by `max`, with an unmeasured period falling back to the
    /// commitment figure rather than to zero.
    ///
    /// No enforcement decision is made here. The report is aggregate untrusted
    /// evidence for later slices and for operator inspection.
    pub fn resource_ceiling(&self, resource_id: &str, at: u64) -> Result<CeilingReport, Error> {
        ceiling_report(&self.db, resource_id, at)
    }
}

/// The `resource_ceiling` computation at `&Connection` level so the admission
/// transaction can fold the same draw it later publishes (slice 3).
pub(crate) fn ceiling_report(
    db: &Connection,
    resource_id: &str,
    at: u64,
) -> Result<CeilingReport, Error> {
    identifier(resource_id, 128)?;
    let resource = read_resource(db, resource_id)?;
    // Period granularity from the ceiling declaration, monthly unless the
    // preset says daily — mirroring backend-v2.js:14015. `Period` keeps the
    // JS missing-vs-null distinction private; serde reads the plain value.
    let period_value = resource
        .ceiling
        .as_ref()
        .map(|c| serde_json::to_value(&c.period))
        .transpose()?
        .unwrap_or(serde_json::Value::Null);
    let period = if period_value == serde_json::json!("daily") {
        UsagePeriodKind::Daily
    } else {
        UsagePeriodKind::Monthly
    };
    let ceiling_tokens = resource
        .ceiling
        .as_ref()
        .and_then(|c| c.tokens)
        .map(u64::from);
    let key = match period {
        UsagePeriodKind::Daily => period_keys(at)?.0,
        UsagePeriodKind::Monthly => period_keys(at)?.1,
    };
    // Commitments aggregated in SQLite like budget() (domain.rs:293): SUM
    // over the resource's holding engagements, never a store scan in Rust.
    let reserved: u64 = db.query_row(
            "SELECT COALESCE(SUM(tokens),0) FROM engagements WHERE resource_id=?1 AND state IN ('reserved','active')",
            params![resource_id],
            |r| r.get(0),
        )?;
    if reserved > JSON_SAFE_MAX {
        return Err(Error::Capacity);
    }
    // Fresh draw for the current period: the per-kind known-growth lower
    // bounds of every holding engagement's bucket, folded with the same
    // JSON-safe addition the summary uses. No bucket at all is unknown —
    // not zero — so the commitment figure stands alone (backend-v2.js:14050).
    let mut statement = db.prepare(
            "SELECT p.known_growth FROM usage_periods p JOIN engagements e ON e.id=p.engagement_id \
             WHERE e.resource_id=?1 AND e.state IN ('reserved','active') AND p.granularity=?2 AND p.period_key=?3",
        )?;
    let rows = statement.query_map(params![resource_id, period.name(), key], |r| {
        r.get::<_, String>(0)
    })?;
    let mut known = KnownTokens::default();
    let mut measured = false;
    for row in rows {
        let growth: KnownTokens = serde_json::from_str(&row?)?;
        known = known.adding(counts_from([
            Some(growth.input),
            Some(growth.output),
            Some(growth.cache_write),
            Some(growth.cache_read),
        ]))?;
        measured = true;
    }
    drop(statement);
    let (spent, consumed) = if measured {
        (
            Some(known.observed_fresh_lower_bound()?),
            Some(known.observed_display_lower_bound()?),
        )
    } else {
        (None, None)
    };
    // `max(reserved, spent)` with unknown-measurement fallback, mirroring
    // backend-v2.js:14052-14053 verbatim.
    let drawn = match spent {
        Some(spent) => reserved.max(spent),
        None => reserved,
    };
    if drawn > JSON_SAFE_MAX {
        return Err(Error::Capacity);
    }
    Ok(CeilingReport {
        period,
        reserved,
        spent,
        consumed,
        ceiling_tokens,
        preset_name: resource.preset_id,
        spend_period_key: measured.then_some(key),
        drawn,
        evidence: UsageEvidence::HostAttributedUntrustedUsage,
    })
}
