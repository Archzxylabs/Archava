//! Midtrans Snap creation and authenticated Get Status. A notification signature
//! does not cover transaction_status, so it is never the settlement authority.
use crate::http::{self, Credential, TransportError};
use archava_domain::erp::{
    billing::SAFE_INTEGER_MAX,
    payments::{whole_idr, PaymentObservation, PaymentState},
};
use reqwest::{Client, Url};
use serde_json::{json, Value};
use sha2::{Digest, Sha512};
use subtle::ConstantTimeEq;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidtransMode {
    Sandbox,
    Production,
}
#[derive(Clone)]
pub struct MidtransClient {
    client: Client,
    key: Credential,
    api: Url,
    snap: Url,
    local: bool,
}
impl std::fmt::Debug for MidtransClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MidtransClient")
            .field("api", &self.api.origin().ascii_serialization())
            .finish_non_exhaustive()
    }
}
/// Only constructed after the authenticated status response matches the bound
/// order, amount and (when supplied) transaction reference.
#[derive(Clone, Debug)]
pub struct VerifiedPayment(PaymentObservation);
impl VerifiedPayment {
    pub fn observation(&self) -> &PaymentObservation {
        &self.0
    }
}

#[derive(Clone)]
pub struct Checkout {
    pub redirect_url: String,
}
impl std::fmt::Debug for Checkout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Checkout([private redirect])")
    }
}
impl MidtransClient {
    pub fn new(mode: MidtransMode, key: String) -> Result<Self, TransportError> {
        let (api, snap) = match mode {
            MidtransMode::Sandbox => (
                "https://api.sandbox.midtrans.com/",
                "https://app.sandbox.midtrans.com/",
            ),
            MidtransMode::Production => ("https://api.midtrans.com/", "https://app.midtrans.com/"),
        };
        Ok(Self {
            client: http::client(false)?,
            key: Credential::new(key)?,
            api: Url::parse(api).map_err(|_| TransportError::Configuration)?,
            snap: Url::parse(snap).map_err(|_| TransportError::Configuration)?,
            local: false,
        })
    }
    /// Finite local transport tests. The application configuration only selects
    /// Sandbox/Production; it cannot redirect provider credentials to this path.
    pub fn for_loopback(origin: &str, key: String) -> Result<Self, TransportError> {
        let base = http::loopback_origin(origin)?;
        Ok(Self {
            client: http::client(true)?,
            key: Credential::new(key)?,
            api: base.clone(),
            snap: base,
            local: true,
        })
    }
    pub fn verifies(&self, order: &str, status: &str, gross: &str, signature: &str) -> bool {
        verify_signature(order, status, gross, self.key.expose(), signature)
    }
    pub async fn create_checkout(
        &self,
        order: &str,
        amount: i64,
    ) -> Result<Checkout, TransportError> {
        if !identifier(order) || !(1..=SAFE_INTEGER_MAX).contains(&amount) {
            return Err(TransportError::Protocol);
        }
        let url = path(&self.snap, &["snap", "v1", "transactions"])?;
        let response = http::json(
            self.client
                .post(url)
                .basic_auth(self.key.expose(), Some(""))
                .header("accept", "application/json")
                .json(&json!({"transaction_details":{"order_id":order,"gross_amount":amount}})),
        )
        .await?;
        if response["token"]
            .as_str()
            .is_none_or(|v| v.is_empty() || v.len() > 4096)
        {
            return Err(TransportError::Protocol);
        }
        let redirect = response["redirect_url"]
            .as_str()
            .ok_or(TransportError::Protocol)?;
        if redirect.len() > 4096 || redirect.chars().any(char::is_control) {
            return Err(TransportError::Protocol);
        }
        let parsed = Url::parse(redirect).map_err(|_| TransportError::Protocol)?;
        if parsed.origin() != self.snap.origin()
            || parsed.username() != ""
            || parsed.password().is_some()
            || parsed.fragment().is_some()
            || (!self.local && parsed.scheme() != "https")
            || !parsed.path().starts_with("/snap/")
        {
            return Err(TransportError::Protocol);
        }
        Ok(Checkout {
            redirect_url: parsed.to_string(),
        })
    }
    pub async fn status(
        &self,
        order: &str,
        transaction: Option<&str>,
        amount: i64,
    ) -> Result<VerifiedPayment, TransportError> {
        if !identifier(order)
            || transaction.is_some_and(|t| !identifier(t))
            || !(1..=SAFE_INTEGER_MAX).contains(&amount)
        {
            return Err(TransportError::Protocol);
        }
        let url = path(&self.api, &["v2", transaction.unwrap_or(order), "status"])?;
        let value = http::json(
            self.client
                .get(url)
                .basic_auth(self.key.expose(), Some(""))
                .header("accept", "application/json"),
        )
        .await?;
        self.parse_status(&value, order, transaction, amount)
    }
    fn parse_status(
        &self,
        value: &Value,
        order: &str,
        transaction: Option<&str>,
        amount: i64,
    ) -> Result<VerifiedPayment, TransportError> {
        let text = |key: &str| value[key].as_str().ok_or(TransportError::Protocol);
        let observed_order = text("order_id")?;
        let observed_transaction = text("transaction_id")?;
        let gross = text("gross_amount")?;
        let status_code = text("status_code")?;
        if observed_order != order
            || !identifier(observed_transaction)
            || transaction.is_some_and(|t| t != observed_transaction)
            || whole_idr(gross).map_err(|_| TransportError::Protocol)? != amount
            || value.get("currency").is_some_and(|v| v != "IDR")
            || !self.verifies(observed_order, status_code, gross, text("signature_key")?)
        {
            return Err(TransportError::Protocol);
        }
        let status = text("transaction_status")?.to_ascii_lowercase();
        if status.is_empty()
            || status.len() > 32
            || !status.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
        {
            return Err(TransportError::Protocol);
        }
        let accepted = value["fraud_status"]
            .as_str()
            .is_some_and(|s| s.eq_ignore_ascii_case("accept"));
        let state = match status.as_str() {
            "settlement" | "capture" if status_code == "200" && accepted => PaymentState::Settled,
            "refund" | "partial_refund" if status_code == "200" && accepted => {
                PaymentState::Refunded
            }
            "pending" | "authorize" => PaymentState::Pending,
            "deny" | "cancel" | "expire" | "failure" => PaymentState::Failed,
            _ => PaymentState::Review,
        };
        let refunded = if state == PaymentState::Refunded {
            let total = match value.get("refund_amount") {
                Some(Value::String(v)) => whole_idr(v).map_err(|_| TransportError::Protocol)?,
                None if status == "refund" => amount,
                _ => return Err(TransportError::Protocol),
            };
            if total <= 0
                || total > amount
                || status == "partial_refund" && total == amount
                || status == "refund" && total != amount
            {
                return Err(TransportError::Protocol);
            }
            if let Some(refunds) = value.get("refunds") {
                let refunds = refunds.as_array().ok_or(TransportError::Protocol)?;
                if refunds.is_empty() || refunds.len() > 1000 {
                    return Err(TransportError::Protocol);
                }
                let mut ids = std::collections::HashSet::new();
                let mut sum = 0_i64;
                for refund in refunds {
                    let id = refund
                        .get("refund_chargeback_id")
                        .and_then(Value::as_u64)
                        .filter(|id| *id > 0)
                        .ok_or(TransportError::Protocol)?;
                    if !ids.insert(id) {
                        return Err(TransportError::Protocol);
                    }
                    let n = whole_idr(
                        refund["refund_amount"]
                            .as_str()
                            .ok_or(TransportError::Protocol)?,
                    )
                    .map_err(|_| TransportError::Protocol)?;
                    if n <= 0 {
                        return Err(TransportError::Protocol);
                    }
                    sum = sum.checked_add(n).ok_or(TransportError::Protocol)?;
                }
                if sum != total {
                    return Err(TransportError::Protocol);
                }
            }
            total
        } else {
            0
        };
        Ok(VerifiedPayment(PaymentObservation {
            order_id: order.into(),
            transaction_id: observed_transaction.into(),
            amount_minor: amount,
            refunded_minor: refunded,
            state,
            provider_status: status,
        }))
    }
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 120
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_:.".contains(&b))
}
fn path(base: &Url, segments: &[&str]) -> Result<Url, TransportError> {
    let mut url = base.clone();
    url.path_segments_mut()
        .map_err(|_| TransportError::Configuration)?
        .clear()
        .extend(segments);
    Ok(url)
}
/// Parse the complete fixed-length tag before a constant-time byte comparison.
pub fn verify_signature(order: &str, status: &str, gross: &str, key: &str, provided: &str) -> bool {
    if provided.len() != 128 {
        return false;
    }
    let mut decoded = [0_u8; 64];
    let nibble = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    };
    for (byte, pair) in decoded.iter_mut().zip(provided.as_bytes().chunks_exact(2)) {
        let (Some(hi), Some(lo)) = (nibble(pair[0]), nibble(pair[1])) else {
            return false;
        };
        *byte = hi * 16 + lo;
    }
    let mut digest = Sha512::new();
    for part in [order, status, gross, key] {
        digest.update(part.as_bytes())
    }
    let expected: [u8; 64] = digest.finalize().into();
    bool::from(expected[..].ct_eq(&decoded[..]))
}
