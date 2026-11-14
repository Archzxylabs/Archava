//! CRM front-office routes, owner configuration lifecycle, and conversation assistant.
//!
//! Note (ADR-018): CRM is a first-class business module of the Archava ERP Monolith.
//! The routes mounted here (`/crm/*`, `/contacts`) serve as canonical front-office
//! client aliases preserving browser contracts, alongside the normalized `/erp/crm/*` routes.

use axum::routing::{get, post};
use axum::Router;

use crate::router::AppState;

/// Mounts the CRM, owner-configuration, and assistant route family.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/agent/turns", post(crate::agent::turn))
        .route("/api/agent/state", get(crate::agent::inspect))
        .route("/api/agent/approve", post(crate::agent::approve))
        .route("/api/agent/cancel", post(crate::agent::control))
        .route("/api/agent/control", post(crate::agent::control))
        // The §1.4B slice: the real, session-scoped contact read (crm.rs)
        // replaces the pre-slice placeholder denial that lived on this path.
        .route("/contacts", get(crate::erp::crm::contacts))
        .route("/crm/records", get(crate::erp::crm::record))
        .route("/crm/commands", post(crate::erp::crm::command))
        .route("/crm/commands/status", get(crate::erp::crm::status))
        // C03: the persisted owner-configuration lifecycle. Session + CSRF
        // first; the archava_owner grant and RLS tenant are checked inside.
        .route("/own/config", get(crate::own::config))
        .route("/own/plan", post(crate::own::plan))
        // The configuration chat's entrypoint. It parses into the SAME typed
        // Request /own/plan takes and resolves through the same plan function,
        // so a chat plan and a dashboard plan cannot drift apart.
        .route("/own/chat", post(crate::own::chat))
        .route("/own/apply", post(crate::own::apply))
        .route("/own/restore/plan", post(crate::own::restore_plan))
        .route("/own/restore", post(crate::own::restore))
        .route("/own/history", get(crate::own::history))
        .route("/own/status", get(crate::own::status))
        // R03's assistant slice: the CRM conversation. Session + CSRF first,
        // tenant from the path, the flow's own gates after.
        .route(
            "/api/crm/{tenant}/conversation",
            post(crate::assistant::conversation),
        )
}
