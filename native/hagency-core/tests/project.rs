use hagency_core::project::{AgentDefinition, public_resource_id};
use serde_json::Value;

#[test]
fn project_identity_vectors_match_javascript() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../fixtures/project-identities.json")).unwrap();
    assert_eq!(
        public_resource_id(fixture["preset"].as_str().unwrap()),
        fixture["resourceId"]
    );
    for vector in fixture["vectors"].as_array().unwrap() {
        let result = serde_json::from_value::<AgentDefinition>(vector["definition"].clone())
            .map_err(|e| e.to_string())
            .and_then(|v| v.validate().map(|()| v).map_err(|e| e.to_string()));
        if vector["rejected"] == true {
            assert!(result.is_err(), "{vector}");
            continue;
        }
        let definition = result.unwrap();
        assert_eq!(
            serde_json::to_value(&definition).unwrap(),
            vector["normalized"]
        );
        let context = &fixture["context"];
        assert_eq!(
            definition
                .runtime_name(
                    context["fleetId"].as_str().unwrap(),
                    context["targetProjectId"].as_str().unwrap(),
                    context["requestId"].as_str().unwrap()
                )
                .unwrap(),
            vector["runtimeName"],
            "{vector}"
        );
    }
}
