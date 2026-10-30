//! Migration & Tabular Data Ingestion Route Handlers (`archava-host::ingestion`).
//!
//! Provides HTTP endpoints for:
//! 1. `POST /api/migration/reconcile` — takes raw tabular CSV/Excel data and automatically maps
//!    it to standard Rust schema models with diff cards and warnings.
//! 2. `POST /api/migration/commit` — commits the reconciled diff into the tenant ledger.

use archava_domain::ingestion::{
    execute_batch_commit, reconcile_schema, DiffPreview, RawTableInput, TargetEntityKind,
};
use axum::extract::Json;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

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
    Json(payload): Json<ReconcileRequest>,
) -> (StatusCode, Json<Value>) {
    match reconcile_schema(
        &payload.table,
        payload.entity_kind,
        payload.custom_overrides.as_ref(),
    ) {
        Ok(diff) => (StatusCode::OK, Json(json!(diff))),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "reconciliation_failed",
                "message": err.to_string(),
            })),
        ),
    }
}

/// POST /api/migration/commit
pub async fn migration_commit_route(
    Json(payload): Json<CommitRequest>,
) -> (StatusCode, Json<Value>) {
    match execute_batch_commit(payload.batch_id, &payload.preview) {
        Ok(result) => (StatusCode::OK, Json(json!(result))),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "commit_failed",
                "message": err.to_string(),
            })),
        ),
    }
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

    #[tokio::test]
    async fn migration_reconcile_route_success() {
        let req = ReconcileRequest {
            entity_kind: TargetEntityKind::CatalogProducts,
            table: RawTableInput {
                columns: vec!["Kode_Brg".into(), "Nama_Barang".into(), "Harga_Jual".into()],
                rows: vec![vec!["SKU-99".into(), "Glow Serum".into(), "150000".into()]],
            },
            custom_overrides: None,
        };

        let (status, Json(res)) = migration_reconcile_route(Json(req)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(res["total_rows"], 1);
        assert_eq!(res["valid_rows"], 1);
    }
}
