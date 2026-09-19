//! Pure Matrix content formatting. This does not authenticate, route or send.
//! Existing truthy `formatted_body` is trusted passthrough, matching the legacy
//! helper; only HTML generated from Markdown passes through our allowlist.
mod links;
mod render;
use serde::Serialize;
use serde_json::Value;

pub const MAX_INPUT_BYTES: usize = 262_144;
pub const MAX_BODY_BYTES: usize = 65_536;
pub const MAX_OUTPUT_BYTES: usize = 524_288;
pub const MAX_JSON_DEPTH: usize = 16;
pub const MAX_EDIT_DEPTH: usize = 8;
/// Conservative syntax complexity budget, including markers inside code spans.
pub const MAX_MARKDOWN_MARKERS: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("Matrix content must be an object with object edit content")]
    Shape,
    #[error("unsupported Markdown structure")]
    Markdown,
    #[error("Matrix formatting capacity exceeded")]
    Capacity,
}

/// Content only: even an extra field named `room_id` is inert preserved data,
/// never a route. The transport must use separately authenticated host authority.
#[derive(Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct MatrixContent(Value);
impl MatrixContent {
    pub fn new(value: Value) -> Result<Self, Error> {
        if !value.is_object() {
            return Err(Error::Shape);
        }
        validate(&value, MAX_INPUT_BYTES)?;
        Ok(Self(value))
    }
    pub fn as_value(&self) -> &Value {
        &self.0
    }
    pub fn into_value(self) -> Value {
        self.0
    }
    /// Returns a new content object or an error, never partially mutating input.
    pub fn formatted(&self) -> Result<Self, Error> {
        let mut value = self.0.clone();
        format_edit(&mut value, 0)?;
        validate(&value, MAX_OUTPUT_BYTES)?;
        Ok(Self(value))
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(v) => v.as_f64() != Some(0.0),
        Value::String(v) => !v.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}
fn format_edit(value: &mut Value, depth: usize) -> Result<(), Error> {
    if depth > MAX_EDIT_DEPTH {
        return Err(Error::Capacity);
    }
    let object = value.as_object_mut().ok_or(Error::Shape)?;
    if matches!(
        object.get("msgtype").and_then(Value::as_str),
        Some("m.text" | "m.notice")
    ) && !object.get("formatted_body").is_some_and(truthy)
        && let Some(body) = object.get("body").and_then(Value::as_str)
    {
        if body.len() > MAX_BODY_BYTES {
            return Err(Error::Capacity);
        }
        let html = render::markdown(body)?;
        object.insert(
            "format".into(),
            Value::String("org.matrix.custom.html".into()),
        );
        object.insert("formatted_body".into(), Value::String(html));
    }
    if let Some(edit) = object.get_mut("m.new_content").filter(|v| truthy(v)) {
        format_edit(edit, depth + 1)?;
    }
    Ok(())
}
fn validate(value: &Value, limit: usize) -> Result<(), Error> {
    // Bound traversal before serialization; avoid allocating another full JSON
    // string merely to discover that input exceeds the size limit.
    let mut stack = vec![(value, 0)];
    let mut nodes = 0;
    while let Some((v, depth)) = stack.pop() {
        nodes += 1;
        if depth > MAX_JSON_DEPTH || nodes > limit {
            return Err(Error::Capacity);
        }
        let child_count = match v {
            Value::Array(v) => v.len(),
            Value::Object(v) => v.len(),
            _ => 0,
        };
        if child_count > limit.saturating_sub(nodes + stack.len()) {
            return Err(Error::Capacity);
        }
        match v {
            Value::Array(values) => stack.extend(values.iter().map(|v| (v, depth + 1))),
            Value::Object(values) => stack.extend(values.values().map(|v| (v, depth + 1))),
            _ => {}
        }
    }
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or_else(|| std::io::Error::other("capacity"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Counter(limit), value).map_err(|_| Error::Capacity)
}
