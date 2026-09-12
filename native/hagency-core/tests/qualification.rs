use hagency_core::{
    project::Resource,
    qualification::{self, ModelProfile, Tier},
};
use serde_json::{Value, json};

#[test]
fn qualification_matches_javascript() {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../fixtures/qualification.json")).unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        let profile: ModelProfile = if case["profile"].is_null() {
            ModelProfile::default()
        } else {
            serde_json::from_value(case["profile"].clone()).unwrap()
        };
        let (tier, family) = qualification::model(&profile);
        assert_eq!(json!(tier), case["tier"], "{case}");
        assert_eq!(json!(family), case["family"], "{case}");
        for (role, expected) in case["roles"].as_object().unwrap() {
            assert_eq!(
                json!(qualification::qualifies(&profile, role, None)),
                expected["default"],
                "{role}: {case}"
            );
            assert_eq!(
                json!(qualification::qualifies(
                    &profile,
                    role,
                    Some(Tier::Lightweight)
                )),
                expected["lightweight"],
                "{role}: {case}"
            );
        }
    }
    let presets: Vec<Resource> = fixtures["presets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| {
            let mut value = value.clone();
            let id = value.as_object_mut().unwrap().remove("id").unwrap();
            value["presetId"] = id;
            value["seatId"] = json!("fixture");
            serde_json::from_value(value).unwrap()
        })
        .collect();
    for rank in fixtures["rankings"].as_array().unwrap() {
        let rows = qualification::resources_for_role(
            &presets,
            rank["role"].as_str().unwrap(),
            Some(serde_json::from_value(rank["tier"].clone()).unwrap()),
        );
        assert_eq!(
            json!(rows.iter().map(|r| &r.preset_id).collect::<Vec<_>>()),
            rank["ids"]
        );
    }
}

#[test]
fn native_resource_configuration_choices() {
    use hagency_core::qualification::{self, ModelProfile};
    for framework in ["codex", "claude", "unknown"] {
        for provider in [None, Some("openai"), Some("wrong-provider")] {
            let profile = ModelProfile {
                framework: framework.into(),
                provider: provider.map(str::to_owned),
                ..ModelProfile::default()
            };
            let choices = qualification::configuration_choices(&profile).unwrap();
            assert!(choices.len() <= 256);
            let mut unique = std::collections::BTreeSet::new();
            for c in &choices {
                assert!(unique.insert((&c.model, &c.reasoning)));
                let candidate = ModelProfile {
                    model: c.model.clone(),
                    reasoning: c.reasoning.clone(),
                    ..profile.clone()
                };
                assert_eq!(qualification::model(&candidate).0, Some(c.tier));
                assert_eq!(
                    c.roles,
                    qualification::roles()
                        .filter(|r| qualification::qualifies(&candidate, r, None))
                        .collect::<Vec<_>>()
                );
            }
            if framework == "unknown" {
                assert!(choices.is_empty());
            }
            if framework == "codex" && provider.is_none() {
                assert!(
                    choices
                        .iter()
                        .any(|c| c.model == "gpt-5.6-sol"
                            && c.reasoning.as_deref() == Some("medium"))
                );
            }
        }
    }
}
