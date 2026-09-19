use crate::Error;
use hagency_core::JSON_SAFE_MAX;
use hagency_metering::TokenCounts;
use serde::{Deserialize, Serialize};

pub const MAX_USAGE_SOURCES: u64 = 4096;
pub const MAX_ENGAGEMENT_USAGE_SOURCES: u64 = 128;
pub const MAX_USAGE_RECEIPTS: u64 = 32768;
pub const MAX_SOURCE_USAGE_RECEIPTS: u64 = 256;
pub const MAX_USAGE_PERIODS: u64 = 32768;
pub const MAX_ENGAGEMENT_USAGE_PERIODS: u64 = 512;

/// Immutable historical host evidence, not provider measurement or runtime authority.
#[derive(Clone)]
pub struct UsageSource {
    pub(super) id: String,
    pub(super) identity_digest: String,
}
impl UsageSource {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub(crate) fn queue_value(&self) -> (&str, &str) {
        (&self.id, &self.identity_digest)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum UsagePeriodKind {
    Daily,
    Monthly,
}
impl UsagePeriodKind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Monthly => "monthly",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KnownTokens {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
}
impl KnownTokens {
    pub fn observed_display_lower_bound(&self) -> Result<u64, Error> {
        [self.input, self.output, self.cache_write, self.cache_read]
            .into_iter()
            .try_fold(0, add)
    }
    /// Arithmetic only: this is not an enforceable or provider-authenticated allowance.
    pub fn observed_fresh_lower_bound(&self) -> Result<u64, Error> {
        [self.input, self.output, self.cache_write]
            .into_iter()
            .try_fold(0, add)
    }
    pub(super) fn adding(self, counts: TokenCounts) -> Result<Self, Error> {
        let value = Self {
            input: add(self.input, counts.input.unwrap_or(0))?,
            output: add(self.output, counts.output.unwrap_or(0))?,
            cache_write: add(self.cache_write, counts.cache_write.unwrap_or(0))?,
            cache_read: add(self.cache_read, counts.cache_read.unwrap_or(0))?,
        };
        value.observed_display_lower_bound()?;
        Ok(value)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageReceipt {
    pub source_id: String,
    pub observed_at: u64,
    pub daily_key: String,
    pub monthly_key: String,
    pub delta: TokenCounts,
    pub regressed: bool,
    pub incomplete: bool,
    pub replayed: bool,
}
#[derive(Debug, Serialize)]
pub struct SourceUsage {
    pub source_id: String,
    pub framework: String,
    pub high_water: TokenCounts,
    pub latest_counts: Option<TokenCounts>,
    pub latest_observation: Option<serde_json::Value>,
    pub latest_incomplete: bool,
    pub latest_regressed: bool,
    pub historical_incomplete: bool,
    pub regressions: u64,
    pub observations: u64,
    pub observed_at: Option<u64>,
}
#[derive(Debug, Serialize)]
pub struct UsageSummary {
    pub sources: u64,
    /// Latest per-source observations can decrease and remain optional.
    pub latest_counts: Option<TokenCounts>,
    /// Historical observed lower bounds. A zero here is not complete usage proof.
    pub known_high_water_lower_bound: Option<KnownTokens>,
    pub latest_incomplete_sources: u64,
    pub historically_incomplete_sources: u64,
    pub regression_observations: u64,
    pub evidence: UsageEvidence,
}
/// Aggregate-only operator projection; source records and authority stay private.
#[derive(Debug, Serialize)]
pub struct UsageReport {
    pub engagement_id: String,
    pub at_ms: u64,
    pub summary: UsageSummary,
    pub daily: Option<UsagePeriod>,
    pub monthly: Option<UsagePeriod>,
    /// The authority's own ceiling figures for this engagement's resource
    /// (slice 3): what drew the ceiling down, the display total, and the
    /// headroom the admission decision itself uses — published so no client
    /// re-derives `ceiling - committed` and diverges the moment metering
    /// works (the console bug the retained JavaScript fixed).
    pub ceiling: UsageCeiling,
}
/// `tokensDrawn`/`tokensUsed`/`remainingTokens` equivalents
/// (`backend-v2.js:14012-14060`), snake_case like the rest of the report.
/// Evidence stays untrusted: these are lower bounds, never allowances.
#[derive(Debug, Serialize)]
pub struct UsageCeiling {
    /// `max(reserved, measured fresh)` with unknown falling back to reserved.
    pub tokens_drawn: u64,
    /// Display figure over all four kinds; `None` when nothing was measured.
    pub tokens_used: Option<u64>,
    /// min of the non-null limits (ceiling after draw, seat quota, pool).
    pub remaining_tokens: Option<u64>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageEvidence {
    HostAttributedUntrustedUsage,
}
/// Read-side projection of one resource's ceiling draw (slice 1 of the
/// ceiling-enforcement plan). Mirrors the retained JavaScript `ceilingSpendFor`
/// object exactly (`backend-v2.js:14012-14034`): what is committed, what fresh
/// tokens were measured this period, what was consumed in total, and the
/// context (ceiling, preset, period) a refusal or console must never re-derive.
///
/// Evidence stays untrusted: `spent`/`consumed` are lower bounds over host
/// observations and confer no allocation authority in this slice. An absent
/// period bucket is `None` — unknown, not zero — so the commitment figure
/// stands alone.
#[derive(Debug, Serialize)]
pub struct CeilingReport {
    /// Ceiling granularity in force; defaults to monthly like the JavaScript.
    pub period: UsagePeriodKind,
    /// Commitments of `reserved`/`active` engagements on this resource.
    pub reserved: u64,
    /// Fresh-token draw (input+output+cacheWrite) for the current period;
    /// `None` when no usage bucket exists.
    pub spent: Option<u64>,
    /// Display figure (all four kinds) for the same period; `None` with spent.
    pub consumed: Option<u64>,
    /// Declared ceiling tokens; `None` when the resource declares none.
    pub ceiling_tokens: Option<u64>,
    /// Preset identity of the resource, named so an operator can raise it.
    pub preset_name: String,
    /// Period key the measurement belongs to; `None` when unmeasured.
    pub spend_period_key: Option<String>,
    /// The figure a ceiling is drawn down by: `max(reserved, spent)`, falling
    /// back to `reserved` when measurement is unknown (`backend-v2.js:14052`).
    pub drawn: u64,
    pub evidence: UsageEvidence,
}
#[derive(Debug, Serialize)]
pub struct UsagePeriod {
    pub kind: UsagePeriodKind,
    pub key: String,
    pub observed_growth: TokenCounts,
    pub known_growth_lower_bound: KnownTokens,
    pub incomplete: bool,
    pub observations: u64,
    pub evidence: UsageEvidence,
}

pub(super) fn add(left: u64, right: u64) -> Result<u64, Error> {
    left.checked_add(right)
        .filter(|n| *n <= JSON_SAFE_MAX)
        .ok_or(Error::Capacity)
}
pub(super) fn unknown() -> TokenCounts {
    TokenCounts {
        input: None,
        output: None,
        cache_write: None,
        cache_read: None,
    }
}
pub(super) fn zero() -> TokenCounts {
    TokenCounts {
        input: Some(0),
        output: Some(0),
        cache_write: Some(0),
        cache_read: Some(0),
    }
}
pub(super) fn counts_array(c: TokenCounts) -> [Option<u64>; 4] {
    [c.input, c.output, c.cache_write, c.cache_read]
}
pub(super) fn counts_from(c: [Option<u64>; 4]) -> TokenCounts {
    TokenCounts {
        input: c[0],
        output: c[1],
        cache_write: c[2],
        cache_read: c[3],
    }
}
pub(super) fn optional_add(left: TokenCounts, right: TokenCounts) -> Result<TokenCounts, Error> {
    let mut values = [None; 4];
    for (i, (left, right)) in counts_array(left)
        .into_iter()
        .zip(counts_array(right))
        .enumerate()
    {
        values[i] = match (left, right) {
            (Some(a), Some(b)) => Some(add(a, b)?),
            _ => None,
        };
    }
    Ok(counts_from(values))
}
pub(super) fn grow(
    old: TokenCounts,
    next: Option<TokenCounts>,
) -> Result<(TokenCounts, TokenCounts, bool), Error> {
    let mut high = counts_array(old);
    let mut delta = [None; 4];
    let mut regressed = false;
    for (i, next) in counts_array(next.unwrap_or_else(unknown))
        .into_iter()
        .enumerate()
    {
        if let Some(next) = next {
            if next > JSON_SAFE_MAX {
                return Err(Error::Capacity);
            }
            let before = high[i].unwrap_or(0);
            regressed |= high[i].is_some_and(|old| next < old);
            high[i] = Some(before.max(next));
            delta[i] = Some(next.saturating_sub(before));
        }
    }
    let high = counts_from(high);
    KnownTokens::default().adding(high)?;
    Ok((high, counts_from(delta), regressed))
}
pub(super) fn period_keys(now: u64) -> Result<(String, String), Error> {
    let at = time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(now) * 1_000_000)
        .map_err(|_| hagency_core::InvalidInput("unsupported usage observation UTC clock"))?;
    // Keep keys sortable and unambiguous within the fixed four-digit year contract.
    if !(1970..=9999).contains(&at.year()) {
        return Err(hagency_core::InvalidInput("unsupported usage observation UTC clock").into());
    }
    let month = format!("{:04}-{:02}", at.year(), at.month() as u8);
    Ok((format!("{month}-{:02}", at.day()), month))
}
