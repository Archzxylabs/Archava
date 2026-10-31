//! Route subtrees for the Archava host router.
//!
//! Decomposed per ADR-015 into modular families:
//! - [`auth`]: `/login`, `/logout`, session lifecycle.
//! - [`commands`]: `/commands`, `/commands/status`, `/approvals`, `/approvals/approve`.
//! - [`crm`]: `/contacts`, `/crm/*`, `/api/crm/*`, `/own/*`.
//! - [`erp`]: `/erp/*` commands, workspaces, reads across all cohorts.
//! - [`cockpit`]: `/api/v1/cockpit/*`, `/api/v1/orbz/*`, feed, presence, preview, migration.
//! - [`webhooks`]: `/api/webhooks/privy`, `/api/webhooks/payment/*`.
//! - [`system`]: `/healthz`, `/readyz`, `/metrics`, static assets, embed checkout.

pub mod auth;
pub mod cockpit;
pub mod commands;
pub mod crm;
pub mod erp;
pub mod system;
pub mod webhooks;

use axum::Router;

use crate::router::AppState;

/// Assembles the complete set of route families.
pub fn mount_routes() -> Router<AppState> {
    Router::new()
        .merge(system::router())
        .merge(auth::router())
        .merge(commands::router())
        .merge(crm::router())
        .merge(erp::router())
        .merge(cockpit::router())
        .merge(webhooks::router())
}
