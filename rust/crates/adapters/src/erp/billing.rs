//! The billing SQL adapter — the Archxy 2 port of
//! `packages/billing-pg/src/repository.ts` (`execute()` + `getCommandStatus()`,
//! the R07 settlement surface, PACKET-007).
//!
//! ## Statement order (the TS port)
//!
//! The `execute()` ladder with the billing vocabulary: the membership row
//! (FOR SHARE through `archava_billing.lock_membership`, resolved by the
//! host's context step), the operation's own write permission
//! (`create`/`attempt`/`provider event` → `billing.write`;
//! `issue`/`correction`/`void` → `billing.settle`), then the arm's own row
//! lock — and the `commands` claim pairing the TS defines, which is NOT the
//! assets shape: billing SELECTs the command row first, refuses a
//! fingerprint mismatch on an existing row, returns a stored receipt as the
//! idempotent REPLAY (`idempotencyReplay: true` in the answer), claims
//! (`INSERT`) only when the row is absent, and falls THROUGH on a
//! receipt-less row so a partial command re-executes inside the same
//! transaction — the TS partial-command case, statement for statement. The
//! replay gates on the FINGERPRINT CELL alone (no recorded-actor gate
//! anywhere in the billing repository — a second actor resubmitting an
//! identical `commandId` replays, verbatim).
//!
//! The arms lock their own invoice row first (`SELECT * ... FOR UPDATE`).
//! The balance arithmetic (`gross + correction − settled`) is the
//! migration's own CHECK as well as every arm's computed invariant; the
//! status transition table and the priced-invoice immutability ride the
//! ledger's guard trigger IN ADDITION to the arms' own
//! `invalid_transition` verdicts.
//!
//! ## The refusal net (billing's TS codes, folded)
//!
//! The TS repository + service raise `forbidden`, `invalid_signature`,
//! `not_found`, `conflict`, `invalid_transition` and
//! `unbalanced_settlement`. The carrier net collapses by HTTP class, as
//! the TS api's own `codeMap` does: `forbidden` AND `invalid_signature`
//! are the 403 class ([`ErpError::Forbidden`]); `not_found` the 404 class
//! ([`ErpError::NotFound`]); and `conflict`, `invalid_transition`,
//! `invoice_immutable` and `unbalanced_settlement` the 409 class
//! ([`ErpError::Conflict`]). `bad_request` never reaches runtime in the
//! TS mutation path (it is the tools boundary and the parse boundary),
//! so no runtime arm carries a 400-class carrier — the label losses
//! (`invalid_signature`, `invalid_transition`,
//! `unbalanced_settlement`) are recorded in the packet's
//! KNOWN_DIFFERENCES ledger, never silent.
//!
//! ## Database errors stay database failures (verbatim TS)
//!
//! The billing repository ships NO `catch` whitelist — unlike purchasing
//! and assets, whose TS catch blocks mapped SQLSTATEs onto domain codes.
//! A refused CHECK, unique constraint or trigger exception surfaces as
//! the driver's own failure (the TS 500 class), and the port answers the
//! same class: every `sqlx::Error` here rides [`PurchaseError::Database`].
//! The ledger's own guards (the `invoice_balance` CHECK,
//! `provider_event_unique`, the append-only triggers) still protect the
//! tables — they protect them inside a rollback whose verdict is a
//! database failure, exactly as the TS answers. No ledger-specific
//! SQLSTATE arm exists here, and no witness depends on a
//! unique-violation verdict either way.
//!
//! ## The safe-integer row boundary
//!
//! `minor()` — the TS read helper raising `conflict` on any stored amount
//! outside ±2^53−1 — rides the ROW reads here (`minor_safe`), so the
//! receipt's rendered amounts are exactly the integers the TS parses
//! from its rows. A caller-SUPPLIED amount carries no safe-integer gate
//! in the TS grammar and carries none here: the bigint column and the
//! checked arithmetic hold the truth.
//!
//! ## The sampled clock
//!
//! One `clock_timestamp()` sample per command (the R04/R05 adapters'
//! family), bound into every stamping position of the arm's statements —
//! created/recorded/issued/settled/voided/verified stamp the SAME
//! instant, while the TS lets each statement's `clock_timestamp()`
//! default and `CASE` re-sample. Recorded with the sampling divergence
//! the R04–R05 adapters already carry.
//!
//! ## Deliberately not here
//!
//! The repository's read paths (`getAccess`, `listInvoices`,
//! `getInvoiceDetail` and the four child lists) and the tools surface
//! ride their serving cohorts: no mounted read route exists yet, so
//! porting them would ship unused SQL with no witness. The mutation +
//! commandStatus surface this module ports is exactly what the host
//! routes and the suite drive, plus `assert_billing_kernel` — the port
//! of repository `create()`'s provision checks.

use sqlx::PgConnection;
use uuid::Uuid;

use archava_domain::erp::billing::{
    BillingAccess, BillingCommand, BillingCommandStatus, BillingMutation, BillingReceipt,
    SettlementOutcome, BILLING_PERMISSIONS, SAFE_INTEGER_MAX, STATUS_DRAFT, STATUS_ISSUED,
    STATUS_SETTLED, STATUS_VOID,
};
use archava_domain::erp::purchasing::PurchasePrincipal;
use archava_domain::erp::ErpError;

use super::commerce_kernel::{load_locked_membership, MembershipRow};
use super::PurchaseError;

/// The ledger label on database errors.
pub const BILLING_LEDGER: &str = "billing";

/// The `archava_billing` namespace — checked constants only.
pub const BILLING_NAMESPACE: &str = "archava_billing";

// --- Refusal helpers ---------------------------------------------------------

fn forbidden() -> PurchaseError {
    PurchaseError::Domain(ErpError::Forbidden)
}

fn conflict() -> PurchaseError {
    PurchaseError::Domain(ErpError::Conflict)
}

fn not_found() -> PurchaseError {
    PurchaseError::Domain(ErpError::NotFound)
}

/// Every driver failure is a database failure — the TS billing
/// repository ships no SQLSTATE catch whitelist (see the module header),
/// so the port folds nothing at this boundary either.
fn refuses(error: sqlx::Error) -> PurchaseError {
    PurchaseError::Database {
        ledger: BILLING_LEDGER,
        source: error,
    }
}

fn parse_uuid_value(value: &str) -> Result<Uuid, PurchaseError> {
    Uuid::parse_str(value).map_err(|_| conflict())
}

/// `minor()` — the stored-amount boundary: outside the JS safe-integer
/// range the TS read helper raises `conflict`, and the port raises the
/// same 409 class, so no receipt renders an amount the TS could not.
fn minor_safe(value: i64) -> Result<i64, PurchaseError> {
    if value.abs() > SAFE_INTEGER_MAX {
        return Err(conflict());
    }
    Ok(value)
}

/// The revision column's int4 bound — the TS `Number(invoice.revision)`
/// travels through an `integer` column; a drift past its range is the
/// conflict the binding would raise as.
fn int4(value: i64) -> Result<i32, PurchaseError> {
    if !(i32::MIN as i64..=i32::MAX as i64).contains(&value) {
        return Err(conflict());
    }
    Ok(value as i32)
}

/// The `clock_timestamp()` sample every command takes once — the canonical
/// millisecond `Z` render, same family as the assets adapter's.
async fn observed_now(connection: &mut PgConnection) -> Result<String, PurchaseError> {
    sqlx::query_as::<_, (String,)>(
        "SELECT to_char(clock_timestamp() at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .fetch_one(&mut *connection)
    .await
    .map(|(text,)| text)
    .map_err(refuses)
}

// --- The context ladder --------------------------------------------------------

/// The membership ladder — ONE row, and billing's own shape: no kind
/// comparison, no on-behalf-of intersection (the TS `transaction()` loads
/// the principal's OWN membership and names its own id as the owner). A
/// service principal's access envelope is that service's row, verbatim —
/// `owner_id` is the service's own id, so the commands' FK and the audit
/// trail name who actually held the hands on the ledger.
pub async fn resolve_billing_context(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<BillingAccess, PurchaseError> {
    let own: MembershipRow = load_locked_membership(
        connection,
        BILLING_NAMESPACE,
        BILLING_LEDGER,
        tenant,
        &principal.id,
        BILLING_PERMISSIONS,
    )
    .await?
    .ok_or_else(forbidden)?;
    if !own.active {
        return Err(forbidden());
    }
    // The TS envelope: the row's permissions and scope, the caller's own
    // id as the owner — there is NO human branch here to intersect with.
    Ok(BillingAccess {
        permissions: own.permissions.clone(),
        owner_id: principal.id.clone(),
        scope: own.scope,
    })
}

// --- The invoice row read ---------------------------------------------------------

/// `loadInvoice` — the arm's lock read. `SELECT * ... FOR UPDATE` over
/// exactly the columns an arm judges; no rows = `not_found`, the TS
/// throw verbatim.
const INVOICE_SELECT_UPDATE: &str = "SELECT tenant_id,id,status,currency,gross_minor,\
correction_minor,settled_minor,outstanding_minor,revision FROM archava_billing.invoices \
WHERE tenant_id=$1 AND id=$2 FOR UPDATE";

/// The invoice row's cells the arms judge — `row` in TS terms. The
/// amount cells ride `minor_safe` at read time (the TS `minor()`); the
/// status/currency cells stay exact strings so every arm's comparison is
/// the TS one (`invoice.status !== 'draft'`), unmediated by an enum that
/// would have to invent verdicts for a drifted cell.
struct InvoiceRow {
    status: String,
    currency: String,
    gross_minor: i64,
    correction_minor: i64,
    settled_minor: i64,
    outstanding_minor: i64,
    revision: i64,
}

impl InvoiceRow {
    fn try_get(
        row: &sqlx::postgres::PgRow,
        tenant: &str,
        invoice_id: Uuid,
    ) -> Result<Self, PurchaseError> {
        use sqlx::Row;
        let tenant_cell: String = row.try_get("tenant_id").map_err(refuses)?;
        if tenant_cell != tenant {
            // RLS makes a cross-tenant screen impossible; a mismatch is drift.
            return Err(conflict());
        }
        let row_id: Uuid = row.try_get("id").map_err(refuses)?;
        if row_id != invoice_id {
            return Err(conflict());
        }
        let revision: i32 = row.try_get("revision").map_err(refuses)?;
        Ok(Self {
            status: row.try_get("status").map_err(refuses)?,
            currency: row.try_get("currency").map_err(refuses)?,
            gross_minor: minor_safe(row.try_get("gross_minor").map_err(refuses)?)?,
            correction_minor: minor_safe(row.try_get("correction_minor").map_err(refuses)?)?,
            settled_minor: minor_safe(row.try_get("settled_minor").map_err(refuses)?)?,
            outstanding_minor: minor_safe(row.try_get("outstanding_minor").map_err(refuses)?)?,
            revision: i64::from(revision),
        })
    }

    /// The balance invariant — `gross + correction − settled`, the exact
    /// arithmetic `outstandingFrom` states and the migration's own
    /// `invoice_balance` CHECK enforces.
    fn outstanding(&self, next_correction: i64, next_settled: i64) -> i64 {
        self.gross_minor + next_correction - next_settled
    }
}

async fn invoice_row(
    connection: &mut PgConnection,
    tenant: &str,
    invoice_id: Uuid,
) -> Result<InvoiceRow, PurchaseError> {
    let row = sqlx::query(INVOICE_SELECT_UPDATE)
        .bind(tenant)
        .bind(invoice_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(refuses)?
        .ok_or_else(not_found)?;
    InvoiceRow::try_get(&row, tenant, invoice_id)
}

// --- The claim + replay plumbing (billing's own SELECT-then-INSERT shape) -------

/// The TS first statement: SELECT the command row by (tenant, request).
/// The row's `fingerprint` and `receipt` cells are the only cells the
/// replay branch reads — no actor column is compared, verbatim.
async fn fetch_command_row(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
) -> Result<Option<(String, Option<serde_json::Value>)>, PurchaseError> {
    sqlx::query_as(
        "SELECT fingerprint,receipt FROM archava_billing.commands \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)
}

/// The TS claim — `INSERT INTO archava_billing.commands(...)` with NO
/// ON CONFLICT clause: the row's absence was just observed, and a
/// concurrent rival claimer would be ROLLBACKed whole by their own
/// unique violation.
async fn claim_billing_command(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    actor_id: &str,
    fingerprint: &str,
) -> Result<(), PurchaseError> {
    sqlx::query(
        "INSERT INTO archava_billing.commands(\
tenant_id,request_id,actor_id,fingerprint) VALUES($1,$2,$3,$4)",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(actor_id)
    .bind(fingerprint)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(())
}

/// The audit row — `(tenant, request, actor, operation, invoice?,
/// revision, reason?)`, one per command, exactly the TS `audit()`
/// helper's statement (the ledger's OWN audit table — invoice-flavoured,
/// not asset-flavoured).
#[allow(clippy::too_many_arguments)]
async fn insert_billing_audit(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    actor_id: &str,
    operation: &str,
    invoice_id: Option<Uuid>,
    revision: i64,
    reason: Option<&str>,
    now: &str,
) -> Result<(), PurchaseError> {
    sqlx::query(
        "INSERT INTO archava_billing.audit(\
tenant_id,request_id,actor_id,operation,invoice_id,revision,reason,occurred_at) \
VALUES($1,$2,$3,$4,$5,$6,$7,$8::timestamptz)",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(actor_id)
    .bind(operation)
    .bind(invoice_id)
    .bind(int4(revision)?)
    .bind(reason)
    .bind(now)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(())
}

/// The TS `audit()` reason strings — the per-arm formats the TS
/// repository splats, one helper here so the arms stay statement-shaped.
fn audit_reason(command: &BillingCommand) -> String {
    match command {
        BillingCommand::CreateInvoice {
            invoice_code,
            lines,
            ..
        } => format!(
            "Created invoice {invoice_code} with {} priced line(s)",
            lines.len()
        ),
        BillingCommand::IssueInvoice { reason, .. } => reason.clone(),
        BillingCommand::AppendCorrection {
            amount_minor,
            reason,
            ..
        } => format!("Appended correction of {amount_minor} minor units: {reason}"),
        BillingCommand::VoidInvoice { reason, .. } => reason.clone(),
        BillingCommand::RecordPaymentAttempt { amount_minor, .. } => {
            format!("Recorded payment attempt of {amount_minor} minor units (no settlement effect)")
        }
        BillingCommand::RecordProviderEvent {
            provider_id,
            provider_event_id,
            outcome,
            amount_minor,
            ..
        } => {
            format!(
                "Provider {provider_id} reported {} for {amount_minor} minor units [{provider_event_id}]",
                outcome.as_str()
            )
        }
    }
}

/// The TS `grant()` — the operation's own permission from the access
/// envelope, refusing closed (`Missing required permission:` in TS).
fn grant(access: &BillingAccess, permission: &str) -> Result<(), PurchaseError> {
    if !access.holds(permission) {
        return Err(forbidden());
    }
    Ok(())
}

// --- The mutation pipeline -------------------------------------------------------

/// The mutation pipeline — the port of `execute()`. The host hands over
/// the ALREADY strict-parsed mutation (its boundary parse answers 400),
/// the ALREADY verified provider signature (the service's check runs
/// BEFORE any SQL, so an under-signed claim never opens a transaction),
/// and the fingerprint that keyed the claim.
///
/// The fingerprint is the TS composition — `HMAC(key,
/// JSON.stringify(mutation))` — carried on the labelled HMAC the other
/// ERP ledgers use (`billing-command-fingerprint`), over the TYPED arm
/// document ALONE: no tenant, no actor (the TS fingerprint covers the
/// mutation object and nothing else). Ledger-internal stability is
/// exact; see the module header.
pub async fn record_billing_command(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &BillingAccess,
    fingerprint: &str,
    mutation: &BillingMutation,
) -> Result<BillingReceipt, PurchaseError> {
    // `assertHumanOperator` — every arm EXCEPT the provider event is
    // refused for a service principal. The one arm allowed through is
    // the one authorised by a signature instead of by a person.
    if principal.kind.as_str() != "human"
        && !matches!(mutation.command, BillingCommand::RecordProviderEvent { .. })
    {
        return Err(forbidden());
    }
    let command = &mutation.command;
    grant(access, command.required_permission())?;
    let request_id = parse_uuid_value(command.command_id())?;
    let actor_id = principal.id.as_str();

    // The TS claim pairing — SELECT first, replay or refuse, claim when
    // absent, and fall through on a receipt-less row so a partial
    // command RE-EXECUTES inside this same transaction.
    if let Some((row_fingerprint, row_receipt)) =
        fetch_command_row(connection, tenant, request_id).await?
    {
        if row_fingerprint != fingerprint {
            // `Command payload fingerprint mismatch on replay`.
            return Err(conflict());
        }
        if let Some(receipt_value) = row_receipt {
            // The idempotent replay: the stored document, re-parsed with
            // `idempotencyReplay: true`.
            let stored = BillingReceipt::parse(&receipt_value, true).map_err(|_| conflict())?;
            return Ok(stored);
        }
    } else {
        claim_billing_command(connection, tenant, request_id, actor_id, fingerprint).await?;
    }

    // The sampled clock — one instant per command, bound everywhere.
    let now = observed_now(connection).await?;

    let receipt =
        execute_billing_arm(connection, tenant, access, request_id, command, &now).await?;
    // The TS `audit()` — every arm records once, the invoice id always
    // the row the arm produced or named.
    let audit_invoice = match &receipt.invoice_id {
        Some(invoice_id) => Some(parse_uuid_value(invoice_id)?),
        None => None,
    };
    insert_billing_audit(
        connection,
        tenant,
        request_id,
        actor_id,
        command.mutation_type(),
        audit_invoice,
        receipt.revision.unwrap_or(1),
        Some(audit_reason(command).as_str()),
        &now,
    )
    .await?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|_| conflict())?;
    sqlx::query(
        "UPDATE archava_billing.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(&receipt_value)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(receipt)
}

// --- execute(): the six arms ---------------------------------------------------

/// The six executed arms, in the TS branch order. The create opens its
/// own invoice; every other arm locks the invoice row first. Every
/// statement binds values the parse already judged; the reason texts and
/// the generated ids travel with the arm.
async fn execute_billing_arm(
    connection: &mut PgConnection,
    tenant: &str,
    access: &BillingAccess,
    request_id: Uuid,
    command: &BillingCommand,
    now: &str,
) -> Result<BillingReceipt, PurchaseError> {
    match command {
        BillingCommand::CreateInvoice {
            invoice_code,
            customer_ref,
            currency,
            consumer_ref,
            lines,
            ..
        } => {
            // create_invoice — the priced line sheet: gross = Σ quantity ×
            // unit, the receipt carries the FIRST line's id. The TS
            // reduce's float product rounds silently for monstrous
            // sheets; the port refuses (checked arithmetic — the product
            // the migration's own line CHECK could not hold either).
            let invoice_id = Uuid::new_v4();
            let mut gross_minor: i64 = 0;
            for line in lines {
                let total = line
                    .quantity
                    .checked_mul(line.unit_minor)
                    .ok_or_else(conflict)?;
                gross_minor = gross_minor.checked_add(total).ok_or_else(conflict)?;
            }
            sqlx::query(
                "INSERT INTO archava_billing.invoices(\
tenant_id,id,invoice_code,customer_ref,currency,status,gross_minor,outstanding_minor,\
consumer_ref,owner_id,created_at) VALUES($1,$2,$3,$4,$5,'draft',$6,$6,$7,$8,$9::timestamptz)",
            )
            .bind(tenant)
            .bind(invoice_id)
            .bind(invoice_code)
            .bind(customer_ref)
            .bind(currency)
            .bind(gross_minor)
            .bind(consumer_ref)
            .bind(&access.owner_id)
            .bind(now)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;

            let mut first_line_id: Option<Uuid> = None;
            for line in lines {
                let line_id = Uuid::new_v4();
                first_line_id.get_or_insert(line_id);
                let line_total = line
                    .quantity
                    .checked_mul(line.unit_minor)
                    .ok_or_else(conflict)?;
                sqlx::query(
                    "INSERT INTO archava_billing.invoice_lines(\
tenant_id,id,invoice_id,description,quantity,unit_minor,line_total_minor,recorded_by,recorded_at) \
VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9::timestamptz)",
                )
                .bind(tenant)
                .bind(line_id)
                .bind(invoice_id)
                .bind(&line.description)
                .bind(int4(line.quantity)?)
                .bind(line.unit_minor)
                .bind(line_total)
                .bind(&access.owner_id)
                .bind(now)
                .execute(&mut *connection)
                .await
                .map_err(refuses)?;
            }

            Ok(BillingReceipt {
                command_id: request_id.to_string(),
                tenant_id: tenant.to_string(),
                action: "create_invoice",
                invoice_id: Some(invoice_id.to_string()),
                line_id: first_line_id.map(|uuid| uuid.to_string()),
                correction_id: None,
                attempt_id: None,
                event_id: None,
                status: STATUS_DRAFT.to_string(),
                outstanding_minor: Some(gross_minor),
                revision: Some(1),
                idempotency_replay: false,
                executed_at: now.to_string(),
            })
        }

        BillingCommand::IssueInvoice {
            invoice_id,
            expected_revision,
            ..
        } => {
            let invoice = parse_uuid_value(invoice_id)?;
            let row = invoice_row(connection, tenant, invoice).await?;
            if row.revision != *expected_revision {
                // `Optimistic revision conflict on invoice`.
                return Err(conflict());
            }
            // `Only a draft invoice can be issued; ...` — the
            // transition verdict behind the 409 fold.
            if row.status != STATUS_DRAFT {
                return Err(conflict());
            }
            let next_rev = row.revision + 1;
            let updated = sqlx::query(
                "UPDATE archava_billing.invoices \
SET status='issued', issued_at=$2::timestamptz, revision=$3 WHERE tenant_id=$1 AND id=$4",
            )
            .bind(tenant)
            .bind(now)
            .bind(int4(next_rev)?)
            .bind(invoice)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            if updated.rows_affected() == 0 {
                return Err(not_found());
            }

            Ok(BillingReceipt {
                command_id: request_id.to_string(),
                tenant_id: tenant.to_string(),
                action: "issue_invoice",
                invoice_id: Some(invoice.to_string()),
                line_id: None,
                correction_id: None,
                attempt_id: None,
                event_id: None,
                status: STATUS_ISSUED.to_string(),
                outstanding_minor: Some(row.outstanding_minor),
                revision: Some(next_rev),
                idempotency_replay: false,
                executed_at: now.to_string(),
            })
        }

        BillingCommand::AppendCorrection {
            invoice_id,
            amount_minor,
            reason,
            reverses_id,
            ..
        } => {
            let invoice = parse_uuid_value(invoice_id)?;
            let row = invoice_row(connection, tenant, invoice).await?;
            // `Corrections apply to an issued invoice; ...`.
            if row.status != STATUS_ISSUED {
                return Err(conflict());
            }
            // The wire's uuid-string rides a UUID-typed parameter — a
            // text-typed bind would raise 42804 even for the NULL case.
            let reverses_id: Option<Uuid> =
                reverses_id.as_deref().map(parse_uuid_value).transpose()?;
            if let Some(reverses_uuid) = reverses_id {
                let target: Option<(Uuid,)> = sqlx::query_as(
                    "SELECT invoice_id FROM archava_billing.invoice_corrections \
WHERE tenant_id=$1 AND id=$2",
                )
                .bind(tenant)
                .bind(reverses_uuid)
                .fetch_optional(&mut *connection)
                .await
                .map_err(refuses)?;
                match target {
                    // `Correction being reversed was not found`.
                    None => return Err(not_found()),
                    // `Correction being reversed belongs to a different
                    // invoice` — the cross-invoice reversal.
                    Some((target_invoice,)) if target_invoice != invoice => return Err(conflict()),
                    Some((_,)) => {}
                }
            }

            // The TS correction arithmetic — no safe-int gate on the SUM
            // (the receipt's rendered amounts ride the row `minor()`
            // reads only).
            let next_correction = row
                .correction_minor
                .checked_add(*amount_minor)
                .ok_or_else(conflict)?;
            let next_outstanding = row.outstanding(next_correction, row.settled_minor);
            let next_rev = row.revision + 1;
            sqlx::query(
                "UPDATE archava_billing.invoices \
SET correction_minor=$1, outstanding_minor=$2, revision=$3 WHERE tenant_id=$4 AND id=$5",
            )
            .bind(next_correction)
            .bind(next_outstanding)
            .bind(int4(next_rev)?)
            .bind(tenant)
            .bind(invoice)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;

            let correction_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO archava_billing.invoice_corrections(\
tenant_id,id,invoice_id,amount_minor,reason,reverses_id,recorded_by,created_at) \
VALUES($1,$2,$3,$4,$5,$6,$7,$8::timestamptz)",
            )
            .bind(tenant)
            .bind(correction_id)
            .bind(invoice)
            .bind(*amount_minor)
            .bind(reason)
            .bind(reverses_id)
            .bind(&access.owner_id)
            .bind(now)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;

            Ok(BillingReceipt {
                command_id: request_id.to_string(),
                tenant_id: tenant.to_string(),
                action: "append_correction",
                invoice_id: Some(invoice.to_string()),
                line_id: None,
                correction_id: Some(correction_id.to_string()),
                attempt_id: None,
                event_id: None,
                status: STATUS_ISSUED.to_string(),
                outstanding_minor: Some(next_outstanding),
                revision: Some(next_rev),
                idempotency_replay: false,
                executed_at: now.to_string(),
            })
        }

        BillingCommand::VoidInvoice {
            invoice_id,
            expected_revision,
            ..
        } => {
            let invoice = parse_uuid_value(invoice_id)?;
            let row = invoice_row(connection, tenant, invoice).await?;
            if row.revision != *expected_revision {
                return Err(conflict());
            }
            // The transition verdict the TS names (`A ${status} invoice
            // cannot be voided`): a settled invoice must refund first,
            // void is terminal.
            if !matches!(row.status.as_str(), STATUS_DRAFT | STATUS_ISSUED) {
                return Err(conflict());
            }
            // `unbalanced_settlement` — `An invoice with recorded
            // settlement must be refunded, not voided`.
            if row.settled_minor != 0 {
                return Err(conflict());
            }
            let next_rev = row.revision + 1;
            let updated = sqlx::query(
                "UPDATE archava_billing.invoices \
SET status='void', voided_at=$2::timestamptz, revision=$3 WHERE tenant_id=$1 AND id=$4",
            )
            .bind(tenant)
            .bind(now)
            .bind(int4(next_rev)?)
            .bind(invoice)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            if updated.rows_affected() == 0 {
                return Err(not_found());
            }

            Ok(BillingReceipt {
                command_id: request_id.to_string(),
                tenant_id: tenant.to_string(),
                action: "void_invoice",
                invoice_id: Some(invoice.to_string()),
                line_id: None,
                correction_id: None,
                attempt_id: None,
                event_id: None,
                status: STATUS_VOID.to_string(),
                outstanding_minor: Some(row.outstanding_minor),
                revision: Some(next_rev),
                idempotency_replay: false,
                executed_at: now.to_string(),
            })
        }

        BillingCommand::RecordPaymentAttempt {
            invoice_id,
            amount_minor,
            provider_ref,
            ..
        } => {
            let invoice = parse_uuid_value(invoice_id)?;
            let row = invoice_row(connection, tenant, invoice).await?;
            // `A voided invoice cannot take a payment attempt`.
            if row.status == STATUS_VOID {
                return Err(conflict());
            }
            let attempt_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO archava_billing.payment_attempts(\
tenant_id,id,invoice_id,amount_minor,currency,provider_ref,recorded_by,created_at) \
VALUES($1,$2,$3,$4,$5,$6,$7,$8::timestamptz)",
            )
            .bind(tenant)
            .bind(attempt_id)
            .bind(invoice)
            .bind(*amount_minor)
            // The attempt rides the invoice's own currency — the TS binds
            // `invoice.currency`, never the caller's (the arm takes none).
            .bind(&row.currency)
            .bind(provider_ref)
            .bind(&access.owner_id)
            .bind(now)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;

            Ok(BillingReceipt {
                command_id: request_id.to_string(),
                tenant_id: tenant.to_string(),
                action: "record_payment_attempt",
                invoice_id: Some(invoice.to_string()),
                line_id: None,
                correction_id: None,
                attempt_id: Some(attempt_id.to_string()),
                event_id: None,
                // `attempted` — the receipt's own label, NOT an invoice
                // status: no arm has moved any money.
                status: "attempted".to_string(),
                outstanding_minor: Some(row.outstanding_minor),
                revision: Some(row.revision),
                idempotency_replay: false,
                executed_at: now.to_string(),
            })
        }

        BillingCommand::RecordProviderEvent {
            invoice_id,
            provider_id,
            provider_event_id,
            outcome,
            amount_minor,
            currency,
            ..
        } => {
            let invoice = parse_uuid_value(invoice_id)?;
            let row = invoice_row(connection, tenant, invoice).await?;
            // `unbalanced_settlement` — `Provider event currency does not
            // match the invoiced currency`.
            if row.currency.as_str() != currency.as_str() {
                return Err(conflict());
            }

            // The TS settlement ladder: `nextSettled`, `nextStatus` and
            // the receipt's own `statusLabel` — started FROM the row.
            let mut next_settled = row.settled_minor;
            let mut next_status = row.status.clone();
            let mut status_label = row.status.clone();

            match outcome {
                SettlementOutcome::Succeeded => {
                    // `A ${status} invoice cannot accept a successful
                    // settlement` — only an issued invoice settles.
                    if row.status != STATUS_ISSUED {
                        return Err(conflict());
                    }
                    next_settled = row
                        .settled_minor
                        .checked_add(*amount_minor)
                        .ok_or_else(conflict)?;
                    let next_outstanding = row.outstanding(row.correction_minor, next_settled);
                    // `Settlement exceeds the outstanding balance`.
                    if next_outstanding < 0 {
                        return Err(conflict());
                    }
                    if next_outstanding == 0 {
                        next_status = STATUS_SETTLED.to_string();
                        status_label = STATUS_SETTLED.to_string();
                    }
                }
                SettlementOutcome::Refunded => {
                    // `Refund exceeds the amount already settled`.
                    if row.settled_minor < *amount_minor {
                        return Err(conflict());
                    }
                    next_settled = row.settled_minor - *amount_minor;
                    // A refund REOPENS a settled invoice.
                    if row.status == STATUS_SETTLED {
                        next_status = STATUS_ISSUED.to_string();
                        status_label = STATUS_ISSUED.to_string();
                    }
                }
                // `failed` — history only: the receipt's own label, and
                // the invoice row is NOT even touched (no revision).
                SettlementOutcome::Failed => {
                    status_label = "failed".to_string();
                }
            }

            // A failed notification moves no money and no state, so it
            // never touches the invoice row — not even its revision.
            // Everything that makes the balance change is confined to
            // the two outcomes above (`movesBalance`).
            let outstanding = row.outstanding(row.correction_minor, next_settled);
            let moves_balance = *outcome != SettlementOutcome::Failed;
            if moves_balance {
                let updated = sqlx::query(
                    "UPDATE archava_billing.invoices \
SET settled_minor=$1, outstanding_minor=$2, status=$3, revision=$4, \
settled_at=CASE WHEN $3='settled' THEN $5::timestamptz ELSE settled_at END \
WHERE tenant_id=$6 AND id=$7",
                )
                .bind(next_settled)
                .bind(outstanding)
                .bind(&next_status)
                .bind(int4(row.revision + 1)?)
                .bind(now)
                .bind(tenant)
                .bind(invoice)
                .execute(&mut *connection)
                .await
                .map_err(refuses)?;
                if updated.rows_affected() == 0 {
                    return Err(not_found());
                }
            }

            let event_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO archava_billing.payment_events(\
tenant_id,id,invoice_id,provider_id,provider_event_id,outcome,amount_minor,currency,recorded_by,verified_at) \
VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10::timestamptz)",
            )
            .bind(tenant)
            .bind(event_id)
            .bind(invoice)
            .bind(provider_id)
            .bind(provider_event_id)
            .bind(outcome.as_str())
            .bind(*amount_minor)
            .bind(currency)
            .bind(&access.owner_id)
            .bind(now)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;

            Ok(BillingReceipt {
                command_id: request_id.to_string(),
                tenant_id: tenant.to_string(),
                action: "record_provider_event",
                invoice_id: Some(invoice.to_string()),
                line_id: None,
                correction_id: None,
                attempt_id: None,
                event_id: Some(event_id.to_string()),
                status: status_label,
                outstanding_minor: Some(outstanding),
                revision: Some(if moves_balance {
                    row.revision + 1
                } else {
                    row.revision
                }),
                idempotency_replay: false,
                executed_at: now.to_string(),
            })
        }
    }
}

// --- commandStatus ---------------------------------------------------------------

/// `getCommandStatus()` — NO permission gate beyond the transaction's
/// own enrollment (the TS status read grants nothing; an already
/// unlocked principal sees any rule the row names), and no
/// recorded-actor gate: the answer is the stored receipt document or the
/// `unknown` status, whichever the row holds. A row whose receipt is
/// still binding (the partial command) is `unknown` here — the durable
/// read never re-executes.
pub async fn billing_command_status(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
) -> Result<BillingCommandStatus, PurchaseError> {
    let Some((_, row_receipt)) = fetch_command_row(connection, tenant, request_id).await? else {
        return Ok(BillingCommandStatus::Unknown);
    };
    let receipt_value = row_receipt.ok_or_else(conflict)?;
    let stored = BillingReceipt::parse(&receipt_value, false).map_err(|_| conflict())?;
    Ok(BillingCommandStatus::Recorded {
        receipt: Box::new(stored),
    })
}

// --- The startup support check ------------------------------------------------------

/// The port of `PostgresBillingRepository.create()`'s checks: the runtime
/// role must be a NON-owner (no super, no bypassrls, no schema ownership
/// of the billing ledger), the migration ledger must match the frozen
/// pair list, and the ledger's eight tenant policies must be forced.
/// Startup support, not an authorization token — the posting ladder
/// belongs to the request path.
///
/// The TS derives the expected pair list from its OWN module's file bytes
/// (`migrate.ts` recomputes at import); this crate consumes the frozen
/// pair list its fixture recorded — the same derivation, the caller
/// supplies it (recorded divergence, ledger-internal).
///
/// Errors are `&'static str` refusals — the TS generic `Error`s. These
/// paths never answer callers; the serving state refuses to come up.
pub async fn assert_billing_kernel(
    pool: &sqlx::PgPool,
    billing_ledger: &[(String, String)],
) -> Result<(), String> {
    let role: Option<(bool,)> = sqlx::query_as(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class \
WHERE relnamespace='archava_billing'::regnamespace AND relowner=pg_roles.oid)) AS unsafe \
FROM pg_roles WHERE rolname=current_user",
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("billing runtime role check failed: {error}"))?;
    if role.map(|(unsafe_,)| unsafe_).unwrap_or(true) {
        return Err("Billing requires a non-owner runtime".to_string());
    }
    let actual: Vec<(String, String)> =
        sqlx::query_as("SELECT id,checksum FROM archava_billing.schema_migrations ORDER BY id")
            .fetch_all(pool)
            .await
            .map_err(|error| format!("unsupported billing ledger read: {error}"))?;
    if actual.len() != billing_ledger.len()
        || actual.iter().zip(billing_ledger).any(|(a, b)| a != b)
    {
        return Err("Unsupported billing migration checksum".to_string());
    }
    let names: &[&str] = &[
        "memberships",
        "invoices",
        "invoice_lines",
        "invoice_corrections",
        "payment_attempts",
        "payment_events",
        "commands",
        "audit",
    ][..];
    let statement = "SELECT count(*)::integer AS count FROM pg_class WHERE \
relnamespace='archava_billing'::regnamespace AND relname=ANY($1::text[]) \
AND relrowsecurity AND relforcerowsecurity";
    let (count,): (i32,) = sqlx::query_as(statement)
        .bind(names)
        .fetch_one(pool)
        .await
        .map_err(|error| format!("billing policy check failed: {error}"))?;
    if count as usize != names.len() {
        return Err("Billing tenant policies must be forced".to_string());
    }
    Ok(())
}
