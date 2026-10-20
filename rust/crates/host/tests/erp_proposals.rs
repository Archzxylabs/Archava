//! The R04 proposals cohort's host witnesses — the stock pool's FOURTH
//! writer, driven through the real router (`tower::ServiceExt::oneshot`)
//! against the disposable PostgreSQL whose `archava_proposals` schema is the
//! real migration ledger (`001_proposals_scope_amendments`). Every assertion
//! is a fact about what the SERVER did:
//!
//! - the authorized flow (create → set_scope_items → submit → accept) ends
//!   with the accepted proposal, the create's scope items rewritten ONCE by
//!   the swapped draft, and ONE accepted snapshot row — the v1 scope
//!   version with the accepted items JSON;
//! - an accept whose proposal is not submitted refuses and holds nothing —
//!   no snapshot, no accepted state, the claim withdrawn;
//! - the reject branch records `rejected` exactly once, with the decide
//!   permission required;
//! - a revoked proposals grant refuses the recovery — yesterday's receipts
//!   are not replayable through revoked access;
//! - the same idempotency key with a DIFFERENT command conflicts without
//!   recording;
//! - a cross-tenant caller is refused before any business statement runs
//!   while the same caller acts freely in its own ledger;
//! - a lost response re-submits the SAME key + bytes and answers `replayed`
//!   with no second snapshot;
//! - the change-request surface: a CR drafted over the ACCEPTED proposal
//!   amends nothing else — a SECOND draft conflicts, the amendment updates,
//!   submits and approves, and the approved snapshot carries the CR-1
//!   amendment line with the deltas;
//! - an actor whose proposals grants are missing is refused by the grant
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
/// every ERP suite pins; the proposals surface RIDES the same serving state
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
/// and the purchasing ledger (`try_new_erp`'s second pass) — the proposals
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

/// The proposals permission vocabulary (migration 001's CHECK).
const PROPOSAL_ALL: &[&str] = &[
    "proposals.read",
    "proposals.write",
    "proposals.decide",
    "changerequests.read",
    "changerequests.write",
    "changerequests.decide",
    "audit.read",
];

/// `px` is the proposals manager: full grants, scope all — the positive-path
/// actor.
/// `pno` holds only `audit.read` — every grant gate refuses.
/// `poff` is `px` with the proposals membership seeded ACTIVE and revoked by
/// the test itself — the recovery must not outlive the grant.
/// `pother` holds full grants but ONLY in GLOBEX — the cross-tenant refusal.
/// `pgx` is GLOBEX's own full-grant manager — the per-tenant
/// key-independence witness.
async fn seed(fixture: &Fixture) {
    for principal in ["px", "pno", "poff", "pother", "pgx"] {
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
        "INSERT INTO archava_proposals.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind("px")
    .bind(PROPOSAL_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed px's proposals membership");
    sqlx::query(
        "INSERT INTO archava_proposals.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind("poff")
    .bind(PROPOSAL_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed poff's ACTIVE proposals membership");
    sqlx::query(
        "INSERT INTO archava_proposals.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind("pno")
    .bind(["audit.read"].to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed pno's proposals membership");
    for principal in ["pother", "pgx"] {
        sqlx::query(
            "INSERT INTO archava_proposals.memberships \
             (tenant_id, principal_id, kind, permissions, scope, active) \
             VALUES ($1, $2, 'human', $3, 'all', true)",
        )
        .bind(GLOBEX)
        .bind(principal)
        .bind(PROPOSAL_ALL.to_vec())
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the GLOBEX proposals membership");
    }
}

/// Fixed idempotency keys — stable so readbacks and the recovery witness can
/// name rows exactly.
const KEY_CREATE: &str = "af59c000-3334-4333-8333-0000000001a1";
const KEY_SET: &str = "af59c000-3334-4333-8333-0000000001a2";
const KEY_SUBMIT: &str = "af59c000-3334-4333-8333-0000000001a3";
const KEY_ACCEPT: &str = "af59c000-3334-4333-8333-0000000001a4";
const KEY_REJECT: &str = "af59c000-3334-4333-8333-0000000001a5";
const KEY_HIJACK: &str = "af59c000-3334-4333-8333-0000000001a6";
const KEY_XTENANT: &str = "af59c000-3334-4333-8333-0000000001a7";
const KEY_REVOKED: &str = "af59c000-3334-4333-8333-0000000001a8";
const KEY_CR_ONE: &str = "af59c000-3334-4333-8333-0000000001a9";
const KEY_CR_TWO: &str = "af59c000-3334-4333-8333-0000000001aa";
const KEY_CR_UPDATE: &str = "af59c000-3334-4333-8333-0000000001ab";
const KEY_CR_SUBMIT: &str = "af59c000-3334-4333-8333-0000000001ac";
const KEY_CR_APPROVE: &str = "af59c000-3334-4333-8333-0000000001ad";

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

/// The two-item draft every create and swap uses — typed so the byte-equal
/// re-submission witnesses diff NOTHING.
fn scope_items(a: &str, b: &str) -> Value {
    json!([
        {
            "title": a,
            "description": format!("Delivery of {a}"),
            "estimatedDays": 5,
            "estimatedValue": 1_000,
            "deliverableType": "service"
        },
        {
            "title": b,
            "description": format!("Installation of {b}"),
            "estimatedValue": 7_000,
            "deliverableType": "service"
        }
    ])
}

fn create_body(tenant: &str, command_id: &str, title: &str, items: Value) -> String {
    json!({
        "tenant": tenant,
        "mutation": {
            "type": "create_proposal",
            "commandId": command_id,
            "title": title,
            "summary": format!("{title} for the annual procurement round"),
            "organizationId": null,
            "contactId": null,
            "leadId": null,
            "currency": "IDR",
            "scopeItems": items,
        }
    })
    .to_string()
}

fn set_scope_body(
    tenant: &str,
    command_id: &str,
    proposal_id: &str,
    expected_revision: i64,
    items: Value,
) -> String {
    json!({
        "tenant": tenant,
        "mutation": {
            "type": "set_scope_items",
            "commandId": command_id,
            "proposalId": proposal_id,
            "expectedRevision": expected_revision,
            "scopeItems": items,
        }
    })
    .to_string()
}

fn transition_body(
    tenant: &str,
    arm: &str,
    command_id: &str,
    proposal_id: &str,
    expected_revision: i64,
) -> String {
    let mut mutation = json!({
        "type": arm,
        "commandId": command_id,
        "proposalId": proposal_id,
        "expectedRevision": expected_revision,
    });
    if arm == "accept_proposal" {
        mutation["linkedProjectId"] = Value::Null;
    }
    if arm == "reject_proposal" {
        mutation["reason"] = json!("the panel chose another vendor");
    }
    json!({ "tenant": tenant, "mutation": mutation }).to_string()
}

fn cr_create_body(tenant: &str, command_id: &str, proposal_id: &str, title: &str) -> String {
    json!({
        "tenant": tenant,
        "mutation": {
            "type": "create_change_request",
            "commandId": command_id,
            "proposalId": proposal_id,
            "title": title,
            "reason": "the client enlarged the plant",
            "deltaSummary": format!("{title} plus a second installation crew"),
            "timeDeltaDays": 3,
            "costDeltaValue": 2_500,
            "newScopeItems": scope_items("Pilot line", "Commissioning"),
        }
    })
    .to_string()
}

fn cr_update_body(tenant: &str, command_id: &str, cr_id: &str, expected_revision: i64) -> String {
    json!({
        "tenant": tenant,
        "mutation": {
            "type": "update_change_request",
            "commandId": command_id,
            "changeRequestId": cr_id,
            "expectedRevision": expected_revision,
            "title": "Doorman expansion revised",
            "reason": "the plant added a wing",
            "deltaSummary": "the second crew plus a night shift",
            "timeDeltaDays": 4,
            "costDeltaValue": 3_000,
            "newScopeItems": scope_items("Pilot line", "Night commissioning"),
        }
    })
    .to_string()
}

fn cr_transition_body(
    tenant: &str,
    arm: &str,
    command_id: &str,
    cr_id: &str,
    expected_revision: i64,
) -> String {
    let mut mutation = json!({
        "type": arm,
        "commandId": command_id,
        "changeRequestId": cr_id,
        "expectedRevision": expected_revision,
    });
    if arm == "reject_change_request" {
        mutation["reason"] = json!("out of the fiscal window");
    }
    json!({ "tenant": tenant, "mutation": mutation }).to_string()
}

/// A malformed arm — the grammar never admits it — for the boundary-400
/// witness.
fn malformed_body(tenant: &str, command_id: &str) -> String {
    json!({
        "tenant": tenant,
        "mutation": {
            "type": "revive_proposal",
            "commandId": command_id,
        }
    })
    .to_string()
}

// --- Database probes ----------------------------------------------------------

/// The proposal's `(state, revision, current_scope_version, accepted?)`.
async fn proposal_row(fixture: &Fixture, tenant: &str, id: &str) -> (String, i32, i32, bool) {
    sqlx::query_as(
        "SELECT state,revision,current_scope_version,(accepted_at IS NOT NULL) \
         FROM archava_proposals.proposals WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(tenant)
    .bind(id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the proposal exists")
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

/// THE core witness: the authorized flow documents its whole life in one
/// chain — the create receipt with the embedded draft's items, the scope
/// swap that rewrites v1 in place, the submit, the accept that freezes the
/// v1 snapshot — and the status read answers `committed` with the stored
/// receipt.
#[tokio::test]
async fn authorized_proposal_flow_documents_its_life() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "px").await;

    // The create receipt carries the proposal id and the draft's version.
    let (status, receipt) = post(
        &app,
        "/erp/proposals/commands",
        &create_body(
            ACME,
            KEY_CREATE,
            "Doorman renewal",
            scope_items("Standard doorman", "Rail assembly"),
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create_proposal is authorized");
    assert_eq!(receipt["decision"], "receipt");
    assert_eq!(receipt["receipt"]["status"], "committed");
    assert_eq!(receipt["receipt"]["revision"], 1);
    assert_eq!(receipt["receipt"]["scopeVersion"], 1);
    let proposal_id = receipt["receipt"]["proposalId"]
        .as_str()
        .expect("the receipt names the proposal id")
        .to_string();
    let row = proposal_row(&fixture, ACME, &proposal_id).await;
    assert_eq!(row, ("draft".into(), 1, 1, false));
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_proposals.scope_items \
             WHERE tenant_id=$1 AND proposal_id=$2::uuid AND scope_version=1",
            &[ACME, &proposal_id],
        )
        .await,
        2,
        "the create inserted both draft items as version 1"
    );

    // The swap rewrites v1 in place and bumps the revision.
    let (status, swap) = post(
        &app,
        "/erp/proposals/commands",
        &set_scope_body(
            ACME,
            KEY_SET,
            &proposal_id,
            1,
            scope_items("Standard doorman sets", "Rail assembly"),
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "set_scope_items is authorized");
    assert_eq!(swap["receipt"]["revision"], 2);
    assert_eq!(swap["receipt"]["scopeVersion"], 1);
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_proposals.scope_items \
             WHERE tenant_id=$1 AND proposal_id=$2::uuid AND title='Standard doorman sets'",
            &[ACME, &proposal_id],
        )
        .await,
        1,
        "the swapped draft replaced the create's first item"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_proposals.scope_items \
             WHERE tenant_id=$1 AND proposal_id=$2::uuid AND scope_version=1",
            &[ACME, &proposal_id],
        )
        .await,
        2,
        "version 1 still holds exactly two items"
    );

    // The submit moves draft → submitted.
    let (status, submit_receipt) = post(
        &app,
        "/erp/proposals/commands",
        &transition_body(ACME, "submit_proposal", KEY_SUBMIT, &proposal_id, 2),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(submit_receipt["receipt"]["revision"], 3);
    assert_eq!(submit_receipt["receipt"]["scopeVersion"], Value::Null);
    let row = proposal_row(&fixture, ACME, &proposal_id).await;
    assert_eq!(row.0, "submitted");

    // The accept freezes the v1 snapshot beside the decision.
    let (status, accept_receipt) = post(
        &app,
        "/erp/proposals/commands",
        &transition_body(ACME, "accept_proposal", KEY_ACCEPT, &proposal_id, 3),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "accept_proposal is authorized");
    assert_eq!(accept_receipt["receipt"]["revision"], 4);
    assert_eq!(accept_receipt["receipt"]["scopeVersion"], 1);
    let row = proposal_row(&fixture, ACME, &proposal_id).await;
    assert_eq!(row, ("accepted".into(), 4, 1, true));
    let snapshot = sqlx::query_as::<_, (i32, String, i64, i32, Option<String>)>(
        "SELECT scope_version,summary,total_value,total_days,change_request_id::text \
         FROM archava_proposals.scope_versions \
         WHERE tenant_id=$1 AND proposal_id=$2::uuid",
    )
    .bind(ACME)
    .bind(&proposal_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the accepted snapshot exists");
    assert_eq!(snapshot.0, 1);
    assert_eq!(
        snapshot.1, "Doorman renewal for the annual procurement round",
        "the snapshot carries the accepted proposal's summary"
    );
    assert_eq!(
        snapshot.2, 8000,
        "the snapshot's total is the accepted items' sum"
    );
    assert_eq!(
        snapshot.3, 5,
        "the snapshot's days are the accepted items' sum"
    );
    assert_eq!(snapshot.4, None, "a first acceptance has no change request");

    // The status read answers `committed` with the stored receipt.
    let (status, status_body) = get(
        &app,
        &format!("/erp/proposals/commands/status?tenant={ACME}&commandId={KEY_ACCEPT}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(status_body["decision"], "committed");
    assert_eq!(status_body["receipt"]["status"], "committed");
    assert_eq!(
        status_body["receipt"]["proposalId"],
        Value::String(proposal_id.clone())
    );
}

/// An accept against a proposal that never reached `submitted` refuses and
/// holds NOTHING: the state gate conflicts, the whole submission rolls back,
/// no snapshot exists, and the refused request's command row never lands.
#[tokio::test]
async fn accept_before_submit_refuses_and_holds_nothing() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "px").await;

    let (status, receipt) = post(
        &app,
        "/erp/proposals/commands",
        &create_body(
            ACME,
            KEY_CREATE,
            "Eager draft",
            scope_items("Item", "Second item"),
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let proposal_id = receipt["receipt"]["proposalId"]
        .as_str()
        .expect("the receipt names the proposal id")
        .to_string();

    let (status, refusal) = post(
        &app,
        "/erp/proposals/commands",
        &transition_body(ACME, "accept_proposal", KEY_ACCEPT, &proposal_id, 1),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "an accept over a draft conflicts at the state gate"
    );
    assert_eq!(refusal["decision"], "refused");
    let row = proposal_row(&fixture, ACME, &proposal_id).await;
    assert_eq!(row, ("draft".into(), 1, 1, false), "the state never moved");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_proposals.scope_versions WHERE tenant_id=$1",
            &[ACME],
        )
        .await,
        0,
        "no snapshot froze"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_proposals.commands \
             WHERE tenant_id=$1 AND command_id=$2::uuid",
            &[ACME, KEY_ACCEPT],
        )
        .await,
        0,
        "the refused submission was never even claimed"
    );
}

/// The reject branch moves `submitted → rejected` exactly once — the decide
/// permission behind it — and a SECOND reject under a fresh key conflicts
/// without recording.
#[tokio::test]
async fn reject_records_rejected_exactly_once() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "px").await;

    let (status, receipt) = post(
        &app,
        "/erp/proposals/commands",
        &create_body(
            ACME,
            KEY_CREATE,
            "Second vendor",
            scope_items("Item", "Second item"),
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let proposal_id = receipt["receipt"]["proposalId"]
        .as_str()
        .expect("the receipt names the proposal id")
        .to_string();
    let (status, _) = post(
        &app,
        "/erp/proposals/commands",
        &transition_body(ACME, "submit_proposal", KEY_SUBMIT, &proposal_id, 1),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, reject_receipt) = post(
        &app,
        "/erp/proposals/commands",
        &transition_body(ACME, "reject_proposal", KEY_REJECT, &proposal_id, 2),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the reject records");
    assert_eq!(reject_receipt["receipt"]["revision"], 3);
    let row = proposal_row(&fixture, ACME, &proposal_id).await;
    assert_eq!(row, ("rejected".into(), 3, 1, false));
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_proposals.audit \
             WHERE tenant_id=$1 AND operation='reject_proposal'",
            &[ACME],
        )
        .await,
        1,
        "the reject's audit row is on the ledger"
    );

    // A second reject under a fresh key conflicts at the state gate —
    // a rejected proposal takes no more submissions.
    let (status, second) = post(
        &app,
        "/erp/proposals/commands",
        &transition_body(ACME, "reject_proposal", KEY_CR_TWO, &proposal_id, 3),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(second["decision"], "refused");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_proposals.audit \
             WHERE tenant_id=$1 AND operation='reject_proposal'",
            &[ACME],
        )
        .await,
        1,
        "no second rejection"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_proposals.commands \
             WHERE tenant_id=$1 AND command_id=$2::uuid",
            &[ACME, KEY_CR_TWO],
        )
        .await,
        0,
        "the refused resubmit never landed"
    );
}

/// A receipt the CURRENT access no longer authorizes does not replay: the
/// same key + same bytes re-submitted after the grant was revoked is refused
/// by the grant gate, and the status read refuses equally — an old receipt
/// is not a permanent grant.
#[tokio::test]
async fn revoked_proposals_grant_refuses_recovery() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "poff").await;

    let (status, receipt) = post(
        &app,
        "/erp/proposals/commands",
        &create_body(
            ACME,
            KEY_REVOKED,
            "Vested proposal",
            scope_items("Item", "Second item"),
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the active grant records");
    assert_eq!(receipt["decision"], "receipt");

    sqlx::query(
        "UPDATE archava_proposals.memberships SET active=false \
         WHERE tenant_id=$1 AND principal_id='poff'",
    )
    .bind(ACME)
    .execute(&fixture.admin_pool)
    .await
    .expect("revoke poff's proposals membership");

    let (status, replay) = post(
        &app,
        "/erp/proposals/commands",
        &create_body(
            ACME,
            KEY_REVOKED,
            "Vested proposal",
            scope_items("Item", "Second item"),
        ),
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
            "SELECT count(*) FROM archava_proposals.proposals \
             WHERE tenant_id=$1 AND title='Vested proposal'",
            &[ACME],
        )
        .await,
        1,
        "no SECOND proposal — the replay recorded nothing new"
    );

    let (status, status_body) = get(
        &app,
        &format!("/erp/proposals/commands/status?tenant={ACME}&commandId={KEY_REVOKED}"),
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
    let session = login(&app, "px").await;

    let (status, first) = post(
        &app,
        "/erp/proposals/commands",
        &create_body(
            ACME,
            KEY_CREATE,
            "Original proposal",
            scope_items("Item", "Second item"),
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["decision"], "receipt");
    let proposal_id = first["receipt"]["proposalId"]
        .as_str()
        .expect("the receipt names the proposal id")
        .to_string();

    // The attacker submits a scope swap under the SAME key: bytes differ,
    // fingerprint differs → conflict, nothing recorded.
    let (status, refusal) = post(
        &app,
        "/erp/proposals/commands",
        &set_scope_body(
            ACME,
            KEY_CREATE,
            &proposal_id,
            1,
            scope_items("Hijacked item", "Another hijack"),
        ),
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
            "SELECT count(*) FROM archava_proposals.scope_items \
             WHERE tenant_id=$1 AND proposal_id=$2::uuid AND title='Hijacked item'",
            &[ACME, &proposal_id],
        )
        .await,
        0,
        "the hijacked items were never written"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_proposals.commands \
             WHERE command_id=$1::uuid AND receipt IS NOT NULL",
            &[KEY_CREATE],
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
    let session = login(&app, "pother").await;

    let (status, refusal) = post(
        &app,
        "/erp/proposals/commands",
        &create_body(
            ACME,
            KEY_XTENANT,
            "Invader proposal",
            scope_items("Item", "Second item"),
        ),
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
            "SELECT count(*) FROM archava_proposals.proposals WHERE tenant_id=$1",
            &[ACME],
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_proposals.commands \
             WHERE tenant_id=$1 AND command_id=$2::uuid",
            &[ACME, KEY_XTENANT],
        )
        .await,
        0,
        "no command row was even claimed in the target ledger"
    );

    let own = login(&app, "pgx").await;
    let (status, receipt) = post(
        &app,
        "/erp/proposals/commands",
        &create_body(
            GLOBEX,
            KEY_XTENANT,
            "Legit globex proposal",
            scope_items("Item", "Second item"),
        ),
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
/// snapshot exists.
#[tokio::test]
async fn lost_response_replays_without_a_second_snapshot() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "px").await;

    let (status, first) = post(
        &app,
        "/erp/proposals/commands",
        &create_body(
            ACME,
            KEY_CREATE,
            "Recoverable proposal",
            scope_items("Item", "Second item"),
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the first submit records");
    assert_eq!(first["decision"], "receipt");
    let proposal_id = first["receipt"]["proposalId"]
        .as_str()
        .expect("the receipt names the proposal id")
        .to_string();

    let (status, replay) = post(
        &app,
        "/erp/proposals/commands",
        &create_body(
            ACME,
            KEY_CREATE,
            "Recoverable proposal",
            scope_items("Item", "Second item"),
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "an identical resubmit replays");
    assert_eq!(replay["decision"], "replayed");
    assert_eq!(
        replay["receipt"]["proposalId"],
        Value::String(proposal_id.clone())
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_proposals.proposals WHERE tenant_id=$1",
            &[ACME],
        )
        .await,
        1,
        "still exactly one proposal — no double create"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_proposals.scope_versions WHERE tenant_id=$1",
            &[ACME],
        )
        .await,
        0,
        "no snapshot froze"
    );

    let (status, status_body) = get(
        &app,
        &format!("/erp/proposals/commands/status?tenant={ACME}&commandId={KEY_CREATE}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(status_body["decision"], "committed");
}

/// The change-request surface: a CR drafts over the ACCEPTED proposal, a
/// SECOND draft conflicts, the amendment updates and approves — and the
/// approved snapshot carries the CR-1 amendment line with the deltas.
#[tokio::test]
async fn change_request_lifecycle_amends_the_scope() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "px").await;

    // Accepted base: create → submit → accept.
    let (status, receipt) = post(
        &app,
        "/erp/proposals/commands",
        &create_body(
            ACME,
            KEY_CREATE,
            "Doorman base",
            scope_items("Standard sets", "Rail spares"),
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let proposal_id = receipt["receipt"]["proposalId"]
        .as_str()
        .expect("the receipt names the proposal id")
        .to_string();
    let (status, _) = post(
        &app,
        "/erp/proposals/commands",
        &transition_body(ACME, "submit_proposal", KEY_SUBMIT, &proposal_id, 1),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = post(
        &app,
        "/erp/proposals/commands",
        &transition_body(ACME, "accept_proposal", KEY_ACCEPT, &proposal_id, 2),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The CR drafts over the accepted proposal.
    let (status, cr) = post(
        &app,
        "/erp/proposals/commands",
        &cr_create_body(ACME, KEY_CR_ONE, &proposal_id, "Doorman expansion"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the CR drafts");
    assert_eq!(cr["receipt"]["revision"], 1, "the CR's own revision");
    let change_request_id = cr["receipt"]["changeRequestId"]
        .as_str()
        .expect("the receipt names the change request id")
        .to_string();

    // A SECOND draft over the same proposal conflicts — one active CR at a
    // time.
    let (status, second) = post(
        &app,
        "/erp/proposals/commands",
        &cr_create_body(ACME, KEY_CR_TWO, &proposal_id, "Rival expansion"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a second active change request conflicts"
    );
    assert_eq!(second["decision"], "refused");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_proposals.change_requests WHERE tenant_id=$1",
            &[ACME],
        )
        .await,
        1,
        "only the first draft stands"
    );

    // The amendment updates while the CR is still a draft.
    let (status, updated) = post(
        &app,
        "/erp/proposals/commands",
        &cr_update_body(ACME, KEY_CR_UPDATE, &change_request_id, 1),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["receipt"]["revision"], 2);

    // Submit then approve: the approved snapshot carries the amendment.
    let (status, _) = post(
        &app,
        "/erp/proposals/commands",
        &cr_transition_body(
            ACME,
            "submit_change_request",
            KEY_CR_SUBMIT,
            &change_request_id,
            2,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, approved) = post(
        &app,
        "/erp/proposals/commands",
        &cr_transition_body(
            ACME,
            "approve_change_request",
            KEY_CR_APPROVE,
            &change_request_id,
            3,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the approval records");
    assert_eq!(
        approved["receipt"]["revision"], 4,
        "the CR's decided revision"
    );
    assert_eq!(
        approved["receipt"]["scopeVersion"], 2,
        "the resulting scope version"
    );

    // The proposals row moved: the current scope version advanced and the
    // revision followed the amendment.
    let row = proposal_row(&fixture, ACME, &proposal_id).await;
    assert_eq!(row.0, "accepted", "the proposal stays accepted");
    assert_eq!(row.1, 4, "the proposal revision followed the amendment");
    assert_eq!(row.2, 2, "the current scope version advanced");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_proposals.scope_versions \
             WHERE tenant_id=$1 AND proposal_id=$2::uuid",
            &[ACME, &proposal_id],
        )
        .await,
        2,
        "the amendment froze a second snapshot"
    );

    // The v2 items: the two clones plus the amendment line.
    let items = sqlx::query_as::<_, (String, Option<i32>, i64)>(
        "SELECT title,estimated_days,estimated_value FROM archava_proposals.scope_items \
         WHERE tenant_id=$1 AND proposal_id=$2::uuid AND scope_version=2 ORDER BY created_at,id",
    )
    .bind(ACME)
    .bind(&proposal_id)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("the amended items read");
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].0, "Standard sets", "the clone kept the item");
    assert_eq!(items[1].0, "Rail spares", "the clone kept the item");
    assert_eq!(
        items[2].0, "CR-1: Doorman expansion revised",
        "the amendment line carries the updated title"
    );
    assert_eq!(items[2].1, Some(4), "the day delta carried");
    assert_eq!(items[2].2, 3000, "the cost delta carried");

    // The CR row is approved and references its resulting version.
    let cr_row = sqlx::query_as::<_, (String, Option<i32>)>(
        "SELECT state,resulting_scope_version FROM archava_proposals.change_requests \
         WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(ACME)
    .bind(&change_request_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the change request row exists");
    assert_eq!(cr_row, ("approved".into(), Some(2)));
    let snapshot = sqlx::query_as::<_, (String, i64, i32, Option<String>)>(
        "SELECT summary,total_value,total_days,change_request_id::text \
         FROM archava_proposals.scope_versions \
         WHERE tenant_id=$1 AND proposal_id=$2::uuid AND scope_version=2",
    )
    .bind(ACME)
    .bind(&proposal_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the amended snapshot exists");
    assert_eq!(
        snapshot.0, "Agreed Scope Version 2 (Amendment via CR-1)",
        "the amendment snapshot's summary"
    );
    assert_eq!(snapshot.1, 11000, "8000 of clones + the 3000 amendment");
    assert_eq!(snapshot.2, 9, "5 clone days + the 4 day delta");
    assert_eq!(snapshot.3.as_deref(), Some(change_request_id.as_str()));
}

/// An actor whose proposals grants are missing is refused by the grant gate
/// with no command row at all — the `audit.read`-only clerk cannot write any
/// more than a stranger can. A malformed arm, next, is a boundary 400 that
/// never reaches a database statement.
#[tokio::test]
async fn grants_missing_actor_refused_before_any_write() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "pno").await;

    let (status, refusal) = post(
        &app,
        "/erp/proposals/commands",
        &create_body(
            ACME,
            KEY_CREATE,
            "Forbidden proposal",
            scope_items("Item", "Second item"),
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "the grant gate refuses");
    assert_eq!(refusal["decision"], "denied");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_proposals.proposals WHERE tenant_id=$1",
            &[ACME],
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_proposals.commands \
             WHERE tenant_id=$1 AND command_id=$2::uuid",
            &[ACME, KEY_CREATE],
        )
        .await,
        0,
        "the refusal happened before the claim"
    );

    let (status, rejected) = post(
        &app,
        "/erp/proposals/commands",
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
            "SELECT count(*) FROM archava_proposals.commands WHERE command_id=$1::uuid",
            &[KEY_HIJACK],
        )
        .await,
        0
    );
}
