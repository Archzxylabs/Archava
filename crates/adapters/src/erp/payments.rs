//! Durable gateway identity, external-money observations and invoice allocation.
//! Call inside a checked tenant transaction. HTTP authentication belongs to the
//! host; current membership, scope and the frozen binding are checked again here.
use super::{
    billing::{record_billing_command, resolve_billing_context},
    PurchaseError,
};
use archava_domain::erp::{
    billing::{BillingAccess, BillingCommand, BillingMutation, SettlementOutcome},
    payments::{allocation, delta, PaymentLink, PaymentObservation, PaymentState},
    purchasing::{PrincipalKind, PurchasePrincipal},
    ErpError, ErpScope,
};
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgRow, PgConnection, Row};
use uuid::Uuid;

fn domain(error: ErpError) -> PurchaseError {
    PurchaseError::Domain(error)
}
fn db(source: sqlx::Error) -> PurchaseError {
    PurchaseError::Database {
        ledger: "billing",
        source,
    }
}
fn conflict() -> PurchaseError {
    domain(ErpError::Conflict)
}
fn forbidden() -> PurchaseError {
    domain(ErpError::Forbidden)
}
fn missing() -> PurchaseError {
    domain(ErpError::NotFound)
}
fn uuid(value: &str) -> Result<Uuid, PurchaseError> {
    Uuid::parse_str(value).map_err(|_| conflict())
}

fn link(row: &PgRow) -> Result<PaymentLink, PurchaseError> {
    let get = |key: &str| row.try_get::<i64, _>(key).map_err(db);
    let received = get("received_minor")?;
    let refunded = get("refunded_minor")?;
    let allocated = get("allocated_minor")?;
    let allocation_refunded = get("allocation_refunded_minor")?;
    Ok(PaymentLink {
        id: row.try_get::<Uuid, _>("id").map_err(db)?.to_string(),
        invoice_id: row
            .try_get::<Uuid, _>("invoice_id")
            .map_err(db)?
            .to_string(),
        integration_id: row.try_get("integration_id").map_err(db)?,
        order_id: row.try_get("order_id").map_err(db)?,
        amount_minor: get("amount_minor")?,
        state: row.try_get("state").map_err(db)?,
        redirect_url: row.try_get("redirect_url").map_err(db)?,
        received_minor: received,
        refunded_minor: refunded,
        allocated_minor: allocated,
        allocation_refunded_minor: allocation_refunded,
        unapplied_minor: (received - refunded - (allocated - allocation_refunded)).max(0),
        requires_review: row.try_get("requires_review").map_err(db)?,
    })
}

async fn invoice_lock(
    connection: &mut PgConnection,
    tenant: &str,
    id: Uuid,
    access: &BillingAccess,
) -> Result<PgRow, PurchaseError> {
    sqlx::query("SELECT * FROM archava_billing.invoices WHERE tenant_id=$1 AND id=$2 AND ($3 OR owner_id=$4) FOR UPDATE")
        .bind(tenant).bind(id).bind(access.scope==ErpScope::All).bind(&access.owner_id)
        .fetch_optional(connection).await.map_err(db)?.ok_or_else(missing)
}

pub struct LinkRequest<'a> {
    pub request_id: Uuid,
    pub invoice_id: Uuid,
    pub integration_id: &'a str,
    pub service_id: &'a str,
}

pub async fn command_status(
    connection: &mut PgConnection,
    tenant: &str,
    access: &BillingAccess,
    request: Uuid,
) -> Result<Option<PaymentLink>, PurchaseError> {
    if !access.holds("billing.read") {
        return Err(forbidden());
    }
    let row=sqlx::query("SELECT l.* FROM archava_billing.gateway_links l JOIN archava_billing.invoices i ON i.tenant_id=l.tenant_id AND i.id=l.invoice_id WHERE l.tenant_id=$1 AND l.request_id=$2 AND l.actor_id=$3 AND ($4 OR i.owner_id=$3)")
        .bind(tenant).bind(request).bind(&access.owner_id).bind(access.scope==ErpScope::All).fetch_optional(connection).await.map_err(db)?;
    row.as_ref().map(link).transpose()
}

/// `true` means this transaction created the sole invocation claim. A replay
/// never invokes Snap again, including after an unknown/lost creation response.
pub async fn request_link(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &BillingAccess,
    request: LinkRequest<'_>,
) -> Result<(PaymentLink, bool), PurchaseError> {
    if principal.kind != PrincipalKind::Human
        || !access.holds("billing.write")
        || !access.holds("billing.read")
    {
        return Err(forbidden());
    }
    let service = PurchasePrincipal {
        kind: PrincipalKind::Service,
        id: request.service_id.into(),
        on_behalf_of: None,
    };
    let service_access = resolve_billing_context(connection, tenant, &service).await?;
    require_service(&service, &service_access)?;
    let invoice = invoice_lock(connection, tenant, request.invoice_id, access).await?;
    let old=sqlx::query("SELECT * FROM archava_billing.gateway_links WHERE tenant_id=$1 AND request_id=$2 FOR UPDATE")
        .bind(tenant).bind(request.request_id).fetch_optional(&mut *connection).await.map_err(db)?;
    if let Some(old) = old {
        if old.try_get::<String, _>("actor_id").map_err(db)? != principal.id
            || old.try_get::<Uuid, _>("invoice_id").map_err(db)? != request.invoice_id
            || old.try_get::<String, _>("integration_id").map_err(db)? != request.integration_id
            || old.try_get::<String, _>("service_id").map_err(db)? != request.service_id
        {
            return Err(conflict());
        }
        return Ok((link(&old)?, false));
    }
    let amount: i64 = invoice.try_get("outstanding_minor").map_err(db)?;
    let held: bool =
        sqlx::query_scalar("SELECT held FROM archava_billing.gateway_runtime WHERE singleton")
            .fetch_one(&mut *connection)
            .await
            .map_err(db)?;
    if held {
        return Err(domain(ErpError::Unavailable));
    }
    if invoice.try_get::<String, _>("status").map_err(db)? != "issued"
        || invoice.try_get::<String, _>("currency").map_err(db)? != "IDR"
        || !(1..=archava_domain::erp::billing::SAFE_INTEGER_MAX).contains(&amount)
    {
        return Err(conflict());
    }
    let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM archava_billing.gateway_links WHERE tenant_id=$1 AND invoice_id=$2 AND closed_at IS NULL)")
        .bind(tenant).bind(request.invoice_id).fetch_one(&mut *connection).await.map_err(db)?;
    if active {
        return Err(conflict());
    }
    let id = Uuid::new_v4();
    let row=sqlx::query("INSERT INTO archava_billing.gateway_links(tenant_id,id,invoice_id,integration_id,service_id,actor_id,request_id,order_id,amount_minor,invoice_revision,state) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'creating') RETURNING *")
        .bind(tenant).bind(id).bind(request.invoice_id).bind(request.integration_id).bind(request.service_id).bind(&principal.id)
        .bind(request.request_id).bind(format!("archava-{id}")).bind(amount).bind(invoice.try_get::<i32,_>("revision").map_err(db)?)
        .fetch_one(connection).await.map_err(db)?;
    Ok((link(&row)?, true))
}

fn require_service(
    principal: &PurchasePrincipal,
    access: &BillingAccess,
) -> Result<(), PurchaseError> {
    if principal.kind != PrincipalKind::Service
        || access.scope != ErpScope::All
        || !access.holds("billing.read")
        || !access.holds("billing.write")
    {
        return Err(forbidden());
    }
    Ok(())
}

/// A visible invoice is required even for replay/status; current scope wins.
pub async fn invoice_links(
    connection: &mut PgConnection,
    tenant: &str,
    access: &BillingAccess,
    invoice: Uuid,
) -> Result<Vec<PaymentLink>, PurchaseError> {
    if !access.holds("billing.read") {
        return Err(forbidden());
    }
    invoice_lock(connection, tenant, invoice, access).await?;
    let rows=sqlx::query("SELECT * FROM archava_billing.gateway_links WHERE tenant_id=$1 AND invoice_id=$2 ORDER BY created_at DESC,id DESC LIMIT 25")
        .bind(tenant).bind(invoice).fetch_all(connection).await.map_err(db)?;
    rows.iter().map(link).collect()
}

#[derive(Debug)]
pub struct Binding {
    pub link: PaymentLink,
    pub transaction_id: Option<String>,
    pub service_id: String,
}
/// Only the registry's service can resolve a provider order; body tenant IDs
/// and invoice codes cannot create a correlation.
pub async fn binding(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &BillingAccess,
    integration: &str,
    order: &str,
) -> Result<Binding, PurchaseError> {
    require_service(principal, access)?;
    let row=sqlx::query("SELECT * FROM archava_billing.gateway_links WHERE tenant_id=$1 AND integration_id=$2 AND order_id=$3 AND service_id=$4")
        .bind(tenant).bind(integration).bind(order).bind(&principal.id).fetch_optional(connection).await.map_err(db)?.ok_or_else(missing)?;
    Ok(Binding {
        link: link(&row)?,
        transaction_id: row.try_get("transaction_id").map_err(db)?,
        service_id: principal.id.clone(),
    })
}

/// Receipt of a private redirect is not a settlement. No response/transport
/// failure leaves a durable unknown claim with the same provider order.
pub async fn finish_creation(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &BillingAccess,
    binding: &Binding,
    redirect: Option<&str>,
) -> Result<PaymentLink, PurchaseError> {
    require_service(principal, access)?;
    invoice_lock(connection, tenant, uuid(&binding.link.invoice_id)?, access).await?;
    let row=sqlx::query("UPDATE archava_billing.gateway_links SET redirect_url=COALESCE(redirect_url,$3),state=CASE WHEN state IN ('creating','creation_unknown') THEN CASE WHEN $3::text IS NULL THEN 'creation_unknown' ELSE 'open' END ELSE state END WHERE tenant_id=$1 AND id=$2 AND service_id=$4 RETURNING *")
        .bind(tenant).bind(uuid(&binding.link.id)?).bind(redirect).bind(&principal.id).fetch_optional(connection).await.map_err(db)?.ok_or_else(missing)?;
    link(&row)
}

/// Reserve a bounded Get Status attempt without holding SQL locks across HTTP.
pub async fn claim_check(
    connection: &mut PgConnection,
    tenant: &str,
    binding: &Binding,
) -> Result<bool, PurchaseError> {
    let count=sqlx::query("UPDATE archava_billing.gateway_links SET last_checked_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2 AND (last_checked_at IS NULL OR last_checked_at < clock_timestamp()-interval '2 seconds')")
        .bind(tenant).bind(uuid(&binding.link.id)?).execute(connection).await.map_err(db)?.rows_affected();
    Ok(count == 1)
}

/// Accept only an authenticated provider observation at the host boundary.
/// Recheck current service grants and every frozen reference in this same TX.
pub async fn apply_observation(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &BillingAccess,
    bound: &Binding,
    observed: &PaymentObservation,
) -> Result<PaymentLink, PurchaseError> {
    require_service(principal, access)?;
    let invoice = invoice_lock(connection, tenant, uuid(&bound.link.invoice_id)?, access).await?;
    let row=sqlx::query("SELECT * FROM archava_billing.gateway_links WHERE tenant_id=$1 AND id=$2 AND service_id=$3 FOR UPDATE")
        .bind(tenant).bind(uuid(&bound.link.id)?).bind(&principal.id).fetch_optional(&mut *connection).await.map_err(db)?.ok_or_else(missing)?;
    let current = link(&row)?;
    let transaction: Option<String> = row.try_get("transaction_id").map_err(db)?;
    if current.order_id != observed.order_id
        || current.integration_id != bound.link.integration_id
        || current.invoice_id != bound.link.invoice_id
        || observed.transaction_id.is_empty()
        || observed.transaction_id.len() > 120
        || transaction
            .as_ref()
            .is_some_and(|old| old != &observed.transaction_id)
    {
        return Err(conflict());
    }
    let change = delta(
        current.amount_minor,
        current.received_minor,
        current.refunded_minor,
        observed,
    )
    .map_err(|_| conflict())?;
    let received = current.received_minor + change.settlement_minor;
    let refunded = current.refunded_minor + change.refund_minor;
    let allocation = allocation(
        received,
        refunded,
        current.allocated_minor,
        current.allocation_refunded_minor,
        invoice.try_get("settled_minor").map_err(db)?,
        invoice.try_get("outstanding_minor").map_err(db)?,
        invoice.try_get::<String, _>("status").map_err(db)? == "issued",
    );
    let mut allocated = current.allocated_minor;
    let mut allocation_refunded = current.allocation_refunded_minor;
    let mut review = observed.state == PaymentState::Review;
    match allocation {
        Ok(change) => {
            for (amount, outcome, total) in [
                (
                    change.settlement_minor,
                    SettlementOutcome::Succeeded,
                    allocated + change.settlement_minor,
                ),
                (
                    change.refund_minor,
                    SettlementOutcome::Refunded,
                    allocation_refunded + change.refund_minor,
                ),
            ] {
                if amount == 0 {
                    continue;
                }
                let direction = if outcome == SettlementOutcome::Succeeded {
                    "credit"
                } else {
                    "refund"
                };
                let mutation = BillingMutation {
                    command: BillingCommand::RecordProviderEvent {
                        command_id: Uuid::new_v4().to_string(),
                        invoice_id: current.invoice_id.clone(),
                        provider_id: format!("midtrans:{}", current.integration_id),
                        provider_event_id: format!("gateway-{}-{direction}-{total}", current.id),
                        outcome,
                        amount_minor: amount,
                        currency: "IDR".into(),
                        signature: String::new(),
                    },
                };
                let encoded = serde_json::to_vec(&mutation.command).map_err(|_| conflict())?;
                let fingerprint = format!("{:x}", Sha256::digest(encoded));
                record_billing_command(
                    connection,
                    tenant,
                    principal,
                    access,
                    &fingerprint,
                    &mutation,
                )
                .await?;
            }
            allocated += change.settlement_minor;
            allocation_refunded += change.refund_minor;
            review |= change.unapplied_minor > 0;
        }
        Err(_) => review = true,
    }
    let json = serde_json::to_value(observed).map_err(|_| conflict())?;
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&json).map_err(|_| conflict())?)
    );
    sqlx::query("INSERT INTO archava_billing.gateway_observations(tenant_id,link_id,fingerprint,observation) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING")
        .bind(tenant).bind(uuid(&current.id)?).bind(fingerprint).bind(json).execute(&mut *connection).await.map_err(db)?;
    // Old pending/failed notifications cannot reverse verified money. Only a
    // verified no-money terminal status or full refund releases the active slot.
    let state = if review {
        "review"
    } else if refunded > 0 {
        "refunded"
    } else if received > 0 {
        "settled"
    } else {
        match observed.state {
            PaymentState::Failed => "failed",
            _ => "pending",
        }
    };
    let close = (received > 0) || (received == 0 && observed.state == PaymentState::Failed);
    let updated=sqlx::query("UPDATE archava_billing.gateway_links SET transaction_id=COALESCE(transaction_id,$3),received_minor=$4,refunded_minor=$5,allocated_minor=$6,allocation_refunded_minor=$7,requires_review=$8,state=$9,closed_at=CASE WHEN $10 THEN COALESCE(closed_at,clock_timestamp()) ELSE closed_at END WHERE tenant_id=$1 AND id=$2 RETURNING *")
        .bind(tenant).bind(uuid(&current.id)?).bind(&observed.transaction_id).bind(received).bind(refunded).bind(allocated).bind(allocation_refunded).bind(review).bind(state).bind(close)
        .fetch_one(connection).await.map_err(db)?;
    link(&updated)
}
