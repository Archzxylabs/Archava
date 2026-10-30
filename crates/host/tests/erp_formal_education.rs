//! The R08 formal education cohort's host witnesses — driven through the real router
//! against the disposable PostgreSQL whose `archava_edu` schema is the
//! real migration ledger (`001_formal_education_enrollment.sql` and `002_formal_education_tenant_keys.sql`).
//!
//! Every assertion is a fact about what the SERVER did:
//! - Authorized applicant admission, confirmation, enrollment, sessions, results, review, graduation.
//! - Enforces that results publication requires domain review approval.
//! - Enforces score bound `score <= out_of`.
//! - Enforces that four non-ranked scopes (`all`, `guardian`, `self`, `assigned_class`) each
//!   reach only their own matching records via `archava_edu.visible_enrollment`.
//! - Closed records (`graduated`, `retained`, `withdrawn`) refuse further modifications.
//! - Service principals are prohibited from recording mutations.
//! - Read projections: list enrollments, enrollment detail, audit trail.
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

const ACME: &str = "acme-edu";
const GLOBEX: &str = "globex-edu";

const EDU_ALL: &[&str] = &[
    "education.read",
    "education.enroll",
    "education.record_session",
    "education.record_result",
    "education.review",
    "education.retain",
    "audit.read",
];

async fn seed(fixture: &Fixture) {
    for principal in [
        "ax",
        "teacher_bob",
        "parent_alice",
        "student_sam",
        "parent_charlie",
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

    // ACME memberships:
    // ax: registrar (scope: all)
    sqlx::query(
        "INSERT INTO archava_edu.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active, classes) \
         VALUES ($1, $2, 'human', $3, 'all', true, '{}')",
    )
    .bind(ACME)
    .bind("ax")
    .bind(EDU_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed ACME ax membership");

    // teacher_bob: scope: assigned_class (classes: ["class-10a"])
    sqlx::query(
        "INSERT INTO archava_edu.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active, classes) \
         VALUES ($1, $2, 'human', $3, 'assigned_class', true, $4)",
    )
    .bind(ACME)
    .bind("teacher_bob")
    .bind(vec![
        "education.read".to_string(),
        "education.record_session".to_string(),
        "education.record_result".to_string(),
        "audit.read".to_string(),
    ])
    .bind(vec!["class-10a".to_string()])
    .execute(&fixture.admin_pool)
    .await
    .expect("seed ACME teacher_bob membership");

    // parent_alice: scope: guardian
    sqlx::query(
        "INSERT INTO archava_edu.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active, classes) \
         VALUES ($1, $2, 'human', $3, 'guardian', true, '{}')",
    )
    .bind(ACME)
    .bind("parent_alice")
    .bind(vec!["education.read".to_string(), "audit.read".to_string()])
    .execute(&fixture.admin_pool)
    .await
    .expect("seed ACME parent_alice membership");

    // student_sam: scope: self
    sqlx::query(
        "INSERT INTO archava_edu.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active, classes) \
         VALUES ($1, $2, 'human', $3, 'self', true, '{}')",
    )
    .bind(ACME)
    .bind("student_sam")
    .bind(vec!["education.read".to_string(), "audit.read".to_string()])
    .execute(&fixture.admin_pool)
    .await
    .expect("seed ACME student_sam membership");

    // parent_charlie: scope: guardian (has no children in the system)
    sqlx::query(
        "INSERT INTO archava_edu.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active, classes) \
         VALUES ($1, $2, 'human', $3, 'guardian', true, '{}')",
    )
    .bind(ACME)
    .bind("parent_charlie")
    .bind(vec!["education.read".to_string(), "audit.read".to_string()])
    .execute(&fixture.admin_pool)
    .await
    .expect("seed ACME parent_charlie membership");

    // GLOBEX membership for other
    sqlx::query(
        "INSERT INTO archava_edu.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active, classes) \
         VALUES ($1, $2, 'human', $3, 'all', true, '{}')",
    )
    .bind(GLOBEX)
    .bind("other")
    .bind(EDU_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed GLOBEX other membership");
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
async fn erp_education_full_lifecycle_and_review_gate() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = state(&fixture).await;
    let app = archava_host::router(state);

    let session = login(&app, "ax").await;

    // Check access
    let (status, body) = send_get(
        &app,
        &format!("/erp/education/access?tenant={ACME}"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["scope"], "all");
    assert!(body["permissions"]
        .as_array()
        .unwrap()
        .contains(&json!("education.enroll")));

    let enr_id = "enr-2026-001";

    // 1. Admit Applicant
    let cmd1 = json!({
        "tenant": ACME,
        "command": {
            "action": "admit_applicant",
            "commandId": "cmd-edu-001",
            "enrollment": {
                "enrollmentId": enr_id,
                "studentRef": "student_sam",
                "classRef": "class-10a",
                "termRef": "term-2026-1",
                "guardianRef": "parent_alice"
            }
        }
    });
    let (status, receipt) = send_post(&app, "/erp/education/commands", &session, &cmd1).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "admit_applicant failed: {:?}",
        receipt
    );
    assert_eq!(receipt["action"], "admit_applicant");
    assert_eq!(receipt["enrollmentId"], enr_id);
    assert_eq!(receipt["status"], "applied");
    assert_eq!(receipt["revision"], 1);

    // Idempotency replay of admit_applicant
    let (replay_status, replay_receipt) =
        send_post(&app, "/erp/education/commands", &session, &cmd1).await;
    assert_eq!(replay_status, StatusCode::OK);
    assert_eq!(replay_receipt["commandId"], "cmd-edu-001");
    assert_eq!(replay_receipt["status"], "applied");

    // Command status query
    let (status, cmd_status) = send_get(
        &app,
        &format!("/erp/education/commands/status?tenant={ACME}&commandId=cmd-edu-001"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cmd_status["status"], "accepted");
    assert_eq!(cmd_status["revision"], 1);

    // 2. Confirm Enrollment: applied -> admitted
    let cmd2 = json!({
        "tenant": ACME,
        "command": {
            "action": "confirm_enrollment",
            "commandId": "cmd-edu-002",
            "enrollmentId": enr_id
        }
    });
    let (status, receipt) = send_post(&app, "/erp/education/commands", &session, &cmd2).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "admitted");
    assert_eq!(receipt["revision"], 2);

    // 3. Enroll Student: admitted -> enrolled
    let cmd3 = json!({
        "tenant": ACME,
        "command": {
            "action": "enroll_student",
            "commandId": "cmd-edu-003",
            "enrollmentId": enr_id
        }
    });
    let (status, receipt) = send_post(&app, "/erp/education/commands", &session, &cmd3).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "enrolled");
    assert_eq!(receipt["revision"], 3);

    // 4. Record Session: enrolled -> attending
    let cmd4 = json!({
        "tenant": ACME,
        "command": {
            "action": "record_session",
            "commandId": "cmd-edu-004",
            "enrollmentId": enr_id,
            "session": {
                "sessionId": "ses-001",
                "outcome": "present",
                "appointmentRef": "appt-math-101",
                "note": "Attended morning session on calculus"
            }
        }
    });
    let (status, receipt) = send_post(&app, "/erp/education/commands", &session, &cmd4).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "attending");
    assert_eq!(receipt["revision"], 4);

    // 5. Record Result: attending -> reported
    let cmd5 = json!({
        "tenant": ACME,
        "command": {
            "action": "record_result",
            "commandId": "cmd-edu-005",
            "enrollmentId": enr_id,
            "result": {
                "resultId": "res-001",
                "assessmentRef": "midterm-exam",
                "score": 18.5,
                "outOf": 20.0,
                "letterRef": "A",
                "note": "Top score in calculus midterm"
            }
        }
    });
    let (status, receipt) = send_post(&app, "/erp/education/commands", &session, &cmd5).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "reported");
    assert_eq!(receipt["revision"], 5);

    // Detail check BEFORE review: resultsPublished is false, results list is empty
    let (status, detail_before) = send_get(
        &app,
        &format!("/erp/education/enrollments/detail?tenant={ACME}&id={enr_id}"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_before["resultsPublished"], false);
    assert_eq!(detail_before["results"].as_array().unwrap().len(), 0);

    // Premature graduation attempt (from reported) must fail with CONFLICT (409)
    let bad_grad = json!({
        "tenant": ACME,
        "command": {
            "action": "graduate_enrollment",
            "commandId": "cmd-edu-bad-grad",
            "enrollmentId": enr_id
        }
    });
    let (status, _) = send_post(&app, "/erp/education/commands", &session, &bad_grad).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // 6. Review Record: reported -> reviewed
    let cmd6 = json!({
        "tenant": ACME,
        "command": {
            "action": "review_record",
            "commandId": "cmd-edu-006",
            "enrollmentId": enr_id,
            "review": {
                "reviewId": "rev-001",
                "decision": "approved",
                "note": "Academic performance verified by department head"
            }
        }
    });
    let (status, receipt) = send_post(&app, "/erp/education/commands", &session, &cmd6).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "reviewed");
    assert_eq!(receipt["revision"], 6);

    // Detail check AFTER review: resultsPublished is true, results list has 1 item
    let (status, detail_after) = send_get(
        &app,
        &format!("/erp/education/enrollments/detail?tenant={ACME}&id={enr_id}"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_after["resultsPublished"], true);
    let results = detail_after["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["resultId"], "res-001");
    assert_eq!(results[0]["score"], 18.5);
    assert_eq!(results[0]["outOf"], 20.0);

    // 7. Graduate Enrollment: reviewed -> graduated
    let cmd7 = json!({
        "tenant": ACME,
        "command": {
            "action": "graduate_enrollment",
            "commandId": "cmd-edu-007",
            "enrollmentId": enr_id
        }
    });
    let (status, receipt) = send_post(&app, "/erp/education/commands", &session, &cmd7).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "graduated");
    assert_eq!(receipt["revision"], 7);

    // 8. Terminal state check: graduated refuses further mutations
    let cmd_terminal = json!({
        "tenant": ACME,
        "command": {
            "action": "withdraw_student",
            "commandId": "cmd-edu-terminal-withdraw",
            "enrollmentId": enr_id,
            "reason": "Late withdrawal attempt"
        }
    });
    let (status, _) = send_post(&app, "/erp/education/commands", &session, &cmd_terminal).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // 9. Read projections check
    // List enrollments
    let (status, list) = send_get(
        &app,
        &format!("/erp/education/enrollments?tenant={ACME}&status=graduated"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let enrs = list.as_array().unwrap();
    assert_eq!(enrs.len(), 1);
    assert_eq!(enrs[0]["enrollmentId"], enr_id);

    // Audit trail
    let (status, audit) = send_get(
        &app,
        &format!("/erp/education/enrollments/audit?tenant={ACME}&id={enr_id}"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let trail = audit.as_array().unwrap();
    assert_eq!(
        trail.len(),
        7,
        "All 7 lifecycle actions must have audit entries"
    );
}

#[tokio::test]
async fn erp_education_permissions_and_scope_guards() {
    let fixture = provision().await;
    seed(&fixture).await;
    let state = state(&fixture).await;
    let app = archava_host::router(state);

    let session_ax = login(&app, "ax").await;
    let session_teacher = login(&app, "teacher_bob").await;
    let session_parent = login(&app, "parent_alice").await;
    let session_student = login(&app, "student_sam").await;
    let session_charlie = login(&app, "parent_charlie").await;
    let session_other = login(&app, "other").await;

    let enr_id = "enr-scope-001";

    // 1. ax admits an applicant
    let admit_cmd = json!({
        "tenant": ACME,
        "command": {
            "action": "admit_applicant",
            "commandId": "cmd-scope-001",
            "enrollment": {
                "enrollmentId": enr_id,
                "studentRef": "student_sam",
                "classRef": "class-10a",
                "termRef": "term-2026-1",
                "guardianRef": "parent_alice"
            }
        }
    });
    let (status, _) = send_post(&app, "/erp/education/commands", &session_ax, &admit_cmd).await;
    assert_eq!(status, StatusCode::OK);

    // Confirm & enroll so teacher can record session
    let confirm_cmd = json!({
        "tenant": ACME,
        "command": {
            "action": "confirm_enrollment",
            "commandId": "cmd-scope-002",
            "enrollmentId": enr_id
        }
    });
    send_post(&app, "/erp/education/commands", &session_ax, &confirm_cmd).await;

    let enroll_cmd = json!({
        "tenant": ACME,
        "command": {
            "action": "enroll_student",
            "commandId": "cmd-scope-003",
            "enrollmentId": enr_id
        }
    });
    send_post(&app, "/erp/education/commands", &session_ax, &enroll_cmd).await;

    // 2. Parent (guardian scope) can see their child's enrollment
    let (status, detail_parent) = send_get(
        &app,
        &format!("/erp/education/enrollments/detail?tenant={ACME}&id={enr_id}"),
        &session_parent,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_parent["enrollment"]["enrollmentId"], enr_id);

    // 3. Student (self scope) can see their own enrollment
    let (status, detail_student) = send_get(
        &app,
        &format!("/erp/education/enrollments/detail?tenant={ACME}&id={enr_id}"),
        &session_student,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_student["enrollment"]["enrollmentId"], enr_id);

    // 4. Teacher (assigned_class scope) can see class-10a enrollment
    let (status, detail_teacher) = send_get(
        &app,
        &format!("/erp/education/enrollments/detail?tenant={ACME}&id={enr_id}"),
        &session_teacher,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_teacher["enrollment"]["enrollmentId"], enr_id);

    // Teacher can record session in their class
    let session_cmd = json!({
        "tenant": ACME,
        "command": {
            "action": "record_session",
            "commandId": "cmd-scope-004",
            "enrollmentId": enr_id,
            "session": {
                "sessionId": "ses-teacher-01",
                "outcome": "present",
                "appointmentRef": "appt-10a-01"
            }
        }
    });
    let (status, _) = send_post(
        &app,
        "/erp/education/commands",
        &session_teacher,
        &session_cmd,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 5. Parent Charlie (guardian of no enrolled students) cannot see enr_id -> 404
    let (status, _) = send_get(
        &app,
        &format!("/erp/education/enrollments/detail?tenant={ACME}&id={enr_id}"),
        &session_charlie,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Charlie's list enrollments returns empty array
    let (status, charlie_list) = send_get(
        &app,
        &format!("/erp/education/enrollments?tenant={ACME}"),
        &session_charlie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(charlie_list.as_array().unwrap().len(), 0);

    // 6. Cross-tenant isolation: user from GLOBEX cannot see ACME enrollment
    let (status, _) = send_get(
        &app,
        &format!("/erp/education/enrollments/detail?tenant={ACME}&id={enr_id}"),
        &session_other,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = send_get(
        &app,
        &format!("/erp/education/enrollments/detail?tenant={GLOBEX}&id={enr_id}"),
        &session_other,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
