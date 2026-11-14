//! Real router + PostgreSQL + owned HTTP provider. No live merchant is contacted.
mod common;
use archava_host::{
    payments::{PaymentIntegration, PaymentRegistry},
    router,
    router::AllReadiness,
    AppState, Ledger,
};
use archava_pg_fixture::{provision, Fixture};
use archava_providers::midtrans::MidtransClient;
use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use common::{call, login, HASH};
use serde_json::{json, Value};
use sha2::{Digest, Sha512};
use std::sync::{Arc, Mutex};
use uuid::Uuid;
const KEY: &str = "synthetic-owned-payment-fixture-key";
#[derive(Default)]
struct Responses {
    origin: String,
    order: String,
    amount: i64,
    state: String,
    refund: i64,
    create_count: u32,
    status_count: u32,
    malformed_creation: bool,
    revoke_on_status: Option<sqlx::PgPool>,
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
fn signature(order: &str, gross: &str) -> String {
    format!(
        "{:x}",
        Sha512::digest(format!("{order}200{gross}{KEY}").as_bytes())
    )
}
async fn mock_create(
    State(state): State<Arc<Mutex<Responses>>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let mut s = state.lock().unwrap();
    s.create_count += 1;
    s.order = body["transaction_details"]["order_id"]
        .as_str()
        .unwrap()
        .into();
    s.amount = body["transaction_details"]["gross_amount"]
        .as_i64()
        .unwrap();
    if s.malformed_creation {
        return Json(json!({"error":"simulated lost creation response"}));
    }
    Json(json!({"token":"owned-token","redirect_url":format!("{}snap/owned-token",s.origin)}))
}
async fn mock_status(State(state): State<Arc<Mutex<Responses>>>) -> Json<Value> {
    let (result, revoke) = {
        let mut s = state.lock().unwrap();
        s.status_count += 1;
        let gross = format!("{}.00", s.amount);
        let mut result = json!({"order_id":s.order,"transaction_id":"owned-transaction","status_code":"200","gross_amount":gross,"currency":"IDR","fraud_status":"accept","transaction_status":s.state,"signature_key":signature(&s.order,&gross)});
        if s.refund > 0 {
            result["refund_amount"] = json!(format!("{}.00", s.refund));
        }
        (result, s.revoke_on_status.clone())
    };
    if let Some(pool) = revoke {
        sqlx::query(
            "UPDATE archava_billing.memberships SET active=false WHERE principal_id='gateway'",
        )
        .execute(&pool)
        .await
        .unwrap();
    }
    Json(result)
}
impl Mock {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(Responses {
            origin: origin.clone(),
            state: "pending".into(),
            ..Default::default()
        }));
        let app = Router::new()
            .route("/snap/v1/transactions", post(mock_create))
            .route("/v2/{reference}/status", get(mock_status))
            .with_state(state.clone());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            origin,
            state,
            task,
        }
    }
    fn registry(&self) -> PaymentRegistry {
        PaymentRegistry::new(vec![PaymentIntegration {
            id: "merchant".into(),
            tenant: archava_core::tenant::TenantId::new("acme").unwrap(),
            service_id: "gateway".into(),
            client: MidtransClient::for_loopback(&self.origin, KEY.into()).unwrap(),
        }])
        .unwrap()
    }
}
async fn setup(mock: &Mock) -> (Fixture, Router, Value, Uuid) {
    let f = provision().await;
    for user in ["alice", "assigned"] {
        sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES($1,$2,$3,true,1)").bind(user).bind(format!("{user}@archava.test")).bind(HASH).execute(&f.admin_pool).await.unwrap();
        sqlx::query("INSERT INTO archava_billing.memberships(tenant_id,principal_id,kind,permissions,scope,active) VALUES('acme',$1,'human',ARRAY['billing.read','billing.write','billing.settle'],$2,true)").bind(user).bind(if user=="assigned"{"assigned"}else{"all"}).execute(&f.admin_pool).await.unwrap();
    }
    sqlx::query("INSERT INTO archava_billing.memberships(tenant_id,principal_id,kind,permissions,scope,active) VALUES('acme','gateway','service',ARRAY['billing.read','billing.write'],'all',true)").execute(&f.admin_pool).await.unwrap();
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
    .with_payment_registry(mock.registry());
    let app = router(state);
    let session = login(&app, "alice").await;
    let (s,invoice)=call(&app,"POST","/erp/billing/commands",json!({"tenant":"acme","mutation":{"type":"create_invoice","commandId":Uuid::new_v4(),"invoiceCode":"GATEWAY","customerRef":"Synthetic customer","consumerRef":"test","currency":"IDR","lines":[{"description":"Service","quantity":1,"unitMinor":100}]}}),Some(&session)).await;
    assert_eq!(s, StatusCode::OK, "{invoice}");
    let id = Uuid::parse_str(invoice["invoiceId"].as_str().unwrap()).unwrap();
    let (s,b)=call(&app,"POST","/erp/billing/commands",json!({"tenant":"acme","mutation":{"type":"issue_invoice","commandId":Uuid::new_v4(),"invoiceId":id,"expectedRevision":1,"reason":"Synthetic issued invoice"}}),Some(&session)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    (f, app, session, id)
}
fn request(invoice: Uuid, key: Uuid) -> Value {
    json!({"tenant":"acme","invoiceId":invoice,"requestId":key,"integrationId":"merchant"})
}
async fn notification(
    f: &Fixture,
    app: &Router,
    mock: &Mock,
    unsigned_status: &str,
) -> (StatusCode, Value) {
    // Advance the test-only reservation, without sleeping or changing product logic.
    sqlx::query("UPDATE archava_billing.gateway_links SET last_checked_at=NULL")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    let body = {
        let s = mock.state.lock().unwrap();
        let gross = format!("{}.00", s.amount);
        json!({"order_id":s.order,"status_code":"200","gross_amount":gross,"signature_key":signature(&s.order,&gross),"transaction_status":unsigned_status,"transaction_id":"owned-transaction"})
    };
    call(app, "POST", "/api/webhooks/payment/merchant", body, None).await
}
async fn balance(f: &Fixture) -> (String, i64, i64) {
    sqlx::query_as("SELECT status,settled_minor,outstanding_minor FROM archava_billing.invoices WHERE invoice_code='GATEWAY'").fetch_one(&f.admin_pool).await.unwrap()
}

#[tokio::test]
async fn gateway_challenges_unsigned_status_and_conserves_settlement_refunds_and_replays() {
    let mock = Mock::start().await;
    let (f, app, session, invoice) = setup(&mock).await;
    let body = request(invoice, Uuid::new_v4());
    let (a, b) = tokio::join!(
        call(
            &app,
            "POST",
            "/erp/billing/payments",
            body.clone(),
            Some(&session)
        ),
        call(
            &app,
            "POST",
            "/erp/billing/payments",
            body.clone(),
            Some(&session)
        )
    );
    assert_eq!(a.0, StatusCode::OK, "{:?}", a);
    assert_eq!(b.0, StatusCode::OK, "{:?}", b);
    assert_eq!(mock.state.lock().unwrap().create_count, 1);
    assert_eq!(balance(&f).await, ("issued".into(), 0, 100));
    let (s, b) = notification(&f, &app, &mock, "settlement").await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["paymentState"], "pending");
    assert_eq!(balance(&f).await, ("issued".into(), 0, 100));
    mock.state.lock().unwrap().state = "settlement".into();
    let (s, b) = notification(&f, &app, &mock, "pending").await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(balance(&f).await, ("settled".into(), 100, 0));
    notification(&f, &app, &mock, "settlement").await;
    {
        let mut s = mock.state.lock().unwrap();
        s.state = "partial_refund".into();
        s.refund = 40;
    }
    let (s, b) = notification(&f, &app, &mock, "settlement").await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(balance(&f).await, ("issued".into(), 60, 40));
    notification(&f, &app, &mock, "settlement").await;
    {
        let mut s = mock.state.lock().unwrap();
        s.state = "settlement".into();
        s.refund = 0;
    }
    let (s, b) = notification(&f, &app, &mock, "settlement").await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(balance(&f).await, ("issued".into(), 60, 40));
    {
        let mut s = mock.state.lock().unwrap();
        s.state = "refund".into();
        s.refund = 100;
    }
    let (s, b) = notification(&f, &app, &mock, "refund").await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(balance(&f).await, ("issued".into(), 0, 100));
    let totals:(i64,i64,i64,i64)=sqlx::query_as("SELECT received_minor,refunded_minor,allocated_minor,allocation_refunded_minor FROM archava_billing.gateway_links").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(totals, (100, 100, 100, 100));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_billing.payment_events")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 3);
    let assigned = login(&app, "assigned").await;
    let (s, _) = call(
        &app,
        "GET",
        &format!("/erp/billing/payments?tenant=acme&invoiceId={invoice}"),
        Value::Null,
        Some(&assigned),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = call(&app, "POST", "/erp/billing/payments", body, Some(&assigned)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    sqlx::query("UPDATE archava_billing.memberships SET active=false WHERE principal_id='gateway'")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    let before = mock.state.lock().unwrap().status_count;
    let (s, _) = notification(&f, &app, &mock, "settlement").await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(mock.state.lock().unwrap().status_count, before);
}

#[tokio::test]
async fn unknown_creation_keeps_original_order_and_blocks_a_fresh_link_even_after_replay() {
    let mock = Mock::start().await;
    mock.state.lock().unwrap().malformed_creation = true;
    let (f, app, session, invoice) = setup(&mock).await;
    let body = request(invoice, Uuid::new_v4());
    let (s, b) = call(
        &app,
        "POST",
        "/erp/billing/payments",
        body.clone(),
        Some(&session),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["link"]["state"], "creation_unknown");
    assert!(b["link"]["redirectUrl"].is_null());
    let original = b["link"]["orderId"].clone();
    let (s, b) = call(&app, "POST", "/erp/billing/payments", body, Some(&session)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["link"]["orderId"], original);
    assert_eq!(b["idempotencyReplay"], true);
    let (s, _) = call(
        &app,
        "POST",
        "/erp/billing/payments",
        request(invoice, Uuid::new_v4()),
        Some(&session),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(mock.state.lock().unwrap().create_count, 1);
    let (s,_)=call(&app,"POST","/erp/billing/commands",json!({"tenant":"acme","mutation":{"type":"void_invoice","commandId":Uuid::new_v4(),"invoiceId":invoice,"expectedRevision":2,"reason":"Cannot discard an uncertain payment"}}),Some(&session)).await;
    assert!(!s.is_success());
    assert_eq!(balance(&f).await, ("issued".into(), 0, 100));
    let (s, b) = notification(&f, &app, &mock, "settlement").await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(balance(&f).await, ("issued".into(), 0, 100));
    let (s,_)=call(&app,"POST","/api/webhooks/payment",json!({"order_id":"unknown","status_code":"200","gross_amount":"100.00","signature_key":"invalid","transaction_status":"settlement"}),None).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn verified_overpayment_is_retained_as_unapplied_money_and_refund_first_is_net_only() {
    let mock = Mock::start().await;
    let (f, app, session, invoice) = setup(&mock).await;
    let (s, b) = call(
        &app,
        "POST",
        "/erp/billing/payments",
        request(invoice, Uuid::new_v4()),
        Some(&session),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    // Another verified/manual settlement can change the balance while a link
    // exists. The gateway must retain actual funds beyond the remaining debt.
    sqlx::query(
        "UPDATE archava_billing.invoices SET settled_minor=20,outstanding_minor=80 WHERE id=$1",
    )
    .bind(invoice)
    .execute(&f.admin_pool)
    .await
    .unwrap();
    mock.state.lock().unwrap().state = "settlement".into();
    let (s, b) = notification(&f, &app, &mock, "settlement").await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["requiresReview"], true);
    assert_eq!(balance(&f).await, ("settled".into(), 100, 0));
    let (s, b) = call(
        &app,
        "GET",
        &format!("/erp/billing/payments?tenant=acme&invoiceId={invoice}"),
        Value::Null,
        Some(&session),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["links"][0]["unappliedMinor"], 20);
    {
        let mut s = mock.state.lock().unwrap();
        s.state = "partial_refund".into();
        s.refund = 30;
    }
    let (s, b) = notification(&f, &app, &mock, "refund").await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["requiresReview"], false);
    assert_eq!(balance(&f).await, ("issued".into(), 90, 10));
}

#[tokio::test]
async fn original_gateway_receipts_recheck_current_scope_and_restore_hold_blocks_new_calls() {
    let mock = Mock::start().await;
    let (f, app, alice, invoice) = setup(&mock).await;
    let assigned = login(&app, "assigned").await;
    sqlx::query("UPDATE archava_billing.memberships SET scope='all' WHERE principal_id='assigned'")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    let key = Uuid::new_v4();
    let body = request(invoice, key);
    sqlx::query("UPDATE archava_billing.gateway_runtime SET held=true")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    let (s, b) = call(
        &app,
        "POST",
        "/erp/billing/payments",
        body.clone(),
        Some(&assigned),
    )
    .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{b}");
    assert_eq!(mock.state.lock().unwrap().create_count, 0);
    sqlx::query("UPDATE archava_billing.gateway_runtime SET held=false")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    let (s, b) = call(
        &app,
        "POST",
        "/erp/billing/payments",
        body.clone(),
        Some(&assigned),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let path = format!("/erp/billing/payments/status?tenant=acme&requestId={key}");
    let (s, b) = call(&app, "GET", &path, Value::Null, Some(&assigned)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["status"], "recorded");
    let (_, b) = call(&app, "GET", &path, Value::Null, Some(&alice)).await;
    assert_eq!(b["status"], "unknown");
    sqlx::query(
        "UPDATE archava_billing.memberships SET scope='assigned' WHERE principal_id='assigned'",
    )
    .execute(&f.admin_pool)
    .await
    .unwrap();
    let (_, b) = call(&app, "GET", &path, Value::Null, Some(&assigned)).await;
    assert_eq!(b["status"], "unknown");
    assert!(b.get("link").is_none());
    let (s, _) = call(&app, "POST", "/erp/billing/payments", body, Some(&assigned)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(mock.state.lock().unwrap().create_count, 1);
}

#[tokio::test]
async fn refund_first_records_external_gross_and_refund_but_allocates_only_the_remaining_net() {
    let mock = Mock::start().await;
    let (f, app, session, invoice) = setup(&mock).await;
    let (s, b) = call(
        &app,
        "POST",
        "/erp/billing/payments",
        request(invoice, Uuid::new_v4()),
        Some(&session),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    {
        let mut s = mock.state.lock().unwrap();
        s.state = "partial_refund".into();
        s.refund = 40;
    }
    let (s, b) = notification(&f, &app, &mock, "pending").await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(balance(&f).await, ("issued".into(), 60, 40));
    let totals:(i64,i64,i64,i64)=sqlx::query_as("SELECT received_minor,refunded_minor,allocated_minor,allocation_refunded_minor FROM archava_billing.gateway_links").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(totals, (100, 40, 60, 0));
    let (s, b) = notification(&f, &app, &mock, "settlement").await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_billing.payment_events")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn service_revocation_while_get_status_is_in_flight_refuses_all_financial_effects() {
    let mock = Mock::start().await;
    let (f, app, session, invoice) = setup(&mock).await;
    let (s, b) = call(
        &app,
        "POST",
        "/erp/billing/payments",
        request(invoice, Uuid::new_v4()),
        Some(&session),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    {
        let mut s = mock.state.lock().unwrap();
        s.state = "settlement".into();
        s.revoke_on_status = Some(f.admin_pool.clone());
    }
    let (s, b) = notification(&f, &app, &mock, "settlement").await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{b}");
    assert_eq!(mock.state.lock().unwrap().status_count, 1);
    assert_eq!(balance(&f).await, ("issued".into(), 0, 100));
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_billing.payment_events")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

#[tokio::test]
async fn a_write_only_human_is_refused_before_any_external_gateway_invocation() {
    let mock = Mock::start().await;
    let (f, app, session, invoice) = setup(&mock).await;
    sqlx::query("UPDATE archava_billing.memberships SET permissions=ARRAY['billing.write'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    let (s, b) = call(
        &app,
        "POST",
        "/erp/billing/payments",
        request(invoice, Uuid::new_v4()),
        Some(&session),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{b}");
    assert_eq!(
        mock.state.lock().unwrap().create_count,
        0,
        "a refusal must not follow an already-dispatched checkout"
    );
}

#[tokio::test]
async fn exact_payment_receipt_recovery_is_not_limited_by_the_recent_history_window() {
    let mock = Mock::start().await;
    let (f, app, session, invoice) = setup(&mock).await;
    let key = Uuid::new_v4();
    let body = request(invoice, key);
    let (s, b) = call(
        &app,
        "POST",
        "/erp/billing/payments",
        body.clone(),
        Some(&session),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let original = b["link"]["id"].clone();
    for _ in 0..26 {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO archava_billing.gateway_links(tenant_id,id,invoice_id,integration_id,service_id,actor_id,request_id,order_id,amount_minor,invoice_revision,state,closed_at) VALUES('acme',$1,$2,'merchant','gateway','alice',$3,$4,100,2,'failed',clock_timestamp())")
            .bind(id).bind(invoice).bind(Uuid::new_v4()).bind(format!("archava-{id}")).execute(&f.admin_pool).await.unwrap();
    }
    let (s, b) = call(&app, "POST", "/erp/billing/payments", body, Some(&session)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["link"]["id"], original);
    assert_eq!(mock.state.lock().unwrap().create_count, 1);
}

/// A manual part payment and the gateway on one invoice. The link is created
/// for what is still outstanding after the manual receipt; while that link is
/// open and unpaid a manual receipt is refused, so the customer cannot pay the
/// same money twice; the gateway then settles the rest. The gateway's event is
/// never reversed by hand, the manual one is, and once a later link has failed
/// (expired) a manual receipt is accepted again.
#[tokio::test]
async fn a_manual_part_payment_leaves_the_rest_to_a_link_and_an_open_link_holds_manual_receipts() {
    let mock = Mock::start().await;
    let (f, app, session, invoice) = setup(&mock).await;
    let today: String =
        sqlx::query_scalar("SELECT (clock_timestamp() AT TIME ZONE 'UTC')::date::text")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    let bill = |command: Value| {
        let (app, session) = (app.clone(), session.clone());
        async move {
            let mut command = command;
            command["commandId"] = json!(Uuid::new_v4());
            command["invoiceId"] = json!(invoice);
            call(
                &app,
                "POST",
                "/erp/billing/commands",
                json!({"tenant":"acme","mutation":command}),
                Some(&session),
            )
            .await
        }
    };
    let receipt = |revision: i64, amount: i64| json!({"type":"record_manual_receipt","expectedRevision":revision,"amountMinor":amount,"method":"bank_transfer","receivedOn":today,"reference":"TRF-1","reason":"Seen on the statement"});
    let (s, manual) = bill(receipt(2, 40)).await;
    assert_eq!(s, StatusCode::OK, "{manual}");
    assert_eq!(balance(&f).await, ("issued".into(), 40, 60));

    // The link freezes the remainder, not the invoice total.
    let (s, link) = call(
        &app,
        "POST",
        "/erp/billing/payments",
        request(invoice, Uuid::new_v4()),
        Some(&session),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{link}");
    assert_eq!(mock.state.lock().unwrap().amount, 60);
    let (s, refused) = bill(receipt(3, 10)).await;
    assert_eq!(s, StatusCode::CONFLICT, "{refused}");
    assert_eq!(
        refused["detail"]["code"], "manual_receipt_open_payment_link",
        "{refused}"
    );
    assert_eq!(balance(&f).await, ("issued".into(), 40, 60));

    mock.state.lock().unwrap().state = "settlement".into();
    let (s, b) = notification(&f, &app, &mock, "settlement").await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(balance(&f).await, ("settled".into(), 100, 0));
    let events: Vec<(String, Option<String>, i64, Uuid)> = sqlx::query_as("SELECT provider_id,manual_method,amount_minor,id FROM archava_billing.payment_events ORDER BY verified_at").fetch_all(&f.admin_pool).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(
        (events[0].0.as_str(), events[0].1.as_deref(), events[0].2),
        ("manual:bank_transfer", Some("bank_transfer"), 40)
    );
    assert_eq!(
        (events[1].0.as_str(), events[1].1.as_deref(), events[1].2),
        ("midtrans:merchant", None, 60)
    );

    // The gateway's money goes back through the gateway, never by hand.
    let reverse = |revision: i64, event: Uuid, amount: i64| json!({"type":"reverse_manual_receipt","expectedRevision":revision,"receiptEventId":event,"amountMinor":amount,"reason":"Returned"});
    let (s, refused) = bill(reverse(4, events[1].3, 60)).await;
    assert_eq!(s, StatusCode::CONFLICT, "{refused}");
    assert_eq!(
        refused["detail"]["code"], "manual_reversal_not_manual",
        "{refused}"
    );
    let (s, reversed) = bill(reverse(4, events[0].3, 40)).await;
    assert_eq!(s, StatusCode::OK, "{reversed}");
    assert_eq!(balance(&f).await, ("issued".into(), 60, 40));

    // A second link that fails (expires) is closed and frees the invoice.
    let (s, link) = call(
        &app,
        "POST",
        "/erp/billing/payments",
        request(invoice, Uuid::new_v4()),
        Some(&session),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{link}");
    assert_eq!(mock.state.lock().unwrap().amount, 40);
    let (s, refused) = bill(receipt(5, 40)).await;
    assert_eq!(
        refused["detail"]["code"], "manual_receipt_open_payment_link",
        "{s} {refused}"
    );
    mock.state.lock().unwrap().state = "expire".into();
    let (s, b) = notification(&f, &app, &mock, "expire").await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (s, paid) = bill(receipt(5, 40)).await;
    assert_eq!(s, StatusCode::OK, "{paid}");
    assert_eq!(balance(&f).await, ("settled".into(), 100, 0));
}
