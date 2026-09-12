mod common;
use common::*;
use hagency_core::{
    authority::*,
    project::{EngagementState, Resource},
};
use hagency_store::{DomainRepository, EffectOutcome, Error};
use serde_json::json;

fn scoped_proof(request: &ProjectRequest, reg: &Registration) -> VerifiedRequest {
    let mut o = observation(request);
    let original = registration().representative_mxid;
    o.reception.joined.remove(&original);
    o.reception.joined.insert(reg.representative_mxid.clone());
    o.project.joined.remove(&original);
    o.project.joined.insert(reg.representative_mxid.clone());
    o.project.binding.as_mut().unwrap()["fleetId"] = json!(reg.fleet_id);
    verify_request(reg, request.clone(), o).unwrap()
}
fn activate(db: &mut DomainRepository, reg: &Registration, resource: &Resource, id: &str) {
    let mut r = request(id, id, resource, 10);
    r.fleet_id = reg.fleet_id.clone();
    let proof = scoped_proof(&r, reg);
    db.admit(&proof, 1000).unwrap();
    db.approve(id, &proof, 1000).unwrap();
    let effect = db.claim_effect().unwrap().unwrap();
    db.observe_effect(
        &effect.id,
        effect.fence,
        &EffectOutcome::Applied {
            receipt: format!("fixture_identity_{id}"),
        },
    )
    .unwrap();
}
fn review_available(db: &DomainRepository, fleet: &str) -> bool {
    db.catalog_for(Some(fleet), "", 100)
        .unwrap()
        .iter()
        .any(|r| r.roles.contains(&"review".to_string()))
}
#[test]
fn domain_qualification_and_cross_family() {
    let root = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    let reg = registration();
    db.register(&reg).unwrap();
    let mut other = reg.clone();
    other.fleet_id = format!("hf_{}", "b".repeat(32));
    other.representative_mxid = format!("@{}_representative:example.test", other.fleet_id);
    db.register(&other).unwrap();
    let mut gpt = resource("gpt", "gpt_seat", 1000);
    gpt.reasoning = Some("high".into());
    let mut claude = resource("claude", "claude_seat", 1000);
    claude.framework = "claude".into();
    claude.model = "claude-opus-5".into();
    claude.reasoning = None;
    db.put_resource(&gpt).unwrap();
    db.put_resource(&claude).unwrap();
    let draft = request("initial", "edison", &gpt, 10);
    let proof = proof(&draft);
    db.set_role_publication("coding", false).unwrap();
    assert!(matches!(db.admit(&proof, 1000), Err(Error::Unqualified)));
    db.set_role_publication("coding", true).unwrap();
    db.admit(&proof, 1000).unwrap();
    db.set_role_publication("coding", false).unwrap();
    assert_eq!(
        db.admit(&proof, 1000).unwrap().state,
        EngagementState::Pending
    );
    assert!(matches!(
        db.approve("deny_withdrawn", &proof, 1000),
        Err(Error::Unqualified)
    ));
    db.set_role_publication("coding", true).unwrap();
    // Unprovisioned configurations cannot establish a second active family.
    assert!(!review_available(&db, &reg.fleet_id));
    activate(&mut db, &reg, &gpt, "gpt_active");
    activate(&mut db, &other, &claude, "claude_other_registration");
    assert!(!review_available(&db, &reg.fleet_id));
    let mut review = request("review_request", "reviewer", &gpt, 10);
    review.role = "review".into();
    assert!(matches!(
        db.admit(&common::proof(&review), 1000),
        Err(Error::Unqualified)
    ));
    activate(&mut db, &reg, &claude, "claude_active");
    assert!(review_available(&db, &reg.fleet_id));
    let proof = common::proof(&review);
    db.admit(&proof, 1000).unwrap();
    let reserved = db.approve("review_approve", &proof, 1000).unwrap();
    db.set_role_publication("review", false).unwrap();
    assert!(!review_available(&db, &reg.fleet_id));
    assert_eq!(
        value(db.approve("review_approve", &proof, 1000).unwrap()),
        value(reserved)
    );
    db.set_role_publication("review", true).unwrap();
    let second = request("second_coding", "newcoding", &gpt, 10);
    db.admit(&common::proof(&second), 1000).unwrap();
    // Active model identities cannot change underneath reserved allocations.
    let mut moved = gpt.clone();
    moved.provider = Some("different".into());
    assert!(matches!(db.put_resource(&moved), Err(Error::State)));
    let mut low = resource("low", "low_seat", 1000);
    low.reasoning = Some("low".into());
    low.roles = vec!["architect".into()];
    db.put_resource(&low).unwrap();
    let mut bad = request("spoof", "architect", &low, 10);
    bad.role = "architect".into();
    assert!(matches!(
        db.admit(&common::proof(&bad), 1000),
        Err(Error::Unqualified)
    ));
    // A fresh pending model is rechecked, even if it qualified at request time.
    let mut medium = resource("pending", "pending_seat", 1000);
    db.put_resource(&medium).unwrap();
    let pending = request("pending", "pending", &medium, 10);
    db.admit(&common::proof(&pending), 1000).unwrap();
    medium.reasoning = Some("low".into());
    db.put_resource(&medium).unwrap();
    assert!(matches!(
        db.approve("cannot_promote", &common::proof(&pending), 1000),
        Err(Error::Unqualified)
    ));
    let claude_id = request("claude_active", "claude_active", &claude, 10)
        .engagement_id()
        .unwrap();
    db.revoke("remove_family", &claude_id).unwrap();
    assert!(!review_available(&db, &reg.fleet_id));
}
