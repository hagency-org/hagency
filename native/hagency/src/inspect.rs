//! Read-only operator inspection (brief 22): render exactly what the
//! operator routes already publish. No server change; no subcommand here
//! creates, verdicts, revokes or resolves anything. The client mirrors the
//! `console::client` pattern — loopback-only address, the local operator
//! token read privately, one bounded hyper exchange, every refusal named
//! the same way the route names it.
use http_body_util::{BodyExt, Full};
use hyper::{Request, body::Bytes, client::conn::http1};
use hyper_util::rt::TokioIo;
use std::{net::SocketAddr, path::Path, time::Duration};
use tokio::net::TcpStream;

/// The three reads (plus the usage-less parity the inventory listed): each
/// names its operator route and its table's columns — taken verbatim from
/// the route's own keys, never a derived figure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Alerts,
    Engagements,
    Resources,
}

impl Kind {
    fn path(self) -> &'static str {
        match self {
            Kind::Alerts => "/api/native/v1/alerts",
            Kind::Engagements => "/api/native/v1/engagements",
            Kind::Resources => "/api/native/v1/resources",
        }
    }
}

/// Refusal classes with DISTINCT exit codes — never a silent 0:
/// 3 unreachable (connect/handshake/timeout), 4 refused (401/403, or no
/// readable local operator credential), 5 invalid request (bad flags or a
/// route 400), 6 busy/unavailable (route 503 or any other server state),
/// 7 route not present (a 404: the running service does not mount that
/// read — e.g. a branch without the alerts slice — which is not a
/// malformed request; E4 of the CLI review).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Unreachable,
    Refused,
    Invalid,
    Unavailable,
    Missing,
}

impl Error {
    pub fn exit_code(self) -> i32 {
        match self {
            Error::Unreachable => 3,
            Error::Refused => 4,
            Error::Invalid => 5,
            Error::Unavailable => 6,
            Error::Missing => 7,
        }
    }
    pub fn describe(self) -> &'static str {
        match self {
            Error::Unreachable => "service unreachable",
            Error::Refused => "operator authority refused",
            Error::Invalid => "invalid inspection request",
            Error::Unavailable => "service busy or unavailable",
            Error::Missing => "route not present",
        }
    }
}

/// One bounded operator GET. The address must be loopback (the operator
/// boundary is local by construction, like `console::client`), the token is
/// read from the private state directory and never logged.
async fn fetch(kind: Kind, state: &Path, address: SocketAddr, limit: u32) -> Result<String, Error> {
    if !address.ip().is_loopback()
        || address.port() == 0
        || matches!(address, SocketAddr::V6(v) if v.scope_id() != 0 || v.flowinfo() != 0)
    {
        return Err(Error::Invalid);
    }
    // E3 of the CLI review: the limit is FORWARDED verbatim — including 0
    // and out-of-range values — so "the CLI never clamps, the route
    // refuses" is literally true: every bound lives in the route
    // (`page()` refuses 1..=100 for resources/engagements, the alerts
    // route 1..=200), and the CLI reports the route's own refusal class.
    let token = hagency_store::private::read_secret(&state.join("operator.token"))
        .map_err(|_| Error::Refused)?;
    let token = std::str::from_utf8(&token).map_err(|_| Error::Refused)?;
    if !(32..=256).contains(&token.len()) || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Error::Refused);
    }
    tokio::time::timeout(
        Duration::from_secs(5),
        exchange(kind.path(), address, token, limit),
    )
    .await
    .map_err(|_| Error::Unreachable)?
}

/// The operator credential header, built exactly like `console::client`'s
/// (`console/client.rs:65-67`): a `HeaderValue` marked SENSITIVE, so any
/// `Debug`/log of the request redacts the token (E1 of the CLI review). A
/// plain `.header(name, String)` would print `Bearer <token>` verbatim.
fn authorization(token: &str) -> Result<hyper::header::HeaderValue, Error> {
    let mut value = hyper::header::HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|_| Error::Invalid)?;
    value.set_sensitive(true);
    Ok(value)
}

async fn exchange(
    path: &str,
    address: SocketAddr,
    token: &str,
    limit: u32,
) -> Result<String, Error> {
    // Mirrors `console::client::exchange`: the connection future MUST be
    // polled beside the read or hyper makes no progress; select on both.
    let read = async {
        let stream = TcpStream::connect(address)
            .await
            .map_err(|_| Error::Unreachable)?;
        let (mut sender, connection) = http1::Builder::new()
            .max_headers(32)
            .max_buf_size(16 * 1024)
            .handshake::<_, Full<Bytes>>(TokioIo::new(stream))
            .await
            .map_err(|_| Error::Unreachable)?;
        // E1 of the CLI review: the authorization header is built exactly
        // like `console::client`'s — a HeaderValue marked SENSITIVE, so any
        // Debug/log of the request redacts the operator token. A plain
        // `.header(name, String)` would print it verbatim.
        let request = Request::builder()
            .method("GET")
            .uri(format!("{path}?limit={limit}"))
            .header("host", address.to_string())
            .header("authorization", authorization(token)?)
            .body(Full::<Bytes>::default())
            .map_err(|_| Error::Invalid)?;
        tokio::pin!(connection);
        let read = async {
            let response = sender
                .send_request(request)
                .await
                .map_err(|_| Error::Unreachable)?;
            let status = response.status();
            // Mirrors `console::client`: bounded collect, then into_data —
            // this hyper version's body exposes no frame-level API.
            let collected = response
                .into_body()
                .collect()
                .await
                .map_err(|_| Error::Unavailable)?;
            let bytes = collected.to_bytes();
            if bytes.len() > 512 * 1024 {
                return Err(Error::Unavailable);
            }
            match status.as_u16() {
                200 => Ok(String::from_utf8(bytes.to_vec()).map_err(|_| Error::Unavailable)?),
                400 => Err(Error::Invalid),
                404 => Err(Error::Missing),
                401 | 403 => Err(Error::Refused),
                _ => Err(Error::Unavailable),
            }
        };
        tokio::pin!(read);
        tokio::select! {
            result = &mut read => result,
            result = &mut connection => { result.map_err(|_| Error::Unreachable)?; read.await }
        }
    };
    read.await
}

/// Render the route's body: `--json` passes it through verbatim; otherwise
/// a table whose columns are the route's own keys (never a derived figure).
pub fn render(kind: Kind, body: &str) -> String {
    let value: serde_json::Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => return format!("{body}\n"),
    };
    let columns: &[&str] = match kind {
        // The envelope's own key `at_ms` leads; the rows carry the rest.
        Kind::Alerts => &[
            "dedupe_key",
            "resource_id",
            "occurrences",
            "resolved",
            "summary",
        ],
        Kind::Engagements => &[
            "id",
            "agentName",
            "projectId",
            "role",
            "requestedTokens",
            "state",
        ],
        Kind::Resources => &["id", "framework", "model", "tier", "ceiling"],
    };
    let rows: Vec<serde_json::Value> = match kind {
        Kind::Alerts => value["alerts"].as_array().cloned().unwrap_or_default(),
        _ => value.as_array().cloned().unwrap_or_default(),
    };
    let cell = |value: &serde_json::Value| -> String {
        match value {
            serde_json::Value::Null => "-".into(),
            serde_json::Value::String(text) => text.clone(),
            serde_json::Value::Bool(flag) => flag.to_string(),
            serde_json::Value::Number(number) => number.to_string(),
            other => other.to_string(),
        }
    };
    let mut table: Vec<Vec<String>> = vec![columns.iter().map(|c| c.to_string()).collect()];
    for row in &rows {
        table.push(columns.iter().map(|key| cell(&row[*key])).collect());
    }
    if let Some(at) = value.get("at_ms").and_then(serde_json::Value::as_u64) {
        table.push(vec![format!("at_ms: {at}")]);
    }
    let mut widths = vec![0usize; columns.len()];
    for row in &table {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.chars().count());
        }
    }
    let mut out = String::new();
    for row in &table {
        let last = row.len() - 1;
        for (index, cell) in row.iter().enumerate() {
            if index == last {
                out.push_str(cell);
            } else {
                out.push_str(&format!("{cell:w$}  ", w = widths[index]));
            }
        }
        out.push('\n');
    }
    out
}

/// The command entry the CLI arms call: fetch, render, and on refusal print
/// the refusal to stderr and exit with its distinct code — never a silent 0.
pub async fn run(kind: Kind, state: &Path, address: SocketAddr, limit: u32, json: bool) {
    match fetch(kind, state, address, limit).await {
        Ok(body) => {
            if json {
                print!("{body}");
            } else {
                print!("{}", render(kind, &body));
            }
        }
        Err(error) => {
            eprintln!("hagency: {}", error.describe());
            std::process::exit(error.exit_code());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// E1 of the CLI review: the operator credential header MUST be marked
    /// sensitive — a plain `.header(name, String)` value fails this
    /// assertion, and `HeaderValue`'s Debug redacts only when the flag is
    /// set (verified by rendering: the token never appears in the
    /// formatted header; a plain value would print it verbatim).
    #[test]
    fn native_inspection_authorization_header_is_sensitive() {
        let value = authorization("fixture_operator_token_32_bytes_minimum").unwrap();
        assert!(value.is_sensitive(), "a plain header value would fail");
        let rendered = format!("{value:?}");
        assert!(
            !rendered.contains("fixture_operator_token"),
            "Debug of the header must redact the token: {rendered}"
        );
        let plain = hyper::header::HeaderValue::from_str("Bearer fixture_operator_token").unwrap();
        assert!(
            !plain.is_sensitive(),
            "the control: a plain value is not flagged"
        );
        assert!(format!("{plain:?}").contains("fixture_operator_token"));
    }

    /// E4 of the CLI review: the missing-route class is distinct from
    /// invalid — exit 7 with its own name, never folded into 5.
    #[test]
    fn native_inspection_missing_route_is_its_own_class() {
        assert_eq!(Error::Missing.exit_code(), 7);
        assert_eq!(Error::Invalid.exit_code(), 5);
        assert_eq!(Error::Missing.describe(), "route not present");
        assert_ne!(
            Error::Missing.exit_code(),
            Error::Invalid.exit_code(),
            "404 must never read as a malformed request"
        );
    }
}
