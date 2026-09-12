use hagency_core::project::Resource;
use hagency_store::{
    DomainRepository, Error, ResourcePublicationAccess, ResourcePublicationRetirement,
    resource_publication_revision,
};
use serde_json::json;
use std::time::{Duration, Instant};

fn resource() -> Resource {
    serde_json::from_value(json!({"presetId":"private_preset","seatId":"private_account","framework":"codex","model":"gpt-5.6-sol","reasoning":"medium","ceiling":{"tokens":1000,"period":"monthly"}})).unwrap()
}
#[test]
fn native_resource_publication_cas() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut db = DomainRepository::open(&state).unwrap();
    let original = resource();
    db.put_resource(&original).unwrap();
    let fence = ResourcePublicationRetirement::default();
    let access = ResourcePublicationAccess::new(Instant::now() + Duration::from_secs(60), fence);
    let revision = resource_publication_revision(&original).unwrap();
    let command = access
        .prepare(
            original.id(),
            revision.clone(),
            false,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    assert!(matches!(access.revoke(), Err(Error::Busy)));
    let result = db.publish_resource(command).unwrap();
    assert!(!result.published);
    let current = db.resource_configurations("", 16).unwrap().remove(0).config;
    let mut expected = original.clone();
    expected.published = false;
    expected.roles = expected.eligible_roles();
    assert_eq!(
        serde_json::to_value(&current).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    assert!(db.catalog("", 16).unwrap().is_empty());
    let stale = access
        .prepare(
            original.id(),
            revision,
            true,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    assert!(matches!(db.publish_resource(stale), Err(Error::Conflict)));
    let before_edit = resource_publication_revision(&current).unwrap();
    let mut edited = current.clone();
    edited.ceiling.as_mut().unwrap().tokens = Some(2000.try_into().unwrap());
    db.edit_resource(&edited, None).unwrap();
    let stale = access
        .prepare(
            original.id(),
            before_edit,
            true,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    assert!(matches!(db.publish_resource(stale), Err(Error::Conflict)));
    assert_eq!(
        resource_publication_revision(&db.resource_configurations("", 16).unwrap()[0].config)
            .unwrap(),
        resource_publication_revision(&edited).unwrap()
    );
    let command = access
        .prepare(
            original.id(),
            resource_publication_revision(&edited).unwrap(),
            true,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    db.publish_resource(command).unwrap();
    access.revoke().unwrap();
    assert!(matches!(
        access.prepare(
            original.id(),
            result.revision,
            false,
            Instant::now() + Duration::from_secs(2)
        ),
        Err(Error::LocalAuthority)
    ));
    drop(db);
    let reopened = DomainRepository::open(&state).unwrap();
    assert!(
        reopened.resource_configurations("", 16).unwrap()[0]
            .config
            .published
    );
    assert_eq!(reopened.catalog("", 16).unwrap().len(), 1);
}
