//! The commerce kernel primitives the ERP stock cohorts reconcile against —
//! the port of the parts of `packages/commerce-pg/src/kernel.ts` and the
//! product slice of `packages/commerce-pg/src/repository.ts` that a
//! purchasing receipt shares ONE transaction with.
//!
//! ## Why this module exists at all
//!
//! The `receipt_complete` trigger on `archava_purchasing.receipts` checks
//! `archava_commerce.commands`, `archava_commerce.movements` and the
//! purchasing receipt against each other inside ONE transaction. That makes
//! "stock changed exactly once" a database property — and the Rust executor
//! of the purchasing half must therefore write the commerce rows *in the
//! same transaction*, through the same guards the TS kernel applies: product
//! lock ordering, `track_stock`, the cell ceilings, the claim pattern.
//! Anything less would be a lie about where the single stock write happens.
//!
//! Every domain call re-reads the tenant setting through the caller's
//! `checked_executor`, the port of the baseline's per-function
//! `context(client, tenant)`.
//!
//! ## Deliberately not here (next packet, same contract sources)
//!
//! The commerce order surface (`order.create`, `reserve`, `fulfill`,
//! `cancel`, `return`) and the managed-menu foodservice ownerModule variant
//! of [`lock_commerce_products`] are ported with their own cohorts. Half-
//! porting them now with invented shapes would be exactly the drift
//! `KNOWN_DIFFERENCES.md` exists to prevent.

use archava_domain::erp::purchasing::{
    CommerceAccess, PrincipalKind, PurchasePrincipal, COST_MAX, REVISION_MAX, TOTAL_MAX,
};
use archava_domain::erp::stock::{CommerceCommandReceipt, STOCK_QTY_MAX};
use archava_domain::erp::{valid_principal_id, ErpError, ErpMembershipKind, ErpScope};
use sqlx::types::Uuid;
use sqlx::PgConnection;

use super::{
    purchase_database_error, PurchaseError, COMMERCE_LEDGER, COMMERCE_NAMESPACE,
    COMMERCE_PERMISSION_VOCABULARY,
};

fn int4(value: i64) -> Result<i32, PurchaseError> {
    i32::try_from(value).map_err(|_| PurchaseError::Domain(ErpError::Conflict))
}

// --- Row grammar -----------------------------------------------------------

/// A commerce product row after the row-level grammar the baseline's
/// `productSchema.parse` applies. One struct, because every caller below
/// needs the same subset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductRow {
    pub company_id: Option<Uuid>,
    pub id: Uuid,
    pub sku: String,
    pub title: String,
    pub unit: String,
    pub track_stock: bool,
    pub price: i64,
    pub currency: String,
    pub active: bool,
    pub revision: i64,
}

/// `productSchema.parse(row)` — the row-side guard. SKU pattern, title bound,
/// unit vocabulary, price cell and the revision cell repeat the migration's
/// CHECKs so a drift between table and code fails closed HERE instead of
/// surfacing from a trigger later.
fn parse_product_row(
    row: &sqlx::postgres::PgRow,
    tenant: &str,
) -> Result<ProductRow, PurchaseError> {
    use sqlx::Row;
    let store_tenant: String = row
        .try_get("tenant_id")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if store_tenant != tenant {
        // RLS makes this impossible in a correct transaction; a mismatch is
        // a cohort failure the refusal must not paper over.
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let id: Uuid = row
        .try_get("id")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    let sku: String = row
        .try_get("sku")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if !archava_domain::erp::valid_code(&sku) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let title: String = row
        .try_get("title")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if title.trim_matches(|c: char| c.is_whitespace()).is_empty() || title.len() > 160 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let unit: String = row
        .try_get("unit")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if !matches!(unit.as_str(), "unit" | "gram" | "millilitre") {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let price: i32 = row
        .try_get("price")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if price < 0 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let currency: String = row
        .try_get("currency")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if currency != "IDR" {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let track_stock: bool = row
        .try_get("track_stock")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    let active: bool = row
        .try_get("active")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    let revision: i32 = row
        .try_get("revision")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    let revision = i64::from(revision);
    if !(1..=REVISION_MAX).contains(&revision) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    Ok(ProductRow {
        company_id: super::company_trade::row_company(row)?,
        id,
        sku,
        title,
        unit,
        track_stock,
        price: i64::from(price),
        currency,
        active,
        revision,
    })
}

/// `lockCommerceProducts` — exact port with the commerce owner-module: the
/// ids deduplicate and sort like the TS set, every row must exist and parse,
/// and the `FOR UPDATE` IS the lock whose timeout the shared cohort's
/// `SET LOCAL lock_timeout` bounds.
///
/// The baseline binds the owner-module filter as `(managed_by IS NULL OR
/// managed_by=$3)` with `$3=NULL` for the commerce module; the in-line form
/// here is that expression evaluated at bind time, plus a comment because
/// the foodservice variant changes the predicate, not the lock style.
pub async fn lock_commerce_products(
    connection: &mut PgConnection,
    tenant: &str,
    ids: &[Uuid],
) -> Result<Vec<ProductRow>, PurchaseError> {
    lock_commerce_products_for(connection, tenant, ids, "commerce").await
}

/// The full product-lock primitive with the owner-module filter the TS
/// kernel carries (`lockCommerceProducts(…, ownerModule)`): `commerce` locks
/// only unmanaged products, `foodservice` additionally locks the
/// `managed_by='foodservice'` portion products it owns.
pub async fn lock_commerce_products_for(
    connection: &mut PgConnection,
    tenant: &str,
    ids: &[Uuid],
    owner_module: &str,
) -> Result<Vec<ProductRow>, PurchaseError> {
    lock_commerce_products_scoped(connection, tenant, ids, owner_module, None).await
}
pub async fn lock_commerce_products_scoped(
    connection: &mut PgConnection,
    tenant: &str,
    ids: &[Uuid],
    owner_module: &str,
    company: Option<Uuid>,
) -> Result<Vec<ProductRow>, PurchaseError> {
    verify_commerce_context(connection, tenant).await?;
    if owner_module != "commerce" && owner_module != "foodservice" {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let mut unique: Vec<Uuid> = ids.to_vec();
    unique.sort();
    unique.dedup();
    if unique.is_empty() || unique.len() > 150 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    // `managed_by IS NULL OR managed_by = $3` — the TS filter with the enum
    // parsed before the query, so an unknown module never reaches SQL.
    let managed_filter = if owner_module == "foodservice" {
        "AND (managed_by IS NULL OR managed_by = $3)"
    } else {
        "AND managed_by IS NULL AND $3::text='commerce'"
    };
    let lock_statement = format!(
        "SELECT * FROM archava_commerce.products WHERE tenant_id=$1 \
         AND id=ANY($2) {managed_filter} AND (to_jsonb(products)->>'company_id')::uuid IS NOT DISTINCT FROM $4::uuid ORDER BY id FOR UPDATE"
    );
    // sqlx 0.9 refuses a dynamic string unless the caller asserts it has been
    // audited. The audit is the two-branch enum check above: the only bytes of
    // `lock_statement` that are not literals came from `managed_filter`, whose
    // two possible strings are fixed right here. If a third branch is ever
    // added, it must be audited the same way before this wrapper.
    let rows = sqlx::query(sqlx::AssertSqlSafe(lock_statement.as_str()))
        .bind(tenant)
        .bind(&unique)
        .bind(owner_module)
        .bind(company)
        .fetch_all(&mut *connection)
        .await
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    let parsed: Vec<ProductRow> = rows
        .iter()
        .map(|row| parse_product_row(row, tenant))
        .collect::<Result<_, _>>()?;
    if parsed.len() != unique.len() {
        return Err(PurchaseError::Domain(ErpError::NotFound));
    }
    Ok(parsed)
}

/// `context(client, tenant)` — the kernel's caller-owns-the-tenant-guard.
/// Every kernel primitive runs inside the caller's tenant transaction, and
/// the guard proves the transaction actually set `archava.tenant_id` to the
/// tenant on the wire before any row lock reads or writes. A mismatch is a
/// transaction-contract break, not a caller-invented condition: the SQL
/// adapter stops the transaction, the refusal surfaces through the ordinary
/// conflict path, and nothing partial can commit.
pub(crate) async fn verify_commerce_context(
    connection: &mut PgConnection,
    tenant: &str,
) -> Result<(), PurchaseError> {
    if !archava_domain::erp::valid_principal_id(tenant) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let found: Option<(String,)> =
        sqlx::query_as("SELECT current_setting('archava.tenant_id',true)::text")
            .fetch_optional(&mut *connection)
            .await
            .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if found.map(|(tenant,)| tenant).as_deref() != Some(tenant) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    Ok(())
}

/// `commerceClock` — one RFC 3339 instant from the database itself, so the
/// stock read's `observed` field can never disagree with the row it
/// evaluated. Rendered through `to_jsonb(...)#>>'{}'`, the same cast the
/// host's CRM read path uses for instants.
pub async fn commerce_clock(connection: &mut PgConnection) -> Result<String, PurchaseError> {
    let instant: String = sqlx::query_as("SELECT to_jsonb(clock_timestamp())#>>'{}' AS observed")
        .fetch_one(&mut *connection)
        .await
        .map(|(instant,)| instant)
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    Ok(instant)
}

/// `readCommerceStock` — the movement-derived stock snapshot: on hand, the
/// held-allocation reservation as of `observed`, and the balance revision
/// the receipts cell-check against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StockRecord {
    pub on_hand: i64,
    pub reserved: i64,
    pub available: i64,
    pub revision: i64,
}

pub async fn read_commerce_stock(
    connection: &mut PgConnection,
    tenant: &str,
    product_id: Uuid,
    observed: &str,
) -> Result<StockRecord, PurchaseError> {
    read_commerce_stock_scoped(connection, tenant, product_id, observed, None).await
}
pub async fn read_commerce_stock_scoped(
    connection: &mut PgConnection,
    tenant: &str,
    product_id: Uuid,
    observed: &str,
    company: Option<Uuid>,
) -> Result<StockRecord, PurchaseError> {
    let row: (i32, i64, i32) = sqlx::query_as(
        "SELECT b.quantity,coalesce((SELECT sum(a.quantity) FROM archava_commerce.allocations a \
         WHERE a.tenant_id=b.tenant_id AND a.product_id=b.product_id AND a.state='held' \
         AND a.expires_at>$3::timestamptz),0)::int8 AS reserved,b.revision \
         FROM archava_commerce.stock_balances b \
         WHERE b.tenant_id=$1 AND b.product_id=$2 AND EXISTS(SELECT 1 FROM archava_commerce.products p WHERE p.tenant_id=b.tenant_id AND p.id=b.product_id AND (to_jsonb(p)->>'company_id')::uuid IS NOT DISTINCT FROM $4::uuid)",
    )
    .bind(tenant)
    .bind(product_id)
    .bind(observed)
    .bind(company)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?
    .ok_or(PurchaseError::Domain(ErpError::NotFound))?;
    let (qty, reserved, revision) = (i64::from(row.0), row.1, i64::from(row.2));
    let cell = |value: i64| -> Result<(), PurchaseError> {
        if !(0..=STOCK_QTY_MAX).contains(&value) {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        }
        Ok(())
    };
    cell(qty)?;
    cell(reserved)?;
    if !(1..=REVISION_MAX).contains(&revision) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    Ok(StockRecord {
        on_hand: qty,
        reserved,
        available: qty - reserved,
        revision,
    })
}

// --- The inbound receipt: `receivePurchaseStock`, exact port ----------------

/// One strict entry the purchasing branch receives. Typed by the caller from
/// already-validated order state; the bounds are re-checked here because
/// THIS function is what both ledgers' triggers will treat as the claim.
#[derive(Debug, Clone)]
pub struct ReceiveStockInput {
    pub request_id: Uuid,
    pub actor_id: String,
    pub fingerprint: String,
    pub purchase_id: Uuid,
    pub purchase_revision: i64,
    pub reason: String,
    pub lines: Vec<archava_domain::erp::stock::InboundLine>,
}

/// The baseline's fingerprint regex — `^[a-f0-9]{64}$` — as a predicate.
pub fn valid_fingerprint(fingerprint: &str) -> bool {
    fingerprint.len() == 64
        && fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (byte.is_ascii_lowercase() && byte <= b'f'))
}

fn valid_receive_input(input: &ReceiveStockInput) -> Result<(), PurchaseError> {
    // `actorId` — the commerce principal shape (`commerceIdSchema`), the
    // same kebab slug the tenants use.
    if !valid_principal_id(&input.actor_id) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    if !valid_fingerprint(&input.fingerprint) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    if input.purchase_revision < 1 || input.purchase_revision > REVISION_MAX {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    if input
        .reason
        .trim_matches(|c: char| c.is_whitespace())
        .is_empty()
        || input.reason.len() > 2000
    {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    if input.lines.len() > 50 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let mut seen: Vec<String> = Vec::with_capacity(input.lines.len());
    for line in &input.lines {
        if line.quantity < 1
            || line.quantity > 1_000_000
            || !line
                .unit_cost_minor
                .is_some_and(|c| (0..=1_000_000_000).contains(&c))
            || !(0..=500_000_000_000_000).contains(&line.landed_fee_minor)
        {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        }
        if seen.contains(&line.product_id) {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        }
        seen.push(line.product_id.clone());
    }
    Ok(())
}

/// Claims the commerce command row for one request id: `ON CONFLICT DO
/// NOTHING RETURNING` — `true` when THIS call owns the claim, `false` when
/// the id is already taken (the caller refuses; a claim loser must never
/// write a second receipt under the same id).
pub(crate) async fn claim_commerce_command(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    actor_id: &str,
    fingerprint: &str,
) -> Result<bool, PurchaseError> {
    let claimed: Option<(Uuid,)> = sqlx::query_as(
        "INSERT INTO archava_commerce.commands(tenant_id,request_id,actor_id,fingerprint) \
         VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING RETURNING request_id",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(actor_id)
    .bind(fingerprint)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    Ok(claimed.is_some())
}

/// The commerce ledger's audit row — the operation names, record id and
/// revision the `command_complete` trigger later reconciles against
/// `commands.receipt`.
///
/// Eight parameters is the SQL row's own arity — one per bound column, in the
/// row grammar's order — so the crate's usual allow for table writers applies.
#[allow(clippy::too_many_arguments)]
pub(super) async fn insert_commerce_audit(
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
        "INSERT INTO archava_commerce.audit(tenant_id,request_id,actor_id,operation,record_id,revision,reason) \
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
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    Ok(())
}

/// Writes the inbound half of a purchasing receipt into the commerce ledger —
/// claim, receipt binding, movements and audit — inside the caller's ONE
/// transaction. The triggers on both ledgers refuse the receipt if any half
/// of this disagrees (`Physical receipt has no complete cross-domain audit`).
/// `input.lines` must be unique by product id; a duplicate would move stock
/// twice under one claim, and the baseline refuses that shape at parse time.
pub async fn receive_purchase_stock(
    connection: &mut PgConnection,
    tenant: &str,
    input: ReceiveStockInput,
) -> Result<(), PurchaseError> {
    valid_receive_input(&input)?;
    let company = super::company_trade::record_company(
        connection,
        tenant,
        super::company_trade::Record::Purchase,
        input.purchase_id,
    )
    .await?;

    // Claim the commerce command row for THIS request id. The TS kernel
    // refuses with conflict when the claim loses (`rowCount !== 1`) — the
    // request id is already recorded, possibly by a sibling ledger surface.
    let claimed = claim_commerce_command(
        connection,
        tenant,
        input.request_id,
        &input.actor_id,
        &input.fingerprint,
    )
    .await?;
    if !claimed {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }

    // The line product ids are uuid SHAPES validated above; parse them once
    // here so every later bind is typed `uuid` — no string round-trip
    // through a typed column.
    let moved: Vec<(Uuid, i64, i64, i64)> = input
        .lines
        .iter()
        .map(|line| {
            let product_id = Uuid::parse_str(&line.product_id)
                .map_err(|_| PurchaseError::Domain(ErpError::Conflict))?;
            Ok((
                product_id,
                line.quantity,
                line.unit_cost_minor
                    .ok_or(PurchaseError::Domain(ErpError::Conflict))?,
                line.landed_fee_minor,
            ))
        })
        .collect::<Result<_, PurchaseError>>()?;

    // Lock the products the lines move stock for; they must all exist and
    // track stock — a menu product or a non-tracked one makes the receipt a
    // lie, and `complete_purchase_stock` would refuse it anyway.
    let ids: Vec<Uuid> = moved.iter().map(|(id, _, _, _)| *id).collect();
    let products = if ids.is_empty() {
        Vec::new()
    } else {
        lock_commerce_products_scoped(connection, tenant, &ids, "commerce", company).await?
    };
    if products.iter().any(|product| !product.track_stock) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }

    // Bind the purchasing-side receipt document.
    let receipt = serde_json::json!({
        "domain": "purchasing",
        "id": input.purchase_id,
        "revision": input.purchase_revision,
        "lines": input.lines
            .iter()
            .map(|line| serde_json::json!({
                "productId": line.product_id,
                "quantity": line.quantity,
                "unitCostMinor":line.unit_cost_minor,
                "landedFeeMinor":line.landed_fee_minor,
            }))
            .collect::<Vec<serde_json::Value>>(),
    });
    sqlx::query(
        "UPDATE archava_commerce.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(input.request_id)
    .bind(&receipt)
    .execute(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;

    for (product_id, quantity, unit_cost, landed_fee) in &moved {
        // The stock read happens per product exactly like the TS loop; the
        // cell ceiling and the revision ceiling are the trigger's own.
        let clock = commerce_clock(connection).await?;
        let stock =
            read_commerce_stock_scoped(connection, tenant, *product_id, &clock, company).await?;
        if stock.on_hand + quantity > STOCK_QTY_MAX || stock.revision >= REVISION_MAX {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        }
        // One movement row per accepted line, quantity exactly `accepted` —
        // the count and the amounts the receipt trigger reconciles.
        sqlx::query(
            "INSERT INTO archava_commerce.movements(tenant_id,request_id,product_id,kind,quantity,reason,purchase_id,unit_cost_minor,landed_fee_minor) \
             VALUES($1,$2,$3,'purchase',$4,$5,$6,$7,$8)",
        )
        .bind(tenant)
        .bind(input.request_id)
        .bind(product_id)
        .bind(i32::try_from(*quantity).map_err(|_| PurchaseError::Domain(ErpError::Conflict))?)
        .bind(input.reason.as_str())
        .bind(input.purchase_id)
        .bind(unit_cost).bind(landed_fee)
        .execute(&mut *connection)
        .await
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    }

    insert_commerce_audit(
        connection,
        tenant,
        input.request_id,
        &input.actor_id,
        "purchase.receive",
        input.purchase_id,
        input.purchase_revision,
        Some(&input.reason),
    )
    .await
}

// --- Membership resolution (the commerce side of every ERP context) --------

/// One ledger's loaded membership row, parsed with the vocabulary gate —
/// the exact contract `member()` applies in both TS repositories: unknown
/// permission values are outside the CHECK's array, so a row carrying one
/// refuses closed instead of being silently judged by string equality.
#[derive(Debug, Clone)]
pub struct MembershipRow {
    pub kind: ErpMembershipKind,
    pub permissions: Vec<String>,
    pub scope: ErpScope,
    pub active: bool,
}

pub(crate) async fn load_locked_membership(
    connection: &mut PgConnection,
    namespace: &'static str,
    ledger: &'static str,
    tenant: &str,
    principal: &str,
    vocabulary: &'static [&'static str],
) -> Result<Option<MembershipRow>, PurchaseError> {
    // The namespace reaches SQL only after this constant check — the shared
    // rls `valid_namespace` rule applied to the one interpolated statement
    // this loader has. Both call sites pass crate constants.
    if !namespace
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let statement =
        format!("SELECT permissions,kind,scope,active FROM {namespace}.lock_membership($1,$2)");
    let row = sqlx::query_as::<_, (Vec<String>, String, String, bool)>(sqlx::AssertSqlSafe(
        statement.as_str(),
    ))
    .bind(tenant)
    .bind(principal)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(ledger, error))?;
    let Some((permissions, kind, scope, active)) = row else {
        return Ok(None);
    };
    if permissions
        .iter()
        .any(|held| !vocabulary.contains(&held.as_str()))
    {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    let Some(kind) = ErpMembershipKind::parse(&kind) else {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    };
    let Some(scope) = ErpScope::parse(&scope) else {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    };
    Ok(Some(MembershipRow {
        kind,
        permissions,
        scope,
        active,
    }))
}

/// The commerce ledger's resolved context — commerce's own `transaction()`
/// shape (ONE ledger's membership, service principals narrowing to the human
/// they name). The purchasing flow loads BOTH ledgers inside the purchasing
/// transaction; the product command opens its own commerce-ledger cohort.
#[derive(Debug, Clone)]
pub struct CommerceContext {
    pub access: CommerceAccess,
    /// `ownerId` — the human whose id narrows `assigned` reads; the actor's
    /// own principal for humans, the named human for services.
    pub owner_id: String,
}

/// Loads the membership row(s) and applies the exact shape of the TS
/// commerce `transaction()`'s member resolution: active required, kind equal
/// to the actor's own, intersection for services, human owner id.
pub async fn resolve_commerce_context(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<CommerceContext, PurchaseError> {
    let own = load_locked_membership(
        connection,
        COMMERCE_NAMESPACE,
        COMMERCE_LEDGER,
        tenant,
        &principal.id,
        COMMERCE_PERMISSION_VOCABULARY,
    )
    .await?
    .ok_or(PurchaseError::Domain(ErpError::Forbidden))?;
    if !own.active {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    if own.kind.as_str() != principal.kind.as_str() {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    let human = match principal.kind {
        PrincipalKind::Human => None,
        PrincipalKind::Service => {
            let on_behalf_of = principal
                .on_behalf_of
                .as_deref()
                .ok_or(PurchaseError::Domain(ErpError::Forbidden))?;
            let human = load_locked_membership(
                connection,
                COMMERCE_NAMESPACE,
                COMMERCE_LEDGER,
                tenant,
                on_behalf_of,
                COMMERCE_PERMISSION_VOCABULARY,
            )
            .await?
            .ok_or(PurchaseError::Domain(ErpError::Forbidden))?;
            if human.kind != ErpMembershipKind::Human {
                return Err(PurchaseError::Domain(ErpError::Forbidden));
            }
            if !human.active {
                return Err(PurchaseError::Domain(ErpError::Forbidden));
            }
            Some(human)
        }
    };
    let (permissions, scope, owner_id) = match &human {
        Some(human) => (
            own.permissions
                .iter()
                .filter(|held| human.permissions.contains(*held))
                .cloned()
                .collect::<Vec<String>>(),
            match (own.scope, human.scope) {
                (ErpScope::Assigned, _) | (_, ErpScope::Assigned) => ErpScope::Assigned,
                (ErpScope::All, ErpScope::All) => ErpScope::All,
            },
            // `principal.id` lookup already proved the named human — the
            // context narrows to them, matching the TS `ownerId` swap.
            principal
                .on_behalf_of
                .clone()
                .expect("service principal carries onBehalfOf when its human is present"),
        ),
        None => (own.permissions.clone(), own.scope, principal.id.clone()),
    };
    Ok(CommerceContext {
        access: CommerceAccess { permissions, scope },
        owner_id,
    })
}

// --- The real product.create path (the commerce repository's slice) --------

/// The strict quick parse of a `product.create` input — the fields the TS
/// union member caries. Validated HERE so the INSERT below can never carry a
/// value the schema would refuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductCreateInput {
    pub company: Option<(super::company_trade::CompanyStamp, i64)>,
    pub sku: String,
    pub title: String,
    pub unit: String,
    pub track_stock: bool,
    pub price: i64,
}

pub fn valid_product_create_input(input: &ProductCreateInput) -> Result<(), PurchaseError> {
    if !archava_domain::erp::valid_code(&input.sku) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    // The schema's trim-min-max, applied to the VALUE the caller passes (the
    // caller builds this struct from already-trimmed parses; re-checking
    // keeps the invariant local).
    if input
        .title
        .trim_matches(|c: char| c.is_whitespace())
        .is_empty()
        || input.title.len() > 160
    {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    if !matches!(input.unit.as_str(), "unit" | "gram" | "millilitre") {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    if input.price < 0 || input.price > archava_domain::erp::purchasing::COST_MAX {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    Ok(())
}

/// Fetches an existing commerce command row: `(actor_id, fingerprint,
/// receipt)` or `None`.
async fn fetch_commerce_command(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
) -> Result<Option<(String, String, Option<serde_json::Value>)>, PurchaseError> {
    sqlx::query_as(
        "SELECT actor_id,fingerprint,receipt FROM archava_commerce.commands \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))
}

/// The commerce `receiptAccess` — what recovery re-verification requires so
/// a stored receipt's record is still reachable under the CURRENT access.
/// The `order` branch ports `ownedOrder` (orders.read, scope rule); the
/// product/stock branch requires catalog.read, and for stock also
/// stock.read, with the product still present and unmanaged.
async fn commerce_receipt_access(
    connection: &mut PgConnection,
    tenant: &str,
    context: &CommerceContext,
    receipt: &CommerceCommandReceipt,
) -> Result<(), PurchaseError> {
    if receipt.kind == "order" {
        let Ok(id) = Uuid::parse_str(&receipt.id) else {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        };
        if !context.access.holds(&["orders.read"]) {
            return Err(PurchaseError::Domain(ErpError::Forbidden));
        }
        let found: Option<i32> = sqlx::query_scalar(
            "SELECT 1 FROM archava_commerce.orders r WHERE tenant_id=$1 AND id=$2 \
             AND ($3::boolean OR owner_id=$4) AND (to_jsonb(r)->>'company_id') IS NULL",
        )
        .bind(tenant)
        .bind(id)
        .bind(context.access.scope == ErpScope::All)
        .bind(&context.owner_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
        if found.is_none() {
            return Err(PurchaseError::Domain(ErpError::NotFound));
        }
        return Ok(());
    }
    if !context.access.holds(&["catalog.read"]) {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    if receipt.kind == "stock" && !context.access.holds(&["stock.read"]) {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    let Ok(id) = Uuid::parse_str(&receipt.id) else {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    };
    let found: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM archava_commerce.products r WHERE tenant_id=$1 AND id=$2 AND managed_by IS NULL AND (to_jsonb(r)->>'company_id') IS NULL",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if found.is_none() {
        return Err(PurchaseError::Domain(ErpError::NotFound));
    }
    Ok(())
}

/// The real `product.create` path — the commerce repository's claim →
/// product insert → audit → command-receipt update pipeline for ONE
/// operation, so the ERP flow never seeds catalog rows by hand.
///
/// Returns `(product_id, revision)` from the recorded receipt on both the
/// fresh and the replayed branch. Refusals:
/// - not the `catalog.write`-granted, global-scope context (the baseline's
///   transaction permission + `globalAccess` + `catalog.read`),
/// - claim lost with a different fingerprint or actor,
/// - a stored receipt that belongs to the purchasing domain (those surface
///   through the PURCHASING side only),
/// - a stored receipt whose record has vanished.
pub async fn record_product_command(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    context: &CommerceContext,
    request_id: Uuid,
    fingerprint: &str,
    input: ProductCreateInput,
) -> Result<(Uuid, i64), PurchaseError> {
    valid_product_create_input(&input)?;
    // The transaction's permission gate (one permission, held).
    if !context.access.holds(&["catalog.write"]) {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    // `globalAccess` + `catalog.read`: catalog administration is a
    // tenant-wide act, never an assigned-scope one.
    if context.access.scope != ErpScope::All {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    if !context.access.holds(&["catalog.read"]) {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }

    let claimed =
        claim_commerce_command(connection, tenant, request_id, &principal.id, fingerprint).await?;
    if !claimed {
        // Recovery: the row MUST exist with the same actor and fingerprint.
        let (row_actor, row_fingerprint, row_receipt) =
            fetch_commerce_command(connection, tenant, request_id)
                .await?
                .ok_or(PurchaseError::Domain(ErpError::Conflict))?;
        if row_actor != principal.id || row_fingerprint != fingerprint {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        }
        if let Some(value) = &row_receipt {
            // The cross-ledger bound: a purchasing receipt surfaces through
            // its OWN domain only — `row.receipt.domain === 'purchasing'` is
            // refused here exactly as the TS status read refuses it.
            if value.get("domain").and_then(serde_json::Value::as_str) == Some("purchasing") {
                return Err(PurchaseError::Domain(ErpError::Forbidden));
            }
            let receipt = CommerceCommandReceipt::parse(value)?;
            if let Some((company, _)) = &input.company {
                let found = super::company_trade::record_company(
                    connection,
                    tenant,
                    super::company_trade::Record::Product,
                    Uuid::parse_str(&receipt.id)
                        .map_err(|_| PurchaseError::Domain(ErpError::Conflict))?,
                )
                .await?;
                if found != Some(company.id) {
                    return Err(PurchaseError::Domain(ErpError::Conflict));
                }
            } else {
                commerce_receipt_access(connection, tenant, context, &receipt).await?;
            }

            return receipt_product_identity(receipt);
        }
        // `commerceReceiptSchema.parse(null)` refuses — a claimed-but-unrecorded
        // commerce command has no replayable receipt.
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }

    if let Some((company, expected)) = &input.company {
        company.validate_new(*expected)?;
    }
    let product_id = Uuid::new_v4();
    let statement = if input.company.is_some() {
        "INSERT INTO archava_commerce.products(tenant_id,id,sku,title,unit,track_stock,price,company_id,company_revision,company_code,company_name) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)"
    } else {
        "INSERT INTO archava_commerce.products(tenant_id,id,sku,title,unit,track_stock,price) VALUES($1,$2,$3,$4,$5,$6,$7)"
    };
    let query = sqlx::query(statement)
        .bind(tenant)
        .bind(product_id)
        .bind(&input.sku)
        .bind(&input.title)
        .bind(&input.unit)
        .bind(input.track_stock)
        .bind(int4(input.price)?);
    let query = if let Some((company, _)) = &input.company {
        query
            .bind(company.id)
            .bind(company.revision)
            .bind(&company.code)
            .bind(&company.name)
    } else {
        query
    };
    query
        .execute(&mut *connection)
        .await
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;

    insert_commerce_audit(
        connection,
        tenant,
        request_id,
        &principal.id,
        if input.company.is_some() {
            "product.create_for_company"
        } else {
            "product.create"
        },
        product_id,
        1,
        None,
    )
    .await?;
    sqlx::query(
        "UPDATE archava_commerce.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(serde_json::json!({
        "kind": "product",
        "id": product_id,
        "revision": 1,
    }))
    .execute(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    Ok((product_id, 1))
}

/// A product-kind receipt becomes the (id, revision) the caller advances
/// with; any other kind at this return point is a caller bug, refused.
fn receipt_product_identity(receipt: CommerceCommandReceipt) -> Result<(Uuid, i64), PurchaseError> {
    if receipt.kind != "product" {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let Ok(id) = Uuid::parse_str(&receipt.id) else {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    };
    Ok((id, receipt.revision))
}

// --- The order-side port (the foodservice cohort consumes the same pool) ---
//
// Everything below ports `packages/commerce-pg/src/kernel.ts`'s order
// half: draft creation, reservation, consumption, cancellation and the
// food-owned menu-product primitives. Purchasing kept this file stock-only
// because its receipts flow inbound (`receivePurchaseStock`); the foodservice
// cohort reads and consumes outbound, which is the second writer the
// single-physical-stock-pool acceptance is about.

/// `OrderRow` — the order fields the kernel arms read. `expires_at` and
/// `fulfilled_at` ride as strings through the JSON instant cast the reads
/// use, because comparisons happen in SQL (`expires_at > $observed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderRow {
    pub company_id: Option<Uuid>,
    pub id: Uuid,
    pub owner_id: String,
    pub customer_name: String,
    pub state: String,
    pub managed_by: Option<String>,
    pub total_estimate: i64,
    pub revision: i64,
}

/// `CommerceError` carry-over for the ORDER carrier: the order row grammar
/// (state vocabulary, managed_by, revision cell) checked at the boundary so
/// drift fails closed here.
fn parse_order_row(
    row: &sqlx::postgres::PgRow,
    tenant: &str,
    order_id: Option<Uuid>,
) -> Result<OrderRow, PurchaseError> {
    use sqlx::Row;
    let store_tenant: String = row
        .try_get("tenant_id")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if store_tenant != tenant {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let id: Uuid = row
        .try_get("id")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if let Some(expected) = order_id {
        if id != expected {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        }
    }
    let owner_id: String = row
        .try_get("owner_id")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if !archava_domain::erp::valid_principal_id(&owner_id) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let customer_name: String = row
        .try_get("customer_name")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if customer_name
        .trim_matches(|c: char| c.is_whitespace())
        .is_empty()
        || customer_name.chars().count() > 160
    {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let state: String = row
        .try_get("state")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if !matches!(
        state.as_str(),
        "draft" | "reserved" | "fulfilled" | "cancelled" | "returned"
    ) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let managed_by: Option<String> = row
        .try_get("managed_by")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if managed_by
        .as_deref()
        .is_some_and(|module| module != "foodservice")
    {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let total_estimate: i64 = row
        .try_get("total_estimate")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if !(0..=TOTAL_MAX).contains(&total_estimate) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let revision: i32 = row
        .try_get::<i32, _>("revision")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    let revision = i64::from(revision);
    if !(1..=REVISION_MAX).contains(&revision) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    Ok(OrderRow {
        company_id: super::company_trade::row_company(row)?,
        id,
        owner_id,
        customer_name,
        state,
        managed_by,
        total_estimate,
        revision,
    })
}

/// `lockCommerceOrder` — the order row under `FOR UPDATE`, with the
/// owner-module boundary: only `foodservice` may drive a
/// `managed_by='foodservice'` order, and only the unmanaged side may be
/// driven without a module.
pub async fn lock_commerce_order(
    connection: &mut PgConnection,
    tenant: &str,
    order_id: Uuid,
    owner_module: &str,
) -> Result<OrderRow, PurchaseError> {
    if owner_module != "commerce" && owner_module != "foodservice" {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let row = sqlx::query(
        "SELECT * FROM archava_commerce.orders WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(|error| match error {
        sqlx::Error::RowNotFound => PurchaseError::Domain(ErpError::NotFound),
        other => purchase_database_error(COMMERCE_LEDGER, other),
    })?;
    let order = parse_order_row(&row, tenant, None)?;
    let expected = if owner_module == "foodservice" {
        Some("foodservice".to_string())
    } else {
        None
    };
    if order.managed_by != expected {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    Ok(order)
}

/// One quoted order line — the product snapshot the quote freezes and the
/// `order_lines` carry, with the id the line row is minted under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotedLine {
    pub line_id: Uuid,
    pub product_id: Uuid,
    pub sku: String,
    pub title: String,
    pub unit: String,
    pub track_stock: bool,
    pub quantity: i64,
    pub price: i64,
    pub product_revision: i64,
}

/// One draft line the caller submits — parsed against
/// `orderLineInputSchema` (`productId` uuid + `quantity` 1..=10000).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DraftLine {
    pub product_id: Uuid,
    pub quantity: i64,
}

/// `quoteCommerceLines` — locks the products, refuses inactive rows, and
/// returns the frozen line snapshot.
pub async fn quote_commerce_lines(
    connection: &mut PgConnection,
    tenant: &str,
    lines: &[DraftLine],
    owner_module: &str,
) -> Result<Vec<QuotedLine>, PurchaseError> {
    quote_commerce_lines_scoped(connection, tenant, lines, owner_module, None).await
}
pub async fn quote_commerce_lines_scoped(
    connection: &mut PgConnection,
    tenant: &str,
    lines: &[DraftLine],
    owner_module: &str,
    company: Option<Uuid>,
) -> Result<Vec<QuotedLine>, PurchaseError> {
    if lines.is_empty() || lines.len() > 50 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    for line in lines {
        if !(1..=10_000).contains(&line.quantity) {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        }
    }
    let mut seen: Vec<Uuid> = Vec::with_capacity(lines.len());
    for line in lines {
        if seen.contains(&line.product_id) {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        }
        seen.push(line.product_id);
    }
    let products =
        lock_commerce_products_scoped(connection, tenant, &seen.to_vec(), owner_module, company)
            .await?;
    let mut quoted = Vec::with_capacity(lines.len());
    for line in lines {
        let product = products
            .iter()
            .find(|product| product.id == line.product_id)
            .ok_or(PurchaseError::Domain(ErpError::Conflict))?;
        if !product.active {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        }
        quoted.push(QuotedLine {
            line_id: Uuid::new_v4(),
            product_id: product.id,
            sku: product.sku.clone(),
            title: product.title.clone(),
            unit: product.unit.clone(),
            track_stock: product.track_stock,
            quantity: line.quantity,
            price: product.price,
            product_revision: product.revision,
        });
    }
    Ok(quoted)
}

/// `insertCommerceLines` — the frozen line rows, checked aggregator UNIQUE
/// (tenant,order,product) upholds.
async fn insert_commerce_lines_internal(
    connection: &mut PgConnection,
    tenant: &str,
    order_id: Uuid,
    lines: &[QuotedLine],
) -> Result<(), PurchaseError> {
    verify_commerce_context(connection, tenant).await?;
    for line in lines {
        sqlx::query(
            "INSERT INTO archava_commerce.order_lines(tenant_id,id,order_id,product_id,sku,title,unit,track_stock,quantity,price,product_revision) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(tenant)
        .bind(line.line_id)
        .bind(order_id)
        .bind(line.product_id)
        .bind(&line.sku)
        .bind(&line.title)
        .bind(&line.unit)
        .bind(line.track_stock)
        .bind(int4(line.quantity)?)
        .bind(int4(line.price)?)
        .bind(int4(line.product_revision)?)
        .execute(&mut *connection)
        .await
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    }
    Ok(())
}

/// `createCommerceDraft` — the shared order header the modules mint (a
/// `managed_by` carrying the owning module when one exists), plus the quote
/// rows. Returns the id, revision, total and the frozen lines.
pub struct CommerceDraft {
    pub order_id: Uuid,
    pub revision: i64,
    pub total_estimate: i64,
    pub lines: Vec<QuotedLine>,
}

pub async fn create_commerce_draft(
    connection: &mut PgConnection,
    tenant: &str,
    owner_id: &str,
    customer_name: &str,
    managed_by: Option<&str>,
    lines: &[DraftLine],
) -> Result<CommerceDraft, PurchaseError> {
    create_commerce_draft_with_customer(
        connection,
        tenant,
        owner_id,
        CommerceCustomer {
            company: None,
            name: customer_name,
            contact: None,
            party: None,
        },
        managed_by,
        lines,
    )
    .await
}

/// A reviewed customer identity plus its historical display-name snapshot.
pub struct CommerceCustomer<'a> {
    pub company: Option<&'a super::company_trade::CompanyStamp>,
    pub name: &'a str,
    pub contact: Option<(Uuid, i32)>,
    pub party: Option<(Uuid, i32)>,
}

pub async fn create_commerce_draft_with_customer(
    connection: &mut PgConnection,
    tenant: &str,
    owner_id: &str,
    customer: CommerceCustomer<'_>,
    managed_by: Option<&str>,
    lines: &[DraftLine],
) -> Result<CommerceDraft, PurchaseError> {
    let customer_name = customer.name;
    verify_commerce_context(connection, tenant).await?;
    if !archava_domain::erp::valid_principal_id(owner_id) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    if let Some(module) = managed_by {
        if module != "foodservice" {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        }
    }
    if customer_name
        .trim_matches(|c: char| c.is_whitespace())
        .is_empty()
        || customer_name.chars().count() > 160
    {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let quoted = quote_commerce_lines_scoped(
        connection,
        tenant,
        lines,
        managed_by.unwrap_or("commerce"),
        customer.company.map(|c| c.id),
    )
    .await?;
    let mut total: i64 = 0;
    for line in &quoted {
        let line_total = line
            .price
            .checked_mul(line.quantity)
            .ok_or(PurchaseError::Domain(ErpError::Conflict))?;
        total = total
            .checked_add(line_total)
            .ok_or(PurchaseError::Domain(ErpError::Conflict))?;
    }
    if !(0..=TOTAL_MAX).contains(&total) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let order_id = Uuid::new_v4();
    let statement = if customer.company.is_some() {
        "INSERT INTO archava_commerce.orders(tenant_id,id,owner_id,customer_name,total_estimate,managed_by,customer_party_id,customer_party_revision,company_id,company_revision,company_code,company_name) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)"
    } else if customer.party.is_some() {
        "INSERT INTO archava_commerce.orders(tenant_id,id,owner_id,customer_name,total_estimate,managed_by,customer_party_id,customer_party_revision) VALUES($1,$2,$3,$4,$5,$6,$7,$8)"
    } else if customer.contact.is_some() {
        "INSERT INTO archava_commerce.orders(tenant_id,id,owner_id,customer_name,total_estimate,managed_by,customer_contact_id,customer_contact_revision) VALUES($1,$2,$3,$4,$5,$6,$7,$8)"
    } else {
        // The unlinked path also supports owned upgrade fixtures before migration 007.
        "INSERT INTO archava_commerce.orders(tenant_id,id,owner_id,customer_name,total_estimate,managed_by) VALUES($1,$2,$3,$4,$5,$6)"
    };
    let query = sqlx::query(statement)
        .bind(tenant)
        .bind(order_id)
        .bind(owner_id)
        .bind(customer_name)
        .bind(total)
        .bind(managed_by);
    let query = if let Some((id, revision)) = customer.party.or(customer.contact) {
        query.bind(id).bind(revision)
    } else {
        query
    };
    let query = if let Some(company) = customer.company {
        query
            .bind(company.id)
            .bind(company.revision)
            .bind(&company.code)
            .bind(&company.name)
    } else {
        query
    };
    query
        .execute(&mut *connection)
        .await
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    insert_commerce_lines_internal(connection, tenant, order_id, &quoted).await?;
    Ok(CommerceDraft {
        order_id,
        revision: 1,
        total_estimate: total,
        lines: quoted,
    })
}

/// One stock allocation — parsed against `allocationInput` (≤100 unique
/// products, quantity `1..=1_000_000_000`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StockAllocation {
    pub product_id: Uuid,
    pub quantity: i64,
}

fn parse_allocations(allocations: &[StockAllocation]) -> Result<(), PurchaseError> {
    if allocations.len() > 100 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    for allocation in allocations {
        if !(1..=1_000_000_000i64).contains(&allocation.quantity) {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        }
    }
    for (index, allocation) in allocations.iter().enumerate() {
        if allocations[index + 1..]
            .iter()
            .any(|other| other.product_id == allocation.product_id)
        {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        }
    }
    Ok(())
}

/// `reserveCommerceOrder` — moves a draft order (any owning module) to
/// reserved with per-product `held` allocations. The `commerce` module owns
/// reservations implicitly from its tracked lines; the `foodservice` module
/// brings its recipe-derived allocations explicitly.
///
/// The expiry is computed by the database from the SAME observed instant the
/// stock reads use, so the reservation read and the held window can never
/// disagree with one another.
#[allow(clippy::too_many_arguments)]
pub async fn reserve_commerce_order(
    connection: &mut PgConnection,
    tenant: &str,
    order_id: Uuid,
    owner_module: &str,
    hold_seconds: i64,
    allocations: Option<&[StockAllocation]>,
) -> Result<i64, PurchaseError> {
    let order = lock_commerce_order(connection, tenant, order_id, owner_module).await?;
    if order.state != "draft" {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    if !(1..=1800).contains(&hold_seconds) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let line_rows: Vec<(Uuid, i32, bool)> = sqlx::query_as(
        "SELECT product_id,quantity,track_stock FROM archava_commerce.order_lines \
         WHERE tenant_id=$1 AND order_id=$2 ORDER BY product_id",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if line_rows.is_empty() || line_rows.len() > 50 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let owns = match (owner_module, allocations) {
        ("commerce", Some(_)) => return Err(PurchaseError::Domain(ErpError::Conflict)),
        ("commerce", None) => line_rows
            .iter()
            .filter(|(_, _, track_stock)| *track_stock)
            .map(|(product_id, quantity, _)| StockAllocation {
                product_id: *product_id,
                quantity: i64::from(*quantity),
            })
            .collect::<Vec<_>>(),
        ("foodservice", Some(allocations)) => {
            parse_allocations(allocations)?;
            allocations.to_vec()
        }
        ("foodservice", None) => return Err(PurchaseError::Domain(ErpError::Conflict)),
        _ => return Err(PurchaseError::Domain(ErpError::Conflict)),
    };
    if owns.is_empty() && owner_module == "foodservice" {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let mut product_ids: Vec<Uuid> = line_rows.iter().map(|(id, _, _)| *id).collect();
    product_ids.extend(owns.iter().map(|allocation| allocation.product_id));
    let products = lock_commerce_products_scoped(
        connection,
        tenant,
        &product_ids,
        owner_module,
        order.company_id,
    )
    .await?;
    if products.iter().any(|product| !product.active) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    for allocation in &owns {
        if !products
            .iter()
            .any(|product| product.id == allocation.product_id && product.track_stock)
        {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        }
    }
    let observed = commerce_clock(connection).await?;
    for allocation in &owns {
        let stock = read_commerce_stock_scoped(
            connection,
            tenant,
            allocation.product_id,
            &observed,
            order.company_id,
        )
        .await?;
        if stock.available < allocation.quantity {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        }
        sqlx::query(
            "INSERT INTO archava_commerce.allocations(tenant_id,order_id,product_id,quantity,state,expires_at) \
             VALUES($1,$2,$3,$4,'held',$5::timestamptz + ($6::int) * interval '1 second')",
        )
        .bind(tenant)
        .bind(order_id)
        .bind(allocation.product_id)
        .bind(int4(allocation.quantity)?)
        .bind(&observed)
        .bind(int4(hold_seconds)?)
        .execute(&mut *connection)
        .await
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    }
    sqlx::query(
        "UPDATE archava_commerce.orders SET state='reserved',expires_at=$3::timestamptz + ($4::int) * interval '1 second',\
         revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(order_id)
    .bind(&observed)
    .bind(int4(hold_seconds)?)
    .execute(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    Ok(order.revision + 1)
}

/// `fulfillCommerceOrder` — consumes held allocations into `consume`
/// movements (which the `stock_movement_apply` trigger applies to the ONE
/// physical stock pool) and fulfils the order. The expiry/held checks read
/// the SAME clock the reservation window was minted from.
pub async fn fulfill_commerce_order(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    order_id: Uuid,
    owner_module: &str,
) -> Result<i64, PurchaseError> {
    let order = lock_commerce_order(connection, tenant, order_id, owner_module).await?;
    if !archava_domain::erp::valid_uuid(&request_id.to_string()) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let allocation_rows: Vec<(Uuid, i32)> = sqlx::query_as(
        "SELECT product_id,quantity FROM archava_commerce.allocations \
         WHERE tenant_id=$1 AND order_id=$2 ORDER BY product_id",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if !allocation_rows.is_empty() {
        let allocation_products: Vec<Uuid> = allocation_rows
            .iter()
            .map(|(product_id, _)| *product_id)
            .collect();
        lock_commerce_products_scoped(
            connection,
            tenant,
            &allocation_products,
            "commerce",
            order.company_id,
        )
        .await?;
    }
    let observed = commerce_clock(connection).await?;
    let holding: bool = sqlx::query_scalar(
        "SELECT count(*)=0 OR count(*) FILTER (WHERE state='held' AND expires_at>$3::timestamptz)=count(*) \
         FROM archava_commerce.allocations WHERE tenant_id=$1 AND order_id=$2",
    )
    .bind(tenant)
    .bind(order_id)
    .bind(&observed)
    .fetch_one(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if order.state != "reserved" || !holding {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let expired: bool = sqlx::query_scalar(
        "SELECT o.expires_at IS NOT NULL AND o.expires_at > $3::timestamptz \
         FROM archava_commerce.orders o WHERE o.tenant_id=$1 AND o.id=$2",
    )
    .bind(tenant)
    .bind(order_id)
    .bind(&observed)
    .fetch_one(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if !expired {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    for (product_id, quantity) in &allocation_rows {
        sqlx::query(
            "UPDATE archava_commerce.allocations SET state='consumed' \
             WHERE tenant_id=$1 AND order_id=$2 AND product_id=$3",
        )
        .bind(tenant)
        .bind(order_id)
        .bind(product_id)
        .execute(&mut *connection)
        .await
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
        sqlx::query(
            "INSERT INTO archava_commerce.movements(tenant_id,request_id,product_id,order_id,kind,quantity) \
             VALUES($1,$2,$3,$4,'consume',$5)",
        )
        .bind(tenant)
        .bind(request_id)
        .bind(product_id)
        .bind(order_id)
        .bind(-i64::from(*quantity))
        .execute(&mut *connection)
        .await
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    }
    sqlx::query(
        "UPDATE archava_commerce.orders SET state='fulfilled',fulfilled_at=clock_timestamp(),\
         revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(order_id)
    .execute(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    Ok(order.revision + 1)
}

/// `cancelCommerceOrder` — releases held allocations and cancels the order;
/// consumption that already happened is irreversible through this port.
pub async fn cancel_commerce_order(
    connection: &mut PgConnection,
    tenant: &str,
    order_id: Uuid,
    owner_module: &str,
) -> Result<i64, PurchaseError> {
    let order = lock_commerce_order(connection, tenant, order_id, owner_module).await?;
    if order.state != "draft" && order.state != "reserved" {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let allocation_rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT product_id FROM archava_commerce.allocations WHERE tenant_id=$1 AND order_id=$2",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    if !allocation_rows.is_empty() {
        let allocation_products: Vec<Uuid> = allocation_rows
            .into_iter()
            .map(|(product_id,)| product_id)
            .collect();
        lock_commerce_products_scoped(
            connection,
            tenant,
            &allocation_products,
            "commerce",
            order.company_id,
        )
        .await?;
    }
    sqlx::query(
        "UPDATE archava_commerce.allocations SET state='released' \
         WHERE tenant_id=$1 AND order_id=$2 AND state='held'",
    )
    .bind(tenant)
    .bind(order_id)
    .execute(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    sqlx::query(
        "UPDATE archava_commerce.orders SET state='cancelled',revision=revision+1,\
         updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(order_id)
    .execute(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    Ok(order.revision + 1)
}

/// `createFoodMenuProduct` — the food-owned portion product: untracked stock,
/// unit-sized, `managed_by='foodservice'`, validated by the same row grammar
/// so an INSERT that violates the menu guard falls out as a conflict.
pub async fn create_food_menu_product(
    connection: &mut PgConnection,
    tenant: &str,
    sku: &str,
    title: &str,
    price: i64,
) -> Result<ProductRow, PurchaseError> {
    verify_commerce_context(connection, tenant).await?;
    if !archava_domain::erp::valid_code(sku) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    if title.trim_matches(|c: char| c.is_whitespace()).is_empty() || title.len() > 160 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    if !(0..=COST_MAX).contains(&price) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let row = sqlx::query(
        "INSERT INTO archava_commerce.products(tenant_id,id,sku,title,unit,track_stock,price,managed_by) \
         VALUES($1,$2,$3,$4,'unit',false,$5,'foodservice') RETURNING *",
    )
    .bind(tenant)
    .bind(Uuid::new_v4())
    .bind(sku)
    .bind(title)
    .bind(int4(price)?)
    .fetch_one(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    parse_product_row(&row, tenant)
}

/// The menu product update — `kind: 'price'|'active'` with the expected
/// revision, driven only for food-service-owned products.
pub async fn update_food_menu_product(
    connection: &mut PgConnection,
    tenant: &str,
    product_id: Uuid,
    kind: MenuProductUpdateKind,
    value: i64,
    expected_revision: i64,
) -> Result<(), PurchaseError> {
    verify_commerce_context(connection, tenant).await?;
    let row = sqlx::query(
        "SELECT * FROM archava_commerce.products WHERE tenant_id=$1 AND id=$2 AND managed_by='foodservice' FOR UPDATE",
    )
    .bind(tenant)
    .bind(product_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?
    .ok_or(PurchaseError::Domain(ErpError::NotFound))?;
    parse_product_row(&row, tenant)?;
    let revision_raw: i32 = sqlx::Row::try_get(&row, "revision")
        .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    let revision = i64::from(revision_raw);
    if revision != expected_revision {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    match kind {
        MenuProductUpdateKind::Price => {
            if !(0..=COST_MAX).contains(&value) {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            sqlx::query(
                "UPDATE archava_commerce.products SET price=$3,revision=revision+1,updated_at=clock_timestamp() \
                 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(product_id)
            .bind(int4(value)?)
            .execute(&mut *connection)
            .await
            .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
        }
        MenuProductUpdateKind::Active => {
            sqlx::query(
                "UPDATE archava_commerce.products SET active=$3,revision=revision+1,updated_at=clock_timestamp() \
                 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(product_id)
            .bind(value != 0)
            .execute(&mut *connection)
            .await
            .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
        }
    }
    Ok(())
}

/// The two menu-product edit kinds — mirroring the TS discriminated union.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuProductUpdateKind {
    Price,
    Active,
}

/// `prepareFoodCommerceOrder` — composes the SHARED claim, consumption and
/// audit with the caller's domain transaction: the foodservice command lane
/// claims the commerce row under the SAME request id, fulfils the order,
/// binds the receipt and records the audit.
pub async fn prepare_food_commerce_order(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    order_id: Uuid,
    actor_id: &str,
    fingerprint: &str,
) -> Result<i64, PurchaseError> {
    verify_commerce_context(connection, tenant).await?;
    if !archava_domain::erp::valid_principal_id(actor_id)
        || !archava_domain::erp::valid_uuid(&request_id.to_string())
        || !valid_fingerprint(fingerprint)
    {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let claimed =
        claim_commerce_command(connection, tenant, request_id, actor_id, fingerprint).await?;
    if !claimed {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let revision =
        fulfill_commerce_order(connection, tenant, request_id, order_id, "foodservice").await?;
    let receipt = serde_json::json!({
        "requestId": request_id,
        "operation": "order.fulfill",
        "id": order_id,
        "revision": revision,
        "replayed": false,
    });
    sqlx::query(
        "UPDATE archava_commerce.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(receipt)
    .execute(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(COMMERCE_LEDGER, error))?;
    insert_commerce_audit(
        connection,
        tenant,
        request_id,
        actor_id,
        "order.fulfill",
        order_id,
        revision,
        None,
    )
    .await?;
    Ok(revision)
}
