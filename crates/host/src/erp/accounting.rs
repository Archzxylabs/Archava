//! Session-derived company books; explicit grants and module state are rechecked per transaction.
use crate::router::{authenticate, query_rejection, rejection, AppState};
use archava_adapters::{
    erp::{accounting as store, PurchaseError},
    rls::TenantTransaction,
};
use archava_core::tenant::TenantId;
use archava_domain::erp::{
    accommodation::parse_calendar_date, accounting, purchasing::PurchasePrincipal, ErpError,
};
use axum::{
    extract::{
        rejection::{JsonRejection, QueryRejection},
        Query, State,
    },
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;
pub(crate) async fn assert_control_readiness(
    pool: &sqlx::PgPool,
) -> Result<(), archava_adapters::rls::TenantContextError> {
    use archava_adapters::rls::{assert_migration_ledger, TenantContextError};
    let expected = archava_adapters::writer::expected_migrations();
    assert_migration_ledger(
        pool,
        "archava_control",
        "accounting writer control",
        &expected,
    )
    .await?;
    let complete: bool = sqlx::query_scalar("SELECT archava_control.write_guards_complete()")
        .fetch_one(pool)
        .await
        .map_err(|source| TenantContextError::Database {
            ledger: "accounting writer control",
            source,
        })?;
    if !complete {
        return Err(TenantContextError::Database {
            ledger: "accounting writer control",
            source: sqlx::Error::Protocol("Accounting write fence coverage is incomplete".into()),
        });
    }
    let source_complete: bool =
        sqlx::query_scalar("SELECT archava_accounting.source_guards_complete() AND archava_accounting.purchase_guards_complete() AND archava_accounting.stock_guards_complete() AND archava_accounting.bank_evidence_guards_complete() AND archava_accounting.bank_matching_guards_complete() AND archava_accounting.bank_period_guards_complete() AND archava_accounting.payable_guards_complete() AND archava_accounting.clearing_guards_complete() AND archava_accounting.terms_guards_complete() AND archava_accounting.opening_guards_complete() AND archava_accounting.tax_guards_complete() AND archava_accounting.tax_document_guards_complete()")
            .fetch_one(pool)
            .await
            .map_err(|source| TenantContextError::Database {
                ledger: "accounting invoice source",
                source,
            })?;
    if !source_complete {
        return Err(TenantContextError::Database {
            ledger: "accounting invoice source",
            source: sqlx::Error::Protocol(
                "Accounting invoice source guard coverage is incomplete".into(),
            ),
        });
    }
    Ok(())
}
type Response = (StatusCode, Json<Value>);
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Submission {
    pub tenant: TenantId,
    pub request_id: Uuid,
    pub command: Value,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceQuery {
    pub tenant: TenantId,
    pub company_id: Option<Uuid>,
    pub id: Option<Uuid>,
    #[serde(default)]
    pub search: String,
    pub collection: Option<String>,
    pub cursor: Option<Uuid>,
    #[serde(default = "page_limit")]
    pub limit: i64,
    /// The date the tax code list answers "which version applies" for.
    pub as_of: Option<String>,
}
fn page_limit() -> i64 {
    100
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatusQuery {
    pub tenant: TenantId,
    pub request_id: Uuid,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReportQuery {
    pub tenant: TenantId,
    pub book_id: Uuid,
    pub from: String,
    pub to: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaxDocumentsQuery {
    pub tenant: TenantId,
    pub book_id: Uuid,
    pub from: String,
    pub to: String,
    /// The `sourceId` of the last row of the page before.
    pub cursor: Option<Uuid>,
    #[serde(default = "page_limit")]
    pub limit: i64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubledgerQuery {
    pub tenant: TenantId,
    pub book_id: Uuid,
    pub side: store::SubledgerSide,
    pub as_of: String,
    pub cursor: Option<Uuid>,
    #[serde(default = "page_limit")]
    pub limit: i64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BankReadinessQuery {
    pub tenant: TenantId,
    pub bank_policy_id: Uuid,
    pub period_id: Uuid,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloseReadinessQuery {
    pub tenant: TenantId,
    pub period_id: Uuid,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InventoryControlQuery {
    pub tenant: TenantId,
    pub book_id: Uuid,
    pub as_of: String,
}
fn refusal(status: StatusCode, reason: &str, message: &str) -> Response {
    (
        status,
        Json(json!({"decision":"refused","reason":reason,"message":message})),
    )
}
fn unavailable(error: impl std::fmt::Display) -> Response {
    tracing::error!(error=%error,"accounting request refused");
    refusal(
        StatusCode::SERVICE_UNAVAILABLE,
        "unavailable",
        "Financial records are temporarily unavailable; no change was confirmed.",
    )
}
fn error(e: PurchaseError) -> Response {
    match e{
    PurchaseError::Domain(ErpError::Forbidden)=>refusal(StatusCode::FORBIDDEN,"forbidden","Your current financial permissions do not allow this operation."),
    PurchaseError::Domain(ErpError::NotFound)=>refusal(StatusCode::NOT_FOUND,"unknown_resource","The financial record is unavailable in your current company scope."),
    PurchaseError::Domain(_)=>refusal(StatusCode::CONFLICT,"conflict","The reviewed record, account or period changed, or a bookkeeping rule needs review. Refresh the record before trying again."),
    // A conflict whose database guard is catalogued: the same 409 and
    // `reason`, with the specific sentence and its code.
    PurchaseError::Refused(refusal)=>(StatusCode::CONFLICT,Json(crate::erp::refused_document(refusal))),
    other=>unavailable(other),
}
}
async fn begin(state: &AppState, tenant: TenantId) -> Result<TenantTransaction<'_>, Response> {
    let mut tx = TenantTransaction::begin(state.pool(), "archava_accounting", "accounting", tenant)
        .await
        .map_err(unavailable)?;
    {
        let c = tx.checked_executor().await.map_err(unavailable)?;
        for q in [
            "SET LOCAL statement_timeout='10s'",
            "SET LOCAL lock_timeout='3s'",
        ] {
            sqlx::query(q).execute(&mut *c).await.map_err(unavailable)?;
        }
    }
    crate::own::require_erp_module(&mut tx, "archava_accounting")
        .await
        .map_err(|e| e.response())?;
    Ok(tx)
}
pub async fn command(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<Submission>, JsonRejection>,
) -> Response {
    let Json(input) = match body {
        Ok(v) => v,
        Err(e) => {
            let (s, b) = rejection(e);
            return (s, Json(json!(b.0)));
        }
    };
    let session = match authenticate(&state, &headers, true).await {
        Ok(v) => v,
        Err((s, b)) => return (s, Json(json!(b.0))),
    };
    let p = PurchasePrincipal::human(session.principal_id);
    let m = match accounting::parse(&json!({"requestId":input.request_id,"command":input.command}))
    {
        Ok(v) => v,
        Err(_) => {
            return refusal(
                StatusCode::BAD_REQUEST,
                "invalid_submission",
                "Review the financial fields, exact minor-unit amounts and balancing lines.",
            )
        }
    };
    let fingerprint = state.identity().digest(
        "erp-accounting-fingerprint",
        &json!({"domain":"accounting","tenant":input.tenant.as_str(),"principal":p,"mutation":m})
            .to_string(),
    );
    let mut tx = match begin(&state, input.tenant.clone()).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let outcome = async {
        let conn = tx.checked_executor().await.map_err(unavailable)?;
        let a = store::access(conn, input.tenant.as_str(), &p)
            .await
            .map_err(error)?;
        if matches!(m.command, accounting::Command::InvoicePolicySet { .. })
            && !crate::own::module_enabled(conn, &input.tenant, "billing")
                .await
                .map_err(|e| e.response())?
        {
            return Err(refusal(
                StatusCode::CONFLICT,
                "source_module_disabled",
                "Enable Billing before configuring future invoice capture.",
            ));
        }
        if matches!(m.command, accounting::Command::PurchasePolicySet { .. }) {
            for dependency in ["purchasing", "commerce"] {
                if !crate::own::module_enabled(conn, &input.tenant, dependency).await.map_err(|e| e.response())? {
                    return Err(refusal(StatusCode::CONFLICT, "source_module_disabled", "Enable Purchasing and Commerce before configuring future purchase capture."));
                }
            }
        }
        if matches!(m.command, accounting::Command::StockPolicySet { .. }) {
            for dependency in ["commerce","purchasing"] {
                if !crate::own::module_enabled(conn,&input.tenant,dependency).await.map_err(|e|e.response())? {
                    return Err(refusal(StatusCode::CONFLICT,"source_module_disabled","Enable Commerce and Purchasing before configuring original stock cost capture."));
                }
            }
        }
        store::record(conn, input.tenant.as_str(), &p, &a, &fingerprint, &m)
            .await
            .map_err(error)
    }
    .await;
    let r = match outcome {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.rollback().await;
            return e;
        }
    };
    match tx.commit().await {
        Ok(()) => (
            StatusCode::OK,
            Json(
                json!({"decision":if r.replayed{"replayed"}else{"receipt"},"tenant":input.tenant.as_str(),"requestId":r.request_id,"receipt":r}),
            ),
        ),
        Err(e) => commit_error(e),
    }
}
fn commit_error(e: archava_adapters::rls::TenantContextError) -> Response {
    match e {
        archava_adapters::rls::TenantContextError::Database { source, .. } => error(
            archava_adapters::erp::purchase_database_error("accounting", source),
        ),
        other => unavailable(other),
    }
}
pub async fn workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<WorkspaceQuery>, QueryRejection>,
) -> Response {
    let Query(input) = match query {
        Ok(v) => v,
        Err(e) => {
            let (s, b) = query_rejection(e);
            return (s, Json(json!(b.0)));
        }
    };
    let page = store::Page {
        search: &input.search,
        collection: input.collection.as_deref(),
        selected: input.id,
        cursor: input.cursor,
        limit: input.limit,
        as_of: input.as_of.as_deref(),
    };
    if !page.valid() {
        return refusal(
            StatusCode::BAD_REQUEST,
            "invalid_query",
            "Choose a supported financial collection and valid page bounds.",
        );
    }
    let session = match authenticate(&state, &headers, false).await {
        Ok(v) => v,
        Err((s, b)) => return (s, Json(json!(b.0))),
    };
    let p = PurchasePrincipal::human(session.principal_id);
    let mut tx = match begin(&state, input.tenant.clone()).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let outcome = async {
        let conn = tx.checked_executor().await.map_err(unavailable)?;
        let a = store::access(conn, input.tenant.as_str(), &p)
            .await
            .map_err(error)?;
        store::workspace(conn, input.tenant.as_str(), &p, &a, input.company_id, page)
            .await
            .map_err(error)
    }
    .await;
    let v = match outcome {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.rollback().await;
            return e;
        }
    };
    match tx.commit().await {
        Ok(()) => (StatusCode::OK, Json(v)),
        Err(e) => commit_error(e),
    }
}
pub async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<StatusQuery>, QueryRejection>,
) -> Response {
    let Query(input) = match query {
        Ok(v) => v,
        Err(e) => {
            let (s, b) = query_rejection(e);
            return (s, Json(json!(b.0)));
        }
    };
    let session = match authenticate(&state, &headers, false).await {
        Ok(v) => v,
        Err((s, b)) => return (s, Json(json!(b.0))),
    };
    let p = PurchasePrincipal::human(session.principal_id);
    let mut tx = match begin(&state, input.tenant.clone()).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let outcome = async {
        let conn = tx.checked_executor().await.map_err(unavailable)?;
        let a = store::access(conn, input.tenant.as_str(), &p)
            .await
            .map_err(error)?;
        store::status(conn, input.tenant.as_str(), &p, &a, input.request_id)
            .await
            .map_err(error)
    }
    .await;
    let v = match outcome {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.rollback().await;
            return e;
        }
    };
    match tx.commit().await {
        Ok(()) => (StatusCode::OK, Json(v)),
        Err(e) => commit_error(e),
    }
}
pub async fn bank_reconciliation(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<BankReadinessQuery>, QueryRejection>,
) -> Response {
    let Query(input) = match query {
        Ok(value) => value,
        Err(e) => {
            let (s, b) = query_rejection(e);
            return (s, Json(json!(b.0)));
        }
    };
    let session = match authenticate(&state, &headers, false).await {
        Ok(value) => value,
        Err((s, b)) => return (s, Json(json!(b.0))),
    };
    let principal = PurchasePrincipal::human(session.principal_id);
    let mut tx = match begin(&state, input.tenant.clone()).await {
        Ok(value) => value,
        Err(e) => return e,
    };
    let outcome = async {
        let conn = tx.checked_executor().await.map_err(unavailable)?;
        let access = store::access(conn, input.tenant.as_str(), &principal)
            .await
            .map_err(error)?;
        store::bank_readiness(
            conn,
            input.tenant.as_str(),
            &principal,
            &access,
            input.bank_policy_id,
            input.period_id,
        )
        .await
        .map_err(error)
    }
    .await;
    let value = match outcome {
        Ok(value) => value,
        Err(e) => {
            let _ = tx.rollback().await;
            return e;
        }
    };
    match tx.commit().await {
        Ok(()) => (StatusCode::OK, Json(value)),
        Err(e) => commit_error(e),
    }
}

/// Every condition the period's close guards test, as counts, before
/// `period.close` is submitted. Read-only.
pub async fn close_readiness(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<CloseReadinessQuery>, QueryRejection>,
) -> Response {
    let Query(input) = match query {
        Ok(value) => value,
        Err(e) => {
            let (s, b) = query_rejection(e);
            return (s, Json(json!(b.0)));
        }
    };
    let session = match authenticate(&state, &headers, false).await {
        Ok(value) => value,
        Err((s, b)) => return (s, Json(json!(b.0))),
    };
    let principal = PurchasePrincipal::human(session.principal_id);
    let mut tx = match begin(&state, input.tenant.clone()).await {
        Ok(value) => value,
        Err(e) => return e,
    };
    let outcome = async {
        let conn = tx.checked_executor().await.map_err(unavailable)?;
        let access = store::access(conn, input.tenant.as_str(), &principal)
            .await
            .map_err(error)?;
        store::close_readiness(
            conn,
            input.tenant.as_str(),
            &principal,
            &access,
            input.period_id,
        )
        .await
        .map_err(error)
    }
    .await;
    let value = match outcome {
        Ok(value) => value,
        Err(e) => {
            let _ = tx.rollback().await;
            return e;
        }
    };
    match tx.commit().await {
        Ok(()) => (StatusCode::OK, Json(value)),
        Err(e) => commit_error(e),
    }
}

/// Posted stock and purchase sources against the inventory control accounts
/// through a business date. Read-only.
pub async fn inventory_control(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<InventoryControlQuery>, QueryRejection>,
) -> Response {
    let Query(input) = match query {
        Ok(value) => value,
        Err(e) => {
            let (s, b) = query_rejection(e);
            return (s, Json(json!(b.0)));
        }
    };
    if archava_domain::erp::accommodation::parse_calendar_date(&input.as_of).is_none() {
        return refusal(
            StatusCode::BAD_REQUEST,
            "invalid_query",
            "Choose a valid report date.",
        );
    }
    let session = match authenticate(&state, &headers, false).await {
        Ok(value) => value,
        Err((s, b)) => return (s, Json(json!(b.0))),
    };
    let principal = PurchasePrincipal::human(session.principal_id);
    let mut tx = match begin(&state, input.tenant.clone()).await {
        Ok(value) => value,
        Err(e) => return e,
    };
    let outcome = async {
        let conn = tx.checked_executor().await.map_err(unavailable)?;
        let access = store::access(conn, input.tenant.as_str(), &principal)
            .await
            .map_err(error)?;
        let mut value = store::inventory_control(
            conn,
            input.tenant.as_str(),
            &principal,
            &access,
            input.book_id,
            &input.as_of,
        )
        .await
        .map_err(error)?;
        let now: String = sqlx::query_scalar("SELECT clock_timestamp()::text")
            .fetch_one(conn)
            .await
            .map_err(unavailable)?;
        value["observedAt"] = json!(now);
        Ok::<_, Response>(value)
    }
    .await;
    match outcome {
        Ok(value) => match tx.commit().await {
            Ok(()) => (StatusCode::OK, Json(value)),
            Err(e) => commit_error(e),
        },
        Err(e) => {
            let _ = tx.rollback().await;
            e
        }
    }
}

/// Output and input tax of posted sources per tax code version for a period,
/// beside the movement of each tax control account. Read-only.
pub async fn tax_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<ReportQuery>, QueryRejection>,
) -> Response {
    let Query(input) = match query {
        Ok(value) => value,
        Err(e) => {
            let (s, b) = query_rejection(e);
            return (s, Json(json!(b.0)));
        }
    };
    if parse_calendar_date(&input.from).is_none()
        || parse_calendar_date(&input.to).is_none()
        || input.from > input.to
    {
        return refusal(
            StatusCode::BAD_REQUEST,
            "invalid_query",
            "Choose a valid tax reporting date range.",
        );
    }
    let session = match authenticate(&state, &headers, false).await {
        Ok(value) => value,
        Err((s, b)) => return (s, Json(json!(b.0))),
    };
    let principal = PurchasePrincipal::human(session.principal_id);
    let mut tx = match begin(&state, input.tenant.clone()).await {
        Ok(value) => value,
        Err(e) => return e,
    };
    let outcome = async {
        let conn = tx.checked_executor().await.map_err(unavailable)?;
        let access = store::access(conn, input.tenant.as_str(), &principal)
            .await
            .map_err(error)?;
        let mut value = store::tax_report(
            conn,
            input.tenant.as_str(),
            &principal,
            &access,
            input.book_id,
            &input.from,
            &input.to,
        )
        .await
        .map_err(error)?;
        let now: String = sqlx::query_scalar("SELECT clock_timestamp()::text")
            .fetch_one(conn)
            .await
            .map_err(unavailable)?;
        value["observedAt"] = json!(now);
        Ok::<_, Response>(value)
    }
    .await;
    match outcome {
        Ok(value) => match tx.commit().await {
            Ok(()) => (StatusCode::OK, Json(value)),
            Err(e) => commit_error(e),
        },
        Err(e) => {
            let _ = tx.rollback().await;
            e
        }
    }
}

/// The posted taxed sources behind the tax report for a period, one row per
/// source with its document, paged. Same grants and period as the report.
/// Read-only.
pub async fn tax_report_documents(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<TaxDocumentsQuery>, QueryRejection>,
) -> Response {
    let Query(input) = match query {
        Ok(value) => value,
        Err(e) => {
            let (s, b) = query_rejection(e);
            return (s, Json(json!(b.0)));
        }
    };
    let page = store::TaxDocumentsPage {
        from: &input.from,
        to: &input.to,
        cursor: input.cursor,
        limit: input.limit,
    };
    if !page.valid() {
        return refusal(
            StatusCode::BAD_REQUEST,
            "invalid_query",
            "Choose a valid tax reporting date range and a page size from 1 to 100.",
        );
    }
    let session = match authenticate(&state, &headers, false).await {
        Ok(value) => value,
        Err((s, b)) => return (s, Json(json!(b.0))),
    };
    let principal = PurchasePrincipal::human(session.principal_id);
    let mut tx = match begin(&state, input.tenant.clone()).await {
        Ok(value) => value,
        Err(e) => return e,
    };
    let outcome = async {
        let conn = tx.checked_executor().await.map_err(unavailable)?;
        let access = store::access(conn, input.tenant.as_str(), &principal)
            .await
            .map_err(error)?;
        let mut value = store::tax_report_documents(
            conn,
            input.tenant.as_str(),
            &principal,
            &access,
            input.book_id,
            page,
        )
        .await
        .map_err(error)?;
        let now: String = sqlx::query_scalar("SELECT clock_timestamp()::text")
            .fetch_one(conn)
            .await
            .map_err(unavailable)?;
        value["observedAt"] = json!(now);
        Ok::<_, Response>(value)
    }
    .await;
    match outcome {
        Ok(value) => match tx.commit().await {
            Ok(()) => (StatusCode::OK, Json(value)),
            Err(e) => commit_error(e),
        },
        Err(e) => {
            let _ = tx.rollback().await;
            e
        }
    }
}

/// The documents listing of a period as a CSV download: a draft export of
/// company-entered data, not a tax return and not in any authority's import
/// format. Same grants and period as the report; a refusal is the usual JSON.
pub async fn tax_report_documents_csv(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<ReportQuery>, QueryRejection>,
) -> axum::response::Response {
    use axum::http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE};
    use axum::response::IntoResponse;
    let Query(input) = match query {
        Ok(value) => value,
        Err(e) => {
            let (s, b) = query_rejection(e);
            return (s, Json(json!(b.0))).into_response();
        }
    };
    if parse_calendar_date(&input.from).is_none()
        || parse_calendar_date(&input.to).is_none()
        || input.from > input.to
    {
        return refusal(
            StatusCode::BAD_REQUEST,
            "invalid_query",
            "Choose a valid tax reporting date range.",
        )
        .into_response();
    }
    let session = match authenticate(&state, &headers, false).await {
        Ok(value) => value,
        Err((s, b)) => return (s, Json(json!(b.0))).into_response(),
    };
    let principal = PurchasePrincipal::human(session.principal_id);
    let mut tx = match begin(&state, input.tenant.clone()).await {
        Ok(value) => value,
        Err(e) => return e.into_response(),
    };
    let outcome = async {
        let conn = tx.checked_executor().await.map_err(unavailable)?;
        let access = store::access(conn, input.tenant.as_str(), &principal)
            .await
            .map_err(error)?;
        store::tax_report_documents_csv(
            conn,
            input.tenant.as_str(),
            &principal,
            &access,
            input.book_id,
            &input.from,
            &input.to,
        )
        .await
        .map_err(error)
    }
    .await;
    match outcome {
        Ok(export) => match tx.commit().await {
            Ok(()) => (
                StatusCode::OK,
                [
                    (CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
                    (
                        CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{}\"", export.filename),
                    ),
                    (CACHE_CONTROL, "no-store".to_string()),
                ],
                export.body,
            )
                .into_response(),
            Err(e) => commit_error(e).into_response(),
        },
        Err(e) => {
            let _ = tx.rollback().await;
            e.into_response()
        }
    }
}

pub async fn reports(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<ReportQuery>, QueryRejection>,
) -> Response {
    let Query(input) = match query {
        Ok(v) => v,
        Err(e) => {
            let (s, b) = query_rejection(e);
            return (s, Json(json!(b.0)));
        }
    };
    if parse_calendar_date(&input.from).is_none()
        || parse_calendar_date(&input.to).is_none()
        || input.from > input.to
    {
        return refusal(
            StatusCode::BAD_REQUEST,
            "invalid_query",
            "Choose a valid financial reporting date range.",
        );
    }
    let session = match authenticate(&state, &headers, false).await {
        Ok(v) => v,
        Err((s, b)) => return (s, Json(json!(b.0))),
    };
    let p = PurchasePrincipal::human(session.principal_id);
    let mut tx = match begin(&state, input.tenant.clone()).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let outcome = async {
        let conn = tx.checked_executor().await.map_err(unavailable)?;
        let a = store::access(conn, input.tenant.as_str(), &p)
            .await
            .map_err(error)?;
        store::reports(
            conn,
            input.tenant.as_str(),
            &p,
            &a,
            input.book_id,
            &input.from,
            &input.to,
        )
        .await
        .map_err(error)
    }
    .await;
    let mut v = match outcome {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.rollback().await;
            return e;
        }
    };
    match sqlx::query_scalar::<_, String>("SELECT clock_timestamp()::text")
        .fetch_one(match tx.checked_executor().await {
            Ok(c) => c,
            Err(e) => return unavailable(e),
        })
        .await
    {
        Ok(now) => v["observedAt"] = json!(now),
        Err(e) => return unavailable(e),
    };
    match tx.commit().await {
        Ok(()) => (StatusCode::OK, Json(v)),
        Err(e) => commit_error(e),
    }
}

/// Original-document positions through an explicit business cutoff, not the
/// mutable current supplier or invoice balance.
pub async fn subledger_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<SubledgerQuery>, QueryRejection>,
) -> Response {
    let Query(input) = match query {
        Ok(v) => v,
        Err(e) => {
            let (s, b) = query_rejection(e);
            return (s, Json(json!(b.0)));
        }
    };
    let page = store::SubledgerPage {
        as_of: &input.as_of,
        side: input.side,
        cursor: input.cursor,
        limit: usize::try_from(input.limit).unwrap_or(0),
    };
    if !page.valid() {
        return refusal(
            StatusCode::BAD_REQUEST,
            "invalid_query",
            "Choose a valid report date and a page size from 1 to 100.",
        );
    }
    let session = match authenticate(&state, &headers, false).await {
        Ok(v) => v,
        Err((s, b)) => return (s, Json(json!(b.0))),
    };
    let p = PurchasePrincipal::human(session.principal_id);
    let mut tx = match begin(&state, input.tenant.clone()).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let outcome = async {
        let conn = tx.checked_executor().await.map_err(unavailable)?;
        let a = store::access(conn, input.tenant.as_str(), &p)
            .await
            .map_err(error)?;
        let mut value =
            store::subledger_report(conn, input.tenant.as_str(), &p, &a, input.book_id, page)
                .await
                .map_err(error)?;
        let now: String = sqlx::query_scalar("SELECT clock_timestamp()::text")
            .fetch_one(conn)
            .await
            .map_err(unavailable)?;
        value["observedAt"] = json!(now);
        Ok::<_, Response>(value)
    }
    .await;
    match outcome {
        Ok(value) => match tx.commit().await {
            Ok(()) => (StatusCode::OK, Json(value)),
            Err(e) => commit_error(e),
        },
        Err(e) => {
            let _ = tx.rollback().await;
            e
        }
    }
}
