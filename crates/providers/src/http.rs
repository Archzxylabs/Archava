//! Bounded native HTTP. Provider errors never carry request headers or response bodies.
use reqwest::{Client, RequestBuilder, Url};
use serde_json::Value;
use std::time::Duration;

pub const MAX_REPLY_BYTES: usize = 64 * 1024;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    #[error("provider configuration is invalid")]
    Configuration,
    #[error("provider request timed out; its outcome may be unknown")]
    Timeout,
    #[error("provider transport did not return a confirmed answer")]
    Unavailable,
    #[error("provider refused the request (HTTP {0})")]
    Status(u16),
    #[error("provider response exceeded its size limit")]
    TooLarge,
    #[error("provider response did not match its required protocol")]
    Protocol,
}
#[derive(Clone)]
pub(crate) struct Credential(String);
impl Credential {
    pub(crate) fn new(value: String) -> Result<Self, TransportError> {
        if value.trim().is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
            return Err(TransportError::Configuration);
        }
        Ok(Self(value))
    }
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credential([redacted])")
    }
}
pub(crate) fn client(local: bool) -> Result<Client, TransportError> {
    Client::builder()
        .https_only(!local)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(2))
        .timeout(REQUEST_TIMEOUT)
        .pool_max_idle_per_host(4)
        .user_agent("Archava native provider/0.1")
        .build()
        .map_err(|_| TransportError::Configuration)
}
pub(crate) fn loopback_origin(value: &str) -> Result<Url, TransportError> {
    let url = Url::parse(value).map_err(|_| TransportError::Configuration)?;
    let loopback = url
        .host_str()
        .and_then(|host| {
            host.trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .ok()
        })
        .is_some_and(|ip| ip.is_loopback());
    if !loopback
        || url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(TransportError::Configuration);
    }
    Ok(url)
}
pub(crate) async fn json(request: RequestBuilder) -> Result<Value, TransportError> {
    tokio::time::timeout(REQUEST_TIMEOUT, async move {
        let mut response = request.send().await.map_err(network)?;
        if !response.status().is_success() {
            return Err(TransportError::Status(response.status().as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_REPLY_BYTES as u64)
        {
            return Err(TransportError::TooLarge);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(network)? {
            if bytes
                .len()
                .checked_add(chunk.len())
                .is_none_or(|n| n > MAX_REPLY_BYTES)
            {
                return Err(TransportError::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| TransportError::Protocol)
    })
    .await
    .map_err(|_| TransportError::Timeout)?
}
fn network(error: reqwest::Error) -> TransportError {
    if error.is_timeout() {
        TransportError::Timeout
    } else {
        TransportError::Unavailable
    }
}
