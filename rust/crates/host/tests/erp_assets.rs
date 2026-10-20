//! The R05 assets cohort's host witnesses — driven through the real router
//! (`tower::ServiceExt::oneshot`) against the disposable PostgreSQL whose
//! `archava_assets` schema is the real migration ledger
//! (`001_assets_custody_maintenance`). Every assertion is a fact about what
//! the SERVER did:
//!
//! - the authorized custody life (create → update → dispatch → maintenance
//!   record → check-in → retire) ends with exactly the rows the
//!   repository's SQL spec freezes — the flat receipt document's
//!   requestId/type/assetId/revision/committedAt cells, the asset's
//!   status/condition/revision chain, the custody_events and
//!   maintenance_records rows each arm binds, and the audit naming every
//!   operation once;
//! - a customer-owned intake starts `in_custody` BY the actor's own hand,
//!   binds the actor as the first custodian, and records the fixed
//!   `service_intake` row — the business-owned twin records none;
//! - the custody vocabulary refuses the movement: a dispatch with a
//!   damaged or unserviceable load surfaces the TS 400-class verdict
//!   (`invalid_condition`) through the recorded `Unavailable` carrier, a
//!   business-owned `customer_return` the same, while dispatching twice
//!   or checking in what is not in custody stays the 409 class — and the
//!   refused submissions hold NOTHING (no command row, no event);
//! - a revoked grant refuses the recovery — yesterday's receipts are not
//!   replayable through a dead membership;
//! - the same idempotency key with a DIFFERENT command conflicts without
//!   recording;
//! - a lost response re-submits the SAME key + bytes and answers the SAME
//!   receipt bytes with still one asset, one event and one command row;
//! - a cross-tenant caller is refused before any business statement runs
//!   while the same caller acts freely in its own ledger;
//! - narrow grants refuse exactly the arms whose gate they lack; and
//! - the revision/terminal gates and the wire boundary refuse at the
//!   right layer, while the status read answers the TS commandStatus
//!   document verbatim.
//!
//! Zone-free by construction: the assets grammar has no calendar, so the
//! suite's only run-relative facts are the SQL-stamped instants the clock
//! binds — read back as bytes and counted, not predicted.

use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

/// The commerce ledger this host serves as the primary — the same table
/// list every ERP suite pins; the assets surface RIDEs the same serving
/// state without touching commerce rows.
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
const IDENTITY_KEY: &[u8] = &[23u8; 32];

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
/// and the purchasing ledger (`try_new_erp`'s second pass) — the assets
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
    )
    .await
    .expect("a ready fixture must construct serving state")
}

// --- The cast -----------------------------------------------------------------

/// The tenants this suite uses.
const ACME: &str = "acme-retail";
const GLOBEX: &str = "globex-corp";

/// The assets permission vocabulary (migration 001's CHECK).
const ASSET_ALL: &[&str] = &[
    "assets.read",
    "assets.write",
    "assets.handover",
    "assets.maintenance",
    "audit.read",
];

/// `ax` is the full-grant manager with scope all — the positive-path actor.
/// `axg` holds read + audit only: the four write arms refuse, while its
/// status reads walk.
/// `ax2` holds read only at an ASSIGNED scope — the status read's
/// RECORDED-ACTOR gate (an assigned-scope caller naming someone else's
/// request refuses) has a live member to pin it without touching a
/// revoked one.
/// `axr` is `ax` with the membership seeded ACTIVE and revoked by the test
/// itself — the recovery must not outlive the grant.
/// `axn` holds only `audit.read` — every grant gate refuses.
/// `other` holds full grants but ONLY in GLOBEX — the cross-tenant refusal
/// that still acts freely in its own ledger.
async fn seed(fixture: &Fixture) {
    for principal in ["ax", "axg", "ax2", "axr", "axn", "other"] {
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

    // The assets ledger's rows: the one-difference-per-actor pattern.
    for (principal, permissions, scope) in [
        ("ax", ASSET_ALL, "all"),
        ("axg", &["assets.read", "audit.read"][..], "all"),
        ("ax2", &["assets.read"][..], "assigned"),
        ("axr", ASSET_ALL, "all"),
        ("axn", &["audit.read"][..], "all"),
    ] {
        sqlx::query(
            "INSERT INTO archava_assets.memberships \
             (tenant_id, principal_id, kind, permissions, scope, active) \
             VALUES ($1, $2, 'human', $3, $4, true)",
        )
        .bind(ACME)
        .bind(principal)
        .bind(permissions.to_vec())
        .bind(scope)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the ACME assets membership");
    }
    sqlx::query(
        "INSERT INTO archava_assets.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(GLOBEX)
    .bind("other")
    .bind(ASSET_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the GLOBEX assets membership");
}

/// Fixed idempotency keys — stable so readbacks and the recovery witness
/// can name rows exactly.
const KEY_CREATE: &str = "7a231000-9999-4999-9999-0000000000a1";
const KEY_UPDATE: &str = "7a231000-9999-4999-9999-0000000000a2";
const KEY_DISPATCH: &str = "7a231000-9999-4999-9999-0000000000a3";
const KEY_MAINTAIN: &str = "7a231000-9999-4999-9999-0000000000a4";
const KEY_CHECKIN: &str = "7a231000-9999-4999-9999-0000000000a5";
const KEY_RETIRE: &str = "7a231000-9999-4999-9999-0000000000a6";
const KEY_CUSTOMER: &str = "7a231000-9999-4999-9999-0000000000a7";
const KEY_CUSTOM2: &str = "7a231000-9999-4999-9999-0000000000a8";
const KEY_VOCAB_ASSET: &str = "7a231000-9999-4999-9999-0000000000b1";
const KEY_VOCAB_CREATE: &str = "7a231000-9999-4999-9999-0000000000b2";
const KEY_VOCAB_DISPATCH_FIRST: &str = "7a231000-9999-4999-9999-0000000000b8";
const KEY_VOCAB_CHECKIN: &str = "7a231000-9999-4999-9999-0000000000b4";
const KEY_VOCAB_CHECKIN2: &str = "7a231000-9999-4999-9999-0000000000bc";
const KEY_VOCAB_RETURN: &str = "7a231000-9999-4999-9999-0000000000b5";
const KEY_VOCAB_RETURN2: &str = "7a231000-9999-4999-9999-0000000000b6";
const KEY_VOCAB_DISPATCH2: &str = "7a231000-9999-4999-9999-0000000000b7";
const KEY_VOCAB_DAMAGED_CREATE: &str = "7a231000-9999-4999-9999-0000000000bd";
const KEY_VOCAB_DAMAGED_DISPATCH: &str = "7a231000-9999-4999-9999-0000000000be";
const KEY_REVOKED: &str = "7a231000-9999-4999-9999-0000000000c1";
const KEY_REVOKED_RETRY: &str = "7a231000-9999-4999-9999-0000000000c2";
const KEY_REUSE: &str = "7a231000-9999-4999-9999-0000000000c3";
const KEY_REUSE_HOT: &str = "7a231000-9999-4999-9999-0000000000c4";
const KEY_REPLAY: &str = "7a231000-9999-4999-9999-0000000000c5";
const KEY_CROSS: &str = "7a231000-9999-4999-9999-0000000000d1";
const KEY_GLOBEX: &str = "7a231000-9999-4999-9999-0000000000d2";
const KEY_NG_CREATE: &str = "7a231000-9999-4999-9999-0000000000d3";
const KEY_NG_GRANT: &str = "7a231000-9999-4999-9999-0000000000d4";
const KEY_NG_DISPATCH: &str = "7a231000-9999-4999-9999-0000000000d5";
const KEY_NG_MAINTAIN: &str = "7a231000-9999-4999-9999-0000000000d6";
const KEY_NG_UPDATE: &str = "7a231000-9999-4999-9999-0000000000d7";
const KEY_REV_BASE: &str = "7a231000-9999-4999-9999-0000000000e0";
const KEY_STALE: &str = "7a231000-9999-4999-9999-0000000000e1";
const KEY_RETIRE_FIRST: &str = "7a231000-9999-4999-9999-0000000000e2";
const KEY_RETIRE_TWICE: &str = "7a231000-9999-4999-9999-0000000000e3";
const KEY_AFTER_GATES: &str = "7a231000-9999-4999-9999-0000000000e4";
const KEY_BAD: &str = "7a231000-9999-4999-9999-0000000000e5";
const KEY_STATUS: &str = "7a231000-9999-4999-9999-0000000000f1";
const KEY_STATUS_FREE: &str = "7a231000-9999-4999-9999-0000000000f2";
const STATUS_UNKNOWN: &str = "7a231000-9999-4999-9999-0000000000ff";

// --- HTTP helpers ------------------------------------------------------------

/// A session's credentials.
struct Session {
    token: String,
    csrf: String,
}

/// How many logins this suite may have in flight at once — the production
/// budget (never raised from inside the suite).
static LOGIN_SLOTS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(archava_host::max_active_derivations());

/// Log in and return the session's bearer token and CSRF value.
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

/// Send a POST with (or without) a session's credentials, always carrying the
/// CSRF header unless `with_csrf` says otherwise.
async fn post(
    app: &axum::Router,
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
    if status != StatusCode::OK {
        eprintln!("DBG-POST {status} {outcome}");
    }
    (status, outcome)
}

/// Send a GET with an optional session's bearer token (no CSRF — a read).
async fn get(app: &axum::Router, uri: &str, session: Option<&Session>) -> (StatusCode, Value) {
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

fn create_body(tenant: &str, request_id: &str, code: &str, name: &str, extra: Value) -> String {
    let mut data = json!({
        "code": code,
        "name": name,
        "category": "Pressure washer",
        "ownership": "business_owned",
    });
    if let Value::Object(fields) = extra {
        for (key, value) in fields {
            data[key] = value;
        }
    }
    json!({
        "tenant": tenant,
        "mutation": {"requestId": request_id, "type": "create_asset", "data": data}
    })
    .to_string()
}

fn update_body(
    tenant: &str,
    request_id: &str,
    asset_id: &str,
    expected_revision: i64,
    extra: Value,
) -> String {
    let mut data = json!({"assetId": asset_id, "expectedRevision": expected_revision});
    if let Value::Object(fields) = extra {
        for (key, value) in fields {
            data[key] = value;
        }
    }
    json!({
        "tenant": tenant,
        "mutation": {"requestId": request_id, "type": "update_asset", "data": data}
    })
    .to_string()
}

fn handover_body(
    tenant: &str,
    request_id: &str,
    asset_id: &str,
    expected_revision: i64,
    event_type: &str,
    condition_after: &str,
    extra: Value,
) -> String {
    let mut data = json!({
        "assetId": asset_id,
        "expectedRevision": expected_revision,
        "eventType": event_type,
        "conditionAfter": condition_after,
    });
    if let Value::Object(fields) = extra {
        for (key, value) in fields {
            data[key] = value;
        }
    }
    json!({
        "tenant": tenant,
        "mutation": {"requestId": request_id, "type": "handover_asset", "data": data}
    })
    .to_string()
}

fn maintenance_body(
    tenant: &str,
    request_id: &str,
    asset_id: &str,
    expected_revision: i64,
    service_type: &str,
    condition_after: &str,
) -> String {
    json!({
        "tenant": tenant,
        "mutation": {"requestId": request_id, "type": "record_maintenance", "data": {
            "assetId": asset_id,
            "expectedRevision": expected_revision,
            "serviceType": service_type,
            "description": "Cleared the intake filter",
            "conditionAfter": condition_after,
        }}
    })
    .to_string()
}

fn retire_body(tenant: &str, request_id: &str, asset_id: &str, expected_revision: i64) -> String {
    json!({
        "tenant": tenant,
        "mutation": {"requestId": request_id, "type": "retire_asset", "data": {
            "assetId": asset_id,
            "expectedRevision": expected_revision,
            "reason": "Concession retired unit",
        }}
    })
    .to_string()
}

// --- Row readers ---------------------------------------------------------------

/// The asset row's cells the witnesses check — status, condition, the
/// custodian and the revision.
async fn asset_row(
    fixture: &Fixture,
    tenant: &str,
    id: &str,
) -> (String, String, Option<String>, i64) {
    let row: (String, String, Option<String>, i32) = sqlx::query_as(
        "SELECT status,condition,current_custodian_id,revision \
         FROM archava_assets.assets WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(tenant)
    .bind(id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the asset row answers");
    (row.0, row.1, row.2, i64::from(row.3))
}

/// How many rows a tenant's assets table holds.
async fn count_of(fixture: &Fixture, table: &str, tenant: &str) -> i64 {
    let statement =
        format!("SELECT count(*)::bigint FROM archava_assets.{table} WHERE tenant_id=$1");
    let (count,): (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(statement.as_str()))
        .bind(tenant)
        .fetch_one(&fixture.admin_pool)
        .await
        .expect("the count answers");
    count
}

/// How many command rows a request id left behind (0 after a rollback).
async fn command_count(fixture: &Fixture, tenant: &str, request_id: &str) -> i64 {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT count(*)::bigint FROM archava_assets.commands \
         WHERE tenant_id=$1 AND request_id=$2::uuid",
    )
    .bind(tenant)
    .bind(request_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the command count answers");
    count
}

/// The audit's operation list, one row per committed command.
async fn audit_operations(fixture: &Fixture, tenant: &str) -> Vec<String> {
    sqlx::query_as::<_, (String,)>(
        "SELECT operation FROM archava_assets.audit WHERE tenant_id=$1 \
         ORDER BY occurred_at,request_id",
    )
    .bind(tenant)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("the audit rows answer")
    .into_iter()
    .map(|(operation,)| operation)
    .collect()
}

/// The custody events a tenant's ledger names, in recorded order —
/// `(event_type, to_custodian_id, notes)`.
async fn custody_event_rows(
    fixture: &Fixture,
    tenant: &str,
) -> Vec<(String, Option<String>, Option<String>)> {
    sqlx::query_as(
        "SELECT event_type,to_custodian_id,notes FROM archava_assets.custody_events \
         WHERE tenant_id=$1 ORDER BY recorded_at,id",
    )
    .bind(tenant)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("the custody event rows answer")
}

// --- W1: the authorized custody life ---------------------------------------------

/// The full custody life walks the ledger: the business-owned asset starts
/// available with nothing recorded, the update keeps the revision chain,
/// the dispatch binds the custodian and the event row, the maintenance
/// record rides its own history table, the check-in releases custody, and
/// the retires closes the life — the audit naming every operation once.
#[tokio::test]
async fn authorized_assets_life_documents_its_custody() {
    let fixture = provision().await;
    seed(&fixture).await;
    // The provision checks (role, the ledger, the forced-RLS width) pass
    // on the fixture's runtime role — the serving state's support check,
    // asserted here because the suite holds the receipt of it.
    archava_adapters::erp::assets::assert_assets_kernel(
        &fixture.runtime_pool,
        &fixture.assets_ledger,
    )
    .await
    .expect("the assets kernel asserts on the fixture");

    let app = archava_host::router(state(&fixture).await);

    let ax = login(&app, "ax").await;

    // 1. create_asset — the bare receipt, five members exactly.
    let (status, receipt) = {
        let body = create_body(
            ACME,
            KEY_CREATE,
            "PW-001",
            "Rooftop pressure washer",
            json!({
                "serialNumber": "SN-77",
                "ownership": "business_owned",
                "condition": "excellent",
                "initialLocation": "Depot shelf 3",
            }),
        );
        post(&app, "/erp/assets/commands", &body, Some(&ax), true).await
    };
    assert_eq!(
        status,
        StatusCode::OK,
        "create_asset must commit: {receipt}"
    );
    assert_eq!(
        receipt["requestId"].as_str(),
        Some(KEY_CREATE),
        "the receipt carries the request id verbatim"
    );
    assert_eq!(receipt["type"].as_str(), Some("create_asset"));
    assert_eq!(
        receipt.as_object().expect("a bare object").len(),
        5,
        "the bare receipt is the flat five-member document: {receipt}"
    );
    assert_eq!(receipt["revision"].as_i64(), Some(1));
    assert!(
        receipt["assetId"].as_str().is_some(),
        "the asset id is a fresh uuid"
    );
    assert!(
        receipt["committedAt"].as_str().is_some(),
        "the commit instant is stamped"
    );
    let asset_id = receipt["assetId"].as_str().unwrap_or_default().to_string();

    let (status_cell, condition_cell, custodian, revision) =
        asset_row(&fixture, ACME, &asset_id).await;
    assert_eq!(status_cell, "available");
    assert_eq!(condition_cell, "excellent");
    assert!(custodian.is_none(), "a business-owned asset holds nobody");
    assert_eq!(revision, 1);
    assert_eq!(count_of(&fixture, "assets", ACME).await, 1);
    assert_eq!(
        count_of(&fixture, "custody_events", ACME).await,
        0,
        "a business-owned intake records NO event"
    );
    assert_eq!(audit_operations(&fixture, ACME).await, ["create_asset"]);

    // 2. update_asset — the revision chain walks; the keep-current cells
    //    stay.
    let (status, receipt) = post(
        &app,
        "/erp/assets/commands",
        &update_body(
            ACME,
            KEY_UPDATE,
            &asset_id,
            1,
            json!({
                "name": "Rooftop pressure washer 2",
                "currentLocation": "Depot shelf 5",
            }),
        ),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "update_asset must commit: {receipt}"
    );
    assert_eq!(receipt["type"].as_str(), Some("update_asset"));
    assert_eq!(receipt["assetId"].as_str(), Some(asset_id.as_str()));
    assert_eq!(receipt["revision"].as_i64(), Some(2));
    let (status_cell, _, _, revision) = asset_row(&fixture, ACME, &asset_id).await;
    assert_eq!(status_cell, "available");
    assert_eq!(revision, 2);

    // 3. handover dispatch — the custodian binds, the event row records.
    let (status, receipt) = post(
        &app,
        "/erp/assets/commands",
        &handover_body(
            ACME,
            KEY_DISPATCH,
            &asset_id,
            2,
            "dispatch",
            "excellent",
            json!({
                "toCustodianId": "crew-lee",
                "location": "Rooftop site A",
                "notes": "Employed for the facade wash",
            }),
        ),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "dispatch must commit: {receipt}");
    assert_eq!(receipt["type"].as_str(), Some("handover_asset"));
    assert_eq!(receipt["revision"].as_i64(), Some(3));
    let (status_cell, _, custodian, revision) = asset_row(&fixture, ACME, &asset_id).await;
    assert_eq!(status_cell, "in_custody");
    assert_eq!(custodian.as_deref(), Some("crew-lee"));
    assert_eq!(revision, 3);
    let events = custody_event_rows(&fixture, ACME).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "dispatch");
    assert_eq!(events[0].1.as_deref(), Some("crew-lee"));

    // 4. record_maintenance — under custody the status stays.
    let (status, receipt) = post(
        &app,
        "/erp/assets/commands",
        &maintenance_body(ACME, KEY_MAINTAIN, &asset_id, 3, "corrective", "good"),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "record_maintenance must commit: {receipt}"
    );
    assert_eq!(receipt["type"].as_str(), Some("record_maintenance"));
    assert_eq!(receipt["revision"].as_i64(), Some(4));
    let (status_cell, condition_cell, _, revision) = asset_row(&fixture, ACME, &asset_id).await;
    assert_eq!(
        status_cell, "in_custody",
        "an in-custody asset keeps its custody through a serviceable record"
    );
    assert_eq!(condition_cell, "good");
    assert_eq!(revision, 4);

    // 5. handover checkin — the custody releases to available.
    let (status, receipt) = post(
        &app,
        "/erp/assets/commands",
        &handover_body(
            ACME,
            KEY_CHECKIN,
            &asset_id,
            4,
            "checkin",
            "excellent",
            json!({"location": "Depot shelf 5"}),
        ),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "checkin must commit: {receipt}");
    assert_eq!(receipt["revision"].as_i64(), Some(5));
    let (status_cell, _, custodian, _) = asset_row(&fixture, ACME, &asset_id).await;
    assert_eq!(status_cell, "available");
    assert!(custodian.is_none());
    assert_eq!(count_of(&fixture, "custody_events", ACME).await, 2);

    // 6. retire_asset — the terminal close.
    let (status, receipt) = post(
        &app,
        "/erp/assets/commands",
        &retire_body(ACME, KEY_RETIRE, &asset_id, 5),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "retire_asset must commit: {receipt}"
    );
    assert_eq!(receipt["type"].as_str(), Some("retire_asset"));
    assert_eq!(receipt["revision"].as_i64(), Some(6));
    let (status_cell, _, custodian, _) = asset_row(&fixture, ACME, &asset_id).await;
    assert_eq!(status_cell, "retired");
    assert!(custodian.is_none());

    // The audit named every operation once, in commit order.
    assert_eq!(
        audit_operations(&fixture, ACME).await,
        [
            "create_asset",
            "update_asset",
            "handover_asset",
            "record_maintenance",
            "handover_asset",
            "retire_asset",
        ]
    );
    // One command row per key; every fingerprint rides.
    let (unnamed, named): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE fingerprint = ''), \
                count(*) FILTER (WHERE fingerprint <> '') \
         FROM archava_assets.commands WHERE tenant_id=$1",
    )
    .bind(ACME)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the command fingerprints answer");
    assert_eq!(unnamed, 0, "every command row carries its fingerprint");
    assert_eq!(named, 6, "one command row per committed key");
}

// --- W2: the customer-owned intake ----------------------------------------------

/// The customer-owned twin: the asset STARTS in_custody BY the actor's
/// own hand, binds the actor as the first custodian, and the fixed
/// `service_intake` row records the contact as the from-side with the
/// condition cell both ways.
#[tokio::test]
async fn customer_owned_intake_records_the_service_intake() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let ax = login(&app, "ax").await;

    let (status, receipt) = post(
        &app,
        "/erp/assets/commands",
        &create_body(
            ACME,
            KEY_CUSTOMER,
            "CU-001",
            "Subsidized heat pump",
            json!({
                "ownership": "customer_owned",
                "customerContactId": "contact-9",
                "condition": "fair",
                "initialLocation": "Customer entrance",
            }),
        ),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the intake must commit: {receipt}");
    let asset_id = receipt["assetId"].as_str().unwrap_or_default().to_string();

    let (status_cell, condition_cell, custodian, revision) =
        asset_row(&fixture, ACME, &asset_id).await;
    assert_eq!(
        status_cell, "in_custody",
        "customer-owned starts in custody"
    );
    assert_eq!(condition_cell, "fair", "the condition the intake carried");
    assert_eq!(
        custodian.as_deref(),
        Some("ax"),
        "the actor IS the custodian"
    );
    assert_eq!(revision, 1);

    let events = custody_event_rows(&fixture, ACME).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "service_intake");
    assert_eq!(
        events[0].2.as_deref(),
        Some("Initial service intake from customer")
    );
    // The from-side is the recorded contact, not the actor.
    let (from_cell, from_recorded): (Option<String>, String) = sqlx::query_as(
        "SELECT from_custodian_id,recorded_by FROM archava_assets.custody_events \
         WHERE tenant_id=$1",
    )
    .bind(ACME)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the intake row answers");
    assert_eq!(from_cell.as_deref(), Some("contact-9"));
    assert_eq!(from_recorded, "ax");
}

// --- W3: the custody vocabulary's refusals ---------------------------------------

/// The movement vocabulary refuses at the state layer: dispatching twice,
/// checking in what is not in custody, destroying a damaged asset's
/// dispatch and a business-owned return — none of the refusals record.
#[tokio::test]
async fn custody_vocabulary_refuses_the_movement() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let ax = login(&app, "ax").await;

    // A healthy asset, first committed: the LEGAL dispatch walks.
    let (_, receipt) = post(
        &app,
        "/erp/assets/commands",
        &create_body(
            ACME,
            KEY_VOCAB_CREATE,
            "VW-001",
            "Observable ladder",
            json!({}),
        ),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(receipt["type"].as_str(), Some("create_asset"));
    let asset_id = receipt["assetId"].as_str().unwrap_or_default().to_string();
    let (_, receipt) = post(
        &app,
        "/erp/assets/commands",
        &handover_body(
            ACME,
            KEY_VOCAB_DISPATCH_FIRST,
            &asset_id,
            1,
            "dispatch",
            "excellent",
            json!({"toCustodianId": "crew-lee"}),
        ),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(
        receipt["revision"].as_i64(),
        Some(2),
        "the first dispatch commits"
    );

    let base_before = count_of(&fixture, "assets", ACME).await;
    let commands_before = count_of(&fixture, "commands", ACME).await;

    // dispatch a second time — in_custody already → 409.
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &handover_body(
            ACME,
            KEY_VOCAB_ASSET,
            &asset_id,
            2,
            "dispatch",
            "excellent",
            json!({"toCustodianId": "crew-kim"}),
        ),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "a second dispatch refuses");
    assert_eq!(command_count(&fixture, ACME, KEY_VOCAB_ASSET).await, 0);

    // The legal check-in releases custody...
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &handover_body(
            ACME,
            KEY_VOCAB_CHECKIN,
            &asset_id,
            2,
            "checkin",
            "excellent",
            json!({}),
        ),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the check-in commits");
    let (status_cell, _, custodian, _) = asset_row(&fixture, ACME, &asset_id).await;
    assert_eq!(status_cell, "available");
    assert!(custodian.is_none());
    // ...and a SECOND check-in refuses — the asset is not in custody.
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &handover_body(
            ACME,
            KEY_VOCAB_CHECKIN2,
            &asset_id,
            3,
            "checkin",
            "excellent",
            json!({}),
        ),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "an available asset cannot check in"
    );
    assert_eq!(command_count(&fixture, ACME, KEY_VOCAB_CHECKIN2).await, 0);

    // A customer return on a business-owned asset → the TS 400 class
    // (the recorded carrier riding `Unavailable`).
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &handover_body(
            ACME,
            KEY_VOCAB_RETURN,
            &asset_id,
            3,
            "customer_return",
            "excellent",
            json!({}),
        ),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a business-owned return refuses 400"
    );
    assert_eq!(command_count(&fixture, ACME, KEY_VOCAB_RETURN).await, 0);

    // A dispatch whose data carries an unknown member is a BOUNDARY 400 —
    // the strict shape refuses before any statement.
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &handover_body(
            ACME,
            KEY_VOCAB_DISPATCH2,
            &asset_id,
            3,
            "dispatch",
            "excellent",
            json!({
                "toCustodianId": "crew-lee",
                "warehouse": "east",
            }),
        ),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an unknown data member is a boundary 400"
    );
    assert_eq!(command_count(&fixture, ACME, KEY_VOCAB_DISPATCH2).await, 0);

    // A damaged asset exists...
    let (_, receipt) = post(
        &app,
        "/erp/assets/commands",
        &create_body(
            ACME,
            KEY_VOCAB_DAMAGED_CREATE,
            "VW-002",
            "Craned scaffold",
            json!({"condition": "damaged"}),
        ),
        Some(&ax),
        true,
    )
    .await;
    let damaged_asset_id = receipt["assetId"].as_str().unwrap_or_default().to_string();
    // ...and its dispatch refuses with the TS 400 class.
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &handover_body(
            ACME,
            KEY_VOCAB_DAMAGED_DISPATCH,
            &damaged_asset_id,
            1,
            "dispatch",
            "excellent",
            json!({"toCustodianId": "crew-lee"}),
        ),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a damaged dispatch refuses 400"
    );
    assert_eq!(
        command_count(&fixture, ACME, KEY_VOCAB_DAMAGED_DISPATCH).await,
        0
    );

    // A customer-owned asset's return is the ONE legal return.
    let (_, receipt) = post(
        &app,
        "/erp/assets/commands",
        &create_body(
            ACME,
            KEY_CUSTOM2,
            "CU-002",
            "Leased plating saw",
            json!({
                "ownership": "customer_owned",
                "customerContactId": "contact-7",
            }),
        ),
        Some(&ax),
        true,
    )
    .await;
    let customer_asset_id = receipt["assetId"].as_str().unwrap_or_default().to_string();
    let (status, receipt) = post(
        &app,
        "/erp/assets/commands",
        &handover_body(
            ACME,
            KEY_VOCAB_RETURN2,
            &customer_asset_id,
            1,
            "customer_return",
            "good",
            json!({}),
        ),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a customer-owned return commits: {receipt}"
    );
    let (status_cell, _, custodian, revision) = asset_row(&fixture, ACME, &customer_asset_id).await;
    assert_eq!(status_cell, "returned_to_customer");
    assert!(custodian.is_none());
    assert_eq!(revision, 2);
    // Under a returned asset the next movement refuses (a fresh dispatch).
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &handover_body(
            ACME,
            KEY_NG_DISPATCH,
            &customer_asset_id,
            2,
            "dispatch",
            "good",
            json!({"toCustodianId": "crew-lee"}),
        ),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "a returned asset is terminal");
    assert_eq!(command_count(&fixture, ACME, KEY_NG_DISPATCH).await, 0);

    // The refusals changed nothing structural: three assets committed,
    // and exactly the keys that WALKED since the capture left command
    // rows behind (the check-in, the damaged create, the customer create
    // and the return).
    assert_eq!(count_of(&fixture, "assets", ACME).await, base_before + 2);
    let commands_after = count_of(&fixture, "commands", ACME).await;
    assert!(
        commands_after == commands_before + 4,
        "exactly the FOUR walked keys recorded claims since the capture: {commands_after}"
    );
    // The event history names only the walks: dispatch, check-in, the
    // customer-owned intake and the return.
    assert_eq!(count_of(&fixture, "custody_events", ACME).await, 4);
}

// --- W4: the revoked-recovery family (RN) -----------------------------------------

/// A membership revoked after the receipt was recorded refuses BOTH the
/// re-execution (a fresh key, same state) and the status read — the
/// recovery must not outlive the grant.
#[tokio::test]
async fn revoked_assets_grant_refuses_the_recovery() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let axr = login(&app, "axr").await;

    // axr commits a create.
    let (_, receipt) = post(
        &app,
        "/erp/assets/commands",
        &create_body(ACME, KEY_REVOKED, "RB-001", "Refundable bolt", json!({})),
        Some(&axr),
        true,
    )
    .await;
    assert_eq!(receipt["type"].as_str(), Some("create_asset"));

    // The membership dies BEFORE any recovery.
    sqlx::query(
        "UPDATE archava_assets.memberships SET active=false \
                 WHERE tenant_id=$1 AND principal_id='axr'",
    )
    .bind(ACME)
    .execute(&fixture.admin_pool)
    .await
    .expect("revoke the membership");

    // A fresh key, same state — refused 403 and recording nothing.
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &update_body(
            ACME,
            KEY_REVOKED_RETRY,
            receipt["assetId"].as_str().unwrap_or_default(),
            1,
            json!({"name": "Refundable bolt 2"}),
        ),
        Some(&axr),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "the executed path refuses");
    assert_eq!(command_count(&fixture, ACME, KEY_REVOKED_RETRY).await, 0);

    // The status read of axr's OWN recorded request is refused too.
    let (status, _) = get(
        &app,
        &format!("/erp/assets/commands/status?tenant={ACME}&requestId={KEY_REVOKED}"),
        Some(&axr),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "the status path refuses");
}

// --- W5: the idempotency family ---------------------------------------------------

/// The same key with a different command conflicts WITHOUT recording —
/// the recorded receipt stays the ORIGINAL command's.
#[tokio::test]
async fn reuse_conflicts_and_records_nothing() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let ax = login(&app, "ax").await;

    let (_, receipt) = post(
        &app,
        "/erp/assets/commands",
        &create_body(ACME, KEY_REUSE, "ID-001", "Idempotent buggy", json!({})),
        Some(&ax),
        true,
    )
    .await;
    let asset_id = receipt["assetId"].as_str().unwrap_or_default().to_string();
    let commands_before = count_of(&fixture, "commands", ACME).await;

    // THE SAME KEY, a different command (dispatch instead of create).
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &handover_body(
            ACME,
            KEY_REUSE,
            &asset_id,
            1,
            "dispatch",
            "excellent",
            json!({"toCustodianId": "crew-lee"}),
        ),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a reuse with a new command conflicts"
    );
    assert_eq!(
        count_of(&fixture, "commands", ACME).await,
        commands_before,
        "the reuse recorded nothing"
    );
    assert_eq!(audit_operations(&fixture, ACME).await, ["create_asset"]);

    // A SECOND, DIFFERENT KEY with the same command commits freely — the
    // fingerprint guards replays, not creations.
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &handover_body(
            ACME,
            KEY_REUSE_HOT,
            &asset_id,
            1,
            "dispatch",
            "excellent",
            json!({"toCustodianId": "crew-lee"}),
        ),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a fresh key with the same command commits"
    );
}

// --- W6: the lost-response recovery ----------------------------------------------

/// A byte-identical resubmit answers the SAME receipt bytes — no second
/// asset, no second event, no revision walk.
#[tokio::test]
async fn replay_answers_the_same_receipt_without_a_second_asset() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let ax = login(&app, "ax").await;

    let body = create_body(
        ACME,
        KEY_REPLAY,
        "RP-001",
        "Replay crane",
        json!({"ownership": "business_owned", "condition": "excellent"}),
    );
    let (status, first) = post(&app, "/erp/assets/commands", &body, Some(&ax), true).await;
    assert_eq!(status, StatusCode::OK, "the original submission commits");

    // The SAME body, the SAME key — the replay answers the same bytes.
    let (status, second) = post(&app, "/erp/assets/commands", &body, Some(&ax), true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first, second, "the replay is the ORIGINAL receipt bytes");

    // The ledger holds exactly one of everything.
    assert_eq!(count_of(&fixture, "assets", ACME).await, 1);
    assert_eq!(command_count(&fixture, ACME, KEY_REPLAY).await, 1);
    assert_eq!(audit_operations(&fixture, ACME).await, ["create_asset"]);
}

// --- W7: the cross-tenant family --------------------------------------------------

/// A caller outside the rows' tenant is refused before any write (the
/// membership lock is the first statement), and the same caller acts
/// freely in its own ledger.
#[tokio::test]
async fn cross_tenant_assets_callers_refuse_before_any_statement() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let other = login(&app, "other").await;

    // The GLOBEX member names ACME's tenant in ITS OWN body — refused.
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &create_body(ACME, KEY_CROSS, "XT-001", "Foreign buggy", json!({})),
        Some(&other),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the cross-tenant write refuses"
    );
    assert_eq!(command_count(&fixture, ACME, KEY_CROSS).await, 0);
    assert_eq!(count_of(&fixture, "assets", ACME).await, 0);

    // The status read is refused the same way.
    let (status, _) = get(
        &app,
        &format!("/erp/assets/commands/status?tenant={ACME}&requestId={KEY_CROSS}"),
        Some(&other),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the cross-tenant read refuses"
    );

    // The same caller acts freely in its own ledger.
    let (status, receipt) = post(
        &app,
        "/erp/assets/commands",
        &create_body(GLOBEX, KEY_GLOBEX, "GL-001", "Own buggy", json!({})),
        Some(&other),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the own-ledger write commits: {receipt}"
    );
    assert_eq!(count_of(&fixture, "assets", GLOBEX).await, 1);
}

// --- W8: the narrow-grant family --------------------------------------------------

/// The grant ladder on assets' own vocabulary: the missing write/handover/
/// maintenance grants refuse exactly their arms while the read walks, and
/// an audit-only clerk is refused before any statement.
#[tokio::test]
async fn narrow_assets_grants_refuse_exactly_the_arms_they_lack() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let ax = login(&app, "ax").await;
    let axg = login(&app, "axg").await;

    // ax commits the life-enders: a fresh asset for axg to poke at.
    let (_, receipt) = post(
        &app,
        "/erp/assets/commands",
        &create_body(ACME, KEY_NG_GRANT, "NG-001", "Narrow buggy", json!({})),
        Some(&ax),
        true,
    )
    .await;
    let asset_id = receipt["assetId"].as_str().unwrap_or_default().to_string();

    // axg (read + audit only): the create refuses.
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &create_body(ACME, KEY_NG_CREATE, "NG-002", "Clergy buggy", json!({})),
        Some(&axg),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a read-only create refuses");
    assert_eq!(command_count(&fixture, ACME, KEY_NG_CREATE).await, 0);

    // ...the dispatch refuses.
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &handover_body(
            ACME,
            KEY_NG_DISPATCH,
            &asset_id,
            1,
            "dispatch",
            "excellent",
            json!({"toCustodianId": "crew-lee"}),
        ),
        Some(&axg),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a read-only dispatch refuses"
    );
    assert_eq!(command_count(&fixture, ACME, KEY_NG_DISPATCH).await, 0);

    // ...the maintenance refuses.
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &maintenance_body(ACME, KEY_NG_MAINTAIN, &asset_id, 1, "inspection", "good"),
        Some(&axg),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a read-only maintenance refuses"
    );
    assert_eq!(command_count(&fixture, ACME, KEY_NG_MAINTAIN).await, 0);

    // ...the update refuses.
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &update_body(ACME, KEY_NG_UPDATE, &asset_id, 1, json!({"name": "Nope"})),
        Some(&axg),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a read-only update refuses");
    assert_eq!(command_count(&fixture, ACME, KEY_NG_UPDATE).await, 0);

    // The read walks for the read-grant holder — and, its scope being
    // `all`, the answer is the COMMITTED document (the recorded-actor
    // gate is the assigned scope's own screen; W10 pins it with `ax2`).
    let (status, document) = get(
        &app,
        &format!("/erp/assets/commands/status?tenant={ACME}&requestId={KEY_NG_GRANT}"),
        Some(&axg),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a read-grant status walk answers: {document}"
    );
    assert_eq!(document["status"].as_str(), Some("committed"));

    // The audit-only clerk axn cannot even READ.
    let axn = login(&app, "axn").await;
    let (status, _) = get(
        &app,
        &format!("/erp/assets/commands/status?tenant={ACME}&requestId={KEY_NG_GRANT}"),
        Some(&axn),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "an audit-only read refuses");
}

// --- W9: the revision/terminal/wire gates ------------------------------------------

/// The revision and terminal gates plus the wire boundary refuse exactly
/// at their own layers — and an accepted walk still rides through after.
#[tokio::test]
async fn revision_state_and_boundary_gates_refuse() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let ax = login(&app, "ax").await;

    let (_, receipt) = post(
        &app,
        "/erp/assets/commands",
        &create_body(ACME, KEY_REV_BASE, "ST-001", "Stale buggy", json!({})),
        Some(&ax),
        true,
    )
    .await;
    let asset_id = receipt["assetId"].as_str().unwrap_or_default().to_string();

    // A stale expected revision conflicts.
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &update_body(ACME, KEY_STALE, &asset_id, 7, json!({"name": "Stale"})),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "a stale revision conflicts");
    assert_eq!(command_count(&fixture, ACME, KEY_STALE).await, 0);

    // The wire boundary — eight 400s, none recorded:
    // empty code, an over-long code, an over-long name, an over-long
    // category, an unknown condition, an unknown type, an extra data
    // member, and a customer-owned create without its contact.
    let long_code = "C".repeat(51);
    let long_name = "N".repeat(161);
    let long_category = "K".repeat(81);
    for bad in [
        // empty code
        json!({
            "tenant": ACME,
            "mutation": {"requestId": KEY_BAD, "type": "create_asset", "data": {
                "code": "", "name": "Buggy", "category": "Gear",
                "ownership": "business_owned"}},
        }),
        // over-long code
        json!({
            "tenant": ACME,
            "mutation": {"requestId": KEY_BAD, "type": "create_asset", "data": {
                "code": long_code, "name": "Buggy", "category": "Gear",
                "ownership": "business_owned"}},
        }),
        // over-long name
        json!({
            "tenant": ACME,
            "mutation": {"requestId": KEY_BAD, "type": "create_asset", "data": {
                "code": "WL-9", "name": long_name, "category": "Gear",
                "ownership": "business_owned"}},
        }),
        // over-long category
        json!({
            "tenant": ACME,
            "mutation": {"requestId": KEY_BAD, "type": "create_asset", "data": {
                "code": "WC-9", "name": "Buggy", "category": long_category,
                "ownership": "business_owned"}},
        }),
        // unknown condition
        json!({
            "tenant": ACME,
            "mutation": {"requestId": KEY_BAD, "type": "create_asset", "data": {
                "code": "WK-9", "name": "Buggy", "category": "Gear",
                "ownership": "business_owned", "condition": "sparkling"}},
        }),
        // unknown type
        json!({
            "tenant": ACME,
            "mutation": {"requestId": KEY_BAD, "type": "send.email", "data": {}},
        }),
        // an extra data member (the strict shape)
        json!({
            "tenant": ACME,
            "mutation": {"requestId": KEY_BAD, "type": "create_asset", "data": {
                "code": "WX-9", "name": "Buggy", "category": "Gear",
                "ownership": "business_owned", "warehouse": "east"}},
        }),
        // customer_owned without the contact
        json!({
            "tenant": ACME,
            "mutation": {"requestId": KEY_BAD, "type": "create_asset", "data": {
                "code": "WK-8", "name": "Buggy", "category": "Gear",
                "ownership": "customer_owned"}},
        }),
    ] {
        let (status, _) = post(
            &app,
            "/erp/assets/commands",
            bad.to_string().as_str(),
            Some(&ax),
            true,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "the boundary refuses: {bad}"
        );
    }
    assert_eq!(
        command_count(&fixture, ACME, KEY_BAD).await,
        0,
        "every boundary refusal recorded nothing"
    );
    assert_eq!(count_of(&fixture, "assets", ACME).await, 1);

    // The accepted walk rides through after the gates.
    let (status, receipt) = post(
        &app,
        "/erp/assets/commands",
        &update_body(
            ACME,
            KEY_AFTER_GATES,
            &asset_id,
            1,
            json!({"name": "Stale buggy 2"}),
        ),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a correct revision commits: {receipt}"
    );

    // retire, then retire again — the terminal refuses its twin.
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &retire_body(ACME, KEY_RETIRE_FIRST, &asset_id, 2),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the first retire commits");
    let (status, _) = post(
        &app,
        "/erp/assets/commands",
        &retire_body(ACME, KEY_RETIRE_TWICE, &asset_id, 3),
        Some(&ax),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a retired asset's arm refuses"
    );
    assert_eq!(command_count(&fixture, ACME, KEY_RETIRE_TWICE).await, 0);
}

// --- W10: the status read ----------------------------------------------------------

/// The status read answers the TS commandStatus document verbatim:
/// `committed` with its receipt, `not_committed` for an unknown id, and
/// the recorded-actor gate under a read authorized OTHER member.
#[tokio::test]
async fn status_read_answers_the_assets_command_status_document() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let ax = login(&app, "ax").await;
    let ax2 = login(&app, "ax2").await;

    // ax commits a life.
    let (_, receipt) = post(
        &app,
        "/erp/assets/commands",
        &create_body(ACME, KEY_STATUS, "SR-001", "Status buggy", json!({})),
        Some(&ax),
        true,
    )
    .await;

    // The committed document — receipt embedded, no request-id member on
    // the document itself.
    let (status, document) = get(
        &app,
        &format!("/erp/assets/commands/status?tenant={ACME}&requestId={KEY_STATUS}"),
        Some(&ax),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the committed read answers: {document}"
    );
    assert_eq!(document["status"].as_str(), Some("committed"));
    assert_eq!(
        document.as_object().expect("an object").len(),
        2,
        "the committed document is {{status,receipt}} exactly: {document}"
    );
    assert_eq!(
        document["receipt"], receipt,
        "the receipt is the SAME document"
    );

    // An unknown request id — not_committed, bare.
    let (status, document) = get(
        &app,
        &format!("/erp/assets/commands/status?tenant={ACME}&requestId={STATUS_UNKNOWN}"),
        Some(&ax),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        document,
        json!({"status": "not_committed"}),
        "the not_committed document is bare: {document}"
    );

    // The recorded-actor gate: an ASSIGNED-scope-shaped read authorized
    // OTHER member (ax2 holds only assets.read) naming ax's request is
    // refused — the row's actor is not the caller.
    let (status, _) = get(
        &app,
        &format!("/erp/assets/commands/status?tenant={ACME}&requestId={KEY_STATUS}"),
        Some(&ax2),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "another actor's request refuses"
    );

    // ax2's OWN unknown id answers free.
    let (status, document) = get(
        &app,
        &format!("/erp/assets/commands/status?tenant={ACME}&requestId={KEY_STATUS_FREE}"),
        Some(&ax2),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(document["status"].as_str(), Some("not_committed"));
}
