//! Closed, private adapter set. Arbitrary streams cannot attach in production.
use hagency_metering::{
    MeteringError, TokenCounts,
    claude_usage::{ClaudeUsage, Coverage},
    observation::UsageObservation,
    runtime_usage::{CodexUsage, ProjectionDiagnostics},
};
use hagency_runtime::{
    claude::session as claude,
    codex::session as codex,
    owned::{OwnedClaudeSession, OwnedSession},
};

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Source {
    Codex(codex::ObservationSource),
    Claude(claude::ObservationSource),
}
impl From<codex::ObservationSource> for Source {
    fn from(s: codex::ObservationSource) -> Self {
        Self::Codex(s)
    }
}
impl From<claude::ObservationSource> for Source {
    fn from(s: claude::ObservationSource) -> Self {
        Self::Claude(s)
    }
}
impl Source {
    pub(super) fn is_retired(&self) -> bool {
        match self {
            Self::Codex(s) => s.is_retired(),
            Self::Claude(s) => s.is_retired(),
        }
    }
    pub(super) fn baseline(&self) -> u64 {
        match self {
            Self::Codex(_) => 0,
            Self::Claude(_) => 1,
        }
    }
    pub(super) fn family(&self) -> &str {
        match self {
            Self::Codex(_) => "codex",
            Self::Claude(_) => "claude",
        }
    }
}
pub(crate) trait OwnedCapture {
    fn capture_source(&self) -> Option<Source>;
}
impl OwnedCapture for OwnedSession {
    fn capture_source(&self) -> Option<Source> {
        self.observation_source()
            .ok()
            .filter(|s| self.matches_observation_source(s))
            .map(Into::into)
    }
}
impl OwnedCapture for OwnedClaudeSession {
    fn capture_source(&self) -> Option<Source> {
        self.observation_source()
            .ok()
            .filter(|s| self.matches_observation_source(s))
            .map(Into::into)
    }
}
#[derive(Clone)]
pub(crate) enum Evidence {
    Codex(codex::UsageEvidence),
    Claude(claude::UsageEvidence),
}
impl Evidence {
    pub(super) fn normalize(&self) -> Result<UsageObservation, MeteringError> {
        match self {
            Self::Codex(e) => {
                let d = e.diagnostics();
                UsageObservation::codex_runtime(CodexUsage {
                    total: super::breakdown(e.total()),
                    last: super::breakdown(e.last()),
                    context_window: e.model_context_window(),
                    diagnostics: ProjectionDiagnostics {
                        missing: d.has_missing_fields(),
                        invalid: d.has_invalid_fields(),
                        unsupported: d.has_unsupported_fields(),
                    },
                })
            }
            Self::Claude(e) => {
                let c = e.counts();
                let d = e.diagnostics();
                let coverage = match e.coverage() {
                    claude::UsageCoverage::MainLoopSteps => Coverage::MainLoopSteps,
                    claude::UsageCoverage::MainLoopResult => Coverage::MainLoopResult,
                    claude::UsageCoverage::ReportedModelsResult => Coverage::ReportedModelsResult,
                };
                UsageObservation::claude_runtime(ClaudeUsage {
                    counts: TokenCounts {
                        input: c.input(),
                        output: c.output(),
                        cache_read: c.cache_read(),
                        cache_write: c.cache_write(),
                    },
                    coverage,
                    steps: e.steps(),
                    models: e.models(),
                    diagnostics: ProjectionDiagnostics {
                        missing: d.has_missing_fields(),
                        invalid: d.has_invalid_fields(),
                        unsupported: d.has_unsupported_fields(),
                    },
                })
            }
        }
    }
}
pub(crate) enum EvidenceRef<'a> {
    Codex(&'a codex::UsageEvidence),
    Claude(&'a claude::UsageEvidence),
}
impl EvidenceRef<'_> {
    pub(super) fn retain(self) -> Evidence {
        match self {
            Self::Codex(e) => Evidence::Codex(e.clone()),
            Self::Claude(e) => Evidence::Claude(e.clone()),
        }
    }
}
pub(crate) enum Kind<'a> {
    Ignored,
    Invalidated,
    End,
    Usage {
        evidence: EvidenceRef<'a>,
        terminal: bool,
    },
}
pub(crate) trait CapturedEvent {
    fn source(&self) -> Source;
    fn sequence(&self) -> u64;
    fn kind(&self) -> Kind<'_>;
}
impl CapturedEvent for codex::Observation {
    fn source(&self) -> Source {
        self.source().clone().into()
    }
    fn sequence(&self) -> u64 {
        self.sequence()
    }
    fn kind(&self) -> Kind<'_> {
        match self.kind() {
            codex::ObservationKind::Invalidated => Kind::Invalidated,
            codex::ObservationKind::TurnEnded(_) => Kind::End,
            codex::ObservationKind::Usage(e) => Kind::Usage {
                evidence: EvidenceRef::Codex(e),
                terminal: false,
            },
            _ => Kind::Ignored,
        }
    }
}
impl CapturedEvent for claude::Observation {
    fn source(&self) -> Source {
        self.source().clone().into()
    }
    fn sequence(&self) -> u64 {
        self.sequence()
    }
    fn kind(&self) -> Kind<'_> {
        match self.kind() {
            claude::ObservationKind::Invalidated => Kind::Invalidated,
            claude::ObservationKind::Result { usage, .. } => Kind::Usage {
                evidence: EvidenceRef::Claude(usage),
                terminal: true,
            },
            claude::ObservationKind::Usage(e) => Kind::Usage {
                evidence: EvidenceRef::Claude(e),
                terminal: false,
            },
            claude::ObservationKind::Ignored => Kind::Ignored,
        }
    }
}
