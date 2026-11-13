//! The field services routes — the Archxy 2 port of the R08 field services surface
//! against `archava_field`:
//! extraction → session → ONE tenant transaction whose member ladder, grant,
//! idempotency claim, job dispatch, visits, work evidence and signoff
//! run on the adapters' step functions.
//!
//! ## Routes
//!
//! - `POST /erp/field/commands` — parses submission, validates session, runs the command ladder.
//! - `GET /erp/field/commands/status?tenant=&commandId=` — returns `unknown` or `accepted`.
//! - `GET /erp/field/access?tenant=` — effective permissions, owner id, scope.
//! - `GET /erp/field/jobs?tenant=&limit=&status=&cursor=` — list jobs query.
//! - `GET /erp/field/jobs/detail?tenant=&id=` — job detail with visits, work lines, signoff.
//! - `GET /erp/field/audit?tenant=&id=` — job audit trail query.

use archava_adapters::erp::field_services::{
    field_command_status, get_field_job_detail, list_field_audit, list_field_jobs,
    record_field_command, resolve_field_context, FieldError,
};
use archava_core::tenant::TenantId;
use archava_domain::erp::field_services::{parse_command, parse_page_query};
use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
use archava_domain::erp::{canonical_json, ErpError};
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::erp::{begin_field, ErpGateError};
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

fn map_field_result(error: FieldError) -> Result<(StatusCode, ErpBody), ErpGateError> {
    match error {
        FieldError::Domain(ErpError::Forbidden) => Ok(forbidden()),
        FieldError::Domain(ErpError::NotFound) => Ok(unknown_resource()),
        FieldError::Domain(ErpError::Conflict) => Ok(conflict(
            "The requested operation conflicted with current state.",
        )),
        FieldError::Domain(ErpError::Unavailable)
        | FieldError::Invalid
        | FieldError::BadRequest(_) => Ok(invalid_submission()),
        FieldError::Conflict(msg) => Ok(conflict(msg)),
        FieldError::InvalidTransition(msg) => Ok(conflict(msg)),
        FieldError::Database { ledger, source } => Err(ErpGateError::Database(ledger, source)),
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
    pub fn id(&self) -> &str {
        match self {
            Self::Human(id) => id,
            Self::Service { id, .. } => id,
        }
    }

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
pub struct FieldSubmission {
    pub tenant: TenantId,
    pub command: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldStatusQuery {
    pub tenant: TenantId,
    pub command_id: Option<String>,
    pub command_id_alt: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldAccessQuery {
    pub tenant: TenantId,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldJobsQuery {
    pub tenant: TenantId,
    pub limit: Option<i64>,
    pub status: Option<String>,
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldDetailQuery {
    pub tenant: TenantId,
    pub id: Option<String>,
    pub job_id: Option<String>,
}

fn compute_fingerprint(command: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_json(command).as_bytes());
    format!("{:x}", hasher.finalize())
}

// --- Route handlers ---

pub(crate) async fn command(
    State(state): State<AppState>,
    headers: HeaderMap,
    submission: Result<Json<FieldSubmission>, JsonRejection>,
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
    match submit_field_command(&state, actor, submission.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<FieldStatusQuery>, QueryRejection>,
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
    let cmd_id = query
        .0
        .command_id
        .or(query.0.command_id_alt)
        .unwrap_or_default();
    match field_status_read(&state, actor, query.0.tenant, &cmd_id).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn field_access_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<FieldAccessQuery>, QueryRejection>,
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
    match field_access_read(&state, actor, query.0.tenant).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn field_jobs(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<FieldJobsQuery>, QueryRejection>,
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
    match field_jobs_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn field_job_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<FieldDetailQuery>, QueryRejection>,
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
    let job_id = query.0.id.or(query.0.job_id).unwrap_or_default();
    match field_detail_read(&state, actor, query.0.tenant, &job_id).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn field_audit(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<FieldDetailQuery>, QueryRejection>,
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
    let job_id = query.0.id.or(query.0.job_id).unwrap_or_default();
    match field_audit_read(&state, actor, query.0.tenant, &job_id).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

// --- Pipeline steps ---

async fn submit_field_command(
    state: &AppState,
    actor: ErpActor,
    submission: FieldSubmission,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let command = match parse_command(&submission.command) {
        Ok(cmd) => cmd,
        Err(_) => return Ok(invalid_submission()),
    };

    let fp = compute_fingerprint(&submission.command);

    let mut transaction = begin_field(state, submission.tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result = record_field_command(
        connection,
        submission.tenant.as_str(),
        &principal,
        command,
        &fp,
        &submission.command,
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
            map_field_result(err)
        }
    }
}

async fn field_status_read(
    state: &AppState,
    actor: ErpActor,
    tenant: TenantId,
    command_id: &str,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    if command_id.is_empty() {
        return Ok(invalid_submission());
    }

    let mut transaction = begin_field(state, tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result = field_command_status(connection, tenant.as_str(), &principal, command_id).await;

    match result {
        Ok(status) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            let doc = serde_json::to_value(&status).unwrap_or_default();
            Ok((StatusCode::OK, ErpBody::Doc(doc)))
        }
        Err(err) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            map_field_result(err)
        }
    }
}

async fn field_access_read(
    state: &AppState,
    actor: ErpActor,
    tenant: TenantId,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let mut transaction = begin_field(state, tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result = resolve_field_context(connection, tenant.as_str(), &principal).await;

    match result {
        Ok(access) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            let doc = serde_json::to_value(&access).unwrap_or_default();
            Ok((StatusCode::OK, ErpBody::Doc(doc)))
        }
        Err(err) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            map_field_result(err)
        }
    }
}

async fn field_jobs_read(
    state: &AppState,
    actor: ErpActor,
    query: FieldJobsQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let page_query = match parse_page_query(
        query.status.as_deref(),
        query.limit,
        query.cursor.as_deref(),
    ) {
        Ok(pq) => pq,
        Err(_) => return Ok(invalid_submission()),
    };

    let mut transaction = begin_field(state, query.tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result = list_field_jobs(connection, query.tenant.as_str(), &principal, page_query).await;

    match result {
        Ok(jobs) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            let doc = serde_json::to_value(&jobs).unwrap_or_default();
            Ok((StatusCode::OK, ErpBody::Doc(doc)))
        }
        Err(err) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            map_field_result(err)
        }
    }
}

async fn field_detail_read(
    state: &AppState,
    actor: ErpActor,
    tenant: TenantId,
    job_id: &str,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    if job_id.is_empty() {
        return Ok(invalid_submission());
    }

    let mut transaction = begin_field(state, tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result = get_field_job_detail(connection, tenant.as_str(), &principal, job_id).await;

    match result {
        Ok(Some(detail)) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            let doc = serde_json::to_value(&detail).unwrap_or_default();
            Ok((StatusCode::OK, ErpBody::Doc(doc)))
        }
        Ok(None) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            Ok(unknown_resource())
        }
        Err(err) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            map_field_result(err)
        }
    }
}

async fn field_audit_read(
    state: &AppState,
    actor: ErpActor,
    tenant: TenantId,
    job_id: &str,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    if job_id.is_empty() {
        return Ok(invalid_submission());
    }

    let mut transaction = begin_field(state, tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result = list_field_audit(connection, tenant.as_str(), &principal, job_id).await;

    match result {
        Ok(entries) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            let doc = serde_json::to_value(&entries).unwrap_or_default();
            Ok((StatusCode::OK, ErpBody::Doc(doc)))
        }
        Err(err) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            map_field_result(err)
        }
    }
}
