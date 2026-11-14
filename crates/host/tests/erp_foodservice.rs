//! The R06 foodservice cohort's host witnesses — the stock pool's SECOND
//! writer, driven through the real router (`tower::ServiceExt::oneshot`)
//! against the disposable PostgreSQL whose `archava_foodservice` schema is
//! the real migration ledger (`001_tables_recipes_tickets`). Every assertion
//! is a fact about what the SERVER did:
//!
//! - the authorized flow (`table.create` → `menu.create` → `ticket.create`
//!   → `ticket.queue` → `ticket.prepare` → `ticket.ready` → `ticket.serve`)
//!   ends with the commerce stock changed EXACTLY once for the recipe
//!   demand — one `consume` movement applied by the ledger's own
//!   `stock_movement_apply` trigger — a foodservice-managed commerce order
//!   walking draft → reserved → fulfilled, `order.fulfill` audit rows in the
//!   commerce ledger, and a receipt bound into BOTH ledgers' commands under
//!   the SAME request id;
//! - a reservation that would over-commit the pool refuses and holds
//!   nothing;
//! - a menu whose recipe names a NON-TRACKED product refuses and records
//!   nothing (the catalog insert rolls back with the recipe);
//! - a revoked foodservice grant refuses the recovery — yesterday's
//!   receipts are not replayable through revoked access;
//! - the same idempotency key with a DIFFERENT command conflicts without
//!   recording;
//! - a cross-tenant caller is refused before any business statement runs;
//! - a lost response re-submits the SAME key + bytes and answers `replayed`
//!   with no second reservation;
//! - a cancel before any prepare releases the reservation — the stock is
//!   untouched;
//! - an actor whose foodservice grants are missing is refused by the grant
//!   gate with no command row at all.
//!
//! The commerce side is the SAME pool the purchasing suite shares; the seed
//! preloads stock the ledger's own honest way — a command row plus an
//! `adjust` movement, so `guard_stock_balance`'s derivation check holds.

use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::routing::Router;
use serde_json::{json, Value};
use sqlx::types::Uuid;
use tower::ServiceExt;

/// The commerce ledger this host serves as the primary — the same table list
/// the purchasing suite's `commerce_ledger()` pins.
const TABLES: &[&str] = &[
    "memberships",
    "products",
    "stock_balances",
    "orders",
    "order_lines",
    "allocations",
    "commands",
    "movements",
    "returns",
    "audit",
    "approvals",
];

/// A 32-byte identity HMAC key — the minimum `Identity::open` accepts.
const IDENTITY_KEY: &[u8] = &[11u8; 32];

/// The test password. The password gate enforces `length >= 15`.
const PASSWORD: &str = "correct horse battery staple";

/// A precomputed scrypt hash of [`PASSWORD`] — one hash serves every user.
const PASSWORD_HASH: &str =
    "scrypt$32768$8$3$OZtInKxSaSZqbEptj2fvUg$jU_gx2rQW-69EYpbRoMKGSgclhV-mO7gXVF-QYvSXVo";

fn commerce_ledger() -> Ledger {
    Ledger {
        namespace: "archava_commerce",
        ledger: "Commerce",
        tables: TABLES,
    }
}

/// Serving state that has passed BOTH readiness gates: commerce (`try_new`)
/// and the purchasing ledger (`try_new_erp`'s second pass) — the foodservice
/// surface rides the same serving state; its own ledger rows exist in the
/// fixture regardless.
async fn state(fixture: &Fixture) -> AppState {
    AppState::try_new_erp(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
        &fixture.purchasing_ledger,
        &fixture.erp_readiness(),
    )
    .await
    .expect("a ready fixture must construct serving state")
}

// --- The cast -----------------------------------------------------------------

/// The tenants this suite uses.
const ACME: &str = "acme-retail";
const GLOBEX: &str = "globex-corp";

/// The foodservice permission vocabulary (migration 001's CHECK).
const FOOD_ALL: &[&str] = &[
    "tables.read",
    "tables.write",
    "menu.read",
    "menu.write",
    "tickets.read",
    "tickets.write",
    "kitchen.write",
    "audit.read",
];

/// The commerce permission vocabulary — the transaction ladder reads BOTH
/// ledger rows for every caller.
const COMMERCE_ALL: &[&str] = &[
    "catalog.read",
    "catalog.write",
    "stock.read",
    "stock.write",
    "orders.read",
    "orders.write",
    "returns.write",
    "audit.read",
];

/// `fx` is the floor manager: full grants, scope all, BOTH ledgers — the
/// positive-path actor.
/// `fnone` holds only `audit.read` in the foodservice ledger — every grant
/// gate refuses.
/// `foff` is `fx` with the foodservice membership seeded ACTIVE and revoked
/// by the test itself — the recovery must not outlive the grant.
/// `fother` holds full grants but ONLY in GLOBEX — the cross-tenant refusal.
/// `fgx` is GLOBEX's own full-grant manager — the per-tenant
/// key-independence witness.
async fn seed(fixture: &Fixture) {
    for principal in ["fx", "fnone", "foff", "fother", "fgx"] {
        sqlx::query(
            "INSERT INTO archava_identity.users (id, username, password_hash, active, version) \
             VALUES ($1, $2, $3, true, 1)",
        )
        .bind(principal)
        .bind(format!("{}@archava.test", principal))
        .bind(PASSWORD_HASH)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the identity user");
    }

    // The commerce ledger's rows: every actor carries a FULL commerce grant
    // so a foodservice refusal in this suite is attributable to the
    // FOODSERVICE half of the ladder, not the commerce floor.
    for principal in ["fx", "fnone", "foff"] {
        sqlx::query(
            "INSERT INTO archava_commerce.memberships \
             (tenant_id, principal_id, kind, permissions, scope, active) \
             VALUES ($1, $2, 'human', $3, 'all', true)",
        )
        .bind(ACME)
        .bind(principal)
        .bind(COMMERCE_ALL.to_vec())
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the commerce membership");
    }
    for principal in ["fother", "fgx"] {
        sqlx::query(
            "INSERT INTO archava_commerce.memberships \
             (tenant_id, principal_id, kind, permissions, scope, active) \
             VALUES ($1, $2, 'human', $3, 'all', true)",
        )
        .bind(GLOBEX)
        .bind(principal)
        .bind(COMMERCE_ALL.to_vec())
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the commerce membership");
    }

    // The foodservice ledger's rows: the patent-one difference per actor.
    sqlx::query(
        "INSERT INTO archava_foodservice.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind("fx")
    .bind(FOOD_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed fx's foodservice membership");
    sqlx::query(
        "INSERT INTO archava_foodservice.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind("foff")
    .bind(FOOD_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed foff's ACTIVE foodservice membership");
    sqlx::query(
        "INSERT INTO archava_foodservice.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind("fnone")
    .bind(["audit.read"].to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed fnone's foodservice membership");
    for principal in ["fother", "fgx"] {
        sqlx::query(
            "INSERT INTO archava_foodservice.memberships \
             (tenant_id, principal_id, kind, permissions, scope, active) \
             VALUES ($1, $2, 'human', $3, 'all', true)",
        )
        .bind(GLOBEX)
        .bind(principal)
        .bind(FOOD_ALL.to_vec())
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the GLOBEX foodservice membership");
    }

    // The catalog: one TRACKED ingredient per tenant (the stock pool the
    // tickets consume) plus one NON-tracked product in ACME (the recipe
    // refusal's ingredient). Each insert runs in a tenant-scoped
    // transaction: `init_stock` is SECURITY DEFINER, so the stock cell rides
    // the session GUC the ledger's own policies read.
    for (tenant, id, sku, tracked) in [
        (ACME, ID_P_FAST, "P-FAST-01", true),
        (ACME, ID_P_MENU, "P-MENU-02", false),
        (GLOBEX, ID_P_GLOBEX, "P-GLBX-01", true),
    ] {
        let mut catalog_tx = fixture
            .admin_pool
            .begin()
            .await
            .expect("begin the catalog seed transaction");
        sqlx::query("SELECT set_config('archava.tenant_id', $1, true)")
            .bind(tenant)
            .execute(&mut *catalog_tx)
            .await
            .expect("set the tenant context for the catalog seed");
        sqlx::query(
            "INSERT INTO archava_commerce.products \
             (tenant_id, id, sku, title, unit, track_stock, price) \
             VALUES ($1, $2, $3, $4, 'unit', $5, 25000)",
        )
        .bind(tenant)
        .bind(Uuid::parse_str(id).expect("a test uuid"))
        .bind(sku)
        .bind(format!("Product {sku}"))
        .bind(tracked)
        .execute(&mut *catalog_tx)
        .await
        .expect("seed the product");
        catalog_tx.commit().await.expect("commit the catalog seed");
    }

    // Preload the pool's stock the ledger's own honest way: a command row
    // (the movements FK points at it) plus an `adjust` movement, applied to
    // the ONE physical pool by the `stock_movement_apply` trigger.
    // `guard_stock_balance` then holds: quantity DERIVES from the movements.
    for (tenant, actor, product, request) in [
        (
            ACME,
            "fx",
            ID_P_FAST,
            "af3eb010-3334-4333-8333-0000000000e1",
        ),
        (
            GLOBEX,
            "fgx",
            ID_P_GLOBEX,
            "af3eb010-3334-4333-8333-0000000000e2",
        ),
    ] {
        let mut stock_tx = fixture
            .admin_pool
            .begin()
            .await
            .expect("begin the stock seed transaction");
        sqlx::query("SELECT set_config('archava.tenant_id', $1, true)")
            .bind(tenant)
            .execute(&mut *stock_tx)
            .await
            .expect("set the tenant context for the stock seed");
        sqlx::query(
            "INSERT INTO archava_commerce.commands (tenant_id, request_id, actor_id, fingerprint) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(tenant)
        .bind(Uuid::parse_str(request).expect("a test uuid"))
        .bind(actor)
        .bind("0".repeat(64))
        .execute(&mut *stock_tx)
        .await
        .expect("seed the movements' command row");
        sqlx::query(
            "INSERT INTO archava_commerce.movements \
             (tenant_id, request_id, product_id, kind, quantity, reason) \
             VALUES ($1, $2, $3, 'adjust', 100, 'fixture seed stock')",
        )
        .bind(tenant)
        .bind(Uuid::parse_str(request).expect("a test uuid"))
        .bind(Uuid::parse_str(product).expect("a test uuid"))
        .execute(&mut *stock_tx)
        .await
        .expect("seed the adjust movement");
        stock_tx.commit().await.expect("commit the stock seed");
    }
}

const ID_P_FAST: &str = "5dee0a10-2222-4222-8222-0000000000f1";
const ID_P_MENU: &str = "5dee0a10-2222-4222-8222-0000000000f2";
const ID_P_GLOBEX: &str = "5dee0a10-2222-4222-8222-0000000000f3";

/// Fixed idempotency keys — stable so readbacks and the recovery witness can
/// name rows exactly.
const KEY_TABLE: &str = "af3eb010-3334-4333-8333-0000000000a1";
const KEY_MENU: &str = "af3eb010-3334-4333-8333-0000000000a2";
const KEY_TICKET: &str = "af3eb010-3334-4333-8333-0000000000a3";
const KEY_QUEUE: &str = "af3eb010-3334-4333-8333-0000000000a4";
const KEY_PREPARE: &str = "af3eb010-3334-4333-8333-0000000000a5";
const KEY_READY: &str = "af3eb010-3334-4333-8333-0000000000a6";
const KEY_SERVE: &str = "af3eb010-3334-4333-8333-0000000000a7";
const KEY_MENU_WIDE: &str = "af3eb010-3334-4333-8333-0000000000a8";
const KEY_MENU_HOLD: &str = "af3eb010-3334-4333-8333-0000000000a9";
const KEY_TICKET_HOLD: &str = "af3eb010-3334-4333-8333-0000000000b1";
const KEY_QUEUE_HOLD: &str = "af3eb010-3334-4333-8333-0000000000b2";
const KEY_CANCEL: &str = "af3eb010-3334-4333-8333-0000000000b3";
const KEY_OFFER_TABLE: &str = "af3eb010-3334-4333-8333-0000000000b4";

// --- HTTP helpers ------------------------------------------------------------

/// A session's credentials.
struct Session {
    token: String,
    csrf: String,
}

/// How many logins this suite may have in flight at once — the production
/// budget (RN04/RN07: never raised from inside the suite).
static LOGIN_SLOTS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(archava_host::max_active_derivations());

/// Log in and return the session's bearer token and CSRF value.
async fn login(app: &Router, principal_id: &str) -> Session {
    let _slot = LOGIN_SLOTS.acquire().await.expect("login slots are open");
    let body = json!({
        "username": format!("{}@archava.test", principal_id),
        "password": PASSWORD,
    })
    .to_string();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header("content-type", "application/json")
                .header("x-archava-client", "cli")
                .body(Body::from(body))
                .expect("build the login request"),
        )
        .await
        .expect("the login route must answer");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "login as {principal_id} must succeed"
    );
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read the login response");
    let outcome: Value = serde_json::from_slice(&bytes).expect("the login response is JSON");

    Session {
        token: outcome["token"]
            .as_str()
            .expect("the login response carries a token")
            .to_string(),
        csrf: outcome["csrf"]
            .as_str()
            .expect("the login response carries a CSRF value")
            .to_string(),
    }
}

/// Send a POST with (or without) a session's credentials, always carrying the
/// CSRF header unless `with_csrf` says otherwise.
async fn post(
    app: &Router,
    uri: &str,
    body: &str,
    session: Option<&Session>,
    with_csrf: bool,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(session) = session {
        builder = builder.header("authorization", format!("Bearer {}", session.token));
        if with_csrf {
            builder = builder.header("x-archava-csrf", &session.csrf);
        }
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(Body::from(body.to_string()))
                .expect("build the request"),
        )
        .await
        .expect("the route must answer");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read the response body");
    let outcome: Value = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!(
            "the response is JSON: {error}; status={status} body={:?}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, outcome)
}

/// Send a GET with an optional session's bearer token (no CSRF — a read).
async fn get(app: &Router, uri: &str, session: Option<&Session>) -> (StatusCode, Value) {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(session) = session {
        builder = builder.header("authorization", format!("Bearer {}", session.token));
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).expect("build the request"))
        .await
        .expect("the route must answer");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read the response body");
    let outcome: Value = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!(
            "the response is JSON: {error}; status={status} body={:?}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, outcome)
}

// --- Submission bodies ---------------------------------------------------------

fn table_body(tenant: &str, request_id: &str, code: &str, seats: i64) -> String {
    json!({
        "tenant": tenant,
        "requestId": request_id,
        "command": {
            "operation": "table.create",
            "code": code,
            "title": format!("Table {code}"),
            "seats": seats,
        },
    })
    .to_string()
}

fn menu_body(
    tenant: &str,
    request_id: &str,
    sku: &str,
    price: i64,
    product_id: &str,
    quantity: i64,
) -> String {
    json!({
        "tenant": tenant,
        "requestId": request_id,
        "command": {
            "operation": "menu.create",
            "sku": sku,
            "title": format!("Dish {sku}"),
            "price": price,
            "recipe": [{ "productId": product_id, "quantity": quantity }],
        },
    })
    .to_string()
}

/// A dine-in ticket: one menu line, a bound table.
fn ticket_body(
    tenant: &str,
    request_id: &str,
    table_id: &str,
    menu_id: &str,
    quantity: i64,
) -> String {
    json!({
        "tenant": tenant,
        "requestId": request_id,
        "command": {
            "operation": "ticket.create",
            "customerName": "Guest party",
            "mode": "dine_in",
            "tableId": table_id,
            "guests": 2,
            "lines": [{ "menuId": menu_id, "quantity": quantity }],
        },
    })
    .to_string()
}

fn ticket_transition(
    tenant: &str,
    request_id: &str,
    operation: &str,
    ticket_id: &str,
    revision: i64,
) -> String {
    json!({
        "tenant": tenant,
        "requestId": request_id,
        "command": {
            "operation": operation,
            "id": ticket_id,
            "expectedRevision": revision,
        },
    })
    .to_string()
}

fn ticket_cancel(tenant: &str, request_id: &str, ticket_id: &str, revision: i64) -> String {
    json!({
        "tenant": tenant,
        "requestId": request_id,
        "command": {
            "operation": "ticket.cancel",
            "id": ticket_id,
            "expectedRevision": revision,
            "reason": "the guests left before the ticket was prepared",
        },
    })
    .to_string()
}

// --- Database probes ----------------------------------------------------------

/// The on-hand quantity of one product's stock cell.
async fn stock(fixture: &Fixture, tenant: &str, product_id: &str) -> i64 {
    let row: (i32,) = sqlx::query_as(
        "SELECT quantity FROM archava_commerce.stock_balances WHERE tenant_id=$1 AND product_id=$2",
    )
    .bind(tenant)
    .bind(Uuid::parse_str(product_id).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the stock cell exists");
    i64::from(row.0)
}

/// The ticket's owning foodservice order id.
async fn ticket_order(fixture: &Fixture, ticket_id: &str) -> Uuid {
    sqlx::query_scalar(
        "SELECT order_id FROM archava_foodservice.tickets WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(ticket_id).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the ticket exists")
}

/// The commerce order's `(state, revision)`.
async fn order_state(fixture: &Fixture, order_id: Uuid) -> (String, i32) {
    sqlx::query_as(
        "SELECT state,revision FROM archava_commerce.orders WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(order_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the order exists")
}

/// The held allocation quantity of one order.
async fn held_count(fixture: &Fixture, order_id: Uuid) -> (i64, i64) {
    sqlx::query_as(
        "SELECT count(*) FILTER (WHERE state='held'),coalesce(sum(quantity) FILTER (WHERE state='held'),0) \
         FROM archava_commerce.allocations WHERE tenant_id=$1 AND order_id=$2",
    )
    .bind(ACME)
    .bind(order_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the order's allocations read back")
}

/// Counting helper for the audit/commands probes. The statement itself is
/// always a `&'static str` literal — sqlx 0.9's injection-refusing
/// `SqlSafeStr` bound is honored, the dynamic part lives in the binds.
async fn count(fixture: &Fixture, statement: &'static str, binds: &[&str]) -> i64 {
    let mut query = sqlx::query_scalar::<_, i64>(statement);
    for bind in binds {
        query = query.bind(*bind);
    }
    query
        .fetch_one(&fixture.admin_pool)
        .await
        .expect("the count probe is a fixed statement")
}

// --- The witnesses --------------------------------------------------------------

/// THE core witness: the full authorized flow through the HTTP surface ends
/// with the commerce stock changed EXACTLY once by ONE consume movement —
/// the recipe demand (2 × 1) — a foodservice-managed order walking
/// draft → reserved → fulfilled, the `order.fulfill` audit row in the
/// commerce ledger, and a receipt bound into BOTH ledgers' commands under
/// the SAME request id. The table/menu setup, the ready/serve transitions
/// and the status read are folded into this chain because they are the
/// flow's own next breaths.
#[tokio::test]
async fn authorized_food_flow_consumes_the_shared_pool_exactly_once() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "fx").await;

    // The table.
    let (status, receipt) = post(
        &app,
        "/erp/foodservice/commands",
        &table_body(ACME, KEY_TABLE, "T-01", 4),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "table.create is authorized");
    assert_eq!(receipt["decision"], "receipt");
    assert_eq!(receipt["receipt"]["operation"], "table.create");
    let table_id = receipt["receipt"]["id"]
        .as_str()
        .expect("the receipt names the table id")
        .to_string();
    assert_eq!(receipt["receipt"]["revision"], 1);

    // The menu: a food-owned commerce portion product whose recipe takes 2
    // units of the tracked ingredient.
    let (status, menu_receipt) = post(
        &app,
        "/erp/foodservice/commands",
        &menu_body(ACME, KEY_MENU, "MENU-01", 35000, ID_P_FAST, 2),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "menu.create is authorized");
    let menu_id = menu_receipt["receipt"]["id"]
        .as_str()
        .expect("the receipt names the menu id")
        .to_string();
    // The menu product IS a commerce product, food-owned, untracked.
    let managed: (String, bool, String) = sqlx::query_as(
        "SELECT coalesce(managed_by,''),track_stock,unit FROM archava_commerce.products \
         WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(&menu_id).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the menu product exists in the commerce catalog");
    assert_eq!(managed.0, "foodservice", "the menu product is food-owned");
    assert!(
        !managed.1,
        "the menu product does not track stock — only the portions do"
    );
    assert_eq!(managed.2, "unit");
    // The menu item and its sealed recipe.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_foodservice.menu_items WHERE id=$1::uuid",
            &[&menu_id],
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_foodservice.recipe_components \
             WHERE menu_id=$1::uuid AND version=1",
            &[&menu_id],
        )
        .await,
        1,
        "exactly one ingredient — the tracked product, quantity 2"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_foodservice.recipe_versions \
             WHERE menu_id=$1::uuid AND version=1 AND sealed=true",
            &[&menu_id],
        )
        .await,
        1,
        "the recipe sealed in the same transaction"
    );

    // The ticket: a dine-in party on T-01 ordering one portion.
    let (status, ticket_receipt) = post(
        &app,
        "/erp/foodservice/commands",
        &ticket_body(ACME, KEY_TICKET, &table_id, &menu_id, 1),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "ticket.create is authorized");
    assert_eq!(ticket_receipt["receipt"]["operation"], "ticket.create");
    let ticket_id = ticket_receipt["receipt"]["id"]
        .as_str()
        .expect("the receipt names the ticket id")
        .to_string();
    let order_id = ticket_order(&fixture, &ticket_id).await;
    // The draft order exists, managed by foodservice — and the pool is
    // UNTOUCHED by the draft (a draft is a quote, not a reservation).
    assert_eq!(order_state(&fixture, order_id).await, ("draft".into(), 1));
    assert_eq!(stock(&fixture, ACME, ID_P_FAST).await, 100);

    // Queue the ticket: the recipe demand (2 × 1 = 2) becomes ONE held
    // allocation against the commerce pool. Still no consumption.
    let (status, queue_receipt) = post(
        &app,
        "/erp/foodservice/commands",
        &ticket_transition(ACME, KEY_QUEUE, "ticket.queue", &ticket_id, 1),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "ticket.queue is authorized");
    assert_eq!(queue_receipt["receipt"]["operation"], "ticket.queue");
    assert_eq!(
        order_state(&fixture, order_id).await,
        ("reserved".into(), 2)
    );
    assert_eq!(held_count(&fixture, order_id).await, (1, 2));
    assert_eq!(stock(&fixture, ACME, ID_P_FAST).await, 100);

    // Prepare the ticket: ONE commerce command claim under the SAME request
    // id, the fulfilment, and ONE consume movement → stock 98, ONCE.
    let (status, prepare_receipt) = post(
        &app,
        "/erp/foodservice/commands",
        &ticket_transition(ACME, KEY_PREPARE, "ticket.prepare", &ticket_id, 2),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "ticket.prepare is authorized");
    assert_eq!(prepare_receipt["receipt"]["operation"], "ticket.prepare");
    assert_eq!(prepare_receipt["receipt"]["revision"], 3);
    assert_eq!(
        order_state(&fixture, order_id).await,
        ("fulfilled".into(), 3)
    );
    assert_eq!(stock(&fixture, ACME, ID_P_FAST).await, 98);
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_commerce.movements \
             WHERE product_id=$1::uuid AND kind='consume'",
            &[ID_P_FAST],
        )
        .await,
        1,
        "exactly one consume movement — the pool changed once"
    );
    // The commerce ledger's own audit row for the cross-ledger claim.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_commerce.audit \
             WHERE actor_id=$1 AND operation='order.fulfill'",
            &["fx"],
        )
        .await,
        1
    );
    // And the foodservice audit row for the ticket command.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_foodservice.audit WHERE actor_id=$1",
            &["fx"],
        )
        .await,
        5,
        "table + menu + ticket + queue + prepare — the flow's five commands"
    );

    // BOTH ledgers bound the receipt under the SAME request id: the
    // foodservice receipt for the ticket, the commerce receipt for the
    // fulfilment.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_foodservice.commands \
             WHERE request_id=$1::uuid AND receipt->>'operation'='ticket.prepare'",
            &[KEY_PREPARE],
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_commerce.commands \
             WHERE request_id=$1::uuid AND receipt->>'operation'='order.fulfill'",
            &[KEY_PREPARE],
        )
        .await,
        1,
        "the commerce receipt binds the SAME key cross-ledger"
    );

    // Ready and serve: the ticket walks its own state machine.
    for (operation, key, revision) in [
        ("ticket.ready", KEY_READY, 3),
        ("ticket.serve", KEY_SERVE, 4),
    ] {
        let (status, walk) = post(
            &app,
            "/erp/foodservice/commands",
            &ticket_transition(ACME, key, operation, &ticket_id, revision),
            Some(&session),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{operation} is authorized");
        assert_eq!(walk["receipt"]["revision"], revision + 1);
    }
    let ticket_state: (String,) = sqlx::query_as(
        "SELECT state FROM archava_foodservice.tickets WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(&ticket_id).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the ticket read back");
    assert_eq!(ticket_state.0, "served");

    // The status read answers `committed` with the stored receipt.
    let (status, status_body) = get(
        &app,
        &format!("/erp/foodservice/commands/status?tenant={ACME}&requestId={KEY_PREPARE}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(status_body["decision"], "committed");
    assert_eq!(status_body["receipt"]["operation"], "ticket.prepare");
    assert_eq!(status_body["receipt"]["revision"], 3);
}

/// A reservation that would over-commit the pool refuses and holds nothing:
/// the menu's recipe demand (60 × 2 = 120) against 100 on hand exceeds the
/// pool BEFORE the allocation exists, so the queue conflicts and both the
/// ticket and the order are withdrawn from the ledger by the rollback.
#[tokio::test]
async fn over_reservation_refuses_without_holding_stock() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "fx").await;

    let (status, receipt) = post(
        &app,
        "/erp/foodservice/commands",
        &table_body(ACME, KEY_TABLE, "T-10", 4),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let table_id = receipt["receipt"]["id"].as_str().unwrap().to_string();
    let (status, menu) = post(
        &app,
        "/erp/foodservice/commands",
        &menu_body(ACME, KEY_MENU_WIDE, "MENU-WIDE", 20000, ID_P_FAST, 60),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let menu_id = menu["receipt"]["id"].as_str().unwrap().to_string();
    let (status, ticket) = post(
        &app,
        "/erp/foodservice/commands",
        &ticket_body(ACME, KEY_TICKET_HOLD, &table_id, &menu_id, 2),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ticket_id = ticket["receipt"]["id"].as_str().unwrap().to_string();
    let order_id = ticket_order(&fixture, &ticket_id).await;
    assert_eq!(order_state(&fixture, order_id).await.0, "draft");

    // 60 × 2 = 120 > 100 on hand: the repository refuses the reservation.
    let (status, refusal) = post(
        &app,
        "/erp/foodservice/commands",
        &ticket_transition(ACME, KEY_QUEUE_HOLD, "ticket.queue", &ticket_id, 1),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "an over-committing reservation conflicts"
    );
    assert_eq!(refusal["decision"], "refused");
    // The rollback withdrew the whole submission: the order is still a
    // draft, no allocation exists, and the stock stands untouched.
    assert_eq!(order_state(&fixture, order_id).await.0, "draft");
    assert_eq!(held_count(&fixture, order_id).await, (0, 0));
    assert_eq!(stock(&fixture, ACME, ID_P_FAST).await, 100);
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_foodservice.audit WHERE request_id=$1::uuid",
            &[KEY_QUEUE_HOLD],
        )
        .await,
        0,
        "the refusal recorded nothing"
    );
}

/// A menu whose recipe names a NON-TRACKED product refuses: recipes consume
/// portions of TRAACKED pool products, and the whole `menu.create` rolls
/// back — no commerce product, no menu item, no recipe.
#[tokio::test]
async fn recipe_refuses_non_tracked_ingredient() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "fx").await;

    let (status, refusal) = post(
        &app,
        "/erp/foodservice/commands",
        &menu_body(ACME, KEY_MENU_HOLD, "MENU-BAD", 12000, ID_P_MENU, 1),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a non-tracked ingredient is a recipe conflict"
    );
    assert_eq!(refusal["decision"], "refused");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_commerce.products \
             WHERE tenant_id=$1 AND sku='MENU-BAD'",
            &[ACME],
        )
        .await,
        0,
        "the menu product rolled back with the refused recipe"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_foodservice.commands \
             WHERE request_id=$1::uuid AND receipt IS NOT NULL",
            &[KEY_MENU_HOLD],
        )
        .await,
        0,
        "no receipt binds the refused submission"
    );
}

/// Foodservice has no company, so its recipes stay IDR-only
/// (docs/architecture/MULTI_CURRENCY.md). The commerce insert guard already
/// keeps every unowned product in IDR, so the only way to put a non-IDR
/// ingredient in front of the recipe writer is to bypass that guard as the
/// database superuser — which is what this witness does, to show the adapter
/// refuses on its own: a non-IDR ingredient is a recipe conflict and the whole
/// `menu.create` rolls back.
#[tokio::test]
async fn recipe_refuses_an_ingredient_priced_in_another_currency() {
    let fixture = provision().await;
    seed(&fixture).await;
    let foreign = Uuid::new_v4();
    let refused = sqlx::query("INSERT INTO archava_commerce.products(tenant_id,id,sku,title,unit,track_stock,price,currency) VALUES($1,$2,'P-USD-GUARDED','Guarded row','unit',true,25000,'USD')")
        .bind(ACME)
        .bind(Uuid::new_v4())
        .execute(&fixture.admin_pool)
        .await
        .expect_err("an unowned product is IDR");
    assert_eq!(
        refused
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23514")
    );
    let mut catalog_tx = fixture.admin_pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('archava.tenant_id', $1, true)")
        .bind(ACME)
        .execute(&mut *catalog_tx)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE archava_commerce.products DISABLE TRIGGER a_currency_product")
        .execute(&mut *catalog_tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO archava_commerce.products(tenant_id,id,sku,title,unit,track_stock,price,currency) VALUES($1,$2,'P-USD-03','Product P-USD-03','unit',true,25000,'USD')")
        .bind(ACME)
        .bind(foreign)
        .execute(&mut *catalog_tx)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE archava_commerce.products ENABLE TRIGGER a_currency_product")
        .execute(&mut *catalog_tx)
        .await
        .unwrap();
    catalog_tx.commit().await.unwrap();

    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "fx").await;
    let request = Uuid::new_v4().to_string();
    let (status, refusal) = post(
        &app,
        "/erp/foodservice/commands",
        &menu_body(ACME, &request, "MENU-USD", 12000, &foreign.to_string(), 1),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a non-IDR ingredient is a recipe conflict: {refusal}"
    );
    assert_eq!(refusal["decision"], "refused");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_commerce.products \
             WHERE tenant_id=$1 AND sku='MENU-USD'",
            &[ACME],
        )
        .await,
        0,
        "the menu product rolled back with the refused recipe"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_foodservice.commands \
             WHERE request_id=$1::uuid AND receipt IS NOT NULL",
            &[request.as_str()],
        )
        .await,
        0,
        "no receipt binds the refused submission"
    );
    // The same recipe over the IDR ingredient is accepted.
    let (status, accepted) = post(
        &app,
        "/erp/foodservice/commands",
        &menu_body(
            ACME,
            &Uuid::new_v4().to_string(),
            "MENU-IDR",
            12000,
            ID_P_FAST,
            1,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
}

/// A receipt the CURRENT access no longer authorizes does not replay: the
/// same key + same bytes re-submitted after the grant was revoked is refused
/// by the grant gate, and the status read refuses equally — an old receipt
/// is not a permanent grant.
#[tokio::test]
async fn revoked_food_grant_refuses_recovery() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "foff").await;

    // While the grant is live, the receipt records.
    let (status, receipt) = post(
        &app,
        "/erp/foodservice/commands",
        &table_body(ACME, KEY_TABLE, "T-40", 2),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the active grant records");
    assert_eq!(receipt["decision"], "receipt");
    assert_eq!(receipt["receipt"]["replayed"], false);

    // The grant dies.
    sqlx::query(
        "UPDATE archava_foodservice.memberships SET active=false \
         WHERE tenant_id=$1 AND principal_id='foff'",
    )
    .bind(ACME)
    .execute(&fixture.admin_pool)
    .await
    .expect("revoke foff's foodservice membership");

    // The same key + same bytes: the claim loses and the recovery refuses —
    // the CURRENT access cannot present `tables.write`.
    let (status, replay) = post(
        &app,
        "/erp/foodservice/commands",
        &table_body(ACME, KEY_TABLE, "T-40", 2),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a revoked grant cannot replay yesterday's receipt"
    );
    assert_eq!(replay["decision"], "denied");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_foodservice.tables \
             WHERE tenant_id=$1 AND code='T-40'",
            &[ACME],
        )
        .await,
        1,
        "no SECOND table — the replay recorded nothing new"
    );

    // The status read refuses equally.
    let (status, status_body) = get(
        &app,
        &format!("/erp/foodservice/commands/status?tenant={ACME}&requestId={KEY_TABLE}"),
        Some(&session),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the revoked access cannot even recover the stored receipt"
    );
    assert_eq!(status_body["decision"], "denied");
}

/// The same idempotency key with a DIFFERENT command conflicts without
/// recording: the fingerprint mismatch refuses, the stored receipt stands,
/// and no second effect happens.
#[tokio::test]
async fn same_key_different_command_conflicts_without_recording() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "fx").await;

    // First submission under the key: a table.
    let (status, first) = post(
        &app,
        "/erp/foodservice/commands",
        &table_body(ACME, KEY_TABLE, "T-70", 4),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["decision"], "receipt");

    // The attacker submits a MENU under the SAME key: bytes differ, fingerprint
    // differs → conflict, nothing recorded.
    let (status, refusal) = post(
        &app,
        "/erp/foodservice/commands",
        &menu_body(ACME, KEY_TABLE, "MENU-HIJACK", 1000, ID_P_FAST, 1),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a key reuse with different bytes conflicts"
    );
    assert_eq!(refusal["decision"], "refused");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_commerce.products \
             WHERE tenant_id=$1 AND sku='MENU-HIJACK'",
            &[ACME],
        )
        .await,
        0,
        "the menu was never created"
    );
    // The stored receipt still answers the SAME command (the honest replay
    // branch, verified against the stored document's identity).
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_foodservice.commands \
             WHERE request_id=$1::uuid AND fingerprint IS NOT NULL",
            &[KEY_TABLE],
        )
        .await,
        1
    );
}

/// A cross-tenant caller is refused before any business statement runs:
/// full GLOBEX grants mean nothing to ACME's ledger.
#[tokio::test]
async fn cross_tenant_caller_refused_before_any_write() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "fother").await;

    let (status, refusal) = post(
        &app,
        "/erp/foodservice/commands",
        &table_body(ACME, KEY_OFFER_TABLE, "T-90", 4),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the ACME ledger refuses a GLOBEX-only caller"
    );
    assert_eq!(refusal["decision"], "denied");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_foodservice.tables \
             WHERE tenant_id=$1 AND code='T-90'",
            &[ACME],
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_foodservice.commands \
             WHERE tenant_id=$1 AND request_id=$2::uuid",
            &[ACME, KEY_OFFER_TABLE],
        )
        .await,
        0,
        "no command row was even claimed in the target ledger"
    );

    // The SAME caller in its OWN tenant is fine — key independence.
    let own = login(&app, "fgx").await;
    let (status, receipt) = post(
        &app,
        "/erp/foodservice/commands",
        &table_body(GLOBEX, KEY_OFFER_TABLE, "T-91", 4),
        Some(&own),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the same caller acts in its own ledger"
    );
    assert_eq!(receipt["decision"], "receipt");
}

/// A lost response recovers by re-submitting the SAME key + SAME bytes: the
/// replay branch answers `replayed` with the stored receipt and the ONE
/// reservation stands — no second held allocation, no double reservation.
#[tokio::test]
async fn lost_response_replays_without_a_second_reservation() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "fx").await;

    let (status, receipt) = post(
        &app,
        "/erp/foodservice/commands",
        &table_body(ACME, KEY_TABLE, "T-77", 4),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let table_id = receipt["receipt"]["id"].as_str().unwrap().to_string();
    let (status, menu) = post(
        &app,
        "/erp/foodservice/commands",
        &menu_body(ACME, KEY_MENU, "MENU-HOLD", 15000, ID_P_FAST, 1),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let menu_id = menu["receipt"]["id"].as_str().unwrap().to_string();
    let (status, ticket) = post(
        &app,
        "/erp/foodservice/commands",
        &ticket_body(ACME, KEY_TICKET_HOLD, &table_id, &menu_id, 2),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ticket_id = ticket["receipt"]["id"].as_str().unwrap().to_string();
    let ticket_revision = ticket["receipt"]["revision"].as_i64().unwrap();
    let order_id = ticket_order(&fixture, &ticket_id).await;

    // The reservation that "never arrived" (the response was lost).
    let (status, first_queue) = post(
        &app,
        "/erp/foodservice/commands",
        &ticket_transition(ACME, KEY_QUEUE, "ticket.queue", &ticket_id, ticket_revision),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the first queue recorded");
    assert_eq!(first_queue["receipt"]["replayed"], false);
    let (held_rows, held_qty) = held_count(&fixture, order_id).await;
    assert_eq!(held_rows, 1, "one held allocation");
    assert_eq!(held_qty, 2, "the recipe demand 1 × 2");

    // The re-submission with the SAME key + SAME bytes replays.
    let (status, replay) = post(
        &app,
        "/erp/foodservice/commands",
        &ticket_transition(ACME, KEY_QUEUE, "ticket.queue", &ticket_id, ticket_revision),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "an identical resubmit replays");
    assert_eq!(replay["decision"], "replayed");
    assert_eq!(replay["receipt"]["replayed"], true);
    assert_eq!(replay["receipt"]["operation"], "ticket.queue");
    let (held_rows, held_qty) = held_count(&fixture, order_id).await;
    assert_eq!(
        (held_rows, held_qty),
        (1, 2),
        "still exactly one held allocation — no double reservation"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_commerce.movements WHERE kind='consume'",
            &[],
        )
        .await,
        0,
        "the replay consumed nothing",
    );

    // The status read answers `committed` with the stored receipt.
    let (status, status_body) = get(
        &app,
        &format!("/erp/foodservice/commands/status?tenant={ACME}&requestId={KEY_QUEUE}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(status_body["decision"], "committed");
    assert_eq!(status_body["receipt"]["operation"], "ticket.queue");
}

/// A cancel before any prepare releases the reservation: the commerce order
/// cancels, the allocation releases, and the stock stands untouched — the
/// pool never saw a movement.
#[tokio::test]
async fn cancel_releases_the_reservation_before_any_consumption() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "fx").await;

    let (status, receipt) = post(
        &app,
        "/erp/foodservice/commands",
        &table_body(ACME, KEY_TABLE, "T-55", 4),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let table_id = receipt["receipt"]["id"].as_str().unwrap().to_string();
    let (status, menu) = post(
        &app,
        "/erp/foodservice/commands",
        &menu_body(ACME, KEY_MENU_HOLD, "MENU-HOLD-2", 15000, ID_P_FAST, 2),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let menu_id = menu["receipt"]["id"].as_str().unwrap().to_string();
    // NOTE: MENU-HOLD was used by the replay witness; these suites run in
    // their own fixture. The hold-menu key is the second menu this test
    // creates; its SKU is MENU-HOLD-2.
    let (status, ticket) = post(
        &app,
        "/erp/foodservice/commands",
        &ticket_body(ACME, KEY_TICKET_HOLD, &table_id, &menu_id, 1),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ticket_id = ticket["receipt"]["id"].as_str().unwrap().to_string();
    let ticket_revision = ticket["receipt"]["revision"].as_i64().unwrap();
    let order_id = ticket_order(&fixture, &ticket_id).await;

    // Reserve, then cancel — before any prepare the reservation releases.
    let (status, _) = post(
        &app,
        "/erp/foodservice/commands",
        &ticket_transition(
            ACME,
            KEY_QUEUE_HOLD,
            "ticket.queue",
            &ticket_id,
            ticket_revision,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        order_state(&fixture, order_id).await,
        ("reserved".into(), 2)
    );
    assert_eq!(held_count(&fixture, order_id).await, (1, 2));

    let (status, cancellation) = post(
        &app,
        "/erp/foodservice/commands",
        &ticket_cancel(ACME, KEY_CANCEL, &ticket_id, 2),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the cancel is authorized");
    assert_eq!(cancellation["receipt"]["operation"], "ticket.cancel");
    assert_eq!(
        order_state(&fixture, order_id).await,
        ("cancelled".into(), 3)
    );
    let released: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_commerce.allocations \
         WHERE tenant_id=$1 AND order_id=$2 AND state='released'",
    )
    .bind(ACME)
    .bind(order_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the allocation released");
    assert_eq!(released, 1, "the held allocation released, not consumed");
    assert_eq!(
        stock(&fixture, ACME, ID_P_FAST).await,
        100,
        "the pool never saw a movement"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_commerce.movements WHERE kind='consume'",
            &[],
        )
        .await,
        0
    );
}

/// An actor whose foodservice grants are missing is refused by the grant
/// gate with no command row at all — the `audit.read`-only floor manager
/// cannot write tables any more than a stranger can.
#[tokio::test]
async fn grants_missing_actor_refused_before_any_write() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "fnone").await;

    let (status, refusal) = post(
        &app,
        "/erp/foodservice/commands",
        &table_body(ACME, KEY_TABLE, "T-30", 4),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "the grant gate refuses");
    assert_eq!(refusal["decision"], "denied");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_foodservice.tables WHERE tenant_id=$1",
            &[ACME],
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_foodservice.commands \
             WHERE tenant_id=$1 AND request_id=$2::uuid",
            &[ACME, KEY_TABLE],
        )
        .await,
        0,
        "the refusal happened before the claim"
    );
}
