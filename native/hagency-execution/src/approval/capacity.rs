use crate::Failure;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct ApprovalHost {
    inner: Arc<Shared>,
}
struct Shared {
    live_limit: u32,
    parked_limit: u32,
    owner_wait_ms: u64,
    response_reserve_ms: u64,
    counts: Mutex<(u32, u32)>,
}
impl ApprovalHost {
    pub fn new(
        live_limit: u32,
        parked_limit: u32,
        owner_wait_ms: u64,
        response_reserve_ms: u64,
    ) -> Result<Self, Failure> {
        if live_limit > 64
            || parked_limit == 0
            || parked_limit >= live_limit
            || owner_wait_ms == 0
            || response_reserve_ms == 0
            || owner_wait_ms
                .checked_add(response_reserve_ms)
                .is_none_or(|n| n > 30_000)
        {
            return Err(Failure::Admission);
        }
        Ok(Self {
            inner: Arc::new(Shared {
                live_limit,
                parked_limit,
                owner_wait_ms,
                response_reserve_ms,
                counts: Mutex::new((0, 0)),
            }),
        })
    }
    pub fn live_limit(&self) -> u32 {
        self.inner.live_limit
    }
    pub(crate) fn policy(&self) -> hagency_runtime::codex::session::ApprovalControlPolicy {
        hagency_runtime::codex::session::ApprovalControlPolicy {
            owner_wait_ms: self.inner.owner_wait_ms,
            response_reserve_ms: self.inner.response_reserve_ms,
        }
    }
    pub(crate) fn fits(&self, limits: crate::Limits) -> bool {
        self.inner.response_reserve_ms >= limits.response_ms
            && self.inner.owner_wait_ms + self.inner.response_reserve_ms <= limits.operation_ms
    }
    pub(crate) fn reserve_live(&self) -> Result<Reservation, Failure> {
        self.reserve(false)
    }
    pub(super) fn reserve_parked(&self) -> Result<Reservation, Failure> {
        self.reserve(true)
    }
    fn reserve(&self, parked: bool) -> Result<Reservation, Failure> {
        let mut counts = self.inner.counts.lock().map_err(|_| Failure::Worker)?;
        let (count, limit) = if parked {
            (&mut counts.1, self.inner.parked_limit)
        } else {
            (&mut counts.0, self.inner.live_limit)
        };
        if *count >= limit {
            return Err(Failure::ApprovalCapacity);
        }
        *count += 1;
        Ok(Reservation {
            host: self.clone(),
            parked,
            held: true,
            before_effect: true,
        })
    }
}

/// A possible effect cannot free capacity merely because its wrapper is lost.
/// The shared count remains occupied until a positively observed release.
pub(crate) struct Reservation {
    host: ApprovalHost,
    parked: bool,
    held: bool,
    before_effect: bool,
}
impl Reservation {
    pub(crate) fn possible(&mut self) {
        self.before_effect = false;
    }
    pub(crate) fn release(&mut self) {
        if self.held
            && let Ok(mut counts) = self.host.inner.counts.lock()
        {
            let count = if self.parked {
                &mut counts.1
            } else {
                &mut counts.0
            };
            if let Some(next) = count.checked_sub(1) {
                *count = next;
                self.held = false;
            }
        }
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if self.before_effect {
            self.release();
        }
    }
}
