use hagency_core::graphs::*;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Fixtures {
    conditions: Vec<ConditionCase>,
    transitions: Vec<TransitionCase>,
}
#[derive(Deserialize)]
struct ConditionCase {
    name: String,
    node: NodeDefinition,
    progress: BTreeMap<String, NodeProgress>,
    expected: Option<bool>,
}
#[derive(Deserialize)]
struct TransitionCase {
    name: String,
    graph: Graph,
    cancel: bool,
    expected: Value,
}
fn fixtures() -> Fixtures {
    serde_json::from_str(include_str!("../../fixtures/graphs.json")).unwrap()
}
fn value(v: impl serde::Serialize) -> Value {
    serde_json::to_value(v).unwrap()
}

#[test]
fn native_graph_condition_vectors() {
    let f = fixtures();
    assert!(f.conditions.len() >= 100);
    for c in f.conditions {
        assert_eq!(
            evaluate_condition(&c.node, &c.progress),
            c.expected,
            "{}",
            c.name
        );
    }
}
#[test]
fn native_graph_transition_vectors() {
    for c in fixtures().transitions {
        let original = value(&c.graph);
        let (next, assignments) = if c.cancel {
            (c.graph.cancel().unwrap(), vec![])
        } else {
            let t = c.graph.advance().unwrap();
            (t.graph, t.assignments)
        };
        assert_eq!(
            json!({"status":next.status,"progress":next.progress,"assignments":assignments}),
            c.expected,
            "{}",
            c.name
        );
        assert_eq!(value(&c.graph), original); // A proposal never commits itself.
        let replay = next.advance().unwrap();
        assert!(replay.assignments.is_empty());
        if !c.cancel {
            let references = c.graph.advance_references().unwrap();
            let mut expected = c.expected.clone();
            for assignment in expected["assignments"].as_array_mut().unwrap() {
                for dependency in assignment["dependency_results"].as_array_mut().unwrap() {
                    dependency["result"] = Value::Null;
                }
            }
            assert_eq!(
                json!({"status":references.graph.status,"progress":references.graph.progress,"assignments":references.assignments}),
                expected,
                "reference planning: {}",
                c.name
            );
        }
    }
}

#[test]
fn native_graph_dependency_outcomes_references_large_results() {
    let mut nodes = vec![
        json!({"id":"source","assignee":"小白","description":"Produce a large result"}),
        json!({"id":"other","assignee":"Edison","description":"Produce a null result"}),
    ];
    for index in 0..16 {
        nodes.push(json!({
            "id":format!("consumer_{index}"), "assignee":"Reviewer", "description":"Read dependencies",
            "depends_on":["other","source"],
            "condition":{"dep":"source","path":"score","eq":0.25}
        }));
    }
    nodes.push(json!({
        "id":"skipped", "assignee":"Reviewer", "description":"Condition is false",
        "depends_on":["source"], "condition":{"path":"score","eq":0.5}
    }));
    let definition =
        serde_json::from_value(json!({"label":"Bounded fan-out","nodes":nodes})).unwrap();
    let initial = Graph::new(definition).unwrap().advance().unwrap();
    assert_eq!(initial.assignments.len(), 2);
    let large_result = json!({"score":0.25,"summary":"中".repeat(15_000)});
    validate_result(&large_result).unwrap();
    let ready = initial
        .graph
        .observe(
            "source",
            &NodeObservation::Complete {
                result: large_result.clone(),
            },
        )
        .unwrap()
        .observe(
            "other",
            &NodeObservation::Complete {
                result: Value::Null,
            },
        )
        .unwrap();
    let original = value(&ready);
    let full = ready.advance().unwrap();
    let references = ready.advance_references().unwrap();
    assert_eq!(value(&ready), original);
    assert_eq!(value(&references.graph), value(&full.graph));
    assert_eq!(references.graph.progress["source"].result, large_result);
    assert_eq!(
        references.graph.progress["skipped"].status,
        NodeStatus::Skipped
    );
    assert_eq!(references.assignments.len(), 16);
    for (index, assignment) in references.assignments.iter().enumerate() {
        assert_eq!(assignment.node_id, format!("consumer_{index}"));
        assert_eq!(assignment.dependency_results.len(), 2);
        assert_eq!(assignment.dependency_results[0].node_id, "other");
        assert_eq!(assignment.dependency_results[0].assignee, "Edison");
        assert_eq!(assignment.dependency_results[1].node_id, "source");
        assert_eq!(assignment.dependency_results[1].assignee, "小白");
        assert!(
            assignment
                .dependency_results
                .iter()
                .all(|d| d.result.is_null())
        );
        assert_eq!(
            full.assignments[index].dependency_results[1].result,
            large_result
        );
    }
    // The real result remains available to condition evaluation, while dispatch
    // metadata stays small even when many consumers depend on the same value.
    assert!(serde_json::to_vec(&full.assignments).unwrap().len() > 700_000);
    assert!(serde_json::to_vec(&references.assignments).unwrap().len() < 8_000);
    let restored: Graph = serde_json::from_value(value(&references.graph)).unwrap();
    restored.validate().unwrap();
    assert!(
        restored
            .advance_references()
            .unwrap()
            .assignments
            .is_empty()
    );
}

#[test]
fn native_graph_dependency_outcomes_wide_utf8_failures() {
    let ids: Vec<_> = (0..64)
        .map(|i| format!("{i:02}-{}", "界".repeat(80)))
        .collect();
    let mut nodes: Vec<_> = ids
        .iter()
        .map(|id| {
            json!({
                "id":id,"assignee":"Worker","description":"Independent work"
            })
        })
        .collect();
    nodes.push(json!({
        "id":"join","assignee":"Reviewer","description":"Needs all workers","depends_on":ids
    }));
    nodes.push(json!({
        "id":"final","assignee":"Reviewer","description":"Needs the join","depends_on":["join"]
    }));
    let definition = serde_json::from_value(json!({"label":"Wide failure","nodes":nodes})).unwrap();
    let initial = Graph::new(definition).unwrap().advance().unwrap();
    assert_eq!(initial.assignments.len(), ids.len());
    let mut graph = initial.graph;
    for id in &ids {
        graph = graph
            .observe(
                id,
                &NodeObservation::Failed {
                    error: "Worker blocked".into(),
                },
            )
            .unwrap();
    }
    let original = value(&graph);
    let transition = graph.advance_references().unwrap();
    assert_eq!(value(&graph), original);
    assert!(transition.assignments.is_empty());
    assert_eq!(transition.graph.status, GraphStatus::Failed);
    assert_eq!(transition.graph.progress["join"].status, NodeStatus::Failed);
    assert_eq!(
        transition.graph.progress["final"].status,
        NodeStatus::Failed
    );
    let error = transition.graph.progress["join"].error.as_ref().unwrap();
    let unbounded = format!("dependency failed: {}", ids.join(", "));
    assert!(unbounded.len() > 4_000);
    assert!((3_998..=4_000).contains(&error.len()));
    assert!(error.ends_with("..."));
    assert!(unbounded.starts_with(error.strip_suffix("...").unwrap()));
    assert_eq!(
        transition.graph.progress["final"].error.as_deref(),
        Some("dependency failed: join")
    );
    // Persisted transitions must remain valid input to subsequent planning.
    // An oversized propagated error used to make this re-read fail validation.
    let restored: Graph =
        serde_json::from_str(&serde_json::to_string(&transition.graph).unwrap()).unwrap();
    restored.validate().unwrap();
    let replay = restored.advance().unwrap();
    assert_eq!(value(&replay.graph), value(&transition.graph));
    assert!(replay.assignments.is_empty());
}
#[test]
fn native_graph_validation() {
    trait Ambiguous<A> {
        fn check() {}
    }
    impl<T: ?Sized> Ambiguous<()> for T {}
    impl<T: serde::de::DeserializeOwned> Ambiguous<u8> for T {}
    let _ = <NodeObservation as Ambiguous<_>>::check;
    let definition:GraphDefinition=serde_json::from_value(json!({"label":"测试","nodes":[{"id":"one","assignee":"小白","description":"Work"},{"id":"two","assignee":"Edison","description":"Review","depends_on":["one"]}]})).unwrap();
    let graph = Graph::new(definition.clone()).unwrap();
    assert!(
        graph
            .observe(
                "one",
                &NodeObservation::Complete {
                    result: json!(0.25)
                }
            )
            .is_err()
    );
    let first = graph.advance().unwrap();
    assert_eq!(first.assignments.len(), 1);
    let observed = first
        .graph
        .observe(
            "one",
            &NodeObservation::Complete {
                result: json!({"score":0.25}),
            },
        )
        .unwrap();
    let second = observed.advance().unwrap();
    assert_eq!(
        second.assignments[0].dependency_results[0].result["score"],
        0.25
    );
    assert!(observed.observe("one", &NodeObservation::Active).is_err());
    for case in [
        "empty",
        "duplicate",
        "missing",
        "self",
        "cycle",
        "condition_cycle",
        "too_many",
        "unknown_condition",
    ] {
        let mut d = definition.clone();
        match case {
            "empty" => d.nodes.clear(),
            "duplicate" => d.nodes[1].id = "one".into(),
            "missing" => d.nodes[1].depends_on = vec!["absent".into()],
            "self" => d.nodes[0].depends_on = vec!["one".into()],
            "cycle" => d.nodes[0].depends_on = vec!["two".into()],
            "condition_cycle" => {
                d.nodes[0].condition = Some(serde_json::from_value(json!({"dep":"two"})).unwrap())
            }
            "too_many" => d.nodes = vec![d.nodes[0].clone(); 129],
            _ => {
                d.nodes[0].condition =
                    Some(serde_json::from_value(json!({"execute":"command"})).unwrap())
            }
        }
        assert!(Graph::new(d).is_err(), "{case}");
    }
    let mut forged = value(&definition);
    forged["owner"] = json!("operator");
    assert!(serde_json::from_value::<GraphDefinition>(forged).is_err());
    let mut forged = value(&definition);
    forged["nodes"][0]["status"] = json!("complete");
    assert!(serde_json::from_value::<GraphDefinition>(forged).is_err());
    let mut deep = Value::Null;
    for _ in 0..66 {
        deep = json!([deep]);
    }
    assert!(validate_result(&deep).is_err());
    assert!(validate_result(&json!("中".repeat(30_000))).is_err());
    assert!(
        first
            .graph
            .observe("one", &NodeObservation::Complete { result: deep })
            .is_err()
    );
    let mut inconsistent = graph.clone();
    inconsistent.progress.remove("one");
    assert!(inconsistent.advance().is_err());
    let mut inconsistent = graph;
    inconsistent.status = GraphStatus::Complete;
    assert!(inconsistent.advance().is_err());
}
