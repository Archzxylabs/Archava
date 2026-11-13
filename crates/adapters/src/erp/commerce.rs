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
        } | CommerceCommand::StockAdjustForCompany {
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
            | CommerceCommand::ProductCreateForCompany { .. }
            | CommerceCommand::StockAdjustForCompany { .. }
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
    principal: &PurchasePrincipal,
    receipt: &CommerceCommandReceipt,
) -> Result<(), PurchaseError> {
    let kind = if receipt.kind == "order" {
        super::company_trade::Record::Sale
    } else {
        super::company_trade::Record::Product
    };
    super::company_trade::authorize(connection, tenant, principal, kind, id(&receipt.id)?).await?;
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
    receipt_access(connection, tenant, context, principal, &receipt).await?;
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
    let selected_company = if let Some((company, _)) = command.company_selection() {
        Some(super::company_trade::resolve(connection, tenant, principal, id(company)?).await?)
    } else {
        None
    };
    let target = match command {
        CommerceCommand::ProductSet { id, .. } => {
            Some((super::company_trade::Record::Product, id.as_str()))
        }
        CommerceCommand::StockAdjust { product_id, .. }
        | CommerceCommand::StockAdjustForCompany { product_id, .. } => {
            Some((super::company_trade::Record::Product, product_id.as_str()))
        }
        CommerceCommand::OrderReserve { id, .. }
        | CommerceCommand::OrderFulfill { id, .. }
        | CommerceCommand::OrderCancel { id, .. }
        | CommerceCommand::OrderReturn { id, .. } => {
            Some((super::company_trade::Record::Sale, id.as_str()))
        }
        _ => None,
    };
    let target_company = if let Some((kind, raw)) = target {
        super::company_trade::authorize(connection, tenant, principal, kind, id(raw)?).await?
    } else {
        None
    };
    if matches!(
        command,
        CommerceCommand::StockAdjust { .. } | CommerceCommand::StockAdjustForCompany { .. }
    ) && target_company != selected_company.as_ref().map(|c| c.id)
    {
        return Err(conflict());
    }
    let linked_customer =
        if let CommerceCommand::OrderCreateFromContact { contact_id, .. } = command {
            Some(super::customer::resolve(connection, tenant, principal, id(contact_id)?).await?)
        } else {
            None
        };
    let linked_party = if let CommerceCommand::OrderCreateFromParty { party_id, .. }
    | CommerceCommand::OrderCreateForCompany { party_id, .. } = command
    {
        Some(
            super::parties::resolve_transaction(connection, tenant, principal, id(party_id)?)
                .await?,
        )
    } else {
        None
    };
    if !kernel::valid_fingerprint(fingerprint) {
        return Err(conflict());
    }
    if let CommerceCommand::ProductCreate {
        sku,
        title,
        unit,
        track_stock,
        price,
    }
    | CommerceCommand::ProductCreateForCompany {
        sku,
        title,
        unit,
        track_stock,
        price,
        ..
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
                company: selected_company
                    .clone()
                    .zip(command.company_selection().map(|(_, r)| r)),
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
        receipt_access(connection, tenant, context, principal, &receipt).await?;
        return Ok(receipt);
    }
    if let Some(company) = &selected_company {
        company.validate_new(command.company_selection().ok_or_else(conflict)?.1)?;
    }
    let (kind, resource, revision, reason) = match command {
        CommerceCommand::ProductSet {
            id: raw,
            expected_revision,
            price,
            active,
        } => {
            let product = id(raw)?;
            let rows = kernel::lock_commerce_products_scoped(
                connection,
                tenant,
                &[product],
                "commerce",
                target_company,
            )
            .await?;
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
        }
        | CommerceCommand::StockAdjustForCompany {
            product_id,
            quantity,
            reason,
            unit_cost_minor,
            ..
        } => {
            let product = id(product_id)?;
            let rows = kernel::lock_commerce_products_scoped(
                connection,
                tenant,
                &[product],
                "commerce",
                target_company,
            )
            .await?;
            if !rows[0].track_stock {
                return Err(conflict());
            }
            if let Some(company) = &selected_company {
                sqlx::query("INSERT INTO archava_commerce.movements(tenant_id,request_id,product_id,kind,quantity,reason,unit_cost_minor,company_id,company_revision) VALUES($1,$2,$3,'adjust',$4,$5,$6,$7,$8)").bind(tenant).bind(request_id).bind(product).bind(*quantity as i32).bind(reason).bind(unit_cost_minor).bind(company.id).bind(company.revision).execute(&mut *connection).await.map_err(db)?;
            } else {
                sqlx::query("INSERT INTO archava_commerce.movements(tenant_id,request_id,product_id,kind,quantity,reason,unit_cost_minor) VALUES($1,$2,$3,'adjust',$4,$5,$6)").bind(tenant).bind(request_id).bind(product).bind(*quantity as i32).bind(reason).bind(unit_cost_minor).execute(&mut *connection).await.map_err(db)?;
            }
            let revision:i32=sqlx::query_scalar("SELECT revision FROM archava_commerce.stock_balances WHERE tenant_id=$1 AND product_id=$2").bind(tenant).bind(product).fetch_one(&mut *connection).await.map_err(db)?;
            ("stock", product, i64::from(revision), Some(reason.clone()))
        }
        CommerceCommand::OrderCreate { lines, .. }
        | CommerceCommand::OrderCreateFromContact { lines, .. }
        | CommerceCommand::OrderCreateFromParty { lines, .. }
        | CommerceCommand::OrderCreateForCompany { lines, .. } => {
            let customer = match command {
                CommerceCommand::OrderCreate { customer_name, .. } => kernel::CommerceCustomer {
                    company: None,
                    name: customer_name,
                    contact: None,
                    party: None,
                },
                CommerceCommand::OrderCreateFromContact {
                    contact_id,
                    expected_contact_revision,
                    ..
                } => {
                    let row = linked_customer.as_ref().ok_or_else(conflict)?;
                    if i64::from(row.revision) != *expected_contact_revision {
                        return Err(conflict());
                    }
                    kernel::CommerceCustomer {
                        company: None,
                        name: &row.full_name,
                        contact: Some((id(contact_id)?, row.revision)),
                        party: None,
                    }
                }
                CommerceCommand::OrderCreateFromParty {
                    expected_party_revision,
                    ..
                }
                | CommerceCommand::OrderCreateForCompany {
                    expected_party_revision,
                    ..
                } => {
                    let row = linked_party.as_ref().ok_or_else(conflict)?;
                    if !row.customer || i64::from(row.revision) != *expected_party_revision {
                        return Err(conflict());
                    }
                    kernel::CommerceCustomer {
                        company: selected_company.as_ref(),
                        name: &row.name,
                        contact: None,
                        party: Some((row.id, row.revision)),
                    }
                }
                _ => return Err(conflict()),
            };
            let inputs = lines
                .iter()
                .map(|line| {
                    Ok(DraftLine {
                        product_id: id(&line.product_id)?,
                        quantity: line.quantity,
                    })
                })
                .collect::<Result<Vec<_>, PurchaseError>>()?;
            let draft = kernel::create_commerce_draft_with_customer(
                connection,
                tenant,
                &context.owner_id,
                customer,
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
            kernel::lock_commerce_products_scoped(
                connection,
                tenant,
                &[product],
                "commerce",
                found.company_id,
            )
            .await?;
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
        CommerceCommand::ProductCreate { .. } | CommerceCommand::ProductCreateForCompany { .. } => {
            unreachable!("handled above")
        }
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
    workspace_scoped(connection, tenant, context, search, None).await
}
pub async fn workspace_scoped(
    connection: &mut PgConnection,
    tenant: &str,
    context: &CommerceContext,
    search: &str,
    company: Option<Uuid>,
) -> Result<Value, PurchaseError> {
    workspace_filtered(connection, tenant, context, search, company, None).await
}
/// Exact IDs are resolved before paging, with the same grants, owner and company filters.
pub async fn workspace_filtered(
    connection: &mut PgConnection,
    tenant: &str,
    context: &CommerceContext,
    search: &str,
    company: Option<Uuid>,
    id: Option<Uuid>,
) -> Result<Value, PurchaseError> {
    kernel::verify_commerce_context(connection, tenant).await?;
    if !context.access.holds(&["catalog.read"]) && !context.access.holds(&["orders.read"]) {
        return Err(forbidden());
    }
    // Compatibility fixtures may predate company-owned trade. Current native
    // profiles use typed columns so filtering does not serialize every product
    // before paging. These are fixed SQL fragments; caller values remain bound.
    let company_columns: bool = sqlx::query_scalar("SELECT count(*)=2 FROM pg_attribute WHERE attrelid IN ('archava_commerce.products'::regclass,'archava_commerce.orders'::regclass) AND attname='company_id' AND attnum>0 AND NOT attisdropped")
        .fetch_one(&mut *connection).await.map_err(db)?;
    let stock = context.access.scope == ErpScope::All && context.access.holds(&["stock.read"]);
    let mut products = if context.access.holds(&["catalog.read"]) {
        let statement = "WITH selected_products AS MATERIALIZED (SELECT p.* FROM archava_commerce.products p WHERE p.tenant_id=$1 AND p.managed_by IS NULL AND (to_jsonb(p)->>'company_id')::uuid IS NOT DISTINCT FROM $4::uuid AND ($5::uuid IS NULL OR p.id=$5) AND ($2='' OR strpos(lower(p.title||' '||p.sku),lower($2))>0) ORDER BY p.created_at DESC,p.id LIMIT 100) SELECT json_build_object('id',p.id,'sku',p.sku,'title',p.title,'unit',p.unit,'trackStock',p.track_stock,'price',p.price,'currency',p.currency,'active',p.active,'revision',p.revision,'companyId',to_jsonb(p)->'company_id','companyRevision',to_jsonb(p)->'company_revision','companyCode',to_jsonb(p)->'company_code','companyName',to_jsonb(p)->'company_name','quantity',CASE WHEN $3 THEN s.quantity END,'reserved',CASE WHEN $3 AND p.track_stock THEN coalesce(a.held,0) END,'available',CASE WHEN $3 AND p.track_stock THEN s.quantity-coalesce(a.held,0) END,'stockRevision',CASE WHEN $3 THEN s.revision END) FROM selected_products p LEFT JOIN archava_commerce.stock_balances s ON s.tenant_id=p.tenant_id AND s.product_id=p.id LEFT JOIN LATERAL (SELECT sum(quantity)::bigint held FROM archava_commerce.allocations WHERE $3 AND p.track_stock AND tenant_id=p.tenant_id AND product_id=p.id AND state='held' AND expires_at>clock_timestamp()) a ON true ORDER BY p.created_at DESC,p.id";
        let statement = if company_columns {
            statement.replace("(to_jsonb(p)->>'company_id')::uuid", "p.company_id")
        } else {
            statement.to_owned()
        };
        sqlx::query_scalar::<_, Value>(sqlx::AssertSqlSafe(statement.as_str()))
            .bind(tenant)
            .bind(search)
            .bind(stock)
            .bind(company)
            .bind(id)
            .fetch_all(&mut *connection)
            .await
            .map_err(db)?
    } else {
        vec![]
    };
    let mut orders = if context.access.holds(&["orders.read"]) {
        let statement = "SELECT json_build_object('id',o.id,'customerName',o.customer_name,'companyId',to_jsonb(o)->'company_id','companyRevision',to_jsonb(o)->'company_revision','companyCode',to_jsonb(o)->'company_code','companyName',to_jsonb(o)->'company_name','customerContactId',o.customer_contact_id,'customerContactRevision',o.customer_contact_revision,'customerPartyId',o.customer_party_id,'customerPartyRevision',o.customer_party_revision,'state',o.state,'total',o.total_estimate,'currency',o.currency,'revision',o.revision,'ownerId',o.owner_id,'lines',coalesce((SELECT json_agg(json_build_object('id',l.id,'productId',l.product_id,'title',l.title,'quantity',l.quantity,'price',l.price,'returnedQuantity',l.returned_quantity) ORDER BY l.id) FROM archava_commerce.order_lines l WHERE l.tenant_id=o.tenant_id AND l.order_id=o.id),'[]'::json)) FROM archava_commerce.orders o WHERE o.tenant_id=$1 AND o.managed_by IS NULL AND (to_jsonb(o)->>'company_id')::uuid IS NOT DISTINCT FROM $5::uuid AND ($2 OR o.owner_id=$3) AND ($6::uuid IS NULL OR o.id=$6) AND ($4='' OR strpos(lower(o.customer_name),lower($4))>0) ORDER BY o.created_at DESC,o.id LIMIT 100";
        let statement = if company_columns {
            statement.replace("(to_jsonb(o)->>'company_id')::uuid", "o.company_id")
        } else {
            statement.to_owned()
        };
        sqlx::query_scalar::<_, Value>(sqlx::AssertSqlSafe(statement.as_str()))
            .bind(tenant)
            .bind(context.access.scope == ErpScope::All)
            .bind(&context.owner_id)
            .bind(search)
            .bind(company)
            .bind(id)
            .fetch_all(&mut *connection)
            .await
            .map_err(db)?
    } else {
        vec![]
    };
    super::inventory_valuation::enrich(connection, tenant, context, &mut products, &mut orders)
        .await?;
    Ok(
        json!({"products":products,"orders":orders,"pageLimit":100,"permissions":context.access.permissions,"scope":context.access.scope}),
    )
}
