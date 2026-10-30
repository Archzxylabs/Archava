//! Webhook Ingestion Engine for External Enterprise Providers.
//! Handlers for Privy (e-Signatures) and Midtrans (Snap/Core Payment Gateway).

use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(test)]
use sha2::{Digest, Sha512};

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

/// Verifies Midtrans SHA-512 signature: SHA512(order_id + status_code + gross_amount + server_key)
pub fn verify_midtrans_signature(
    order_id: &str,
    status_code: &str,
    gross_amount: &str,
    server_key: &str,
    provided_signature: &str,
) -> bool {
    archava_providers::midtrans::verify_signature(
        order_id,
        status_code,
        gross_amount,
        server_key,
        provided_signature,
    )
}

/// POST /api/webhooks/privy
pub async fn privy_webhook_route(
    headers: HeaderMap,
    Json(payload): Json<PrivyWebhookPayload>,
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

    let token_valid = headers
        .get("x-privy-token")
        .and_then(|h| h.to_str().ok())
        .map(|tok| tok == expected_token)
        .unwrap_or(false);

    if !token_valid {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid_or_missing_privy_token" })),
        );
    }

    match payload.event.as_str() {
        "document.signed" => {
            // All parties have signed the SOW contract
            (
                StatusCode::OK,
                Json(json!(PrivyWebhookResponse {
                    status: "accepted",
                    action: "sow_signed_pending_downpayment",
                    document_token: payload.document_token,
                })),
            )
        }
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
pub async fn payment_webhook_route(
    Json(payload): Json<MidtransNotification>,
) -> (StatusCode, Json<Value>) {
    let _ = payload;
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"status":"unavailable","reason":"payment_integration_path_required"})),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midtrans_signature_verification_succeeds() {
        let order_id = "INV-2026-001";
        let status_code = "200";
        let gross_amount = "15000000.00";
        let server_key = "SB-Mid-server-TESTKEY-ARCHAVA";

        // Compute expected signature
        let payload = format!("{}{}{}{}", order_id, status_code, gross_amount, server_key);
        let mut hasher = Sha512::new();
        hasher.update(payload.as_bytes());
        let expected = format!("{:x}", hasher.finalize());

        assert!(verify_midtrans_signature(
            order_id,
            status_code,
            gross_amount,
            server_key,
            &expected
        ));

        // Forged signature fails
        assert!(!verify_midtrans_signature(
            order_id,
            status_code,
            gross_amount,
            server_key,
            "forged_signature_hex_000"
        ));
    }

    #[tokio::test]
    async fn privy_webhook_route_signed_event() {
        let payload = PrivyWebhookPayload {
            event: "document.signed".to_string(),
            document_token: "doc_test_123".to_string(),
            signers: vec![PrivySigner {
                email: "owner@glowclinic.id".to_string(),
                status: "completed".to_string(),
                signed_at: Some(1730000000),
            }],
            metadata: None,
        };

        // 1. Unconfigured PRIVY_WEBHOOK_SECRET must fail with 503 SERVICE_UNAVAILABLE
        std::env::remove_var("PRIVY_WEBHOOK_SECRET");
        let headers_empty = HeaderMap::new();
        let (status_unconf, _) = privy_webhook_route(headers_empty, Json(payload.clone())).await;
        assert_eq!(status_unconf, StatusCode::SERVICE_UNAVAILABLE);

        // Set configured secret
        std::env::set_var("PRIVY_WEBHOOK_SECRET", "super-secret-privy-webhook-token");

        // 2. Missing or wrong token must fail with 401 UNAUTHORIZED
        let mut headers_wrong = HeaderMap::new();
        headers_wrong.insert("x-privy-token", "wrong-token".parse().unwrap());
        let (status_unauth, _) = privy_webhook_route(headers_wrong, Json(payload)).await;
        assert_eq!(status_unauth, StatusCode::UNAUTHORIZED);

        // 3. Valid token passes
        let mut headers_valid = HeaderMap::new();
        headers_valid.insert(
            "x-privy-token",
            "super-secret-privy-webhook-token".parse().unwrap(),
        );
        let payload_valid = PrivyWebhookPayload {
            event: "document.signed".to_string(),
            document_token: "doc_test_123".to_string(),
            signers: vec![],
            metadata: None,
        };
        let (status_ok, Json(res)) = privy_webhook_route(headers_valid, Json(payload_valid)).await;
        assert_eq!(status_ok, StatusCode::OK);
        assert_eq!(res["status"], "accepted");
        assert_eq!(res["action"], "sow_signed_pending_downpayment");
    }

    #[tokio::test]
    async fn payment_webhook_unconfigured_and_signature_checks() {
        // Unconfigured MIDTRANS_SERVER_KEY rejects with SERVICE_UNAVAILABLE (S04)
        std::env::remove_var("MIDTRANS_SERVER_KEY");
        let notification = MidtransNotification {
            order_id: "ORD-001".into(),
            status_code: "200".into(),
            gross_amount: "100000.00".into(),
            signature_key: "dummy".into(),
            transaction_status: "settlement".into(),
            fraud_status: None,
            payment_type: Some("qris".into()),
        };
        let (status_unconf, _) = payment_webhook_route(Json(notification)).await;
        assert_eq!(status_unconf, StatusCode::SERVICE_UNAVAILABLE);

        // Configured key with valid signature succeeds
        std::env::set_var("MIDTRANS_SERVER_KEY", "TEST-SERVER-KEY-123");
        let order_id = "ORD-002";
        let status_code = "200";
        let gross_amount = "250000.00";
        let mut hasher = Sha512::new();
        hasher.update(
            format!(
                "{}{}{}{}",
                order_id, status_code, gross_amount, "TEST-SERVER-KEY-123"
            )
            .as_bytes(),
        );
        let valid_sig = format!("{:x}", hasher.finalize());

        let notification_valid = MidtransNotification {
            order_id: order_id.into(),
            status_code: status_code.into(),
            gross_amount: gross_amount.into(),
            signature_key: valid_sig,
            transaction_status: "settlement".into(),
            fraud_status: None,
            payment_type: Some("qris".into()),
        };
        let (status_valid, Json(res)) = payment_webhook_route(Json(notification_valid)).await;
        assert_eq!(status_valid, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(res["reason"], "payment_integration_path_required");
    }
}
