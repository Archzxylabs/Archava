//! Reviewed tabular imports. Full server-bound input, current grants, one
//! transaction for all domain effects and the durable batch receipt.
mod commit;
use crate::{
    business::{refused, Response},
    router::{authenticate, AppState},
    workflows,
};
use archava_core::tenant::TenantId;
use archava_domain::ingestion::{reconcile_schema, RawTableInput, TargetEntityKind};
use axum::{
    extract::{rejection::JsonRejection, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconcileRequest {
    pub tenant: TenantId,
    pub entity_kind: TargetEntityKind,
    pub table: RawTableInput,
    pub custom_overrides: Option<HashMap<String, String>>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitRequest {
    pub tenant: TenantId,
    pub batch_id: Uuid,
    pub plan_hash: String,
    pub confirmed: bool,
}
fn invalid(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"reason":"invalid_import","message":message})),
    )
}
fn money(raw: &str) -> Result<i64, Response> {
    let raw = raw
        .trim()
        .trim_start_matches("Rp")
        .trim_start_matches("IDR")
        .trim();
    let groups = raw.split('.').collect::<Vec<_>>();
    if raw.is_empty()
        || !groups
            .iter()
            .all(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
        || groups.len() > 1 && (groups[0].len() > 3 || groups.iter().skip(1).any(|s| s.len() != 3))
    {
        return Err(invalid(
            "Money must be a whole IDR amount, for example 150000 or Rp 150.000.",
        ));
    }
    groups
        .concat()
        .parse::<i64>()
        .ok()
        .filter(|v| (0..=1_000_000_000).contains(v))
        .ok_or_else(|| invalid("Money is outside the supported range."))
}
async fn reconcile(
    state: &AppState,
    headers: &HeaderMap,
    input: ReconcileRequest,
) -> Result<Value, Response> {
    let session = authenticate(state, headers, true)
        .await
        .map_err(|(s, b)| (s, Json(json!(b.0))))?;
    if input.table.rows.len() > 500
        || input.table.columns.len() > 50
        || input.table.rows.iter().any(|r| {
            r.len() != input.table.columns.len() || r.iter().any(|v| v.chars().count() > 2000)
        })
    {
        return Err(invalid("Import up to 500 complete rows and 50 columns; each cell may contain up to 2000 characters."));
    }
    if input.table.columns.iter().collect::<BTreeSet<_>>().len() != input.table.columns.len() {
        return Err(invalid("Column names must be unique."));
    }
    let mut diff = reconcile_schema(
        &input.table,
        input.entity_kind,
        input.custom_overrides.as_ref(),
    )
    .map_err(|_| invalid("Required columns or records are missing."))?;
    if diff.invalid_rows > 0 {
        return Err(invalid(
            "Fix invalid records before reviewing the import. No rows were written.",
        ));
    }
    if diff
        .resolved_mappings
        .iter()
        .map(|m| &m.target_field)
        .collect::<BTreeSet<_>>()
        .len()
        != diff.resolved_mappings.len()
    {
        return Err(invalid("Each target field may be mapped once."));
    }
    let (module, permission) = match input.entity_kind {
        TargetEntityKind::CrmContacts => ("crm", "contacts.write"),
        TargetEntityKind::CatalogProducts => ("commerce", "catalog.write"),
        TargetEntityKind::InventoryStock => ("commerce", "stock.write"),
    };
    let mut tx = workflows::begin(state, &input.tenant).await?;
    workflows::authorize(
        tx.checked_executor()
            .await
            .map_err(workflows::unavailable)?,
        &input.tenant,
        &session.principal_id,
        module,
        permission,
    )
    .await?;
    if module != "crm" {
        crate::own::require_erp_module(&mut tx, "archava_commerce")
            .await
            .map_err(crate::own::OwnRefusal::response)?;
    }
    if module == "commerce" {
        let context = archava_adapters::erp::commerce_kernel::resolve_commerce_context(
            tx.checked_executor()
                .await
                .map_err(workflows::unavailable)?,
            input.tenant.as_str(),
            &archava_domain::erp::purchasing::PurchasePrincipal {
                kind: archava_domain::erp::purchasing::PrincipalKind::Human,
                id: session.principal_id.clone(),
                on_behalf_of: None,
            },
        )
        .await
        .map_err(workflows::domain_error)?;
        if !context.access.holds(&["catalog.read"])
            || input.entity_kind == TargetEntityKind::InventoryStock
                && (!context.access.holds(&["stock.read"])
                    || context.access.scope != archava_domain::erp::ErpScope::All)
        {
            return Err(refused(StatusCode::FORBIDDEN, "missing_permission"));
        }
    }
    let mut commands = Vec::new();
    let mut records = Vec::new();
    let mut omitted = BTreeSet::new();
    for row in &input.table.rows {
        let mut mapped = serde_json::Map::new();
        for mapping in &diff.resolved_mappings {
            let index = input
                .table
                .columns
                .iter()
                .position(|c| c == &mapping.source_column)
                .ok_or_else(|| invalid("A mapped column is missing."))?;
            mapped.insert(mapping.target_field.clone(), json!(row[index].trim()));
        }
        let text = |key: &str| mapped.get(key).and_then(Value::as_str).unwrap_or_default();
        let command = match input.entity_kind {
            TargetEntityKind::CrmContacts => {
                for key in mapped.keys() {
                    if !matches!(key.as_str(), "name" | "email" | "company") {
                        omitted.insert(key.clone());
                    }
                }
                let command:crate::erp::crm::CrmCommand=serde_json::from_value(json!({"operation":"contact.create","data":{"fullName":text("name"),"email":if text("email").is_empty(){None}else{Some(text("email"))},"organization":if text("company").is_empty(){None}else{Some(text("company"))}}})).map_err(|_|invalid("A contact record is invalid."))?;
                if !command.valid() {
                    return Err(invalid("Check contact names and email addresses."));
                }
                json!({"command":command})
            }
            TargetEntityKind::CatalogProducts => {
                for key in mapped.keys() {
                    if !matches!(key.as_str(), "sku" | "title" | "price_minor") {
                        omitted.insert(key.clone());
                    }
                }
                let command:archava_domain::erp::commerce::CommerceCommand=serde_json::from_value(json!({"operation":"product.create","sku":text("sku").to_ascii_uppercase(),"title":text("title"),"price":money(if text("price_minor").is_empty(){"0"}else{text("price_minor")})?,"unit":"unit","trackStock":true})).map_err(|_|invalid("A product record is invalid."))?;
                command
                    .validate()
                    .map_err(|_| invalid("Check product codes, titles and prices."))?;
                json!({"command":command})
            }
            TargetEntityKind::InventoryStock => {
                for key in mapped.keys() {
                    if !matches!(key.as_str(), "sku" | "quantity" | "unit_cost_minor") {
                        omitted.insert(key.clone());
                    }
                }
                let quantity = text("quantity")
                    .parse::<i64>()
                    .ok()
                    .filter(|v| (1..=1_000_000_000).contains(v))
                    .ok_or_else(|| invalid("Opening stock must be a positive whole quantity."))?;
                if text("unit_cost_minor").is_empty() {
                    return Err(invalid("Opening stock requires a unit_cost_minor column with the acquisition cost in whole IDR."));
                }
                let record:Option<(String,i32,i32)>=sqlx::query_as("SELECT p.id::text,b.quantity,b.revision FROM archava_commerce.products p JOIN archava_commerce.stock_balances b ON b.tenant_id=p.tenant_id AND b.product_id=p.id WHERE p.tenant_id=$1 AND p.sku=$2 AND p.active AND p.track_stock AND (to_jsonb(p)->>'company_id') IS NULL").bind(input.tenant.as_str()).bind(text("sku").to_ascii_uppercase()).fetch_optional(tx.checked_executor().await.map_err(workflows::unavailable)?).await.map_err(workflows::unavailable)?;
                let Some((id, 0, revision)) = record else {
                    return Err(invalid("Opening stock requires an active stock product with a zero balance. Review existing stock through inventory adjustments."));
                };
                json!({"command":{"operation":"stock.adjust","productId":id,"quantity":quantity,"unitCostMinor":money(text("unit_cost_minor"))?,"reason":"Confirmed opening inventory import"},"stockRevision":revision})
            }
        };
        records.push(command["command"].clone());
        commands.push(command);
    }
    let plan = json!({"module":module,"permission":permission,"entity_kind":input.entity_kind,"commands":commands,"omitted_fields":omitted});
    let (id, hash) = workflows::issue(
        tx.checked_executor()
            .await
            .map_err(workflows::unavailable)?,
        &input.tenant,
        &session.principal_id,
        "import",
        &plan,
    )
    .await?;
    tx.commit().await.map_err(workflows::unavailable)?;
    diff.sample_mapped_records = records.iter().take(5).cloned().collect();
    Ok(
        json!({"batch_id":id,"plan_hash":hash,"preview":diff,"records":records,"omitted_fields":omitted,"expires_in_seconds":1800,"money_unit":"whole IDR","status":"review_required"}),
    )
}
pub async fn migration_reconcile_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<ReconcileRequest>, JsonRejection>,
) -> Response {
    // Authentication precedes body errors, including legacy malformed callers.
    if let Err((s, b)) = authenticate(&state, &headers, true).await {
        return (s, Json(json!(b.0)));
    }
    let input = match payload {
        Ok(Json(v)) => v,
        Err(_) => return invalid("The import request is malformed."),
    };
    match reconcile(&state, &headers, input).await {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => e,
    }
}
pub async fn migration_commit_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<CommitRequest>, JsonRejection>,
) -> Response {
    if let Err((s, b)) = authenticate(&state, &headers, true).await {
        return (s, Json(json!(b.0)));
    }
    let input = match payload {
        Ok(Json(v)) => v,
        Err(_) => {
            return invalid(
                "Confirm a server-issued import plan with its tenant, batch_id and plan_hash.",
            )
        }
    };
    match commit::persist(&state, &headers, input).await {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reconcile_request_structure() {
        let req = ReconcileRequest {
            tenant: TenantId::new("acme-retail").unwrap(),
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
