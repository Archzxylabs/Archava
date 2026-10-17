//! P-AUTHORITY-PG — RA01's proof against real HTTP and real PostgreSQL.
//!
//! ## Why this suite exists
//!
//! Audit RA01's finding was structural: "request body does not become authority
//! for tenant/role/grants/approval." Before this suite, the router's refusal
//! paths were only exercised by in-crate tests on a pool that never connects —
//! they proved the *ordering* (extraction before session), not the *authority*
//! (a membership row, an approval row, a revocation).
//!
//! This file drives the seven routes through `tower::ServiceExt::oneshot`,
//! against a disposable PostgreSQL instance seeded with real identity users and
//! real commerce memberships. Every assertion here is a fact about what the
//! SERVER did, not what the client claimed:
//!
//! - A forged `role`/`capability`/`human_approved` in the body changes nothing.
//! - A revocation in the database is honoured on the next request, with no
//!   cache to invalidate.
//! - A bound approval is a row, not a boolean: it is created, stamped by a
//!   distinct human, consumed exactly once, and its expiry and binding are
//!   enforced by a database trigger the host cannot circumvent.
//! - A receipt from one tenant is invisible to another tenant at the row-level
//!   security boundary, not merely hidden by a WHERE clause.
//!
//! ## What each witness proves
//!
//! The eight required checks from `P-AUTHORITY-PG` (see
//! `docs/rust/required-coverage.json`) map to witnesses below. The suite also
//! carries CSRF, approval-single-use, service-approver, self-approval, and
//! scope-rule negatives, which are additional structural refusals, not required
//! checks — they guard the same gate from the sides the required checks do not
//! cover.
//!
//! ## Why this is its own file, in-crate
//!
//! It lives in `tests/` (integration-test target) rather than `src/` because it
//! drives the router through its public HTTP surface, the same way a real
//! caller would. It needs PostgreSQL, so it is `#[cfg(unix)]`-gated like the
//! other fixture-backed suites and runs only when `initdb` is available.

use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::routing::Router;
use tower::ServiceExt;

/// The commerce ledger's tenant-scoped tables — the count `assert_startup_readiness`
/// expects to find forced. `approvals` is included because migration 004 added it.
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
const IDENTITY_KEY: &[u8] = &[9u8; 32];

/// The test password. The password gate enforces `length >= 15`.
const PASSWORD: &str = "correct horse battery staple";

/// A precomputed scrypt hash of [`PASSWORD`], so the seeder does not need to
/// run 128 concurrent derivations (which trips `MAX_ACTIVE_DERIVATIONS = 2`
/// when 15 tests seed in parallel). The encoded form embeds its own salt, so
/// one hash is valid for every seeded user.
const PASSWORD_HASH: &str =
    "scrypt$32768$8$3$OZtInKxSaSZqbEptj2fvUg$jU_gx2rQW-69EYpbRoMKGSgclhV-mO7gXVF-QYvSXVo";

fn commerce_ledger() -> Ledger {
    Ledger {
        namespace: "archava_commerce",
        ledger: "Commerce",
        tables: TABLES,
    }
}

// --- Seeding -----------------------------------------------------------------

/// The tenants this suite uses. `acme-retail` is the active one; `globex` is a
/// second tenant for the cross-tenant-read proof.
const ACME: &str = "acme-retail";
const GLOBEX: &str = "globex-corp";

/// The principals seeded in `acme-retail`, and what each is for.
struct Principal {
    id: &'static str,
    kind: &'static str,
    role: &'static str,
    capability: &'static str,
    scope: &'static str,
    active: bool,
    tenants: &'static [&'static str],
}

/// The cast:
///
/// - `alice` is the human owner: she can do everything a human owner can do,
///   including approving an L5 command that a *distinct* human submitted.
/// - `bob` is a human admin: a distinct approver, so `alice` and `bob` can
///   stamp each other's approvals without self-approval.
/// - `svc` is a service principal with L0 `product.read` — the positive path.
/// - `svc-l3` is a service principal with L3 `crm.lead.create`, scope `all`,
///   for the L3-scope rule (L3+ requires `scope='all'`).
/// - `svc-assigned` has L3 `crm.lead.create` with scope `assigned` — the
///   scope-rule negative: L3+ with an `assigned` scope is `missing_scope`.
/// - `svc-admin` is a service principal with role `admin`, capability
///   `enterprise`, scope `all` — for the "service cannot approve" negative.
/// - `viewer` has role `viewer`, capability `assist`, scope `assigned` — the
///   role/capability negative: `product.read` is `ASSISTANT_ONLY`, so a viewer
///   is `role_not_allowed`.
/// - `revoked` is `svc` with `active=false`, for the revocation suite.
const PRINCIPALS: &[Principal] = &[
    Principal {
        id: "alice",
        kind: "human",
        role: "owner",
        capability: "enterprise",
        scope: "all",
        active: true,
        tenants: &[ACME],
    },
    Principal {
        id: "bob",
        kind: "human",
        role: "admin",
        capability: "enterprise",
        scope: "all",
        active: true,
        tenants: &[ACME],
    },
    Principal {
        id: "svc",
        kind: "service",
        role: "archava_assistant",
        capability: "assist",
        scope: "assigned",
        active: true,
        tenants: &[ACME],
    },
    Principal {
        id: "svc-l3",
        kind: "service",
        role: "archava_assistant",
        capability: "act",
        scope: "all",
        active: true,
        tenants: &[ACME],
    },
    Principal {
        id: "svc-assigned",
        kind: "service",
        role: "archava_assistant",
        capability: "act",
        scope: "assigned",
        active: true,
        tenants: &[ACME],
    },
    Principal {
        id: "svc-admin",
        kind: "service",
        role: "admin",
        capability: "enterprise",
        scope: "all",
        active: true,
        tenants: &[ACME],
    },
    Principal {
        id: "viewer",
        kind: "service",
        role: "viewer",
        capability: "assist",
        scope: "assigned",
        active: true,
        tenants: &[ACME],
    },
    Principal {
        id: "revoked",
        kind: "service",
        role: "archava_assistant",
        capability: "assist",
        scope: "assigned",
        active: false,
        tenants: &[ACME],
    },
];

/// Seed a user into `archava_identity` and a membership into each tenant.
///
/// This uses `fixture.admin_pool` (superuser), which bypasses row-level
/// security by definition — it is FIXTURE ADMINISTRATION, not authority
/// evidence. The evidence in this file is always what the RUNTIME role did
/// through the router.
///
/// There is deliberately no call that raises the KDF budget here. The suite
/// runs on the production value of 2 concurrent derivations, and
/// [`LOGIN_SLOTS`] is what keeps it inside that budget rather than above it:
/// a suite that raised the ceiling was measuring a concurrency policy no build
/// of the host actually ships (RN07/RN04).
async fn seed(fixture: &Fixture) {
    for principal in PRINCIPALS {
        sqlx::query(
            "INSERT INTO archava_identity.users (id, username, password_hash, active, version) \
             VALUES ($1, $2, $3, true, 1)",
        )
        .bind(principal.id)
        .bind(format!("{}@archava.test", principal.id))
        .bind(PASSWORD_HASH)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the identity user");

        for tenant in principal.tenants {
            sqlx::query(
                "INSERT INTO archava_commerce.memberships \
                 (tenant_id, principal_id, kind, permissions, scope, active, role, capability) \
                 VALUES ($1, $2, $3, '{}', $4, $5, $6, $7)",
            )
            .bind(tenant)
            .bind(principal.id)
            .bind(principal.kind)
            .bind(principal.scope)
            .bind(principal.active)
            .bind(principal.role)
            .bind(principal.capability)
            .execute(&fixture.admin_pool)
            .await
            .expect("seed the membership");
        }
    }
}

// --- HTTP helpers ------------------------------------------------------------

/// A session's credentials.
struct Session {
    token: String,
    csrf: String,
}

/// How many logins this suite may have in flight at once.
///
/// Exactly the production budget (`max_active_derivations()`), and the point of
/// RN07/RN04: the suite used to raise that budget to 32 through a setter that
/// shipped in the normal build, so its green rows described a concurrency
/// policy no deployment has. Now it stays INSIDE the real budget by
/// serialising on a permit — the same 2 concurrent scrypt derivations a
/// production login flood gets, and the suite passes on them.
///
/// A runtime-agnostic `tokio::sync::Semaphore` so a process-wide static works
/// across the per-test runtimes `#[tokio::test]` builds (the same reason
/// `pg-fixture`'s `PROVISION_SLOTS` is one).
static LOGIN_SLOTS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(archava_host::max_active_derivations());

/// Log in and return the session's bearer token and CSRF value.
async fn login(app: &Router, principal_id: &str) -> Session {
    // Hold a slot for the whole login so at most `max_active_derivations()`
    // derivations run at once. Without this the parallel tests would trip the
    // production `busy` refusal — which is the budget working, not a defect.
    let _slot = LOGIN_SLOTS.acquire().await.expect("login slots are open");
    let body = serde_json::json!({
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
    let outcome: serde_json::Value =
        serde_json::from_slice(&bytes).expect("the login response is JSON");

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

/// Send a POST with a session's credentials and return the status and body.
async fn post(
    app: &Router,
    uri: &str,
    body: &str,
    session: Option<&Session>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(session) = session {
        builder = builder
            .header("authorization", format!("Bearer {}", session.token))
            .header("x-archava-csrf", &session.csrf);
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
    let outcome: serde_json::Value = serde_json::from_slice(&bytes).expect("the response is JSON");
    (status, outcome)
}

/// Send a GET with a session's bearer token (no CSRF — GET is a read).
async fn get(
    app: &Router,
    uri: &str,
    session: Option<&Session>,
) -> (StatusCode, serde_json::Value) {
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
    let outcome: serde_json::Value = serde_json::from_slice(&bytes).expect("the response is JSON");
    (status, outcome)
}

/// A fully valid `POST /commands` body for a service principal's `product.read`.
///
/// `product.read` is L0, `ASSISTANT_ONLY`, `CapabilityTier::Assist`,
/// `ConfirmationMode::None`: the simplest positive path.
fn product_read_body(tenant: &str, request_id: &str, input: &str) -> String {
    serde_json::json!({
        "tenant": tenant,
        "action_id": "product.read",
        "request_id": request_id,
        "input": input,
        "revision": 1,
    })
    .to_string()
}

// --- The eight required checks -----------------------------------------------

/// **Required check: "unauthenticated command refused."**
///
/// A well-formed, fully valid `product.read` body with NO session is refused
/// before the gate — not with an empty result, not with a 403, but with the
/// fixed `unauthenticated` 401.
#[tokio::test]
async fn an_unauthenticated_command_is_refused() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);

    let (status, outcome) = post(
        &app,
        "/commands",
        &product_read_body(
            ACME,
            "7d9f4c2e-1b3a-4c7d-9e8f-0a1b2c3d4e5f",
            "{\"productId\":1}",
        ),
        None,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "unauthenticated must be a 401"
    );
    assert_eq!(outcome["decision"], "unauthenticated");
    assert_eq!(outcome["reason"], "no_valid_session");
}

/// **Required check: "forged tenant/role/approval refused."**
///
/// A body carrying a valid session and a forged `role`/`capability`/
/// `human_approved` (the pre-RA01 authority fields) is parsed (they are
/// unknown, dropped) and then refused by the gate, because the gate reads the
/// membership row — not the body.
///
/// `viewer` has role `viewer`, capability `assist`, scope `assigned`. The body
/// claims `role: "owner"`, `capability: "enterprise"`, `human_approved: true`.
/// `product.read` is `ASSISTANT_ONLY`, so `viewer` is `role_not_allowed` —
/// and the forged role must not change that.
#[tokio::test]
async fn a_forged_role_and_approval_in_the_body_change_nothing() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);

    let session = login(&app, "viewer").await;
    let body = serde_json::json!({
        "tenant": ACME,
        "action_id": "product.read",
        "request_id": "7d9f4c2e-1b3a-4c7d-9e8f-0a1b2c3d4e5f",
        "input": "{\"productId\":1}",
        "revision": 1,
        "role": "owner",
        "capability": "enterprise",
        "human_approved": true,
    })
    .to_string();

    let (status, outcome) = post(&app, "/commands", &body, Some(&session)).await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a forged role must be ignored, not honoured"
    );
    assert_eq!(outcome["decision"], "denied");
    assert_eq!(outcome["reason"], "role_not_allowed");
}

/// **Required check: "revoked grant refused on execute."**
///
/// `revoked` is a seeded membership with `active=false`. Its `product.read`
/// is `missing_membership` — the same answer as a principal that never had a
/// grant there, because the two must not be distinguishable.
#[tokio::test]
async fn a_revoked_grant_is_refused_on_execute() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);

    let session = login(&app, "revoked").await;
    let (status, outcome) = post(
        &app,
        "/commands",
        &product_read_body(
            ACME,
            "7d9f4c2e-1b3a-4c7d-9e8f-0a1b2c3d4e5f",
            "{\"productId\":1}",
        ),
        Some(&session),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a revoked membership must be refused"
    );
    assert_eq!(outcome["reason"], "missing_membership");
}

/// **Required check: "revoked grant refused on status."**
///
/// The same revoked principal attempting `GET /commands/status` is refused
/// with `missing_membership`, not with a 404 that would let it distinguish
/// "revoked" from "no such receipt."
#[tokio::test]
async fn a_revoked_grant_is_refused_on_status() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);

    let session = login(&app, "revoked").await;
    let (status, outcome) = get(
        &app,
        "/commands/status?tenant=acme-retail&request_id=7d9f4c2e-1b3a-4c7d-9e8f-0a1b2c3d4e5f",
        Some(&session),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a revoked membership must be refused on the status route too"
    );
    assert_eq!(outcome["reason"], "missing_membership");
}

/// **Required check: "revoked grant refused on replay."**
///
/// `svc` executes a `product.read` (200, receipt recorded). Then its
/// membership is revoked by an UPDATE. A replay of the SAME `request_id` —
/// which would have returned the stored receipt under a live grant — is now
/// refused with `missing_membership`, because the gate re-evaluates on the
/// CURRENT grant before answering a replay.
#[tokio::test]
async fn a_revoked_grant_is_refused_on_replay() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);

    let session = login(&app, "svc").await;
    let request_id = "a1b2c3d4-e5f6-4a7b-8c9d-0e1f2a3b4c5d";
    let (status, _) = post(
        &app,
        "/commands",
        &product_read_body(ACME, request_id, "{\"productId\":1}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the first execution must succeed");

    // Revoke the grant. The admin_pool is superuser and bypasses RLS by
    // definition; this is fixture administration, not authority evidence. The
    // evidence is what the HOST does when the runtime role next reads this row.
    sqlx::query("UPDATE archava_commerce.memberships SET active=false WHERE principal_id='svc' AND tenant_id='acme-retail'")
        .execute(&fixture.admin_pool)
        .await
        .expect("revoke the grant");

    let (status, outcome) = post(
        &app,
        "/commands",
        &product_read_body(ACME, request_id, "{\"productId\":1}"),
        Some(&session),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a replay after revocation must be refused"
    );
    assert_eq!(outcome["reason"], "missing_membership");
}

/// **Required check: "revoked grant refused on recovery."**
///
/// After a host RESTART (a new `AppState`, new connections, no in-memory
/// session state), a revoked principal is still refused. This is the
/// structural proof that nothing about authority lives in the process: the
/// token survives (it is an HMAC the new state can verify), the grant does
/// not (it was revoked in the database, and the new state reads it there).
#[tokio::test]
async fn a_revoked_grant_is_refused_on_recovery_after_restart() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);

    let session = login(&app, "svc").await;
    let request_id = "b2c3d4e5-f6a7-4b8c-9d0e-1f2a3b4c5d6e";
    let (status, _) = post(
        &app,
        "/commands",
        &product_read_body(ACME, request_id, "{\"productId\":1}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the first execution must succeed");

    sqlx::query("UPDATE archava_commerce.memberships SET active=false WHERE principal_id='svc' AND tenant_id='acme-retail'")
        .execute(&fixture.admin_pool)
        .await
        .expect("revoke the grant");

    // A genuinely new pool — the restart's structural signature. The session
    // token is stateless (an HMAC digest of a value only the server knows);
    // the grant is a database row the new state must read.
    let restarted_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&fixture.runtime_url())
        .await
        .expect("the restarted pool must connect");
    let restarted = AppState::try_new(
        restarted_pool,
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state after restart");
    let restarted_app = archava_host::router(restarted);

    let (status, outcome) = post(
        &restarted_app,
        "/commands",
        &product_read_body(ACME, request_id, "{\"productId\":1}"),
        Some(&session),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a revoked grant must be refused after a host restart"
    );
    assert_eq!(outcome["reason"], "missing_membership");
}

/// **Required check: "authorized operation succeeds."**
///
/// The positive path: a service principal with a live grant executes
/// `product.read` (L0, no confirmation), and the response is 200 with a
/// receipt whose fingerprint matches the input. A replay of the same
/// `request_id` returns the SAME receipt (idempotency), and a different
/// `request_id` with the same input is a distinct command.
#[tokio::test]
async fn an_authorized_operation_succeeds_with_a_receipt() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);

    let session = login(&app, "svc").await;
    let request_id = "c3d4e5f6-a7b8-4c9d-0e1f-2a3b4c5d6e7f";
    let input = "{\"productId\":42}";

    let (status, outcome) = post(
        &app,
        "/commands",
        &product_read_body(ACME, request_id, input),
        Some(&session),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an authorized command must be allowed"
    );
    assert_eq!(outcome["decision"], "allowed");
    assert_eq!(outcome["tenant"], ACME);
    assert_eq!(
        outcome["request_id"], request_id,
        "the response must echo the idempotency key"
    );
    let receipt = outcome["receipt"]
        .as_object()
        .expect("an allowed command carries a receipt");
    assert_eq!(receipt["action"], "product.read");
    assert_eq!(
        receipt["input"], input,
        "the receipt stores the input verbatim for authorized recovery"
    );

    // A replay of the same key returns the same receipt, without re-executing.
    let (status, replay) = post(
        &app,
        "/commands",
        &product_read_body(ACME, request_id, input),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["decision"], "replayed");
    assert_eq!(
        replay["receipt"], outcome["receipt"],
        "a replay must return the recorded receipt, not a fresh one"
    );

    // The same key with a DIFFERENT input is refused: the key is bound to the
    // command it first executed.
    let (status, outcome) = post(
        &app,
        "/commands",
        &product_read_body(ACME, request_id, "{\"productId\":99}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(outcome["reason"], "request_id_reused");
}

/// **Required check: "cross-tenant read refused at DB."**
///
/// A receipt written under `acme-retail` is invisible to a principal whose
/// membership is in `globex-corp`. The 404 is produced by the row-level
/// security boundary: the receipt row exists, but the tenant transaction for
/// `globex-corp` cannot see it. This is the structural proof that the
/// isolation is in the DATABASE, not in a WHERE clause the host could forget.
#[tokio::test]
async fn a_receipt_of_another_tenant_is_invisible_at_the_db_boundary() {
    let fixture = provision().await;
    seed(&fixture).await;
    // Give `svc` a membership in `globex-corp` too, so the refusal is about
    // the tenant-scoped READ, not about a missing membership.
    sqlx::query(
        "INSERT INTO archava_commerce.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active, role, capability) \
         VALUES ($1, 'svc', 'service', '{}', 'assigned', true, 'archava_assistant', 'assist')",
    )
    .bind(GLOBEX)
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the second-tenant membership");

    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);

    let session = login(&app, "svc").await;
    let request_id = "d4e5f6a7-b8c9-4d0e-1f2a-3b4c5d6e7f8a";

    // Write the receipt under `acme-retail`.
    let (status, _) = post(
        &app,
        "/commands",
        &product_read_body(ACME, request_id, "{\"productId\":1}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Read the SAME request_id under `globex-corp`: not found — the row is
    // invisible at the RLS boundary, even though the principal has a
    // membership in `globex-corp`.
    let (status, outcome) = get(
        &app,
        &format!("/commands/status?tenant={GLOBEX}&request_id={request_id}"),
        Some(&session),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a receipt of one tenant must be invisible to another tenant's read"
    );
    assert_eq!(outcome["reason"], "unknown_request");

    // And the read under the correct tenant DOES find it: the 404 above is
    // the tenant boundary, not a broken read path.
    let (status, outcome) = get(
        &app,
        &format!("/commands/status?tenant={ACME}&request_id={request_id}"),
        Some(&session),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the correct-tenant read must succeed"
    );
    assert_eq!(outcome["decision"], "receipt");
}

// --- Additional structural refusals (not required checks) --------------------

/// A mutation with a valid session but NO CSRF header is refused with
/// `csrf_required`. The CSRF value is derived from the token and the HMAC key;
/// a cross-site page that can neither read the token nor compute the digest
/// cannot forge it.
#[tokio::test]
async fn a_mutation_without_the_csrf_header_is_refused() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);

    let session = login(&app, "svc").await;
    let body = product_read_body(
        ACME,
        "e5f6a7b8-c9d0-4e1f-2a3b-4c5d6e7f8a9b",
        "{\"productId\":1}",
    );

    // Send with the bearer token but WITHOUT the CSRF header.
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/commands")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", session.token))
                .body(Body::from(body))
                .expect("build the request"),
        )
        .await
        .expect("the route must answer");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read the response body");
    let outcome: serde_json::Value = serde_json::from_slice(&bytes).expect("the response is JSON");

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a mutation without the CSRF header must be refused"
    );
    assert_eq!(outcome["reason"], "csrf_required");
}

/// A consumed approval cannot authorize a second command.
///
/// The L5 path: `alice` (human owner) creates an approval for an
/// `admin.config.update` command. `bob` (human admin, a distinct human) stamps
/// it. The command executes with the approval, and the approval is consumed.
/// A second command with the SAME approval but a different `request_id` is
/// refused: the approval is single-use.
#[tokio::test]
async fn a_consumed_approval_cannot_authorize_a_second_command() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);

    let owner_session = login(&app, "alice").await;
    let admin_session = login(&app, "bob").await;
    let input = "{\"config\":{\"theme\":\"dark\"}}";

    // Alice asks for an approval.
    let approval_request = serde_json::json!({
        "tenant": ACME,
        "action_id": "admin.config.update",
        "input": input,
        "revision": 1,
    })
    .to_string();
    let (status, outcome) = post(&app, "/approvals", &approval_request, Some(&owner_session)).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the approval request must be created"
    );
    assert_eq!(outcome["decision"], "approval_requested");
    let approval_id = outcome["approval_id"]
        .as_str()
        .expect("the approval response carries an id")
        .to_string();

    // Bob stamps it (a distinct human, eligible approver).
    let stamp = serde_json::json!({
        "tenant": ACME,
        "approval_id": approval_id,
    })
    .to_string();
    let (status, outcome) = post(&app, "/approvals/approve", &stamp, Some(&admin_session)).await;
    assert_eq!(status, StatusCode::OK, "the approval must be stamped");
    assert_eq!(outcome["decision"], "approved");

    // The command executes with the approval.
    let first = serde_json::json!({
        "tenant": ACME,
        "action_id": "admin.config.update",
        "request_id": "f6a7b8c9-d0e1-4f2a-3b4c-5d6e7f8a9b0c",
        "input": input,
        "revision": 1,
        "approval_id": approval_id,
    })
    .to_string();
    let (status, outcome) = post(&app, "/commands", &first, Some(&owner_session)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the approved command must be allowed"
    );
    assert_eq!(outcome["decision"], "allowed");

    // A SECOND command with the SAME approval is refused. The approval is
    // consumed, so `load_bound_approval` returns false, so the gate denies
    // with `AdminActionRequiresHuman` — the same answer as a command with no
    // approval at all. This is the single-use property: the approval is gone.
    let second = serde_json::json!({
        "tenant": ACME,
        "action_id": "admin.config.update",
        "request_id": "a7b8c9d0-e1f2-4a3b-4c5d-6e7f8a9b0c1d",
        "input": input,
        "revision": 1,
        "approval_id": approval_id,
    })
    .to_string();
    let (status, outcome) = post(&app, "/commands", &second, Some(&owner_session)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a consumed approval must be refused on a second command"
    );
    assert_eq!(outcome["reason"], "admin_action_requires_human");
}

/// A service principal cannot approve, even with role `admin` and capability
/// `enterprise`. Approver eligibility requires `kind = 'human'` — a structural
/// fact about the principal, not a role it can be granted.
#[tokio::test]
async fn a_service_principal_cannot_approve() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);

    let owner_session = login(&app, "alice").await;
    let service_session = login(&app, "svc-admin").await;

    let approval_request = serde_json::json!({
        "tenant": ACME,
        "action_id": "admin.config.update",
        "input": "{\"config\":{\"theme\":\"light\"}}",
        "revision": 1,
    })
    .to_string();
    let (status, outcome) = post(&app, "/approvals", &approval_request, Some(&owner_session)).await;
    assert_eq!(status, StatusCode::CREATED);
    let approval_id = outcome["approval_id"]
        .as_str()
        .expect("the approval response carries an id")
        .to_string();

    let stamp = serde_json::json!({
        "tenant": ACME,
        "approval_id": approval_id,
    })
    .to_string();
    let (status, outcome) = post(&app, "/approvals/approve", &stamp, Some(&service_session)).await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a service principal must be refused as an approver"
    );
    assert_eq!(outcome["reason"], "approver_ineligible");
}

/// An actor may not approve their own command. `alice` (human owner, eligible
/// approver by role) attempts to stamp an approval she created herself; the
/// `actor_id == approver` check refuses it. This is the two-person rule,
/// enforced at the database row, not by a UI convention.
#[tokio::test]
async fn an_actor_may_not_approve_their_own_command() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);

    let owner_session = login(&app, "alice").await;

    let approval_request = serde_json::json!({
        "tenant": ACME,
        "action_id": "admin.config.update",
        "input": "{\"config\":{\"theme\":\"dark\"}}",
        "revision": 1,
    })
    .to_string();
    let (status, outcome) = post(&app, "/approvals", &approval_request, Some(&owner_session)).await;
    assert_eq!(status, StatusCode::CREATED);
    let approval_id = outcome["approval_id"]
        .as_str()
        .expect("the approval response carries an id")
        .to_string();

    // Alice attempts to stamp her own approval.
    let stamp = serde_json::json!({
        "tenant": ACME,
        "approval_id": approval_id,
    })
    .to_string();
    let (status, outcome) = post(&app, "/approvals/approve", &stamp, Some(&owner_session)).await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "an actor must be refused as their own approver"
    );
    assert_eq!(outcome["reason"], "self_approval");
}

/// An unknown action is refused by the gate (404), not by a crash or an empty
/// result. This is the G03 "unknown service" negative at the real entrypoint.
#[tokio::test]
async fn an_unknown_action_is_refused_by_the_gate() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);

    let session = login(&app, "svc").await;
    let body = serde_json::json!({
        "tenant": ACME,
        "action_id": "nonexistent.action",
        "request_id": "b8c9d0e1-f2a3-4b4c-5d6e-7f8a9b0c1d2e",
        "input": "{}",
        "revision": 1,
    })
    .to_string();

    let (status, outcome) = post(&app, "/commands", &body, Some(&session)).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "an unknown action must be a 404, not a crash or an empty result"
    );
    assert_eq!(outcome["reason"], "unknown_action");
}

/// The scope rule: L3+ requires `scope='all'`. A service principal with an
/// L3 action and an `assigned` scope is refused with `missing_scope`.
#[tokio::test]
async fn an_assigned_scope_is_refused_above_the_scope_gate() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);

    let session = login(&app, "svc-assigned").await;
    let body = serde_json::json!({
        "tenant": ACME,
        "action_id": "crm.lead.create",
        "request_id": "c9d0e1f2-a3b4-4c5d-6e7f-8a9b0c1d2e3f",
        "input": "{\"contact\":{\"email\":\"a@b.com\"},\"source\":\"web\"}",
        "revision": 1,
        "confirmed": true,
    })
    .to_string();

    let (status, outcome) = post(&app, "/commands", &body, Some(&session)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "L3+ with an assigned scope must be refused"
    );
    assert_eq!(outcome["reason"], "missing_scope");
}

/// `declined` is a narrowing signal, not a revocation: a command with
/// `declined: true` is refused by the gate (`declined_by_visitor`), and the
/// same command without it is allowed. The gate's `declined` is a per-request
/// signal; it does not change the grant.
#[tokio::test]
async fn a_declined_confirmation_is_refused_by_the_gate() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);

    let session = login(&app, "svc-l3").await;
    let declined = serde_json::json!({
        "tenant": ACME,
        "action_id": "crm.lead.create",
        "request_id": "d0e1f2a3-b4c5-4d6e-7f8a-9b0c1d2e3f4a",
        "input": "{\"contact\":{\"email\":\"a@b.com\"},\"source\":\"web\"}",
        "revision": 1,
        "declined": true,
    })
    .to_string();

    let (status, outcome) = post(&app, "/commands", &declined, Some(&session)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a declined confirmation must be refused"
    );
    assert_eq!(outcome["reason"], "declined_by_visitor");

    // The same command without `declined` is allowed: the grant was never
    // revoked; the visitor declined the confirmation.
    let accepted = serde_json::json!({
        "tenant": ACME,
        "action_id": "crm.lead.create",
        "request_id": "e1f2a3b4-c5d6-4e7f-8a9b-0c1d2e3f4a5b",
        "input": "{\"contact\":{\"email\":\"a@b.com\"},\"source\":\"web\"}",
        "revision": 1,
        "confirmed": true,
    })
    .to_string();

    let (status, outcome) = post(&app, "/commands", &accepted, Some(&session)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the same command without the declined signal must be allowed"
    );
    assert_eq!(outcome["decision"], "allowed");
}

// --- RN01: recovery answers the gate on CURRENT grants ------------------------

/// **RN01.** A receipt is not a hole in the authority chain.
///
/// The audit's probe, reproduced against the current source: `svc` (an
/// assistant, `product.read` allowed) runs a command and receives a receipt.
/// Its membership is then downgraded to `viewer` — an ACTIVE membership, the
/// same principal, the same tenant. The status route used to answer 200 with
/// the original input for that downgraded principal, because it asked only
/// "does an active membership row exist"; the replay of the same command
/// meanwhile answered 403 `role_not_allowed`, so execute and recovery
/// disagreed about the same principal and the same action.
///
/// Now recovery re-answers the same gate. The downgraded principal gets a
/// refusal, not a payload it could no longer produce, and — the other half of
/// the requirement — *authorized* recovery still works once the grant is
/// restored, on the very same receipt.
#[tokio::test]
async fn a_downgraded_grant_cannot_recover_a_receipt_it_may_no_longer_act_on() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);

    let request_id = "4a1ff2f0-9b57-4a35-b2ee-6dcee95b68a1";
    let session = login(&app, "svc").await;

    // 1. The receipt exists, recorded while `svc` was still an assistant.
    let (status, outcome) = post(
        &app,
        "/commands",
        &product_read_body(ACME, request_id, "{\"productId\":1}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the assistant runs the command");
    assert_eq!(
        outcome["decision"], "allowed",
        "witnessing setup: {outcome}"
    );

    // 2. Downgrade. Still active, still the same tenant: only the role went
    //    from `archava_assistant` to `viewer` — the exact shape the audit used.
    sqlx::query(
        "UPDATE archava_commerce.memberships SET role='viewer' \
         WHERE tenant_id=$1 AND principal_id=$2",
    )
    .bind(ACME)
    .bind("svc")
    .execute(&fixture.admin_pool)
    .await
    .expect("downgrade the membership role");

    let (status, outcome) = get(&app, &status_uri(request_id), Some(&session)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "an active membership whose role no longer reaches the action must NOT read the payload"
    );
    assert_eq!(
        outcome["receipt"],
        serde_json::Value::Null,
        "no receipt body on a refused recovery: {outcome}"
    );
    assert_eq!(outcome["decision"], "denied");
    assert_eq!(
        outcome["reason"], "role_not_allowed",
        "the refusal names the grant that no longer reaches the action"
    );

    // 3. And the same command is refused on execute too, so both paths
    //    describe the same grant.
    let (status, outcome) = post(
        &app,
        "/commands",
        &product_read_body(
            ACME,
            "5a1ff2f0-9b57-4a35-b2ee-6dcee95b68a1",
            "{\"productId\":1}",
        ),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(outcome["reason"], "role_not_allowed");

    // 4. Authorized recovery still works: restore the grant, and the SAME
    //    request id reads its record back. No second effect, no new receipt —
    //    `authorized recovery without a second effect` is the other half of
    //    the requirement.
    sqlx::query(
        "UPDATE archava_commerce.memberships SET role='archava_assistant' \
         WHERE tenant_id=$1 AND principal_id=$2",
    )
    .bind(ACME)
    .bind("svc")
    .execute(&fixture.admin_pool)
    .await
    .expect("restore the membership role");

    let (status, outcome) = get(&app, &status_uri(request_id), Some(&session)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "restoring the grant must restore recovery of the receipt"
    );
    assert_eq!(outcome["decision"], "receipt");
    assert_eq!(outcome["receipt"]["action"], "product.read");
}

/// The GET query for one of this file's request ids.
fn status_uri(request_id: &str) -> String {
    format!("/commands/status?tenant={ACME}&request_id={request_id}")
}

// --- RN02: an idempotency key returns ITS OWN command's receipt ---------------

/// **RN02, sequential.** A key identifies a slot; the fingerprint identifies
/// the command. Reusing a key for a different command is a conflict and never
/// the other command's receipt — and retrying the SAME command gets the SAME
/// receipt, with one effect, not two.
#[tokio::test]
async fn a_key_reused_for_a_different_command_conflicts_on_execute_and_in_race() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);
    let session = login(&app, "svc").await;

    let key = "6b2ff3f1-9b57-4a35-b2ee-6dcee95b68a2";

    // First run: records slot `key` with input `first`.
    let (status, outcome) = post(
        &app,
        "/commands",
        &product_read_body(ACME, key, "{\"productId\":1}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let fingerprint_first = outcome["fingerprint"].as_str().expect("the fingerprint");

    // Second run, SAME key, DIFFERENT input. `product.read` with `{productId:
    // 1}` and with `{productId: 2}` are different commands; one key serves
    // one of them.
    let (status, outcome) = post(
        &app,
        "/commands",
        &product_read_body(ACME, key, "{\"productId\":2}"),
        Some(&session),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "reusing a key for another command must conflict"
    );
    assert_eq!(outcome["reason"], "request_id_reused");
    assert_eq!(
        outcome["receipt"],
        serde_json::Value::Null,
        "the conflicted retry must not receive the first command's receipt"
    );

    // Third run, SAME key, SAME input: an idempotent retry, answered as a
    // replay of the recorded receipt — with the SAME fingerprint.
    let (status, outcome) = post(
        &app,
        "/commands",
        &product_read_body(ACME, key, "{\"productId\":1}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outcome["decision"], "replayed");
    assert_eq!(
        outcome["fingerprint"].as_str(),
        Some(fingerprint_first),
        "the replay must be of the recorded command, not of the retry's input"
    );

    // Exactly one receipt rows for this key: one effect.
    let stored: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_commerce.commands WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(ACME)
    .bind(sqlx::types::Uuid::parse_str(key).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the stored receipts");
    assert_eq!(stored, 1);
}

/// **RN02, concurrent.** Two submissions of the same key with different
/// inputs, in flight at the same time, must not hand either the other's
/// receipt.
///
/// The audit's own probe held a row lock until two INSERT waiters were
/// observed; that construction is reproduced here with a `pg_advisory_xact_lock`
/// the winner holds while the loser is already inserted and waiting. The exact
/// interleaving is not what is pinned — what is pinned is the INVARIANT:
///
/// - one of the two wins and is told so;
/// - the loser is refused, and receives NO receipt for an input it did not
///   submit;
/// - only one command row exists for the key.
///
/// OS threads rather than tokio tasks, because the concurrency is the subject:
/// a blocked worker cannot be preempted, so logical concurrency on one runtime
/// can serialize exactly the way the original probe's flat result looked.
/// **RN02, concurrent.** Two submissions of the same key with different
/// inputs, in flight at the same time, must not hand either the other's
/// receipt.
///
/// What produces the collision deterministically is the same thing the audit
/// used: a database lock both requests must cross, held until both are parked.
/// A two-way wall of racing threads makes "did they collide" a property of the
/// scheduler, and a green row then proves the invariant only when the race
/// happened — the exact shape RN08 taught this suite not to accept. With the
/// lock, both submissions are stopped at the same statement, so the
/// interleaving is not a probability.
///
/// What is pinned:
///
/// - one of the two records and is told so (`allowed`);
/// - the loser answers `conflict`, carrying NO receipt for an input it did
///   not submit;
/// - exactly one command row exists for the key afterwards.
/// **RN02, concurrent.** Two submissions of the same key with different
/// inputs, in flight at the same time, must not hand either the other's
/// receipt.
///
/// What produces the collision deterministically is the same thing the
/// audit's probe used: a database lock both requests must cross, held until
/// both are parked. Two threads released together make "did they collide" a
/// property of the scheduler, and a green row then proves the invariant only
/// when the race happened — the exact shape RN08 taught this suite not to
/// accept. With the lock, both submissions are stopped at the same statement,
/// so the interleaving is a fact before anything is asserted.
///
/// What is pinned:
///
/// - one of the two records and is told so (`allowed`);
/// - the loser answers `conflict`, carrying NO receipt for an input it did
///   not submit;
/// - exactly one command row exists for the key afterwards.
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_same_key_different_input_yields_conflict_not_the_other_receipt() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);
    let session = login(&app, "svc").await;

    let key = "7c3ff402-9b57-4a35-b2ee-6dcee95b68a3";

    // Hold the table both INSERTs target, so both submissions park at their
    // write instead of one finishing before the other starts. The lock is
    // fixture administration on the superuser pool; the two requests still
    // take the RUNTIME role through the router.
    let (parked, release) = hold_commands_table(&fixture).await;

    // Each task carries its own copy of the credentials, because `Session`
    // is not `Clone` (the suite treats it as a value from `login`) and each
    // submission is an independent request.
    //
    // Both submissions spawn on THIS test's runtime — the same runtime the
    // fixture's pool belongs to — rather than one OS thread with a private
    // runtime apiece. The private-runtime shape is not one any deployment
    // has: a real serving host is one axum runtime, many request tasks on
    // it, and its sqlx pool is never touched by another runtime. Sharing one
    // pool across runtimes jams the pool's `test_before_acquire` health
    // check (a socket opened under runtime A polled under runtime B never
    // completes), which is a harness artifact, not host behavior — RN08's
    // lesson cuts both ways: a manufactured failure is as worthless as a
    // manufactured pass.
    let thread_two = {
        let app = app.clone();
        let token = session.token.clone();
        let csrf = session.csrf.clone();
        tokio::spawn(async move {
            let held = Session { token, csrf };
            post(
                &app,
                "/commands",
                &product_read_body(ACME, key, "{\"productId\":8}"),
                Some(&held),
            )
            .await
        })
    };

    let own = {
        let app = app.clone();
        let token = session.token.clone();
        let csrf = session.csrf.clone();
        tokio::spawn(async move {
            let held = Session { token, csrf };
            post(
                &app,
                "/commands",
                &product_read_body(ACME, key, "{\"productId\":9}"),
                Some(&held),
            )
            .await
        })
    };

    // Wait until both requests are recorded as waiting server-side, then open
    // the gate. `parked` resolving with 2 is what makes the collision a fact
    // rather than a race.
    let waiters = parked
        .await
        .expect("the lock holder reports the parked count");
    assert_eq!(
        waiters, 2,
        "both same-key submissions must have parked on the write lock"
    );
    release
        .send(())
        .expect("the lock holder is still waiting for the gate");

    let own = own.await.expect("the first submission task must finish");
    let other = thread_two
        .await
        .expect("the second submission task must finish");

    let statuses = [own.0, other.0];
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::OK)
            .count(),
        1,
        "exactly one of two same-key submissions may record, got {statuses:?}"
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::CONFLICT)
            .count(),
        1,
        "the loser must be conflicted, got {statuses:?}: {own:?} / {other:?}"
    );

    // And the loser's answer contains no receipt at all.
    for (status, outcome) in [own, other] {
        if status == StatusCode::CONFLICT {
            assert_eq!(
                outcome["receipt"],
                serde_json::Value::Null,
                "a conflicted submission never sees the winner's receipt: {outcome}"
            );
        }
    }

    // One key, one effect, one recording.
    let stored: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_commerce.commands WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(ACME)
    .bind(sqlx::types::Uuid::parse_str(key).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the stored receipts");
    assert_eq!(stored, 1);
}

/// Holds `LOCK TABLE archava_commerce.commands IN EXCLUSIVE MODE` until a
/// signal, telling the caller how many requests are parked behind it.
///
/// Returns `(parked, release)`: `parked` resolves with the number of
/// ungranted lock requests on the locked table when both submissions have
/// arrived; `release` opens the gate. The waiting is POLLED against the
/// server's own `pg_locks` view — the same server-side evidence the audit
/// recorded — with a deadline, so a submission that never reached the lock
/// fails the test instead of hanging it.
async fn hold_commands_table(
    fixture: &Fixture,
) -> (
    tokio::sync::oneshot::Receiver<usize>,
    tokio::sync::oneshot::Sender<()>,
) {
    let (gate_tx, gate_rx) = tokio::sync::oneshot::channel::<()>();
    let (report_tx, report_rx) = tokio::sync::oneshot::channel::<usize>();

    let admin = fixture.runtime_url();
    // A spawned task on THIS test's runtime, with its own private pool: the
    // lock must live on one connection that no other statement can steal.
    // Running it as a task inside the runtime the test already drives keeps
    // the process at exactly one runtime — the shape any real serving host
    // has — so no pool socket is ever created or polled under a foreign
    // driver (the cross-runtime jam RN08's neighbor, the kdf flood suite,
    // traced to `pool timed out while waiting for an open connection`).
    // `_holder`: the JoinHandle is dropped rather than awaited. Dropping a
    // task's handle detaches it, which is exactly what is wanted — the holder
    // ends on its own once the test body resumes or the gate sender drops —
    // and the final `stored` assertion already gates on both submissions
    // having completed, which the holder's commit precedes. A panic inside
    // the holder surfaces as a failed assertion here (the reported count
    // would not be 2) rather than a hang.
    let _holder = tokio::spawn(async move {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&admin)
            .await
            .expect("connect the lock holder");
        let mut transaction = pool.begin().await.expect("begin the lock");
        sqlx::query("LOCK TABLE archava_commerce.commands IN EXCLUSIVE MODE")
            .execute(&mut *transaction)
            .await
            .expect("lock the commands table");

        // Now park until both requests have actually arrived and blocked on
        // this lock. The count comes from `pg_locks`, filtered to the locked
        // table, so nothing else in the process can be mistaken for one of
        // the two submissions. The deadline only bounds the wait; falling
        // short reports the real count and the caller's assert turns it into
        // a failing test rather than a hang.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let waiting = loop {
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_locks WHERE NOT granted \
                 AND relation = 'archava_commerce.commands'::regclass",
            )
            .fetch_one(&pool)
            .await
            .expect("count the parked lock requests");
            if count >= 2 || std::time::Instant::now() >= deadline {
                break count as usize;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        };

        // Hand the count to the test, then hold the lock until the test says
        // go. The test asserts the count BEFORE opening the gate, so the
        // interleaving it asserts on is the one it observed. A dropped gate
        // sender still releases this await, so a test that fails early
        // cannot leave a parked lock behind.
        let _ = report_tx.send(waiting);
        let _ = gate_rx.await;

        transaction.commit().await.expect("release the lock");
    });
    // The JoinHandle is dropped, not awaited: the spawned holder ends on its
    // own once the test body resumes or the gate sender drops, and the final
    // `stored` assertion already gates on both submissions having completed —
    // which the holder's commit precedes. A panic inside the holder surfaces
    // as a failed assertion here rather than a hang, because the count it
    // reports would never be 2.
    (report_rx, gate_tx)
}
