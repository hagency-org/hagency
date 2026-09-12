//! Local operator command; its output contains only a short-lived ticket for the explicitly selected scope.
use super::Error;
use http_body_util::{BodyExt, Full};
use hyper::{Request, body::Bytes, client::conn::http1};
use hyper_util::rt::TokioIo;
use serde::Deserialize;
use std::{net::SocketAddr, path::Path, time::Duration};
use tokio::net::TcpStream;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Issued {
    ticket: String,
    expires_in: u64,
}
pub async fn access(state: &Path, address: SocketAddr) -> Result<String, Error> {
    scoped_access(state, address, false, false).await
}
pub async fn publication_access(state: &Path, address: SocketAddr) -> Result<String, Error> {
    scoped_access(state, address, true, false).await
}
pub async fn configuration_access(state: &Path, address: SocketAddr) -> Result<String, Error> {
    scoped_access(state, address, false, true).await
}
async fn scoped_access(
    state: &Path,
    address: SocketAddr,
    publication: bool,
    configuration: bool,
) -> Result<String, Error> {
    if !address.ip().is_loopback()
        || address.port() == 0
        || matches!(address, SocketAddr::V6(v) if v.scope_id()!=0 || v.flowinfo()!=0)
    {
        return Err(Error::Invalid);
    }
    let token = hagency_store::private::read_secret(&state.join("operator.token"))
        .map_err(|_| Error::Unavailable)?;
    let token = std::str::from_utf8(&token).map_err(|_| Error::Unavailable)?;
    if !(32..=256).contains(&token.len()) || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Error::Unavailable);
    }
    tokio::time::timeout(
        Duration::from_secs(5),
        exchange(address, token, publication, configuration),
    )
    .await
    .map_err(|_| Error::Unavailable)?
}
async fn exchange(
    address: SocketAddr,
    token: &str,
    publication: bool,
    configuration: bool,
) -> Result<String, Error> {
    let stream = TcpStream::connect(address)
        .await
        .map_err(|_| Error::Unavailable)?;
    let (mut sender, connection) = http1::Builder::new()
        .max_headers(32)
        .max_buf_size(16 * 1024)
        .handshake::<_, Full<Bytes>>(TokioIo::new(stream))
        .await
        .map_err(|_| Error::Unavailable)?;
    let mut authorization = hyper::header::HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|_| Error::Invalid)?;
    authorization.set_sensitive(true);
    let request = Request::builder()
        .method("POST")
        .uri(if configuration {
            "/api/native/v1/console/resource-configuration-access"
        } else if publication {
            "/api/native/v1/console/resource-publication-access"
        } else {
            "/api/native/v1/console/access"
        })
        .header("host", address.to_string())
        .header("authorization", authorization)
        .header("connection", "close")
        .body(Full::new(Bytes::new()))
        .map_err(|_| Error::Invalid)?;
    let read = async {
        let mut response = sender
            .send_request(request)
            .await
            .map_err(|_| Error::Unavailable)?;
        if response.status().as_u16() != 200
            || response.headers().contains_key("content-encoding")
            || response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .is_none_or(|v| !v.starts_with("application/json"))
        {
            return Err(Error::Unavailable);
        }
        let mut bytes = Vec::new();
        while let Some(frame) = response.body_mut().frame().await {
            let data = frame
                .map_err(|_| Error::Unavailable)?
                .into_data()
                .map_err(|_| Error::Unavailable)?;
            if bytes.len().saturating_add(data.len()) > 256 {
                return Err(Error::Unavailable);
            }
            bytes.extend_from_slice(&data);
        }
        let issued: Issued = serde_json::from_slice(&bytes).map_err(|_| Error::Unavailable)?;
        if issued.expires_in != 120
            || issued.ticket.len() != 64
            || !issued
                .ticket
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(Error::Unavailable);
        }
        Ok(format!(
            "http://{address}/console/{}/#access={}",
            if publication || configuration {
                "resources"
            } else {
                "usage"
            },
            issued.ticket
        ))
    };
    tokio::pin!(read, connection);
    tokio::select! {
        result = &mut read => result,
        result = &mut connection => { result.map_err(|_| Error::Unavailable)?; read.await }
    }
}
