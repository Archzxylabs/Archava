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
        &fixture.erp_readiness(),
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
/// `pb` is a second full-grant buyer in ACME — the peer whose status read must
/// not recover `pa`'s receipt.
/// `pnone` holds only `audit.read` in the purchasing ledger — every grant
/// gate refuses.
/// `poff` is `pa` with the purchasing membership revoked (active=false).
/// `pother` holds full grants but ONLY in GLOBEX — the cross-tenant refusal.
/// `pax` is GLOBEX's own full-grant buyer — the per-tenant key-independence
/// witness (the same requestId as ACME's works there, keyed per tenant).
async fn seed(fixture: &Fixture) {
    for principal in ["pa", "pb", "pnone", "poff", "pother", "pax"] {
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
    for principal in ["pa", "pb", "pnone", "poff"] {
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
    for principal in ["pa", "pb", "pother", "pax"] {
        sqlx::query(
            "INSERT INTO archava_purchasing.memberships \
             (tenant_id, principal_id, kind, permissions, scope, active) \
             VALUES ($1, $2, 'human', $3, 'all', true)",
        )
        .bind(if matches!(principal, "pother" | "pax") {
            GLOBEX
        } else {
            ACME
        })
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

/// A same-tenant peer with full purchasing grants still cannot recover another
/// actor's receipt by naming its request id. The stored `actor_id` is part of
/// the recovery gate, not just the tenant and the current grants.
#[tokio::test]
async fn status_refuses_a_peer_actor() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let owner = login(&app, "pa").await;
    let peer = login(&app, "pb").await;

    let (status, receipt) = post(
        &app,
        "/erp/purchasing/commands",
        &supplier_body(ACME, KEY_SUPPLIER, "ACME-SUP-01", "Acme Groceries Corp"),
        Some(&owner),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "supplier.create is authorized");
    assert_eq!(receipt["decision"], "receipt");

    let uri = format!("/erp/purchasing/commands/status?tenant={ACME}&requestId={KEY_SUPPLIER}");

    // The owner recovers its own receipt.
    let (status, body) = get(&app, &uri, Some(&owner)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["decision"], "committed");
    assert_eq!(body["receipt"]["operation"], "supplier.create");

    // The peer shares the tenant and the grants, but not the actor id.
    let (status, body) = get(&app, &uri, Some(&peer)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a peer must not recover another actor's receipt"
    );
    assert_eq!(body["decision"], "denied");
    assert_eq!(body["reason"], "forbidden");
    assert!(body["receipt"].is_null(), "no receipt bytes leak");
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

// --- Trade currency (docs/architecture/MULTI_CURRENCY.md, stage 2) ---------

/// The whole mounted router, with `trader` able to register companies and
/// parties, create books, own a catalog and purchase (documents included).
async fn trade_setup() -> (Fixture, Router, Session) {
    let f = provision().await;
    sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES('trader','trader@archava.test',$1,true,1)").bind(PASSWORD_HASH).execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_organizations.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,'trader','human',ARRAY['organizations.read','organizations.write','companies.read','companies.write','parties.read','parties.write'],'all')").bind(ACME).execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_accounting.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,'trader','human',$2,'all')").bind(ACME).bind(archava_domain::erp::accounting::PERMISSIONS).execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,'trader','human',$2,'all')").bind(ACME).bind(archava_adapters::erp::COMMERCE_PERMISSION_VOCABULARY).execute(&f.admin_pool).await.unwrap();
    let mut purchasing = PURCHASE_ALL.to_vec();
    purchasing.extend(["documents.read", "documents.write"]);
    sqlx::query("INSERT INTO archava_purchasing.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,'trader','human',$2,'all')").bind(ACME).bind(purchasing).execute(&f.admin_pool).await.unwrap();
    let state = AppState::try_new_all(
        f.runtime_pool.clone(),
        commerce_ledger(),
        archava_host::router::AllReadiness {
            commerce: &f.commerce_ledger,
            identity: &f.identity_ledger,
            crm: &f.crm_ledger,
            owner: &f.owner_ledger,
            purchasing: &f.purchasing_ledger,
            additional_erp: &f.erp_readiness(),
        },
        IDENTITY_KEY,
    )
    .await
    .unwrap();
    let app = archava_host::router(state);
    let session = login(&app, "trader").await;
    (f, app, session)
}
/// One command of `domain` (`commerce`, `purchasing`, `accounting`): the
/// status and the whole answer.
async fn trade_try(
    app: &Router,
    session: &Session,
    domain: &str,
    request_id: Uuid,
    command: Value,
) -> (StatusCode, Value) {
    post(
        app,
        &format!("/erp/{domain}/commands"),
        &json!({"tenant":ACME,"requestId":request_id,"command":command}).to_string(),
        Some(session),
        true,
    )
    .await
}
/// An accepted command's receipt.
async fn trade(app: &Router, session: &Session, domain: &str, command: Value) -> Value {
    let (status, body) = trade_try(app, session, domain, Uuid::new_v4(), command).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["receipt"].clone()
}
/// A refused command: a conflict that recorded no purchasing command.
async fn trade_refused(f: &Fixture, app: &Router, session: &Session, command: Value) {
    let request_id = Uuid::new_v4();
    let (status, body) = trade_try(app, session, "purchasing", request_id, command).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let recorded: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.commands WHERE request_id=$1")
            .bind(request_id)
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(recorded, 0);
}
async fn organization_command(app: &Router, session: &Session, kind: &str, data: Value) -> Value {
    let (status, body) = post(
        app,
        "/erp/organizations/commands",
        &json!({"tenant":ACME,"mutation":{"requestId":Uuid::new_v4(),"type":kind,"data":data}})
            .to_string(),
        Some(session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["receipt"].clone()
}
/// A reviewed managed company; returns its id.
async fn trade_company(app: &Router, session: &Session, code: &str) -> String {
    let organization = organization_command(
        app,
        session,
        "create_organization",
        json!({"name":format!("Company {code}"),"legalName":format!("Company {code} Official")}),
    )
    .await;
    organization_command(
        app,
        session,
        "company.create_from_organization",
        json!({"code":code,"organizationId":organization["organizationId"],"expectedSourceRevision":1,"reason":"Explicit synthetic legal entity"}),
    )
    .await["companyId"]
        .as_str()
        .unwrap()
        .into()
}
/// A supplier created from a reviewed supplier party; returns its id.
async fn trade_supplier(app: &Router, session: &Session, code: &str) -> String {
    let organization = organization_command(
        app,
        session,
        "create_organization",
        json!({"name":format!("Vendor {code}"),"legalName":format!("Vendor {code} Official")}),
    )
    .await;
    let party = organization_command(
        app,
        session,
        "party.create_from_organization",
        json!({"organizationId":organization["organizationId"],"expectedSourceRevision":1,"customer":false,"supplier":true}),
    )
    .await;
    trade(
        app,
        session,
        "purchasing",
        json!({"operation":"supplier.create-from-party","code":code,"partyId":party["partyId"],"expectedPartyRevision":1}),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .into()
}
async fn trade_book(app: &Router, session: &Session, company: &str, currency: &str) {
    trade(app, session, "accounting", book_command(company, currency)).await;
}
fn book_command(company: &str, currency: &str) -> Value {
    json!({"type":"book.create","companyId":company,"expectedCompanyRevision":1,"code":format!("BOOK-{currency}"),"name":"Explicit synthetic official book","currency":currency,"sourceAuthority":"native","independentPosting":true,"reason":"Reviewed book setup; no historical import"})
}
async fn books(f: &Fixture, company: &str) -> Vec<String> {
    sqlx::query_scalar("SELECT currency FROM archava_accounting.books WHERE company_id=$1::uuid")
        .bind(company)
        .fetch_all(&f.admin_pool)
        .await
        .unwrap()
}
/// `book.create` in `currency` is refused as a conflict and writes nothing.
async fn book_refused(f: &Fixture, app: &Router, session: &Session, company: &str, currency: &str) {
    let request_id = Uuid::new_v4();
    let (status, body) = trade_try(
        app,
        session,
        "accounting",
        request_id,
        book_command(company, currency),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(books(f, company).await, Vec::<String>::new());
    let recorded: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_accounting.commands WHERE request_id=$1")
            .bind(request_id)
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(recorded, 0, "the refused book leaves no command");
}
/// A non-IDR book for a company that already trades. The book guard
/// (Accounting 015) refuses that through every runtime path, so the database
/// owner disables it to plant the state the older guards still defend against.
async fn planted_book(f: &Fixture, app: &Router, session: &Session, company: &str, currency: &str) {
    sqlx::query("ALTER TABLE archava_accounting.books DISABLE TRIGGER a_currency_book")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    trade_book(app, session, company, currency).await;
    sqlx::query("ALTER TABLE archava_accounting.books ENABLE TRIGGER a_currency_book")
        .execute(&f.admin_pool)
        .await
        .unwrap();
}
/// A stock-tracked product of `company`; returns its id.
async fn company_product(app: &Router, session: &Session, company: &str, sku: &str) -> String {
    trade(app,session,"commerce",json!({"operation":"product.create_for_company","companyId":company,"expectedCompanyRevision":1,"sku":sku,"title":format!("Product {sku}"),"unit":"unit","trackStock":true,"price":1999})).await["id"].as_str().unwrap().into()
}
async fn trade_offer(
    app: &Router,
    session: &Session,
    supplier: &str,
    product: &str,
    cost: i64,
) -> String {
    trade(
        app,
        session,
        "purchasing",
        json!({"operation":"offer.create","supplierId":supplier,"productId":product,"unitCost":cost}),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .into()
}
fn company_purchase(company: &str, supplier: &str, offers: &[&str], quantity: i64) -> Value {
    let lines: Vec<Value> = offers
        .iter()
        .map(|offer| json!({"offerId":offer,"quantity":quantity}))
        .collect();
    json!({"operation":"order.create-for-company","companyId":company,"expectedCompanyRevision":1,"supplierId":supplier,"lines":lines})
}
/// A supplier claim on a received order: `(order id, its revision, line id)`.
fn supplier_claim(
    (order, revision, line): (&str, i64, Uuid),
    supplier: &str,
    reference: &str,
    currency: &str,
    quantity: i64,
    cost: i64,
) -> Value {
    json!({"operation":"document.capture","orderId":order,"expectedOrderRevision":revision,"supplierId":supplier,"vendorReference":reference,"documentDate":"2026-11-07","currency":currency,"declaredTotal":quantity*cost,"lines":[{"lineId":line,"quantity":quantity,"unitCost":cost}]})
}
async fn purchase_view(app: &Router, session: &Session, company: Option<&str>) -> Value {
    let company = company
        .map(|company| format!("&companyId={company}"))
        .unwrap_or_default();
    let (status, body) = get(
        app,
        &format!("/erp/purchasing/workspace?tenant={ACME}{company}"),
        Some(session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}
/// The `currency` the workspace read returns for one offer, order or document.
fn read_currency(view: &Value, collection: &str, id: &str) -> String {
    view[collection]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["id"] == id)
        .unwrap_or_else(|| panic!("{collection} {id} is missing from {view}"))["currency"]
        .as_str()
        .unwrap()
        .into()
}
async fn stored_currency(f: &Fixture, table: &str, id: &str) -> String {
    let statement = format!("SELECT currency FROM archava_purchasing.{table} WHERE id=$1::uuid");
    sqlx::query_scalar(sqlx::AssertSqlSafe(statement.as_str()))
        .bind(id)
        .fetch_one(&f.admin_pool)
        .await
        .unwrap()
}
fn refused_check(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>) -> String {
    let error = result.expect_err("the database must refuse this row");
    let database = error.as_database_error().expect("a database refusal");
    assert_eq!(database.code().as_deref(), Some("23514"), "{database}");
    database.message().to_owned()
}
/// A direct offer insert; `currency` None leaves the column default.
async fn insert_offer(
    f: &Fixture,
    supplier: &str,
    product: &str,
    currency: Option<&str>,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    let query = match currency {
        Some(_) => sqlx::query("INSERT INTO archava_purchasing.offers(tenant_id,id,supplier_id,product_id,unit_cost,currency) VALUES($1,$2,$3::uuid,$4::uuid,100,$5)"),
        None => sqlx::query("INSERT INTO archava_purchasing.offers(tenant_id,id,supplier_id,product_id,unit_cost) VALUES($1,$2,$3::uuid,$4::uuid,100)"),
    }
    .bind(ACME)
    .bind(Uuid::new_v4())
    .bind(supplier)
    .bind(product);
    match currency {
        Some(currency) => query.bind(currency),
        None => query,
    }
    .execute(&f.admin_pool)
    .await
}
/// A direct purchase-order header insert carrying the company's current
/// reviewed snapshot (or no company at all).
async fn insert_order(
    f: &Fixture,
    supplier: &str,
    company: Option<&str>,
    currency: &str,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    match company {
        Some(company) => sqlx::query("INSERT INTO archava_purchasing.orders(tenant_id,id,owner_id,supplier_id,supplier_code,supplier_name,total_estimate,company_id,company_revision,company_code,company_name,currency) SELECT s.tenant_id,$1,'trader',s.id,s.code,s.name,0,c.id,c.revision,c.code,c.legal_name,$2 FROM archava_purchasing.suppliers s JOIN archava_organizations.companies c ON c.tenant_id=s.tenant_id WHERE s.id=$3::uuid AND c.id=$4::uuid")
            .bind(Uuid::new_v4())
            .bind(currency)
            .bind(supplier)
            .bind(company)
            .execute(&f.admin_pool)
            .await,
        None => sqlx::query("INSERT INTO archava_purchasing.orders(tenant_id,id,owner_id,supplier_id,supplier_code,supplier_name,total_estimate,currency) SELECT s.tenant_id,$1,'trader',s.id,s.code,s.name,0,$2 FROM archava_purchasing.suppliers s WHERE s.id=$3::uuid")
            .bind(Uuid::new_v4())
            .bind(currency)
            .bind(supplier)
            .execute(&f.admin_pool)
            .await,
    }
}
async fn order_line(f: &Fixture, order: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM archava_purchasing.lines WHERE order_id=$1::uuid")
        .bind(order)
        .fetch_one(&f.admin_pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn company_offers_purchase_orders_and_supplier_documents_take_the_book_currency() {
    let (f, app, session) = trade_setup().await;
    let company = trade_company(&app, &session, "USDCO").await;
    trade_book(&app, &session, &company, "USD").await;
    let supplier = trade_supplier(&app, &session, "USD-VENDOR").await;
    let item = company_product(&app, &session, &company, "USD-ITEM").await;

    let offer = trade_offer(&app, &session, &supplier, &item, 1250).await;
    let order = trade(
        &app,
        &session,
        "purchasing",
        company_purchase(&company, &supplier, &[&offer], 4),
    )
    .await;
    let order = order["id"].as_str().unwrap();
    assert_eq!(stored_currency(&f, "offers", &offer).await, "USD");
    assert_eq!(stored_currency(&f, "orders", order).await, "USD");
    let view = purchase_view(&app, &session, Some(&company)).await;
    assert_eq!(read_currency(&view, "offers", &offer), "USD");
    assert_eq!(read_currency(&view, "orders", order), "USD");
    let listed = view["orders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["id"] == order)
        .unwrap();
    // Amounts are untouched: 4 x 1250 in the record's own unit.
    assert_eq!(listed["total"], 5000);
    assert_eq!(listed["lines"][0]["unitCost"], 1250);

    // The write paths read the non-IDR rows back instead of refusing them.
    trade(&app,&session,"purchasing",json!({"operation":"offer.set-cost","id":offer,"expectedRevision":1,"reason":"Supplier repriced","unitCost":1300})).await;
    trade(&app,&session,"purchasing",json!({"operation":"order.replace-lines","id":order,"expectedRevision":1,"reason":"Requote at the new cost","lines":[{"offerId":offer,"quantity":4}]})).await;
    trade(&app,&session,"purchasing",json!({"operation":"order.confirm","id":order,"expectedRevision":2,"reason":"Approved commitment"})).await;
    let line = order_line(&f, order).await;
    let receipt=trade(&app,&session,"purchasing",json!({"operation":"order.receive","id":order,"expectedRevision":3,"reason":"Three accepted, one rejected","landedFeeMinor":77,"allocationBasis":"value","lines":[{"lineId":line,"accepted":3,"rejected":1}]})).await;
    assert_eq!(order_state(&f, order).await, ("received".to_owned(), 4));
    assert_eq!(stored_currency(&f, "orders", order).await, "USD");

    // Stock and cost carry the order's integers unchanged: no conversion, no
    // whole-Rupiah scaling.
    let request = Uuid::parse_str(receipt["requestId"].as_str().unwrap()).unwrap();
    let movement: (i32, Option<i64>, i64) = sqlx::query_as("SELECT quantity,unit_cost_minor,landed_fee_minor FROM archava_commerce.movements WHERE request_id=$1 AND kind='purchase' AND product_id=$2::uuid")
        .bind(request)
        .bind(&item)
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(movement, (3, Some(1300), 77));
    let layer: (i64, i64, Option<i64>) = sqlx::query_as("SELECT original_quantity,remaining_quantity,total_cost_minor FROM archava_commerce.inventory_layers WHERE source_request_id=$1 AND product_id=$2::uuid")
        .bind(request)
        .bind(&item)
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(layer, (3, 3, Some(3 * 1300 + 77)));
    assert_eq!(stock(&f, ACME, &item).await.0, 3);
    // A purchase policy cannot exist on a non-IDR book, so nothing was
    // captured for the ledger (stage 4) and nothing refused the receipt.
    let captured: (i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM archava_accounting.purchase_origins),(SELECT count(*) FROM archava_accounting.purchase_sources)")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(captured, (0, 0));

    // A supplier document carries its purchase order's currency: USD is
    // accepted, IDR (what every document was before) is refused.
    let source = (order, 4, line);
    trade_refused(
        &f,
        &app,
        &session,
        supplier_claim(source, &supplier, "INV-IDR", "IDR", 3, 1300),
    )
    .await;
    // A malformed code never reaches the order comparison.
    let (status, body) = trade_try(
        &app,
        &session,
        "purchasing",
        Uuid::new_v4(),
        supplier_claim(source, &supplier, "INV-LOWER", "usd", 3, 1300),
    )
    .await;
    assert!(
        status.is_client_error() && status != StatusCode::CONFLICT,
        "{status} {body}"
    );
    let document = trade(
        &app,
        &session,
        "purchasing",
        supplier_claim(source, &supplier, "INV-USD", "USD", 3, 1300),
    )
    .await;
    let document = document["id"].as_str().unwrap();
    assert_eq!(
        stored_currency(&f, "supplier_documents", document).await,
        "USD"
    );
    let view = purchase_view(&app, &session, Some(&company)).await;
    assert_eq!(read_currency(&view, "documents", document), "USD");
    // A revision cannot move the document to another currency either.
    trade_refused(&f,&app,&session,json!({"operation":"document.revise","id":document,"expectedRevision":1,"expectedOrderRevision":4,"reason":"Restate the claim","documentDate":"2026-11-08","currency":"IDR","declaredTotal":3900,"lines":[{"lineId":line,"quantity":3,"unitCost":1300}]})).await;
    trade(&app,&session,"purchasing",json!({"operation":"document.match","id":document,"expectedRevision":1,"expectedOrderRevision":4,"reason":"Exact accepted-goods claim"})).await;
    let matched: (String, i64, String) = sqlx::query_as("SELECT state,declared_total,currency FROM archava_purchasing.supplier_documents WHERE id=$1::uuid")
        .bind(document)
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(matched, ("matched".to_owned(), 3900, "USD".to_owned()));
    let documents: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.supplier_documents")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(documents, 1);
}

#[tokio::test]
async fn purchases_without_a_company_or_a_book_stay_idr() {
    let (f, app, session) = trade_setup().await;
    let supplier = trade_supplier(&app, &session, "IDR-VENDOR").await;
    let unowned=trade(&app,&session,"commerce",json!({"operation":"product.create","sku":"NO-COMPANY","title":"Unowned product","unit":"unit","trackStock":true,"price":10000})).await;
    let unowned = unowned["id"].as_str().unwrap();
    let offer = trade_offer(&app, &session, &supplier, unowned, 6000).await;
    let order=trade(&app,&session,"purchasing",json!({"operation":"order.create","supplierId":supplier,"lines":[{"offerId":offer,"quantity":2}]})).await;
    let order = order["id"].as_str().unwrap();
    assert_eq!(stored_currency(&f, "offers", &offer).await, "IDR");
    assert_eq!(stored_currency(&f, "orders", order).await, "IDR");
    let view = purchase_view(&app, &session, None).await;
    assert_eq!(read_currency(&view, "offers", &offer), "IDR");
    assert_eq!(read_currency(&view, "orders", order), "IDR");
    trade(&app,&session,"purchasing",json!({"operation":"order.confirm","id":order,"expectedRevision":1,"reason":"Approved commitment"})).await;
    let line = order_line(&f, order).await;
    trade(&app,&session,"purchasing",json!({"operation":"order.receive","id":order,"expectedRevision":2,"reason":"Delivery accepted","lines":[{"lineId":line,"accepted":2,"rejected":0}]})).await;
    assert_eq!(stock(&f, ACME, unowned).await.0, 2);

    // A company without a book trades in IDR, documents included.
    let company = trade_company(&app, &session, "NOBOOK").await;
    let item = company_product(&app, &session, &company, "NOBOOK-ITEM").await;
    let offer = trade_offer(&app, &session, &supplier, &item, 7000).await;
    let order = trade(
        &app,
        &session,
        "purchasing",
        company_purchase(&company, &supplier, &[&offer], 2),
    )
    .await;
    let order = order["id"].as_str().unwrap();
    assert_eq!(stored_currency(&f, "offers", &offer).await, "IDR");
    assert_eq!(stored_currency(&f, "orders", order).await, "IDR");
    let view = purchase_view(&app, &session, Some(&company)).await;
    assert_eq!(read_currency(&view, "offers", &offer), "IDR");
    assert_eq!(read_currency(&view, "orders", order), "IDR");
    trade(&app,&session,"purchasing",json!({"operation":"order.confirm","id":order,"expectedRevision":1,"reason":"Approved commitment"})).await;
    let line = order_line(&f, order).await;
    trade(&app,&session,"purchasing",json!({"operation":"order.receive","id":order,"expectedRevision":2,"reason":"Delivery accepted","lines":[{"lineId":line,"accepted":2,"rejected":0}]})).await;
    trade_refused(
        &f,
        &app,
        &session,
        supplier_claim((order, 3, line), &supplier, "INV-USD", "USD", 2, 7000),
    )
    .await;
    let document = trade(
        &app,
        &session,
        "purchasing",
        supplier_claim((order, 3, line), &supplier, "INV-IDR", "IDR", 2, 7000),
    )
    .await;
    assert_eq!(
        stored_currency(&f, "supplier_documents", document["id"].as_str().unwrap()).await,
        "IDR"
    );
}

#[tokio::test]
async fn the_database_refuses_a_purchase_currency_other_than_the_company_book() {
    let (f, app, session) = trade_setup().await;
    let named: Vec<(String, String)> = sqlx::query_as("SELECT conrelid::regclass::text,conname::text FROM pg_constraint WHERE contype='c' AND conrelid IN ('archava_purchasing.offers'::regclass,'archava_purchasing.orders'::regclass,'archava_purchasing.supplier_documents'::regclass) AND pg_get_constraintdef(oid) LIKE '%currency%' ORDER BY 1")
        .fetch_all(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(
        named,
        vec![
            (
                "archava_purchasing.offers".to_owned(),
                "offer_currency_code".to_owned()
            ),
            (
                "archava_purchasing.orders".to_owned(),
                "purchase_currency_code".to_owned()
            ),
            (
                "archava_purchasing.supplier_documents".to_owned(),
                "supplier_document_currency_code".to_owned()
            ),
        ]
    );

    let supplier = trade_supplier(&app, &session, "GUARD-VENDOR").await;
    let company = trade_company(&app, &session, "GUARDED").await;
    // Before the book exists the company purchases in IDR, and only in IDR.
    let legacy = company_product(&app, &session, &company, "LEGACY-IDR").await;
    let spare = company_product(&app, &session, &company, "SPARE-IDR").await;
    let message = refused_check(insert_offer(&f, &supplier, &spare, Some("USD")).await);
    assert_eq!(message, "Currency differs from the company book");
    let message = refused_check(insert_order(&f, &supplier, Some(&company), "USD").await);
    assert_eq!(message, "Currency differs from the company book");
    let legacy_offer = trade_offer(&app, &session, &supplier, &legacy, 6000).await;
    let early = trade(
        &app,
        &session,
        "purchasing",
        company_purchase(&company, &supplier, &[&legacy_offer], 2),
    )
    .await;
    let early = early["id"].as_str().unwrap();
    // A company with products, an offer and a purchase order cannot open a USD
    // book; the mixed state the guards below defend against is planted by the
    // database owner.
    book_refused(&f, &app, &session, &company, "USD").await;
    planted_book(&f, &app, &session, &company, "USD").await;
    let item = company_product(&app, &session, &company, "USD-ITEM").await;

    // Offers: the company's book currency, which is also the product's.
    let message = refused_check(insert_offer(&f, &supplier, &item, Some("IDR")).await);
    assert_eq!(message, "Currency differs from the company book");
    // The column default is IDR, which this company may no longer use.
    refused_check(insert_offer(&f, &supplier, &item, None).await);
    refused_check(insert_offer(&f, &supplier, &item, Some("JPY")).await);
    // The format check replaced the IDR-only one.
    refused_check(insert_offer(&f, &supplier, &item, Some("usd")).await);
    // The planted IDR product: no offer fits it any more.
    let message = refused_check(insert_offer(&f, &supplier, &spare, Some("USD")).await);
    assert_eq!(
        message,
        "Offer is priced in another currency than its product"
    );
    let message = refused_check(insert_offer(&f, &supplier, &spare, Some("IDR")).await);
    assert_eq!(message, "Currency differs from the company book");
    trade_refused(
        &f,
        &app,
        &session,
        json!({"operation":"offer.create","supplierId":supplier,"productId":spare,"unitCost":6000}),
    )
    .await;
    // An offer on a product without a company is IDR whatever books exist.
    let unowned=trade(&app,&session,"commerce",json!({"operation":"product.create","sku":"NO-COMPANY","title":"Unowned product","unit":"unit","trackStock":true,"price":10000})).await;
    let unowned = unowned["id"].as_str().unwrap();
    let message = refused_check(insert_offer(&f, &supplier, unowned, Some("USD")).await);
    assert_eq!(message, "Currency differs from the company book");

    // Order headers obey the same rule.
    let message = refused_check(insert_order(&f, &supplier, Some(&company), "IDR").await);
    assert_eq!(message, "Currency differs from the company book");
    refused_check(insert_order(&f, &supplier, Some(&company), "JPY").await);
    let message = refused_check(insert_order(&f, &supplier, None, "USD").await);
    assert_eq!(message, "Currency differs from the company book");
    refused_check(insert_order(&f, &supplier, None, "idr").await);

    // One order, one currency: the same company's planted IDR offer and
    // product cannot enter its USD order, by command or by row.
    let offer = trade_offer(&app, &session, &supplier, &item, 1250).await;
    assert_eq!(stored_currency(&f, "offers", &offer).await, "USD");
    trade_refused(
        &f,
        &app,
        &session,
        company_purchase(&company, &supplier, &[&offer, &legacy_offer], 2),
    )
    .await;
    trade_refused(
        &f,
        &app,
        &session,
        company_purchase(&company, &supplier, &[&legacy_offer], 2),
    )
    .await;
    let order = trade(
        &app,
        &session,
        "purchasing",
        company_purchase(&company, &supplier, &[&offer], 2),
    )
    .await;
    let order = order["id"].as_str().unwrap();
    assert_eq!(stored_currency(&f, "orders", order).await, "USD");
    let message=refused_check(sqlx::query("INSERT INTO archava_purchasing.lines(tenant_id,id,order_id,offer_id,offer_revision,product_id,sku,title,unit,quantity,unit_cost) SELECT o.tenant_id,$1,$2::uuid,o.id,o.revision,p.id,p.sku,p.title,p.unit,1,o.unit_cost FROM archava_purchasing.offers o JOIN archava_commerce.products p ON p.tenant_id=o.tenant_id AND p.id=o.product_id WHERE o.id=$3::uuid").bind(Uuid::new_v4()).bind(order).bind(&legacy_offer).execute(&f.admin_pool).await);
    assert_eq!(message, "Purchase line is priced in another currency");
    trade_refused(&f,&app,&session,json!({"operation":"order.replace-lines","id":order,"expectedRevision":1,"reason":"Mix in the earlier offer","lines":[{"offerId":offer,"quantity":2},{"offerId":legacy_offer,"quantity":1}]})).await;
    let lines: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.lines WHERE order_id=$1::uuid")
            .bind(order)
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(lines, 1);

    // The planted IDR order stays IDR and, with no purchase policy, workable.
    assert_eq!(stored_currency(&f, "orders", early).await, "IDR");
    trade(&app,&session,"purchasing",json!({"operation":"order.confirm","id":early,"expectedRevision":1,"reason":"Approved commitment"})).await;
    let line = order_line(&f, early).await;
    trade(&app,&session,"purchasing",json!({"operation":"order.receive","id":early,"expectedRevision":2,"reason":"Delivery accepted","lines":[{"lineId":line,"accepted":2,"rejected":0}]})).await;
    assert_eq!(stored_currency(&f, "orders", early).await, "IDR");
    assert_eq!(stock(&f, ACME, &legacy).await.0, 2);
    // Currency never changes after insert.
    for table in ["offers", "orders"] {
        let statement = format!(
            "UPDATE archava_purchasing.{table} SET currency='IDR',revision=revision+1 WHERE id=$1::uuid"
        );
        let message = refused_check(
            sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
                .bind(if table == "offers" {
                    offer.as_str()
                } else {
                    order
                })
                .execute(&f.admin_pool)
                .await,
        );
        assert!(message.contains("immutable"), "{message}");
    }
}

#[tokio::test]
async fn a_non_idr_book_is_refused_once_the_company_has_a_purchase_order() {
    let (f, app, session) = trade_setup().await;
    let supplier = trade_supplier(&app, &session, "BOOK-VENDOR").await;
    // Product, offer and draft purchase order through the commands. The
    // purchase-order check alone is isolated in the accounting suite, where a
    // bare order row is planted; here a header without lines cannot commit.
    let company = trade_company(&app, &session, "PURCHASED").await;
    let item = company_product(&app, &session, &company, "IDR-ITEM").await;
    let offer = trade_offer(&app, &session, &supplier, &item, 6000).await;
    let order = trade(
        &app,
        &session,
        "purchasing",
        company_purchase(&company, &supplier, &[&offer], 2),
    )
    .await;
    let order = order["id"].as_str().unwrap();
    book_refused(&f, &app, &session, &company, "USD").await;

    // An IDR book is still accepted and the order stays workable in IDR.
    trade_book(&app, &session, &company, "IDR").await;
    assert_eq!(books(&f, &company).await, vec!["IDR".to_owned()]);
    assert_eq!(stored_currency(&f, "offers", &offer).await, "IDR");
    assert_eq!(stored_currency(&f, "orders", order).await, "IDR");
    trade(&app,&session,"purchasing",json!({"operation":"order.confirm","id":order,"expectedRevision":1,"reason":"Approved commitment"})).await;
    let again = trade(
        &app,
        &session,
        "purchasing",
        company_purchase(&company, &supplier, &[&offer], 1),
    )
    .await;
    assert_eq!(
        stored_currency(&f, "orders", again["id"].as_str().unwrap()).await,
        "IDR"
    );
}
