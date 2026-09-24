//! Original-worker evidence custody, independent of mutable Report diagnostics.
use crate::workspace::Binding;
use hagency_core::tasks::RunnerCapability;
use hagency_store::{DomainStore, OwnedDispatchScope};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopInspectionStatus {
    Unavailable,
    Refused,
    Pending,
    Recorded,
}
pub(crate) struct Inspection {
    pending: Option<Pending>,
    state: StopInspectionStatus,
}
struct Pending {
    domain: DomainStore,
    cap: RunnerCapability,
    scope: OwnedDispatchScope,
    inventory: Value,
}
impl Inspection {
    pub(crate) fn unavailable() -> Self {
        Self {
            pending: None,
            state: StopInspectionStatus::Unavailable,
        }
    }
    /// Called only inside the original worker after its actual full stop and
    /// exact negative fence. Neither returned Report fields nor runtime input
    /// reach this constructor.
    pub(crate) async fn capture(
        domain: &DomainStore,
        cap: &RunnerCapability,
        scope: &OwnedDispatchScope,
        workspace: &Binding,
    ) -> Self {
        let Ok(inventory) = workspace.inspect_stopped() else {
            return Self {
                pending: None,
                state: StopInspectionStatus::Refused,
            };
        };
        let mut observation = Self {
            pending: Some(Pending {
                domain: domain.clone(),
                cap: cap.clone(),
                scope: scope.clone(),
                inventory,
            }),
            state: StopInspectionStatus::Pending,
        };
        observation.retry().await;
        observation
    }
    pub(crate) fn status(&self) -> StopInspectionStatus {
        self.state
    }
    pub(crate) async fn retry(&mut self) -> StopInspectionStatus {
        if let Some(original) = &self.pending {
            match original
                .domain
                .record_owned_stop_inspection(
                    original.cap.clone(),
                    original.scope.clone(),
                    original.inventory.clone(),
                )
                .await
            {
                Ok(_) => {
                    self.state = StopInspectionStatus::Recorded;
                    self.pending = None;
                }
                Err(
                    hagency_store::Error::Busy
                    | hagency_store::Error::Unavailable
                    | hagency_store::Error::OutcomeUnknown,
                ) => {}
                Err(_) => {
                    self.state = StopInspectionStatus::Refused;
                    self.pending = None;
                }
            }
        }
        self.state
    }
}
