//! Finite in-memory browser authority, distinct from the operator credential.
use super::Error;
use hagency_store::{
    ResourceConfigurationAccess, ResourceConfigurationCommand, ResourcePublicationAccess,
    ResourcePublicationCommand, ResourcePublicationRetirement,
};
use sha2::{Digest, Sha256};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;

const TICKET_LIFETIME: Duration = Duration::from_secs(120);
const SESSION_LIFETIME: Duration = Duration::from_secs(15 * 60);
pub(super) const COOKIE: &str = "hagency_console";

#[derive(Clone, Copy)]
enum Scope {
    ReadOnly,
    Publication,
    Configuration,
}
enum MutationAccess {
    Publication(ResourcePublicationAccess),
    Configuration(ResourceConfigurationAccess),
}
impl MutationAccess {
    fn revoke(&self) -> Result<(), hagency_store::Error> {
        match self {
            Self::Publication(access) => access.revoke(),
            Self::Configuration(access) => access.revoke(),
        }
    }
}
struct Grant {
    hash: [u8; 32],
    expires: Instant,
    scope: Scope,
    mutation: Option<MutationAccess>,
}
struct State {
    retired: bool,
    ticket: Option<Grant>,
    sessions: Vec<Grant>,
    issued: Option<Instant>,
}
pub(super) struct Authority(Mutex<State>, ResourcePublicationRetirement);
pub(super) struct Session([u8; 32]);

fn secret() -> Result<String, Error> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn hash(value: &str) -> Result<[u8; 32], Error> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(Error::Unauthorized);
    }
    Ok(Sha256::digest(value.as_bytes()).into())
}
fn matches(grant: &Grant, digest: &[u8; 32], now: Instant) -> bool {
    bool::from(grant.hash.ct_eq(digest)) && now < grant.expires
}
impl Authority {
    pub(super) fn new() -> Self {
        Self(
            Mutex::new(State {
                retired: false,
                ticket: None,
                sessions: Vec::new(),
                issued: None,
            }),
            ResourcePublicationRetirement::default(),
        )
    }
    pub(super) fn issue(&self) -> Result<String, Error> {
        self.issue_with(Instant::now)
    }
    pub(super) fn issue_configuration(&self) -> Result<String, Error> {
        self.issue_scope(Instant::now, Scope::Configuration)
    }
    pub(super) fn issue_publication(&self) -> Result<String, Error> {
        self.issue_scope(Instant::now, Scope::Publication)
    }
    #[cfg(test)]
    fn issue_at(&self, now: Instant) -> Result<String, Error> {
        self.issue_with(|| now)
    }
    fn issue_with(&self, clock: impl FnOnce() -> Instant) -> Result<String, Error> {
        self.issue_scope(clock, Scope::ReadOnly)
    }
    fn issue_scope(&self, clock: impl FnOnce() -> Instant, scope: Scope) -> Result<String, Error> {
        let mut state = self.0.lock().map_err(|_| Error::Unavailable)?;
        let now = clock();
        if state.retired {
            return Err(Error::Unavailable);
        }
        if state
            .issued
            .is_some_and(|at| now.saturating_duration_since(at) < Duration::from_secs(1))
        {
            return Err(Error::Busy);
        }
        let value = secret()?;
        state.ticket = Some(Grant {
            hash: hash(&value)?,
            expires: now + TICKET_LIFETIME,
            scope,
            mutation: None,
        });
        state.issued = Some(now);
        Ok(value)
    }
    pub(super) fn exchange(&self, ticket: &str) -> Result<String, Error> {
        self.exchange_with(ticket, Instant::now)
    }
    #[cfg(test)]
    fn exchange_at(&self, ticket: &str, now: Instant) -> Result<String, Error> {
        self.exchange_with(ticket, || now)
    }
    fn exchange_with(
        &self,
        ticket: &str,
        clock: impl FnOnce() -> Instant,
    ) -> Result<String, Error> {
        let digest = hash(ticket)?;
        let mut state = self.0.lock().map_err(|_| Error::Unavailable)?;
        let now = clock();
        if state.retired {
            return Err(Error::Unavailable);
        }
        state.sessions.retain(|s| now < s.expires);
        if !state
            .ticket
            .as_ref()
            .is_some_and(|t| matches(t, &digest, now))
        {
            return Err(Error::Unauthorized);
        }
        if state.sessions.len() >= 4 {
            return Err(Error::Busy);
        }
        let value = secret()?;
        let scope = state.ticket.as_ref().ok_or(Error::Unauthorized)?.scope;
        state.ticket = None;
        let mutation = match scope {
            Scope::ReadOnly => None,
            Scope::Publication => Some(MutationAccess::Publication(
                ResourcePublicationAccess::new(now + SESSION_LIFETIME, self.1.clone()),
            )),
            Scope::Configuration => Some(MutationAccess::Configuration(
                ResourceConfigurationAccess::new(now + SESSION_LIFETIME, self.1.clone()),
            )),
        };
        state.sessions.push(Grant {
            hash: hash(&value)?,
            expires: now + SESSION_LIFETIME,
            scope,
            mutation,
        });
        Ok(value)
    }
    pub(super) fn authenticate(&self, value: &str) -> Result<Session, Error> {
        let session = Session(hash(value)?);
        self.check(&session)?;
        Ok(session)
    }
    pub(super) fn check(&self, session: &Session) -> Result<(), Error> {
        self.check_with(session, Instant::now)
    }
    #[cfg(test)]
    fn check_at(&self, session: &Session, now: Instant) -> Result<(), Error> {
        self.check_with(session, || now)
    }
    fn check_with(&self, session: &Session, clock: impl FnOnce() -> Instant) -> Result<(), Error> {
        let state = self.0.lock().map_err(|_| Error::Unavailable)?;
        let now = clock();
        if state.retired {
            return Err(Error::Unavailable);
        }
        if state.sessions.iter().any(|s| matches(s, &session.0, now)) {
            Ok(())
        } else {
            Err(Error::Unauthorized)
        }
    }
    pub(super) fn revoke(&self, session: &Session) -> Result<(), Error> {
        let mut state = self.0.lock().map_err(|_| Error::Unavailable)?;
        if let Some(access) = state
            .sessions
            .iter()
            .find(|s| bool::from(s.hash.ct_eq(&session.0)))
            .and_then(|s| s.mutation.as_ref())
        {
            access.revoke().map_err(|e| {
                if matches!(e, hagency_store::Error::Busy) {
                    Error::Busy
                } else {
                    Error::Unavailable
                }
            })?;
        }
        state
            .sessions
            .retain(|s| !bool::from(s.hash.ct_eq(&session.0)));
        Ok(())
    }
    pub(super) fn retire(&self) {
        self.1.retire();
        if let Ok(mut state) = self.0.lock() {
            state.retired = true;
            state.ticket = None;
            state.sessions.clear();
        }
    }
    pub(super) fn can_publish(&self, session: &Session) -> Result<bool, Error> {
        let state = self.0.lock().map_err(|_| Error::Unavailable)?;
        if state.retired {
            return Err(Error::Unavailable);
        }
        let now = Instant::now();
        state
            .sessions
            .iter()
            .find(|s| matches(s, &session.0, now))
            .map(|s| matches!(&s.mutation, Some(MutationAccess::Publication(_))))
            .ok_or(Error::Unauthorized)
    }
    pub(super) fn can_configure(&self, session: &Session) -> Result<bool, Error> {
        let state = self.0.lock().map_err(|_| Error::Unavailable)?;
        if state.retired {
            return Err(Error::Unavailable);
        }
        let now = Instant::now();
        state
            .sessions
            .iter()
            .find(|s| matches(s, &session.0, now))
            .map(|s| matches!(&s.mutation, Some(MutationAccess::Configuration(_))))
            .ok_or(Error::Unauthorized)
    }
    pub(super) fn configuration(
        &self,
        session: &Session,
        input: super::resource_configuration::PreparedInput,
        deadline: Instant,
    ) -> Result<ResourceConfigurationCommand, Error> {
        let state = self.0.lock().map_err(|_| Error::Unavailable)?;
        if state.retired {
            return Err(Error::Unavailable);
        }
        let now = Instant::now();
        let grant = state
            .sessions
            .iter()
            .find(|s| matches(s, &session.0, now))
            .ok_or(Error::Unauthorized)?;
        let Some(MutationAccess::Configuration(access)) = &grant.mutation else {
            return Err(Error::ConfigurationForbidden);
        };
        access
            .prepare(
                input.resource,
                input.revision,
                input.create,
                input.profile,
                input.ceiling,
                deadline,
            )
            .map_err(|e| match e {
                hagency_store::Error::Busy => Error::Busy,
                hagency_store::Error::LocalAuthority => Error::Unauthorized,
                hagency_store::Error::Invalid(_) => Error::Invalid,
                _ => Error::Unavailable,
            })
    }
    pub(super) fn publication(
        &self,
        session: &Session,
        resource: String,
        revision: String,
        published: bool,
        deadline: Instant,
    ) -> Result<ResourcePublicationCommand, Error> {
        let state = self.0.lock().map_err(|_| Error::Unavailable)?;
        if state.retired {
            return Err(Error::Unavailable);
        }
        let now = Instant::now();
        let grant = state
            .sessions
            .iter()
            .find(|s| matches(s, &session.0, now))
            .ok_or(Error::Unauthorized)?;
        let Some(MutationAccess::Publication(access)) = &grant.mutation else {
            return Err(Error::Forbidden);
        };
        access
            .prepare(resource, revision, published, deadline)
            .map_err(|e| match e {
                hagency_store::Error::Busy => Error::Busy,
                hagency_store::Error::LocalAuthority => Error::Unauthorized,
                hagency_store::Error::Invalid(_) => Error::Invalid,
                _ => Error::Unavailable,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_console_finite_clock() {
        let authority = Authority::new();
        let now = Instant::now();
        let first = authority.issue_at(now).unwrap();
        assert!(matches!(authority.issue_at(now), Err(Error::Busy)));
        let replaced = authority.issue_at(now + Duration::from_secs(1)).unwrap();
        assert!(authority.exchange_at(&first, now).is_err());
        assert!(
            authority
                .exchange_at(&replaced, now + Duration::from_secs(121))
                .is_err()
        );
        for i in 2..6 {
            let at = now + Duration::from_secs(i);
            let ticket = authority.issue_at(at).unwrap();
            let cookie = authority.exchange_at(&ticket, at).unwrap();
            assert!(authority.exchange_at(&ticket, at).is_err());
            let session = Session(hash(&cookie).unwrap());
            authority
                .check_at(&session, at + Duration::from_secs(899))
                .unwrap();
            assert!(
                authority
                    .check_at(&session, at + Duration::from_secs(900))
                    .is_err()
            );
        }
        let ticket = authority.issue_at(now + Duration::from_secs(6)).unwrap();
        assert!(matches!(
            authority.exchange_at(&ticket, now + Duration::from_secs(6)),
            Err(Error::Busy)
        ));
        authority.retire();
        assert!(authority.issue().is_err());
    }

    #[test]
    fn native_console_clock_after_lock() {
        use std::sync::{Arc, mpsc};
        // Inspect the actual mutex at each production clock callback. Sampling
        // before acquisition would obtain this lock and fail deterministically.
        let ordered = Authority::new();
        let clock = || {
            assert!(matches!(
                ordered.0.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            ));
            Instant::now()
        };
        let ticket = ordered.issue_with(clock).unwrap();
        let cookie = ordered.exchange_with(&ticket, clock).unwrap();
        ordered
            .check_with(&Session(hash(&cookie).unwrap()), clock)
            .unwrap();
        for ticket_check in [false, true] {
            let authority = Arc::new(Authority::new());
            let ticket = authority.issue().unwrap();
            let credential = if ticket_check {
                ticket
            } else {
                authority.exchange(&ticket).unwrap()
            };
            let mut held = authority.0.lock().unwrap();
            let expires = Instant::now() + Duration::from_millis(80);
            if ticket_check {
                held.ticket.as_mut().unwrap().expires = expires;
            } else {
                held.sessions[0].expires = expires;
            }
            let (began, entered) = mpsc::channel();
            let other = authority.clone();
            let call = std::thread::spawn(move || {
                began.send(()).unwrap();
                if ticket_check {
                    other.exchange(&credential).map(|_| ())
                } else {
                    other.authenticate(&credential).map(|_| ())
                }
            });
            entered.recv().unwrap();
            std::thread::sleep(
                expires.saturating_duration_since(Instant::now()) + Duration::from_millis(20),
            );
            drop(held);
            assert!(matches!(call.join().unwrap(), Err(Error::Unauthorized)));
        }
    }
    #[test]
    fn native_console_resource_scope_expiry() {
        let authority = Authority::new();
        let now = Instant::now();
        let ticket = authority.issue_scope(|| now, Scope::Publication).unwrap();
        assert!(matches!(
            authority.exchange_at(&ticket, now + TICKET_LIFETIME),
            Err(Error::Unauthorized)
        ));
        let ticket = authority
            .issue_scope(|| now + Duration::from_secs(1), Scope::Publication)
            .unwrap();
        let cookie = authority
            .exchange_at(&ticket, now + Duration::from_secs(1))
            .unwrap();
        let session = authority.authenticate(&cookie).unwrap();
        assert!(authority.can_publish(&session).unwrap());
        let command = authority
            .publication(
                &session,
                "resource".into(),
                "a".repeat(64),
                false,
                Instant::now() + Duration::from_secs(2),
            )
            .unwrap();
        assert!(matches!(authority.revoke(&session), Err(Error::Busy)));
        authority.check(&session).unwrap();
        drop(command);
        authority.revoke(&session).unwrap();
        assert!(matches!(
            authority.check(&session),
            Err(Error::Unauthorized)
        ));
        let ticket = authority
            .issue_scope(|| now + Duration::from_secs(2), Scope::Publication)
            .unwrap();
        let cookie = authority
            .exchange_at(&ticket, now + Duration::from_secs(2))
            .unwrap();
        let session = Session(hash(&cookie).unwrap());
        assert!(matches!(
            authority.check_at(&session, now + Duration::from_secs(2) + SESSION_LIFETIME),
            Err(Error::Unauthorized)
        ));
        authority.retire();
        assert!(matches!(
            authority.publication(
                &session,
                "resource".into(),
                "a".repeat(64),
                false,
                Instant::now() + Duration::from_secs(2)
            ),
            Err(Error::Unavailable)
        ));
    }
    #[test]
    fn native_console_configuration_scope_expiry() {
        let authority = Authority::new();
        let now = Instant::now();
        let ticket = authority.issue_scope(|| now, Scope::Configuration).unwrap();
        assert!(matches!(
            authority.exchange_at(&ticket, now + TICKET_LIFETIME),
            Err(Error::Unauthorized)
        ));
        let ticket = authority
            .issue_scope(|| now + Duration::from_secs(1), Scope::Configuration)
            .unwrap();
        let cookie = authority
            .exchange_at(&ticket, now + Duration::from_secs(1))
            .unwrap();
        let session = authority.authenticate(&cookie).unwrap();
        assert!(authority.can_configure(&session).unwrap());
        assert!(!authority.can_publish(&session).unwrap());
        let input = || super::super::resource_configuration::PreparedInput {
            resource: "resource".into(),
            revision: "a".repeat(64),
            create: false,
            profile: hagency_store::ProfileChange::Preserve {},
            ceiling: hagency_store::CeilingChange::Preserve {},
        };
        let command = authority
            .configuration(&session, input(), now + Duration::from_secs(2))
            .unwrap();
        assert!(matches!(authority.revoke(&session), Err(Error::Busy)));
        drop(command);
        assert!(matches!(
            authority.check_at(&session, now + Duration::from_secs(1) + SESSION_LIFETIME),
            Err(Error::Unauthorized)
        ));
        authority.retire();
        assert!(matches!(
            authority.configuration(&session, input(), now + Duration::from_secs(2)),
            Err(Error::Unavailable)
        ));
    }
}
