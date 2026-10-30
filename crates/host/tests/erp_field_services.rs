//! The R08 field services cohort's host witnesses — driven through the real router
//! against the disposable PostgreSQL whose `archava_field` schema is the
//! real migration ledger (`001_field_services_dispatch_visits` and `002_field_services_tenant_isolation`).
//!
//! Every assertion is a fact about what the SERVER did:
//! - Authorized job creation, assignment, site visit, labour and part work lines.
//! - Enforces that work evidence requires a recorded site visit.
//! - Accumulates labour hours (decimal) and parts pieces (integer) on separate axes.
//! - Enforces that job approval strictly requires an accepted customer signoff.
//! - Closed jobs (`approved`, `canceled`) refuse further mutations.
//! - Read projections: list jobs, job detail with visits, work lines, signoff, audit trail.
//! - Scope narrowing: `assigned` sees only own or assigned jobs.
//! - Idempotency replay with identical fingerprint.

use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

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

const ACME: &str = "acme-field";
const GLOBEX: &str = "globex-field";

const FIELD_ALL: &[&str] = &[
    "field.read",
    "field.write",
    "field.dispatch",
    "field.signoff",
    "audit.read",
];

async fn seed(fixture: &Fixture) {
    for principal in [
        "ax",
        "ax_reader",
        "ax_nowrite",
        "ax_assigned",
        "tech_bob",
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
        ("ax", FIELD_ALL, "all"),
        ("ax_reader", &["field.read"][..], "all"),
        ("ax_nowrite", &["field.read", "field.dispatch"][..], "all"),
        ("ax_assigned", FIELD_ALL, "assigned"),
        ("tech_bob", FIELD_ALL, "assigned"),
    ] {
        sqlx::query(
            "INSERT INTO archava_field.memberships \
             (tenant_id, principal_id, kind, permissions, scope, active) \
             VALUES ($1, $2, 'human', $3, $4, true)",
        )
        .bind(ACME)
        .bind(principal)
        .bind(permissions.to_vec())
        .bind(scope)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed ACME field membership");
    }

    sqlx::query(
        "INSERT INTO archava_field.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(GLOBEX)
    .bind("other")
    .bind(FIELD_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed GLOBEX field membership");
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
    let bytes = to_bytes(response.into_body(), 16_384).await.unwrap();
    let doc = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, doc)
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
    let doc = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, doc)
}

#[tokio::test]
async fn erp_field_full_lifecycle_and_signoff_gate() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app_state = state(&fixture).await;
    let app = archava_host::router(app_state);
    let session = login(&app, "ax").await;

    let job_id = "job-hvac-2026-001";

    // 1. Create field job
    let create_cmd = json!({
        "tenant": ACME,
        "command": {
            "action": "create_job",
            "commandId": "cmd-fld-001",
            "job": {
                "jobId": job_id,
                "customerRef": "cust-building-a",
                "siteRef": "site-rooftop-hvac",
                "summary": "Compressor failing to start during heatwave",
                "priority": "urgent",
                "appointmentRef": "appt-sched-20260401",
                "assetRef": "asset-chiller-09"
            }
        }
    });

    let (status, receipt) = send_post(&app, "/erp/field/commands", &session, &create_cmd).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "create job should succeed: {receipt}"
    );
    assert_eq!(receipt["action"], "create_job");
    assert_eq!(receipt["jobId"], job_id);
    assert_eq!(receipt["status"], "requested");
    assert_eq!(receipt["revision"], 1);

    // Idempotency replay check: exact same command returns same receipt
    let (replay_status, replay_receipt) =
        send_post(&app, "/erp/field/commands", &session, &create_cmd).await;
    assert_eq!(replay_status, StatusCode::OK);
    assert_eq!(replay_receipt["commandId"], "cmd-fld-001");
    assert_eq!(replay_receipt["revision"], 1);

    // 2. Precondition check: trying to approve directly from requested must fail
    let bad_approve = json!({
        "tenant": ACME,
        "command": {
            "action": "approve_job",
            "commandId": "cmd-fld-bad-approve",
            "jobId": job_id
        }
    });
    let (status, _) = send_post(&app, "/erp/field/commands", &session, &bad_approve).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // 3. Precondition check: adding work line before site visit must fail
    let premature_work = json!({
        "tenant": ACME,
        "command": {
            "action": "append_work_line",
            "commandId": "cmd-fld-premature-line",
            "jobId": job_id,
            "line": {
                "lineId": "line-01",
                "kind": "labour",
                "description": "Diagnostic labour",
                "quantity": 1.5,
                "unit": "hours"
            }
        }
    });
    let (status, _) = send_post(&app, "/erp/field/commands", &session, &premature_work).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // 4. Assign job to technician
    let assign_cmd = json!({
        "tenant": ACME,
        "command": {
            "action": "assign_job",
            "commandId": "cmd-fld-002",
            "jobId": job_id,
            "assignedPrincipal": "tech_bob"
        }
    });
    let (status, receipt) = send_post(&app, "/erp/field/commands", &session, &assign_cmd).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "assigned");
    assert_eq!(receipt["revision"], 2);

    // 5. Record site visit
    let visit_cmd = json!({
        "tenant": ACME,
        "command": {
            "action": "record_visit",
            "commandId": "cmd-fld-003",
            "jobId": job_id,
            "visit": {
                "visitId": "visit-01",
                "outcome": "completed",
                "note": "Arrived on site, verified faulty capacitor and low refrigerant level",
                "technicianRef": "tech_bob"
            }
        }
    });
    let (status, receipt) = send_post(&app, "/erp/field/commands", &session, &visit_cmd).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "visited");
    assert_eq!(receipt["revision"], 3);

    // 6. Append labour work line
    let labour_cmd = json!({
        "tenant": ACME,
        "command": {
            "action": "append_work_line",
            "commandId": "cmd-fld-004",
            "jobId": job_id,
            "line": {
                "lineId": "line-labour-01",
                "kind": "labour",
                "description": "Capacitor replacement and leak test",
                "quantity": 2.5,
                "unit": "hours"
            }
        }
    });
    let (status, receipt) = send_post(&app, "/erp/field/commands", &session, &labour_cmd).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "evidenced");
    assert_eq!(receipt["revision"], 4);

    // 7. Append part work line
    let part_cmd = json!({
        "tenant": ACME,
        "command": {
            "action": "append_work_line",
            "commandId": "cmd-fld-005",
            "jobId": job_id,
            "line": {
                "lineId": "line-part-01",
                "kind": "part",
                "description": "Heavy-duty dual run capacitor",
                "quantity": 2.0,
                "unit": "pieces",
                "partRef": "PRT-CAP-45-5"
            }
        }
    });
    let (status, receipt) = send_post(&app, "/erp/field/commands", &session, &part_cmd).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "evidenced");
    assert_eq!(receipt["revision"], 5);

    // 8. Gate check: approve_job before signoff MUST be rejected
    let approve_cmd = json!({
        "tenant": ACME,
        "command": {
            "action": "approve_job",
            "commandId": "cmd-fld-006",
            "jobId": job_id
        }
    });
    let (status, _) = send_post(&app, "/erp/field/commands", &session, &approve_cmd).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // 9. Record rejected signoff: customer disputes something
    let rejected_signoff = json!({
        "tenant": ACME,
        "command": {
            "action": "record_signoff",
            "commandId": "cmd-fld-007",
            "jobId": job_id,
            "signoff": {
                "signoffId": "signoff-01",
                "signerRef": "Facility Manager John",
                "accepted": false,
                "comment": "Chiller makes unusual humming noise"
            }
        }
    });
    let (status, receipt) =
        send_post(&app, "/erp/field/commands", &session, &rejected_signoff).await;
    assert_eq!(status, StatusCode::OK);
    // Demoted or remains at evidenced
    assert_eq!(receipt["status"], "evidenced");
    assert_eq!(receipt["revision"], 6);

    // Gate check: rejected signoff cannot permit approval
    let (status, _) = send_post(&app, "/erp/field/commands", &session, &approve_cmd).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // 10. Record accepted signoff
    let accepted_signoff = json!({
        "tenant": ACME,
        "command": {
            "action": "record_signoff",
            "commandId": "cmd-fld-008",
            "jobId": job_id,
            "signoff": {
                "signoffId": "signoff-02",
                "signerRef": "Facility Manager John",
                "accepted": true,
                "comment": "Inspected after adjustment. Noise resolved. Cooling nominal."
            }
        }
    });
    let (status, receipt) =
        send_post(&app, "/erp/field/commands", &session, &accepted_signoff).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "signed_off");
    assert_eq!(receipt["revision"], 7);

    // 11. Now approve the job!
    let (status, receipt) = send_post(&app, "/erp/field/commands", &session, &approve_cmd).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "approved");
    assert_eq!(receipt["revision"], 8);

    // 12. Approved is terminal: further mutations are refused
    let post_approval_cmd = json!({
        "tenant": ACME,
        "command": {
            "action": "cancel_job",
            "commandId": "cmd-fld-cancel-closed",
            "jobId": job_id,
            "reason": "Too late to cancel"
        }
    });
    let (status, _) = send_post(&app, "/erp/field/commands", &session, &post_approval_cmd).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // 13. Verify read projections
    // Detail view
    let (status, detail) = send_get(
        &app,
        &format!("/erp/field/jobs/detail?tenant={ACME}&id={job_id}"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["job"]["jobId"], job_id);
    assert_eq!(detail["job"]["status"], "approved");
    assert_eq!(detail["job"]["laborHours"], 2.5);
    assert_eq!(detail["job"]["partsPieces"], 2);
    assert_eq!(detail["job"]["revision"], 8);
    assert_eq!(detail["visits"].as_array().unwrap().len(), 1);
    assert_eq!(detail["workLines"].as_array().unwrap().len(), 2);
    assert!(detail["signoff"]["accepted"].as_bool().unwrap());

    // List jobs
    let (status, list) = send_get(&app, &format!("/erp/field/jobs?tenant={ACME}"), &session).await;
    assert_eq!(status, StatusCode::OK);
    let items = list.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["jobId"], job_id);

    // Audit trail
    let (status, audit) = send_get(
        &app,
        &format!("/erp/field/audit?tenant={ACME}&id={job_id}"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let audit_items = audit.as_array().unwrap();
    assert_eq!(audit_items.len(), 8);

    // Command status
    let (status, cmd_stat) = send_get(
        &app,
        &format!("/erp/field/commands/status?tenant={ACME}&commandId=cmd-fld-001"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cmd_stat["status"], "accepted");
    assert_eq!(cmd_stat["action"], "create_job");
}

#[tokio::test]
async fn erp_field_permissions_and_scope_guards() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app_state = state(&fixture).await;
    let app = archava_host::router(app_state);

    let admin_session = login(&app, "ax").await;
    let reader_session = login(&app, "ax_reader").await;
    let nowrite_session = login(&app, "ax_nowrite").await;
    let tech_bob_session = login(&app, "tech_bob").await;
    let globex_session = login(&app, "other").await;

    // 1. Reader cannot write jobs
    let create_cmd = json!({
        "tenant": ACME,
        "command": {
            "action": "create_job",
            "commandId": "cmd-fld-sec-01",
            "job": {
                "jobId": "job-sec-01",
                "customerRef": "cust-1",
                "siteRef": "site-1",
                "summary": "Security test job",
                "priority": "normal",
                "appointmentRef": "appt-01"
            }
        }
    });

    let (status, _) = send_post(&app, "/erp/field/commands", &reader_session, &create_cmd).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 2. ax_nowrite has dispatch, but not write: cannot create job
    let (status, _) = send_post(&app, "/erp/field/commands", &nowrite_session, &create_cmd).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 3. Admin creates job and assigns to tech_bob
    let (status, _) = send_post(&app, "/erp/field/commands", &admin_session, &create_cmd).await;
    assert_eq!(status, StatusCode::OK);

    let assign_cmd = json!({
        "tenant": ACME,
        "command": {
            "action": "assign_job",
            "commandId": "cmd-fld-sec-02",
            "jobId": "job-sec-01",
            "assignedPrincipal": "tech_bob"
        }
    });
    let (status, _) = send_post(&app, "/erp/field/commands", &admin_session, &assign_cmd).await;
    assert_eq!(status, StatusCode::OK);

    // 4. tech_bob has assigned scope: sees this job because assigned_principal == tech_bob
    let (status, list) = send_get(
        &app,
        &format!("/erp/field/jobs?tenant={ACME}"),
        &tech_bob_session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);

    // 5. Admin creates another job unassigned
    let unassigned_cmd = json!({
        "tenant": ACME,
        "command": {
            "action": "create_job",
            "commandId": "cmd-fld-sec-03",
            "job": {
                "jobId": "job-sec-02",
                "customerRef": "cust-2",
                "siteRef": "site-2",
                "summary": "Unassigned job",
                "priority": "low",
                "appointmentRef": "appt-02"
            }
        }
    });
    let (status, _) = send_post(&app, "/erp/field/commands", &admin_session, &unassigned_cmd).await;
    assert_eq!(status, StatusCode::OK);

    // tech_bob cannot see unassigned job because neither created_by nor assigned_principal matches
    let (status, list) = send_get(
        &app,
        &format!("/erp/field/jobs?tenant={ACME}"),
        &tech_bob_session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["jobId"], "job-sec-01");

    // tech_bob detail query for job-sec-02 returns 404 (unknown resource under assigned scope)
    let (status, _) = send_get(
        &app,
        &format!("/erp/field/jobs/detail?tenant={ACME}&id=job-sec-02"),
        &tech_bob_session,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 6. Cross-tenant isolation: Globex user cannot access Acme job
    let (status, _) = send_get(
        &app,
        &format!("/erp/field/jobs/detail?tenant={ACME}&id=job-sec-01"),
        &globex_session,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
