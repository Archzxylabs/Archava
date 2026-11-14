//! Operational supplier matching. Configured financial capture shares the same transaction.
use super::{
    purchasing::{owned_order, ResolvedPurchaseContext},
    PurchaseError, PURCHASING_LEDGER,
};
use archava_domain::erp::{
    purchasing::{PurchaseCommand, PurchaseMutation},
    purchasing_documents::ClaimLine,
    ErpError,
};
use serde_json::{json, Value};
use sqlx::{PgConnection, Row};
use uuid::Uuid;
fn db(e: sqlx::Error) -> PurchaseError {
    super::purchase_database_error(PURCHASING_LEDGER, e)
}
fn uuid(s: &str) -> Result<Uuid, PurchaseError> {
    Uuid::parse_str(s).map_err(|_| ErpError::Conflict.into())
}
async fn validate_lines(
    c: &mut PgConnection,
    t: &str,
    order: Uuid,
    lines: &[ClaimLine],
) -> Result<(), PurchaseError> {
    let ids: Vec<Uuid> = lines
        .iter()
        .map(|l| uuid(&l.line_id))
        .collect::<Result<_, _>>()?;
    let n:i64=sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.lines WHERE tenant_id=$1 AND order_id=$2 AND id=ANY($3)").bind(t).bind(order).bind(&ids).fetch_one(c).await.map_err(db)?;
    if n != lines.len() as i64 {
        return Err(ErpError::Conflict.into());
    }
    Ok(())
}
pub(super) async fn order_id(
    c: &mut PgConnection,
    t: &str,
    id: Uuid,
) -> Result<Uuid, PurchaseError> {
    sqlx::query_scalar(
        "SELECT order_id FROM archava_purchasing.supplier_documents WHERE tenant_id=$1 AND id=$2",
    )
    .bind(t)
    .bind(id)
    .fetch_optional(c)
    .await
    .map_err(db)?
    .ok_or_else(|| ErpError::NotFound.into())
}
pub(super) async fn execute(
    c: &mut PgConnection,
    t: &str,
    context: &ResolvedPurchaseContext,
    m: &PurchaseMutation,
) -> Result<(Uuid, i64), PurchaseError> {
    let request = uuid(&m.request_id)?;
    let (order, id, expected, po_expected) = match &m.command {
        PurchaseCommand::DocumentCapture { input } => (
            uuid(&input.order_id)?,
            Uuid::new_v4(),
            None,
            Some(input.expected_order_revision),
        ),
        PurchaseCommand::DocumentRevise { input } => {
            let id = uuid(&input.id)?;
            (
                order_id(c, t, id).await?,
                id,
                Some(input.expected_revision),
                Some(input.expected_order_revision),
            )
        }
        PurchaseCommand::DocumentMatch { input } => {
            let id = uuid(&input.id)?;
            (
                order_id(c, t, id).await?,
                id,
                Some(input.expected_revision),
                Some(input.expected_order_revision),
            )
        }
        PurchaseCommand::DocumentVoid { input } => {
            let id = uuid(&input.id)?;
            (
                order_id(c, t, id).await?,
                id,
                Some(input.expected_revision),
                None,
            )
        }
        _ => return Err(ErpError::Conflict.into()),
    };
    // Every operation locks source PO first, then document. Receipts and competing matches share this lock.
    let source = owned_order(c, t, &context.purchasing, order, true).await?;
    if source.confirmed_at.is_none() || po_expected.is_some_and(|v| v != source.revision) {
        return Err(ErpError::Conflict.into());
    }
    let company: Option<Uuid> = sqlx::query_scalar(
        "SELECT company_id FROM archava_purchasing.orders WHERE tenant_id=$1 AND id=$2",
    )
    .bind(t)
    .bind(order)
    .fetch_one(&mut *c)
    .await
    .map_err(db)?;
    if company.is_none() {
        return Err(ErpError::Conflict.into());
    }
    let old = if let Some(rev) = expected {
        let row=sqlx::query("SELECT revision,state,claim_lines,declared_total FROM archava_purchasing.supplier_documents WHERE tenant_id=$1 AND id=$2 FOR UPDATE").bind(t).bind(id).fetch_one(&mut *c).await.map_err(db)?;
        if i64::from(row.try_get::<i32, _>("revision").map_err(db)?) != rev {
            return Err(ErpError::Conflict.into());
        }
        Some(row)
    } else {
        None
    };
    let next = expected.map_or(1, |r| r + 1) as i32;
    match &m.command {
        PurchaseCommand::DocumentCapture { input } => {
            if uuid(&input.supplier_id)? != source.supplier_id || input.currency != source.currency
            {
                return Err(ErpError::Conflict.into());
            }
            validate_lines(c, t, order, &input.lines).await?;
            sqlx::query("INSERT INTO archava_purchasing.supplier_documents(tenant_id,id,order_id,owner_id,supplier_id,supplier_code,supplier_name,company_id,company_revision,company_code,company_name,vendor_reference,document_date,currency,declared_total,claim_lines,state,observed_order_revision) SELECT tenant_id,$3,id,owner_id,supplier_id,supplier_code,supplier_name,company_id,company_revision,company_code,company_name,$4,$5::date,currency,$6,$7,'captured',revision FROM archava_purchasing.orders WHERE tenant_id=$1 AND id=$2")
  .bind(t).bind(order).bind(id).bind(&input.vendor_reference).bind(&input.document_date).bind(input.declared_total).bind(json!(input.lines)).execute(&mut *c).await.map_err(db)?;
        }
        PurchaseCommand::DocumentRevise { input } => {
            let old = old.as_ref().ok_or(ErpError::Conflict)?;
            let state: String = old.try_get("state").map_err(db)?;
            if state == "matched" || input.currency != source.currency {
                return Err(ErpError::Conflict.into());
            }
            validate_lines(c, t, order, &input.lines).await?;
            sqlx::query("UPDATE archava_purchasing.supplier_documents SET document_date=$3::date,declared_total=$4,claim_lines=$5,state='captured',match_evidence='{}'::jsonb,observed_order_revision=$6,revision=$7,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2").bind(t).bind(id).bind(&input.document_date).bind(input.declared_total).bind(json!(input.lines)).bind(source.revision as i32).bind(next).execute(&mut *c).await.map_err(db)?;
        }
        PurchaseCommand::DocumentVoid { .. } => {
            let state: String = old
                .as_ref()
                .ok_or(ErpError::Conflict)?
                .try_get("state")
                .map_err(db)?;
            if state == "void" {
                return Err(ErpError::Conflict.into());
            }
            sqlx::query("UPDATE archava_purchasing.supplier_documents SET state='void',revision=$3,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2").bind(t).bind(id).bind(next).execute(&mut *c).await.map_err(db)?;
            sqlx::query("UPDATE archava_purchasing.supplier_document_allocations SET released_revision=$3 WHERE tenant_id=$1 AND document_id=$2 AND released_revision IS NULL").bind(t).bind(id).bind(next).execute(&mut *c).await.map_err(db)?;
        }
        PurchaseCommand::DocumentMatch { .. } => {
            let old = old.as_ref().ok_or(ErpError::Conflict)?;
            let state: String = old.try_get("state").map_err(db)?;
            if !matches!(state.as_str(), "captured" | "held") {
                return Err(ErpError::Conflict.into());
            }
            let claims: Vec<ClaimLine> =
                serde_json::from_value(old.try_get("claim_lines").map_err(db)?)
                    .map_err(|_| ErpError::Conflict)?;
            let declared: i64 = old.try_get("declared_total").map_err(db)?;
            let mut reasons = Vec::<Value>::new();
            let mut observed = Vec::<Value>::new();
            let mut planned = Vec::<(Uuid, Uuid, i32)>::new();
            let mut total = 0i64;
            for claim in &claims {
                let line = uuid(&claim.line_id)?;
                let row=sqlx::query("SELECT l.quantity,l.unit_cost,l.accepted_quantity,l.rejected_quantity,coalesce((SELECT sum(a.quantity) FROM archava_purchasing.supplier_document_allocations a WHERE a.tenant_id=l.tenant_id AND a.order_id=l.order_id AND a.line_id=l.id AND a.released_revision IS NULL),0)::bigint AS allocated FROM archava_purchasing.lines l WHERE l.tenant_id=$1 AND l.order_id=$2 AND l.id=$3").bind(t).bind(order).bind(line).fetch_one(&mut *c).await.map_err(db)?;
                let price: i32 = row.try_get("unit_cost").map_err(db)?;
                let accepted: i32 = row.try_get("accepted_quantity").map_err(db)?;
                let used: i64 = row.try_get("allocated").map_err(db)?;
                let available = i64::from(accepted) - used;
                total = total
                    .checked_add(
                        claim
                            .quantity
                            .checked_mul(claim.unit_cost)
                            .ok_or(ErpError::Conflict)?,
                    )
                    .ok_or(ErpError::Conflict)?;
                if claim.unit_cost != i64::from(price) {
                    reasons.push(json!({"code":"unit_price_mismatch","lineId":claim.line_id,"claimed":claim.unit_cost,"ordered":price}))
                }
                if claim.quantity > available {
                    reasons.push(json!({"code":"insufficient_unallocated_accepted_goods","lineId":claim.line_id,"claimed":claim.quantity,"available":available}))
                }
                observed.push(json!({"lineId":claim.line_id,"claimedQuantity":claim.quantity,"claimedUnitCost":claim.unit_cost,"orderedQuantity":row.try_get::<i32,_>("quantity").map_err(db)?,"orderedUnitCost":price,"accepted":accepted,"rejected":row.try_get::<i32,_>("rejected_quantity").map_err(db)?,"alreadyAllocated":used,"available":available}));
                if claim.quantity <= available {
                    let receipts=sqlx::query("SELECT r.request_id,r.accepted-coalesce((SELECT sum(a.quantity) FROM archava_purchasing.supplier_document_allocations a WHERE a.tenant_id=r.tenant_id AND a.receipt_request_id=r.request_id AND a.line_id=r.line_id AND a.released_revision IS NULL),0)::bigint AS available FROM archava_purchasing.receipt_lines r JOIN archava_purchasing.receipts h ON h.tenant_id=r.tenant_id AND h.request_id=r.request_id WHERE r.tenant_id=$1 AND r.order_id=$2 AND r.line_id=$3 AND r.accepted>0 AND r.accepted>coalesce((SELECT sum(a.quantity) FROM archava_purchasing.supplier_document_allocations a WHERE a.tenant_id=r.tenant_id AND a.receipt_request_id=r.request_id AND a.line_id=r.line_id AND a.released_revision IS NULL),0) ORDER BY h.occurred_at,h.request_id LIMIT 501").bind(t).bind(order).bind(line).fetch_all(&mut *c).await.map_err(db)?;
                    let mut remaining = claim.quantity;
                    for r in receipts {
                        if remaining == 0 || planned.len() >= 500 {
                            break;
                        }
                        let n = remaining.min(r.try_get::<i64, _>("available").map_err(db)?);
                        if n > 0 {
                            planned.push((r.try_get("request_id").map_err(db)?, line, n as i32));
                            remaining -= n
                        }
                    }
                    if remaining > 0 {
                        reasons.push(json!({"code":"receipt_allocation_limit","lineId":claim.line_id,"maximumAllocations":500}))
                    }
                }
            }
            if total != declared {
                reasons.push(
                    json!({"code":"declared_total_mismatch","claimed":declared,"computed":total}),
                )
            }
            let matched = reasons.is_empty();
            let evidence = json!({"policy":"exact-goods-v1","orderId":order,"orderRevision":source.revision,"declaredTotal":declared,"computedTotal":total,"lines":observed,"reasons":reasons});
            sqlx::query("UPDATE archava_purchasing.supplier_documents SET state=$3,match_evidence=$4,observed_order_revision=$5,revision=$6,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2").bind(t).bind(id).bind(if matched{"matched"}else{"held"}).bind(evidence).bind(source.revision as i32).bind(next).execute(&mut *c).await.map_err(db)?;
            if matched {
                for (receipt, line, quantity) in planned {
                    sqlx::query("INSERT INTO archava_purchasing.supplier_document_allocations(tenant_id,document_id,document_revision,order_id,receipt_request_id,line_id,quantity) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(t).bind(id).bind(next).bind(order).bind(receipt).bind(line).bind(quantity).execute(&mut *c).await.map_err(db)?;
                }
            }
        }
        _ => return Err(ErpError::Conflict.into()),
    }
    sqlx::query("INSERT INTO archava_purchasing.supplier_document_versions(tenant_id,document_id,revision,request_id,snapshot) SELECT tenant_id,id,revision,$3,to_jsonb(d) FROM archava_purchasing.supplier_documents d WHERE tenant_id=$1 AND id=$2").bind(t).bind(id).bind(request).execute(&mut *c).await.map_err(db)?;
    Ok((id, i64::from(next)))
}

pub(super) async fn workspace(
    c: &mut PgConnection,
    t: &str,
    context: &ResolvedPurchaseContext,
    search: &str,
    company: Option<Uuid>,
    id: Option<Uuid>,
) -> Result<Vec<Value>, PurchaseError> {
    if company.is_none()
        || !context
            .purchasing
            .holds(&["documents.read", "orders.read", "receipts.read"])
    {
        return Ok(vec![]);
    }
    let rows=sqlx::query_scalar("SELECT jsonb_build_object('id',d.id,'orderId',d.order_id,'supplierId',d.supplier_id,'supplierName',d.supplier_name,'companyId',d.company_id,'companyRevision',d.company_revision,'companyCode',d.company_code,'companyName',d.company_name,'vendorReference',d.vendor_reference,'documentDate',to_char(d.document_date,'YYYY-MM-DD'),'currency',d.currency,'declaredTotal',d.declared_total,'lines',d.claim_lines,'state',d.state,'revision',d.revision,'observedOrderRevision',d.observed_order_revision,'matchEvidence',d.match_evidence,'accountingStatus','not_inferred','sourceOrder',(SELECT jsonb_build_object('id',o.id,'supplierId',o.supplier_id,'revision',o.revision,'lines',(SELECT jsonb_agg(jsonb_build_object('id',l.id,'title',l.title,'unitCost',l.unit_cost,'acceptedQuantity',l.accepted_quantity,'rejectedQuantity',l.rejected_quantity) ORDER BY l.id) FROM archava_purchasing.lines l WHERE l.tenant_id=o.tenant_id AND l.order_id=o.id)) FROM archava_purchasing.orders o WHERE o.tenant_id=d.tenant_id AND o.id=d.order_id),'versions',coalesce((SELECT jsonb_agg(x.doc ORDER BY x.revision DESC) FROM (SELECT v.revision,jsonb_build_object('revision',v.revision,'requestId',v.request_id,'state',v.snapshot->'state','matchEvidence',v.snapshot->'match_evidence') AS doc FROM archava_purchasing.supplier_document_versions v WHERE v.tenant_id=d.tenant_id AND v.document_id=d.id ORDER BY v.revision DESC LIMIT 20) x),'[]'::jsonb),'historyLimit',20,'activeAllocatedQuantity',(SELECT coalesce(sum(a.quantity),0) FROM archava_purchasing.supplier_document_allocations a WHERE a.tenant_id=d.tenant_id AND a.document_id=d.id AND a.released_revision IS NULL)) FROM archava_purchasing.supplier_documents d WHERE d.tenant_id=$1 AND d.company_id=$2 AND ($3 OR d.owner_id=$4) AND ($6::uuid IS NULL OR d.id=$6) AND ($5='' OR strpos(lower(d.vendor_reference||' '||d.supplier_name),lower($5))>0) ORDER BY d.created_at DESC,d.id LIMIT 100").bind(t).bind(company).bind(context.purchasing.scope.covers_all()).bind(&context.purchasing.owner_id).bind(search).bind(id).fetch_all(c).await.map_err(db)?;
    Ok(rows)
}
