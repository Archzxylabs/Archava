//! Ava Cockpit autonomous missions, Orbz memory, proactive feeds, and presence routes.

use axum::routing::{get, post};
use axum::Router;

use crate::router::AppState;

/// Mounts the Cockpit, Orbz, Feed, Presence, Preview, and Ingestion routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/preview/generate",
            post(crate::preview::generate_preview_route),
        )
        // Realtime Voice & Presence Lifecycle
        .route(
            "/presence/sessions",
            post(crate::presence::start_presence_session),
        )
        .route(
            "/presence/sessions/{id}",
            get(crate::presence::get_presence_session),
        )
        .route(
            "/presence/sessions/{id}/turn",
            post(crate::presence::process_presence_turn),
        )
        .route(
            "/presence/sessions/{id}/degrade",
            post(crate::presence::degrade_presence_session),
        )
        // AI Legacy Data Migration & Schema Ingestion
        .route(
            "/api/migration/reconcile",
            post(crate::ingestion::migration_reconcile_route),
        )
        .route(
            "/api/migration/commit",
            post(crate::ingestion::migration_commit_route),
        )
        // Ava Cockpit: Autonomous Durable Flight Operations
        .route(
            "/api/v1/cockpit/missions",
            post(crate::cockpit::create_mission).get(crate::cockpit::list_missions),
        )
        .route(
            "/api/v1/cockpit/missions/{id}",
            get(crate::cockpit::get_mission),
        )
        .route(
            "/api/v1/cockpit/missions/{id}/steps/{step_index}/complete",
            post(crate::cockpit::complete_step),
        )
        .route(
            "/api/v1/cockpit/missions/{id}/abort",
            post(crate::cockpit::abort_mission),
        )
        .route(
            "/api/v1/cockpit/missions/{id}/hold",
            post(crate::cockpit::hold_mission),
        )
        .route(
            "/api/v1/cockpit/missions/{id}/resume",
            post(crate::cockpit::resume_mission),
        )
        .route(
            "/api/v1/cockpit/missions/{id}/run-next",
            post(crate::cockpit::run_next),
        )
        // Ava Orbz: Sovereign Multi-Layer Agentic Memory Engine
        .route(
            "/api/v1/orbz/crystallize",
            post(crate::orbz::crystallize_node),
        )
        .route("/api/v1/orbz/query", post(crate::orbz::query_nodes))
        .route("/api/v1/orbz/episodes", post(crate::orbz::record_episode))
        // Industry Presets & Proactive Action Feed (Frictionless Operations)
        .route("/api/v1/presets/profiles", get(crate::feed::list_profiles))
        .route(
            "/api/v1/presets/profiles/{slug}",
            get(crate::feed::get_profile),
        )
        .route(
            "/api/v1/feed/action-cards",
            get(crate::feed::get_action_cards),
        )
        .route(
            "/api/v1/feed/action-cards/{id}/execute",
            post(crate::feed::execute_action),
        )
}
