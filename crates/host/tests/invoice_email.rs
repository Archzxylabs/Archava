//! Real router + PostgreSQL + owned mock Resend HTTP provider. No live email service is contacted.
mod common;
use archava_host::{
    mail::{EmailIntegration, EmailRegistry},
    router,
    router::AllReadiness,
    AppState, Ledger,
};
use archava_pg_fixture::{provision, Fixture};
use archava_providers::resend::ResendClient;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use common::{call, login, HASH};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const KEY: &str = "re_synthetic_owned_email_key_12345";

#[derive(Default)]
struct Responses {
    send_count: u32,
    last_idempotency_key: String,
    last_to: Vec<String>,
    last_html: String,
}

struct Mock {
    origin: String,
    state: Arc<Mutex<Responses>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn mock_send(
    State(state): State<Arc<Mutex<Responses>>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let mut s = state.lock().unwrap();
    s.send_count += 1;
    if let Some(key) = headers.get("idempotency-key").and_then(|v| v.to_str().ok()) {
        s.last_idempotency_key = key.to_string();
    }
    if let Some(to) = body.get("to").and_then(|v| v.as_array()) {
        s.last_to = to
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
    s.last_html = body["html"].as_str().unwrap_or("").into();
    (StatusCode::OK, Json(json!({"id": "resend_msg_test_9876"})))
}

impl Mock {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(Responses::default()));
        let app = Router::new()
            .route("/emails", post(mock_send))
            .with_state(state.clone());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            origin,
            state,
            task,
        }
    }

    fn registry(&self) -> EmailRegistry {
        EmailRegistry::new(vec![EmailIntegration {
            id: "resend-primary".into(),
            tenant: archava_core::tenant::TenantId::new("acme").unwrap(),
            from_address: "Acme Billing <billing@acme.com>".into(),
            client: ResendClient::for_loopback(&self.origin, KEY.into()).unwrap(),
        }])
        .unwrap()
    }
}

async fn setup(mock: &Mock) -> (Fixture, Router, Value, Uuid) {
    let f = provision().await;
    for user in ["alice", "bob"] {
        sqlx::query(
            "INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES($1,$2,$3,true,1)",
        )
        .bind(user)
        .bind(format!("{user}@archava.test"))
        .bind(HASH)
        .execute(&f.admin_pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO archava_billing.memberships(tenant_id,principal_id,kind,permissions,scope,active) \
             VALUES('acme',$1,'human',$2,'all',true)",
        )
        .bind(user)
        .bind(if user == "alice" {
            vec!["billing.read", "billing.write", "billing.settle"]
        } else {
            vec!["billing.read"]
        })
        .execute(&f.admin_pool)
        .await
        .unwrap();
    }

    let additional = f.erp_readiness();
    let state = AppState::try_new_all(
        f.runtime_pool.clone(),
        Ledger {
            namespace: "archava_commerce",
            ledger: "Commerce",
            tables: &[
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
            ],
        },
        AllReadiness {
            commerce: &f.commerce_ledger,
            identity: &f.identity_ledger,
            crm: &f.crm_ledger,
            owner: &f.owner_ledger,
            purchasing: &f.purchasing_ledger,
            additional_erp: &additional,
        },
        &[7u8; 32],
    )
    .await
    .unwrap()
    .with_email_registry(mock.registry());

    let app = router(state);
    let session = login(&app, "alice").await;

    let (s, invoice) = call(
        &app,
        "POST",
        "/erp/billing/commands",
        json!({
            "tenant": "acme",
            "mutation": {
                "type": "create_invoice",
                "commandId": Uuid::new_v4(),
                "invoiceCode": "INV-EMAIL",
                "customerRef": "Customer Co",
                "consumerRef": "test",
                "currency": "IDR",
                "lines": [{"description": "Service", "quantity": 1, "unitMinor": 100}]
            }
        }),
        Some(&session),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{invoice}");
    let invoice_id = Uuid::parse_str(invoice["invoiceId"].as_str().unwrap()).unwrap();

    let (s, b) = call(
        &app,
        "POST",
        "/erp/billing/commands",
        json!({
            "tenant": "acme",
            "mutation": {
                "type": "issue_invoice",
                "commandId": Uuid::new_v4(),
                "invoiceId": invoice_id,
                "expectedRevision": 1,
                "reason": "Ready for email"
            }
        }),
        Some(&session),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");

    (f, app, session, invoice_id)
}

#[tokio::test]
async fn send_invoice_email_records_outbox_and_accepts() {
    let mock = Mock::start().await;
    let (f, app, auth, invoice_id) = setup(&mock).await;
    let req_id = Uuid::new_v4();

    let (status, body) = call(
        &app,
        "POST",
        &format!("/erp/billing/invoices/{invoice_id}/send-email?tenant=acme"),
        json!({
            "requestId": req_id,
            "recipient": "customer@example.com",
            "subject": "Your Invoice INV-EMAIL",
            "message": "Thank you for your business."
        }),
        Some(&auth),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "Body: {body}");
    assert_eq!(body["state"], "accepted");
    assert_eq!(body["recipient"], "customer@example.com");
    assert_eq!(body["providerMessageId"], "resend_msg_test_9876");

    assert_eq!(mock.state.lock().unwrap().send_count, 1);
    assert_eq!(
        mock.state.lock().unwrap().last_idempotency_key,
        req_id.to_string()
    );
    assert_eq!(
        mock.state.lock().unwrap().last_to,
        vec!["customer@example.com"]
    );

    // Check list history
    let (list_status, list_body) = call(
        &app,
        "GET",
        &format!("/erp/billing/invoices/{invoice_id}/emails?tenant=acme"),
        json!({}),
        Some(&auth),
    )
    .await;

    assert_eq!(list_status, StatusCode::OK);
    let list = list_body.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], body["id"]);
    assert_eq!(list[0]["state"], "accepted");

    let snapshot = sqlx::query(
        "SELECT payload_text, payload_html, from_address FROM archava_billing.email_outbox \
         WHERE tenant_id = 'acme' AND request_id = $1",
    )
    .bind(req_id)
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    let payload_text: String = sqlx::Row::try_get(&snapshot, "payload_text").unwrap();
    let payload_html: Option<String> = sqlx::Row::try_get(&snapshot, "payload_html").unwrap();
    let from_address: String = sqlx::Row::try_get(&snapshot, "from_address").unwrap();
    assert!(payload_text.contains("INV-EMAIL"));
    assert!(payload_html
        .unwrap_or_default()
        .contains("Invoice INV-EMAIL"));
    assert_eq!(from_address, "Acme Billing <billing@acme.com>");
}

#[tokio::test]
async fn idempotent_retry_returns_existing_without_second_call() {
    let mock = Mock::start().await;
    let (_f, app, auth, invoice_id) = setup(&mock).await;
    let req_id = Uuid::new_v4();

    let (status1, body1) = call(
        &app,
        "POST",
        &format!("/erp/billing/invoices/{invoice_id}/send-email?tenant=acme"),
        json!({
            "requestId": req_id,
            "recipient": "customer@example.com",
            "subject": "Invoice INV-EMAIL"
        }),
        Some(&auth),
    )
    .await;

    assert_eq!(status1, StatusCode::OK);
    assert_eq!(mock.state.lock().unwrap().send_count, 1);

    // Exact replay with same requestId and payload
    let (status2, body2) = call(
        &app,
        "POST",
        &format!("/erp/billing/invoices/{invoice_id}/send-email?tenant=acme"),
        json!({
            "requestId": req_id,
            "recipient": "customer@example.com",
            "subject": "Invoice INV-EMAIL"
        }),
        Some(&auth),
    )
    .await;

    assert_eq!(status2, StatusCode::OK);
    assert_eq!(body1["id"], body2["id"]);
    // Mock was not called a second time!
    assert_eq!(mock.state.lock().unwrap().send_count, 1);

    // Conflicting replay with different recipient
    let (status3, _body3) = call(
        &app,
        "POST",
        &format!("/erp/billing/invoices/{invoice_id}/send-email?tenant=acme"),
        json!({
            "requestId": req_id,
            "recipient": "another-customer@example.com",
            "subject": "Invoice INV-EMAIL"
        }),
        Some(&auth),
    )
    .await;

    assert_eq!(status3, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_request_id_cannot_replay_a_receipt_for_another_invoice() {
    let mock = Mock::start().await;
    let (_f, app, auth, first_invoice) = setup(&mock).await;
    let (status, second) = call(
        &app,
        "POST",
        "/erp/billing/commands",
        json!({
            "tenant": "acme",
            "mutation": {
                "type": "create_invoice",
                "commandId": Uuid::new_v4(),
                "invoiceCode": "INV-EMAIL-SECOND",
                "customerRef": "Another Customer",
                "consumerRef": "test",
                "currency": "IDR",
                "lines": [{"description": "Another service", "quantity": 1, "unitMinor": 200}]
            }
        }),
        Some(&auth),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    let second_invoice = second["invoiceId"].as_str().unwrap();

    let request_id = Uuid::new_v4();
    let first_path = format!("/erp/billing/invoices/{first_invoice}/send-email?tenant=acme");
    let (first_status, first_body) = call(
        &app,
        "POST",
        &first_path,
        json!({
            "requestId": request_id,
            "recipient": "customer@example.com",
            "subject": "Invoice replay boundary"
        }),
        Some(&auth),
    )
    .await;
    assert_eq!(first_status, StatusCode::OK, "{first_body}");
    assert_eq!(mock.state.lock().unwrap().send_count, 1);

    let second_path = format!("/erp/billing/invoices/{second_invoice}/send-email?tenant=acme");
    let (second_status, second_body) = call(
        &app,
        "POST",
        &second_path,
        json!({
            "requestId": request_id,
            "recipient": "customer@example.com",
            "subject": "Invoice replay boundary"
        }),
        Some(&auth),
    )
    .await;
    assert_eq!(second_status, StatusCode::CONFLICT, "{second_body}");
    assert_eq!(mock.state.lock().unwrap().send_count, 1);
}

#[tokio::test]
async fn reader_only_user_is_forbidden() {
    let mock = Mock::start().await;
    let (_f, app, _auth, invoice_id) = setup(&mock).await;
    let bob_session = login(&app, "bob").await;

    let (status, body) = call(
        &app,
        "POST",
        &format!("/erp/billing/invoices/{invoice_id}/send-email?tenant=acme"),
        json!({
            "requestId": Uuid::new_v4(),
            "recipient": "customer@example.com"
        }),
        Some(&bob_session),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN, "Body: {body}");
}

#[tokio::test]
async fn review_gemini_email_without_csrf_is_refused_before_provider() {
    use tower::ServiceExt;
    let mock = Mock::start().await;
    let (_f, app, auth, invoice) = setup(&mock).await;
    let request = axum::http::Request::builder()
        .method("POST")
        .uri(format!(
            "/erp/billing/invoices/{invoice}/send-email?tenant=acme"
        ))
        .header(
            "authorization",
            format!("Bearer {}", auth["token"].as_str().unwrap()),
        )
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            json!({
                "requestId": Uuid::new_v4(),
                "recipient": "customer@example.com",
                "subject": "CSRF regression"
            })
            .to_string(),
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let sends = mock.state.lock().unwrap().send_count;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "email dispatch without the required CSRF header reached the provider"
    );
    assert_eq!(sends, 0, "provider was contacted when CSRF was missing");
}

#[tokio::test]
async fn review_gemini_lost_ack_then_ui_style_new_key_must_not_duplicate_email() {
    let mock = Mock::start().await;
    let (_f, app, auth, invoice) = setup(&mock).await;
    let path = format!("/erp/billing/invoices/{invoice}/send-email?tenant=acme");
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    for key in [first, second] {
        let (status, body) = call(
            &app,
            "POST",
            &path,
            json!({
                "requestId": key,
                "recipient": "customer@example.com",
                "subject": "Original invoice",
                "message": "Please review the invoice details attached."
            }),
            Some(&auth),
        )
        .await;
        if key == first {
            assert_eq!(status, StatusCode::OK);
        } else {
            assert_eq!(status, StatusCode::CONFLICT);
            assert_eq!(body["reason"], "email_already_recorded");
            assert_eq!(body["requestId"], key.to_string());
            assert_eq!(body["duplicateOf"], first.to_string());
        }
    }
    let count = mock.state.lock().unwrap().send_count;
    assert_eq!(
        count, 1,
        "current UI retry construction duplicates customer email after a lost acknowledgement"
    );
}

#[tokio::test]
async fn review_gemini_email_message_markup_is_escaped() {
    let mock = Mock::start().await;
    let (_f, app, auth, invoice) = setup(&mock).await;
    let markup = "<img src=\"https://attacker.invalid/pixel\">";
    let (status, _) = call(
        &app,
        "POST",
        &format!("/erp/billing/invoices/{invoice}/send-email?tenant=acme"),
        json!({
            "requestId": Uuid::new_v4(),
            "recipient": "customer@example.com",
            "message": markup
        }),
        Some(&auth),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let unescaped = mock.state.lock().unwrap().last_html.contains(markup);
    assert!(
        !unescaped,
        "untrusted message markup was inserted into the provider HTML unchanged"
    );
}

#[tokio::test]
async fn review_gemini_email_restore_hold_refuses_transmission() {
    let mock = Mock::start().await;
    let (f, app, auth, invoice) = setup(&mock).await;

    // Simulate post-restore hold
    sqlx::query("UPDATE archava_billing.gateway_runtime SET held=true WHERE singleton")
        .execute(&f.admin_pool)
        .await
        .unwrap();

    let (status, body) = call(
        &app,
        "POST",
        &format!("/erp/billing/invoices/{invoice}/send-email?tenant=acme"),
        json!({
            "requestId": Uuid::new_v4(),
            "recipient": "customer@example.com",
            "subject": "During hold"
        }),
        Some(&auth),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "Email transmission must be refused while restore hold is active: {body}"
    );
    assert_eq!(
        body["reason"], "outbox_unavailable",
        "Reason must indicate outbox unavailable during hold"
    );
    assert_eq!(mock.state.lock().unwrap().send_count, 0);

    // Operator reconciles and releases hold
    sqlx::query("UPDATE archava_billing.gateway_runtime SET held=false WHERE singleton")
        .execute(&f.admin_pool)
        .await
        .unwrap();

    let (status2, body2) = call(
        &app,
        "POST",
        &format!("/erp/billing/invoices/{invoice}/send-email?tenant=acme"),
        json!({
            "requestId": Uuid::new_v4(),
            "recipient": "customer@example.com",
            "subject": "After hold released"
        }),
        Some(&auth),
    )
    .await;

    assert_eq!(status2, StatusCode::OK, "Body: {body2}");
    assert_eq!(mock.state.lock().unwrap().send_count, 1);
}

#[tokio::test]
async fn review_c10_05_expired_idempotency_window_refuses_retry_and_holds() {
    let mock = Mock::start().await;
    let (f, app, auth, invoice) = setup(&mock).await;

    let request_id = Uuid::new_v4();
    let email_id = Uuid::new_v4();

    // Insert an unconfirmed transmission record from 25 hours ago (>24h window)
    sqlx::query(
        "INSERT INTO archava_billing.email_outbox \
         (tenant_id, id, invoice_id, actor_id, request_id, recipient, subject, payload_sha256, state, provider, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'invoking', 'resend', clock_timestamp() - interval '25 hours', clock_timestamp() - interval '25 hours')",
    )
    .bind("acme")
    .bind(email_id)
    .bind(invoice)
    .bind("alice")
    .bind(request_id)
    .bind("customer@example.com")
    .bind("Invoice from acme")
    .bind("0000000000000000000000000000000000000000000000000000000000000000")
    .execute(&f.admin_pool)
    .await
    .unwrap();

    // Attempt to retry the expired transmission
    let (status, body) = call(
        &app,
        "POST",
        &format!("/erp/billing/invoices/{invoice}/send-email?tenant=acme"),
        json!({
            "requestId": request_id,
            "recipient": "customer@example.com",
            "subject": "Invoice from acme"
        }),
        Some(&auth),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "Expired (>24h) transmission retry must fail closed: {body}"
    );
    assert_eq!(
        body["reason"], "outbox_unavailable",
        "Must return outbox_unavailable when idempotency window expired"
    );
    assert_eq!(
        mock.state.lock().unwrap().send_count,
        0,
        "Provider must NOT be invoked when idempotency window expired"
    );

    // Verify database record has transitioned to 'held' with error reason
    let row = sqlx::query(
        "SELECT state, error_reason FROM archava_billing.email_outbox WHERE tenant_id = 'acme' AND request_id = $1",
    )
    .bind(request_id)
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();

    let state: String = sqlx::Row::try_get(&row, "state").unwrap();
    let error: Option<String> = sqlx::Row::try_get(&row, "error_reason").unwrap();
    assert_eq!(state, "held");
    assert!(
        error
            .unwrap_or_default()
            .contains("Idempotency window expired (>24h)"),
        "error_reason must record expired idempotency window"
    );
}
