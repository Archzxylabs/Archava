//! Regression test for Cockpit L4 Human-in-the-Loop Holding Pattern and Signal Resume.

use archava_host::{router, AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;

const KEY: &[u8] = &[8u8; 32];
const PASSWORD: &str = "correct horse battery staple";
const HASH: &str =
    "scrypt$32768$8$3$OZtInKxSaSZqbEptj2fvUg$jU_gx2rQW-69EYpbRoMKGSgclhV-mO7gXVF-QYvSXVo";

fn ledger() -> Ledger {
    Ledger {
        namespace: "archava_commerce",
        ledger: "Commerce",
        tables: &["memberships"],
    }
}

async fn state(f: &Fixture, pool: sqlx::PgPool) -> AppState {
    AppState::try_new(pool, ledger(), &f.commerce_ledger, &f.identity_ledger, KEY)
        .await
        .unwrap()
}

async fn setup() -> (Fixture, AppState, Router, Value) {
    let f = provision().await;
    for (id, tenant, capability, scope) in [
        ("alice", "acme", "enterprise", "all"),
        ("bob", "acme", "enterprise", "all"),
        ("mallory", "other", "enterprise", "all"),
    ] {
        sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES($1,$2,$3,true,1)")
            .bind(id)
            .bind(format!("{id}@archava.test"))
            .bind(HASH)
            .execute(&f.admin_pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope,active,role,capability) VALUES($1,$2,'human','{\"catalog.read\",\"orders.read\",\"stock.read\"}',$3,true,'owner',$4)")
            .bind(tenant)
            .bind(id)
            .bind(scope)
            .bind(capability)
            .execute(&f.admin_pool)
            .await
            .unwrap();
    }

    let state = state(&f, f.runtime_pool.clone()).await;
    let app = router(state.clone());
    let session = login(&app, "alice").await;
    (f, state, app, session)
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    session: Option<&Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header("x-archava-client", "cli");
    if let Some(s) = session {
        request = request
            .header(
                "authorization",
                format!("Bearer {}", s["token"].as_str().unwrap()),
            )
            .header("x-archava-csrf", s["csrf"].as_str().unwrap());
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn login(app: &Router, user: &str) -> Value {
    let (status, value) = call(
        app,
        "POST",
        "/login",
        json!({"username": format!("{user}@archava.test"), "password": PASSWORD}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    value
}

#[tokio::test]
async fn test_cockpit_holding_pattern_and_resume_lifecycle() {
    let (f, _state, app, session) = setup().await;

    // 1. Create a mission with auto_launch = true (status -> in_flight)
    let (status, create_json) = call(
        &app,
        "POST",
        "/api/v1/cockpit/missions",
        json!({
            "tenant_id": "acme",
            "mission_code": "MSN-HOLD-01",
            "title": "Invoice Reconciliation & Payout",
            "description": "Requires CFO approval before payout",
            "auto_launch": true,
            "steps": [
                {
                    "step_index": 1,
                    "step_name": "Read Commerce Workspace",
                    "tool_name": "commerce.workspace.read",
                    "payload_input": {},
                    "depends_on": []
                }
            ]
        }),
        Some(&session),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let mission_id = create_json["id"].as_str().unwrap();
    assert_eq!(create_json["status"], "in_flight");

    // 2. Pause into Holding Pattern awaiting human confirmation
    let (status, hold_json) = call(
        &app,
        "POST",
        &format!("/api/v1/cockpit/missions/{mission_id}/hold?tenant=acme"),
        json!({
            "reason": "Payout exceeds 50M IDR threshold; awaiting CFO signature",
            "signal_key": "cfo-approval-sig-9988"
        }),
        Some(&session),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(hold_json["status"], "holding_pattern");
    assert_eq!(
        hold_json["holding_reason"],
        "Payout exceeds 50M IDR threshold; awaiting CFO signature"
    );
    assert_eq!(hold_json["holding_signal_key"], "cfo-approval-sig-9988");

    // 3. Step execution while in holding pattern is REFUSED
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/cockpit/missions/{mission_id}/run-next?tenant=acme"),
        json!({ "stepIndex": 1 }),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // 4. Resume with wrong signal key is REFUSED
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/cockpit/missions/{mission_id}/resume?tenant=acme"),
        json!({ "signal_key": "wrong-key" }),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // 5. Resume with correct signal key succeeds -> back to in_flight
    let (status, res_json) = call(
        &app,
        "POST",
        &format!("/api/v1/cockpit/missions/{mission_id}/resume?tenant=acme"),
        json!({ "signal_key": "cfo-approval-sig-9988" }),
        Some(&session),
    )
    .await;
    println!("hold_status: {:?}, hold_json: {:?}", status, hold_json);
    assert_eq!(status, StatusCode::OK);
    assert_eq!(res_json["status"], "in_flight");
    assert!(res_json["holding_reason"].is_null());
    assert!(res_json["holding_signal_key"].is_null());

    // 6. Verify in database
    let db_row: (String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT status, holding_reason, holding_signal_key FROM archava_cockpit.missions WHERE id = $1",
    )
    .bind(uuid::Uuid::parse_str(mission_id).unwrap())
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();

    assert_eq!(db_row.0, "in_flight");
    assert_eq!(db_row.1, None);
    assert_eq!(db_row.2, None);

    // 7. Step 1 can now run to completion via registered application read tool
    let (status, run_json) = call(
        &app,
        "POST",
        &format!("/api/v1/cockpit/missions/{mission_id}/run-next?tenant=acme"),
        json!({ "stepIndex": 1 }),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(run_json["status"], "touchdown");
}

#[tokio::test]
async fn test_cockpit_l4_self_resume_refusal_and_independent_approval() {
    let (_f, _state, app, alice) = setup().await;
    let bob = login(&app, "bob").await;

    // 1. Create a mission as Alice with L4 authority
    let (status, create_json) = call(
        &app,
        "POST",
        "/api/v1/cockpit/missions",
        json!({
            "tenant_id": "acme",
            "mission_code": "MSN-L4-01",
            "title": "High-Value Treasury Reallocation",
            "description": "Requires independent L4 supervisor signoff",
            "auto_launch": true,
            "required_authority": "L4",
            "steps": [
                {
                    "step_index": 1,
                    "step_name": "Read Workspace",
                    "tool_name": "commerce.workspace.read",
                    "payload_input": {},
                    "depends_on": []
                }
            ]
        }),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let mission_id = create_json["id"].as_str().unwrap();

    // 2. Put mission into holding pattern
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/cockpit/missions/{mission_id}/hold?tenant=acme"),
        json!({
            "reason": "L4 treasury transaction threshold hit",
            "signal_key": "l4-treasury-sig-5544"
        }),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 3. Supervisor (Bob) does not see Alice's mission in his own private worklist (privacy preserved)
    let (list_status, list_json) = call(
        &app,
        "GET",
        "/api/v1/cockpit/missions?tenant=acme",
        json!({}),
        Some(&bob),
    )
    .await;
    assert_eq!(list_status, StatusCode::OK);
    let items = list_json["items"].as_array().unwrap();
    assert!(
        items.is_empty(),
        "Private mission list must remain isolated to the initiator"
    );

    // 4. Initiator (Alice) attempting self-resume on L4 mission is REFUSED (409 Conflict)
    let (status_self, body_self) = call(
        &app,
        "POST",
        &format!("/api/v1/cockpit/missions/{mission_id}/resume?tenant=acme"),
        json!({ "signal_key": "l4-treasury-sig-5544" }),
        Some(&alice),
    )
    .await;
    assert_eq!(
        status_self,
        StatusCode::CONFLICT,
        "Self-resume on L4 mission must be refused: {body_self}"
    );

    // 4. Independent supervisor (Bob) in acme resumes with valid signal key -> SUCCEEDS
    let (status_bob, body_bob) = call(
        &app,
        "POST",
        &format!("/api/v1/cockpit/missions/{mission_id}/resume?tenant=acme"),
        json!({ "signal_key": "l4-treasury-sig-5544" }),
        Some(&bob),
    )
    .await;
    assert_eq!(
        status_bob,
        StatusCode::OK,
        "Independent supervisor must be allowed to resume: {body_bob}"
    );
    assert_eq!(body_bob["status"], "in_flight");
}

#[tokio::test]
async fn test_cockpit_step_approval_gate_lifecycle_and_non_self_approval() {
    let (_f, _state, app, alice) = setup().await;
    let bob = login(&app, "bob").await;

    // 1. Create a mission as Alice with step 1 requiring L4 authority
    let (status, create_json) = call(
        &app,
        "POST",
        "/api/v1/cockpit/missions",
        json!({
            "tenant_id": "acme",
            "mission_code": "MSN-STEP-L4-01",
            "title": "Autonomous Supplier Settlement",
            "description": "Settlement step requires independent supervisor approval",
            "auto_launch": true,
            "steps": [
                {
                    "step_index": 1,
                    "step_name": "Read Workspace",
                    "tool_name": "commerce.workspace.read",
                    "payload_input": {},
                    "depends_on": [],
                    "required_authority": "L4"
                }
            ]
        }),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let mission_id = create_json["id"].as_str().unwrap();
    assert_eq!(create_json["steps"][0]["status"], "requires_approval");

    // 2. Calling run-next before approval is REFUSED (409 Conflict)
    let (status_run, run_refused) = call(
        &app,
        "POST",
        &format!("/api/v1/cockpit/missions/{mission_id}/run-next?tenant=acme"),
        json!({ "stepIndex": 1 }),
        Some(&alice),
    )
    .await;
    assert_eq!(
        status_run,
        StatusCode::CONFLICT,
        "Execution before approval must be refused: {run_refused}"
    );

    // 3. Calling complete before approval is REFUSED (409 Conflict)
    let (status_comp, comp_refused) = call(
        &app,
        "POST",
        &format!("/api/v1/cockpit/missions/{mission_id}/steps/1/complete?tenant=acme"),
        json!({
            "agent_role": "settlement_agent",
            "success": true,
            "high_signal_summary": "attempting unauthorized completion",
            "output_payload": {},
            "token_cost": 100
        }),
        Some(&alice),
    )
    .await;
    assert_eq!(
        status_comp,
        StatusCode::CONFLICT,
        "Step completion before approval must be refused: {comp_refused}"
    );

    // 4. Initiator (Alice) attempting self-approval on L4 step is REFUSED (409 Conflict)
    let (status_self_app, self_app_body) = call(
        &app,
        "POST",
        &format!("/api/v1/cockpit/missions/{mission_id}/steps/1/approve?tenant=acme"),
        json!({ "reason": "Self-authorizing payout" }),
        Some(&alice),
    )
    .await;
    assert_eq!(
        status_self_app,
        StatusCode::CONFLICT,
        "Initiator self-approval must be refused on L4 step: {self_app_body}"
    );

    // 5. Independent supervisor (Bob) approves step 1 -> SUCCEEDS (200 OK)
    let (status_bob_app, bob_app_body) = call(
        &app,
        "POST",
        &format!("/api/v1/cockpit/missions/{mission_id}/steps/1/approve?tenant=acme"),
        json!({ "reason": "Verified invoice and signed off" }),
        Some(&bob),
    )
    .await;
    assert_eq!(
        status_bob_app,
        StatusCode::OK,
        "Supervisor approval must succeed: {bob_app_body}"
    );
    assert_eq!(bob_app_body["steps"][0]["status"], "pending");
    assert_eq!(
        bob_app_body["steps"][0]["approval_gate"]["status"],
        "approved"
    );

    // 6. Now step 1 is pending and can be executed via run-next -> SUCCEEDS
    let (status_run_ok, run_ok_body) = call(
        &app,
        "POST",
        &format!("/api/v1/cockpit/missions/{mission_id}/run-next?tenant=acme"),
        json!({ "stepIndex": 1 }),
        Some(&alice),
    )
    .await;
    assert_eq!(
        status_run_ok,
        StatusCode::OK,
        "Execution after approval must succeed: {run_ok_body}"
    );
    assert_eq!(run_ok_body["status"], "touchdown");
    assert_eq!(run_ok_body["steps"][0]["status"], "passed");
}
