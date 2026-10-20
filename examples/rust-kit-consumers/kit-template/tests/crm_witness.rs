//! C04's consumer half: the REAL CRM read/write/recovery flow, driven from
//! outside the product workspace.
//!
//! This file is byte-identical in both business-model consumers. It reads only
//! `kit.config.json` for the workspace it runs as; everything else is the
//! shipped artifact in `deps/`. The test provisions its own disposable
//! PostgreSQL through `archava-pg-fixture`, seeds one identity and one CRM
//! membership, and then drives the host's real router with `tower::ServiceExt::
//! oneshot` — the same entrypoint a deployed consumer serves:
//!
//! - `contact.create` and `contact.update` persist real rows;
//! - `activity.add`, `followup.create`/`complete`, and
//!   `opportunity.create`/`move` persist their effects and revisions;
//! - `GET /contacts` and `GET /crm/records` read those rows back through the
//!   session's membership, including the pipeline history;
//! - a replayed request and the status route recover the same receipt from
//!   PostgreSQL with exactly one effect;
//! - a command addressed to the other kit's tenant is refused with
//!   `missing_membership`, and the seeded foreign row never appears in this
//!   consumer's page.
//!
//! What this does NOT claim: the two consumers are not the product's browser
//! surface (the browser proof owns that), and the kits' TypeScript modules are
//! not exercised here. The claim is narrow and the C04 audit asks for exactly
//! it — the same shipped CRM behavior runs in two independently built
//! consumers on isolated persisted data.

use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::routing::Router;
use serde_json::{json, Value};
use sqlx::types::Uuid;
use tower::ServiceExt;

/// The commerce ledger the host serves alongside CRM — the same table list the
/// host's own suites use.
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

/// A 32-byte identity HMAC key — the minimum `Identity::open` accepts. A
/// fixture-only constant, never a production bootstrap default.
const IDENTITY_KEY: &[u8] = &[7u8; 32];

/// The fixture password. The password gate enforces `length >= 15`.
const PASSWORD: &str = "correct horse battery staple";

/// A precomputed scrypt hash of [`PASSWORD`] — one hash serves the fixture cast.
const PASSWORD_HASH: &str =
    "scrypt$32768$8$3$OZtInKxSaSZqbEptj2fvUg$jU_gx2rQW-69EYpbRoMKGSgclhV-mO7gXVF-QYvSXVo";

const CONSUMER: &str = "consumer";
const FOREIGN_OWNER: &str = "foreign-owner";
const FOREIGN_CONTACT: &str = "2c3e5a10-3333-4333-8333-0000000000f1";

/// The full CRM grant the consumer's own workspace membership carries.
const PERMISSIONS: &[&str] = &[
    "contacts.read",
    "contacts.write",
    "activities.read",
    "activities.write",
    "followups.read",
    "followups.write",
    "pipeline.read",
    "pipeline.write",
];

const KIT: &str = include_str!("../kit.config.json");

fn config() -> Value {
    serde_json::from_str(KIT).expect("kit.config.json must parse")
}

fn workspace_tenant(config: &Value) -> String {
    config["workspace"]["tenant"]
        .as_str()
        .expect("workspace.tenant")
        .to_string()
}

fn foreign_tenant(config: &Value) -> String {
    config["workspace"]["foreignTenant"]
        .as_str()
        .expect("workspace.foreignTenant")
        .to_string()
}

fn commerce_ledger() -> Ledger {
    Ledger {
        namespace: "archava_commerce",
        ledger: "Commerce",
        tables: TABLES,
    }
}

/// Serving state that has passed the commerce, identity and CRM readiness
/// gates. The consumer runs the real host, not a test double.
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
    .expect("a ready fixture must construct CRM serving state")
}

async fn seed(fixture: &Fixture) {
    let config = config();
    let tenant = workspace_tenant(&config);
    let foreign = foreign_tenant(&config);

    sqlx::query(
        "INSERT INTO archava_identity.users (id, username, password_hash, active, version) \
         VALUES ($1, $2, $3, true, 1)",
    )
    .bind(CONSUMER)
    .bind(format!("{CONSUMER}@archava.test"))
    .bind(PASSWORD_HASH)
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the consumer identity");

    sqlx::query(
        "INSERT INTO archava_crm.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1,$2,'human',$3,'all',true)",
    )
    .bind(&tenant)
    .bind(CONSUMER)
    .bind(PERMISSIONS.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the consumer's CRM membership");

    // A row in the OTHER kit's tenant. It is a real row the consumer must not
    // see or be able to address; its owner is a separate principal so the
    // consumer has no foreign membership to borrow.
    sqlx::query(
        "INSERT INTO archava_crm.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1,$2,'human',ARRAY['contacts.read'],'all',true)",
    )
    .bind(&foreign)
    .bind(FOREIGN_OWNER)
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the foreign membership");
    sqlx::query(
        "INSERT INTO archava_crm.contacts (tenant_id, id, full_name, owner_id) \
         VALUES ($1,$2,'Foreign Contact',$3)",
    )
    .bind(&foreign)
    .bind(Uuid::parse_str(FOREIGN_CONTACT).expect("a fixture uuid"))
    .bind(FOREIGN_OWNER)
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the foreign contact");
}

struct Session {
    token: String,
    csrf: String,
}

async fn login(app: &Router) -> Session {
    let body = json!({
        "username": format!("{CONSUMER}@archava.test"),
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
                .expect("build login"),
        )
        .await
        .expect("login answers");
    assert_eq!(response.status(), StatusCode::OK, "the consumer logs in");
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read login body");
    let outcome: Value = serde_json::from_slice(&bytes).expect("login JSON");
    Session {
        token: outcome["token"].as_str().expect("token").to_string(),
        csrf: outcome["csrf"].as_str().expect("csrf").to_string(),
    }
}

async fn post(
    app: &Router,
    uri: &str,
    body: &Value,
    session: &Session,
) -> (StatusCode, Value) {
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
                .expect("build request"),
        )
        .await
        .expect("route answers");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    (status, serde_json::from_slice(&bytes).expect("JSON response"))
}

async fn get(app: &Router, uri: &str, session: &Session) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header("authorization", format!("Bearer {}", session.token))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("route answers");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    (status, serde_json::from_slice(&bytes).expect("JSON response"))
}

/// Submit one reviewed command and require a receipt. The caller supplies the
/// request id so the recovery witness can replay the SAME key.
async fn submit(
    app: &Router,
    tenant: &str,
    request_id: &str,
    command: Value,
    session: &Session,
) -> Value {
    let body = json!({
        "tenant": tenant,
        "requestId": request_id,
        "command": command,
    });
    let (status, outcome) = post(app, "/crm/commands", &body, session).await;
    assert_eq!(status, StatusCode::OK, "command {request_id} refused: {outcome}");
    assert_eq!(outcome["decision"], "receipt", "command {request_id}: {outcome}");
    outcome
}

async fn record(app: &Router, tenant: &str, contact_id: &str, session: &Session) -> Value {
    let (status, outcome) = get(
        app,
        &format!("/crm/records?tenant={tenant}&contact={contact_id}"),
        session,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the record read refused: {outcome}");
    outcome
}

#[tokio::test]
async fn a_packaged_consumer_runs_the_real_crm_read_write_recovery_flow() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app).await;
    let config = config();
    let tenant = workspace_tenant(&config);
    let foreign = foreign_tenant(&config);

    // The producer parses this line: the fixture is a real, isolated
    // PostgreSQL instance this consumer provisioned for itself.
    println!(
        "C04_CONSUMER kit={} tenant={} fixture_port={}",
        config["kit"]["id"].as_str().expect("kit.id"),
        tenant,
        fixture.port
    );

    // 1. contact.create — the write, then the real list read.
    let created = submit(
        &app,
        &tenant,
        "00000000-0000-4000-8000-000000000201",
        json!({
            "operation": "contact.create",
            "data": {
                "fullName": "  Packaged Consumer Contact  ",
                "email": "packaged@consumer.test",
                "organization": "Packaged Org",
            },
        }),
        &session,
    )
    .await;
    assert_eq!(created["receipt"]["kind"], "contact");
    assert_eq!(created["receipt"]["revision"], 1);
    let contact_id = created["receipt"]["id"]
        .as_str()
        .expect("the contact receipt id")
        .to_string();

    let (status, page) = get(
        &app,
        &format!("/contacts?tenant={tenant}&search=Packaged"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the contact list refused: {page}");
    let items = page["items"].as_array().expect("the page carries items");
    assert_eq!(items.len(), 1, "one matching row: {page}");
    assert_eq!(items[0]["id"], contact_id);
    assert_eq!(items[0]["fullName"], "Packaged Consumer Contact");
    assert_eq!(items[0]["email"], "packaged@consumer.test");
    assert_eq!(items[0]["ownerId"], CONSUMER);
    assert_eq!(items[0]["revision"], 1);
    assert!(
        !items.iter().any(|item| item["fullName"] == "Foreign Contact"),
        "the other kit's row never enters this consumer's page"
    );

    // 2. contact.update — revision-bound, partial.
    let updated = submit(
        &app,
        &tenant,
        "00000000-0000-4000-8000-000000000202",
        json!({
            "operation": "contact.update",
            "id": contact_id,
            "expectedRevision": 1,
            "data": {"fullName": "Packaged Consumer Contact (updated)"},
        }),
        &session,
    )
    .await;
    assert_eq!(updated["receipt"]["kind"], "contact");
    assert_eq!(updated["receipt"]["revision"], 2);

    // 3. activity.add — the contact revision moves with the activity.
    let activity = submit(
        &app,
        &tenant,
        "00000000-0000-4000-8000-000000000203",
        json!({
            "operation": "activity.add",
            "contactId": contact_id,
            "contactRevision": 2,
            "kind": "note",
            "summary": "Packaged consumer activity",
        }),
        &session,
    )
    .await;
    assert_eq!(activity["receipt"]["kind"], "activity");
    let after_activity = record(&app, &tenant, &contact_id, &session).await;
    assert_eq!(after_activity["contact"]["revision"], 3);
    assert_eq!(after_activity["activities"][0]["kind"], "note");
    assert_eq!(
        after_activity["activities"][0]["summary"],
        "Packaged consumer activity"
    );

    // 4. followup.create then followup.complete — the open task closes exactly
    //    once, and the record read shows the completed status.
    let followup = submit(
        &app,
        &tenant,
        "00000000-0000-4000-8000-000000000204",
        json!({
            "operation": "followup.create",
            "contactId": contact_id,
            "contactRevision": 3,
            "summary": "Packaged consumer follow-up",
            "dueAt": "2027-03-04T10:15:00Z",
        }),
        &session,
    )
    .await;
    assert_eq!(followup["receipt"]["kind"], "followup");
    let followup_id = followup["receipt"]["id"]
        .as_str()
        .expect("the followup receipt id")
        .to_string();
    let after_followup = record(&app, &tenant, &contact_id, &session).await;
    assert_eq!(
        after_followup["contact"]["revision"], 4,
        "followup.create bumps the contact revision"
    );

    let completed = submit(
        &app,
        &tenant,
        "00000000-0000-4000-8000-000000000205",
        json!({
            "operation": "followup.complete",
            "id": followup_id,
            "expectedRevision": 1,
        }),
        &session,
    )
    .await;
    assert_eq!(completed["receipt"]["kind"], "followup");
    assert_eq!(completed["receipt"]["revision"], 2);

    // 5. opportunity.create and opportunity.move — the pipeline row and its
    //    immutable transition history are real reads.
    let opportunity = submit(
        &app,
        &tenant,
        "00000000-0000-4000-8000-000000000206",
        json!({
            "operation": "opportunity.create",
            "contactId": contact_id,
            "contactRevision": 4,
            "title": "Packaged consumer opportunity",
        }),
        &session,
    )
    .await;
    assert_eq!(opportunity["receipt"]["kind"], "opportunity");
    let opportunity_id = opportunity["receipt"]["id"]
        .as_str()
        .expect("the opportunity receipt id")
        .to_string();

    let move_body = json!({
        "operation": "opportunity.move",
        "id": opportunity_id,
        "expectedRevision": 1,
        "stage": "qualified",
    });
    let moved = submit(
        &app,
        &tenant,
        "00000000-0000-4000-8000-000000000207",
        move_body.clone(),
        &session,
    )
    .await;
    assert_eq!(moved["receipt"]["kind"], "opportunity");
    assert_eq!(moved["receipt"]["revision"], 2);

    let final_record = record(&app, &tenant, &contact_id, &session).await;
    assert_eq!(
        final_record["contact"]["fullName"],
        "Packaged Consumer Contact (updated)"
    );
    assert_eq!(final_record["followups"][0]["status"], "completed");
    assert_eq!(final_record["opportunities"][0]["stage"], "qualified");
    let transitions = final_record["opportunities"][0]["transitions"]
        .as_array()
        .expect("the transition history");
    assert_eq!(transitions.len(), 2);
    assert_eq!(transitions[0]["to"], "new");
    assert_eq!(transitions[1]["to"], "qualified");
    assert_eq!(final_record["pipelineEnabled"], true);

    // 6. Recovery: the same key replays the same receipt, the status route
    //    resolves it, and PostgreSQL still has exactly one effect.
    let replayed = submit(
        &app,
        &tenant,
        "00000000-0000-4000-8000-000000000207",
        move_body,
        &session,
    )
    .await;
    assert_eq!(replayed["receipt"]["id"], moved["receipt"]["id"]);
    let (status, recovered) = get(
        &app,
        &format!(
            "/crm/commands/status?tenant={tenant}&requestId=00000000-0000-4000-8000-000000000207"
        ),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the recovery route refused: {recovered}");
    assert_eq!(recovered["status"], "committed");
    assert_eq!(recovered["receipt"]["id"], moved["receipt"]["id"]);

    let opportunities: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_crm.opportunities WHERE tenant_id=$1 AND contact_id=$2",
    )
    .bind(&tenant)
    .bind(Uuid::parse_str(&contact_id).expect("a receipt uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the opportunities");
    assert_eq!(opportunities, 1, "the replay wrote no second opportunity");
    let transitions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_crm.opportunity_transitions \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(&tenant)
    .bind(Uuid::parse_str("00000000-0000-4000-8000-000000000207").expect("a request uuid"))
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the move transitions");
    assert_eq!(transitions, 1, "the replay wrote no second transition");
    let audits: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_crm.audit WHERE tenant_id=$1")
            .bind(&tenant)
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("count the audit rows");
    assert_eq!(audits, 7, "one audit row per command, none for the replay");

    // 7. The other kit's tenant is refused, and its seeded row is untouched.
    let foreign_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_crm.contacts WHERE tenant_id=$1")
            .bind(&foreign)
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("count the foreign rows");
    let (status, refused) = post(
        &app,
        "/crm/commands",
        &json!({
            "tenant": foreign,
            "requestId": "00000000-0000-4000-8000-000000000208",
            "command": {
                "operation": "contact.create",
                "data": {"fullName": "Should Not Exist"},
            },
        }),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a forged tenant was accepted: {refused}");
    assert_eq!(refused["reason"], "missing_membership");
    let foreign_after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_crm.contacts WHERE tenant_id=$1")
            .bind(&foreign)
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("count the foreign rows");
    assert_eq!(foreign_after, foreign_before, "the refused write changed nothing");

    // The effect rows the consumer wrote are visible through the same real
    // schema the product ships, not through a test-only projection.
    let rows: (i64, i64, i64) = sqlx::query_as(
        "SELECT \
           (SELECT count(*) FROM archava_crm.contacts WHERE tenant_id=$1), \
           (SELECT count(*) FROM archava_crm.activities WHERE tenant_id=$1), \
           (SELECT count(*) FROM archava_crm.followups WHERE tenant_id=$1)",
    )
    .bind(&tenant)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the consumer's rows");
    assert_eq!(rows, (1, 1, 1));
}
