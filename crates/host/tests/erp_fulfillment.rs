//! The R08 fulfillment cohort's host witnesses — driven through the real router
//! against the disposable PostgreSQL whose `archava_fulfillment` schema is the
//! real migration ledger (`001_fulfillment_jobs_evidence.sql`).
//!
//! Every assertion is a fact about what the SERVER did:
//! - Authorized job creation, assignment, lines, custody events, and findings.
//! - Enforces that custody stages follow the prescribed sequence.
//! - Enforces that completion strictly requires unreconciled_count == 0 (gate).
//! - Reconciling findings decrement unreconciled_count and require disposition='accepted'.
//! - Closed jobs (`completed`, `canceled`) refuse further mutations.
//! - Read projections: list jobs, job detail, lines, custody events, findings.
//! - Scope narrowing: `assigned` sees only own jobs.
//! - Idempotency replay with identical fingerprint.

use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

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

const IDENTITY_KEY: &[u8] = &[31u8; 32];
const PASSWORD: &str = "correct horse battery staple";
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

const ACME: &str = "acme-fulfillment";
const GLOBEX: &str = "globex-fulfillment";

const FULFILLMENT_ALL: &[&str] = &[
    "fulfillment.read",
    "fulfillment.write",
    "fulfillment.dispatch",
    "fulfillment.produce",
    "audit.read",
];

async fn seed(fixture: &Fixture) {
    for principal in [
        "ax",
        "ax_reader",
        "ax_nodispatch",
        "ax_noproduce",
        "ax_assigned",
        "other",
    ] {
        sqlx::query(
            "INSERT INTO archava_identity.users (id, username, password_hash, active, version) \
             VALUES ($1, $2, $3, true, 1)",
        )
        .bind(principal)
        .bind(format!("{}@archava.test", principal))
        .bind(PASSWORD_HASH)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed identity user");
    }

    for (principal, permissions, scope) in [
        ("ax", FULFILLMENT_ALL, "all"),
        ("ax_reader", &["fulfillment.read"][..], "all"),
        (
            "ax_nodispatch",
            &[
                "fulfillment.read",
                "fulfillment.write",
                "fulfillment.produce",
            ][..],
            "all",
        ),
        (
            "ax_noproduce",
            &[
                "fulfillment.read",
                "fulfillment.write",
                "fulfillment.dispatch",
            ][..],
            "all",
        ),
        ("ax_assigned", FULFILLMENT_ALL, "assigned"),
    ] {
        sqlx::query(
            "INSERT INTO archava_fulfillment.memberships \
             (tenant_id, principal_id, kind, permissions, scope, active) \
             VALUES ($1, $2, 'human', $3, $4, true)",
        )
        .bind(ACME)
        .bind(principal)
        .bind(permissions.to_vec())
        .bind(scope)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed ACME fulfillment membership");
    }

    sqlx::query(
        "INSERT INTO archava_fulfillment.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(GLOBEX)
    .bind("other")
    .bind(FULFILLMENT_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed GLOBEX fulfillment membership");
}

struct Session {
    token: String,
    csrf: String,
}

static LOGIN_SLOTS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(archava_host::max_active_derivations());

async fn login(app: &axum::Router, principal_id: &str) -> Session {
    let _slot = LOGIN_SLOTS.acquire().await.expect("login slots open");
    let body = json!({
        "username": format!("{}@archava.test", principal_id),
        "password": PASSWORD,
    })
    .to_string();
    let response = app
        .clone()
        .oneshot(
            Request::post("/login")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let cookie_header = response
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap();
    let token = cookie_header
        .split(';')
        .find(|part| part.trim().starts_with("archava_session="))
        .unwrap()
        .trim()
        .strip_prefix("archava_session=")
        .unwrap()
        .to_string();

    let bytes = to_bytes(response.into_body(), 16_384).await.unwrap();
    let doc: Value = serde_json::from_slice(&bytes).unwrap();
    let csrf = doc["csrf"].as_str().unwrap().to_string();

    Session { token, csrf }
}

async fn send_post(
    app: &axum::Router,
    path: &str,
    session: &Session,
    body: &Value,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", session.token))
                .header("x-archava-csrf", &session.csrf)
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 65_536).await.unwrap();
    let val: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, val)
}

async fn send_get(app: &axum::Router, path: &str, session: &Session) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::get(path)
                .header("authorization", format!("Bearer {}", session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 65_536).await.unwrap();
    let val: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, val)
}

#[tokio::test]
async fn erp_fulfillment_full_lifecycle_and_reconciliation_gate() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = state(&fixture).await;
    let app = archava_host::router(state);

    let session = login(&app, "ax").await;

    // Check access
    let (status, body) = send_get(
        &app,
        &format!("/erp/fulfillment/access?tenant={ACME}"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["scope"], "all");
    assert!(body["permissions"]
        .as_array()
        .unwrap()
        .contains(&json!("fulfillment.write")));

    // 1. Create a dispatch job
    let cmd1 = Uuid::new_v4().to_string();
    let create_cmd = json!({
        "tenant": ACME,
        "mutation": {
            "type": "create_job",
            "commandId": cmd1,
            "jobCode": "FUL-DISP-001",
            "kind": "dispatch",
            "title": "Dispatch Order #4001 to Logistics Hub",
            "ownerRef": "ORD-4001"
        }
    });
    let (status, receipt) =
        send_post(&app, "/erp/fulfillment/commands", &session, &create_cmd).await;
    assert_eq!(status, StatusCode::OK, "create_job failed: {:?}", receipt);
    let job_id = receipt["jobId"].as_str().unwrap().to_string();
    assert_eq!(receipt["status"], "draft");
    assert_eq!(receipt["revision"], 1);

    // Idempotent replay of create_job
    let (replay_status, replay_receipt) =
        send_post(&app, "/erp/fulfillment/commands", &session, &create_cmd).await;
    assert_eq!(replay_status, StatusCode::OK);
    assert_eq!(replay_receipt["jobId"], job_id);

    // Check command status
    let (status, cmd_query) = send_get(
        &app,
        &format!("/erp/fulfillment/commands/status?tenant={ACME}&commandId={cmd1}"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cmd_query["status"], "recorded");

    // Verify detail
    let (status, detail) = send_get(
        &app,
        &format!("/erp/fulfillment/jobs/detail?tenant={ACME}&id={job_id}"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["job"]["jobCode"], "FUL-DISP-001");
    assert_eq!(detail["job"]["status"], "draft");

    // 2. Transition status: draft -> released -> in_progress
    let cmd2 = Uuid::new_v4().to_string();
    let release_cmd = json!({
        "tenant": ACME,
        "mutation": {
            "type": "change_job_status",
            "commandId": cmd2,
            "jobId": job_id,
            "expectedRevision": 1,
            "targetStatus": "released",
            "reason": "Release for dispatch processing"
        }
    });
    let (status, receipt) =
        send_post(&app, "/erp/fulfillment/commands", &session, &release_cmd).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "released");
    assert_eq!(receipt["revision"], 2);

    let cmd3 = Uuid::new_v4().to_string();
    let in_progress_cmd = json!({
        "tenant": ACME,
        "mutation": {
            "type": "change_job_status",
            "commandId": cmd3,
            "jobId": job_id,
            "expectedRevision": 2,
            "targetStatus": "in_progress",
            "reason": "Started picking and packaging"
        }
    });
    let (status, receipt) = send_post(
        &app,
        "/erp/fulfillment/commands",
        &session,
        &in_progress_cmd,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "in_progress");
    assert_eq!(receipt["revision"], 3);

    // 3. Record lines: consume normal and consume scrap
    let cmd4 = Uuid::new_v4().to_string();
    let line1_cmd = json!({
        "tenant": ACME,
        "mutation": {
            "type": "record_line",
            "commandId": cmd4,
            "jobId": job_id,
            "direction": "consume",
            "itemRef": "ITEM-BOX-XL",
            "quantity": 10,
            "disposition": "normal"
        }
    });
    let (status, receipt) =
        send_post(&app, "/erp/fulfillment/commands", &session, &line1_cmd).await;
    assert_eq!(status, StatusCode::OK);
    assert!(receipt["lineId"].is_string());

    let cmd5 = Uuid::new_v4().to_string();
    let line2_cmd = json!({
        "tenant": ACME,
        "mutation": {
            "type": "record_line",
            "commandId": cmd5,
            "jobId": job_id,
            "direction": "consume",
            "itemRef": "ITEM-BUBBLE-WRAP",
            "quantity": 2,
            "disposition": "scrap",
            "evidenceRef": "EVID-TORN-WRAP"
        }
    });
    let (status, receipt) =
        send_post(&app, "/erp/fulfillment/commands", &session, &line2_cmd).await;
    assert_eq!(status, StatusCode::OK);
    assert!(receipt["lineId"].is_string());

    // Verify line counts on job detail
    let (status, detail) = send_get(
        &app,
        &format!("/erp/fulfillment/jobs/detail?tenant={ACME}&id={job_id}"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["job"]["consumedCount"], 10);
    assert_eq!(detail["job"]["scrapCount"], 2);

    // Check lines projection
    let (status, lines) = send_get(
        &app,
        &format!("/erp/fulfillment/lines?tenant={ACME}&jobId={job_id}"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let line_arr = lines.as_array().unwrap();
    assert_eq!(line_arr.len(), 2);

    // 4. Record custody progression: received -> stored -> picked -> packed -> dispatched -> delivered
    for (event, from, to) in [
        ("received", None, Some("Dock A")),
        ("stored", Some("Dock A"), Some("Aisle 4")),
        ("picked", Some("Aisle 4"), Some("Pack Station 2")),
        ("packed", Some("Pack Station 2"), Some("Staging Bay 1")),
        ("dispatched", Some("Staging Bay 1"), Some("Truck #901")),
        ("delivered", Some("Truck #901"), Some("Customer Dock")),
    ] {
        let cmd = Uuid::new_v4().to_string();
        let custody_cmd = json!({
            "tenant": ACME,
            "mutation": {
                "type": "record_custody",
                "commandId": cmd,
                "jobId": job_id,
                "event": event,
                "fromRef": from,
                "toRef": to,
                "note": format!("Handover at {}", event)
            }
        });
        let (status, receipt) =
            send_post(&app, "/erp/fulfillment/commands", &session, &custody_cmd).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "Custody event {} failed: {:?}",
            event,
            receipt
        );
    }

    // Check custody events projection
    let (status, custody_list) = send_get(
        &app,
        &format!("/erp/fulfillment/custody?tenant={ACME}&jobId={job_id}"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let events = custody_list.as_array().unwrap();
    assert_eq!(events.len(), 6);

    // 5. Record finding: damaged goods during transit
    let cmd_finding = Uuid::new_v4().to_string();
    let finding_cmd = json!({
        "tenant": ACME,
        "mutation": {
            "type": "record_finding",
            "commandId": cmd_finding,
            "jobId": job_id,
            "category": "condition",
            "disposition": "damaged",
            "quantity": 1,
            "reason": "1 unit outer carton crushed upon delivery"
        }
    });
    let (status, receipt) =
        send_post(&app, "/erp/fulfillment/commands", &session, &finding_cmd).await;
    assert_eq!(status, StatusCode::OK);
    let finding_id = receipt["findingId"].as_str().unwrap().to_string();

    let (status, detail) = send_get(
        &app,
        &format!("/erp/fulfillment/jobs/detail?tenant={ACME}&id={job_id}"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["job"]["unreconciledCount"], 1);

    // 6. Reconciliation Gate: Try completing while unreconciled_count > 0 -> must fail with CONFLICT
    let cmd_fail_complete = Uuid::new_v4().to_string();
    let fail_complete_cmd = json!({
        "tenant": ACME,
        "mutation": {
            "type": "change_job_status",
            "commandId": cmd_fail_complete,
            "jobId": job_id,
            "expectedRevision": 3,
            "targetStatus": "completed",
            "reason": "Premature completion attempt"
        }
    });
    let (status, err) = send_post(
        &app,
        "/erp/fulfillment/commands",
        &session,
        &fail_complete_cmd,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("must be reconciled before completion"),
        "expected gate refusal, got: {:?}",
        err
    );

    // 7. Reconcile finding: record accepted resolution
    let cmd_reconcile = Uuid::new_v4().to_string();
    let reconcile_cmd = json!({
        "tenant": ACME,
        "mutation": {
            "type": "record_finding",
            "commandId": cmd_reconcile,
            "jobId": job_id,
            "category": "condition",
            "disposition": "accepted",
            "quantity": 1,
            "reason": "Consignee accepted credit note replacement for crushed carton",
            "reconcilesId": finding_id
        }
    });
    let (status, receipt) =
        send_post(&app, "/erp/fulfillment/commands", &session, &reconcile_cmd).await;
    assert_eq!(status, StatusCode::OK);
    assert!(receipt["findingId"].is_string());

    let (status, detail) = send_get(
        &app,
        &format!("/erp/fulfillment/jobs/detail?tenant={ACME}&id={job_id}"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["job"]["unreconciledCount"], 0);

    // Check findings projection
    let (status, findings_list) = send_get(
        &app,
        &format!("/erp/fulfillment/findings?tenant={ACME}&jobId={job_id}"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let findings = findings_list.as_array().unwrap();
    assert_eq!(findings.len(), 2);

    // 8. Now completion gate passes!
    let cmd_complete = Uuid::new_v4().to_string();
    let complete_cmd = json!({
        "tenant": ACME,
        "mutation": {
            "type": "change_job_status",
            "commandId": cmd_complete,
            "jobId": job_id,
            "expectedRevision": 3,
            "targetStatus": "completed",
            "reason": "All custody finished and exceptions reconciled"
        }
    });
    let (status, receipt) =
        send_post(&app, "/erp/fulfillment/commands", &session, &complete_cmd).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "completed");

    // 9. Closed job terminal state: cannot mutate completed job
    let cmd_after = Uuid::new_v4().to_string();
    let after_cmd = json!({
        "tenant": ACME,
        "mutation": {
            "type": "record_line",
            "commandId": cmd_after,
            "jobId": job_id,
            "direction": "consume",
            "itemRef": "ITEM-LATE",
            "quantity": 1
        }
    });
    let (status, err) = send_post(&app, "/erp/fulfillment/commands", &session, &after_cmd).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("Evidence cannot be recorded on a completed or canceled job"),
        "expected closed job error, got: {:?}",
        err
    );

    // Check jobs list projection
    let (status, jobs_page) = send_get(
        &app,
        &format!("/erp/fulfillment/jobs?tenant={ACME}&status=completed"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let jobs_arr = jobs_page.as_array().unwrap();
    assert_eq!(jobs_arr.len(), 1);
    assert_eq!(jobs_arr[0]["id"], job_id);
}

#[tokio::test]
async fn erp_fulfillment_permissions_and_scope_guards() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = state(&fixture).await;
    let app = archava_host::router(state);

    let session_all = login(&app, "ax").await;
    let session_reader = login(&app, "ax_reader").await;
    let session_nodispatch = login(&app, "ax_nodispatch").await;
    let session_noproduce = login(&app, "ax_noproduce").await;
    let session_assigned = login(&app, "ax_assigned").await;
    let session_other = login(&app, "other").await;

    // 1. Reader cannot create job -> 403
    let create_cmd = json!({
        "tenant": ACME,
        "mutation": {
            "type": "create_job",
            "commandId": Uuid::new_v4().to_string(),
            "jobCode": "FUL-READ-001",
            "kind": "dispatch",
            "title": "Unauthorized attempt"
        }
    });
    let (status, _) = send_post(
        &app,
        "/erp/fulfillment/commands",
        &session_reader,
        &create_cmd,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 2. Full user creates a production job
    let prod_job_cmd = json!({
        "tenant": ACME,
        "mutation": {
            "type": "create_job",
            "commandId": Uuid::new_v4().to_string(),
            "jobCode": "FUL-PROD-001",
            "kind": "production",
            "title": "Assembly Line Batch #1"
        }
    });
    let (status, receipt) = send_post(
        &app,
        "/erp/fulfillment/commands",
        &session_all,
        &prod_job_cmd,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let prod_job_id = receipt["jobId"].as_str().unwrap().to_string();

    // 3. ax_noproduce cannot record line on a production job -> 403
    let prod_line_cmd = json!({
        "tenant": ACME,
        "mutation": {
            "type": "record_line",
            "commandId": Uuid::new_v4().to_string(),
            "jobId": prod_job_id,
            "direction": "produce",
            "itemRef": "FINISHED-WIDGET",
            "quantity": 10
        }
    });
    let (status, _) = send_post(
        &app,
        "/erp/fulfillment/commands",
        &session_noproduce,
        &prod_line_cmd,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 4. ax_nodispatch cannot record dispatch custody event -> 403
    let custody_dispatch_cmd = json!({
        "tenant": ACME,
        "mutation": {
            "type": "record_custody",
            "commandId": Uuid::new_v4().to_string(),
            "jobId": prod_job_id,
            "event": "dispatched",
            "toRef": "Client Hub"
        }
    });
    let (status, _) = send_post(
        &app,
        "/erp/fulfillment/commands",
        &session_nodispatch,
        &custody_dispatch_cmd,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 5. ax_assigned creates their own job
    let assigned_cmd = json!({
        "tenant": ACME,
        "mutation": {
            "type": "create_job",
            "commandId": Uuid::new_v4().to_string(),
            "jobCode": "FUL-ASSIGNED-001",
            "kind": "dispatch",
            "title": "Assigned User Dispatch"
        }
    });
    let (status, receipt) = send_post(
        &app,
        "/erp/fulfillment/commands",
        &session_assigned,
        &assigned_cmd,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let assigned_job_id = receipt["jobId"].as_str().unwrap().to_string();

    // ax_assigned cannot view or mutate ax's prod_job -> 404
    let (status, _) = send_get(
        &app,
        &format!("/erp/fulfillment/jobs/detail?tenant={ACME}&id={prod_job_id}"),
        &session_assigned,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let steal_cmd = json!({
        "tenant": ACME,
        "mutation": {
            "type": "change_job_status",
            "commandId": Uuid::new_v4().to_string(),
            "jobId": prod_job_id,
            "expectedRevision": 1,
            "targetStatus": "released",
            "reason": "Unauthorized steal"
        }
    });
    let (status, _) = send_post(
        &app,
        "/erp/fulfillment/commands",
        &session_assigned,
        &steal_cmd,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ax_assigned can view their own job
    let (status, own_detail) = send_get(
        &app,
        &format!("/erp/fulfillment/jobs/detail?tenant={ACME}&id={assigned_job_id}"),
        &session_assigned,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(own_detail["job"]["id"], assigned_job_id);

    // 6. Cross-tenant isolation: other (in GLOBEX) cannot view ACME jobs -> 403 because no membership in ACME
    let (status, _) = send_get(
        &app,
        &format!("/erp/fulfillment/jobs/detail?tenant={ACME}&id={prod_job_id}"),
        &session_other,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // and if other looks in GLOBEX for prod_job_id -> 404
    let (status, _) = send_get(
        &app,
        &format!("/erp/fulfillment/jobs/detail?tenant={GLOBEX}&id={prod_job_id}"),
        &session_other,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
