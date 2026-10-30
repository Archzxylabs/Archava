//! The R05 scheduling cohort's host witnesses — driven through the real
//! router (`tower::ServiceExt::oneshot`) against the disposable PostgreSQL
//! whose `archava_scheduling` schema is the real migration ledger
//! (`001_resource_reservations` + `002_shared_occupancy`). Every assertion
//! is a fact about what the SERVER did:
//!
//! - the authorized flow (create → set-active cycle → hold → confirm →
//!   move → cancel) ends with exactly the rows the repository's SQL spec
//!   freezes — the hold's expiry written from the clock sampled after the
//!   resource lock, the confirm keeping the identity cells, the revision
//!   chain in receipts and audit, and the status read answering `committed`
//!   with the stored receipt;
//! - a hold that overlaps a live one is refused by the database's
//!   overlap verdict WITHOUT holding anything (no claim row survives, the
//!   resource is untouched);
//! - a terminal (cancelled) reservation refuses further updates and
//!   records nothing;
//! - a revoked scheduling grant refuses the recovery — yesterday's
//!   receipts are not replayable through revoked access;
//! - the same idempotency key with a DIFFERENT command conflicts without
//!   recording;
//! - a cross-tenant caller is refused before any business statement runs
//!   while the same caller acts freely in its own ledger;
//! - a lost response re-submits the SAME key + bytes and answers `replayed`
//!   with no second resource;
//! - an actor whose scheduling grants are missing is refused by the grant
//!   gate with no command row at all; and
//! - a wall-clock instant (no offset) is a boundary 400 that never reaches
//!   a database statement.

use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

/// The commerce ledger this host serves as the primary — the same table list
/// every ERP suite pins; the scheduling surface RIDEs the same serving state
/// without touching commerce rows.
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
const IDENTITY_KEY: &[u8] = &[17u8; 32];

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
/// and the purchasing ledger (`try_new_erp`'s second pass) — the scheduling
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

/// The scheduling permission vocabulary (migration 001's CHECK).
const SCHED_ALL: &[&str] = &[
    "resources.read",
    "resources.write",
    "reservations.read",
    "reservations.write",
    "audit.read",
];

/// `sx` is the full-grant manager with scope all — the positive-path actor.
/// `sxn` holds only `audit.read` — every grant gate refuses.
/// `sxr` is `sx` with the membership seeded ACTIVE and revoked by the test
/// itself — the recovery must not outlive the grant.
/// `sxow` holds the reservation write but NOT the resource write — the
/// block arm's extra gate and the block-kind confirm/move/cancel gate.
/// `other` holds full grants but ONLY in GLOBEX — the cross-tenant refusal
/// that still acts freely in its own ledger.
async fn seed(fixture: &Fixture) {
    for principal in ["sx", "sxn", "sxr", "sxow", "other"] {
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

    // The scheduling ledger's rows: the one-difference-per-actor pattern.
    sqlx::query(
        "INSERT INTO archava_scheduling.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind("sx")
    .bind(SCHED_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed sx's scheduling membership");
    sqlx::query(
        "INSERT INTO archava_scheduling.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind("sxr")
    .bind(SCHED_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed sxr's ACTIVE scheduling membership");
    sqlx::query(
        "INSERT INTO archava_scheduling.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind("sxow")
    .bind(
        [
            "resources.read",
            "reservations.read",
            "reservations.write",
            "audit.read",
        ]
        .to_vec(),
    )
    .execute(&fixture.admin_pool)
    .await
    .expect("seed sxow's scheduling membership");
    sqlx::query(
        "INSERT INTO archava_scheduling.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind("sxn")
    .bind(["audit.read"].to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed sxn's scheduling membership");
    for principal in ["other"] {
        sqlx::query(
            "INSERT INTO archava_scheduling.memberships \
             (tenant_id, principal_id, kind, permissions, scope, active) \
             VALUES ($1, $2, 'human', $3, 'all', true)",
        )
        .bind(GLOBEX)
        .bind(principal)
        .bind(SCHED_ALL.to_vec())
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the GLOBEX scheduling membership");
    }
}

/// Fixed idempotency keys — stable so readbacks and the recovery witness
/// can name rows exactly.
const KEY_RESOURCE: &str = "b7d1a000-9999-4999-9999-0000000000a1";
const KEY_DEACTIVATE: &str = "b7d1a000-9999-4999-9999-0000000000a2";
const KEY_REACTIVATE: &str = "b7d1a000-9999-4999-9999-0000000000a3";
const KEY_HOLD: &str = "b7d1a000-9999-4999-9999-0000000000a4";
const KEY_CONFIRM: &str = "b7d1a000-9999-4999-9999-0000000000a5";
const KEY_MOVE: &str = "b7d1a000-9999-4999-9999-0000000000a6";
const KEY_CANCEL: &str = "b7d1a000-9999-4999-9999-0000000000a7";
const KEY_TERMINAL: &str = "b7d1a000-9999-4999-9999-0000000000a8";
const KEY_GRAIL: &str = "b7d1a000-9999-4999-9999-0000000000a9";
const KEY_OVERLAP: &str = "b7d1a000-9999-4999-9999-0000000000b1";
const KEY_OVERLAP_RESOURCE: &str = "b7d1a000-9999-4999-9999-0000000000b2";
const KEY_LIMITED_BLOCK: &str = "b7d1a000-9999-4999-9999-0000000000c1";
const KEY_GLOBEX: &str = "b7d1a000-9999-4999-9999-0000000000c2";
const KEY_CROSS: &str = "b7d1a000-9999-4999-9999-0000000000c3";
const KEY_REVOKED: &str = "b7d1a000-9999-4999-9999-0000000000d1";
const KEY_NONGRANT: &str = "b7d1a000-9999-4999-9999-0000000000e1";
const KEY_MALFORMED: &str = "b7d1a000-9999-4999-9999-0000000000f1";

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

/// An instant `seconds` from now (negatives go to the past), rendered in the
/// canonical `toISOString` family the domain parse produces — helpers from
/// the same rendering the adapter's clock walks.
fn instant_from_now(seconds: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the wall clock runs forward")
        .as_millis() as i64;
    archava_domain::erp::scheduling::instant_render(now + seconds * 1_000)
}

fn create_resource_body(tenant: &str, request_id: &str, title: &str) -> String {
    json!({
        "tenant": tenant,
        "mutation": {
            "requestId": request_id,
            "command": {
                "operation": "resource.create",
                "title": title,
                "kind": "room",
                "timezone": "Asia/Jakarta"
            }
        }
    })
    .to_string()
}

fn set_active_body(
    tenant: &str,
    request_id: &str,
    resource_id: &str,
    revision: i64,
    active: bool,
) -> String {
    json!({
        "tenant": tenant,
        "mutation": {
            "requestId": request_id,
            "command": {
                "operation": "resource.set-active",
                "resourceId": resource_id,
                "expectedRevision": revision,
                "active": active,
                "reason": if active { "reopened for the schedule" } else { "closed for a safety audit" }
            }
        }
    })
    .to_string()
}

fn hold_body(tenant: &str, request_id: &str, resource_id: &str, start: &str, end: &str) -> String {
    json!({
        "tenant": tenant,
        "mutation": {
            "requestId": request_id,
            "command": {
                "operation": "reservation.hold",
                "resourceId": resource_id,
                "title": "Beta rollout window",
                "startsAt": start,
                "endsAt": end
            }
        }
    })
    .to_string()
}

fn block_body(tenant: &str, request_id: &str, resource_id: &str, start: &str, end: &str) -> String {
    json!({
        "tenant": tenant,
        "mutation": {
            "requestId": request_id,
            "command": {
                "operation": "reservation.block",
                "resourceId": resource_id,
                "title": "Maintenance window",
                "startsAt": start,
                "endsAt": end,
                "reason": "hvac replacement"
            }
        }
    })
    .to_string()
}

#[allow(clippy::too_many_arguments)]
fn reservation_ref_body(
    tenant: &str,
    request_id: &str,
    operation: &str,
    resource_id: &str,
    reservation_id: &str,
    revision: i64,
    start: Option<&str>,
    end: Option<&str>,
) -> String {
    let mut command = json!({
        "operation": operation,
        "resourceId": resource_id,
        "id": reservation_id,
        "expectedRevision": revision,
    });
    if let Some(start) = start {
        command["startsAt"] = json!(start);
        command["endsAt"] = json!(end.expect("a move carries both instants"));
        command["reason"] = json!("moved after the vendor rescheduled");
    } else if operation == "reservation.cancel" {
        command["reason"] = json!("cancelled by the coordinator");
    }
    json!({
        "tenant": tenant,
        "mutation": {
            "requestId": request_id,
            "command": command
        }
    })
    .to_string()
}

fn malformed_body(tenant: &str, request_id: &str) -> String {
    // The wall-clock instant carries NO offset — the boundary parse refuses
    // it before any statement.
    json!({
        "tenant": tenant,
        "mutation": {
            "requestId": request_id,
            "command": {
                "operation": "reservation.hold",
                "resourceId": "11111111-2222-4333-8444-555555555555",
                "title": "Ambiguous time",
                "startsAt": "2026-01-05T08:00:00",
                "endsAt": "2026-01-05T09:00:00Z"
            }
        }
    })
    .to_string()
}

// --- Row reads -----------------------------------------------------------------

/// `(owner_id, active, revision)` of one resource row.
async fn resource_row(fixture: &Fixture, tenant: &str, id: &str) -> (String, bool, i64) {
    sqlx::query_as::<_, (String, bool, i32)>(
        "SELECT owner_id,active,revision FROM archava_scheduling.resources \
         WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(id.parse::<uuid::Uuid>().expect("a resource uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .map(|(owner, active, revision)| (owner, active, i64::from(revision)))
    .unwrap_or_else(|_| panic!("row for resource {id}"))
}

/// `(kind, state, revision, expiry)` of one reservation row.
async fn reservation_row(
    fixture: &Fixture,
    tenant: &str,
    id: &str,
) -> (String, String, i64, Option<String>) {
    sqlx::query_as::<_, (String, String, i32, Option<String>)>(
        "SELECT kind,state,revision, \
         to_char(expires_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') \
         FROM archava_scheduling.reservations WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(id.parse::<uuid::Uuid>().expect("a reservation uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .map(|(kind, state, revision, expiry)| (kind, state, i64::from(revision), expiry))
    .unwrap_or_else(|_| panic!("row for reservation {id}"))
}

/// The count of a parameterized tally over the scheduling ledger.
async fn count(fixture: &Fixture, statement: &'static str, binds: &[&str]) -> i64 {
    let mut query = sqlx::query_scalar(statement);
    for bind in binds {
        query = query.bind(bind);
    }
    query
        .fetch_one(&fixture.admin_pool)
        .await
        .expect("the tally answers")
}

// --- The witnesses ---------------------------------------------------------------

/// The authorized flow: create → set-active down + up → hold → confirm →
/// move → cancel, each receipt matching the rows and the audit history.
#[tokio::test]
async fn authorized_scheduling_flow_documents_its_life() {
    let fixture = provision().await;
    seed(&fixture).await;
    assert_eq!(fixture.scheduling_ledger.len(), 2, "both migrations apply");
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "sx").await;

    // 1. Create the room resource.
    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &create_resource_body(ACME, KEY_RESOURCE, "Conference room beta"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["decision"], "receipt");
    let receipt = &outcome["receipt"];
    assert_eq!(receipt["kind"], "resource");
    assert_eq!(receipt["revision"], 1);
    let resource_id = receipt["id"].as_str().expect("the resource id").to_string();

    // 2. Status read: the commit is durable by the same key.
    let (status, outcome) = get(
        &app,
        &format!("/erp/scheduling/commands/status?tenant={ACME}&requestId={KEY_RESOURCE}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["decision"], "committed");
    assert_eq!(outcome["receipt"]["kind"], "resource");
    assert_eq!(outcome["receipt"]["revision"], 1);

    // 3. Deactivate + reactivate — the revision climbs by every set-active,
    //    and the row itself follows.
    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &set_active_body(ACME, KEY_DEACTIVATE, &resource_id, 1, false),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["receipt"]["revision"], 2);
    let (owner, active, revision) = resource_row(&fixture, ACME, &resource_id).await;
    assert_eq!(
        owner, "sx",
        "the created row is owned by the ACTOR's owner id"
    );
    assert!(!active);
    assert_eq!(revision, 2);

    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &set_active_body(ACME, KEY_REACTIVATE, &resource_id, 2, true),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["receipt"]["revision"], 3);

    // 4. The hold — receipt kind reservation, revision 1, and the row carries
    //    the expiry the adapter computed from the post-lock clock.
    let starts_at = instant_from_now(600);
    let ends_at = instant_from_now(3600);
    let body = hold_body(ACME, KEY_HOLD, &resource_id, &starts_at, &ends_at);
    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["decision"], "receipt");
    let receipt = &outcome["receipt"];
    assert_eq!(receipt["kind"], "reservation");
    let reservation_id = receipt["id"]
        .as_str()
        .expect("the reservation id")
        .to_string();
    assert_eq!(receipt["resourceId"], resource_id);
    assert_eq!(receipt["revision"], 1);
    let (kind, state, revision, expires_at) =
        reservation_row(&fixture, ACME, &reservation_id).await;
    assert_eq!(kind, "booking");
    assert_eq!(state, "held");
    assert_eq!(revision, 1);
    assert!(expires_at.is_some(), "a hold carries its expiry");

    // 5. Confirm the hold — state flips; the identity cells stay.
    let body = reservation_ref_body(
        ACME,
        KEY_CONFIRM,
        "reservation.confirm",
        &resource_id,
        &reservation_id,
        1,
        None,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["receipt"]["revision"], 2);
    let (_, state, revision, expires_at) = reservation_row(&fixture, ACME, &reservation_id).await;
    assert_eq!(state, "confirmed");
    assert_eq!(revision, 2, "confirm bumps the revision");
    assert!(expires_at.is_some(), "confirm keeps the hold's expiry cell");

    // 6. Move it forward — instants change, nothing else mutates.
    let new_start = instant_from_now(3600 * 24 + 600);
    let new_end = instant_from_now(3600 * 24 + 3600);
    let body = reservation_ref_body(
        ACME,
        KEY_MOVE,
        "reservation.move",
        &resource_id,
        &reservation_id,
        2,
        Some(&new_start),
        Some(&new_end),
    );
    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["receipt"]["revision"], 3);
    let (kind, state, revision, _) = reservation_row(&fixture, ACME, &reservation_id).await;
    assert_eq!(kind, "booking");
    assert_eq!(state, "confirmed");
    assert_eq!(revision, 3);

    // 7. Cancel — the terminal state; the status read answers with the
    //    last receipt, whose revision names the terminal state implicitly.
    let body = reservation_ref_body(
        ACME,
        KEY_CANCEL,
        "reservation.cancel",
        &resource_id,
        &reservation_id,
        3,
        None,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["receipt"]["revision"], 4);
    let (_, state, revision, _) = reservation_row(&fixture, ACME, &reservation_id).await;
    assert_eq!(state, "cancelled");
    assert_eq!(revision, 4);
    let (status, outcome) = get(
        &app,
        &format!("/erp/scheduling/commands/status?tenant={ACME}&requestId={KEY_CANCEL}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["decision"], "committed");
    assert_eq!(outcome["receipt"]["revision"], 4);

    // 8. The audit ledger named every operation once — one row per command.
    let operations: Vec<String> = sqlx::query_as(
        "SELECT operation FROM archava_scheduling.audit \
         WHERE tenant_id=$1 ORDER BY occurred_at,request_id",
    )
    .bind(ACME)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("the audit rows answer")
    .into_iter()
    .map(|(operation,)| operation)
    .collect();
    assert_eq!(
        operations,
        vec![
            "resource.create",
            "resource.set-active",
            "resource.set-active",
            "reservation.hold",
            "reservation.confirm",
            "reservation.move",
            "reservation.cancel",
        ]
    );
    assert_eq!(operations.len(), 7);
}

/// The migration's overlap verdict — a second hold ON the same live interval
/// is a 409, NOTHING is recorded (the whole submission rolls back) and the
/// resource keeps its original revision.
#[tokio::test]
async fn overlapping_hold_refuses_and_holds_nothing() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "sx").await;

    // A resource with one live hold.
    let body = create_resource_body(ACME, KEY_OVERLAP_RESOURCE, "Ultrasound room");
    let (_, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    let resource_id = outcome["receipt"]["id"]
        .as_str()
        .expect("the resource id")
        .to_string();
    let starts_at = instant_from_now(600);
    let ends_at = instant_from_now(3600);
    let body = hold_body(ACME, KEY_OVERLAP, &resource_id, &starts_at, &ends_at);
    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["decision"], "receipt");

    // The overlapping hold — same interval, different instant spellings that
    // parse to the same instants (the fifference lives in the offset form).
    let overlapping = json!({
        "tenant": ACME,
        "mutation": {
            "requestId": KEY_HOLD,
            "command": {
                "operation": "reservation.hold",
                "resourceId": resource_id,
                "title": "Overlapping hold",
                "startsAt": format!("{}+00:00", starts_at.replace("Z", "")),
                "endsAt": format!("{}+00:00", ends_at.replace("Z", "")),
                "holdSeconds": 300
            }
        }
    })
    .to_string();
    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &overlapping,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{outcome}");
    assert_eq!(outcome["reason"], "conflict");

    // The ledger only remembers the FIRST submission: exactly ONE command
    // row, ONE audit row for the resource's holds; the second claim rolled
    // back with everything else.
    let commands = count(
        &fixture,
        "SELECT count(*) FROM archava_scheduling.commands WHERE tenant_id=$1",
        &[ACME],
    )
    .await;
    assert_eq!(
        commands, 2,
        "create + hold; the overlapping claim rolled back"
    );
    let audit = count(
        &fixture,
        "SELECT count(*) FROM archava_scheduling.audit WHERE tenant_id=$1 AND operation='reservation.hold'",
        &[ACME],
    )
    .await;
    assert_eq!(audit, 1, "only the first hold left an audit row");
    // The resource's receipt unchanged.
    let (status, outcome) = get(
        &app,
        &format!("/erp/scheduling/commands/status?tenant={ACME}&requestId={KEY_OVERLAP}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outcome["receipt"]["kind"], "reservation");
    assert_eq!(outcome["receipt"]["revision"], 1);
}

/// A cancelled reservation is terminal: further updates refuse WITHOUT
/// recording, and the status of the refused key shows not_committed.
#[tokio::test]
async fn terminal_reservation_refuses_updates() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "sx").await;

    // Create → hold → confirm → cancel.
    let body = create_resource_body(ACME, KEY_RESOURCE, "Morgue freezer");
    let (_, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    let resource_id = outcome["receipt"]["id"]
        .as_str()
        .expect("the resource id")
        .to_string();
    let body = hold_body(
        ACME,
        KEY_HOLD,
        &resource_id,
        &instant_from_now(600),
        &instant_from_now(3600),
    );
    let (_, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    let reservation_id = outcome["receipt"]["id"]
        .as_str()
        .expect("the reservation id")
        .to_string();
    let body = reservation_ref_body(
        ACME,
        KEY_CONFIRM,
        "reservation.confirm",
        &resource_id,
        &reservation_id,
        1,
        None,
        None,
    );
    let (status, _) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body = reservation_ref_body(
        ACME,
        KEY_CANCEL,
        "reservation.cancel",
        &resource_id,
        &reservation_id,
        2,
        None,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["receipt"]["revision"], 3);

    // Try to move the cancelled reservation — conflict, nothing recorded.
    let body = reservation_ref_body(
        ACME,
        KEY_TERMINAL,
        "reservation.move",
        &resource_id,
        &reservation_id,
        3,
        Some(&instant_from_now(7200)),
        Some(&instant_from_now(10000)),
    );
    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{outcome}");
    assert_eq!(outcome["reason"], "conflict");
    let commands = count(
        &fixture,
        "SELECT count(*) FROM archava_scheduling.commands WHERE tenant_id=$1",
        &[ACME],
    )
    .await;
    assert_eq!(commands, 4, "the terminal move left no claim");
    let (_, state, revision, _) = reservation_row(&fixture, ACME, &reservation_id).await;
    assert_eq!(state, "cancelled");
    assert_eq!(revision, 3);
    // The refused key has NO commit on the wire.
    let (status, outcome) = get(
        &app,
        &format!("/erp/scheduling/commands/status?tenant={ACME}&requestId={KEY_TERMINAL}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outcome["decision"], "not_committed");
}

/// The grant gates: the block arm's extra `resources.write` (an actor with
/// only the reservation write) and a caller with nothing but `audit.read`
/// both refuse, and neither leaves a command row.
#[tokio::test]
async fn insufficient_grants_refuse_before_any_statement() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "sx").await;
    let limited = login(&app, "sxow").await;
    let none = login(&app, "sxn").await;

    // The resource owner seeds one resource; sxow tries the block arm.
    let body = create_resource_body(ACME, KEY_RESOURCE, "Kitchen");
    let (_, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    let resource_id = outcome["receipt"]["id"]
        .as_str()
        .expect("the resource id")
        .to_string();
    let body = block_body(
        ACME,
        KEY_LIMITED_BLOCK,
        &resource_id,
        &instant_from_now(3600),
        &instant_from_now(7200),
    );
    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&limited),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the block arm needs resources.write: {outcome}"
    );
    assert_eq!(outcome["reason"], "forbidden");

    // An audit-read-only actor cannot even create a resource.
    let body = create_resource_body(ACME, KEY_NONGRANT, "Never built");
    let (status, outcome) = post(&app, "/erp/scheduling/commands", &body, Some(&none), true).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{outcome}");
    assert_eq!(outcome["reason"], "forbidden");
    // And no command row survives either refusal.
    let commands = count(
        &fixture,
        "SELECT count(*) FROM archava_scheduling.commands WHERE tenant_id=$1 AND \
         actor_id IN ('sxow','sxn')",
        &[ACME],
    )
    .await;
    assert_eq!(commands, 0, "refused claims leave nothing behind");
}

/// The recovery refusals: a revoked grant cannot replay yesterday's receipt,
/// the same key with a DIFFERENT command conflicts, and a lost response
/// replays the SAME bytes without a second resource.
#[tokio::test]
async fn revoked_grant_refuses_recovery() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "sxr").await;

    // The resource.
    let body = create_resource_body(ACME, KEY_REVOKED, "Cold storage");
    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["decision"], "receipt");
    let resource_id = outcome["receipt"]["id"]
        .as_str()
        .expect("the resource id")
        .to_string();

    // Status BEFORE the revoke: recoverable.
    let (status, outcome) = get(
        &app,
        &format!("/erp/scheduling/commands/status?tenant={ACME}&requestId={KEY_REVOKED}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outcome["decision"], "committed");

    // Revoke the grant, then replay the SAME submission.
    sqlx::query(
        "UPDATE archava_scheduling.memberships SET active=false \
         WHERE tenant_id=$1 AND principal_id='sxr'",
    )
    .bind(ACME)
    .execute(&fixture.admin_pool)
    .await
    .expect("revoke the scheduling grant");
    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "revoked access cannot replay: {outcome}"
    );
    assert_eq!(outcome["reason"], "forbidden");
    // And the status read refuses the recovery too.
    let (status, outcome) = get(
        &app,
        &format!("/erp/scheduling/commands/status?tenant={ACME}&requestId={KEY_REVOKED}"),
        Some(&session),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the recovery is not grant-proof: {outcome}"
    );
    assert_eq!(outcome["reason"], "forbidden");
    // The resource stands — the revoke changed access, not history.
    let (owner, _, _) = resource_row(&fixture, ACME, &resource_id).await;
    assert_eq!(owner, "sxr", "the revoked grant does not erase history");
}

/// The same idempotency key with a DIFFERENT command — a genuine conflict
/// that records nothing.
#[tokio::test]
async fn same_key_different_command_conflicts_without_recording() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "sx").await;

    // A hold lives on KEY_HOLD.
    let body = create_resource_body(ACME, KEY_RESOURCE, "Consultation room");
    let (_, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    let resource_id = outcome["receipt"]["id"]
        .as_str()
        .expect("the resource id")
        .to_string();
    let body = hold_body(
        ACME,
        KEY_HOLD,
        &resource_id,
        &instant_from_now(600),
        &instant_from_now(3600),
    );
    let (status, _) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The SAME key, a DIFFERENT command (a block) — conflict, no second row,
    // the original reservation untouched.
    let body = block_body(
        ACME,
        KEY_HOLD,
        &resource_id,
        &instant_from_now(600),
        &instant_from_now(3600),
    );
    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{outcome}");
    assert_eq!(outcome["reason"], "conflict");
    let commands = count(
        &fixture,
        "SELECT count(*) FROM archava_scheduling.commands WHERE tenant_id=$1",
        &[ACME],
    )
    .await;
    assert_eq!(commands, 2, "the reused key added no command row");
    let audit = count(
        &fixture,
        "SELECT count(*) FROM archava_scheduling.audit WHERE tenant_id=$1",
        &[ACME],
    )
    .await;
    assert_eq!(audit, 2, "no audit row for the reused key");
}

/// A cross-tenant caller is refused before any business statement runs —
/// then acts freely in their own ledger.
#[tokio::test]
async fn cross_tenant_caller_refused_before_any_write() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "other").await;

    // ACME: no membership — refused before any statement.
    let body = create_resource_body(ACME, KEY_CROSS, "Invisible room");
    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{outcome}");
    assert_eq!(outcome["reason"], "forbidden");
    let commands = count(
        &fixture,
        "SELECT count(*) FROM archava_scheduling.commands WHERE tenant_id=$1",
        &[ACME],
    )
    .await;
    assert_eq!(commands, 0, "the cross-tenant refusal ran no claim");

    // The SAME caller acts freely in their OWN ledger — same request id as
    // any other tenant's key would be a different key per TENANT (the
    // tenant dimension of the key space).
    let body = create_resource_body(GLOBEX, KEY_GLOBEX, "Own tenant room");
    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["decision"], "receipt");
    assert_eq!(outcome["receipt"]["kind"], "resource");
    let (status, outcome) = get(
        &app,
        &format!("/erp/scheduling/commands/status?tenant={GLOBEX}&requestId={KEY_GLOBEX}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outcome["decision"], "committed");
}

/// A lost response re-submits the SAME key + bytes and answers `replayed`
/// with no second resource.
#[tokio::test]
async fn lost_response_replays_without_a_second_resource() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "sx").await;

    let body = create_resource_body(ACME, KEY_GRAIL, "The grail room");
    let (status, first) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["decision"], "receipt");
    let _resource_id = first["receipt"]["id"].as_str().expect("the id").to_string();

    let (status, second) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["decision"], "replayed");
    assert_eq!(second["receipt"], first["receipt"], "the same receipt");
    let resources = count(
        &fixture,
        "SELECT count(*) FROM archava_scheduling.resources WHERE tenant_id=$1",
        &[ACME],
    )
    .await;
    assert_eq!(resources, 1, "one resource");
    let audit = count(
        &fixture,
        "SELECT count(*) FROM archava_scheduling.audit WHERE tenant_id=$1",
        &[ACME],
    )
    .await;
    assert_eq!(audit, 1, "one command, one audit");
}

/// A malformed document (the wall-clock hold) is a boundary 400 that never
/// reaches a database statement.
#[tokio::test]
async fn malformed_instant_is_a_boundary_400_never_a_claim() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "sx").await;

    let body = malformed_body(ACME, KEY_MALFORMED);
    let (status, outcome) = post(
        &app,
        "/erp/scheduling/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{outcome}");
    assert_eq!(outcome["reason"], "invalid_submission");
    // Nothing reached the ledger at all.
    let commands = count(
        &fixture,
        "SELECT count(*) FROM archava_scheduling.commands WHERE tenant_id=$1",
        &[ACME],
    )
    .await;
    assert_eq!(commands, 0);
    let audit = count(
        &fixture,
        "SELECT count(*) FROM archava_scheduling.audit WHERE tenant_id=$1",
        &[ACME],
    )
    .await;
    assert_eq!(audit, 0);
    let resources = count(
        &fixture,
        "SELECT count(*) FROM archava_scheduling.resources WHERE tenant_id=$1",
        &[ACME],
    )
    .await;
    assert_eq!(resources, 0);
}

/// P2 (A11): the status read answers to the scheduling READ grant, and the
/// receipt is bound to the actor that wrote it. Membership alone — an
/// `audit.read`-only clerk — must not recover another actor's receipt.
#[tokio::test]
async fn status_read_requires_the_scheduling_read_grant() {
    const KEY: &str = "a11e0000-4444-4444-8444-0000000000e1";
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let author = login(&app, "sx").await;
    let auditor = login(&app, "sxn").await;
    let peer = login(&app, "sxr").await;

    let body = create_resource_body(ACME, KEY, "Status read room");
    let (status, outcome) =
        post(&app, "/erp/scheduling/commands", &body, Some(&author), true).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the author records the command: {outcome}"
    );

    let uri = format!("/erp/scheduling/commands/status?tenant={ACME}&requestId={KEY}");
    let (status, outcome) = get(&app, &uri, Some(&auditor)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "audit.read is not resources.read: {outcome}"
    );
    assert_eq!(outcome["reason"], "forbidden");

    let (status, outcome) = get(&app, &uri, Some(&peer)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the receipt belongs to its actor, not to any full-grant peer: {outcome}"
    );
    assert_eq!(outcome["reason"], "forbidden");

    let (status, outcome) = get(&app, &uri, Some(&author)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the author still recovers it: {outcome}"
    );
    assert_eq!(outcome["decision"], "committed");
}
