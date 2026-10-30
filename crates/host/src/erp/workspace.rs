//! Read projections for native operator workspaces; each uses its domain's current grant resolver.
use super::commerce::{error, refused, unavailable};
use crate::router::{authenticate, AppState};
use archava_adapters::{
    erp::{billing, purchasing},
    rls::TenantTransaction,
};
use archava_core::tenant::TenantId;
use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceQuery {
    pub tenant: TenantId,
    #[serde(default)]
    pub search: String,
}
type Response = (StatusCode, Json<Value>);
async fn read(
    state: &AppState,
    headers: &HeaderMap,
    input: WorkspaceQuery,
    is_billing: bool,
) -> Result<Value, Response> {
    if input.search.chars().count() > 160 {
        return Err(refused(
            StatusCode::BAD_REQUEST,
            "invalid_search",
            "Search is too long.",
        ));
    }
    let session = authenticate(state, headers, false)
        .await
        .map_err(|(s, b)| (s, Json(json!(b.0))))?;
    let principal = PurchasePrincipal {
        kind: PrincipalKind::Human,
        id: session.principal_id,
        on_behalf_of: None,
    };
    let (namespace, ledger) = if is_billing {
        ("archava_billing", "billing")
    } else {
        ("archava_purchasing", "purchasing")
    };
    let mut tx = TenantTransaction::begin(state.pool(), namespace, ledger, input.tenant.clone())
        .await
        .map_err(unavailable)?;
    let value = if is_billing {
        let ctx = billing::resolve_billing_context(
            tx.checked_executor().await.map_err(unavailable)?,
            input.tenant.as_str(),
            &principal,
        )
        .await
        .map_err(error)?;
        billing::workspace_view(
            tx.checked_executor().await.map_err(unavailable)?,
            input.tenant.as_str(),
            &ctx,
            &input.search,
        )
        .await
        .map_err(error)?
    } else {
        let ctx = purchasing::resolve_purchasing_context(
            tx.checked_executor().await.map_err(unavailable)?,
            input.tenant.as_str(),
            &principal,
        )
        .await
        .map_err(error)?;
        purchasing::workspace_view(
            tx.checked_executor().await.map_err(unavailable)?,
            input.tenant.as_str(),
            &ctx,
            &input.search,
        )
        .await
        .map_err(error)?
    };
    tx.commit().await.map_err(unavailable)?;
    Ok(value)
}
pub async fn purchasing(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(input): Query<WorkspaceQuery>,
) -> Response {
    match read(&state, &headers, input, false).await {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(r) => r,
    }
}
pub async fn billing(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(input): Query<WorkspaceQuery>,
) -> Response {
    match read(&state, &headers, input, true).await {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(r) => r,
    }
}
