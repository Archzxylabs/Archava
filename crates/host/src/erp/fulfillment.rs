//! The fulfillment routes — the Archxy 2 port of the R08 fulfillment surface against
//! `archava_fulfillment`, in the shape pinned by the other ERP cohorts:
//! extraction → session → ONE tenant transaction whose member ladder, grant,
//! idempotency claim, job lifecycle, custody tracking, work lines and findings
//! run on the adapters' step functions.
//!
//! ## Routes
//!
//! - `POST /erp/fulfillment/commands` — parses submission, validates session, runs the command ladder.
//! - `GET /erp/fulfillment/commands/status?tenant=&commandId=` — returns `unknown` or `recorded`.
//! - `GET /erp/fulfillment/access?tenant=` — effective permissions, owner id, scope.
//! - `GET /erp/fulfillment/jobs?tenant=&limit=&offset=&status=&kind=` — list jobs query.
//! - `GET /erp/fulfillment/jobs/detail?tenant=&id=` — job detail with lines, findings, custody.
//! - `GET /erp/fulfillment/lines?tenant=&jobId=` — lines query.
//! - `GET /erp/fulfillment/findings?tenant=&jobId=` — findings query.
//! - `GET /erp/fulfillment/custody?tenant=&jobId=` — custody events query.

use archava_adapters::erp::fulfillment::{
    fulfillment_command_status, get_fulfillment_job_detail, list_fulfillment_custody,
    list_fulfillment_findings, list_fulfillment_jobs, list_fulfillment_lines,
    record_fulfillment_mutation, resolve_fulfillment_context, FulfillmentError,
};
use archava_core::tenant::TenantId;
use archava_domain::erp::fulfillment::{parse_mutation, parse_page_query};
use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
use archava_domain::erp::{canonical_json, ErpError};
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::erp::{begin_fulfillment, ErpGateError};
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

fn map_fulfillment_result(error: FulfillmentError) -> Result<(StatusCode, ErpBody), ErpGateError> {
    match error {
        FulfillmentError::Domain(ErpError::Forbidden) => Ok(forbidden()),
        FulfillmentError::Domain(ErpError::NotFound) => Ok(unknown_resource()),
        FulfillmentError::Domain(ErpError::Conflict) => Ok(conflict(
            "The requested operation conflicted with current state.",
        )),
        FulfillmentError::Domain(ErpError::Unavailable)
        | FulfillmentError::Invalid
        | FulfillmentError::BadRequest(_) => Ok(invalid_submission()),
        FulfillmentError::Conflict(msg) => Ok(conflict(msg)),
        FulfillmentError::InvalidTransition(msg) => Ok(conflict(msg)),
        FulfillmentError::CustodyOutOfOrder(msg) => Ok(conflict(msg)),
        FulfillmentError::UnreconciledExceptions(msg) => Ok(conflict(msg)),
        FulfillmentError::Database { ledger, source } => {
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
pub struct FulfillmentSubmission {
    pub tenant: TenantId,
    pub mutation: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FulfillmentStatusQuery {
    pub tenant: TenantId,
    pub command_id: Option<String>,
    pub request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FulfillmentAccessQuery {
    pub tenant: TenantId,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FulfillmentJobsQuery {
    pub tenant: TenantId,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub status: Option<String>,
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FulfillmentDetailQuery {
    pub tenant: TenantId,
    pub id: Option<String>,
    pub job_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FulfillmentChildQuery {
    pub tenant: TenantId,
    pub job_id: Option<String>,
    pub id: Option<String>,
}

fn fingerprint(state: &AppState, tenant: &TenantId, actor: &ErpActor, mutation: &Value) -> String {
    let document = json!({
        "domain": "fulfillment",
        "tenant": tenant.as_str(),
        "principal": {"kind": "human", "id": actor.id()},
        "command": mutation,
    });
    state.identity().digest(
        "fulfillment-command-fingerprint",
        &canonical_json(&document),
    )
}

// --- Route handlers ---

pub(crate) async fn fulfillment_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    submission: Result<Json<FulfillmentSubmission>, JsonRejection>,
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
    match submit_fulfillment_command(&state, actor, submission.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn fulfillment_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<FulfillmentStatusQuery>, QueryRejection>,
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
        .or(query.0.request_id)
        .unwrap_or_default();
    match fulfillment_status_read(&state, actor, query.0.tenant, &cmd_id).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn fulfillment_access_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<FulfillmentAccessQuery>, QueryRejection>,
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
    match fulfillment_access_read(&state, actor, query.0.tenant).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn fulfillment_jobs_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<FulfillmentJobsQuery>, QueryRejection>,
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
    match fulfillment_jobs_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn fulfillment_detail_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<FulfillmentDetailQuery>, QueryRejection>,
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
    match fulfillment_detail_read(&state, actor, query.0.tenant, &job_id).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn fulfillment_lines_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<FulfillmentChildQuery>, QueryRejection>,
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
    let job_id = query.0.job_id.or(query.0.id).unwrap_or_default();
    match fulfillment_lines_read(&state, actor, query.0.tenant, &job_id).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn fulfillment_findings_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<FulfillmentChildQuery>, QueryRejection>,
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
    let job_id = query.0.job_id.or(query.0.id).unwrap_or_default();
    match fulfillment_findings_read(&state, actor, query.0.tenant, &job_id).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn fulfillment_custody_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<FulfillmentChildQuery>, QueryRejection>,
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
    let job_id = query.0.job_id.or(query.0.id).unwrap_or_default();
    match fulfillment_custody_read(&state, actor, query.0.tenant, &job_id).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

// --- Pipeline steps ---

async fn submit_fulfillment_command(
    state: &AppState,
    actor: ErpActor,
    submission: FulfillmentSubmission,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let mutation = match parse_mutation(&submission.mutation) {
        Ok(m) => m,
        Err(_) => return Ok(invalid_submission()),
    };

    let fp = fingerprint(state, &submission.tenant, &actor, &submission.mutation);

    let mut transaction = begin_fulfillment(state, submission.tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result = record_fulfillment_mutation(
        connection,
        submission.tenant.as_str(),
        &principal,
        mutation,
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
            map_fulfillment_result(err)
        }
    }
}

async fn fulfillment_status_read(
    state: &AppState,
    actor: ErpActor,
    tenant: TenantId,
    command_id: &str,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    if command_id.is_empty() {
        return Ok(invalid_submission());
    }

    let mut transaction = begin_fulfillment(state, tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result =
        fulfillment_command_status(connection, tenant.as_str(), &principal, command_id).await;

    match result {
        Ok(status) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            let doc = serde_json::to_value(&status).unwrap_or_default();
            Ok((StatusCode::OK, ErpBody::Doc(doc)))
        }
        Err(err) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            map_fulfillment_result(err)
        }
    }
}

async fn fulfillment_access_read(
    state: &AppState,
    actor: ErpActor,
    tenant: TenantId,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let mut transaction = begin_fulfillment(state, tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result = resolve_fulfillment_context(connection, tenant.as_str(), &principal).await;

    match result {
        Ok(access) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            let doc = serde_json::to_value(&access).unwrap_or_default();
            Ok((StatusCode::OK, ErpBody::Doc(doc)))
        }
        Err(err) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            map_fulfillment_result(err)
        }
    }
}

async fn fulfillment_jobs_read(
    state: &AppState,
    actor: ErpActor,
    query: FulfillmentJobsQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let page_query = match parse_page_query(
        query.limit,
        query.offset,
        query.status.as_deref(),
        query.kind.as_deref(),
    ) {
        Ok(pq) => pq,
        Err(_) => return Ok(invalid_submission()),
    };

    let mut transaction = begin_fulfillment(state, query.tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result =
        list_fulfillment_jobs(connection, query.tenant.as_str(), &principal, page_query).await;

    match result {
        Ok(jobs) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            let doc = serde_json::to_value(&jobs).unwrap_or_default();
            Ok((StatusCode::OK, ErpBody::Doc(doc)))
        }
        Err(err) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            map_fulfillment_result(err)
        }
    }
}

async fn fulfillment_detail_read(
    state: &AppState,
    actor: ErpActor,
    tenant: TenantId,
    job_id: &str,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    if job_id.is_empty() {
        return Ok(invalid_submission());
    }

    let mut transaction = begin_fulfillment(state, tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result = get_fulfillment_job_detail(connection, tenant.as_str(), &principal, job_id).await;

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
            map_fulfillment_result(err)
        }
    }
}

async fn fulfillment_lines_read(
    state: &AppState,
    actor: ErpActor,
    tenant: TenantId,
    job_id: &str,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    if job_id.is_empty() {
        return Ok(invalid_submission());
    }

    let mut transaction = begin_fulfillment(state, tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result = list_fulfillment_lines(connection, tenant.as_str(), &principal, job_id).await;

    match result {
        Ok(lines) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            let doc = serde_json::to_value(&lines).unwrap_or_default();
            Ok((StatusCode::OK, ErpBody::Doc(doc)))
        }
        Err(err) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            map_fulfillment_result(err)
        }
    }
}

async fn fulfillment_findings_read(
    state: &AppState,
    actor: ErpActor,
    tenant: TenantId,
    job_id: &str,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    if job_id.is_empty() {
        return Ok(invalid_submission());
    }

    let mut transaction = begin_fulfillment(state, tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result = list_fulfillment_findings(connection, tenant.as_str(), &principal, job_id).await;

    match result {
        Ok(findings) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            let doc = serde_json::to_value(&findings).unwrap_or_default();
            Ok((StatusCode::OK, ErpBody::Doc(doc)))
        }
        Err(err) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            map_fulfillment_result(err)
        }
    }
}

async fn fulfillment_custody_read(
    state: &AppState,
    actor: ErpActor,
    tenant: TenantId,
    job_id: &str,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    if job_id.is_empty() {
        return Ok(invalid_submission());
    }

    let mut transaction = begin_fulfillment(state, tenant.clone()).await?;
    let connection = transaction.checked_executor().await?;

    let principal = actor.principal();
    let result = list_fulfillment_custody(connection, tenant.as_str(), &principal, job_id).await;

    match result {
        Ok(custody) => {
            transaction.commit().await.map_err(ErpGateError::from)?;
            let doc = serde_json::to_value(&custody).unwrap_or_default();
            Ok((StatusCode::OK, ErpBody::Doc(doc)))
        }
        Err(err) => {
            transaction.rollback().await.map_err(ErpGateError::from)?;
            map_fulfillment_result(err)
        }
    }
}
