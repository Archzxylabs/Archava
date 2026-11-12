//! Invoice creation from explicit priced lines or a frozen native sales order.
//! Called only inside the billing command's tenant transaction and receipt claim.
use super::{
    billing::{conflict, forbidden, refuses},
    commerce_kernel::{lock_commerce_order, resolve_commerce_context},
    PurchaseError,
};
use archava_domain::erp::{
    billing::{BillingAccess, BillingCommand, BillingReceipt, PricedLine},
    purchasing::PurchasePrincipal,
    ErpScope,
};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

/// Recheck source visibility before claiming OR replaying the conversion.
/// The row lock serializes conversion with returns and competing invoices.
pub(super) async fn authorize_order(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    order: &str,
) -> Result<(), PurchaseError> {
    let context = resolve_commerce_context(connection, tenant, principal).await?;
    if !context.access.holds(&["orders.read"]) {
        return Err(forbidden());
    }
    let id = Uuid::parse_str(order).map_err(|_| conflict())?;
    let row = lock_commerce_order(connection, tenant, id, "commerce").await?;
    if context.access.scope == ErpScope::Assigned && row.owner_id != context.owner_id {
        return Err(forbidden());
    }
    super::company_trade::authorize(
        connection,
        tenant,
        principal,
        super::company_trade::Record::Sale,
        id,
    )
    .await?;
    Ok(())
}

struct Draft {
    code: String,
    customer: String,
    currency: String,
    consumer: String,
    lines: Vec<(Option<Uuid>, PricedLine)>,
    source: Option<(Uuid, i32)>,
    customer_contact: Option<(Uuid, i32)>,
    customer_party: Option<(Uuid, i32)>,
    company: Option<(Uuid, i32, String, String)>,
}

async fn draft(
    connection: &mut PgConnection,
    tenant: &str,
    command: &BillingCommand,
) -> Result<Draft, PurchaseError> {
    match command {
        BillingCommand::CreateInvoice {
            invoice_code,
            customer_ref,
            currency,
            consumer_ref,
            lines,
            ..
        } => Ok(Draft {
            code: invoice_code.clone(),
            customer: customer_ref.clone(),
            currency: currency.clone(),
            consumer: consumer_ref.clone(),
            lines: lines.iter().cloned().map(|line| (None, line)).collect(),
            source: None,
            customer_contact: None,
            customer_party: None,
            company: None,
        }),
        BillingCommand::CreateInvoiceFromOrder {
            invoice_code,
            order_id,
            expected_order_revision,
            ..
        } => {
            // authorize_order already holds the source lock in this transaction.
            let order_id = Uuid::parse_str(order_id).map_err(|_| conflict())?;
            let order = lock_commerce_order(connection, tenant, order_id, "commerce").await?;
            if order.state != "fulfilled" || order.revision != *expected_order_revision {
                return Err(conflict());
            }
            let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM archava_billing.invoices WHERE tenant_id=$1 AND source_order_id=$2)")
                .bind(tenant).bind(order_id).fetch_one(&mut *connection).await.map_err(refuses)?;
            if exists {
                return Err(conflict());
            }
            let rows = sqlx::query("SELECT id,title,quantity,price,returned_quantity FROM archava_commerce.order_lines WHERE tenant_id=$1 AND order_id=$2 ORDER BY id")
                .bind(tenant).bind(order_id).fetch_all(&mut *connection).await.map_err(refuses)?;
            if rows.is_empty() || rows.len() > 200 {
                return Err(conflict());
            }
            let mut lines = Vec::with_capacity(rows.len());
            for row in rows {
                if row
                    .try_get::<i32, _>("returned_quantity")
                    .map_err(refuses)?
                    != 0
                {
                    return Err(conflict());
                }
                lines.push((
                    Some(row.try_get("id").map_err(refuses)?),
                    PricedLine {
                        description: row.try_get("title").map_err(refuses)?,
                        quantity: i64::from(row.try_get::<i32, _>("quantity").map_err(refuses)?),
                        unit_minor: i64::from(row.try_get::<i32, _>("price").map_err(refuses)?),
                    },
                ));
            }
            let contact: (Option<Uuid>, Option<i32>) = sqlx::query_as("SELECT customer_contact_id,customer_contact_revision FROM archava_commerce.orders WHERE tenant_id=$1 AND id=$2")
                .bind(tenant).bind(order_id).fetch_one(&mut *connection).await.map_err(refuses)?;
            let customer_contact = match contact {
                (None, None) => None,
                (Some(id), Some(revision)) => Some((id, revision)),
                _ => return Err(conflict()),
            };
            let party:(Option<Uuid>,Option<i32>)=sqlx::query_as("SELECT customer_party_id,customer_party_revision FROM archava_commerce.orders WHERE tenant_id=$1 AND id=$2").bind(tenant).bind(order_id).fetch_one(&mut *connection).await.map_err(refuses)?;
            let customer_party = match party {
                (None, None) => None,
                (Some(id), Some(revision)) => Some((id, revision)),
                _ => return Err(conflict()),
            };
            let total = total(&lines)?;
            if total != order.total_estimate {
                return Err(conflict());
            }
            let company: Option<(Uuid, i32, String, String)> = if order.company_id.is_some() {
                Some(sqlx::query_as("SELECT company_id,company_revision,company_code,company_name FROM archava_commerce.orders WHERE tenant_id=$1 AND id=$2").bind(tenant).bind(order_id).fetch_one(&mut *connection).await.map_err(refuses)?)
            } else {
                None
            };
            Ok(Draft {
                code: invoice_code.clone(),
                customer: order.customer_name,
                currency: "IDR".into(),
                consumer: format!("commerce:{order_id}"),
                lines,
                customer_contact,
                customer_party,
                company,
                source: Some((
                    order_id,
                    i32::try_from(order.revision).map_err(|_| conflict())?,
                )),
            })
        }
        _ => Err(conflict()),
    }
}

fn total(lines: &[(Option<Uuid>, PricedLine)]) -> Result<i64, PurchaseError> {
    lines.iter().try_fold(0i64, |sum, (_, line)| {
        let amount = line
            .quantity
            .checked_mul(line.unit_minor)
            .ok_or_else(conflict)?;
        sum.checked_add(amount).ok_or_else(conflict)
    })
}

/// Both creation paths share the same invoice/line writes and receipt envelope.
pub(super) async fn create(
    connection: &mut PgConnection,
    tenant: &str,
    access: &BillingAccess,
    request: Uuid,
    command: &BillingCommand,
    now: &str,
) -> Result<BillingReceipt, PurchaseError> {
    let draft = draft(connection, tenant, command).await?;
    let gross = total(&draft.lines)?;
    let invoice = Uuid::new_v4();
    let statement = if draft.company.is_some() {
        "INSERT INTO archava_billing.invoices(tenant_id,id,invoice_code,customer_ref,currency,status,gross_minor,outstanding_minor,consumer_ref,owner_id,created_at,source_order_id,source_order_revision,customer_contact_id,customer_contact_revision,customer_party_id,customer_party_revision,company_id,company_revision,company_code,company_name) VALUES($1,$2,$3,$4,$5,'draft',$6,$6,$7,$8,$9::timestamptz,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)"
    } else {
        "INSERT INTO archava_billing.invoices(tenant_id,id,invoice_code,customer_ref,currency,status,gross_minor,outstanding_minor,consumer_ref,owner_id,created_at,source_order_id,source_order_revision,customer_contact_id,customer_contact_revision,customer_party_id,customer_party_revision) VALUES($1,$2,$3,$4,$5,'draft',$6,$6,$7,$8,$9::timestamptz,$10,$11,$12,$13,$14,$15)"
    };
    let query = sqlx::query(statement)
        .bind(tenant)
        .bind(invoice)
        .bind(&draft.code)
        .bind(&draft.customer)
        .bind(&draft.currency)
        .bind(gross)
        .bind(&draft.consumer)
        .bind(&access.owner_id)
        .bind(now)
        .bind(draft.source.map(|s| s.0))
        .bind(draft.source.map(|s| s.1))
        .bind(draft.customer_contact.map(|c| c.0))
        .bind(draft.customer_contact.map(|c| c.1))
        .bind(draft.customer_party.map(|c| c.0))
        .bind(draft.customer_party.map(|c| c.1));
    let query = if let Some((id, revision, code, name)) = &draft.company {
        query.bind(id).bind(revision).bind(code).bind(name)
    } else {
        query
    };
    query.execute(&mut *connection).await.map_err(refuses)?;
    let mut first = None;
    for (source, line) in draft.lines {
        let id = Uuid::new_v4();
        first.get_or_insert(id);
        let quantity = i32::try_from(line.quantity).map_err(|_| conflict())?;
        let amount = line
            .quantity
            .checked_mul(line.unit_minor)
            .ok_or_else(conflict)?;
        sqlx::query("INSERT INTO archava_billing.invoice_lines(tenant_id,id,invoice_id,description,quantity,unit_minor,line_total_minor,recorded_by,recorded_at,source_order_line_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9::timestamptz,$10)")
            .bind(tenant).bind(id).bind(invoice).bind(line.description).bind(quantity)
            .bind(line.unit_minor).bind(amount).bind(&access.owner_id).bind(now).bind(source)
            .execute(&mut *connection).await.map_err(refuses)?;
    }
    Ok(BillingReceipt {
        command_id: request.to_string(),
        tenant_id: tenant.into(),
        action: command.mutation_type(),
        invoice_id: Some(invoice.to_string()),
        line_id: first.map(|id| id.to_string()),
        correction_id: None,
        attempt_id: None,
        event_id: None,
        status: "draft".into(),
        outstanding_minor: Some(gross),
        revision: Some(1),
        idempotency_replay: false,
        executed_at: now.into(),
    })
}
