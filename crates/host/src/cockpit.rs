//! Mission HTTP boundary. PostgreSQL is authoritative; RAM is an advisory projection.
use crate::{
    authority::{resolve_membership, Membership, MembershipKind},
    router::{authenticate, AppState},
};
use archava_cockpit::{CockpitEngine, FlightStatus, FlightStep, Mission, StepStatus, WorkerReport};
use archava_core::{
    roles::{CapabilityTier, RoleName},
    tenant::TenantId,
};
use axum::{
    extract::{rejection::JsonRejection, Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgConnection, Postgres, Row, Transaction};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub const COCKPIT_LEDGER: crate::router::Ledger = crate::router::Ledger {
    namespace: "archava_cockpit",
    ledger: "cockpit",
    tables: &["missions", "flight_steps", "flight_events", "checkpoints"],
};
#[derive(Debug, Deserialize, Serialize)]
pub struct CreateStepRequest {
    pub step_index: usize,
    pub step_name: String,
    pub tool_name: String,
    #[serde(default)]
    pub payload_input: Value,
    #[serde(default)]
    pub depends_on: Vec<usize>,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct CreateMissionRequest {
    pub tenant_id: String,
    pub mission_code: String,
    pub title: String,
    pub description: String,
    pub steps: Vec<CreateStepRequest>,
    #[serde(default)]
    pub auto_launch: bool,
    #[serde(default)]
    pub required_authority: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct CompleteStepRequest {
    pub agent_role: String,
    pub success: bool,
    pub high_signal_summary: String,
    #[serde(default)]
    pub output_payload: Value,
    #[serde(default)]
    pub token_cost: usize,
}
#[derive(Debug, Deserialize)]
pub struct AbortMissionRequest {
    pub reason: String,
}
/// Explicit tenant selection makes mission URLs recoverable after a cold restart.
/// A warm-cache tenant hint remains compatible with earlier callers; it grants no access.
#[derive(Debug, Default, Deserialize)]
pub struct MissionScope {
    pub tenant: Option<String>,
}
type Response = (StatusCode, Json<Value>);
type Result<T> = std::result::Result<T, Response>;
fn masked(status: StatusCode, message: &'static str) -> Response {
    (status, Json(json!({"error":{"message":message}})))
}
fn unavailable(error: impl std::fmt::Display) -> Response {
    tracing::error!(error = %error, "cockpit database operation refused");
    masked(
        StatusCode::SERVICE_UNAVAILABLE,
        "Mission storage is unavailable.",
    )
}
fn conflict(message: &'static str) -> Response {
    masked(StatusCode::CONFLICT, message)
}
fn ok(mission: &Mission) -> Response {
    (StatusCode::OK, Json(json!(mission)))
}
async fn principal(state: &AppState, headers: &HeaderMap, write: bool) -> Result<String> {
    authenticate(state, headers, write)
        .await
        .map(|s| s.principal_id)
        .map_err(|(status, outcome)| (status, Json(json!(outcome.0))))
}
fn tenant_hint(state: &AppState, id: Uuid, scope: MissionScope) -> Result<TenantId> {
    let hint = scope
        .tenant
        .or_else(|| state.cockpit().get_mission(id).map(|m| m.tenant_id))
        .ok_or_else(|| {
            masked(
                StatusCode::BAD_REQUEST,
                "Supply the mission tenant query parameter.",
            )
        })?;
    TenantId::new(hint).map_err(|_| masked(StatusCode::BAD_REQUEST, "Invalid tenant format."))
}
/// Lock the real grant with the existing least-privilege security-definer seam.
/// Revocation cannot race the mutation after this grant has been resolved.
async fn begin(
    state: &AppState,
    tenant: &TenantId,
    actor: &str,
    write: bool,
) -> Result<(Transaction<'static, Postgres>, Membership)> {
    let mut tx = state.pool().begin().await.map_err(unavailable)?;
    sqlx::query("SELECT set_config('archava.tenant_id',$1,true)")
        .bind(tenant.as_str())
        .execute(&mut *tx)
        .await
        .map_err(unavailable)?;
    let ledger = *state.ledger();
    let lock = format!(
        "SELECT principal_id FROM {}.lock_membership($1,$2)",
        ledger.namespace
    );
    sqlx::query(sqlx::AssertSqlSafe(lock.as_str()))
        .bind(tenant.as_str())
        .bind(actor)
        .fetch_optional(&mut *tx)
        .await
        .map_err(unavailable)?;
    let membership = resolve_membership(&mut tx, ledger.namespace, ledger.ledger, tenant, actor)
        .await
        .map_err(unavailable)?
        .ok_or_else(|| masked(StatusCode::FORBIDDEN, "Tenant membership required."))?;
    if membership.kind != MembershipKind::Human {
        return Err(masked(
            StatusCode::FORBIDDEN,
            "A human membership is required.",
        ));
    }
    if write && membership.capability == CapabilityTier::Assist {
        return Err(masked(
            StatusCode::FORBIDDEN,
            "Read-only actors cannot change missions.",
        ));
    }
    Ok((tx, membership))
}
fn authorize_record(actor: &str, initiator: &str) -> Result<()> {
    if actor == initiator {
        return Ok(());
    }
    Err(masked(StatusCode::FORBIDDEN, "Mission scope refused."))
}
fn decode<T: DeserializeOwned>(text: String) -> std::result::Result<T, sqlx::Error> {
    serde_json::from_value(Value::String(text)).map_err(|e| sqlx::Error::Decode(Box::new(e)))
}
fn index(value: i32) -> std::result::Result<usize, sqlx::Error> {
    usize::try_from(value).map_err(|e| sqlx::Error::Decode(Box::new(e)))
}
/// Shared reads and exclusive mutations lock the mission before reading its steps.
/// All handlers use the database even when another process has populated a RAM cache.
async fn load(
    connection: &mut PgConnection,
    id: Uuid,
    write: bool,
) -> std::result::Result<Option<(Mission, String, String)>, sqlx::Error> {
    let suffix = if write { "FOR UPDATE" } else { "FOR SHARE" };
    let query = format!("SELECT id,tenant_id,mission_code,title,objective,status,holding_reason,holding_signal_key,initiator_user_id,required_authority FROM archava_cockpit.missions WHERE id=$1 {suffix}");
    let Some(row) = sqlx::query(sqlx::AssertSqlSafe(query.as_str()))
        .bind(id)
        .fetch_optional(&mut *connection)
        .await?
    else {
        return Ok(None);
    };
    let step_rows = sqlx::query("SELECT step_index,step_name,tool_target,payload_input,payload_output,status,depends_on_steps,subagent_role,isolated_context_summary,error_message FROM archava_cockpit.flight_steps WHERE mission_id=$1 ORDER BY step_index").bind(id).fetch_all(connection).await?;
    let mut steps = Vec::with_capacity(step_rows.len());
    for step in step_rows {
        let deps: Option<Vec<i32>> = step.try_get("depends_on_steps")?;
        steps.push(FlightStep {
            step_index: index(step.try_get("step_index")?)?,
            step_name: step.try_get("step_name")?,
            tool_target: step.try_get("tool_target")?,
            payload_input: step.try_get("payload_input")?,
            payload_output: step.try_get("payload_output")?,
            status: decode(step.try_get("status")?)?,
            depends_on_steps: deps
                .unwrap_or_default()
                .into_iter()
                .map(index)
                .collect::<std::result::Result<_, _>>()?,
            subagent_role: step.try_get("subagent_role")?,
            isolated_context_summary: step.try_get("isolated_context_summary")?,
            error_message: step.try_get("error_message")?,
        });
    }
    Ok(Some((
        Mission {
            id: row.try_get("id")?,
            tenant_id: row.try_get("tenant_id")?,
            mission_code: row.try_get("mission_code")?,
            title: row.try_get("title")?,
            objective: row.try_get("objective")?,
            status: decode(row.try_get("status")?)?,
            steps,
            holding_reason: row.try_get("holding_reason")?,
            holding_signal_key: row.try_get("holding_signal_key")?,
        },
        row.try_get("initiator_user_id")?,
        row.try_get("required_authority")?,
    )))
}
async fn event(
    connection: &mut PgConnection,
    mission: &Mission,
    kind: &str,
    payload: Value,
) -> std::result::Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO archava_cockpit.flight_events(mission_id,tenant_id,event_type,event_payload) VALUES($1,$2,$3,$4)").bind(mission.id).bind(&mission.tenant_id).bind(kind).bind(payload).execute(connection).await?;
    Ok(())
}
async fn store_status(
    connection: &mut PgConnection,
    mission: &Mission,
) -> std::result::Result<(), sqlx::Error> {
    let result = sqlx::query("UPDATE archava_cockpit.missions SET status=$1,holding_reason=$2,completed_at=CASE WHEN $1 IN ('touchdown','aborted') THEN CURRENT_TIMESTAMP ELSE completed_at END,updated_at=CURRENT_TIMESTAMP WHERE id=$3 AND tenant_id=$4")
        .bind(mission.status.as_str()).bind(&mission.holding_reason).bind(mission.id).bind(&mission.tenant_id).execute(connection).await?;
    if result.rows_affected() != 1 {
        return Err(sqlx::Error::RowNotFound);
    }
    Ok(())
}
fn bounded(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= max
}
async fn create(
    state: &AppState,
    headers: &HeaderMap,
    req: CreateMissionRequest,
) -> Result<Response> {
    let actor = principal(state, headers, true).await?;
    let tenant = TenantId::new(&req.tenant_id)
        .map_err(|_| masked(StatusCode::BAD_REQUEST, "Invalid tenant format."))?;
    if !bounded(&req.mission_code, 64)
        || !bounded(&req.title, 255)
        || req.steps.len() > 1024
        || req.steps.iter().any(|s| {
            i32::try_from(s.step_index).is_err()
                || s.depends_on.iter().any(|&i| i32::try_from(i).is_err())
                || !bounded(&s.step_name, 128)
                || !bounded(&s.tool_name, 128)
        })
    {
        return Err(masked(
            StatusCode::BAD_REQUEST,
            "Invalid mission fields or step indices.",
        ));
    }
    let request_document = json!(&req);
    let steps = req
        .steps
        .into_iter()
        .map(|s| {
            FlightStep::new(
                s.step_index,
                s.step_name,
                s.tool_name,
                s.payload_input,
                s.depends_on,
            )
        })
        .collect();
    let mut mission = Mission::new(
        tenant.as_str(),
        req.mission_code,
        req.title,
        req.description,
        steps,
    );
    mission
        .validate_dag()
        .map_err(|e| masked(StatusCode::BAD_REQUEST, e))?;
    if req.auto_launch {
        mission.status = FlightStatus::InFlight;
    }
    let (mut tx, _membership) = begin(state, &tenant, &actor, true).await?;
    // Serialize this actor's creation key. A retry observes the committed original.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!(
            "mission:{}:{}:{}",
            tenant.as_str(),
            actor,
            mission.mission_code
        ))
        .execute(&mut *tx)
        .await
        .map_err(unavailable)?;
    let prior:Option<Uuid>=sqlx::query_scalar("SELECT id FROM archava_cockpit.missions WHERE tenant_id=$1 AND initiator_user_id=$2 AND mission_code=$3 ORDER BY created_at LIMIT 1").bind(tenant.as_str()).bind(&actor).bind(&mission.mission_code).fetch_optional(&mut *tx).await.map_err(unavailable)?;
    if let Some(id) = prior {
        let original:Option<Value>=sqlx::query_scalar("SELECT event_payload->'request' FROM archava_cockpit.flight_events WHERE mission_id=$1 AND event_type='mission_created' ORDER BY id LIMIT 1").bind(id).fetch_optional(&mut *tx).await.map_err(unavailable)?;
        if original != Some(request_document) {
            return Err(conflict(
                "This mission code already names a different creation request.",
            ));
        }
        let (prior, _, _) = load(&mut tx, id, false)
            .await
            .map_err(unavailable)?
            .ok_or_else(|| conflict("Mission receipt is unavailable."))?;
        tx.commit().await.map_err(unavailable)?;
        return Ok(ok(&prior));
    }

    let authority = match req.required_authority.as_deref() {
        Some(a) if matches!(a, "L1" | "L2" | "L3" | "L4" | "L5") => a,
        Some(_) => {
            return Err(masked(
                StatusCode::BAD_REQUEST,
                "Invalid required_authority level.",
            ))
        }
        None => "L1",
    };

    sqlx::query("INSERT INTO archava_cockpit.missions(id,tenant_id,mission_code,title,objective,status,initiator_user_id,initiator_role,required_authority,started_at) VALUES($1,$2,$3,$4,$5,$6,$7,'operator',$8,CASE WHEN $6='in_flight' THEN CURRENT_TIMESTAMP ELSE NULL END)")
        .bind(mission.id).bind(&mission.tenant_id).bind(&mission.mission_code).bind(&mission.title).bind(&mission.objective).bind(mission.status.as_str()).bind(&actor).bind(authority).execute(&mut *tx).await.map_err(unavailable)?;
    for step in &mission.steps {
        let deps: Vec<i32> = step.depends_on_steps.iter().map(|&i| i as i32).collect();
        sqlx::query("INSERT INTO archava_cockpit.flight_steps(id,mission_id,tenant_id,step_index,step_name,tool_target,payload_input,status,depends_on_steps,subagent_role) VALUES($1,$2,$3,$4,$5,$6,$7,'pending',$8,$9)")
            .bind(Uuid::new_v4()).bind(mission.id).bind(&mission.tenant_id).bind(step.step_index as i32).bind(&step.step_name).bind(&step.tool_target).bind(&step.payload_input).bind(deps).bind(&step.subagent_role).execute(&mut *tx).await.map_err(unavailable)?;
    }
    event(
        &mut tx,
        &mission,
        "mission_created",
        json!({"status":mission.status,"request":request_document}),
    )
    .await
    .map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    state.cockpit().register_mission(mission.clone());
    Ok((StatusCode::CREATED, Json(json!(mission))))
}
async fn read(
    state: &AppState,
    id: Uuid,
    scope: MissionScope,
    headers: &HeaderMap,
) -> Result<Response> {
    let actor = principal(state, headers, false).await?;
    let tenant = tenant_hint(state, id, scope)?;
    let (mut tx, _membership) = begin(state, &tenant, &actor, false).await?;
    let (mission, initiator, _) = load(&mut tx, id, false)
        .await
        .map_err(unavailable)?
        .ok_or_else(|| masked(StatusCode::NOT_FOUND, "Mission not found."))?;
    authorize_record(&actor, &initiator)?;
    tx.commit().await.map_err(unavailable)?;
    state.cockpit().register_mission(mission.clone());
    Ok(ok(&mission))
}
async fn complete(
    state: &AppState,
    id: Uuid,
    step_index: usize,
    scope: MissionScope,
    headers: &HeaderMap,
    req: CompleteStepRequest,
) -> Result<Response> {
    let actor = principal(state, headers, true).await?;
    if i32::try_from(step_index).is_err() || !bounded(&req.agent_role, 64) {
        return Err(masked(
            StatusCode::BAD_REQUEST,
            "Invalid worker or step index.",
        ));
    }
    let tenant = tenant_hint(state, id, scope)?;
    let (mut tx, _membership) = begin(state, &tenant, &actor, true).await?;
    let (mission, initiator, _) = load(&mut tx, id, true)
        .await
        .map_err(unavailable)?
        .ok_or_else(|| masked(StatusCode::NOT_FOUND, "Mission not found."))?;
    authorize_record(&actor, &initiator)?;
    let report = WorkerReport {
        worker_role: req.agent_role,
        success: req.success,
        high_signal_summary: req.high_signal_summary,
        output_payload: req.output_payload,
        tokens_consumed: req.token_cost,
    };
    let step = mission
        .steps
        .iter()
        .find(|s| s.step_index == step_index)
        .ok_or_else(|| masked(StatusCode::NOT_FOUND, "Step not found."))?;
    if matches!(step.status, StepStatus::Passed | StepStatus::Failed) {
        let prior:Option<Value>=sqlx::query_scalar("SELECT COALESCE(event_payload->'report','null'::jsonb) FROM archava_cockpit.flight_events WHERE mission_id=$1 AND event_type='step_completed' AND event_payload->>'step_index'=$2 ORDER BY id DESC LIMIT 1")
            .bind(id).bind(step_index.to_string()).fetch_optional(&mut *tx).await.map_err(unavailable)?;
        if prior != Some(json!(&report)) {
            return Err(conflict(
                "Completed step report differs from its durable result.",
            ));
        }
        tx.commit().await.map_err(unavailable)?;
        state.cockpit().register_mission(mission.clone());
        return Ok(ok(&mission));
    }
    // Validate on an isolated projection; no shared RAM changes precede SQL commit.
    let engine = CockpitEngine::new();
    engine.register_mission(mission);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    engine
        .record_step_result(id, step_index, report.clone(), now)
        .map_err(conflict)?;
    let mut updated = engine
        .get_mission(id)
        .expect("validated mission is registered");
    let step = updated
        .steps
        .iter_mut()
        .find(|s| s.step_index == step_index)
        .expect("validated step exists");
    step.subagent_role = report.worker_role.clone();
    step.payload_output = Some(report.output_payload.clone());
    step.isolated_context_summary = Some(report.high_signal_summary.clone());
    let result=sqlx::query("UPDATE archava_cockpit.flight_steps SET status=$1,payload_output=$2,isolated_context_summary=$3,error_message=$4,subagent_role=$5,completed_at=CURRENT_TIMESTAMP WHERE mission_id=$6 AND step_index=$7 AND status IN ('pending','running')")
        .bind(if report.success {"passed"} else {"failed"}).bind(&step.payload_output).bind(&step.isolated_context_summary).bind(&step.error_message).bind(&step.subagent_role).bind(id).bind(step_index as i32).execute(&mut *tx).await.map_err(unavailable)?;
    if result.rows_affected() != 1 {
        return Err(conflict("Step state changed."));
    }
    if updated.is_all_steps_passed() {
        updated.status = FlightStatus::Touchdown;
    }
    store_status(&mut tx, &updated).await.map_err(unavailable)?;
    event(
        &mut tx,
        &updated,
        "step_completed",
        json!({"step_index":step_index,"report":&report}),
    )
    .await
    .map_err(unavailable)?;
    sqlx::query("INSERT INTO archava_cockpit.checkpoints(mission_id,tenant_id,current_step_index,checkpoint_state) VALUES($1,$2,$3,$4) ON CONFLICT(mission_id) DO UPDATE SET current_step_index=EXCLUDED.current_step_index,checkpoint_state=EXCLUDED.checkpoint_state,updated_at=CURRENT_TIMESTAMP")
        .bind(id).bind(&updated.tenant_id).bind(step_index as i32).bind(json!({"last_completed_step":step_index,"mission":&updated})).execute(&mut *tx).await.map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    state.cockpit().register_mission(updated.clone());
    Ok(ok(&updated))
}
async fn abort(
    state: &AppState,
    id: Uuid,
    scope: MissionScope,
    headers: &HeaderMap,
    req: AbortMissionRequest,
) -> Result<Response> {
    let actor = principal(state, headers, true).await?;
    if req.reason.trim().is_empty() {
        return Err(masked(
            StatusCode::BAD_REQUEST,
            "An abort reason is required.",
        ));
    }
    let tenant = tenant_hint(state, id, scope)?;
    let (mut tx, _membership) = begin(state, &tenant, &actor, true).await?;
    let (mut mission, initiator, _) = load(&mut tx, id, true)
        .await
        .map_err(unavailable)?
        .ok_or_else(|| masked(StatusCode::NOT_FOUND, "Mission not found."))?;
    authorize_record(&actor, &initiator)?;
    if mission.status == FlightStatus::Aborted
        && mission.holding_reason.as_ref() == Some(&req.reason)
    {
        tx.commit().await.map_err(unavailable)?;
        state.cockpit().register_mission(mission.clone());
        return Ok(ok(&mission));
    }
    if !mission.status.can_transition_to(FlightStatus::Aborted) {
        return Err(conflict(
            "Mission cannot be aborted from its current state.",
        ));
    }
    mission.status = FlightStatus::Aborted;
    mission.holding_reason = Some(req.reason);
    store_status(&mut tx, &mission).await.map_err(unavailable)?;
    event(
        &mut tx,
        &mission,
        "mission_aborted",
        json!({"reason":mission.holding_reason}),
    )
    .await
    .map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    state.cockpit().register_mission(mission.clone());
    Ok(ok(&mission))
}

async fn hold(
    state: &AppState,
    id: Uuid,
    scope: MissionScope,
    headers: &HeaderMap,
    req: HoldMissionRequest,
) -> Result<Response> {
    let actor = principal(state, headers, true).await?;
    if req.reason.trim().is_empty() || req.signal_key.trim().is_empty() {
        return Err(masked(
            StatusCode::BAD_REQUEST,
            "A holding reason and signal key are required.",
        ));
    }
    let tenant = tenant_hint(state, id, scope)?;
    let (mut tx, _membership) = begin(state, &tenant, &actor, true).await?;
    let (mut mission, initiator, _) = load(&mut tx, id, true)
        .await
        .map_err(unavailable)?
        .ok_or_else(|| masked(StatusCode::NOT_FOUND, "Mission not found."))?;
    authorize_record(&actor, &initiator)?;

    if mission.status == FlightStatus::HoldingPattern
        && mission.holding_reason.as_ref() == Some(&req.reason)
        && mission.holding_signal_key.as_ref() == Some(&req.signal_key)
    {
        tx.commit().await.map_err(unavailable)?;
        state.cockpit().register_mission(mission.clone());
        return Ok(ok(&mission));
    }

    if !mission
        .status
        .can_transition_to(FlightStatus::HoldingPattern)
    {
        return Err(conflict(
            "Mission cannot enter holding pattern from current state.",
        ));
    }

    mission.status = FlightStatus::HoldingPattern;
    mission.holding_reason = Some(req.reason.clone());
    mission.holding_signal_key = Some(req.signal_key.clone());

    sqlx::query("UPDATE archava_cockpit.missions SET status=$1,holding_reason=$2,holding_signal_key=$3,updated_at=CURRENT_TIMESTAMP WHERE id=$4 AND tenant_id=$5")
        .bind(mission.status.as_str())
        .bind(&mission.holding_reason)
        .bind(&mission.holding_signal_key)
        .bind(mission.id)
        .bind(&mission.tenant_id)
        .execute(&mut *tx)
        .await
        .map_err(unavailable)?;

    event(
        &mut tx,
        &mission,
        "mission_holding_pattern",
        json!({"reason": req.reason, "signal_key": req.signal_key}),
    )
    .await
    .map_err(unavailable)?;

    tx.commit().await.map_err(unavailable)?;
    state.cockpit().register_mission(mission.clone());
    Ok(ok(&mission))
}

async fn resume(
    state: &AppState,
    id: Uuid,
    scope: MissionScope,
    headers: &HeaderMap,
    req: ResumeMissionRequest,
) -> Result<Response> {
    let actor = principal(state, headers, true).await?;
    if req.signal_key.trim().is_empty() {
        return Err(masked(
            StatusCode::BAD_REQUEST,
            "A signal key is required to resume.",
        ));
    }
    let tenant = tenant_hint(state, id, scope)?;
    let (mut tx, membership) = begin(state, &tenant, &actor, true).await?;
    let (mut mission, initiator, required_authority) = load(&mut tx, id, true)
        .await
        .map_err(unavailable)?
        .ok_or_else(|| masked(StatusCode::NOT_FOUND, "Mission not found."))?;
    if mission.status != FlightStatus::HoldingPattern {
        return Err(conflict("Mission is not in a holding pattern."));
    }

    if mission.holding_signal_key.as_deref() != Some(&req.signal_key) {
        return Err(conflict("Invalid holding signal key."));
    }

    if matches!(required_authority.as_str(), "L4" | "L5") {
        if actor == initiator {
            return Err(conflict(
                "Missions requiring L4 or L5 authority cannot be resumed by the initiator; independent supervisor approval is required.",
            ));
        }
        if !matches!(membership.role, RoleName::Owner | RoleName::Admin) {
            return Err(masked(
                StatusCode::FORBIDDEN,
                "Supervisor authority required.",
            ));
        }
    } else if actor != initiator && !matches!(membership.role, RoleName::Owner | RoleName::Admin) {
        return Err(masked(StatusCode::FORBIDDEN, "Mission scope refused."));
    }

    mission.status = FlightStatus::InFlight;
    mission.holding_reason = None;
    mission.holding_signal_key = None;

    sqlx::query("UPDATE archava_cockpit.missions SET status=$1,holding_reason=NULL,holding_signal_key=NULL,updated_at=CURRENT_TIMESTAMP WHERE id=$2 AND tenant_id=$3")
        .bind(mission.status.as_str())
        .bind(mission.id)
        .bind(&mission.tenant_id)
        .execute(&mut *tx)
        .await
        .map_err(unavailable)?;

    event(
        &mut tx,
        &mission,
        "mission_resumed",
        json!({"signal_key": req.signal_key}),
    )
    .await
    .map_err(unavailable)?;

    tx.commit().await.map_err(unavailable)?;
    state.cockpit().register_mission(mission.clone());
    Ok(ok(&mission))
}

pub async fn create_mission(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: std::result::Result<Json<CreateMissionRequest>, JsonRejection>,
) -> Response {
    match body {
        Ok(Json(req)) => create(&state, &headers, req).await.unwrap_or_else(|e| e),
        Err(_) => masked(StatusCode::BAD_REQUEST, "Invalid request payload."),
    }
}
pub async fn get_mission(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(scope): Query<MissionScope>,
    headers: HeaderMap,
) -> Response {
    read(&state, id, scope, &headers)
        .await
        .unwrap_or_else(|e| e)
}
pub async fn complete_step(
    State(state): State<AppState>,
    Path((id, index)): Path<(Uuid, usize)>,
    Query(scope): Query<MissionScope>,
    headers: HeaderMap,
    body: std::result::Result<Json<CompleteStepRequest>, JsonRejection>,
) -> Response {
    match body {
        Ok(Json(req)) => complete(&state, id, index, scope, &headers, req)
            .await
            .unwrap_or_else(|e| e),
        Err(_) => masked(StatusCode::BAD_REQUEST, "Invalid request payload."),
    }
}
pub async fn abort_mission(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(scope): Query<MissionScope>,
    headers: HeaderMap,
    body: std::result::Result<Json<AbortMissionRequest>, JsonRejection>,
) -> Response {
    match body {
        Ok(Json(req)) => abort(&state, id, scope, &headers, req)
            .await
            .unwrap_or_else(|e| e),
        Err(_) => masked(StatusCode::BAD_REQUEST, "Invalid request payload."),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldMissionRequest {
    pub reason: String,
    pub signal_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResumeMissionRequest {
    pub signal_key: String,
}

pub async fn hold_mission(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(scope): Query<MissionScope>,
    headers: HeaderMap,
    body: std::result::Result<Json<HoldMissionRequest>, JsonRejection>,
) -> Response {
    match body {
        Ok(Json(req)) => hold(&state, id, scope, &headers, req)
            .await
            .unwrap_or_else(|e| e),
        Err(_) => masked(StatusCode::BAD_REQUEST, "Invalid request payload."),
    }
}

pub async fn resume_mission(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(scope): Query<MissionScope>,
    headers: HeaderMap,
    body: std::result::Result<Json<ResumeMissionRequest>, JsonRejection>,
) -> Response {
    match body {
        Ok(Json(req)) => resume(&state, id, scope, &headers, req)
            .await
            .unwrap_or_else(|e| e),
        Err(_) => masked(StatusCode::BAD_REQUEST, "Invalid request payload."),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MissionListQuery {
    pub tenant: TenantId,
}
/// Bounded, private worklist; SQL remains authoritative after a cold restart.
pub async fn list_missions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MissionListQuery>,
) -> Response {
    async fn execute(state: &AppState, headers: &HeaderMap, tenant: TenantId) -> Result<Response> {
        let actor = principal(state, headers, false).await?;
        let (mut tx, membership) = begin(state, &tenant, &actor, false).await?;
        let items: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'mission_code',mission_code,'title',title,'status',status,'created_at',created_at,'initiator_user_id',initiator_user_id,'required_authority',required_authority) FROM archava_cockpit.missions WHERE tenant_id=$1 AND initiator_user_id=$2 ORDER BY created_at DESC,id DESC LIMIT 100")
            .bind(tenant.as_str())
            .bind(&actor)
            .fetch_all(&mut *tx)
            .await
            .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
        Ok((
            StatusCode::OK,
            Json(
                json!({"items":items,"canWrite":membership.capability!=CapabilityTier::Assist,"limit":100}),
            ),
        ))
    }
    execute(&state, &headers, query.tenant)
        .await
        .unwrap_or_else(|e| e)
}
/// Execute one ready step through a finite allowlist of read-only application tools.
/// No browser-supplied result, network target, code or write command is executed.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunStepRequest {
    pub step_index: usize,
}
pub async fn run_next(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(scope): Query<MissionScope>,
    headers: HeaderMap,
    Json(request): Json<RunStepRequest>,
) -> Response {
    async fn execute(
        state: &AppState,
        id: Uuid,
        scope: MissionScope,
        headers: &HeaderMap,
        step_index: usize,
    ) -> Result<Response> {
        let actor = principal(state, headers, true).await?;
        let tenant = tenant_hint(state, id, scope)?;
        let (mut tx, _membership) = begin(state, &tenant, &actor, true).await?;
        let (mission, initiator, _) = load(&mut tx, id, false)
            .await
            .map_err(unavailable)?
            .ok_or_else(|| masked(StatusCode::NOT_FOUND, "Mission not found."))?;
        authorize_record(&actor, &initiator)?;
        let step = mission
            .steps
            .iter()
            .find(|s| s.step_index == step_index)
            .cloned()
            .ok_or_else(|| conflict("The requested step does not exist."))?;
        if step.status == StepStatus::Passed {
            tx.commit().await.map_err(unavailable)?;
            return Ok(ok(&mission));
        }
        if mission.status != FlightStatus::InFlight
            || step.status != StepStatus::Pending
            || !step.depends_on_steps.iter().all(|index| {
                mission
                    .steps
                    .iter()
                    .any(|p| p.step_index == *index && p.status == StepStatus::Passed)
            })
        {
            return Err(conflict("This step is not ready to run."));
        }
        if !step.payload_input.is_object()
            || step
                .payload_input
                .as_object()
                .is_some_and(|o| !o.is_empty())
        {
            return Err(masked(
                StatusCode::BAD_REQUEST,
                "These read tools require an empty input object.",
            ));
        }
        tx.commit().await.map_err(unavailable)?;
        let response = match step.tool_target.as_str() {
            "commerce.workspace.read" => {
                crate::erp::commerce::workspace(
                    State(state.clone()),
                    headers.clone(),
                    Query(crate::erp::commerce::WorkspaceQuery {
                        tenant: tenant.clone(),
                        search: String::new(),
                    }),
                )
                .await
            }
            "purchasing.workspace.read" => {
                crate::erp::workspace::purchasing(
                    State(state.clone()),
                    headers.clone(),
                    Query(crate::erp::workspace::WorkspaceQuery {
                        tenant: tenant.clone(),
                        search: String::new(),
                    }),
                )
                .await
            }
            "billing.workspace.read" => {
                crate::erp::workspace::billing(
                    State(state.clone()),
                    headers.clone(),
                    Query(crate::erp::workspace::WorkspaceQuery {
                        tenant: tenant.clone(),
                        search: String::new(),
                    }),
                )
                .await
            }
            _ => {
                return Err(masked(
                    StatusCode::BAD_REQUEST,
                    "This mission tool is not registered for operator execution.",
                ))
            }
        };
        if response.0 != StatusCode::OK {
            return Err(response);
        }
        let counts: serde_json::Map<String, Value> = response
            .1
             .0
            .as_object()
            .into_iter()
            .flat_map(|o| o.iter())
            .filter_map(|(key, value)| {
                value
                    .as_array()
                    .map(|rows| (key.clone(), json!(rows.len())))
            })
            .filter(|(key, _)| key != "permissions")
            .collect();
        complete(state,id,step.step_index,MissionScope{tenant:Some(tenant.to_string())},headers,CompleteStepRequest{agent_role:"workspace_reader".into(),success:true,high_signal_summary:format!("{} read completed under current operator grants.",step.tool_target),output_payload:json!({"tool":step.tool_target,"recordCounts":counts,"bounded":true}),token_cost:0}).await
    }
    execute(&state, id, scope, &headers, request.step_index)
        .await
        .unwrap_or_else(|e| e)
}
