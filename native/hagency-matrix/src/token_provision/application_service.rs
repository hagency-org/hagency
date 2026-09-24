//! Fixed side master, separate from its private dedicated SDK device session.
use super::custody::{Custody, Responses};
use super::{Context, SavedResponse, TokenAccountProvision, WHOAMI, checkpoint, write};
use crate::{CancellationToken, Error, Limits, http::Http};
use hagency_core::{canonical, project};
use reqwest::{Url, header::HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::time::{Instant, timeout_at};

/// Process Host configuration only; no serde/debug/clone or secret getter.
pub struct ApplicationServiceCredential {
    pub(crate) token: String,
    pub(crate) namespace: String,
}
impl ApplicationServiceCredential {
    pub fn new(token: &str, namespace_prefix: &str) -> Result<Self, Error> {
        if !(16..=4096).contains(&token.len())
            || !token.bytes().all(|b| (33..=126).contains(&b))
            || namespace_prefix.is_empty()
            || namespace_prefix.len() > 128
            || !namespace_prefix
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(Error::Config);
        }
        Ok(Self {
            token: token.into(),
            namespace: namespace_prefix.into(),
        })
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Profile {
    pub namespace: String,
    pub sender: String,
}
/// Immutable private side capability. No runtime result or callback can mint it.
pub(crate) struct Guard {
    http: Http,
    sender: String,
    user: String,
    outside: String,
    binding: String,
}
impl Guard {
    pub(super) fn new(
        endpoint: &Url,
        token: &str,
        context: &Context,
        limits: &Limits,
        roots: &[reqwest::Certificate],
    ) -> Result<Self, Error> {
        let profile = context.application_service.as_ref().ok_or(Error::Config)?;
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| Error::Config)?;
        authorization.set_sensitive(true);
        let outside = format!(
            "@hagency_namespace_probe_{}:{}",
            project::hash(context.user.as_bytes()),
            context.registration.server
        );
        Ok(Self {
            http: Http::for_host(endpoint, Some(&authorization), limits, roots)?,
            sender: profile.sender.clone(),
            user: context.user.clone(),
            outside,
            binding: canonical::transport_digest(&json!(context)).map_err(|_| Error::Config)?,
        })
    }
    pub(crate) fn binding(&self) -> &str {
        &self.binding
    }
    async fn identity(
        &self,
        user: &str,
        masquerade: bool,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        let query = [("user_id", user)];
        let value = self
            .http
            .request(WHOAMI, masquerade.then_some(query.as_slice()), cancel)
            .await?
            .success()?;
        if value.get("user_id").and_then(|v| v.as_str()) != Some(user)
            || value.get("is_guest").is_some_and(|v| v != &json!(false))
        {
            return Err(Error::Identity);
        }
        Ok(())
    }
    pub(super) async fn sender(&self, cancel: &CancellationToken) -> Result<(), Error> {
        self.identity(&self.sender, false, cancel).await?;
        let response = self
            .http
            .request(WHOAMI, Some(&[("user_id", &self.outside)]), cancel)
            .await?;
        if response.status != 403
            || !matches!(
                response
                    .value
                    .as_ref()
                    .and_then(|v| v.get("errcode"))
                    .and_then(|v| v.as_str()),
                Some("M_EXCLUSIVE" | "M_FORBIDDEN")
            )
        {
            return Err(Error::Identity);
        }
        Ok(())
    }
    pub(crate) async fn check(&self, cancel: &CancellationToken) -> Result<(), Error> {
        self.sender(cancel).await?;
        self.identity(&self.user, true, cancel).await
    }
}
impl TokenAccountProvision {
    pub(super) async fn application_service_run(
        &mut self,
        custody: &Arc<Custody>,
        records: Responses,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<SavedResponse, Error> {
        let guard = Arc::new(Guard::new(
            &self.endpoint,
            &self.token,
            &self.context,
            &self.http_limits(),
            &self.roots,
        )?);
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {}", self.token)).map_err(|_| Error::Config)?;
        authorization.set_sensitive(true);
        let http = Http::for_host(
            &self.endpoint,
            Some(&authorization),
            &self.http_limits(),
            &self.roots,
        )?;
        let initial = if let Some(initial) = records.initial {
            initial
        } else {
            if records.possible {
                return Err(Error::OutcomeUnknown);
            }
            timeout_at(deadline, guard.sender(cancel))
                .await
                .map_err(|_| Error::OutcomeUnknown)??;
            self.current(cancel, deadline).await?;
            write(custody, "possible", json!(null)).await?;
            timeout_at(deadline, guard.sender(cancel))
                .await
                .map_err(|_| Error::OutcomeUnknown)??;
            let body=serde_json::to_string(&json!({"type":"m.login.application_service",
                "username":self.context.user.split_once(':').ok_or(Error::Config)?.0.trim_start_matches('@'),"inhibit_login":true}))
                .map_err(|_|Error::Config)?;
            let response = self
                .post(&http, super::REGISTER, body, cancel, deadline)
                .await?;
            write(
                custody,
                "initial",
                serde_json::to_value(&response).map_err(|_| Error::Storage)?,
            )
            .await?;
            response
        };
        if initial.status != 200 {
            return Err(remote(&initial));
        }
        let value = initial.value.as_ref().ok_or(Error::Wire)?;
        if value.get("user_id").and_then(|v| v.as_str()) != Some(&self.context.user)
            || value
                .get("home_server")
                .is_some_and(|v| v.as_str() != Some(&self.context.registration.server))
            || [
                "access_token",
                "device_id",
                "refresh_token",
                "expires_in_ms",
            ]
            .iter()
            .any(|k| value.get(k).is_some())
        {
            return Err(Error::Identity);
        }
        self.as_guard = Some(guard);
        let response = if let Some(response) = records.auth {
            if !records.auth_possible {
                return Err(Error::Storage);
            }
            response
        } else {
            if records.auth_possible || records.complete.is_some() {
                return Err(Error::OutcomeUnknown);
            }
            self.current(cancel, deadline).await?;
            write(custody, "login-possible", json!(null)).await?;
            let body=serde_json::to_string(&json!({"type":"m.login.application_service","identifier":{"type":"m.id.user","user":self.context.user},
                "device_id":self.context.device,"refresh_token":false,"initial_device_display_name":"Hagency private agent device"})).map_err(|_|Error::Config)?;
            let response = self
                .post(
                    &http,
                    &["_matrix", "client", "v3", "login"],
                    body,
                    cancel,
                    deadline,
                )
                .await?;
            write(
                custody,
                "login",
                serde_json::to_value(&response).map_err(|_| Error::Storage)?,
            )
            .await?;
            response
        };
        if response.status != 200 {
            return Err(remote(&response));
        }
        checkpoint(cancel, deadline)?;
        Ok(response)
    }
}
fn remote(response: &SavedResponse) -> Error {
    if response
        .value
        .as_ref()
        .and_then(|v| v.get("errcode"))
        .and_then(|v| v.as_str())
        == Some("M_APPSERVICE_LOGIN_UNSUPPORTED")
    {
        Error::Unsupported
    } else if matches!(response.status, 401 | 403) {
        Error::Unauthorized
    } else {
        Error::Remote(response.status)
    }
}
