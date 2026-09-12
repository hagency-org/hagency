//! Bounded pure graph planning. A transition is a proposal, not a durable dispatch
//! or proof that a canonical task completed. Only the host may apply observations.
use crate::{InvalidInput, tasks::text};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GraphStatus {
    Active,
    Complete,
    Failed,
    Cancelled,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    Pending,
    Dispatched,
    Active,
    Complete,
    Failed,
    Skipped,
    Cancelled,
}
impl NodeStatus {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Complete | Self::Failed | Self::Skipped | Self::Cancelled
        )
    }
}
/// JSON fields preserve missing versus explicit null, including comparison operands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Condition(pub Map<String, Value>);
impl Condition {
    fn string(&self, key: &str) -> Option<&str> {
        self.0
            .get(key)?
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }
    fn validate(&self) -> Result<(), InvalidInput> {
        for (key, value) in &self.0 {
            match key.as_str() {
                "dep" | "path" | "field" | "op" => text(
                    value
                        .as_str()
                        .ok_or(InvalidInput("condition field must be text"))?,
                    512,
                )?,
                "eq" | "neq" | "in" | "value" => validate_result(value)?,
                _ => return Err(InvalidInput("unknown condition field")),
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDefinition {
    pub id: String,
    pub assignee: String,
    pub description: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub condition: Option<Condition>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphDefinition {
    pub label: String,
    pub nodes: Vec<NodeDefinition>,
}
impl GraphDefinition {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        text(&self.label, 4000)?;
        if self.nodes.is_empty() || self.nodes.len() > 128 {
            return Err(InvalidInput("graph requires 1..128 nodes"));
        }
        let mut ids = BTreeSet::new();
        for n in &self.nodes {
            text(&n.id, 255)?;
            text(&n.assignee, 255)?;
            text(&n.description, 4000)?;
            if !ids.insert(n.id.as_str()) {
                return Err(InvalidInput("duplicate graph node"));
            }
            if n.depends_on.len() > 128
                || n.depends_on.iter().collect::<BTreeSet<_>>().len() != n.depends_on.len()
            {
                return Err(InvalidInput("invalid graph dependencies"));
            }
            if let Some(c) = &n.condition {
                c.validate()?;
            }
        }
        let mut edges: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for n in &self.nodes {
            let mut deps: Vec<_> = n.depends_on.iter().map(String::as_str).collect();
            if let Some(dep) = n.condition.as_ref().and_then(|c| c.string("dep"))
                && !deps.contains(&dep)
            {
                deps.push(dep);
            }
            if deps.iter().any(|dep| !ids.contains(dep) || *dep == n.id) {
                return Err(InvalidInput("missing or self dependency"));
            }
            edges.insert(&n.id, deps);
        }
        fn visit<'a>(
            id: &'a str,
            edges: &BTreeMap<&'a str, Vec<&'a str>>,
            active: &mut BTreeSet<&'a str>,
            done: &mut BTreeSet<&'a str>,
        ) -> Result<(), InvalidInput> {
            if done.contains(id) {
                return Ok(());
            }
            if !active.insert(id) {
                return Err(InvalidInput("graph dependency cycle"));
            }
            for dep in &edges[id] {
                visit(dep, edges, active, done)?;
            }
            active.remove(id);
            done.insert(id);
            Ok(())
        }
        let mut active = BTreeSet::new();
        let mut done = BTreeSet::new();
        for id in ids {
            visit(id, &edges, &mut active, &mut done)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProgress {
    pub status: NodeStatus,
    pub result: Value,
    pub error: Option<String>,
}
impl Default for NodeProgress {
    fn default() -> Self {
        Self {
            status: NodeStatus::Pending,
            result: Value::Null,
            error: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Graph {
    pub definition: GraphDefinition,
    pub status: GraphStatus,
    pub progress: BTreeMap<String, NodeProgress>,
}
#[derive(Debug, Clone, Serialize)]
pub struct DependencyResult {
    pub node_id: String,
    pub assignee: String,
    pub result: Value,
}
#[derive(Debug, Clone, Serialize)]
pub struct Assignment {
    pub node_id: String,
    pub assignee: String,
    pub description: String,
    pub dependency_results: Vec<DependencyResult>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Transition {
    pub graph: Graph,
    pub assignments: Vec<Assignment>,
}
#[derive(Debug, Clone, Serialize)]
pub enum NodeObservation {
    Active,
    Complete { result: Value },
    Failed { error: String },
    Cancelled,
}

pub fn validate_result(value: &Value) -> Result<(), InvalidInput> {
    fn depth(value: &Value, n: usize) -> Result<(), InvalidInput> {
        if n > 64 {
            return Err(InvalidInput("graph JSON exceeds depth limit"));
        }
        match value {
            Value::Array(items) => {
                for item in items {
                    depth(item, n + 1)?;
                }
            }
            Value::Object(items) => {
                for item in items.values() {
                    depth(item, n + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    depth(value, 0)?;
    if serde_json::to_vec(value)
        .map_err(|_| InvalidInput("invalid graph JSON"))?
        .len()
        > 65536
    {
        return Err(InvalidInput("graph result exceeds 64 KiB"));
    }
    Ok(())
}
enum Selected<'a> {
    Json(Cow<'a, Value>),
    InheritedFunction,
}
fn nested<'a>(value: &'a Value, path: &str) -> Option<Selected<'a>> {
    if path.is_empty()
        || path
            .split('.')
            .any(|part| matches!(part, "__proto__" | "constructor" | "prototype"))
    {
        return None;
    }
    let mut current = Selected::Json(Cow::Borrowed(value));
    for part in path.split('.') {
        current = match current {
            Selected::Json(Cow::Borrowed(v)) => property(v, part)?,
            Selected::Json(Cow::Owned(v)) => match property(&v, part)? {
                Selected::Json(value) => Selected::Json(Cow::Owned(value.into_owned())),
                Selected::InheritedFunction => Selected::InheritedFunction,
            },
            // The existing getNestedValue only traverses typeof === object.
            Selected::InheritedFunction => return None,
        };
    }
    Some(current)
}
fn property<'a>(value: &'a Value, key: &str) -> Option<Selected<'a>> {
    let object_method = matches!(
        key,
        "__defineGetter__"
            | "__defineSetter__"
            | "hasOwnProperty"
            | "__lookupGetter__"
            | "__lookupSetter__"
            | "isPrototypeOf"
            | "propertyIsEnumerable"
            | "toString"
            | "valueOf"
            | "toLocaleString"
    );
    let array_method = matches!(
        key,
        "at" | "concat"
            | "copyWithin"
            | "fill"
            | "find"
            | "findIndex"
            | "findLast"
            | "findLastIndex"
            | "lastIndexOf"
            | "pop"
            | "push"
            | "reverse"
            | "shift"
            | "unshift"
            | "slice"
            | "sort"
            | "splice"
            | "includes"
            | "indexOf"
            | "join"
            | "keys"
            | "entries"
            | "values"
            | "forEach"
            | "filter"
            | "flat"
            | "flatMap"
            | "map"
            | "every"
            | "some"
            | "reduce"
            | "reduceRight"
            | "toReversed"
            | "toSorted"
            | "toSpliced"
            | "with"
    );
    match value {
        Value::Object(o) => o
            .get(key)
            .map(|v| Selected::Json(Cow::Borrowed(v)))
            .or_else(|| object_method.then_some(Selected::InheritedFunction)),
        Value::Array(a) if key == "length" => {
            Some(Selected::Json(Cow::Owned(Value::from(a.len()))))
        }
        Value::Array(_) if object_method || array_method => Some(Selected::InheritedFunction),
        Value::Array(a) => {
            let index = key.parse::<usize>().ok().filter(|n| n.to_string() == key)?;
            a.get(index).map(|v| Selected::Json(Cow::Borrowed(v)))
        }
        // Primitive string/number/boolean property access is refused by JS policy.
        _ => None,
    }
}
fn strict_equal(a: Option<&Selected<'_>>, b: Option<&Value>) -> bool {
    let a = match a {
        Some(Selected::InheritedFunction) => return false,
        Some(Selected::Json(v)) => Some(v.as_ref()),
        None => None,
    };
    match (a, b) {
        (None, None) | (Some(Value::Null), Some(Value::Null)) => true,
        (Some(Value::Bool(a)), Some(Value::Bool(b))) => a == b,
        (Some(Value::String(a)), Some(Value::String(b))) => a == b,
        (Some(Value::Number(a)), Some(Value::Number(b))) => a.as_f64() == b.as_f64(),
        // JS does not compare separately materialized JSON objects by structure.
        _ => false,
    }
}
fn truthy(value: Option<&Selected<'_>>) -> bool {
    let value = match value {
        Some(Selected::InheritedFunction) => return true,
        Some(Selected::Json(v)) => Some(v.as_ref()),
        None => None,
    };
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(v)) => *v,
        Some(Value::Number(v)) => v.as_f64().is_some_and(|v| v != 0.0),
        Some(Value::String(v)) => !v.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}
pub fn evaluate_condition(
    node: &NodeDefinition,
    progress: &BTreeMap<String, NodeProgress>,
) -> Option<bool> {
    let Some(condition) = &node.condition else {
        return Some(true);
    };
    if condition.0.is_empty() {
        return Some(true);
    }
    let Some(dep) = condition
        .string("dep")
        .or_else(|| node.depends_on.first().map(String::as_str))
    else {
        return Some(false);
    };
    let dep = progress.get(dep)?;
    if !dep.status.terminal() {
        return None;
    }
    if dep.status != NodeStatus::Complete {
        return Some(false);
    }
    let selected = match condition
        .string("path")
        .or_else(|| condition.string("field"))
    {
        Some(path) => nested(&dep.result, path),
        None => Some(Selected::Json(Cow::Borrowed(&dep.result))),
    };
    let actual = selected.as_ref();
    for op in ["eq", "neq", "in"] {
        if let Some(expected) = condition.0.get(op) {
            return Some(compare(op, actual, Some(expected)));
        }
    }
    if let Some(op @ ("eq" | "neq" | "in")) = condition.string("op") {
        return Some(compare(op, actual, condition.0.get("value")));
    }
    Some(truthy(actual))
}
fn compare(op: &str, actual: Option<&Selected<'_>>, expected: Option<&Value>) -> bool {
    match op {
        "eq" => strict_equal(actual, expected),
        "neq" => !strict_equal(actual, expected),
        "in" => expected
            .and_then(Value::as_array)
            .is_some_and(|items| items.iter().any(|v| strict_equal(actual, Some(v)))),
        _ => false,
    }
}
impl Graph {
    pub fn new(definition: GraphDefinition) -> Result<Self, InvalidInput> {
        definition.validate()?;
        let progress = definition
            .nodes
            .iter()
            .map(|n| (n.id.clone(), NodeProgress::default()))
            .collect();
        Ok(Self {
            definition,
            status: GraphStatus::Active,
            progress,
        })
    }
    pub fn validate(&self) -> Result<(), InvalidInput> {
        self.definition.validate()?;
        if self.progress.len() != self.definition.nodes.len()
            || self
                .definition
                .nodes
                .iter()
                .any(|n| !self.progress.contains_key(&n.id))
        {
            return Err(InvalidInput("graph progress does not match nodes"));
        }
        for p in self.progress.values() {
            validate_result(&p.result)?;
            if let Some(error) = &p.error {
                text(error, 4000)?;
            }
        }
        if self.status != GraphStatus::Active
            && self.progress.values().any(|p| !p.status.terminal())
        {
            return Err(InvalidInput("terminal graph has live nodes"));
        }
        Ok(())
    }
    pub fn advance(&self) -> Result<Transition, InvalidInput> {
        self.advance_inner(false)
    }
    /// Durable dispatch stores immutable result references. Avoid multiplying
    /// every large dependency value into each assignment in a wide graph.
    pub fn advance_references(&self) -> Result<Transition, InvalidInput> {
        self.advance_inner(true)
    }
    fn advance_inner(&self, references: bool) -> Result<Transition, InvalidInput> {
        self.validate()?;
        let mut next = self.clone();
        let mut assignments = Vec::new();
        if next.status != GraphStatus::Active {
            return Ok(Transition {
                graph: next,
                assignments,
            });
        }
        loop {
            let mut changed = false;
            for node in &next.definition.nodes {
                if next.progress[&node.id].status != NodeStatus::Pending {
                    continue;
                }
                let failed: Vec<_> = node
                    .depends_on
                    .iter()
                    .filter(|dep| {
                        matches!(
                            next.progress[*dep].status,
                            NodeStatus::Failed | NodeStatus::Cancelled
                        )
                    })
                    .cloned()
                    .collect();
                let status = if !failed.is_empty() {
                    Some(NodeStatus::Failed)
                } else if node.depends_on.iter().all(|dep| {
                    matches!(
                        next.progress[dep].status,
                        NodeStatus::Complete | NodeStatus::Skipped
                    )
                }) {
                    evaluate_condition(node, &next.progress).map(|pass| {
                        if pass {
                            NodeStatus::Dispatched
                        } else {
                            NodeStatus::Skipped
                        }
                    })
                } else {
                    None
                };
                let Some(status) = status else {
                    continue;
                };
                if status == NodeStatus::Dispatched {
                    let results = node
                        .depends_on
                        .iter()
                        .filter(|dep| next.progress[*dep].status == NodeStatus::Complete)
                        .map(|dep| DependencyResult {
                            node_id: dep.clone(),
                            assignee: next
                                .definition
                                .nodes
                                .iter()
                                .find(|n| n.id == *dep)
                                .expect("validated dependency")
                                .assignee
                                .clone(),
                            result: if references {
                                Value::Null
                            } else {
                                next.progress[dep].result.clone()
                            },
                        })
                        .collect();
                    assignments.push(Assignment {
                        node_id: node.id.clone(),
                        assignee: node.assignee.clone(),
                        description: node.description.clone(),
                        dependency_results: results,
                    });
                }
                let p = next.progress.get_mut(&node.id).expect("validated node");
                p.status = status;
                if !failed.is_empty() {
                    let mut error = format!("dependency failed: {}", failed.join(", "));
                    if error.len() > 4000 {
                        let mut end = 3997;
                        while !error.is_char_boundary(end) {
                            end -= 1;
                        }
                        error.truncate(end);
                        error.push_str("...");
                    }
                    p.error = Some(error);
                }
                changed = true;
            }
            if !changed {
                break;
            }
        }
        if next.progress.values().all(|p| p.status.terminal()) {
            next.status = if next
                .progress
                .values()
                .any(|p| p.status == NodeStatus::Failed)
            {
                GraphStatus::Failed
            } else {
                GraphStatus::Complete
            };
        }
        Ok(Transition {
            graph: next,
            assignments,
        })
    }
    pub fn observe(&self, id: &str, observation: &NodeObservation) -> Result<Self, InvalidInput> {
        self.validate()?;
        if self.status != GraphStatus::Active {
            return Err(InvalidInput("graph is terminal"));
        }
        let mut next = self.clone();
        let p = next
            .progress
            .get_mut(id)
            .ok_or(InvalidInput("graph node not found"))?;
        if !matches!(p.status, NodeStatus::Dispatched | NodeStatus::Active) {
            return Err(InvalidInput("graph node is not dispatched"));
        }
        p.status = match observation {
            NodeObservation::Active => NodeStatus::Active,
            NodeObservation::Complete { result } => {
                validate_result(result)?;
                p.result = result.clone();
                NodeStatus::Complete
            }
            NodeObservation::Failed { error } => {
                text(error, 4000)?;
                p.error = Some(error.clone());
                NodeStatus::Failed
            }
            NodeObservation::Cancelled => NodeStatus::Cancelled,
        };
        Ok(next)
    }
    pub fn cancel(&self) -> Result<Self, InvalidInput> {
        self.validate()?;
        let mut next = self.clone();
        next.status = GraphStatus::Cancelled;
        for p in next.progress.values_mut() {
            if !p.status.terminal() {
                p.status = NodeStatus::Cancelled;
            }
        }
        Ok(next)
    }
}
