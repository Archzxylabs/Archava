//! The R04 projects cohort's host witnesses — the stock pool's FIFTH writer,
//! driven through the real router (`tower::ServiceExt::oneshot`) against the
//! disposable PostgreSQL whose `archava_projects` schema is the real
//! migration ledger (`001_project_delivery`). Every assertion is a fact
//! about what the SERVER did:
//!
//! - the authorized delivery chain (create → milestone → tasks → activate →
//!   complete the work → delivered) walks the frozen state table and leaves
//!   exactly the revision trail the TS repository freezes — the transition
//!   arms move the STATE without bumping the row, every other arm bumps
//!   through the trailing write, the milestone completes on evidence;
//! - the delivered gate counts required work — pending tasks or unreached
//!   required milestones conflict, zero required work conflicts, and the
//!   refusal records no delivered audit row;
//! - out-of-table states and no-op transitions conflict and hold nothing;
//! - a revoked projects grant refuses the recovery — yesterday's receipts
//!   are not replayable through revoked access;
//! - the same idempotency key with a DIFFERENT command conflicts without
//!   recording;
//! - a cross-tenant caller is refused before any business statement runs
//!   while the same caller acts freely in its own ledger;
//! - a lost response re-submits the SAME key + bytes and answers `replayed`
//!   with no second project;
//! - a task living under a REACHED milestone cannot reopen — the parent
//!   gate conflicts and the task stays put;
//! - an actor whose projects grants are missing is refused by the grant
//!   gate with no command row at all, and a malformed submission is a
//!   boundary 400 that never reaches a database statement.

use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::routing::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

/// The commerce ledger this host serves as the primary — the same table list
/// every ERP suite pins; the projects surface RIDES the same serving state
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
/// and the purchasing ledger (`try_new_erp`'s second pass) — the projects
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

/// The projects permission vocabulary (migration 001's CHECK).
const PROJECT_ALL: &[&str] = &[
    "projects.read",
    "projects.write",
    "tasks.read",
    "tasks.write",
    "milestones.read",
    "milestones.write",
    "audit.read",
];

/// `jx` is the delivery manager: full grants, scope all — the positive-path
/// actor.
/// `jno` holds only `audit.read` — every grant gate refuses.
/// `joff` is `jx` with the projects membership seeded ACTIVE and revoked by
/// the test itself — the recovery must not outlive the grant.
/// `jother` holds full grants but ONLY in GLOBEX — the cross-tenant refusal.
/// `jgx` is GLOBEX's own full-grant manager — the per-tenant
/// key-independence witness.
async fn seed(fixture: &Fixture) {
    for principal in ["jx", "jno", "joff", "jother", "jgx"] {
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

    sqlx::query(
        "INSERT INTO archava_projects.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind("jx")
    .bind(PROJECT_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed jx's projects membership");
    sqlx::query(
        "INSERT INTO archava_projects.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind("joff")
    .bind(PROJECT_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed joff's ACTIVE projects membership");
    sqlx::query(
        "INSERT INTO archava_projects.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind("jno")
    .bind(["audit.read"].to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed jno's projects membership");
    for principal in ["jother", "jgx"] {
        sqlx::query(
            "INSERT INTO archava_projects.memberships \
             (tenant_id, principal_id, kind, permissions, scope, active) \
             VALUES ($1, $2, 'human', $3, 'all', true)",
        )
        .bind(GLOBEX)
        .bind(principal)
        .bind(PROJECT_ALL.to_vec())
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the GLOBEX projects membership");
    }
}

/// Fixed idempotency keys — stable so readbacks and the recovery witness can
/// name rows exactly.
const KEY_PROJ: &str = "af6cb000-3334-4333-8333-0000000002b1";
const KEY_PROJ2: &str = "af6cb000-3334-4333-8333-0000000002b2";
const KEY_MILESTONE: &str = "af6cb000-3334-4333-8333-0000000002b3";
const KEY_TASK_A: &str = "af6cb000-3334-4333-8333-0000000002b4";
const KEY_TASK_B: &str = "af6cb000-3334-4333-8333-0000000002b5";
const KEY_ACTIVE: &str = "af6cb000-3334-4333-8333-0000000002b6";
const KEY_REVIEW: &str = "af6cb000-3334-4333-8333-0000000002b7";
const KEY_DELIVERED: &str = "af6cb000-3334-4333-8333-0000000002b8";
const KEY_T1: &str = "af6cb000-3334-4333-8333-0000000002b9";
const KEY_T2: &str = "af6cb000-3334-4333-8333-0000000002ba";
const KEY_T3: &str = "af6cb000-3334-4333-8333-0000000002bb";
const KEY_MILESTONE_COMPLETE: &str = "af6cb000-3334-4333-8333-0000000002bc";
const KEY_HIJACK: &str = "af6cb000-3334-4333-8333-0000000002bd";
const KEY_REVOKED: &str = "af6cb000-3334-4333-8333-0000000002be";
const KEY_XTENANT: &str = "af6cb000-3334-4333-8333-0000000002bf";
const KEY_EMPTY_ACTIVE: &str = "af6cb000-3334-4333-8333-0000000002c1";
const KEY_EMPTY_DELIVERED: &str = "af6cb000-3334-4333-8333-0000000002c2";

// --- HTTP helpers -------------------------------------------------------------

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

fn mutation_body(tenant: &str, request_id: &str, command: Value) -> String {
    json!({
        "tenant": tenant,
        "mutation": {
            "requestId": request_id,
            "command": command,
        }
    })
    .to_string()
}

fn project_create(tenant: &str, request_id: &str, title: &str) -> String {
    mutation_body(
        tenant,
        request_id,
        json!({
            "operation": "project.create",
            "title": title,
            "scopeSummary": format!("Scope of {title}: retrofit the milling plant"),
        }),
    )
}

fn project_transition(
    tenant: &str,
    request_id: &str,
    project_id: &str,
    project_revision: i64,
    state: &str,
) -> String {
    mutation_body(
        tenant,
        request_id,
        json!({
            "operation": "project.transition",
            "projectId": project_id,
            "projectRevision": project_revision,
            "state": state,
        }),
    )
}

fn task_create(
    tenant: &str,
    request_id: &str,
    project_id: &str,
    project_revision: i64,
    title: &str,
    milestone_id: Option<&str>,
    required: bool,
) -> String {
    mutation_body(
        tenant,
        request_id,
        json!({
            "operation": "task.create",
            "projectId": project_id,
            "projectRevision": project_revision,
            "title": title,
            "milestoneId": milestone_id,
            "required": required,
        }),
    )
}

fn task_transition(
    tenant: &str,
    request_id: &str,
    project_id: &str,
    project_revision: i64,
    task_id: &str,
    expected_revision: i64,
    state: &str,
) -> String {
    mutation_body(
        tenant,
        request_id,
        json!({
            "operation": "task.transition",
            "projectId": project_id,
            "projectRevision": project_revision,
            "id": task_id,
            "expectedRevision": expected_revision,
            "state": state,
            "reason": "the crew rescheduled the run",
        }),
    )
}

fn milestone_create(
    tenant: &str,
    request_id: &str,
    project_id: &str,
    project_revision: i64,
    title: &str,
    required: bool,
) -> String {
    mutation_body(
        tenant,
        request_id,
        json!({
            "operation": "milestone.create",
            "projectId": project_id,
            "projectRevision": project_revision,
            "title": title,
            "required": required,
        }),
    )
}

fn milestone_complete(
    tenant: &str,
    request_id: &str,
    project_id: &str,
    project_revision: i64,
    milestone_id: &str,
    expected_revision: i64,
) -> String {
    mutation_body(
        tenant,
        request_id,
        json!({
            "operation": "milestone.complete",
            "projectId": project_id,
            "projectRevision": project_revision,
            "id": milestone_id,
            "expectedRevision": expected_revision,
            "evidence": "the sign-off photo is filed in the drive",
        }),
    )
}

/// A malformed arm — the grammar never admits it — for the boundary-400
/// witness.
fn malformed_body(tenant: &str, request_id: &str) -> String {
    mutation_body(
        tenant,
        request_id,
        json!({
            "operation": "project.demolish",
        }),
    )
}

// --- Database probes -----------------------------------------------------------

/// The project's `(state, revision, scope-confirmed?)`.
async fn project_row(fixture: &Fixture, tenant: &str, id: &str) -> (String, i32, bool) {
    sqlx::query_as(
        "SELECT state,revision,(scope_confirmed_at IS NOT NULL) \
         FROM archava_projects.projects WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(tenant)
    .bind(id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the project exists")
}

/// The task's `(state, revision, milestone)`.
async fn task_row(fixture: &Fixture, tenant: &str, id: &str) -> (String, i32, Option<String>) {
    sqlx::query_as(
        "SELECT state,revision,milestone_id::text \
         FROM archava_projects.tasks WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(tenant)
    .bind(id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the task exists")
}

/// The milestone's `(state, revision)`.
async fn milestone_row(fixture: &Fixture, tenant: &str, id: &str) -> (String, i32) {
    sqlx::query_as(
        "SELECT state,revision \
         FROM archava_projects.milestones WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(tenant)
    .bind(id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the milestone exists")
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

/// THE core witness: the authorized delivery chain walks the frozen state
/// table on the real rows — the transition arms move the STATE without
/// bumping the row, every other arm bumps the project through the trailing
/// write, the milestone completes on evidence — and the delivered gate
/// finally lets the project through with the audit row on the ledger.
#[tokio::test]
async fn authorized_delivery_chain_walks_the_state_table() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "jx").await;

    // The project — a draft until activation confirms the scope.
    let (status, receipt) = post(
        &app,
        "/erp/projects/commands",
        &project_create(ACME, KEY_PROJ, "Plant retrofit"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "project.create is authorized");
    assert_eq!(receipt["decision"], "receipt");
    assert_eq!(receipt["receipt"]["kind"], "project");
    assert_eq!(receipt["receipt"]["revision"], 1);
    assert_eq!(receipt["receipt"]["projectRevision"], 1);
    let project_id = receipt["receipt"]["projectId"]
        .as_str()
        .expect("the receipt names the project id")
        .to_string();
    let row = project_row(&fixture, ACME, &project_id).await;
    assert_eq!(row, ("draft".into(), 1, false));

    // The required milestone.
    let (status, milestone) = post(
        &app,
        "/erp/projects/commands",
        &milestone_create(
            ACME,
            KEY_MILESTONE,
            &project_id,
            1,
            "Foundations poured",
            true,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(milestone["receipt"]["kind"], "milestone");
    assert_eq!(milestone["receipt"]["revision"], 1);
    assert_eq!(milestone["receipt"]["projectRevision"], 2);
    let milestone_id = milestone["receipt"]["id"]
        .as_str()
        .expect("the receipt names the milestone id")
        .to_string();

    // Two required tasks — one free, one under the milestone.
    let (status, task_a) = post(
        &app,
        "/erp/projects/commands",
        &task_create(ACME, KEY_TASK_A, &project_id, 2, "Conduit run", None, true),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(task_a["receipt"]["kind"], "task");
    assert_eq!(task_a["receipt"]["projectRevision"], 3);
    let task_a_id = task_a["receipt"]["id"]
        .as_str()
        .expect("the receipt names the task id")
        .to_string();
    let (status, task_b) = post(
        &app,
        "/erp/projects/commands",
        &task_create(
            ACME,
            KEY_TASK_B,
            &project_id,
            3,
            "Panel wiring",
            Some(&milestone_id),
            true,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "an open milestone can gain work");
    assert_eq!(task_b["receipt"]["projectRevision"], 4);
    let task_b_id = task_b["receipt"]["id"]
        .as_str()
        .expect("the receipt names the task id")
        .to_string();
    assert_eq!(
        task_row(&fixture, ACME, &task_b_id).await,
        ("todo".into(), 1, Some(milestone_id.clone()))
    );

    // Activation confirms the scope — the immutable confirmation lands and
    // the row's revision stands (the transition arms do not bump).
    let (status, activation) = post(
        &app,
        "/erp/projects/commands",
        &project_transition(ACME, KEY_ACTIVE, &project_id, 4, "active"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(activation["receipt"]["revision"], 5);
    assert_eq!(activation["receipt"]["projectRevision"], 5);
    assert_eq!(
        project_row(&fixture, ACME, &project_id).await,
        ("active".into(), 4, true),
        "the confirmation froze with the activation"
    );

    // The work: A runs and finishes, B finishes.
    let (status, running) = post(
        &app,
        "/erp/projects/commands",
        &task_transition(ACME, KEY_T1, &project_id, 4, &task_a_id, 1, "doing"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(running["receipt"]["revision"], 2);
    assert_eq!(running["receipt"]["projectRevision"], 5);
    let (status, finished_a) = post(
        &app,
        "/erp/projects/commands",
        &task_transition(ACME, KEY_T2, &project_id, 5, &task_a_id, 2, "done"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(finished_a["receipt"]["projectRevision"], 6);
    let (status, finished_b) = post(
        &app,
        "/erp/projects/commands",
        &task_transition(ACME, KEY_T3, &project_id, 6, &task_b_id, 1, "done"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(finished_b["receipt"]["projectRevision"], 7);

    // The milestone completes on evidence.
    let (status, completed) = post(
        &app,
        "/erp/projects/commands",
        &milestone_complete(
            ACME,
            KEY_MILESTONE_COMPLETE,
            &project_id,
            7,
            &milestone_id,
            1,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the milestone completes");
    assert_eq!(completed["receipt"]["kind"], "milestone");
    assert_eq!(completed["receipt"]["revision"], 2);
    assert_eq!(completed["receipt"]["projectRevision"], 8);
    assert_eq!(
        milestone_row(&fixture, ACME, &milestone_id).await,
        ("reached".into(), 2)
    );

    // Review, then delivered — the gate counts zero pending required work.
    let (status, _) = post(
        &app,
        "/erp/projects/commands",
        &project_transition(ACME, KEY_REVIEW, &project_id, 8, "review"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, delivered) = post(
        &app,
        "/erp/projects/commands",
        &project_transition(ACME, KEY_DELIVERED, &project_id, 8, "delivered"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the complete work delivers");
    assert_eq!(delivered["receipt"]["projectRevision"], 9);
    assert_eq!(
        project_row(&fixture, ACME, &project_id).await,
        ("delivered".into(), 8, true),
        "the delivered row keeps the transition arms' no-bump trail"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_projects.audit \
             WHERE tenant_id=$1 AND operation='project.transition'",
            &[ACME],
        )
        .await,
        3,
        "three state moves carry their audit rows"
    );

    // The status read answers `committed` with the stored receipt.
    let (status, status_body) = get(
        &app,
        &format!("/erp/projects/commands/status?tenant={ACME}&requestId={KEY_DELIVERED}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(status_body["decision"], "committed");
    assert_eq!(status_body["requestId"], KEY_DELIVERED);
    assert_eq!(status_body["receipt"]["kind"], "project");
}

/// The delivered gate counts required work: pending required tasks conflict,
/// a required milestone not reached conflicts, and a project with no
/// required work conflicts too — every refurtsal records no delivered audit
/// row and the state stands.
#[tokio::test]
async fn delivered_gate_requires_the_work() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "jx").await;

    // Project 1: required work that is NOT done.
    let (status, receipt) = post(
        &app,
        "/erp/projects/commands",
        &project_create(ACME, KEY_PROJ, "Pending-work project"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let project_id = receipt["receipt"]["projectId"]
        .as_str()
        .expect("the receipt names the project id")
        .to_string();
    let (status, task) = post(
        &app,
        "/erp/projects/commands",
        &task_create(ACME, KEY_TASK_A, &project_id, 1, "Conduit run", None, true),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let _task_id = task["receipt"]["id"]
        .as_str()
        .expect("the receipt names the task id")
        .to_string();
    let (status, _) = post(
        &app,
        "/erp/projects/commands",
        &project_transition(ACME, KEY_ACTIVE, &project_id, 2, "active"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = post(
        &app,
        "/erp/projects/commands",
        &project_transition(ACME, KEY_REVIEW, &project_id, 2, "review"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, refusal) = post(
        &app,
        "/erp/projects/commands",
        &project_transition(ACME, KEY_DELIVERED, &project_id, 2, "delivered"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "pending required work refuses the delivery"
    );
    assert_eq!(refusal["decision"], "refused");
    assert_eq!(
        project_row(&fixture, ACME, &project_id).await,
        ("review".into(), 2, true),
        "the state stands"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_projects.audit \
             WHERE tenant_id=$1 AND operation='project.transition'",
            &[ACME],
        )
        .await,
        2,
        "only the active + review moves carry audit rows — no delivered row"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_projects.commands \
             WHERE tenant_id=$1 AND request_id=$2::uuid",
            &[ACME, KEY_DELIVERED],
        )
        .await,
        0,
        "the refused submission was never even claimed"
    );

    // An un-reached REQUIRED milestone conflicts equally.
    let (status, milestone) = post(
        &app,
        "/erp/projects/commands",
        &milestone_create(ACME, KEY_MILESTONE, &project_id, 2, "Waiting gates", true),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let milestone_id = milestone["receipt"]["id"]
        .as_str()
        .expect("the receipt names the milestone id")
        .to_string();
    let (_, finished) = post(
        &app,
        "/erp/projects/commands",
        &task_transition(ACME, KEY_T3, &project_id, 3, &_task_id, 1, "done"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        finished["decision"], "receipt",
        "the task finishes on its own"
    );
    let (status, second) = post(
        &app,
        "/erp/projects/commands",
        &project_transition(ACME, KEY_DELIVERED, &project_id, 4, "delivered"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "an unreached required milestone refuses the delivery"
    );
    assert_eq!(second["decision"], "refused");
    let _ = milestone_id;

    // Project 2: ZERO required work also refuses.
    let (status, second_project) = post(
        &app,
        "/erp/projects/commands",
        &project_create(ACME, KEY_PROJ2, "Empty-work project"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let second_id = second_project["receipt"]["projectId"]
        .as_str()
        .expect("the receipt names the project id")
        .to_string();
    let (status, _) = post(
        &app,
        "/erp/projects/commands",
        &project_transition(ACME, KEY_EMPTY_ACTIVE, &second_id, 1, "active"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, third) = post(
        &app,
        "/erp/projects/commands",
        &project_transition(ACME, KEY_EMPTY_DELIVERED, &second_id, 1, "delivered"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a project without required work delivers from nothing"
    );
    assert_eq!(third["decision"], "refused");
}

/// Out-of-table states and no-op transitions conflict and hold nothing: a
/// delivered move straight from draft refuses at the frozen state table, and
/// re-activating an active project refuses too — every refusal rolls back,
/// no command row lands.
#[tokio::test]
async fn out_of_table_transitions_refuse_and_hold_nothing() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "jx").await;

    let (status, receipt) = post(
        &app,
        "/erp/projects/commands",
        &project_create(ACME, KEY_PROJ, "Fussy project"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let project_id = receipt["receipt"]["projectId"]
        .as_str()
        .expect("the receipt names the project id")
        .to_string();

    // draft → delivered is outside the frozen state table.
    let (status, refusal) = post(
        &app,
        "/erp/projects/commands",
        &project_transition(ACME, KEY_DELIVERED, &project_id, 1, "delivered"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "an out-of-table move conflicts at the state table"
    );
    assert_eq!(refusal["decision"], "refused");
    assert_eq!(
        project_row(&fixture, ACME, &project_id).await,
        ("draft".into(), 1, false),
        "the state never moved"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_projects.commands \
             WHERE tenant_id=$1 AND request_id=$2::uuid",
            &[ACME, KEY_DELIVERED],
        )
        .await,
        0,
        "the refused move was never claimed"
    );

    // The activation records — then a SECOND active move is a no-op
    // transition the table refuses.
    let (status, _) = post(
        &app,
        "/erp/projects/commands",
        &project_transition(ACME, KEY_ACTIVE, &project_id, 1, "active"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        project_row(&fixture, ACME, &project_id).await,
        ("active".into(), 1, true),
        "the activation confirmed the scope"
    );
    let (status, second) = post(
        &app,
        "/erp/projects/commands",
        &project_transition(ACME, KEY_T1, &project_id, 1, "active"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "active → active is not a transition the table admits"
    );
    assert_eq!(second["decision"], "refused");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_projects.audit \
             WHERE tenant_id=$1 AND operation='project.transition'",
            &[ACME],
        )
        .await,
        1,
        "only the real activation carries an audit row"
    );
}

/// A receipt the CURRENT access no longer authorizes does not replay: the
/// same key + same bytes re-submitted after the grant was revoked is refused
/// by the grant gate, and the status read refuses equally — an old receipt
/// is not a permanent grant.
#[tokio::test]
async fn revoked_projects_grant_refuses_recovery() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "joff").await;

    let (status, receipt) = post(
        &app,
        "/erp/projects/commands",
        &project_create(ACME, KEY_REVOKED, "Vested project"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the active grant records");
    assert_eq!(receipt["decision"], "receipt");

    sqlx::query(
        "UPDATE archava_projects.memberships SET active=false \
         WHERE tenant_id=$1 AND principal_id='joff'",
    )
    .bind(ACME)
    .execute(&fixture.admin_pool)
    .await
    .expect("revoke joff's projects membership");

    let (status, replay) = post(
        &app,
        "/erp/projects/commands",
        &project_create(ACME, KEY_REVOKED, "Vested project"),
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
            "SELECT count(*) FROM archava_projects.projects \
             WHERE tenant_id=$1 AND title='Vested project'",
            &[ACME],
        )
        .await,
        1,
        "no SECOND project — the replay recorded nothing new"
    );

    let (status, status_body) = get(
        &app,
        &format!("/erp/projects/commands/status?tenant={ACME}&requestId={KEY_REVOKED}"),
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
    let session = login(&app, "jx").await;

    let (status, first) = post(
        &app,
        "/erp/projects/commands",
        &project_create(ACME, KEY_PROJ, "Original project"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["decision"], "receipt");
    let project_id = first["receipt"]["projectId"]
        .as_str()
        .expect("the receipt names the project id")
        .to_string();

    // The attacker submits a task create under the SAME key: bytes differ,
    // fingerprint differs → conflict, nothing recorded.
    let (status, refusal) = post(
        &app,
        "/erp/projects/commands",
        &task_create(ACME, KEY_PROJ, &project_id, 1, "Hijacked task", None, true),
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
            "SELECT count(*) FROM archava_projects.tasks \
             WHERE tenant_id=$1 AND project_id=$2::uuid",
            &[ACME, &project_id],
        )
        .await,
        0,
        "the hijacked task was never written"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_projects.commands \
             WHERE request_id=$1::uuid AND receipt IS NOT NULL",
            &[KEY_PROJ],
        )
        .await,
        1,
        "the stored receipt still answers the SAME command"
    );
}

/// A cross-tenant caller is refused before any business statement runs:
/// full GLOBEX grants mean nothing to ACME's ledger. The SAME caller — and
/// the SAME key — in its own tenant is fine.
#[tokio::test]
async fn cross_tenant_caller_refused_before_any_write() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "jother").await;

    let (status, refusal) = post(
        &app,
        "/erp/projects/commands",
        &project_create(ACME, KEY_XTENANT, "Invader project"),
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
            "SELECT count(*) FROM archava_projects.projects WHERE tenant_id=$1",
            &[ACME],
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_projects.commands \
             WHERE tenant_id=$1 AND request_id=$2::uuid",
            &[ACME, KEY_XTENANT],
        )
        .await,
        0,
        "no command row was even claimed in the target ledger"
    );

    let own = login(&app, "jgx").await;
    let (status, receipt) = post(
        &app,
        "/erp/projects/commands",
        &project_create(GLOBEX, KEY_XTENANT, "Legit globex project"),
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
/// replay branch answers `replayed` with the stored receipt and no second
/// project exists.
#[tokio::test]
async fn lost_response_replays_without_a_second_project() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "jx").await;

    let (status, first) = post(
        &app,
        "/erp/projects/commands",
        &project_create(ACME, KEY_PROJ, "Recoverable project"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the first submit records");
    assert_eq!(first["decision"], "receipt");
    let project_id = first["receipt"]["projectId"]
        .as_str()
        .expect("the receipt names the project id")
        .to_string();

    let (status, replay) = post(
        &app,
        "/erp/projects/commands",
        &project_create(ACME, KEY_PROJ, "Recoverable project"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "an identical resubmit replays");
    assert_eq!(replay["decision"], "replayed");
    assert_eq!(
        replay["receipt"]["projectId"],
        Value::String(project_id.clone())
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_projects.projects WHERE tenant_id=$1",
            &[ACME],
        )
        .await,
        1,
        "still exactly one project — no double create"
    );

    let (status, status_body) = get(
        &app,
        &format!("/erp/projects/commands/status?tenant={ACME}&requestId={KEY_PROJ}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(status_body["decision"], "committed");
}

/// A task living under a REACHED milestone cannot reopen: the parent gate
/// conflicts on every non-done move and the task stays put.
#[tokio::test]
async fn task_under_reached_milestone_cannot_reopen() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "jx").await;

    let (status, receipt) = post(
        &app,
        "/erp/projects/commands",
        &project_create(ACME, KEY_PROJ, "Milestone-gated project"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let project_id = receipt["receipt"]["projectId"]
        .as_str()
        .expect("the receipt names the project id")
        .to_string();
    let (status, milestone) = post(
        &app,
        "/erp/projects/commands",
        &milestone_create(ACME, KEY_MILESTONE, &project_id, 1, "Survey signed", false),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let milestone_id = milestone["receipt"]["id"]
        .as_str()
        .expect("the receipt names the milestone id")
        .to_string();
    let (status, task) = post(
        &app,
        "/erp/projects/commands",
        &task_create(
            ACME,
            KEY_TASK_B,
            &project_id,
            2,
            "Report writing",
            Some(&milestone_id),
            false,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let task_id = task["receipt"]["id"]
        .as_str()
        .expect("the receipt names the task id")
        .to_string();
    assert_eq!(
        task_row(&fixture, ACME, &task_id).await,
        ("todo".into(), 1, Some(milestone_id.clone()))
    );

    // The milestone reaches on evidence.
    let (status, _) = post(
        &app,
        "/erp/projects/commands",
        &milestone_complete(
            ACME,
            KEY_MILESTONE_COMPLETE,
            &project_id,
            3,
            &milestone_id,
            1,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        milestone_row(&fixture, ACME, &milestone_id).await,
        ("reached".into(), 2)
    );

    // Now the task refuses every non-done move under its reached parent.
    let attempts = [(KEY_T1, "doing"), (KEY_T2, "cancelled")];
    for (key, state) in attempts {
        let (status, refusal) = post(
            &app,
            "/erp/projects/commands",
            &task_transition(ACME, key, &project_id, 4, &task_id, 1, state),
            Some(&session),
            true,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "a task under a reached milestone refuses {state}"
        );
        assert_eq!(refusal["decision"], "refused");
    }
    assert_eq!(
        task_row(&fixture, ACME, &task_id).await,
        ("todo".into(), 1, Some(milestone_id)),
        "the task stays put"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_projects.tasks \
             WHERE tenant_id=$1 AND project_id=$2::uuid AND state='doing'",
            &[ACME, &project_id],
        )
        .await,
        0,
        "nothing reopened"
    );
}

/// An actor whose projects grants are missing is refused by the grant gate
/// with no command row at all — the `audit.read`-only clerk cannot write any
/// more than a stranger can. A malformed arm, next, is a boundary 400 that
/// never reaches a database statement.
#[tokio::test]
async fn grants_missing_actor_refused_before_any_write() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "jno").await;

    let (status, refusal) = post(
        &app,
        "/erp/projects/commands",
        &project_create(ACME, KEY_PROJ, "Forbidden project"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "the grant gate refuses");
    assert_eq!(refusal["decision"], "denied");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_projects.projects WHERE tenant_id=$1",
            &[ACME],
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_projects.commands \
             WHERE tenant_id=$1 AND request_id=$2::uuid",
            &[ACME, KEY_PROJ],
        )
        .await,
        0,
        "the refusal happened before the claim"
    );

    let (status, rejected) = post(
        &app,
        "/erp/projects/commands",
        &malformed_body(ACME, KEY_HIJACK),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an arm the grammar never admits is a 400"
    );
    assert_eq!(rejected["decision"], "rejected");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_projects.commands WHERE request_id=$1::uuid",
            &[KEY_HIJACK],
        )
        .await,
        0
    );
}

/// P2 (A11): the status read answers to the `projects.read` grant, and the
/// receipt is bound to the actor that wrote it. Membership alone — an
/// `audit.read`-only clerk — must not recover another actor's receipt, and a
/// full-grant peer must not either.
#[tokio::test]
async fn status_read_requires_the_projects_read_grant() {
    const KEY: &str = "a11b0000-4444-4444-8444-0000000000b1";
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let author = login(&app, "jx").await;
    let auditor = login(&app, "jno").await;
    let peer = login(&app, "joff").await;

    let body = mutation_body(
        ACME,
        KEY,
        json!({
            "operation": "project.create",
            "title": "Status read project",
            "scopeSummary": "Scope of the status read project: retrofit the mill",
        }),
    );
    let (status, outcome) = post(&app, "/erp/projects/commands", &body, Some(&author), true).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the author records the command: {outcome}"
    );

    let uri = format!("/erp/projects/commands/status?tenant={ACME}&requestId={KEY}");
    let (status, outcome) = get(&app, &uri, Some(&auditor)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "audit.read is not projects.read: {outcome}"
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
