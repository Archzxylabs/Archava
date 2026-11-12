//! The attendance routes — the Archxy 2 port of the R07 attendance surface
//! against `archava_attendance`, in the shape [`crate::erp::subscriptions`]
//! pinned for this host: extraction → session → ONE tenant transaction whose
//! member ladder, authorization, idempotency claim, effect and audit all run
//! on the adapters' step functions.
//!
//! ## The answers the seven routes give
//!
//! - `POST /erp/attendance/commands` — the TS api answers
//!   `json(await service.mutate(...))`: the BARE receipt document, or one of
//!   the repository's refusal codes folded by HTTP class exactly as the TS
//!   api's own `codeMap` does — `forbidden` 403 (`denied`), `not_found` 404
//!   (`denied`/`unknown_resource`), `conflict`/`capacity_exceeded`/
//!   `already_checked_in` 409 (`refused`), `bad_request` 400 (`rejected`),
//!   and a malformed submission 400 (`rejected`/`invalid_submission`).
//! - `GET /erp/attendance/commands/status?tenant=&requestId=` — the TS
//!   commandStatus document verbatim: `recorded` with its receipt, or the
//!   bare `unknown`. The read requires the current `attendance.read` grant
//!   and answers only for the recorded initiator; another actor's key gets
//!   the same `unknown` document, with no receipt fields.
//! - `GET /erp/attendance/access?tenant=` — the TS `getAccess()` envelope.
//! - `GET /erp/attendance/sessions?tenant=&limit=&offset=&status=` — the TS
//!   `listSessions()`: bounded (1..100, default 50), `offset` ≥ 0, oldest
//!   start first.
//! - `GET /erp/attendance/sessions/detail?tenant=&id=` — the TS
//!   `getSessionDetail()`: session + roster + check-in ledger; `null` for an
//!   unknown or scope-hidden session.
//! - `GET /erp/attendance/roster?tenant=&sessionId=` — the TS `listRoster()`.
//! - `GET /erp/attendance/check-ins?tenant=&sessionId=` — the TS
//!   `listCheckIns()`.
//!
//! ## The labor order (the service layer's)
//!
//! The TS `AttendanceService.execute` runs its parse, the HUMAN-OPERATOR rule
//! (a service principal refuses every arm — this ledger's mutations are
//! operator actions, never automation) and the repository call. This gate
//! keeps that order: parse → human rule → transaction (member ladder →
//! grant → claim → arm).
//!
//! ## The corrected rule (audit A04 — P1)
//!
//! The baseline's check-in loads the roster by id alone; the gate binds the
//! roster to the SUPPLIED session in the SQL row SELECT and in the pure
//! domain policy. A cross-session roster is 404 and its contact never
//! surfaces. The policy itself lives in the domain crate
//! (`check_in_refusal`), so the rule is one pure function and not a branch
//! buried in SQL.
//!
//! ## The fingerprint
//!
//! The TS composition is `HMAC(this.key, JSON.stringify(mutation))` — the
//! MUTATION OBJECT ONLY, no tenant, no actor (this ledger's replay gates on
//! the payload, so a second actor resubmitting an identical `commandId`
//! REPLAYS here exactly as the TS does). The port carries it on the labelled
//! HMAC the other ERP ledgers use (`attendance-command-fingerprint`) over
//! the exact typed arm document the parse produced.
//!
//! Every route is session-bound (`authenticate`), CSRF-bound on the write,
//! and the tenant comes only from the caller's own document — the database,
//! not the session, has the final word on whether the principal belongs to
//! it.

use archava_adapters::erp::attendance::{
    attendance_command_status, list_attendance_sessions, read_attendance_check_ins,
    read_attendance_detail, read_attendance_roster, record_attendance_command,
    resolve_attendance_context, AttendanceError, AttendancePageQuery,
};
use archava_core::tenant::TenantId;
use archava_domain::erp::attendance::{
    parse_mutation, AttendanceAccess, AttendanceCommandStatus, AttendanceMutation, SessionStatus,
};
use archava_domain::erp::purchasing::PurchasePrincipal;
use archava_domain::erp::ErpError;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::erp::{begin_attendance, ErpGateError};
use crate::router::{
    authenticate, authority_refusal, query_rejection, rejection, AppState, CommandOutcome,
};

// --- Response envelope ------------------------------------------------------

/// The union body this module's routes answer with — same shape as the
/// other ERP modules', module-local by the codebase's per-module rule.
pub enum ErpBody {
    Doc(Value),
    Outcome(CommandOutcome),
}

impl IntoResponse for ErpBody {
    fn into_response(self) -> Response {
        match self {
            ErpBody::Doc(value) => Json(value).into_response(),
            ErpBody::Outcome(value) => Json(value).into_response(),
        }
    }
}

/// Levels a host `Json<CommandOutcome>` answer up to this module's type.
fn outcome(answer: (StatusCode, Json<CommandOutcome>)) -> (StatusCode, ErpBody) {
    (answer.0, ErpBody::Outcome(answer.1 .0))
}

/// 400 for a document the boundary parse refuses.
fn invalid_submission() -> (StatusCode, ErpBody) {
    (
        StatusCode::BAD_REQUEST,
        ErpBody::Outcome(CommandOutcome {
            reason: Some("invalid_submission".to_string()),
            message: Some("The submission could not be processed.".to_string()),
            ..CommandOutcome::base("rejected")
        }),
    )
}

/// 400 for the lifecycle the TS refuses with its `bad_request` class.
fn bad_request() -> (StatusCode, ErpBody) {
    (
        StatusCode::BAD_REQUEST,
        ErpBody::Outcome(CommandOutcome {
            reason: Some("bad_request".to_string()),
            message: Some("The attendance lifecycle refuses this submission.".to_string()),
            ..CommandOutcome::base("rejected")
        }),
    )
}

/// 403 for a grant gate the caller can act on.
fn forbidden() -> (StatusCode, ErpBody) {
    (
        StatusCode::FORBIDDEN,
        ErpBody::Outcome(CommandOutcome {
            reason: Some("forbidden".to_string()),
            message: Some("This session cannot perform that operation.".to_string()),
            ..CommandOutcome::base("denied")
        }),
    )
}

/// 404 for a record the caller cannot see or that no longer exists.
fn unknown_resource() -> (StatusCode, ErpBody) {
    (
        StatusCode::NOT_FOUND,
        ErpBody::Outcome(CommandOutcome {
            reason: Some("unknown_resource".to_string()),
            message: Some("No resource matches this request.".to_string()),
            ..CommandOutcome::base("denied")
        }),
    )
}

/// 409 for the conflicts the repository rejects — a stale revision, a
/// fingerprint-mismatched replay, an already-checked-in roster, a spent seat
/// allocation.
fn conflict(reason: &str) -> (StatusCode, ErpBody) {
    (
        StatusCode::CONFLICT,
        ErpBody::Outcome(CommandOutcome {
            reason: Some(reason.to_string()),
            message: Some(
                "A concurrent write or a state boundary refused this submission.".to_string(),
            ),
            ..CommandOutcome::base("refused")
        }),
    )
}

/// The refusal vocabulary mapped by the TS api's own `codeMap`.
fn map_attendance_result(error: AttendanceError) -> Result<(StatusCode, ErpBody), ErpGateError> {
    match error {
        AttendanceError::Domain(ErpError::Forbidden) => Ok(forbidden()),
        AttendanceError::Domain(ErpError::NotFound) => Ok(unknown_resource()),
        AttendanceError::Domain(ErpError::Conflict) => Ok(conflict("conflict")),
        AttendanceError::Domain(ErpError::Unavailable) => Ok(invalid_submission()),
        AttendanceError::CapacityExceeded => Ok(conflict("capacity_exceeded")),
        AttendanceError::AlreadyCheckedIn => Ok(conflict("already_checked_in")),
        AttendanceError::BadRequest => Ok(bad_request()),
        AttendanceError::Database { ledger, source } => Err(ErpGateError::Database(ledger, source)),
    }
}

fn gate_outcome(error: ErpGateError) -> (StatusCode, ErpBody) {
    match error {
        ErpGateError::Configuration(error) => {
            let (status, body) = error.response();
            (status, ErpBody::Doc(body.0))
        }
        ErpGateError::Authority(refusal) => outcome(authority_refusal(&refusal)),
        ErpGateError::Database(ledger, source) => {
            tracing::error!(error = %source, "refusing: {ledger} database failure");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ErpBody::Outcome(CommandOutcome {
                    reason: Some("database_failure".to_string()),
                    ..CommandOutcome::base("unavailable")
                }),
            )
        }
    }
}

// --- Actor model ------------------------------------------------------------

/// Who is acting — module-local, same shape as the other ERP modules'.
#[derive(Debug, Clone)]
pub enum ErpActor {
    Human(String),
}

impl ErpActor {
    fn principal(&self) -> PurchasePrincipal {
        match self {
            Self::Human(id) => PurchasePrincipal::human(id.clone()),
        }
    }
}

// --- Shapes -----------------------------------------------------------------

/// The mutation submission: tenant from the caller's own document and the
/// raw flat mutation document the strict parse judges.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttendanceSubmission {
    pub tenant: TenantId,
    pub mutation: Value,
}

/// The status query.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttendanceStatusQuery {
    pub tenant: TenantId,
    pub request_id: Uuid,
}

/// The access query.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttendanceAccessQuery {
    pub tenant: TenantId,
}

/// The list query — the TS `sessionPageQuerySchema`'s bounds.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttendanceListQuery {
    pub tenant: TenantId,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub status: Option<String>,
}

/// The detail query.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttendanceDetailQuery {
    pub tenant: TenantId,
    pub id: Uuid,
}

/// The roster/check-in query.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttendanceSessionQuery {
    pub tenant: TenantId,
    pub session_id: Uuid,
}

/// The fingerprint a submission records — the TS digest composition
/// (`HMAC(this.key, JSON.stringify(mutation))`) carried on the labelled
/// HMAC: over the typed arm document ALONE, because the TS covers the
/// mutation object and nothing else.
fn fingerprint(state: &AppState, mutation: &AttendanceMutation) -> String {
    let document = serde_json::to_value(&mutation.command)
        .expect("a typed command serializes back to its own JSON");
    state
        .identity()
        .digest("attendance-command-fingerprint", &document.to_string())
}

// --- Routes -----------------------------------------------------------------

/// `POST /erp/attendance/commands` — submit (or replay) an attendance
/// command. CSRF-bound like the other ERP write routes; the tenant comes
/// only from the body. The OK body is the BARE receipt.
pub(crate) async fn attendance_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    submission: Result<Json<AttendanceSubmission>, JsonRejection>,
) -> (StatusCode, ErpBody) {
    let submission = match submission {
        Ok(submission) => submission,
        Err(rejected) => return outcome(rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, true).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());
    match submit_attendance_command(&state, actor, submission.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

/// `GET /erp/attendance/commands/status?tenant=&requestId=` — the durable
/// status of one command id.
pub(crate) async fn attendance_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<AttendanceStatusQuery>, QueryRejection>,
) -> (StatusCode, ErpBody) {
    let query = match query {
        Ok(query) => query,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());
    match attendance_status_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

/// `GET /erp/attendance/access?tenant=` — the caller's grant envelope.
pub(crate) async fn attendance_access(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<AttendanceAccessQuery>, QueryRejection>,
) -> (StatusCode, ErpBody) {
    let query = match query {
        Ok(query) => query,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());
    match attendance_access_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

/// `GET /erp/attendance/sessions?tenant=&limit=&offset=&status=` — the
/// session list.
pub(crate) async fn attendance_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<AttendanceListQuery>, QueryRejection>,
) -> (StatusCode, ErpBody) {
    let query = match query {
        Ok(query) => query,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());
    match attendance_list_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

/// `GET /erp/attendance/sessions/detail?tenant=&id=` — one session with its
/// roster and check-in ledger.
pub(crate) async fn attendance_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<AttendanceDetailQuery>, QueryRejection>,
) -> (StatusCode, ErpBody) {
    let query = match query {
        Ok(query) => query,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());
    match attendance_detail_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

/// `GET /erp/attendance/roster?tenant=&sessionId=` — one session's roster.
pub(crate) async fn attendance_roster(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<AttendanceSessionQuery>, QueryRejection>,
) -> (StatusCode, ErpBody) {
    let query = match query {
        Ok(query) => query,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());
    match attendance_roster_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

/// `GET /erp/attendance/check-ins?tenant=&sessionId=` — one session's
/// check-in ledger.
pub(crate) async fn attendance_check_ins(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<AttendanceSessionQuery>, QueryRejection>,
) -> (StatusCode, ErpBody) {
    let query = match query {
        Ok(query) => query,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());
    match attendance_check_ins_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

// --- One transaction per request: the submit ---------------------------------

/// The submission pipeline — the port of `mutate()` driven by the host's
/// identity key, with the service layer's labor order: parse, the human
/// rule, then ONE transaction for the whole request.
pub async fn submit_attendance_command(
    state: &AppState,
    actor: ErpActor,
    submission: AttendanceSubmission,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    // 0. The boundary parse — the strict verdict the TS service's parse
    //    raises (400 `invalid_request` in TS, the gate's shared 400 carrier).
    let mutation = match parse_mutation(&submission.mutation) {
        Ok(mutation) => mutation,
        Err(_) => return Ok(invalid_submission()),
    };
    // 0b. The human-operator rule — the TS service refuses a service
    //     principal on every arm of this ledger.
    if principal.kind != archava_domain::erp::purchasing::PrincipalKind::Human {
        return Ok(forbidden());
    }
    // 0c. The replay's key — over the mutation document alone.
    let fingerprint = fingerprint(state, &mutation);

    let mut transaction = begin_attendance(state, submission.tenant.clone()).await?;

    // 1. The member ladder — the principal's own membership, active, with
    //    its permissions and scope.
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_attendance_context(connection, submission.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_attendance_result(error);
            }
        }
    };

    // 2. The claim → execute → audit → receipt pipeline.
    let receipt = {
        let connection = transaction.checked_executor().await?;
        match record_attendance_command(
            connection,
            submission.tenant.as_str(),
            &access,
            &fingerprint,
            &mutation,
        )
        .await
        {
            Ok(receipt) => receipt,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_attendance_result(error);
            }
        }
    };

    transaction.commit().await.map_err(ErpGateError::from)?;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            serde_json::to_value(&receipt)
                .expect("a typed receipt serializes back to its own JSON"),
        ),
    ))
}

// --- One transaction per request: the reads ----------------------------------

/// The durable status of a command id — the caller needs `attendance.read`
/// and must be the actor the command recorded.
pub async fn attendance_status_read(
    state: &AppState,
    actor: ErpActor,
    query: AttendanceStatusQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_attendance(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_attendance_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_attendance_result(error);
            }
        }
    };
    let status: AttendanceCommandStatus = {
        let connection = transaction.checked_executor().await?;
        match attendance_command_status(
            connection,
            query.tenant.as_str(),
            &access,
            &query.request_id.to_string(),
        )
        .await
        {
            Ok(status) => status,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_attendance_result(error);
            }
        }
    };
    let _ = transaction.rollback().await;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            serde_json::to_value(&status).expect("a typed status serializes back to its own JSON"),
        ),
    ))
}

/// The caller's access envelope.
pub async fn attendance_access_read(
    state: &AppState,
    actor: ErpActor,
    query: AttendanceAccessQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_attendance(state, query.tenant.clone()).await?;
    let access: AttendanceAccess = {
        let connection = transaction.checked_executor().await?;
        match resolve_attendance_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_attendance_result(error);
            }
        }
    };
    let _ = transaction.rollback().await;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            serde_json::to_value(&access).expect("a typed access serializes back to its own JSON"),
        ),
    ))
}

/// The session list — bounded, ordered, scoped.
pub async fn attendance_list_read(
    state: &AppState,
    actor: ErpActor,
    query: AttendanceListQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    // The TS query's own bounds: `limit` 1..100 (default 50), `offset` ≥ 0
    // (default 0), and the status filter from the status vocabulary.
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);
    if !(1..=100).contains(&limit) || offset < 0 {
        return Ok(invalid_submission());
    }
    if let Some(status) = query.status.as_deref() {
        if SessionStatus::parse(status).is_none() {
            return Ok(invalid_submission());
        }
    }
    let principal = actor.principal();
    let mut transaction = begin_attendance(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_attendance_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_attendance_result(error);
            }
        }
    };
    let page = AttendancePageQuery {
        limit,
        offset,
        status: query.status.clone(),
    };
    let rows = {
        let connection = transaction.checked_executor().await?;
        match list_attendance_sessions(connection, query.tenant.as_str(), &access, &page).await {
            Ok(rows) => rows,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_attendance_result(error);
            }
        }
    };
    let _ = transaction.rollback().await;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            serde_json::to_value(&rows).expect("typed rows serialize back to their own JSON"),
        ),
    ))
}

/// One session with its roster and check-in ledger.
pub async fn attendance_detail_read(
    state: &AppState,
    actor: ErpActor,
    query: AttendanceDetailQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_attendance(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_attendance_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_attendance_result(error);
            }
        }
    };
    let detail = {
        let connection = transaction.checked_executor().await?;
        match read_attendance_detail(
            connection,
            query.tenant.as_str(),
            &access,
            &query.id.to_string(),
        )
        .await
        {
            Ok(detail) => detail,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_attendance_result(error);
            }
        }
    };
    let _ = transaction.rollback().await;
    // The TS answers `null` for an unknown session; the gate answers the
    // same document (a caller may tell "no such session" from a forbidden
    // row by the shape, not by a fabricated empty page).
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            serde_json::to_value(&detail).expect("a typed detail serializes back to its own JSON"),
        ),
    ))
}

/// One session's roster.
pub async fn attendance_roster_read(
    state: &AppState,
    actor: ErpActor,
    query: AttendanceSessionQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_attendance(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_attendance_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_attendance_result(error);
            }
        }
    };
    let rows = {
        let connection = transaction.checked_executor().await?;
        match read_attendance_roster(
            connection,
            query.tenant.as_str(),
            &access,
            &query.session_id.to_string(),
        )
        .await
        {
            Ok(rows) => rows,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_attendance_result(error);
            }
        }
    };
    let _ = transaction.rollback().await;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            serde_json::to_value(&rows).expect("typed rows serialize back to their own JSON"),
        ),
    ))
}

/// One session's check-in ledger.
pub async fn attendance_check_ins_read(
    state: &AppState,
    actor: ErpActor,
    query: AttendanceSessionQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_attendance(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_attendance_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_attendance_result(error);
            }
        }
    };
    let rows = {
        let connection = transaction.checked_executor().await?;
        match read_attendance_check_ins(
            connection,
            query.tenant.as_str(),
            &access,
            &query.session_id.to_string(),
        )
        .await
        {
            Ok(rows) => rows,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_attendance_result(error);
            }
        }
    };
    let _ = transaction.rollback().await;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            serde_json::to_value(&rows).expect("typed rows serialize back to their own JSON"),
        ),
    ))
}
