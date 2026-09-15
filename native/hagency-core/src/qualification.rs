//! One embedded policy shared with the existing implementation, never a second
//! model-to-tier table. Model qualification does not prove executable readiness.
use crate::{InvalidInput, project::Resource};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{MapAccess, Visitor},
};
use std::{collections::BTreeMap, fmt, sync::LazyLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Lightweight,
    Medium,
    Strong,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelProfile {
    #[serde(default)]
    pub framework: String,
    #[serde(default)]
    pub model: String,
    pub provider: Option<String>,
    pub reasoning: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Role {
    default_tier: Tier,
    #[serde(default)]
    cross_family: bool,
}
#[derive(Debug, Deserialize)]
struct AcceptedModel {
    framework: String,
    model: String,
    provider: Option<String>,
    reasoning: Option<String>,
    family: Option<String>,
}
impl AcceptedModel {
    fn matches(&self, profile: &ModelProfile) -> bool {
        self.framework == profile.framework
            && self.model == profile.model
            && (self.provider.is_none()
                || profile.provider.as_deref().is_none_or(str::is_empty)
                || self.provider == profile.provider)
            && (self.reasoning.is_none()
                || self.reasoning.as_deref()
                    == profile.reasoning.as_deref().filter(|s| !s.is_empty()))
    }
}
#[derive(Debug, Deserialize)]
struct Exclusion {
    role: String,
    models: Vec<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Policy {
    tiers: Vec<Tier>,
    #[serde(deserialize_with = "ordered_roles")]
    roles: Vec<(String, Role)>,
    tier_accepts: BTreeMap<Tier, Vec<AcceptedModel>>,
    excluded: Vec<Exclusion>,
}
fn ordered_roles<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(String, Role)>, D::Error> {
    struct Roles;
    impl<'de> Visitor<'de> for Roles {
        type Value = Vec<(String, Role)>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("ordered role definitions")
        }
        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
            let mut roles = Vec::new();
            while let Some(entry) = map.next_entry()? {
                roles.push(entry);
            }
            Ok(roles)
        }
    }
    d.deserialize_map(Roles)
}
static POLICY: LazyLock<Policy> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../lib/role-capacity.json"))
        .expect("validated embedded role policy")
});

/// A selectable model/reasoning pair from the original embedded policy.
/// These roles describe model qualification, never live role or runtime readiness.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationChoice {
    pub model: String,
    pub reasoning: Option<String>,
    pub tier: Tier,
    pub roles: Vec<String>,
}
pub fn configuration_choices(
    profile: &ModelProfile,
) -> Result<Vec<ConfigurationChoice>, InvalidInput> {
    if !matches!(profile.framework.as_str(), "claude" | "codex") {
        return Ok(Vec::new());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut choices = Vec::new();
    for tier in &POLICY.tiers {
        for entry in POLICY.tier_accepts.get(tier).into_iter().flatten() {
            if entry.framework != profile.framework {
                continue;
            }
            let candidate = ModelProfile {
                framework: profile.framework.clone(),
                provider: profile.provider.clone(),
                model: entry.model.clone(),
                reasoning: entry.reasoning.clone(),
            };
            if !entry.matches(&candidate)
                || !seen.insert((candidate.model.clone(), candidate.reasoning.clone()))
            {
                continue;
            }
            if choices.len() == 256 {
                return Err(InvalidInput("configuration choices exceed capacity"));
            }
            let Some(tier) = model(&candidate).0 else {
                continue;
            };
            choices.push(ConfigurationChoice {
                roles: roles()
                    .filter(|role| qualifies(&candidate, role, None))
                    .map(str::to_owned)
                    .collect(),
                model: candidate.model,
                reasoning: candidate.reasoning,
                tier,
            });
        }
    }
    Ok(choices)
}

pub fn model(profile: &ModelProfile) -> (Option<Tier>, Option<&'static str>) {
    if profile.model.is_empty() {
        return (None, None);
    }
    for tier in &POLICY.tiers {
        if let Some(found) = POLICY
            .tier_accepts
            .get(tier)
            .and_then(|rows| rows.iter().find(|row| row.matches(profile)))
        {
            return (Some(*tier), found.family.as_deref());
        }
    }
    (None, None)
}
pub fn roles() -> impl Iterator<Item = &'static str> {
    POLICY.roles.iter().map(|(name, _)| name.as_str())
}
pub fn default_tier(role: &str) -> Option<Tier> {
    POLICY
        .roles
        .iter()
        .find(|(name, _)| name == role)
        .map(|(_, r)| r.default_tier)
}
pub fn cross_family(role: &str) -> bool {
    POLICY
        .roles
        .iter()
        .any(|(name, r)| name == role && r.cross_family)
}
pub fn check_role(role: &str) -> Result<(), InvalidInput> {
    default_tier(role)
        .map(|_| ())
        .ok_or(InvalidInput("unknown role"))
}
pub fn qualifies(profile: &ModelProfile, role: &str, requested: Option<Tier>) -> bool {
    let Some(needed) = requested.or_else(|| default_tier(role)) else {
        return false;
    };
    if !roles().any(|name| name == role) {
        return false;
    }
    model(profile).0.is_some_and(|got| got >= needed)
        && !POLICY
            .excluded
            .iter()
            .any(|entry| entry.role == role && entry.models.contains(&profile.model))
}
pub fn resources_for_role<'a>(
    resources: &'a [Resource],
    role: &str,
    tier: Option<Tier>,
) -> Vec<&'a Resource> {
    let mut result: Vec<_> = resources
        .iter()
        .filter(|r| {
            r.ceiling.as_ref().and_then(|c| c.tokens).is_some()
                && qualifies(&r.profile(), role, tier)
        })
        .collect();
    result.sort_by(|a, b| {
        model(&a.profile())
            .0
            .cmp(&model(&b.profile()).0)
            .then_with(|| a.preset_id.cmp(&b.preset_id))
    });
    result
}
