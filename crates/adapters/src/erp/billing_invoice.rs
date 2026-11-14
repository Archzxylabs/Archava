//! Invoice creation from explicit priced lines or a frozen native sales order.
//! Called only inside the billing command's tenant transaction and receipt claim.
use super::{
    billing::{conflict, forbidden, refused, refuses},
    commerce_kernel::{lock_commerce_order, resolve_commerce_context},
    PurchaseError,
};
use archava_domain::erp::{
    billing::{BillingAccess, BillingCommand, BillingReceipt, PricedLine},
    purchasing::PurchasePrincipal,
    tax::{DocumentTax, Fraction, TaxRule},
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
    tax: Option<TaxSnapshot>,
}

/// What an invoice was taxed with (TAX.md rule 6): the tax code version that
/// applies on the invoice date and the amounts computed from it. Stored with
/// the invoice; the guards of Billing 009 recompute and compare all of it.
struct TaxSnapshot {
    code_id: Uuid,
    code: String,
    version: i32,
    rate: Fraction,
    base: Fraction,
    price_includes_tax: bool,
    amounts: DocumentTax,
}

/// The order's tax code and price basis, read through jsonb so an install
/// whose Commerce ledger predates them reads an untaxed order.
async fn order_tax(
    connection: &mut PgConnection,
    tenant: &str,
    order: Uuid,
) -> Result<Option<(String, bool)>, PurchaseError> {
    let stated: (Option<String>, Option<bool>) = sqlx::query_as("SELECT to_jsonb(o)->>'tax_code',(to_jsonb(o)->>'price_includes_tax')::boolean FROM archava_commerce.orders o WHERE tenant_id=$1 AND id=$2")
        .bind(tenant).bind(order).fetch_one(connection).await.map_err(refuses)?;
    match stated {
        (None, None) => Ok(None),
        (Some(code), Some(inclusive)) => Ok(Some((code, inclusive))),
        _ => Err(conflict()),
    }
}

/// Taxes an order total with the version of the order's tax code that applies
/// on the invoice date: the database's date of the invoice's `created_at`
/// (`now`, the command's one clock sample), the only date an invoice has. The
/// version row is share-locked so it cannot be retired under this transaction.
/// No applicable version refuses the invoice; it is never created untaxed.
async fn tax_snapshot(
    connection: &mut PgConnection,
    tenant: &str,
    company: Uuid,
    code: String,
    price_includes_tax: bool,
    total: i64,
    now: &str,
) -> Result<TaxSnapshot, PurchaseError> {
    let version: Option<(Uuid, i32, i32, i32, i32, i32)> = sqlx::query_as(
        "SELECT t.id,t.version,t.rate_numerator,t.rate_denominator,t.base_numerator,t.base_denominator \
         FROM archava_accounting.tax_codes t JOIN archava_accounting.books b ON b.tenant_id=t.tenant_id AND b.id=t.book_id \
         WHERE t.tenant_id=$1 AND b.company_id=$2 AND t.code=$3 AND t.kind='output' AND t.state IN ('confirmed','retired') \
         AND $4::timestamptz::date BETWEEN t.effective_from AND coalesce(t.effective_to,DATE 'infinity') FOR SHARE OF t",
    )
    .bind(tenant)
    .bind(company)
    .bind(&code)
    .bind(now)
    .fetch_optional(connection)
    .await
    .map_err(refuses)?;
    let Some((code_id, version, rate_n, rate_d, base_n, base_d)) = version else {
        return Err(refused("invoice_tax_code_not_applicable"));
    };
    let rate = Fraction {
        numerator: rate_n,
        denominator: rate_d,
    };
    let base = Fraction {
        numerator: base_n,
        denominator: base_d,
    };
    let amounts = TaxRule::new(rate, base)
        .and_then(|rule| rule.document(total, price_includes_tax))
        .map_err(|_| conflict())?;
    Ok(TaxSnapshot {
        code_id,
        code,
        version,
        rate,
        base,
        price_includes_tax,
        amounts,
    })
}

/// The tax id of the invoice's managed company and of its customer party, as
/// the two rows state them now, each share-locked so neither changes under
/// this transaction. Either may be absent: a company or an organization
/// without a tax id, a party sourced from a contact, or a customer that is
/// not a business party. Read through jsonb so an install whose Organizations
/// ledger predates the party tax id reads "none", as the guard of Billing 011
/// does.
async fn tax_identifiers(
    connection: &mut PgConnection,
    tenant: &str,
    company: Uuid,
    party: Option<Uuid>,
) -> Result<(Option<String>, Option<String>), PurchaseError> {
    let seller: Option<String> = sqlx::query_scalar("SELECT to_jsonb(c)->>'tax_id' FROM archava_organizations.companies c WHERE c.tenant_id=$1 AND c.id=$2 FOR SHARE OF c")
        .bind(tenant).bind(company).fetch_optional(&mut *connection).await.map_err(refuses)?.flatten();
    let buyer: Option<String> = match party {
        Some(party) => sqlx::query_scalar("SELECT to_jsonb(p)->>'tax_id' FROM archava_organizations.parties p WHERE p.tenant_id=$1 AND p.id=$2 FOR SHARE OF p")
            .bind(tenant).bind(party).fetch_optional(&mut *connection).await.map_err(refuses)?.flatten(),
        None => None,
    };
    Ok((seller, buyer))
}

async fn draft(
    connection: &mut PgConnection,
    tenant: &str,
    command: &BillingCommand,
    now: &str,
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
            // A manual invoice has no company and carries no tax.
            tax: None,
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
            if order.revision != *expected_order_revision {
                return Err(conflict());
            }
            // The same rules as the provenance guard (Billing 005), decided
            // here so the refusal can say which one.
            if order.state != "fulfilled" {
                return Err(refused("invoice_order_not_fulfilled"));
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
                    return Err(refused("invoice_order_has_returns"));
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
            // The order names the tax code and the price basis; the rate is
            // the one that applies now, on the invoice date.
            let tax = match (order_tax(connection, tenant, order_id).await?, &company) {
                (None, _) => None,
                (Some((code, inclusive)), Some((company_id, ..))) => Some(
                    tax_snapshot(connection, tenant, *company_id, code, inclusive, total, now)
                        .await?,
                ),
                (Some(_), None) => return Err(conflict()),
            };
            Ok(Draft {
                code: invoice_code.clone(),
                customer: order.customer_name,
                // The invoice is in its order's currency; the provenance
                // guard (Billing 005) refuses any other.
                currency: order.currency,
                consumer: format!("commerce:{order_id}"),
                lines,
                customer_contact,
                customer_party,
                company,
                tax,
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
    let draft = draft(connection, tenant, command, now).await?;
    // The amount the customer owes: the priced lines, plus the tax when the
    // order's prices exclude it. An untaxed invoice is its lines, as before.
    let gross = match &draft.tax {
        Some(tax) => tax.amounts.gross,
        None => total(&draft.lines)?,
    };
    // A taxed invoice keeps the tax ids of its company and its customer party
    // as they are now (Billing 011). Stored as entered; nothing is verified.
    let tax_ids = match (&draft.tax, &draft.company) {
        (Some(_), Some((company, ..))) => Some(
            tax_identifiers(
                connection,
                tenant,
                *company,
                draft.customer_party.map(|party| party.0),
            )
            .await?,
        ),
        _ => None,
    };
    let invoice = Uuid::new_v4();
    let statement = if draft.tax.is_some() {
        "INSERT INTO archava_billing.invoices(tenant_id,id,invoice_code,customer_ref,currency,status,gross_minor,outstanding_minor,consumer_ref,owner_id,created_at,source_order_id,source_order_revision,customer_contact_id,customer_contact_revision,customer_party_id,customer_party_revision,company_id,company_revision,company_code,company_name,tax_minor,tax_code_id,tax_code,tax_code_version,tax_kind,tax_rate_numerator,tax_rate_denominator,tax_base_numerator,tax_base_denominator,price_includes_tax,seller_tax_id,buyer_tax_id) VALUES($1,$2,$3,$4,$5,'draft',$6,$6,$7,$8,$9::timestamptz,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,'output',$24,$25,$26,$27,$28,$29,$30)"
    } else if draft.company.is_some() {
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
    let query = if let Some(tax) = &draft.tax {
        query
            .bind(tax.amounts.tax)
            .bind(tax.code_id)
            .bind(&tax.code)
            .bind(tax.version)
            .bind(tax.rate.numerator)
            .bind(tax.rate.denominator)
            .bind(tax.base.numerator)
            .bind(tax.base.denominator)
            .bind(tax.price_includes_tax)
            .bind(tax_ids.as_ref().and_then(|ids| ids.0.clone()))
            .bind(tax_ids.as_ref().and_then(|ids| ids.1.clone()))
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
