//! The formal education routes — the Archxy 2 port of the R08 formal education surface
//! against `archava_edu`:
//! extraction → session → ONE tenant transaction whose member ladder, grant,
//! idempotency claim, student admission, attendance, results, review and retention
//! run on the adapters' step functions.
//!
//! ## Routes
//!
//! - `POST /erp/education/commands` — parses submission, validates session, runs the command ladder.
//! - `GET /erp/education/commands/status?tenant=&commandId=` — returns `unknown` or `accepted`.
//! - `GET /erp/education/access?tenant=` — effective permissions, owner id, scope.
//! - `GET /erp/education/enrollments?tenant=&limit=&status=&cursor=` — list enrollments query.
//! - `GET /erp/education/enrollments/detail?tenant=&id=` — enrollment detail with sessions, results, review.
//! - `GET /erp/education/enrollments/audit?tenant=&id=` — enrollment audit trail query.

use archava_adapters::erp::formal_education::{
    execute_education_command, get_education_command_status, get_education_enrollment_detail,
    list_education_audit, list_education_enrollments, resolve_education_context,
    FormalEducationError,
};
use archava_core::tenant::TenantId;
use archava_domain::erp::canonical_json;
use archava_domain::erp::formal_education::{parse_command, parse_page_query};
use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::erp::{begin_education, ErpGateError};
use crate::router::{
    authenticate, authority_refusal, query_rejection, rejection, AppState, CommandOutcome,
};

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

fn outcome(answer: (StatusCode, Json<CommandOutcome>)) -> (StatusCode, ErpBody) {
    (answer.0, ErpBody::Outcome(answer.1 .0))
}

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

fn conflict(msg: &str) -> (StatusCode, ErpBody) {
    (
        StatusCode::CONFLICT,
        ErpBody::Outcome(CommandOutcome {
            reason: Some("conflict".to_string()),
            message: Some(msg.to_string()),
            ..CommandOutcome::base("denied")
        }),
    )
}

fn map_education_result(
    error: FormalEducationError,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    match error {
        FormalEducationError::Forbidden => Ok(forbidden()),
        FormalEducationError::NotFound => Ok(unknown_resource()),
        FormalEducationError::Conflict(msg) => Ok(conflict(&msg)),
        FormalEducationError::IllegalTransition(msg) => Ok(conflict(&msg)),
        FormalEducationError::ReviewRequired(msg) => Ok(conflict(&msg)),
        FormalEducationError::UnknownEffect => Ok(conflict("Command effect is not yet known")),
        FormalEducationError::ValidationFailed(_) => Ok(invalid_submission()),
        FormalEducationError::Database { ledger, source } => {
            Err(ErpGateError::Database(ledger, source))
        }
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

#[derive(Debug, Clone)]
pub enum ErpActor {
    Human(String),
    Service { id: String, on_behalf_of: String },
}

impl ErpActor {
    pub fn principal(&self) -> PurchasePrincipal {
        match self {
            Self::Human(id) => PurchasePrincipal {
                id: id.clone(),
                kind: PrincipalKind::Human,
                on_behalf_of: None,
            },
            Self::Service { id, on_behalf_of } => PurchasePrincipal {
                id: id.clone(),
                kind: PrincipalKind::Service,
                on_behalf_of: Some(on_behalf_of.clone()),
            },
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EducationSubmission {
    pub tenant: TenantId,
    pub command: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EducationStatusQuery {
    pub tenant: TenantId,
    pub command_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EducationAccessQuery {
    pub tenant: TenantId,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EducationEnrollmentsQuery {
    pub tenant: TenantId,
    pub limit: Option<i64>,
    pub status: Option<String>,
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EducationDetailQuery {
    pub tenant: TenantId,
    pub id: Option<String>,
    pub enrollment_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EducationAuditQuery {
    pub tenant: TenantId,
    pub id: Option<String>,
    pub enrollment_id: Option<String>,
}

fn compute_fingerprint(command: &Value) -> String {
    let canonical = canonical_json(command);
    format!("{:x}", Sha256::digest(canonical.as_bytes()))
}

// --- Route handlers ---

pub(crate) async fn education_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    submission: Result<Json<EducationSubmission>, JsonRejection>,
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

    match education_submit(&state, actor, submission.0).await {
        Ok(result) => result,
        Err(gate) => gate_outcome(gate),
    }
}

pub(crate) async fn education_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<EducationStatusQuery>, QueryRejection>,
) -> (StatusCode, ErpBody) {
    let query = match query {
        Ok(query) => query.0,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());

    let command_id = match query.command_id {
        Some(ref id) if !id.trim().is_empty() => id.trim().to_string(),
        _ => return invalid_submission(),
    };

    match education_status_read(&state, actor, query.tenant, &command_id).await {
        Ok(result) => result,
        Err(gate) => gate_outcome(gate),
    }
}

pub(crate) async fn education_access_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<EducationAccessQuery>, QueryRejection>,
) -> (StatusCode, ErpBody) {
    let query = match query {
        Ok(query) => query.0,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());

    match education_access_read(&state, actor, query.tenant).await {
        Ok(result) => result,
        Err(gate) => gate_outcome(gate),
    }
}

pub(crate) async fn education_enrollments(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<EducationEnrollmentsQuery>, QueryRejection>,
) -> (StatusCode, ErpBody) {
    let query = match query {
        Ok(query) => query.0,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());

    match education_enrollments_read(&state, actor, query).await {
        Ok(result) => result,
        Err(gate) => gate_outcome(gate),
    }
}

pub(crate) async fn education_enrollment_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<EducationDetailQuery>, QueryRejection>,
) -> (StatusCode, ErpBody) {
    let query = match query {
        Ok(query) => query.0,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());

    let target_id = query
        .id
        .or(query.enrollment_id)
        .unwrap_or_default()
        .trim()
        .to_string();

    match education_detail_read(&state, actor, query.tenant, &target_id).await {
        Ok(result) => result,
        Err(gate) => gate_outcome(gate),
    }
}

pub(crate) async fn education_audit(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<EducationAuditQuery>, QueryRejection>,
) -> (StatusCode, ErpBody) {
    let query = match query {
        Ok(query) => query.0,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());

    let target_id = query
        .id
        .or(query.enrollment_id)
        .unwrap_or_default()
        .trim()
        .to_string();

    match education_audit_read(&state, actor, query.tenant, &target_id).await {
        Ok(result) => result,
        Err(gate) => gate_outcome(gate),
    }
}

// --- Execution functions ---

async fn education_submit(
    state: &AppState,
    actor: ErpActor,
    submission: EducationSubmission,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let command = match parse_command(&submission.command) {
        Ok(c) => c,
        Err(_) => return Ok(invalid_submission()),
    };

    let fp = compute_fingerprint(&submission.command);

    let mut transaction = begin_education(state, submission.tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result = execute_education_command(
        connection,
        submission.tenant.as_str(),
        &principal,
        command,
        &fp,
    )
    .await;

    match result {
        Ok(receipt) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            let doc = serde_json::to_value(&receipt).unwrap_or_default();
            Ok((StatusCode::OK, ErpBody::Doc(doc)))
        }
        Err(err) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            map_education_result(err)
        }
    }
}

async fn education_status_read(
    state: &AppState,
    actor: ErpActor,
    tenant: TenantId,
    command_id: &str,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let mut transaction = begin_education(state, tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result =
        get_education_command_status(connection, tenant.as_str(), &principal, command_id).await;

    match result {
        Ok(status) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            let doc = serde_json::to_value(&status).unwrap_or_default();
            Ok((StatusCode::OK, ErpBody::Doc(doc)))
        }
        Err(err) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            map_education_result(err)
        }
    }
}

async fn education_access_read(
    state: &AppState,
    actor: ErpActor,
    tenant: TenantId,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let mut transaction = begin_education(state, tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result = resolve_education_context(connection, tenant.as_str(), &principal).await;

    match result {
        Ok(access) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            let doc = serde_json::to_value(&access).unwrap_or_default();
            Ok((StatusCode::OK, ErpBody::Doc(doc)))
        }
        Err(err) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            map_education_result(err)
        }
    }
}

async fn education_enrollments_read(
    state: &AppState,
    actor: ErpActor,
    query: EducationEnrollmentsQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let page_query = match parse_page_query(
        query.limit,
        query.cursor.as_deref(),
        query.status.as_deref(),
    ) {
        Ok(pq) => pq,
        Err(_) => return Ok(invalid_submission()),
    };

    let mut transaction = begin_education(state, query.tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result =
        list_education_enrollments(connection, query.tenant.as_str(), &principal, page_query).await;

    match result {
        Ok(enrollments) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            let doc = serde_json::to_value(&enrollments).unwrap_or_default();
            Ok((StatusCode::OK, ErpBody::Doc(doc)))
        }
        Err(err) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            map_education_result(err)
        }
    }
}

async fn education_detail_read(
    state: &AppState,
    actor: ErpActor,
    tenant: TenantId,
    enrollment_id: &str,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    if enrollment_id.is_empty() {
        return Ok(invalid_submission());
    }

    let mut transaction = begin_education(state, tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result =
        get_education_enrollment_detail(connection, tenant.as_str(), &principal, enrollment_id)
            .await;

    match result {
        Ok(Some(detail)) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            let doc = serde_json::to_value(&detail).unwrap_or_default();
            Ok((StatusCode::OK, ErpBody::Doc(doc)))
        }
        Ok(None) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            Ok(unknown_resource())
        }
        Err(err) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            map_education_result(err)
        }
    }
}

async fn education_audit_read(
    state: &AppState,
    actor: ErpActor,
    tenant: TenantId,
    enrollment_id: &str,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    if enrollment_id.is_empty() {
        return Ok(invalid_submission());
    }

    let mut transaction = begin_education(state, tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result = list_education_audit(connection, tenant.as_str(), &principal, enrollment_id).await;

    match result {
        Ok(audit) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            let doc = serde_json::to_value(&audit).unwrap_or_default();
            Ok((StatusCode::OK, ErpBody::Doc(doc)))
        }
        Err(err) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            map_education_result(err)
        }
    }
}
