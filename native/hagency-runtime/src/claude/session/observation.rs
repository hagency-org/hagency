//! Immutable, private-constructor receipts from one admitted native stream.
use super::{Error, Message, Phase, SessionDriver, UsageEvidence};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
pub const MAX_OBSERVATIONS: u64 = 16_384;

#[derive(Clone)]
pub struct ObservationSource {
    live: Arc<AtomicBool>,
    session: String,
}
impl PartialEq for ObservationSource {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.live, &other.live) && self.session == other.session
    }
}
impl Eq for ObservationSource {}
impl ObservationSource {
    pub fn is_retired(&self) -> bool {
        !self.live.load(Ordering::Acquire)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum ObservationKind {
    Ignored,
    Invalidated,
    Usage(UsageEvidence),
    Result {
        usage: UsageEvidence,
        is_error: bool,
    },
}
#[derive(Clone, PartialEq, Eq)]
pub struct Observation {
    source: ObservationSource,
    sequence: u64,
    kind: ObservationKind,
}
impl Observation {
    pub fn source(&self) -> &ObservationSource {
        &self.source
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn kind(&self) -> &ObservationKind {
        &self.kind
    }
}
pub(super) struct State {
    live: Arc<AtomicBool>,
    sequence: u64,
    last: Option<Observation>,
    usage: super::usage::Tracker,
}
impl Default for State {
    fn default() -> Self {
        Self {
            live: Arc::new(AtomicBool::new(true)),
            sequence: 0,
            last: None,
            usage: Default::default(),
        }
    }
}
impl State {
    pub(super) fn retire(&self) {
        self.live.store(false, Ordering::Release);
    }
    pub(super) fn accept(&mut self, message: &Message, session: &str) -> Result<(), Error> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .filter(|n| *n <= MAX_OBSERVATIONS)
            .ok_or(Error::Capacity)?;
        let kind = match message {
            Message::Event { kind, payload, .. } => self.usage.project(*kind, payload),
            _ => ObservationKind::Ignored,
        };
        self.last = Some(Observation {
            source: self.source(session),
            sequence: self.sequence,
            kind,
        });
        Ok(())
    }
    fn source(&self, session: &str) -> ObservationSource {
        ObservationSource {
            live: self.live.clone(),
            session: session.into(),
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.retire();
    }
}
impl<R, W, E> SessionDriver<R, W, E> {
    /// The system/init receipt is the baseline (sequence1), not usage. Late
    /// attachment is refused even if the caller used only ordinary reads.
    pub fn observation_source(&self) -> Result<ObservationSource, Error> {
        if self.phase != Phase::Running || self.observations.sequence != 1 {
            return Err(Error::State);
        }
        Ok(self
            .observations
            .source(self.session_id.as_deref().ok_or(Error::State)?))
    }
    pub fn matches_observation_source(&self, source: &ObservationSource) -> bool {
        matches!(self.phase, Phase::Running | Phase::ResultObserved)
            && !source.is_retired()
            && self
                .session_id
                .as_deref()
                .is_some_and(|id| &self.observations.source(id) == source)
    }
    /// Clone immediately after each returned Message on any read/control path.
    /// Control/flush returns leave this unchanged; replay is rejected downstream.
    pub fn last_observation(&self) -> Option<&Observation> {
        self.observations.last.as_ref()
    }
}
