mod common;
use common::*;
use hagency_core::{authority::*, canonical, project::Resource};
use hagency_store::{DomainRepository, EffectOutcome, Error, outbound::RegistrationIdentity};
use serde_json::{Value, json};

fn identity(reg: &Registration) -> RegistrationIdentity {
    RegistrationIdentity {
        binding: "original_catalog".into(),
        side_id: reg.server_name.clone(),
        fleet_id: reg.fleet_id.clone(),
        registration_generation: reg.generation,
        registration_fingerprint: canonical::digest(&json!(reg)).unwrap(),
    }
}
fn catalog(db: &DomainRepository, reg: &Registration) -> Value {
    db.published_catalog(&identity(reg)).unwrap().into_update()
}
fn has_role(value: &Value, role: &str) -> bool {
    value["capabilities"]["offers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["role"] == role)
}
fn activate(db: &mut DomainRepository, reg: &Registration, resource: &Resource, id: &str) {
    let mut req = request(id, id, resource, 10);
    req.fleet_id = reg.fleet_id.clone();
    let mut observed = observation(&req);
    let old = registration().representative_mxid;
    observed.reception.joined.remove(&old);
    observed.project.joined.remove(&old);
    observed
        .reception
        .joined
        .insert(reg.representative_mxid.clone());
    observed
        .project
        .joined
        .insert(reg.representative_mxid.clone());
    observed.project.binding.as_mut().unwrap()["fleetId"] = json!(reg.fleet_id);
    let proof = verify_request(reg, req, observed).unwrap();
    db.admit(&proof, 1000).unwrap();
    db.approve(id, &proof, 1000).unwrap();
    let effect = db.claim_effect().unwrap().unwrap();
    db.observe_effect(
        &effect.id,
        effect.fence,
        &EffectOutcome::Applied {
            receipt: format!("fixture_{id}"),
        },
    )
    .unwrap();
}

#[test]
fn native_catalog_snapshot_scope_and_roles() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&dir.path().join("domain")).unwrap();
    let reg = registration();
    db.register(&reg).unwrap();
    assert_eq!(catalog(&db, &reg)["capabilities"]["offers"], json!([]));
    let mut other = reg.clone();
    other.fleet_id = format!("hf_{}", "b".repeat(32));
    other.representative_mxid = format!("@{}_representative:example.test", other.fleet_id);
    db.register(&other).unwrap();
    let mut gpt = resource("private_preset", "private_seat", 1000);
    gpt.reasoning = Some("high".into());
    let mut claude = resource("private_claude", "private_account", 1000);
    claude.framework = "claude".into();
    claude.model = "claude-opus-5".into();
    claude.reasoning = None;
    db.put_resource(&gpt).unwrap();
    db.put_resource(&claude).unwrap();
    let before = catalog(&db, &reg);
    assert!(has_role(&before, "coding"));
    assert!(!has_role(&before, "review"));
    activate(&mut db, &reg, &gpt, "gpt_active");
    activate(&mut db, &other, &claude, "claude_other");
    assert!(!has_role(&catalog(&db, &reg), "review"));
    activate(&mut db, &reg, &claude, "claude_same");
    let current = catalog(&db, &reg);
    assert!(has_role(&current, "review"));
    let cap = &current["capabilities"];
    assert_eq!(cap["fleetId"], reg.fleet_id);
    assert_eq!(cap["serverName"], reg.server_name);
    assert_eq!(cap["representativeMxid"], reg.representative_mxid);
    assert_eq!(cap["approvalBotMxid"], reg.approval_bot_mxid);
    for offer in cap["offers"].as_array().unwrap() {
        for r in offer["resources"].as_array().unwrap() {
            assert_eq!(
                r.as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                ["framework", "id", "model", "name", "reasoning"]
            );
            assert!(r["id"] == gpt.id() || r["id"] == claude.id());
        }
    }
    for private in [
        "private_preset",
        "private_seat",
        "private_account",
        "ownerDmRoomId",
        "receptionRoomId",
        "ceiling",
        "provider",
    ] {
        assert!(!current.to_string().contains(private));
    }
    db.set_role_publication("coding", false).unwrap();
    assert!(!has_role(&catalog(&db, &reg), "coding"));
    for r in [&mut gpt, &mut claude] {
        r.published = false;
        db.put_resource(r).unwrap();
    }
    assert_eq!(catalog(&db, &reg)["capabilities"]["offers"], json!([]));
    let mut wrong = identity(&reg);
    wrong.registration_fingerprint = "0".repeat(64);
    assert!(matches!(
        db.published_catalog(&wrong),
        Err(Error::Generation)
    ));
    let mut rotated = reg.clone();
    rotated.generation += 1;
    db.register(&rotated).unwrap();
    assert!(matches!(
        db.check_publication_registration(&identity(&reg)),
        Err(Error::Generation)
    ));
    assert_eq!(catalog(&db, &rotated)["capabilities"]["offers"], json!([]));
}

#[test]
fn native_catalog_snapshot_capacity() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&dir.path().join("domain")).unwrap();
    let reg = registration();
    db.register(&reg).unwrap();
    for n in 0..200 {
        db.put_resource(&resource(&format!("preset_{n}"), "same_seat", 1000))
            .unwrap();
    }
    let complete = catalog(&db, &reg);
    for offer in complete["capabilities"]["offers"].as_array().unwrap() {
        assert_eq!(offer["resources"].as_array().unwrap().len(), 200);
        let ids: std::collections::BTreeSet<_> = offer["resources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids.len(), 200);
    }
    let mut excess = resource("excess", "same_seat", 1000);
    db.put_resource(&excess).unwrap();
    assert!(matches!(
        db.published_catalog(&identity(&reg)),
        Err(Error::Capacity)
    ));
    excess.published = false;
    db.put_resource(&excess).unwrap();
    assert_eq!(catalog(&db, &reg), complete);
    // A supported Claude profile accepts optional reasoning independently of
    // qualification, so this exercises the actual retained peer field bound.
    let mut bounded = resource("preset_0", "same_seat", 1000);
    bounded.framework = "claude".into();
    bounded.model = "claude-opus-5".into();
    bounded.reasoning = Some("x".repeat(64));
    db.put_resource(&bounded).unwrap();
    assert!(db.published_catalog(&identity(&reg)).is_ok());
    bounded.reasoning = Some("x".repeat(65));
    db.put_resource(&bounded).unwrap();
    assert!(matches!(
        db.published_catalog(&identity(&reg)),
        Err(Error::Capacity)
    ));
}

/// G5 (ADR-108 amendment): the three derived catalogue values — families,
/// fillable, overTier — come from ONE predicate (`Resource::qualifies`,
/// the same set that produces `available`), and `families` is the MODEL
/// family from `qualification::model()`, never the framework. Seeded: the
/// gpt-strong codex preset qualifies everywhere; an octos/kimi-k3 preset
/// (real family, non-provisionable) counts nowhere; a codex model with no
/// policy tier counts nowhere and is never over-tier.
#[test]
fn catalogue_derived_keys_use_one_predicate_and_model_families() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&dir.path().join("state")).unwrap();
    db.put_resource(&resource("catalogue_gpt", "catalogue_seat", 1000))
        .unwrap();
    db.put_resource(
        &serde_json::from_value(json!({
            "presetId":"catalogue_kimi","seatId":"catalogue_seat","framework":"octos",
            "model":"kimi-k3","ceiling":{"tokens":1000,"period":"monthly"}}))
        .unwrap(),
    )
    .unwrap();
    db.put_resource(
        &serde_json::from_value(json!({
            "presetId":"catalogue_mystery","seatId":"catalogue_seat","framework":"codex",
            "model":"mystery_model_v9","ceiling":{"tokens":1000,"period":"monthly"}}))
        .unwrap(),
    )
    .unwrap();
    let roles = db.role_publications().unwrap();
    assert_eq!(roles.len(), 6, "one row per policy role");
    for row in &roles {
        let role = row["role"].as_str().unwrap();
        assert_eq!(
            row["fillable"], 1,
            "{role}: only the tiered, provisionable preset counts"
        );
        let families = row["families"].as_array().unwrap();
        assert_eq!(families.len(), 1, "{role}: one family");
        assert_eq!(
            families[0], "gpt",
            "{role}: the model family, never the framework"
        );
        assert_ne!(families[0], "codex");
        assert_ne!(families[0], "octos");
        assert!(
            if row["crossFamily"].as_bool().unwrap() {
                !row["available"].as_bool().unwrap()
            } else {
                row["available"].as_bool().unwrap()
            },
            "{role}: available agrees with the predicate — cross-family roles additionally need two families among active engagements, and this store has none"
        );
        // strong default: gpt-strong is not above; medium/lightweight: it is.
        let over = matches!(role, "coding" | "testing" | "integration" | "documentation");
        assert_eq!(
            row["overTier"],
            json!(u64::from(over)),
            "{role}: strictly stronger only"
        );
    }
    // The key set is exactly eight — deny_unknown_fields downstream makes
    // any ninth key a schema fault, so pin the count at the source.
    let keys = roles[0].as_object().unwrap();
    assert_eq!(
        keys.len(),
        8,
        "role, explicitPublication, available, crossFamily, defaultTier, families, fillable, overTier"
    );
}
