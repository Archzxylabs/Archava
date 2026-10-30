//! Durable email outbox and transmission records for invoice snapshots.
//! Call inside a checked tenant transaction.

use super::{billing::resolve_billing_context, PurchaseError};
use archava_domain::erp::{
    billing::BillingAccess,
    mail::{EmailRecipient, EmailState, EmailSubject, EmailTransmissionReceipt},
    purchasing::{PrincipalKind, PurchasePrincipal},
    ErpError, ErpScope,
};
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

pub fn receipt_from_row(row: &PgRow) -> Result<EmailTransmissionReceipt, PurchaseError> {
    let state_str: String = row.try_get("state").map_err(db)?;
    let state = EmailState::parse(&state_str).ok_or_else(conflict)?;
    Ok(EmailTransmissionReceipt {
        id: row.try_get::<Uuid, _>("id").map_err(db)?.to_string(),
        request_id: row
            .try_get::<Uuid, _>("request_id")
            .map_err(db)?
            .to_string(),
        invoice_id: row
            .try_get::<Uuid, _>("invoice_id")
            .map_err(db)?
            .to_string(),
        recipient: row.try_get("recipient").map_err(db)?,
        subject: row.try_get("subject").map_err(db)?,
        payload_sha256: row.try_get("payload_sha256").map_err(db)?,
        state,
        provider: row.try_get("provider").map_err(db)?,
        provider_message_id: row.try_get("provider_message_id").map_err(db)?,
        error_reason: row.try_get("error_reason").map_err(db)?,
    })
}

pub struct EmailTransmissionRequest<'a> {
    pub request_id: Uuid,
    pub invoice_id: Uuid,
    pub recipient: &'a EmailRecipient,
    pub subject: &'a EmailSubject,
    pub payload_sha256: &'a str,
    pub provider: &'a str,
}

/// Pre-commit email transmission intent before invoking the provider.
/// If the request_id already exists for this tenant with the same payload digest,
/// it idempotently returns the existing record. If the payload differs, it returns Conflict.
pub async fn prepare_email_transmission(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    request: EmailTransmissionRequest<'_>,
) -> Result<(EmailTransmissionReceipt, bool), PurchaseError> {
    if request.payload_sha256.len() != 64
        || !request
            .payload_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(conflict());
    }

    let access = resolve_billing_context(connection, tenant, principal).await?;

    if principal.kind != PrincipalKind::Human
        || !access.holds("billing.write")
        || !access.holds("billing.read")
    {
        return Err(forbidden());
    }

    // Check if invoice exists and belongs to this tenant and scope.
    let invoice_row = sqlx::query(
        "SELECT status FROM archava_billing.invoices \
         WHERE tenant_id = $1 AND id = $2 AND ($3 OR owner_id = $4) FOR SHARE",
    )
    .bind(tenant)
    .bind(request.invoice_id)
    .bind(access.scope == ErpScope::All)
    .bind(&access.owner_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(db)?
    .ok_or_else(missing)?;

    let invoice_status: String = invoice_row.try_get("status").map_err(db)?;
    if invoice_status == "draft" || invoice_status == "void" {
        return Err(conflict());
    }

    // Check restore hold (G09): if restore recovery is holding operations, fail closed.
    let held: bool =
        sqlx::query_scalar("SELECT held FROM archava_billing.gateway_runtime WHERE singleton")
            .fetch_one(&mut *connection)
            .await
            .map_err(db)?;
    if held {
        return Err(domain(ErpError::Unavailable));
    }

    // Check existing transmission under this request_id
    let existing = sqlx::query(
        "SELECT id, request_id, invoice_id, recipient, subject, payload_sha256, state, provider, provider_message_id, error_reason, \
         (updated_at < clock_timestamp() - interval '15 seconds') AS lease_expired, \
         (created_at >= clock_timestamp() - interval '24 hours') AS within_idempotency_window \
         FROM archava_billing.email_outbox WHERE tenant_id = $1 AND request_id = $2 FOR UPDATE",
    )
    .bind(tenant)
    .bind(request.request_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(db)?;

    if let Some(row) = existing {
        let existing_hash: String = row.try_get("payload_sha256").map_err(db)?;
        if existing_hash != request.payload_sha256 {
            return Err(conflict());
        }
        let state_str: String = row.try_get("state").map_err(db)?;
        let state = EmailState::parse(&state_str).ok_or_else(conflict)?;

        if state == EmailState::Accepted
            || state == EmailState::Rejected
            || state == EmailState::Held
        {
            return Ok((receipt_from_row(&row)?, false));
        }

        let within_window: bool = row.try_get("within_idempotency_window").map_err(db)?;
        if !within_window {
            // C10-05: Upstream idempotency window expired (>24h). Blind retry would risk duplicate sends.
            sqlx::query(
                "UPDATE archava_billing.email_outbox \
                 SET state = 'held', error_reason = 'Idempotency window expired (>24h); reconciliation required' \
                 WHERE tenant_id = $1 AND request_id = $2",
            )
            .bind(tenant)
            .bind(request.request_id)
            .execute(&mut *connection)
            .await
            .map_err(db)?;
            return Err(domain(ErpError::Unavailable));
        }

        let lease_expired: bool = row.try_get("lease_expired").map_err(db)?;
        if (state == EmailState::Invoking && lease_expired) || state == EmailState::Unknown {
            sqlx::query(
                "UPDATE archava_billing.email_outbox SET state = 'invoking' WHERE tenant_id = $1 AND request_id = $2",
            )
            .bind(tenant)
            .bind(request.request_id)
            .execute(&mut *connection)
            .await
            .map_err(db)?;
            return Ok((receipt_from_row(&row)?, true));
        }

        return Ok((receipt_from_row(&row)?, false));
    }

    // Advisory xact lock to serialize concurrent attempts for the same invoice email payload
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!(
            "email:{}:{}:{}",
            tenant, request.invoice_id, request.payload_sha256
        ))
        .execute(&mut *connection)
        .await
        .map_err(db)?;

    // Check existing transmission for this invoice with identical payload (G03 duplicate prevention)
    let existing_by_invoice = sqlx::query(
        "SELECT id, request_id, invoice_id, recipient, subject, payload_sha256, state, provider, provider_message_id, error_reason \
         FROM archava_billing.email_outbox \
         WHERE tenant_id = $1 AND invoice_id = $2 AND payload_sha256 = $3 AND state IN ('accepted', 'invoking') \
         ORDER BY created_at DESC LIMIT 1 FOR SHARE",
    )
    .bind(tenant)
    .bind(request.invoice_id)
    .bind(request.payload_sha256)
    .fetch_optional(&mut *connection)
    .await
    .map_err(db)?;

    if let Some(row) = existing_by_invoice {
        return Ok((receipt_from_row(&row)?, false));
    }

    let id = Uuid::new_v4();
    let initial_state = EmailState::Invoking.as_str();

    let inserted = sqlx::query(
        "INSERT INTO archava_billing.email_outbox \
         (tenant_id, id, invoice_id, actor_id, request_id, recipient, subject, payload_sha256, state, provider) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
         RETURNING id, request_id, invoice_id, recipient, subject, payload_sha256, state, provider, provider_message_id, error_reason",
    )
    .bind(tenant)
    .bind(id)
    .bind(request.invoice_id)
    .bind(&principal.id)
    .bind(request.request_id)
    .bind(request.recipient.as_str())
    .bind(request.subject.as_str())
    .bind(request.payload_sha256)
    .bind(initial_state)
    .bind(request.provider)
    .fetch_one(&mut *connection)
    .await
    .map_err(db)?;

    Ok((receipt_from_row(&inserted)?, true))
}

/// Authoritative outcome from the provider invocation.
pub async fn record_email_outcome(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    state: EmailState,
    provider_message_id: Option<&str>,
    error_reason: Option<&str>,
) -> Result<EmailTransmissionReceipt, PurchaseError> {
    let updated = sqlx::query(
        "UPDATE archava_billing.email_outbox \
         SET state = $3, provider_message_id = COALESCE($4, provider_message_id), error_reason = $5 \
         WHERE tenant_id = $1 AND request_id = $2 \
         RETURNING id, request_id, invoice_id, recipient, subject, payload_sha256, state, provider, provider_message_id, error_reason",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(state.as_str())
    .bind(provider_message_id)
    .bind(error_reason)
    .fetch_optional(&mut *connection)
    .await
    .map_err(db)?
    .ok_or_else(missing)?;

    receipt_from_row(&updated)
}

/// Retrieve email transmission history for an invoice with optional exact requestId filter.
pub async fn list_invoice_emails(
    connection: &mut PgConnection,
    tenant: &str,
    access: &BillingAccess,
    invoice_id: Uuid,
    request_id: Option<Uuid>,
) -> Result<Vec<EmailTransmissionReceipt>, PurchaseError> {
    if !access.holds("billing.read") {
        return Err(forbidden());
    }

    let rows = sqlx::query(
        "SELECT o.id, o.request_id, o.invoice_id, o.recipient, o.subject, o.payload_sha256, o.state, o.provider, o.provider_message_id, o.error_reason \
         FROM archava_billing.email_outbox o \
         JOIN archava_billing.invoices i ON i.tenant_id = o.tenant_id AND i.id = o.invoice_id \
         WHERE o.tenant_id = $1 AND o.invoice_id = $2 AND ($3 OR i.owner_id = $4) \
           AND ($5::uuid IS NULL OR o.request_id = $5) \
         ORDER BY o.created_at DESC, o.id \
         LIMIT 50",
    )
    .bind(tenant)
    .bind(invoice_id)
    .bind(access.scope == ErpScope::All)
    .bind(&access.owner_id)
    .bind(request_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(db)?;

    rows.iter().map(receipt_from_row).collect()
}
