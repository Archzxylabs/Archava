//! P-RUST-BROWSER's backend half — the §1.4B vertical slice against real HTTP
//! and real PostgreSQL.
//!
//! ## Why this suite exists
//!
//! §3 of the mandate: "Empat baris komentar, health route atau placeholder
//! contacts tidak menutup vertical slice" — a placeholder `/contacts` refusal
//! and a receipt-shaped answer without an effect do not deliver the slice.
//! Before this suite, the host served the commerce authority end to end, and
//! the CRM contact route was an explicit refusal-route placeholder.
//!
//! This file drives the CRM slice through the real router (`tower::ServiceExt::
//! oneshot`), against a disposable PostgreSQL whose `archava_crm` schema is the
//! real migration ledger (`001_crm_contacts`, `002_sales_pipeline`,
//! `003_native_approval`). Every assertion is a fact about what the SERVER did:
//!
//! - the contact list reads REAL rows through the session's CRM membership
//!   (`contacts.read` + assigned/all scope), not a canned payload;
//! - a reviewed `followup.create` produces a REAL business effect: a
//!   `followups` row, a bumped `contacts.revision`, an audit row (with
//!   `initiator_id` for the service path) and a durable receipt, all in the
//!   one transaction the caller's key claims;
//! - the recovery route answers `committed` with that receipt — against a
//!   fresh state (restart), after a lost response (replay), and refuses when
//!   the caller's CURRENT grants no longer admit it;
//! - revocation, wrong grants, a stale revision, an out-of-scope contact and
//!   an unimplemented operation refuse without recording; a racing
//!   same-key submission yields exactly one effect (server-side lock +
//!   `pg_locks`-observed waiters, the same harness `authority.rs` uses).
//!
//! The service-on-behalf-of witnesses (grant intersection, initiator
//! recording, scope containment) drive [`archava_host::erp::crm::submit_followup`]
//! directly — the same core the HTTP route calls with a session-resolved human
//! — because the TS baseline reaches native actions through the operator's
//! epoch-controlled daemon, not through a public HTTP route.
//!
//! negatives here are the backend equivalents of `P-RUST-BROWSER`'s required
//! checks (denied / forged-tenant / revoked authority, CSRF, stale revision,
//! duplicate/racing submission, refresh / lost-response / restart recovery);
//! the browser half then proves the same flow through the visible UI.

use archava_core::tenant::TenantId;
use archava_host::erp::crm::{
    ActivityKind, ContactFields, ContactPatch, CrmActor, CrmCommand, CrmSubmission, PipelineStage,
};
use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::routing::Router;
use sqlx::types::Uuid;
use tower::ServiceExt;

/// The commerce ledger this host serves alongside CRM — same table list as
/// `authority.rs`.
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
const IDENTITY_KEY: &[u8] = &[7u8; 32];

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

/// Serving state that has passed BOTH readiness gates: commerce (`try_new`) and
/// the CRM ledger (`try_new_crm`'s second pass).
async fn state(fixture: &Fixture) -> AppState {
    AppState::try_new_crm(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
        &fixture.crm_ledger,
    )
    .await
    .expect("a ready fixture must construct serving state")
}

// --- The cast -----------------------------------------------------------------

/// The tenants this suite uses.
const ACME: &str = "acme-retail";
const GLOBEX: &str = "globex-corp";

struct Principal {
    id: &'static str,
    kind: &'static str,
    permissions: &'static [&'static str],
    scope: &'static str,
    active: bool,
    tenants: &'static [&'static str],
}

/// The CRM cast, in the vocabulary the memberships table carries (no
/// role/capability columns — CRM permissions ARE the grant):
///
/// - `alice` is the human reviewer with the full read/write surface, scope
///   `all` — the owner of the contacts the reviewed command touches.
/// - `bob` holds reads only — the missing-write-grant refusal.
/// - `carol` holds write AND reads but scope `assigned`, owning one contact —
///   the scope-containment refusals.
/// - `dave` holds only `followups.write`, scope `all` — the missing-read-grant
///   refusals (list and receipt recovery).
/// - `svc` is a service principal with the assembly the human path accepts —
///   the service-on-behalf positive.
/// - `svc-narrow` is a service principal whose write grant the intended human
///   does not carry — the intersection refusal.
/// - `revoked` is `alice` with `active=false` — one shape with absent.
/// - `another` is a reader in `globex` only — the cross-tenant refusal.
const PRINCIPALS: &[Principal] = &[
    Principal {
        id: "alice",
        kind: "human",
        permissions: &[
            "contacts.read",
            "contacts.write",
            "activities.read",
            "activities.write",
            "followups.read",
            "followups.write",
            "pipeline.read",
            "pipeline.write",
        ],
        scope: "all",
        active: true,
        tenants: &[ACME, GLOBEX],
    },
    Principal {
        id: "bob",
        kind: "human",
        permissions: &[
            "contacts.read",
            "activities.read",
            "followups.read",
            "pipeline.read",
        ],
        scope: "all",
        active: true,
        tenants: &[ACME],
    },
    Principal {
        id: "carol",
        kind: "human",
        permissions: &[
            "contacts.read",
            "contacts.write",
            "activities.read",
            "activities.write",
            "followups.read",
            "followups.write",
            "pipeline.read",
            "pipeline.write",
        ],
        scope: "assigned",
        active: true,
        tenants: &[ACME],
    },
    Principal {
        id: "dave",
        kind: "human",
        permissions: &["followups.write"],
        scope: "all",
        active: true,
        tenants: &[ACME],
    },
    Principal {
        id: "svc",
        kind: "service",
        permissions: &["contacts.read", "followups.read", "followups.write"],
        scope: "all",
        active: true,
        tenants: &[ACME],
    },
    Principal {
        id: "svc-narrow",
        kind: "service",
        permissions: &["followups.write"],
        scope: "all",
        active: true,
        tenants: &[ACME],
    },
    Principal {
        id: "revoked",
        kind: "human",
        permissions: &[
            "contacts.read",
            "contacts.write",
            "followups.read",
            "followups.write",
        ],
        scope: "all",
        active: false,
        tenants: &[ACME],
    },
    Principal {
        id: "another",
        kind: "human",
        permissions: &["contacts.read"],
        scope: "all",
        active: true,
        tenants: &[GLOBEX],
    },
];

/// The their contacts and requests this suite addresses, all fixed so the DB
/// assertions can name rows exactly.
const ID_ALICE_A: &str = "2c3e5a10-1111-4111-8111-0000000000a1";
const ID_ALICE_B: &str = "2c3e5a10-1111-4111-8111-0000000000a2";
const ID_CAROL: &str = "2c3e5a10-1111-4111-8111-0000000000b1";
const ID_ESCAPED: &str = "2c3e5a10-1111-4111-8111-0000000000c1";
const ID_PLAIN: &str = "2c3e5a10-1111-4111-8111-0000000000c2";
const ID_GLOBEX: &str = "2c3e5a10-1111-4111-8111-0000000000d1";

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
                "INSERT INTO archava_crm.memberships \
                 (tenant_id, principal_id, kind, permissions, scope, active) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(tenant)
            .bind(principal.id)
            .bind(principal.kind)
            .bind(principal.permissions.to_vec())
            .bind(principal.scope)
            .bind(principal.active)
            .execute(&fixture.admin_pool)
            .await
            .expect("seed the CRM membership");
        }
    }

    // The contacts. Each row is a real business record the list and the
    // reviewed command address; `owner_id` carries the assigned-scope binding
    // and is FK-checked against the membership, so the cast above must hold
    // the row's tenant.
    for (tenant, id, full_name, email, owner) in [
        (
            ACME,
            ID_ALICE_A,
            "Ada Lovelace",
            Some("ada@lovelace.test"),
            "alice",
        ),
        (
            ACME,
            ID_ALICE_B,
            "Grace Hopper",
            Some("grace@hopper.test"),
            "alice",
        ),
        (ACME, ID_CAROL, "Carol Onwire", None, "carol"),
        (ACME, ID_ESCAPED, "A%B Format", None, "bob"),
        (ACME, ID_PLAIN, "AXB Format", None, "bob"),
        (GLOBEX, ID_GLOBEX, "Globex Lead", None, "another"),
    ] {
        sqlx::query(
            "INSERT INTO archava_crm.contacts \
             (tenant_id, id, full_name, email, organization, owner_id) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(tenant)
        .bind(Uuid::parse_str(id).expect("a test uuid"))
        .bind(full_name)
        .bind(email)
        .bind(None::<String>)
        .bind(owner)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the contact");
    }
}

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

/// Send a POST with (or without) a session's credentials, always carrying the
/// CSRF header unless `with_csrf` says otherwise.
async fn post(
    app: &Router,
    uri: &str,
    body: &str,
    session: Option<&Session>,
    with_csrf: bool,
) -> (StatusCode, serde_json::Value) {
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
    let outcome: serde_json::Value = serde_json::from_slice(&bytes).expect("the response is JSON");
    (status, outcome)
}

/// Send a GET with an optional session's bearer token (no CSRF — a read).
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

/// A valid `POST /crm/commands` body: the caller-owned tenant and key plus the
/// typed `followup.create` command, field names exactly as
/// `packages/crm/src/contracts.ts` writes them.
fn followup_body(
    tenant: &str,
    request_id: &str,
    contact_id: &str,
    contact_revision: i64,
    summary: &str,
) -> String {
    serde_json::json!({
        "tenant": tenant,
        "requestId": request_id,
        "command": {
            "operation": "followup.create",
            "contactId": contact_id,
            "contactRevision": contact_revision,
            "summary": summary,
            "dueAt": "2027-03-04T10:15:00Z",
        },
    })
    .to_string()
}

async fn tenant(name: &str) -> TenantId {
    TenantId::new(name).expect("a test tenant slug")
}

/// The suite's one core-driven submission — the exact function the HTTP route
/// calls with a session, here driven with the suite's chosen actor.
async fn core_submit(
    fixture: &Fixture,
    actor: CrmActor,
    request_id: &str,
    contact_id: &str,
    contact_revision: i64,
    summary: &str,
) -> Result<(StatusCode, serde_json::Value), archava_host::AuthorityError> {
    let app_state = state(fixture).await;
    let submission = CrmSubmission {
        tenant: tenant("acme-retail").await,
        request_id: Uuid::parse_str(request_id).expect("a test uuid"),
        command: CrmCommand::FollowupCreate {
            contact_id: Uuid::parse_str(contact_id).expect("a test uuid"),
            contact_revision,
            summary: summary.to_string(),
            due_at: "2027-03-04T10:15:00Z".to_string(),
        },
    };
    let (status, body) =
        archava_host::erp::crm::submit_followup(&app_state, actor, submission).await?;
    let body = match body {
        archava_host::erp::crm::CrmBody::Outcome(value) => {
            serde_json::to_value(value).expect("the command outcome serializes")
        }
        _ => panic!("submit_followup answers with the command outcome"),
    };
    Ok((status, body))
}

// --- The contact list ---------------------------------------------------------

/// The named contacts of [`seed`], by id, for later assertions.
fn contacts_page(items: &serde_json::Value) -> Vec<(String, String, String, i64)> {
    items["items"]
        .as_array()
        .expect("the page carries items")
        .iter()
        .map(|item| {
            (
                item["id"].as_str().expect("an id").to_string(),
                item["fullName"].as_str().expect("a name").to_string(),
                item["ownerId"].as_str().expect("an owner").to_string(),
                item["revision"].as_i64().expect("a revision"),
            )
        })
        .collect()
}

/// **Witness: the contact list is a real read.**
///
/// `alice` holds `contacts.read` at scope `all`, so her page carries every
/// contact of the tenant with the REAL database values — revisions, owners,
/// names — not a canned list.
#[tokio::test]
async fn the_contact_list_reads_real_rows_for_a_granted_reader() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "alice").await;

    let (status, page) = get(&app, &format!("/contacts?tenant={ACME}"), Some(&session)).await;

    assert_eq!(status, StatusCode::OK, "a granted reader gets the list");
    let rows = contacts_page(&page);
    assert_eq!(rows.len(), 5, "every tenant contact is on the page");
    let named = rows
        .iter()
        .find(|(id, _, _, _)| id == ID_ALICE_A)
        .expect("the seeded contact is on the list");
    assert_eq!(named.1, "Ada Lovelace");
    assert_eq!(named.2, "alice");
    assert_eq!(named.3, 1, "the real revision, not a placeholder");
    assert_eq!(page["next"], serde_json::Value::Null);
}

/// **Witness: `assigned` scope sees only its own contacts.**
///
/// `carol` owns one contact; her page carries exactly that one, and a second
/// page cursor never appears — the scope filter is the same predicate the
/// write containment uses, so the list cannot drift from the command gate.
#[tokio::test]
async fn an_assigned_scope_sees_only_its_own_contacts() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "carol").await;

    let (status, page) = get(&app, &format!("/contacts?tenant={ACME}"), Some(&session)).await;

    assert_eq!(status, StatusCode::OK);
    let rows = contacts_page(&page);
    assert_eq!(rows.len(), 1, "only the owned contact is visible");
    assert_eq!(rows[0].0, ID_CAROL);
    assert_eq!(rows[0].2, "carol");
}

/// **Witness: a caller without `contacts.read` gets the grant refusal.**
///
/// `dave` holds only `followups.write`: the list is refused by name
/// (`role_not_allowed`), not answered with an empty page — an empty page
/// would read as "no contacts", which is a different fact than "no grant".
#[tokio::test]
async fn a_reader_without_a_contact_read_grant_is_refused() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "dave").await;

    let (status, outcome) = get(&app, &format!("/contacts?tenant={ACME}"), Some(&session)).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(outcome["reason"], "role_not_allowed");
}

/// **Witness: no session, no list.**
#[tokio::test]
async fn an_unauthenticated_contact_read_is_refused() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);

    let (status, outcome) = get(&app, &format!("/contacts?tenant={ACME}"), None).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(outcome["decision"], "unauthenticated");
}

/// **Witness: a principal with no CRM membership is refused, list and
/// command alike.**
///
/// The forged-tenant property carries over from the commerce side: a tenant
/// from the body the identity holds no CRM grant in is the SAME answer as a
/// tenant that does not exist — `missing_membership` on both routes.
#[tokio::test]
async fn a_principal_without_a_crm_membership_is_refused() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "another").await;

    let (status, outcome) = get(&app, &format!("/contacts?tenant={ACME}"), Some(&session)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(outcome["reason"], "missing_membership");

    let (status, outcome) = post(
        &app,
        "/crm/commands",
        &followup_body(ACME, KEY_NO_MEMBER, ID_ALICE_A, 1, "No membership here"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(outcome["reason"], "missing_membership");
}

/// **Witness: the search string stays a literal.**
///
/// The seeded rows `A%B Format` and `AXB Format` differ by one character that
/// is a wildcard only to a LIKE reader. The baseline escapes the caller's
/// `%`/`_`/`\` (`ESCAPE '\'`), so searching for `A%B` must find exactly the
/// `A%B` row — an unescaped pattern would match both.
#[tokio::test]
async fn a_search_is_escaped_so_the_caller_percent_stays_a_literal() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "alice").await;

    let (status, page) = get(
        &app,
        &format!("/contacts?tenant={ACME}&search=A%25B"),
        Some(&session),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let rows = contacts_page(&page);
    assert_eq!(rows.len(), 1, "only the literal A%B row matches");
    assert_eq!(rows[0].0, ID_ESCAPED);
}

/// **Witness: a limit outside the page bounds refuses as a submission.**
///
/// TS `pageSchema` caps the limit at 100; exceeding it is a caller mistake, a
/// 400, and never a truncated read somebody mistook for a full one.
#[tokio::test]
async fn a_limit_outside_the_page_bounds_is_refused() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "alice").await;

    let (status, outcome) = get(
        &app,
        &format!("/contacts?tenant={ACME}&limit=101"),
        Some(&session),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(outcome["reason"], "invalid_submission");
}

// --- The reviewed command ------------------------------------------------------

/// **Witness: a reviewed command persists the real effect.**
///
/// The reviewed draft binds to a REAL contact at a REAL revision; the effect
/// is a follows-up row, the contact's revision bump, an audit row and the
/// recorded receipt — all inside the one transaction the key claims.
#[tokio::test]
async fn a_reviewed_followup_create_persists_the_real_effect() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "alice").await;

    let (status, outcome) = post(
        &app,
        "/crm/commands",
        &followup_body(ACME, KEY_FIRST, ID_ALICE_A, 1, "Repair the intake note"),
        Some(&session),
        true,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "a valid reviewed command records: {outcome}"
    );
    assert_eq!(outcome["decision"], "receipt");
    assert_eq!(outcome["receipt"]["kind"], "followup");
    assert_eq!(outcome["receipt"]["revision"], 1);

    // The business row: the followup exists, bound to the contact, open,
    // with the summary and due time as submitted.
    let followup: (String, String, String, String) = sqlx::query_as(
        "SELECT f.status, f.summary, c.full_name, a.operation \
         FROM archava_crm.followups f \
         JOIN archava_crm.contacts c ON c.id=f.contact_id AND c.tenant_id=f.tenant_id \
         JOIN archava_crm.audit a \
           ON a.resource_id=f.id AND a.tenant_id=f.tenant_id \
         WHERE f.tenant_id=$1 AND f.id=$2",
    )
    .bind(ACME)
    .bind(
        Uuid::parse_str(outcome["receipt"]["id"].as_str().expect("a receipt id"))
            .expect("a test uuid"),
    )
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the effect row exists");
    assert_eq!(followup.0, "open");
    assert_eq!(followup.1, "Repair the intake note");
    assert_eq!(followup.2, "Ada Lovelace");
    assert_eq!(followup.3, "followup.create");

    // The contact's revision moved: the stamp the NEXT submission must observe.
    let revision: i32 = sqlx::query_scalar(
        "SELECT revision FROM archava_crm.contacts WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(ID_ALICE_A).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the contact row");
    assert_eq!(revision, 2);

    // The receipt is recorded on the claim row, alongside the audit entries.
    let receipt: serde_json::Value = sqlx::query_scalar(
        "SELECT receipt FROM archava_crm.commands WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(KEY_FIRST).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the claim row");
    assert_eq!(receipt["kind"], "followup");
    assert_eq!(receipt["id"], outcome["receipt"]["id"]);
    assert_eq!(receipt["revision"], 1);

    // And the audit row's initiator for a human submission is genuinely
    // nobody — the distinction the service path exists to name.
    let initiator: Option<String> = sqlx::query_scalar(
        "SELECT initiator_id FROM archava_crm.audit \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(KEY_FIRST).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the audit row");
    assert_eq!(initiator, None);
}

/// **Witness: a stale revision conflicts and records nothing new.**
///
/// After the first command bumps the contact to revision 2, a draft written
/// against revision 1 is exactly the concurrent-edit danger the binding
/// exists for: 409 `revision_changed`, and the database carries the SAME one
/// followup row — the conflict left no trace.
#[tokio::test]
async fn a_stale_revision_conflicts_and_records_nothing_new() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "alice").await;

    let (status, _) = post(
        &app,
        "/crm/commands",
        &followup_body(ACME, KEY_FIRST, ID_ALICE_A, 1, "First draft"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, outcome) = post(
        &app,
        "/crm/commands",
        // A different key, a stale revision: the resource moved under it.
        &followup_body(ACME, KEY_SECOND, ID_ALICE_A, 1, "Stale draft"),
        Some(&session),
        true,
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(outcome["reason"], "revision_changed");
    assert_eq!(
        outcome["receipt"],
        serde_json::Value::Null,
        "a conflicted command carries no receipt"
    );

    let followups: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_crm.followups WHERE tenant_id=$1 AND contact_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(ID_ALICE_A).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the followups");
    assert_eq!(followups, 1, "the refused draft recorded nothing");
}

/// **Witness: an unknown contact is an unknown resource.**
#[tokio::test]
async fn an_unknown_contact_is_unknown_resource() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "alice").await;

    let (status, outcome) = post(
        &app,
        "/crm/commands",
        &followup_body(
            ACME,
            KEY_NO_MEMBER,
            "9f2c5a10-9999-4999-8999-00000000009f",
            1,
            "Nothing behind this id",
        ),
        Some(&session),
        true,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(outcome["reason"], "unknown_resource");
}

/// **Witness: a contact outside `assigned` scope is not disclosed.**
///
/// `carol` addresses `alice`'s contact by name: to an assigned caller a
/// contact she does not own and a contact that does not exist are ONE answer
/// (`unknown_resource`) — the refusal cannot be probed for neighbour rows.
#[tokio::test]
async fn an_out_of_scope_contact_is_not_disclosed_to_an_assigned_caller() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "carol").await;

    let (status, outcome) = post(
        &app,
        "/crm/commands",
        &followup_body(ACME, KEY_SECOND, ID_ALICE_A, 1, "Not mine"),
        Some(&session),
        true,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{outcome}");
    assert_eq!(outcome["reason"], "unknown_resource");

    let followups: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_crm.followups WHERE tenant_id=$1 AND contact_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(ID_ALICE_A).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the followups");
    assert_eq!(followups, 0, "the refused draft recorded nothing");
}

/// **Witness: a missing write grant is refused before any claim exists.**
///
/// `bob` holds reads but not `followups.write`; the refusal arrives BEFORE
/// the idempotency claim is taken, so the key remains unlisted — a refusal
/// that recorded a row would make the route a receipt oracle.
#[tokio::test]
async fn a_missing_write_grant_is_refused_before_any_claim() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "bob").await;

    let (status, outcome) = post(
        &app,
        "/crm/commands",
        &followup_body(ACME, KEY_NO_MEMBER, ID_ALICE_A, 1, "I only read"),
        Some(&session),
        true,
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(outcome["reason"], "role_not_allowed");

    let claims: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_crm.commands WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(KEY_NO_MEMBER).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the claims");
    assert_eq!(claims, 0, "a refused command must not have claimed a key");
}

/// **Witness: revocation is honoured on execute and on recovery alike.**
///
/// `revoked` holds the same grants as `alice` with `active=false`; a fresh
/// submission and the recovery read of a recorded receipt BOTH refuse — the
/// gate re-reads the row the database holds, with no cache to invalidate.
/// (The recorded receipt exists: the identical, active `alice` records it
/// first.)
#[tokio::test]
async fn a_revoked_membership_is_refused_on_execute_and_on_recovery() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "alice").await;

    let (status, _) = post(
        &app,
        "/crm/commands",
        &followup_body(ACME, KEY_FIRST, ID_ALICE_A, 1, "Recorded while active"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let revoked = login(&app, "revoked").await;
    let (status, outcome) = post(
        &app,
        "/crm/commands",
        &followup_body(ACME, KEY_SECOND, ID_ALICE_A, 2, "Not while revoked"),
        Some(&revoked),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(outcome["reason"], "missing_membership");

    let (status, outcome) = get(
        &app,
        &format!("/crm/commands/status?tenant={ACME}&requestId={KEY_FIRST}"),
        Some(&revoked),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(outcome["reason"], "missing_membership");
}

// --- Idempotency ---------------------------------------------------------------

/// **Witness: a replay answers the recorded receipt.**
///
/// The same key with byte-identical bytes reaches the replay branch and
/// returns the STORED receipt — a duplicate submission is answered, not
/// re-applied (one followup row, one revision bump total).
#[tokio::test]
async fn the_same_key_replays_the_same_receipt() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "alice").await;

    let body = followup_body(ACME, KEY_FIRST, ID_ALICE_A, 1, "Repair the intake note");
    let (first, outcome) = post(&app, "/crm/commands", &body, Some(&session), true).await;
    assert_eq!(first, StatusCode::OK, "{outcome}");
    let (second, replayed) = post(&app, "/crm/commands", &body, Some(&session), true).await;
    assert_eq!(second, StatusCode::OK);
    assert_eq!(replayed["decision"], "receipt");
    assert_eq!(
        replayed["receipt"]["id"], outcome["receipt"]["id"],
        "a replay returns the SAME receipt"
    );
    assert!(
        replayed["fingerprint"].is_string(),
        "the replay names the stored fingerprint so the caller can see the effect is its own command's"
    );

    let followups: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_crm.followups WHERE tenant_id=$1 AND contact_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(ID_ALICE_A).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the followups");
    assert_eq!(followups, 1, "the replay did not apply twice");
    let revision: i32 = sqlx::query_scalar(
        "SELECT revision FROM archava_crm.contacts WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(ID_ALICE_A).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the contact");
    assert_eq!(revision, 2, "one bump, not two");
}

/// **Witness: the same key with a different command is a conflict without a
/// receipt.**
///
/// The key is the idempotency contract: a DIFFERENT summary under the SAME
/// key must never be answered with the first submissions receipt — the caller
/// would conclude its own command took effect.
#[tokio::test]
async fn the_same_key_with_a_different_command_conflicts_without_a_receipt() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "alice").await;

    let (status, _) = post(
        &app,
        "/crm/commands",
        &followup_body(ACME, KEY_FIRST, ID_ALICE_A, 1, "Recorded summary"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, outcome) = post(
        &app,
        "/crm/commands",
        // The same key, a second, categorically different command.
        &followup_body(ACME, KEY_FIRST, ID_ALICE_A, 2, "Some other summary"),
        Some(&session),
        true,
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(outcome["reason"], "request_id_reused");
    assert_eq!(
        outcome["receipt"],
        serde_json::Value::Null,
        "the conflict must never carry the other command's receipt"
    );
}

/// **Witness: another principal's key is unknown — the cold shoulder, with
/// one followup row.**
///
/// `carol` claims her key on her contact; `alice` then submits a different
/// command under that same key. The answer is 404 `unknown_request` — the key
/// exists, but not for this actor, and its existence is not disclosed.
#[tokio::test]
async fn another_principals_key_is_unknown_with_no_leak() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let carol_session = login(&app, "carol").await;
    let alice_session = login(&app, "alice").await;

    let (status, _) = post(
        &app,
        "/crm/commands",
        &followup_body(ACME, KEY_FIRST, ID_CAROL, 1, "Carol drafts first"),
        Some(&carol_session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "carol's own command must record");

    let (status, outcome) = post(
        &app,
        "/crm/commands",
        &followup_body(ACME, KEY_FIRST, ID_ALICE_A, 1, "Alice collisions"),
        Some(&alice_session),
        true,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(outcome["reason"], "unknown_request");
    assert_eq!(
        outcome["receipt"],
        serde_json::Value::Null,
        "another actor's record must not be visible in this answer: {outcome}"
    );

    // And Carol's row is untouched: her key still holds HER submission.
    let followups: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_crm.followups WHERE tenant_id=$1 AND contact_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(ID_CAROL).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count carol's followups");
    assert_eq!(followups, 1);
}

/// **Witness: a racing same key yields ONE effect.**
///
/// Two submissions carrying the same key, different summaries, parked behind
/// the real commands write-lock and released together (the observed count
/// gated on 2 waiters). The recorded outcome: exactly one records, exactly
/// one conflicts, one row exists.
#[tokio::test]
async fn the_racing_same_key_yields_conflict_not_the_other_receipt() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "alice").await;

    let (locked, parked, release) = hold_commands_table(&fixture).await;

    // Same ordering discipline as the authority suite's copy: the submissions
    // spawn only once the holder confirms `LOCK TABLE` is held. The holder
    // pays a cold pool connect while the racers ride the app pool login
    // warmed — without this await the first racer can record before the lock
    // exists, and the waiters poll answers 0 forever (sixteen-thread
    // width made that race lose every run in the authority variant).
    locked
        .await
        .expect("the holder must confirm the CRM commands-table lock before the racers spawn");

    let one = {
        let app = app.clone();
        let token = session.token.clone();
        let csrf = session.csrf.clone();
        tokio::spawn(async move {
            let held = Session { token, csrf };
            post(
                &app,
                "/crm/commands",
                &followup_body(ACME, KEY_RACE, ID_ALICE_A, 1, "Racing draft one"),
                Some(&held),
                true,
            )
            .await
        })
    };
    let two = {
        let app = app.clone();
        let token = session.token.clone();
        let csrf = session.csrf.clone();
        tokio::spawn(async move {
            let held = Session { token, csrf };
            post(
                &app,
                "/crm/commands",
                &followup_body(ACME, KEY_RACE, ID_ALICE_A, 1, "Racing draft two"),
                Some(&held),
                true,
            )
            .await
        })
    };

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

    let one = one.await.expect("the first submission task must finish");
    let two = two.await.expect("the second submission task must finish");

    let statuses = [one.0, two.0];
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::OK)
            .count(),
        1,
        "exactly one of the racing submissions may record, got {statuses:?}"
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::CONFLICT)
            .count(),
        1,
        "the loser must be conflicted, got {statuses:?}"
    );
    for (status, outcome) in [one, two] {
        if status == StatusCode::CONFLICT {
            assert_eq!(
                outcome["receipt"],
                serde_json::Value::Null,
                "the conflicted racer never sees the winner's receipt: {outcome}"
            );
        }
    }

    let followups: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_crm.followups WHERE tenant_id=$1 AND contact_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(ID_ALICE_A).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the followups");
    assert_eq!(followups, 1, "one effect total: {followups}");
}

// --- Authorized recovery -------------------------------------------------------

/// **Witness: recovery through replay after a lost response, and through the
/// status route.**
///
/// The caller may never see a submission's answer — the connection drops, the
/// process is restarted, whatever. Recovery is the same key replayed (the
/// receipt returns) plus the status route (same receipt). DB: still exactly
/// one effect.
#[tokio::test]
async fn a_lost_response_recovers_through_replay_and_status() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "alice").await;

    let body = followup_body(ACME, KEY_FIRST, ID_ALICE_A, 1, "Lost-response draft");
    let (status, _) = post(&app, "/crm/commands", &body, Some(&session), true).await;
    assert_eq!(status, StatusCode::OK);

    // The replay answers the receipt the caller missed.
    let (status, replayed) = post(&app, "/crm/commands", &body, Some(&session), true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replayed["decision"], "receipt", "{replayed}");

    // The status route — the authorized recovery — resolves the same receipt.
    let (status, recovered) = get(
        &app,
        &format!("/crm/commands/status?tenant={ACME}&requestId={KEY_FIRST}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recovered["status"], "committed");
    assert_eq!(
        recovered["receipt"]["id"], replayed["receipt"]["id"],
        "the recovery shows the recorded receipt"
    );

    let followups: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_crm.followups WHERE tenant_id=$1 AND contact_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(ID_ALICE_A).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the followups");
    assert_eq!(followups, 1);
}

/// **Witness: an unknown key reads as `unconfirmed`, not as an error.**
///
/// "Absence is not proof of failure" (the contracts file states the rule): a
/// committed response the caller missed and a key that was never accepted
/// carry the SAME recovery answer, so a caller re-submitting the SAME command
/// is safe either way.
#[tokio::test]
async fn an_unknown_keys_status_is_unconfirmed_not_an_error() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "alice").await;

    let (status, outcome) = get(
        &app,
        &format!("/crm/commands/status?tenant={ACME}&requestId={KEY_NO_MEMBER}"),
        Some(&session),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(outcome["status"], "unconfirmed");
    assert_eq!(outcome["requestId"], KEY_NO_MEMBER);
    assert_eq!(outcome["receipt"], serde_json::Value::Null);
}

/// **Witness: a receipt is refused without the read grants.**
///
/// `dave` may WRITE followups but holds no read grants; his own recorded
/// receipt is refused on recovery — the receipts grant nothing, the CURRENT
/// membership row does. The submission path's write grant does not become a
/// recovery grant.
#[tokio::test]
async fn a_receipt_is_refused_without_the_read_grants() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "dave").await;

    let (status, _) = post(
        &app,
        "/crm/commands",
        &followup_body(ACME, KEY_FIRST, ID_ALICE_A, 1, "Recorded by dave"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "dave holds followups.write: {status}"
    );

    let (status, outcome) = get(
        &app,
        &format!("/crm/commands/status?tenant={ACME}&requestId={KEY_FIRST}"),
        Some(&session),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(outcome["reason"], "role_not_allowed");
}

/// **Witness: a receipt hides when its contact leaves the scope.**
///
/// Carol's own receipt exists; the administration then moves the owner of her
/// contact (a plain FK-safe update every operator can do). Carol's recovery
/// answer becomes `unknown_resource` — the CURRENT containment, not the
/// recorded one, gates the receipt.
#[tokio::test]
async fn a_receipt_hides_when_its_contact_leaves_the_scope() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "carol").await;

    let (status, _) = post(
        &app,
        "/crm/commands",
        &followup_body(ACME, KEY_FIRST, ID_CAROL, 1, "Carol's recorded draft"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = get(
        &app,
        &format!("/crm/commands/status?tenant={ACME}&requestId={KEY_FIRST}"),
        Some(&session),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "own receipt, own grants: recoverable"
    );

    // Fixture administration, not authority evidence: move the owner. The
    // contact still exists; Carol's assigned scope just no longer reaches it.
    let moved =
        sqlx::query("UPDATE archava_crm.contacts SET owner_id='bob' WHERE tenant_id=$1 AND id=$2")
            .bind(ACME)
            .bind(Uuid::parse_str(ID_CAROL).expect("a test uuid"))
            .execute(&fixture.admin_pool)
            .await
            .expect("move the owner");
    assert_eq!(moved.rows_affected(), 1);

    let (status, outcome) = get(
        &app,
        &format!("/crm/commands/status?tenant={ACME}&requestId={KEY_FIRST}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{outcome}");
    assert_eq!(outcome["reason"], "unknown_resource");
}

/// **Witness: a mutation without the CSRF header is refused before the
/// membership is read.**
/// The bound-confirmation property of the reviewed flow: the browser carries
/// the CSRF value; a cross-site page cannot, and a stolen session token alone
/// (no header, no cookie match) cannot write.
#[tokio::test]
async fn a_mutation_without_csrf_is_refused() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "alice").await;

    let (status, outcome) = post(
        &app,
        "/crm/commands",
        &followup_body(ACME, KEY_FIRST, ID_ALICE_A, 1, "No CSRF, no write"),
        Some(&session),
        false,
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(outcome["reason"], "csrf_required");

    let followups: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_crm.followups WHERE tenant_id=$1 AND contact_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(ID_ALICE_A).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the followups");
    assert_eq!(followups, 0);
}

/// **Witness: an operation the slice does not implement refuses at the
/// boundary.**
///
/// The baseline's CRM vocabulary has five operations; the slice serves ONE.
/// A body claiming `contact.create` reaches an explicit refusal — the host
/// never hard-fails, never pretends it recorded something it did not.
#[tokio::test]
async fn an_unimplemented_operation_is_refused_at_the_boundary() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "alice").await;

    let body = serde_json::json!({
        "tenant": ACME,
        "requestId": KEY_NO_MEMBER,
        "command": {
            "operation": "contact.delete",
            "id": "2c3e5a10-1111-4111-8111-0000000000a1",
        },
    })
    .to_string();

    let (status, outcome) = post(&app, "/crm/commands", &body, Some(&session), true).await;

    // An unrepresentable submission is answered at the boundary with 422
    // (axum's `Json`) rejection status for a payload the host cannot turn
    // into a command) and the fixed refusal, with the detail logged rather
    // than echoed.
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(outcome["reason"], "malformed_submission");

    let claims: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_crm.commands WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(KEY_NO_MEMBER).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the claims");
    assert_eq!(claims, 0);
}

// --- The service-on-behalf path -----------------------------------------------

/// **Witness: the service path records the human initiator when the grants
/// intersect.**
///
/// `svc` (contact read + write grants, scope all) acts for `alice` (the full
/// grant set): the effect records — and every recorded row carries BOTH
/// principals: `commands.initiator_id` and `audit.initiator_id` name alice.
/// This is the human-on-behalf property TS records in the same columns.
#[tokio::test]
async fn the_service_path_records_the_human_initiator_when_the_grants_intersect() {
    let fixture = provision().await;
    seed(&fixture).await;

    let (status, outcome) = core_submit(
        &fixture,
        CrmActor::Service {
            id: "svc".to_string(),
            on_behalf_of: "alice".to_string(),
        },
        KEY_FIRST,
        ID_ALICE_A,
        1,
        "Service on behalf of alice",
    )
    .await
    .expect("the submission resolves");

    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["decision"], "receipt");

    let initiator: Option<String> = sqlx::query_scalar(
        "SELECT initiator_id FROM archava_crm.commands \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(KEY_FIRST).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the claim row");
    assert_eq!(initiator.as_deref(), Some("alice"));

    let audit_initiator: Option<String> = sqlx::query_scalar(
        "SELECT initiator_id FROM archava_crm.audit \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(KEY_FIRST).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the audit row");
    assert_eq!(audit_initiator.as_deref(), Some("alice"));

    let revision: i32 = sqlx::query_scalar(
        "SELECT revision FROM archava_crm.contacts WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(ID_ALICE_A).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the contact");
    assert_eq!(revision, 2, "the effect really happened");
}

/// **Witness: the service path cannot grant a permission the human lacks.**
///
/// `svc-narrow` carries `followups.write`; `bob` does not. The EFFECTIVE
/// grant is the intersection — so the same service id that acts for alice is
/// refused acting for bob. The initiator's grant is not a rubber stamp.
#[tokio::test]
async fn the_service_path_refuses_when_the_human_grant_is_not_a_subset() {
    let fixture = provision().await;
    seed(&fixture).await;

    let (status, outcome) = core_submit(
        &fixture,
        CrmActor::Service {
            id: "svc-narrow".to_string(),
            on_behalf_of: "bob".to_string(),
        },
        KEY_SECOND,
        ID_ALICE_A,
        1,
        "Acting beyond bob's grants",
    )
    .await
    .expect("the submission resolves");

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(outcome["reason"], "role_not_allowed");

    let followups: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_crm.followups WHERE tenant_id=$1 AND contact_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(ID_ALICE_A).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the followups");
    assert_eq!(followups, 0, "the refused effect happened nowhere");
}

/// **Witness: the service scope never widens the human's scope.**
///
/// `svc` holds scope `all` but acts on behalf of `carol` (scope `assigned`);
/// the effective scope is `assigned` and the owner is carol, so `alice`'s
/// contact is invisible to the pair — the service cannot use the human's
/// membership while exporting its own scope line.
#[tokio::test]
async fn the_service_scope_never_widens_the_human_scope() {
    let fixture = provision().await;
    seed(&fixture).await;

    let (status, outcome) = core_submit(
        &fixture,
        CrmActor::Service {
            id: "svc".to_string(),
            on_behalf_of: "carol".to_string(),
        },
        KEY_NO_MEMBER,
        ID_ALICE_A,
        1,
        "Scope-breach attempt",
    )
    .await
    .expect("the submission resolves");

    assert_eq!(status, StatusCode::NOT_FOUND, "{outcome}");
    assert_eq!(outcome["reason"], "unknown_resource");
}

// --- Fixtures ------------------------------------------------------------------

/// Holds `LOCK TABLE archava_crm.commands IN EXCLUSIVE MODE` until a signal,
/// telling the caller how many requests are parked behind it — the core the
/// racing-same-key witness needs, on the CRM namespace.
///
/// The holder runs its OWN private admin pool on THIS test's runtime, so the
/// process keeps the one-runtime shape any real serving host has (no pool
/// socket is ever created or polled under a foreign driver).
/// Holds `LOCK TABLE archava_crm.commands IN EXCLUSIVE MODE` until a signal.
///
/// Returns `(locked, parked, release)`: `locked` resolves once the holder's
/// `LOCK TABLE` transaction has actually taken the lock — the guarantee the
/// racing test fans its submissions out against; `parked` resolves with the
/// number of ungranted lock requests on the locked table when both
/// submissions have arrived; `release` opens the gate. The parked count is
/// POLLED against the server's own `pg_locks` view, with a deadline, so a
/// submission that never reached the lock fails the test instead of hanging
/// it.
async fn hold_commands_table(
    fixture: &Fixture,
) -> (
    tokio::sync::oneshot::Receiver<()>,
    tokio::sync::oneshot::Receiver<usize>,
    tokio::sync::oneshot::Sender<()>,
) {
    let (gate_tx, gate_rx) = tokio::sync::oneshot::channel::<()>();
    let (report_tx, report_rx) = tokio::sync::oneshot::channel::<usize>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();

    let admin = fixture.runtime_url();
    let _holder = tokio::spawn(async move {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&admin)
            .await
            .expect("connect the lock holder");
        let mut transaction = pool.begin().await.expect("begin the lock");
        sqlx::query("LOCK TABLE archava_crm.commands IN EXCLUSIVE MODE")
            .execute(&mut *transaction)
            .await
            .expect("lock the CRM commands table");

        // Confirm the lock BEFORE the poll begins: the racing test fans its
        // submissions out against this signal, so what follows measures only
        // requests arriving behind a lock that was never absent.
        let _ = ready_tx.send(());

        // The deadline bounds a genuinely failed arrival, not the healthy
        // path: the authority suite's copy of this harness tripped on a
        // setup race (submissions spawned before the lock was taken, so the
        // poll observed zero forever under sixteen-thread load), and the
        // ordering fix lives in the caller. Patience stays generous for the
        // loaded-gate shape.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        let waiting = loop {
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_locks WHERE NOT granted \
                 AND relation = 'archava_crm.commands'::regclass",
            )
            .fetch_one(&pool)
            .await
            .expect("count the parked lock requests");
            if count >= 2 || std::time::Instant::now() >= deadline {
                break count as usize;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        };

        let _ = report_tx.send(waiting);
        let _ = gate_rx.await;

        transaction.commit().await.expect("release the lock");
    });

    (ready_rx, report_rx, gate_tx)
}

// --- keys ---------------------------------------------------------------------

// Fixed uuids the assertions name. The `KEY_NO_MEMBER` key doubles as the
// unknown-resource / never-recorded key where the test names it as such.
const KEY_FIRST: &str = "5a1b2c3d-0001-4001-8001-000000000001";
const KEY_SECOND: &str = "5a1b2c3d-0002-4002-8002-000000000002";
const KEY_RACE: &str = "5a1b2c3d-0003-4003-8003-000000000003";
const KEY_NO_MEMBER: &str = "5a1b2c3d-0004-4004-8004-000000000004";

// --- The browser session contract ---------------------------------------------

/// **Witness: a successful login records the session in an HttpOnly cookie.**
///
/// The browser flow carries its session in the cookie (a stolen page cannot
/// read an HttpOnly value, which is the point); the CSRF value arrives in the
/// body for the script to echo as the mutation header. A REFUSED login sets
/// nothing.
#[tokio::test]
async fn the_login_route_records_the_session_in_an_httponly_cookie() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);

    let body = serde_json::json!({
        "username": "alice@archava.test",
        "password": PASSWORD,
    })
    .to_string();
    // The login competes for the production KDF budget (RN04/RN07), like
    // every other login in this suite: without a slot this test's direct
    // oneshot received the honest 503 `busy` whenever parallel tests'
    // derivations were already in flight — a flake the budget never caused.
    let _slot = LOGIN_SLOTS.acquire().await.expect("login slots are open");
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
    assert_eq!(response.status(), StatusCode::OK);

    let cookie = response
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("a successful login sets the session cookie")
        .to_string();
    assert!(cookie.starts_with("archava_session="), "got {cookie}");
    assert!(
        cookie.contains("HttpOnly"),
        "the token is script-invisible: {cookie}"
    );
    assert!(cookie.contains("SameSite=Strict"), "{cookie}");
    assert!(cookie.contains("Path=/"), "{cookie}");
}

// --- The seven writes (C01) ----------------------------------------------------
//
// `packages/crm/src/contracts.ts` and `pipeline-contracts.ts` define seven
// operations. Each witness below drives the REAL submission path and then asks
// PostgreSQL what happened: the effect row, its revision, the audit row bound
// to the same request id, and the containment refusal for a caller who cannot
// see the resource. A receipt alone is never the proof.

/// A generic core-driven submission for any typed command — the same function
/// the HTTP route calls, with the suite's chosen actor.
async fn core_send(
    fixture: &Fixture,
    actor: CrmActor,
    request_id: &str,
    command: CrmCommand,
) -> Result<(StatusCode, serde_json::Value), archava_host::AuthorityError> {
    let app_state = state(fixture).await;
    let submission = CrmSubmission {
        tenant: tenant(ACME).await,
        request_id: Uuid::parse_str(request_id).expect("a test uuid"),
        command,
    };
    let (status, body) =
        archava_host::erp::crm::submit_command(&app_state, actor, submission).await?;
    let body = match body {
        archava_host::erp::crm::CrmBody::Outcome(value) => {
            serde_json::to_value(value).expect("the command outcome serializes")
        }
        _ => panic!("submit_command answers with the command outcome"),
    };
    Ok((status, body))
}

fn human(id: &str) -> CrmActor {
    CrmActor::Human(id.to_string())
}

/// The audit row a command left behind: `(operation, resource id, revision)`.
async fn audit_of(fixture: &Fixture, request_id: &str) -> (String, Uuid, i32) {
    sqlx::query_as(
        "SELECT operation, resource_id, revision FROM archava_crm.audit \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(request_id).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the command left exactly one audit row")
}

/// A contact's revision and owner, straight from the table.
async fn contact_facts(fixture: &Fixture, id: &str) -> (i32, String) {
    sqlx::query_as(
        "SELECT revision, owner_id FROM archava_crm.contacts WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(id).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the contact exists")
}

/// **Witness: `contact.create` decodes the typed wire contract over HTTP,
/// trims it, stamps the caller as owner, and leaves the audit row.**
#[tokio::test]
async fn a_contact_create_over_http_persists_the_row_and_its_audit() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "alice").await;
    let request_id = "00000000-0000-4000-8000-000000000101";

    let body = serde_json::json!({
        "tenant": ACME,
        "requestId": request_id,
        "command": {
            "operation": "contact.create",
            "data": {
                "fullName": "  Ada Byron  ",
                "email": "ada@byron.test",
                "organization": "  Engines  ",
            },
        },
    })
    .to_string();
    let (status, outcome) = post(&app, "/crm/commands", &body, Some(&session), true).await;

    assert_eq!(
        status,
        StatusCode::OK,
        "a granted writer creates: {outcome}"
    );
    assert_eq!(outcome["decision"], "receipt");
    assert_eq!(outcome["receipt"]["kind"], "contact");
    assert_eq!(outcome["receipt"]["revision"], 1);
    let id = outcome["receipt"]["id"].as_str().expect("a receipt id");

    let row: (String, Option<String>, Option<String>, String, i32) = sqlx::query_as(
        "SELECT full_name, email, organization, owner_id, revision \
         FROM archava_crm.contacts WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(id).expect("a receipt uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the contact row exists");
    assert_eq!(row.0, "Ada Byron", "the contract trims before it stores");
    assert_eq!(row.1.as_deref(), Some("ada@byron.test"));
    assert_eq!(row.2.as_deref(), Some("Engines"));
    assert_eq!(row.3, "alice", "the caller owns what it creates");
    assert_eq!(row.4, 1);

    let (operation, resource_id, revision) = audit_of(&fixture, request_id).await;
    assert_eq!(operation, "contact.create");
    assert_eq!(resource_id.to_string(), id);
    assert_eq!(revision, 1);
}

/// **Witness: `contact.update` is partial, revision-bound, and null clears a
/// nullable field.**
#[tokio::test]
async fn a_contact_update_is_partial_and_null_clears_the_field() {
    let fixture = provision().await;
    seed(&fixture).await;

    // Only the name moves: email and organization stay as they were.
    let (status, outcome) = core_send(
        &fixture,
        human("alice"),
        "00000000-0000-4000-8000-000000000102",
        CrmCommand::ContactUpdate {
            id: Uuid::parse_str(ID_ALICE_A).expect("a test uuid"),
            expected_revision: 1,
            data: ContactPatch {
                full_name: Some("Ada King".to_string()),
                email: None,
                organization: None,
            },
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["receipt"]["revision"], 2);

    let row: (String, Option<String>) = sqlx::query_as(
        "SELECT full_name, email FROM archava_crm.contacts WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(ID_ALICE_A).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the contact exists");
    assert_eq!(row.0, "Ada King");
    assert_eq!(row.1.as_deref(), Some("ada@lovelace.test"), "untouched");

    // `email: null` clears it; the organization stays untouched.
    let (status, outcome) = core_send(
        &fixture,
        human("alice"),
        "00000000-0000-4000-8000-000000000103",
        CrmCommand::ContactUpdate {
            id: Uuid::parse_str(ID_ALICE_A).expect("a test uuid"),
            expected_revision: 2,
            data: ContactPatch {
                full_name: None,
                email: Some(None),
                organization: None,
            },
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let email: Option<String> =
        sqlx::query_scalar("SELECT email FROM archava_crm.contacts WHERE tenant_id=$1 AND id=$2")
            .bind(ACME)
            .bind(Uuid::parse_str(ID_ALICE_A).expect("a test uuid"))
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("the contact exists");
    assert_eq!(email, None, "a present null clears the field");

    // A moved revision is one conflict, and the contact did not change.
    let (status, outcome) = core_send(
        &fixture,
        human("alice"),
        "00000000-0000-4000-8000-000000000104",
        CrmCommand::ContactUpdate {
            id: Uuid::parse_str(ID_ALICE_A).expect("a test uuid"),
            expected_revision: 1,
            data: ContactPatch {
                full_name: Some("Stale".to_string()),
                email: None,
                organization: None,
            },
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(outcome["reason"], "revision_changed");
    let name: String = sqlx::query_scalar(
        "SELECT full_name FROM archava_crm.contacts WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(ID_ALICE_A).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the contact exists");
    assert_eq!(name, "Ada King", "the stale attempt changed nothing");
}

/// **Witness: `activity.add` writes the activity, bumps the contact and
/// records both facts.**
#[tokio::test]
async fn an_activity_add_persists_the_activity_and_bumps_the_contact() {
    let fixture = provision().await;
    seed(&fixture).await;
    let request_id = "00000000-0000-4000-8000-000000000105";

    let (status, outcome) = core_send(
        &fixture,
        human("alice"),
        request_id,
        CrmCommand::ActivityAdd {
            contact_id: Uuid::parse_str(ID_ALICE_A).expect("a test uuid"),
            contact_revision: 1,
            kind: ActivityKind::Call,
            summary: "  Discussed the renewal  ".to_string(),
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["receipt"]["kind"], "activity");
    assert_eq!(outcome["receipt"]["revision"], 1);
    let id = outcome["receipt"]["id"].as_str().expect("a receipt id");

    let row: (String, String, String) = sqlx::query_as(
        "SELECT contact_id::text, kind, summary FROM archava_crm.activities \
         WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(id).expect("a receipt uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the activity row exists");
    assert_eq!(row.0, ID_ALICE_A);
    assert_eq!(row.1, "call");
    assert_eq!(row.2, "Discussed the renewal");

    let (revision, _) = contact_facts(&fixture, ID_ALICE_A).await;
    assert_eq!(revision, 2, "the contact moved with the activity");

    let (operation, resource_id, revision) = audit_of(&fixture, request_id).await;
    assert_eq!(operation, "activity.add");
    assert_eq!(resource_id.to_string(), id);
    assert_eq!(revision, 1);
}

/// **Witness: `followup.complete` closes an open follow-up exactly once — a
/// second completion and a stale bind are both one conflict.**
#[tokio::test]
async fn a_followup_complete_closes_the_open_followup_exactly_once() {
    let fixture = provision().await;
    seed(&fixture).await;

    let (status, created) = core_send(
        &fixture,
        human("alice"),
        "00000000-0000-4000-8000-000000000106",
        CrmCommand::FollowupCreate {
            contact_id: Uuid::parse_str(ID_ALICE_A).expect("a test uuid"),
            contact_revision: 1,
            summary: "Close me".to_string(),
            due_at: "2027-05-01T09:00:00Z".to_string(),
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::OK, "{created}");
    let followup_id = created["receipt"]["id"]
        .as_str()
        .expect("a receipt id")
        .to_string();
    let contact_after_create = contact_facts(&fixture, ID_ALICE_A).await.0;

    let (status, completed) = core_send(
        &fixture,
        human("alice"),
        "00000000-0000-4000-8000-000000000107",
        CrmCommand::FollowupComplete {
            id: Uuid::parse_str(&followup_id).expect("a receipt uuid"),
            expected_revision: 1,
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::OK, "{completed}");
    assert_eq!(completed["receipt"]["revision"], 2);

    let row: (String, i32) = sqlx::query_as(
        "SELECT status, revision FROM archava_crm.followups WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(&followup_id).expect("a receipt uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the followup exists");
    assert_eq!(row.0, "completed");
    assert_eq!(row.1, 2);
    let contact_after_complete = contact_facts(&fixture, ID_ALICE_A).await.0;
    assert_eq!(
        contact_after_complete, contact_after_create,
        "completing a follow-up does not bump the contact"
    );

    // The same completion again — with the revision it now holds — is still a
    // conflict: a completed follow-up is not open work.
    let (status, outcome) = core_send(
        &fixture,
        human("alice"),
        "00000000-0000-4000-8000-000000000108",
        CrmCommand::FollowupComplete {
            id: Uuid::parse_str(&followup_id).expect("a receipt uuid"),
            expected_revision: 2,
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(outcome["reason"], "revision_changed");

    // And a stale bind is the same answer.
    let (status, outcome) = core_send(
        &fixture,
        human("alice"),
        "00000000-0000-4000-8000-000000000109",
        CrmCommand::FollowupComplete {
            id: Uuid::parse_str(&followup_id).expect("a receipt uuid"),
            expected_revision: 1,
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(outcome["reason"], "revision_changed");
}

/// **Witness: `opportunity.create` opens the pipeline row and its first
/// transition, owned by the contact's owner.**
#[tokio::test]
async fn an_opportunity_create_opens_the_row_and_its_first_transition() {
    let fixture = provision().await;
    seed(&fixture).await;
    let request_id = "00000000-0000-4000-8000-000000000110";

    let (status, outcome) = core_send(
        &fixture,
        human("alice"),
        request_id,
        CrmCommand::OpportunityCreate {
            contact_id: Uuid::parse_str(ID_ALICE_A).expect("a test uuid"),
            contact_revision: 1,
            title: "  Renewal 2027  ".to_string(),
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["receipt"]["kind"], "opportunity");
    assert_eq!(outcome["receipt"]["revision"], 1);
    let id = outcome["receipt"]["id"].as_str().expect("a receipt id");

    let row: (String, String, String, i32) = sqlx::query_as(
        "SELECT contact_id::text, owner_id, stage, revision \
         FROM archava_crm.opportunities WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(id).expect("a receipt uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the opportunity row exists");
    assert_eq!(row.0, ID_ALICE_A);
    assert_eq!(row.1, "alice", "the contact's owner holds the opportunity");
    assert_eq!(row.2, "new");
    assert_eq!(row.3, 1);

    let history: (Option<String>, String, i32) = sqlx::query_as(
        "SELECT from_stage, to_stage, revision FROM archava_crm.opportunity_transitions \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(request_id).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the first transition exists");
    assert_eq!(history.0, None, "a new opportunity has no previous stage");
    assert_eq!(history.1, "new");
    assert_eq!(history.2, 1);

    let (operation, resource_id, revision) = audit_of(&fixture, request_id).await;
    assert_eq!(operation, "opportunity.create");
    assert_eq!(resource_id.to_string(), id);
    assert_eq!(revision, 1);
}

/// **Witness: `opportunity.move` follows the state machine, records history,
/// refuses illegal jumps, and requires a loss reason.**
#[tokio::test]
async fn an_opportunity_move_follows_the_state_machine_and_records_history() {
    let fixture = provision().await;
    seed(&fixture).await;

    let (status, created) = core_send(
        &fixture,
        human("alice"),
        "00000000-0000-4000-8000-000000000111",
        CrmCommand::OpportunityCreate {
            contact_id: Uuid::parse_str(ID_ALICE_A).expect("a test uuid"),
            contact_revision: 1,
            title: "Move me".to_string(),
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["receipt"]["id"]
        .as_str()
        .expect("a receipt id")
        .to_string();

    // new → qualified: allowed, history records the edge.
    let (status, moved) = core_send(
        &fixture,
        human("alice"),
        "00000000-0000-4000-8000-000000000112",
        CrmCommand::OpportunityMove {
            id: Uuid::parse_str(&id).expect("a receipt uuid"),
            expected_revision: 1,
            stage: PipelineStage::Qualified,
            reason: None,
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["receipt"]["revision"], 2);
    let history: (String, String, i32) = sqlx::query_as(
        "SELECT from_stage, to_stage, revision FROM archava_crm.opportunity_transitions \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str("00000000-0000-4000-8000-000000000112").expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the transition exists");
    assert_eq!(history.0, "new");
    assert_eq!(history.1, "qualified");
    assert_eq!(history.2, 2);

    // qualified → won is NOT a legal edge (proposal → won is).
    let (status, outcome) = core_send(
        &fixture,
        human("alice"),
        "00000000-0000-4000-8000-000000000113",
        CrmCommand::OpportunityMove {
            id: Uuid::parse_str(&id).expect("a receipt uuid"),
            expected_revision: 2,
            stage: PipelineStage::Won,
            reason: None,
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(outcome["reason"], "transition_not_allowed");
    let stage: String = sqlx::query_scalar(
        "SELECT stage FROM archava_crm.opportunities WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(&id).expect("a receipt uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the opportunity exists");
    assert_eq!(stage, "qualified", "the refused move changed nothing");

    // A loss without a reason is a submission the contract refuses.
    let (status, outcome) = core_send(
        &fixture,
        human("alice"),
        "00000000-0000-4000-8000-000000000114",
        CrmCommand::OpportunityMove {
            id: Uuid::parse_str(&id).expect("a receipt uuid"),
            expected_revision: 2,
            stage: PipelineStage::Lost,
            reason: None,
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(outcome["reason"], "invalid_submission");

    // qualified → lost with a reason: the stage and the loss reason land.
    let (status, lost) = core_send(
        &fixture,
        human("alice"),
        "00000000-0000-4000-8000-000000000115",
        CrmCommand::OpportunityMove {
            id: Uuid::parse_str(&id).expect("a receipt uuid"),
            expected_revision: 2,
            stage: PipelineStage::Lost,
            reason: Some("  Budget cut  ".to_string()),
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::OK, "{lost}");
    assert_eq!(lost["receipt"]["revision"], 3);
    let row: (String, Option<String>) = sqlx::query_as(
        "SELECT stage, loss_reason FROM archava_crm.opportunities WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(&id).expect("a receipt uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the opportunity exists");
    assert_eq!(row.0, "lost");
    assert_eq!(row.1.as_deref(), Some("Budget cut"));
}

/// **Witness: replay returns the SAME receipt, and a different command under
/// the same key is a conflict — across the new operations too.**
#[tokio::test]
async fn a_new_operation_replays_its_receipt_and_rejects_key_reuse() {
    let fixture = provision().await;
    seed(&fixture).await;
    let key = "00000000-0000-4000-8000-000000000116";

    let command = CrmCommand::ContactCreate {
        data: ContactFields {
            full_name: "Replay Target".to_string(),
            email: None,
            organization: None,
        },
    };
    let (status, first) = core_send(&fixture, human("alice"), key, command.clone())
        .await
        .expect("the store answers");
    assert_eq!(status, StatusCode::OK, "{first}");
    let receipt = first["receipt"].clone();

    let (status, replayed) = core_send(&fixture, human("alice"), key, command)
        .await
        .expect("the store answers");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        replayed["receipt"], receipt,
        "the same key, the same receipt"
    );
    assert_eq!(replayed["decision"], "receipt");

    let (status, reused) = core_send(
        &fixture,
        human("alice"),
        key,
        CrmCommand::ContactCreate {
            data: ContactFields {
                full_name: "A Different Command".to_string(),
                email: None,
                organization: None,
            },
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(reused["reason"], "request_id_reused");
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_crm.contacts WHERE tenant_id=$1 AND full_name='A Different Command'",
    )
    .bind(ACME)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the contacts");
    assert_eq!(rows, 0, "the reused key wrote nothing");
}

/// **Witness: the write grants gate each operation family, and an assigned
/// caller cannot reach a contact it does not own.**
#[tokio::test]
async fn the_new_writes_are_gated_by_grants_and_scope() {
    let fixture = provision().await;
    seed(&fixture).await;

    // bob reads contacts and follow-ups but holds no write grant.
    let (status, outcome) = core_send(
        &fixture,
        human("bob"),
        "00000000-0000-4000-8000-000000000117",
        CrmCommand::ContactUpdate {
            id: Uuid::parse_str(ID_ALICE_A).expect("a test uuid"),
            expected_revision: 1,
            data: ContactPatch {
                full_name: Some("Not Yours".to_string()),
                email: None,
                organization: None,
            },
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(outcome["reason"], "role_not_allowed");

    // carol holds every write grant but only for her own contacts.
    let (status, outcome) = core_send(
        &fixture,
        human("carol"),
        "00000000-0000-4000-8000-000000000118",
        CrmCommand::ContactUpdate {
            id: Uuid::parse_str(ID_ALICE_A).expect("a test uuid"),
            expected_revision: 1,
            data: ContactPatch {
                full_name: Some("Out Of Scope".to_string()),
                email: None,
                organization: None,
            },
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(outcome["reason"], "unknown_resource");

    // The same containment holds for a pipeline create bound to a foreign
    // contact, and for an activity on it.
    let (status, outcome) = core_send(
        &fixture,
        human("carol"),
        "00000000-0000-4000-8000-000000000119",
        CrmCommand::OpportunityCreate {
            contact_id: Uuid::parse_str(ID_ALICE_A).expect("a test uuid"),
            contact_revision: 1,
            title: "Not Yours".to_string(),
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(outcome["reason"], "unknown_resource");

    let (status, outcome) = core_send(
        &fixture,
        human("carol"),
        "00000000-0000-4000-8000-00000000011a",
        CrmCommand::ActivityAdd {
            contact_id: Uuid::parse_str(ID_ALICE_A).expect("a test uuid"),
            contact_revision: 1,
            kind: ActivityKind::Note,
            summary: "Not Yours".to_string(),
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(outcome["reason"], "unknown_resource");

    // carol CAN create a contact: it is stamped with her as owner.
    let (status, created) = core_send(
        &fixture,
        human("carol"),
        "00000000-0000-4000-8000-00000000011b",
        CrmCommand::ContactCreate {
            data: ContactFields {
                full_name: "Carol's New Lead".to_string(),
                email: None,
                organization: None,
            },
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["receipt"]["id"].as_str().expect("a receipt id");
    let (_, owner) = contact_facts(&fixture, id).await;
    assert_eq!(owner, "carol");
}

/// **Witness: the assistant's prepared completion is EXECUTABLE — a service
/// principal on behalf of the human closes the follow-up through the same
/// command lane, and the audit row records both ids.**
///
/// The conversation flow prepares a completion; the audited gap was that no
/// execution existed behind that preparation. The typed command lane is that
/// execution: `followup.complete` under the actor×initiator intersection.
#[tokio::test]
async fn a_service_completion_executes_the_prepared_followup_and_records_the_initiator() {
    let fixture = provision().await;
    seed(&fixture).await;

    let (status, created) = core_send(
        &fixture,
        human("alice"),
        "00000000-0000-4000-8000-00000000011c",
        CrmCommand::FollowupCreate {
            contact_id: Uuid::parse_str(ID_ALICE_A).expect("a test uuid"),
            contact_revision: 1,
            summary: "Prepared, then confirmed".to_string(),
            due_at: "2027-06-01T09:00:00Z".to_string(),
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::OK, "{created}");
    let followup_id = created["receipt"]["id"]
        .as_str()
        .expect("a receipt id")
        .to_string();
    let request_id = "00000000-0000-4000-8000-00000000011d";

    let (status, completed) = core_send(
        &fixture,
        CrmActor::Service {
            id: "svc".to_string(),
            on_behalf_of: "alice".to_string(),
        },
        request_id,
        CrmCommand::FollowupComplete {
            id: Uuid::parse_str(&followup_id).expect("a receipt uuid"),
            expected_revision: 1,
        },
    )
    .await
    .expect("the store answers");
    assert_eq!(status, StatusCode::OK, "{completed}");
    assert_eq!(completed["receipt"]["revision"], 2);

    let row: (String, i32) = sqlx::query_as(
        "SELECT status, revision FROM archava_crm.followups WHERE tenant_id=$1 AND id=$2",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(&followup_id).expect("a receipt uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the followup exists");
    assert_eq!(row.0, "completed");
    assert_eq!(row.1, 2);

    let ids: (String, Option<String>) = sqlx::query_as(
        "SELECT actor_id, initiator_id FROM archava_crm.audit \
         WHERE tenant_id=$1 AND request_id=$2 AND operation='followup.complete'",
    )
    .bind(ACME)
    .bind(Uuid::parse_str(request_id).expect("a test uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the completion left an audit row");
    assert_eq!(ids.0, "svc");
    assert_eq!(ids.1.as_deref(), Some("alice"));
}
