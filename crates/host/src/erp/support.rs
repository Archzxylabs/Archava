//! The support routes — the Archxy 2 port of the R07 support surface
//! against `archava_support`, in the shape [`crate::erp::attendance`] pinned:
//! extraction → session → ONE tenant transaction whose member ladder, grant,
//! idempotency claim, case effect and history all run on the adapters' step
//! functions.
//!
//! ## The six routes
//!
//! - `POST /erp/support/commands` — the TS `mutate()`: parse, the
//!   HUMAN-OPERATOR rule, the claim/authorize/arm/history pipeline. The OK
//!   body is the BARE receipt; the refusals are the TS `SupportError` codes
//!   folded by class (403/404/409/400).
//! - `GET /erp/support/commands/status?tenant=&requestId=` — the
//!   `unknown`/`committed` document.
//! - `GET /erp/support/access?tenant=` — the effective grant envelope.
//! - `GET /erp/support/assignees?tenant=&limit=&after=` — the human-reader
//!   picker (scope `all` only).
//! - `GET /erp/support/cases?tenant=&limit=&after=&state=&priority=&overdue=`
//!   — the worklist page with the clock the overdue flag used.
//! - `GET /erp/support/cases/detail?tenant=&id=` — the case plus the three
//!   bounded histories.
//!
//! ## The fingerprint
//!
//! The TS composition is `HMAC(key, canonical({domain,tenant,principal,
//! command}))`. The gate carries the same labelled HMAC over the canonical
//! JSON of `{domain:"support", tenant, principal, command}`; the command
//! member is the caller's own command document (the strict parse has already
//! refused anything the schema would refuse, and a replay resends the same
//! document). `principal` is the human session principal — a service
//! principal never reaches this route.
//!
//! ## The labor order
//!
//! The TS service refuses a service principal on every mutation, the
//! assignee picker and the command-status read. This gate's session is
//! always human, and the ledger's membership kind must also match: a human
//! session cannot use a `service` membership row.

use archava_adapters::erp::support::{
    record_support_command, resolve_support_context, support_assignees, support_cases,
    support_command_status, support_detail, SupportError,
};
use archava_core::tenant::TenantId;
use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
use archava_domain::erp::support::{
    command_document, parse_mutation, SupportAccess, SupportCommandStatus,
};
use archava_domain::erp::{canonical_json, ErpError};
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::erp::{begin_support, ErpGateError};
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

fn conflict() -> (StatusCode, ErpBody) {
    (
        StatusCode::CONFLICT,
        ErpBody::Outcome(CommandOutcome {
            reason: Some("conflict".to_string()),
            message: Some(
                "A concurrent write or a state boundary refused this submission.".to_string(),
            ),
            ..CommandOutcome::base("refused")
        }),
    )
}

fn map_support_result(error: SupportError) -> Result<(StatusCode, ErpBody), ErpGateError> {
    match error {
        SupportError::Domain(ErpError::Forbidden) => Ok(forbidden()),
        SupportError::Domain(ErpError::NotFound) => Ok(unknown_resource()),
        SupportError::Domain(ErpError::Conflict) => Ok(conflict()),
        SupportError::Domain(ErpError::Unavailable) | SupportError::Invalid => {
            Ok(invalid_submission())
        }
        SupportError::Database { ledger, source } => Err(ErpGateError::Database(ledger, source)),
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
}

impl ErpActor {
    fn principal(&self) -> PurchasePrincipal {
        match self {
            Self::Human(id) => PurchasePrincipal::human(id.clone()),
        }
    }

    fn id(&self) -> &str {
        match self {
            Self::Human(id) => id,
        }
    }
}

// --- Shapes -----------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupportSubmission {
    pub tenant: TenantId,
    pub mutation: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupportStatusQuery {
    pub tenant: TenantId,
    pub request_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupportTenantQuery {
    pub tenant: TenantId,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupportCaseQuery {
    pub search: Option<String>,
    pub tenant: TenantId,
    pub limit: Option<i64>,
    pub after: Option<Uuid>,
    pub state: Option<String>,
    pub priority: Option<String>,
    pub overdue: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupportAssigneeQuery {
    pub tenant: TenantId,
    pub limit: Option<i64>,
    pub after: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupportDetailQuery {
    pub tenant: TenantId,
    pub id: Uuid,
}

fn fingerprint(state: &AppState, tenant: &TenantId, actor: &ErpActor, command: &Value) -> String {
    let document = json!({
        "domain": "support",
        "tenant": tenant.as_str(),
        "principal": {"kind": "human", "id": actor.id()},
        "command": command,
    });
    state
        .identity()
        .digest("support-command-fingerprint", &canonical_json(&document))
}

// --- Routes -----------------------------------------------------------------

pub(crate) async fn support_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    submission: Result<Json<SupportSubmission>, JsonRejection>,
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
    match submit_support_command(&state, actor, submission.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn support_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<SupportStatusQuery>, QueryRejection>,
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
    match support_status_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn support_access_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<SupportTenantQuery>, QueryRejection>,
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
    match support_access_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn support_assignee_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<SupportAssigneeQuery>, QueryRejection>,
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
    match support_assignee_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn support_case_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<SupportCaseQuery>, QueryRejection>,
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
    match support_case_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn support_detail_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<SupportDetailQuery>, QueryRejection>,
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
    match support_detail_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

// --- The writes -------------------------------------------------------------

pub async fn submit_support_command(
    state: &AppState,
    actor: ErpActor,
    submission: SupportSubmission,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    if principal.kind != PrincipalKind::Human {
        return Ok(forbidden());
    }
    let mutation = match parse_mutation(&submission.mutation) {
        Ok(mutation) => mutation,
        Err(_) => return Ok(invalid_submission()),
    };
    let fingerprint = fingerprint(
        state,
        &submission.tenant,
        &actor,
        &command_document(&mutation.command),
    );

    let mut transaction = begin_support(state, submission.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_support_context(connection, submission.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_support_result(error);
            }
        }
    };
    let receipt = {
        let connection = transaction.checked_executor().await?;
        match record_support_command(
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
                return map_support_result(error);
            }
        }
    };
    if let Err(error) = transaction.commit().await {
        // The support history is a DEFERRED constraint: the complete-chain
        // verdict arrives at commit. A SQLSTATE the repository reclassifies
        // is the same conflict the TS api maps; anything else is a real
        // unavailability.
        if commit_conflict(&error) {
            return Ok(conflict());
        }
        return Err(ErpGateError::from(error));
    }
    Ok((
        StatusCode::OK,
        ErpBody::Doc(serde_json::to_value(&receipt).expect("a typed receipt serializes")),
    ))
}

/// The repository's conflict whitelist, read from a commit-time
/// `TenantContextError` (the deferred history trigger's 23514 is the one the
/// support ledger actually raises).
fn commit_conflict(error: &archava_adapters::rls::TenantContextError) -> bool {
    use archava_adapters::rls::TenantContextError;
    let TenantContextError::Database { source, .. } = error else {
        return false;
    };
    let sqlx::Error::Database(database) = source else {
        return false;
    };
    matches!(
        database.code().as_deref(),
        Some("23514")
            | Some("23503")
            | Some("23505")
            | Some("22003")
            | Some("40P01")
            | Some("55P03")
    )
}

// --- The reads --------------------------------------------------------------

pub async fn support_status_read(
    state: &AppState,
    actor: ErpActor,
    query: SupportStatusQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_support(state, query.tenant.clone()).await?;
    let status: SupportCommandStatus = {
        let connection = transaction.checked_executor().await?;
        match support_command_status(
            connection,
            query.tenant.as_str(),
            &principal,
            &query.request_id.to_string(),
        )
        .await
        {
            Ok(status) => status,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_support_result(error);
            }
        }
    };
    let _ = transaction.rollback().await;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(serde_json::to_value(&status).expect("a typed status serializes")),
    ))
}

pub async fn support_access_read(
    state: &AppState,
    actor: ErpActor,
    query: SupportTenantQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_support(state, query.tenant.clone()).await?;
    let access: SupportAccess = {
        let connection = transaction.checked_executor().await?;
        match resolve_support_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_support_result(error);
            }
        }
    };
    let _ = transaction.rollback().await;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(serde_json::to_value(&access).expect("a typed access serializes")),
    ))
}

pub async fn support_assignee_read(
    state: &AppState,
    actor: ErpActor,
    query: SupportAssigneeQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Ok(invalid_submission());
    }
    let principal = actor.principal();
    let mut transaction = begin_support(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_support_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_support_result(error);
            }
        }
    };
    let page = {
        let connection = transaction.checked_executor().await?;
        let mut document = serde_json::Map::new();
        document.insert("limit".to_string(), json!(limit));
        if let Some(after) = &query.after {
            document.insert("after".to_string(), json!(after));
        }
        let document = Value::Object(document);
        let parsed = match archava_domain::erp::support::parse_assignee_page(&document) {
            Ok(parsed) => parsed,
            Err(_) => {
                let _ = transaction.rollback().await;
                return Ok(invalid_submission());
            }
        };
        match support_assignees(connection, query.tenant.as_str(), &access, &parsed).await {
            Ok(page) => page,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_support_result(error);
            }
        }
    };
    let _ = transaction.rollback().await;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(serde_json::to_value(&page).expect("a typed page serializes")),
    ))
}

pub async fn support_case_read(
    state: &AppState,
    actor: ErpActor,
    query: SupportCaseQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let mut document = serde_json::Map::new();
    if let Some(search) = query.search {
        document.insert("search".into(), json!(search));
    }
    if let Some(limit) = query.limit {
        document.insert("limit".to_string(), json!(limit));
    }
    if let Some(after) = query.after {
        document.insert("after".to_string(), json!(after.to_string()));
    }
    if let Some(state) = &query.state {
        document.insert("state".to_string(), json!(state));
    }
    if let Some(priority) = &query.priority {
        document.insert("priority".to_string(), json!(priority));
    }
    if let Some(overdue) = query.overdue {
        document.insert("overdue".to_string(), json!(overdue));
    }
    let page_document = Value::Object(document);
    if archava_domain::erp::support::parse_case_page(&page_document).is_err() {
        return Ok(invalid_submission());
    }
    let principal = actor.principal();
    let mut transaction = begin_support(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_support_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_support_result(error);
            }
        }
    };
    let page = {
        let connection = transaction.checked_executor().await?;
        match support_cases(connection, query.tenant.as_str(), &access, &page_document).await {
            Ok(page) => page,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_support_result(error);
            }
        }
    };
    let _ = transaction.rollback().await;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(serde_json::to_value(&page).expect("a typed page serializes")),
    ))
}

pub async fn support_detail_read(
    state: &AppState,
    actor: ErpActor,
    query: SupportDetailQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_support(state, query.tenant.clone()).await?;
    let detail = {
        let connection = transaction.checked_executor().await?;
        let access =
            match resolve_support_context(connection, query.tenant.as_str(), &principal).await {
                Ok(access) => access,
                Err(error) => {
                    let _ = transaction.rollback().await;
                    return map_support_result(error);
                }
            };
        match support_detail(
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
                return map_support_result(error);
            }
        }
    };
    let _ = transaction.rollback().await;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(serde_json::to_value(&detail).expect("a typed detail serializes")),
    ))
}
