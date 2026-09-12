mod common;
use common::*;
use hagency_core::authority::*;
use hagency_store::{DomainRepository, Error};
use serde_json::json;

#[test]
fn project_authority_fails_closed() {
    let resource = resource("preset", "seat", 100);
    let request = request("req", "小白", &resource, 10);
    let original = observation(&request);
    type Mutation = Box<dyn Fn(&mut RequestObservation)>;
    let mutations: Vec<Mutation> = vec![
        Box::new(|o| o.source.sender = "@owner:impostor.test".into()),
        Box::new(|o| o.source.room_id = "!elsewhere:example.test".into()),
        Box::new(|o| o.source.event_id = "$different".into()),
        Box::new(|o| o.source.content["requestedTokens"] = json!(11)),
        Box::new(|o| {
            o.source.content["agentDefinition"]["resourceId"] =
                json!("resource_000000000000000000000000")
        }),
        Box::new(|o| o.reception.joined.clear()),
        Box::new(|o| o.reception.encryption = Some("m.megolm.v1.aes-sha2".into())),
        Box::new(|o| o.project.invite_only = false),
        Box::new(|o| {
            o.project.joined.remove("@owner:example.test");
        }),
        Box::new(|o| {
            o.project.powers.insert("@owner:example.test".into(), 99);
        }),
        Box::new(|o| {
            o.project.binding.as_mut().unwrap()["ownerMxid"] = json!("@owner:impostor.test");
        }),
        Box::new(|o| {
            o.project.binding.as_mut().unwrap()["projectId"] = json!("other_project");
        }),
        Box::new(|o| {
            o.owner_room.joined.insert("@observer:example.test".into());
        }),
        Box::new(|o| o.owner_room.encryption = None),
        Box::new(|o| o.owner_room.room_id = "!project:example.test".into()),
        Box::new(|o| o.registration_generation = 2),
    ];
    for (index, mutate) in mutations.into_iter().enumerate() {
        let mut changed = original.clone();
        mutate(&mut changed);
        assert!(
            verify_request(&registration(), request.clone(), changed).is_err(),
            "mutation {index}"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&dir.path().join("state")).unwrap();
    db.register(&registration()).unwrap();
    db.put_resource(&resource).unwrap();
    let proof = proof(&request);
    assert!(db.admit(&proof, 999).is_err());
    assert!(db.admit(&proof, 31_001).is_err());
    db.admit(&proof, 1000).unwrap();
    let mut remapped = request.clone();
    remapped.request_id = "remapped".into();
    remapped.source_event_id = "$remapped".into();
    remapped.target_room_id = "!different:example.test".into();
    // Even a fully self-consistent fresh room snapshot cannot remap a stored project ID.
    assert!(matches!(
        db.admit(&common::proof(&remapped), 1000),
        Err(Error::Generation)
    ));
    let inspection = rusqlite::Connection::open(dir.path().join("state/domain.sqlite3")).unwrap();
    let evidence: String = inspection
        .query_row("SELECT evidence FROM engagements", [], |r| r.get(0))
        .unwrap();
    let evidence: serde_json::Value = serde_json::from_str(&evidence).unwrap();
    assert_eq!(evidence["source"]["sender"], "@owner:example.test");
    assert_eq!(evidence["owner_room"]["encryption"], "m.megolm.v1.aes-sha2");
    drop(inspection);
    let mut newer = registration();
    newer.generation = 2;
    db.register(&newer).unwrap();
    assert!(matches!(
        db.approve("approve", &proof, 1000),
        Err(Error::Generation)
    ));
    assert_eq!(
        db.get(&request.engagement_id().unwrap()).unwrap().state,
        hagency_core::project::EngagementState::Pending
    );
}
