//! Selected resource accounting (ADR-025). A projection is not an approval.
use crate::{InvalidInput, JSON_SAFE_MAX};
use serde::{Deserialize, Serialize};

/// Exact nonnegative token counts. Null/unknown is represented by Option<Tokens>.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct Tokens(u64);
impl TryFrom<u64> for Tokens {
    type Error = InvalidInput;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value > JSON_SAFE_MAX {
            Err(InvalidInput("token count exceeds JSON-safe range"))
        } else {
            Ok(Self(value))
        }
    }
}
impl From<Tokens> for u64 {
    fn from(value: Tokens) -> Self {
        value.0
    }
}
impl Tokens {
    fn add(self, other: Self) -> Result<Self, InvalidInput> {
        Self::try_from(
            self.0
                .checked_add(other.0)
                .ok_or(InvalidInput("token count overflow"))?,
        )
    }
    fn remaining(self, committed: Self) -> Self {
        Self(self.0.saturating_sub(committed.0))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input {
    pub preset: Preset,
    pub seat_id: String,
    pub declaration: Option<Declaration>,
    pub commitments: Vec<Commitment>,
    pub exclude_engagement_id: Option<String>,
    #[serde(default)]
    pub for_auto_join: bool,
}
#[derive(Debug, Deserialize)]
pub struct Preset {
    pub id: String,
    pub ceiling: Option<Ceiling>,
}
/// Undefined and explicit null compare differently in the current JS period guard.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Period(Option<Option<String>>);
impl<'de> Deserialize<'de> for Period {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(Some(Option::<String>::deserialize(deserializer)?)))
    }
}
impl Period {
    fn is_missing(&self) -> bool {
        self.0.is_none()
    }
    fn value(self) -> Option<String> {
        self.0.flatten()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ceiling {
    pub tokens: Option<Tokens>,
    #[serde(default, skip_serializing_if = "Period::is_missing")]
    pub period: Period,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Declaration {
    pub quota_tokens: Option<Tokens>,
    #[serde(default, skip_serializing_if = "Period::is_missing")]
    pub period: Period,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commitment {
    pub id: String,
    pub preset_id: Option<String>,
    pub seat_id: Option<String>,
    pub allocated_tokens: Option<Tokens>,
    pub state: String,
    pub fulfillment: Option<Fulfillment>,
}
#[derive(Debug, Deserialize)]
pub struct Fulfillment {
    pub phase: Option<String>,
}
impl Commitment {
    fn holds_allocation(&self) -> bool {
        self.state == "active"
            || self.state == "pending"
                && self
                    .fulfillment
                    .as_ref()
                    .is_some_and(|f| !matches!(f.phase.as_deref(), Some("failed" | "complete")))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Budget {
    pub scope: Scope,
    pub pool: PoolBudget,
    pub seat: SeatBudget,
    pub reserved: Tokens,
    pub remaining_tokens: Option<Tokens>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Resource,
}
#[derive(Debug, Serialize)]
pub struct PoolBudget {
    pub ceiling: Option<Tokens>,
    pub period: Option<String>,
    pub committed: Tokens,
    pub remaining: Option<Tokens>,
}
#[derive(Debug, Serialize)]
pub struct SeatBudget {
    pub quota: Option<Tokens>,
    pub period: Option<String>,
    pub committed: Tokens,
    pub remaining: Option<Tokens>,
    pub status: SeatStatus,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SeatStatus {
    Undeclared,
    Declared,
    PeriodMismatch,
}

pub fn resource_budget(input: &Input) -> Result<Budget, InvalidInput> {
    let mut pool_committed = Tokens::default();
    let mut seat_committed = Tokens::default();
    let mut reserved = Tokens::default();
    for row in &input.commitments {
        if !row.holds_allocation() {
            continue;
        }
        let tokens = row.allocated_tokens.unwrap_or_default();
        if input.exclude_engagement_id.as_ref() == Some(&row.id) {
            if row.preset_id.as_ref() == Some(&input.preset.id) {
                reserved = tokens;
            }
            continue;
        }
        if row.preset_id.as_ref() == Some(&input.preset.id) {
            pool_committed = pool_committed.add(tokens)?;
        }
        if row.seat_id.as_ref() == Some(&input.seat_id) {
            seat_committed = seat_committed.add(tokens)?;
        }
    }
    let ceiling = input.preset.ceiling.as_ref().and_then(|c| c.tokens);
    let pool_period = input
        .preset
        .ceiling
        .as_ref()
        .map(|c| c.period.clone())
        .unwrap_or_default();
    let quota = input.declaration.as_ref().and_then(|d| d.quota_tokens);
    let seat_period = input
        .declaration
        .as_ref()
        .map(|d| d.period.clone())
        .unwrap_or_default();
    let matching_period = seat_period == pool_period;
    let pool = PoolBudget {
        ceiling,
        period: pool_period.value(),
        committed: pool_committed,
        remaining: ceiling.map(|c| c.remaining(pool_committed)),
    };
    let seat = SeatBudget {
        quota,
        period: seat_period.value(),
        committed: seat_committed,
        remaining: quota
            .filter(|_| matching_period)
            .map(|q| q.remaining(seat_committed)),
        status: if quota.is_none() {
            SeatStatus::Undeclared
        } else if matching_period {
            SeatStatus::Declared
        } else {
            SeatStatus::PeriodMismatch
        },
    };
    let unknown = ceiling.is_none()
        || seat.status == SeatStatus::PeriodMismatch
        || input.for_auto_join && input.declaration.is_some() && seat.remaining.is_none();
    let remaining_tokens = if unknown {
        None
    } else {
        pool.remaining
            .map(|p| seat.remaining.map_or(p, |s| p.min(s)))
    };
    Ok(Budget {
        scope: Scope::Resource,
        pool,
        seat,
        reserved,
        remaining_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn allocation_vectors_match_javascript() {
        let vectors: Vec<Value> =
            serde_json::from_str(include_str!("../../fixtures/allocation.json")).unwrap();
        for vector in vectors {
            let input: Input = serde_json::from_value(vector["input"].clone()).unwrap();
            assert_eq!(
                serde_json::to_value(resource_budget(&input).unwrap()).unwrap(),
                vector["expected"],
                "{}",
                vector["name"]
            );
        }
    }
    #[test]
    fn allocation_rejects_unsafe_token_arithmetic() {
        for value in [json!(-1), json!(1.5), json!(JSON_SAFE_MAX + 1)] {
            assert!(serde_json::from_value::<Tokens>(value).is_err());
        }
        let input: Input = serde_json::from_value(json!({
            "preset":{"id":"pool", "ceiling":{"tokens":JSON_SAFE_MAX, "period":"monthly"}},
            "seatId":"shared", "commitments":[
                {"id":"a", "presetId":"pool", "seatId":"shared", "allocatedTokens":JSON_SAFE_MAX, "state":"active"},
                {"id":"b", "presetId":"pool", "seatId":"shared", "allocatedTokens":1, "state":"active"}
            ]
        })).unwrap();
        assert!(resource_budget(&input).is_err());
    }
}
