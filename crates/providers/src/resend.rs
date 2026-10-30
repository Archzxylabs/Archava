//! Resend transactional email transport with 24-hour native idempotency keys.
//! Never logged with credentials or unredacted raw bodies.

use crate::http::{self, Credential, TransportError};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const RESEND_API_URL: &str = "https://api.resend.com/";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResendSuccess {
    pub id: String,
}

#[derive(Clone)]
pub struct ResendClient {
    client: Client,
    key: Credential,
    api: Url,
    local: bool,
}

impl std::fmt::Debug for ResendClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResendClient")
            .field("api", &self.api.origin().ascii_serialization())
            .finish_non_exhaustive()
    }
}

fn valid_idempotency_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 256
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl ResendClient {
    pub fn new(key: String) -> Result<Self, TransportError> {
        Ok(Self {
            client: http::client(false)?,
            key: Credential::new(key)?,
            api: Url::parse(RESEND_API_URL).map_err(|_| TransportError::Configuration)?,
            local: false,
        })
    }

    /// Finite local transport tests only. Production configuration selects the
    /// official Resend endpoint and refuses loopback/HTTP origins.
    pub fn for_loopback(origin: &str, key: String) -> Result<Self, TransportError> {
        let base = http::loopback_origin(origin)?;
        Ok(Self {
            client: http::client(true)?,
            key: Credential::new(key)?,
            api: base,
            local: true,
        })
    }

    pub fn is_local(&self) -> bool {
        self.local
    }

    pub async fn send_email(
        &self,
        idempotency_key: &str,
        from: &str,
        to: &str,
        subject: &str,
        text: &str,
        html: Option<&str>,
    ) -> Result<ResendSuccess, TransportError> {
        if !valid_idempotency_key(idempotency_key)
            || from.trim().is_empty()
            || to.trim().is_empty()
            || subject.trim().is_empty()
            || text.trim().is_empty()
        {
            return Err(TransportError::Protocol);
        }

        let mut url = self.api.clone();
        url.path_segments_mut()
            .map_err(|_| TransportError::Configuration)?
            .push("emails");

        let mut body = json!({
            "from": from,
            "to": [to],
            "subject": subject,
            "text": text,
        });

        if let Some(h) = html {
            if let Some(obj) = body.as_object_mut() {
                obj.insert("html".to_string(), json!(h));
            }
        }

        let response = http::json(
            self.client
                .post(url)
                .bearer_auth(self.key.expose())
                .header("Idempotency-Key", idempotency_key)
                .header("accept", "application/json")
                .json(&body),
        )
        .await?;

        let id = response
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty() && s.len() <= 256)
            .ok_or(TransportError::Protocol)?;

        Ok(ResendSuccess { id: id.to_string() })
    }
}
