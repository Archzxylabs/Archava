//! Bulk valuation projections over the same tenant transaction as the visible worklist.
use super::{
    commerce_kernel::CommerceContext, purchase_database_error, PurchaseError, COMMERCE_LEDGER,
};
use archava_domain::erp::{ErpError, ErpScope};
use serde_json::{json, Value};
use sqlx::types::Uuid;
use sqlx::{PgConnection, Row};
fn db(error: sqlx::Error) -> PurchaseError {
    purchase_database_error(COMMERCE_LEDGER, error)
}
fn ids(rows: &[Value]) -> Result<Vec<Uuid>, PurchaseError> {
    rows.iter()
        .map(|r| {
            r["id"]
                .as_str()
                .and_then(|id| Uuid::parse_str(id).ok())
                .ok_or_else(|| ErpError::Conflict.into())
        })
        .collect()
}
/// Aggregated money travels as exact decimal strings, including beyond JS's integer ceiling.
pub async fn enrich(
    connection: &mut PgConnection,
    tenant: &str,
    context: &CommerceContext,
    products: &mut [Value],
    orders: &mut [Value],
) -> Result<(), PurchaseError> {
    if !context.access.holds(&["valuation.read"]) {
        return Ok(());
    }
    if context.access.scope == ErpScope::All && context.access.holds(&["stock.read"]) {
        let product_ids = ids(products)?;
        let rows=sqlx::query("SELECT product_id,CASE WHEN count(*) FILTER(WHERE total_cost_minor IS NULL)>0 THEN NULL ELSE coalesce(sum(archava_commerce.cost_portion(total_cost_minor,original_quantity,original_quantity-remaining_quantity,remaining_quantity)),0)::text END AS value,coalesce(sum(remaining_quantity) FILTER(WHERE total_cost_minor IS NULL),0)::bigint AS unknown_quantity FROM archava_commerce.inventory_layers WHERE tenant_id=$1 AND product_id=ANY($2) AND remaining_quantity>0 GROUP BY product_id").bind(tenant).bind(product_ids).fetch_all(&mut *connection).await.map_err(db)?;
        let mut values = std::collections::HashMap::new();
        for row in rows {
            values.insert(
                row.try_get::<Uuid, _>("product_id")
                    .map_err(db)?
                    .to_string(),
                (
                    row.try_get::<Option<String>, _>("value").map_err(db)?,
                    row.try_get::<i64, _>("unknown_quantity").map_err(db)?,
                ),
            );
        }
        for product in products {
            if product["trackStock"] != true {
                continue;
            }
            let id = product["id"].as_str().unwrap_or_default();
            let (value, unknown) = values.remove(id).unwrap_or((Some("0".into()), 0));
            product["inventoryValueMinor"] = json!(value);
            product["unknownCostQuantity"] = json!(unknown);
        }
    }
    let order_ids = ids(orders)?;
    let rows=sqlx::query("WITH expected AS (SELECT order_id,sum(quantity)::bigint quantity FROM archava_commerce.order_lines WHERE tenant_id=$1 AND order_id=ANY($2) AND track_stock GROUP BY order_id),used AS (SELECT order_id,sum(quantity)::bigint quantity,count(*) FILTER(WHERE total_cost_minor IS NULL) unknown,sum(total_cost_minor)::text cost FROM archava_commerce.inventory_uses WHERE tenant_id=$1 AND order_id=ANY($2) GROUP BY order_id),restocked AS (SELECT l.order_id,count(*) FILTER(WHERE r.total_cost_minor IS NULL) unknown,sum(r.total_cost_minor)::text cost FROM archava_commerce.inventory_return_costs r JOIN archava_commerce.order_lines l ON l.tenant_id=r.tenant_id AND l.id=r.line_id WHERE r.tenant_id=$1 AND l.order_id=ANY($2) AND r.restock GROUP BY l.order_id) SELECT o.id,CASE WHEN coalesce(e.quantity,0)<>coalesce(u.quantity,0) OR coalesce(u.unknown,0)>0 THEN NULL ELSE coalesce(u.cost,'0') END AS fulfillment_cost,CASE WHEN coalesce(r.unknown,0)>0 THEN NULL ELSE coalesce(r.cost,'0') END AS restocked_cost FROM archava_commerce.orders o LEFT JOIN expected e ON e.order_id=o.id LEFT JOIN used u ON u.order_id=o.id LEFT JOIN restocked r ON r.order_id=o.id WHERE o.tenant_id=$1 AND o.id=ANY($2) AND o.state IN('fulfilled','returned')").bind(tenant).bind(order_ids).fetch_all(connection).await.map_err(db)?;
    let mut values = std::collections::HashMap::new();
    for row in rows {
        values.insert(
            row.try_get::<Uuid, _>("id").map_err(db)?.to_string(),
            (
                row.try_get::<Option<String>, _>("fulfillment_cost")
                    .map_err(db)?,
                row.try_get::<Option<String>, _>("restocked_cost")
                    .map_err(db)?,
            ),
        );
    }
    for order in orders {
        if let Some((cost, restocked)) = values.remove(order["id"].as_str().unwrap_or_default()) {
            order["fulfillmentCostMinor"] = json!(cost);
            order["restockedCostMinor"] = json!(restocked);
        }
    }
    Ok(())
}
