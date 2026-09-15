//! Ceiling refusal wording, ported byte-for-byte from the retained JavaScript
//! (`lib/engagement-store.js:58-103`). Pure formatting: no IO, no decision —
//! admission stays in the store, and slice 3 wires measured spend in.

use serde::{Deserialize, Serialize};

/// The spend facts an over-commit refusal names. Field names mirror the
/// retained JavaScript `spendContext` one-to-one. Every field is optional:
/// a caller without a ledger still gets a usable refusal, and an absent
/// measurement is never read as zero.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendContext {
    /// Ceiling granularity: `monthly` unless the preset says `daily`.
    pub period: Option<String>,
    /// Tokens committed to active engagements.
    pub reserved: Option<u64>,
    /// Fresh tokens measured this period; `None` when nobody measured.
    pub spent: Option<u64>,
    /// Display figure over all four kinds (cache reads included), same period.
    pub consumed: Option<u64>,
    /// Declared ceiling tokens.
    pub ceiling_tokens: Option<u64>,
    /// The preset an operator can raise.
    pub preset_name: Option<String>,
    /// The period key the measurement belongs to.
    pub spend_period_key: Option<String>,
}

/// `13609601` -> `13.6M`, so a refusal reads at a glance.
/// Byte-faithful to the retained `compactTokens`.
fn compact_tokens(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{}k", (value as f64 / 1_000.0).round() as u64)
    } else {
        value.to_string()
    }
}

/// Why this allocation was refused, in terms an operator can act on.
///
/// The order is deliberate (retained `overCommitMessage`): what is left, what
/// the limit was, then WHICH of the two competing draws is binding, then the
/// two things that change the answer — raise the preset, or wait for the
/// period. `ctx = None` yields the plain head: a caller with no ledger must
/// still get a usable error rather than a crash or a fabricated breakdown.
pub fn over_commit_message(
    agent: &str,
    alloc: u64,
    remaining: u64,
    ctx: Option<&SpendContext>,
) -> String {
    let head = format!(
        "allocating {} would exceed the {} left on {}",
        compact_tokens(alloc),
        compact_tokens(remaining),
        agent
    );
    let Some(ctx) = ctx else {
        return head;
    };
    let period = ctx.period.as_deref().unwrap_or("period");
    let mut parts: Vec<String> = Vec::new();
    if let Some(ceiling) = ctx.ceiling_tokens {
        parts.push(format!(
            "its ceiling is {} per {period}",
            compact_tokens(ceiling)
        ));
    }
    let committed = ctx.reserved;
    let measured = ctx.spent;
    if let (Some(committed), Some(measured)) = (committed, measured) {
        // Naming the binding one removes the guess: the draw takes the larger
        // of commitments and measured fresh tokens, and which one bound is
        // not inferable from the refusal alone.
        let binding = if measured > committed {
            "measured spend"
        } else {
            "committed allocations"
        };
        parts.push(format!(
            "{} is committed to active engagements and {} of fresh tokens was measured this {}{}, so {} is what is binding",
            compact_tokens(committed),
            compact_tokens(measured),
            period,
            ctx.spend_period_key
                .as_deref()
                .filter(|key| !key.is_empty())
                .map(|key| format!(" ({key})"))
                .unwrap_or_default(),
            binding
        ));
    } else if let Some(committed) = committed {
        parts.push(format!(
            "{} is committed to active engagements and nothing has been measured",
            compact_tokens(committed)
        ));
    }
    // The console shows a LARGER consumption figure than the one enforced
    // here, because cache reads are measured and reported but never draw the
    // ceiling. Left unexplained, that discrepancy looks like one of the two
    // numbers being wrong.
    if let (Some(consumed), Some(spent)) = (ctx.consumed, ctx.spent)
        && consumed > spent
    {
        parts.push(format!(
            "total consumption reads {} because cache reads are measured and shown but do not draw against a ceiling",
            compact_tokens(consumed)
        ));
    }
    let remedy = match ctx.preset_name.as_deref().filter(|name| !name.is_empty()) {
        Some(name) => {
            format!("raise the ceiling on preset \"{name}\" or wait for the {period} to roll over")
        }
        None => {
            format!("raise the agent's declared ceiling or wait for the {period} to roll over")
        }
    };
    format!("{head} — {}. To proceed, {remedy}.", parts.join("; "))
}
