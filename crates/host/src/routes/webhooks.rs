//! Webhook routes for external signatures and payment provider notifications.

use axum::routing::post;
use axum::Router;

use crate::router::AppState;

/// Mounts the Privy and payment provider webhook routes.
pub fn router() -> Router<AppState> {
    Router::new()
        // Autonomous Closing Webhooks: Privy Digital Signatures & Payment Gateways
        .route(
            "/api/webhooks/privy",
            post(crate::webhooks::privy_webhook_route),
        )
        .route(
            "/api/webhooks/payment",
            post(crate::webhooks::payment_webhook_route),
        )
        .route(
            "/api/webhooks/payment/{integration}",
            post(crate::payments::webhook),
        )
}
