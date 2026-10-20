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
    let outcome: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
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

fn plan_body(base: i64, enable: &[&str], disable: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "tenant": ACME,
        "request": {"baseRevision": base, "enable": enable, "disable": disable}
    })
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
