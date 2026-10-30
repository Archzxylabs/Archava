//! Native commerce application boundary, sharing the existing physical stock kernel.
use super::{
    commerce_kernel::{self as kernel, CommerceContext, DraftLine, ProductCreateInput},
    purchase_database_error, PurchaseError, COMMERCE_LEDGER,
};
use archava_domain::erp::{
    commerce::CommerceCommand, purchasing::PurchasePrincipal, stock::CommerceCommandReceipt,
    ErpError, ErpScope,
};
use serde_json::{json, Value};
use sqlx::types::Uuid;
use sqlx::{PgConnection, Row};
fn db(error: sqlx::Error) -> PurchaseError {
    purchase_database_error(COMMERCE_LEDGER, error)
}
fn forbidden() -> PurchaseError {
    ErpError::Forbidden.into()
}
fn conflict() -> PurchaseError {
    ErpError::Conflict.into()
}
fn id(raw: &str) -> Result<Uuid, PurchaseError> {
    Uuid::parse_str(raw).map_err(|_| conflict())
}
fn authorize(context: &CommerceContext, command: &CommerceCommand) -> Result<(), PurchaseError> {
    if !context.access.holds(command.permissions()) {
        return Err(forbidden());
    }
    if matches!(
        command,
        CommerceCommand::StockAdjust {
            unit_cost_minor: Some(_),
            ..
        }
    ) && !context.access.holds(&["valuation.write"])
    {
        return Err(forbidden());
    }
    if matches!(
        command,
        CommerceCommand::ProductCreate { .. }
            | CommerceCommand::ProductSet { .. }
            | CommerceCommand::StockAdjust { .. }
    ) && context.access.scope != ErpScope::All
    {
        return Err(forbidden());
    }
    Ok(())
}
async fn order_access(
    connection: &mut PgConnection,
    tenant: &str,
    context: &CommerceContext,
    order: Uuid,
    revision: Option<i64>,
) -> Result<kernel::OrderRow, PurchaseError> {
    let row = kernel::lock_commerce_order(connection, tenant, order, "commerce").await?;
    if context.access.scope == ErpScope::Assigned && row.owner_id != context.owner_id {
        return Err(forbidden());
    }
    if revision.is_some_and(|r| r != row.revision) {
        return Err(conflict());
    }
    Ok(row)
}
async fn receipt_access(
    connection: &mut PgConnection,
    tenant: &str,
    context: &CommerceContext,
    receipt: &CommerceCommandReceipt,
) -> Result<(), PurchaseError> {
    match receipt.kind.as_str() {
        "order" => {
            if !context.access.holds(&["orders.read"]) {
                return Err(forbidden());
            }
            order_access(connection, tenant, context, id(&receipt.id)?, None).await?;
        }
        "product" => {
            if !context.access.holds(&["catalog.read"]) {
                return Err(forbidden());
            }
        }
        "stock" => {
            if !context.access.holds(&["stock.read"]) || context.access.scope != ErpScope::All {
                return Err(forbidden());
            }
        }
        _ => return Err(conflict()),
    }
    Ok(())
}
pub async fn status(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    context: &CommerceContext,
    request_id: Uuid,
) -> Result<Option<CommerceCommandReceipt>, PurchaseError> {
    kernel::verify_commerce_context(connection, tenant).await?;
    let value:Option<Option<Value>>=sqlx::query_scalar("SELECT receipt FROM archava_commerce.commands WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3")
        .bind(tenant).bind(request_id).bind(&principal.id).fetch_optional(&mut *connection).await.map_err(db)?;
    let Some(Some(raw)) = value else {
        return Ok(None);
    };
    if raw.get("domain").is_some() {
        return Err(forbidden());
    }
    let receipt = CommerceCommandReceipt::parse(&raw)?;
    receipt_access(connection, tenant, context, &receipt).await?;
    Ok(Some(receipt))
}
pub async fn record(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    context: &CommerceContext,
    request_id: Uuid,
    fingerprint: &str,
    command: &CommerceCommand,
) -> Result<CommerceCommandReceipt, PurchaseError> {
    kernel::verify_commerce_context(connection, tenant).await?;
    command.validate().map_err(|_| conflict())?;
    authorize(context, command)?;
    if !kernel::valid_fingerprint(fingerprint) {
        return Err(conflict());
    }
    if let CommerceCommand::ProductCreate {
        sku,
        title,
        unit,
        track_stock,
        price,
    } = command
    {
        let (product, revision) = kernel::record_product_command(
            connection,
            tenant,
            principal,
            context,
            request_id,
            fingerprint,
            ProductCreateInput {
                sku: sku.clone(),
                title: title.clone(),
                unit: unit.clone(),
                track_stock: *track_stock,
                price: *price,
            },
        )
        .await?;
        return Ok(CommerceCommandReceipt {
            kind: "product".into(),
            id: product.to_string(),
            revision,
        });
    }
    if !kernel::claim_commerce_command(connection, tenant, request_id, &principal.id, fingerprint)
        .await?
    {
        let row=sqlx::query("SELECT actor_id,fingerprint,receipt FROM archava_commerce.commands WHERE tenant_id=$1 AND request_id=$2").bind(tenant).bind(request_id).fetch_one(&mut *connection).await.map_err(db)?;
        if row.get::<String, _>("actor_id") != principal.id {
            return Err(forbidden());
        }
        if row.get::<String, _>("fingerprint") != fingerprint {
            return Err(conflict());
        }
        let receipt = CommerceCommandReceipt::parse(&row.get::<Value, _>("receipt"))?;
        receipt_access(connection, tenant, context, &receipt).await?;
        return Ok(receipt);
    }
    let (kind, resource, revision, reason) = match command {
        CommerceCommand::ProductSet {
            id: raw,
            expected_revision,
            price,
            active,
        } => {
            let product = id(raw)?;
            let rows = kernel::lock_commerce_products(connection, tenant, &[product]).await?;
            if rows[0].revision != *expected_revision {
                return Err(conflict());
            }
            let revision:i32=sqlx::query_scalar("UPDATE archava_commerce.products SET price=$1,active=$2,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$3 AND id=$4 RETURNING revision").bind(*price as i32).bind(active).bind(tenant).bind(product).fetch_one(&mut *connection).await.map_err(db)?;
            (
                "product",
                product,
                i64::from(revision),
                Some(format!("Price {price}; active {active}")),
            )
        }
        CommerceCommand::StockAdjust {
            product_id,
            quantity,
            reason,
            unit_cost_minor,
        } => {
            let product = id(product_id)?;
            let rows = kernel::lock_commerce_products(connection, tenant, &[product]).await?;
            if !rows[0].track_stock {
                return Err(conflict());
            }
            sqlx::query("INSERT INTO archava_commerce.movements(tenant_id,request_id,product_id,kind,quantity,reason,unit_cost_minor) VALUES($1,$2,$3,'adjust',$4,$5,$6)").bind(tenant).bind(request_id).bind(product).bind(*quantity as i32).bind(reason).bind(unit_cost_minor).execute(&mut *connection).await.map_err(db)?;
            let revision:i32=sqlx::query_scalar("SELECT revision FROM archava_commerce.stock_balances WHERE tenant_id=$1 AND product_id=$2").bind(tenant).bind(product).fetch_one(&mut *connection).await.map_err(db)?;
            ("stock", product, i64::from(revision), Some(reason.clone()))
        }
        CommerceCommand::OrderCreate {
            customer_name,
            lines,
        } => {
            let inputs = lines
                .iter()
                .map(|l| {
                    Ok(DraftLine {
                        product_id: id(&l.product_id)?,
                        quantity: l.quantity,
                    })
                })
                .collect::<Result<Vec<_>, PurchaseError>>()?;
            let draft = kernel::create_commerce_draft(
                connection,
                tenant,
                &context.owner_id,
                customer_name,
                None,
                &inputs,
            )
            .await?;
            ("order", draft.order_id, draft.revision, None)
        }
        CommerceCommand::OrderReserve {
            id: raw,
            expected_revision,
            hold_seconds,
        } => {
            let order = id(raw)?;
            order_access(connection, tenant, context, order, Some(*expected_revision)).await?;
            let revision = kernel::reserve_commerce_order(
                connection,
                tenant,
                order,
                "commerce",
                *hold_seconds,
                None,
            )
            .await?;
            ("order", order, revision, None)
        }
        CommerceCommand::OrderFulfill {
            id: raw,
            expected_revision,
        } => {
            let order = id(raw)?;
            order_access(connection, tenant, context, order, Some(*expected_revision)).await?;
            let revision =
                kernel::fulfill_commerce_order(connection, tenant, request_id, order, "commerce")
                    .await?;
            ("order", order, revision, None)
        }
        CommerceCommand::OrderCancel {
            id: raw,
            expected_revision,
            reason,
        } => {
            let order = id(raw)?;
            order_access(connection, tenant, context, order, Some(*expected_revision)).await?;
            let revision =
                kernel::cancel_commerce_order(connection, tenant, order, "commerce").await?;
            ("order", order, revision, Some(reason.clone()))
        }
        CommerceCommand::OrderReturn {
            id: raw,
            expected_revision,
            line_id,
            quantity,
            disposition,
            reason,
        } => {
            let order = id(raw)?;
            let found =
                order_access(connection, tenant, context, order, Some(*expected_revision)).await?;
            if found.state != "fulfilled" {
                return Err(conflict());
            }
            let line = id(line_id)?;
            let product:Option<(Uuid,bool)>=sqlx::query_as("SELECT product_id,track_stock FROM archava_commerce.order_lines WHERE tenant_id=$1 AND order_id=$2 AND id=$3").bind(tenant).bind(order).bind(line).fetch_optional(&mut *connection).await.map_err(db)?;
            let (product, tracked) = product.ok_or(PurchaseError::Domain(ErpError::NotFound))?;
            kernel::lock_commerce_products(connection, tenant, &[product]).await?;
            if disposition == "restock" && !tracked {
                return Err(conflict());
            }
            sqlx::query("INSERT INTO archava_commerce.returns(tenant_id,request_id,order_id,line_id,quantity,disposition,reason) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(tenant).bind(request_id).bind(order).bind(line).bind(*quantity as i32).bind(disposition).bind(reason).execute(&mut *connection).await.map_err(db)?;
            if disposition == "restock" {
                sqlx::query("INSERT INTO archava_commerce.movements(tenant_id,request_id,product_id,order_id,kind,quantity,reason) VALUES($1,$2,$3,$4,'return',$5,$6)").bind(tenant).bind(request_id).bind(product).bind(order).bind(*quantity as i32).bind(reason).execute(&mut *connection).await.map_err(db)?;
            }
            let all_returned:bool=sqlx::query_scalar("SELECT bool_and(returned_quantity=quantity) FROM archava_commerce.order_lines WHERE tenant_id=$1 AND order_id=$2").bind(tenant).bind(order).fetch_one(&mut *connection).await.map_err(db)?;
            let revision:i32=sqlx::query_scalar("UPDATE archava_commerce.orders SET state=$1,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$2 AND id=$3 RETURNING revision").bind(if all_returned{"returned"}else{"fulfilled"}).bind(tenant).bind(order).fetch_one(&mut *connection).await.map_err(db)?;
            ("order", order, i64::from(revision), Some(reason.clone()))
        }
        CommerceCommand::ProductCreate { .. } => unreachable!("handled above"),
    };
    kernel::insert_commerce_audit(
        connection,
        tenant,
        request_id,
        &principal.id,
        command.operation(),
        resource,
        revision,
        reason.as_deref(),
    )
    .await?;
    let receipt = CommerceCommandReceipt {
        kind: kind.into(),
        id: resource.to_string(),
        revision,
    };
    sqlx::query(
        "UPDATE archava_commerce.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(json!(&receipt))
    .execute(connection)
    .await
    .map_err(db)?;
    Ok(receipt)
}
/// A bounded current-page snapshot. Hidden stock quantities remain null, never fabricated zeros.
pub async fn workspace(
    connection: &mut PgConnection,
    tenant: &str,
    context: &CommerceContext,
    search: &str,
) -> Result<Value, PurchaseError> {
    kernel::verify_commerce_context(connection, tenant).await?;
    if !context.access.holds(&["catalog.read"]) && !context.access.holds(&["orders.read"]) {
        return Err(forbidden());
    }
    let stock = context.access.scope == ErpScope::All && context.access.holds(&["stock.read"]);
    let mut products = if context.access.holds(&["catalog.read"]) {
        sqlx::query_scalar::<_,Value>("SELECT json_build_object('id',p.id,'sku',p.sku,'title',p.title,'unit',p.unit,'trackStock',p.track_stock,'price',p.price,'currency',p.currency,'active',p.active,'revision',p.revision,'quantity',CASE WHEN $3 THEN s.quantity END,'reserved',CASE WHEN $3 AND p.track_stock THEN coalesce(a.held,0) END,'available',CASE WHEN $3 AND p.track_stock THEN s.quantity-coalesce(a.held,0) END,'stockRevision',CASE WHEN $3 THEN s.revision END) FROM archava_commerce.products p LEFT JOIN archava_commerce.stock_balances s ON s.tenant_id=p.tenant_id AND s.product_id=p.id LEFT JOIN LATERAL (SELECT sum(quantity)::bigint held FROM archava_commerce.allocations WHERE tenant_id=p.tenant_id AND product_id=p.id AND state='held' AND expires_at>clock_timestamp()) a ON true WHERE p.tenant_id=$1 AND p.managed_by IS NULL AND ($2='' OR strpos(lower(p.title||' '||p.sku),lower($2))>0) ORDER BY p.created_at DESC,p.id LIMIT 100")
            .bind(tenant).bind(search).bind(stock).fetch_all(&mut *connection).await.map_err(db)?
    } else {
        vec![]
    };
    let mut orders = if context.access.holds(&["orders.read"]) {
        sqlx::query_scalar::<_,Value>("SELECT json_build_object('id',o.id,'customerName',o.customer_name,'state',o.state,'total',o.total_estimate,'currency',o.currency,'revision',o.revision,'ownerId',o.owner_id,'lines',coalesce((SELECT json_agg(json_build_object('id',l.id,'productId',l.product_id,'title',l.title,'quantity',l.quantity,'price',l.price,'returnedQuantity',l.returned_quantity) ORDER BY l.id) FROM archava_commerce.order_lines l WHERE l.tenant_id=o.tenant_id AND l.order_id=o.id),'[]'::json)) FROM archava_commerce.orders o WHERE o.tenant_id=$1 AND o.managed_by IS NULL AND ($2 OR o.owner_id=$3) AND ($4='' OR strpos(lower(o.customer_name),lower($4))>0) ORDER BY o.created_at DESC,o.id LIMIT 100")
            .bind(tenant).bind(context.access.scope==ErpScope::All).bind(&context.owner_id).bind(search).fetch_all(&mut *connection).await.map_err(db)?
    } else {
        vec![]
    };
    super::inventory_valuation::enrich(connection, tenant, context, &mut products, &mut orders)
        .await?;
    Ok(
        json!({"products":products,"orders":orders,"pageLimit":100,"permissions":context.access.permissions,"scope":context.access.scope}),
    )
}
