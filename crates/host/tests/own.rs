//! C03 — the owner-configuration lifecycle against real HTTP and real
//! PostgreSQL.
//!
//! Each witness drives the router (`tower::ServiceExt::oneshot`) with a real
//! session and a real `archava_owner` ledger:
//!
//! - inspect reads the persisted config and the principal's grants;
//! - plan is deterministic and read-only;
//! - apply writes exactly one revision + receipt + audit in one transaction,
//!   replays a duplicate request id, refuses a stale base or a foreign
//!   confirmation, and is refused entirely when the grant is missing/revoked;
//! - the disabled `crm-pipeline` module refuses `opportunity.create` at the
//!   backend and starts accepting it again after the owner re-enables it;
//! - restore lands as a NEW revision and leaves history and business rows
//!   intact; a mandatory module cannot be disabled.
//!
//! The engine's own semantics (identity, blocked dependencies, T09) are pinned
//! by `cargo test -p archava-own`; this file proves the persisted product
//! lifecycle around them.

use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::routing::Router;
use tower::ServiceExt;
#[path = "own/disclosure.rs"]
mod disclosure;

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

const IDENTITY_KEY: &[u8] = &[7u8; 32];
const PASSWORD: &str = "correct horse battery staple";
const PASSWORD_HASH: &str =
    "scrypt$32768$8$3$OZtInKxSaSZqbEptj2fvUg$jU_gx2rQW-69EYpbRoMKGSgclhV-mO7gXVF-QYvSXVo";

/// Keep this integration binary inside the shipped process-wide derivation
/// budget while libtest runs the owner witnesses in parallel. The production
/// limit is two; the test must queue its own logins rather than turn a normal
/// successful login into a 503 `busy` response merely because another witness
/// is authenticating at the same time.
static LOGIN_SLOTS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(archava_host::max_active_derivations());

const ACME: &str = "acme-retail";
const ID_ADA: &str = "2c3e5a10-2222-4222-8222-0000000000a1";

fn commerce_ledger() -> Ledger {
    Ledger {
        namespace: "archava_commerce",
        ledger: "Commerce",
        tables: TABLES,
    }
}

async fn state(fixture: &Fixture) -> AppState {
    AppState::try_new_owner(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
        &fixture.crm_ledger,
        &fixture.owner_ledger,
    )
    .await
    .expect("a ready fixture must construct owner serving state")
}

/// Alice holds the owner management grant; Bob has a CRM read membership but
/// no owner grant at all (the missing-grant refusal).
async fn seed(fixture: &Fixture) {
    for (id, permissions) in [
        (
            "alice",
            vec![
                "contacts.read",
                "contacts.write",
                "activities.read",
                "activities.write",
                "followups.read",
                "followups.write",
                "audit.read",
                "pipeline.read",
                "pipeline.write",
            ],
        ),
        ("bob", vec!["contacts.read"]),
        // Carol holds the SAME owner grants as Alice and only a view-only CRM
        // membership. She is the second owner whose operations must stay
        // separate: the receipt ledger's key is tenant-wide, so naming
        // Alice's request id must not read or replay Alice's operation.
        ("carol", vec!["contacts.read"]),
    ] {
        sqlx::query(
            "INSERT INTO archava_identity.users (id, username, password_hash, active, version) \
             VALUES ($1, $2, $3, true, 1)",
        )
        .bind(id)
        .bind(format!("{id}@archava.test"))
        .bind(PASSWORD_HASH)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed identity");
        sqlx::query(
            "INSERT INTO archava_crm.memberships \
             (tenant_id, principal_id, kind, permissions, scope, active) \
             VALUES ($1,$2,'human',$3,'all',true)",
        )
        .bind(ACME)
        .bind(id)
        .bind(permissions)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed CRM membership");
    }
    sqlx::query(
        "INSERT INTO archava_owner.owner_grants (tenant_id, principal_id, permissions, active) \
         VALUES ($1,'alice',ARRAY['config.read','config.apply'],true)",
    )
    .bind(ACME)
    .execute(&fixture.admin_pool)
    .await
    .expect("seed owner grant");
    sqlx::query(
        "INSERT INTO archava_owner.owner_grants (tenant_id, principal_id, permissions, active) \
         VALUES ($1,'carol',ARRAY['config.read','config.apply'],true)",
    )
    .bind(ACME)
    .execute(&fixture.admin_pool)
    .await
    .expect("seed carol's owner grant");
    sqlx::query(
        "INSERT INTO archava_crm.contacts (tenant_id,id,full_name,owner_id) \
         VALUES ($1,$2,'Ada Lovelace','alice')",
    )
    .bind(ACME)
    .bind(uuid::Uuid::parse_str(ID_ADA).expect("a test uuid"))
    .execute(&fixture.admin_pool)
    .await
    .expect("seed contact");
}

struct Session {
    token: String,
    csrf: String,
}

async fn login(app: &Router, principal: &str) -> Session {
    let _slot = LOGIN_SLOTS.acquire().await.expect("login slots are open");
    let body = serde_json::json!({
        "username": format!("{principal}@archava.test"),
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
                .expect("build login"),
        )
        .await
        .expect("login answers");
    assert_eq!(response.status(), StatusCode::OK, "login as {principal}");
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let outcome: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    Session {
        token: outcome["token"].as_str().expect("token").to_string(),
        csrf: outcome["csrf"].as_str().expect("csrf").to_string(),
    }
}

async fn post(
    app: &Router,
    uri: &str,
    body: &serde_json::Value,
    session: &Session,
) -> (StatusCode, serde_json::Value) {
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
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    // Extraction rejections (422) answer a text body; a test asserting the
    // refusal must not panic before it can read the status.
    let outcome: serde_json::Value =
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, outcome)
}

async fn get(app: &Router, uri: &str, session: &Session) -> (StatusCode, serde_json::Value) {
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
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let outcome: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    (status, outcome)
}

/// A request with an optional (possibly forged) bearer and no CSRF header.
/// The session gate runs before the CSRF gate, which is exactly the order the
/// unauthenticated surface needs to be proven in.
async fn raw(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<&serde_json::Value>,
    bearer: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
                .expect("build request"),
        )
        .await
        .expect("route answers");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let outcome: serde_json::Value =
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, outcome)
}

fn plan_body(base: i64, enable: &[&str], disable: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "tenant": ACME,
        "request": {"baseRevision": base, "enable": enable, "disable": disable}
    })
}

fn chat_body(message: &str) -> serde_json::Value {
    serde_json::json!({"tenant": ACME, "message": message})
}

fn apply_body(
    request_id: &str,
    base: i64,
    enable: &[&str],
    disable: &[&str],
    plan_hash: &str,
) -> serde_json::Value {
    serde_json::json!({
        "tenant": ACME,
        "requestId": request_id,
        "request": {"baseRevision": base, "enable": enable, "disable": disable},
        "confirmation": {"planHash": plan_hash, "baseRevision": base}
    })
}

fn opportunity_body(request_id: &str) -> serde_json::Value {
    serde_json::json!({
        "tenant": ACME,
        "requestId": request_id,
        "command": {
            "operation": "opportunity.create",
            "contactId": ID_ADA,
            "contactRevision": 1,
            "title": "Renewal 2027"
        }
    })
}

async fn count(fixture: &Fixture, table: &str) -> i64 {
    let sql = format!("SELECT count(*) FROM archava_owner.{table}");
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_one(&fixture.admin_pool)
        .await
        .expect("count owner rows")
}

#[tokio::test]
async fn disabling_the_pipeline_refuses_the_backend_and_enabling_restores_it() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    let (status, inspected) = get(&app, &format!("/own/config?tenant={ACME}"), &alice).await;
    assert_eq!(status, StatusCode::OK, "{inspected}");
    assert_eq!(inspected["grants"]["apply"], true);
    let pipeline = inspected["modules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|module| module["id"] == "crm-pipeline")
        .expect("the catalogue names the pipeline");
    assert_eq!(pipeline["enabled"], true, "the default config keeps it on");

    let (status, planned) = post(
        &app,
        "/own/plan",
        &plan_body(0, &[], &["crm-pipeline"]),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    assert_eq!(planned["plan"]["blocked"], serde_json::Value::Null);
    assert_eq!(planned["plan"]["config"]["revision"], 1);
    let hash = planned["plan"]["planHash"]
        .as_str()
        .expect("plan hash")
        .to_string();

    let disable = apply_body("own-request-disable", 0, &[], &["crm-pipeline"], &hash);
    let (status, applied) = post(&app, "/own/apply", &disable, &alice).await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["receipt"]["revision"], 1);
    assert_eq!(applied["replayed"], false);

    // The UI is not the authority: the same operation refuses at the backend.
    let (status, refused) = post(
        &app,
        "/crm/commands",
        &opportunity_body("00000000-0000-4000-8000-0000000000c1"),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert_eq!(refused["reason"], "module_disabled");

    // Re-enable and the same operation succeeds.
    let (status, planned) = post(
        &app,
        "/own/plan",
        &plan_body(1, &["crm-pipeline"], &[]),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    let hash = planned["plan"]["planHash"]
        .as_str()
        .expect("plan hash")
        .to_string();
    let enable = apply_body("own-request-enable", 1, &["crm-pipeline"], &[], &hash);
    let (status, applied) = post(&app, "/own/apply", &enable, &alice).await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["receipt"]["revision"], 2);

    let (status, accepted) = post(
        &app,
        "/crm/commands",
        &opportunity_body("00000000-0000-4000-8000-0000000000c2"),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert_eq!(accepted["receipt"]["kind"], "opportunity");
}

#[tokio::test]
async fn a_duplicate_submission_replays_one_receipt_and_one_effect() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    let (_, planned) = post(
        &app,
        "/own/plan",
        &plan_body(0, &[], &["crm-pipeline"]),
        &alice,
    )
    .await;
    let hash = planned["plan"]["planHash"].as_str().unwrap().to_string();
    let body = apply_body("own-request-replay", 0, &[], &["crm-pipeline"], &hash);

    let (status, first) = post(&app, "/own/apply", &body, &alice).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let (status, second) = post(&app, "/own/apply", &body, &alice).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["replayed"], true);
    assert_eq!(second["receipt"]["revision"], first["receipt"]["revision"]);
    assert_eq!(count(&fixture, "config_revisions").await, 1);
    assert_eq!(count(&fixture, "apply_receipts").await, 1);
    assert_eq!(count(&fixture, "config_audit").await, 1);
}

#[tokio::test]
async fn a_stale_base_and_a_foreign_confirmation_refuse_without_writing() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    let (_, planned) = post(
        &app,
        "/own/plan",
        &plan_body(0, &[], &["crm-pipeline"]),
        &alice,
    )
    .await;
    let hash = planned["plan"]["planHash"].as_str().unwrap().to_string();
    let (status, _) = post(
        &app,
        "/own/apply",
        &apply_body("own-request-first", 0, &[], &["crm-pipeline"], &hash),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, stale) = post(
        &app,
        "/own/plan",
        &plan_body(0, &["crm-pipeline"], &[]),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{stale}");
    assert_eq!(stale["reason"], "stale_base");
    assert_eq!(stale["detail"]["currentRevision"], 1);

    let (status, mismatch) = post(
        &app,
        "/own/apply",
        &apply_body(
            "own-request-mismatch",
            1,
            &["crm-pipeline"],
            &[],
            "0000000000000000",
        ),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{mismatch}");
    assert_eq!(mismatch["reason"], "confirmation_mismatch");
    assert_eq!(count(&fixture, "config_revisions").await, 1);
}

#[tokio::test]
async fn a_missing_or_revoked_grant_refuses_the_operation() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;
    let bob = login(&app, "bob").await;

    let (status, refused) = get(&app, &format!("/own/config?tenant={ACME}"), &bob).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert_eq!(refused["reason"], "missing_grant");

    let (_, planned) = post(
        &app,
        "/own/plan",
        &plan_body(0, &[], &["crm-pipeline"]),
        &alice,
    )
    .await;
    let hash = planned["plan"]["planHash"].as_str().unwrap().to_string();
    let (status, applied) = post(
        &app,
        "/own/apply",
        &apply_body(
            "own-request-before-revoke",
            0,
            &[],
            &["crm-pipeline"],
            &hash,
        ),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");

    sqlx::query("UPDATE archava_owner.owner_grants SET active=false WHERE tenant_id=$1 AND principal_id='alice'")
        .bind(ACME)
        .execute(&fixture.admin_pool)
        .await
        .expect("revoke the owner grant");

    let (status, revoked) = post(
        &app,
        "/own/apply",
        &apply_body("own-request-after-revoke", 1, &["crm-pipeline"], &[], &hash),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{revoked}");
    assert_eq!(revoked["reason"], "missing_grant");
    assert_eq!(count(&fixture, "config_revisions").await, 1);

    // Revocation is checked BEFORE the receipt lookup: a replay under the
    // original request id is refused too, not answered from the stored
    // receipt. The same grant gate covers the inspect, plan and history
    // reads, so a revoked principal cannot discover or replay anything.
    let (status, replay) = post(
        &app,
        "/own/apply",
        &apply_body(
            "own-request-before-revoke",
            0,
            &[],
            &["crm-pipeline"],
            &hash,
        ),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{replay}");
    assert_eq!(replay["reason"], "missing_grant");
    for uri in [
        format!("/own/config?tenant={ACME}"),
        format!("/own/history?tenant={ACME}"),
    ] {
        let (status, refused) = get(&app, &uri, &alice).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}: {refused}");
        assert_eq!(refused["reason"], "missing_grant");
    }
    let (status, planned) = post(
        &app,
        "/own/plan",
        &plan_body(1, &["crm-pipeline"], &[]),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{planned}");
    assert_eq!(planned["reason"], "missing_grant");
    assert_eq!(count(&fixture, "config_revisions").await, 1);
}

#[tokio::test]
async fn restore_lands_as_a_new_revision_and_keeps_history_and_business_rows() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    let (_, planned) = post(
        &app,
        "/own/plan",
        &plan_body(0, &[], &["crm-pipeline"]),
        &alice,
    )
    .await;
    let hash = planned["plan"]["planHash"].as_str().unwrap().to_string();
    post(
        &app,
        "/own/apply",
        &apply_body("own-request-disable", 0, &[], &["crm-pipeline"], &hash),
        &alice,
    )
    .await;
    let (_, planned) = post(
        &app,
        "/own/plan",
        &plan_body(1, &["crm-pipeline"], &[]),
        &alice,
    )
    .await;
    let hash = planned["plan"]["planHash"].as_str().unwrap().to_string();
    post(
        &app,
        "/own/apply",
        &apply_body("own-request-enable", 1, &["crm-pipeline"], &[], &hash),
        &alice,
    )
    .await;

    let (status, preview) = post(
        &app,
        "/own/restore/plan",
        &serde_json::json!({"tenant": ACME, "targetRevision": 1}),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["plan"]["config"]["revision"], 3);
    let hash = preview["plan"]["planHash"].as_str().unwrap().to_string();
    let (status, restored) = post(
        &app,
        "/own/restore",
        &serde_json::json!({
            "tenant": ACME,
            "requestId": "own-request-restore",
            "targetRevision": 1,
            "confirmation": {"planHash": hash, "baseRevision": 2}
        }),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["receipt"]["revision"], 3);
    assert_eq!(restored["receipt"]["outcome"], "restore");

    let (status, history) = get(&app, &format!("/own/history?tenant={ACME}"), &alice).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    let revisions: Vec<i64> = history["revisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["revision"].as_i64().unwrap())
        .collect();
    assert_eq!(revisions, vec![3, 2, 1]);
    assert_eq!(history["revisions"][0]["actor"], "alice");
    let contacts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_crm.contacts WHERE tenant_id=$1")
            .bind(ACME)
            .fetch_one(&fixture.admin_pool)
            .await
            .unwrap();
    assert_eq!(contacts, 1, "restore never rewrites business rows");
}

#[tokio::test]
async fn a_mandatory_module_is_blocked_and_nothing_is_written() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    let (status, planned) = post(
        &app,
        "/own/plan",
        &plan_body(0, &[], &["crm-contacts"]),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    assert_eq!(planned["plan"]["blocked"]["reason"], "mandatory");
    let hash = planned["plan"]["planHash"].as_str().unwrap().to_string();
    let (status, refused) = post(
        &app,
        "/own/apply",
        &apply_body("own-request-mandatory", 0, &[], &["crm-contacts"], &hash),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["reason"], "plan_blocked");
    assert_eq!(count(&fixture, "config_revisions").await, 0);
}

#[tokio::test]
async fn racing_applies_converge_to_one_revision_and_one_receipt() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    let (_, planned) = post(
        &app,
        "/own/plan",
        &plan_body(0, &[], &["crm-pipeline"]),
        &alice,
    )
    .await;
    let hash = planned["plan"]["planHash"].as_str().unwrap().to_string();
    let first = apply_body("own-race-first", 0, &[], &["crm-pipeline"], &hash);
    let second = apply_body("own-race-second", 0, &[], &["crm-pipeline"], &hash);

    // Two independent submissions of the SAME plan against the SAME base.
    // The advisory tenant lock serialises them; the second sees the moved
    // base and replans instead of minting a second revision.
    let (left, right) = tokio::join!(
        post(&app, "/own/apply", &first, &alice),
        post(&app, "/own/apply", &second, &alice),
    );
    let mut statuses = [left.0, right.0];
    statuses.sort_by_key(|status| status.as_u16());
    assert_eq!(
        statuses,
        [StatusCode::OK, StatusCode::CONFLICT],
        "one apply commits, the other refuses"
    );
    assert_eq!(count(&fixture, "config_revisions").await, 1);
    assert_eq!(count(&fixture, "apply_receipts").await, 1);
    assert_eq!(count(&fixture, "config_audit").await, 1);
}

#[tokio::test]
async fn a_rebuilt_host_recovers_the_same_receipt_and_history_from_postgres() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    let (_, planned) = post(
        &app,
        "/own/plan",
        &plan_body(0, &[], &["crm-pipeline"]),
        &alice,
    )
    .await;
    let hash = planned["plan"]["planHash"].as_str().unwrap().to_string();
    let body = apply_body("own-restart-request", 0, &[], &["crm-pipeline"], &hash);
    let (status, applied) = post(&app, "/own/apply", &body, &alice).await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["receipt"]["revision"], 1);
    drop(app);

    // A NEW serving state over the same durable ledger — no in-memory carry.
    // The same session token still authenticates (the session row is in
    // PostgreSQL), inspect reads revision 1, and the SAME request id replays
    // the SAME receipt instead of minting revision 2.
    let restarted = archava_host::router(state(&fixture).await);
    let (status, inspected) = get(&restarted, &format!("/own/config?tenant={ACME}"), &alice).await;
    assert_eq!(status, StatusCode::OK, "{inspected}");
    assert_eq!(inspected["config"]["revision"], 1);
    let (status, replayed) = post(&restarted, "/own/apply", &body, &alice).await;
    assert_eq!(status, StatusCode::OK, "{replayed}");
    assert_eq!(replayed["replayed"], true);
    assert_eq!(replayed["receipt"]["revision"], 1);

    let (status, history) = get(&restarted, &format!("/own/history?tenant={ACME}"), &alice).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    assert_eq!(history["revisions"].as_array().unwrap().len(), 1);
    assert_eq!(count(&fixture, "config_revisions").await, 1);
    assert_eq!(count(&fixture, "apply_receipts").await, 1);
}

/// **Witness: the chat and the dashboard resolve ONE plan identity.**
///
/// The chat parses `disable crm-pipeline` into the same typed request the
/// dashboard posts, reads the base revision from the persisted config, and
/// calls the same `catalogue().plan`. The plan hash, base config, catalogue
/// version and pricing version agree, and the chat's plan is actionable through
/// the normal apply endpoint. A relabel instruction agrees the same way.
#[tokio::test]
async fn the_chat_plan_equals_the_dashboard_plan_and_applies() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    let (status, dashboard) = post(
        &app,
        "/own/plan",
        &plan_body(0, &[], &["crm-pipeline"]),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dashboard}");
    // The preview document is closed: the plan plus the authoritative base,
    // catalogue and pricing identity, public provider destination and optional
    // signed disclosure review. Preview still has no external provider effect.
    let mut keys: Vec<&str> = dashboard
        .as_object()
        .expect("the preview is an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "aiProvider",
            "baseConfig",
            "catalogueVersion",
            "disclosureApproval",
            "disclosureCompanies",
            "plan",
            "pricingVersion",
            "request"
        ]
    );
    assert_eq!(dashboard["plan"]["changes"][0]["kind"], "disable");
    assert!(dashboard.get("payment").is_none());
    assert!(dashboard.get("provider").is_none());
    assert!(dashboard.get("activation").is_none());
    let (status, chat) = post(
        &app,
        "/own/chat",
        &chat_body("disable crm-pipeline"),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{chat}");
    assert_eq!(chat["plan"], dashboard["plan"]);
    assert_eq!(chat["plan"]["planHash"], dashboard["plan"]["planHash"]);
    assert_eq!(chat["plan"]["baseRevision"], 0);
    assert_eq!(chat["baseConfig"], dashboard["baseConfig"]);
    assert_eq!(chat["catalogueVersion"], dashboard["catalogueVersion"]);
    assert_eq!(chat["pricingVersion"], dashboard["pricingVersion"]);

    let hash = chat["plan"]["planHash"]
        .as_str()
        .expect("the chat plan hash");
    let (status, applied) = post(
        &app,
        "/own/apply",
        &apply_body("own-chat-parity-1", 0, &[], &["crm-pipeline"], hash),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["receipt"]["revision"], 1);

    let (status, dashboard_label) = post(
        &app,
        "/own/plan",
        &serde_json::json!({
            "tenant": ACME,
            "request": {
                "baseRevision": 1,
                "terminology": {"pipelineLabels": {"qualified": "Bid"}}
            }
        }),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dashboard_label}");
    let (status, chat_label) = post(
        &app,
        "/own/chat",
        &chat_body("rename qualified to Bid"),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{chat_label}");
    assert_eq!(chat_label["plan"], dashboard_label["plan"]);
    assert_eq!(chat_label["plan"]["baseRevision"], 1);
    assert_eq!(
        chat_label["plan"]["config"]["terminology"]["pipelineLabels"]["qualified"],
        "Bid"
    );

    // The chat does not silently accept instructions it cannot type: an
    // unknown verb, an invented stage and a missing module are explicit 400s,
    // not empty plans.
    for message in ["paint the sky", "rename invented to Bid", "disable"] {
        let (status, refused) = post(&app, "/own/chat", &chat_body(message), &alice).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{message}: {refused}");
        assert_eq!(refused["reason"], "unrecognized_request");
    }
    assert_eq!(count(&fixture, "config_revisions").await, 1);
}

/// **Witness: the owner surface has no unauthenticated door.**
///
/// A missing session and a forged bearer are both refused before the owner
/// ledger is consulted, on the inspect, plan and chat routes. No revision is
/// written by any of them.
#[tokio::test]
async fn the_public_owner_surface_rejects_missing_and_forged_credentials() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);

    let (status, refused) = raw(
        &app,
        "GET",
        &format!("/own/config?tenant={ACME}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{refused}");
    let (status, refused) = raw(
        &app,
        "GET",
        &format!("/own/config?tenant={ACME}"),
        None,
        Some("forged-token"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{refused}");
    let (status, refused) = raw(
        &app,
        "POST",
        "/own/plan",
        Some(&plan_body(0, &[], &["crm-pipeline"])),
        Some("forged-token"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{refused}");
    let (status, refused) = raw(
        &app,
        "POST",
        "/own/chat",
        Some(&chat_body("disable crm-pipeline")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{refused}");
    assert_eq!(count(&fixture, "config_revisions").await, 0);
}

/// **Witness: a foreign tenant, an injected role and an injected approval are
/// refused before any write.**
#[tokio::test]
async fn a_foreign_tenant_role_or_approval_is_refused_without_writing() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    // Alice has no owner grant in globex-corp: the tenant is the body's, the
    // authority is the ledger's.
    let (status, refused) = post(
        &app,
        "/own/plan",
        &serde_json::json!({
            "tenant": "globex-corp",
            "request": {"baseRevision": 0, "disable": ["crm-pipeline"]}
        }),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert_eq!(refused["reason"], "missing_grant");

    // Role and approval are not fields of the typed request. The extractor
    // refuses them before the handler, so a caller cannot claim either.
    let mut role = plan_body(0, &[], &["crm-pipeline"]);
    role["role"] = serde_json::json!("owner");
    let (status, refused) = post(&app, "/own/plan", &role, &alice).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");

    let (_, planned) = post(
        &app,
        "/own/plan",
        &plan_body(0, &[], &["crm-pipeline"]),
        &alice,
    )
    .await;
    let hash = planned["plan"]["planHash"].as_str().unwrap();
    let mut approval = apply_body("own-foreign-role-1", 0, &[], &["crm-pipeline"], hash);
    approval["approval"] = serde_json::json!({"approved": true, "principal": "owner"});
    let (status, refused) = post(&app, "/own/apply", &approval, &alice).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(count(&fixture, "config_revisions").await, 0);
}

/// **A2/F04 — a confirmation names the catalogue it was shown under.**
///
/// The receipt already records `catalogueVersion`/`pricingVersion`, but nothing
/// compared them with what the confirming client holds. A client that confirms
/// against an older catalogue/pricing identity must be refused explicitly
/// instead of executing an answer the owner was never shown.
#[tokio::test]
async fn a_confirmation_from_a_previous_catalogue_or_pricing_refuses() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    let (_, planned) = post(
        &app,
        "/own/plan",
        &plan_body(0, &["crm-pipeline"], &[]),
        &alice,
    )
    .await;
    let hash = planned["plan"]["planHash"].as_str().unwrap().to_string();
    assert_eq!(
        planned["plan"]["catalogueVersion"],
        archava_host::own::CATALOGUE_VERSION,
        "the plan the owner sees must name its catalogue: {planned}"
    );
    assert_eq!(planned["plan"]["pricingVersion"], "included@1");

    // An older catalogue identity: refused, and nothing is written.
    let mut stale_catalogue = apply_body(
        "own-request-old-catalogue",
        0,
        &["crm-pipeline"],
        &[],
        &hash,
    );
    stale_catalogue["confirmation"]["catalogueVersion"] = serde_json::json!("owner-catalogue@0");
    let (status, refusal) = post(&app, "/own/apply", &stale_catalogue, &alice).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["reason"], "catalogue_changed");
    assert_eq!(refusal["detail"]["held"], "owner-catalogue@0");
    assert_eq!(
        refusal["detail"]["live"],
        archava_host::own::CATALOGUE_VERSION
    );
    assert_eq!(count(&fixture, "config_revisions").await, 0);

    // An older pricing identity: the same refusal, never a silent apply.
    let mut stale_pricing = apply_body("own-request-old-pricing", 0, &["crm-pipeline"], &[], &hash);
    stale_pricing["confirmation"]["pricingVersion"] = serde_json::json!("included@0");
    let (status, refusal) = post(&app, "/own/apply", &stale_pricing, &alice).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["reason"], "pricing_changed");
    assert_eq!(refusal["detail"]["held"], "included@0");
    assert_eq!(refusal["detail"]["live"], "included@1");
    assert_eq!(count(&fixture, "config_revisions").await, 0);

    // The SAME confirmation, carrying the live identities, applies — and the
    // receipt records them.
    let mut live = apply_body(
        "own-request-live-catalogue",
        0,
        &["crm-pipeline"],
        &[],
        &hash,
    );
    live["confirmation"]["catalogueVersion"] =
        serde_json::json!(archava_host::own::CATALOGUE_VERSION);
    live["confirmation"]["pricingVersion"] = serde_json::json!("included@1");
    let (status, applied) = post(&app, "/own/apply", &live, &alice).await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["receipt"]["revision"], 1);
    assert_eq!(
        applied["receipt"]["catalogueVersion"],
        archava_host::own::CATALOGUE_VERSION
    );
    assert_eq!(applied["receipt"]["pricingVersion"], "included@1");
}

/// **A3/F06 — restore lands the target label SET, deletions included.**
///
/// Revision 1 carries `won`; revision 2 adds `new`. Restoring revision 1 must
/// DELETE `new`, not merge it forward, and the delete must be a visible change
/// in the confirmed plan.
#[tokio::test]
async fn restore_removes_the_labels_the_target_snapshot_never_had() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    // Revision 1: the owner labels `won`.
    let won = serde_json::json!({
        "baseRevision": 0,
        "terminology": {"pipelineLabels": {"won": "Closed deal"}}
    });
    let (status, planned) = post(
        &app,
        "/own/plan",
        &serde_json::json!({"tenant": ACME, "request": won}),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    let hash = planned["plan"]["planHash"].as_str().unwrap().to_string();
    let (status, applied) = post(
        &app,
        "/own/apply",
        &serde_json::json!({
            "tenant": ACME,
            "requestId": "own-request-label-won",
            "request": won,
            "confirmation": {"planHash": hash, "baseRevision": 0}
        }),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["receipt"]["revision"], 1);

    // Revision 2 adds a second label on top of the first.
    let added = serde_json::json!({
        "baseRevision": 1,
        "terminology": {"pipelineLabels": {"new": "Inquiry"}}
    });
    let (_, planned) = post(
        &app,
        "/own/plan",
        &serde_json::json!({"tenant": ACME, "request": added}),
        &alice,
    )
    .await;
    let hash = planned["plan"]["planHash"].as_str().unwrap().to_string();
    let (status, applied) = post(
        &app,
        "/own/apply",
        &serde_json::json!({
            "tenant": ACME,
            "requestId": "own-request-label-new",
            "request": added,
            "confirmation": {"planHash": hash, "baseRevision": 1}
        }),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["receipt"]["revision"], 2);

    // Restoring revision 1 must propose the DELETE of `new` and land exactly
    // the snapshot's label set.
    let (status, preview) = post(
        &app,
        "/own/restore/plan",
        &serde_json::json!({"tenant": ACME, "targetRevision": 1}),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(
        preview["plan"]["config"]["terminology"]["pipelineLabels"],
        serde_json::json!({"won": "Closed deal"}),
        "the restore plan must land the snapshot's labels, not a merge: {preview}"
    );
    let changes = preview["plan"]["changes"].as_array().unwrap();
    assert!(
        changes
            .iter()
            .any(|change| change["kind"] == "removeStage" && change["stage"] == "new"),
        "restore must propose deleting `new`: {preview}"
    );
    assert_eq!(preview["plan"]["config"]["revision"], 3);

    let hash = preview["plan"]["planHash"].as_str().unwrap().to_string();
    let (status, restored) = post(
        &app,
        "/own/restore",
        &serde_json::json!({
            "tenant": ACME,
            "requestId": "own-request-restore-labels",
            "targetRevision": 1,
            "confirmation": {"planHash": hash, "baseRevision": 2}
        }),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["receipt"]["revision"], 3);
    assert_eq!(restored["receipt"]["outcome"], "restore");

    let (_, inspected) = get(&app, &format!("/own/config?tenant={ACME}"), &alice).await;
    assert_eq!(
        inspected["config"]["terminology"]["pipelineLabels"],
        serde_json::json!({"won": "Closed deal"}),
        "the applied restore must persist the deleted label: {inspected}"
    );
    let (_, history) = get(&app, &format!("/own/history?tenant={ACME}"), &alice).await;
    let revisions: Vec<i64> = history["revisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["revision"].as_i64().unwrap())
        .collect();
    assert_eq!(revisions, vec![3, 2, 1], "restore never rewrites history");
}

/// F05 — the browser can lose the ANSWER to an apply whose transaction
/// committed. The request id is the only handle that survives that loss, so
/// the owner surface needs a read that answers "what did THIS principal's
/// operation with this id do?" without executing anything.
#[tokio::test]
async fn a_committed_request_id_reads_back_its_receipt_after_a_lost_response() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    let (_, planned) = post(
        &app,
        "/own/plan",
        &plan_body(0, &[], &["crm-pipeline"]),
        &alice,
    )
    .await;
    let hash = planned["plan"]["planHash"].as_str().unwrap().to_string();
    let body = apply_body("own-request-lost-answer", 0, &[], &["crm-pipeline"], &hash);
    let (status, applied) = post(&app, "/own/apply", &body, &alice).await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["replayed"], false);

    let uri = format!("/own/status?tenant={ACME}&requestId=own-request-lost-answer");
    let (status, found) = get(&app, &uri, &alice).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the owner recovery route must exist: {found}"
    );
    assert_eq!(found["decision"], "receipt", "{found}");
    assert_eq!(
        found["replayed"], true,
        "reading a committed operation replays its stored outcome"
    );
    assert_eq!(
        found["receipt"], applied["receipt"],
        "the recovery read must return the stored receipt verbatim"
    );

    // Reading the outcome is not an effect: the same question twice still
    // leaves exactly one revision, one receipt and one audit row.
    let (status, again) = get(&app, &uri, &alice).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["receipt"], applied["receipt"]);
    assert_eq!(count(&fixture, "config_revisions").await, 1);
    assert_eq!(count(&fixture, "apply_receipts").await, 1);
    assert_eq!(count(&fixture, "config_audit").await, 1);
}

/// The recovery read answers `unknown` — not a refusal, not a 404 — for a
/// request id that never committed. Absence is not proof of failure: the
/// caller must keep the key unresolved instead of minting a second one.
#[tokio::test]
async fn an_uncommitted_request_id_answers_unknown_and_writes_nothing() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    let uri = format!("/own/status?tenant={ACME}&requestId=own-request-never-sent");
    let (status, found) = get(&app, &uri, &alice).await;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(found["decision"], "unknown", "{found}");
    assert_eq!(found["requestId"], "own-request-never-sent");
    assert_eq!(count(&fixture, "config_revisions").await, 0);
    assert_eq!(count(&fixture, "apply_receipts").await, 0);
}

/// The receipt ledger's key is `(tenant, request_id)`, so it is tenant-wide.
/// A second principal holding the same grants must not read — or replay — the
/// first principal's operation by naming its id, even when the plan she would
/// submit is byte-identical (the catalogue is deterministic).
#[tokio::test]
async fn another_principal_cannot_read_or_replay_a_foreign_request_id() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;
    let carol = login(&app, "carol").await;

    // Both owners plan the SAME (base 0) change before either commits, so the
    // two confirmations are byte-identical: nothing but the actor separates
    // Carol from Alice's receipt.
    let (_, planned) = post(
        &app,
        "/own/plan",
        &plan_body(0, &[], &["crm-pipeline"]),
        &alice,
    )
    .await;
    let hash = planned["plan"]["planHash"].as_str().unwrap().to_string();
    let (_, carol_planned) = post(
        &app,
        "/own/plan",
        &plan_body(0, &[], &["crm-pipeline"]),
        &carol,
    )
    .await;
    assert_eq!(
        carol_planned["plan"]["planHash"],
        hash.as_str(),
        "the two owners' plans must be identical: {carol_planned}"
    );

    let (status, applied) = post(
        &app,
        "/own/apply",
        &apply_body("own-request-alice-only", 0, &[], &["crm-pipeline"], &hash),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["receipt"]["revision"], 1);

    let uri = format!("/own/status?tenant={ACME}&requestId=own-request-alice-only");
    let (status, found) = get(&app, &uri, &carol).await;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(
        found["decision"], "unknown",
        "a foreign request id must read as unknown, not as a receipt: {found}"
    );

    let (status, refused) = post(
        &app,
        "/own/apply",
        &apply_body("own-request-alice-only", 0, &[], &["crm-pipeline"], &hash),
        &carol,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a foreign request id must not replay Alice's committed plan: {refused}"
    );
    assert_eq!(refused["reason"], "request_id_reused");

    // Alice's single effect is untouched and Carol added none.
    assert_eq!(count(&fixture, "config_revisions").await, 1);
    assert_eq!(count(&fixture, "apply_receipts").await, 1);
    let (_, history) = get(&app, &format!("/own/history?tenant={ACME}"), &alice).await;
    assert_eq!(history["revisions"][0]["actor"], "alice");
}

/// Configured labels are public CRM presentation, not Owner management authority.
#[tokio::test]
async fn crm_presentation_uses_current_labels_without_granting_owner_access() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;
    let bob = login(&app, "bob").await;
    let path = format!("/crm/records?tenant={ACME}&contact={ID_ADA}");
    let (status, initial) = get(&app, &path, &bob).await;
    assert_eq!(status, StatusCode::OK, "{initial}");
    assert_eq!(initial["pipelineLabels"], serde_json::json!({}));
    assert_eq!(initial["configurationRevision"], 0);
    let request = serde_json::json!({"baseRevision":0,"terminology":{"pipelineLabels":{"new":"Discovery <&>"}}});
    let (status, preview) = post(
        &app,
        "/own/plan",
        &serde_json::json!({"tenant":ACME,"request":request}),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    let (status, applied) = post(&app, "/own/apply", &serde_json::json!({"tenant":ACME,"requestId":"crm-label-review","request":request,"confirmation":{"planHash":preview["plan"]["planHash"],"baseRevision":0}}), &alice).await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    let (status, shown) = get(&app, &path, &bob).await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!(shown["pipelineLabels"]["new"], "Discovery <&>");
    assert_eq!(shown["configurationRevision"], 1);
    assert_eq!(shown["contact"], initial["contact"]);
    for secret in ["config", "grants", "permissions", "enabled", "ownerGrants"] {
        assert!(shown.get(secret).is_none());
    }
    assert_eq!(
        get(&app, &format!("/own/config?tenant={ACME}"), &bob)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    // Old responses still decode without requiring a newer server immediately.
    let mut older = shown.clone();
    older.as_object_mut().unwrap().remove("pipelineLabels");
    older
        .as_object_mut()
        .unwrap()
        .remove("configurationRevision");
    let decoded: archava_host::erp::crm::CrmRecordValue = serde_json::from_value(older).unwrap();
    assert!(decoded.pipeline_labels.is_empty());
    assert_eq!(decoded.configuration_revision, 0);
    let (status, foreign) = get(
        &app,
        &format!("/crm/records?tenant=unrelated-business&contact={ID_ADA}"),
        &bob,
    )
    .await;
    assert!(matches!(
        status,
        StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
    ));
    assert!(foreign.get("pipelineLabels").is_none());
    sqlx::query("UPDATE archava_crm.memberships SET scope='assigned' WHERE tenant_id=$1 AND principal_id='bob'").bind(ACME).execute(&fixture.admin_pool).await.unwrap();
    let (status, hidden) = get(&app, &path, &bob).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(hidden.get("pipelineLabels").is_none());
    sqlx::query(
        "UPDATE archava_crm.memberships SET active=false WHERE tenant_id=$1 AND principal_id='bob'",
    )
    .bind(ACME)
    .execute(&fixture.admin_pool)
    .await
    .unwrap();
    let (status, revoked) = get(&app, &path, &bob).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(revoked.get("pipelineLabels").is_none());
}

/// **Witness (OWN-T05, OWN-T07): disabling a module that another enabled
/// module requires is an explicit blocked plan on the installed catalogue, the
/// blocked plan cannot be applied, and nothing is written.** The requirements
/// come from the module registry, so every declared requirement is exercised.
#[tokio::test]
async fn disabling_a_required_module_is_blocked_and_nothing_is_written() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    let (status, planned) = post(
        &app,
        "/own/plan",
        &plan_body(0, &[], &["scheduling"]),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    let blocked = &planned["plan"]["blocked"];
    assert_eq!(blocked["reason"], "missing_dependency", "{planned}");
    assert_eq!(blocked["module"], "accommodation", "{planned}");
    assert!(
        blocked["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("scheduling")),
        "the refusal names the module that must stay enabled: {planned}"
    );
    // A blocked plan proposes nothing: it carries the unchanged configuration.
    assert_eq!(planned["plan"]["config"]["revision"], 0, "{planned}");

    let hash = planned["plan"]["planHash"].as_str().unwrap().to_string();
    let (status, refused) = post(
        &app,
        "/own/apply",
        &apply_body("own-request-dependency", 0, &[], &["scheduling"], &hash),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["reason"], "plan_blocked");

    // The chat grammar reaches the same refusal as the dashboard request.
    let (status, chatted) = post(&app, "/own/chat", &chat_body("disable scheduling"), &alice).await;
    assert_eq!(status, StatusCode::OK, "{chatted}");
    assert_eq!(
        chatted["plan"]["blocked"]["reason"], "missing_dependency",
        "{chatted}"
    );

    // Disabling the dependent together with its requirement is compatible.
    let (status, together) = post(
        &app,
        "/own/plan",
        &plan_body(0, &[], &["accommodation", "scheduling"]),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{together}");
    assert!(together["plan"]["blocked"].is_null(), "{together}");

    let mut exercised = 0;
    for module in archava_domain::modules::all() {
        for required in module.owner_requires {
            let (status, planned) =
                post(&app, "/own/plan", &plan_body(0, &[], &[required]), &alice).await;
            assert_eq!(status, StatusCode::OK, "{required}: {planned}");
            assert_eq!(
                planned["plan"]["blocked"]["reason"], "missing_dependency",
                "disabling {required} while {} is enabled: {planned}",
                module.id
            );
            exercised += 1;
        }
    }
    assert!(exercised >= 4, "the registry declares module requirements");
    assert_eq!(count(&fixture, "config_revisions").await, 0);
    assert_eq!(count(&fixture, "apply_receipts").await, 0);
}

/// Every owner route with a request that would otherwise be served: `(method,
/// uri, body)`.
fn owner_surface(plan_hash: &str) -> Vec<(&'static str, String, Option<serde_json::Value>)> {
    vec![
        ("GET", format!("/own/config?tenant={ACME}"), None),
        ("GET", format!("/own/history?tenant={ACME}"), None),
        (
            "GET",
            format!("/own/status?tenant={ACME}&requestId=own-request-surface"),
            None,
        ),
        (
            "POST",
            "/own/plan".into(),
            Some(plan_body(1, &["crm-pipeline"], &[])),
        ),
        (
            "POST",
            "/own/chat".into(),
            Some(chat_body("enable crm-pipeline")),
        ),
        (
            "POST",
            "/own/apply".into(),
            Some(apply_body(
                "own-request-refused",
                1,
                &["crm-pipeline"],
                &[],
                plan_hash,
            )),
        ),
        (
            "POST",
            "/own/restore/plan".into(),
            Some(serde_json::json!({"tenant": ACME, "targetRevision": 1})),
        ),
        (
            "POST",
            "/own/restore".into(),
            Some(serde_json::json!({
                "tenant": ACME,
                "requestId": "own-request-refused-restore",
                "targetRevision": 1,
                "confirmation": {"planHash": plan_hash, "baseRevision": 1}
            })),
        ),
    ]
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<&serde_json::Value>,
    session: &Session,
) -> (StatusCode, serde_json::Value) {
    match body {
        Some(body) => post(app, uri, body, session).await,
        None => {
            assert_eq!(method, "GET");
            get(app, uri, session).await
        }
    }
}

/// **Witness (OWN-T04): a principal who never held the owner grant, and one
/// whose grant was revoked after a committed apply, are refused on every owner
/// route — inspect, history, status, plan, chat, apply, restore preview and
/// restore — and nothing is written.**
#[tokio::test]
async fn a_never_granted_or_revoked_principal_is_refused_on_every_owner_route() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;
    let bob = login(&app, "bob").await;

    let (_, planned) = post(
        &app,
        "/own/plan",
        &plan_body(0, &[], &["crm-pipeline"]),
        &alice,
    )
    .await;
    let hash = planned["plan"]["planHash"].as_str().unwrap().to_string();
    let (status, applied) = post(
        &app,
        "/own/apply",
        &apply_body("own-request-surface", 0, &[], &["crm-pipeline"], &hash),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");

    // Every request is one Alice is served while she holds the grant.
    for (method, uri, body) in owner_surface(&hash)
        .into_iter()
        .filter(|(method, _, _)| *method == "GET")
    {
        let (status, served) = call(&app, method, &uri, body.as_ref(), &alice).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {served}");
    }

    for (method, uri, body) in owner_surface(&hash) {
        let (status, refused) = call(&app, method, &uri, body.as_ref(), &bob).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "never granted, {uri}: {refused}"
        );
        assert_eq!(refused["reason"], "missing_grant", "{uri}");
    }

    sqlx::query(
        "UPDATE archava_owner.owner_grants SET active=false \
         WHERE tenant_id=$1 AND principal_id='alice'",
    )
    .bind(ACME)
    .execute(&fixture.admin_pool)
    .await
    .expect("revoke the owner grant");
    for (method, uri, body) in owner_surface(&hash) {
        let (status, refused) = call(&app, method, &uri, body.as_ref(), &alice).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "revoked, {uri}: {refused}");
        assert_eq!(refused["reason"], "missing_grant", "{uri}");
    }
    assert_eq!(count(&fixture, "config_revisions").await, 1);
    assert_eq!(count(&fixture, "apply_receipts").await, 1);
}

/// **Witness (OWN-T04): a caller without a session, or with a forged bearer,
/// is refused on every owner route before anything is read or written.**
#[tokio::test]
async fn an_unauthenticated_caller_is_refused_on_every_owner_route() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);

    for bearer in [None, Some("forged-token")] {
        for (method, uri, body) in owner_surface("0".repeat(64).as_str()) {
            let (status, refused) = raw(&app, method, &uri, body.as_ref(), bearer).await;
            assert_eq!(
                status,
                StatusCode::UNAUTHORIZED,
                "{method} {uri} with bearer {bearer:?}: {refused}"
            );
        }
    }
    assert_eq!(count(&fixture, "config_revisions").await, 0);
    assert_eq!(count(&fixture, "apply_receipts").await, 0);
}

async fn rows(fixture: &Fixture, table: &str) -> Vec<String> {
    let sql = format!("SELECT t::text FROM {table} t ORDER BY 1");
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_all(&fixture.admin_pool)
        .await
        .unwrap_or_else(|error| panic!("read {table}: {error}"))
}

async fn relabel(app: &Router, session: &Session, request_id: &str, base: i64, label: &str) {
    let request = serde_json::json!({
        "baseRevision": base,
        "terminology": {"pipelineLabels": {"won": label}}
    });
    let (status, planned) = post(
        app,
        "/own/plan",
        &serde_json::json!({"tenant": ACME, "request": request}),
        session,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    let hash = planned["plan"]["planHash"].as_str().unwrap();
    let (status, applied) = post(
        app,
        "/own/apply",
        &serde_json::json!({
            "tenant": ACME,
            "requestId": request_id,
            "request": request,
            "confirmation": {"planHash": hash, "baseRevision": base}
        }),
        session,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
}

/// **Witness (OWN-T08): after a restore, the label each earlier revision
/// carried is still readable from history with the request and plan that
/// produced it, and the restore only appends — earlier revisions, receipts and
/// audit rows are byte-identical, and grants, memberships and business rows are
/// untouched.**
#[tokio::test]
async fn restore_keeps_historical_labels_traceable_and_only_appends() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    relabel(&app, &alice, "own-request-label-1", 0, "Closed deal").await;
    relabel(&app, &alice, "own-request-label-2", 1, "Signed").await;

    let append_only = [
        "archava_owner.config_revisions",
        "archava_owner.apply_receipts",
        "archava_owner.config_audit",
    ];
    let unchanged = [
        "archava_owner.owner_grants",
        "archava_crm.memberships",
        "archava_crm.contacts",
    ];
    let mut before = Vec::new();
    for table in append_only.iter().chain(&unchanged) {
        before.push(rows(&fixture, table).await);
    }
    assert!(before.iter().take(3).all(|rows| rows.len() >= 2));

    let (status, preview) = post(
        &app,
        "/own/restore/plan",
        &serde_json::json!({"tenant": ACME, "targetRevision": 1}),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    let hash = preview["plan"]["planHash"].as_str().unwrap();
    let (status, restored) = post(
        &app,
        "/own/restore",
        &serde_json::json!({
            "tenant": ACME,
            "requestId": "own-request-label-restore",
            "targetRevision": 1,
            "confirmation": {"planHash": hash, "baseRevision": 2}
        }),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["receipt"]["revision"], 3);

    let (status, history) = get(&app, &format!("/own/history?tenant={ACME}"), &alice).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    let revisions = history["revisions"].as_array().expect("history rows");
    let traced: Vec<(i64, &str, &str, &str)> = revisions
        .iter()
        .map(|row| {
            (
                row["revision"].as_i64().unwrap(),
                row["config"]["terminology"]["pipelineLabels"]["won"]
                    .as_str()
                    .unwrap_or_else(|| panic!("revision keeps its label: {row}")),
                row["requestId"].as_str().expect("request id"),
                row["outcome"].as_str().expect("outcome"),
            )
        })
        .collect();
    assert_eq!(
        traced,
        vec![
            (3, "Closed deal", "own-request-label-restore", "restore"),
            (2, "Signed", "own-request-label-2", "apply"),
            (1, "Closed deal", "own-request-label-1", "apply"),
        ],
        "{history}"
    );
    for row in revisions {
        assert_eq!(row["actor"], "alice", "{row}");
        assert!(row["planHash"]
            .as_str()
            .is_some_and(|hash| !hash.is_empty()));
    }

    for (index, table) in append_only.iter().enumerate() {
        let after = rows(&fixture, table).await;
        assert_eq!(
            after.len(),
            before[index].len() + 1,
            "{table} gains one row"
        );
        for row in &before[index] {
            assert!(after.contains(row), "{table} rewrote an earlier row: {row}");
        }
    }
    for (index, table) in unchanged.iter().enumerate() {
        assert_eq!(rows(&fixture, table).await, before[index + 3], "{table}");
    }
}

const GLOBEX: &str = "globex-corp";
const ID_GRACE: &str = "2c3e5a10-2222-4222-8222-0000000000b1";

/// A second business on the same database and the same router: Dana owns
/// `globex-corp`, with her own contact, and holds nothing in `acme-retail`.
async fn seed_second_business(fixture: &Fixture) {
    sqlx::query(
        "INSERT INTO archava_identity.users (id, username, password_hash, active, version) \
         VALUES ('dana', 'dana@archava.test', $1, true, 1)",
    )
    .bind(PASSWORD_HASH)
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the second owner");
    sqlx::query(
        "INSERT INTO archava_crm.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1,'dana','human',$2,'all',true)",
    )
    .bind(GLOBEX)
    .bind(vec![
        "contacts.read",
        "contacts.write",
        "pipeline.read",
        "pipeline.write",
    ])
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the second CRM membership");
    sqlx::query(
        "INSERT INTO archava_owner.owner_grants (tenant_id, principal_id, permissions, active) \
         VALUES ($1,'dana',ARRAY['config.read','config.apply'],true)",
    )
    .bind(GLOBEX)
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the second owner grant");
    sqlx::query(
        "INSERT INTO archava_crm.contacts (tenant_id,id,full_name,owner_id) \
         VALUES ($1,$2,'Grace Hopper','dana')",
    )
    .bind(GLOBEX)
    .bind(uuid::Uuid::parse_str(ID_GRACE).expect("a test uuid"))
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the second contact");
}

/// Plan and apply one owner request for `tenant`, returning the receipt.
async fn configure(
    app: &Router,
    session: &Session,
    tenant: &str,
    request_id: &str,
    request: serde_json::Value,
) -> serde_json::Value {
    let (status, planned) = post(
        app,
        "/own/plan",
        &serde_json::json!({"tenant": tenant, "request": request}),
        session,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    assert!(planned["plan"]["blocked"].is_null(), "{planned}");
    let (status, applied) = post(
        app,
        "/own/apply",
        &serde_json::json!({
            "tenant": tenant,
            "requestId": request_id,
            "request": request,
            "confirmation": {
                "planHash": planned["plan"]["planHash"],
                "baseRevision": request["baseRevision"]
            }
        }),
        session,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    applied["receipt"].clone()
}

/// **Witness (OWN-T01): two businesses run on the same binary, router and
/// catalogue, and differ only by the configuration each owner applied.** One
/// disables the pipeline and relabels a stage; the other keeps the pipeline
/// and uses its own label. Each sees only its own configuration and history,
/// neither owner can read or change the other's, and the same CRM command is
/// refused for one business and served for the other.
#[tokio::test]
async fn two_businesses_share_one_release_and_differ_only_by_owner_configuration() {
    let fixture = provision().await;
    seed(&fixture).await;
    seed_second_business(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;
    let dana = login(&app, "dana").await;

    let retail = configure(
        &app,
        &alice,
        ACME,
        "own-request-shared-id",
        serde_json::json!({
            "baseRevision": 0,
            "disable": ["crm-pipeline"],
            "terminology": {"pipelineLabels": {"won": "Closed deal"}}
        }),
    )
    .await;
    // The same request id in another tenant is a different operation.
    let studio = configure(
        &app,
        &dana,
        GLOBEX,
        "own-request-shared-id",
        serde_json::json!({
            "baseRevision": 0,
            "terminology": {"pipelineLabels": {"won": "Menang"}}
        }),
    )
    .await;
    assert_eq!(retail["revision"], 1);
    assert_eq!(studio["revision"], 1, "each business has its own revisions");
    assert_ne!(retail["planHash"], studio["planHash"]);

    let enabled = |config: &serde_json::Value, id: &str| {
        config["modules"]
            .as_array()
            .expect("catalogue modules")
            .iter()
            .find(|module| module["id"] == id)
            .unwrap_or_else(|| panic!("{id} is in the catalogue"))["enabled"]
            .clone()
    };
    let (status, acme) = get(&app, &format!("/own/config?tenant={ACME}"), &alice).await;
    assert_eq!(status, StatusCode::OK, "{acme}");
    let (status, globex) = get(&app, &format!("/own/config?tenant={GLOBEX}"), &dana).await;
    assert_eq!(status, StatusCode::OK, "{globex}");
    assert_eq!(enabled(&acme, "crm-pipeline"), false);
    assert_eq!(enabled(&globex, "crm-pipeline"), true);
    assert_eq!(
        acme["config"]["terminology"]["pipelineLabels"]["won"],
        "Closed deal"
    );
    assert_eq!(
        globex["config"]["terminology"]["pipelineLabels"]["won"],
        "Menang"
    );
    // One release: both businesses are offered the identical catalogue.
    let offered = |config: &serde_json::Value| -> Vec<String> {
        config["modules"]
            .as_array()
            .expect("catalogue modules")
            .iter()
            .map(|module| module["id"].as_str().expect("module id").to_string())
            .collect()
    };
    assert_eq!(offered(&acme), offered(&globex));
    assert_eq!(retail["catalogueVersion"], studio["catalogueVersion"]);
    assert!(retail["catalogueVersion"].is_string(), "{retail}");

    // The same command, the same binary: refused where the owner disabled the
    // capability, served where the owner kept it.
    let command = |tenant: &str, contact: &str, request_id: &str| {
        serde_json::json!({
            "tenant": tenant,
            "requestId": request_id,
            "command": {
                "operation": "opportunity.create",
                "contactId": contact,
                "contactRevision": 1,
                "title": "Renewal 2027"
            }
        })
    };
    let (status, refused) = post(
        &app,
        "/crm/commands",
        &command(ACME, ID_ADA, "00000000-0000-4000-8000-0000000000d1"),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert_eq!(refused["reason"], "module_disabled");
    let (status, accepted) = post(
        &app,
        "/crm/commands",
        &command(GLOBEX, ID_GRACE, "00000000-0000-4000-8000-0000000000d2"),
        &dana,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert_eq!(accepted["receipt"]["kind"], "opportunity");

    // Neither owner reaches the other business.
    for (session, tenant) in [(&alice, GLOBEX), (&dana, ACME)] {
        for uri in [
            format!("/own/config?tenant={tenant}"),
            format!("/own/history?tenant={tenant}"),
            format!("/own/status?tenant={tenant}&requestId=own-request-shared-id"),
        ] {
            let (status, refused) = get(&app, &uri, session).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{uri}: {refused}");
            assert_eq!(refused["reason"], "missing_grant", "{uri}");
        }
        let (status, refused) = post(
            &app,
            "/own/plan",
            &serde_json::json!({"tenant": tenant, "request": {"baseRevision": 1, "enable": ["crm-pipeline"]}}),
            session,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    }
    let (_, history) = get(&app, &format!("/own/history?tenant={GLOBEX}"), &dana).await;
    let revisions = history["revisions"].as_array().expect("history rows");
    assert_eq!(revisions.len(), 1, "{history}");
    assert_eq!(revisions[0]["actor"], "dana");
    assert_eq!(count(&fixture, "config_revisions").await, 2);
    assert_eq!(count(&fixture, "apply_receipts").await, 2);
}

/// **Witness (OWN-T03, OWN-T05): on the installed catalogue the preview names
/// each change with its readiness and cost classification under the server's
/// catalogue and pricing identity, previewing writes nothing, and a module the
/// catalogue does not offer is an explicit blocked result that cannot be
/// applied.**
#[tokio::test]
async fn the_preview_states_changes_readiness_and_cost_and_writes_nothing() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    configure(
        &app,
        &alice,
        ACME,
        "own-request-preview-setup",
        serde_json::json!({"baseRevision": 0, "disable": ["accommodation", "laundry"]}),
    )
    .await;
    let tables = [
        "archava_owner.config_revisions",
        "archava_owner.apply_receipts",
        "archava_owner.config_audit",
    ];
    let mut before = Vec::new();
    for table in tables {
        before.push(rows(&fixture, table).await);
    }

    let (status, planned) = post(
        &app,
        "/own/plan",
        &plan_body(1, &["accommodation", "laundry"], &[]),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    let plan = &planned["plan"];
    assert!(plan["blocked"].is_null(), "{planned}");
    assert_eq!(plan["baseRevision"], 1);
    assert_eq!(plan["config"]["revision"], 2);
    assert_eq!(
        plan["catalogueVersion"],
        archava_host::own::CATALOGUE_VERSION
    );
    assert_eq!(plan["pricingVersion"], archava_host::own::PRICING_VERSION);
    let changes = plan["changes"].as_array().expect("changes");
    assert_eq!(
        changes.len(),
        2,
        "one change per requested module: {planned}"
    );
    let effects = plan["effects"].as_array().expect("effects");
    for module in ["accommodation", "laundry"] {
        assert!(
            changes
                .iter()
                .any(|change| change.to_string().contains(module)),
            "{module} is named among the changes: {planned}"
        );
        let effect = effects
            .iter()
            .find(|effect| effect["module"] == module)
            .unwrap_or_else(|| panic!("{module} has a stated effect: {planned}"));
        assert_eq!(effect["readiness"], "ready", "{planned}");
        assert_eq!(effect["cost"], "included", "{planned}");
    }

    let (status, unknown) = post(&app, "/own/plan", &plan_body(1, &["payroll"], &[]), &alice).await;
    assert_eq!(status, StatusCode::OK, "{unknown}");
    assert_eq!(unknown["plan"]["blocked"]["reason"], "unknown_module");
    assert_eq!(unknown["plan"]["blocked"]["module"], "payroll");
    assert_eq!(
        unknown["plan"]["config"]["revision"], 1,
        "nothing is proposed"
    );
    let hash = unknown["plan"]["planHash"].as_str().unwrap();
    let (status, refused) = post(
        &app,
        "/own/apply",
        &apply_body("own-request-unknown", 1, &["payroll"], &[], hash),
        &alice,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["reason"], "plan_blocked");

    for (index, table) in tables.iter().enumerate() {
        assert_eq!(rows(&fixture, table).await, before[index], "{table}");
    }
}

/// **Witness (OWN-T02): a request typed in chat and the same request made on
/// the dashboard are one plan.** A business sentence that enables several
/// modules and disables another, and two requests the host blocks, each give
/// the identical plan, base configuration, catalogue and pricing identity by
/// both routes, and neither route writes anything.
#[tokio::test]
async fn chat_and_dashboard_agree_on_business_sentences_and_blocked_plans() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let alice = login(&app, "alice").await;

    configure(
        &app,
        &alice,
        ACME,
        "own-request-chat-setup",
        serde_json::json!({"baseRevision": 0, "disable": ["accommodation", "laundry"]}),
    )
    .await;
    let tables = [
        "archava_owner.config_revisions",
        "archava_owner.apply_receipts",
        "archava_owner.config_audit",
    ];
    let mut before = Vec::new();
    for table in tables {
        before.push(rows(&fixture, table).await);
    }

    for (message, blocked) in [
        (
            "Bisnis saya hotel, tambahkan laundry tanpa restaurant",
            None,
        ),
        ("disable crm-contacts", Some("mandatory")),
        ("disable commerce", Some("missing_dependency")),
    ] {
        let (status, chat) = post(&app, "/own/chat", &chat_body(message), &alice).await;
        assert_eq!(status, StatusCode::OK, "{message}: {chat}");
        // The dashboard sends the request the chat parsed, as its own request.
        let (status, dashboard) = post(
            &app,
            "/own/plan",
            &serde_json::json!({"tenant": ACME, "request": chat["request"]}),
            &alice,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{message}: {dashboard}");
        for key in ["plan", "baseConfig", "catalogueVersion", "pricingVersion"] {
            assert_eq!(chat[key], dashboard[key], "{message}: {key}");
        }
        assert_eq!(chat["plan"]["baseRevision"], 1, "{message}");
        match blocked {
            Some(reason) => assert_eq!(chat["plan"]["blocked"]["reason"], reason, "{chat}"),
            None => {
                assert!(chat["plan"]["blocked"].is_null(), "{chat}");
                let changed = |kind: &str| -> Vec<&str> {
                    chat["plan"]["changes"]
                        .as_array()
                        .expect("the plan lists its changes")
                        .iter()
                        .filter(|change| change["kind"] == kind)
                        .filter_map(|change| change["module"].as_str())
                        .collect()
                };
                // Scheduling stayed enabled, so the hotel adds only itself.
                assert_eq!(changed("enable"), ["accommodation", "laundry"], "{chat}");
                assert_eq!(changed("disable"), ["foodservice"], "{chat}");
            }
        }
    }

    for (index, table) in tables.iter().enumerate() {
        assert_eq!(rows(&fixture, table).await, before[index], "{table}");
    }
}
