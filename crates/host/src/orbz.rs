//! Authenticated Orbz HTTP boundary. PostgreSQL is authoritative for reads and
//! writes; every operation locks the current membership in its tenant TX.
use std::time::{SystemTime, UNIX_EPOCH};

use archava_adapters::rls::TenantTransaction;
use archava_core::{
    roles::{CapabilityTier, RoleName},
    tenant::TenantId,
};
use archava_orbz::{
    AuthorityTier, ContextTier, Episode, MemoryOrb, OrbzActor, OrbzScope, OrbzUri, ProgressiveNode,
    TemporalCoordinate,
};
use axum::{
    extract::{rejection::JsonRejection, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::{
    authority::{Membership, MembershipKind, Scope},
    router::{authenticate, AppState},
};

pub const ORBZ_LEDGER: crate::router::Ledger = crate::router::Ledger {
    namespace: "archava_orbz",
    ledger: "orbz",
    tables: &["nodes", "episodes", "trajectories"],
};
type Response = (StatusCode, Json<Value>);
type Result<T> = std::result::Result<T, Response>;

#[derive(Debug, Deserialize)]
pub struct CrystallizeMemoryRequest {
    pub tenant_id: String,
    pub scope: OrbzScope,
    pub category: String,
    pub resource: String,
    pub summary_l0: String,
    pub overview_l1: String,
    #[serde(default)]
    pub detail_l2: Option<Value>,
    pub min_authority: AuthorityTier,
}
#[derive(Debug, Deserialize)]
pub struct QueryMemoryRequest {
    pub tenant_id: String,
    pub point_in_time_millis: Option<i64>,
    pub tier: ContextTier,
    /// A filter only. Until persisted department grants exist, departmental
    /// and entity memories are available exclusively to tenant-wide owners.
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
fn masked(status: StatusCode, message: &'static str) -> Response {
    (status, Json(json!({"error":{"message":message}})))
}
fn unavailable(error: impl std::fmt::Display) -> Response {
    tracing::error!(error=%error, "Orbz transaction refused");
    masked(
        StatusCode::SERVICE_UNAVAILABLE,
        "Memory storage is unavailable.",
    )
}
fn authority(capability: CapabilityTier) -> AuthorityTier {
    match capability {
        CapabilityTier::Enterprise => AuthorityTier::L5,
        CapabilityTier::Transact => AuthorityTier::L4,
        CapabilityTier::Act => AuthorityTier::L3,
        CapabilityTier::Assist => AuthorityTier::L1,
    }
}
async fn principal(state: &AppState, headers: &HeaderMap, write: bool) -> Result<String> {
    authenticate(state, headers, write)
        .await
        .map(|s| s.principal_id)
        .map_err(|(status, body)| (status, Json(json!(body.0))))
}
async fn begin_access<'a>(
    state: &'a AppState,
    tenant: &TenantId,
    actor: &str,
) -> Result<(TenantTransaction<'a>, Membership)> {
    let mut tx = crate::authority::begin(state.pool(), *state.ledger(), tenant.clone())
        .await
        .map_err(unavailable)?;
    let conn = tx.checked_executor().await.map_err(unavailable)?;
    let ledger = state.ledger();
    let lock = format!(
        "SELECT principal_id FROM {}.lock_membership($1,$2)",
        ledger.namespace
    );
    sqlx::query(sqlx::AssertSqlSafe(lock.as_str()))
        .bind(tenant.as_str())
        .bind(actor)
        .fetch_optional(&mut *conn)
        .await
        .map_err(unavailable)?;
    let membership =
        crate::authority::resolve_membership(conn, ledger.namespace, ledger.ledger, tenant, actor)
            .await
            .map_err(unavailable)?
            .filter(|m| m.kind == MembershipKind::Human)
            .ok_or_else(|| {
                masked(
                    StatusCode::FORBIDDEN,
                    "An active human membership is required.",
                )
            })?;
    Ok((tx, membership))
}
fn owner(membership: &Membership) -> bool {
    membership.role == RoleName::Owner && membership.scope == Scope::All
}
fn can_write(membership: &Membership, scope: &OrbzScope, minimum: AuthorityTier) -> bool {
    if membership.scope != Scope::All
        || membership.capability == CapabilityTier::Assist
        || !matches!(
            membership.role,
            RoleName::Owner | RoleName::Admin | RoleName::Editor
        )
        || authority(membership.capability) < minimum
    {
        return false;
    }
    match scope {
        OrbzScope::Public => true,
        OrbzScope::Sovereign => {
            owner(membership) && membership.capability == CapabilityTier::Enterprise
        }
        OrbzScope::Department(_) | OrbzScope::Entity { .. } => owner(membership),
    }
}
fn segment(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && value != "."
        && value != ".."
}
fn valid_node(req: &CrystallizeMemoryRequest) -> bool {
    segment(&req.category, 64)
        && segment(&req.resource, 128)
        && !req.summary_l0.trim().is_empty()
        && req.summary_l0.len() <= 4096
        && req.overview_l1.len() <= 32768
        && req
            .detail_l2
            .as_ref()
            .is_none_or(|v| v.to_string().len() <= 65536)
        && match &req.scope {
            OrbzScope::Department(dept) => segment(dept, 64),
            OrbzScope::Entity {
                entity_type,
                entity_id,
            } => segment(entity_type, 64) && segment(entity_id, 64),
            _ => true,
        }
}
async fn persist_node(
    conn: &mut PgConnection,
    node: &ProgressiveNode,
) -> std::result::Result<i64, sqlx::Error> {
    let entity = match &node.uri.scope {
        OrbzScope::Entity { entity_id, .. } => Some(entity_id.as_str()),
        _ => None,
    };
    sqlx::query_scalar("INSERT INTO archava_orbz.nodes(id,tenant_id,uri,scope,category,entity_id,min_authority,summary_l0,overview_l1,detail_l2) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING floor(extract(epoch FROM created_at)*1000)::bigint")
        .bind(node.id).bind(&node.uri.tenant_id).bind(node.uri.to_uri_string()).bind(node.uri.scope.as_str()).bind(&node.uri.category).bind(entity)
        .bind(node.min_authority.as_str()).bind(&node.summary_l0).bind(&node.overview_l1).bind(&node.detail_l2).fetch_one(conn).await
}
async fn load_nodes(
    conn: &mut PgConnection,
    tenant: &TenantId,
    point: i64,
) -> std::result::Result<Vec<MemoryOrb>, sqlx::Error> {
    let rows=sqlx::query("SELECT id,uri,min_authority,summary_l0,overview_l1,detail_l2,floor(extract(epoch FROM tx_timestamp)*1000)::bigint AS tx_millis,floor(extract(epoch FROM valid_from)*1000)::bigint AS from_millis,floor(extract(epoch FROM valid_to)*1000)::bigint AS to_millis,superseded_by,floor(extract(epoch FROM created_at)*1000)::bigint AS created_millis FROM archava_orbz.nodes WHERE tenant_id=$1 AND floor(extract(epoch FROM valid_from)*1000)::bigint<=$2 AND (valid_to IS NULL OR floor(extract(epoch FROM valid_to)*1000)::bigint>$2) ORDER BY created_at DESC,id DESC")
        .bind(tenant.as_str()).bind(point).fetch_all(conn).await?;
    rows.into_iter()
        .map(|r| {
            let uri = OrbzUri::parse(&r.try_get::<String, _>("uri")?)
                .map_err(|_| sqlx::Error::Protocol("Invalid stored memory URI".into()))?;
            if uri.tenant_id != tenant.as_str() {
                return Err(sqlx::Error::Protocol("Memory URI tenant mismatch".into()));
            }
            let minimum = AuthorityTier::parse(&r.try_get::<String, _>("min_authority")?)
                .ok_or_else(|| sqlx::Error::Protocol("Invalid stored memory authority".into()))?;
            Ok(MemoryOrb {
                node: ProgressiveNode::new(
                    r.try_get("id")?,
                    uri,
                    minimum,
                    r.try_get::<String, _>("summary_l0")?,
                    r.try_get::<String, _>("overview_l1")?,
                    r.try_get("detail_l2")?,
                ),
                temporal: TemporalCoordinate {
                    tx_timestamp: r.try_get("tx_millis")?,
                    valid_from: r.try_get("from_millis")?,
                    valid_to: r.try_get("to_millis")?,
                    superseded_by: r.try_get("superseded_by")?,
                },
                created_at_millis: r.try_get("created_millis")?,
            })
        })
        .collect()
}
async fn create(
    state: &AppState,
    headers: &HeaderMap,
    req: CrystallizeMemoryRequest,
) -> Result<Response> {
    let tenant = TenantId::new(&req.tenant_id)
        .map_err(|_| masked(StatusCode::BAD_REQUEST, "Invalid tenant format."))?;
    if !valid_node(&req) {
        return Err(masked(StatusCode::BAD_REQUEST, "Invalid memory fields."));
    }
    let actor = principal(state, headers, true).await?;
    let (mut tx, membership) = begin_access(state, &tenant, &actor).await?;
    if !can_write(&membership, &req.scope, req.min_authority) {
        return Err(masked(StatusCode::FORBIDDEN, "Memory write scope refused."));
    }
    let node = ProgressiveNode::new(
        Uuid::new_v4(),
        OrbzUri {
            tenant_id: tenant.to_string(),
            scope: req.scope,
            category: req.category,
            resource_name: req.resource,
        },
        req.min_authority,
        req.summary_l0,
        req.overview_l1,
        req.detail_l2,
    );
    let uri = node.uri.to_uri_string();
    if uri.len() > 255 {
        return Err(masked(StatusCode::BAD_REQUEST, "Invalid memory URI."));
    }
    let created = persist_node(tx.checked_executor().await.map_err(unavailable)?, &node)
        .await
        .map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"orb_id":node.id,"uri":uri,"created_at_millis":created})),
    ))
}
pub async fn crystallize_node(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: std::result::Result<Json<CrystallizeMemoryRequest>, JsonRejection>,
) -> Response {
    let req = match body {
        Ok(Json(req)) => req,
        Err(e) => return masked(e.status(), "Invalid request payload."),
    };
    create(&state, &headers, req).await.unwrap_or_else(|e| e)
}
async fn query(state: &AppState, headers: &HeaderMap, req: QueryMemoryRequest) -> Result<Response> {
    let tenant = TenantId::new(&req.tenant_id)
        .map_err(|_| masked(StatusCode::BAD_REQUEST, "Invalid tenant format."))?;
    if req.department.as_ref().is_some_and(|d| !segment(d, 64)) {
        return Err(masked(
            StatusCode::BAD_REQUEST,
            "Invalid department filter.",
        ));
    }
    let actor_id = principal(state, headers, false).await?;
    let (mut tx, membership) = begin_access(state, &tenant, &actor_id).await?;
    let point = match req.point_in_time_millis {
        Some(point) => point,
        None => {
            sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint")
                .fetch_one(tx.checked_executor().await.map_err(unavailable)?)
                .await
                .map_err(unavailable)?
        }
    };
    let actor = OrbzActor {
        tenant_id: tenant.to_string(),
        user_id: actor_id,
        authority: authority(membership.capability),
        department: None,
        is_owner: owner(&membership),
    };
    // Always query durable state, including on warm processes: cached records
    // cannot hide another instance's writes or resurrect invalidated facts.
    let nodes = load_nodes(
        tx.checked_executor().await.map_err(unavailable)?,
        &tenant,
        point,
    )
    .await
    .map_err(unavailable)?;
    let rendered: Vec<RenderedNodeResponse> = nodes
        .into_iter()
        .filter(|orb| {
            let node = &orb.node;
            if matches!(node.uri.scope, OrbzScope::Entity { .. }) && !actor.is_owner {
                return false;
            }
            actor.can_access(&node.uri.scope, node.min_authority)
                && orb.temporal.is_active_at(point)
                && req.department.as_ref().is_none_or(
                    |dept| matches!(&node.uri.scope,OrbzScope::Department(d) if d==dept),
                )
        })
        .map(|orb| RenderedNodeResponse {
            id: orb.node.id,
            uri: orb.node.uri.to_uri_string(),
            content: orb.node.render_tier(req.tier),
            estimated_tokens: orb.node.estimated_tokens(req.tier),
            min_authority: orb.node.min_authority,
        })
        .collect();
    tx.commit().await.map_err(unavailable)?;
    Ok((StatusCode::OK, Json(json!(rendered))))
}
pub async fn query_nodes(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: std::result::Result<Json<QueryMemoryRequest>, JsonRejection>,
) -> Response {
    let req = match body {
        Ok(Json(req)) => req,
        Err(e) => return masked(e.status(), "Invalid request payload."),
    };
    query(&state, &headers, req).await.unwrap_or_else(|e| e)
}
async fn episode(
    state: &AppState,
    headers: &HeaderMap,
    req: RecordEpisodeRequest,
) -> Result<Response> {
    let tenant = TenantId::new(&req.tenant_id)
        .map_err(|_| masked(StatusCode::BAD_REQUEST, "Invalid tenant format."))?;
    if !segment(&req.session_id, 64)
        || !segment(&req.channel, 32)
        || req.user_utterance.len() > 32768
        || req.agent_response.len() > 65536
        || req.tools_called.len() > 64
        || req.tools_called.iter().any(|t| !segment(t, 128))
    {
        return Err(masked(StatusCode::BAD_REQUEST, "Invalid episode fields."));
    }
    let actor = principal(state, headers, true).await?;
    let (mut tx, membership) = begin_access(state, &tenant, &actor).await?;
    let value = Episode::new(
        tenant.to_string(),
        req.session_id,
        actor,
        membership.role.as_str(),
        req.channel,
        req.user_utterance,
        req.agent_response,
        req.tools_called,
        current_time_millis(),
    );
    let timestamp:i64=sqlx::query_scalar("INSERT INTO archava_orbz.episodes(id,tenant_id,session_id,user_id,user_role,channel,user_utterance,agent_response,tools_called,context_tokens_used) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,0) RETURNING floor(extract(epoch FROM created_at)*1000)::bigint")
        .bind(value.id).bind(&value.tenant_id).bind(&value.session_id).bind(&value.user_id).bind(&value.user_role).bind(&value.channel).bind(&value.user_utterance).bind(&value.agent_response).bind(json!(value.tools_called))
        .fetch_one(tx.checked_executor().await.map_err(unavailable)?).await.map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"status":"recorded","timestamp_millis":timestamp})),
    ))
}
pub async fn record_episode(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: std::result::Result<Json<RecordEpisodeRequest>, JsonRejection>,
) -> Response {
    let req = match body {
        Ok(Json(req)) => req,
        Err(e) => return masked(e.status(), "Invalid request payload."),
    };
    episode(&state, &headers, req).await.unwrap_or_else(|e| e)
}
