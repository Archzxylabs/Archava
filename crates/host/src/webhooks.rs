//! Webhook ingestion for external enterprise providers.
//!
//! `POST /api/webhooks/privy` verifies the shared token before it reads any
//! part of the event. `POST /api/webhooks/payment` is the Midtrans-shaped
//! endpoint, which this host does not implement: the live payment path is
//! `POST /api/webhooks/payment/{integration}` in `payments.rs`, where the
//! signature is checked against a configured server key before any
//! transaction opens. This endpoint answers 503 so a caller cannot mistake it
//! for a settlement path.
//!
//! Two audit fixes live here. The token comparison is now timing-safe
//! (`subtle`, already a workspace dependency) instead of `==` on a bearer
//! secret. And the duplicate `verify_midtrans_signature` wrapper is gone: it
//! was exported and unit-tested but never called by any route, and its test
//! computed a valid signature and asserted a 503, which documented a
//! verification the route never performs. The real verifier stays in
//! `archava_providers::midtrans`.

use axum::extract::rejection::JsonRejection;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use subtle::ConstantTimeEq;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PrivySigner {
    pub email: String,
    pub status: String,
    pub signed_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PrivyWebhookPayload {
    pub event: String,
    pub document_token: String,
    pub signers: Vec<PrivySigner>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PrivyWebhookResponse {
    pub status: &'static str,
    pub action: &'static str,
    pub document_token: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MidtransNotification {
    pub order_id: String,
    pub status_code: String,
    pub gross_amount: String,
    pub signature_key: String,
    pub transaction_status: String,
    pub fraud_status: Option<String>,
    pub payment_type: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PaymentWebhookResponse {
    pub status: &'static str,
    pub order_id: String,
    pub provisioning_status: &'static str,
}

/// Compares two secrets without leaking their common prefix length.
///
/// `ConstantTimeEq` returns 0 for differing lengths, which is the right answer
/// here: this is a bearer token, and the length of the configured secret is not
/// something a probe should be able to recover.
fn secrets_match(expected: &str, presented: &str) -> bool {
    expected.as_bytes().ct_eq(presented.as_bytes()).into()
}

/// POST /api/webhooks/privy
pub async fn privy_webhook_route(
    headers: HeaderMap,
    payload: Result<Json<PrivyWebhookPayload>, JsonRejection>,
) -> (StatusCode, Json<Value>) {
    let expected_token = match std::env::var("PRIVY_WEBHOOK_SECRET") {
        Ok(tok) if !tok.trim().is_empty() => tok,
        _ => {
            tracing::warn!("PRIVY_WEBHOOK_SECRET is unconfigured; rejecting webhook request");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "privy_webhook_unconfigured" })),
            );
        }
    };

    let presented = headers.get("x-privy-token").and_then(|h| h.to_str().ok());
    let token_valid = presented.is_some_and(|token| secrets_match(&expected_token, token));

    if !token_valid {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid_or_missing_privy_token" })),
        );
    }

    let payload = match payload {
        Ok(Json(payload)) => payload,
        Err(rejection) => {
            return (
                rejection.status(),
                Json(json!({ "error": "malformed_webhook" })),
            )
        }
    };

    match payload.event.as_str() {
        "document.signed" => (
            StatusCode::OK,
            Json(json!(PrivyWebhookResponse {
                status: "accepted",
                action: "sow_signed_pending_downpayment",
                document_token: payload.document_token,
            })),
        ),
        "document.rejected" => (
            StatusCode::OK,
            Json(json!(PrivyWebhookResponse {
                status: "declined",
                action: "sow_rejected_by_signer",
                document_token: payload.document_token,
            })),
        ),
        _ => (
            StatusCode::OK,
            Json(json!(PrivyWebhookResponse {
                status: "ignored",
                action: "unhandled_event",
                document_token: payload.document_token,
            })),
        ),
    }
}

/// POST /api/webhooks/payment
///
/// Not a settlement path. The real one is
/// `POST /api/webhooks/payment/{integration}`, which verifies a provider
/// signature before opening a transaction; this route refuses so a caller
/// cannot treat an unverified notification as accepted.
pub async fn payment_webhook_route(
    payload: Result<Json<MidtransNotification>, JsonRejection>,
) -> (StatusCode, Json<Value>) {
    if let Err(rejection) = payload {
        return (
            rejection.status(),
            Json(json!({ "error": "malformed_webhook" })),
        );
    }
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "status": "unavailable",
            "reason": "payment_integration_path_required",
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    fn payload() -> PrivyWebhookPayload {
        PrivyWebhookPayload {
            event: "document.signed".to_string(),
            document_token: "doc_test_123".to_string(),
            signers: vec![PrivySigner {
                email: "owner@glowclinic.id".to_string(),
                status: "completed".to_string(),
                signed_at: Some(1730000000),
            }],
            metadata: None,
        }
    }

    #[test]
    fn the_secret_comparison_is_constant_time_and_length_safe() {
        assert!(secrets_match("shared-secret", "shared-secret"));
        assert!(!secrets_match("shared-secret", "shared-secre"));
        assert!(!secrets_match("shared-secret", "shared-secret-longer"));
        assert!(!secrets_match("shared-secret", ""));
    }

    #[tokio::test]
    async fn an_unconfigured_privy_secret_refuses_with_service_unavailable() {
        std::env::remove_var("PRIVY_WEBHOOK_SECRET");
        let (status, _) = privy_webhook_route(HeaderMap::new(), Ok(Json(payload()))).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn a_missing_or_wrong_privy_token_is_unauthorized() {
        std::env::set_var("PRIVY_WEBHOOK_SECRET", "super-secret-privy-webhook-token");

        let (status_missing, _) = privy_webhook_route(HeaderMap::new(), Ok(Json(payload()))).await;
        assert_eq!(status_missing, StatusCode::UNAUTHORIZED);

        let mut wrong = HeaderMap::new();
        wrong.insert("x-privy-token", "wrong-token".parse().unwrap());
        let (status_wrong, _) = privy_webhook_route(wrong, Ok(Json(payload()))).await;
        assert_eq!(status_wrong, StatusCode::UNAUTHORIZED);

        std::env::remove_var("PRIVY_WEBHOOK_SECRET");
    }

    #[tokio::test]
    async fn a_verified_privy_event_is_answered() {
        std::env::set_var("PRIVY_WEBHOOK_SECRET", "super-secret-privy-webhook-token");
        let mut valid = HeaderMap::new();
        valid.insert(
            "x-privy-token",
            "super-secret-privy-webhook-token".parse().unwrap(),
        );
        let (status, Json(response)) = privy_webhook_route(valid, Ok(Json(payload()))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["status"], "accepted");
        assert_eq!(response["action"], "sow_signed_pending_downpayment");
        std::env::remove_var("PRIVY_WEBHOOK_SECRET");
    }

    /// The Midtrans-shaped endpoint verifies nothing and settles nothing. This
    /// test pins that it says so: a caller that computed a plausible-looking
    /// signature still gets 503, and the body names the real path.
    #[tokio::test]
    async fn the_payment_webhook_does_not_claim_to_settle() {
        let notification = MidtransNotification {
            order_id: "ORD-002".into(),
            status_code: "200".into(),
            gross_amount: "250000.00".into(),
            signature_key: "any-signature-shape".into(),
            transaction_status: "settlement".into(),
            fraud_status: None,
            payment_type: Some("qris".into()),
        };
        let (status, Json(response)) = payment_webhook_route(Ok(Json(notification))).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response["reason"], "payment_integration_path_required");
        assert!(
            response.get("receipt_id").is_none(),
            "a refusal must not hand back anything receipt-shaped"
        );
    }
}
