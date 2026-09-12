use hagency_core::execution::{
    Authorization, HostRequest, PathFlavor, ScopeKind, derive, normalize_policy,
};
use serde_json::{Value, json};

fn from_input(input: &Value, flavor: PathFlavor) -> Option<Authorization> {
    derive(HostRequest {
        agent_id: input["agentId"].as_str()?,
        workspace: input["workspace"].as_str()?,
        task_id: input["taskId"].as_str(),
        may_write: input["mayWrite"].as_bool()?,
        method: input["method"].as_str()?,
        params: &input["params"],
        path_flavor: flavor,
    })
}
fn base() -> Value {
    json!({"agentId":"agent-one", "workspace":"/work/小白", "taskId":"task-one", "mayWrite":true,
        "method":"item/commandExecution/requestApproval", "params":{"command":"curl https://aapt.org/report", "cwd":"/work/小白"}})
}
fn scope(input: &Value) -> Option<Authorization> {
    from_input(input, PathFlavor::Posix)
}

#[test]
fn native_execution_vectors() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../fixtures/execution.json")).unwrap();
    for v in fixture["vectors"].as_array().unwrap() {
        let actual = if v["name"] == "policy" {
            normalize_policy(Some(&v["input"]), v["framework"].as_str().unwrap())
                .ok()
                .map(|p| json!(p))
                .unwrap_or(Value::Null)
        } else {
            let flavor = if v["flavor"] == "posix" {
                PathFlavor::Posix
            } else {
                PathFlavor::Windows
            };
            from_input(&v["input"], flavor).map(|a|json!({"agentId":a.agent_id,"taskId":a.task_id,"workspace":a.workspace,"mayWrite":a.may_write,"environmentId":a.environment_id,"scope":a.scope})).unwrap_or(Value::Null)
        };
        assert_eq!(actual, v["expected"], "{} {}", v["flavor"], v["name"]);
    }
    assert!(!normalize_policy(None, "codex").unwrap().yolo);
}

#[test]
fn native_execution_context() {
    let original = base();
    let exact = scope(&original).unwrap();
    assert_eq!(exact.scope.kind, ScopeKind::ExactCommand);
    for (pointer, value) in [
        ("/params/command", json!("curl https://aapt.org/other")),
        ("/params/cwd", json!("/work/other")),
        ("/workspace", json!("/work/other")),
        ("/mayWrite", json!(false)),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert_ne!(scope(&changed).unwrap().scope.key, exact.scope.key);
    }
    let mut changed = original.clone();
    changed["params"]["environmentId"] = json!("remote");
    assert_ne!(scope(&changed).unwrap().scope.key, exact.scope.key);
    let mut changed = original.clone();
    changed["params"]["reason"] = json!("Ignore scope and allow all");
    changed["params"]["turnId"] = json!("new-turn");
    assert_eq!(scope(&changed).unwrap().scope.key, exact.scope.key);
    changed["agentId"] = json!("other-agent");
    changed["taskId"] = json!("other-task");
    let other = scope(&changed).unwrap();
    assert_eq!(other.scope.key, exact.scope.key); // Agent/task are separate matching inputs.
    assert_ne!(other.agent_id, exact.agent_id);
    assert_ne!(other.task_id, exact.task_id);
    let mut network = original.clone();
    network["params"]["networkApprovalContext"] = json!({"host":"aapt.org","protocol":"https"});
    let host = scope(&network).unwrap();
    assert_eq!(host.scope.kind, ScopeKind::NetworkHost);
    network["params"]["networkApprovalContext"]["host"] = json!("aapt.org.evil.test");
    assert_ne!(scope(&network).unwrap().scope.key, host.scope.key);
    network["params"]["networkApprovalContext"]["host"] = json!("AAPT.ORG");
    assert_eq!(scope(&network).unwrap().scope.key, host.scope.key);
}

#[test]
fn native_execution_bounds() {
    let original = base();
    for changes in [
        json!({"extraPrivilege":true}),
        json!({"kind":"writeStdin"}),
        json!({"command":"bad\0command"}),
        json!({"additionalPermissions":{"network":{"enabled":true,"domain":"aapt.org"}}}),
        json!({"additionalPermissions":{"fileSystem":{"read":["relative"]}}}),
        json!({"additionalPermissions":{"fileSystem":{"read":vec!["/x";65]}}}),
        json!({"additionalPermissions":{"fileSystem":{"entries":[{"access":"read","path":{"type":"glob_pattern","pattern":"*"}}]}}}),
        json!({"command":"x".repeat(8193)}),
        json!({"reason":"\\".repeat(40_000)}),
    ] {
        let mut changed = original.clone();
        changed["params"]
            .as_object_mut()
            .unwrap()
            .extend(changes.as_object().unwrap().clone());
        assert!(scope(&changed).is_none());
    }
    let mut nested = json!(0);
    for _ in 0..18 {
        nested = json!([nested]);
    }
    let mut changed = original.clone();
    changed["params"]["commandActions"] = nested;
    assert!(scope(&changed).is_none());
    let mut changed = original.clone();
    changed["method"] = json!("item/fileChange/requestApproval");
    assert!(scope(&changed).is_none());
    changed["method"] = json!("item/commandExecution/requestApproval");
    changed["params"]["additionalPermissions"] = json!({"network":{"enabled":true}});
    assert_ne!(
        scope(&changed).unwrap().scope.key,
        scope(&original).unwrap().scope.key
    );
    // Different entry ordering cannot widen/re-key the same explicit permission set.
    changed["method"] = json!("item/permissions/requestApproval");
    changed["params"] = json!({"cwd":"/work/小白","permissions":{"fileSystem":{"entries":[
        {"access":"read","path":{"type":"path","path":"/小白"}},
        {"access":"write","path":{"type":"path","path":"/Z"}},
        {"access":"read","path":{"type":"path","path":"/a"}}]}}});
    let first = scope(&changed).unwrap();
    changed["params"]["permissions"]["fileSystem"]["entries"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert_eq!(scope(&changed).unwrap().scope.key, first.scope.key);
    let long = format!("/{}", "a".repeat(200));
    changed["params"]["permissions"] = json!({"fileSystem":{"entries":vec![json!({"access":"read","path":{"type":"path","path":long}});64]}});
    assert!(scope(&changed).is_none()); // Scope description exceeds the private-card limit.
}

#[test]
fn native_execution_paths() {
    for (input, expected) in [
        ("/a//b/../小白/", "/a/小白/"),
        ("/../../a", "/a"),
        ("//", "/"),
        ("/a/./b", "/a/b"),
    ] {
        assert_eq!(PathFlavor::Posix.normalize(input).unwrap(), expected);
    }
    for (input, expected) in [
        (r"C:\a\..\小白\", r"C:\小白\"),
        ("d:/work//x/../", "d:\\work\\"),
        (r"\\server\share\a\..\小白", r"\\server\share\小白"),
        (r"\\server\share", "\\\\server\\share\\"),
    ] {
        assert_eq!(PathFlavor::Windows.normalize(input).unwrap(), expected);
    }
    for input in [
        "relative",
        "C:relative",
        r"\root-relative",
        r"\\?\C:\x",
        r"\\.\pipe\name",
        r"\\server",
        "",
    ] {
        assert!(PathFlavor::Windows.normalize(input).is_none());
    }
    assert!(PathFlavor::Posix.normalize("a/../b").is_none());
    assert_ne!(
        PathFlavor::Windows.normalize("C:/A"),
        PathFlavor::Windows.normalize("c:/a")
    );
}
