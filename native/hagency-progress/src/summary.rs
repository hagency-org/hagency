use crate::{Error, MAX_COUNTER};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Start,
    Step,
    Done,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Verb {
    #[serde(rename = "read")]
    Read,
    #[serde(rename = "searched")]
    Searched,
    #[serde(rename = "ran commands")]
    Commands,
    #[serde(rename = "edited")]
    Edited,
    #[serde(rename = "wrote")]
    Wrote,
    #[serde(rename = "fetched")]
    Fetched,
    #[serde(rename = "worked")]
    Worked,
}
impl Verb {
    pub fn for_tool(tool: &str) -> Self {
        match tool {
            "Read" => Self::Read,
            "Glob" | "Grep" | "WebSearch" => Self::Searched,
            "Bash" => Self::Commands,
            "Edit" | "NotebookEdit" => Self::Edited,
            "Write" => Self::Wrote,
            "WebFetch" => Self::Fetched,
            _ => Self::Worked,
        }
    }
    pub fn text(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Searched => "searched",
            Self::Commands => "ran commands",
            Self::Edited => "edited",
            Self::Wrote => "wrote",
            Self::Fetched => "fetched",
            Self::Worked => "worked",
        }
    }
}

/// Fixed vocabulary in first-observation order. Caller strings never become keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts(Vec<(Verb, u32)>);
impl Counts {
    pub fn add(&mut self, verb: Verb, count: u32) -> Result<(), Error> {
        if count == 0 {
            return Err(Error::Shape);
        }
        let total: u64 = self.0.iter().map(|(_, n)| u64::from(*n)).sum();
        if total + u64::from(count) > u64::from(MAX_COUNTER) {
            return Err(Error::Capacity);
        }
        if let Some((_, n)) = self.0.iter_mut().find(|(v, _)| *v == verb) {
            *n += count;
        } else {
            self.0.push((verb, count));
        }
        Ok(())
    }
    pub fn entries(&self) -> &[(Verb, u32)] {
        &self.0
    }
}

/// `done` means the observed runtime finished, never that a canonical task is Done.
/// `delivered` is optional host evidence about answers, not progress activity.
pub fn build_summary(
    kind: Kind,
    counts: &Counts,
    failures: u32,
    delivered: Option<u32>,
) -> Option<String> {
    if kind == Kind::Start {
        return Some("started".into());
    }
    let parts: Vec<_> = counts
        .0
        .iter()
        .map(|(v, n)| {
            if *n > 1 {
                format!("{} ×{n}", v.text())
            } else {
                v.text().into()
            }
        })
        .collect();
    let failed = format!(
        "{failures} failed attempt{}",
        if failures > 1 { "s" } else { "" }
    );
    if kind == Kind::Done {
        if failures > 0 && parts.is_empty() {
            return Some(format!("finished, but nothing succeeded — {failed}"));
        }
        if failures > 0 {
            return Some(format!(
                "finished — {}, {failures} failed",
                parts.join(", ")
            ));
        }
        if parts.is_empty() {
            return Some("finished".into());
        }
        return Some(format!(
            "finished — {}{}",
            parts.join(", "),
            if delivered == Some(0) {
                ", but sent nothing"
            } else {
                ""
            }
        ));
    }
    if failures > 0 && parts.is_empty() {
        return Some(failed);
    }
    if failures > 0 {
        return Some(format!("{}, {failures} failed", parts.join(", ")));
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}
