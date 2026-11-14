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
//! replay gates on the FINGERPRINT CELL alone (a second actor resubmitting
//! an identical `commandId` replays, verbatim); the recorded-actor gate
//! lives on the status read, which hands a receipt only to its initiator.
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
    ManualMethod, SettlementOutcome, BILLING_PERMISSIONS, MANUAL_PROVIDER_PREFIX, SAFE_INTEGER_MAX,
    STATUS_DRAFT, STATUS_ISSUED, STATUS_SETTLED, STATUS_VOID,
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

pub(super) fn forbidden() -> PurchaseError {
    PurchaseError::Domain(ErpError::Forbidden)
}

pub(super) fn conflict() -> PurchaseError {
    PurchaseError::Domain(ErpError::Conflict)
}

fn not_found() -> PurchaseError {
    PurchaseError::Domain(ErpError::NotFound)
}

/// A conflict the adapter decides before the database guard would, named by
/// its catalogue code so the caller learns which rule refused.
pub(super) fn refused(code: &str) -> PurchaseError {
    super::refused_by_code(code)
}

/// Preserve the historical billing driver contract except the installed
/// accounting source guards: a reviewed financial close, an amount beyond the
/// ledger bound and an invoice in another currency than its company's book
/// (trade currency rule 5) are business conflicts, not database failures. So
/// are the two tax guards of Billing 009 a person can meet: no tax code
/// version applies on the invoice date, and a taxed invoice on a book with
/// invoice posting (TAX.md stage B). Lock failures (deadlock, lock timeout)
/// are conflicts as everywhere else; the other conflict states of the shared
/// classifier (`23503`, `23505`, `22003`) stay database failures here.
pub(super) fn refuses(error: sqlx::Error) -> PurchaseError {
    if error.as_database_error().is_some_and(|e| {
        e.code().as_deref() == Some("23514")
            && (e.message() == "Reopen the financial source period before changing its invoice"
                || e.message() == "Invoice currency differs from its company book"
                || e.message()
                    == "No confirmed version of the invoice tax code applies on the invoice date"
                || e.message()
                    == "The invoice tax code is not a confirmed output tax code of the posting book"
                || e.message() == "The tax control account of the invoice tax code is inactive"
                || e.message()
                    == "The tax control account is also an account of the invoice posting policy"
                || e.message() == "Only a taxed invoice can carry a tax invoice reference"
                || e.message()
                    == "A tax invoice reference can be recorded only on an issued or settled invoice"
                || e.message() == "The invoice already has a tax invoice reference"
                || e.message() == "The invoice has no tax invoice reference to replace"
                || MANUAL_RECEIPT_GUARDS.contains(&e.message())
                || e.constraint() == Some("invoice_sources_amount_minor_check"))
    }) {
        // The message guards are catalogued; the amount bound is a column
        // constraint and stays a plain conflict.
        return super::catalogued_refusal(&error).map_or_else(conflict, PurchaseError::Refused);
    }
    // A deadlock (`40P01`) or a lock that was not available (`55P03`) is a
    // race the caller can retry, as in the shared classifier
    // (`purchase_database_error`): a conflict, not a database failure.
    if error
        .as_database_error()
        .is_some_and(|e| matches!(e.code().as_deref(), Some("40P01") | Some("55P03")))
    {
        return conflict();
    }
    PurchaseError::Database {
        ledger: BILLING_LEDGER,
        source: error,
    }
}

/// The catalogued guard messages of Billing 013 (`guard_manual_receipt`). The
/// manual receipt arms decide the same rules first and name the same codes;
/// the guard is what a direct insert, or a race the arm could not see, meets.
const MANUAL_RECEIPT_GUARDS: &[&str] = &[
    "A manual payment event needs its method, reason and recording person",
    "A manual receipt can be recorded only on an issued invoice",
    "A manual receipt cannot exceed the outstanding amount of its invoice",
    "A manual receipt cannot be dated in the future",
    "A manual receipt cannot be dated before its invoice was issued",
    "Close the open online payment before recording a manual receipt",
    "Only a manual receipt of the same invoice can be reversed",
    "A manual receipt reversal cannot exceed the unreversed amount of its receipt",
];

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

/// Whether the invoice has a gateway link the customer could still pay: not
/// closed and nothing received on it.
async fn pending_gateway(
    connection: &mut PgConnection,
    tenant: &str,
    invoice: Uuid,
) -> Result<bool, PurchaseError> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM archava_billing.gateway_links WHERE tenant_id=$1 AND invoice_id=$2 AND closed_at IS NULL AND received_minor=0)")
        .bind(tenant).bind(invoice).fetch_one(connection).await.map_err(refuses)
}

async fn require_no_pending_gateway(
    connection: &mut PgConnection,
    tenant: &str,
    invoice: Uuid,
) -> Result<(), PurchaseError> {
    if pending_gateway(connection, tenant, invoice).await? {
        Err(conflict())
    } else {
        Ok(())
    }
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
    if !own.active || own.kind.as_str() != principal.kind.as_str() {
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
WHERE tenant_id=$1 AND id=$2 AND ($3 OR owner_id=$4) FOR UPDATE";

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
    access: &BillingAccess,
) -> Result<InvoiceRow, PurchaseError> {
    let row = sqlx::query(INVOICE_SELECT_UPDATE)
        .bind(tenant)
        .bind(invoice_id)
        .bind(access.scope == archava_domain::erp::ErpScope::All)
        .bind(&access.owner_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(refuses)?
        .ok_or_else(not_found)?;
    InvoiceRow::try_get(&row, tenant, invoice_id)
}

/// The stored tax rule of a taxed invoice and the tax it carries now: the
/// original tax (`tax_minor`, the tax part of `gross_minor`) and the tax part
/// of its corrections so far.
struct InvoiceTax {
    rule: archava_domain::erp::tax::TaxRule,
    /// `tax_minor + correction_tax_minor`: the tax of the corrected total.
    tax_minor: i64,
    correction_tax_minor: i64,
}

/// The tax snapshot of an invoice the caller has locked, or `None` for an
/// untaxed invoice. Read through jsonb so an install whose Billing ledger
/// predates the tax columns (an owned upgrade fixture) reads "untaxed".
async fn invoice_tax_rule(
    connection: &mut PgConnection,
    tenant: &str,
    invoice: Uuid,
) -> Result<Option<InvoiceTax>, PurchaseError> {
    use archava_domain::erp::tax::{Fraction, TaxRule};
    let stored: serde_json::Value = sqlx::query_scalar(
        "SELECT to_jsonb(i) FROM archava_billing.invoices i WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(invoice)
    .fetch_one(connection)
    .await
    .map_err(refuses)?;
    if stored["tax_code_id"].is_null() {
        return Ok(None);
    }
    let integer = |name: &str| stored[name].as_i64().ok_or_else(conflict);
    let part = |name: &str| integer(name).and_then(|n| i32::try_from(n).map_err(|_| conflict()));
    let rule = TaxRule::new(
        Fraction {
            numerator: part("tax_rate_numerator")?,
            denominator: part("tax_rate_denominator")?,
        },
        Fraction {
            numerator: part("tax_base_numerator")?,
            denominator: part("tax_base_denominator")?,
        },
    )
    .map_err(|_| conflict())?;
    let correction_tax_minor = minor_safe(integer("correction_tax_minor")?)?;
    Ok(Some(InvoiceTax {
        rule,
        tax_minor: minor_safe(integer("tax_minor")?)?
            .checked_add(correction_tax_minor)
            .ok_or_else(conflict)?,
        correction_tax_minor,
    }))
}

// --- The claim + replay plumbing (billing's own SELECT-then-INSERT shape) -------

/// The TS first statement: SELECT the command row by (tenant, request).
/// The row's `actor_id`, `fingerprint` and `receipt` cells are the cells
/// the replay branch reads: the recorded initiator is compared beside the
/// fingerprint, so a receipt only ever replays to the actor that claimed
/// the command (A1 — `actor_id=$3`).
async fn fetch_command_row(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
) -> Result<Option<(String, String, Option<serde_json::Value>)>, PurchaseError> {
    sqlx::query_as(
        "SELECT actor_id,fingerprint,receipt FROM archava_billing.commands \
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
        BillingCommand::CreateInvoiceFromOrder {
            invoice_code,
            order_id,
            ..
        } => {
            format!("Created invoice {invoice_code} from sales order {order_id}")
        }
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
        BillingCommand::RecordManualReceipt {
            amount_minor,
            method,
            received_on,
            reference,
            reason,
            ..
        } => format!(
            "Manual {} receipt of {amount_minor} minor units received on {received_on}, reference \"{reference}\": {reason}",
            method.as_str()
        ),
        BillingCommand::ReverseManualReceipt {
            receipt_event_id,
            amount_minor,
            reason,
            ..
        } => format!(
            "Reversed {amount_minor} minor units of manual receipt {receipt_event_id}: {reason}"
        ),
        // The arm states the full reason, with the value a replacement
        // replaced; this is only the fallback wording.
        BillingCommand::RecordTaxReference {
            reference,
            transaction_code,
            ..
        }
        | BillingCommand::ReplaceTaxReference {
            reference,
            transaction_code,
            ..
        } => format!(
            "Set tax invoice reference {}",
            tax_reference_words(reference, transaction_code.as_deref())
        ),
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
    if let BillingCommand::CreateInvoiceFromOrder { order_id, .. } = command {
        super::billing_invoice::authorize_order(connection, tenant, principal, order_id).await?;
    }
    if principal.kind.as_str() == "human" {
        if let Some(raw) = command.invoice_ref() {
            super::company_trade::authorize(
                connection,
                tenant,
                principal,
                super::company_trade::Record::Invoice,
                parse_uuid_value(raw)?,
            )
            .await?;
        }
    }
    let request_id = parse_uuid_value(command.command_id())?;
    let actor_id = principal.id.as_str();

    // The TS claim pairing — SELECT first, replay or refuse, claim when
    // absent, and fall through on a receipt-less row so a partial
    // command RE-EXECUTES inside this same transaction.
    if let Some((row_actor, row_fingerprint, row_receipt)) =
        fetch_command_row(connection, tenant, request_id).await?
    {
        if row_fingerprint != fingerprint || row_actor != actor_id {
            // `Command payload fingerprint mismatch on replay` — and the
            // recorded initiator: a same-grant rival never receives the
            // receipt another actor claimed.
            return Err(conflict());
        }
        if let Some(receipt_value) = row_receipt {
            // The idempotent replay: the stored document, re-parsed with
            // `idempotencyReplay: true`.
            let stored = BillingReceipt::parse(&receipt_value, true).map_err(|_| conflict())?;
            if let Some(invoice) = &stored.invoice_id {
                invoice_row(connection, tenant, parse_uuid_value(invoice)?, access).await?;
            }
            return Ok(stored);
        }
    } else {
        claim_billing_command(connection, tenant, request_id, actor_id, fingerprint).await?;
    }

    // The sampled clock — one instant per command, bound everywhere.
    let now = observed_now(connection).await?;

    // A tax reference arm states its own audit reason: a replacement names
    // the value it replaced, which the command alone does not carry.
    let mut audit_note = None;
    let receipt = execute_billing_arm(
        connection,
        tenant,
        access,
        request_id,
        command,
        &now,
        &mut audit_note,
    )
    .await?;
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
        Some(audit_note.unwrap_or_else(|| audit_reason(command)).as_str()),
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
    audit_note: &mut Option<String>,
) -> Result<BillingReceipt, PurchaseError> {
    match command {
        BillingCommand::CreateInvoice { .. } | BillingCommand::CreateInvoiceFromOrder { .. } => {
            super::billing_invoice::create(connection, tenant, access, request_id, command, now)
                .await
        }
        BillingCommand::IssueInvoice {
            invoice_id,
            expected_revision,
            ..
        } => {
            let invoice = parse_uuid_value(invoice_id)?;
            let row = invoice_row(connection, tenant, invoice, access).await?;
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
            let row = invoice_row(connection, tenant, invoice, access).await?;
            require_no_pending_gateway(connection, tenant, invoice).await?;
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
            // A taxed invoice splits the correction with its stored rule
            // (TAX.md rule 6); an untaxed one runs the statements it always
            // ran, which also serve an install without the tax columns.
            let tax_part = match invoice_tax_rule(connection, tenant, invoice).await? {
                Some(taxed) => {
                    let (_, tax_part) = taxed
                        .rule
                        .correction(
                            row.gross_minor
                                .checked_add(row.correction_minor)
                                .ok_or_else(conflict)?,
                            taxed.tax_minor,
                            *amount_minor,
                        )
                        .map_err(|_| conflict())?;
                    Some((
                        tax_part,
                        taxed
                            .correction_tax_minor
                            .checked_add(tax_part)
                            .ok_or_else(conflict)?,
                    ))
                }
                None => None,
            };
            let update = if tax_part.is_some() {
                "UPDATE archava_billing.invoices \
SET correction_minor=$1, outstanding_minor=$2, revision=$3, correction_tax_minor=$6 WHERE tenant_id=$4 AND id=$5"
            } else {
                "UPDATE archava_billing.invoices \
SET correction_minor=$1, outstanding_minor=$2, revision=$3 WHERE tenant_id=$4 AND id=$5"
            };
            let update = sqlx::query(update)
                .bind(next_correction)
                .bind(next_outstanding)
                .bind(int4(next_rev)?)
                .bind(tenant)
                .bind(invoice);
            let update = match tax_part {
                Some((_, next_correction_tax)) => update.bind(next_correction_tax),
                None => update,
            };
            update.execute(&mut *connection).await.map_err(refuses)?;

            let correction_id = Uuid::new_v4();
            let insert = if tax_part.is_some() {
                "INSERT INTO archava_billing.invoice_corrections(\
tenant_id,id,invoice_id,amount_minor,reason,reverses_id,recorded_by,created_at,tax_minor) \
VALUES($1,$2,$3,$4,$5,$6,$7,$8::timestamptz,$9)"
            } else {
                "INSERT INTO archava_billing.invoice_corrections(\
tenant_id,id,invoice_id,amount_minor,reason,reverses_id,recorded_by,created_at) \
VALUES($1,$2,$3,$4,$5,$6,$7,$8::timestamptz)"
            };
            let insert = sqlx::query(insert)
                .bind(tenant)
                .bind(correction_id)
                .bind(invoice)
                .bind(*amount_minor)
                .bind(reason)
                .bind(reverses_id)
                .bind(&access.owner_id)
                .bind(now);
            let insert = match tax_part {
                Some((tax_part, _)) => insert.bind(tax_part),
                None => insert,
            };
            insert.execute(&mut *connection).await.map_err(refuses)?;

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
            let row = invoice_row(connection, tenant, invoice, access).await?;
            require_no_pending_gateway(connection, tenant, invoice).await?;
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
            let row = invoice_row(connection, tenant, invoice, access).await?;
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
            let row = invoice_row(connection, tenant, invoice, access).await?;
            // `unbalanced_settlement` — `Provider event currency does not
            // match the invoiced currency`.
            if row.currency.as_str() != currency.as_str() {
                return Err(conflict());
            }

            // `manual:` is the namespace of manual receipts. The parse refuses
            // it for a submitted provider event; a caller that builds the
            // command itself (the gateway reconciler) is held to it here.
            if provider_id.starts_with(MANUAL_PROVIDER_PREFIX) {
                return Err(conflict());
            }

            // The TS settlement ladder: `nextSettled`, `nextStatus` and
            // the receipt's own `statusLabel` — started FROM the row. Every
            // refusal of the ladder is the same conflict on this arm.
            let settlement =
                settlement_of(&row, *outcome, *amount_minor).map_err(|_| conflict())?;
            let outstanding = settlement.outstanding;
            let status_label = settlement.status_label.clone();
            let moves_balance = settlement.moves_balance;
            apply_settlement(connection, tenant, invoice, &row, &settlement, now).await?;

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

        BillingCommand::RecordManualReceipt {
            invoice_id,
            expected_revision,
            amount_minor,
            method,
            received_on,
            reference,
            reason,
            ..
        } => {
            manual_receipt(
                connection,
                tenant,
                access,
                request_id,
                now,
                invoice_id,
                *expected_revision,
                *amount_minor,
                *method,
                received_on,
                reference,
                reason,
            )
            .await
        }
        BillingCommand::ReverseManualReceipt {
            invoice_id,
            expected_revision,
            receipt_event_id,
            amount_minor,
            reason,
            ..
        } => {
            manual_reversal(
                connection,
                tenant,
                access,
                request_id,
                now,
                invoice_id,
                *expected_revision,
                receipt_event_id,
                *amount_minor,
                reason,
            )
            .await
        }

        BillingCommand::RecordTaxReference {
            invoice_id,
            expected_revision,
            reference,
            transaction_code,
            ..
        } => {
            tax_reference(
                connection,
                tenant,
                access,
                request_id,
                now,
                audit_note,
                TaxReferenceChange {
                    action: "record_tax_reference",
                    invoice_id,
                    expected_revision: *expected_revision,
                    reference,
                    transaction_code: transaction_code.as_deref(),
                    replace_reason: None,
                },
            )
            .await
        }
        BillingCommand::ReplaceTaxReference {
            invoice_id,
            expected_revision,
            reference,
            transaction_code,
            reason,
            ..
        } => {
            tax_reference(
                connection,
                tenant,
                access,
                request_id,
                now,
                audit_note,
                TaxReferenceChange {
                    action: "replace_tax_reference",
                    invoice_id,
                    expected_revision: *expected_revision,
                    reference,
                    transaction_code: transaction_code.as_deref(),
                    replace_reason: Some(reason),
                },
            )
            .await
        }
    }
}

// --- The settlement ladder (provider events and manual receipts) -----------------

/// Which rule of the settlement ladder refused. A provider event answers
/// every one as the same conflict; a manual receipt names its reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettlementRefusal {
    /// `A ${status} invoice cannot accept a successful settlement`.
    NotIssued,
    /// `Settlement exceeds the outstanding balance`.
    ExceedsOutstanding,
    /// `Refund exceeds the amount already settled`.
    ExceedsSettled,
    /// The sum left the 64-bit range.
    Overflow,
}

/// What one payment event does to its invoice.
struct Settlement {
    next_settled: i64,
    next_status: String,
    /// The receipt's own label: the invoice status, or `failed`.
    status_label: String,
    outstanding: i64,
    /// False only for a failed event, which touches nothing.
    moves_balance: bool,
}

/// The settlement arithmetic, the one place it is written: `succeeded` needs
/// an issued invoice, adds to what is settled, may leave a remainder and never
/// takes the balance below zero, and settles the invoice at zero; `refunded`
/// subtracts no more than is settled and reopens a settled invoice; `failed`
/// is history that moves nothing.
fn settlement_of(
    row: &InvoiceRow,
    outcome: SettlementOutcome,
    amount_minor: i64,
) -> Result<Settlement, SettlementRefusal> {
    let mut next_settled = row.settled_minor;
    let mut next_status = row.status.clone();
    let mut status_label = row.status.clone();
    match outcome {
        SettlementOutcome::Succeeded => {
            if row.status != STATUS_ISSUED {
                return Err(SettlementRefusal::NotIssued);
            }
            next_settled = row
                .settled_minor
                .checked_add(amount_minor)
                .ok_or(SettlementRefusal::Overflow)?;
            let next_outstanding = row.outstanding(row.correction_minor, next_settled);
            if next_outstanding < 0 {
                return Err(SettlementRefusal::ExceedsOutstanding);
            }
            if next_outstanding == 0 {
                next_status = STATUS_SETTLED.to_string();
                status_label = STATUS_SETTLED.to_string();
            }
        }
        SettlementOutcome::Refunded => {
            if row.settled_minor < amount_minor {
                return Err(SettlementRefusal::ExceedsSettled);
            }
            next_settled = row.settled_minor - amount_minor;
            // A refund REOPENS a settled invoice.
            if row.status == STATUS_SETTLED {
                next_status = STATUS_ISSUED.to_string();
                status_label = STATUS_ISSUED.to_string();
            }
        }
        // `failed` — history only: the receipt's own label, and the invoice
        // row is NOT even touched (no revision).
        SettlementOutcome::Failed => {
            status_label = "failed".to_string();
        }
    }
    Ok(Settlement {
        outstanding: row.outstanding(row.correction_minor, next_settled),
        next_settled,
        next_status,
        status_label,
        moves_balance: outcome != SettlementOutcome::Failed,
    })
}

/// Writes a settlement to its locked invoice row: the new settled and
/// outstanding amounts, the status and the next revision. A failed event
/// moves no money and no state, so it never touches the row.
async fn apply_settlement(
    connection: &mut PgConnection,
    tenant: &str,
    invoice: Uuid,
    row: &InvoiceRow,
    settlement: &Settlement,
    now: &str,
) -> Result<(), PurchaseError> {
    if !settlement.moves_balance {
        return Ok(());
    }
    let updated = sqlx::query(
        "UPDATE archava_billing.invoices \
SET settled_minor=$1, outstanding_minor=$2, status=$3, revision=$4, \
settled_at=CASE WHEN $3='settled' THEN $5::timestamptz ELSE settled_at END \
WHERE tenant_id=$6 AND id=$7",
    )
    .bind(settlement.next_settled)
    .bind(settlement.outstanding)
    .bind(&settlement.next_status)
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
    Ok(())
}

// --- Manual receipts -------------------------------------------------------------

/// A manual receipt: a person with `billing.settle` states that money for an
/// issued invoice arrived by bank transfer or in cash. It settles through the
/// same ladder as a succeeded provider event and stores a payment event under
/// the `manual:` provider id with who, when, method, reference and reason
/// (Billing 013). The rules are the guard's, decided here so each refusal
/// names its reason.
#[allow(clippy::too_many_arguments)]
async fn manual_receipt(
    connection: &mut PgConnection,
    tenant: &str,
    access: &BillingAccess,
    request_id: Uuid,
    now: &str,
    invoice_id: &str,
    expected_revision: i64,
    amount_minor: i64,
    method: ManualMethod,
    received_on: &str,
    reference: &str,
    reason: &str,
) -> Result<BillingReceipt, PurchaseError> {
    let invoice = parse_uuid_value(invoice_id)?;
    let row = invoice_row(connection, tenant, invoice, access).await?;
    if row.revision != expected_revision {
        return Err(conflict());
    }
    // A settled invoice owes nothing: any amount is more than is outstanding.
    // A draft or void invoice is not one that can be paid at all.
    if row.status == STATUS_SETTLED {
        return Err(refused("manual_receipt_exceeds_outstanding"));
    }
    let settlement =
        settlement_of(&row, SettlementOutcome::Succeeded, amount_minor).map_err(|refusal| {
            match refusal {
                SettlementRefusal::NotIssued => refused("manual_receipt_not_issued"),
                SettlementRefusal::ExceedsOutstanding => {
                    refused("manual_receipt_exceeds_outstanding")
                }
                SettlementRefusal::ExceedsSettled | SettlementRefusal::Overflow => conflict(),
            }
        })?;
    // The received-on date is a UTC calendar date: not after the day of this
    // command and not before the day the invoice was issued.
    let (future, before_issue): (bool, bool) = sqlx::query_as(
        "SELECT $3::date>($4::timestamptz AT TIME ZONE 'UTC')::date,\
issued_at IS NULL OR $3::date<(issued_at AT TIME ZONE 'UTC')::date \
FROM archava_billing.invoices WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(invoice)
    .bind(received_on)
    .bind(now)
    .fetch_one(&mut *connection)
    .await
    .map_err(refuses)?;
    if future {
        return Err(refused("manual_receipt_future_date"));
    }
    if before_issue {
        return Err(refused("manual_receipt_before_issue"));
    }
    // While the customer can still pay the frozen amount online, a manual
    // receipt could settle the same money twice.
    if pending_gateway(connection, tenant, invoice).await? {
        return Err(refused("manual_receipt_open_payment_link"));
    }
    apply_settlement(connection, tenant, invoice, &row, &settlement, now).await?;
    let event_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO archava_billing.payment_events(\
tenant_id,id,invoice_id,provider_id,provider_event_id,outcome,amount_minor,currency,recorded_by,verified_at,\
manual_method,received_on,reference,reason) \
VALUES($1,$2,$3,$4,$5,'succeeded',$6,$7,$8,$9::timestamptz,$10,$11::date,$12,$13)",
    )
    .bind(tenant)
    .bind(event_id)
    .bind(invoice)
    .bind(method.provider_id())
    .bind(format!("{MANUAL_PROVIDER_PREFIX}{event_id}"))
    .bind(amount_minor)
    .bind(&row.currency)
    .bind(&access.owner_id)
    .bind(now)
    .bind(method.as_str())
    .bind(received_on)
    .bind(reference)
    .bind(reason)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(BillingReceipt {
        command_id: request_id.to_string(),
        tenant_id: tenant.to_string(),
        action: "record_manual_receipt",
        invoice_id: Some(invoice.to_string()),
        line_id: None,
        correction_id: None,
        attempt_id: None,
        event_id: Some(event_id.to_string()),
        status: settlement.status_label,
        outstanding_minor: Some(settlement.outstanding),
        revision: Some(row.revision + 1),
        idempotency_replay: false,
        executed_at: now.to_string(),
    })
}

/// Reverses all or part of one manual receipt of the invoice: a payment event
/// with outcome `refunded` under the receipt's own `manual:` provider id,
/// naming the receipt it reverses. It runs the refund side of the settlement
/// ladder, so it reopens a settled invoice. A gateway event is never reversed
/// here, and the reversals of one receipt never exceed it.
#[allow(clippy::too_many_arguments)]
async fn manual_reversal(
    connection: &mut PgConnection,
    tenant: &str,
    access: &BillingAccess,
    request_id: Uuid,
    now: &str,
    invoice_id: &str,
    expected_revision: i64,
    receipt_event_id: &str,
    amount_minor: i64,
    reason: &str,
) -> Result<BillingReceipt, PurchaseError> {
    let invoice = parse_uuid_value(invoice_id)?;
    let original_id = parse_uuid_value(receipt_event_id)?;
    let row = invoice_row(connection, tenant, invoice, access).await?;
    if row.revision != expected_revision {
        return Err(conflict());
    }
    let original: Option<(Uuid, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT invoice_id,outcome,amount_minor,manual_method FROM archava_billing.payment_events \
WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(original_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)?;
    let (original_invoice, outcome, original_amount, method) = original.ok_or_else(not_found)?;
    let method = method.as_deref().and_then(ManualMethod::parse);
    let Some(method) = method.filter(|_| original_invoice == invoice && outcome == "succeeded")
    else {
        return Err(refused("manual_reversal_not_manual"));
    };
    // The invoice row is locked, so the reversals of its receipts are serial.
    let reversed: i64 = sqlx::query_scalar(
        "SELECT coalesce(sum(amount_minor),0)::bigint FROM archava_billing.payment_events \
WHERE tenant_id=$1 AND reverses_event_id=$2",
    )
    .bind(tenant)
    .bind(original_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(refuses)?;
    if reversed
        .checked_add(amount_minor)
        .is_none_or(|total| total > original_amount)
    {
        return Err(refused("manual_reversal_exceeds_receipt"));
    }
    let settlement =
        settlement_of(&row, SettlementOutcome::Refunded, amount_minor).map_err(|_| conflict())?;
    apply_settlement(connection, tenant, invoice, &row, &settlement, now).await?;
    let event_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO archava_billing.payment_events(\
tenant_id,id,invoice_id,provider_id,provider_event_id,outcome,amount_minor,currency,recorded_by,verified_at,\
manual_method,reason,reverses_event_id) \
VALUES($1,$2,$3,$4,$5,'refunded',$6,$7,$8,$9::timestamptz,$10,$11,$12)",
    )
    .bind(tenant)
    .bind(event_id)
    .bind(invoice)
    .bind(method.provider_id())
    .bind(format!("{MANUAL_PROVIDER_PREFIX}{event_id}"))
    .bind(amount_minor)
    .bind(&row.currency)
    .bind(&access.owner_id)
    .bind(now)
    .bind(method.as_str())
    .bind(reason)
    .bind(original_id)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(BillingReceipt {
        command_id: request_id.to_string(),
        tenant_id: tenant.to_string(),
        action: "reverse_manual_receipt",
        invoice_id: Some(invoice.to_string()),
        line_id: None,
        correction_id: None,
        attempt_id: None,
        event_id: Some(event_id.to_string()),
        status: settlement.status_label,
        outstanding_minor: Some(settlement.outstanding),
        revision: Some(row.revision + 1),
        idempotency_replay: false,
        executed_at: now.to_string(),
    })
}

/// One change of an invoice's tax invoice reference: the first recording
/// (`replace_reason` is none) or a replacement.
struct TaxReferenceChange<'a> {
    action: &'static str,
    invoice_id: &'a str,
    expected_revision: i64,
    reference: &'a str,
    transaction_code: Option<&'a str>,
    replace_reason: Option<&'a str>,
}

/// A reference and its optional transaction code as the audit trail words it.
fn tax_reference_words(reference: &str, transaction_code: Option<&str>) -> String {
    match transaction_code {
        Some(code) => format!("\"{reference}\" (transaction code \"{code}\")"),
        None => format!("\"{reference}\" (no transaction code)"),
    }
}

/// Records or replaces the tax invoice reference of a taxed, issued or settled
/// invoice (Billing 011). The reference is text issued outside Archava and
/// stored as entered. The rules are the guard's, decided here so each refusal
/// names its reason. The invoice keeps only the current value; the audit row of
/// every command states the value it set, and a replacement also states the
/// value it replaced, so the history is the immutable audit trail.
async fn tax_reference(
    connection: &mut PgConnection,
    tenant: &str,
    access: &BillingAccess,
    request_id: Uuid,
    now: &str,
    audit_note: &mut Option<String>,
    change: TaxReferenceChange<'_>,
) -> Result<BillingReceipt, PurchaseError> {
    let invoice = parse_uuid_value(change.invoice_id)?;
    let row = invoice_row(connection, tenant, invoice, access).await?;
    if row.revision != change.expected_revision {
        return Err(conflict());
    }
    let (taxed, held, held_code, version): (bool, Option<String>, Option<String>, i32) = sqlx::query_as(
        "SELECT tax_code_id IS NOT NULL,tax_reference,tax_transaction_code,tax_reference_version \
FROM archava_billing.invoices WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(invoice)
    .fetch_one(&mut *connection)
    .await
    .map_err(refuses)?;
    if !taxed {
        return Err(refused("invoice_tax_reference_untaxed"));
    }
    if !matches!(row.status.as_str(), STATUS_ISSUED | STATUS_SETTLED) {
        return Err(refused("invoice_tax_reference_not_issued"));
    }
    let set = tax_reference_words(change.reference, change.transaction_code);
    let note = match (change.replace_reason, held.as_deref()) {
        (None, Some(_)) => return Err(refused("invoice_tax_reference_already_recorded")),
        (Some(_), None) => return Err(refused("invoice_tax_reference_missing")),
        (None, None) => format!("Recorded tax invoice reference {set}"),
        (Some(reason), Some(previous)) => {
            // Replacing a value with itself changes nothing to keep.
            if previous == change.reference && held_code.as_deref() == change.transaction_code {
                return Err(conflict());
            }
            format!(
                "Replaced tax invoice reference {} with {set}: {reason}",
                tax_reference_words(previous, held_code.as_deref())
            )
        }
    };
    let next_rev = row.revision + 1;
    sqlx::query(
        "UPDATE archava_billing.invoices SET tax_reference=$1,tax_transaction_code=$2,\
tax_reference_version=$3,revision=$4 WHERE tenant_id=$5 AND id=$6",
    )
    .bind(change.reference)
    .bind(change.transaction_code)
    .bind(version.checked_add(1).ok_or_else(conflict)?)
    .bind(int4(next_rev)?)
    .bind(tenant)
    .bind(invoice)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    *audit_note = Some(note);
    Ok(BillingReceipt {
        command_id: request_id.to_string(),
        tenant_id: tenant.to_string(),
        action: change.action,
        invoice_id: Some(invoice.to_string()),
        line_id: None,
        correction_id: None,
        attempt_id: None,
        event_id: None,
        status: row.status.clone(),
        outstanding_minor: Some(row.outstanding_minor),
        revision: Some(next_rev),
        idempotency_replay: false,
        executed_at: now.to_string(),
    })
}

// --- commandStatus ---------------------------------------------------------------

/// `getCommandStatus()` — gated on the current `billing.read` grant and
/// bound to the recorded initiator: the stored receipt document goes only to
/// the principal that recorded the command, and every other caller —
/// including a grant holder naming a foreign command — gets the bare
/// `unknown` status with no receipt fields. A row whose receipt is still
/// binding (the partial command) is `unknown` here — the durable read never
/// re-executes.
pub async fn billing_command_status(
    connection: &mut PgConnection,
    tenant: &str,
    access: &BillingAccess,
    request_id: Uuid,
) -> Result<BillingCommandStatus, PurchaseError> {
    // The read permission, then the recorded initiator: a receipt is the
    // initiating actor's own. `actor_id` rides the WHERE clause, so another
    // actor's key is indistinguishable from a missing one — the bare
    // `unknown` document, no receipt fields.
    grant(access, "billing.read")?;
    let row: Option<(String, Option<serde_json::Value>)> = sqlx::query_as(
        "SELECT actor_id,receipt FROM archava_billing.commands \
         WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(access.owner_id.as_str())
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)?;
    let Some((_, receipt_value)) = row else {
        return Ok(BillingCommandStatus::Unknown);
    };
    let receipt_value = receipt_value.ok_or_else(conflict)?;
    let stored = BillingReceipt::parse(&receipt_value, false).map_err(|_| conflict())?;
    if let Some(raw) = &stored.invoice_id {
        super::company_trade::authorize_human(
            connection,
            tenant,
            &access.owner_id,
            super::company_trade::Record::Invoice,
            parse_uuid_value(raw)?,
        )
        .await?;
    }
    if access.scope != archava_domain::erp::ErpScope::All {
        if let Some(invoice) = &stored.invoice_id {
            let visible:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM archava_billing.invoices WHERE tenant_id=$1 AND id=$2 AND owner_id=$3)")
                .bind(tenant).bind(parse_uuid_value(invoice)?).bind(&access.owner_id).fetch_one(&mut *connection).await.map_err(refuses)?;
            if !visible {
                return Ok(BillingCommandStatus::Unknown);
            }
        }
    }
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
        "gateway_links",
        "gateway_runtime",
        "gateway_observations",
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

/// The payment events of invoice `i`, oldest first: what settled it and what
/// was taken back. `origin` is `manual` for a manual receipt or its reversal
/// (a person's statement, with `method`, `receivedOn`, `reference`, `reason`
/// and who recorded it) and `gateway` for a provider event verified by its
/// signature or by the payment gateway. `reversedAmount` is what the reversals
/// of a manual receipt have taken back so far. The Billing 013 columns are read
/// through jsonb, so an install whose ledger predates them lists every event as
/// a gateway event.
const PAYMENTS_PROJECTION: &str = "coalesce((SELECT json_agg(json_build_object(\
'id',e.id,'origin',CASE WHEN to_jsonb(e)->>'manual_method' IS NULL THEN 'gateway' ELSE 'manual' END,\
'method',to_jsonb(e)->'manual_method','providerId',e.provider_id,'outcome',e.outcome,\
'amount',e.amount_minor,'currency',e.currency,'recordedAt',e.verified_at,\
'recordedOn',(e.verified_at AT TIME ZONE 'UTC')::date,'receivedOn',to_jsonb(e)->'received_on',\
'reference',to_jsonb(e)->'reference','reason',to_jsonb(e)->'reason','recordedBy',e.recorded_by,\
'recordedByName',(SELECT nullif(m.display_name,'') FROM archava_billing.memberships m WHERE m.tenant_id=e.tenant_id AND m.principal_id=e.recorded_by),\
'reversesEventId',to_jsonb(e)->'reverses_event_id',\
'reversedAmount',(SELECT coalesce(sum(r.amount_minor),0) FROM archava_billing.payment_events r WHERE r.tenant_id=e.tenant_id AND r.invoice_id=e.invoice_id AND to_jsonb(r)->>'reverses_event_id'=e.id::text),\
'verifiedByProvider',to_jsonb(e)->>'manual_method' IS NULL) ORDER BY e.verified_at,e.id) \
FROM archava_billing.payment_events e WHERE e.tenant_id=i.tenant_id AND e.invoice_id=i.id),'[]'::json)";

/// Invoice balances and immutable line snapshots, scoped to the resolved reader.
pub async fn workspace_view(
    connection: &mut PgConnection,
    tenant: &str,
    access: &BillingAccess,
    search: &str,
) -> Result<serde_json::Value, PurchaseError> {
    workspace_view_scoped(connection, tenant, access, search, None).await
}
pub async fn workspace_view_scoped(
    connection: &mut PgConnection,
    tenant: &str,
    access: &BillingAccess,
    search: &str,
    company: Option<Uuid>,
) -> Result<serde_json::Value, PurchaseError> {
    workspace_view_filtered(connection, tenant, access, search, company, None).await
}
pub async fn workspace_view_filtered(
    connection: &mut PgConnection,
    tenant: &str,
    access: &BillingAccess,
    search: &str,
    company: Option<Uuid>,
    id: Option<Uuid>,
) -> Result<serde_json::Value, PurchaseError> {
    if !access.holds("billing.read") {
        return Err(forbidden());
    }
    let statement=[r#"SELECT json_build_object('id',i.id,'code',i.invoice_code,'customer',i.customer_ref,'companyId',to_jsonb(i)->'company_id','companyRevision',to_jsonb(i)->'company_revision','companyCode',to_jsonb(i)->'company_code','companyName',to_jsonb(i)->'company_name','currency',i.currency,'status',i.status,'gross',i.gross_minor,'correction',i.correction_minor,'settled',i.settled_minor,'outstanding',i.outstanding_minor,'taxMinor',coalesce((to_jsonb(i)->>'tax_minor')::bigint,0),'netMinor',i.gross_minor-coalesce((to_jsonb(i)->>'tax_minor')::bigint,0),'correctionTaxMinor',coalesce((to_jsonb(i)->>'correction_tax_minor')::bigint,0),'tax',CASE WHEN to_jsonb(i)->>'tax_code_id' IS NOT NULL THEN json_build_object('codeId',to_jsonb(i)->'tax_code_id','code',to_jsonb(i)->'tax_code','version',to_jsonb(i)->'tax_code_version','kind',to_jsonb(i)->'tax_kind','rate',json_build_object('numerator',to_jsonb(i)->'tax_rate_numerator','denominator',to_jsonb(i)->'tax_rate_denominator'),'base',json_build_object('numerator',to_jsonb(i)->'tax_base_numerator','denominator',to_jsonb(i)->'tax_base_denominator'),'priceIncludesTax',to_jsonb(i)->'price_includes_tax','enteredByCompany',true,'verifiedByArchava',false) END,'sellerTaxId',to_jsonb(i)->'seller_tax_id','buyerTaxId',to_jsonb(i)->'buyer_tax_id','taxReference',CASE WHEN to_jsonb(i)->>'tax_reference' IS NOT NULL THEN json_build_object('reference',to_jsonb(i)->'tax_reference','transactionCode',to_jsonb(i)->'tax_transaction_code','version',to_jsonb(i)->'tax_reference_version','enteredByCompany',true,'verifiedByArchava',false) END,'revision',i.revision,'consumerRef',i.consumer_ref,'customerContactId',i.customer_contact_id,'customerContactRevision',i.customer_contact_revision,'customerPartyId',i.customer_party_id,'customerPartyRevision',i.customer_party_revision,'sourceOrderId',i.source_order_id,'sourceOrderRevision',i.source_order_revision,'issuedOn',(i.issued_at AT TIME ZONE 'UTC')::date,'payments',"#,PAYMENTS_PROJECTION,r#",'lines',coalesce((SELECT json_agg(json_build_object('id',l.id,'sourceOrderLineId',l.source_order_line_id,'description',l.description,'quantity',l.quantity,'unitAmount',l.unit_minor,'total',l.line_total_minor) ORDER BY l.id) FROM archava_billing.invoice_lines l WHERE l.tenant_id=i.tenant_id AND l.invoice_id=i.id),'[]'::json)) FROM archava_billing.invoices i WHERE i.tenant_id=$1 AND (to_jsonb(i)->>'company_id')::uuid IS NOT DISTINCT FROM $5::uuid AND ($2 OR i.owner_id=$3) AND ($6::uuid IS NULL OR i.id=$6) AND ($4='' OR strpos(lower(i.invoice_code||' '||i.customer_ref),lower($4))>0) ORDER BY i.created_at DESC,i.id LIMIT 100"#].concat();
    let rows = sqlx::query_scalar::<_, serde_json::Value>(sqlx::AssertSqlSafe(statement.as_str()))
        .bind(tenant)
        .bind(access.scope == archava_domain::erp::ErpScope::All)
        .bind(&access.owner_id)
        .bind(search)
        .bind(company)
        .bind(id)
        .fetch_all(&mut *connection)
        .await
        .map_err(|e| super::purchase_database_error(BILLING_LEDGER, e))?;
    // The UTC calendar date of the database clock: the default received-on
    // date of a manual receipt and the latest one it accepts.
    let today: String =
        sqlx::query_scalar("SELECT (clock_timestamp() AT TIME ZONE 'UTC')::date::text")
            .fetch_one(&mut *connection)
            .await
            .map_err(|e| super::purchase_database_error(BILLING_LEDGER, e))?;
    Ok(
        serde_json::json!({"invoices":rows,"taxNotice":archava_domain::erp::tax::DOCUMENT_NOTICE,"today":today,"pageLimit":100,"permissions":access.permissions,"scope":access.scope}),
    )
}
