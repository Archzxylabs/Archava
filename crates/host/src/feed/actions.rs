//! Only real low-stock observations with an available supplier offer produce
//! actionable cards. A card drafts a PO; purchase confirmation stays explicit.
use super::*;
use crate::{
    business::{refused, Response},
    workflows,
};
use archava_adapters::erp::purchasing::resolve_purchasing_context;
use archava_domain::erp::{
    purchasing::{PrincipalKind, PurchasePrincipal},
    ErpScope,
};
use uuid::Uuid;
pub(super) async fn cards(
    state: &AppState,
    headers: &HeaderMap,
    input: FeedQueryParams,
) -> Result<Value, Response> {
    let session = crate::router::authenticate(state, headers, false)
        .await
        .map_err(|(s, b)| (s, Json(json!(b.0))))?;
    let commerce = crate::erp::commerce::workspace(
        State(state.clone()),
        headers.clone(),
        Ok(Query(crate::erp::commerce::WorkspaceQuery {
            tenant: input.tenant.clone(),
            search: String::new(),
            company_id: None,
            id: None,
        })),
    )
    .await;
    if commerce.0 != StatusCode::OK {
        return Err(commerce);
    }
    let purchasing = crate::erp::workspace::purchasing(
        State(state.clone()),
        headers.clone(),
        Ok(Query(crate::erp::workspace::WorkspaceQuery {
            tenant: input.tenant.clone(),
            search: String::new(),
            company_id: None,
            id: None,
        })),
    )
    .await;
    if purchasing.0 != StatusCode::OK {
        return Err(purchasing);
    }
    let mut tx = workflows::begin(state, &input.tenant).await?;
    workflows::authorize(
        tx.checked_executor()
            .await
            .map_err(workflows::unavailable)?,
        &input.tenant,
        &session.principal_id,
        "purchasing",
        "orders.read",
    )
    .await?;
    let all_orders = purchasing.1 .0["scope"] == "all";
    let mut cards = Vec::new();
    for product in commerce.1 .0["products"].as_array().into_iter().flatten() {
        let Some(quantity) = product["quantity"].as_i64() else {
            continue;
        };
        if quantity > 5 || product["active"] != true || product["trackStock"] != true {
            continue;
        }
        let product_id = product["id"].as_str().unwrap_or_default();
        let open:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM archava_purchasing.orders o JOIN archava_purchasing.lines l ON l.tenant_id=o.tenant_id AND l.order_id=o.id WHERE o.tenant_id=$1 AND l.product_id=$2::uuid AND o.state IN ('draft','confirmed','partial') AND ($3 OR o.owner_id=$4))").bind(input.tenant.as_str()).bind(product_id).bind(all_orders).bind(&session.principal_id).fetch_one(tx.checked_executor().await.map_err(workflows::unavailable)?).await.map_err(workflows::unavailable)?;
        if open {
            continue;
        }
        let offer = purchasing.1 .0["offers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|o| o["productId"] == product["id"] && o["active"] == true)
            .filter(|o| {
                purchasing.1 .0["suppliers"]
                    .as_array()
                    .is_some_and(|suppliers| {
                        suppliers
                            .iter()
                            .any(|s| s["id"] == o["supplierId"] && s["active"] == true)
                    })
            })
            .min_by_key(|o| o["unitCost"].as_i64().unwrap_or(i64::MAX));
        let Some(offer) = offer else {
            continue;
        };
        let replenish = 10 - quantity;
        let plan = json!({"module":"purchasing","permission":"orders.write","tool":"purchasing.order.create","values":{"supplierId":offer["supplierId"],"lines":[{"offerId":offer["id"],"quantity":replenish}]},"productId":product["id"],"stockRevision":product["stockRevision"],"observedQuantity":quantity,"title":format!("Replenish {}",product["title"].as_str().unwrap_or("stock")),"supplierName":offer["supplierName"],"quantity":replenish,"estimatedUnitCost":offer["unitCost"]});
        let (id, hash) = workflows::issue(
            tx.checked_executor()
                .await
                .map_err(workflows::unavailable)?,
            &input.tenant,
            &session.principal_id,
            "feed",
            &plan,
        )
        .await?;
        cards.push(json!({"id":id,"plan_hash":hash,"action_type":"generate_reorder_po","title":plan["title"],"supplierName":plan["supplierName"],"quantity":replenish,"observedQuantity":quantity,"estimatedUnitCost":plan["estimatedUnitCost"],"currency":"IDR","detail":"Creates a draft using current supplier prices. Review the draft before confirming a purchase.","requires_confirmation":true}));
        if cards.len() >= 20 {
            break;
        }
    }
    tx.commit().await.map_err(workflows::unavailable)?;
    Ok(
        json!({"feed":{"tenant":input.tenant,"cards":cards,"source":"tenant_database","bounded":true},"available_actions":["generate_reorder_po"],"unavailable_actions":["dispatch_ar_reminders","print_shift_summary","blast_patient_confirmations","issue_milestone_invoice"]}),
    )
}
pub(super) async fn execute(
    state: &AppState,
    headers: &HeaderMap,
    id: Uuid,
    input: ExecuteActionRequest,
) -> Result<Value, Response> {
    if !input.confirmed {
        return Err(refused(StatusCode::BAD_REQUEST, "confirmation_required"));
    }
    let session = crate::router::authenticate(state, headers, true)
        .await
        .map_err(|(s, b)| (s, Json(json!(b.0))))?;
    let mut tx = workflows::begin(state, &input.tenant).await?;
    let held = workflows::load(
        tx.checked_executor()
            .await
            .map_err(workflows::unavailable)?,
        &input.tenant,
        &session.principal_id,
        "feed",
        id,
        Some(&input.plan_hash),
    )
    .await?;
    let product = held.plan["productId"]
        .as_str()
        .ok_or_else(|| workflows::unavailable("invalid stored feed product"))?
        .to_string();
    tx.commit().await.map_err(workflows::unavailable)?;
    // This transaction holds only the product advisory lock. Domain grants and
    // owner configuration locks are released before entering another handler.
    let mut serial = state.pool().begin().await.map_err(workflows::unavailable)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(format!("archava-feed:{}:{product}", input.tenant.as_str()))
        .execute(&mut *serial)
        .await
        .map_err(workflows::unavailable)?;
    let mut tx = workflows::begin(state, &input.tenant).await?;
    let held = workflows::load(
        tx.checked_executor()
            .await
            .map_err(workflows::unavailable)?,
        &input.tenant,
        &session.principal_id,
        "feed",
        id,
        Some(&input.plan_hash),
    )
    .await?;
    workflows::authorize(
        tx.checked_executor()
            .await
            .map_err(workflows::unavailable)?,
        &input.tenant,
        &session.principal_id,
        "purchasing",
        "orders.write",
    )
    .await?;
    crate::own::require_erp_module(&mut tx, "archava_purchasing")
        .await
        .map_err(crate::own::OwnRefusal::response)?;
    if let Some(receipt) = held.receipt {
        tx.commit().await.map_err(workflows::unavailable)?;
        return Ok(receipt);
    }
    let product = held.plan["productId"]
        .as_str()
        .ok_or_else(|| workflows::unavailable("invalid stored feed product"))?;
    let conn = tx
        .checked_executor()
        .await
        .map_err(workflows::unavailable)?;
    let access = resolve_purchasing_context(
        &mut *conn,
        input.tenant.as_str(),
        &PurchasePrincipal {
            kind: PrincipalKind::Human,
            id: session.principal_id.clone(),
            on_behalf_of: None,
        },
    )
    .await
    .map_err(|error| match error {
        archava_adapters::erp::PurchaseError::Domain(_)
        | archava_adapters::erp::PurchaseError::Refused(_) => {
            refused(StatusCode::FORBIDDEN, "purchasing_access_refused")
        }
        other => workflows::unavailable(other),
    })?;
    let committed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM archava_purchasing.commands WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3 AND receipt IS NOT NULL)").bind(input.tenant.as_str()).bind(id).bind(&session.principal_id).fetch_one(&mut *conn).await.map_err(workflows::unavailable)?;
    if !committed {
        let changed:bool=sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM archava_commerce.stock_balances WHERE tenant_id=$1 AND product_id=$2::uuid AND revision=$3 AND quantity=$4) OR EXISTS(SELECT 1 FROM archava_purchasing.orders o JOIN archava_purchasing.lines l ON l.tenant_id=o.tenant_id AND l.order_id=o.id WHERE o.tenant_id=$1 AND l.product_id=$2::uuid AND o.state IN ('draft','confirmed','partial') AND ($5 OR o.owner_id=$6))").bind(input.tenant.as_str()).bind(product).bind(held.plan["stockRevision"].as_i64().unwrap_or(-1) as i32).bind(held.plan["observedQuantity"].as_i64().unwrap_or(-1) as i32).bind(access.purchasing.scope == ErpScope::All).bind(&access.purchasing.owner_id).fetch_one(conn).await.map_err(workflows::unavailable)?;
        if changed {
            return Err(refused(StatusCode::CONFLICT, "feed_observation_changed"));
        }
    }
    tx.commit().await.map_err(workflows::unavailable)?;
    // The domain's stable key survives a crash between business commit and this
    // receipt append. A retry recovers that same domain receipt.
    let response = crate::business::execute(
        state,
        headers,
        input.tenant.as_str(),
        id,
        held.plan["tool"].as_str().unwrap_or_default(),
        &held.plan["values"],
    )
    .await;
    if response.0 != StatusCode::OK {
        return Err(response);
    }
    let mut tx = workflows::begin(state, &input.tenant).await?;
    let current = workflows::load(
        tx.checked_executor()
            .await
            .map_err(workflows::unavailable)?,
        &input.tenant,
        &session.principal_id,
        "feed",
        id,
        Some(&input.plan_hash),
    )
    .await?;
    if let Some(receipt) = current.receipt {
        tx.commit().await.map_err(workflows::unavailable)?;
        serial.commit().await.map_err(workflows::unavailable)?;
        return Ok(receipt);
    }
    let receipt = json!({"card_id":id,"plan_hash":held.hash,"status":"committed","action_type":"generate_reorder_po","receipt":response.1.0});
    workflows::record(
        tx.checked_executor()
            .await
            .map_err(workflows::unavailable)?,
        &input.tenant,
        id,
        &receipt,
    )
    .await?;
    tx.commit().await.map_err(workflows::unavailable)?;
    serial.commit().await.map_err(workflows::unavailable)?;
    Ok(receipt)
}
