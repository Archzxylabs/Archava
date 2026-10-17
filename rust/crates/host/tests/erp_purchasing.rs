//! The R06 purchasing cohort's host witnesses — the seven the ERP mandate
//! names, driven through the real router (`tower::ServiceExt::oneshot`)
//! against the disposable PostgreSQL whose `archava_purchasing` schema is the
//! real migration ledger (`001_suppliers_orders_receipts`). Every assertion is
//! a fact about what the SERVER did:
//!
//! - the authorized flow (`supplier.create` → `offer.create` →
//!   `order.create` → `order.confirm` → `order.receive`) ends with the
//!   commerce stock changed EXACTLY once per accepted quantity — applied by
//!   the ledger's own `stock_movement_apply` trigger from the kernel's
//!   movement rows — plus buying-side receipts, audit rows and a durable
//!   receipt bound into BOTH ledgers' `commands.receipt`;
//! - a partial receipt that would over-accept refuses and adds no stock;
//! - a rejected-only receipt records its documents and moves no stock;
//! - a revoked purchasing grant refuses the receive and the recovery — the
//!   OLD grants cannot replay yesterday's receipt;
//! - the same idempotency key with a DIFFERENT command conflicts without
//!   recording;
//! - a cross-tenant caller is refused before any business statement runs;
//! - a lost response recovers by re-submitting the SAME key + bytes: the
//!   replay branch answers `replayed` with the stored receipt and no second
//!   effect — and the status read answers `committed` with the same document.
//!
//! The kernel side (commerce stock reads/locks) is the archava_commerce
//! fixture's own; membership rows are seeded through the admin pool exactly
//! like `crm.rs`, both ledger vocabularies the transaction ladder reads.

use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::routing::Router;
use serde_json::{json, Value};
use sqlx::types::Uuid;
use tower::ServiceExt;

/// The commerce ledger this host serves as the primary — same table list the
/// authority suite's `commerce_ledger()` pins.
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
const IDENTITY_KEY: &[u8] = &[7u8; 32];

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
/// and the purchasing ledger (`try_new_erp`'s second pass).
async fn state(fixture: &Fixture) -> AppState {
    AppState::try_new_erp(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
        &fixture.purchasing_ledger,
    )
    .await
    .expect("a ready fixture must construct serving state")
}

// --- The cast -----------------------------------------------------------------

/// The tenants this suite uses.
const ACME: &str = "acme-retail";
const GLOBEX: &str = "globex-corp";

/// The purchasing permission vocabulary (migration 001's CHECK).
const PURCHASE_ALL: &[&str] = &[
    "suppliers.read",
    "suppliers.write",
    "offers.read",
    "offers.write",
    "orders.read",
    "orders.write",
    "receipts.read",
    "receipts.write",
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

/// `pa` is the office buyer: full grants, scope all, BOTH ledgers — the
/// positive-path actor.
/// `pnone` holds only `audit.read` in the purchasing ledger — every grant
/// gate refuses.
/// `poff` is `pa` with the purchasing membership revoked (active=false).
/// `pother` holds full grants but ONLY in GLOBEX — the cross-tenant refusal.
/// `pax` is GLOBEX's own full-grant buyer — the per-tenant key-independence
/// witness (the same requestId as ACME's works there, keyed per tenant).
async fn seed(fixture: &Fixture) {
    for principal in ["pa", "pnone", "poff", "pother", "pax"] {
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

    // The commerce ledger's rows first: every single actor carries a FULL
    // commerce grant so a purchasing refusal in this suite is attributable to
    // the PURCHASING half of the ladder, not the commerce floor.
    for principal in ["pa", "pnone", "poff"] {
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
    for principal in ["pother", "pax"] {
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

    // The purchasing ledger's rows: the patent-one difference per actor.
    for principal in ["pa", "pother", "pax"] {
        sqlx::query(
            "INSERT INTO archava_purchasing.memberships \
             (tenant_id, principal_id, kind, permissions, scope, active) \
             VALUES ($1, $2, 'human', $3, 'all', true)",
        )
        .bind(if principal == "pa" { ACME } else { GLOBEX })
        .bind(principal)
        .bind(PURCHASE_ALL.to_vec())
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the purchasing membership");
    }
    sqlx::query(
        "INSERT INTO archava_purchasing.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind("pnone")
    .bind(["audit.read"].to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed pnone's purchasing membership");
    sqlx::query(
        "INSERT INTO archava_purchasing.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', false)",
    )
    .bind(ACME)
    .bind("poff")
    .bind(PURCHASE_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed poff's purchasing membership");

    // The catalog the offers point into, one tracked product per tenant plus
    // one non-tracked product in ACME. The kernel refuses to move stock for a
    // non-tracked product, so an offer ON p-acme-menu must refuse at receive.
    // Each insert runs in a tenant-scoped transaction: `init_stock` is
    // SECURITY DEFINER, so its stock-balances seed rides the session GUC the
    // ledger's own policies read (`archava.tenant_id`).
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
        // The stock cell exists from birth — `init_stock` created it above.
        catalog_tx.commit().await.expect("commit the catalog seed");
    }
}

const ID_P_FAST: &str = "5dee0a10-2222-4222-8222-0000000000f1";
const ID_P_MENU: &str = "5dee0a10-2222-4222-8222-0000000000f2";
const ID_P_GLOBEX: &str = "5dee0a10-2222-4222-8222-0000000000f3";

/// Fixed idempotency keys — stable so concurrent-witness readbacks can name
/// rows exactly.
const KEY_SUPPLIER: &str = "af3eb010-3333-4333-8333-0000000000a1";
const KEY_OFFER: &str = "af3eb010-3333-4333-8333-0000000000a2";
const KEY_ORDER: &str = "af3eb010-3333-4333-8333-0000000000a3";
const KEY_RECEIVE_1: &str = "af3eb010-3333-4333-8333-000000000091";
const KEY_RECEIVE_2: &str = "af3eb010-3333-4333-8333-000000000092";
const KEY_RECEIVE_3: &str = "af3eb010-3333-4333-8333-000000000093";

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

fn supplier_body(tenant: &str, request_id: &str, code: &str, name: &str) -> String {
    json!({
        "tenant": tenant,
        "requestId": request_id,
        "command": {
            "operation": "supplier.create",
            "code": code,
            "name": name,
        },
    })
    .to_string()
}

fn offer_body(
    tenant: &str,
    request_id: &str,
    supplier_id: &str,
    product_id: &str,
    unit_cost: i64,
) -> String {
    json!({
        "tenant": tenant,
        "requestId": request_id,
        "command": {
            "operation": "offer.create",
            "supplierId": supplier_id,
            "productId": product_id,
            "unitCost": unit_cost,
        },
    })
    .to_string()
}

fn order_body(tenant: &str, request_id: &str, supplier_id: &str, offer_id: &str) -> String {
    json!({
        "tenant": tenant,
        "requestId": request_id,
        "command": {
            "operation": "order.create",
            "supplierId": supplier_id,
            "lines": [{ "offerId": offer_id, "quantity": 10 }],
        },
    })
    .to_string()
}

fn confirm_body(tenant: &str, request_id: &str, order_id: &str, revision: i64) -> String {
    json!({
        "tenant": tenant,
        "requestId": request_id,
        "command": {
            "operation": "order.confirm",
            "id": order_id,
            "expectedRevision": revision,
            "reason": "quotation verified with the supplier",
        },
    })
    .to_string()
}

fn receive_body(
    tenant: &str,
    request_id: &str,
    order_id: &str,
    revision: i64,
    line_id: &str,
    accepted: i64,
    rejected: i64,
) -> String {
    json!({
        "tenant": tenant,
        "requestId": request_id,
        "command": {
            "operation": "order.receive",
            "id": order_id,
            "expectedRevision": revision,
            "reason": "delivery accepted at the goods-in desk",
            "lines": [{ "lineId": line_id, "accepted": accepted, "rejected": rejected }],
        },
    })
    .to_string()
}

// --- Database probes ----------------------------------------------------------

/// The on-hand quantity of one product's stock cell.
async fn stock(fixture: &Fixture, tenant: &str, product_id: &str) -> (i64, i64) {
    let row: (i32, i32) = sqlx::query_as(
        "SELECT quantity,revision FROM archava_commerce.stock_balances \
         WHERE tenant_id=$1 AND product_id=$2",
    )
    .bind(tenant)
    .bind(Uuid::parse_str(product_id).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the stock cell exists");
    (i64::from(row.0), i64::from(row.1))
}

/// The purchasing line rows of one order — `(line_id, accepted, rejected)`.
async fn lines_of(fixture: &Fixture, order_id: &str) -> Vec<(Uuid, i32, i32)> {
    sqlx::query_as(
        "SELECT id,accepted_quantity,rejected_quantity FROM archava_purchasing.lines \
         WHERE order_id=$1::uuid ORDER BY id",
    )
    .bind(Uuid::parse_str(order_id).expect("a test uuid"))
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("the order's lines exist")
}

async fn order_state(fixture: &Fixture, order_id: &str) -> (String, i32) {
    let row: (String, i32) =
        sqlx::query_as("SELECT state,revision FROM archava_purchasing.orders WHERE id=$1")
            .bind(Uuid::parse_str(order_id).expect("a test uuid"))
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("the order exists");
    row
}

/// Counting helper for the audit/commands probes. The statement itself is
/// always a `&'static str` literal — sqlx 0.9's injection refusing
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
/// with the commerce stock changed exactly once per accepted quantity, both
/// sides' audit trails record every step, and the durable receipt is bound
/// into BOTH commands ledgers. The partial delivery, the idempotent replay
/// and the status read are folded into this chain because they are the flow's
/// own next breaths.
#[tokio::test]
async fn authorized_flow_changes_stock_exactly_once() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "pa").await;

    // The supplier.
    let (status, receipt) = post(
        &app,
        "/erp/purchasing/commands",
        &supplier_body(ACME, KEY_SUPPLIER, "ACME-SUP-01", "Acme Groceries Corp"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "supplier.create is authorized");
    assert_eq!(receipt["decision"], "receipt");
    let supplier_id = receipt["receipt"]["id"]
        .as_str()
        .expect("the receipt names the supplier id")
        .to_string();
    assert_eq!(receipt["receipt"]["operation"], "supplier.create");
    assert_eq!(receipt["receipt"]["revision"], 1);

    // The offer for the tracked product.
    let (status, offer) = post(
        &app,
        "/erp/purchasing/commands",
        &offer_body(
            ACME,
            KEY_OFFER,
            &supplier_id,
            ID_P_FAST,
            10_000, // IDR 10.000 per unit → 10 units = 100_000 total
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "offer.create is authorized");
    let offer_id = offer["receipt"]["id"]
        .as_str()
        .expect("the receipt names the offer id")
        .to_string();

    // The order.
    let (status, order_receipt) = post(
        &app,
        "/erp/purchasing/commands",
        &order_body(ACME, KEY_ORDER, &supplier_id, &offer_id),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "order.create is authorized");
    let order_id = order_receipt["receipt"]["id"]
        .as_str()
        .expect("the receipt names the order id")
        .to_string();
    assert_eq!(order_state(&fixture, &order_id).await.0, "draft");
    assert_eq!(stock(&fixture, ACME, ID_P_FAST).await.0, 0);

    // The line id the receipts will address comes from the frozen snapshot.
    let (line_id, _, _) = lines_of(&fixture, &order_id)
        .await
        .into_iter()
        .next()
        .expect("the draft has one line");

    // Confirm — the re-quote must pair today's offer with the frozen line.
    let (status, confirmation) = post(
        &app,
        "/erp/purchasing/commands",
        &confirm_body(ACME, "af3eb010-3333-4333-8333-0000000000a4", &order_id, 1),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "order.confirm is authorized");
    assert_eq!(confirmation["receipt"]["revision"], 2);
    assert_eq!(
        order_state(&fixture, &order_id).await,
        ("confirmed".into(), 2)
    );

    // First receive: 6 of 10 accepted → stock EXACTLY +6.
    let (status, partial) = post(
        &app,
        "/erp/purchasing/commands",
        &receive_body(
            ACME,
            KEY_RECEIVE_1,
            &order_id,
            2,
            &line_id.to_string(),
            6,
            0,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the partial receive is authorized");
    assert_eq!(partial["receipt"]["operation"], "order.receive");
    assert_eq!(partial["receipt"]["revision"], 3);
    assert_eq!(partial["receipt"]["replayed"], false);
    assert_eq!(
        order_state(&fixture, &order_id).await,
        ("partial".into(), 3)
    );
    assert_eq!(stock(&fixture, ACME, ID_P_FAST).await.0, 6);
    // One movement carries the accepted quantity, purchase-bound.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_commerce.movements \
             WHERE product_id=$1::uuid AND kind='purchase'",
            &[ID_P_FAST],
        )
        .await,
        1,
        "exactly one purchase movement — one stock change, recorded once"
    );

    // The receipts ledger carries the delivery document and its lines.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_purchasing.receipts WHERE order_id=$1::uuid",
            &[&order_id],
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_purchasing.receipt_lines WHERE line_id=$1::uuid",
            &[&line_id.to_string()],
        )
        .await,
        1
    );

    // Second receive: the rest → stock EXACTLY +10 total, order received.
    let (status, _done) = post(
        &app,
        "/erp/purchasing/commands",
        &receive_body(
            ACME,
            KEY_RECEIVE_3,
            &order_id,
            3,
            &line_id.to_string(),
            4,
            0,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the completing receive is authorized"
    );
    assert_eq!(
        order_state(&fixture, &order_id).await,
        ("received".into(), 4)
    );
    assert_eq!(stock(&fixture, ACME, ID_P_FAST).await.0, 10);

    // Both sides' audit trails: a purchasing row per command — supplier,
    // offer, order, confirmation and two real receives (the REPLAY re-answers
    // from the stored receipt and records nothing) — plus the commerce
    // kernel's purchase.receive row for each real receive.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_purchasing.audit WHERE actor_id=$1",
            &["pa"],
        )
        .await,
        6
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_commerce.audit WHERE actor_id=$1 AND operation='purchase.receive'",
            &["pa"],
        )
        .await,
        2
    );

    // The receipts are durable: BOTH ledgers' commands carry the receive
    // document the deferred triggers reconciled at commit.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_purchasing.commands \
             WHERE request_id=$1::uuid AND receipt IS NOT NULL",
            &[KEY_RECEIVE_1],
        )
        .await,
        1,
        "the purchasing receipt binds the key"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_commerce.commands \
             WHERE request_id=$1::uuid AND receipt->>'domain'='purchasing'",
            &[KEY_RECEIVE_1],
        )
        .await,
        1,
        "the commerce receipt binds the same key cross-ledger"
    );

    // The replay branch: the SAME key + SAME bytes answers with the stored
    // receipt and adds NO second effect.
    let (status, replay) = post(
        &app,
        "/erp/purchasing/commands",
        &receive_body(
            ACME,
            KEY_RECEIVE_1,
            &order_id,
            2,
            &line_id.to_string(),
            6,
            0,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "an identical resubmit replays");
    assert_eq!(replay["decision"], "replayed");
    assert_eq!(replay["receipt"]["replayed"], true);
    assert_eq!(replay["receipt"]["operation"], "order.receive");
    assert_eq!(
        stock(&fixture, ACME, ID_P_FAST).await.0,
        10,
        "the replay writes no stock"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_commerce.movements WHERE product_id=$1::uuid AND kind='purchase'",
            &[ID_P_FAST],
        )
        .await,
        2,
        "still exactly two movements — one per real delivery"
    );

    // The status read answers `committed` with the stored receipt.
    let (status, status_body) = get(
        &app,
        &format!("/erp/purchasing/commands/status?tenant={ACME}&requestId={KEY_RECEIVE_1}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(status_body["decision"], "committed");
    assert_eq!(status_body["receipt"]["operation"], "order.receive");
    assert_eq!(status_body["receipt"]["revision"], 3);
}

/// An over-accepting partial receipt refuses and moves no stock: the bound is
/// the line's remaining quantity, checked BEFORE the receipt rows exist.
#[tokio::test]
async fn over_acceptance_refuses_without_stock() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "pa").await;

    let (status, receipt) = post(
        &app,
        "/erp/purchasing/commands",
        &supplier_body(ACME, KEY_SUPPLIER, "ACME-SUP-01", "Acme Groceries Corp"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let supplier_id = receipt["receipt"]["id"].as_str().unwrap().to_string();
    let (status, offer) = post(
        &app,
        "/erp/purchasing/commands",
        &offer_body(ACME, KEY_OFFER, &supplier_id, ID_P_FAST, 10_000),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let offer_id = offer["receipt"]["id"].as_str().unwrap().to_string();
    let (status, order_receipt) = post(
        &app,
        "/erp/purchasing/commands",
        &order_body(ACME, KEY_ORDER, &supplier_id, &offer_id),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let order_id = order_receipt["receipt"]["id"].as_str().unwrap().to_string();
    let (line_id, _, _) = lines_of(&fixture, &order_id)
        .await
        .into_iter()
        .next()
        .expect("the draft has one line");
    let (status, _) = post(
        &app,
        "/erp/purchasing/commands",
        &confirm_body(ACME, "af3eb010-3333-4333-8333-0000000000a4", &order_id, 1),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 7 accepted of 10 → partial.
    let (status, _) = post(
        &app,
        "/erp/purchasing/commands",
        &receive_body(
            ACME,
            KEY_RECEIVE_1,
            &order_id,
            2,
            &line_id.to_string(),
            7,
            0,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stock(&fixture, ACME, ID_P_FAST).await.0, 7);

    // 4 more would reach 11 on a 10-line: the repository refuses.
    let (status, refusal) = post(
        &app,
        "/erp/purchasing/commands",
        &receive_body(
            ACME,
            KEY_RECEIVE_2,
            &order_id,
            3,
            &line_id.to_string(),
            4,
            0,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "an over-accepting receipt conflicts"
    );
    assert_eq!(refusal["decision"], "refused");
    assert_eq!(stock(&fixture, ACME, ID_P_FAST).await.0, 7);
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_purchasing.receipts WHERE order_id=$1::uuid",
            &[&order_id],
        )
        .await,
        1,
        "only the first delivery's receipt exists"
    );
    // And the refused receive's own key was NOT pillaged: a fresh finding on
    // the same command row re-checks the CURRENT access — status is its
    // honest answer when nothing was committed under that key.
    let (status, status_body) = get(
        &app,
        &format!("/erp/purchasing/commands/status?tenant={ACME}&requestId={KEY_RECEIVE_2}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        status_body["decision"], "unknown",
        "a rolled-back command never commits a phantom receipt"
    );
}

/// A rejected-only delivery records the receipt documents and moves no
/// stock — the goods-in desk refused the shipment at the dock.
#[tokio::test]
async fn rejected_only_delivery_adds_no_stock() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "pa").await;

    let (status, receipt) = post(
        &app,
        "/erp/purchasing/commands",
        &supplier_body(ACME, KEY_SUPPLIER, "ACME-SUP-01", "Acme Groceries Corp"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let supplier_id = receipt["receipt"]["id"].as_str().unwrap().to_string();
    let (status, offer) = post(
        &app,
        "/erp/purchasing/commands",
        &offer_body(ACME, KEY_OFFER, &supplier_id, ID_P_FAST, 5_000),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let offer_id = offer["receipt"]["id"].as_str().unwrap().to_string();
    let (status, order_receipt) = post(
        &app,
        "/erp/purchasing/commands",
        &order_body(ACME, KEY_ORDER, &supplier_id, &offer_id),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let order_id = order_receipt["receipt"]["id"].as_str().unwrap().to_string();
    let (line_id, _, _) = lines_of(&fixture, &order_id)
        .await
        .into_iter()
        .next()
        .expect("the draft has one line");
    let (status, _) = post(
        &app,
        "/erp/purchasing/commands",
        &confirm_body(ACME, "af3eb010-3333-4333-8333-0000000000a4", &order_id, 1),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A fully rejected delivery: accepted 0, rejected 5.
    let (status, _done) = post(
        &app,
        "/erp/purchasing/commands",
        &receive_body(
            ACME,
            KEY_RECEIVE_1,
            &order_id,
            2,
            &line_id.to_string(),
            0,
            5,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "a rejected-only receive is allowed");
    assert_eq!(
        order_state(&fixture, &order_id).await,
        ("partial".into(), 3)
    );
    assert_eq!(
        stock(&fixture, ACME, ID_P_FAST).await.0,
        0,
        "rejected goods never touch the stock cell"
    );
    // The receipt documents still exist — the delivery was DECLARED.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_purchasing.receipts WHERE order_id=$1::uuid",
            &[&order_id],
        )
        .await,
        1
    );
    // The product the order's line names is the MENU product; but no movement
    // exists anywhere in the ledger for this request.
    // The receive side of the ledger KNOWS the delivery was declared: the
    // commerce commands row carries the receipt (with its empty line list the
    // trigger reconciled), the kernel audit row exists, and the movement
    // table holds NOTHING — no stock change is what makes it rejected-only.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_commerce.movements WHERE request_id=$1::uuid",
            &[KEY_RECEIVE_1],
        )
        .await,
        0,
        "a rejected-only receipt writes no commerce movement"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_commerce.audit WHERE request_id=$1::uuid \
             AND operation='purchase.receive'",
            &[KEY_RECEIVE_1],
        )
        .await,
        1,
        "the kernel still audits the declared delivery"
    );
}

/// The quotation gate refuses an offer whose product does not track stock:
/// buying a MENU product would make any later stock claim a lie, so the
/// ORDER never gets created at all.
#[tokio::test]
async fn non_tracked_offers_refuse_the_order() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "pa").await;

    let (status, receipt) = post(
        &app,
        "/erp/purchasing/commands",
        &supplier_body(ACME, KEY_SUPPLIER, "ACME-SUP-01", "Acme Groceries Corp"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let supplier_id = receipt["receipt"]["id"].as_str().unwrap().to_string();
    let (status, offender) = post(
        &app,
        "/erp/purchasing/commands",
        &offer_body(ACME, KEY_OFFER, &supplier_id, ID_P_MENU, 5_000),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "an offer over a non-tracked product conflicts at creation"
    );
    assert_eq!(offender["decision"], "refused");

    // With no offer there is no quotation: the order attempt is moot, but a
    // direct one against the same supplier proves no line was minted.
    let (status, refusal) = post(
        &app,
        "/erp/purchasing/commands",
        json!({
            "tenant": ACME,
            "requestId": KEY_ORDER,
            "command": {
                "operation": "order.create",
                "supplierId": supplier_id,
                "lines": [{ "offerId": ID_P_MENU, "quantity": 10 }],
            },
        })
        .to_string()
        .as_str(),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "no order can quote an offer that does not exist"
    );
    assert_eq!(refusal["decision"], "refused");

    // Nothing was recorded: no offer row, no order row, no lines.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_purchasing.offers",
            &[],
        )
        .await,
        0,
        "no offer exists for a refused creation"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_purchasing.orders",
            &[],
        )
        .await,
        0,
        "no order exists for a refused quotation"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_purchasing.lines",
            &[],
        )
        .await,
        0
    );
}

/// A revoked purchasing grant refuses the receive — and refuses the
/// recovery of an earlier receipt, because recovery re-judges CURRENT access.
#[tokio::test]
async fn revoked_grant_refuses_receive_and_recovery() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "pa").await;

    let (status, receipt) = post(
        &app,
        "/erp/purchasing/commands",
        &supplier_body(ACME, KEY_SUPPLIER, "ACME-SUP-01", "Acme Groceries Corp"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let supplier_id = receipt["receipt"]["id"].as_str().unwrap().to_string();
    let (status, offer) = post(
        &app,
        "/erp/purchasing/commands",
        &offer_body(ACME, KEY_OFFER, &supplier_id, ID_P_FAST, 10_000),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let offer_id = offer["receipt"]["id"].as_str().unwrap().to_string();
    let (status, order_receipt) = post(
        &app,
        "/erp/purchasing/commands",
        &order_body(ACME, KEY_ORDER, &supplier_id, &offer_id),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let order_id = order_receipt["receipt"]["id"].as_str().unwrap().to_string();
    let (line_id, _, _) = lines_of(&fixture, &order_id)
        .await
        .into_iter()
        .next()
        .expect("the draft has one line");
    let (status, _) = post(
        &app,
        "/erp/purchasing/commands",
        &confirm_body(ACME, "af3eb010-3333-4333-8333-0000000000a4", &order_id, 1),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Revocation happens OUTSIDE any request — the operator's own SQL.
    sqlx::query(
        "UPDATE archava_purchasing.memberships SET active=false \
         WHERE principal_id='pa' AND tenant_id=$1",
    )
    .bind(ACME)
    .execute(&fixture.admin_pool)
    .await
    .expect("revoke the purchasing membership");

    let (status, refusal) = post(
        &app,
        "/erp/purchasing/commands",
        &receive_body(
            ACME,
            KEY_RECEIVE_1,
            &order_id,
            2,
            &line_id.to_string(),
            10,
            0,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a revoked purchasing grant refuses the receive"
    );
    assert_eq!(refusal["decision"], "denied");
    assert_eq!(refusal["reason"], "forbidden");
    assert_eq!(stock(&fixture, ACME, ID_P_FAST).await.0, 0);

    // The status read under the revoked grant refuses too — recovery is not
    // a loophole around the ladder.
    let (status, recovery_refusal) = get(
        &app,
        &format!("/erp/purchasing/commands/status?tenant={ACME}&requestId={KEY_ORDER}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(recovery_refusal["decision"], "denied");
}

/// The same idempotency key with a DIFFERENT command conflicts without
/// recording: the fingerprint binds bytes, not just keys.
#[tokio::test]
async fn same_key_different_command_conflicts() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "pa").await;

    let (status, _) = post(
        &app,
        "/erp/purchasing/commands",
        &supplier_body(ACME, KEY_SUPPLIER, "ACME-SUP-01", "Acme Groceries Corp"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Same key, same operation, DIFFERENT bytes: the command name differs.
    let (status, refusal) = post(
        &app,
        "/erp/purchasing/commands",
        &supplier_body(ACME, KEY_SUPPLIER, "ACME-SUP-01", "A Different Name Ltd"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "same key with different bytes must conflict"
    );
    assert_eq!(refusal["decision"], "refused");

    // And nothing was recorded for the rejected attempt.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_purchasing.audit WHERE actor_id=$1",
            &["pa"],
        )
        .await,
        1,
        "one command, one audit row"
    );
    let stored: String =
        sqlx::query_scalar("SELECT name FROM archava_purchasing.suppliers WHERE code=$1")
            .bind("ACME-SUP-01")
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("the supplier exists");
    assert_eq!(
        stored, "Acme Groceries Corp",
        "the stored row keeps the FIRST command's name"
    );
}

/// A caller with no membership in the tenant is refused before any business
/// statement — and the same idempotency key remains free per-tenant: GLOBEX's
/// own buyer can use it in its own ledger.
#[tokio::test]
async fn cross_tenant_caller_is_refused_and_keys_stay_per_tenant() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);

    // GLOBEX's own buyer proves the key works in ITS tenant.
    let globex_session = login(&app, "pax").await;
    let (status, globex_receipt) = post(
        &app,
        "/erp/purchasing/commands",
        &supplier_body(GLOBEX, KEY_SUPPLIER, "GLBX-SUP-01", "Globex Supplies Ltd"),
        Some(&globex_session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the same key works in the caller's own tenant"
    );
    assert_eq!(
        globex_receipt["receipt"]["operation"], "supplier.create",
        "the GLOBEX receipt is its own command, not ACME's replay"
    );

    // ACME's session, reused by `pother` (a GLOBEX-only member): the session
    // is valid, but the PURCHASING tower refuses before any statement.
    let acme_session = login(&app, "pother").await;
    let (status, refusal) = post(
        &app,
        "/erp/purchasing/commands",
        &supplier_body(ACME, KEY_OFFER, "ACME-SUP-07", "Should Never Exist Ltd"),
        Some(&acme_session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a non-member cannot run a command in a tenant they have no grant in"
    );
    assert_eq!(refusal["decision"], "denied");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_purchasing.suppliers WHERE code=$1",
            &["ACME-SUP-07"],
        )
        .await,
        0,
        "no row exists for the refused attempt"
    );
}

/// A grant-missing caller is refused at the ladder even in their own tenant:
/// `pnone` holds only `audit.read` in the purchasing ledger.
#[tokio::test]
async fn missing_purchasing_grant_refuses_the_ladder() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "pnone").await;

    let (status, refusal) = post(
        &app,
        "/erp/purchasing/commands",
        &supplier_body(ACME, KEY_SUPPLIER, "ACME-SUP-08", "Should Never Exist Corp"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the grants gate refuses before any business statement"
    );
    assert_eq!(refusal["decision"], "denied");
    assert_eq!(refusal["reason"], "forbidden");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_purchasing.suppliers WHERE code=$1",
            &["ACME-SUP-08"],
        )
        .await,
        0
    );
}

/// The lost-response witness, isolated: a submit whose response never
/// reaches the caller (the answer packet dies in transit) recovers by
/// re-submitting the SAME key and bytes — `replayed`, no second effect,
/// `committed` on the status read.
#[tokio::test]
async fn lost_response_recovers_by_replay_and_status() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "pa").await;

    // The first submission SUCCEEDS server-side; the caller never sees it
    // (nothing distinguishes the wire from a hard crash besides the caller's
    // own knowledge).
    let (status, first) = post(
        &app,
        "/erp/purchasing/commands",
        &supplier_body(ACME, KEY_SUPPLIER, "ACME-SUP-01", "Acme Groceries Corp"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let supplier_id = first["receipt"]["id"].as_str().unwrap().to_string();

    // The status read — the recovery oracle — answers `committed` with the
    // stored receipt BEFORE any resubmit.
    let (status, committed) = get(
        &app,
        &format!("/erp/purchasing/commands/status?tenant={ACME}&requestId={KEY_SUPPLIER}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(committed["decision"], "committed");
    assert_eq!(committed["receipt"]["operation"], "supplier.create");
    assert_eq!(committed["receipt"]["id"], json!(supplier_id));

    // The byte-identical resubmit replays rather than re-executing.
    let (status, replay) = post(
        &app,
        "/erp/purchasing/commands",
        &supplier_body(ACME, KEY_SUPPLIER, "ACME-SUP-01", "Acme Groceries Corp"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["decision"], "replayed");
    assert_eq!(replay["receipt"]["replayed"], true);
    assert_eq!(replay["receipt"]["id"], json!(supplier_id));

    // Exactly one supplier row, exactly one audit row.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_purchasing.suppliers WHERE code=$1",
            &["ACME-SUP-01"],
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_purchasing.audit WHERE actor_id=$1",
            &["pa"],
        )
        .await,
        1
    );

    // An UNKNOWN key answers `unknown` honestly.
    let (status, unknown) = get(
        &app,
        &format!("/erp/purchasing/commands/status?tenant={ACME}&requestId={KEY_RECEIVE_2}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unknown["decision"], "unknown");
    assert_eq!(unknown["receipt"], Value::Null);
}
