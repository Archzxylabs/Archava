//! Session-derived native commerce HTTP surface. No client principal/grant fields.
use crate::router::{authenticate, AppState};
use archava_adapters::{
    erp::{
        commerce as store,
        commerce_kernel::{resolve_commerce_context, CommerceContext},
        PurchaseError, COMMERCE_LEDGER, COMMERCE_NAMESPACE,
    },
    rls::TenantTransaction,
};
use archava_core::tenant::TenantId;
use archava_domain::erp::{
    commerce::CommerceCommand,
    purchasing::{PrincipalKind, PurchasePrincipal},
    ErpError,
};
use axum::{
    extract::{rejection::JsonRejection, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;
type Response = (StatusCode, Json<Value>);
type Result<T> = std::result::Result<T, Response>;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Submission {
    pub tenant: TenantId,
    pub request_id: Uuid,
    pub command: CommerceCommand,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceQuery {
    pub tenant: TenantId,
    #[serde(default)]
    pub search: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatusQuery {
    pub tenant: TenantId,
    pub request_id: Uuid,
}
pub(super) fn refused(status: StatusCode, reason: &'static str, message: &'static str) -> Response {
    (
        status,
        Json(json!({"decision":"refused","reason":reason,"message":message})),
    )
}
pub(super) fn unavailable(error: impl std::fmt::Display) -> Response {
    tracing::error!(error=%error,"commerce request refused");
    refused(
        StatusCode::SERVICE_UNAVAILABLE,
        "unavailable",
        "Commerce storage is unavailable.",
    )
}
pub(super) fn error(error: PurchaseError) -> Response {
    match error {
        PurchaseError::Domain(ErpError::Forbidden) => refused(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Commerce access was refused.",
        ),
        PurchaseError::Domain(ErpError::NotFound) => refused(
            StatusCode::NOT_FOUND,
            "not_found",
            "The record was not found.",
        ),
        PurchaseError::Domain(ErpError::Conflict) => refused(
            StatusCode::CONFLICT,
            "conflict",
            "The record changed or the operation is not valid in its current state.",
        ),
        other => unavailable(other),
    }
}
async fn begin<'a>(
    state: &'a AppState,
    headers: &HeaderMap,
    tenant: &TenantId,
    write: bool,
) -> Result<(TenantTransaction<'a>, PurchasePrincipal, CommerceContext)> {
    let session = authenticate(state, headers, write)
        .await
        .map_err(|(s, b)| (s, Json(json!(b.0))))?;
    let principal = PurchasePrincipal {
        kind: PrincipalKind::Human,
        id: session.principal_id,
        on_behalf_of: None,
    };
    let mut tx = TenantTransaction::begin(
        state.pool(),
        COMMERCE_NAMESPACE,
        COMMERCE_LEDGER,
        tenant.clone(),
    )
    .await
    .map_err(unavailable)?;
    let context = resolve_commerce_context(
        tx.checked_executor().await.map_err(unavailable)?,
        tenant.as_str(),
        &principal,
    )
    .await
    .map_err(error)?;
    Ok((tx, principal, context))
}
async fn submit(state: &AppState, headers: &HeaderMap, input: Submission) -> Result<Value> {
    input
        .command
        .validate()
        .map_err(|message| refused(StatusCode::BAD_REQUEST, "invalid_submission", message))?;
    let (mut tx, principal, context) = begin(state, headers, &input.tenant, true).await?;
    let fingerprint=state.identity().digest("erp-commerce-fingerprint",&json!({"domain":"commerce","tenant":input.tenant.as_str(),"principal":principal,"command":input.command}).to_string());
    let receipt = store::record(
        tx.checked_executor().await.map_err(unavailable)?,
        input.tenant.as_str(),
        &principal,
        &context,
        input.request_id,
        &fingerprint,
        &input.command,
    )
    .await
    .map_err(error)?;
    tx.commit().await.map_err(unavailable)?;
    Ok(
        json!({"decision":"receipt","tenant":input.tenant.as_str(),"requestId":input.request_id,"receipt":receipt}),
    )
}
pub async fn command(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: std::result::Result<Json<Submission>, JsonRejection>,
) -> Response {
    let Ok(Json(input)) = body else {
        return refused(
            StatusCode::BAD_REQUEST,
            "invalid_submission",
            "Invalid commerce command.",
        );
    };
    match submit(&state, &headers, input).await {
        Ok(value) => (StatusCode::OK, Json(value)),
        Err(response) => response,
    }
}
async fn read_workspace(
    state: &AppState,
    headers: &HeaderMap,
    input: WorkspaceQuery,
) -> Result<Value> {
    if input.search.chars().count() > 160 {
        return Err(refused(
            StatusCode::BAD_REQUEST,
            "invalid_search",
            "Search is too long.",
        ));
    }
    let (mut tx, _principal, context) = begin(state, headers, &input.tenant, false).await?;
    let value = store::workspace(
        tx.checked_executor().await.map_err(unavailable)?,
        input.tenant.as_str(),
        &context,
        &input.search,
    )
    .await
    .map_err(error)?;
    tx.commit().await.map_err(unavailable)?;
    Ok(value)
}
pub async fn workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(input): Query<WorkspaceQuery>,
) -> Response {
    match read_workspace(&state, &headers, input).await {
        Ok(value) => (StatusCode::OK, Json(value)),
        Err(response) => response,
    }
}
async fn read_status(state: &AppState, headers: &HeaderMap, input: StatusQuery) -> Result<Value> {
    let (mut tx, principal, context) = begin(state, headers, &input.tenant, false).await?;
    let receipt = store::status(
        tx.checked_executor().await.map_err(unavailable)?,
        input.tenant.as_str(),
        &principal,
        &context,
        input.request_id,
    )
    .await
    .map_err(error)?;
    tx.commit().await.map_err(unavailable)?;
    Ok(match receipt {
        Some(receipt) => {
            json!({"status":"committed","requestId":input.request_id,"receipt":receipt})
        }
        None => json!({"status":"not_committed","requestId":input.request_id}),
    })
}
pub async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(input): Query<StatusQuery>,
) -> Response {
    match read_status(&state, &headers, input).await {
        Ok(value) => (StatusCode::OK, Json(value)),
        Err(response) => response,
    }
}
