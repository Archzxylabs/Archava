//! Migration and tabular-data ingestion route handlers.
//!
//! Provides HTTP endpoints for:
//! 1. `POST /api/migration/reconcile` — takes raw tabular CSV/Excel data and maps
//!    it to standard schema models with diff cards and warnings.
//! 2. `POST /api/migration/commit` — reserved for committing a reconciled diff.
//!
//! Both routes were anonymous, and the commit route answered
//! `status: "committed_atomic"` with `committed_records_count` taken from the
//! caller's own preview. Nothing was written: `archava_domain::ingestion` has no
//! persistence, and the handler opened no transaction. The audit's finding was
//! that the response asserted a durable outcome that did not exist, so the
//! commit route now answers 501 until a real tenant-scoped writer lands behind
//! it. Both routes run the session and CSRF gate first, and both refusals are
//! fixed messages rather than the domain error text, which described internals.

use archava_domain::ingestion::{reconcile_schema, DiffPreview, RawTableInput, TargetEntityKind};
use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::router::{require_session, AppState};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconcileRequest {
    pub entity_kind: TargetEntityKind,
    pub table: RawTableInput,
    pub custom_overrides: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommitRequest {
    pub batch_id: String,
    pub preview: DiffPreview,
}

/// POST /api/migration/reconcile
pub async fn migration_reconcile_route(
    state: State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<ReconcileRequest>, JsonRejection>,
) -> (StatusCode, Json<Value>) {
    if let Err((status, reason)) = require_session(&state, &headers, true).await {
        return (status, Json(json!({ "error": reason })));
    }
    let payload = match payload {
        Ok(Json(payload)) => payload,
        Err(rejection) => {
            return (
                rejection.status(),
                Json(json!({ "error": "malformed_submission" })),
            )
        }
    };
    match reconcile_schema(
        &payload.table,
        payload.entity_kind,
        payload.custom_overrides.as_ref(),
    ) {
        Ok(diff) => (StatusCode::OK, Json(json!(diff))),
        Err(error) => {
            // The domain error names column mappings and internal invariants;
            // it is logged, not returned.
            tracing::warn!(error = %error, "refused a reconciliation at the boundary");
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "reconciliation_failed" })),
            )
        }
    }
}

/// POST /api/migration/commit
///
/// Reserved, and deliberately not a success. There is no tenant-scoped commit
/// writer in this host, so the only truthful answers are "unauthorized" or
/// "not implemented" — never a fabricated `committed_atomic` receipt.
pub async fn migration_commit_route(
    state: State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<CommitRequest>, JsonRejection>,
) -> (StatusCode, Json<Value>) {
    if let Err((status, reason)) = require_session(&state, &headers, true).await {
        return (status, Json(json!({ "error": reason })));
    }
    if let Err(rejection) = payload {
        return (
            rejection.status(),
            Json(json!({ "error": "malformed_submission" })),
        );
    }
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "migration_commit_not_implemented",
            "detail": "No tenant-scoped commit writer is mounted on this host.",
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reconcile_request_structure() {
        let req = ReconcileRequest {
            entity_kind: TargetEntityKind::CatalogProducts,
            table: RawTableInput {
                columns: vec!["Kode_Brg".into(), "Nama_Barang".into()],
                rows: vec![vec!["SKU-01".into(), "Serum".into()]],
            },
            custom_overrides: None,
        };

        let serialized = serde_json::to_string(&req).expect("serializes");
        assert!(serialized.contains("catalog_products"));
    }

    #[test]
    fn the_reconciler_maps_a_catalog_sheet_without_persisting_anything() {
        let input = RawTableInput {
            columns: vec!["Kode_Brg".into(), "Nama_Barang".into(), "Harga_Jual".into()],
            rows: vec![vec!["SKU-99".into(), "Glow Serum".into(), "150000".into()]],
        };

        let diff = reconcile_schema(&input, TargetEntityKind::CatalogProducts, None)
            .expect("reconciliation passes on a well-formed sheet");
        assert_eq!(diff.total_rows, 1);
        assert_eq!(diff.valid_rows, 1);
    }
}
