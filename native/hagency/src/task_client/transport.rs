//! Closed operation dispatch shares the existing bounded local transport.
use super::{Context, Error, coordination, files, received};
use hagency_core::{project::identifier, tasks::TaskMutation};
use http_body_util::{BodyExt, Full};
use hyper::{Request, body::Bytes, client::conn::http1};
use hyper_util::rt::TokioIo;
use serde_json::json;
use std::time::Duration;
use tokio::{net::TcpStream, time::timeout};
const RESPONSE_LIMIT: usize = 64 * 1024;
pub(super) enum Operation<'a> {
    Complete(&'a hagency_core::completions::CompleteTaskWithReply),
    Task {
        operation: Option<&'a TaskMutation>,
        call_id: Option<&'a str>,
    },
    Coordination(&'a coordination::Command),
    Files(&'a files::Command),
    Received(&'a received::Command),
}
struct Prepared {
    path: String,
    body: Vec<u8>,
    method: &'static str,
    mutation: bool,
}
pub(super) async fn request(
    context: &Context,
    operation: Operation<'_>,
    deadline: Duration,
) -> Result<Vec<u8>, Error> {
    if deadline.is_zero() || deadline > Duration::from_secs(30) {
        return Err(Error::Invalid);
    }
    let response_limit = match &operation {
        Operation::Files(_) => 4096,
        Operation::Received(command) => command.response_limit(),
        _ => RESPONSE_LIMIT,
    };
    let (request, limit) = match operation {
        Operation::Received(command) => {
            command.validate(context)?;
            let (path, body, method) = command.wire()?;
            (
                Prepared {
                    path,
                    body,
                    method,
                    mutation: command.mutates(),
                },
                16 * 1024,
            )
        }
        Operation::Files(command) => {
            command.validate(context)?;
            let (path, body, method) = command.wire()?;
            (
                Prepared {
                    path,
                    body,
                    method,
                    mutation: command.mutates(),
                },
                16 * 1024,
            )
        }
        Operation::Complete(input) => {
            input.validate().map_err(|_| Error::Invalid)?;
            if input.id != context.task_id {
                return Err(Error::Invalid);
            }
            (
                Prepared {
                    path: "/api/native/v1/runner/complete-task-with-reply".into(),
                    body: serde_json::to_vec(input).map_err(|_| Error::Invalid)?,
                    method: "POST",
                    mutation: true,
                },
                32 * 1024,
            )
        }
        Operation::Task { operation, call_id } => {
            let (path, body, method) = if let Some(operation) = operation {
                let call = call_id.ok_or(Error::Invalid)?;
                identifier(call, 512).map_err(|_| Error::Invalid)?;
                (
                    format!("/api/native/v1/runner/tasks/{}/operations", context.task_id),
                    serde_json::to_vec(&json!({"call_id":call,"operation":operation}))
                        .map_err(|_| Error::Invalid)?,
                    "POST",
                )
            } else {
                if call_id.is_some() {
                    return Err(Error::Invalid);
                }
                (
                    format!("/api/native/v1/runner/tasks/{}", context.task_id),
                    vec![],
                    "GET",
                )
            };
            (
                Prepared {
                    path,
                    body,
                    method,
                    mutation: operation.is_some(),
                },
                16 * 1024,
            )
        }
        Operation::Coordination(command) => {
            command.validate(context)?;
            let (path, body, method) = command.wire()?;
            (
                Prepared {
                    path,
                    body,
                    method,
                    mutation: command.mutates(),
                },
                32 * 1024,
            )
        }
    };
    if request.body.len() > limit {
        return Err(Error::Invalid);
    }
    let mutation = request.mutation;
    let mut submitted = false;
    match timeout(
        deadline,
        exchange(context, request, response_limit, &mut submitted),
    )
    .await
    {
        Ok(v) => v,
        Err(_) => Err(if submitted && mutation {
            Error::Unknown
        } else {
            Error::Unavailable
        }),
    }
}
async fn exchange(
    context: &Context,
    request: Prepared,
    response_limit: usize,
    submitted: &mut bool,
) -> Result<Vec<u8>, Error> {
    let Prepared {
        path,
        body,
        mutation,
        method,
    } = request;
    let stream = TcpStream::connect(context.address)
        .await
        .map_err(|_| Error::Unavailable)?;
    let (mut sender, connection) = http1::Builder::new()
        .max_headers(32)
        .max_buf_size(16 * 1024)
        .handshake::<_, Full<Bytes>>(TokioIo::new(stream))
        .await
        .map_err(|_| Error::Unavailable)?;
    let mut authorization =
        hyper::header::HeaderValue::from_str(&format!("Bearer {}", context.capability.secret))
            .map_err(|_| Error::Invalid)?;
    authorization.set_sensitive(true);
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", context.address.to_string())
        .header("authorization", authorization)
        .header("x-hagency-dispatch", &context.capability.dispatch_id)
        .header("x-hagency-runner", &context.capability.runner_id)
        .header("x-hagency-fence", context.capability.fence.to_string())
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .header("connection", "close")
        .body(Full::new(Bytes::from(body)))
        .map_err(|_| Error::Invalid)?;
    let failure = if mutation {
        Error::Unknown
    } else {
        Error::Response
    };
    *submitted = true;
    let response = async {
        let mut response = sender.send_request(request).await.map_err(|_| failure)?;
        let status = response.status();
        if !status.is_success() {
            // Never consume/log private error bodies, follow redirects or retry.
            return Err(if mutation && status.is_server_error() {
                Error::Unknown
            } else {
                Error::Refused(status.as_u16())
            });
        }
        if status.as_u16() != 200
            || response.headers().get_all("content-type").iter().count() != 1
            || response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .is_none_or(|s| {
                    s.split(';')
                        .next()
                        .is_none_or(|s| s.trim() != "application/json")
                })
            || response.headers().contains_key("content-encoding")
        {
            return Err(failure);
        }
        if let Some(length) = response.headers().get("content-length") {
            let length: usize = length
                .to_str()
                .ok()
                .and_then(|s| s.parse().ok())
                .ok_or(failure)?;
            if length > response_limit {
                return Err(failure);
            }
        }
        let mut bytes = Vec::new();
        while let Some(frame) = response.body_mut().frame().await {
            let frame = frame.map_err(|_| failure)?;
            let data = frame.into_data().map_err(|_| failure)?;
            if bytes
                .len()
                .checked_add(data.len())
                .is_none_or(|len| len > response_limit)
            {
                return Err(failure);
            }
            bytes.extend_from_slice(&data);
        }
        Ok(bytes)
    };
    tokio::pin!(response, connection);
    // Poll the connection in this operation, without detached tasks. Cancelling
    // the outer future drops the connection and closes its owned socket.
    tokio::select! {
        biased;
        result = &mut response => result,
        result = &mut connection => {
            result.map_err(|_|failure)?;
            response.await
        }
    }
}
