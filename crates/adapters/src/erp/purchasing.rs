//! The purchasing repository — the port of
//! `packages/purchasing-pg/src/repository.ts`, split into step functions the
//! host drives from ONE transaction.
//!
//! ## What is ported, in order
//!
//! - `transaction()`'s member ladder (both ledgers, the service narrow, the
//!   `catalog.read` floor, the commerce-narrows-purchasing scope rule) —
//!   [`resolve_purchasing_context`].
//! - `authorize()` — the domain's [`authorize_operation`], reapplied by
//!   recovery exactly like `recovery()` re-runs the ladder.
//! - `mutate()` — [`record_purchase_command`]: claim → execute → audit →
//!   command-receipt update, in the SAME statement order, so the deferred
//!   constraint triggers see the same states in the same order at commit.
//! - `quote()` / `writeLines()` / `ownedOrder()` — the shared guards, with
//!   the same lock styles: supplier `FOR SHARE`, offers `FOR SHARE OF o`,
//!   commerce products `FOR UPDATE`.
//! - `execute()` — every branch, with the same conflict conditions, and the
//!   receive branch calling
//!   [`receive_purchase_stock`](super::commerce_kernel::receive_purchase_stock)
//!   — the cross-ledger stock write happens inside this ONE transaction.
//!   That is the R06 single-stock contract.
//! - `recovery()` — [`recover_from_row`], `commandStatus()` —
//!   [`command_status`].
//!
//! ## Posture reminders
//!
//! Every statement runs on a `checked_executor` inside a transaction opened
//! by the host with `SET LOCAL` timeouts. SQL here binds the typed tenant on
//! EVERY statement — RLS's `USING(current_setting(...))` plus this bind is
//! what makes a cross-tenant read impossible. Error mapping follows the TS
//! catch: the whitelisted SQL states are conflicts, everything else is a
//! database failure.

use archava_domain::erp::purchasing::{
    authorize_operation, authorize_recovery, replayed_receipt, CommerceAccess, LineInput,
    PrincipalKind, PurchaseCommand, PurchaseContext, PurchaseMutation, PurchasePrincipal,
    PurchaseReceipt, COST_MAX, PERMISSION_VOCABULARY, REVISION_MAX, TOTAL_MAX,
};
use archava_domain::erp::stock::InboundLine;
use archava_domain::erp::{valid_uuid, ErpError, ErpMembershipKind, ErpScope};
use sqlx::types::Uuid;
use sqlx::PgConnection;
use sqlx::Row;

use super::commerce_kernel::{load_locked_membership, receive_purchase_stock, ReceiveStockInput};
use super::{
    purchase_database_error, PurchaseError, COMMERCE_LEDGER, COMMERCE_NAMESPACE,
    COMMERCE_PERMISSION_VOCABULARY, PURCHASING_LEDGER, PURCHASING_NAMESPACE,
};

/// The purchasing ledger's tenant-scoped tables — the nine the trigger set
/// protects and the forced-RLS gate counts at startup readiness.
pub const TABLES: &[&str] = &[
    "memberships",
    "suppliers",
    "offers",
    "orders",
    "lines",
    "receipts",
    "receipt_lines",
    "supplier_documents",
    "supplier_document_versions",
    "supplier_document_allocations",
    "commands",
    "audit",
];

/// The driver-error mapper bound to this ledger.
fn refuses(error: sqlx::Error) -> PurchaseError {
    purchase_database_error(PURCHASING_LEDGER, error)
}

/// The catalogue codes (`config/refusal-reasons.v1.json`) of the two currency
/// rules this adapter decides before their Purchasing 007 guards would.
const LINE_CURRENCY_MISMATCH: &str = "purchase_line_currency_mismatch";
const OFFER_CURRENCY_MISMATCH: &str = "offer_currency_differs_from_product";

/// A conflict-mapped `i32` conversion for int4 columns whose values the
/// contract bounds strictly below `i32::MAX` (the callers validate the
/// ceiling before reaching this point; anything larger is corrupt state).
fn int4(value: i64) -> Result<i32, PurchaseError> {
    if value < 0 || value > i32::MAX as i64 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    Ok(value as i32)
}

/// The plain-uuid parse the zod `.uuid()` scopes apply on the stored/untyped
/// strings crossing this boundary.
fn parse_uuid(value: &str) -> Result<Uuid, PurchaseError> {
    Uuid::parse_str(value).map_err(|_| PurchaseError::Domain(ErpError::Conflict))
}

// --- Row grammar (the TS `supplier`/`offer`/`order`/`line` parsers) ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupplierRow {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub active: bool,
    pub revision: i64,
}

fn parse_supplier_row(
    row: &sqlx::postgres::PgRow,
    tenant: &str,
) -> Result<SupplierRow, PurchaseError> {
    let stored_tenant: String = row.try_get("tenant_id").map_err(refuses)?;
    if stored_tenant != tenant {
        // RLS makes this unreachable in a correct transaction; a mismatch is
        // a cohort failure, not a caller choice.
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let code: String = row.try_get("code").map_err(refuses)?;
    if !archava_domain::erp::valid_code(&code) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let name: String = row.try_get("name").map_err(refuses)?;
    if name.trim_matches(|c: char| c.is_whitespace()).is_empty() || name.chars().count() > 160 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    Ok(SupplierRow {
        id: row.try_get("id").map_err(refuses)?,
        code,
        name,
        active: row.try_get("active").map_err(refuses)?,
        revision: i64::from(row.try_get::<i32, _>("revision").map_err(refuses)?),
    })
}

/// A bare offer row — the offer-mutation branches load it without the
/// product join.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferRow {
    pub id: Uuid,
    pub supplier_id: Uuid,
    pub product_id: Uuid,
    pub unit_cost: i64,
    pub currency: String,
    pub active: bool,
    pub revision: i64,
}

fn parse_offer_row(row: &sqlx::postgres::PgRow, tenant: &str) -> Result<OfferRow, PurchaseError> {
    let stored_tenant: String = row.try_get("tenant_id").map_err(refuses)?;
    if stored_tenant != tenant {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let unit_cost = i64::from(row.try_get::<i32, _>("unit_cost").map_err(refuses)?);
    if !(0..=COST_MAX).contains(&unit_cost) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let currency: String = row.try_get("currency").map_err(refuses)?;
    if !super::commerce_kernel::valid_currency_code(&currency) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let revision = i64::from(row.try_get::<i32, _>("revision").map_err(refuses)?);
    if revision < 1 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    Ok(OfferRow {
        id: row.try_get("id").map_err(refuses)?,
        supplier_id: row.try_get("supplier_id").map_err(refuses)?,
        product_id: row.try_get("product_id").map_err(refuses)?,
        unit_cost,
        currency,
        active: row.try_get("active").map_err(refuses)?,
        revision,
    })
}

/// An offer row with the joined product fields the TS `offerSelect` carries
/// (`SELECT o.*,p.sku,p.title,p.unit FROM offers o JOIN products p ...`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferJoinedRow {
    pub id: Uuid,
    pub supplier_id: Uuid,
    pub product_id: Uuid,
    pub unit_cost: i64,
    pub currency: String,
    pub active: bool,
    pub revision: i64,
    pub sku: String,
    pub title: String,
    pub unit: String,
}

fn parse_offer_joined_row(
    row: &sqlx::postgres::PgRow,
    tenant: &str,
) -> Result<OfferJoinedRow, PurchaseError> {
    let bare = parse_offer_row(row, tenant)?;
    // The join fields come from the product row; the kernel's product grammar
    // validated the same three columns inside the lock. Here they only ride
    // into the frozen line snapshot.
    Ok(OfferJoinedRow {
        id: bare.id,
        supplier_id: bare.supplier_id,
        product_id: bare.product_id,
        unit_cost: bare.unit_cost,
        currency: bare.currency,
        active: bare.active,
        revision: bare.revision,
        sku: row.try_get("sku").map_err(refuses)?,
        title: row.try_get("title").map_err(refuses)?,
        unit: row.try_get("unit").map_err(refuses)?,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseOrderRow {
    pub id: Uuid,
    pub owner_id: String,
    pub supplier_id: Uuid,
    pub supplier_code: String,
    pub supplier_name: String,
    pub state: String,
    pub total_estimate: i64,
    pub currency: String,
    pub confirmed_at: Option<String>,
    pub revision: i64,
}

fn parse_order_row(
    row: &sqlx::postgres::PgRow,
    tenant: &str,
) -> Result<PurchaseOrderRow, PurchaseError> {
    let stored_tenant: String = row.try_get("tenant_id").map_err(refuses)?;
    if stored_tenant != tenant {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let state: String = row.try_get("state").map_err(refuses)?;
    if !archava_domain::erp::purchasing::ORDER_STATES.contains(&state.as_str()) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    // `total_estimate` is int8 — the quote bound (5e14) fits int4 rows not at
    // all: it MUST decode as i64 or a legitimate nine-digit-cent order
    // breaks the read.
    let total_estimate: i64 = row.try_get("total_estimate").map_err(refuses)?;
    if !(0..=TOTAL_MAX).contains(&total_estimate) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let supplier_code: String = row.try_get("supplier_code").map_err(refuses)?;
    if !archava_domain::erp::valid_code(&supplier_code) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let supplier_name: String = row.try_get("supplier_name").map_err(refuses)?;
    if supplier_name
        .trim_matches(|c: char| c.is_whitespace())
        .is_empty()
        || supplier_name.chars().count() > 160
    {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let currency: String = row.try_get("currency").map_err(refuses)?;
    if !super::commerce_kernel::valid_currency_code(&currency) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let revision = i64::from(row.try_get::<i32, _>("revision").map_err(refuses)?);
    if revision < 1 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    Ok(PurchaseOrderRow {
        id: row.try_get("id").map_err(refuses)?,
        owner_id: row.try_get("owner_id").map_err(refuses)?,
        supplier_id: row.try_get("supplier_id").map_err(refuses)?,
        supplier_code,
        supplier_name,
        state,
        total_estimate,
        currency,
        // The RFC 3339 instant, through the same `to_jsonb(...)#>>'{}'` cast
        // the host's CRM read path uses for instants (NULL stays NULL; the
        // microsecond render is one of ERP_SCOPE's recorded divergences).
        confirmed_at: row
            .try_get::<Option<String>, _>("confirmed_at_text")
            .map_err(refuses)?,
        revision,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseLineRow {
    pub id: Uuid,
    pub order_id: Uuid,
    pub offer_id: Uuid,
    pub offer_revision: i64,
    pub product_id: Uuid,
    pub sku: String,
    pub title: String,
    pub unit: String,
    pub quantity: i64,
    pub unit_cost: i64,
    pub accepted_quantity: i64,
    pub rejected_quantity: i64,
}

fn parse_line_row(
    row: &sqlx::postgres::PgRow,
    tenant: &str,
) -> Result<PurchaseLineRow, PurchaseError> {
    let stored_tenant: String = row.try_get("tenant_id").map_err(refuses)?;
    if stored_tenant != tenant {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let clamp = |field: &str| -> Result<i64, PurchaseError> {
        Ok(i64::from(row.try_get::<i32, _>(field).map_err(refuses)?))
    };
    let offer_revision = clamp("offer_revision")?;
    if offer_revision < 1 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let quantity = clamp("quantity")?;
    if quantity < 1 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let unit_cost = clamp("unit_cost")?;
    if !(0..=COST_MAX).contains(&unit_cost) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let accepted_quantity = clamp("accepted_quantity")?;
    let rejected_quantity = clamp("rejected_quantity")?;
    if accepted_quantity < 0 || rejected_quantity < 0 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    Ok(PurchaseLineRow {
        id: row.try_get("id").map_err(refuses)?,
        order_id: row.try_get("order_id").map_err(refuses)?,
        offer_id: row.try_get("offer_id").map_err(refuses)?,
        offer_revision,
        product_id: row.try_get("product_id").map_err(refuses)?,
        sku: row.try_get("sku").map_err(refuses)?,
        title: row.try_get("title").map_err(refuses)?,
        unit: row.try_get("unit").map_err(refuses)?,
        quantity,
        unit_cost,
        accepted_quantity,
        rejected_quantity,
    })
}

// --- Context resolution (the TS `transaction()` member ladder) --------------

/// Both ledgers' resolved grants, post-narrowing — the shape `authorize()`
/// and `owned_order()` judge.
#[derive(Debug, Clone)]
pub struct ResolvedPurchaseContext {
    pub purchasing: archava_domain::erp::purchasing::PurchaseAccess,
    pub commerce: CommerceAccess,
    /// The actor principal id — `p.id` on the TS side: the id commands are
    /// recorded under and recovery re-checks.
    pub actor_id: String,
}

impl ResolvedPurchaseContext {
    /// The full context the pure rule judges.
    pub fn context(&self) -> PurchaseContext {
        PurchaseContext {
            purchasing: self.purchasing.clone(),
            commerce: self.commerce.clone(),
        }
    }
}

/// Resolves both ledger memberships inside the caller's transaction — the
/// port of `transaction()`'s member ladder. An absent row, an inactive
/// grant, a kind mismatch, a non-human `onBehalfOf`, or a commerce context
/// missing `catalog.read` all refuse before the first business statement.
/// The TS narrowing (`if (a.commerce.scope === 'assigned') a.scope =
/// 'assigned'`) runs before the `catalog.read` floor, and so does this port.
pub async fn resolve_purchasing_context(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<ResolvedPurchaseContext, PurchaseError> {
    let own = load_locked_membership(
        connection,
        PURCHASING_NAMESPACE,
        PURCHASING_LEDGER,
        tenant,
        &principal.id,
        PERMISSION_VOCABULARY,
    )
    .await?
    .ok_or(PurchaseError::Domain(ErpError::Forbidden))?;
    if !own.active {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    if own.kind.as_str() != principal.kind.as_str() {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    let commerce_own = load_locked_membership(
        connection,
        COMMERCE_NAMESPACE,
        COMMERCE_LEDGER,
        tenant,
        &principal.id,
        COMMERCE_PERMISSION_VOCABULARY,
    )
    .await?
    .ok_or(PurchaseError::Domain(ErpError::Forbidden))?;
    if !commerce_own.active {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    if commerce_own.kind.as_str() != principal.kind.as_str() {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    let human = match principal.kind {
        PrincipalKind::Human => None,
        PrincipalKind::Service => {
            let on_behalf_of = principal
                .on_behalf_of
                .as_deref()
                .ok_or(PurchaseError::Domain(ErpError::Forbidden))?;
            let human_purchase = load_locked_membership(
                connection,
                PURCHASING_NAMESPACE,
                PURCHASING_LEDGER,
                tenant,
                on_behalf_of,
                PERMISSION_VOCABULARY,
            )
            .await?
            .ok_or(PurchaseError::Domain(ErpError::Forbidden))?;
            let human_commerce = load_locked_membership(
                connection,
                COMMERCE_NAMESPACE,
                COMMERCE_LEDGER,
                tenant,
                on_behalf_of,
                COMMERCE_PERMISSION_VOCABULARY,
            )
            .await?
            .ok_or(PurchaseError::Domain(ErpError::Forbidden))?;
            if human_purchase.kind != ErpMembershipKind::Human
                || human_commerce.kind != ErpMembershipKind::Human
            {
                return Err(PurchaseError::Domain(ErpError::Forbidden));
            }
            if !human_purchase.active || !human_commerce.active {
                return Err(PurchaseError::Domain(ErpError::Forbidden));
            }
            Some((human_purchase, human_commerce))
        }
    };
    let context = match &human {
        Some((human_purchase, human_commerce)) => {
            let purchasing = archava_domain::erp::purchasing::PurchaseAccess {
                permissions: own
                    .permissions
                    .iter()
                    .filter(|held| human_purchase.permissions.contains(*held))
                    .cloned()
                    .collect(),
                // The TS swap: `ownerId = hp.principalId` — the named human.
                owner_id: principal
                    .on_behalf_of
                    .clone()
                    .expect("service principal resolved its human above"),
                scope: match (own.scope, human_purchase.scope) {
                    (ErpScope::Assigned, _) | (_, ErpScope::Assigned) => ErpScope::Assigned,
                    (ErpScope::All, ErpScope::All) => ErpScope::All,
                },
            };
            let commerce = CommerceAccess {
                permissions: commerce_own
                    .permissions
                    .iter()
                    .filter(|held| human_commerce.permissions.contains(*held))
                    .cloned()
                    .collect(),
                scope: match (commerce_own.scope, human_commerce.scope) {
                    (ErpScope::Assigned, _) | (_, ErpScope::Assigned) => ErpScope::Assigned,
                    (ErpScope::All, ErpScope::All) => ErpScope::All,
                },
            };
            PurchaseContext {
                purchasing,
                commerce,
            }
            .narrowed()
        }
        None => PurchaseContext {
            purchasing: archava_domain::erp::purchasing::PurchaseAccess {
                permissions: own.permissions.clone(),
                owner_id: principal.id.clone(),
                scope: own.scope,
            },
            commerce: CommerceAccess {
                permissions: commerce_own.permissions.clone(),
                scope: commerce_own.scope,
            },
        }
        .narrowed(),
    };
    if !context.commerce.holds(&["catalog.read"]) {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    Ok(ResolvedPurchaseContext {
        purchasing: context.purchasing,
        commerce: context.commerce,
        actor_id: principal.id.clone(),
    })
}

/// `ownedOrder()` — the order read that carries BOTH the `orders.read` grant
/// and the assignment scope rule (`a.scope==='all' OR owner_id=a.ownerId`),
/// under `FOR UPDATE` when a mutation will follow, `FOR SHARE` for
/// recovery's re-read.
pub(super) async fn owned_order(
    connection: &mut PgConnection,
    tenant: &str,
    purchasing: &archava_domain::erp::purchasing::PurchaseAccess,
    id: Uuid,
    lock: bool,
) -> Result<PurchaseOrderRow, PurchaseError> {
    if !purchasing.holds(&["orders.read"]) {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    let statement = if lock {
        "SELECT *,to_jsonb(confirmed_at)#>>'{}' AS confirmed_at_text FROM archava_purchasing.orders \
         WHERE tenant_id=$1 AND id=$2 AND ($3::boolean OR owner_id=$4) FOR UPDATE"
    } else {
        "SELECT *,to_jsonb(confirmed_at)#>>'{}' AS confirmed_at_text FROM archava_purchasing.orders \
         WHERE tenant_id=$1 AND id=$2 AND ($3::boolean OR owner_id=$4) FOR SHARE"
    };
    let row = sqlx::query(statement)
        .bind(tenant)
        .bind(id)
        .bind(purchasing.scope == ErpScope::All)
        .bind(&purchasing.owner_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(refuses)?
        .ok_or(PurchaseError::Domain(ErpError::NotFound))?;
    parse_order_row(&row, tenant)
}

// --- Quote (the TS `quote()`) -----------------------------------------------

/// The currency a new supplier offer or purchase order is created in: its
/// company's trade currency, or IDR without a company. The `a_currency_*`
/// insert guards of the purchasing ledger re-derive and enforce it.
async fn purchase_currency(
    connection: &mut PgConnection,
    tenant: &str,
    company: Option<Uuid>,
) -> Result<String, PurchaseError> {
    match company {
        Some(company) => {
            super::commerce_kernel::company_trade_currency(connection, tenant, company).await
        }
        None => Ok("IDR".to_owned()),
    }
}

/// One frozen line a quote produced — the line row the `writeLines` insert
/// carries, with the offer's snapshot columns.
struct QuotedLine {
    line_id: Uuid,
    offer_id: Uuid,
    offer_revision: i64,
    product_id: Uuid,
    sku: String,
    title: String,
    unit: String,
    quantity: i64,
    unit_cost: i64,
}

struct Quote {
    supplier: SupplierRow,
    lines: Vec<QuotedLine>,
    total: i64,
}

/// `quote()` — supplier must exist and be active under `FOR SHARE`; every
/// offer must exist for THIS supplier and be active under `FOR SHARE OF o`;
/// every product must still be active, stock-tracked and locked `FOR
/// UPDATE` through the commerce kernel; the total may never exceed the
/// order-total cell. `currency` is the purchase order's: every offer and
/// every product must be priced in it, so one order holds one currency.
async fn quote(
    connection: &mut PgConnection,
    tenant: &str,
    supplier_id: Uuid,
    lines: &[LineInput],
    company: Option<Uuid>,
    currency: &str,
) -> Result<Quote, PurchaseError> {
    let supplier_row = sqlx::query(
        "SELECT * FROM archava_purchasing.suppliers WHERE tenant_id=$1 AND id=$2 FOR SHARE",
    )
    .bind(tenant)
    .bind(supplier_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)?
    .ok_or(PurchaseError::Domain(ErpError::Conflict))?;
    let supplier = parse_supplier_row(&supplier_row, tenant)?;
    if !supplier.active {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }

    let offer_ids: Vec<Uuid> = lines
        .iter()
        .map(|line| parse_uuid(&line.offer_id))
        .collect::<Result<_, _>>()?;
    let offer_rows = sqlx::query(
        "SELECT o.*,p.sku,p.title,p.unit FROM archava_purchasing.offers o \
         JOIN archava_commerce.products p ON p.tenant_id=o.tenant_id AND p.id=o.product_id \
         WHERE o.tenant_id=$1 AND o.supplier_id=$2 AND o.id=ANY($3) \
         ORDER BY o.id FOR SHARE OF o",
    )
    .bind(tenant)
    .bind(supplier_id)
    .bind(&offer_ids)
    .fetch_all(&mut *connection)
    .await
    .map_err(refuses)?;
    let offers = offer_rows
        .iter()
        .map(|row: &sqlx::postgres::PgRow| parse_offer_joined_row(row, tenant))
        .collect::<Result<Vec<_>, _>>()?;
    // Input lines are unique per offer (parse level) and offers are unique
    // per id (primary key): a missing or inactive offer is the same conflict
    // either way.
    if offers.len() != lines.len() || offers.iter().any(|offer| !offer.active) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    // One order, one currency. This is the rule of the line guard
    // (Purchasing 007), decided here, so the refusal carries its reason.
    if offers.iter().any(|offer| offer.currency != currency) {
        return Err(super::refused_by_code(LINE_CURRENCY_MISMATCH));
    }

    let products = super::commerce_kernel::lock_document_products_refusing(
        connection,
        tenant,
        &offers
            .iter()
            .map(|offer| offer.product_id)
            .collect::<Vec<_>>(),
        "commerce",
        company,
        currency,
        LINE_CURRENCY_MISMATCH,
    )
    .await?;
    if products
        .iter()
        .any(|product| !product.active || !product.track_stock)
    {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }

    let mut quoted = Vec::with_capacity(lines.len());
    for line in lines {
        let offer = offers
            .iter()
            .find(|offer| offer.id.to_string() == line.offer_id)
            .ok_or(PurchaseError::Domain(ErpError::Conflict))?;
        let product = products
            .iter()
            .find(|product| product.id == offer.product_id)
            .ok_or(PurchaseError::Domain(ErpError::Conflict))?;
        quoted.push(QuotedLine {
            line_id: Uuid::new_v4(),
            offer_id: parse_uuid(&line.offer_id)?,
            offer_revision: offer.revision,
            product_id: product.id,
            sku: product.sku.clone(),
            title: product.title.clone(),
            unit: product.unit.clone(),
            quantity: line.quantity,
            unit_cost: offer.unit_cost,
        });
    }
    // The exact total bound: `Number.isSafeInteger(total) && total <= 5e14`.
    // i128 absorbs the per-line products (≤ 1e6 × 1e9 each) before the one
    // comparison that decides.
    let total = quoted
        .iter()
        .map(|line| i128::from(line.quantity) * i128::from(line.unit_cost))
        .sum::<i128>();
    if total < 0 || total > i128::from(TOTAL_MAX) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    Ok(Quote {
        supplier,
        lines: quoted,
        total: total as i64,
    })
}

/// `writeLines()` — the frozen snapshot insert, one statement per line in
/// input order (the deferred trigger set verifies each after).
async fn write_lines(
    connection: &mut PgConnection,
    tenant: &str,
    order_id: Uuid,
    lines: &[QuotedLine],
) -> Result<(), PurchaseError> {
    for line in lines {
        sqlx::query(
            "INSERT INTO archava_purchasing.lines(tenant_id,id,order_id,offer_id,offer_revision,product_id,sku,title,unit,quantity,unit_cost) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(tenant)
        .bind(line.line_id)
        .bind(order_id)
        .bind(line.offer_id)
        .bind(int4(line.offer_revision)?)
        .bind(line.product_id)
        .bind(line.sku.as_str())
        .bind(line.title.as_str())
        .bind(line.unit.as_str())
        .bind(int4(line.quantity)?)
        .bind(int4(line.unit_cost)?)
        .execute(&mut *connection)
        .await
        .map_err(refuses)?;
    }
    Ok(())
}

// --- Commands: claim, recovery, audit ---------------------------------------

/// Claims the purchasing command row (`ON CONFLICT DO NOTHING`): `true`
/// when THIS call owns the claim.
async fn claim_purchase_command(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    actor_id: &str,
    fingerprint: &str,
) -> Result<bool, PurchaseError> {
    let claimed = sqlx::query(
        "INSERT INTO archava_purchasing.commands(tenant_id,request_id,actor_id,fingerprint) \
         VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(actor_id)
    .bind(fingerprint)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(claimed.rows_affected() == 1)
}

/// Fetches an existing purchasing command row: `(actor_id, fingerprint,
/// receipt)` or `None`.
async fn fetch_command_row(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
) -> Result<Option<(String, String, Option<serde_json::Value>)>, PurchaseError> {
    sqlx::query_as(
        "SELECT actor_id,fingerprint,receipt FROM archava_purchasing.commands \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)
}

/// The purchasing ledger's audit row — the operation, record id and revision
/// the `command_complete` trigger reconciles against `commands.receipt`.
///
/// Eight parameters is the SQL row's own arity — one per bound column, in the
/// row grammar's order — so the crate's usual allow for table writers applies.
#[allow(clippy::too_many_arguments)]
async fn insert_purchase_audit(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    actor_id: &str,
    operation: &str,
    record_id: Uuid,
    revision: i64,
    reason: Option<&str>,
) -> Result<(), PurchaseError> {
    sqlx::query(
        "INSERT INTO archava_purchasing.audit(tenant_id,request_id,actor_id,operation,record_id,revision,reason) \
         VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(actor_id)
    .bind(operation)
    .bind(record_id)
    .bind(int4(revision)?)
    .bind(reason)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(())
}

/// The full mutation pipeline inside the caller's transaction — the port of
/// `mutate()`. Fingerprint and resolution are supplied by the host (the
/// identity store's keyed digest is a host concern); everything from the
/// authorization ladder to the receipt-binding UPDATE happens here, in the
/// TS statement order.
struct CommandSources {
    company: Option<super::company_trade::CompanyStamp>,
    stock_company: Option<Uuid>,
    contact: Option<crate::erp::crm::LockedContact>,
    party: Option<super::parties::TransactionParty>,
}
pub async fn record_purchase_command(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    context: &ResolvedPurchaseContext,
    fingerprint: &str,
    mutation: &PurchaseMutation,
) -> Result<PurchaseReceipt, PurchaseError> {
    // `mutate` records HUMAN commands only — the same route-level kind gate
    // the TS server applies before the transaction.
    if principal.kind.as_str() != "human" {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    authorize_operation(&context.context(), mutation.command.operation())
        .map_err(PurchaseError::Domain)?;
    // Current CRM authority is required even on a replay. Lock the source
    // before the supplier; the immutable UUID makes the unlocked lookup safe.
    let company = if let PurchaseCommand::OrderCreateForCompany { company_id, .. } =
        &mutation.command
    {
        Some(
            super::company_trade::resolve(connection, tenant, principal, parse_uuid(company_id)?)
                .await?,
        )
    } else {
        None
    };
    let target = match &mutation.command {
        PurchaseCommand::DocumentCapture { input } => Some((
            super::company_trade::Record::Purchase,
            input.order_id.as_str(),
        )),
        PurchaseCommand::DocumentRevise { input } => Some((
            super::company_trade::Record::SupplierDocument,
            input.id.as_str(),
        )),
        PurchaseCommand::DocumentMatch { input } => Some((
            super::company_trade::Record::SupplierDocument,
            input.id.as_str(),
        )),
        PurchaseCommand::DocumentVoid { input } => Some((
            super::company_trade::Record::SupplierDocument,
            input.id.as_str(),
        )),
        PurchaseCommand::OfferCreate { product_id, .. } => {
            Some((super::company_trade::Record::Product, product_id.as_str()))
        }
        PurchaseCommand::OfferSetCost { id, .. } | PurchaseCommand::OfferSetActive { id, .. } => {
            Some((super::company_trade::Record::Offer, id.as_str()))
        }
        PurchaseCommand::OrderReplaceLines { id, .. }
        | PurchaseCommand::OrderConfirm { id, .. }
        | PurchaseCommand::OrderCancel { id, .. }
        | PurchaseCommand::OrderShortClose { id, .. }
        | PurchaseCommand::OrderReceive { id, .. } => {
            Some((super::company_trade::Record::Purchase, id.as_str()))
        }
        _ => None,
    };
    let stock_company = if let Some((kind, id)) = target {
        super::company_trade::authorize(connection, tenant, principal, kind, parse_uuid(id)?)
            .await?
    } else {
        company.as_ref().map(|c| c.id)
    };
    let offer_company = if matches!(
        mutation.command,
        PurchaseCommand::OfferCreate { .. }
            | PurchaseCommand::OfferSetCost { .. }
            | PurchaseCommand::OfferSetActive { active: true, .. }
    ) {
        if let Some(id) = stock_company {
            Some(super::company_trade::resolve(connection, tenant, principal, id).await?)
        } else {
            None
        }
    } else {
        None
    };
    let source_id = match &mutation.command {
        PurchaseCommand::SupplierCreateFromContact { contact_id, .. } => {
            Some(parse_uuid(contact_id)?)
        }
        PurchaseCommand::SupplierRefreshContact { id, .. } => {
            let source = sqlx::query_scalar::<_, Option<Uuid>>(
                "SELECT supplier_contact_id FROM archava_purchasing.suppliers WHERE tenant_id=$1 AND id=$2",
            ).bind(tenant).bind(parse_uuid(id)?).fetch_optional(&mut *connection).await.map_err(refuses)?
                .ok_or(ErpError::NotFound)?.ok_or(ErpError::Conflict)?;
            Some(source)
        }
        _ => None,
    };
    let contact_source = if let Some(id) = source_id {
        Some(super::customer::resolve(connection, tenant, principal, id).await?)
    } else {
        None
    };
    let supplier_id = match &mutation.command {
        PurchaseCommand::SupplierRefreshParty { id, .. }
        | PurchaseCommand::SupplierSetActive {
            id, active: true, ..
        } => Some(parse_uuid(id)?),
        PurchaseCommand::OfferCreate { supplier_id, .. }
        | PurchaseCommand::OrderCreate { supplier_id, .. }
        | PurchaseCommand::OrderCreateForCompany { supplier_id, .. } => {
            Some(parse_uuid(supplier_id)?)
        }
        _ => None,
    };
    let party_id = if let PurchaseCommand::SupplierCreateFromParty { party_id, .. } =
        &mutation.command
    {
        Some(parse_uuid(party_id)?)
    } else if let Some(id) = supplier_id {
        sqlx::query_scalar::<_,Option<Uuid>>("SELECT supplier_party_id FROM archava_purchasing.suppliers WHERE tenant_id=$1 AND id=$2").bind(tenant).bind(id).fetch_optional(&mut *connection).await.map_err(refuses)?.ok_or(ErpError::NotFound)?
    } else {
        None
    };
    if matches!(
        mutation.command,
        PurchaseCommand::SupplierRefreshParty { .. }
    ) && party_id.is_none()
    {
        return Err(ErpError::Conflict.into());
    }
    let party_source = if let Some(id) = party_id {
        Some(super::parties::resolve_transaction(connection, tenant, principal, id).await?)
    } else {
        None
    };
    let source = CommandSources {
        company,
        stock_company,
        contact: contact_source,
        party: party_source,
    };
    let request_id = parse_uuid(&mutation.request_id)?;

    let claimed = claim_purchase_command(
        connection,
        tenant,
        request_id,
        &context.actor_id,
        fingerprint,
    )
    .await?;
    if !claimed {
        // Recovery: the row MUST exist, with the same fingerprint and the
        // same actor, and carry a replayable receipt — one conflict covers
        // every mismatch shape, exactly like the TS single throw.
        let (row_actor, row_fingerprint, row_receipt) =
            fetch_command_row(connection, tenant, request_id)
                .await?
                .ok_or(PurchaseError::Domain(ErpError::Conflict))?;
        if row_fingerprint != fingerprint || row_actor != context.actor_id || row_receipt.is_none()
        {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        }
        let receipt_value = row_receipt.expect("checked is_some above");
        let stored =
            recover_from_row(connection, tenant, context, principal, &receipt_value).await?;
        // The replayed receipt is served WITHOUT re-mentioning the original:
        // the commands row's stored receipt is untouched.
        return Ok(stored);
    }

    if let Some(company) = &source.company {
        if let PurchaseCommand::OrderCreateForCompany {
            expected_company_revision,
            ..
        } = mutation.command
        {
            company.validate_new(expected_company_revision)?;
        }
    }
    if offer_company.is_some_and(|c| !c.active) {
        return Err(ErpError::Conflict.into());
    }
    if let Some(party) = &source.party {
        if !party.supplier {
            return Err(ErpError::Conflict.into());
        }
        if let PurchaseCommand::SupplierCreateFromParty {
            expected_party_revision,
            ..
        }
        | PurchaseCommand::SupplierRefreshParty {
            expected_party_revision,
            ..
        } = &mutation.command
        {
            if i64::from(party.revision) != *expected_party_revision {
                return Err(ErpError::Conflict.into());
            }
        }
        if let Some(id) = supplier_id.filter(|_| {
            !matches!(
                mutation.command,
                PurchaseCommand::SupplierRefreshParty { .. }
            )
        }) {
            let revision:Option<i32>=sqlx::query_scalar("SELECT supplier_party_revision FROM archava_purchasing.suppliers WHERE tenant_id=$1 AND id=$2").bind(tenant).bind(id).fetch_one(&mut *connection).await.map_err(refuses)?;
            if revision != Some(party.revision) {
                return Err(ErpError::Conflict.into());
            }
        }
    }
    let (id, revision) = execute_command(
        connection,
        tenant,
        &context.actor_id,
        context,
        mutation,
        fingerprint,
        &source,
    )
    .await?;
    let receipt = PurchaseReceipt {
        request_id: mutation.request_id.clone(),
        operation: mutation.command.operation().to_string(),
        id: id.to_string(),
        revision,
        replayed: false,
    };
    insert_purchase_audit(
        connection,
        tenant,
        request_id,
        &context.actor_id,
        receipt.operation.as_str(),
        id,
        revision,
        mutation.command.reason(),
    )
    .await?;
    let receipt_value =
        serde_json::to_value(&receipt).map_err(|_| PurchaseError::Domain(ErpError::Conflict))?;
    sqlx::query(
        "UPDATE archava_purchasing.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(&receipt_value)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(receipt)
}

/// `recovery()` — replays a stored receipt under the CURRENT grants: the
/// ladder re-runs, and for order branches the order itself is re-read under
/// the current ownership scope (`FOR SHARE`). Revoking a grant takes the
/// receipt's recoverability with it.
async fn recover_from_row(
    connection: &mut PgConnection,
    tenant: &str,
    context: &ResolvedPurchaseContext,
    principal: &PurchasePrincipal,
    receipt_value: &serde_json::Value,
) -> Result<PurchaseReceipt, PurchaseError> {
    let stored: PurchaseReceipt = serde_json::from_value(receipt_value.clone())
        .map_err(|_| PurchaseError::Domain(ErpError::Conflict))?;
    // The stored receipt's schema bounds, repeated on the read path: the ids
    // are uuids, the revision inside the command ceiling.
    if !valid_uuid(&stored.request_id) || !valid_uuid(&stored.id) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    if stored.revision < 1 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let target = if stored.operation.starts_with("document.") {
        Some(super::company_trade::Record::SupplierDocument)
    } else if stored.operation.starts_with("order.") {
        Some(super::company_trade::Record::Purchase)
    } else if stored.operation.starts_with("offer.") {
        Some(super::company_trade::Record::Offer)
    } else {
        None
    };
    if let Some(kind) = target {
        super::company_trade::authorize(
            connection,
            tenant,
            principal,
            kind,
            parse_uuid(&stored.id)?,
        )
        .await?;
    }
    authorize_recovery(&context.context(), &stored).map_err(PurchaseError::Domain)?;
    // The typed union leaves only order-prefixed operations for this branch;
    // the TS check is `isOrderOperation(op)` with the same prefix.
    if stored.operation.starts_with("document.") {
        let source =
            super::supplier_documents::order_id(connection, tenant, parse_uuid(&stored.id)?)
                .await?;
        owned_order(connection, tenant, &context.purchasing, source, false).await?;
    }
    if stored.operation.starts_with("order.") {
        let order_id = parse_uuid(&stored.id)?;
        owned_order(connection, tenant, &context.purchasing, order_id, false).await?;
    }
    Ok(replayed_receipt(&stored))
}

/// `commandStatus()` — what a redone request finds. A committed receipt is
/// only recoverable while the CURRENT access still authorizes the recorded
/// operation, so recovery refusals surface here too.
pub async fn command_status(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    context: &ResolvedPurchaseContext,
    request_id: Uuid,
) -> Result<archava_domain::erp::purchasing::PurchaseCommandStatus, PurchaseError> {
    if principal.kind.as_str() != "human" {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    let Some((row_actor, _, row_receipt)) =
        fetch_command_row(connection, tenant, request_id).await?
    else {
        return Ok(archava_domain::erp::purchasing::PurchaseCommandStatus::Unknown);
    };
    // The receipt belongs to the actor who wrote it: a peer in the same
    // tenant cannot recover another actor's command by naming its request id.
    // The TS twin's `recovery()` refuses `r.actor_id !== p.id`, and the other
    // ERP command-status ports gate the same way.
    if row_actor != principal.id {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    let receipt_value = row_receipt.ok_or(PurchaseError::Domain(ErpError::Conflict))?;
    let receipt = recover_from_row(connection, tenant, context, principal, &receipt_value).await?;
    Ok(archava_domain::erp::purchasing::PurchaseCommandStatus::Committed { receipt })
}

// --- execute(): the eleven branches -----------------------------------------

/// `execute()` — one typed command to its `(id, revision)` change. The
/// conflict conditions are the TS comparisons, verbatim.
async fn execute_command(
    connection: &mut PgConnection,
    tenant: &str,
    actor_id: &str,
    context: &ResolvedPurchaseContext,
    mutation: &PurchaseMutation,
    fingerprint: &str,
    sources: &CommandSources,
) -> Result<(Uuid, i64), PurchaseError> {
    let source = sources.contact.as_ref();
    match &mutation.command {
        PurchaseCommand::DocumentCapture { .. }
        | PurchaseCommand::DocumentRevise { .. }
        | PurchaseCommand::DocumentMatch { .. }
        | PurchaseCommand::DocumentVoid { .. } => {
            super::supplier_documents::execute(connection, tenant, context, mutation).await
        }
        PurchaseCommand::SupplierCreate { code, name } => {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO archava_purchasing.suppliers(tenant_id,id,code,name) VALUES($1,$2,$3,$4)",
            )
            .bind(tenant)
            .bind(id)
            .bind(code)
            .bind(name)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((id, 1))
        }
        PurchaseCommand::SupplierCreateFromContact {
            code,
            contact_id,
            expected_contact_revision,
        } => {
            let source = source.ok_or(ErpError::Conflict)?;
            if i64::from(source.revision) != *expected_contact_revision
                || source.full_name.trim().is_empty()
                || source.full_name.chars().count() > 160
            {
                return Err(ErpError::Conflict.into());
            }
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO archava_purchasing.suppliers(tenant_id,id,code,name,supplier_contact_id,supplier_contact_revision) VALUES($1,$2,$3,$4,$5,$6)")
                .bind(tenant).bind(id).bind(code).bind(&source.full_name)
                .bind(parse_uuid(contact_id)?).bind(source.revision)
                .execute(&mut *connection).await.map_err(refuses)?;
            Ok((id, 1))
        }
        PurchaseCommand::SupplierRefreshContact {
            id,
            expected_revision,
            expected_contact_revision,
            ..
        } => {
            let source = source.ok_or(ErpError::Conflict)?;
            let id = parse_uuid(id)?;
            let row = sqlx::query("SELECT * FROM archava_purchasing.suppliers WHERE tenant_id=$1 AND id=$2 FOR UPDATE")
                .bind(tenant).bind(id).fetch_optional(&mut *connection).await.map_err(refuses)?.ok_or(ErpError::NotFound)?;
            let supplier = parse_supplier_row(&row, tenant)?;
            let stored: Option<i32> = row.try_get("supplier_contact_revision").map_err(refuses)?;
            if supplier.revision != *expected_revision
                || *expected_revision >= REVISION_MAX
                || i64::from(source.revision) != *expected_contact_revision
                || stored.is_none_or(|revision| source.revision <= revision)
                || source.full_name.trim().is_empty()
                || source.full_name.chars().count() > 160
            {
                return Err(ErpError::Conflict.into());
            }
            sqlx::query("UPDATE archava_purchasing.suppliers SET name=$3,supplier_contact_revision=$4,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2")
                .bind(tenant).bind(id).bind(&source.full_name).bind(source.revision)
                .execute(&mut *connection).await.map_err(refuses)?;
            Ok((id, expected_revision + 1))
        }
        PurchaseCommand::SupplierCreateFromParty { code, party_id, .. } => {
            let party = sources.party.as_ref().ok_or(ErpError::Conflict)?;
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO archava_purchasing.suppliers(tenant_id,id,code,name,supplier_party_id,supplier_party_revision) VALUES($1,$2,$3,$4,$5,$6)").bind(tenant).bind(id).bind(code).bind(&party.name).bind(parse_uuid(party_id)?).bind(party.revision).execute(&mut *connection).await.map_err(refuses)?;
            Ok((id, 1))
        }
        PurchaseCommand::SupplierRefreshParty {
            id,
            expected_revision,
            ..
        } => {
            let party = sources.party.as_ref().ok_or(ErpError::Conflict)?;
            let id = parse_uuid(id)?;
            let row=sqlx::query("SELECT * FROM archava_purchasing.suppliers WHERE tenant_id=$1 AND id=$2 FOR UPDATE").bind(tenant).bind(id).fetch_optional(&mut *connection).await.map_err(refuses)?.ok_or(ErpError::NotFound)?;
            let supplier = parse_supplier_row(&row, tenant)?;
            let stored: Option<i32> = row.try_get("supplier_party_revision").map_err(refuses)?;
            if supplier.revision != *expected_revision
                || *expected_revision >= REVISION_MAX
                || stored.is_none_or(|revision| party.revision <= revision)
            {
                return Err(ErpError::Conflict.into());
            }
            sqlx::query("UPDATE archava_purchasing.suppliers SET name=$3,supplier_party_revision=$4,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2").bind(tenant).bind(id).bind(&party.name).bind(party.revision).execute(&mut *connection).await.map_err(refuses)?;
            Ok((id, expected_revision + 1))
        }
        PurchaseCommand::SupplierSetActive {
            id,
            expected_revision,
            reason: _,
            active,
        } => {
            let id = parse_uuid(id)?;
            let row = sqlx::query(
                "SELECT * FROM archava_purchasing.suppliers WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(refuses)?
            .ok_or(PurchaseError::Domain(ErpError::NotFound))?;
            let supplier = parse_supplier_row(&row, tenant)?;
            if supplier.revision != *expected_revision
                || supplier.active == *active
                || *expected_revision >= REVISION_MAX
            {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            sqlx::query(
                "UPDATE archava_purchasing.suppliers SET active=$3,revision=revision+1,updated_at=clock_timestamp() \
                 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(id)
            .bind(active)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((id, expected_revision + 1))
        }
        PurchaseCommand::OfferCreate {
            supplier_id,
            product_id,
            unit_cost,
        } => {
            let supplier_id = parse_uuid(supplier_id)?;
            let product_id = parse_uuid(product_id)?;
            let row = sqlx::query(
                "SELECT * FROM archava_purchasing.suppliers WHERE tenant_id=$1 AND id=$2 FOR SHARE",
            )
            .bind(tenant)
            .bind(supplier_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(refuses)?
            .ok_or(PurchaseError::Domain(ErpError::Conflict))?;
            let supplier = parse_supplier_row(&row, tenant)?;
            if !supplier.active {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            // An offer is priced in the trade currency of the company that
            // owns its product (IDR for an unowned product), and the product
            // itself must be priced in it.
            let currency = purchase_currency(connection, tenant, sources.stock_company).await?;
            let products = super::commerce_kernel::lock_document_products_refusing(
                connection,
                tenant,
                &[product_id],
                "commerce",
                sources.stock_company,
                &currency,
                OFFER_CURRENCY_MISMATCH,
            )
            .await?;
            if products
                .first()
                .map(|product| !product.active || !product.track_stock)
                .unwrap_or(true)
            {
                // The TS shape: a missing list element never matches
                // (`!product?.active`), which for a `lock` that already
                // refuses missing ids remains one conflict.
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO archava_purchasing.offers(tenant_id,id,supplier_id,product_id,unit_cost,currency) \
                 VALUES($1,$2,$3,$4,$5,$6)",
            )
            .bind(tenant)
            .bind(id)
            .bind(supplier_id)
            .bind(product_id)
            .bind(int4(*unit_cost)?)
            .bind(currency.as_str())
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((id, 1))
        }
        PurchaseCommand::OfferSetCost {
            id,
            expected_revision,
            reason: _,
            unit_cost,
        } => {
            let id = parse_uuid(id)?;
            let row = sqlx::query(
                "SELECT * FROM archava_purchasing.offers WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(refuses)?
            .ok_or(PurchaseError::Domain(ErpError::NotFound))?;
            let offer = parse_offer_row(&row, tenant)?;
            if offer.revision != *expected_revision || *expected_revision >= REVISION_MAX {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            if offer.unit_cost == *unit_cost {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            sqlx::query(
                        "UPDATE archava_purchasing.offers SET unit_cost=$3,revision=revision+1,updated_at=clock_timestamp() \
                         WHERE tenant_id=$1 AND id=$2",
                    )
                    .bind(tenant)
                    .bind(id)
                    .bind(int4(*unit_cost)?)
                    .execute(&mut *connection)
                    .await
                    .map_err(refuses)?;
            Ok((id, *expected_revision + 1))
        }
        PurchaseCommand::OfferSetActive {
            id,
            expected_revision,
            reason: _,
            active,
        } => {
            let id = parse_uuid(id)?;
            let row = sqlx::query(
                "SELECT * FROM archava_purchasing.offers WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(refuses)?
            .ok_or(PurchaseError::Domain(ErpError::NotFound))?;
            let offer = parse_offer_row(&row, tenant)?;
            if offer.revision != *expected_revision || *expected_revision >= REVISION_MAX {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            if offer.active == *active {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            sqlx::query(
                        "UPDATE archava_purchasing.offers SET active=$3,revision=revision+1,updated_at=clock_timestamp() \
                         WHERE tenant_id=$1 AND id=$2",
                    )
                    .bind(tenant)
                    .bind(id)
                    .bind(active)
                    .execute(&mut *connection)
                    .await
                    .map_err(refuses)?;
            Ok((id, *expected_revision + 1))
        }
        PurchaseCommand::OrderCreate { supplier_id, lines }
        | PurchaseCommand::OrderCreateForCompany {
            supplier_id, lines, ..
        } => {
            let supplier_id = parse_uuid(supplier_id)?;
            // A company's order is in its book currency, any other order IDR;
            // the quote refuses an offer or a product priced in another one.
            let currency = purchase_currency(
                connection,
                tenant,
                sources.company.as_ref().map(|company| company.id),
            )
            .await?;
            let quote = quote(
                connection,
                tenant,
                supplier_id,
                lines,
                sources.stock_company,
                &currency,
            )
            .await?;
            let id = Uuid::new_v4();
            let statement = if sources.company.is_some() {
                "INSERT INTO archava_purchasing.orders(tenant_id,id,owner_id,supplier_id,supplier_code,supplier_name,total_estimate,company_id,company_revision,company_code,company_name,currency) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)"
            } else {
                "INSERT INTO archava_purchasing.orders(tenant_id,id,owner_id,supplier_id,supplier_code,supplier_name,total_estimate) VALUES($1,$2,$3,$4,$5,$6,$7)"
            };
            let query = sqlx::query(statement)
                .bind(tenant)
                .bind(id)
                .bind(&context.purchasing.owner_id)
                .bind(supplier_id)
                .bind(&quote.supplier.code)
                .bind(&quote.supplier.name)
                .bind(quote.total);
            let query = if let Some(company) = &sources.company {
                query
                    .bind(company.id)
                    .bind(company.revision)
                    .bind(&company.code)
                    .bind(&company.name)
                    .bind(currency.as_str())
            } else {
                query
            };
            query.execute(&mut *connection).await.map_err(refuses)?;
            write_lines(connection, tenant, id, &quote.lines).await?;
            Ok((id, 1))
        }
        PurchaseCommand::OrderReplaceLines {
            id,
            expected_revision,
            reason: _,
            lines,
        } => {
            let id = parse_uuid(id)?;
            let order = owned_order(connection, tenant, &context.purchasing, id, true).await?;
            if order.revision != *expected_revision || *expected_revision >= REVISION_MAX {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            if order.state != "draft" {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            let quote = quote(
                connection,
                tenant,
                order.supplier_id,
                lines,
                sources.stock_company,
                &order.currency,
            )
            .await?;
            sqlx::query("DELETE FROM archava_purchasing.lines WHERE tenant_id=$1 AND order_id=$2")
                .bind(tenant)
                .bind(order.id)
                .execute(&mut *connection)
                .await
                .map_err(refuses)?;
            write_lines(connection, tenant, order.id, &quote.lines).await?;
            sqlx::query(
                "UPDATE archava_purchasing.orders SET total_estimate=$3,revision=revision+1,updated_at=clock_timestamp() \
                 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(order.id)
            .bind(quote.total)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((order.id, order.revision + 1))
        }
        PurchaseCommand::OrderConfirm {
            id,
            expected_revision,
            reason: _,
        }
        | PurchaseCommand::OrderCancel {
            id,
            expected_revision,
            reason: _,
        }
        | PurchaseCommand::OrderShortClose {
            id,
            expected_revision,
            reason: _,
        }
        | PurchaseCommand::OrderReceive {
            id,
            expected_revision,
            reason: _,
            lines: _,
            ..
        } => {
            let id = parse_uuid(id)?;
            let order = owned_order(connection, tenant, &context.purchasing, id, true).await?;
            if order.revision != *expected_revision || *expected_revision >= REVISION_MAX {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            let state: String = match &mutation.command {
                PurchaseCommand::OrderConfirm { .. } => {
                    if order.state != "draft" {
                        return Err(PurchaseError::Domain(ErpError::Conflict));
                    }
                    // Confirm re-quotes the STORED lines: today's offer
                    // revision and unit cost must still pair with what the
                    // draft froze, or the drift refuses the confirmation.
                    let line_rows = sqlx::query(
                        "SELECT * FROM archava_purchasing.lines WHERE tenant_id=$1 AND order_id=$2 ORDER BY id",
                    )
                    .bind(tenant)
                    .bind(order.id)
                    .fetch_all(&mut *connection)
                    .await
                    .map_err(refuses)?;
                    let old = line_rows
                        .iter()
                        .map(|row| parse_line_row(row, tenant))
                        .collect::<Result<Vec<_>, _>>()?;
                    let requote = quote(
                        connection,
                        tenant,
                        order.supplier_id,
                        &old.iter()
                            .map(|line| LineInput {
                                offer_id: line.offer_id.to_string(),
                                quantity: line.quantity,
                            })
                            .collect::<Vec<_>>(),
                        sources.stock_company,
                        &order.currency,
                    )
                    .await?;
                    if requote.lines.iter().any(|quoted| {
                        old.iter()
                            .find(|previous| previous.offer_id == quoted.offer_id)
                            .map(|previous| {
                                previous.offer_revision != quoted.offer_revision
                                    || previous.unit_cost != quoted.unit_cost
                            })
                            .unwrap_or(true)
                    }) {
                        return Err(PurchaseError::Domain(ErpError::Conflict));
                    }
                    "confirmed".to_string()
                }
                PurchaseCommand::OrderCancel { .. } => {
                    if order.state != "draft" && order.state != "confirmed" {
                        return Err(PurchaseError::Domain(ErpError::Conflict));
                    }
                    "cancelled".to_string()
                }
                PurchaseCommand::OrderShortClose { .. } => {
                    if order.state != "confirmed" && order.state != "partial" {
                        return Err(PurchaseError::Domain(ErpError::Conflict));
                    }
                    "short_closed".to_string()
                }
                PurchaseCommand::OrderReceive {
                    lines,
                    reason,
                    landed_fee_minor,
                    allocation_basis,
                    ..
                } => {
                    if order.state != "confirmed" && order.state != "partial" {
                        return Err(PurchaseError::Domain(ErpError::Conflict));
                    }
                    let line_rows = sqlx::query(
                        "SELECT * FROM archava_purchasing.lines WHERE tenant_id=$1 AND order_id=$2 ORDER BY id",
                    )
                    .bind(tenant)
                    .bind(order.id)
                    .fetch_all(&mut *connection)
                    .await
                    .map_err(refuses)?;
                    let mut stored = line_rows
                        .iter()
                        .map(|row| parse_line_row(row, tenant))
                        .collect::<Result<Vec<_>, _>>()?;
                    // The bound check BEFORE the receipt row exists: every
                    // finding must name a line of this order and stay inside
                    // the line's remaining quantity.
                    for finding in lines {
                        let line = stored
                            .iter()
                            .find(|line| line.id.to_string() == finding.line_id)
                            .ok_or(PurchaseError::Domain(ErpError::Conflict))?;
                        if line.accepted_quantity
                            + line.rejected_quantity
                            + finding.accepted
                            + finding.rejected
                            > line.quantity
                        {
                            return Err(PurchaseError::Domain(ErpError::Conflict));
                        }
                    }
                    let cost_lines = lines
                        .iter()
                        .filter(|finding| finding.accepted > 0)
                        .map(|finding| {
                            let line = stored
                                .iter()
                                .find(|line| line.id.to_string() == finding.line_id)
                                .expect("validated receipt line");
                            archava_domain::erp::inventory_valuation::Line {
                                id: finding.line_id.clone(),
                                quantity: finding.accepted,
                                unit_minor: line.unit_cost,
                            }
                        })
                        .collect::<Vec<_>>();
                    let fee = landed_fee_minor.unwrap_or(0);
                    if cost_lines.is_empty() && fee != 0 {
                        return Err(PurchaseError::Domain(ErpError::Conflict));
                    }
                    let costs = if cost_lines.is_empty() {
                        Vec::new()
                    } else {
                        archava_domain::erp::inventory_valuation::allocate(
                            &cost_lines,
                            fee,
                            allocation_basis
                                .unwrap_or(archava_domain::erp::inventory_valuation::Basis::Value),
                        )
                        .map_err(|_| PurchaseError::Domain(ErpError::Conflict))?
                    };
                    sqlx::query(
                        "INSERT INTO archava_purchasing.receipts(tenant_id,request_id,order_id,actor_id,revision,reason,landed_fee_minor,allocation_basis) \
                         VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
                    )
                    .bind(tenant)
                    .bind(parse_uuid(&mutation.request_id)?)
                    .bind(order.id)
                    .bind(actor_id)
                    .bind(int4(order.revision + 1)?)
                    .bind(reason.as_str())
                    .bind(fee)
                    .bind(match allocation_basis.unwrap_or(archava_domain::erp::inventory_valuation::Basis::Value){archava_domain::erp::inventory_valuation::Basis::Value=>"value",archava_domain::erp::inventory_valuation::Basis::Quantity=>"quantity"})
                    .execute(&mut *connection)
                    .await
                    .map_err(refuses)?;
                    let mut stock: Vec<InboundLine> = Vec::new();
                    for finding in lines {
                        let index = stored
                            .iter()
                            .position(|line| line.id.to_string() == finding.line_id)
                            .ok_or(PurchaseError::Domain(ErpError::Conflict))?;
                        let line = stored[index].clone();
                        sqlx::query(
                            "INSERT INTO archava_purchasing.receipt_lines(tenant_id,request_id,order_id,line_id,product_id,accepted,rejected) \
                             VALUES($1,$2,$3,$4,$5,$6,$7)",
                        )
                        .bind(tenant)
                        .bind(parse_uuid(&mutation.request_id)?)
                        .bind(order.id)
                        .bind(line.id)
                        .bind(line.product_id)
                        .bind(int4(finding.accepted)?)
                        .bind(int4(finding.rejected)?)
                        .execute(&mut *connection)
                        .await
                        .map_err(refuses)?;
                        sqlx::query(
                            "UPDATE archava_purchasing.lines SET \
                             accepted_quantity=accepted_quantity+$3,rejected_quantity=rejected_quantity+$4 \
                             WHERE tenant_id=$1 AND id=$2",
                        )
                        .bind(tenant)
                        .bind(line.id)
                        .bind(int4(finding.accepted)?)
                        .bind(int4(finding.rejected)?)
                        .execute(&mut *connection)
                        .await
                        .map_err(refuses)?;
                        if finding.accepted > 0 {
                            stock.push(InboundLine {
                                product_id: line.product_id.to_string(),
                                quantity: finding.accepted,
                                unit_cost_minor: Some(line.unit_cost),
                                landed_fee_minor: costs
                                    .iter()
                                    .find(|cost| cost.id == finding.line_id)
                                    .expect("allocated accepted line")
                                    .fee_minor,
                            });
                        }
                        stored[index].accepted_quantity += finding.accepted;
                        stored[index].rejected_quantity += finding.rejected;
                    }
                    if stock.len() > 50 {
                        // The kernel input's own bound; the findings are
                        // already ≤ 50 so this is belt and braces.
                        return Err(PurchaseError::Domain(ErpError::Conflict));
                    }
                    receive_purchase_stock(
                        connection,
                        tenant,
                        ReceiveStockInput {
                            request_id: parse_uuid(&mutation.request_id)?,
                            actor_id: actor_id.to_string(),
                            fingerprint: fingerprint.to_string(),
                            purchase_id: order.id,
                            purchase_revision: order.revision + 1,
                            reason: reason.clone(),
                            lines: stock,
                        },
                    )
                    .await?;
                    // The receipt completes the line exactly when the
                    // accepted+rejected pair reaches the ordered quantity.
                    if stored
                        .iter()
                        .all(|line| line.completion_total() == line.quantity)
                    {
                        "received".to_string()
                    } else {
                        "partial".to_string()
                    }
                }
                _ => unreachable!("matched by the outer arm"),
            };
            sqlx::query(
                "UPDATE archava_purchasing.orders SET \
                 state=$3,confirmed_at=CASE WHEN $3='confirmed' THEN clock_timestamp() ELSE confirmed_at END,\
                 revision=revision+1,updated_at=clock_timestamp() \
                 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(order.id)
            .bind(state.as_str())
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((order.id, order.revision + 1))
        }
    }
}

impl PurchaseLineRow {
    /// accepted+rejected — the completion tally per line.
    fn completion_total(&self) -> i64 {
        self.accepted_quantity + self.rejected_quantity
    }
}

/// Bounded operator snapshot, read under the same locked grants as purchasing commands.
pub async fn workspace_view(
    connection: &mut PgConnection,
    tenant: &str,
    context: &ResolvedPurchaseContext,
    search: &str,
) -> Result<serde_json::Value, PurchaseError> {
    workspace_view_scoped(connection, tenant, context, search, None).await
}
pub async fn workspace_view_scoped(
    connection: &mut PgConnection,
    tenant: &str,
    context: &ResolvedPurchaseContext,
    search: &str,
    company: Option<Uuid>,
) -> Result<serde_json::Value, PurchaseError> {
    workspace_view_filtered(connection, tenant, context, search, company, None).await
}
pub async fn workspace_view_filtered(
    connection: &mut PgConnection,
    tenant: &str,
    context: &ResolvedPurchaseContext,
    search: &str,
    company: Option<Uuid>,
    id: Option<Uuid>,
) -> Result<serde_json::Value, PurchaseError> {
    let access = context.context().narrowed();
    let holds = |p: &str| access.purchasing.permissions.iter().any(|v| v == p);
    if !holds("suppliers.read") && !holds("offers.read") && !holds("orders.read") {
        return Err(ErpError::Forbidden.into());
    }
    let suppliers = if holds("suppliers.read") {
        sqlx::query_scalar::<_,serde_json::Value>("SELECT json_build_object('id',id,'code',code,'name',name,'active',active,'revision',revision,'supplierContactId',supplier_contact_id,'supplierContactRevision',supplier_contact_revision,'supplierPartyId',supplier_party_id,'supplierPartyRevision',supplier_party_revision) FROM archava_purchasing.suppliers WHERE tenant_id=$1 AND ($3::uuid IS NULL OR id=$3) AND ($2='' OR strpos(lower(name||' '||code),lower($2))>0) ORDER BY created_at DESC,id LIMIT 100").bind(tenant).bind(search).bind(id).fetch_all(&mut *connection).await.map_err(|e|purchase_database_error(PURCHASING_LEDGER,e))?
    } else {
        vec![]
    };
    let offers = if holds("offers.read") {
        sqlx::query_scalar::<_,serde_json::Value>("SELECT json_build_object('id',o.id,'supplierId',o.supplier_id,'supplierName',s.name,'productId',o.product_id,'productTitle',p.title,'companyId',to_jsonb(p)->'company_id','unitCost',o.unit_cost,'currency',o.currency,'active',o.active,'revision',o.revision) FROM archava_purchasing.offers o JOIN archava_purchasing.suppliers s ON s.tenant_id=o.tenant_id AND s.id=o.supplier_id JOIN archava_commerce.products p ON p.tenant_id=o.tenant_id AND p.id=o.product_id WHERE o.tenant_id=$1 AND (to_jsonb(p)->>'company_id')::uuid IS NOT DISTINCT FROM $3::uuid AND ($4::uuid IS NULL OR o.id=$4) AND ($2='' OR strpos(lower(s.name||' '||p.title),lower($2))>0) ORDER BY o.created_at DESC,o.id LIMIT 100").bind(tenant).bind(search).bind(company).bind(id).fetch_all(&mut *connection).await.map_err(|e|purchase_database_error(PURCHASING_LEDGER,e))?
    } else {
        vec![]
    };
    let orders = if holds("orders.read") {
        sqlx::query_scalar::<_,serde_json::Value>("SELECT json_build_object('id',o.id,'supplierId',o.supplier_id,'supplierContactId',o.supplier_contact_id,'supplierContactRevision',o.supplier_contact_revision,'supplierPartyId',o.supplier_party_id,'supplierPartyRevision',o.supplier_party_revision,'supplierName',o.supplier_name,'companyId',to_jsonb(o)->'company_id','companyRevision',to_jsonb(o)->'company_revision','companyCode',to_jsonb(o)->'company_code','companyName',to_jsonb(o)->'company_name','state',o.state,'total',o.total_estimate,'currency',o.currency,'revision',o.revision,'ownerId',o.owner_id,'lines',coalesce((SELECT json_agg(json_build_object('id',l.id,'offerId',l.offer_id,'productId',l.product_id,'title',l.title,'quantity',l.quantity,'acceptedQuantity',l.accepted_quantity,'rejectedQuantity',l.rejected_quantity,'unitCost',l.unit_cost) ORDER BY l.id) FROM archava_purchasing.lines l WHERE l.tenant_id=o.tenant_id AND l.order_id=o.id),'[]'::json)) FROM archava_purchasing.orders o WHERE o.tenant_id=$1 AND (to_jsonb(o)->>'company_id')::uuid IS NOT DISTINCT FROM $5::uuid AND ($2 OR o.owner_id=$3) AND ($6::uuid IS NULL OR o.id=$6) AND ($4='' OR strpos(lower(o.supplier_name),lower($4))>0) ORDER BY o.created_at DESC,o.id LIMIT 100").bind(tenant).bind(access.purchasing.scope==archava_domain::erp::ErpScope::All).bind(&access.purchasing.owner_id).bind(search).bind(company).bind(id).fetch_all(&mut *connection).await.map_err(|e|purchase_database_error(PURCHASING_LEDGER,e))?
    } else {
        vec![]
    };
    let documents =
        super::supplier_documents::workspace(connection, tenant, context, search, company, id)
            .await?;
    // The input tax codes a supplier document of the selected company may name
    // (TAX.md stage D): read here so the supplier documents screen needs no
    // Accounting grant.
    let tax_codes =
        super::supplier_documents::input_tax_codes(connection, tenant, context, company).await?;
    Ok(
        serde_json::json!({"suppliers":suppliers,"offers":offers,"orders":orders,"documents":documents,"taxCodes":tax_codes,"taxNotice":super::supplier_documents::TAX_NOTICE,"pageLimit":100,"permissions":access.purchasing.permissions,"scope":access.purchasing.scope}),
    )
}
