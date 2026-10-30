//! The R07 subscriptions cohort's host witnesses — driven through the real
//! router (`tower::ServiceExt::oneshot`) against the disposable PostgreSQL
//! whose `archava_subscriptions` schema is the real migration ledger
//! (`001_subscriptions_entitlements_usage`). Every assertion is a fact about
//! what the SERVER did:
//!
//! - the authorized entitlement life (create → pay → consume) ends with
//!   exactly the rows the repository's SQL spec freezes — the receipt
//!   document's cells, the subscription's status/revision chain, the
//!   entitlement's consumed counter, the usage event, and the audit naming
//!   every operation once; the READ projections answer the same rows;
//! - **an unpaid draft grants NO consumption right** (audit A05, P1): the
//!   baseline lets a paid plan's entitlement be consumed before any
//!   `record_payment`; the port refuses with the 402 class and records
//!   nothing;
//! - the quota boundary refuses beyond the allocation and holds nothing;
//! - a duplicate usage idempotency key is a CONFLICT, not the baseline's
//!   database failure, and the second attempt records nothing;
//! - a `past_due` subscription refuses usage with the TS's own 402 class;
//! - activation without a payment record refuses, and a canceled
//!   subscription is terminal;
//! - a revoked entitlement refuses usage with the 403 class;
//! - a stale revision conflicts and records nothing;
//! - the same command id with a different payload conflicts; the same
//!   command id with the same bytes replays with `idempotencyReplay: true`
//!   and still one row everywhere;
//! - a cross-tenant caller is refused before any statement runs;
//! - narrow grants refuse exactly the arms whose gate they lack while
//!   their reads walk; and
//! - the wire boundary (unknown arm, extra member, lowercase currency,
//!   zero units, unknown status filter) refuses at the right layer, and the
//!   read projections are bounded and scoped.
//!
//! Zone-free by construction: the suite's only run-relative facts are the
//! SQL-stamped instants the clock binds — read back as bytes and counted,
//! not predicted.

use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};

/// The parsed uuid a grammar-admitted id string carries — the columns are
/// `uuid`, so a text bind does not compare (`operator does not exist`).
fn uuid_of(id: &str) -> uuid::Uuid {
    uuid::Uuid::parse_str(id).expect("a grammar-admitted uuid")
}
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

/// The commerce ledger this host serves as the primary — the same table
/// list every ERP suite pins; the subscriptions surface RIDES the same
/// serving state without touching commerce rows.
const TABLES: &[&str] = &[
    "products",
    "stock_balances",
    "orders",
    "order_lines",
    "allocations",
    "movements",
    "returns",
    "commands",
    "audit",
    "approvals",
];

/// A 32-byte identity HMAC key — the minimum `Identity::open` accepts.
const IDENTITY_KEY: &[u8] = &[29u8; 32];

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
/// and the purchasing ledger (`try_new_erp`'s second pass) — the
/// subscriptions surface rides the same serving state; its own ledger rows
/// exist in the fixture regardless, and the suite's FIRST step checks the
/// subscriptions ledger's own provision facts (`assert_subscriptions_kernel`)
/// before any route is mounted.
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

/// The subscriptions permission vocabulary (migration 001's CHECK).
const SUBSCRIPTION_ALL: &[&str] = &[
    "subscriptions.read",
    "subscriptions.write",
    "subscriptions.payment",
    "subscriptions.usage",
    "audit.read",
];

/// `sx` is the full-grant manager with scope all — the positive-path actor.
/// `sxg` holds read + audit only: the four write arms refuse, while its
/// reads walk.
/// `sxn` holds only `audit.read` — every grant gate refuses.
/// `sxv` holds the full grants but the test revokes the membership itself —
/// the recovery must not outlive the grant.
/// `other` holds full grants but ONLY in GLOBEX — the cross-tenant refusal
/// that still acts freely in its own ledger.
async fn seed(fixture: &Fixture) {
    for principal in ["sx", "sxg", "sxn", "sxv", "other"] {
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

    for (principal, permissions) in [
        ("sx", SUBSCRIPTION_ALL),
        ("sxg", &["subscriptions.read", "audit.read"][..]),
        ("sxn", &["audit.read"][..]),
        ("sxv", SUBSCRIPTION_ALL),
    ] {
        sqlx::query(
            "INSERT INTO archava_subscriptions.memberships \
             (tenant_id, principal_id, kind, permissions, scope, active) \
             VALUES ($1, $2, 'human', $3, 'all', true)",
        )
        .bind(ACME)
        .bind(principal)
        .bind(permissions.to_vec())
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the ACME subscriptions membership");
    }
    sqlx::query(
        "INSERT INTO archava_subscriptions.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(GLOBEX)
    .bind("other")
    .bind(SUBSCRIPTION_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the GLOBEX subscriptions membership");
}

// --- The command keys (witness-local; hex only) -------------------------------

const KEY_CREATE: &str = "6b240300-9999-4999-9999-0000000000a1";
const KEY_PAY: &str = "6b240300-9999-4999-9999-0000000000a2";
const KEY_USE: &str = "6b240300-9999-4999-9999-0000000000a3";

const KEY_DRAFT_CREATE: &str = "6b240300-9999-4999-9999-0000000000b1";
const KEY_DRAFT_USE: &str = "6b240300-9999-4999-9999-0000000000b2";

const KEY_QUOTA_CREATE: &str = "6b240300-9999-4999-9999-0000000000c1";
const KEY_QUOTA_PAY: &str = "6b240300-9999-4999-9999-0000000000c2";
const KEY_QUOTA_OVER: &str = "6b240300-9999-4999-9999-0000000000c3";
const KEY_QUOTA_FULL: &str = "6b240300-9999-4999-9999-0000000000c4";
const KEY_QUOTA_AFTER: &str = "6b240300-9999-4999-9999-0000000000c5";

const KEY_DUP_CREATE: &str = "6b240300-9999-4999-9999-0000000000d1";
const KEY_DUP_PAY: &str = "6b240300-9999-4999-9999-0000000000d2";
const KEY_DUP_USE_1: &str = "6b240300-9999-4999-9999-0000000000d3";
const KEY_DUP_USE_2: &str = "6b240300-9999-4999-9999-0000000000d4";

const KEY_PASTDUE_CREATE: &str = "6b240300-9999-4999-9999-0000000000e1";
const KEY_PASTDUE_PAY: &str = "6b240300-9999-4999-9999-0000000000e2";
const KEY_PASTDUE_STATUS: &str = "6b240300-9999-4999-9999-0000000000e3";
const KEY_PASTDUE_USE: &str = "6b240300-9999-4999-9999-0000000000e4";

const KEY_ACTIVATE_CREATE: &str = "6b240300-9999-4999-9999-000000000101";
const KEY_ACTIVATE_TRY: &str = "6b240300-9999-4999-9999-000000000102";
const KEY_ACTIVATE_CANCEL: &str = "6b240300-9999-4999-9999-000000000103";
const KEY_ACTIVATE_RESUME: &str = "6b240300-9999-4999-9999-000000000104";

const KEY_REVOKE_CREATE: &str = "6b240300-9999-4999-9999-000000000201";
const KEY_REVOKE_PAY: &str = "6b240300-9999-4999-9999-000000000202";
const KEY_REVOKE_CANCEL: &str = "6b240300-9999-4999-9999-000000000203";
const KEY_REVOKE_USE: &str = "6b240300-9999-4999-9999-000000000204";

const KEY_STALE_CREATE: &str = "6b240300-9999-4999-9999-000000000301";
const KEY_STALE_PAY: &str = "6b240300-9999-4999-9999-000000000302";
const KEY_STALE_STATUS: &str = "6b240300-9999-4999-9999-000000000303";

const KEY_REPLAY_CREATE: &str = "6b240300-9999-4999-9999-000000000401";
const KEY_SEALED_CREATE: &str = "6b240300-9999-4999-9999-000000000601";
const KEY_SEALED_PAY: &str = "6b240300-9999-4999-9999-000000000602";
const KEY_SEALED_USE: &str = "6b240300-9999-4999-9999-000000000603";

const KEY_GRANT: &str = "6b240300-9999-4999-9999-000000000501";

/// The contact the subscriptions hang off — an opaque reference, exactly as
/// the TS treats it.
const CONTACT: &str = "6b240300-9999-4999-9999-00000000c001";

/// The plan money the suite spends.
const UNIT_PRICE: i64 = 100_000;
const ALLOCATION: i64 = 10;

// --- Session plumbing ---------------------------------------------------------

/// A logged-in session: the bearer token and the CSRF value.
struct Session {
    token: String,
    csrf: String,
}

/// The login slots — the login route's own bound.
static LOGIN_SLOTS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(archava_host::max_active_derivations());

async fn login(app: &axum::Router, principal_id: &str) -> Session {
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

/// Send a POST with a session's credentials, always carrying the CSRF
/// header — every mutation writer in this suite is authenticated and
/// CSRF-bound, like the production router.
async fn post(app: &axum::Router, uri: &str, body: &str, session: &Session) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", session.token))
                .header("x-archava-csrf", &session.csrf)
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

/// Send a GET with a session's bearer token (no CSRF — a read).
async fn get(app: &axum::Router, uri: &str, session: &Session) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header("authorization", format!("Bearer {}", session.token))
                .body(Body::empty())
                .expect("build the request"),
        )
        .await
        .expect("the route must answer");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read the response body");
    let outcome: Value = serde_json::from_slice(&bytes).expect("the response is JSON");
    (status, outcome)
}

// --- The served state ---------------------------------------------------------

/// The serving state, the router, and the five sessions.
async fn served() -> (
    Fixture,
    axum::Router,
    Session,
    Session,
    Session,
    Session,
    Session,
) {
    let fixture = provision().await;
    seed(&fixture).await;
    // The provision checks (role, the ledger, the forced-RLS width) pass
    // on the fixture's runtime role — the serving state's support check,
    // asserted here because the suite holds the receipt of it.
    archava_adapters::erp::subscriptions::assert_subscriptions_kernel(
        &fixture.runtime_pool,
        &fixture.subscriptions_ledger,
    )
    .await
    .expect("the subscriptions kernel asserts on the fixture");

    let app = archava_host::router(state(&fixture).await);
    let sx = login(&app, "sx").await;
    let sxg = login(&app, "sxg").await;
    let sxn = login(&app, "sxn").await;
    let sxv = login(&app, "sxv").await;
    let other = login(&app, "other").await;
    (fixture, app, sx, sxg, sxn, sxv, other)
}

// --- The submission bodies ----------------------------------------------------

/// The mutation envelope: tenant from the caller's own document, the flat
/// mutation document inside.
fn envelope(tenant: &str, mutation: Value) -> String {
    json!({ "tenant": tenant, "mutation": mutation }).to_string()
}

/// `create_subscription` — a metered entitlement with an allocation.
fn create_body(tenant: &str, command_id: &str, plan_code: &str, allocation: i64) -> String {
    envelope(
        tenant,
        json!({
            "type": "create_subscription",
            "commandId": command_id,
            "contactId": CONTACT,
            "planCode": plan_code,
            "planName": "Team plan",
            "billingInterval": "monthly",
            "unitPrice": UNIT_PRICE,
            "currency": "IDR",
            "quantity": 1,
            "initialEntitlements": [{
                "featureCode": "api-calls",
                "featureName": "API calls",
                "entitlementType": "metered",
                "allocatedQuantity": allocation
            }]
        }),
    )
}

/// `record_payment` — the human-confirmed payment that activates.
fn pay_body(tenant: &str, command_id: &str, subscription_id: &str) -> String {
    envelope(
        tenant,
        json!({
            "type": "record_payment",
            "commandId": command_id,
            "subscriptionId": subscription_id,
            "paymentReference": "pay-0001",
            "amountPaid": UNIT_PRICE,
            "periodStart": "2026-01-01T00:00:00Z",
            "periodEnd": "2026-02-01T00:00:00Z"
        }),
    )
}

/// `consume_usage` — the metered consumption.
fn use_body(
    tenant: &str,
    command_id: &str,
    entitlement_id: &str,
    units: i64,
    idempotency_key: &str,
) -> String {
    envelope(
        tenant,
        json!({
            "type": "consume_usage",
            "commandId": command_id,
            "entitlementId": entitlement_id,
            "units": units,
            "idempotencyKey": idempotency_key,
            "evidenceRef": "call-77"
        }),
    )
}

/// `change_status` — the state move.
fn status_body(
    tenant: &str,
    command_id: &str,
    subscription_id: &str,
    expected_revision: i64,
    target: &str,
) -> String {
    envelope(
        tenant,
        json!({
            "type": "change_status",
            "commandId": command_id,
            "subscriptionId": subscription_id,
            "expectedRevision": expected_revision,
            "targetStatus": target,
            "reason": "operator moved the subscription"
        }),
    )
}

/// Submit an arm and assert it committed — the OK receipt's channel.
async fn receipt_of(app: &axum::Router, session: &Session, body: &str) -> Value {
    let (status, receipt) = post(app, "/erp/subscriptions/commands", body, session).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the arm must commit: {receipt} — body {body}"
    );
    receipt
}

/// Submit an arm and assert it refused with the given status class.
async fn refused_with(
    app: &axum::Router,
    session: &Session,
    body: &str,
    expected: StatusCode,
) -> Value {
    let (status, answer) = post(app, "/erp/subscriptions/commands", body, session).await;
    assert_eq!(
        status,
        expected,
        "the arm must refuse with {}: {answer} — body {body}",
        expected.as_u16()
    );
    answer
}

/// The entitlement of a subscription — read through the READ projection
/// (the create receipt names no entitlement, exactly as the TS's does).
async fn entitlement_of(app: &axum::Router, session: &Session, subscription_id: &str) -> Value {
    let (status, rows) = get(
        app,
        &format!("/erp/subscriptions/entitlements?tenant={ACME}&targetId={subscription_id}"),
        session,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the entitlement read must answer: {rows}"
    );
    rows.as_array()
        .expect("the entitlement read answers an array")
        .first()
        .cloned()
        .expect("the subscription owns an entitlement")
}

/// The row count of one table in the subscriptions schema.
async fn count(fixture: &Fixture, table: &str) -> i64 {
    // The table name is a literal from this suite's own list, never a
    // caller value — the crate's SQL-safety marker still rides it.
    let statement = format!("SELECT count(*)::bigint FROM archava_subscriptions.{table}");
    let (count,): (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(statement.as_str()))
        .fetch_one(&fixture.admin_pool)
        .await
        .expect("count the rows");
    count
}

/// Withdraw ONE grant from a seat that stays active — the closure A1-FU3
/// exercises: the membership is not deactivated, only the permission dies.
async fn withdraw_subscription_grant(fixture: &Fixture, principal: &str, grant: &str) {
    sqlx::query(
        "UPDATE archava_subscriptions.memberships SET permissions = array_remove(permissions, $3::text) \
WHERE tenant_id=$1 AND principal_id=$2 AND active",
    )
    .bind(ACME)
    .bind(principal)
    .bind(grant)
    .execute(&fixture.admin_pool)
    .await
    .expect("withdraw the grant");
}

/// Put one grant back after [`withdraw_subscription_grant`].
async fn restore_subscription_grant(fixture: &Fixture, principal: &str, grant: &str) {
    sqlx::query(
        "UPDATE archava_subscriptions.memberships SET permissions = array_append(permissions, $3::text) \
WHERE tenant_id=$1 AND principal_id=$2 AND active",
    )
    .bind(ACME)
    .bind(principal)
    .bind(grant)
    .execute(&fixture.admin_pool)
    .await
    .expect("restore the grant");
}

/// The seat's own liveness — a withdrawn grant must never touch it.
async fn seat_active_subscription(fixture: &Fixture, principal: &str) -> bool {
    sqlx::query_scalar(
        "SELECT active FROM archava_subscriptions.memberships \
WHERE tenant_id=$1 AND principal_id=$2",
    )
    .bind(ACME)
    .bind(principal)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the seat")
}

/// The subscription's committed revision.
async fn subscription_revision(fixture: &Fixture, subscription: &str) -> i32 {
    sqlx::query_scalar(
        "SELECT revision FROM archava_subscriptions.subscriptions \
WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(ACME)
    .bind(subscription)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the revision")
}

// --- The witnesses ------------------------------------------------------------

#[tokio::test]
async fn authorized_subscriptions_life_documents_its_entitlements() {
    let (fixture, app, sx, _sxg, _sxn, _sxv, _other) = served().await;

    // 1. create_subscription — the receipt names the subscription only.
    let created = receipt_of(
        &app,
        &sx,
        &create_body(ACME, KEY_CREATE, "saas-team", ALLOCATION),
    )
    .await;
    assert_eq!(created["commandId"].as_str(), Some(KEY_CREATE));
    assert_eq!(created["tenantId"].as_str(), Some(ACME));
    assert_eq!(created["action"].as_str(), Some("create_subscription"));
    assert_eq!(created["status"].as_str(), Some("draft"));
    assert_eq!(created["revision"].as_i64(), Some(1));
    assert_eq!(created["idempotencyReplay"].as_bool(), Some(false));
    assert!(created["executedAt"].as_str().is_some());
    let subscription_id = created["subscriptionId"]
        .as_str()
        .expect("the create names its subscription")
        .to_string();
    assert!(
        created["entitlementId"].is_null(),
        "the create arm names no entitlement"
    );

    // The READ projection answers the subscription itself.
    let (status, list) = get(
        &app,
        &format!("/erp/subscriptions/subscriptions?tenant={ACME}"),
        &sx,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the list read must answer: {list}");
    let rows = list.as_array().expect("the list read answers an array");
    assert_eq!(rows.len(), 1, "one subscription exists");
    assert_eq!(rows[0]["id"].as_str(), Some(subscription_id.as_str()));
    assert_eq!(rows[0]["status"].as_str(), Some("draft"));
    assert_eq!(rows[0]["unitPrice"].as_i64(), Some(UNIT_PRICE));
    assert_eq!(rows[0]["quantity"].as_i64(), Some(1));
    assert_eq!(rows[0]["currency"].as_str(), Some("IDR"));
    assert!(
        rows[0]["paymentReference"].is_null(),
        "an unpaid plan has no reference"
    );

    // The entitlement arrives with its own allocation.
    let entitlement = entitlement_of(&app, &sx, &subscription_id).await;
    assert_eq!(entitlement["featureCode"].as_str(), Some("api-calls"));
    assert_eq!(entitlement["entitlementType"].as_str(), Some("metered"));
    assert_eq!(entitlement["allocatedQuantity"].as_i64(), Some(ALLOCATION));
    assert_eq!(entitlement["consumedQuantity"].as_i64(), Some(0));
    assert_eq!(entitlement["status"].as_str(), Some("active"));
    let entitlement_id = entitlement["id"]
        .as_str()
        .expect("the entitlement has an id")
        .to_string();

    // 2. record_payment — the subscription activates.
    let paid = receipt_of(&app, &sx, &pay_body(ACME, KEY_PAY, &subscription_id)).await;
    assert_eq!(paid["action"].as_str(), Some("record_payment"));
    assert_eq!(paid["status"].as_str(), Some("active"));
    assert_eq!(paid["revision"].as_i64(), Some(2));

    // 3. consume_usage — the meter moves.
    let used = receipt_of(
        &app,
        &sx,
        &use_body(ACME, KEY_USE, &entitlement_id, 3, "use-0001"),
    )
    .await;
    assert_eq!(used["action"].as_str(), Some("consume_usage"));
    assert_eq!(used["status"].as_str(), Some("active"));
    assert_eq!(
        used["entitlementId"].as_str(),
        Some(entitlement_id.as_str())
    );
    assert_eq!(
        used["subscriptionId"].as_str(),
        Some(subscription_id.as_str())
    );
    assert!(
        used["usageEventId"].as_str().is_some(),
        "the meter names its event"
    );
    assert_eq!(used["idempotencyReplay"].as_bool(), Some(false));

    // The rows: one subscription (active, revision 2), one entitlement
    // (consumed 3), one usage event, two audit rows.
    let (status,): (String,) = sqlx::query_as(
        "SELECT status FROM archava_subscriptions.subscriptions WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&subscription_id))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the subscription row");
    assert_eq!(status, "active");
    let (revision, reference): (i64, Option<String>) = sqlx::query_as(
        "SELECT revision::bigint AS revision, payment_reference FROM archava_subscriptions.subscriptions \
WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&subscription_id))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the subscription row");
    assert_eq!(revision, 2, "the payment moved the revision");
    assert_eq!(reference.as_deref(), Some("pay-0001"));
    let (consumed, entitlement_status): (i64, String) = sqlx::query_as(
        "SELECT consumed_quantity::bigint AS consumed_quantity, status FROM archava_subscriptions.entitlements \
WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&entitlement_id))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the entitlement row");
    assert_eq!(consumed, 3, "the meter counted the consumption");
    assert_eq!(entitlement_status, "active");
    assert_eq!(count(&fixture, "usage_events").await, 1);
    assert_eq!(
        count(&fixture, "audit").await,
        3,
        "one audit row per command"
    );
    let (operations,): (String,) = sqlx::query_as(
        "SELECT string_agg(operation, ',' ORDER BY operation) FROM archava_subscriptions.audit",
    )
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the audit rows");
    assert_eq!(
        operations, "consume_usage,create_subscription,record_payment",
        "the audit names every operation once"
    );
}

#[tokio::test]
async fn an_unpaid_draft_grants_no_consumption_right() {
    let (fixture, app, sx, _sxg, _sxn, _sxv, _other) = served().await;

    // The audit's counterexample, verbatim: a PAID plan with an initial
    // metered entitlement and NO payment.
    let created = receipt_of(
        &app,
        &sx,
        &create_body(ACME, KEY_DRAFT_CREATE, "saas-team", ALLOCATION),
    )
    .await;
    let subscription_id = created["subscriptionId"]
        .as_str()
        .expect("the create names its subscription")
        .to_string();
    let entitlement = entitlement_of(&app, &sx, &subscription_id).await;
    let entitlement_id = entitlement["id"]
        .as_str()
        .expect("the entitlement has an id")
        .to_string();

    // The consumption is refused — the 402 class, with its own label.
    let refused = refused_with(
        &app,
        &sx,
        &use_body(ACME, KEY_DRAFT_USE, &entitlement_id, 1, "use-draft"),
        StatusCode::PAYMENT_REQUIRED,
    )
    .await;
    assert_eq!(
        refused["reason"].as_str(),
        Some("not_paid"),
        "the refusal names the unpaid state"
    );

    // Nothing moved: no usage event, no consumed counter, still draft.
    assert_eq!(
        count(&fixture, "usage_events").await,
        0,
        "no usage was recorded"
    );
    assert_eq!(count(&fixture, "audit").await, 1, "only the create audited");
    let (consumed,): (i64,) = sqlx::query_as(
        "SELECT consumed_quantity::bigint AS consumed_quantity FROM archava_subscriptions.entitlements WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&entitlement_id))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the entitlement row");
    assert_eq!(consumed, 0, "the meter did not move");
    let (status,): (String,) = sqlx::query_as(
        "SELECT status FROM archava_subscriptions.subscriptions WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&subscription_id))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the subscription row");
    assert_eq!(status, "draft", "the subscription stays unpaid");
}

#[tokio::test]
async fn the_quota_boundary_refuses_and_records_nothing() {
    let (fixture, app, sx, _sxg, _sxn, _sxv, _other) = served().await;

    let created = receipt_of(
        &app,
        &sx,
        &create_body(ACME, KEY_QUOTA_CREATE, "saas-team", 2),
    )
    .await;
    let subscription_id = created["subscriptionId"]
        .as_str()
        .expect("the create names its subscription")
        .to_string();
    let entitlement = entitlement_of(&app, &sx, &subscription_id).await;
    let entitlement_id = entitlement["id"]
        .as_str()
        .expect("the entitlement has an id")
        .to_string();
    receipt_of(&app, &sx, &pay_body(ACME, KEY_QUOTA_PAY, &subscription_id)).await;

    // Beyond the allocation — refused, and nothing is recorded.
    refused_with(
        &app,
        &sx,
        &use_body(ACME, KEY_QUOTA_OVER, &entitlement_id, 3, "use-over"),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(
        count(&fixture, "usage_events").await,
        0,
        "the refusal recorded nothing"
    );
    let (consumed,): (i64,) = sqlx::query_as(
        "SELECT consumed_quantity::bigint AS consumed_quantity FROM archava_subscriptions.entitlements WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&entitlement_id))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the entitlement row");
    assert_eq!(consumed, 0, "the meter did not move");

    // Exactly the allocation — accepted, and the entitlement exhausts.
    let filled = receipt_of(
        &app,
        &sx,
        &use_body(ACME, KEY_QUOTA_FULL, &entitlement_id, 2, "use-full"),
    )
    .await;
    assert_eq!(filled["status"].as_str(), Some("exhausted"));
    let (consumed, entitlement_status): (i64, String) = sqlx::query_as(
        "SELECT consumed_quantity::bigint AS consumed_quantity, status FROM archava_subscriptions.entitlements \
WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&entitlement_id))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the entitlement row");
    assert_eq!(consumed, 2);
    assert_eq!(entitlement_status, "exhausted");

    // One more unit — the exhausted entitlement refuses.
    refused_with(
        &app,
        &sx,
        &use_body(ACME, KEY_QUOTA_AFTER, &entitlement_id, 1, "use-after"),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(
        count(&fixture, "usage_events").await,
        1,
        "only the accepted use recorded"
    );
}

#[tokio::test]
async fn a_duplicate_usage_key_conflicts_and_records_nothing() {
    let (fixture, app, sx, _sxg, _sxn, _sxv, _other) = served().await;

    let created = receipt_of(
        &app,
        &sx,
        &create_body(ACME, KEY_DUP_CREATE, "saas-team", ALLOCATION),
    )
    .await;
    let subscription_id = created["subscriptionId"]
        .as_str()
        .expect("the create names its subscription")
        .to_string();
    let entitlement = entitlement_of(&app, &sx, &subscription_id).await;
    let entitlement_id = entitlement["id"]
        .as_str()
        .expect("the entitlement has an id")
        .to_string();
    receipt_of(&app, &sx, &pay_body(ACME, KEY_DUP_PAY, &subscription_id)).await;

    receipt_of(
        &app,
        &sx,
        &use_body(ACME, KEY_DUP_USE_1, &entitlement_id, 1, "use-same"),
    )
    .await;
    // The same idempotency key under a different command id — the
    // baseline lets the UNIQUE constraint raise (a database failure after
    // the counter already moved); the port refuses with the conflict class.
    refused_with(
        &app,
        &sx,
        &use_body(ACME, KEY_DUP_USE_2, &entitlement_id, 1, "use-same"),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(
        count(&fixture, "usage_events").await,
        1,
        "the meter counted once"
    );
    let (consumed,): (i64,) = sqlx::query_as(
        "SELECT consumed_quantity::bigint AS consumed_quantity FROM archava_subscriptions.entitlements WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&entitlement_id))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the entitlement row");
    assert_eq!(consumed, 1, "the refused duplicate moved nothing");
}

#[tokio::test]
async fn a_past_due_subscription_refuses_usage() {
    let (fixture, app, sx, _sxg, _sxn, _sxv, _other) = served().await;

    let created = receipt_of(
        &app,
        &sx,
        &create_body(ACME, KEY_PASTDUE_CREATE, "saas-team", ALLOCATION),
    )
    .await;
    let subscription_id = created["subscriptionId"]
        .as_str()
        .expect("the create names its subscription")
        .to_string();
    let entitlement = entitlement_of(&app, &sx, &subscription_id).await;
    let entitlement_id = entitlement["id"]
        .as_str()
        .expect("the entitlement has an id")
        .to_string();
    receipt_of(
        &app,
        &sx,
        &pay_body(ACME, KEY_PASTDUE_PAY, &subscription_id),
    )
    .await;
    receipt_of(
        &app,
        &sx,
        &status_body(ACME, KEY_PASTDUE_STATUS, &subscription_id, 2, "past_due"),
    )
    .await;

    // The TS's own 402 class.
    let refused = refused_with(
        &app,
        &sx,
        &use_body(ACME, KEY_PASTDUE_USE, &entitlement_id, 1, "use-past-due"),
        StatusCode::PAYMENT_REQUIRED,
    )
    .await;
    assert_eq!(refused["reason"].as_str(), Some("past_due"));
    assert_eq!(
        count(&fixture, "usage_events").await,
        0,
        "nothing was metered"
    );
}

#[tokio::test]
async fn activation_without_a_payment_record_refuses() {
    let (fixture, app, sx, _sxg, _sxn, _sxv, _other) = served().await;

    let created = receipt_of(
        &app,
        &sx,
        &create_body(ACME, KEY_ACTIVATE_CREATE, "saas-team", ALLOCATION),
    )
    .await;
    let subscription_id = created["subscriptionId"]
        .as_str()
        .expect("the create names its subscription")
        .to_string();

    // A draft cannot be pushed to `active` without a payment record.
    let refused = refused_with(
        &app,
        &sx,
        &status_body(ACME, KEY_ACTIVATE_TRY, &subscription_id, 1, "active"),
        StatusCode::PAYMENT_REQUIRED,
    )
    .await;
    assert_eq!(refused["reason"].as_str(), Some("not_paid"));
    let (status,): (String,) = sqlx::query_as(
        "SELECT status FROM archava_subscriptions.subscriptions WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&subscription_id))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the subscription row");
    assert_eq!(status, "draft", "the refusal moved nothing");

    // A draft may still be canceled — and canceled is terminal.
    receipt_of(
        &app,
        &sx,
        &status_body(ACME, KEY_ACTIVATE_CANCEL, &subscription_id, 1, "canceled"),
    )
    .await;
    refused_with(
        &app,
        &sx,
        &status_body(ACME, KEY_ACTIVATE_RESUME, &subscription_id, 2, "active"),
        StatusCode::CONFLICT,
    )
    .await;
    let (status,): (String,) = sqlx::query_as(
        "SELECT status FROM archava_subscriptions.subscriptions WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&subscription_id))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the subscription row");
    assert_eq!(status, "canceled", "a canceled subscription stays canceled");
}

#[tokio::test]
async fn a_revoked_entitlement_refuses_usage() {
    let (fixture, app, sx, _sxg, _sxn, _sxv, _other) = served().await;

    let created = receipt_of(
        &app,
        &sx,
        &create_body(ACME, KEY_REVOKE_CREATE, "saas-team", ALLOCATION),
    )
    .await;
    let subscription_id = created["subscriptionId"]
        .as_str()
        .expect("the create names its subscription")
        .to_string();
    let entitlement = entitlement_of(&app, &sx, &subscription_id).await;
    let entitlement_id = entitlement["id"]
        .as_str()
        .expect("the entitlement has an id")
        .to_string();
    receipt_of(&app, &sx, &pay_body(ACME, KEY_REVOKE_PAY, &subscription_id)).await;
    receipt_of(
        &app,
        &sx,
        &status_body(ACME, KEY_REVOKE_CANCEL, &subscription_id, 2, "canceled"),
    )
    .await;

    // Canceling the subscription revokes its entitlements — the refusal is
    // the TS's own `forbidden` (403).
    let (entitlement_status,): (String,) = sqlx::query_as(
        "SELECT status FROM archava_subscriptions.entitlements WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&entitlement_id))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the entitlement row");
    assert_eq!(
        entitlement_status, "revoked",
        "the cancel revoked the entitlement"
    );
    refused_with(
        &app,
        &sx,
        &use_body(ACME, KEY_REVOKE_USE, &entitlement_id, 1, "use-revoked"),
        StatusCode::FORBIDDEN,
    )
    .await;
    assert_eq!(
        count(&fixture, "usage_events").await,
        0,
        "nothing was metered"
    );
}

/// A1-FU3 — the per-command grant gates the CLAIM, not just the arm. A seat
/// that stays active while one grant is withdrawn must not recover that arm's
/// stored receipt; the other arms keep replaying their own.
#[tokio::test]
async fn revoked_subscription_grants_refuse_the_stored_writes() {
    let (fixture, app, sx, _sxg, _sxn, _sxv, _other) = served().await;

    // (a) All three protected arms commit under the full grants.
    let create = create_body(ACME, KEY_SEALED_CREATE, "saas-team", ALLOCATION);
    let created = receipt_of(&app, &sx, &create).await;
    assert_eq!(created["idempotencyReplay"].as_bool(), Some(false));
    let subscription_id = created["subscriptionId"]
        .as_str()
        .expect("the create names its subscription")
        .to_string();
    let entitlement_id = entitlement_of(&app, &sx, &subscription_id).await["id"]
        .as_str()
        .expect("the entitlement has an id")
        .to_string();
    let pay = pay_body(ACME, KEY_SEALED_PAY, &subscription_id);
    let paid = receipt_of(&app, &sx, &pay).await;
    assert_eq!(paid["idempotencyReplay"].as_bool(), Some(false));
    let revision = subscription_revision(&fixture, &subscription_id).await;
    let usage = use_body(ACME, KEY_SEALED_USE, &entitlement_id, 2, "use-sealed");
    let used = receipt_of(&app, &sx, &usage).await;
    assert_eq!(used["idempotencyReplay"].as_bool(), Some(false));
    assert_eq!(count(&fixture, "commands").await, 3);
    assert_eq!(count(&fixture, "usage_events").await, 1);
    assert_eq!(
        entitlement_of(&app, &sx, &subscription_id).await["consumedQuantity"].as_i64(),
        Some(2)
    );

    // (b) `subscriptions.usage` dies while the seat stays ACTIVE.
    withdraw_subscription_grant(&fixture, "sx", "subscriptions.usage").await;
    assert!(
        seat_active_subscription(&fixture, "sx").await,
        "withdrawing one grant must not deactivate the seat"
    );

    // (c) The byte-identical usage replay is refused before the claim: no
    // receipt, no metered row, no command bookkeeping.
    let (status, answer) = post(&app, "/erp/subscriptions/commands", &usage, &sx).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{answer}");
    assert_eq!(answer["reason"].as_str(), Some("forbidden"));
    assert!(
        answer.get("entitlementId").is_none(),
        "no receipt: {answer}"
    );
    assert!(
        answer.get("idempotencyReplay").is_none(),
        "no replay: {answer}"
    );
    assert_eq!(count(&fixture, "usage_events").await, 1, "nothing metered");
    assert_eq!(count(&fixture, "commands").await, 3, "the claim never ran");
    // The write arm still answers its own stored receipt.
    let replay = receipt_of(&app, &sx, &create).await;
    assert_eq!(replay["idempotencyReplay"].as_bool(), Some(true));
    restore_subscription_grant(&fixture, "sx", "subscriptions.usage").await;
    let replay = receipt_of(&app, &sx, &usage).await;
    assert_eq!(replay["idempotencyReplay"].as_bool(), Some(true));
    assert_eq!(count(&fixture, "usage_events").await, 1);

    // (d) `subscriptions.payment`: the pay replay is refused and the revision
    // stays where the committed payment left it.
    withdraw_subscription_grant(&fixture, "sx", "subscriptions.payment").await;
    let (status, answer) = post(&app, "/erp/subscriptions/commands", &pay, &sx).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{answer}");
    assert_eq!(answer["reason"].as_str(), Some("forbidden"));
    assert!(
        answer.get("subscriptionId").is_none(),
        "no receipt: {answer}"
    );
    assert_eq!(
        subscription_revision(&fixture, &subscription_id).await,
        revision
    );
    assert_eq!(count(&fixture, "commands").await, 3);
    restore_subscription_grant(&fixture, "sx", "subscriptions.payment").await;
    let replay = receipt_of(&app, &sx, &pay).await;
    assert_eq!(replay["idempotencyReplay"].as_bool(), Some(true));
    assert_eq!(replay["revision"].as_i64(), Some(i64::from(revision)));

    // (e) `subscriptions.write`: the create replay is refused, no second row.
    withdraw_subscription_grant(&fixture, "sx", "subscriptions.write").await;
    let (status, answer) = post(&app, "/erp/subscriptions/commands", &create, &sx).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{answer}");
    assert_eq!(answer["reason"].as_str(), Some("forbidden"));
    assert!(
        answer.get("subscriptionId").is_none(),
        "no receipt: {answer}"
    );
    assert!(
        answer.get("idempotencyReplay").is_none(),
        "no replay: {answer}"
    );
    assert_eq!(count(&fixture, "subscriptions").await, 1);
    assert_eq!(count(&fixture, "commands").await, 3);
    restore_subscription_grant(&fixture, "sx", "subscriptions.write").await;
    let replay = receipt_of(&app, &sx, &create).await;
    assert_eq!(replay["idempotencyReplay"].as_bool(), Some(true));
    assert_eq!(
        replay["subscriptionId"].as_str(),
        Some(subscription_id.as_str())
    );
    assert_eq!(count(&fixture, "subscriptions").await, 1);
}

#[tokio::test]
async fn the_stale_revision_conflicts_and_records_nothing() {
    let (fixture, app, sx, _sxg, _sxn, _sxv, _other) = served().await;

    let created = receipt_of(
        &app,
        &sx,
        &create_body(ACME, KEY_STALE_CREATE, "saas-team", ALLOCATION),
    )
    .await;
    let subscription_id = created["subscriptionId"]
        .as_str()
        .expect("the create names its subscription")
        .to_string();
    receipt_of(&app, &sx, &pay_body(ACME, KEY_STALE_PAY, &subscription_id)).await;

    // The revision is 2 now; the stale document says 1.
    refused_with(
        &app,
        &sx,
        &status_body(ACME, KEY_STALE_STATUS, &subscription_id, 1, "canceled"),
        StatusCode::CONFLICT,
    )
    .await;
    let (revision,): (i64,) = sqlx::query_as(
        "SELECT revision::bigint AS revision FROM archava_subscriptions.subscriptions WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&subscription_id))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the subscription row");
    assert_eq!(revision, 2, "the stale refusal moved nothing");
}

#[tokio::test]
async fn the_same_key_a_different_command_conflicts() {
    let (fixture, app, sx, _sxg, _sxn, _sxv, _other) = served().await;

    receipt_of(
        &app,
        &sx,
        &create_body(ACME, KEY_REPLAY_CREATE, "saas-team", ALLOCATION),
    )
    .await;
    // The same command id with a DIFFERENT payload — the fingerprint
    // mismatch refuses.
    refused_with(
        &app,
        &sx,
        &create_body(ACME, KEY_REPLAY_CREATE, "other-plan", ALLOCATION),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(
        count(&fixture, "subscriptions").await,
        1,
        "the conflicting payload recorded nothing"
    );
}

#[tokio::test]
async fn lost_response_replays_the_recorded_receipt() {
    let (fixture, app, sx, _sxg, _sxn, _sxv, _other) = served().await;

    let first = receipt_of(
        &app,
        &sx,
        &create_body(ACME, KEY_REPLAY_CREATE, "saas-team", ALLOCATION),
    )
    .await;
    // The very same document again — the stored receipt replays.
    let replay = receipt_of(
        &app,
        &sx,
        &create_body(ACME, KEY_REPLAY_CREATE, "saas-team", ALLOCATION),
    )
    .await;
    assert_eq!(
        replay["idempotencyReplay"].as_bool(),
        Some(true),
        "the replay marks itself"
    );
    assert_eq!(replay["subscriptionId"], first["subscriptionId"]);
    assert_eq!(replay["executedAt"], first["executedAt"]);
    assert_eq!(
        count(&fixture, "subscriptions").await,
        1,
        "one subscription, one command row"
    );

    // The durable status read answers the TS commandStatus document.
    let (status, document) = get(
        &app,
        &format!("/erp/subscriptions/commands/status?tenant={ACME}&requestId={KEY_REPLAY_CREATE}"),
        &sx,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the status read must answer: {document}"
    );
    assert_eq!(document["status"].as_str(), Some("recorded"));
    assert_eq!(
        document["receipt"]["commandId"].as_str(),
        Some(KEY_REPLAY_CREATE)
    );
}

#[tokio::test]
async fn cross_tenant_callers_refuse_before_any_write() {
    let (fixture, app, sx, _sxg, _sxn, _sxv, other) = served().await;

    // `other` holds full grants — but ONLY in GLOBEX.
    refused_with(
        &app,
        &other,
        &create_body(
            ACME,
            "6b240300-9999-4999-9999-000000000601",
            "saas-team",
            ALLOCATION,
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
    assert_eq!(
        count(&fixture, "subscriptions").await,
        0,
        "no cross-tenant row"
    );
    assert_eq!(count(&fixture, "audit").await, 0, "no cross-tenant audit");

    // In its own ledger the same caller acts freely.
    let created = receipt_of(
        &app,
        &other,
        &create_body(
            GLOBEX,
            "6b240300-9999-4999-9999-000000000602",
            "saas-team",
            ALLOCATION,
        ),
    )
    .await;
    assert!(created["subscriptionId"].as_str().is_some());
    let _ = sx;
}

#[tokio::test]
async fn narrow_grants_refuse_exactly_the_arms_they_lack() {
    let (fixture, app, sx, sxg, sxn, sxv, _other) = served().await;

    // `sxg` holds read + audit: the four write arms refuse.
    refused_with(
        &app,
        &sxg,
        &create_body(
            ACME,
            "6b240300-9999-4999-9999-000000000701",
            "saas-team",
            ALLOCATION,
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
    refused_with(
        &app,
        &sxn,
        &create_body(
            ACME,
            "6b240300-9999-4999-9999-000000000702",
            "saas-team",
            ALLOCATION,
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
    assert_eq!(
        count(&fixture, "subscriptions").await,
        0,
        "no row from a narrow grant"
    );

    // The reads walk for the read holder.
    let created = receipt_of(
        &app,
        &sx,
        &create_body(ACME, KEY_GRANT, "saas-team", ALLOCATION),
    )
    .await;
    let subscription_id = created["subscriptionId"]
        .as_str()
        .expect("the create names its subscription")
        .to_string();
    // The status read binds `subscriptions.read` to the recorded initiator:
    // the read holder sees `unknown` for the manager's command — no receipt
    // fields — while the manager recovers its own.
    let (status, foreign) = get(
        &app,
        &format!("/erp/subscriptions/commands/status?tenant={ACME}&requestId={KEY_GRANT}"),
        &sxg,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(foreign["status"].as_str(), Some("unknown"));
    assert!(foreign.get("receipt").is_none(), "no receipt fields");
    let (status, own) = get(
        &app,
        &format!("/erp/subscriptions/commands/status?tenant={ACME}&requestId={KEY_GRANT}"),
        &sx,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(own["status"].as_str(), Some("recorded"));

    let (status, list) = get(
        &app,
        &format!("/erp/subscriptions/subscriptions?tenant={ACME}"),
        &sxg,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the read holder may read: {list}");
    assert_eq!(list.as_array().map(|rows| rows.len()), Some(1));

    // An ACTIVE membership whose grants carry only `audit.read` is refused at the
    // status route: recovery is gated on the CURRENT read grant, not the seat.
    let (status, body) = get(
        &app,
        &format!("/erp/subscriptions/commands/status?tenant={ACME}&requestId={KEY_GRANT}"),
        &sxn,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "an audit-only member may not read a command: {body}"
    );

    // Revoking the read grant while the membership stays ACTIVE closes the same
    // route for the read holder that just used it.
    sqlx::query(
        "UPDATE archava_subscriptions.memberships SET permissions=$2 \
         WHERE tenant_id=$1 AND principal_id='sxg'",
    )
    .bind(ACME)
    .bind(vec!["audit.read".to_string()])
    .execute(&fixture.admin_pool)
    .await
    .expect("revoke the subscriptions read grant");
    let (status, body) = get(
        &app,
        &format!("/erp/subscriptions/commands/status?tenant={ACME}&requestId={KEY_GRANT}"),
        &sxg,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a revoked read grant refuses recovery: {body}"
    );

    // A revoked membership refuses — and its replay refuses too.
    sqlx::query(
        "UPDATE archava_subscriptions.memberships SET active=false \
WHERE tenant_id=$1 AND principal_id=$2",
    )
    .bind(ACME)
    .bind("sxv")
    .execute(&fixture.admin_pool)
    .await
    .expect("revoke the membership");
    refused_with(
        &app,
        &sxv,
        &create_body(
            ACME,
            "6b240300-9999-4999-9999-000000000703",
            "saas-team",
            ALLOCATION,
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
    let _ = subscription_id;
}

#[tokio::test]
async fn the_wire_boundary_and_the_read_projections_hold() {
    let (fixture, app, sx, _sxg, _sxn, _sxv, _other) = served().await;

    // The grammar refuses at the boundary: unknown arm, extra member,
    // lowercase currency, zero units.
    let unknown_arm = envelope(
        ACME,
        json!({ "type": "not_a_command", "commandId": KEY_CREATE }),
    );
    refused_with(&app, &sx, &unknown_arm, StatusCode::BAD_REQUEST).await;
    let extra_member = envelope(
        ACME,
        json!({
            "type": "consume_usage",
            "commandId": KEY_CREATE,
            "entitlementId": CONTACT,
            "units": 1,
            "idempotencyKey": "k",
            "status": "active"
        }),
    );
    refused_with(&app, &sx, &extra_member, StatusCode::BAD_REQUEST).await;
    let lowercase_currency = envelope(
        ACME,
        json!({
            "type": "create_subscription",
            "commandId": KEY_CREATE,
            "contactId": CONTACT,
            "planCode": "saas-team",
            "planName": "Team plan",
            "billingInterval": "monthly",
            "unitPrice": 100000,
            "currency": "idr"
        }),
    );
    refused_with(&app, &sx, &lowercase_currency, StatusCode::BAD_REQUEST).await;
    let zero_units = envelope(
        ACME,
        json!({
            "type": "consume_usage",
            "commandId": KEY_CREATE,
            "entitlementId": CONTACT,
            "units": 0,
            "idempotencyKey": "k"
        }),
    );
    refused_with(&app, &sx, &zero_units, StatusCode::BAD_REQUEST).await;

    // The read projections are bounded: an out-of-range limit and an
    // unknown status filter refuse at the boundary.
    let (status, _) = get(
        &app,
        &format!("/erp/subscriptions/subscriptions?tenant={ACME}&limit=0"),
        &sx,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "the limit bound holds");
    let (status, _) = get(
        &app,
        &format!("/erp/subscriptions/subscriptions?tenant={ACME}&limit=101"),
        &sx,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "the limit ceiling holds");
    let (status, _) = get(
        &app,
        &format!("/erp/subscriptions/subscriptions?tenant={ACME}&status=nonsense"),
        &sx,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "the status filter holds");

    // An unknown subscription reads as `null` — absence is unconfirmed,
    // never a fabricated empty page.
    let (status, detail) = get(
        &app,
        &format!("/erp/subscriptions/subscriptions/detail?tenant={ACME}&id={CONTACT}"),
        &sx,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the detail read must answer: {detail}"
    );
    assert!(detail.is_null(), "an unknown subscription reads as null");
    assert_eq!(
        count(&fixture, "subscriptions").await,
        0,
        "the refusals recorded nothing"
    );
}

// --- The owner-scoped entitlement reads (the isUuid gate's removal) ----------

/// A principal whose ACME membership carries the whole ledger but is scoped to
/// `assigned`: it may act, and it may read only what it owns.
async fn assigned_principal(fixture: &Fixture, principal: &str) {
    sqlx::query(
        "INSERT INTO archava_identity.users (id, username, password_hash, active, version) \
         VALUES ($1, $2, $3, true, 1)",
    )
    .bind(principal)
    .bind(format!("{}@archava.test", principal))
    .bind(PASSWORD_HASH)
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the assigned identity user");
    sqlx::query(
        "INSERT INTO archava_subscriptions.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'assigned', true)",
    )
    .bind(ACME)
    .bind(principal)
    .bind(SUBSCRIPTION_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the assigned ACME membership");
}

/// The read contract of the two entitlement reads, proven at the wire.
///
/// - The target kind is the ROUTE. `entitlements.contact_id` is uuid-shaped in
///   the TS schema, so the old `valid_uuid` inference sent every contact read
///   into the subscription branch and answered `not_found`; the contact read
///   now answers rows for that same uuid on its own route, and the
///   subscription route still calls it missing.
/// - `assigned` scope binds BOTH reads to the owner: the contact read hides a
///   foreign entitlement (`EXISTS` on the parent), and the subscription read's
///   parent probe refuses a foreign subscription exactly like a missing one.
#[tokio::test]
async fn assigned_scope_binds_both_entitlement_reads_to_the_owner() {
    let (fixture, app, sx, _sxg, _sxn, _sxv, _other) = served().await;
    assigned_principal(&fixture, "sxa").await;
    let sxa = login(&app, "sxa").await;

    // The all-scope manager's subscription and the assigned caller's own, for
    // the SAME contact — which is what makes the contact read a place where
    // two owners meet.
    let created = receipt_of(
        &app,
        &sx,
        &create_body(ACME, KEY_CREATE, "saas-team", ALLOCATION),
    )
    .await;
    let manager_sub = created["subscriptionId"]
        .as_str()
        .expect("the create names its subscription")
        .to_string();
    let created = receipt_of(
        &app,
        &sxa,
        &create_body(
            ACME,
            "6b240300-9999-4999-9999-000000000601",
            "saas-owner",
            7,
        ),
    )
    .await;
    let own_sub = created["subscriptionId"]
        .as_str()
        .expect("the assigned create names its subscription")
        .to_string();
    assert_ne!(manager_sub, own_sub, "two subscriptions, two owners");

    // The assigned caller reads its own subscription.
    let (status, rows) = get(
        &app,
        &format!("/erp/subscriptions/entitlements?tenant={ACME}&targetId={own_sub}"),
        &sxa,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the owner reads its own subscription: {rows}"
    );
    assert_eq!(rows.as_array().map(|rows| rows.len()), Some(1));

    // … and the contact: its OWN entitlement only. The manager's row hangs off
    // the same contact, so a missing owner predicate leaks it here.
    let (status, rows) = get(
        &app,
        &format!("/erp/subscriptions/contact-entitlements?tenant={ACME}&contactId={CONTACT}"),
        &sxa,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the contact read must answer: {rows}"
    );
    let rows = rows.as_array().expect("the contact read answers an array");
    assert_eq!(
        rows.len(),
        1,
        "the assigned caller sees its own contact entitlement only: {rows:?}"
    );
    assert_eq!(rows[0]["subscriptionId"].as_str(), Some(own_sub.as_str()));

    // A foreign subscription answers exactly like a missing one — the probe's
    // owner predicate, not a filter after the read.
    let (status, body) = get(
        &app,
        &format!("/erp/subscriptions/entitlements?tenant={ACME}&targetId={manager_sub}"),
        &sxa,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a foreign subscription reads as missing: {body}"
    );

    // Scope `all` sees the whole contact — both entitlements.
    let (status, rows) = get(
        &app,
        &format!("/erp/subscriptions/contact-entitlements?tenant={ACME}&contactId={CONTACT}"),
        &sx,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the manager's contact read must answer: {rows}"
    );
    assert_eq!(
        rows.as_array().map(|rows| rows.len()),
        Some(2),
        "all scope sees the contact's rows: {rows}"
    );

    // The subscription route stays subscription-keyed: the uuid-shaped contact
    // reference is not a subscription, so it reads as missing there. The
    // contact route is the only contact read.
    let (status, body) = get(
        &app,
        &format!("/erp/subscriptions/entitlements?tenant={ACME}&targetId={CONTACT}"),
        &sx,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the subscription route does not fall through to the contact read: {body}"
    );
}

/// The contact read keeps the T02 ownership shape — `EXISTS` on the
/// tenant-scoped parent, never an inner join — and the contact ordering.
///
/// An entitlement carries no owner of its own, so a row whose
/// `subscription_id` is NULL inherits none: `all` scope keeps it (no predicate
/// is added at all there), `assigned` hides it, and neither scope loses it to
/// a join. The older row must also sort last (`created_at DESC`).
#[tokio::test]
async fn contact_reads_keep_subscription_less_rows_for_all_scope_only() {
    let (fixture, app, sx, _sxg, _sxn, _sxv, _other) = served().await;
    assigned_principal(&fixture, "sxa").await;
    let sxa = login(&app, "sxa").await;

    let created = receipt_of(
        &app,
        &sx,
        &create_body(ACME, KEY_CREATE, "saas-team", ALLOCATION),
    )
    .await;
    let subscription = created["subscriptionId"]
        .as_str()
        .expect("the create names its subscription")
        .to_string();

    // Two contact entitlements with no subscription at all, on explicit
    // instants so the ordering is a fact of the data, not of the clock.
    sqlx::query(
        "INSERT INTO archava_subscriptions.entitlements \
         (tenant_id, id, subscription_id, contact_id, feature_code, feature_name, \
          entitlement_type, allocated_quantity, consumed_quantity, status, \
          valid_from, created_at, updated_at) VALUES \
         ($1, $2, NULL, $3, 'legacy-pass', 'Legacy pass', 'boolean_access', NULL, 0, 'active', \
          clock_timestamp() - interval '3 days', clock_timestamp() - interval '3 days', \
          clock_timestamp() - interval '3 days'), \
         ($1, $4, NULL, $3, 'legacy-boost', 'Legacy boost', 'boolean_access', NULL, 0, 'active', \
          clock_timestamp() - interval '1 day', clock_timestamp() - interval '1 day', \
          clock_timestamp() - interval '1 day')",
    )
    .bind(ACME)
    .bind(uuid_of("6b240300-9999-4999-9999-00000000d001"))
    .bind(CONTACT)
    .bind(uuid_of("6b240300-9999-4999-9999-00000000d002"))
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the subscription-less entitlements");

    let (status, rows) = get(
        &app,
        &format!("/erp/subscriptions/contact-entitlements?tenant={ACME}&contactId={CONTACT}"),
        &sx,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the contact read must answer: {rows}"
    );
    let rows = rows.as_array().expect("the contact read answers an array");
    assert_eq!(
        rows.len(),
        3,
        "all scope keeps the subscription-less rows (EXISTS, not a join): {rows:?}"
    );
    let features = rows
        .iter()
        .filter_map(|row| row["featureCode"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        features,
        vec!["api-calls", "legacy-boost", "legacy-pass"],
        "the contact read stays newest-first: {features:?}"
    );

    // The assigned caller owns nothing on this contact: the manager's
    // subscription is foreign and the subscription-less rows inherit no owner.
    let (status, rows) = get(
        &app,
        &format!("/erp/subscriptions/contact-entitlements?tenant={ACME}&contactId={CONTACT}"),
        &sxa,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the assigned contact read must answer: {rows}"
    );
    assert_eq!(
        rows.as_array().map(|rows| rows.len()),
        Some(0),
        "an assigned caller inherits nothing from an owner-less row: {rows}"
    );

    // The subscription read did not move either: its own row, oldest-first.
    let (status, rows) = get(
        &app,
        &format!("/erp/subscriptions/entitlements?tenant={ACME}&targetId={subscription}"),
        &sx,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the subscription read answers: {rows}"
    );
    assert_eq!(rows.as_array().map(|rows| rows.len()), Some(1));
}

/// A `targetId` that cannot name a subscription is a missing resource, never a
/// panic: the parameter is an unrestricted query string, and the read parses it
/// fallibly instead of asserting inside the driver. The answer must be the same
/// structured refusal a foreign subscription gets, so the route stays silent
/// about which ids exist.
#[tokio::test]
async fn malformed_target_ids_answer_not_found_instead_of_panicking() {
    let (_fixture, app, sx, _sxg, _sxn, _sxv, _other) = served().await;
    for target in ["not-a-uuid", "", "0000", "%20"] {
        let (status, body) = get(
            &app,
            &format!("/erp/subscriptions/entitlements?tenant={ACME}&targetId={target}"),
            &sx,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "targetId={target:?} -> {body}"
        );
        assert_eq!(
            body["reason"], "unknown_resource",
            "targetId={target:?} -> {body}"
        );
    }
}

/// The usage history is the third read on this surface, and `assigned` binds it
/// to the owner the way the contact read is bound: through the entitlement,
/// whose owner lives on its subscription. The hop sits inside `EXISTS`, so a
/// foreign entitlement answers `[]` — indistinguishable from a missing one —
/// and a usage event on a subscription-less entitlement stays visible to `all`
/// only, because it has no owner to inherit.
#[tokio::test]
async fn assigned_scope_binds_the_usage_history_to_the_entitlement_owner() {
    const MANAGER_USAGE: &str = "6b240300-9999-4999-9999-000000000801";
    const OWN_USAGE: &str = "6b240300-9999-4999-9999-000000000802";
    const LEGACY_USAGE: &str = "6b240300-9999-4999-9999-000000000803";
    const LEGACY_ENTITLEMENT: &str = "6b240300-9999-4999-9999-0000000008e0";

    let (fixture, app, sx, _sxg, _sxn, _sxv, _other) = served().await;
    assigned_principal(&fixture, "sxa").await;
    let sxa = login(&app, "sxa").await;

    let created = receipt_of(
        &app,
        &sx,
        &create_body(ACME, KEY_CREATE, "saas-team", ALLOCATION),
    )
    .await;
    let manager_sub = created["subscriptionId"]
        .as_str()
        .expect("the manager's subscription id")
        .to_string();
    let created = receipt_of(
        &app,
        &sxa,
        &create_body(
            ACME,
            "6b240300-9999-4999-9999-000000000702",
            "saas-owner",
            7,
        ),
    )
    .await;
    let own_sub = created["subscriptionId"]
        .as_str()
        .expect("the assigned caller's subscription id")
        .to_string();

    let manager_entitlement = entitlement_of(&app, &sx, &manager_sub).await["id"]
        .as_str()
        .expect("the manager's entitlement id")
        .to_string();
    let own_entitlement = entitlement_of(&app, &sxa, &own_sub).await["id"]
        .as_str()
        .expect("the assigned caller's entitlement id")
        .to_string();

    sqlx::query(
        "INSERT INTO archava_subscriptions.entitlements \
         (tenant_id, id, subscription_id, contact_id, feature_code, feature_name, \
          entitlement_type, allocated_quantity, consumed_quantity, status, valid_from) \
         VALUES ($1, $2, NULL, $3, 'legacy-pass', 'Legacy pass', 'boolean_access', NULL, 0, 'active', clock_timestamp())",
    )
    .bind(ACME)
    .bind(uuid_of(LEGACY_ENTITLEMENT))
    .bind(CONTACT)
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the subscription-less entitlement");

    for (id, entitlement, subscription, key) in [
        (
            MANAGER_USAGE,
            manager_entitlement.as_str(),
            Some(manager_sub.as_str()),
            "usage-manager",
        ),
        (
            OWN_USAGE,
            own_entitlement.as_str(),
            Some(own_sub.as_str()),
            "usage-own",
        ),
        (LEGACY_USAGE, LEGACY_ENTITLEMENT, None, "usage-legacy"),
    ] {
        sqlx::query(
            "INSERT INTO archava_subscriptions.usage_events \
             (tenant_id, id, entitlement_id, subscription_id, contact_id, units_consumed, \
              idempotency_key, recorded_by) \
             VALUES ($1, $2, $3, $4, $5, 1, $6, 'witness')",
        )
        .bind(ACME)
        .bind(uuid_of(id))
        .bind(uuid_of(entitlement))
        .bind(subscription.map(uuid_of))
        .bind(CONTACT)
        .bind(key)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the usage event");
    }

    let (status, rows) = get(
        &app,
        &format!("/erp/subscriptions/usage-events?tenant={ACME}&entitlementId={own_entitlement}"),
        &sxa,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the owner reads its own usage: {rows}"
    );
    assert_eq!(rows.as_array().map(|rows| rows.len()), Some(1));
    assert_eq!(rows[0]["id"].as_str(), Some(OWN_USAGE));

    let (status, rows) = get(
        &app,
        &format!(
            "/erp/subscriptions/usage-events?tenant={ACME}&entitlementId={manager_entitlement}"
        ),
        &sxa,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "a foreign entitlement: {rows}");
    assert_eq!(
        rows.as_array().map(|rows| rows.len()),
        Some(0),
        "foreign == missing: {rows}"
    );

    let (status, rows) = get(
        &app,
        &format!(
            "/erp/subscriptions/usage-events?tenant={ACME}&entitlementId=6b240300-9999-4999-9999-000000000fff"
        ),
        &sxa,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "a missing entitlement: {rows}");
    assert_eq!(rows.as_array().map(|rows| rows.len()), Some(0));

    let (status, rows) = get(
        &app,
        &format!(
            "/erp/subscriptions/usage-events?tenant={ACME}&entitlementId={LEGACY_ENTITLEMENT}"
        ),
        &sx,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "all scope, owner-less: {rows}");
    assert_eq!(
        rows.as_array().map(|rows| rows.len()),
        Some(1),
        "all scope keeps history whose entitlement has no subscription: {rows}"
    );

    let (status, rows) = get(
        &app,
        &format!(
            "/erp/subscriptions/usage-events?tenant={ACME}&entitlementId={LEGACY_ENTITLEMENT}"
        ),
        &sxa,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "assigned scope, owner-less: {rows}");
    assert_eq!(
        rows.as_array().map(|rows| rows.len()),
        Some(0),
        "assigned inherits no owner from a subscription-less entitlement: {rows}"
    );
}

// --- The replay is bound to the recorded initiator --------------------------------------------

const KEY_FOREIGN_REPLAY: &str = "6b240300-9999-4999-9999-000000000c01";

/// A rival holding the SAME `subscriptions.write` grant never collects
/// another actor's receipt: the claim compares the recorded `actor_id`
/// beside the fingerprint, so the refusal is the same CONFLICT class and
/// records nothing — while the initiating actor replays its own receipt
/// (A1 — `actor_id=$3`).
#[tokio::test]
async fn a_same_grant_rival_never_replays_another_actors_subscription_receipt() {
    let (fixture, app, sx, _sxg, _sxn, sxv, _other) = served().await;

    let body = create_body(ACME, KEY_FOREIGN_REPLAY, "saas-team", ALLOCATION);
    let first = receipt_of(&app, &sx, &body).await;
    assert_eq!(first["idempotencyReplay"].as_bool(), Some(false));
    let subscription_id = first["subscriptionId"]
        .as_str()
        .expect("a uuid")
        .to_string();

    // `sxv` holds SUBSCRIPTION_ALL on the same tenant and its membership
    // is active: the grant gate passes, the RECORDED INITIATOR refuses —
    // the body is byte-identical, so only the claimant differs.
    let refused = refused_with(&app, &sxv, &body, StatusCode::CONFLICT).await;
    assert_eq!(refused["reason"].as_str(), Some("conflict"), "{refused}");
    assert!(
        refused.get("receipt").is_none() && refused.get("subscriptionId").is_none(),
        "a foreign replay leaks no receipt: {refused}"
    );
    assert_eq!(
        count(&fixture, "commands").await,
        1,
        "no second command row"
    );
    assert_eq!(
        count(&fixture, "subscriptions").await,
        1,
        "no second subscription"
    );
    assert_eq!(
        count(&fixture, "entitlements").await,
        1,
        "and no second entitlement"
    );

    // The row names the actor that claimed it.
    let (actor,): (String,) = sqlx::query_as(
        "SELECT actor_id FROM archava_subscriptions.commands \
         WHERE tenant_id=$1 AND request_id=$2::uuid",
    )
    .bind(ACME)
    .bind(KEY_FOREIGN_REPLAY)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the command row answers");
    assert_eq!(actor, "sx", "the command records its initiator");

    // The initiating actor's own replay is untouched.
    let replay = receipt_of(&app, &sx, &body).await;
    assert_eq!(replay["idempotencyReplay"].as_bool(), Some(true));
    assert_eq!(
        replay["subscriptionId"].as_str(),
        Some(subscription_id.as_str())
    );
}
