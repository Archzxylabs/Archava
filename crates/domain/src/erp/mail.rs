//! Transactional email domain grammar for invoice/statement snapshots.
//! No sockets or external network calls live here; domain logic is WASM-safe.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmailState {
    Queued,
    Invoking,
    Accepted,
    Unknown,
    Rejected,
    Held,
}

impl EmailState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Invoking => "invoking",
            Self::Accepted => "accepted",
            Self::Unknown => "unknown",
            Self::Rejected => "rejected",
            Self::Held => "held",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "queued" => Some(Self::Queued),
            "invoking" => Some(Self::Invoking),
            "accepted" => Some(Self::Accepted),
            "unknown" => Some(Self::Unknown),
            "rejected" => Some(Self::Rejected),
            "held" => Some(Self::Held),
            _ => None,
        }
    }

    /// An email that has reached a terminal or holding state cannot transition
    /// to invoking without an explicit re-intent.
    pub fn can_invoke(&self) -> bool {
        matches!(self, Self::Queued | Self::Unknown)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EmailValidationError {
    #[error("recipient email address is invalid")]
    InvalidRecipient,
    #[error("subject contains illegal characters or invalid length")]
    InvalidSubject,
    #[error("body cannot be empty or exceeds maximum size")]
    InvalidBody,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailRecipient(String);

impl EmailRecipient {
    pub fn new(address: &str) -> Result<Self, EmailValidationError> {
        let trimmed = address.trim();
        if trimmed.len() < 3 || trimmed.len() > 320 {
            return Err(EmailValidationError::InvalidRecipient);
        }
        if trimmed.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(EmailValidationError::InvalidRecipient);
        }
        let parts: Vec<&str> = trimmed.split('@').collect();
        if parts.len() != 2 {
            return Err(EmailValidationError::InvalidRecipient);
        }
        let (local, domain) = (parts[0], parts[1]);
        if local.is_empty() || local.len() > 64 || domain.is_empty() || domain.len() > 255 {
            return Err(EmailValidationError::InvalidRecipient);
        }
        if !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
            return Err(EmailValidationError::InvalidRecipient);
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailSubject(String);

impl EmailSubject {
    pub fn new(subject: &str) -> Result<Self, EmailValidationError> {
        let trimmed = subject.trim();
        if trimmed.is_empty() || trimmed.len() > 256 {
            return Err(EmailValidationError::InvalidSubject);
        }
        if trimmed.contains('\r') || trimmed.contains('\n') {
            return Err(EmailValidationError::InvalidSubject);
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailPayload {
    pub recipient: EmailRecipient,
    pub subject: EmailSubject,
    pub text_body: String,
    pub html_body: Option<String>,
}

impl EmailPayload {
    pub fn new(
        recipient: EmailRecipient,
        subject: EmailSubject,
        text_body: String,
        html_body: Option<String>,
    ) -> Result<Self, EmailValidationError> {
        if text_body.trim().is_empty() || text_body.len() > 256 * 1024 {
            return Err(EmailValidationError::InvalidBody);
        }
        if let Some(ref html) = html_body {
            if html.len() > 512 * 1024 {
                return Err(EmailValidationError::InvalidBody);
            }
        }
        Ok(Self {
            recipient,
            subject,
            text_body,
            html_body,
        })
    }

    /// Exact SHA-256 fingerprint binding the invariant payload.
    pub fn digest(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.recipient.as_str().as_bytes());
        hasher.update(b"\0");
        hasher.update(self.subject.as_str().as_bytes());
        hasher.update(b"\0");
        hasher.update(self.text_body.as_bytes());
        hasher.update(b"\0");
        if let Some(ref html) = self.html_body {
            hasher.update(html.as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailTransmissionReceipt {
    pub id: String,
    pub request_id: String,
    pub invoice_id: String,
    pub recipient: String,
    pub subject: String,
    pub payload_sha256: String,
    pub state: EmailState,
    pub provider: String,
    pub provider_message_id: Option<String>,
    pub error_reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_recipient_validation() {
        assert!(EmailRecipient::new("customer@example.com").is_ok());
        assert!(EmailRecipient::new("billing+invoicing@corp.co.id").is_ok());

        assert_eq!(
            EmailRecipient::new("").unwrap_err(),
            EmailValidationError::InvalidRecipient
        );
        assert_eq!(
            EmailRecipient::new("no-at-sign.com").unwrap_err(),
            EmailValidationError::InvalidRecipient
        );
        assert_eq!(
            EmailRecipient::new("two@@at.com").unwrap_err(),
            EmailValidationError::InvalidRecipient
        );
        assert_eq!(
            EmailRecipient::new("has space@example.com").unwrap_err(),
            EmailValidationError::InvalidRecipient
        );
        assert_eq!(
            EmailRecipient::new("newline\n@example.com").unwrap_err(),
            EmailValidationError::InvalidRecipient
        );
        assert_eq!(
            EmailRecipient::new("user@nodot").unwrap_err(),
            EmailValidationError::InvalidRecipient
        );
    }

    #[test]
    fn test_email_subject_crlf_injection_rejection() {
        assert!(EmailSubject::new("Invoice PAY-001 from Acme").is_ok());
        assert_eq!(
            EmailSubject::new("Invoice PAY-001\r\nBcc: evil@hacker.com").unwrap_err(),
            EmailValidationError::InvalidSubject
        );
        assert_eq!(
            EmailSubject::new("").unwrap_err(),
            EmailValidationError::InvalidSubject
        );
    }

    #[test]
    fn test_email_payload_digest_reproducible() {
        let rec = EmailRecipient::new("test@example.com").unwrap();
        let sub = EmailSubject::new("Test subject").unwrap();
        let payload1 =
            EmailPayload::new(rec.clone(), sub.clone(), "Hello text".into(), None).unwrap();
        let payload2 = EmailPayload::new(rec, sub, "Hello text".into(), None).unwrap();
        assert_eq!(payload1.digest(), payload2.digest());
        assert_eq!(payload1.digest().len(), 64);
    }
}
