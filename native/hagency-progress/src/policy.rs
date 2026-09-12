use crate::{Error, Kind, Verb, fingerprint};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Default,
    File,
    PerGroup,
}
#[derive(Clone)]
pub struct Filter {
    events: Vec<String>,
    include: Option<Vec<String>>,
    exclude: Vec<String>,
    interval: f64,
    source: Source,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Decision {
    pub report: bool,
    pub kind: Kind,
    pub verb: Option<Verb>,
}
impl Filter {
    pub fn parse(raw: &Value, group: Option<&str>) -> Result<Self, Error> {
        fingerprint(raw, 65_536)?;
        if group.is_some_and(|g| !crate::identity(g)) {
            return Err(Error::Shape);
        }
        if raw.is_null() {
            return Ok(Self::default());
        }
        let raw = raw
            .as_object()
            .ok_or(Error::Config("filter is not an object"))?;
        let per_group = raw
            .get("perGroup")
            .map(|v| {
                v.as_object()
                    .ok_or(Error::Config("perGroup is not an object"))
            })
            .transpose()?;
        let scoped = group.and_then(|g| per_group.and_then(|p| p.get(g)));
        let (base, source) = match scoped {
            Some(v) => (
                v.as_object()
                    .ok_or(Error::Config("selected perGroup rule is not an object"))?,
                Source::PerGroup,
            ),
            None => (raw, Source::File),
        };
        let events = list(
            base.get("events"),
            Some(vec!["start".into(), "step".into(), "done".into()]),
            "events must be an array of non-empty strings",
        )?
        .ok_or(Error::Shape)?;
        let tools = base
            .get("tools")
            .map(|v| v.as_object().ok_or(Error::Config("tools is not an object")))
            .transpose()?;
        let include = list(
            tools.and_then(|t| t.get("include")),
            None,
            "tools.include must be an array of strings or null",
        )?;
        let exclude = list(
            tools.and_then(|t| t.get("exclude")),
            Some(vec![]),
            "tools.exclude must be an array of strings",
        )?
        .ok_or(Error::Shape)?;
        let interval = match base.get("minIntervalMs") {
            None | Some(Value::Null) => 60_000.0,
            Some(v) => v
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .ok_or(Error::Config("minIntervalMs must be a non-negative number"))?,
        }
        .max(5000.0);
        Ok(Self {
            events,
            include,
            exclude,
            interval,
            source,
        })
    }
    pub fn min_interval_ms(&self) -> f64 {
        self.interval
    }
    pub fn source(&self) -> Source {
        self.source
    }
    pub fn events(&self) -> &[String] {
        &self.events
    }
    pub fn included_tools(&self) -> Option<&[String]> {
        self.include.as_deref()
    }
    pub fn excluded_tools(&self) -> &[String] {
        &self.exclude
    }
    pub fn decide(&self, event: Option<&str>, tool: Option<&str>) -> Decision {
        let kind = match event {
            Some("Stop" | "done") => Kind::Done,
            Some("start" | "SessionStart") => Kind::Start,
            _ => Kind::Step,
        };
        let name = match kind {
            Kind::Start => "start",
            Kind::Step => "step",
            Kind::Done => "done",
        };
        let mut decision = Decision {
            report: false,
            kind,
            verb: None,
        };
        if !self.events.iter().any(|e| e == name) {
            return decision;
        }
        if kind != Kind::Step {
            decision.report = true;
            return decision;
        }
        let Some(tool) = tool.filter(|t| !t.is_empty() && t.len() <= 256) else {
            return decision;
        };
        if self.exclude.iter().any(|t| t == tool)
            || self
                .include
                .as_ref()
                .is_some_and(|l| !l.iter().any(|t| t == tool))
        {
            return decision;
        }
        decision.report = true;
        decision.verb = Some(Verb::for_tool(tool));
        decision
    }
}
impl Default for Filter {
    fn default() -> Self {
        Self {
            events: vec!["start".into(), "step".into(), "done".into()],
            include: None,
            exclude: vec![],
            interval: 60_000.0,
            source: Source::Default,
        }
    }
}
fn list(
    value: Option<&Value>,
    fallback: Option<Vec<String>>,
    reason: &'static str,
) -> Result<Option<Vec<String>>, Error> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(fallback);
    };
    let values = value.as_array().ok_or(Error::Config(reason))?;
    if values.len() > 128 {
        return Err(Error::Capacity);
    }
    values
        .iter()
        .map(|v| {
            let text = v
                .as_str()
                .ok_or(Error::Config(reason))?
                .trim_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}');
            if text.is_empty() {
                return Err(Error::Config(reason));
            }
            if text.len() > 256 {
                return Err(Error::Capacity);
            }
            Ok(text.into())
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

pub fn acp_tool(update: &Value) -> Option<&'static str> {
    if update.get("sessionUpdate").and_then(Value::as_str) != Some("tool_call") {
        return None;
    }
    match update.get("kind").and_then(Value::as_str) {
        Some("read") => Some("Read"),
        Some("search") => Some("Grep"),
        Some("execute") => Some("Bash"),
        Some("edit" | "delete" | "move") => Some("Edit"),
        Some("fetch") => Some("WebFetch"),
        Some("think" | "other") => None,
        _ => Some("AcpTool"),
    }
}
pub fn acp_failed(update: &Value) -> bool {
    update.get("sessionUpdate").and_then(Value::as_str) == Some("tool_call_update")
        && update
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case("failed"))
}
