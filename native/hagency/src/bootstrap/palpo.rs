//! One service-owned outbound adapter; no registration or execution authority.
use super::{Failure, config::read};
use hagency_core::{authority::Registration, canonical};
use hagency_palpo::{Adapter, CancellationToken, Error, HostConfig, Limits};
use hagency_store::{DomainStore, Store, outbound::RegistrationIdentity};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::task::JoinHandle;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    profile: String,
    endpoint: String,
    registration: Registration,
    machine_generation: u64,
}

pub(super) struct Prepared {
    host: HostConfig,
    registration: RegistrationIdentity,
}
impl Prepared {
    pub(super) fn load(state: &Path) -> Result<Self, Failure> {
        let value: Config =
            serde_json::from_slice(&read(&state.join("palpo-transport.json"), 16 * 1024)?)
                .map_err(|_| Failure::Config)?;
        if value.profile != "palpo_v2_resources_v1" || !value.endpoint.starts_with("https://") {
            return Err(Failure::Config);
        }
        value.registration.validate().map_err(|_| Failure::Config)?;
        let registration_fingerprint = canonical::digest(
            &serde_json::to_value(&value.registration).map_err(|_| Failure::Config)?,
        )
        .map_err(|_| Failure::Config)?;
        let registration = RegistrationIdentity {
            binding: "native-palpo-v2".into(),
            side_id: value.registration.server_name,
            fleet_id: value.registration.fleet_id,
            registration_generation: value.registration.generation,
            registration_fingerprint,
        };
        let token = read(&state.join("palpo.machine_token"), 4096)?;
        let token = std::str::from_utf8(&token).map_err(|_| Failure::Config)?;
        let mut host = HostConfig::new(
            registration.clone(),
            &value.endpoint,
            token,
            value.machine_generation,
            Limits::default(),
        )
        .map_err(|_| Failure::Config)?;
        let ca = state.join("palpo.ca.pem");
        match std::fs::symlink_metadata(&ca) {
            Ok(_) => {
                host = host
                    .with_root_pem(&read(&ca, 16 * 1024)?)
                    .map_err(|_| Failure::Config)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Failure::Config),
        }
        Ok(Self { host, registration })
    }
}

#[derive(Clone, Serialize)]
pub(crate) struct Status {
    configured: bool,
    state: &'static str,
    error: Option<&'static str>,
}
#[derive(Clone)]
pub(crate) struct StatusHandle(Arc<Mutex<Status>>);
impl StatusHandle {
    pub(super) fn new(configured: bool) -> Self {
        Self(Arc::new(Mutex::new(Status {
            configured,
            state: if configured { "starting" } else { "disabled" },
            error: None,
        })))
    }
    pub(crate) fn get(&self) -> Status {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn set(&self, state: &'static str, error: Option<&'static str>) {
        let mut status = self.0.lock().unwrap_or_else(|e| e.into_inner());
        status.state = state;
        status.error = error;
    }
    fn finish(&self, result: Result<(), Error>) {
        match result {
            Ok(()) => self.set("stopped", None),
            Err(error) => self.set(
                if matches!(error, Error::OutcomeUnknown | Error::Unavailable) {
                    "outcome_unknown"
                } else {
                    "unavailable"
                },
                Some(error_label(error)),
            ),
        }
    }
}
fn error_label(error: Error) -> &'static str {
    match error {
        Error::Config => "config",
        Error::Busy => "busy",
        Error::Cancelled => "cancelled",
        Error::Timeout => "timeout",
        Error::Transport => "transport",
        Error::Redirect => "redirect",
        Error::Headers => "headers",
        Error::BodyTooLarge => "body_too_large",
        Error::InvalidJson => "invalid_json",
        Error::Wire => "wire",
        Error::Generation => "generation",
        Error::Unauthorized => "unauthorized",
        Error::Remote(_) => "remote",
        Error::Custody => "custody",
        Error::Unavailable => "unavailable",
        Error::OutcomeUnknown => "outcome_unknown",
        Error::Capacity => "capacity",
        Error::Conflict => "conflict",
    }
}

// A panic or executor abandonment must not leave a false running observation.
struct Completion {
    status: StatusHandle,
    finished: bool,
}
impl Drop for Completion {
    fn drop(&mut self) {
        if !self.finished {
            self.status.set("outcome_unknown", Some("worker"));
        }
    }
}

pub(super) struct Owner {
    cancel: CancellationToken,
    task: Option<JoinHandle<Result<(), Error>>>,
    joined: Option<Result<(), Failure>>,
}
impl Owner {
    /// Synchronous handoff: Bootstrap retains the original join before awaiting.
    pub(super) fn start(
        prepared: Prepared,
        store: Store,
        domain: DomainStore,
        status: StatusHandle,
    ) -> Self {
        let cancel = CancellationToken::new();
        let signal = cancel.clone();
        let task = tokio::spawn(async move {
            let mut completion = Completion {
                status,
                finished: false,
            };
            let result = async {
                if signal.is_cancelled() {
                    return Ok(());
                }
                // Registration is an existing canonical prerequisite. In
                // particular, do not register/rotate then fail custody attach.
                domain
                    .check_publication_registration(prepared.registration)
                    .await?;
                if signal.is_cancelled() {
                    return Ok(());
                }
                // Preserve the original activation future through its receipt.
                let adapter = Adapter::attach(prepared.host, store).await?;
                if signal.is_cancelled() {
                    return Ok(());
                }
                completion.status.set("running", None);
                // No cancellation select around this joined operation. Its
                // original received custody/known receipts settle before return.
                // ADR109 rechecks domain identity after custody waits before
                // HTTP admission; already admitted bytes cannot be recalled.
                adapter.run_with_resources(&domain, &signal).await
            }
            .await;
            completion.status.finish(result);
            completion.finished = true;
            result
        });
        Self {
            cancel,
            task: Some(task),
            joined: None,
        }
    }
    pub(super) fn cancel(&self) {
        self.cancel.cancel();
    }
    pub(super) async fn close(&mut self) -> Result<(), Failure> {
        self.cancel();
        if let Some(result) = self.joined {
            return result;
        }
        let task = self.task.as_mut().ok_or(Failure::OutcomeUnknown)?;
        let joined = match tokio::time::timeout(Duration::from_secs(2), task).await {
            Ok(result) => result,
            Err(_) => return Err(Failure::OutcomeUnknown), // original join retained
        };
        // Adapter failure is separately retained status, not an active worker.
        // A failed join remains unknown, never inferred to be a clean close.
        let result = joined.map(|_| ()).map_err(|_| Failure::OutcomeUnknown);
        self.joined = Some(result);
        self.task = None;
        result
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        // Abrupt abandonment only requests cancellation; it is no close ACK.
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn native_palpo_service_cancel_custody_retained_join() {
        // A gated task models a delayed worker completion, not HTTPS acceptance.
        // The production close implementation must keep the original join even
        // after caller cancellation and after its own unchanged 2s observation.
        let (release, wait) = tokio::sync::oneshot::channel();
        let finishes = Arc::new(AtomicUsize::new(0));
        let finished = finishes.clone();
        let task = tokio::spawn(async move {
            wait.await.unwrap();
            finished.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        let mut owner = Owner {
            cancel: CancellationToken::new(),
            task: Some(task),
            joined: None,
        };
        assert!(
            tokio::time::timeout(Duration::from_millis(10), owner.close())
                .await
                .is_err()
        );
        assert_eq!(owner.close().await, Err(Failure::OutcomeUnknown));
        assert_eq!(finishes.load(Ordering::SeqCst), 0);
        assert!(owner.task.is_some());
        release.send(()).unwrap();
        assert_eq!(owner.close().await, Ok(()));
        assert_eq!(owner.close().await, Ok(()));
        assert_eq!(finishes.load(Ordering::SeqCst), 1);

        let status = StatusHandle::new(true);
        let observed = status.clone();
        let task = tokio::spawn(async move {
            let _completion = Completion {
                status: observed,
                finished: false,
            };
            panic!("offline original Palpo worker panic");
            #[allow(unreachable_code)]
            Ok(())
        });
        let mut owner = Owner {
            cancel: CancellationToken::new(),
            task: Some(task),
            joined: None,
        };
        assert_eq!(owner.close().await, Err(Failure::OutcomeUnknown));
        assert_eq!(owner.close().await, Err(Failure::OutcomeUnknown));
        assert_eq!(status.get().state, "outcome_unknown");
        assert_eq!(status.get().error, Some("worker"));
    }

    #[test]
    fn native_palpo_service_configuration_refusal_projection() {
        let status = StatusHandle::new(true);
        status.finish(Err(Error::OutcomeUnknown));
        let value = serde_json::to_string(&status.get()).unwrap();
        assert!(value.len() <= 128);
        status.finish(Err(Error::Remote(u16::MAX)));
        let value = serde_json::to_string(&status.get()).unwrap();
        assert!(!value.contains("65535"));
        assert_eq!(status.get().error, Some("remote"));
        assert_eq!(status.get().state, "unavailable");
    }
}
