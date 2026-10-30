//! The laundry routes — the Archxy 2 port of the R08 laundry surface against
//! `archava_laundry`, in the shape pinned by the other ERP cohorts:
//! extraction → session → ONE tenant transaction whose member ladder, grant,
//! idempotency claim, order lifecycle, sequential process events and findings
//! run on the adapters' step functions.
//!
//! ## Routes
//!
//! - `POST /erp/laundry/commands` — parses submission, validates session, runs the command ladder.
//! - `GET /erp/laundry/commands/status?tenant=&requestId=` — returns `unknown` or `recorded`.
//! - `GET /erp/laundry/access?tenant=` — effective permissions, owner id, scope.
//! - `GET /erp/laundry/orders?tenant=&limit=&offset=&status=&kind=` — list orders query.
//! - `GET /erp/laundry/orders/detail?tenant=&id=` — order detail with lines, process, findings.
//! - `GET /erp/laundry/process?tenant=&orderId=` — process events query.
//! - `GET /erp/laundry/findings?tenant=&orderId=` — findings query.

use archava_adapters::erp::laundry::{
    get_laundry_order_detail, laundry_command_status, list_laundry_findings, list_laundry_orders,
    list_laundry_process, record_laundry_command, resolve_laundry_context, LaundryError,
};
use archava_core::tenant::TenantId;
use archava_domain::erp::laundry::{parse_mutation, parse_page_query};
use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
use archava_domain::erp::{canonical_json, ErpError};
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::erp::{begin_laundry, ErpGateError};
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
                "The requested state transition or command revision conflicted.".to_string(),
            ),
            ..CommandOutcome::base("denied")
        }),
    )
}

fn map_laundry_result(error: LaundryError) -> Result<(StatusCode, ErpBody), ErpGateError> {
    match error {
        LaundryError::Domain(ErpError::Forbidden) => Ok(forbidden()),
        LaundryError::Domain(ErpError::NotFound) => Ok(unknown_resource()),
        LaundryError::Domain(ErpError::Conflict) => Ok(conflict()),
        LaundryError::Domain(ErpError::Unavailable)
        | LaundryError::Invalid
        | LaundryError::BadRequest(_) => Ok(invalid_submission()),
        LaundryError::InvalidTransition(_)
        | LaundryError::ProcessOutOfOrder(_)
        | LaundryError::UnreconciledExceptions(_) => Ok(conflict()),
        LaundryError::Database { ledger, source } => Err(ErpGateError::Database(ledger, source)),
    }
}

fn gate_outcome(error: ErpGateError) -> (StatusCode, ErpBody) {
    match error {
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
pub struct LaundrySubmission {
    pub tenant: TenantId,
    pub mutation: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaundryStatusQuery {
    pub tenant: TenantId,
    pub request_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaundryAccessQuery {
    pub tenant: TenantId,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaundryOrdersQuery {
    pub tenant: TenantId,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub status: Option<String>,
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaundryDetailQuery {
    pub tenant: TenantId,
    pub id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaundryChildQuery {
    pub tenant: TenantId,
    pub order_id: Uuid,
}

fn fingerprint(state: &AppState, tenant: &TenantId, actor: &ErpActor, mutation: &Value) -> String {
    let document = json!({
        "domain": "laundry",
        "tenant": tenant.as_str(),
        "principal": {"kind": "human", "id": actor.id()},
        "command": mutation,
    });
    state
        .identity()
        .digest("laundry-command-fingerprint", &canonical_json(&document))
}

// --- Routes ---

pub(crate) async fn laundry_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    submission: Result<Json<LaundrySubmission>, JsonRejection>,
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
    match submit_laundry_command(&state, actor, submission.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn laundry_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<LaundryStatusQuery>, QueryRejection>,
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
    match laundry_status_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn laundry_access_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<LaundryAccessQuery>, QueryRejection>,
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
    match laundry_access_read(&state, actor, query.0.tenant).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn laundry_orders_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<LaundryOrdersQuery>, QueryRejection>,
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
    match laundry_orders_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn laundry_detail_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<LaundryDetailQuery>, QueryRejection>,
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
    match laundry_detail_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn laundry_process_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<LaundryChildQuery>, QueryRejection>,
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
    match laundry_process_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

pub(crate) async fn laundry_findings_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<LaundryChildQuery>, QueryRejection>,
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
    match laundry_findings_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

// --- Implementation ---

pub async fn submit_laundry_command(
    state: &AppState,
    actor: ErpActor,
    submission: LaundrySubmission,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    if principal.kind != PrincipalKind::Human {
        return Ok(forbidden());
    }

    let mutation = match parse_mutation(&submission.mutation) {
        Ok(m) => m,
        Err(_) => return Ok(invalid_submission()),
    };

    let fp = fingerprint(state, &submission.tenant, &actor, &submission.mutation);

    let mut transaction = begin_laundry(state, submission.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_laundry_context(connection, submission.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_laundry_result(error);
            }
        }
    };

    let receipt = {
        let connection = transaction.checked_executor().await?;
        match record_laundry_command(
            connection,
            submission.tenant.as_str(),
            &access,
            &fp,
            &mutation,
        )
        .await
        {
            Ok(receipt) => receipt,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_laundry_result(error);
            }
        }
    };

    if let Err(error) = transaction.commit().await {
        return Err(ErpGateError::from(error));
    }

    Ok((
        StatusCode::OK,
        ErpBody::Doc(serde_json::to_value(&receipt).expect("a typed receipt serializes")),
    ))
}

pub async fn laundry_status_read(
    state: &AppState,
    actor: ErpActor,
    query: LaundryStatusQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_laundry(state, query.tenant.clone()).await?;
    let status = {
        let connection = transaction.checked_executor().await?;
        match laundry_command_status(
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
                return map_laundry_result(error);
            }
        }
    };
    transaction.commit().await?;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(serde_json::to_value(&status).expect("serializable")),
    ))
}

pub async fn laundry_access_read(
    state: &AppState,
    actor: ErpActor,
    tenant: TenantId,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_laundry(state, tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_laundry_context(connection, tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_laundry_result(error);
            }
        }
    };
    transaction.commit().await?;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(serde_json::to_value(&access).expect("serializable")),
    ))
}

pub async fn laundry_orders_read(
    state: &AppState,
    actor: ErpActor,
    query: LaundryOrdersQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let page_query = match parse_page_query(
        query.limit,
        query.offset,
        query.status.as_deref(),
        query.kind.as_deref(),
    ) {
        Ok(q) => q,
        Err(_) => return Ok(invalid_submission()),
    };

    let mut transaction = begin_laundry(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_laundry_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_laundry_result(error);
            }
        }
    };

    let orders = {
        let connection = transaction.checked_executor().await?;
        match list_laundry_orders(connection, query.tenant.as_str(), &access, &page_query).await {
            Ok(orders) => orders,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_laundry_result(error);
            }
        }
    };
    transaction.commit().await?;

    Ok((
        StatusCode::OK,
        ErpBody::Doc(serde_json::to_value(&orders).expect("serializable")),
    ))
}

pub async fn laundry_detail_read(
    state: &AppState,
    actor: ErpActor,
    query: LaundryDetailQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_laundry(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_laundry_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_laundry_result(error);
            }
        }
    };

    let detail = {
        let connection = transaction.checked_executor().await?;
        match get_laundry_order_detail(
            connection,
            query.tenant.as_str(),
            &access,
            &query.id.to_string(),
        )
        .await
        {
            Ok(Some(detail)) => detail,
            Ok(None) => {
                let _ = transaction.rollback().await;
                return Ok(unknown_resource());
            }
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_laundry_result(error);
            }
        }
    };
    transaction.commit().await?;

    Ok((
        StatusCode::OK,
        ErpBody::Doc(serde_json::to_value(&detail).expect("serializable")),
    ))
}

pub async fn laundry_process_read(
    state: &AppState,
    actor: ErpActor,
    query: LaundryChildQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_laundry(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_laundry_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_laundry_result(error);
            }
        }
    };

    let events = {
        let connection = transaction.checked_executor().await?;
        match list_laundry_process(
            connection,
            query.tenant.as_str(),
            &access,
            &query.order_id.to_string(),
        )
        .await
        {
            Ok(events) => events,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_laundry_result(error);
            }
        }
    };
    transaction.commit().await?;

    Ok((
        StatusCode::OK,
        ErpBody::Doc(serde_json::to_value(&events).expect("serializable")),
    ))
}

pub async fn laundry_findings_read(
    state: &AppState,
    actor: ErpActor,
    query: LaundryChildQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_laundry(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_laundry_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_laundry_result(error);
            }
        }
    };

    let findings = {
        let connection = transaction.checked_executor().await?;
        match list_laundry_findings(
            connection,
            query.tenant.as_str(),
            &access,
            &query.order_id.to_string(),
        )
        .await
        {
            Ok(findings) => findings,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_laundry_result(error);
            }
        }
    };
    transaction.commit().await?;

    Ok((
        StatusCode::OK,
        ErpBody::Doc(serde_json::to_value(&findings).expect("serializable")),
    ))
}
