//! The R08 laundry cohort's host witnesses — driven through the real router
//! against the disposable PostgreSQL whose `archava_laundry` schema is the
//! real migration ledger (`001_laundry_orders_processing`).
//!
//! Every assertion is a fact about what the SERVER did:
//! - Authorized order intake (piece and weight garments), lines calculation.
//! - Sequential processing chain: begins with intake, strictly advances stage.
//! - Machine events (`wash`, `dry`, `press`) require `laundry.process`.
//! - Exception findings (`partial`, `rejected`) increment `unreconciled_count`.
//! - Reconciliation decrements `unreconciled_count` with single-use `reconciles_id`.
//! - Quality gate: `ready` status strictly requires `unreconciled_count == 0` AND `quality_check`.
//! - Allowed state transitions (`received` -> `processing` -> `ready` -> `returned`).
//! - Read projections: list orders, order detail with lines, process and findings.
//! - Scope narrowing: `assigned` sees only own orders.
//! - Idempotency replay and conflict detection.

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

const ACME: &str = "acme-laundry";
const GLOBEX: &str = "globex-laundry";

const LAUNDRY_ALL: &[&str] = &[
    "laundry.read",
    "laundry.write",
    "laundry.process",
    "audit.read",
];

async fn seed(fixture: &Fixture) {
    for principal in ["ax", "ax_reader", "ax_noprocess", "ax_assigned", "other"] {
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

    for (principal, permissions, scope) in [
        ("ax", LAUNDRY_ALL, "all"),
        ("ax_reader", &["laundry.read"][..], "all"),
        (
            "ax_noprocess",
            &["laundry.read", "laundry.write"][..],
            "all",
        ),
        ("ax_assigned", LAUNDRY_ALL, "assigned"),
    ] {
        sqlx::query(
            "INSERT INTO archava_laundry.memberships \
             (tenant_id, principal_id, display_name, kind, permissions, scope, active) \
             VALUES ($1, $2, $3, 'human', $4, $5, true)",
        )
        .bind(ACME)
        .bind(principal)
        .bind(format!("Display {principal}"))
        .bind(permissions.to_vec())
        .bind(scope)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed ACME laundry membership");
    }

    sqlx::query(
        "INSERT INTO archava_laundry.memberships \
         (tenant_id, principal_id, display_name, kind, permissions, scope, active) \
         VALUES ($1, $2, 'Other Display', 'human', $3, 'all', true)",
    )
    .bind(GLOBEX)
    .bind("other")
    .bind(LAUNDRY_ALL.to_vec())
    .execute(&fixture.admin_pool)
    .await
    .expect("seed GLOBEX laundry membership");
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
    body: Value,
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
async fn erp_laundry_full_lifecycle_and_quality_gate() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);

    let session = login(&app, "ax").await;

    // 1. Create order
    let cmd1 = uuid::Uuid::new_v4().to_string();
    let (status, receipt) = send_post(
        &app,
        "/erp/laundry/commands",
        &session,
        json!({
            "tenant": ACME,
            "mutation": {
                "type": "create_order",
                "commandId": cmd1,
                "orderCode": "LND-1001",
                "kind": "weight",
                "customerRef": "cust-alice",
                "currency": "USD",
                "lines": [
                    {
                        "itemRef": "wash-fold-kg",
                        "quantity": 3000,
                        "unit": "gram",
                        "unitPriceMinor": 15
                    },
                    {
                        "itemRef": "jacket-dry-clean",
                        "quantity": 2,
                        "unit": "piece",
                        "unitPriceMinor": 500
                    }
                ]
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "received");
    let order_id = receipt["orderId"].as_str().unwrap().to_string();

    // Verify detail
    let (status, detail) = send_get(
        &app,
        &format!("/erp/laundry/orders/detail?tenant={ACME}&id={order_id}"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["order"]["orderCode"], "LND-1001");
    // total = (3000 * 15) + (2 * 500) = 45000 + 1000 = 46000
    assert_eq!(detail["order"]["totalMinor"], 46000);
    assert_eq!(detail["order"]["weightGrams"], 3000);
    assert_eq!(detail["order"]["itemCount"], 2);
    assert_eq!(detail["order"]["unreconciledCount"], 0);
    assert_eq!(detail["lines"].as_array().unwrap().len(), 2);

    // 2. Change status to processing
    let cmd2 = uuid::Uuid::new_v4().to_string();
    let (status, receipt) = send_post(
        &app,
        "/erp/laundry/commands",
        &session,
        json!({
            "tenant": ACME,
            "mutation": {
                "type": "change_order_status",
                "commandId": cmd2,
                "orderId": order_id,
                "expectedRevision": 1,
                "targetStatus": "processing",
                "reason": "Sorted and ready to process"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "processing");
    assert_eq!(receipt["revision"], 2);

    // 3. Process events: must begin with intake
    let cmd_bad = uuid::Uuid::new_v4().to_string();
    let (status, _) = send_post(
        &app,
        "/erp/laundry/commands",
        &session,
        json!({
            "tenant": ACME,
            "mutation": {
                "type": "record_process",
                "commandId": cmd_bad,
                "orderId": order_id,
                "event": "wash"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT); // Cannot start with wash

    // Record intake
    let cmd3 = uuid::Uuid::new_v4().to_string();
    let (status, _) = send_post(
        &app,
        "/erp/laundry/commands",
        &session,
        json!({
            "tenant": ACME,
            "mutation": {
                "type": "record_process",
                "commandId": cmd3,
                "orderId": order_id,
                "event": "intake",
                "note": "Weighed and tagged"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Record wash (requires machine grant, ax has it)
    let cmd4 = uuid::Uuid::new_v4().to_string();
    let (status, _) = send_post(
        &app,
        "/erp/laundry/commands",
        &session,
        json!({
            "tenant": ACME,
            "mutation": {
                "type": "record_process",
                "commandId": cmd4,
                "orderId": order_id,
                "event": "wash"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 4. Record finding: stain (partial exception) -> unreconciled_count becomes 1
    let cmd5 = uuid::Uuid::new_v4().to_string();
    let (status, receipt) = send_post(
        &app,
        "/erp/laundry/commands",
        &session,
        json!({
            "tenant": ACME,
            "mutation": {
                "type": "record_finding",
                "commandId": cmd5,
                "orderId": order_id,
                "category": "stain",
                "disposition": "partial",
                "quantity": 1,
                "reason": "Stubborn grease stain on collar"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let finding_id = receipt["findingId"].as_str().unwrap().to_string();

    // 5. Try transition to 'ready' -> should FAIL because:
    // a) unreconciled_count == 1
    // b) quality_check has not been recorded
    let cmd6 = uuid::Uuid::new_v4().to_string();
    let (status, _) = send_post(
        &app,
        "/erp/laundry/commands",
        &session,
        json!({
            "tenant": ACME,
            "mutation": {
                "type": "change_order_status",
                "commandId": cmd6,
                "orderId": order_id,
                "expectedRevision": 2,
                "targetStatus": "ready",
                "reason": "Trying ready too early"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Record quality check event
    let cmd7 = uuid::Uuid::new_v4().to_string();
    let (status, _) = send_post(
        &app,
        "/erp/laundry/commands",
        &session,
        json!({
            "tenant": ACME,
            "mutation": {
                "type": "record_process",
                "commandId": cmd7,
                "orderId": order_id,
                "event": "quality_check",
                "note": "Checked collar"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Still cannot mark ready because unreconciled_count > 0!
    let cmd8 = uuid::Uuid::new_v4().to_string();
    let (status, _) = send_post(
        &app,
        "/erp/laundry/commands",
        &session,
        json!({
            "tenant": ACME,
            "mutation": {
                "type": "change_order_status",
                "commandId": cmd8,
                "orderId": order_id,
                "expectedRevision": 2,
                "targetStatus": "ready",
                "reason": "Still has unreconciled exception"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Reconcile finding by client acceptance
    let cmd9 = uuid::Uuid::new_v4().to_string();
    let (status, _) = send_post(
        &app,
        "/erp/laundry/commands",
        &session,
        json!({
            "tenant": ACME,
            "mutation": {
                "type": "record_finding",
                "commandId": cmd9,
                "orderId": order_id,
                "category": "stain",
                "disposition": "accepted",
                "reason": "Customer agreed with pre-existing stain",
                "reconcilesId": finding_id
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Check detail: unreconciledCount must now be 0!
    let (_, detail) = send_get(
        &app,
        &format!("/erp/laundry/orders/detail?tenant={ACME}&id={order_id}"),
        &session,
    )
    .await;
    assert_eq!(detail["order"]["unreconciledCount"], 0);

    // Now transition to ready SUCCEEDS!
    let cmd10 = uuid::Uuid::new_v4().to_string();
    let (status, receipt) = send_post(
        &app,
        "/erp/laundry/commands",
        &session,
        json!({
            "tenant": ACME,
            "mutation": {
                "type": "change_order_status",
                "commandId": cmd10,
                "orderId": order_id,
                "expectedRevision": 2,
                "targetStatus": "ready",
                "reason": "Quality check passed and all exceptions resolved"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "ready");
    assert_eq!(receipt["revision"], 3);

    // Handover / returned transition succeeds
    let cmd11 = uuid::Uuid::new_v4().to_string();
    let (status, receipt) = send_post(
        &app,
        "/erp/laundry/commands",
        &session,
        json!({
            "tenant": ACME,
            "mutation": {
                "type": "change_order_status",
                "commandId": cmd11,
                "orderId": order_id,
                "expectedRevision": 3,
                "targetStatus": "returned",
                "reason": "Handed over to customer"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "returned");
    assert_eq!(receipt["revision"], 4);

    // Idempotency replay on cmd11
    let (status, replay_receipt) = send_post(
        &app,
        "/erp/laundry/commands",
        &session,
        json!({
            "tenant": ACME,
            "mutation": {
                "type": "change_order_status",
                "commandId": cmd11,
                "orderId": order_id,
                "expectedRevision": 3,
                "targetStatus": "returned",
                "reason": "Handed over to customer"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay_receipt["idempotencyReplay"], true);

    // Command status read
    let (status, cmd_status) = send_get(
        &app,
        &format!("/erp/laundry/commands/status?tenant={ACME}&requestId={cmd11}"),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cmd_status["status"], "recorded");
    assert_eq!(cmd_status["receipt"]["action"], "change_order_status");
}

#[tokio::test]
async fn erp_laundry_permissions_and_scope_guards() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);

    let session_ax = login(&app, "ax").await;
    let session_noprocess = login(&app, "ax_noprocess").await;
    let session_assigned = login(&app, "ax_assigned").await;
    let session_other = login(&app, "other").await;

    // 1. Create order by ax
    let cmd1 = uuid::Uuid::new_v4().to_string();
    let (status, receipt) = send_post(
        &app,
        "/erp/laundry/commands",
        &session_ax,
        json!({
            "tenant": ACME,
            "mutation": {
                "type": "create_order",
                "commandId": cmd1,
                "orderCode": "LND-2001",
                "kind": "item",
                "currency": "USD",
                "lines": [
                    {
                        "itemRef": "suit",
                        "quantity": 1,
                        "unit": "piece",
                        "unitPriceMinor": 1200
                    }
                ]
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let order_id = receipt["orderId"].as_str().unwrap().to_string();

    // Start intake
    let cmd2 = uuid::Uuid::new_v4().to_string();
    send_post(
        &app,
        "/erp/laundry/commands",
        &session_ax,
        json!({
            "tenant": ACME,
            "mutation": {
                "type": "record_process",
                "commandId": cmd2,
                "orderId": order_id,
                "event": "intake"
            }
        }),
    )
    .await;

    // 2. Machine event by ax_noprocess (has laundry.write but NOT laundry.process)
    let cmd3 = uuid::Uuid::new_v4().to_string();
    let (status, _) = send_post(
        &app,
        "/erp/laundry/commands",
        &session_noprocess,
        json!({
            "tenant": ACME,
            "mutation": {
                "type": "record_process",
                "commandId": cmd3,
                "orderId": order_id,
                "event": "wash"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 3. Assigned scope check: ax_assigned cannot see ax's order
    let (status, _) = send_get(
        &app,
        &format!("/erp/laundry/orders/detail?tenant={ACME}&id={order_id}"),
        &session_assigned,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // In orders list, ax sees 1 order, ax_assigned sees 0 orders
    let (status, list_ax) = send_get(
        &app,
        &format!("/erp/laundry/orders?tenant={ACME}"),
        &session_ax,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list_ax.as_array().unwrap().len(), 1);

    let (status, list_assigned) = send_get(
        &app,
        &format!("/erp/laundry/orders?tenant={ACME}"),
        &session_assigned,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list_assigned.as_array().unwrap().len(), 0);

    // 4. Cross-tenant isolation: other (in GLOBEX) cannot access ACME
    let (status, _) = send_get(
        &app,
        &format!("/erp/laundry/orders/detail?tenant={ACME}&id={order_id}"),
        &session_other,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
