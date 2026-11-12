//! Authenticated read-only workspaces for the retained business domains.
use super::commerce::{error, refused, unavailable};
use crate::router::{authenticate, query_rejection, AppState};
use archava_adapters::{
    erp::projections::{workspace_view, WorkspaceDomain},
    rls::TenantTransaction,
};
use archava_core::tenant::TenantId;
use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
use axum::{
    extract::{rejection::QueryRejection, Query, State},
    http::{HeaderMap, StatusCode},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionQuery {
    pub tenant: TenantId,
    #[serde(default)]
    pub search: String,
    pub collection: Option<String>,
    pub id: Option<String>,
    #[serde(default)]
    pub cursor: String,
    #[serde(default = "default_limit")]
    pub limit: i64,
}
fn default_limit() -> i64 {
    50
}
type Response = (StatusCode, Json<Value>);
async fn read(
    state: AppState,
    headers: HeaderMap,
    query: Result<Query<ProjectionQuery>, QueryRejection>,
    domain: WorkspaceDomain,
) -> Response {
    let input = match query {
        Ok(Query(v)) => v,
        Err(e) => {
            let (s, b) = query_rejection(e);
            return (s, Json(json!(b.0)));
        }
    };
    if input.search.chars().count() > 160
        || input.cursor.len() > 256
        || input.id.as_ref().is_some_and(|id| id.len() > 128)
        || !(1..=100).contains(&input.limit)
    {
        return refused(
            StatusCode::BAD_REQUEST,
            "invalid_query",
            "Search or page bounds are invalid.",
        );
    }
    let session = match authenticate(&state, &headers, false).await {
        Ok(v) => v,
        Err((s, b)) => return (s, Json(json!(b.0))),
    };
    let principal = PurchasePrincipal {
        kind: PrincipalKind::Human,
        id: session.principal_id,
        on_behalf_of: None,
    };
    let mut tx = match TenantTransaction::begin(
        state.pool(),
        domain.namespace(),
        domain.slug(),
        input.tenant.clone(),
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return unavailable(e),
    };
    if let Err(error) = crate::own::require_erp_module(&mut tx, domain.namespace()).await {
        return error.response();
    }
    let conn = match tx.checked_executor().await {
        Ok(v) => v,
        Err(e) => return unavailable(e),
    };
    let value = match workspace_view(
        conn,
        input.tenant.as_str(),
        &principal,
        domain,
        archava_adapters::erp::projections::WorkspacePage {
            search: &input.search,
            collection: input.collection.as_deref(),
            cursor: &input.cursor,
            limit: input.limit,
            selected: input.id.as_deref(),
        },
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return error(e),
    };
    match tx.commit().await {
        Ok(()) => (StatusCode::OK, Json(value)),
        Err(e) => unavailable(e),
    }
}
macro_rules! endpoint {
    ($name:ident,$domain:ident) => {
        pub async fn $name(
            State(state): State<AppState>,
            headers: HeaderMap,
            query: Result<Query<ProjectionQuery>, QueryRejection>,
        ) -> Response {
            read(state, headers, query, WorkspaceDomain::$domain).await
        }
    };
}
endpoint!(subscriptions, Subscriptions);
endpoint!(attendance, Attendance);
endpoint!(laundry, Laundry);
endpoint!(field, Field);
endpoint!(education, Education);
endpoint!(fulfillment, Fulfillment);
endpoint!(organizations, Organizations);
endpoint!(proposals, Proposals);
endpoint!(projects, Projects);
endpoint!(foodservice, Foodservice);
endpoint!(scheduling, Scheduling);
endpoint!(accommodation, Accommodation);
endpoint!(assets, Assets);
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/erp/organizations/workspace", get(organizations))
        .route("/erp/proposals/workspace", get(proposals))
        .route("/erp/projects/workspace", get(projects))
        .route("/erp/foodservice/workspace", get(foodservice))
        .route("/erp/scheduling/workspace", get(scheduling))
        .route("/erp/accommodation/workspace", get(accommodation))
        .route("/erp/assets/workspace", get(assets))
        .route("/erp/subscriptions/workspace", get(subscriptions))
        .route("/erp/attendance/workspace", get(attendance))
        .route("/erp/laundry/workspace", get(laundry))
        .route("/erp/field/workspace", get(field))
        .route("/erp/education/workspace", get(education))
        .route("/erp/fulfillment/workspace", get(fulfillment))
}
