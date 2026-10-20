//! The R05 accommodation cohort's host witnesses — driven through the real
//! router (`tower::ServiceExt::oneshot`) against the disposable PostgreSQL
//! whose `archava_accommodation` schema is the real migration ledger
//! (`001_room_night_stays`) over the scheduling kernel the PACKET-004
//! fixture provisioned. Every assertion is a fact about what the SERVER
//! did:
//!
//! - the authorized life (property → room type → room → hold → confirm →
//!   check-in → check-out) ends with exactly the rows the repository's SQL
//!   spec freezes — the receipt kinds/revisions/identity cells, the kernel
//!   slot's state machine inside `archava_scheduling.occupancy`, the
//!   room's housekeeping vocabulary + revision chain (including the
//!   migration's own `dirty_after_stay` bumps), and the audit naming
//!   every operation once;
//! - a wall-clock local time inside a DST gap or the fall-back repeat is a
//!   409 that records NOTHING (the `CalendarTimeError` verdict rides the
//!   conflict whitelist) while a normal-zone resolve pins the exact
//!   instant bytes the property's timezone owns;
//! - a stay window overlapping a live one on the SAME room is refused by
//!   the kernel's overlap verdict WITHOUT holding anything, while a
//!   disjoint window on the same room goes through;
//! - the housekeeping vocabulary refuses the illegal transitions, writes
//!   under physical occupancy and deactivation under occupancy, and an
//!   inactive room refuses holds;
//! - a revoked grant refuses the recovery — yesterday's receipts are not
//!   replayable through a dead membership;
//! - the same idempotency key with a DIFFERENT command conflicts without
//!   recording;
//! - a lost response re-submits the SAME key + bytes and answers the SAME
//!   receipt bytes with still exactly one stay and one kernel slot;
//! - a cross-tenant caller is refused before any business statement runs
//!   while the same caller acts freely in its own ledger;
//! - narrow grants refuse exactly the arms whose gate they lack; and
//! - the hold's window/capacity gates and the wire boundary (night span,
//!   hold cap, checkout order) refuse at the right layer.
//!
//! The lifecycle property lives in the `UTC` zone with `00:00`/`00:00`
//! daily times: a hold for today→tomorrow is then EXACTLY the current UTC
//! day (`[today 00:00, tomorrow 00:00)`), which contains whatever instant
//! the suite runs at — the hold gate holds because `arrival == the
//! property's today`, and the window gates (confirm/check-in/check-out)
//! hold because `now` is inside the window by construction. Every
//! run-relative calendar date builds through the domain's civil arithmetic
//! ([`render_civil_date`]) — the same way the scheduling suite builds
//! relative instants.

use archava_domain::erp::accommodation::render_civil_date;
use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

/// The commerce ledger this host serves as the primary — the same table
/// list every ERP suite pins; the accommodation surface RIDEs the same
/// serving state without touching commerce rows.
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
/// and the purchasing ledger (`try_new_erp`'s second pass) — the
/// accommodation surface rides the same serving state; its own ledger rows
/// exist in the fixture regardless.
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

/// The accommodation permission vocabulary (migration 001's CHECK).
const ACC_ALL: &[&str] = &[
    "properties.read",
    "properties.write",
    "rooms.read",
    "rooms.write",
    "housekeeping.write",
    "rooms.inspect",
    "stays.read",
    "stays.write",
    "audit.read",
];

/// `ax` is the full-grant manager with scope all — the positive-path actor.
/// `axg` holds everything EXCEPT `rooms.write`, `housekeeping.write` and
/// `rooms.inspect` — the arms whose gates it lacks refuse, while its own
/// stay arms walk.
/// `axr` is `ax` with the membership seeded ACTIVE and revoked by the test
/// itself — the recovery must not outlive the grant.
/// `axn` holds only `audit.read` — every grant gate refuses.
/// `other` holds full grants but ONLY in GLOBEX — the cross-tenant refusal
/// that still acts freely in its own ledger.
async fn seed(fixture: &Fixture) {
    for principal in ["ax", "axg", "axr", "axn", "other"] {
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

    // The accommodation ledger's rows: the one-difference-per-actor pattern.
    for (principal, permissions) in [
        ("ax", ACC_ALL),
        (
            "axg",
            &[
                "properties.read",
                "properties.write",
                "rooms.read",
                "stays.read",
                "stays.write",
                "audit.read",
            ][..],
        ),
        ("axr", ACC_ALL),
        ("axn", &["audit.read"][..]),
    ] {
        sqlx::query(
            "INSERT INTO archava_accommodation.memberships \
             (tenant_id, principal_id, kind, permissions, scope, active) \
             VALUES ($1, $2, 'human', $3, 'all', true)",
        )
        .bind(ACME)
        .bind(principal)
        .bind(permissions.to_vec())
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the ACME accommodation membership");
    }
    sqlx::query(
        "INSERT INTO archava_accommodation.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(GLOBEX)
    .bind("other")
    .bind(ACC_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the GLOBEX accommodation membership");
}

/// Fixed idempotency keys — stable so readbacks and the recovery witness
/// can name rows exactly.
const KEY_PROPERTY: &str = "b7d1b000-9999-4999-9999-0000000000a1";
const KEY_TYPE: &str = "b7d1b000-9999-4999-9999-0000000000a2";
const KEY_ROOM: &str = "b7d1b000-9999-4999-9999-0000000000a3";
const KEY_HK_CLEAN: &str = "b7d1b000-9999-4999-9999-0000000000a4";
const KEY_HK_INSPECT: &str = "b7d1b000-9999-4999-9999-0000000000a5";
const KEY_HOLD: &str = "b7d1b000-9999-4999-9999-0000000000a6";
const KEY_CONFIRM: &str = "b7d1b000-9999-4999-9999-0000000000a7";
const KEY_CHECKIN: &str = "b7d1b000-9999-4999-9999-0000000000a8";
const KEY_CHECKOUT: &str = "b7d1b000-9999-4999-9999-0000000000a9";
const KEY_EU_PROPERTY: &str = "b7d1b000-9999-4999-9999-0000000000b1";
const KEY_EU_TYPE: &str = "b7d1b000-9999-4999-9999-0000000000b2";
const KEY_EU_ROOM: &str = "b7d1b000-9999-4999-9999-0000000000b3";
const KEY_GAP: &str = "b7d1b000-9999-4999-9999-0000000000b4";
const KEY_AMBIGUOUS: &str = "b7d1b000-9999-4999-9999-0000000000b5";
const KEY_MD_PROPERTY: &str = "b7d1b000-9999-4999-9999-0000000000b6";
const KEY_MD_TYPE: &str = "b7d1b000-9999-4999-9999-0000000000b7";
const KEY_MD_ROOM: &str = "b7d1b000-9999-4999-9999-0000000000b8";
const KEY_BRIGHT: &str = "b7d1b000-9999-4999-9999-0000000000b9";
const KEY_OVERLAP_PROPERTY: &str = "b7d1b000-9999-4999-9999-0000000000c1";
const KEY_OVERLAP_TYPE: &str = "b7d1b000-9999-4999-9999-0000000000c2";
const KEY_OVERLAP_ROOM: &str = "b7d1b000-9999-4999-9999-0000000000c3";
const KEY_OVERLAP_A: &str = "b7d1b000-9999-4999-9999-0000000000c4";
const KEY_OVERLAP_B: &str = "b7d1b000-9999-4999-9999-0000000000c5";
const KEY_OVERLAP_FREE: &str = "b7d1b000-9999-4999-9999-0000000000c6";
const KEY_HK_PROPERTY: &str = "b7d1b000-9999-4999-9999-0000000000d1";
const KEY_HK_TYPE: &str = "b7d1b000-9999-4999-9999-0000000000d2";
const KEY_HK_ROOM1: &str = "b7d1b000-9999-4999-9999-0000000000d3";
const KEY_HK_ROOM2: &str = "b7d1b000-9999-4999-9999-0000000000d4";
const KEY_HK_HOLD: &str = "b7d1b000-9999-4999-9999-0000000000d5";
const KEY_HK_CONFIRM: &str = "b7d1b000-9999-4999-9999-0000000000d6";
const KEY_HK_CHECKIN: &str = "b7d1b000-9999-4999-9999-0000000000d7";
const KEY_HK_CLEAN1: &str = "b7d1b000-9999-4999-9999-0000000000d8";
const KEY_HK_INSPECT1: &str = "b7d1b000-9999-4999-9999-0000000000d9";
const KEY_HK_BLOCKED: &str = "b7d1b000-9999-4999-9999-0000000000e1";
const KEY_HK_DEACT: &str = "b7d1b000-9999-4999-9999-0000000000e2";
const KEY_HK_BAD_DIRTY_INSPECT: &str = "b7d1b000-9999-4999-9999-0000000000e3";
const KEY_HK_BAD_SAME_DIRTY: &str = "b7d1b000-9999-4999-9999-0000000000e4";
const KEY_HK_CLEAN2: &str = "b7d1b000-9999-4999-9999-0000000000e5";
const KEY_HK_BAD_SAME_CLEAN: &str = "b7d1b000-9999-4999-9999-0000000000e6";
const KEY_HK_INSPECT2: &str = "b7d1b000-9999-4999-9999-0000000000e7";
const KEY_HK_BAD_INSPECT_CLEAN: &str = "b7d1b000-9999-4999-9999-0000000000e8";
const KEY_HK_DIRTY2: &str = "b7d1b000-9999-4999-9999-0000000000e9";
const KEY_HK_DEACT_DONE: &str = "b7d1b000-9999-4999-9999-0000000000f1";
const KEY_HK_INACTIVE_HOLD: &str = "b7d1b000-9999-4999-9999-0000000000f2";
const KEY_REVOKED_HOLD: &str = "b7d1b000-9999-4999-9999-000000000a10";
const KEY_REVOKED_AGAIN: &str = "b7d1b000-9999-4999-9999-000000000a11";
const KEY_REUSE: &str = "b7d1b000-9999-4999-9999-000000000a12";
const KEY_REPLAY: &str = "b7d1b000-9999-4999-9999-000000000a13";
const KEY_CROSS: &str = "b7d1b000-9999-4999-9999-000000000a14";
const KEY_GLOBEX: &str = "b7d1b000-9999-4999-9999-000000000a15";
const KEY_NG_TYPE: &str = "b7d1b000-9999-4999-9999-000000000a16";
const KEY_NG_HK: &str = "b7d1b000-9999-4999-9999-000000000a17";
const KEY_NG_HOLD: &str = "b7d1b000-9999-4999-9999-000000000a18";
const KEY_NG_CANCEL: &str = "b7d1b000-9999-4999-9999-000000000a19";
const KEY_NONGRANT: &str = "b7d1b000-9999-4999-9999-000000000a20";
const KEY_PAST: &str = "b7d1b000-9999-4999-9999-000000000a21";
const KEY_CROWDED: &str = "b7d1b000-9999-4999-9999-000000000a22";
const KEY_FREEHOLD: &str = "b7d1b000-9999-4999-9999-000000000a23";
const KEY_LONG: &str = "b7d1b000-9999-4999-9999-000000000a24";
const KEY_UNKNOWN: &str = "b7d1b000-9999-4999-9999-0000000000ff";

// --- Run-relative calendar dates ----------------------------------------------

/// Today's UTC civil-day ordinal — the same instant family the property's
/// zone resolution walks.
fn epoch_days_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the wall clock runs forward")
        .as_millis() as i64
        / 86_400_000
}

/// A calendar date `days` from today, canonically rendered — the suite's
/// ONLY date builder (no fixed dates anywhere near the run-relative gates).
fn date_offset(days: i64) -> String {
    render_civil_date(epoch_days_now() + days)
}

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

fn property_body(
    tenant: &str,
    request_id: &str,
    title: &str,
    timezone: &str,
    check_in: Option<&str>,
    check_out: Option<&str>,
) -> String {
    let mut command = json!({
        "operation": "property.create",
        "title": title,
        "timezone": timezone,
    });
    if let Some(check_in) = check_in {
        command["checkInTime"] = json!(check_in);
    }
    if let Some(check_out) = check_out {
        command["checkOutTime"] = json!(check_out);
    }
    json!({"tenant": tenant, "mutation": {"requestId": request_id, "command": command}}).to_string()
}

fn room_type_body(
    tenant: &str,
    request_id: &str,
    property_id: &str,
    title: &str,
    max_guests: i64,
    nightly_rate: i64,
) -> String {
    json!({
        "tenant": tenant,
        "mutation": {"requestId": request_id, "command": {
            "operation": "room-type.create",
            "propertyId": property_id,
            "title": title,
            "maxGuests": max_guests,
            "nightlyRate": nightly_rate,
        }}
    })
    .to_string()
}

fn room_body(
    tenant: &str,
    request_id: &str,
    property_id: &str,
    room_type_id: &str,
    code: &str,
) -> String {
    json!({
        "tenant": tenant,
        "mutation": {"requestId": request_id, "command": {
            "operation": "room.create",
            "propertyId": property_id,
            "roomTypeId": room_type_id,
            "code": code,
        }}
    })
    .to_string()
}

fn set_active_body(
    tenant: &str,
    request_id: &str,
    property_id: &str,
    room_id: &str,
    expected_revision: i64,
    active: bool,
) -> String {
    json!({
        "tenant": tenant,
        "mutation": {"requestId": request_id, "command": {
            "operation": "room.set-active",
            "propertyId": property_id,
            "roomId": room_id,
            "expectedRevision": expected_revision,
            "active": active,
            "reason": "the availability lever",
        }}
    })
    .to_string()
}

fn housekeeping_body(
    tenant: &str,
    request_id: &str,
    property_id: &str,
    room_id: &str,
    expected_revision: i64,
    state: &str,
    reason: &str,
) -> String {
    json!({
        "tenant": tenant,
        "mutation": {"requestId": request_id, "command": {
            "operation": "room.housekeeping",
            "propertyId": property_id,
            "roomId": room_id,
            "expectedRevision": expected_revision,
            "state": state,
            "reason": reason,
        }}
    })
    .to_string()
}

#[allow(clippy::too_many_arguments)]
fn hold_body(
    tenant: &str,
    request_id: &str,
    property_id: &str,
    room_id: &str,
    arrival: &str,
    departure: &str,
    guests: i64,
    hold_seconds: Option<i64>,
) -> String {
    let mut command = json!({
        "operation": "stay.hold",
        "propertyId": property_id,
        "roomId": room_id,
        "arrival": arrival,
        "departure": departure,
        "guestName": "Dewi Anggraini",
        "guests": guests,
    });
    if let Some(hold_seconds) = hold_seconds {
        command["holdSeconds"] = json!(hold_seconds);
    }
    json!({"tenant": tenant, "mutation": {"requestId": request_id, "command": command}}).to_string()
}

fn stay_ref_body(
    tenant: &str,
    request_id: &str,
    operation: &str,
    property_id: &str,
    stay_id: &str,
    expected_revision: i64,
    reason: Option<&str>,
) -> String {
    let mut command = json!({
        "operation": operation,
        "propertyId": property_id,
        "id": stay_id,
        "expectedRevision": expected_revision,
    });
    if let Some(reason) = reason {
        command["reason"] = json!(reason);
    }
    json!({"tenant": tenant, "mutation": {"requestId": request_id, "command": command}}).to_string()
}

// --- Row readers ---------------------------------------------------------------

/// The stay row's cells the witnesses check — the state, the revision and
/// the two stamp instants as epoch milliseconds (NULL until stamped).
async fn stay_row(
    fixture: &Fixture,
    tenant: &str,
    id: &str,
) -> (String, i64, Option<i64>, Option<i64>) {
    let row: (String, i32, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT state,revision, \
         round(extract(epoch from checked_in_at)*1000)::bigint, \
         round(extract(epoch from checked_out_at)*1000)::bigint \
         FROM archava_accommodation.stays WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(tenant)
    .bind(id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the stay row answers");
    (row.0, i64::from(row.1), row.2, row.3)
}

/// The stay's resolved window as epoch milliseconds — the instantiated
/// edges the kernel slot binds.
async fn stay_instants(fixture: &Fixture, tenant: &str, id: &str) -> (i64, i64) {
    let row: (i64, i64) = sqlx::query_as(
        "SELECT round(extract(epoch from starts_at)*1000)::bigint, \
         round(extract(epoch from ends_at)*1000)::bigint \
         FROM archava_accommodation.stays WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(tenant)
    .bind(id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the stay instants answer");
    (row.0, row.1)
}

/// The occupancy slot's state (the kernel's own word for the stay).
async fn occupancy_state(fixture: &Fixture, tenant: &str, id: &str) -> Option<String> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT state FROM archava_scheduling.occupancy WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&fixture.admin_pool)
    .await
    .expect("the occupancy row answers");
    row.map(|(state,)| state)
}

/// The room row's cells.
async fn room_row(fixture: &Fixture, tenant: &str, id: &str) -> (bool, String, i64) {
    let row: (bool, String, i32) = sqlx::query_as(
        "SELECT active,housekeeping,revision \
         FROM archava_accommodation.rooms WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(tenant)
    .bind(id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the room row answers");
    (row.0, row.1, i64::from(row.2))
}

/// The shared resource's cells (the kernel's view of a room).
async fn resource_row(fixture: &Fixture, tenant: &str, id: &str) -> (bool, Option<String>) {
    sqlx::query_as(
        "SELECT active,managed_by FROM archava_scheduling.resources WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(tenant)
    .bind(id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the shared resource answers")
}

/// How many rows a tenant's accommodation table holds.
async fn count_of(fixture: &Fixture, table: &str, tenant: &str) -> i64 {
    let statement =
        format!("SELECT count(*)::bigint FROM archava_accommodation.{table} WHERE tenant_id=$1");
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
        "SELECT count(*)::bigint FROM archava_accommodation.commands \
         WHERE tenant_id=$1 AND request_id=$2::uuid",
    )
    .bind(tenant)
    .bind(request_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the command count answers");
    count
}

/// How many live kernel slots a room carries.
async fn slot_count(fixture: &Fixture, tenant: &str, resource_id: &str) -> i64 {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT count(*)::bigint FROM archava_scheduling.occupancy \
         WHERE tenant_id=$1 AND resource_id=$2::uuid AND state<>'cancelled'",
    )
    .bind(tenant)
    .bind(resource_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the slot count answers");
    count
}

/// The audit's operation list, one row per committed command.
async fn audit_operations(fixture: &Fixture, tenant: &str) -> Vec<String> {
    sqlx::query_as::<_, (String,)>(
        "SELECT operation FROM archava_accommodation.audit WHERE tenant_id=$1 \
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

// --- W1: the authorized life ----------------------------------------------------

/// The full lifecycle walks the kernel: the property's family builds, the
/// room's hold inserts the occupancy slot, confirm moves the kernel state,
/// check-in flips the room's housekeeping through the migrated vocabulary,
/// check-out dirties again through `dirty_after_stay`, and the audit
/// ledger named every operation once.
#[tokio::test]
async fn authorized_accommodation_life_commits_through_the_kernel() {
    let fixture = provision().await;
    seed(&fixture).await;
    // The provision checks (role, BOTH ledgers, BOTH forced-RLS widths)
    // pass on the fixture's runtime role — the serving state's support
    // check, asserted here because the suite holds the receipt of it.
    archava_adapters::erp::accommodation::assert_accommodation_kernel(
        &fixture.runtime_pool,
        &fixture.scheduling_ledger,
        &fixture.accommodation_ledger,
    )
    .await
    .expect("the accommodation kernel's provision checks pass");
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "ax").await;

    // 1. The property — the UTC zone with 00:00/00:00 daily times, so the
    //    hold window (today→tomorrow) IS the current UTC day and contains
    //    whatever instant the suite runs at.
    let body = property_body(
        ACME,
        KEY_PROPERTY,
        "Villa Nusantara Ubud",
        "UTC",
        Some("00:00"),
        Some("00:00"),
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let property_id = outcome["id"].as_str().expect("the property id").to_string();
    assert_eq!(outcome["kind"], "property");
    assert_eq!(outcome["propertyId"], property_id);
    assert_eq!(outcome["revision"], 1);
    // The record stores the daily times the command carried.
    let (check_in, check_out): (String, String) = sqlx::query_as(
        "SELECT check_in_time,check_out_time FROM archava_accommodation.properties \
         WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(ACME)
    .bind(&property_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the property row answers");
    assert_eq!(check_in, "00:00");
    assert_eq!(check_out, "00:00");

    // 2. The room type — the nightly quote the stay header carries.
    let body = room_type_body(ACME, KEY_TYPE, &property_id, "Garden villa", 2, 450_001);
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let room_type_id = outcome["id"]
        .as_str()
        .expect("the room type id")
        .to_string();
    assert_eq!(outcome["kind"], "room-type");

    // 3. The room — becomes a shared, accommodation-managed resource.
    let body = room_body(ACME, KEY_ROOM, &property_id, &room_type_id, "V-101");
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let room_id = outcome["id"].as_str().expect("the room id").to_string();
    assert_eq!(outcome["kind"], "room");
    let (resource_active, managed_by) = resource_row(&fixture, ACME, &room_id).await;
    assert_eq!(managed_by.as_deref(), Some("accommodation"));
    assert!(resource_active);
    // The room row was born dirty at revision 1.
    let (_, housekeeping, revision) = room_row(&fixture, ACME, &room_id).await;
    assert_eq!(housekeeping, "dirty");
    assert_eq!(revision, 1);

    // 4–5. The housekeeping walk: dirty → clean → inspected, one revision
    //       per move, the audit carrying the reason.
    let body = housekeeping_body(
        ACME,
        KEY_HK_CLEAN,
        &property_id,
        &room_id,
        1,
        "clean",
        "morning round",
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["revision"], 2);
    let body = housekeeping_body(
        ACME,
        KEY_HK_INSPECT,
        &property_id,
        &room_id,
        2,
        "inspected",
        "passed the checklist",
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["revision"], 3);
    let (_, housekeeping, revision) = room_row(&fixture, ACME, &room_id).await;
    assert_eq!(housekeeping, "inspected");
    assert_eq!(revision, 3);

    // 6. The hold — one night covering the current UTC day, inserted INTO
    //    the kernel.
    let arrival = date_offset(0);
    let departure = date_offset(1);
    let body = hold_body(
        ACME,
        KEY_HOLD,
        &property_id,
        &room_id,
        &arrival,
        &departure,
        2,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let stay_id = outcome["id"].as_str().expect("the stay id").to_string();
    assert_eq!(outcome["kind"], "stay");
    assert_eq!(outcome["propertyId"], property_id);
    assert_eq!(outcome["revision"], 1);
    let (state, revision, _, _) = stay_row(&fixture, ACME, &stay_id).await;
    assert_eq!(state, "held");
    assert_eq!(revision, 1);
    assert_eq!(
        occupancy_state(&fixture, ACME, &stay_id).await.as_deref(),
        Some("held"),
        "the kernel slot rides the hold"
    );
    // The quote: nights, rate, rate revision, total.
    let (nights, nightly_rate, rate_revision, total): (i32, i32, i32, i64) = sqlx::query_as(
        "SELECT nights,nightly_rate,rate_revision,total_estimate \
         FROM archava_accommodation.stays WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(ACME)
    .bind(&stay_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the stay quote answers");
    assert_eq!(nights, 1);
    assert_eq!(nightly_rate, 450_001);
    assert_eq!(rate_revision, 1);
    assert_eq!(total, 450_001);
    let (night_count,): (i64,) = sqlx::query_as(
        "SELECT count(*)::bigint FROM archava_accommodation.stay_nights \
         WHERE tenant_id=$1 AND stay_id=$2::uuid",
    )
    .bind(ACME)
    .bind(&stay_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the night quotes answer");
    assert_eq!(night_count, 1);
    // The resolved window: `00:00` daily times in the UTC zone translate
    // the calendar days to whole UTC days — the window IS the current UTC
    // day, which is what makes the lifecycle gates time-of-day-invariant.
    let (starts_ms, ends_ms) = stay_instants(&fixture, ACME, &stay_id).await;
    assert_eq!(
        (starts_ms, ends_ms),
        (
            epoch_days_now() * 86_400_000,
            (epoch_days_now() + 1) * 86_400_000
        ),
        "00:00 daily times in the UTC zone open the window at the day boundary"
    );

    // 7. Confirm — the kernel flips to confirmed, the identity cells stay.
    let body = stay_ref_body(
        ACME,
        KEY_CONFIRM,
        "stay.confirm",
        &property_id,
        &stay_id,
        1,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["revision"], 2);
    assert_eq!(
        occupancy_state(&fixture, ACME, &stay_id).await.as_deref(),
        Some("confirmed")
    );
    let (state, revision, _, _) = stay_row(&fixture, ACME, &stay_id).await;
    assert_eq!(state, "confirmed");
    assert_eq!(revision, 2);

    // 8. Check-in — the room is INSPECTED, the stay window is now (the
    //    00:00 daily times make it so whatever the wall clock says); the
    //    `dirty_after_stay` trigger dirties the room and bumps its
    //    revision through that side write.
    let body = stay_ref_body(
        ACME,
        KEY_CHECKIN,
        "stay.check-in",
        &property_id,
        &stay_id,
        2,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["revision"], 3);
    let (room_active, room_housekeeping, room_revision) = room_row(&fixture, ACME, &room_id).await;
    assert!(room_active);
    assert_eq!(room_housekeeping, "dirty", "check-in dirties the room");
    assert_eq!(room_revision, 4, "the trigger's side write bumps ONCE");
    let (state, revision, checked_in, _) = stay_row(&fixture, ACME, &stay_id).await;
    assert_eq!(state, "in_house");
    assert_eq!(revision, 3);
    assert!(checked_in.is_some(), "check-in stamps the instant");
    // The kernel state stays `confirmed` — occupancy's vocabulary has no
    // in-house word; the stays row carries the lifecycle.
    assert_eq!(
        occupancy_state(&fixture, ACME, &stay_id).await.as_deref(),
        Some("confirmed")
    );

    // 9. Check-out — the terminal stay, the kernel slot cancels, the room
    //    dirties again through the same trigger.
    let body = stay_ref_body(
        ACME,
        KEY_CHECKOUT,
        "stay.check-out",
        &property_id,
        &stay_id,
        3,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["revision"], 4);
    let (_, room_housekeeping, room_revision) = room_row(&fixture, ACME, &room_id).await;
    assert_eq!(room_housekeeping, "dirty", "check-out dirties the room");
    assert_eq!(room_revision, 5, "the trigger's side write bumps ONCE");
    let (state, revision, checked_in, checked_out) = stay_row(&fixture, ACME, &stay_id).await;
    assert_eq!(state, "checked_out");
    assert_eq!(revision, 4);
    assert!(checked_in.is_some());
    assert!(checked_out.is_some());
    assert_eq!(
        occupancy_state(&fixture, ACME, &stay_id).await.as_deref(),
        Some("cancelled")
    );

    // 10. The status read answers the TS commandStatus document.
    let (status, outcome) = get(
        &app,
        &format!("/erp/accommodation/commands/status?tenant={ACME}&requestId={KEY_CHECKIN}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["status"], "committed");
    assert_eq!(outcome["requestId"], KEY_CHECKIN);
    assert_eq!(outcome["receipt"]["kind"], "stay");
    assert_eq!(outcome["receipt"]["revision"], 3);
    let (status, outcome) = get(
        &app,
        &format!("/erp/accommodation/commands/status?tenant={ACME}&requestId={KEY_PROPERTY}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["receipt"]["kind"], "property");

    // 11. The audit ledger named every operation once — one row per
    //     committed command, the reasons riding theirs.
    assert_eq!(
        audit_operations(&fixture, ACME).await,
        vec![
            "property.create",
            "room-type.create",
            "room.create",
            "room.housekeeping",
            "room.housekeeping",
            "stay.hold",
            "stay.confirm",
            "stay.check-in",
            "stay.check-out",
        ]
    );
    let (reason,): (Option<String>,) = sqlx::query_as(
        "SELECT reason FROM archava_accommodation.audit \
         WHERE tenant_id=$1 AND operation='room.housekeeping' AND record_id=$2::uuid",
    )
    .bind(ACME)
    .bind(&room_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the audit reasons answer");
    assert_eq!(reason.as_deref(), Some("passed the checklist"));

    // 12. The identity cells — the stay and the property own the acting
    //     human.
    let (stay_owner,): (String,) = sqlx::query_as(
        "SELECT owner_id FROM archava_accommodation.stays WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(ACME)
    .bind(&stay_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the stay owner answers");
    assert_eq!(stay_owner, "ax");
    let (property_owner,): (String,) = sqlx::query_as(
        "SELECT owner_id FROM archava_accommodation.properties WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(ACME)
    .bind(&property_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the property owner answers");
    assert_eq!(property_owner, "ax");
}

// --- W2: the calendar resolution ------------------------------------------------

/// DST wall gaps and fall-back repeats are the `CalendarTimeError`
/// verdict — a 409 conflict that records NOTHING — while a normal zone
/// resolves the exact instant bytes the property's timezone owns, both
/// through the recorded SQL-tzdata divergence.
#[tokio::test]
async fn dst_wall_gaps_conflict_and_normal_zones_resolve_bytes_exactly() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "ax").await;

    // The Amsterdam property checks guests in at 02:30 LOCAL — the daily
    // time the grammar accepts and the zone's spring-forward gap reuses.
    let body = property_body(
        ACME,
        KEY_EU_PROPERTY,
        "Hotel Amsterdam",
        "Europe/Amsterdam",
        Some("02:30"),
        Some("02:30"),
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let eu_property = outcome["id"].as_str().expect("the property id").to_string();
    let body = room_type_body(ACME, KEY_EU_TYPE, &eu_property, "Canal twin", 2, 190_000);
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let eu_type = outcome["id"].as_str().expect("the type id").to_string();
    let body = room_body(ACME, KEY_EU_ROOM, &eu_property, &eu_type, "A-201");
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let eu_room = outcome["id"].as_str().expect("the room id").to_string();

    // The spring-forward gap: 2026-03-29 02:30 does not exist in
    // Europe/Amsterdam — the resolve refuses as a conflict and NOTHING
    // records.
    let body = hold_body(
        ACME,
        KEY_GAP,
        &eu_property,
        &eu_room,
        "2026-03-29",
        "2026-03-31",
        1,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{outcome}");
    assert_eq!(outcome["reason"], "conflict");
    assert_eq!(count_of(&fixture, "stays", ACME).await, 0);
    assert_eq!(slot_count(&fixture, ACME, &eu_room).await, 0);
    assert_eq!(command_count(&fixture, ACME, KEY_GAP).await, 0);

    // The fall-back repeat: 2026-10-25 02:30 happens twice — ambiguous,
    // conflict, still nothing recorded.
    let body = hold_body(
        ACME,
        KEY_AMBIGUOUS,
        &eu_property,
        &eu_room,
        "2026-10-25",
        "2026-10-27",
        1,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{outcome}");
    assert_eq!(count_of(&fixture, "stays", ACME).await, 0);
    assert_eq!(command_count(&fixture, ACME, KEY_AMBIGUOUS).await, 0);

    // The Makassar property with 00:00 daily times in a fixed UTC+8 zone
    // resolves the records exactly — no DST anywhere near.
    let body = property_body(
        ACME,
        KEY_MD_PROPERTY,
        "Hotel Makassar",
        "Asia/Makassar",
        Some("00:00"),
        Some("00:00"),
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let md_property = outcome["id"].as_str().expect("the property id").to_string();
    let body = room_type_body(ACME, KEY_MD_TYPE, &md_property, "Sea twin", 2, 210_000);
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let md_type = outcome["id"].as_str().expect("the type id").to_string();
    let body = room_body(ACME, KEY_MD_ROOM, &md_property, &md_type, "M-301");
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let md_room = outcome["id"].as_str().expect("the room id").to_string();
    let arrival = date_offset(3);
    let departure = date_offset(5);
    let body = hold_body(
        ACME,
        KEY_BRIGHT,
        &md_property,
        &md_room,
        &arrival,
        &departure,
        1,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let stay_id = outcome["id"].as_str().expect("the stay id").to_string();
    // The resolved bytes: (departure−2 ...) the arrival's 00:00 in
    // UTC+8 is 08:00 UTC of the same civil day; departure likewise.
    let (starts_ms, ends_ms) = stay_instants(&fixture, ACME, &stay_id).await;
    assert_eq!(
        (starts_ms, ends_ms),
        (
            (epoch_days_now() + 3) * 86_400_000 - 28_800_000,
            (epoch_days_now() + 5) * 86_400_000 - 28_800_000
        ),
        "00:00 WITA opens at 16:00 UTC on the eve of the arrival civil day and closes at 16:00 UTC on the eve of the departure day"
    );
}

// --- W3: the kernel overlap ------------------------------------------------------

/// A stay window overlapping a live slot on the SAME room is the kernel's
/// overlap verdict (23P01 → conflict): the whole submission rolls back,
/// keeping exactly one stay and one live slot on the room, while a
/// DISJOINT window on the same room records its own slot.
#[tokio::test]
async fn overlapping_hold_refuses_and_holds_nothing() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "ax").await;

    let body = property_body(
        ACME,
        KEY_OVERLAP_PROPERTY,
        "Pondok Wisata",
        "UTC",
        Some("00:00"),
        Some("00:00"),
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let property_id = outcome["id"].as_str().unwrap().to_string();
    let body = room_type_body(ACME, KEY_OVERLAP_TYPE, &property_id, "Bungalow", 2, 150_000);
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let room_type_id = outcome["id"].as_str().unwrap().to_string();
    let body = room_body(ACME, KEY_OVERLAP_ROOM, &property_id, &room_type_id, "B-1");
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let room_id = outcome["id"].as_str().unwrap().to_string();

    // The live hold around today.
    let arrival = date_offset(0);
    let departure = date_offset(2);
    let body = hold_body(
        ACME,
        KEY_OVERLAP_A,
        &property_id,
        &room_id,
        &arrival,
        &departure,
        1,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let hold_a = outcome["id"].as_str().unwrap().to_string();

    // The overlapping hold: its window contains the live one's body.
    let later = date_offset(1);
    let far = date_offset(3);
    let body = hold_body(
        ACME,
        KEY_OVERLAP_B,
        &property_id,
        &room_id,
        &later,
        &far,
        1,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{outcome}");
    // NOTHING recorded: one stay, one live slot, no claim row, the room
    // revision untouched.
    assert_eq!(count_of(&fixture, "stays", ACME).await, 1);
    assert_eq!(slot_count(&fixture, ACME, &room_id).await, 1);
    assert_eq!(command_count(&fixture, ACME, KEY_OVERLAP_B).await, 0);
    let (_, _, revision) = room_row(&fixture, ACME, &room_id).await;
    assert_eq!(revision, 1);

    // A DISJOINT window on the same room goes through — the kernel
    // refuses intersections, not the room itself.
    let far_departure = date_offset(4);
    let body = hold_body(
        ACME,
        KEY_OVERLAP_FREE,
        &property_id,
        &room_id,
        &departure,
        &far_departure,
        1,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(slot_count(&fixture, ACME, &room_id).await, 2);
    let _ = hold_a;
}

// --- W4: the housekeeping vocabulary ----------------------------------------------

/// The housekeeping vocabulary refuses the illegal transitions, writes
/// under physical occupancy and deactivation under occupancy; an inactive
/// room refuses holds; the legal walk bumps exactly once per move.
#[tokio::test]
async fn housekeeping_vocabulary_and_occupancy_refusals_hold_the_room() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "ax").await;

    // The family: one property, one room type, TWO rooms.
    let body = property_body(
        ACME,
        KEY_HK_PROPERTY,
        "Hotel Vakansi",
        "UTC",
        Some("00:00"),
        Some("00:00"),
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let property_id = outcome["id"].as_str().unwrap().to_string();
    let body = room_type_body(ACME, KEY_HK_TYPE, &property_id, "Standard", 2, 220_000);
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let room_type_id = outcome["id"].as_str().unwrap().to_string();
    let body = room_body(ACME, KEY_HK_ROOM1, &property_id, &room_type_id, "W-101");
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let room1 = outcome["id"].as_str().unwrap().to_string();
    let body = room_body(ACME, KEY_HK_ROOM2, &property_id, &room_type_id, "W-102");
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let room2 = outcome["id"].as_str().unwrap().to_string();

    // ROOM1: make it inspect-ready, then take a guest in.
    let body = housekeeping_body(
        ACME,
        KEY_HK_CLEAN1,
        &property_id,
        &room1,
        1,
        "clean",
        "morning",
    );
    let (status, _) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body = housekeeping_body(
        ACME,
        KEY_HK_INSPECT1,
        &property_id,
        &room1,
        2,
        "inspected",
        "checked",
    );
    let (status, _) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let arrival = date_offset(0);
    let departure = date_offset(1);
    let body = hold_body(
        ACME,
        KEY_HK_HOLD,
        &property_id,
        &room1,
        &arrival,
        &departure,
        1,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let stay1 = outcome["id"].as_str().unwrap().to_string();
    let body = stay_ref_body(
        ACME,
        KEY_HK_CONFIRM,
        "stay.confirm",
        &property_id,
        &stay1,
        1,
        None,
    );
    let (status, _) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body = stay_ref_body(
        ACME,
        KEY_HK_CHECKIN,
        "stay.check-in",
        &property_id,
        &stay1,
        2,
        None,
    );
    let (status, _) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, _, room1_revision) = room_row(&fixture, ACME, &room1).await;

    // Physically occupied: houseking and deactivation both refuse with
    // the room untouched.
    let body = housekeeping_body(
        ACME,
        KEY_HK_BLOCKED,
        &property_id,
        &room1,
        room1_revision,
        "clean",
        "the guest just left",
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{outcome}");
    assert_eq!(
        room_row(&fixture, ACME, &room1).await,
        (true, "dirty".to_string(), room1_revision)
    );
    let body = set_active_body(
        ACME,
        KEY_HK_DEACT,
        &property_id,
        &room1,
        room1_revision,
        false,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{outcome}");
    assert_eq!(
        room_row(&fixture, ACME, &room1).await,
        (true, "dirty".to_string(), room1_revision)
    );

    // ROOM2 walks the FULL vocabulary: the created row is dirty.
    let body = housekeeping_body(
        ACME,
        KEY_HK_BAD_DIRTY_INSPECT,
        &property_id,
        &room2,
        1,
        "inspected",
        "skipped clean",
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{outcome}");
    let body = housekeeping_body(
        ACME,
        KEY_HK_BAD_SAME_DIRTY,
        &property_id,
        &room2,
        1,
        "dirty",
        "already dirty",
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{outcome}");
    let body = housekeeping_body(
        ACME,
        KEY_HK_CLEAN2,
        &property_id,
        &room2,
        1,
        "clean",
        "second round",
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let body = housekeeping_body(
        ACME,
        KEY_HK_BAD_SAME_CLEAN,
        &property_id,
        &room2,
        2,
        "clean",
        "already clean",
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{outcome}");
    let body = housekeeping_body(
        ACME,
        KEY_HK_INSPECT2,
        &property_id,
        &room2,
        2,
        "inspected",
        "checked",
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let body = housekeeping_body(
        ACME,
        KEY_HK_BAD_INSPECT_CLEAN,
        &property_id,
        &room2,
        3,
        "clean",
        "back to clean",
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{outcome}");
    let body = housekeeping_body(
        ACME,
        KEY_HK_DIRTY2,
        &property_id,
        &room2,
        3,
        "dirty",
        "mark for redo",
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let (_, housekeeping, room2_revision) = room_row(&fixture, ACME, &room2).await;
    assert_eq!(housekeeping, "dirty");
    assert_eq!(
        room2_revision, 4,
        "dirty started at 1: clean, inspect, dirty"
    );

    // An inactive room refuses holds: deactivate, then try.
    let body = set_active_body(
        ACME,
        KEY_HK_DEACT_DONE,
        &property_id,
        &room2,
        room2_revision,
        false,
    );
    let (status, _) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body = hold_body(
        ACME,
        KEY_HK_INACTIVE_HOLD,
        &property_id,
        &room2,
        &arrival,
        &departure,
        1,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{outcome}");
    assert_eq!(
        count_of(&fixture, "stays", ACME).await,
        1,
        "only room1's stay exists"
    );
    // The kernel slot: room2 got NO slot — but the room's SHARED resource
    // flag flipped WITH the room's.
    let (resource_active, _) = resource_row(&fixture, ACME, &room2).await;
    assert!(!resource_active);
}

// --- W5: the revoked grant --------------------------------------------------------

/// A revoked grant refuses the recovery: the same key + bytes re-submits
/// to a DEAD membership and both the mutation and the status read answer
/// 403 — yesterday's receipts are not replayable through revoked access.
#[tokio::test]
async fn revoked_grant_refuses_the_recovery() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "axr").await;

    // Build a small family and hold one stay, all as `axr`.
    let body = property_body(
        ACME,
        KEY_REVOKED_HOLD,
        "Penginapan Sejahtera",
        "UTC",
        Some("00:00"),
        Some("00:00"),
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let property_id = outcome["id"].as_str().unwrap().to_string();
    let body = room_type_body(ACME, KEY_ROOM, &property_id, "Family", 3, 320_000);
    let (status, _) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (room_type_id,): (String,) = sqlx::query_as(
        "SELECT id::text FROM archava_accommodation.room_types WHERE tenant_id=$1 AND property_id=$2::uuid",
    )
    .bind(ACME)
    .bind(&property_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the room type answers");
    let body = room_body(ACME, KEY_CHECKOUT, &property_id, &room_type_id, "F-1");
    let (status, _) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (room_id,): (String,) = sqlx::query_as(
        "SELECT id::text FROM archava_accommodation.rooms WHERE tenant_id=$1 AND property_id=$2::uuid",
    )
    .bind(ACME)
    .bind(&property_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the room answers");
    let arrival = date_offset(1);
    let departure = date_offset(3);
    let body = hold_body(
        ACME,
        KEY_HOLD,
        &property_id,
        &room_id,
        &arrival,
        &departure,
        2,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let stay_id = outcome["id"].as_str().unwrap().to_string();
    assert_eq!(stay_row(&fixture, ACME, &stay_id).await.0, "held");

    // Revoke the membership out from under the caller.
    sqlx::query(
        "UPDATE archava_accommodation.memberships SET active=false \
         WHERE tenant_id=$1 AND principal_id='axr'",
    )
    .bind(ACME)
    .execute(&fixture.admin_pool)
    .await
    .expect("revoke the membership");

    // The SAME submission recovered via re-post answers 403 and records
    // nothing new.
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{outcome}");
    let (stay_state, revision, _, _) = stay_row(&fixture, ACME, &stay_id).await;
    assert_eq!(stay_state, "held");
    assert_eq!(revision, 1);

    // A NEW request ids the same refusal — the context is dead.
    let fresh = hold_body(
        ACME,
        KEY_REVOKED_AGAIN,
        &property_id,
        &room_id,
        &arrival,
        &departure,
        2,
        None,
    );
    let (status, _) = post(
        &app,
        "/erp/accommodation/commands",
        &fresh,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // The stored receipt is unrecoverable through the status read.
    let (status, outcome) = get(
        &app,
        &format!("/erp/accommodation/commands/status?tenant={ACME}&requestId={KEY_HOLD}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{outcome}");
}

// --- W6: the idempotency reuse ----------------------------------------------------

/// The same idempotency key with a DIFFERENT command is a conflict that
/// records nothing — the first command's committed effect stays the only
/// one.
#[tokio::test]
async fn idempotent_reuse_conflicts_without_recording() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "ax").await;

    let first = property_body(ACME, KEY_REUSE, "Villa Satu", "Asia/Makassar", None, None);
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &first,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let second = property_body(ACME, KEY_REUSE, "Villa Dua", "Asia/Makassar", None, None);
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &second,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{outcome}");
    assert_eq!(count_of(&fixture, "properties", ACME).await, 1);
    let (title,): (String,) =
        sqlx::query_as("SELECT title FROM archava_accommodation.properties WHERE tenant_id=$1")
            .bind(ACME)
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("the property answers");
    assert_eq!(title, "Villa Satu");
    assert_eq!(audit_operations(&fixture, ACME).await.len(), 1);
}

// --- W7: the replay ----------------------------------------------------------------

/// A lost response re-submits the SAME key + bytes and answers the SAME
/// receipt bytes with still exactly one stay and one kernel slot — the
/// replay branch walks the receipt recovery's re-authorization.
#[tokio::test]
async fn replay_answers_the_same_receipt_without_a_second_stay() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "ax").await;

    let body = property_body(
        ACME,
        KEY_REPLAY,
        "Wisma Sejuk",
        "UTC",
        Some("00:00"),
        Some("00:00"),
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let first = outcome;
    // The identical submission: the SAME receipt bytes come back.
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome, first, "the replay is wire-identical");
    assert_eq!(count_of(&fixture, "properties", ACME).await, 1);
    assert_eq!(audit_operations(&fixture, ACME).await.len(), 1);

    // The same through a stay hold: the hold's effect stays singular.
    let (property_id,): (String,) =
        sqlx::query_as("SELECT id::text FROM archava_accommodation.properties WHERE tenant_id=$1")
            .bind(ACME)
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("the property answers");
    let type_body = room_type_body(ACME, KEY_TYPE, &property_id, "Twin", 2, 180_000);
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &type_body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let room_type_id = outcome["id"].as_str().unwrap().to_string();
    let room = room_body(ACME, KEY_ROOM, &property_id, &room_type_id, "S-7");
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &room,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let room_id = outcome["id"].as_str().unwrap().to_string();
    let arrival = date_offset(2);
    let departure = date_offset(3);
    let hold = hold_body(
        ACME,
        KEY_HOLD,
        &property_id,
        &room_id,
        &arrival,
        &departure,
        1,
        None,
    );
    let (status, receipt) = post(
        &app,
        "/erp/accommodation/commands",
        &hold,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{receipt}");
    let (status, replayed) = post(
        &app,
        "/erp/accommodation/commands",
        &hold,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replayed}");
    assert_eq!(receipt, replayed, "the hold replay is wire-identical");
    assert_eq!(count_of(&fixture, "stays", ACME).await, 1);
    assert_eq!(slot_count(&fixture, ACME, &room_id).await, 1);
}

// --- W8: the cross-tenant refusal ---------------------------------------------------

/// A caller whose membership lives in GLOBEX is refused on ACME BEFORE any
/// business statement runs (no property, no command row), while the SAME
/// caller submits freely into its own ledger.
#[tokio::test]
async fn cross_tenant_callers_refuse_before_any_statement() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let stranger = login(&app, "other").await;

    // ACME from GLOBEX: 403, zero rows.
    let body = property_body(ACME, KEY_CROSS, "Intrusion", "Asia/Makassar", None, None);
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&stranger),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{outcome}");
    assert_eq!(count_of(&fixture, "properties", ACME).await, 0);
    assert_eq!(command_count(&fixture, ACME, KEY_CROSS).await, 0);
    // GLOBEX from ACME: symmetrical.
    let local = login(&app, "ax").await;
    let body = property_body(GLOBEX, KEY_CROSS, "Intrusion", "Asia/Makassar", None, None);
    let (status, _) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&local),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(count_of(&fixture, "properties", GLOBEX).await, 0);

    // The stranger acts freely in its OWN ledger.
    let body = property_body(
        GLOBEX,
        KEY_GLOBEX,
        "Globex Ville",
        "Asia/Makassar",
        None,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&stranger),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let (owner,): (String,) =
        sqlx::query_as("SELECT owner_id FROM archava_accommodation.properties WHERE tenant_id=$1")
            .bind(GLOBEX)
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("the globex property answers");
    assert_eq!(owner, "other");
    assert_eq!(count_of(&fixture, "properties", ACME).await, 0);
}

// --- W9: the narrow grants -----------------------------------------------------------

/// Narrow grants refuse exactly the arms whose gate they lack — the
/// missing `rooms.write` refuses room-type/room creation, the missing
/// housekeeping grants refuse housekeeping — while the stay arms (the
/// grants the actor DOES hold) walk, and the audit-only actor is refused
/// before any statement.
#[tokio::test]
async fn narrow_grants_refuse_exactly_the_arms_they_lack() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let full = login(&app, "ax").await;
    let narrow = login(&app, "axg").await;
    let nongrant = login(&app, "axn").await;

    // `ax` builds the family `axg` will walk.
    let body = property_body(
        ACME,
        KEY_PROPERTY,
        "Hotel Bersama",
        "UTC",
        Some("00:00"),
        Some("00:00"),
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&full),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let property_id = outcome["id"].as_str().unwrap().to_string();
    let body = room_type_body(ACME, KEY_TYPE, &property_id, "Shared twin", 2, 160_000);
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&full),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let room_type_id = outcome["id"].as_str().unwrap().to_string();
    let body = room_body(ACME, KEY_ROOM, &property_id, &room_type_id, "X-1");
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&full),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let room_id = outcome["id"].as_str().unwrap().to_string();

    // `axg` lacks `rooms.write`: room-type creation refuses, no row.
    let body = room_type_body(ACME, KEY_NG_TYPE, &property_id, "Narrow twin", 2, 111_000);
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&narrow),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{outcome}");
    assert_eq!(count_of(&fixture, "room_types", ACME).await, 1);
    assert_eq!(command_count(&fixture, ACME, KEY_NG_TYPE).await, 0);
    // ...and lacks BOTH housekeeping/inspect grants.
    let body = housekeeping_body(
        ACME,
        KEY_NG_HK,
        &property_id,
        &room_id,
        1,
        "clean",
        "the narrow round",
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&narrow),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{outcome}");
    assert_eq!(command_count(&fixture, ACME, KEY_NG_HK).await, 0);
    let (_, housekeeping, _) = room_row(&fixture, ACME, &room_id).await;
    assert_eq!(housekeeping, "dirty");

    // ...but the stay arms (whose grants the narrow actor DOES hold) walk:
    // hold,
    let arrival = date_offset(0);
    let departure = date_offset(1);
    let body = hold_body(
        ACME,
        KEY_NG_HOLD,
        &property_id,
        &room_id,
        &arrival,
        &departure,
        1,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&narrow),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let stay_id = outcome["id"].as_str().unwrap().to_string();
    assert_eq!(outcome["revision"], 1);
    // ...cancel (the arm the narrow actor holds the essentials of; the
    // owner matches its own access owner).
    let body = stay_ref_body(
        ACME,
        KEY_NG_CANCEL,
        "stay.cancel",
        &property_id,
        &stay_id,
        1,
        Some("the narrow change of plans"),
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&narrow),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["revision"], 2);
    let (stay_state, _, _, _) = stay_row(&fixture, ACME, &stay_id).await;
    assert_eq!(stay_state, "cancelled");
    assert_eq!(
        occupancy_state(&fixture, ACME, &stay_id).await.as_deref(),
        Some("cancelled")
    );

    // The audit-only actor: every gate refuses; no command row at all.
    let body = property_body(ACME, KEY_NONGRANT, "Nones", "Asia/Makassar", None, None);
    let (status, _) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&nongrant),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(command_count(&fixture, ACME, KEY_NONGRANT).await, 0);
    assert_eq!(count_of(&fixture, "properties", ACME).await, 1);
}

// --- W10: the window/capacity/boundary gates ------------------------------------------

/// The hold gates refuse where the TS refuses: a past arrival, a capacity
/// overflow, impossible night spans (400 at the wire), the hold cap (400)
/// and the property's daily order — and the free space beneath them all
/// holds fine.
#[tokio::test]
async fn window_and_capacity_gates_refuse_while_free_space_holds() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "ax").await;

    let body = property_body(
        ACME,
        KEY_PROPERTY,
        "Pantai Sebelas",
        "UTC",
        Some("00:00"),
        Some("00:00"),
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let property_id = outcome["id"].as_str().unwrap().to_string();
    let body = room_type_body(ACME, KEY_TYPE, &property_id, "Twin", 2, 140_000);
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let room_type_id = outcome["id"].as_str().unwrap().to_string();
    let body = room_body(ACME, KEY_ROOM, &property_id, &room_type_id, "P-7");
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let room_id = outcome["id"].as_str().unwrap().to_string();

    // The daily order refuses at the WIRE (the property.create refine).
    let body = property_body(
        ACME,
        KEY_CHECKIN,
        "Late Checkout",
        "Asia/Makassar",
        Some("15:00"),
        Some("18:30"),
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{outcome}");
    assert_eq!(count_of(&fixture, "properties", ACME).await, 1);

    // Zero nights (same-day) is a parse refine → 400 with no rows.
    let arrival = date_offset(0);
    let body = hold_body(
        ACME,
        KEY_CHECKOUT,
        &property_id,
        &room_id,
        &arrival,
        &arrival,
        1,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{outcome}");
    assert_eq!(command_count(&fixture, ACME, KEY_CHECKOUT).await, 0);
    assert_eq!(count_of(&fixture, "stays", ACME).await, 0);

    // Thirty-two nights is also the refine's window → 400.
    let too_long = date_offset(32);
    let body = hold_body(
        ACME,
        KEY_LONG,
        &property_id,
        &room_id,
        &arrival,
        &too_long,
        1,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{outcome}");
    assert_eq!(count_of(&fixture, "stays", ACME).await, 0);
    // The hold cap at 1800 is parse boundary too.
    let day_after = date_offset(1);
    let body = hold_body(
        ACME,
        KEY_LONG,
        &property_id,
        &room_id,
        &arrival,
        &day_after,
        1,
        Some(1801),
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{outcome}");

    // A past arrival is the hold gate's own 409.
    let yesterday = date_offset(-2);
    let back_then = date_offset(0);
    let body = hold_body(
        ACME,
        KEY_PAST,
        &property_id,
        &room_id,
        &yesterday,
        &back_then,
        1,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{outcome}");
    assert_eq!(command_count(&fixture, ACME, KEY_PAST).await, 0);

    // Capacity: max 2 guests, 3 asked — the hold gate's 409.
    let tomorrow = date_offset(2);
    let after = date_offset(3);
    let body = hold_body(
        ACME,
        KEY_CROWDED,
        &property_id,
        &room_id,
        &tomorrow,
        &after,
        3,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{outcome}");
    assert_eq!(count_of(&fixture, "stays", ACME).await, 0);

    // Beneath every gate the free holding walks: one guest, one night,
    // tomorrow.
    let body = hold_body(
        ACME,
        KEY_FREEHOLD,
        &property_id,
        &room_id,
        &tomorrow,
        &after,
        1,
        None,
    );
    let (status, outcome) = post(
        &app,
        "/erp/accommodation/commands",
        &body,
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let stay_id = outcome["id"].as_str().unwrap().to_string();
    assert_eq!(stay_row(&fixture, ACME, &stay_id).await.0, "held");
    assert_eq!(slot_count(&fixture, ACME, &room_id).await, 1);

    // An unknown request id answers `not_committed` — never a fabricated
    // receipt.
    let (status, outcome) = get(
        &app,
        &format!("/erp/accommodation/commands/status?tenant={ACME}&requestId={KEY_UNKNOWN}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["status"], "not_committed");
    assert_eq!(outcome["requestId"], KEY_UNKNOWN);
    assert!(
        outcome.get("receipt").is_none(),
        "no receipt for a not-committed read"
    );
}
