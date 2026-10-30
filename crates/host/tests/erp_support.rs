//! The R07 support cohort's host witnesses — driven through the real router
//! (`tower::ServiceExt::oneshot`) against the disposable PostgreSQL whose
//! `archava_support` schema is the real migration ledger
//! (`001_cases_notes_resolution`). Every assertion is a fact about what the
//! SERVER did:
//!
//! - the authorized case life ends with exactly the rows the repository's
//!   SQL spec freezes — the receipt documents, the case's state/cycle/
//!   revision chain, the notes, the transition per revision and the audit
//!   naming every operation once; the four read projections answer the same
//!   rows;
//! - a stale revision is a conflict before any write, and a refused arm
//!   records nothing;
//! - the closed state table refuses illegal moves (start twice, wait from
//!   open, close from waiting, resolve from open, cancel a closed case);
//! - a reopen starts a clean cycle (resolution and stamps cleared, cycle+1)
//!   while the transition keeps the previous resolution;
//! - assignment needs `cases.assign` AND tenant-wide scope, and the target
//!   must be an active human case reader;
//! - the assignee picker is tenant-wide, human, `cases.read`-holding and
//!   keyset-bounded; a scoped caller is refused;
//! - the `assigned` scope narrows every read (list, detail), a case becomes
//!   visible through assignment, and a cross-tenant caller is refused;
//! - the same command id with the same bytes replays with `replayed: true`
//!   and still one row everywhere; a changed payload is a conflict; the
//!   status read answers `committed`/`unknown` and refuses another actor's
//!   id;
//! - a revoked membership loses the write and the status read, and the
//!   restored grant replays the stored receipt;
//! - the history is append-only (the triggers refuse UPDATE/DELETE) and
//!   complete (one transition + one audit per case revision, bound to the
//!   receipt);
//! - the wire boundary (unknown arm, extra member, zero revision, invalid
//!   state/priority, list bounds, unknown query member) refuses at the
//!   right layer, and notes/audit are permission-gated.
//!
//! Zone-free by construction: the suite's only run-relative facts are the
//! SQL-stamped instants the clock binds — read back as bytes and counted,
//! not predicted.

use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

/// The commerce ledger this host serves as the primary — the same table
/// list every ERP suite pins; the support surface RIDES the same serving
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

const ACME: &str = "acme-events";
const GLOBEX: &str = "globex-events";

/// The support permission vocabulary (migration 001's CHECK).
const SUPPORT_ALL: &[&str] = &[
    "cases.read",
    "cases.write",
    "cases.assign",
    "notes.read",
    "notes.write",
    "audit.read",
];

/// `ax` is the full-grant manager with scope all — the positive-path actor.
/// `axg` holds the read histories (`cases.read`, `notes.read`, `audit.read`)
/// but no writes. `axno` holds the writes and note grants but NOT
/// `cases.assign`. `axnr` holds only `cases.read`, so every history member
/// answers null. `axn` holds only `audit.read`, so the case read itself is
/// refused. `axv` holds the full grants but the test revokes it. `axa` holds
/// the full grants with scope `assigned`. `other` holds the full grants in
/// GLOBEX only.
async fn seed(fixture: &Fixture) {
    for principal in ["ax", "axg", "axno", "axnr", "axn", "axv", "axa", "other"] {
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
        ("ax", SUPPORT_ALL, "all"),
        (
            "axg",
            &["cases.read", "notes.read", "audit.read"][..],
            "all",
        ),
        (
            "axno",
            &[
                "cases.read",
                "cases.write",
                "notes.read",
                "notes.write",
                "audit.read",
            ][..],
            "all",
        ),
        ("axnr", &["cases.read"][..], "all"),
        ("axn", &["audit.read"][..], "all"),
        ("axv", SUPPORT_ALL, "all"),
        ("axa", SUPPORT_ALL, "assigned"),
    ] {
        sqlx::query(
            "INSERT INTO archava_support.memberships \
             (tenant_id, principal_id, display_name, kind, permissions, scope, active) \
             VALUES ($1, $2, $3, 'human', $4, $5, true)",
        )
        .bind(ACME)
        .bind(principal)
        .bind(format!("Display {principal}"))
        .bind(permissions.to_vec())
        .bind(scope)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the ACME support membership");
    }
    sqlx::query(
        "INSERT INTO archava_support.memberships \
         (tenant_id, principal_id, display_name, kind, permissions, scope, active) \
         VALUES ($1, $2, 'Other Display', 'human', $3, 'all', true)",
    )
    .bind(GLOBEX)
    .bind("other")
    .bind(SUPPORT_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the GLOBEX support membership");
}

// --- Session plumbing ---------------------------------------------------------

struct Session {
    token: String,
    csrf: String,
}

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

/// The fixture budget: every witness owns a full disposable PostgreSQL and
/// a runtime pool, so the suite keeps only four ALIVE at once. The support
/// cohort's eight-principal cast and twelve witnesses otherwise exhaust the
/// host's process/connection budget and turn a late test's first request
/// into a pool timeout — a harness fact, not a product one.
static LIVE_FIXTURES: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);

struct Cast {
    ax: Session,
    axg: Session,
    axno: Session,
    axnr: Session,
    axn: Session,
    axv: Session,
    axa: Session,
    other: Session,
    _live: tokio::sync::SemaphorePermit<'static>,
}

async fn served() -> (Fixture, axum::Router, Cast) {
    let live = LIVE_FIXTURES
        .acquire()
        .await
        .expect("the fixture budget is never closed");
    let fixture = provision().await;
    seed(&fixture).await;
    archava_adapters::erp::support::assert_support_kernel(
        &fixture.runtime_pool,
        &fixture.support_ledger,
    )
    .await
    .expect("the support kernel asserts on the fixture");
    let app = archava_host::router(state(&fixture).await);
    let ax = login(&app, "ax").await;
    let axg = login(&app, "axg").await;
    let axno = login(&app, "axno").await;
    let axnr = login(&app, "axnr").await;
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
            axno,
            axnr,
            axn,
            axv,
            axa,
            other,
            _live: live,
        },
    )
}

// --- The submission documents --------------------------------------------------

fn envelope(tenant: &str, mutation: Value) -> String {
    json!({ "tenant": tenant, "mutation": mutation }).to_string()
}

fn create_body(tenant: &str, request_id: &str, subject: &str) -> String {
    envelope(
        tenant,
        json!({
            "requestId": request_id,
            "command": {
                "operation": "case.create",
                "requesterName": "Ada Lovelace",
                "subject": subject,
                "description": "The printer reports a paper jam on every run.",
                "priority": "normal",
                "dueAt": null
            }
        }),
    )
}

fn create_body_with_priority(tenant: &str, request_id: &str, priority: &str) -> String {
    envelope(
        tenant,
        json!({
            "requestId": request_id,
            "command": {
                "operation": "case.create",
                "requesterName": "Ada Lovelace",
                "subject": "A printer with a priority",
                "description": "The description is long enough.",
                "priority": priority,
                "dueAt": null
            }
        }),
    )
}

fn arm_body(tenant: &str, request_id: &str, command: Value) -> String {
    envelope(
        tenant,
        json!({ "requestId": request_id, "command": command }),
    )
}

fn transition_body(
    tenant: &str,
    request_id: &str,
    operation: &str,
    case_id: &str,
    revision: i64,
    reason: &str,
) -> String {
    arm_body(
        tenant,
        request_id,
        json!({
            "operation": operation,
            "id": case_id,
            "expectedRevision": revision,
            "reason": reason
        }),
    )
}

async fn receipt_of(app: &axum::Router, session: &Session, body: &str) -> Value {
    let (status, receipt) = post(app, "/erp/support/commands", body, session).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the arm must commit: {receipt} — body {body}"
    );
    receipt
}

async fn refused_with(
    app: &axum::Router,
    session: &Session,
    body: &str,
    expected: StatusCode,
) -> Value {
    let (status, answer) = post(app, "/erp/support/commands", body, session).await;
    assert_eq!(
        status,
        expected,
        "the arm must refuse with {}: {answer} — body {body}",
        expected.as_u16()
    );
    answer
}

fn case_of(receipt: &Value) -> String {
    receipt["id"]
        .as_str()
        .expect("the receipt names its case")
        .to_string()
}

async fn detail_of(app: &axum::Router, session: &Session, case_id: &str) -> Value {
    let (status, detail) = get(
        app,
        &format!("/erp/support/cases/detail?tenant={ACME}&id={case_id}"),
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

async fn count(fixture: &Fixture, table: &str) -> i64 {
    let statement = format!("SELECT count(*)::bigint FROM archava_support.{table}");
    let (count,): (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(statement.as_str()))
        .fetch_one(&fixture.admin_pool)
        .await
        .expect("count the rows");
    count
}

async fn case_row(fixture: &Fixture, case_id: &str) -> (String, i64, i64, Option<String>) {
    sqlx::query_as(
        "SELECT state, cycle::bigint, revision::bigint, resolution FROM archava_support.cases \
         WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(uuid::Uuid::parse_str(case_id).expect("a grammar-admitted uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("read the case row")
}

// --- The witnesses ------------------------------------------------------------

#[tokio::test]
async fn authorized_support_life_documents_the_case_and_history() {
    let (fixture, app, cast) = served().await;

    // 1. create — open, revision 1, unassigned, cycle 1.
    let created = receipt_of(
        &app,
        &cast.ax,
        &create_body(ACME, "10000000-0000-4000-8000-000000000001", "Printer jam"),
    )
    .await;
    assert_eq!(created["operation"].as_str(), Some("case.create"));
    assert_eq!(created["revision"].as_i64(), Some(1));
    assert_eq!(created["replayed"].as_bool(), Some(false));
    assert_eq!(
        created["requestId"].as_str(),
        Some("10000000-0000-4000-8000-000000000001")
    );
    let case_id = case_of(&created);

    // 2..8 — the whole authorized life.
    let started = receipt_of(
        &app,
        &cast.ax,
        &transition_body(
            ACME,
            "10000000-0000-4000-8000-000000000002",
            "case.start",
            &case_id,
            1,
            "triage",
        ),
    )
    .await;
    assert_eq!(started["revision"].as_i64(), Some(2));
    let waited = receipt_of(
        &app,
        &cast.ax,
        &transition_body(
            ACME,
            "10000000-0000-4000-8000-000000000003",
            "case.wait",
            &case_id,
            2,
            "waiting on the vendor",
        ),
    )
    .await;
    assert_eq!(waited["revision"].as_i64(), Some(3));
    let resumed = receipt_of(
        &app,
        &cast.ax,
        &transition_body(
            ACME,
            "10000000-0000-4000-8000-000000000004",
            "case.resume",
            &case_id,
            3,
            "vendor replied",
        ),
    )
    .await;
    assert_eq!(resumed["revision"].as_i64(), Some(4));
    let noted = receipt_of(
        &app,
        &cast.ax,
        &arm_body(
            ACME,
            "10000000-0000-4000-8000-000000000005",
            json!({
                "operation": "note.add",
                "id": case_id,
                "expectedRevision": 4,
                "body": "Replaced the feed roller."
            }),
        ),
    )
    .await;
    assert_eq!(noted["revision"].as_i64(), Some(5));
    let prioritized = receipt_of(
        &app,
        &cast.ax,
        &arm_body(
            ACME,
            "10000000-0000-4000-8000-000000000006",
            json!({
                "operation": "case.set-priority",
                "id": case_id,
                "expectedRevision": 5,
                "priority": "high",
                "reason": "customer escalation"
            }),
        ),
    )
    .await;
    assert_eq!(prioritized["revision"].as_i64(), Some(6));
    let resolved = receipt_of(
        &app,
        &cast.ax,
        &arm_body(
            ACME,
            "10000000-0000-4000-8000-000000000007",
            json!({
                "operation": "case.resolve",
                "id": case_id,
                "expectedRevision": 6,
                "resolution": "Roller replaced and tested."
            }),
        ),
    )
    .await;
    assert_eq!(resolved["revision"].as_i64(), Some(7));
    let closed = receipt_of(
        &app,
        &cast.ax,
        &transition_body(
            ACME,
            "10000000-0000-4000-8000-000000000008",
            "case.close",
            &case_id,
            7,
            "confirmed with the requester",
        ),
    )
    .await;
    assert_eq!(closed["revision"].as_i64(), Some(8));

    // The stored chain.
    let (state, cycle, revision, resolution) = case_row(&fixture, &case_id).await;
    assert_eq!(state, "closed");
    assert_eq!(cycle, 1);
    assert_eq!(revision, 8);
    assert_eq!(resolution.as_deref(), Some("Roller replaced and tested."));

    // The detail read answers the same record and all three histories.
    let detail = detail_of(&app, &cast.ax, &case_id).await;
    assert_eq!(detail["record"]["state"], "closed");
    assert_eq!(detail["record"]["revision"].as_i64(), Some(8));
    assert_eq!(detail["record"]["priority"], "high");
    assert_eq!(detail["record"]["cycle"].as_i64(), Some(1));
    assert_eq!(detail["record"]["overdue"].as_bool(), Some(false));
    assert!(detail["record"]["resolvedAt"].as_str().is_some());
    assert!(detail["record"]["closedAt"].as_str().is_some());
    assert_eq!(detail["notes"].as_array().map(Vec::len), Some(1));
    assert_eq!(detail["transitions"].as_array().map(Vec::len), Some(8));
    assert_eq!(detail["audit"].as_array().map(Vec::len), Some(8));
    assert_eq!(detail["notesHasMore"].as_bool(), Some(false));
    assert_eq!(detail["transitionsHasMore"].as_bool(), Some(false));
    assert_eq!(detail["auditHasMore"].as_bool(), Some(false));
    assert_eq!(detail["notes"][0]["body"], "Replaced the feed roller.");
    assert_eq!(detail["transitions"][0]["operation"], "case.close");
    assert_eq!(detail["transitions"][0]["from"], "resolved");
    assert_eq!(detail["transitions"][0]["to"], "closed");
    assert_eq!(detail["transitions"][0]["cycle"].as_i64(), Some(1));

    // The worklist and the access envelope.
    let (status, page) = get(&app, &format!("/erp/support/cases?tenant={ACME}"), &cast.ax).await;
    assert_eq!(status, StatusCode::OK, "the worklist answers: {page}");
    assert_eq!(page["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(page["items"][0]["id"], case_id);
    assert_eq!(page["items"][0]["state"], "closed");
    assert!(page["next"].is_null());
    assert!(page["evaluatedAt"].as_str().is_some());

    let (status, access) = get(
        &app,
        &format!("/erp/support/access?tenant={ACME}"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(access["ownerId"], "ax");
    assert_eq!(access["scope"], "all");
    let permissions = access["permissions"]
        .as_array()
        .expect("the access carries permissions");
    for permission in SUPPORT_ALL {
        assert!(
            permissions.iter().any(|held| held == permission),
            "the full grant includes {permission}: {access}"
        );
    }

    // Every audit row names its operation exactly once.
    let operations: Vec<(String,)> = sqlx::query_as(
        "SELECT operation FROM archava_support.audit WHERE tenant_id=$1 AND record_id=$2 \
         ORDER BY revision",
    )
    .bind(ACME)
    .bind(uuid::Uuid::parse_str(&case_id).expect("uuid"))
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("read the audit");
    let operations: Vec<String> = operations
        .into_iter()
        .map(|(operation,)| operation)
        .collect();
    assert_eq!(operations.len(), 8);
    assert_eq!(operations[0], "case.create");
    assert_eq!(operations[7], "case.close");
}

#[tokio::test]
async fn a_stale_revision_conflicts_and_records_nothing() {
    let (fixture, app, cast) = served().await;
    let created = receipt_of(
        &app,
        &cast.ax,
        &create_body(ACME, "20000000-0000-4000-8000-000000000001", "Stale"),
    )
    .await;
    let case_id = case_of(&created);

    // Expected 2 while the case is at 1: a conflict, no history.
    let refusal = refused_with(
        &app,
        &cast.ax,
        &transition_body(
            ACME,
            "20000000-0000-4000-8000-000000000002",
            "case.start",
            &case_id,
            2,
            "wrong base",
        ),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(refusal["reason"], "conflict");
    assert_eq!(count(&fixture, "audit").await, 1);
    assert_eq!(count(&fixture, "transitions").await, 1);
    assert_eq!(count(&fixture, "commands").await, 1);

    // The correct base commits.
    let started = receipt_of(
        &app,
        &cast.ax,
        &transition_body(
            ACME,
            "20000000-0000-4000-8000-000000000003",
            "case.start",
            &case_id,
            1,
            "right base",
        ),
    )
    .await;
    assert_eq!(started["revision"].as_i64(), Some(2));
    let (state, _, revision, _) = case_row(&fixture, &case_id).await;
    assert_eq!(state, "in_progress");
    assert_eq!(revision, 2);
    assert_eq!(count(&fixture, "audit").await, 2);
    assert_eq!(count(&fixture, "transitions").await, 2);
}

#[tokio::test]
async fn the_state_table_refuses_illegal_moves() {
    let (fixture, app, cast) = served().await;
    let created = receipt_of(
        &app,
        &cast.ax,
        &create_body(ACME, "30000000-0000-4000-8000-000000000001", "Illegal"),
    )
    .await;
    let case_id = case_of(&created);

    // Open: close, resolve and wait are outside the table.
    for (index, operation) in ["case.close", "case.resolve", "case.wait"]
        .iter()
        .enumerate()
    {
        let body = if *operation == "case.resolve" {
            arm_body(
                ACME,
                &format!("30000000-0000-4000-8000-00000000001{}", index + 2),
                json!({
                    "operation": operation,
                    "id": case_id,
                    "expectedRevision": 1,
                    "resolution": "premature"
                }),
            )
        } else {
            transition_body(
                ACME,
                &format!("30000000-0000-4000-8000-00000000001{}", index + 2),
                operation,
                &case_id,
                1,
                "premature",
            )
        };
        refused_with(&app, &cast.ax, &body, StatusCode::CONFLICT).await;
    }
    assert_eq!(count(&fixture, "audit").await, 1);

    // Start, then start again: the second is outside the table.
    receipt_of(
        &app,
        &cast.ax,
        &transition_body(
            ACME,
            "30000000-0000-4000-8000-000000000010",
            "case.start",
            &case_id,
            1,
            "go",
        ),
    )
    .await;
    refused_with(
        &app,
        &cast.ax,
        &transition_body(
            ACME,
            "30000000-0000-4000-8000-000000000011",
            "case.start",
            &case_id,
            2,
            "again",
        ),
        StatusCode::CONFLICT,
    )
    .await;

    // Wait, then cancel; the closed/cancelled lifecycle refuses the edits.
    receipt_of(
        &app,
        &cast.ax,
        &transition_body(
            ACME,
            "30000000-0000-4000-8000-000000000012",
            "case.wait",
            &case_id,
            2,
            "wait",
        ),
    )
    .await;
    receipt_of(
        &app,
        &cast.ax,
        &transition_body(
            ACME,
            "30000000-0000-4000-8000-000000000013",
            "case.cancel",
            &case_id,
            3,
            "duplicate",
        ),
    )
    .await;
    for (index, operation) in ["case.start", "case.set-priority", "case.assign"]
        .iter()
        .enumerate()
    {
        let body = match *operation {
            "case.start" => transition_body(
                ACME,
                &format!("30000000-0000-4000-8000-00000000002{index}"),
                operation,
                &case_id,
                4,
                "no",
            ),
            "case.set-priority" => arm_body(
                ACME,
                &format!("30000000-0000-4000-8000-00000000002{index}"),
                json!({
                    "operation": operation,
                    "id": case_id,
                    "expectedRevision": 4,
                    "priority": "high",
                    "reason": "no"
                }),
            ),
            _ => arm_body(
                ACME,
                &format!("30000000-0000-4000-8000-00000000002{index}"),
                json!({
                    "operation": operation,
                    "id": case_id,
                    "expectedRevision": 4,
                    "assigneeId": "axg",
                    "reason": "no"
                }),
            ),
        };
        refused_with(&app, &cast.ax, &body, StatusCode::CONFLICT).await;
    }
    let (state, _, revision, _) = case_row(&fixture, &case_id).await;
    assert_eq!(state, "cancelled");
    assert_eq!(revision, 4);
}

#[tokio::test]
async fn a_reopen_starts_a_clean_cycle() {
    let (fixture, app, cast) = served().await;
    let created = receipt_of(
        &app,
        &cast.ax,
        &create_body(ACME, "40000000-0000-4000-8000-000000000001", "Reopen"),
    )
    .await;
    let case_id = case_of(&created);
    receipt_of(
        &app,
        &cast.ax,
        &transition_body(
            ACME,
            "40000000-0000-4000-8000-000000000002",
            "case.start",
            &case_id,
            1,
            "go",
        ),
    )
    .await;
    receipt_of(
        &app,
        &cast.ax,
        &arm_body(
            ACME,
            "40000000-0000-4000-8000-000000000003",
            json!({
                "operation": "case.resolve",
                "id": case_id,
                "expectedRevision": 2,
                "resolution": "first cycle resolution"
            }),
        ),
    )
    .await;

    let reopened = receipt_of(
        &app,
        &cast.ax,
        &arm_body(
            ACME,
            "40000000-0000-4000-8000-000000000004",
            json!({
                "operation": "case.reopen",
                "id": case_id,
                "expectedRevision": 3,
                "reason": "the customer returned",
                "dueAt": "2026-11-15T09:00:00+07:00"
            }),
        ),
    )
    .await;
    assert_eq!(reopened["revision"].as_i64(), Some(4));
    let (state, cycle, revision, resolution) = case_row(&fixture, &case_id).await;
    assert_eq!(state, "open");
    assert_eq!(cycle, 2);
    assert_eq!(revision, 4);
    assert!(resolution.is_none());

    let detail = detail_of(&app, &cast.ax, &case_id).await;
    assert!(detail["record"]["resolvedAt"].is_null());
    assert!(detail["record"]["closedAt"].is_null());
    assert_eq!(
        detail["record"]["dueAt"].as_str(),
        Some("2026-11-15T02:00:00.000Z")
    );
    let latest = &detail["transitions"][0];
    assert_eq!(latest["operation"], "case.reopen");
    assert_eq!(latest["from"], "resolved");
    assert_eq!(latest["to"], "open");
    assert_eq!(latest["previousResolution"], "first cycle resolution");
    assert!(latest["resolution"].is_null());
    assert_eq!(latest["cycle"].as_i64(), Some(2));

    // A second reopen (already open) is outside the table.
    refused_with(
        &app,
        &cast.ax,
        &arm_body(
            ACME,
            "40000000-0000-4000-8000-000000000005",
            json!({
                "operation": "case.reopen",
                "id": case_id,
                "expectedRevision": 4,
                "reason": "again",
                "dueAt": null
            }),
        ),
        StatusCode::CONFLICT,
    )
    .await;
}

#[tokio::test]
async fn assignment_requires_the_grant_the_scope_and_a_human_reader() {
    let (fixture, app, cast) = served().await;
    let created = receipt_of(
        &app,
        &cast.ax,
        &create_body(ACME, "50000000-0000-4000-8000-000000000001", "Assign"),
    )
    .await;
    let case_id = case_of(&created);

    // No cases.assign.
    refused_with(
        &app,
        &cast.axno,
        &arm_body(
            ACME,
            "50000000-0000-4000-8000-000000000002",
            json!({
                "operation": "case.assign",
                "id": case_id,
                "expectedRevision": 1,
                "assigneeId": "axg",
                "reason": "no grant"
            }),
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
    // Scoped caller: the TS requires tenant-wide scope for assignment.
    refused_with(
        &app,
        &cast.axa,
        &arm_body(
            ACME,
            "50000000-0000-4000-8000-000000000003",
            json!({
                "operation": "case.assign",
                "id": case_id,
                "expectedRevision": 1,
                "assigneeId": "axg",
                "reason": "scoped"
            }),
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
    // Not an active human case reader.
    refused_with(
        &app,
        &cast.ax,
        &arm_body(
            ACME,
            "50000000-0000-4000-8000-000000000004",
            json!({
                "operation": "case.assign",
                "id": case_id,
                "expectedRevision": 1,
                "assigneeId": "axn",
                "reason": "readerless"
            }),
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
    // Unknown principal.
    refused_with(
        &app,
        &cast.ax,
        &arm_body(
            ACME,
            "50000000-0000-4000-8000-000000000005",
            json!({
                "operation": "case.assign",
                "id": case_id,
                "expectedRevision": 1,
                "assigneeId": "nobody",
                "reason": "unknown"
            }),
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
    assert_eq!(count(&fixture, "audit").await, 1);

    // The valid assignment.
    let assigned = receipt_of(
        &app,
        &cast.ax,
        &arm_body(
            ACME,
            "50000000-0000-4000-8000-000000000006",
            json!({
                "operation": "case.assign",
                "id": case_id,
                "expectedRevision": 1,
                "assigneeId": "axg",
                "reason": "take it"
            }),
        ),
    )
    .await;
    assert_eq!(assigned["revision"].as_i64(), Some(2));
    let detail = detail_of(&app, &cast.ax, &case_id).await;
    assert_eq!(detail["record"]["assigneeId"], "axg");
    assert!(detail["transitions"][0]["previousAssigneeId"].is_null());
    assert_eq!(detail["transitions"][0]["assigneeId"], "axg");

    // Unassigning is the same arm with a null target.
    let unassigned = receipt_of(
        &app,
        &cast.ax,
        &arm_body(
            ACME,
            "50000000-0000-4000-8000-000000000007",
            json!({
                "operation": "case.assign",
                "id": case_id,
                "expectedRevision": 2,
                "assigneeId": null,
                "reason": "back to the queue"
            }),
        ),
    )
    .await;
    assert_eq!(unassigned["revision"].as_i64(), Some(3));
    let detail = detail_of(&app, &cast.ax, &case_id).await;
    assert!(detail["record"]["assigneeId"].is_null());
}

#[tokio::test]
async fn the_assignee_picker_is_tenant_wide_and_bounded() {
    let (_fixture, app, cast) = served().await;
    let (status, page) = get(
        &app,
        &format!("/erp/support/assignees?tenant={ACME}"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the picker answers: {page}");
    let items = page["items"].as_array().expect("items");
    let ids: Vec<&str> = items
        .iter()
        .map(|item| item["id"].as_str().expect("an id"))
        .collect();
    assert_eq!(ids, vec!["ax", "axa", "axg", "axno", "axnr", "axv"]);
    assert!(!ids.contains(&"axn"), "audit-only is not a case reader");
    assert!(
        !ids.contains(&"other"),
        "the picker never crosses the tenant boundary"
    );
    assert_eq!(items[0]["displayName"], "Display ax");

    // Keyset paging: two at a time.
    let (status, first) = get(
        &app,
        &format!("/erp/support/assignees?tenant={ACME}&limit=2"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["items"].as_array().map(Vec::len), Some(2));
    let next = first["next"].as_str().expect("a next cursor");
    assert_eq!(next, "axa");
    let (status, second) = get(
        &app,
        &format!("/erp/support/assignees?tenant={ACME}&limit=2&after={next}"),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["items"][0]["id"], "axg");

    // The picker needs cases.assign and tenant-wide scope.
    let (status, refusal) = get(
        &app,
        &format!("/erp/support/assignees?tenant={ACME}"),
        &cast.axg,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "no grant: {refusal}");
    let (status, refusal) = get(
        &app,
        &format!("/erp/support/assignees?tenant={ACME}"),
        &cast.axa,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "scoped: {refusal}");
}

#[tokio::test]
async fn the_scope_narrows_every_read_and_a_cross_tenant_caller_is_refused() {
    let (_fixture, app, cast) = served().await;
    let ax_case = case_of(
        &receipt_of(
            &app,
            &cast.ax,
            &create_body(ACME, "60000000-0000-4000-8000-000000000001", "ax case"),
        )
        .await,
    );
    let axa_case = case_of(
        &receipt_of(
            &app,
            &cast.axa,
            &create_body(ACME, "60000000-0000-4000-8000-000000000002", "axa case"),
        )
        .await,
    );

    // ax sees both; axa sees only its own.
    let (status, page) = get(&app, &format!("/erp/support/cases?tenant={ACME}"), &cast.ax).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["items"].as_array().map(Vec::len), Some(2));
    let (status, page) = get(
        &app,
        &format!("/erp/support/cases?tenant={ACME}"),
        &cast.axa,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(page["items"][0]["id"], axa_case);

    // The detail is scoped the same way: a hidden case is 404.
    let (status, refusal) = get(
        &app,
        &format!("/erp/support/cases/detail?tenant={ACME}&id={ax_case}"),
        &cast.axa,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "hidden: {refusal}");
    let (status, _) = get(
        &app,
        &format!("/erp/support/cases/detail?tenant={ACME}&id={axa_case}"),
        &cast.axa,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Assignment makes the case visible to the scoped reader.
    receipt_of(
        &app,
        &cast.ax,
        &arm_body(
            ACME,
            "60000000-0000-4000-8000-000000000003",
            json!({
                "operation": "case.assign",
                "id": ax_case,
                "expectedRevision": 1,
                "assigneeId": "axa",
                "reason": "hand off"
            }),
        ),
    )
    .await;
    let (status, page) = get(
        &app,
        &format!("/erp/support/cases?tenant={ACME}"),
        &cast.axa,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["items"].as_array().map(Vec::len), Some(2));

    // A GLOBEX membership cannot act on ACME.
    let (status, refusal) = post(
        &app,
        "/erp/support/commands",
        &create_body(ACME, "60000000-0000-4000-8000-000000000004", "cross tenant"),
        &cast.other,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "cross tenant: {refusal}");
    let (status, refusal) = get(
        &app,
        &format!("/erp/support/cases?tenant={ACME}"),
        &cast.other,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "cross tenant read: {refusal}"
    );
}

#[tokio::test]
async fn a_duplicate_command_id_replays_and_a_changed_payload_conflicts() {
    let (fixture, app, cast) = served().await;
    let body = create_body(ACME, "70000000-0000-4000-8000-000000000001", "Idempotent");
    let first = receipt_of(&app, &cast.ax, &body).await;
    let case_id = case_of(&first);
    assert_eq!(first["replayed"].as_bool(), Some(false));

    // Same id, same bytes: replay, same case, no second row.
    let replay = receipt_of(&app, &cast.ax, &body).await;
    assert_eq!(replay["replayed"].as_bool(), Some(true));
    assert_eq!(replay["id"], case_id);
    assert_eq!(replay["revision"].as_i64(), Some(1));
    assert_eq!(count(&fixture, "cases").await, 1);
    assert_eq!(count(&fixture, "audit").await, 1);
    assert_eq!(count(&fixture, "transitions").await, 1);

    // Same id, different payload: conflict, no second case.
    let changed = create_body(
        ACME,
        "70000000-0000-4000-8000-000000000001",
        "A different subject",
    );
    let refusal = refused_with(&app, &cast.ax, &changed, StatusCode::CONFLICT).await;
    assert_eq!(refusal["reason"], "conflict");
    assert_eq!(count(&fixture, "cases").await, 1);

    // The status read: committed, then unknown.
    let (status, answer) = get(
        &app,
        &format!(
            "/erp/support/commands/status?tenant={ACME}&requestId=70000000-0000-4000-8000-000000000001"
        ),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "status: {answer}");
    assert_eq!(answer["status"], "committed");
    assert_eq!(answer["receipt"]["replayed"].as_bool(), Some(true));
    assert_eq!(answer["receipt"]["id"], case_id);
    let (status, answer) = get(
        &app,
        &format!(
            "/erp/support/commands/status?tenant={ACME}&requestId=70000000-0000-4000-8000-000000000099"
        ),
        &cast.ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(answer["status"], "unknown");

    // Another actor cannot read this command's status.
    let (status, refusal) = get(
        &app,
        &format!(
            "/erp/support/commands/status?tenant={ACME}&requestId=70000000-0000-4000-8000-000000000001"
        ),
        &cast.axg,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "actor mismatch: {refusal}");
}

#[tokio::test]
async fn a_revoked_grant_loses_the_write_and_the_status_and_restore_replays() {
    let (fixture, app, cast) = served().await;
    let created = receipt_of(
        &app,
        &cast.axv,
        &create_body(ACME, "80000000-0000-4000-8000-000000000001", "Revoked"),
    )
    .await;
    let case_id = case_of(&created);
    let start = transition_body(
        ACME,
        "80000000-0000-4000-8000-000000000002",
        "case.start",
        &case_id,
        1,
        "go",
    );
    sqlx::query("UPDATE archava_support.memberships SET active=false WHERE tenant_id=$1 AND principal_id=$2")
        .bind(ACME)
        .bind("axv")
        .execute(&fixture.admin_pool)
        .await
        .expect("revoke the membership");

    refused_with(&app, &cast.axv, &start, StatusCode::FORBIDDEN).await;
    let (status, refusal) = get(
        &app,
        &format!(
            "/erp/support/commands/status?tenant={ACME}&requestId=80000000-0000-4000-8000-000000000002"
        ),
        &cast.axv,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "revoked status: {refusal}");
    assert_eq!(count(&fixture, "audit").await, 1);

    sqlx::query(
        "UPDATE archava_support.memberships SET active=true WHERE tenant_id=$1 AND principal_id=$2",
    )
    .bind(ACME)
    .bind("axv")
    .execute(&fixture.admin_pool)
    .await
    .expect("restore the membership");
    let started = receipt_of(&app, &cast.axv, &start).await;
    assert_eq!(started["revision"].as_i64(), Some(2));
    let (status, answer) = get(
        &app,
        &format!(
            "/erp/support/commands/status?tenant={ACME}&requestId=80000000-0000-4000-8000-000000000002"
        ),
        &cast.axv,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(answer["status"], "committed");
    assert_eq!(answer["receipt"]["replayed"].as_bool(), Some(true));
    assert_eq!(answer["receipt"]["revision"].as_i64(), Some(2));
}

#[tokio::test]
async fn the_wire_boundary_and_the_read_bounds_refuse() {
    let (fixture, app, cast) = served().await;

    // Unknown arm.
    refused_with(
        &app,
        &cast.ax,
        &arm_body(
            ACME,
            "90000000-0000-4000-8000-000000000001",
            json!({"operation": "case.teleport", "id": "90000000-0000-4000-8000-000000000099"}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    // Extra member in a valid arm.
    refused_with(
        &app,
        &cast.ax,
        &envelope(
            ACME,
            json!({
                "requestId": "90000000-0000-4000-8000-000000000002",
                "command": {
                    "operation": "case.create",
                    "requesterName": "Ada",
                    "subject": "Extra",
                    "description": "Extra member",
                    "priority": "normal",
                    "dueAt": null,
                    "extra": true
                }
            }),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    // Zero revision and an invalid priority.
    refused_with(
        &app,
        &cast.ax,
        &arm_body(
            ACME,
            "90000000-0000-4000-8000-000000000003",
            json!({
                "operation": "case.start",
                "id": "90000000-0000-4000-8000-000000000099",
                "expectedRevision": 0,
                "reason": "go"
            }),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    refused_with(
        &app,
        &cast.ax,
        &create_body_with_priority(ACME, "90000000-0000-4000-8000-000000000004", "urgent"),
        StatusCode::BAD_REQUEST,
    )
    .await;
    // A write grant the actor does not hold.
    refused_with(
        &app,
        &cast.axg,
        &create_body(ACME, "90000000-0000-4000-8000-000000000005", "no write"),
        StatusCode::FORBIDDEN,
    )
    .await;
    // Note without notes.write.
    let note = envelope(
        ACME,
        json!({
            "requestId": "90000000-0000-4000-8000-000000000006",
            "command": {
                "operation": "note.add",
                "id": "90000000-0000-4000-8000-000000000099",
                "expectedRevision": 1,
                "body": "not allowed"
            }
        }),
    );
    refused_with(&app, &cast.axg, &note, StatusCode::FORBIDDEN).await;

    // Query bounds and unknown members.
    for uri in [
        format!("/erp/support/cases?tenant={ACME}&limit=0"),
        format!("/erp/support/cases?tenant={ACME}&limit=101"),
        format!("/erp/support/cases?tenant={ACME}&state=unknown"),
        format!("/erp/support/cases?tenant={ACME}&priority=urgent"),
        format!("/erp/support/cases?tenant={ACME}&extra=1"),
        format!("/erp/support/cases?tenant={ACME}&after=not-a-uuid"),
        format!("/erp/support/assignees?tenant={ACME}&limit=0"),
        format!("/erp/support/cases/detail?tenant={ACME}&id=not-a-uuid"),
    ] {
        let (status, answer) = get(&app, &uri, &cast.ax).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{uri} must refuse at the wire: {answer}"
        );
    }
    assert_eq!(count(&fixture, "cases").await, 0);
    assert_eq!(count(&fixture, "commands").await, 0);
}

#[tokio::test]
async fn the_history_is_append_only_and_complete() {
    let (fixture, app, cast) = served().await;
    let created = receipt_of(
        &app,
        &cast.ax,
        &create_body(ACME, "a0000000-0000-4000-8000-000000000001", "History"),
    )
    .await;
    let case_id = case_of(&created);
    receipt_of(
        &app,
        &cast.ax,
        &transition_body(
            ACME,
            "a0000000-0000-4000-8000-000000000002",
            "case.start",
            &case_id,
            1,
            "go",
        ),
    )
    .await;
    receipt_of(
        &app,
        &cast.ax,
        &arm_body(
            ACME,
            "a0000000-0000-4000-8000-000000000003",
            json!({
                "operation": "note.add",
                "id": case_id,
                "expectedRevision": 2,
                "body": "a note"
            }),
        ),
    )
    .await;

    assert_eq!(count(&fixture, "audit").await, 3);
    assert_eq!(count(&fixture, "transitions").await, 3);
    assert_eq!(count(&fixture, "notes").await, 1);
    let (state, _, revision, _) = case_row(&fixture, &case_id).await;
    assert_eq!(state, "in_progress");
    assert_eq!(revision, 3);

    let revisions: Vec<(i64,)> = sqlx::query_as(
        "SELECT revision::bigint FROM archava_support.audit WHERE tenant_id=$1 AND record_id=$2 \
         ORDER BY revision",
    )
    .bind(ACME)
    .bind(uuid::Uuid::parse_str(&case_id).expect("uuid"))
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("audit revisions");
    assert_eq!(
        revisions.into_iter().map(|(r,)| r).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    let revision_chain: Vec<(i64,)> = sqlx::query_as(
        "SELECT case_revision::bigint FROM archava_support.transitions \
         WHERE tenant_id=$1 AND case_id=$2 ORDER BY case_revision",
    )
    .bind(ACME)
    .bind(uuid::Uuid::parse_str(&case_id).expect("uuid"))
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("transition revisions");
    assert_eq!(
        revision_chain.into_iter().map(|(r,)| r).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );

    // Every history table refuses an in-place edit.
    let update = "UPDATE archava_support.audit SET operation='tampered' WHERE tenant_id=$1";
    assert!(
        sqlx::query(update)
            .bind(ACME)
            .execute(&fixture.admin_pool)
            .await
            .is_err(),
        "audit must be immutable"
    );
    let update = "UPDATE archava_support.notes SET body='tampered' WHERE tenant_id=$1";
    assert!(
        sqlx::query(update)
            .bind(ACME)
            .execute(&fixture.admin_pool)
            .await
            .is_err(),
        "notes must be immutable"
    );
    let delete = "DELETE FROM archava_support.transitions WHERE tenant_id=$1";
    assert!(
        sqlx::query(delete)
            .bind(ACME)
            .execute(&fixture.admin_pool)
            .await
            .is_err(),
        "transitions must be immutable"
    );

    // A revision that skips the chain is refused by the case guard.
    assert!(
        sqlx::query(
            "UPDATE archava_support.cases SET revision=revision+2 WHERE tenant_id=$1 AND id=$2"
        )
        .bind(ACME)
        .bind(uuid::Uuid::parse_str(&case_id).expect("uuid"))
        .execute(&fixture.admin_pool)
        .await
        .is_err(),
        "the case revision chain is guarded"
    );
}

#[tokio::test]
async fn notes_and_audit_are_permission_gated() {
    let (_fixture, app, cast) = served().await;
    let created = receipt_of(
        &app,
        &cast.ax,
        &create_body(ACME, "b0000000-0000-4000-8000-000000000001", "Gated"),
    )
    .await;
    let case_id = case_of(&created);
    receipt_of(
        &app,
        &cast.ax,
        &arm_body(
            ACME,
            "b0000000-0000-4000-8000-000000000002",
            json!({
                "operation": "note.add",
                "id": case_id,
                "expectedRevision": 1,
                "body": "the reader note"
            }),
        ),
    )
    .await;

    // cases.read + notes.read + audit.read: all three histories answer.
    let detail = detail_of(&app, &cast.axg, &case_id).await;
    assert_eq!(detail["notes"][0]["body"], "the reader note");
    assert_eq!(detail["transitions"].as_array().map(Vec::len), Some(2));
    assert_eq!(detail["audit"].as_array().map(Vec::len), Some(2));

    // cases.read only: the record answers, the histories are null.
    let detail = detail_of(&app, &cast.axnr, &case_id).await;
    assert_eq!(detail["record"]["id"], case_id);
    assert!(detail["notes"].is_null());
    assert!(detail["transitions"].is_null());
    assert!(detail["audit"].is_null());
    assert_eq!(detail["notesHasMore"].as_bool(), Some(false));

    // audit.read alone cannot read the case at all.
    let (status, refusal) = get(
        &app,
        &format!("/erp/support/cases/detail?tenant={ACME}&id={case_id}"),
        &cast.axn,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "audit-only: {refusal}");

    // A note needs notes.write, which the read-only manager does not hold.
    refused_with(
        &app,
        &cast.axg,
        &arm_body(
            ACME,
            "b0000000-0000-4000-8000-000000000003",
            json!({
                "operation": "note.add",
                "id": case_id,
                "expectedRevision": 2,
                "body": "writer note"
            }),
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
}
