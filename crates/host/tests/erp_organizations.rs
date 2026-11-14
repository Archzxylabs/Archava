//! The R04 organizations cohort's host witnesses — the stock pool's THIRD
//! writer, driven through the real router (`tower::ServiceExt::oneshot`)
//! against the disposable PostgreSQL whose `archava_organizations` schema is
//! the real migration ledger (`001_organizations_affiliations`). Every
//! assertion is a fact about what the SERVER did:
//!
//! - the authorized flow (create → affiliation → update →
//!   update_affiliation) ends with exactly the rows the repository's SQL
//!   spec freeze — keep-old for absent-and-null detail cells, replace for
//!   non-null, the org revision counting every affiliation write — and the
//!   status read answers `committed` with the stored receipt;
//! - an archived organization refuses further updates and records nothing
//!   (the whole submission rolls back, claim included);
//! - a duplicate (contact, role) affiliation and a second primary active
//!   both refuse without holding anything;
//! - a revoked organization grant refuses the recovery — yesterday's
//!   receipts are not replayable through revoked access;
//! - the same idempotency key with a DIFFERENT command conflicts without
//!   recording;
//! - a cross-tenant caller is refused before any business statement runs
//!   while the same caller acts freely in its own ledger;
//! - a lost response re-submits the SAME key + bytes and answers `replayed`
//!   with no second organization;
//! - `remove_affiliation` on an ARCHIVED organization still runs (the
//!   record-only arms skip the archived gate) and inactivates the row;
//! - an actor whose organization grants are missing is refused by the grant
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
/// every ERP suite pins; the organizations surface RIDE S the same serving
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
/// and the purchasing ledger (`try_new_erp`'s second pass) — the
/// organizations surface rides the same serving state; its own ledger rows
/// exist in the fixture regardless.
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

/// The organizations permission vocabulary (migration 001's CHECK).
const ORG_ALL: &[&str] = &[
    "organizations.read",
    "organizations.write",
    "affiliations.read",
    "affiliations.write",
    "audit.read",
];

/// `ox` is the registry manager: full grants, scope all — the positive-path
/// actor.
/// `onone` holds only `audit.read` — every grant gate refuses.
/// `ooff` is `ox` with the organization membership seeded ACTIVE and revoked
/// by the test itself — the recovery must not outlive the grant.
/// `other` holds full grants but ONLY in GLOBEX — the cross-tenant refusal.
/// `ogx` is GLOBEX's own full-grant manager — the per-tenant
/// key-independence witness.
async fn seed(fixture: &Fixture) {
    for principal in ["ox", "onone", "ooff", "other", "ogx"] {
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

    // The organizations ledger's rows: the patent-one difference per actor.
    sqlx::query(
        "INSERT INTO archava_organizations.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind("ox")
    .bind(ORG_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed ox's organizations membership");
    sqlx::query(
        "INSERT INTO archava_organizations.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind("ooff")
    .bind(ORG_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed ooff's ACTIVE organizations membership");
    sqlx::query(
        "INSERT INTO archava_organizations.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind("onone")
    .bind(["audit.read"].to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed onone's organizations membership");
    for principal in ["other", "ogx"] {
        sqlx::query(
            "INSERT INTO archava_organizations.memberships \
             (tenant_id, principal_id, kind, permissions, scope, active) \
             VALUES ($1, $2, 'human', $3, 'all', true)",
        )
        .bind(GLOBEX)
        .bind(principal)
        .bind(ORG_ALL.to_vec())
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the GLOBEX organizations membership");
    }
    // Real CRM references for affiliation writes; arbitrary UUIDs no longer
    // count as a contact. These are synthetic opening records, not app data.
    for (tenant, actors) in [
        (ACME, vec!["ox", "onone", "ooff"]),
        (GLOBEX, vec!["other", "ogx"]),
    ] {
        for actor in &actors {
            sqlx::query("INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,$2,'human',ARRAY['contacts.read'],'all')")
                .bind(tenant).bind(actor).execute(&fixture.admin_pool).await.unwrap();
        }
        for id in [CONTACT_A, CONTACT_B, CONTACT_C] {
            sqlx::query("INSERT INTO archava_crm.contacts(tenant_id,id,full_name,owner_id) VALUES($1,$2,'Synthetic organization contact',$3)")
                .bind(tenant).bind(uuid::Uuid::parse_str(id).unwrap()).bind(actors[0]).execute(&fixture.admin_pool).await.unwrap();
        }
    }
}

const CONTACT_A: &str = "c1aa0000-4444-4444-4444-0000000000c1";
const CONTACT_B: &str = "c1aa0000-4444-4444-4444-0000000000c2";
const CONTACT_C: &str = "c1aa0000-4444-4444-4444-0000000000c3";

/// Fixed idempotency keys — stable so readbacks and the recovery witness can
/// name rows exactly.
const KEY_ORG: &str = "af4cb000-3334-4333-8333-0000000000b1";
const KEY_AFF: &str = "af4cb000-3334-4333-8333-0000000000b2";
const KEY_UPD: &str = "af4cb000-3334-4333-8333-0000000000b3";
const KEY_UPD_AFF: &str = "af4cb000-3334-4333-8333-0000000000b4";
const KEY_PRIMARY: &str = "af4cb000-3334-4333-8333-0000000000b5";
const KEY_DUPLICATE: &str = "af4cb000-3334-4333-8333-0000000000b6";
const KEY_ARCH: &str = "af4cb000-3334-4333-8333-0000000000b7";
const KEY_REMOVE: &str = "af4cb000-3334-4333-8333-0000000000b8";
const KEY_XTENANT: &str = "af4cb000-3334-4333-8333-0000000000b9";
const KEY_REVOKED: &str = "af4cb000-3334-4333-8333-0000000000ba";
const KEY_HOLD_UPD: &str = "af4cb000-3334-4333-8333-0000000000bb";

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
    if status != StatusCode::OK {
        eprintln!("DBG-POST {status} {outcome}");
    }
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

fn create_organization_body(tenant: &str, request_id: &str, name: &str) -> String {
    json!({
        "tenant": tenant,
        "mutation": {
            "requestId": request_id,
            "type": "create_organization",
            "data": {
                "name": name,
                "legalName": format!("PT {name}"),
                "taxId": "01.234.567.8-999.000",
                "website": "https://acme.test",
                "industry": "retail",
                "billingAddress": "Jl. Merdeka 10, Jakarta"
            }
        }
    })
    .to_string()
}

fn update_organization_body(
    tenant: &str,
    request_id: &str,
    organization_id: &str,
    revision: i64,
    name: Option<&str>,
    website: Option<&str>,
) -> String {
    let mut data = json!({
        "id": organization_id,
        "revision": revision,
    });
    if let Some(name) = name {
        data["name"] = json!(name);
    }
    if let Some(website) = website {
        data["website"] = json!(website);
    }
    json!({
        "tenant": tenant,
        "mutation": {
            "requestId": request_id,
            "type": "update_organization",
            "data": data,
        }
    })
    .to_string()
}

fn archive_organization_body(
    tenant: &str,
    request_id: &str,
    organization_id: &str,
    revision: i64,
) -> String {
    json!({
        "tenant": tenant,
        "mutation": {
            "requestId": request_id,
            "type": "archive_organization",
            "data": {
                "id": organization_id,
                "revision": revision,
                "reason": "the acquisition closed the subsidiary",
            }
        }
    })
    .to_string()
}

fn create_affiliation_body(
    tenant: &str,
    request_id: &str,
    organization_id: &str,
    contact_id: &str,
    role: &str,
    title: Option<&str>,
    is_primary: bool,
) -> String {
    let mut data = json!({
        "organizationId": organization_id,
        "contactId": contact_id,
        "role": role,
        "isPrimary": is_primary,
    });
    if let Some(title) = title {
        data["title"] = json!(title);
    }
    json!({
        "tenant": tenant,
        "mutation": {
            "requestId": request_id,
            "type": "create_affiliation",
            "data": data,
        }
    })
    .to_string()
}

fn update_affiliation_body(
    tenant: &str,
    request_id: &str,
    organization_id: &str,
    affiliation_id: &str,
    fields: Value,
) -> String {
    let mut data = json!({
        "id": affiliation_id,
        "organizationId": organization_id,
    });
    let fields = fields.as_object().expect("structured fields");
    for (key, value) in fields {
        data[key.as_str()] = value.clone();
    }
    json!({
        "tenant": tenant,
        "mutation": {
            "requestId": request_id,
            "type": "update_affiliation",
            "data": data,
        }
    })
    .to_string()
}

fn remove_affiliation_body(
    tenant: &str,
    request_id: &str,
    organization_id: &str,
    affiliation_id: &str,
) -> String {
    json!({
        "tenant": tenant,
        "mutation": {
            "requestId": request_id,
            "type": "remove_affiliation",
            "data": {
                "id": affiliation_id,
                "organizationId": organization_id,
                "reason": "the contact left the organization",
            }
        }
    })
    .to_string()
}

/// A malformed mutation — an arm the grammar never admits — for the
/// boundary-400 witness.
fn malformed_body(tenant: &str, request_id: &str) -> String {
    json!({
        "tenant": tenant,
        "mutation": {
            "requestId": request_id,
            "type": "amalgamate_organizations",
        }
    })
    .to_string()
}

// --- Database probes ----------------------------------------------------------

/// The organization's `(state, revision, legal_name, website)`.
async fn org_row(fixture: &Fixture, tenant: &str, id: &str) -> (String, i32, String, String) {
    sqlx::query_as(
        "SELECT state,revision,coalesce(legal_name,''),coalesce(website,'') \
         FROM archava_organizations.organizations WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(tenant)
    .bind(id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the organization exists")
}

/// One affiliation's `(role, title, is_primary, status)`.
async fn aff_row(fixture: &Fixture, tenant: &str, id: &str) -> (String, String, bool, String) {
    sqlx::query_as(
        "SELECT role,coalesce(title,''),is_primary,status \
         FROM archava_organizations.affiliations WHERE tenant_id=$1 AND id=$2::uuid",
    )
    .bind(tenant)
    .bind(id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("the affiliation exists")
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
/// chain — the create receipt, the primary affiliation (the org revision
/// follows the affiliation write), the keep-old/replace update semantics on
/// the detail cells, the affiliation's role/department replace — and the
/// status read answers `committed` with the stored receipt.
#[tokio::test]
async fn authorized_org_flow_documents_its_life() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "ox").await;

    // The organization.
    let (status, receipt) = post(
        &app,
        "/erp/organizations/commands",
        &create_organization_body(ACME, KEY_ORG, "ACME Holding"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create_organization is authorized");
    assert_eq!(receipt["decision"], "receipt");
    assert_eq!(receipt["receipt"]["type"], "create_organization");
    assert_eq!(receipt["receipt"]["revision"], 1);
    assert_eq!(receipt["receipt"]["affiliationId"], Value::Null);
    let organization_id = receipt["receipt"]["organizationId"]
        .as_str()
        .expect("the receipt names the organization id")
        .to_string();
    let org = org_row(&fixture, ACME, &organization_id).await;
    assert_eq!(org.0, "active");
    assert_eq!(org.1, 1);
    assert_eq!(org.2, "PT ACME Holding", "the legal name carried the seed");

    // The primary affiliation — the org revision follows it.
    let (status, aff_receipt) = post(
        &app,
        "/erp/organizations/commands",
        &create_affiliation_body(
            ACME,
            KEY_AFF,
            &organization_id,
            CONTACT_A,
            "procurement",
            Some("Buyer"),
            true,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create_affiliation is authorized");
    assert_eq!(aff_receipt["receipt"]["type"], "create_affiliation");
    assert_eq!(aff_receipt["receipt"]["revision"], 2);
    let affiliation_id = aff_receipt["receipt"]["affiliationId"]
        .as_str()
        .expect("the receipt names the affiliation id")
        .to_string();
    let aff = aff_row(&fixture, ACME, &affiliation_id).await;
    assert_eq!(
        aff,
        ("procurement".into(), "Buyer".into(), true, "active".into())
    );

    // The status read answers `committed` with the stored receipt.
    let (status, status_body) = get(
        &app,
        &format!("/erp/organizations/commands/status?tenant={ACME}&requestId={KEY_AFF}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(status_body["decision"], "committed");
    assert_eq!(status_body["receipt"]["type"], "create_affiliation");

    // The update replaces ONLY the cells the document names: the absent
    // legal_name keeps "PT ACME Holding" while the website lands.
    let (status, update_receipt) = post(
        &app,
        "/erp/organizations/commands",
        &update_organization_body(
            ACME,
            KEY_UPD,
            &organization_id,
            2,
            Some("ACME Holding Group"),
            Some("https://group.acme.test"),
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update_organization is authorized");
    assert_eq!(update_receipt["receipt"]["type"], "update_organization");
    assert_eq!(update_receipt["receipt"]["revision"], 3);
    let org = org_row(&fixture, ACME, &organization_id).await;
    assert_eq!(org.1, 3);
    assert_eq!(org.2, "PT ACME Holding", "the absent keep-old cell holds");
    assert_eq!(org.3, "https://group.acme.test");

    // The affiliation update replaces role and department, keeps the title
    // (the detail cells' keep-old rule binds affiliations too).
    let (status, aff_update_receipt) = post(
        &app,
        "/erp/organizations/commands",
        &update_affiliation_body(
            ACME,
            KEY_UPD_AFF,
            &organization_id,
            &affiliation_id,
            json!({"role": "procurement lead", "department": "Purchasing"}),
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(aff_update_receipt["receipt"]["revision"], 4);
    let aff = aff_row(&fixture, ACME, &affiliation_id).await;
    assert_eq!(
        aff,
        (
            "procurement lead".into(),
            "Buyer".into(),
            true,
            "active".into()
        ),
        "the role and department replaced; the title kept"
    );
}

/// An update against an ARCHIVED organization refuses and holds NOTHING:
/// the conflict rolls back the whole submission — no revision moves, and
/// the refused request's command row never exists.
#[tokio::test]
async fn update_on_archived_refuses_and_holds_nothing() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "ox").await;

    let (status, receipt) = post(
        &app,
        "/erp/organizations/commands",
        &create_organization_body(ACME, KEY_ORG, "Sunset Co"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let organization_id = receipt["receipt"]["organizationId"]
        .as_str()
        .expect("the receipt names the organization id")
        .to_string();

    let (status, archive_receipt) = post(
        &app,
        "/erp/organizations/commands",
        &archive_organization_body(ACME, KEY_ARCH, &organization_id, 1),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the archive records");
    assert_eq!(archive_receipt["receipt"]["revision"], 2);

    // The archived gate: an update refuses.
    let (status, refusal) = post(
        &app,
        "/erp/organizations/commands",
        &update_organization_body(
            ACME,
            KEY_HOLD_UPD,
            &organization_id,
            2,
            Some("Phoenix Rising"),
            None,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a write against a dead organization conflicts"
    );
    assert_eq!(refusal["decision"], "refused");
    let org = org_row(&fixture, ACME, &organization_id).await;
    assert_eq!(org.0, "archived");
    assert_eq!(org.1, 2, "nothing moved");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_organizations.commands \
             WHERE tenant_id=$1 AND request_id=$2::uuid",
            &[ACME, KEY_HOLD_UPD],
        )
        .await,
        0,
        "the refused submission was never even claimed"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_organizations.audit WHERE request_id=$1::uuid",
            &[KEY_HOLD_UPD]
        )
        .await,
        0,
        "the refusal recorded nothing"
    );

    // Archiving an ARCHIVED organization conflicts the same way.
    let (status, second) = post(
        &app,
        "/erp/organizations/commands",
        &archive_organization_body(ACME, KEY_REMOVE, &organization_id, 2),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(second["decision"], "refused");
}

/// The affiliation uniqueness gates refuse without holding anything: a
/// second (contact, role) row errors at the unique index (23505 whitelist),
/// a second PRIMARY active trips the repository's precheck, and a fresh
/// affiliation still works after both refusals.
#[tokio::test]
async fn affiliation_gates_refuse_without_holding() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "ox").await;

    let (status, receipt) = post(
        &app,
        "/erp/organizations/commands",
        &create_organization_body(ACME, KEY_ORG, "Gate Co"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let organization_id = receipt["receipt"]["organizationId"]
        .as_str()
        .expect("the receipt names the organization id")
        .to_string();

    let (status, aff_receipt) = post(
        &app,
        "/erp/organizations/commands",
        &create_affiliation_body(
            ACME,
            KEY_AFF,
            &organization_id,
            CONTACT_A,
            "procurement",
            Some("Buyer"),
            true,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // The refusal checks below count affiliations by role — the id needs no
    // naming here.
    let _affiliation_id = aff_receipt["receipt"]["affiliationId"]
        .as_str()
        .expect("the receipt names the affiliation id")
        .to_string();

    // The same contact + role again: the database's unique index refuses.
    let (status, refusal) = post(
        &app,
        "/erp/organizations/commands",
        &create_affiliation_body(
            ACME,
            KEY_DUPLICATE,
            &organization_id,
            CONTACT_A,
            "procurement",
            None,
            false,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "the same (contact, role) pair conflicts at the index"
    );
    assert_eq!(refusal["decision"], "refused");

    // A different contact claiming the PRIMARY slot: the repository's
    // precheck refuses.
    let (status, second) = post(
        &app,
        "/erp/organizations/commands",
        &create_affiliation_body(
            ACME,
            KEY_PRIMARY,
            &organization_id,
            CONTACT_B,
            "billing",
            None,
            true,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a second active primary conflicts before the insert"
    );
    assert_eq!(second["decision"], "refused");

    // Both refusals recorded nothing.
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_organizations.affiliations \
             WHERE tenant_id=$1",
            &[ACME]
        )
        .await,
        1,
        "one affiliation stands"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_organizations.commands \
             WHERE tenant_id=$1 AND request_id IN ($2::uuid, $3::uuid)",
            &[ACME, KEY_DUPLICATE, KEY_PRIMARY],
        )
        .await,
        0,
        "the refusals were never claimed"
    );

    // A fresh NON-primary affiliation still lands after both refusals —
    // the gates refuse rows, not the ledger.
    let (status, fresh) = post(
        &app,
        "/erp/organizations/commands",
        &create_affiliation_body(
            ACME,
            KEY_UPD_AFF,
            &organization_id,
            CONTACT_C,
            "logistics",
            None,
            false,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the ledger still writes");
    assert_eq!(fresh["receipt"]["revision"], 3);
}

/// A receipt the CURRENT access no longer authorizes does not replay: the
/// same key + same bytes re-submitted after the grant was revoked is refused
/// by the grant gate, and the status read refuses equally — an old receipt
/// is not a permanent grant.
#[tokio::test]
async fn revoked_org_grant_refuses_recovery() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "ooff").await;

    // While the grant is live, the receipt records.
    let (status, receipt) = post(
        &app,
        "/erp/organizations/commands",
        &create_organization_body(ACME, KEY_REVOKED, "Vested Co"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the active grant records");
    assert_eq!(receipt["decision"], "receipt");
    assert!(
        receipt["receipt"].get("replayed").is_none(),
        "the strict TS receipt shape carries no replayed key"
    );

    // The grant dies.
    sqlx::query(
        "UPDATE archava_organizations.memberships SET active=false \
         WHERE tenant_id=$1 AND principal_id='ooff'",
    )
    .bind(ACME)
    .execute(&fixture.admin_pool)
    .await
    .expect("revoke ooff's organizations membership");

    // The same key + same bytes: the claim loses and the recovery refuses —
    // the CURRENT access cannot present `organizations.write`.
    let (status, replay) = post(
        &app,
        "/erp/organizations/commands",
        &create_organization_body(ACME, KEY_REVOKED, "Vested Co"),
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
            "SELECT count(*) FROM archava_organizations.organizations \
             WHERE tenant_id=$1 AND name='Vested Co'",
            &[ACME]
        )
        .await,
        1,
        "no SECOND organization — the replay recorded nothing new"
    );

    // The status read refuses equally.
    let (status, status_body) = get(
        &app,
        &format!("/erp/organizations/commands/status?tenant={ACME}&requestId={KEY_REVOKED}"),
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
    let session = login(&app, "ox").await;

    // First submission under the key: an organization.
    let (status, first) = post(
        &app,
        "/erp/organizations/commands",
        &create_organization_body(ACME, KEY_ORG, "Original Co"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["decision"], "receipt");
    let organization_id = first["receipt"]["organizationId"]
        .as_str()
        .expect("the receipt names the organization id")
        .to_string();

    // The attacker submits an AFFILIATION under the SAME key: bytes differ,
    // fingerprint differs → conflict, nothing recorded.
    let (status, refusal) = post(
        &app,
        "/erp/organizations/commands",
        &create_affiliation_body(
            ACME,
            KEY_ORG,
            &organization_id,
            CONTACT_A,
            "hijacking",
            None,
            false,
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
            "SELECT count(*) FROM archava_organizations.affiliations \
             WHERE tenant_id=$1 AND role='hijacking'",
            &[ACME]
        )
        .await,
        0,
        "the affiliation was never created"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_organizations.commands \
             WHERE request_id=$1::uuid AND receipt IS NOT NULL",
            &[KEY_ORG]
        )
        .await,
        1,
        "the stored receipt still answers the SAME command"
    );
}

/// A cross-tenant caller is refused before any business statement runs:
/// full GLOBEX grants mean nothing to ACME's ledger. The SAME caller in its
/// own tenant is fine — key independence.
#[tokio::test]
async fn cross_tenant_caller_refused_before_any_write() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "other").await;

    let (status, refusal) = post(
        &app,
        "/erp/organizations/commands",
        &create_organization_body(ACME, KEY_XTENANT, "Invader Co"),
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
            "SELECT count(*) FROM archava_organizations.organizations \
             WHERE tenant_id=$1",
            &[ACME]
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_organizations.commands \
             WHERE tenant_id=$1 AND request_id=$2::uuid",
            &[ACME, KEY_XTENANT]
        )
        .await,
        0,
        "no command row was even claimed in the target ledger"
    );

    // The SAME key in the caller's OWN tenant records — the idempotency
    // surface is per-tenant.
    let own = login(&app, "ogx").await;
    let (status, receipt) = post(
        &app,
        "/erp/organizations/commands",
        &create_organization_body(GLOBEX, KEY_XTENANT, "Legit Globex"),
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
/// replay branch answers `replayed` with the stored receipt and the ONE
/// organization stands — no second row, no double effect.
#[tokio::test]
async fn lost_response_replays_without_a_second_organization() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "ox").await;

    let (status, first) = post(
        &app,
        "/erp/organizations/commands",
        &create_organization_body(ACME, KEY_ORG, "Recoverable Co"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the first submit records");
    assert!(
        first["receipt"].get("replayed").is_none(),
        "strict TS receipt"
    );
    let organization_id = first["receipt"]["organizationId"]
        .as_str()
        .expect("the receipt names the organization id")
        .to_string();

    // The re-submission with the SAME key + SAME bytes replays.
    let (status, replay) = post(
        &app,
        "/erp/organizations/commands",
        &create_organization_body(ACME, KEY_ORG, "Recoverable Co"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "an identical resubmit replays");
    assert_eq!(replay["decision"], "replayed");
    assert!(
        replay["receipt"].get("replayed").is_none(),
        "strict TS receipt"
    );
    assert_eq!(replay["receipt"]["type"], "create_organization");
    assert_eq!(
        replay["receipt"]["organizationId"],
        probe_json_str(organization_id.as_str()),
        "the replayed receipt names the SAME organization"
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_organizations.organizations \
             WHERE tenant_id=$1",
            &[ACME]
        )
        .await,
        1,
        "still exactly one organization — no double create"
    );

    // The status read answers `committed` with the stored receipt.
    let (status, status_body) = get(
        &app,
        &format!("/erp/organizations/commands/status?tenant={ACME}&requestId={KEY_ORG}"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(status_body["decision"], "committed");
    assert_eq!(status_body["receipt"]["type"], "create_organization");
}

/// The `remove_affiliation` arm skips the archived gate exactly as the TS
/// branch does: an archived organization still releases its affiliation —
/// the row inactivates and the org revision follows the write, which is the
/// audit's evidence.
#[tokio::test]
async fn archived_org_still_releases_its_affiliation() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "ox").await;

    let (status, receipt) = post(
        &app,
        "/erp/organizations/commands",
        &create_organization_body(ACME, KEY_ORG, "Archive Co"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let organization_id = receipt["receipt"]["organizationId"]
        .as_str()
        .expect("the receipt names the organization id")
        .to_string();
    let (status, aff_receipt) = post(
        &app,
        "/erp/organizations/commands",
        &create_affiliation_body(
            ACME,
            KEY_AFF,
            &organization_id,
            CONTACT_A,
            "procurement",
            None,
            false,
        ),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let affiliation_id = aff_receipt["receipt"]["affiliationId"]
        .as_str()
        .expect("the receipt names the affiliation id")
        .to_string();

    // Archive: the still-active affiliations deactivate with the archive.
    let (status, _) = post(
        &app,
        "/erp/organizations/commands",
        &archive_organization_body(ACME, KEY_ARCH, &organization_id, 2),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let aff = aff_row(&fixture, ACME, &affiliation_id).await;
    assert_eq!(aff.3, "inactive", "the archive deactivated the row");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_organizations.audit \
             WHERE tenant_id=$1 AND operation='archive_organization'",
            &[ACME]
        )
        .await,
        1,
        "the archive's audit reason is on the ledger"
    );

    // remove_affiliation runs WITHOUT the archived gate.
    let (status, removal) = post(
        &app,
        "/erp/organizations/commands",
        &remove_affiliation_body(ACME, KEY_REMOVE, &organization_id, &affiliation_id),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the record-only arm skips the archived gate"
    );
    assert_eq!(removal["receipt"]["type"], "remove_affiliation");
    assert_eq!(removal["receipt"]["revision"], 4, "the org revision moved");
    let aff = aff_row(&fixture, ACME, &affiliation_id).await;
    assert_eq!(aff.3, "inactive", "the row is released, not resurrected");
}

/// An actor whose organization grants are missing is refused by the grant
/// gate with no command row at all — the `audit.read`-only registrar
/// cannot write any more than a stranger can. A malformed mutation, next,
/// is a boundary 400 that never reaches a database statement.
#[tokio::test]
async fn grants_missing_actor_refused_before_any_write() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app, "onone").await;

    let (status, refusal) = post(
        &app,
        "/erp/organizations/commands",
        &create_organization_body(ACME, KEY_ORG, "Forbidden Co"),
        Some(&session),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "the grant gate refuses");
    assert_eq!(refusal["decision"], "denied");
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_organizations.organizations WHERE tenant_id=$1",
            &[ACME],
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT count(*) FROM archava_organizations.commands \
             WHERE tenant_id=$1 AND request_id=$2::uuid",
            &[ACME, KEY_ORG]
        )
        .await,
        0,
        "the refusal happened before the claim"
    );

    // The malformed arm is a boundary rejection — HTTP 400 — and nothing
    // about it ever reaches either ledger table.
    let (status, rejected) = post(
        &app,
        "/erp/organizations/commands",
        &malformed_body(ACME, KEY_HOLD_UPD),
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
            "SELECT count(*) FROM archava_organizations.commands \
             WHERE request_id=$1::uuid",
            &[KEY_HOLD_UPD]
        )
        .await,
        0
    );
}

/// The replayed receipt's org id probe helper asserts through JSON for the
/// string-vs-number entry. (Kept as a fn so the assertion above stays
/// one-line.)
fn probe_json_str(value: &str) -> Value {
    json!(value)
}

/// P2 (A11): the status read answers to the `organizations.read` grant, and the
/// receipt is bound to the actor that wrote it. Membership alone — an
/// `audit.read`-only clerk — must not recover another actor's receipt, and a
/// full-grant peer must not either.
#[tokio::test]
async fn status_read_requires_the_organizations_read_grant() {
    const KEY: &str = "a11c0000-4444-4444-8444-0000000000a1";
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let author = login(&app, "ox").await;
    let auditor = login(&app, "onone").await;
    let peer = login(&app, "ooff").await;

    let body = create_organization_body(ACME, KEY, "Status read Co");
    let (status, outcome) = post(
        &app,
        "/erp/organizations/commands",
        &body,
        Some(&author),
        true,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the author records the command: {outcome}"
    );

    let uri = format!("/erp/organizations/commands/status?tenant={ACME}&requestId={KEY}");
    let (status, outcome) = get(&app, &uri, Some(&auditor)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "audit.read is not organizations.read: {outcome}"
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
    assert_eq!(outcome["receipt"]["type"], "create_organization");
}
