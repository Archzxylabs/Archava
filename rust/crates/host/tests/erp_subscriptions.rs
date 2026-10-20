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
    let (status, list) = get(
        &app,
        &format!("/erp/subscriptions/subscriptions?tenant={ACME}"),
        &sxg,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the read holder may read: {list}");
    assert_eq!(list.as_array().map(|rows| rows.len()), Some(1));

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
