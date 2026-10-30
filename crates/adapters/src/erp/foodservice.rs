//! The foodservice repository adapter — the Archxy 2 port of
//! `packages/foodservice-pg/src/repository.ts`'s mutation surface against
//! `archava_foodservice`, on the same transactional shape
//! [`super::purchasing`] pinned: ONE tenant transaction whose membership
//! ladder (both ledgers), authorization, command claim, effect and audit run
//! through the adapters' step functions.
//!
//! ## The one physical stock pool, from the foodservice side
//!
//! A `ticket.queue` reserves commerce stock through the SAME
//! `archava_commerce.allocations` rows a commerce reservation writes, and a
//! `ticket.prepare` consumes it through the SAME `movements` rows the
//! purchasing receipts add to — which is the cross-package single-writer
//! property R06 freezes. Nothing below writes any other stock lane, and the
//! commerce claim (`order.fulfill` under the ticket's request id) keeps the
//! two command ledgers mutually consistent.
//!
//! ## Refusal vocabulary
//!
//! Mirrors the TS repository exactly: `forbidden` (403), `not_found` (404),
//! `conflict` (409), `unavailable` (503) through
//! [`super::PurchaseError`], with the SQLSTATE whitelist reclassifying
//! trigger-refused writes into `conflict` (the crate-shared rule in
//! [`super::purchase_database_error`]).

use sqlx::types::Uuid;
use sqlx::PgConnection;

use archava_domain::erp::foodservice::{
    authorize_operation, command_operation, command_reason, replayed_receipt, FoodAccess,
    FoodCommand, FoodCommandStatus, FoodContext, FoodMutation, FoodReceipt, FOOD_PERMISSIONS,
};
use archava_domain::erp::purchasing::REVISION_MAX;
use archava_domain::erp::{valid_principal_id, valid_uuid, ErpError, ErpMembershipKind, ErpScope};

use super::commerce_kernel::{
    cancel_commerce_order, create_commerce_draft, create_food_menu_product, load_locked_membership,
    lock_commerce_products, prepare_food_commerce_order, reserve_commerce_order,
    update_food_menu_product, MenuProductUpdateKind, StockAllocation,
};
use super::{purchase_database_error, PurchaseError, FOODSERVICE_LEDGER, FOODSERVICE_NAMESPACE};
use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};

/// The menu read shape — `menuSelect`: the foodservice row plus its commerce
/// product half, the columns every menu arm needs (mode and the two
/// revisions come on one row so the lock order is deterministic).
#[derive(Debug, Clone, PartialEq, Eq)]
struct MenuRow {
    id: Uuid,
    station: String,
    active: bool,
    recipe_version: i64,
    revision: i64,
    product_revision: i64,
}

fn parse_menu_row(row: &sqlx::postgres::PgRow) -> Result<MenuRow, PurchaseError> {
    use sqlx::Row;
    let station: String = row
        .try_get("station")
        .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
    if !matches!(station.as_str(), "kitchen" | "bar") {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let id: Uuid = row
        .try_get("id")
        .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
    let active: bool = row
        .try_get("active")
        .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
    let recipe_version: i32 = row
        .try_get("recipe_version")
        .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
    let recipe_version = i64::from(recipe_version);
    if !(1..=REVISION_MAX).contains(&recipe_version) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let revision: i32 = row
        .try_get("revision")
        .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
    let revision = i64::from(revision);
    if !(1..=REVISION_MAX).contains(&revision) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let product_revision: i32 = row
        .try_get("product_revision")
        .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
    let product_revision = i64::from(product_revision);
    if !(1..=REVISION_MAX).contains(&product_revision) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    Ok(MenuRow {
        id,
        station,
        active,
        recipe_version,
        revision,
        product_revision,
    })
}

/// One loaded ticket (the executor's view): only the columns used after the
/// lock, with `prepared_at` as a bool because only null-ness is read.
#[derive(Debug, Clone)]
struct OwnedTicket {
    state: String,
    mode: String,
    order_id: Uuid,
    revision: i64,
    prepared: bool,
}

/// `ownedTicket` — the ticket the CURRENT access owns (`tickets.read` plus
/// the scope rule: an `all` access sees tenant-wide, an `assigned` access
/// only its own rows).
async fn owned_ticket(
    connection: &mut PgConnection,
    tenant: &str,
    context: &FoodContext,
    ticket_id: Uuid,
    lock: bool,
) -> Result<OwnedTicket, PurchaseError> {
    if !context.food.holds(&["tickets.read"]) {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    let scope_is_all = context.food.is_global();
    let owner = &context.food.owner_id;
    let suffix = if lock { " FOR UPDATE OF t" } else { "" };
    let statement = format!(
        "SELECT t.state,t.mode,t.order_id,t.revision,(t.prepared_at IS NOT NULL) AS prepared \
         FROM archava_foodservice.tickets t \
         WHERE t.tenant_id=$1 AND t.id=$2 AND ($3::boolean OR t.owner_id=$4){suffix}"
    );
    // sqlx 0.9 refuses a dynamic string unless the caller asserts it has
    // been audited. The audit is the boolean above: the only bytes of
    // `statement` that are not literals came from `suffix`, whose two
    // possible strings are fixed right here. The SELECT narrows to the five
    // columns the adapter reads; the row id and owner stay in SQL because
    // the scope rule and the lock key already pinned them.
    let row: (String, String, Uuid, i32, bool) =
        sqlx::query_as(sqlx::AssertSqlSafe(statement.as_str()))
            .bind(tenant)
            .bind(ticket_id)
            .bind(scope_is_all)
            .bind(owner)
            .fetch_one(&mut *connection)
            .await
            .map_err(|error| match error {
                sqlx::Error::RowNotFound => PurchaseError::Domain(ErpError::NotFound),
                other => purchase_database_error(FOODSERVICE_LEDGER, other),
            })?;
    let (state, mode, order_id, revision, prepared) = row;
    let revision = i64::from(revision);
    if !(1..=REVISION_MAX).contains(&revision) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    if !matches!(
        state.as_str(),
        "draft" | "queued" | "preparing" | "ready" | "served" | "closed" | "cancelled"
    ) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    Ok(OwnedTicket {
        state,
        mode,
        order_id,
        revision,
        prepared,
    })
}

/// `foodservice.lock_membership` resolution — the double-ledger shape the TS
/// `transaction()` header composes: both rows read in the ONE transaction,
/// checked active and kind-matched, the service principal narrowed against
/// the named human on BOTH sides, then the commerce-scope rule narrows the
/// foodservice side, and the baseline `catalog.read` + `orders.read` grants
/// every foodservice transaction rides on are enforced last.
pub async fn resolve_food_context(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<FoodContext, PurchaseError> {
    if !valid_principal_id(tenant) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    // The commerce side reuses the shared ladder verbatim (kind check,
    // active check, service narrowing, scope interning).
    let commerce_side =
        super::commerce_kernel::resolve_commerce_context(connection, tenant, principal).await?;
    // The foodservice side runs the same ladder against its own vocabulary.
    let own = load_locked_membership(
        connection,
        FOODSERVICE_NAMESPACE,
        FOODSERVICE_LEDGER,
        tenant,
        &principal.id,
        FOOD_PERMISSIONS,
    )
    .await?
    .ok_or(PurchaseError::Domain(ErpError::Forbidden))?;
    if !own.active {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    if own.kind.as_str() != principal.kind.as_str() {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    let (permissions, owner_id, scope) = match principal.kind {
        PrincipalKind::Human => (own.permissions.clone(), principal.id.clone(), own.scope),
        PrincipalKind::Service => {
            let on_behalf_of = principal
                .on_behalf_of
                .as_deref()
                .ok_or(PurchaseError::Domain(ErpError::Forbidden))?;
            let human = load_locked_membership(
                connection,
                FOODSERVICE_NAMESPACE,
                FOODSERVICE_LEDGER,
                tenant,
                on_behalf_of,
                FOOD_PERMISSIONS,
            )
            .await?
            .ok_or(PurchaseError::Domain(ErpError::Forbidden))?;
            if human.kind != ErpMembershipKind::Human {
                return Err(PurchaseError::Domain(ErpError::Forbidden));
            }
            if !human.active {
                return Err(PurchaseError::Domain(ErpError::Forbidden));
            }
            let permissions = own
                .permissions
                .iter()
                .filter(|held| human.permissions.contains(*held))
                .cloned()
                .collect::<Vec<String>>();
            let scope = match (own.scope, human.scope) {
                (ErpScope::Assigned, _) | (_, ErpScope::Assigned) => ErpScope::Assigned,
                (ErpScope::All, ErpScope::All) => ErpScope::All,
            };
            (permissions, on_behalf_of.to_string(), scope)
        }
    };
    let mut context = FoodContext {
        food: FoodAccess {
            permissions,
            scope,
            owner_id,
        },
        commerce: commerce_side.access,
    };
    // The commerce-scope rule: an assigned commerce grant narrows the
    // foodservice side too (`if (access.commerce.scope === 'assigned')
    // access.scope = 'assigned'`).
    if context.commerce.scope == ErpScope::Assigned {
        context = context.clone().narrowed();
    }
    // The transaction baseline every foodservice mutation rides on
    // (`commerceGrant(access, 'catalog.read', 'orders.read')`).
    if !context.commerce.holds(&["catalog.read", "orders.read"]) {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    Ok(context)
}

/// The strict cell check on a receipt read back from the ledger —
/// `foodReceiptSchema.parse(row.receipt)`'s bounds.
fn validate_stored_receipt(receipt: &FoodReceipt) -> Result<(), PurchaseError> {
    if !valid_uuid(&receipt.request_id) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    if receipt.operation.is_empty() || receipt.operation.len() > 100 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    if !valid_uuid(&receipt.id) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    if !(1..=REVISION_MAX).contains(&receipt.revision) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    Ok(())
}

/// Parses a stored receipt through the domain struct — the parse that fails
/// closed when the JSON cell does not carry exactly the five wire fields.
fn stored_receipt(value: &serde_json::Value) -> Result<FoodReceipt, PurchaseError> {
    let receipt: FoodReceipt = serde_json::from_value(value.clone())
        .map_err(|_| PurchaseError::Domain(ErpError::Conflict))?;
    validate_stored_receipt(&receipt)?;
    Ok(receipt)
}

/// The fetched command row: `(actor_id, fingerprint, receipt)` or `None`.
async fn fetch_food_command(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
) -> Result<Option<(String, String, Option<serde_json::Value>)>, PurchaseError> {
    sqlx::query_as(
        "SELECT actor_id,fingerprint,receipt FROM archava_foodservice.commands \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))
}

/// The foodservice audit row — the same seven-parameter row grammar the
/// commerce ledger uses, reconciled by the owning triggers.
#[allow(clippy::too_many_arguments)]
async fn insert_food_audit(
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
        "INSERT INTO archava_foodservice.audit(tenant_id,request_id,actor_id,operation,record_id,revision,reason) \
         VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(actor_id)
    .bind(operation)
    .bind(record_id)
    .bind(revision)
    .bind(reason)
    .execute(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
    Ok(())
}

/// The recovery branch — `recovery()`: the stored receipt replays only while
/// the CURRENT access still authorizes the recorded operation. The row must
/// be the same actor's, and the ticket-bearing operations re-verify their
/// record still exists under the current access.
async fn recovery(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    context: &FoodContext,
    row_actor: &str,
    row_receipt: Option<&serde_json::Value>,
) -> Result<FoodReceipt, PurchaseError> {
    if row_actor != principal.id {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let Some(value) = row_receipt else {
        // `foodReceiptSchema.parse(null)` refuses — a claimed-but-unrecorded
        // command has no replayable receipt.
        return Err(PurchaseError::Domain(ErpError::Conflict));
    };
    let receipt = stored_receipt(value)?;
    let operation = receipt.operation.as_str();
    // The operation-specific CURRENT grant rule, port of the recovery arms.
    if operation.starts_with("table.") {
        if !context.food.holds(&["tables.write", "tables.read"]) {
            return Err(PurchaseError::Domain(ErpError::Forbidden));
        }
        if !context.food.is_global() || !context.commerce.scope.covers_all() {
            return Err(PurchaseError::Domain(ErpError::Forbidden));
        }
    } else if operation.starts_with("menu.") {
        if !context.food.holds(&["menu.write", "menu.read"]) {
            return Err(PurchaseError::Domain(ErpError::Forbidden));
        }
        if !context.food.is_global() || !context.commerce.scope.covers_all() {
            return Err(PurchaseError::Domain(ErpError::Forbidden));
        }
        if !context.commerce.holds(&["catalog.write", "stock.read"]) {
            return Err(PurchaseError::Domain(ErpError::Forbidden));
        }
    } else {
        // Ticket-bearing operations: the kitchen tier prepares and readies;
        // everything else rides tickets.write. The record must still exist.
        if !context.food.holds(
            &[if matches!(operation, "ticket.prepare" | "ticket.ready") {
                "kitchen.write"
            } else {
                "tickets.write"
            }],
        ) {
            return Err(PurchaseError::Domain(ErpError::Forbidden));
        }
        if !context.commerce.holds(&["orders.write", "stock.read"]) {
            return Err(PurchaseError::Domain(ErpError::Forbidden));
        }
        let Ok(record_id) = Uuid::parse_str(&receipt.id) else {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        };
        let current = owned_ticket(connection, tenant, context, record_id, false).await?;
        if operation == "ticket.create" {
            if !context.food.holds(&["menu.read"]) {
                return Err(PurchaseError::Domain(ErpError::Forbidden));
            }
            if current.mode == "dine_in" && !context.food.holds(&["tables.read"]) {
                return Err(PurchaseError::Domain(ErpError::Forbidden));
            }
        }
    }
    Ok(replayed_receipt(&receipt))
}

/// `commandStatus` — the human-only status read: `unknown` when no command
/// row exists, otherwise the recovery path (which re-authorized the stored
/// receipt against the CURRENT access).
pub async fn food_command_status(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    context: &FoodContext,
    request_id: Uuid,
) -> Result<FoodCommandStatus, PurchaseError> {
    if !valid_uuid(&request_id.to_string()) {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    if principal.kind != PrincipalKind::Human {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    let row = fetch_food_command(connection, tenant, request_id).await?;
    let Some((row_actor, _row_fingerprint, row_receipt)) = row else {
        return Ok(FoodCommandStatus::Unknown);
    };
    let receipt = recovery(
        connection,
        tenant,
        principal,
        context,
        &row_actor,
        row_receipt.as_ref(),
    )
    .await?;
    Ok(FoodCommandStatus::Committed { receipt })
}

/// The full mutation run — `mutate`: authorize, claim, execute, audit,
/// receipt. A claim loss recovers when (and only when) the same actor and
/// fingerprint re-submit; anything else refuses.
pub async fn record_food_command(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    context: &FoodContext,
    fingerprint: &str,
    mutation: &FoodMutation,
) -> Result<FoodReceipt, PurchaseError> {
    // The mutation surface is human-only — a service principal has no
    // foodservice write path (`if principal.kind !== 'human'`).
    if principal.kind != PrincipalKind::Human {
        return Err(PurchaseError::Domain(ErpError::Forbidden));
    }
    validate_fingerprint(fingerprint)?;
    authorize_operation(context, &mutation.command)?;
    let operation = command_operation(&mutation.command);
    let request_id = Uuid::parse_str(&mutation.request_id)
        .map_err(|_| PurchaseError::Domain(ErpError::Conflict))?;
    let claimed =
        claim_food_command(connection, tenant, request_id, &principal.id, fingerprint).await?;
    if !claimed {
        let (row_actor, row_fingerprint, row_receipt) =
            fetch_food_command(connection, tenant, request_id)
                .await?
                .ok_or(PurchaseError::Domain(ErpError::Conflict))?;
        if row_actor != principal.id || row_fingerprint != fingerprint {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        }
        return recovery(
            connection,
            tenant,
            principal,
            context,
            &row_actor,
            row_receipt.as_ref(),
        )
        .await;
    }
    let (record_id, revision) = execute_food_command(
        connection,
        tenant,
        principal,
        context,
        mutation,
        fingerprint,
    )
    .await?;
    let receipt = FoodReceipt {
        request_id: mutation.request_id.clone(),
        operation: operation.to_string(),
        id: record_id.to_string(),
        revision,
        replayed: false,
    };
    insert_food_audit(
        connection,
        tenant,
        request_id,
        &principal.id,
        operation,
        record_id,
        revision,
        command_reason(&mutation.command),
    )
    .await?;
    sqlx::query(
        "UPDATE archava_foodservice.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(serde_json::json!({
        "requestId": receipt.request_id,
        "operation": receipt.operation,
        "id": receipt.id,
        "revision": receipt.revision,
        "replayed": receipt.replayed,
    }))
    .execute(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
    Ok(receipt)
}

/// `foodservice.commands` claim — the same ON CONFLICT shape the commerce
/// and purchasing command ledgers use; the loser reads the winner's row.
async fn claim_food_command(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    actor_id: &str,
    fingerprint: &str,
) -> Result<bool, PurchaseError> {
    let claimed: Option<(Uuid,)> = sqlx::query_as(
        "INSERT INTO archava_foodservice.commands(tenant_id,request_id,actor_id,fingerprint) \
         VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING RETURNING request_id",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(actor_id)
    .bind(fingerprint)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
    Ok(claimed.is_some())
}

/// The fingerprint's shape check (64 lowercase hex) — the HMAC content is
/// the host layer's; the ledger refuses anything the digest could not
/// produce.
fn validate_fingerprint(fingerprint: &str) -> Result<(), PurchaseError> {
    if fingerprint.len() != 64
        || !fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (byte.is_ascii_lowercase() && byte <= b'f'))
    {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    Ok(())
}

/// `writeRecipe` — a new sealed recipe version: unmanaged tracked products
/// only (the ingredient guard refuses managed rows), 1..=20 components,
/// sealed in this transaction. The completeness trigger defers its final
/// say to COMMIT, exactly like the TS flow's constraint.
async fn write_recipe(
    connection: &mut PgConnection,
    tenant: &str,
    menu_id: Uuid,
    version: i64,
    recipe: &[archava_domain::erp::foodservice::RecipeComponent],
) -> Result<(), PurchaseError> {
    let product_ids: Vec<Uuid> = recipe
        .iter()
        .map(|component| {
            Uuid::parse_str(&component.product_id)
                .map_err(|_| PurchaseError::Domain(ErpError::Conflict))
        })
        .collect::<Result<_, _>>()?;
    let products = lock_commerce_products(connection, tenant, &product_ids).await?;
    if products
        .iter()
        .any(|product| !product.track_stock || !product.active)
    {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    sqlx::query(
        "INSERT INTO archava_foodservice.recipe_versions(tenant_id,menu_id,version) VALUES($1,$2,$3)",
    )
    .bind(tenant)
    .bind(menu_id)
    .bind(version)
    .execute(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
    for component in recipe {
        // The column is uuid: parse the (already uuid-validated) string once
        // here so the bind carries the column's own type.
        let product_id = Uuid::parse_str(&component.product_id)
            .map_err(|_| PurchaseError::Domain(ErpError::Conflict))?;
        sqlx::query(
            "INSERT INTO archava_foodservice.recipe_components(tenant_id,menu_id,version,product_id,quantity) \
             VALUES($1,$2,$3,$4,$5)",
        )
        .bind(tenant)
        .bind(menu_id)
        .bind(version)
        .bind(product_id)
        .bind(component.quantity)
        .execute(&mut *connection)
        .await
        .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
    }
    sqlx::query(
        "UPDATE archava_foodservice.recipe_versions SET sealed=true WHERE tenant_id=$1 AND menu_id=$2 AND version=$3",
    )
    .bind(tenant)
    .bind(menu_id)
    .bind(version)
    .execute(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
    Ok(())
}

/// `allocations()` — the recipe-derived stock demand: every component's
/// quantity multiplied by the ticket line's quantity, grouped per product.
async fn ticket_allocations(
    connection: &mut PgConnection,
    tenant: &str,
    ticket_id: Uuid,
) -> Result<Vec<StockAllocation>, PurchaseError> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT c.product_id::text,sum(c.quantity::bigint*l.quantity)::int8 AS quantity \
         FROM archava_foodservice.ticket_lines l \
         JOIN archava_foodservice.recipe_components c \
           ON c.tenant_id=l.tenant_id AND c.menu_id=l.menu_id AND c.version=l.recipe_version \
         WHERE l.tenant_id=$1 AND l.ticket_id=$2 \
         GROUP BY c.product_id ORDER BY c.product_id",
    )
    .bind(tenant)
    .bind(ticket_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
    if rows.is_empty() || rows.len() > 100 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    let mut parsed = Vec::with_capacity(rows.len());
    for (product_id, quantity) in rows {
        let product_id =
            Uuid::parse_str(&product_id).map_err(|_| PurchaseError::Domain(ErpError::Conflict))?;
        if !(1..=1_000_000_000i64).contains(&quantity) {
            return Err(PurchaseError::Domain(ErpError::Conflict));
        }
        parsed.push(StockAllocation {
            product_id,
            quantity,
        });
    }
    Ok(parsed)
}

/// `execute` — the thirteen-arm dispatcher. Everything unauthorized is
/// refused before [`record_food_command`] reaches here; every arm returns
/// the (id, revision) receipt pair.
#[allow(clippy::too_many_arguments)]
async fn execute_food_command(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    context: &FoodContext,
    mutation: &FoodMutation,
    fingerprint: &str,
) -> Result<(Uuid, i64), PurchaseError> {
    match &mutation.command {
        FoodCommand::TableCreate { code, title, seats } => {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO archava_foodservice.tables(tenant_id,id,code,title,seats) VALUES($1,$2,$3,$4,$5)",
            )
            .bind(tenant)
            .bind(id)
            .bind(code)
            .bind(title)
            .bind(*seats)
            .execute(&mut *connection)
            .await
            .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
            Ok((id, 1))
        }
        FoodCommand::TableSetActive {
            id,
            expected_revision,
            active,
            ..
        } => {
            let record_id =
                Uuid::parse_str(id).map_err(|_| PurchaseError::Domain(ErpError::Conflict))?;
            let revision: i32 = sqlx::query_scalar(
                "SELECT revision FROM archava_foodservice.tables WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(record_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?
            .ok_or(PurchaseError::Domain(ErpError::NotFound))?;
            if i64::from(revision) != *expected_revision {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            sqlx::query(
                "UPDATE archava_foodservice.tables SET active=$3,revision=revision+1,updated_at=clock_timestamp() \
                 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(record_id)
            .bind(active)
            .execute(&mut *connection)
            .await
            .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
            Ok((record_id, expected_revision + 1))
        }
        FoodCommand::MenuCreate {
            sku,
            title,
            price,
            station,
            recipe,
        } => {
            let product = create_food_menu_product(connection, tenant, sku, title, *price).await?;
            sqlx::query(
                "INSERT INTO archava_foodservice.menu_items(tenant_id,id,station) VALUES($1,$2,$3)",
            )
            .bind(tenant)
            .bind(product.id)
            .bind(station.as_str())
            .execute(&mut *connection)
            .await
            .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
            write_recipe(connection, tenant, product.id, 1, recipe).await?;
            Ok((product.id, 1))
        }
        FoodCommand::MenuSetPrice {
            id,
            expected_revision,
            ..
        }
        | FoodCommand::MenuSetActive {
            id,
            expected_revision,
            ..
        }
        | FoodCommand::MenuReplaceRecipe {
            id,
            expected_revision,
            ..
        } => {
            let record_id =
                Uuid::parse_str(id).map_err(|_| PurchaseError::Domain(ErpError::Conflict))?;
            let menu = sqlx::query(
                "SELECT m.*,p.revision AS product_revision FROM archava_foodservice.menu_items m \
                 JOIN archava_commerce.products p ON p.tenant_id=m.tenant_id AND p.id=m.id \
                 WHERE m.tenant_id=$1 AND m.id=$2 FOR UPDATE OF m",
            )
            .bind(tenant)
            .bind(record_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(|error| match error {
                sqlx::Error::RowNotFound => PurchaseError::Domain(ErpError::NotFound),
                other => purchase_database_error(FOODSERVICE_LEDGER, other),
            })?;
            let menu = parse_menu_row(&menu)?;
            if menu.revision != *expected_revision {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            let mut version = menu.recipe_version;
            match &mutation.command {
                FoodCommand::MenuReplaceRecipe { recipe, .. } => {
                    version += 1;
                    write_recipe(connection, tenant, record_id, version, recipe).await?;
                }
                FoodCommand::MenuSetPrice { price, .. } => {
                    update_food_menu_product(
                        connection,
                        tenant,
                        record_id,
                        MenuProductUpdateKind::Price,
                        *price,
                        menu.product_revision,
                    )
                    .await?;
                }
                FoodCommand::MenuSetActive { active, .. } => {
                    update_food_menu_product(
                        connection,
                        tenant,
                        record_id,
                        MenuProductUpdateKind::Active,
                        i64::from(*active),
                        menu.product_revision,
                    )
                    .await?;
                }
                _ => unreachable!("the outer match pinned this arm"),
            }
            sqlx::query(
                "UPDATE archava_foodservice.menu_items SET recipe_version=$3,revision=revision+1,updated_at=clock_timestamp() \
                 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(record_id)
            .bind(version)
            .execute(&mut *connection)
            .await
            .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
            Ok((record_id, expected_revision + 1))
        }
        FoodCommand::TicketCreate {
            customer_name,
            mode,
            table_id,
            guests,
            lines,
        } => {
            if let Some(table_id) = table_id {
                let table = Uuid::parse_str(table_id)
                    .map_err(|_| PurchaseError::Domain(ErpError::Conflict))?;
                let found: Option<(bool, i32)> = sqlx::query_as(
                    "SELECT active,seats FROM archava_foodservice.tables WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
                )
                .bind(tenant)
                .bind(table)
                .fetch_optional(&mut *connection)
                .await
                .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
                let Some((active, seats)) = found else {
                    return Err(PurchaseError::Domain(ErpError::Conflict));
                };
                if !active || i64::from(seats) < *guests {
                    return Err(PurchaseError::Domain(ErpError::Conflict));
                }
            }
            // The menu half locks under a SHARE so the rows cannot change
            // under the draft that quotes them; inactive menus refuse.
            let menu_ids = lines
                .iter()
                .map(|line| {
                    Uuid::parse_str(&line.menu_id)
                        .map_err(|_| PurchaseError::Domain(ErpError::Conflict))
                })
                .collect::<Result<Vec<Uuid>, _>>()?;
            let menus: Vec<(Uuid, String, bool, i32)> = sqlx::query_as(
                "SELECT m.id,m.station,p.active,m.recipe_version \
                 FROM archava_foodservice.menu_items m \
                 JOIN archava_commerce.products p ON p.tenant_id=m.tenant_id AND p.id=m.id \
                 WHERE m.tenant_id=$1 AND m.id=ANY($2) ORDER BY m.id FOR SHARE OF m",
            )
            .bind(tenant)
            .bind(&menu_ids)
            .fetch_all(&mut *connection)
            .await
            .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
            if menus.len() != menu_ids.len() || menus.iter().any(|(_, _, active, _)| !active) {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            // The draft: a foodservice-managed commerce order quoting the
            // SAME menu product ids the ticket carries — one line per menu,
            // one order per ticket, frozen in this transaction.
            let draft_lines: Vec<super::commerce_kernel::DraftLine> = lines
                .iter()
                .map(|line| {
                    let product_id = Uuid::parse_str(&line.menu_id)
                        .map_err(|_| PurchaseError::Domain(ErpError::Conflict))?;
                    Ok::<_, PurchaseError>(super::commerce_kernel::DraftLine {
                        product_id,
                        quantity: line.quantity,
                    })
                })
                .collect::<Result<Vec<_>, PurchaseError>>()?;
            let draft = create_commerce_draft(
                connection,
                tenant,
                &context.food.owner_id,
                customer_name,
                Some("foodservice"),
                &draft_lines,
            )
            .await?;
            let ticket_id = Uuid::new_v4();
            let bound_table = table_id
                .as_deref()
                .map(|table| {
                    Uuid::parse_str(table).map_err(|_| PurchaseError::Domain(ErpError::Conflict))
                })
                .transpose()?;
            sqlx::query(
                "INSERT INTO archava_foodservice.tickets(tenant_id,id,owner_id,order_id,mode,table_id,guests) \
                 VALUES($1,$2,$3,$4,$5,$6,$7)",
            )
            .bind(tenant)
            .bind(ticket_id)
            .bind(&context.food.owner_id)
            .bind(draft.order_id)
            .bind(mode.as_str())
            .bind(bound_table)
            .bind(guests)
            .execute(&mut *connection)
            .await
            .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
            for line in lines {
                let menu = menus
                    .iter()
                    .find(|(menu_id, _, _, _)| menu_id.to_string() == line.menu_id)
                    .ok_or(PurchaseError::Domain(ErpError::Conflict))?;
                let quoted = draft
                    .lines
                    .iter()
                    .find(|quoted| quoted.product_id.to_string() == line.menu_id)
                    .ok_or(PurchaseError::Domain(ErpError::Conflict))?;
                let menu_ref = Uuid::parse_str(&line.menu_id)
                    .map_err(|_| PurchaseError::Domain(ErpError::Conflict))?;
                sqlx::query(
                    "INSERT INTO archava_foodservice.ticket_lines(tenant_id,ticket_id,order_id,order_line_id,menu_id,recipe_version,quantity,station) \
                     VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
                )
                .bind(tenant)
                .bind(ticket_id)
                .bind(draft.order_id)
                .bind(quoted.line_id)
                .bind(menu_ref)
                .bind(menu.3)
                .bind(line.quantity)
                .bind(menu.1.as_str())
                .execute(&mut *connection)
                .await
                .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
            }
            // The recipe completeness trigger evaluates at COMMIT; the
            // allocations read proves the demand is well-formed now.
            ticket_allocations(connection, tenant, ticket_id).await?;
            Ok((ticket_id, 1))
        }
        FoodCommand::TicketQueue {
            id,
            expected_revision,
            hold_seconds,
            ..
        } => {
            let ticket_id =
                Uuid::parse_str(id).map_err(|_| PurchaseError::Domain(ErpError::Conflict))?;
            let ticket = owned_ticket(connection, tenant, context, ticket_id, true).await?;
            if ticket.revision != *expected_revision {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            if ticket.state != "draft" {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            let allocations = ticket_allocations(connection, tenant, ticket_id).await?;
            reserve_commerce_order(
                connection,
                tenant,
                ticket.order_id,
                "foodservice",
                *hold_seconds,
                Some(&allocations),
            )
            .await?;
            update_ticket_state(connection, tenant, ticket_id, "queued", false, None).await?;
            Ok((ticket_id, expected_revision + 1))
        }
        FoodCommand::TicketPrepare {
            id,
            expected_revision,
        } => {
            let ticket_id =
                Uuid::parse_str(id).map_err(|_| PurchaseError::Domain(ErpError::Conflict))?;
            let ticket = owned_ticket(connection, tenant, context, ticket_id, true).await?;
            if ticket.revision != *expected_revision {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            if ticket.state != "queued" {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            // The consumption: ONE commerce command claim under the SAME
            // request id, the fulfilment, the receipt and the audit — the
            // cross-package writer that makes the pool single.
            let request_id = Uuid::parse_str(&mutation.request_id)
                .map_err(|_| PurchaseError::Domain(ErpError::Conflict))?;
            prepare_food_commerce_order(
                connection,
                tenant,
                request_id,
                ticket.order_id,
                &principal.id,
                fingerprint,
            )
            .await?;
            update_ticket_state(connection, tenant, ticket_id, "preparing", true, None).await?;
            Ok((ticket_id, expected_revision + 1))
        }
        FoodCommand::TicketCancel {
            id,
            expected_revision,
            reason,
        } => {
            let ticket_id =
                Uuid::parse_str(id).map_err(|_| PurchaseError::Domain(ErpError::Conflict))?;
            let ticket = owned_ticket(connection, tenant, context, ticket_id, true).await?;
            if ticket.revision != *expected_revision {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            if ticket.state == "closed" || ticket.state == "cancelled" {
                return Err(PurchaseError::Domain(ErpError::Conflict));
            }
            // Only unprepared tickets release their reservation; consumption
            // that already happened is not reversible through this surface.
            if !ticket.prepared {
                cancel_commerce_order(connection, tenant, ticket.order_id, "foodservice").await?;
            }
            update_ticket_state(
                connection,
                tenant,
                ticket_id,
                "cancelled",
                false,
                Some(reason),
            )
            .await?;
            Ok((ticket_id, expected_revision + 1))
        }
        FoodCommand::TicketReady {
            id,
            expected_revision,
        } => {
            transition_ticket(
                connection,
                tenant,
                context,
                id,
                *expected_revision,
                "ticket.ready",
                ("preparing", "ready"),
            )
            .await
        }
        FoodCommand::TicketServe {
            id,
            expected_revision,
        } => {
            transition_ticket(
                connection,
                tenant,
                context,
                id,
                *expected_revision,
                "ticket.serve",
                ("ready", "served"),
            )
            .await
        }
        FoodCommand::TicketClose {
            id,
            expected_revision,
        } => {
            transition_ticket(
                connection,
                tenant,
                context,
                id,
                *expected_revision,
                "ticket.close",
                ("served", "closed"),
            )
            .await
        }
    }
}

/// The ticket-state UPDATE every ticket arm shares — the CASE keeps
/// `prepared_at` when no prepare is happening, and the trigger applies the
/// transition legality (draft→queued etc.).
async fn update_ticket_state(
    connection: &mut PgConnection,
    tenant: &str,
    ticket_id: Uuid,
    state: &str,
    prepared: bool,
    cancellation_reason: Option<&str>,
) -> Result<(), PurchaseError> {
    sqlx::query(
        "UPDATE archava_foodservice.tickets SET state=$3, \
         prepared_at=CASE WHEN $4::boolean THEN clock_timestamp() ELSE prepared_at END, \
         cancellation_reason=$5,revision=revision+1,updated_at=clock_timestamp() \
         WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(ticket_id)
    .bind(state)
    .bind(prepared)
    .bind(cancellation_reason)
    .execute(&mut *connection)
    .await
    .map_err(|error| purchase_database_error(FOODSERVICE_LEDGER, error))?;
    Ok(())
}

/// The three forward transitions — `ticket.ready`, `ticket.serve`,
/// `ticket.close`: the locked ticket's current state must match the
/// transition's source exactly.
async fn transition_ticket(
    connection: &mut PgConnection,
    tenant: &str,
    context: &FoodContext,
    ticket_text: &str,
    expected_revision: i64,
    _operation: &str,
    transition: (&str, &str),
) -> Result<(Uuid, i64), PurchaseError> {
    let ticket_id =
        Uuid::parse_str(ticket_text).map_err(|_| PurchaseError::Domain(ErpError::Conflict))?;
    let ticket = owned_ticket(connection, tenant, context, ticket_id, true).await?;
    if ticket.revision != expected_revision {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    if ticket.state != transition.0 {
        return Err(PurchaseError::Domain(ErpError::Conflict));
    }
    update_ticket_state(connection, tenant, ticket_id, transition.1, false, None).await?;
    Ok((ticket_id, expected_revision + 1))
}
