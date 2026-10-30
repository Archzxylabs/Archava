//! The R07 attendance cohort's host witnesses — driven through the real
//! router (`tower::ServiceExt::oneshot`) against the disposable PostgreSQL
//! whose `archava_attendance` schema is the real migration ledger
//! (`001_attendance_sessions_rosters`). Every assertion is a fact about what
//! the SERVER did:
//!
//! - the authorized session/roster life ends with exactly the rows the
//!   repository's SQL spec freezes — the receipt documents, the session's
//!   `enrolled_count`/`attended_count` chain, the roster's status and
//!   revision, the check-in event and the audit naming every operation once;
//!   the READ projections answer the same rows;
//! - **a roster from another session cannot check in** (audit A04, RN-ERP-04):
//!   the roster is bound to the supplied session in the row SELECT and the
//!   pure policy, so a valid UUID from session B is 404 under session A and
//!   no counter moves;
//! - the seat boundary refuses atomically, concurrent registrations for the
//!   last seat produce exactly one roster, and concurrent check-ins record
//!   exactly one attendance;
//! - a duplicate registration/ticket is a conflict, not a second row;
//! - the same check-in key under a different command is
//!   `already_checked_in` — never a raw SQLSTATE — and the second attempt
//!   records nothing;
//! - a closed session refuses registration and check-in with the 400 class;
//! - a stale revision conflicts and records nothing;
//! - the same command id with a different payload conflicts; the same
//!   command id with the same bytes replays with `idempotencyReplay: true`
//!   and still one row everywhere;
//! - a cross-tenant caller is refused before any statement runs;
//! - a revoked membership loses both the write and the status read, and the
//!   restored grant replays the stored receipt; and
//! - the wire boundary (unknown arm, extra member, reversed window, zero
//!   capacity, list bounds, unknown status) refuses at the right layer, and
//!   the read projections are bounded and scoped.
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
/// list every ERP suite pins; the attendance surface RIDES the same serving
/// state without touching commerce rows.
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
const IDENTITY_KEY: &[u8] = &[31u8; 32];

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
/// and the purchasing ledger (`try_new_erp`'s second pass) — the attendance
/// surface rides the same serving state; its own ledger rows exist in the
/// fixture regardless, and the suite's FIRST step checks the attendance
/// ledger's own provision facts (`assert_attendance_kernel`) before any
/// route is mounted.
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
const ACME: &str = "acme-events";
const GLOBEX: &str = "globex-events";

/// The attendance permission vocabulary (migration 001's CHECK).
const ATTENDANCE_ALL: &[&str] = &[
    "attendance.read",
    "attendance.write",
    "attendance.checkin",
    "audit.read",
];

/// `ax` is the full-grant manager with scope all — the positive-path actor.
/// `axg` holds read + audit only: the write/check-in arms refuse, while its
/// reads walk. `axn` holds only `audit.read` — every read gate refuses, the
/// status read included. `axv` holds the full grants, and the recovery test
/// withdraws only its `attendance.read` grant while the seat stays active.
/// `axa` holds the full grants with scope
/// `assigned` — only its own sessions are visible. `other` holds full grants
/// but ONLY in GLOBEX.
async fn seed(fixture: &Fixture) {
    for principal in ["ax", "axg", "axn", "axv", "axa", "other"] {
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

    for (principal, permissions, scope) in [
        ("ax", ATTENDANCE_ALL, "all"),
        ("axg", &["attendance.read", "audit.read"][..], "all"),
        ("axn", &["audit.read"][..], "all"),
        ("axv", ATTENDANCE_ALL, "all"),
        ("axa", ATTENDANCE_ALL, "assigned"),
    ] {
        sqlx::query(
            "INSERT INTO archava_attendance.memberships \
             (tenant_id, principal_id, kind, permissions, scope, active) \
             VALUES ($1, $2, 'human', $3, $4, true)",
        )
        .bind(ACME)
        .bind(principal)
        .bind(permissions.to_vec())
        .bind(scope)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the ACME attendance membership");
    }
    sqlx::query(
        "INSERT INTO archava_attendance.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(GLOBEX)
    .bind("other")
    .bind(ATTENDANCE_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the GLOBEX attendance membership");
}

// --- The window and contacts ---------------------------------------------------

/// The session window every witness opens inside — a fixed future slot, so
/// the suite never depends on the wall clock.
const START: &str = "2026-11-01T09:00:00.000Z";
const END: &str = "2026-11-01T17:00:00.000Z";

/// The contacts the rosters hang off — opaque references, exactly as the TS
/// treats them (the roster column is `text`, length ≤ 80).
const CONTACT_A: &str = "7c240400-9999-4999-9999-00000000c001";
const CONTACT_B: &str = "7c240400-9999-4999-9999-00000000c002";
const CONTACT_C: &str = "7c240400-9999-4999-9999-00000000c003";

// --- The command keys (witness-local; hex only) -------------------------------

const KEY_LIFE_SESSION: &str = "7c240400-9999-4999-9999-000000000001";
const KEY_LIFE_START: &str = "7c240400-9999-4999-9999-000000000002";
const KEY_LIFE_REG_A: &str = "7c240400-9999-4999-9999-000000000003";
const KEY_LIFE_REG_B: &str = "7c240400-9999-4999-9999-000000000004";
const KEY_LIFE_CONFIRM: &str = "7c240400-9999-4999-9999-000000000005";
const KEY_LIFE_CHECK_IN: &str = "7c240400-9999-4999-9999-000000000006";

const KEY_A04_SESSION_A: &str = "7c240400-9999-4999-9999-000000000101";
const KEY_A04_SESSION_B: &str = "7c240400-9999-4999-9999-000000000102";
const KEY_A04_REG_B: &str = "7c240400-9999-4999-9999-000000000103";
const KEY_A04_CHECK_IN: &str = "7c240400-9999-4999-9999-000000000104";
const KEY_A04_RANDOM: &str = "7c240400-9999-4999-9999-000000000105";

const KEY_SEAT_SESSION: &str = "7c240400-9999-4999-9999-000000000201";
const KEY_SEAT_FIRST: &str = "7c240400-9999-4999-9999-000000000202";
const KEY_SEAT_SECOND: &str = "7c240400-9999-4999-9999-000000000203";
const KEY_SEAT_RACE_SESSION: &str = "7c240400-9999-4999-9999-000000000204";
const KEY_SEAT_RACE_C: &str = "7c240400-9999-4999-9999-000000000205";
const KEY_SEAT_RACE_D: &str = "7c240400-9999-4999-9999-000000000206";

const KEY_DUP_SESSION: &str = "7c240400-9999-4999-9999-000000000301";
const KEY_DUP_FIRST: &str = "7c240400-9999-4999-9999-000000000302";
const KEY_DUP_SECOND: &str = "7c240400-9999-4999-9999-000000000303";
const KEY_DUP_TICKET: &str = "7c240400-9999-4999-9999-000000000304";
const KEY_DUP_SESSION_CODE: &str = "7c240400-9999-4999-9999-000000000305";

const KEY_KEY_SESSION: &str = "7c240400-9999-4999-9999-000000000401";
const KEY_KEY_REG: &str = "7c240400-9999-4999-9999-000000000402";
const KEY_KEY_CHECK_IN: &str = "7c240400-9999-4999-9999-000000000403";
const KEY_KEY_AGAIN: &str = "7c240400-9999-4999-9999-000000000404";

const KEY_RACE_SESSION: &str = "7c240400-9999-4999-9999-000000000501";
const KEY_RACE_REG: &str = "7c240400-9999-4999-9999-000000000502";
const KEY_RACE_CHECK_IN_1: &str = "7c240400-9999-4999-9999-000000000503";
const KEY_RACE_CHECK_IN_2: &str = "7c240400-9999-4999-9999-000000000504";

const KEY_CLOSED_SESSION: &str = "7c240400-9999-4999-9999-000000000601";
const KEY_CLOSED_REG: &str = "7c240400-9999-4999-9999-000000000602";
const KEY_CLOSED_CANCEL: &str = "7c240400-9999-4999-9999-000000000603";
const KEY_CLOSED_REG_LATE: &str = "7c240400-9999-4999-9999-000000000604";
const KEY_CLOSED_CHECK_IN: &str = "7c240400-9999-4999-9999-000000000605";

const KEY_STALE_SESSION: &str = "7c240400-9999-4999-9999-000000000701";
const KEY_STALE_REG: &str = "7c240400-9999-4999-9999-000000000702";

const KEY_REPLAY_SESSION: &str = "7c240400-9999-4999-9999-000000000801";

const KEY_CONFLICT_SESSION: &str = "7c240400-9999-4999-9999-000000000901";

const KEY_GRANT_SESSION: &str = "7c240400-9999-4999-9999-000000000a01";

const KEY_STORED_SESSION: &str = "7c240400-9999-4999-9999-000000000c01";
const KEY_STORED_REG: &str = "7c240400-9999-4999-9999-000000000c02";
const KEY_STORED_CHECK_IN: &str = "7c240400-9999-4999-9999-000000000c03";

const KEY_WIRE_SESSION: &str = "7c240400-9999-4999-9999-000000000b01";
const KEY_WIRE_SECOND: &str = "7c240400-9999-4999-9999-000000000b02";

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

/// The serving state, the router, and the six sessions.
struct Cast {
    ax: Session,
    axg: Session,
    axn: Session,
    axv: Session,
    axa: Session,
    other: Session,
}

async fn served() -> (Fixture, axum::Router, Cast) {
    let fixture = provision().await;
    seed(&fixture).await;
    // The provision checks (role, the ledger, the forced-RLS width) pass on
    // the fixture's runtime role — the serving state's support check,
    // asserted here because the suite holds the receipt of it.
    archava_adapters::erp::attendance::assert_attendance_kernel(
        &fixture.runtime_pool,
        &fixture.attendance_ledger,
    )
    .await
    .expect("the attendance kernel asserts on the fixture");

    let app = archava_host::router(state(&fixture).await);
    let ax = login(&app, "ax").await;
    let axg = login(&app, "axg").await;
    let axn = login(&app, "axn").await;
    let axv = login(&app, "axv").await;
    let axa = login(&app, "axa").await;
    let other = login(&app, "other").await;
    (
        fixture,
        app,
        Cast {
            ax,
            axg,
            axn,
            axv,
            axa,
            other,
        },
    )
}

// --- The submission bodies ----------------------------------------------------

/// The mutation envelope: tenant from the caller's own document, the flat
/// mutation document inside.
fn envelope(tenant: &str, mutation: Value) -> String {
    json!({ "tenant": tenant, "mutation": mutation }).to_string()
}

/// `create_session` — a scheduled session with an optional seat count.
fn create_body(tenant: &str, command_id: &str, code: &str, capacity: Option<i64>) -> String {
    let mut mutation = json!({
        "type": "create_session",
        "commandId": command_id,
        "title": "Fullstack Bootcamp Week 1",
        "sessionCode": code,
        "startTime": START,
        "endTime": END,
    });
    if let Some(capacity) = capacity {
        mutation["capacity"] = json!(capacity);
    }
    envelope(tenant, mutation)
}

/// `register_participant`.
fn register_body(
    tenant: &str,
    command_id: &str,
    session_id: &str,
    contact_id: &str,
    ticket_code: &str,
) -> String {
    envelope(
        tenant,
        json!({
            "type": "register_participant",
            "commandId": command_id,
            "sessionId": session_id,
            "contactId": contact_id,
            "ticketCode": ticket_code
        }),
    )
}

/// `record_check_in`.
fn check_in_body(
    tenant: &str,
    command_id: &str,
    session_id: &str,
    roster_id: &str,
    idempotency_key: &str,
) -> String {
    envelope(
        tenant,
        json!({
            "type": "record_check_in",
            "commandId": command_id,
            "sessionId": session_id,
            "rosterId": roster_id,
            "method": "qr_scan",
            "evidenceRef": "Scanner gate A",
            "idempotencyKey": idempotency_key
        }),
    )
}

/// `update_roster_status`.
fn roster_status_body(
    tenant: &str,
    command_id: &str,
    roster_id: &str,
    expected_revision: i64,
    target: &str,
) -> String {
    envelope(
        tenant,
        json!({
            "type": "update_roster_status",
            "commandId": command_id,
            "rosterId": roster_id,
            "expectedRevision": expected_revision,
            "targetStatus": target,
            "reason": "operator moved the roster"
        }),
    )
}

/// `change_session_status`.
fn session_status_body(
    tenant: &str,
    command_id: &str,
    session_id: &str,
    expected_revision: i64,
    target: &str,
) -> String {
    envelope(
        tenant,
        json!({
            "type": "change_session_status",
            "commandId": command_id,
            "sessionId": session_id,
            "expectedRevision": expected_revision,
            "targetStatus": target,
            "reason": "operator moved the session"
        }),
    )
}

/// Submit an arm and assert it committed — the OK receipt's channel.
async fn receipt_of(app: &axum::Router, session: &Session, body: &str) -> Value {
    let (status, receipt) = post(app, "/erp/attendance/commands", body, session).await;
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
    let (status, answer) = post(app, "/erp/attendance/commands", body, session).await;
    assert_eq!(
        status,
        expected,
        "the arm must refuse with {}: {answer} — body {body}",
        expected.as_u16()
    );
    answer
}

/// The session id a create receipt names.
fn session_of(receipt: &Value) -> String {
    receipt["sessionId"]
        .as_str()
        .expect("the create names its session")
        .to_string()
}

/// The roster id a register receipt names.
fn roster_of(receipt: &Value) -> String {
    receipt["rosterId"]
        .as_str()
        .expect("the register names its roster")
        .to_string()
}

/// The detail read of one session.
async fn detail_of(app: &axum::Router, session: &Session, session_id: &str) -> Value {
    let (status, detail) = get(
        app,
        &format!("/erp/attendance/sessions/detail?tenant={ACME}&id={session_id}"),
        session,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the detail read must answer: {detail}"
    );
    detail
}

/// The row count of one table in the attendance schema.
async fn count(fixture: &Fixture, table: &str) -> i64 {
    // The table name is a literal from this suite's own list, never a
    // caller value — the crate's SQL-safety marker still rides it.
    let statement = format!("SELECT count(*)::bigint FROM archava_attendance.{table}");
    let (count,): (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(statement.as_str()))
        .fetch_one(&fixture.admin_pool)
        .await
        .expect("count the rows");
    count
}

/// One session's stored counters and status.
async fn session_row(fixture: &Fixture, session_id: &str) -> (String, i64, i64, i64) {
    sqlx::query_as(
        "SELECT status, enrolled_count::bigint, attended_count::bigint, revision::bigint \
FROM archava_attendance.sessions WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(session_id))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the session row")
}

/// Withdraw ONE grant from a seat that stays active — the closure A1-FU3
/// exercises: the membership is not deactivated, only the permission dies.
async fn withdraw_attendance_grant(fixture: &Fixture, principal: &str, grant: &str) {
    sqlx::query(
        "UPDATE archava_attendance.memberships SET permissions = array_remove(permissions, $3::text) \
WHERE tenant_id=$1 AND principal_id=$2 AND active",
    )
    .bind(ACME)
    .bind(principal)
    .bind(grant)
    .execute(&fixture.admin_pool)
    .await
    .expect("withdraw the grant");
}

/// Put one grant back after [`withdraw_attendance_grant`].
async fn restore_attendance_grant(fixture: &Fixture, principal: &str, grant: &str) {
    sqlx::query(
        "UPDATE archava_attendance.memberships SET permissions = array_append(permissions, $3::text) \
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
async fn seat_active(fixture: &Fixture, principal: &str) -> bool {
    sqlx::query_scalar(
        "SELECT active FROM archava_attendance.memberships \
WHERE tenant_id=$1 AND principal_id=$2",
    )
    .bind(ACME)
    .bind(principal)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the seat")
}

// --- The witnesses ------------------------------------------------------------

#[tokio::test]
async fn authorized_attendance_life_documents_its_sessions_and_check_ins() {
    let (fixture, app, cast) = served().await;

    // 1. create_session — scheduled, revision 1, no roster/check-in id.
    let created = receipt_of(
        &app,
        &cast.ax,
        &create_body(ACME, KEY_LIFE_SESSION, "FS-BC-W1", Some(3)),
    )
    .await;
    assert_eq!(created["commandId"].as_str(), Some(KEY_LIFE_SESSION));
    assert_eq!(created["tenantId"].as_str(), Some(ACME));
    assert_eq!(created["action"].as_str(), Some("create_session"));
    assert_eq!(created["status"].as_str(), Some("scheduled"));
    assert_eq!(created["revision"].as_i64(), Some(1));
    assert_eq!(created["idempotencyReplay"].as_bool(), Some(false));
    assert!(created["executedAt"].as_str().is_some());
    let session_id = session_of(&created);
    assert!(created["rosterId"].is_null(), "the create names no roster");
    assert!(created["checkInId"].is_null(), "the create names no event");

    // 2. change_session_status — the session opens.
    let started = receipt_of(
        &app,
        &cast.ax,
        &session_status_body(ACME, KEY_LIFE_START, &session_id, 1, "in_progress"),
    )
    .await;
    assert_eq!(started["action"].as_str(), Some("change_session_status"));
    assert_eq!(started["status"].as_str(), Some("in_progress"));
    assert_eq!(started["revision"].as_i64(), Some(2));

    // 3. two registrations — each receipt names its roster at revision 1.
    let registered_a = receipt_of(
        &app,
        &cast.ax,
        &register_body(ACME, KEY_LIFE_REG_A, &session_id, CONTACT_A, "TCK-0001"),
    )
    .await;
    assert_eq!(
        registered_a["action"].as_str(),
        Some("register_participant")
    );
    assert_eq!(registered_a["status"].as_str(), Some("registered"));
    assert_eq!(registered_a["revision"].as_i64(), Some(1));
    let roster_a = roster_of(&registered_a);
    let registered_b = receipt_of(
        &app,
        &cast.ax,
        &register_body(ACME, KEY_LIFE_REG_B, &session_id, CONTACT_B, "TCK-0002"),
    )
    .await;
    let roster_b = roster_of(&registered_b);

    // 4. update_roster_status — the second participant confirms.
    let confirmed = receipt_of(
        &app,
        &cast.ax,
        &roster_status_body(ACME, KEY_LIFE_CONFIRM, &roster_b, 1, "confirmed"),
    )
    .await;
    assert_eq!(confirmed["action"].as_str(), Some("update_roster_status"));
    assert_eq!(confirmed["status"].as_str(), Some("confirmed"));
    assert_eq!(confirmed["revision"].as_i64(), Some(2));
    assert_eq!(confirmed["sessionId"].as_str(), Some(session_id.as_str()));

    // 5. record_check_in — the first participant attends.
    let checked = receipt_of(
        &app,
        &cast.ax,
        &check_in_body(ACME, KEY_LIFE_CHECK_IN, &session_id, &roster_a, "scan-0001"),
    )
    .await;
    assert_eq!(checked["action"].as_str(), Some("record_check_in"));
    assert_eq!(checked["status"].as_str(), Some("attended"));
    assert_eq!(checked["revision"].as_i64(), Some(2));
    assert_eq!(checked["rosterId"].as_str(), Some(roster_a.as_str()));
    assert!(checked["checkInId"].as_str().is_some());
    let check_in_id = checked["checkInId"].as_str().unwrap().to_string();

    // The detail read answers the same rows the writes committed.
    let detail = detail_of(&app, &cast.ax, &session_id).await;
    assert_eq!(detail["session"]["id"].as_str(), Some(session_id.as_str()));
    assert_eq!(detail["session"]["status"].as_str(), Some("in_progress"));
    assert_eq!(detail["session"]["capacity"].as_i64(), Some(3));
    assert_eq!(detail["session"]["enrolledCount"].as_i64(), Some(2));
    assert_eq!(detail["session"]["attendedCount"].as_i64(), Some(1));
    assert_eq!(detail["session"]["revision"].as_i64(), Some(4));
    let roster = detail["roster"].as_array().expect("the roster is an array");
    assert_eq!(roster.len(), 2);
    // Locate by id: the two registrations may share a millisecond, and the
    // read orders by `created_at` (the TS's own order) — the witness must
    // not depend on a tie the database is free to break either way.
    let entry = |id: &str| {
        roster
            .iter()
            .find(|row| row["id"].as_str() == Some(id))
            .unwrap_or_else(|| panic!("the detail names roster {id}"))
    };
    let entry_a = entry(&roster_a);
    assert_eq!(entry_a["status"].as_str(), Some("attended"));
    assert_eq!(entry_a["revision"].as_i64(), Some(2));
    assert!(entry_a["checkedInAt"].as_str().is_some());
    assert_eq!(entry_a["checkInMethod"].as_str(), Some("qr_scan"));
    assert_eq!(entry_a["contactId"].as_str(), Some(CONTACT_A));
    assert_eq!(entry(&roster_b)["status"].as_str(), Some("confirmed"));
    let check_ins = detail["checkIns"]
        .as_array()
        .expect("the check-ins are an array");
    assert_eq!(check_ins.len(), 1);
    assert_eq!(
        check_ins[0]["id"].as_str(),
        Some(check_in_id.as_str()),
        "the detail names the event the receipt named"
    );
    assert_eq!(check_ins[0]["rosterId"].as_str(), Some(roster_a.as_str()));
    assert_eq!(check_ins[0]["contactId"].as_str(), Some(CONTACT_A));
    assert_eq!(check_ins[0]["method"].as_str(), Some("qr_scan"));
    assert_eq!(check_ins[0]["idempotencyKey"].as_str(), Some("scan-0001"));
    assert_eq!(check_ins[0]["recordedBy"].as_str(), Some("ax"));
    assert_eq!(
        check_ins[0]["evidenceRef"].as_str(),
        Some("Scanner gate A"),
        "the event keeps the operator's evidence"
    );

    // The list read: one row, bounded and ordered; the status filter walks.
    let (status, list) = get(
        &app,
        &format!("/erp/attendance/sessions?tenant={ACME}"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the list read must answer: {list}");
    let rows = list.as_array().expect("the list read answers an array");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"].as_str(), Some(session_id.as_str()));
    let (status, filtered) = get(
        &app,
        &format!("/erp/attendance/sessions?tenant={ACME}&status=in_progress"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        filtered.as_array().map(Vec::len),
        Some(1),
        "the status filter matches"
    );
    let (status, empty) = get(
        &app,
        &format!("/erp/attendance/sessions?tenant={ACME}&status=completed"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty.as_array().map(Vec::len), Some(0));

    // The sub-reads answer the same roster/events.
    let (status, roster_rows) = get(
        &app,
        &format!("/erp/attendance/roster?tenant={ACME}&sessionId={session_id}"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the roster read must answer");
    assert_eq!(roster_rows.as_array().map(Vec::len), Some(2));
    let (status, check_in_rows) = get(
        &app,
        &format!("/erp/attendance/check-ins?tenant={ACME}&sessionId={session_id}"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the check-in read must answer");
    assert_eq!(check_in_rows.as_array().map(Vec::len), Some(1));

    // The access envelope and the durable status read.
    let (status, access) = get(
        &app,
        &format!("/erp/attendance/access?tenant={ACME}"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(access["ownerId"].as_str(), Some("ax"));
    assert_eq!(access["scope"].as_str(), Some("all"));
    assert_eq!(
        access["permissions"].as_array().map(Vec::len),
        Some(4),
        "the full grant envelope"
    );
    let (status, command_status) = get(
        &app,
        &format!("/erp/attendance/commands/status?tenant={ACME}&requestId={KEY_LIFE_SESSION}"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(command_status["status"].as_str(), Some("recorded"));
    assert_eq!(
        command_status["receipt"]["action"].as_str(),
        Some("create_session")
    );

    // The rows: the scheduled session advanced to in_progress with two
    // enrolments and one attendance; the roster/event rows match the reads.
    let (session_status, enrolled, attended, revision) = session_row(&fixture, &session_id).await;
    assert_eq!(session_status, "in_progress");
    assert_eq!(enrolled, 2);
    assert_eq!(attended, 1);
    assert_eq!(revision, 4, "create + start + two registrations");
    assert_eq!(count(&fixture, "rosters").await, 2);
    assert_eq!(count(&fixture, "check_in_events").await, 1);
    assert_eq!(count(&fixture, "commands").await, 6);
    assert_eq!(
        count(&fixture, "audit").await,
        6,
        "one audit row per command"
    );
    let (operations,): (String,) = sqlx::query_as(
        "SELECT string_agg(operation, ',' ORDER BY operation) FROM archava_attendance.audit",
    )
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the audit rows");
    assert_eq!(
        operations,
        "change_session_status,create_session,record_check_in,register_participant,\
register_participant,update_roster_status"
    );
    let (roster_status, roster_revision): (String, i64) = sqlx::query_as(
        "SELECT status, revision::bigint FROM archava_attendance.rosters WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&roster_b))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the confirmed roster");
    assert_eq!(roster_status, "confirmed");
    assert_eq!(roster_revision, 2);

    // The ledger's own defenses: the audit history and the recorded
    // receipts are immutable, even for the schema owner.
    let tampered = sqlx::query("UPDATE archava_attendance.audit SET operation='tampered'")
        .execute(&fixture.admin_pool)
        .await;
    assert!(tampered.is_err(), "the audit history must be immutable");
    let cleared = sqlx::query("UPDATE archava_attendance.commands SET receipt=NULL")
        .execute(&fixture.admin_pool)
        .await;
    assert!(cleared.is_err(), "a recorded receipt must not be cleared");
}

#[tokio::test]
async fn a_roster_from_another_session_cannot_check_in() {
    let (fixture, app, cast) = served().await;

    // Two sessions; the roster exists in B only.
    let session_a = session_of(
        &receipt_of(
            &app,
            &cast.ax,
            &create_body(ACME, KEY_A04_SESSION_A, "A04-SESSION-A", Some(5)),
        )
        .await,
    );
    let session_b = session_of(
        &receipt_of(
            &app,
            &cast.ax,
            &create_body(ACME, KEY_A04_SESSION_B, "A04-SESSION-B", Some(5)),
        )
        .await,
    );
    let roster_b = roster_of(
        &receipt_of(
            &app,
            &cast.ax,
            &register_body(ACME, KEY_A04_REG_B, &session_b, CONTACT_A, "TCK-A04"),
        )
        .await,
    );

    // The A04 counterexample: B's roster id presented under session A. The
    // row SELECT binds the session, so the roster is invisible — 404.
    let refused = refused_with(
        &app,
        &cast.ax,
        &check_in_body(ACME, KEY_A04_CHECK_IN, &session_a, &roster_b, "scan-a04"),
        StatusCode::NOT_FOUND,
    )
    .await;
    assert_eq!(refused["reason"].as_str(), Some("unknown_resource"));

    // A random roster id under B is equally invisible.
    refused_with(
        &app,
        &cast.ax,
        &check_in_body(
            ACME,
            KEY_A04_RANDOM,
            &session_b,
            &uuid::Uuid::new_v4().to_string(),
            "scan-random",
        ),
        StatusCode::NOT_FOUND,
    )
    .await;

    // Nothing moved: no event, no attendance, no failed command row, and
    // B's roster is still registered.
    assert_eq!(count(&fixture, "check_in_events").await, 0);
    assert_eq!(count(&fixture, "commands").await, 3);
    let (a_status, a_enrolled, a_attended, a_revision) = session_row(&fixture, &session_a).await;
    assert_eq!(a_status, "scheduled");
    assert_eq!(a_enrolled, 0);
    assert_eq!(a_attended, 0);
    assert_eq!(a_revision, 1);
    let (b_status, b_enrolled, b_attended, _) = session_row(&fixture, &session_b).await;
    assert_eq!(b_status, "scheduled");
    assert_eq!(b_enrolled, 1);
    assert_eq!(b_attended, 0);
    let (roster_status, roster_revision): (String, i64) = sqlx::query_as(
        "SELECT status, revision::bigint FROM archava_attendance.rosters WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&roster_b))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the roster");
    assert_eq!(roster_status, "registered", "the roster never moved");
    assert_eq!(roster_revision, 1);

    // And the contrast: A's detail exposes an empty roster, B's exposes its
    // own row — the wrong session cannot read the other record.
    let detail_a = detail_of(&app, &cast.ax, &session_a).await;
    assert_eq!(detail_a["roster"].as_array().map(Vec::len), Some(0));
    assert_eq!(detail_a["checkIns"].as_array().map(Vec::len), Some(0));
    let detail_b = detail_of(&app, &cast.ax, &session_b).await;
    assert_eq!(detail_b["roster"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        detail_b["roster"][0]["id"].as_str(),
        Some(roster_b.as_str())
    );
}

#[tokio::test]
async fn the_seat_boundary_refuses_and_holds_nothing() {
    let (fixture, app, cast) = served().await;

    let session_id = session_of(
        &receipt_of(
            &app,
            &cast.ax,
            &create_body(ACME, KEY_SEAT_SESSION, "SEAT-1", Some(1)),
        )
        .await,
    );
    receipt_of(
        &app,
        &cast.ax,
        &register_body(ACME, KEY_SEAT_FIRST, &session_id, CONTACT_A, "TCK-SEAT-1"),
    )
    .await;
    let refused = refused_with(
        &app,
        &cast.ax,
        &register_body(ACME, KEY_SEAT_SECOND, &session_id, CONTACT_B, "TCK-SEAT-2"),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(refused["reason"].as_str(), Some("capacity_exceeded"));
    assert_eq!(count(&fixture, "rosters").await, 1);
    let (_, enrolled, attended, _) = session_row(&fixture, &session_id).await;
    assert_eq!(enrolled, 1);
    assert_eq!(attended, 0);

    // The same boundary under CONCURRENCY: two registrations race for the
    // last seat of a fresh session. Exactly one commits; the loser is the
    // capacity class; the counter never overbooks and there is one roster.
    let race_session = session_of(
        &receipt_of(
            &app,
            &cast.ax,
            &create_body(ACME, KEY_SEAT_RACE_SESSION, "SEAT-RACE", Some(1)),
        )
        .await,
    );
    let first_body = register_body(
        ACME,
        KEY_SEAT_RACE_C,
        &race_session,
        CONTACT_B,
        "TCK-RACE-C",
    );
    let second_body = register_body(
        ACME,
        KEY_SEAT_RACE_D,
        &race_session,
        CONTACT_C,
        "TCK-RACE-D",
    );
    let first = post(&app, "/erp/attendance/commands", &first_body, &cast.ax);
    let second = post(&app, "/erp/attendance/commands", &second_body, &cast.ax);
    let (one, two) = tokio::join!(first, second);
    let mut statuses = [one.0, two.0];
    statuses.sort_by_key(|status| status.as_u16());
    assert_eq!(
        statuses,
        [StatusCode::OK, StatusCode::CONFLICT],
        "exactly one registration wins the last seat"
    );
    let loser = if one.0 == StatusCode::CONFLICT {
        &one.1
    } else {
        &two.1
    };
    assert_eq!(loser["reason"].as_str(), Some("capacity_exceeded"));
    let (_, race_enrolled, _, race_revision) = session_row(&fixture, &race_session).await;
    assert_eq!(race_enrolled, 1, "the seat counter never overbooks");
    assert_eq!(race_revision, 2, "one registration moved the revision once");
    let (races,): (i64,) = sqlx::query_as(
        "SELECT count(*)::bigint FROM archava_attendance.rosters WHERE tenant_id=$1 AND session_id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&race_session))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the race rosters");
    assert_eq!(races, 1);
}

#[tokio::test]
async fn a_duplicate_registration_conflicts_and_records_nothing() {
    let (fixture, app, cast) = served().await;

    let session_id = session_of(
        &receipt_of(
            &app,
            &cast.ax,
            &create_body(ACME, KEY_DUP_SESSION, "DUP-1", Some(10)),
        )
        .await,
    );
    receipt_of(
        &app,
        &cast.ax,
        &register_body(ACME, KEY_DUP_FIRST, &session_id, CONTACT_A, "TCK-DUP-1"),
    )
    .await;

    // The same contact on the same session: the UNIQUE constraint would be
    // a 500 in the TS; the port answers the conflict class, and the
    // transaction (counter UPDATE included) records nothing.
    refused_with(
        &app,
        &cast.ax,
        &register_body(ACME, KEY_DUP_SECOND, &session_id, CONTACT_A, "TCK-DUP-2"),
        StatusCode::CONFLICT,
    )
    .await;
    // A second contact reusing the ticket code is the same class.
    refused_with(
        &app,
        &cast.ax,
        &register_body(ACME, KEY_DUP_TICKET, &session_id, CONTACT_B, "TCK-DUP-1"),
        StatusCode::CONFLICT,
    )
    .await;
    // The session-code UNIQUE constraint is the same class at creation.
    refused_with(
        &app,
        &cast.ax,
        &create_body(ACME, KEY_DUP_SESSION_CODE, "DUP-1", Some(10)),
        StatusCode::CONFLICT,
    )
    .await;

    assert_eq!(
        count(&fixture, "sessions").await,
        1,
        "the duplicate code creates no session"
    );
    assert_eq!(count(&fixture, "rosters").await, 1);
    assert_eq!(count(&fixture, "commands").await, 2);
    let (_, enrolled, _, revision) = session_row(&fixture, &session_id).await;
    assert_eq!(enrolled, 1);
    assert_eq!(revision, 2, "the first registration moved it once");
}

#[tokio::test]
async fn the_same_check_in_key_a_different_command_conflicts() {
    let (fixture, app, cast) = served().await;

    let session_id = session_of(
        &receipt_of(
            &app,
            &cast.ax,
            &create_body(ACME, KEY_KEY_SESSION, "KEY-1", Some(5)),
        )
        .await,
    );
    let roster_id = roster_of(
        &receipt_of(
            &app,
            &cast.ax,
            &register_body(ACME, KEY_KEY_REG, &session_id, CONTACT_A, "TCK-KEY"),
        )
        .await,
    );
    receipt_of(
        &app,
        &cast.ax,
        &check_in_body(ACME, KEY_KEY_CHECK_IN, &session_id, &roster_id, "scan-key"),
    )
    .await;

    // A second command with the SAME roster and the SAME idempotency key:
    // the baseline lets the UNIQUE constraint raise; the port answers
    // `already_checked_in` and records nothing.
    let refused = refused_with(
        &app,
        &cast.ax,
        &check_in_body(ACME, KEY_KEY_AGAIN, &session_id, &roster_id, "scan-key"),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(refused["reason"].as_str(), Some("already_checked_in"));
    // A different key cannot revive the attendance either.
    let refused = refused_with(
        &app,
        &cast.ax,
        &check_in_body(
            ACME,
            "7c240400-9999-4999-9999-000000000405",
            &session_id,
            &roster_id,
            "scan-key-2",
        ),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(refused["reason"].as_str(), Some("already_checked_in"));

    assert_eq!(count(&fixture, "check_in_events").await, 1);
    assert_eq!(count(&fixture, "commands").await, 3);
    let (_, enrolled, attended, _) = session_row(&fixture, &session_id).await;
    assert_eq!(enrolled, 1);
    assert_eq!(attended, 1, "the attendance counts once");
    let (roster_status, roster_revision): (String, i64) = sqlx::query_as(
        "SELECT status, revision::bigint FROM archava_attendance.rosters WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&roster_id))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the roster");
    assert_eq!(roster_status, "attended");
    assert_eq!(roster_revision, 2, "the refused commands moved nothing");
}

#[tokio::test]
async fn concurrent_check_ins_record_exactly_one_attendance() {
    let (fixture, app, cast) = served().await;

    let session_id = session_of(
        &receipt_of(
            &app,
            &cast.ax,
            &create_body(ACME, KEY_RACE_SESSION, "RACE-CHECK", Some(5)),
        )
        .await,
    );
    let roster_id = roster_of(
        &receipt_of(
            &app,
            &cast.ax,
            &register_body(ACME, KEY_RACE_REG, &session_id, CONTACT_A, "TCK-RACE"),
        )
        .await,
    );

    // Two scanners, two keys, one roster — the concurrent case. Exactly one
    // event commits; the loser waits on the roster lock, sees `attended`,
    // and refuses with the TS's own class.
    let first_body = check_in_body(
        ACME,
        KEY_RACE_CHECK_IN_1,
        &session_id,
        &roster_id,
        "scan-race-1",
    );
    let second_body = check_in_body(
        ACME,
        KEY_RACE_CHECK_IN_2,
        &session_id,
        &roster_id,
        "scan-race-2",
    );
    let first = post(&app, "/erp/attendance/commands", &first_body, &cast.ax);
    let second = post(&app, "/erp/attendance/commands", &second_body, &cast.ax);
    let (one, two) = tokio::join!(first, second);
    let mut statuses = [one.0, two.0];
    statuses.sort_by_key(|status| status.as_u16());
    assert_eq!(
        statuses,
        [StatusCode::OK, StatusCode::CONFLICT],
        "exactly one scanner wins"
    );
    let loser = if one.0 == StatusCode::CONFLICT {
        &one.1
    } else {
        &two.1
    };
    assert_eq!(loser["reason"].as_str(), Some("already_checked_in"));

    assert_eq!(count(&fixture, "check_in_events").await, 1);
    let (_, enrolled, attended, revision) = session_row(&fixture, &session_id).await;
    assert_eq!(enrolled, 1);
    assert_eq!(attended, 1, "the counter advanced exactly once");
    assert_eq!(
        revision, 2,
        "registration moved the revision; check-in does not"
    );
    let (roster_status, roster_revision): (String, i64) = sqlx::query_as(
        "SELECT status, revision::bigint FROM archava_attendance.rosters WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&roster_id))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the roster");
    assert_eq!(roster_status, "attended");
    assert_eq!(roster_revision, 2);
}

#[tokio::test]
async fn a_closed_session_refuses_registration_and_check_in() {
    let (fixture, app, cast) = served().await;

    let session_id = session_of(
        &receipt_of(
            &app,
            &cast.ax,
            &create_body(ACME, KEY_CLOSED_SESSION, "CLOSED-1", Some(5)),
        )
        .await,
    );
    let roster_id = roster_of(
        &receipt_of(
            &app,
            &cast.ax,
            &register_body(ACME, KEY_CLOSED_REG, &session_id, CONTACT_A, "TCK-CLOSED"),
        )
        .await,
    );
    let canceled = receipt_of(
        &app,
        &cast.ax,
        &session_status_body(ACME, KEY_CLOSED_CANCEL, &session_id, 2, "canceled"),
    )
    .await;
    assert_eq!(canceled["status"].as_str(), Some("canceled"));
    assert_eq!(canceled["revision"].as_i64(), Some(3));

    // The cancellation cascaded over the non-attended roster.
    let (roster_status, _): (String, i64) = sqlx::query_as(
        "SELECT status, revision::bigint FROM archava_attendance.rosters WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&roster_id))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the roster");
    assert_eq!(roster_status, "canceled");

    // A late registration and a late check-in are the 400 class.
    let refused = refused_with(
        &app,
        &cast.ax,
        &register_body(
            ACME,
            KEY_CLOSED_REG_LATE,
            &session_id,
            CONTACT_B,
            "TCK-LATE",
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(refused["reason"].as_str(), Some("bad_request"));
    let refused = refused_with(
        &app,
        &cast.ax,
        &check_in_body(
            ACME,
            KEY_CLOSED_CHECK_IN,
            &session_id,
            &roster_id,
            "scan-late",
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(refused["reason"].as_str(), Some("bad_request"));

    assert_eq!(count(&fixture, "rosters").await, 1);
    assert_eq!(count(&fixture, "check_in_events").await, 0);
    let (status, enrolled, attended, revision) = session_row(&fixture, &session_id).await;
    assert_eq!(status, "canceled");
    assert_eq!(enrolled, 1);
    assert_eq!(attended, 0);
    assert_eq!(revision, 3);
    assert_eq!(count(&fixture, "audit").await, 3);
}

#[tokio::test]
async fn the_stale_revision_conflicts_and_records_nothing() {
    let (fixture, app, cast) = served().await;

    let session_id = session_of(
        &receipt_of(
            &app,
            &cast.ax,
            &create_body(ACME, KEY_STALE_SESSION, "STALE-1", Some(5)),
        )
        .await,
    );
    let roster_id = roster_of(
        &receipt_of(
            &app,
            &cast.ax,
            &register_body(ACME, KEY_STALE_REG, &session_id, CONTACT_A, "TCK-STALE"),
        )
        .await,
    );

    let refused = refused_with(
        &app,
        &cast.ax,
        &roster_status_body(
            ACME,
            "7c240400-9999-4999-9999-000000000703",
            &roster_id,
            99,
            "confirmed",
        ),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(refused["reason"].as_str(), Some("conflict"));
    let refused = refused_with(
        &app,
        &cast.ax,
        &session_status_body(
            ACME,
            "7c240400-9999-4999-9999-000000000704",
            &session_id,
            99,
            "completed",
        ),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(refused["reason"].as_str(), Some("conflict"));

    let (roster_status, roster_revision): (String, i64) = sqlx::query_as(
        "SELECT status, revision::bigint FROM archava_attendance.rosters WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid_of(&roster_id))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the roster");
    assert_eq!(roster_status, "registered");
    assert_eq!(roster_revision, 1);
    let (status, _, _, revision) = session_row(&fixture, &session_id).await;
    assert_eq!(status, "scheduled");
    assert_eq!(revision, 2, "the registration's revision, nothing else");
    assert_eq!(count(&fixture, "audit").await, 2);
    assert_eq!(count(&fixture, "commands").await, 2);
}

#[tokio::test]
async fn lost_response_replays_the_recorded_attendance_receipt() {
    let (fixture, app, cast) = served().await;

    let body = create_body(ACME, KEY_REPLAY_SESSION, "REPLAY-1", Some(5));
    let first = receipt_of(&app, &cast.ax, &body).await;
    let session_id = session_of(&first);
    let replay = receipt_of(&app, &cast.ax, &body).await;
    assert_eq!(replay["idempotencyReplay"].as_bool(), Some(true));
    assert_eq!(replay["commandId"].as_str(), first["commandId"].as_str());
    assert_eq!(replay["sessionId"].as_str(), first["sessionId"].as_str());
    assert_eq!(replay["executedAt"].as_str(), first["executedAt"].as_str());
    assert_eq!(count(&fixture, "sessions").await, 1, "no second session");
    assert_eq!(count(&fixture, "commands").await, 1);
    assert_eq!(count(&fixture, "audit").await, 1);

    // The status read answers the recorded receipt verbatim (with the
    // replay flag false, exactly like the TS's stored document), and the
    // unknown id answers the bare status.
    let (status, command_status) = get(
        &app,
        &format!("/erp/attendance/commands/status?tenant={ACME}&requestId={KEY_REPLAY_SESSION}"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(command_status["status"].as_str(), Some("recorded"));
    assert_eq!(
        command_status["receipt"]["sessionId"].as_str(),
        Some(session_id.as_str())
    );
    assert_eq!(
        command_status["receipt"]["idempotencyReplay"].as_bool(),
        Some(false),
        "the stored document is not the replay's echo"
    );
    let (status, unknown) = get(
        &app,
        &format!(
            "/erp/attendance/commands/status?tenant={ACME}&requestId={}",
            uuid::Uuid::new_v4()
        ),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unknown, json!({ "status": "unknown" }));
}

#[tokio::test]
async fn the_same_attendance_key_a_different_command_conflicts() {
    let (fixture, app, cast) = served().await;

    receipt_of(
        &app,
        &cast.ax,
        &create_body(ACME, KEY_CONFLICT_SESSION, "CONFLICT-1", Some(5)),
    )
    .await;
    // Same command id, different payload: the fingerprint refuses.
    let refused = refused_with(
        &app,
        &cast.ax,
        &create_body(ACME, KEY_CONFLICT_SESSION, "CONFLICT-2", Some(9)),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(refused["reason"].as_str(), Some("conflict"));
    // Even across arm types the slot is the command id, the fingerprint is
    // the document.
    refused_with(
        &app,
        &cast.ax,
        &register_body(
            ACME,
            KEY_CONFLICT_SESSION,
            "7c240400-9999-4999-9999-000000000902",
            CONTACT_A,
            "TCK-CONFLICT",
        ),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(count(&fixture, "sessions").await, 1);
    assert_eq!(count(&fixture, "commands").await, 1);
    assert_eq!(count(&fixture, "audit").await, 1);
}

#[tokio::test]
async fn cross_tenant_attendance_callers_refuse_before_any_write() {
    let (fixture, app, cast) = served().await;

    // GLOBEX's member submitting an ACME-tenant command: the ladder refuses
    // before any statement, and ACME stays empty.
    let refused = refused_with(
        &app,
        &cast.other,
        &create_body(
            ACME,
            "7c240400-9999-4999-9999-000000000a02",
            "CROSS-1",
            Some(5),
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
    assert_eq!(refused["reason"].as_str(), Some("forbidden"));
    // The status read is equally gated (a dead cross-tenant read proves
    // nothing).
    let (status, answer) = get(
        &app,
        &format!("/erp/attendance/commands/status?tenant={ACME}&requestId={KEY_GRANT_SESSION}"),
        &cast.other,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{answer}");
    assert_eq!(answer["reason"].as_str(), Some("forbidden"));
    assert_eq!(count(&fixture, "sessions").await, 0);
    assert_eq!(count(&fixture, "commands").await, 0);

    // Its own ledger acts freely: keys stay per tenant.
    let own = receipt_of(
        &app,
        &cast.other,
        &create_body(
            GLOBEX,
            "7c240400-9999-4999-9999-000000000a03",
            "CROSS-2",
            Some(5),
        ),
    )
    .await;
    assert_eq!(own["tenantId"].as_str(), Some(GLOBEX));
    assert_eq!(count(&fixture, "sessions").await, 1);
    let (status, acme_list) = get(
        &app,
        &format!("/erp/attendance/sessions?tenant={ACME}"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(acme_list.as_array().map(Vec::len), Some(0));
}

#[tokio::test]
async fn revoked_attendance_grant_refused_on_recovery() {
    let (fixture, app, cast) = served().await;

    let body = create_body(ACME, KEY_GRANT_SESSION, "GRANT-1", Some(5));
    let created = receipt_of(&app, &cast.axv, &body).await;
    let session_id = session_of(&created);

    // A read grant is visibility of the ledger, never another initiator's
    // receipt: `ax` holds the full grants but did not record this command, so
    // it answers the same bare document a missing key answers.
    let (status, foreign) = get(
        &app,
        &format!("/erp/attendance/commands/status?tenant={ACME}&requestId={KEY_GRANT_SESSION}"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(foreign, json!({ "status": "unknown" }), "no receipt fields");
    let (status, own) = get(
        &app,
        &format!("/erp/attendance/commands/status?tenant={ACME}&requestId={KEY_GRANT_SESSION}"),
        &cast.axv,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(own["status"].as_str(), Some("recorded"));

    // Only the read grant dies; the seat stays active and the write arm keeps
    // its own permission. Recovery must re-answer the ladder under CURRENT
    // grants — never the stored receipt alone.
    sqlx::query(
        "UPDATE archava_attendance.memberships \
SET permissions = array_remove(permissions, 'attendance.read'::text) \
WHERE tenant_id=$1 AND principal_id=$2 AND active",
    )
    .bind(ACME)
    .bind("axv")
    .execute(&fixture.admin_pool)
    .await
    .expect("withdraw the read grant");
    let active: bool = sqlx::query_scalar(
        "SELECT active FROM archava_attendance.memberships \
WHERE tenant_id=$1 AND principal_id=$2",
    )
    .bind(ACME)
    .bind("axv")
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the membership");
    assert!(active, "withdrawing one grant must not deactivate the seat");
    let (status, answer) = get(
        &app,
        &format!("/erp/attendance/commands/status?tenant={ACME}&requestId={KEY_GRANT_SESSION}"),
        &cast.axv,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{answer}");
    assert_eq!(answer["reason"].as_str(), Some("forbidden"));
    // The write arm is a different permission and the membership is still
    // active: the byte-identical replay answers the stored receipt.
    let replay = receipt_of(&app, &cast.axv, &body).await;
    assert_eq!(replay["idempotencyReplay"].as_bool(), Some(true));
    assert_eq!(replay["sessionId"].as_str(), Some(session_id.as_str()));
    assert_eq!(count(&fixture, "sessions").await, 1);

    // The grant returns: the status read answers the stored receipt again,
    // still one session.
    sqlx::query(
        "UPDATE archava_attendance.memberships \
SET permissions = array_append(permissions, 'attendance.read'::text) \
WHERE tenant_id=$1 AND principal_id=$2 AND active",
    )
    .bind(ACME)
    .bind("axv")
    .execute(&fixture.admin_pool)
    .await
    .expect("restore the read grant");
    let (status, restored) = get(
        &app,
        &format!("/erp/attendance/commands/status?tenant={ACME}&requestId={KEY_GRANT_SESSION}"),
        &cast.axv,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["status"].as_str(), Some("recorded"));
    assert_eq!(count(&fixture, "sessions").await, 1);

    // The seat dying is a different closure than a withdrawn grant: the same
    // status request refuses while the membership is inactive.
    sqlx::query(
        "UPDATE archava_attendance.memberships SET active=false \
WHERE tenant_id=$1 AND principal_id=$2",
    )
    .bind(ACME)
    .bind("axv")
    .execute(&fixture.admin_pool)
    .await
    .expect("deactivate the seat");
    let (status, inactive) = get(
        &app,
        &format!("/erp/attendance/commands/status?tenant={ACME}&requestId={KEY_GRANT_SESSION}"),
        &cast.axv,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{inactive}");
    assert_eq!(inactive["reason"].as_str(), Some("forbidden"));
    sqlx::query(
        "UPDATE archava_attendance.memberships SET active=true \
WHERE tenant_id=$1 AND principal_id=$2",
    )
    .bind(ACME)
    .bind("axv")
    .execute(&fixture.admin_pool)
    .await
    .expect("restore the seat");
    assert_eq!(count(&fixture, "sessions").await, 1);
}

/// A1-FU3 — the per-command grant gates the CLAIM, not just the arm. A seat
/// that is still active but has lost an arm's permission must not recover that
/// arm's stored receipt: the byte-identical replay is `forbidden`, carries no
/// receipt fields, and writes nothing. The other arm keeps its own replay.
#[tokio::test]
async fn revoked_attendance_grants_refuse_the_stored_writes() {
    let (fixture, app, cast) = served().await;

    // (a) Both protected arms commit under the full grant.
    let session_body = create_body(ACME, KEY_STORED_SESSION, "STORED-1", Some(4));
    let created = receipt_of(&app, &cast.axv, &session_body).await;
    assert_eq!(created["idempotencyReplay"].as_bool(), Some(false));
    let session_id = session_of(&created);
    let registration = register_body(
        ACME,
        KEY_STORED_REG,
        &session_id,
        "6b240400-9999-4999-9999-0000000000c1",
        "TICKET-STORED",
    );
    let registered = receipt_of(&app, &cast.axv, &registration).await;
    let roster_id = registered["rosterId"]
        .as_str()
        .expect("the registration names its roster")
        .to_string();
    let check_in = check_in_body(
        ACME,
        KEY_STORED_CHECK_IN,
        &session_id,
        &roster_id,
        "scan-stored",
    );
    let checked = receipt_of(&app, &cast.axv, &check_in).await;
    assert_eq!(checked["idempotencyReplay"].as_bool(), Some(false));
    assert_eq!(count(&fixture, "commands").await, 3);
    assert_eq!(count(&fixture, "check_in_events").await, 1);

    // (b) The write grant dies while the seat itself stays ACTIVE.
    withdraw_attendance_grant(&fixture, "axv", "attendance.write").await;
    assert!(
        seat_active(&fixture, "axv").await,
        "withdrawing one grant must not deactivate the seat"
    );

    // (c) The byte-identical write replays are refused before the claim: no
    // receipt, no row, no command bookkeeping.
    for body in [&session_body, &registration] {
        let (status, answer) = post(&app, "/erp/attendance/commands", body, &cast.axv).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{answer}");
        assert_eq!(answer["reason"].as_str(), Some("forbidden"));
        assert!(answer.get("sessionId").is_none(), "no receipt: {answer}");
        assert!(
            answer.get("idempotencyReplay").is_none(),
            "no replay marker: {answer}"
        );
    }
    assert_eq!(count(&fixture, "sessions").await, 1);
    assert_eq!(count(&fixture, "rosters").await, 1);
    assert_eq!(count(&fixture, "commands").await, 3, "the claim never ran");
    assert_eq!(count(&fixture, "check_in_events").await, 1);

    // The check-in arm is a DIFFERENT permission: its stored receipt still
    // replays while `attendance.write` is withdrawn.
    let replay = receipt_of(&app, &cast.axv, &check_in).await;
    assert_eq!(replay["idempotencyReplay"].as_bool(), Some(true));
    assert_eq!(replay["rosterId"].as_str(), Some(roster_id.as_str()));

    // The grant returns: the same replay answers the stored receipt again.
    restore_attendance_grant(&fixture, "axv", "attendance.write").await;
    let replay = receipt_of(&app, &cast.axv, &session_body).await;
    assert_eq!(replay["idempotencyReplay"].as_bool(), Some(true));
    assert_eq!(replay["sessionId"].as_str(), Some(session_id.as_str()));
    assert_eq!(count(&fixture, "sessions").await, 1);

    // (d) The check-in permission closes the same way.
    withdraw_attendance_grant(&fixture, "axv", "attendance.checkin").await;
    let (status, answer) = post(&app, "/erp/attendance/commands", &check_in, &cast.axv).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{answer}");
    assert_eq!(answer["reason"].as_str(), Some("forbidden"));
    assert!(answer.get("rosterId").is_none(), "no receipt: {answer}");
    assert_eq!(count(&fixture, "check_in_events").await, 1);
    assert_eq!(count(&fixture, "commands").await, 3);
    // The write arm still replays — the closure is per-command, not per-seat.
    let replay = receipt_of(&app, &cast.axv, &session_body).await;
    assert_eq!(replay["idempotencyReplay"].as_bool(), Some(true));

    restore_attendance_grant(&fixture, "axv", "attendance.checkin").await;
    let replay = receipt_of(&app, &cast.axv, &check_in).await;
    assert_eq!(replay["idempotencyReplay"].as_bool(), Some(true));
    assert_eq!(replay["rosterId"].as_str(), Some(roster_id.as_str()));
    assert_eq!(count(&fixture, "check_in_events").await, 1);
}

#[tokio::test]
async fn narrow_attendance_grants_refuse_exactly_the_arms_they_lack() {
    let (fixture, app, cast) = served().await;

    // read+audit: the reads walk, the write and check-in arms refuse before
    // any statement.
    let (status, list) = get(
        &app,
        &format!("/erp/attendance/sessions?tenant={ACME}"),
        &cast.axg,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    refused_with(
        &app,
        &cast.axg,
        &create_body(
            ACME,
            "7c240400-9999-4999-9999-000000000a04",
            "NARROW-1",
            Some(5),
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
    refused_with(
        &app,
        &cast.axg,
        &check_in_body(
            ACME,
            "7c240400-9999-4999-9999-000000000a05",
            "7c240400-9999-4999-9999-000000000a06",
            "7c240400-9999-4999-9999-000000000a07",
            "scan-narrow",
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
    // `axg` holds `attendance.read` but never recorded this key, so the
    // recorded-initiator gate reduces the answer to the bare unknown.
    let (status, unknown) = get(
        &app,
        &format!(
            "/erp/attendance/commands/status?tenant={ACME}&requestId={}",
            uuid::Uuid::new_v4()
        ),
        &cast.axg,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unknown, json!({ "status": "unknown" }));

    // audit-only holds no `attendance.read` at all: every read gate refuses,
    // command status included.
    let (status, answer) = get(
        &app,
        &format!("/erp/attendance/sessions?tenant={ACME}"),
        &cast.axn,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{answer}");
    let (status, answer) = get(
        &app,
        &format!("/erp/attendance/access?tenant={ACME}"),
        &cast.axn,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the access read is the ladder itself"
    );
    assert_eq!(answer["permissions"], json!(["audit.read"]));
    let (status, answer) = get(
        &app,
        &format!(
            "/erp/attendance/commands/status?tenant={ACME}&requestId={}",
            uuid::Uuid::new_v4()
        ),
        &cast.axn,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{answer}");
    assert_eq!(answer["reason"].as_str(), Some("forbidden"));
    assert_eq!(count(&fixture, "sessions").await, 0);
    assert_eq!(count(&fixture, "commands").await, 0);
}

#[tokio::test]
async fn the_attendance_wire_boundary_and_read_projections_hold() {
    let (fixture, app, cast) = served().await;

    // The boundary parse: unknown arm, extra member, reversed window, zero
    // capacity — all the 400 class, and nothing recorded.
    for mutation in [
        json!({ "type": "delete_session", "commandId": KEY_WIRE_SESSION }),
        json!({
            "type": "create_session",
            "commandId": KEY_WIRE_SESSION,
            "title": "Wire session",
            "sessionCode": "WIRE-1",
            "startTime": START,
            "endTime": END,
            "extra": true
        }),
        json!({
            "type": "create_session",
            "commandId": KEY_WIRE_SESSION,
            "title": "Wire session",
            "sessionCode": "WIRE-1",
            "startTime": END,
            "endTime": START
        }),
        json!({
            "type": "create_session",
            "commandId": KEY_WIRE_SESSION,
            "title": "Wire session",
            "sessionCode": "WIRE-1",
            "capacity": 0,
            "startTime": START,
            "endTime": END
        }),
    ] {
        refused_with(
            &app,
            &cast.ax,
            &envelope(ACME, mutation),
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
    // The list bounds and the status vocabulary.
    for query in ["limit=0", "limit=101", "offset=-1", "status=unknown"] {
        let (status, answer) = get(
            &app,
            &format!("/erp/attendance/sessions?tenant={ACME}&{query}"),
            &cast.ax,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{query} must refuse: {answer}"
        );
    }

    // The read projections: unknown ids answer null/empty, never a
    // fabricated page.
    let unknown = uuid::Uuid::new_v4().to_string();
    let (status, detail) = get(
        &app,
        &format!("/erp/attendance/sessions/detail?tenant={ACME}&id={unknown}"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(detail.is_null(), "an unknown session is null");
    let (status, roster) = get(
        &app,
        &format!("/erp/attendance/roster?tenant={ACME}&sessionId={unknown}"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(roster, json!([]));
    let (status, check_ins) = get(
        &app,
        &format!("/erp/attendance/check-ins?tenant={ACME}&sessionId={unknown}"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(check_ins, json!([]));

    // The assigned scope sees only its own sessions: `axa` creates one,
    // `ax` creates another, and `axa`'s reads hide the other.
    let owned = session_of(
        &receipt_of(
            &app,
            &cast.axa,
            &create_body(ACME, KEY_WIRE_SECOND, "ASSIGNED-A", Some(5)),
        )
        .await,
    );
    let foreign = session_of(
        &receipt_of(
            &app,
            &cast.ax,
            &create_body(
                ACME,
                "7c240400-9999-4999-9999-000000000b03",
                "ASSIGNED-B",
                Some(5),
            ),
        )
        .await,
    );
    let (status, list) = get(
        &app,
        &format!("/erp/attendance/sessions?tenant={ACME}"),
        &cast.axa,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let rows = list.as_array().expect("an array");
    assert_eq!(rows.len(), 1, "the assigned scope lists only its own row");
    assert_eq!(rows[0]["id"].as_str(), Some(owned.as_str()));
    let (status, detail) = get(
        &app,
        &format!("/erp/attendance/sessions/detail?tenant={ACME}&id={foreign}"),
        &cast.axa,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(detail.is_null(), "the assigned scope hides the foreign row");
    let (status, roster) = get(
        &app,
        &format!("/erp/attendance/roster?tenant={ACME}&sessionId={foreign}"),
        &cast.axa,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(roster, json!([]), "and its roster page");
    // The all-scope manager sees both.
    let (status, list) = get(
        &app,
        &format!("/erp/attendance/sessions?tenant={ACME}"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().map(Vec::len), Some(2));
    assert_eq!(count(&fixture, "sessions").await, 2);

    // Nothing in the refused wire submissions was recorded.
    assert_eq!(count(&fixture, "commands").await, 2);
    assert_eq!(count(&fixture, "audit").await, 2);
}

// --- The replay is bound to the recorded initiator --------------------------------------------

const KEY_FOREIGN_REPLAY: &str = "7c240400-9999-4999-9999-000000000c01";

/// A rival holding the SAME `attendance.write` grant never collects
/// another actor's receipt: the claim compares the recorded `actor_id`
/// beside the fingerprint, so the refusal is the same CONFLICT class and
/// records nothing — while the initiating actor replays its own receipt
/// (A1 — `actor_id=$3`).
#[tokio::test]
async fn a_same_grant_rival_never_replays_another_actors_attendance_receipt() {
    let (fixture, app, cast) = served().await;

    let body = create_body(ACME, KEY_FOREIGN_REPLAY, "FOREIGN-1", Some(5));
    let first = receipt_of(&app, &cast.ax, &body).await;
    assert_eq!(first["idempotencyReplay"].as_bool(), Some(false));
    let session_id = session_of(&first);

    // `axv` holds ATTENDANCE_ALL on the same tenant and its membership is
    // active: the grant gate passes, the RECORDED INITIATOR refuses — the
    // body is byte-identical, so only the claimant differs.
    let refused = refused_with(&app, &cast.axv, &body, StatusCode::CONFLICT).await;
    assert_eq!(refused["reason"].as_str(), Some("conflict"), "{refused}");
    assert!(
        refused.get("receipt").is_none() && refused.get("sessionId").is_none(),
        "a foreign replay leaks no receipt: {refused}"
    );
    assert_eq!(
        count(&fixture, "commands").await,
        1,
        "no second command row"
    );
    assert_eq!(count(&fixture, "sessions").await, 1, "no second session");
    assert_eq!(count(&fixture, "audit").await, 1, "no foreign audit row");

    // The row names the actor that claimed it.
    let (actor,): (String,) = sqlx::query_as(
        "SELECT actor_id FROM archava_attendance.commands \
         WHERE tenant_id=$1 AND request_id=$2::uuid",
    )
    .bind(ACME)
    .bind(KEY_FOREIGN_REPLAY)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the command row answers");
    assert_eq!(actor, "ax", "the command records its initiator");

    // The initiating actor's own replay is untouched.
    let replay = receipt_of(&app, &cast.ax, &body).await;
    assert_eq!(replay["idempotencyReplay"].as_bool(), Some(true));
    assert_eq!(session_of(&replay), session_id);
}
