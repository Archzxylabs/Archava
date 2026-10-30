//! HTTP handlers for Ava Orbz (Sovereign Multi-Layer Agentic Memory Engine).
//! Exposes memory crystallization, progressive L0/L1/L2 disclosure queries, and episodic audit streams.

use std::time::{SystemTime, UNIX_EPOCH};

use archava_core::roles::CapabilityTier;
use archava_core::tenant::TenantId;
use archava_orbz::{
    AuthorityTier, ContextTier, Episode, OrbzActor, OrbzScope, OrbzUri, ProgressiveNode,
};
use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::router::{authenticate, AppState};

/// The orbz ledger descriptor — bi-temporal knowledge nodes, episodic streams, and reusable trajectories.
pub const ORBZ_LEDGER: crate::router::Ledger = crate::router::Ledger {
    namespace: "archava_orbz",
    ledger: "orbz",
    tables: &["nodes", "episodes", "trajectories"],
};

#[derive(Debug, Deserialize)]
pub struct CrystallizeMemoryRequest {
    pub tenant_id: String,
    pub scope: OrbzScope,
    pub category: String,
    pub resource: String,
    pub summary_l0: String,
    pub overview_l1: String,
    #[serde(default)]
    pub detail_l2: Option<serde_json::Value>,
    pub min_authority: AuthorityTier,
}

#[derive(Debug, Deserialize)]
pub struct QueryMemoryRequest {
    pub tenant_id: String,
    pub point_in_time_millis: Option<i64>,
    pub tier: ContextTier,
    #[serde(default)]
    pub department: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RenderedNodeResponse {
    pub id: Uuid,
    pub uri: String,
    pub content: String,
    pub estimated_tokens: usize,
    pub min_authority: AuthorityTier,
}

#[derive(Debug, Deserialize)]
pub struct RecordEpisodeRequest {
    pub tenant_id: String,
    pub session_id: String,
    #[serde(default)]
    pub user_id: Option<String>,
    pub user_role: String,
    pub channel: String,
    pub user_utterance: String,
    pub agent_response: String,
    pub tools_called: Vec<String>,
}

fn current_time_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Helper to generate uniform masked error responses.
fn masked(status: StatusCode, message: &'static str) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({ "error": { "message": message } })),
    )
}

async fn persist_node_to_db(
    pool: &sqlx::PgPool,
    node: &ProgressiveNode,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    // R03 FIX: Set transaction-local tenant context for FORCE RLS
    sqlx::query("SELECT set_config('archava.tenant_id', $1, true)")
        .bind(&node.uri.tenant_id)
        .execute(&mut *tx)
        .await?;

    let scope_str = format!("{:?}", node.uri.scope).to_lowercase();
    let auth_str = format!("{:?}", node.min_authority);
    sqlx::query(
        r#"
        INSERT INTO archava_orbz.nodes (
            id, tenant_id, uri, scope, category, min_authority,
            summary_l0, overview_l1, detail_l2,
            tx_timestamp, valid_from, created_at, updated_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        ON CONFLICT (id) DO UPDATE SET
            summary_l0 = EXCLUDED.summary_l0,
            overview_l1 = EXCLUDED.overview_l1,
            detail_l2 = EXCLUDED.detail_l2,
            updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(node.id)
    .bind(&node.uri.tenant_id)
    .bind(node.uri.to_uri_string())
    .bind(scope_str)
    .bind(&node.uri.category)
    .bind(auth_str)
    .bind(&node.summary_l0)
    .bind(&node.overview_l1)
    .bind(&node.detail_l2)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

async fn persist_episode_to_db(pool: &sqlx::PgPool, episode: &Episode) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    // R03 FIX: Set transaction-local tenant context for FORCE RLS
    sqlx::query("SELECT set_config('archava.tenant_id', $1, true)")
        .bind(&episode.tenant_id)
        .execute(&mut *tx)
        .await?;

    let tools_value =
        serde_json::to_value(&episode.tools_called).unwrap_or_else(|_| serde_json::json!([]));
    sqlx::query(
        r#"
        INSERT INTO archava_orbz.episodes (
            id, tenant_id, session_id, user_id, user_role, channel,
            user_utterance, agent_response, tools_called, context_tokens_used, created_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, CURRENT_TIMESTAMP)
        "#,
    )
    .bind(episode.id)
    .bind(&episode.tenant_id)
    .bind(&episode.session_id)
    .bind(&episode.user_id)
    .bind(&episode.user_role)
    .bind(&episode.channel)
    .bind(&episode.user_utterance)
    .bind(&episode.agent_response)
    .bind(tools_value)
    .bind(0i32)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

type OrbzNodeDbRow = (
    Uuid,
    String,
    String,
    String,
    String,
    Option<serde_json::Value>,
);

async fn load_active_nodes_from_db(
    pool: &sqlx::PgPool,
    tenant_id: &str,
) -> Result<Vec<ProgressiveNode>, sqlx::Error> {
    let rows: Vec<OrbzNodeDbRow> = sqlx::query_as(
        r#"
        SELECT id, uri, min_authority, summary_l0, overview_l1, detail_l2
        FROM archava_orbz.nodes
        WHERE tenant_id = $1 AND valid_to IS NULL
        ORDER BY created_at DESC
        "#,
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?;

    let mut nodes = Vec::new();
    for (id, uri_str, min_auth_str, summary_l0, overview_l1, detail_l2) in rows {
        if let Ok(parsed_uri) = OrbzUri::parse(&uri_str) {
            let auth_tier = match min_auth_str.as_str() {
                "L2" => AuthorityTier::L2,
                "L3" => AuthorityTier::L3,
                "L4" => AuthorityTier::L4,
                "L5" => AuthorityTier::L5,
                _ => AuthorityTier::L1,
            };
            nodes.push(ProgressiveNode::new(
                id,
                parsed_uri,
                auth_tier,
                summary_l0,
                overview_l1,
                detail_l2,
            ));
        }
    }
    Ok(nodes)
}

/// POST /api/v1/orbz/crystallize
/// Stores and crystallizes a new multi-tier memory node into Ava Orbz.
pub async fn crystallize_node(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<CrystallizeMemoryRequest>, JsonRejection>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Ok(Json(req)) = body else {
        return masked(StatusCode::BAD_REQUEST, "Invalid request payload.");
    };

    let Ok(tenant) = TenantId::new(&req.tenant_id) else {
        return masked(StatusCode::BAD_REQUEST, "Invalid tenant format.");
    };

    let session = match authenticate(&state, &headers, true).await {
        Ok(s) => s,
        Err((status, outcome)) => {
            let outcome = serde_json::to_value(outcome.0).unwrap_or_else(
                |_| serde_json::json!({ "error": { "message": "Authentication required." } }),
            );
            return (status, Json(outcome));
        }
    };

    // S01 / R01 FIX: Server-side verified membership check inside tenant transaction (Fail-Closed)
    let membership = match crate::authority::resolve_tenant_membership(
        state.pool(),
        *state.ledger(),
        &tenant,
        &session.principal_id,
    )
    .await
    {
        Ok(Some(m)) => m,
        Ok(None) => {
            return masked(StatusCode::FORBIDDEN, "Tenant membership required.");
        }
        Err(e) => {
            tracing::warn!(error = %e, tenant = %tenant, principal = %session.principal_id, "crystallize memory membership check failed");
            return masked(
                StatusCode::FORBIDDEN,
                "Tenant membership verification failed.",
            );
        }
    };

    if membership.capability == CapabilityTier::Assist {
        return masked(
            StatusCode::FORBIDDEN,
            "Read-only actors cannot crystallize memories.",
        );
    }

    let uri = OrbzUri {
        tenant_id: tenant.to_string(),
        scope: req.scope,
        category: req.category,
        resource_name: req.resource,
    };
    let uri_string = uri.to_uri_string();
    let node_id = Uuid::new_v4();

    let node = ProgressiveNode::new(
        node_id,
        uri,
        req.min_authority,
        req.summary_l0,
        req.overview_l1,
        req.detail_l2,
    );

    let now_millis = current_time_millis();
    // R03 FIX: Commit memory node to PostgreSQL BEFORE registering in RAM cache
    if let Err(e) = persist_node_to_db(state.pool(), &node).await {
        tracing::error!(error = %e, uri = %uri_string, "failed to commit memory node to durable store");
        return masked(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to commit memory node to durable store.",
        );
    }
    let orb_id = state.orbz().crystallize(node, now_millis);

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "orb_id": orb_id,
            "uri": uri_string,
            "created_at_millis": now_millis,
        })),
    )
}

/// POST /api/v1/orbz/query
/// Queries active memory nodes with Progressive Disclosure (L0, L1, or L2) and RLS filtering.
pub async fn query_nodes(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<QueryMemoryRequest>, JsonRejection>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Ok(Json(req)) = body else {
        return masked(StatusCode::BAD_REQUEST, "Invalid request payload.");
    };

    let Ok(tenant) = TenantId::new(&req.tenant_id) else {
        return masked(StatusCode::BAD_REQUEST, "Invalid tenant format.");
    };

    let session = match authenticate(&state, &headers, false).await {
        Ok(s) => s,
        Err((status, outcome)) => {
            let outcome = serde_json::to_value(outcome.0).unwrap_or_else(
                |_| serde_json::json!({ "error": { "message": "Authentication required." } }),
            );
            return (status, Json(outcome));
        }
    };

    // S01 / R01 FIX: Derive verified authority and owner status from server-side membership inside tenant transaction (Fail-Closed).
    // NEVER trusting client body parameters or falling back to unverified L1 on DB error!
    let membership = match crate::authority::resolve_tenant_membership(
        state.pool(),
        *state.ledger(),
        &tenant,
        &session.principal_id,
    )
    .await
    {
        Ok(Some(m)) => m,
        Ok(None) => {
            return masked(StatusCode::FORBIDDEN, "Tenant membership required.");
        }
        Err(e) => {
            tracing::warn!(error = %e, tenant = %tenant, principal = %session.principal_id, "query memory membership check failed");
            return masked(
                StatusCode::FORBIDDEN,
                "Tenant membership verification failed.",
            );
        }
    };

    let authority = match membership.capability {
        CapabilityTier::Enterprise => AuthorityTier::L5,
        CapabilityTier::Transact => AuthorityTier::L4,
        CapabilityTier::Act => AuthorityTier::L3,
        CapabilityTier::Assist => AuthorityTier::L1,
    };
    // Strictly verify role is Owner — Enterprise capability tier alone does NOT grant owner authority!
    let is_owner = membership.role == archava_core::roles::RoleName::Owner;

    let now_millis = req.point_in_time_millis.unwrap_or_else(current_time_millis);

    let actor = OrbzActor {
        tenant_id: tenant.to_string(),
        user_id: session.principal_id,
        authority,
        department: req.department,
        is_owner,
    };

    let mut accessible_nodes = state.orbz().query_accessible(&actor, now_millis);
    if accessible_nodes.is_empty() {
        if let Ok(db_nodes) = load_active_nodes_from_db(state.pool(), &tenant.to_string()).await {
            for n in db_nodes {
                state.orbz().crystallize(n, now_millis);
            }
            accessible_nodes = state.orbz().query_accessible(&actor, now_millis);
        }
    }
    let rendered: Vec<RenderedNodeResponse> = accessible_nodes
        .into_iter()
        .map(|node| {
            let content = node.render_tier(req.tier);
            let estimated_tokens = node.estimated_tokens(req.tier);
            let uri_str = node.uri.to_uri_string();
            RenderedNodeResponse {
                id: node.id,
                uri: uri_str,
                content,
                estimated_tokens,
                min_authority: node.min_authority,
            }
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::to_value(rendered).unwrap_or_else(
            |_| serde_json::json!({"error": {"message": "Serialization failure"}}),
        )),
    )
}

/// POST /api/v1/orbz/episodes
/// Records an episodic interaction into the chronological memory stream.
pub async fn record_episode(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<RecordEpisodeRequest>, JsonRejection>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Ok(Json(req)) = body else {
        return masked(StatusCode::BAD_REQUEST, "Invalid request payload.");
    };

    let Ok(tenant) = TenantId::new(&req.tenant_id) else {
        return masked(StatusCode::BAD_REQUEST, "Invalid tenant format.");
    };

    let session = match authenticate(&state, &headers, true).await {
        Ok(s) => s,
        Err((status, outcome)) => {
            let outcome = serde_json::to_value(outcome.0).unwrap_or_else(
                |_| serde_json::json!({ "error": { "message": "Authentication required." } }),
            );
            return (status, Json(outcome));
        }
    };

    // S01 / R01 FIX: Verify membership inside tenant transaction (Fail-Closed)
    match crate::authority::resolve_tenant_membership(
        state.pool(),
        *state.ledger(),
        &tenant,
        &session.principal_id,
    )
    .await
    {
        Ok(Some(_)) => {}
        Ok(None) => {
            return masked(StatusCode::FORBIDDEN, "Tenant membership required.");
        }
        Err(e) => {
            tracing::warn!(error = %e, tenant = %tenant, principal = %session.principal_id, "record episode membership check failed");
            return masked(
                StatusCode::FORBIDDEN,
                "Tenant membership verification failed.",
            );
        }
    }

    let now_millis = current_time_millis();
    let episode = Episode::new(
        tenant.to_string(),
        req.session_id,
        session.principal_id, // Hardened: bound to session identity
        req.user_role,
        req.channel,
        req.user_utterance,
        req.agent_response,
        req.tools_called,
        now_millis,
    );

    // R03 FIX: Commit episode to PostgreSQL BEFORE registering in RAM cache
    if let Err(e) = persist_episode_to_db(state.pool(), &episode).await {
        tracing::error!(error = %e, session_id = %episode.session_id, "failed to commit episode to durable store");
        return masked(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to commit episode to durable store.",
        );
    }
    state.orbz().record_episode(episode);

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "status": "recorded",
            "timestamp_millis": now_millis,
        })),
    )
}
