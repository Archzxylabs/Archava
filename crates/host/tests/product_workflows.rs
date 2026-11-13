//! Product effects, current authority, confirmed input, atomic rollback and replay.
mod common;
use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::{http::StatusCode, Router};
use serde_json::{json, Value};
use uuid::Uuid;
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
async fn setup() -> (Fixture, Router, Value, Value) {
    let f = provision().await;
    for actor in ["alice", "bob"] {
        sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES($1,$2,$3,true,1)").bind(actor).bind(format!("{actor}@archava.test")).bind(common::HASH).execute(&f.admin_pool).await.unwrap();
        for module in archava_domain::operator::catalogue()["modules"]
            .as_array()
            .unwrap()
        {
            let namespace = module["namespace"].as_str().unwrap();
            let permissions: Vec<&str> = module["permissions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            let statement=format!("INSERT INTO {namespace}.memberships(tenant_id,principal_id,kind,permissions,scope,active) VALUES('acme-retail',$1,'human',$2,'all',true)");
            sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
                .bind(actor)
                .bind(permissions)
                .execute(&f.admin_pool)
                .await
                .unwrap();
        }
    }
    sqlx::query("UPDATE archava_commerce.memberships SET role='owner',capability='enterprise'")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    let state = AppState::try_new_erp(
        f.runtime_pool.clone(),
        Ledger {
            namespace: "archava_commerce",
            ledger: "commerce",
            tables: TABLES,
        },
        &f.commerce_ledger,
        &f.identity_ledger,
        &[7; 32],
        &f.purchasing_ledger,
        &f.erp_readiness(),
    )
    .await
    .unwrap();
    let app = archava_host::router(state);
    let alice = common::login(&app, "alice").await;
    let bob = common::login(&app, "bob").await;
    (f, app, alice, bob)
}
async fn post(app: &Router, path: &str, body: Value, session: &Value) -> Value {
    let (status, body) = common::call(app, "POST", path, body, Some(session)).await;
    assert!(status.is_success(), "{path}: {status} {body}");
    body
}
async fn review(app: &Router, session: &Value, kind: &str, columns: Value, rows: Value) -> Value {
    post(app,"/api/migration/reconcile",json!({"tenant":"acme-retail","entity_kind":kind,"table":{"columns":columns,"rows":rows},"custom_overrides":null}),session).await
}
fn confirmation(plan: &Value) -> Value {
    json!({"tenant":"acme-retail","batch_id":plan["batch_id"],"plan_hash":plan["plan_hash"],"confirmed":true})
}
#[tokio::test]
async fn import_binds_the_review_to_actor_tenant_and_hash_then_commits_atomically_once() {
    let (f, app, alice, bob) = setup().await;
    let plan = review(
        &app,
        &alice,
        "crm_contacts",
        json!(["name", "email"]),
        json!([["Synthetic Import", "import@archava.test"]]),
    )
    .await;
    let input = confirmation(&plan);
    let mut foreign = input.clone();
    foreign["tenant"] = json!("other");
    for body in [
        foreign,
        json!({"tenant":"acme-retail","batch_id":plan["batch_id"],"plan_hash":"forged","confirmed":true}),
        json!({"tenant":"acme-retail","batch_id":plan["batch_id"],"plan_hash":plan["plan_hash"],"confirmed":false}),
    ] {
        let (status, _) =
            common::call(&app, "POST", "/api/migration/commit", body, Some(&alice)).await;
        assert!(status.is_client_error());
    }
    let (status, _) = common::call(
        &app,
        "POST",
        "/api/migration/commit",
        input.clone(),
        Some(&bob),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let receipt = post(&app, "/api/migration/commit", input.clone(), &alice).await;
    assert_eq!(receipt["status"], "committed_atomic");
    assert_eq!(
        post(&app, "/api/migration/commit", input, &alice).await,
        receipt
    );
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_crm.contacts WHERE full_name='Synthetic Import'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    let (_, status) = common::call(
        &app,
        "GET",
        &format!(
            "/api/workflows/status?tenant=acme-retail&kind=import&id={}",
            plan["batch_id"].as_str().unwrap()
        ),
        json!({}),
        Some(&alice),
    )
    .await;
    assert_eq!(status["receipt"], receipt);
    let products = review(
        &app,
        &alice,
        "catalog_products",
        json!(["sku", "title", "price"]),
        json!([
            ["DUP-IMPORT", "First", "Rp 150.000"],
            ["DUP-IMPORT", "Second", "10"]
        ]),
    )
    .await;
    assert_eq!(products["records"][0]["price"], 150000);
    let (status, _) = common::call(
        &app,
        "POST",
        "/api/migration/commit",
        confirmation(&products),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_commerce.products WHERE sku='DUP-IMPORT'")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(
        count, 0,
        "the first row must roll back when a later row fails"
    );
    let cancelled=post(&app,"/api/workflows/cancel",json!({"tenant":"acme-retail","id":products["batch_id"],"kind":"import","plan_hash":products["plan_hash"]}),&alice).await;
    assert_eq!(cancelled["status"], "cancelled");
    let single = review(
        &app,
        &alice,
        "catalog_products",
        json!(["sku", "title", "price"]),
        json!([["OPENING-STOCK", "Opening product", "12000"]]),
    )
    .await;
    post(&app, "/api/migration/commit", confirmation(&single), &alice).await;
    let stock = review(
        &app,
        &alice,
        "inventory_stock",
        json!(["sku", "quantity", "unit_cost_minor"]),
        json!([["OPENING-STOCK", "5", "4000"]]),
    )
    .await;
    post(&app, "/api/migration/commit", confirmation(&stock), &alice).await;
    let quantity:i32=sqlx::query_scalar("SELECT b.quantity FROM archava_commerce.stock_balances b JOIN archava_commerce.products p ON p.tenant_id=b.tenant_id AND p.id=b.product_id WHERE p.sku='OPENING-STOCK'").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(quantity, 5);
    let(status,_)=common::call(&app,"POST","/api/migration/reconcile",json!({"tenant":"acme-retail","entity_kind":"inventory_stock","table":{"columns":["sku","quantity","unit_cost_minor"],"rows":[["OPENING-STOCK","3","4000"]]},"custom_overrides":null}),Some(&alice)).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "opening inventory cannot silently add to an existing balance"
    );
    sqlx::query("UPDATE archava_crm.memberships SET active=false WHERE principal_id='alice'")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    let (status, _) = common::call(
        &app,
        "POST",
        "/api/migration/commit",
        confirmation(&plan),
        Some(&alice),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "replay still needs current authority"
    );
}
#[tokio::test]
async fn feed_observes_real_stock_and_dispatches_one_recoverable_purchase_draft() {
    let (f, app, alice, bob) = setup().await;
    let product=post(&app,"/erp/commerce/commands",json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"command":{"operation":"product.create","sku":"FEED-STOCK","title":"Real stock card","unit":"unit","trackStock":true,"price":10000}}),&alice).await;
    let product = product["receipt"]["id"].clone();
    let supplier=post(&app,"/erp/purchasing/commands",json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"command":{"operation":"supplier.create","code":"FEED-SUPPLIER","name":"Real supplier"}}),&alice).await;
    post(&app,"/erp/purchasing/commands",json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"command":{"operation":"offer.create","supplierId":supplier["receipt"]["id"],"productId":product,"unitCost":5000}}),&alice).await;
    let path = "/api/v1/feed/action-cards?tenant=acme-retail&profile=retail_commerce";
    let (status, cards) = common::call(&app, "GET", path, json!({}), Some(&alice)).await;
    assert_eq!(status, StatusCode::OK, "{cards}");
    let card = &cards["feed"]["cards"][0];
    assert_eq!(card["observedQuantity"], 0);
    assert_eq!(card["quantity"], 10);
    assert_eq!(cards["feed"]["source"], "tenant_database");
    let (_, same) = common::call(&app, "GET", path, json!({}), Some(&alice)).await;
    assert_eq!(same["feed"]["cards"][0]["id"], card["id"]);
    let path = format!(
        "/api/v1/feed/action-cards/{}/execute",
        card["id"].as_str().unwrap()
    );
    let body = json!({"tenant":"acme-retail","plan_hash":card["plan_hash"],"confirmed":true});
    let (status, _) = common::call(&app, "POST", &path, body.clone(), Some(&bob)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Fail only the checkpoint after the independent domain transaction commits.
    sqlx::raw_sql("CREATE FUNCTION archava_owner.product_test_failure() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'owned checkpoint failure'; END $$; CREATE TRIGGER product_test_failure BEFORE UPDATE ON archava_owner.reviewed_workflows FOR EACH ROW EXECUTE FUNCTION archava_owner.product_test_failure()").execute(&f.admin_pool).await.unwrap();
    let (status, _) = common::call(&app, "POST", &path, body.clone(), Some(&alice)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.orders")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(
        count, 1,
        "business commit survives a failed workflow receipt checkpoint"
    );
    sqlx::query("DROP TRIGGER product_test_failure ON archava_owner.reviewed_workflows")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    let receipt = post(&app, &path, body.clone(), &alice).await;
    assert_eq!(post(&app, &path, body, &alice).await, receipt);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.orders")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let (_, cards) = common::call(
        &app,
        "GET",
        "/api/v1/feed/action-cards?tenant=acme-retail",
        json!({}),
        Some(&alice),
    )
    .await;
    assert!(
        cards["feed"]["cards"].as_array().unwrap().is_empty(),
        "an existing draft suppresses another reorder"
    );
    sqlx::query(
        "UPDATE archava_purchasing.memberships SET scope='assigned' WHERE principal_id='bob'",
    )
    .execute(&f.admin_pool)
    .await
    .unwrap();
    let (status, cards) = common::call(
        &app,
        "GET",
        "/api/v1/feed/action-cards?tenant=acme-retail",
        json!({}),
        Some(&bob),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cards}");
    let card = &cards["feed"]["cards"][0];
    assert!(
        card["id"].is_string(),
        "another actor's hidden draft must not change this scope's feed"
    );
    let path = format!(
        "/api/v1/feed/action-cards/{}/execute",
        card["id"].as_str().unwrap()
    );
    post(
        &app,
        &path,
        json!({"tenant":"acme-retail","plan_hash":card["plan_hash"],"confirmed":true}),
        &bob,
    )
    .await;
    let owners: Vec<String> =
        sqlx::query_scalar("SELECT owner_id FROM archava_purchasing.orders ORDER BY owner_id")
            .fetch_all(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(owners, vec!["alice", "bob"]);
}
#[tokio::test]
async fn business_mission_requires_approval_refuses_forged_results_and_executes_once() {
    let (f, app, alice, _) = setup().await;
    let mission=post(&app,"/api/v1/cockpit/missions",json!({"tenant_id":"acme-retail","mission_code":"real-business","title":"Create product","description":"Confirmed synthetic business effect","auto_launch":true,"steps":[{"step_index":0,"step_name":"Create stock product","tool_name":"commerce.product.create","payload_input":{"sku":"MISSION-PRODUCT","title":"Mission product","unit":"unit","trackStock":true,"price":5000},"depends_on":[]}]}),&alice).await;
    let id = mission["id"].as_str().unwrap();
    let path = format!("/api/v1/cockpit/missions/{id}/run-next?tenant=acme-retail");
    let (status, _) = common::call(&app, "POST", &path, json!({"stepIndex":0}), Some(&alice)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    post(
        &app,
        &format!("/api/v1/cockpit/missions/{id}/steps/0/approve?tenant=acme-retail"),
        json!({}),
        &alice,
    )
    .await;
    let(status,_)=common::call(&app,"POST",&format!("/api/v1/cockpit/missions/{id}/steps/0/complete?tenant=acme-retail"),json!({"agent_role":"fake","success":true,"high_signal_summary":"Pretend write","output_payload":{},"token_cost":0}),Some(&alice)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    sqlx::raw_sql("CREATE FUNCTION archava_cockpit.product_test_failure() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'owned checkpoint failure'; END $$; CREATE TRIGGER product_test_failure BEFORE UPDATE ON archava_cockpit.flight_steps FOR EACH ROW EXECUTE FUNCTION archava_cockpit.product_test_failure()").execute(&f.admin_pool).await.unwrap();
    let (status, _) = common::call(&app, "POST", &path, json!({"stepIndex":0}), Some(&alice)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_commerce.products WHERE sku='MISSION-PRODUCT'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(
        count, 1,
        "a lost checkpoint must not lose the committed business reference"
    );
    sqlx::query("DROP TRIGGER product_test_failure ON archava_cockpit.flight_steps")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    let first = post(&app, &path, json!({"stepIndex":0}), &alice).await;
    assert_eq!(first["status"], "touchdown");
    assert_eq!(first["steps"][0]["payload_output"]["verified"], true);
    assert_eq!(
        post(&app, &path, json!({"stepIndex":0}), &alice).await,
        first
    );
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_commerce.products WHERE sku='MISSION-PRODUCT'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn accommodation_mission_binds_its_bare_receipt_via_original_request_status() {
    let (f, app, alice, _) = setup().await;
    let mission=post(&app,"/api/v1/cockpit/missions",json!({"tenant_id":"acme-retail","mission_code":"real-property","title":"Create property","description":"Confirmed synthetic property","auto_launch":true,"steps":[{"step_index":0,"step_name":"Create property","tool_name":"accommodation.property.create","payload_input":{"title":"Mission property","timezone":"Asia/Jakarta","checkInTime":"14:00","checkOutTime":"11:00"},"depends_on":[]}]}),&alice).await;
    let id = mission["id"].as_str().unwrap();
    post(
        &app,
        &format!("/api/v1/cockpit/missions/{id}/steps/0/approve?tenant=acme-retail"),
        json!({}),
        &alice,
    )
    .await;
    let path = format!("/api/v1/cockpit/missions/{id}/run-next?tenant=acme-retail");
    let first = post(&app, &path, json!({"stepIndex":0}), &alice).await;
    assert_eq!(first["status"], "touchdown", "{first}");
    assert_eq!(first["steps"][0]["payload_output"]["verified"], true);
    assert_eq!(
        post(&app, &path, json!({"stepIndex":0}), &alice).await,
        first
    );
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_accommodation.properties WHERE title='Mission property'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn all_thirteen_native_contracts_execute_real_approved_business_commands() {
    let (f, app, alice, _) = setup().await;
    let contact=post(&app,"/crm/commands",json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"command":{"operation":"contact.create","data":{"fullName":"Contract customer"}}}),&alice).await;
    let contact = contact["receipt"]["id"].clone();
    let cases = [
        (
            "organizations",
            "organizations",
            "organizations",
            "create_organization",
            json!({"name":"Contract organization"}),
        ),
        (
            "proposals",
            "proposals",
            "proposals",
            "create_proposal",
            json!({"title":"Contract proposal","summary":"Reviewed scope","scopeItems":[{"title":"Delivery","description":"Synthetic scope","estimatedDays":1,"estimatedValue":1000,"deliverableType":"service"}]}),
        ),
        (
            "projects",
            "projects",
            "projects",
            "project.create",
            json!({"title":"Contract project","scopeSummary":"Reviewed delivery"}),
        ),
        (
            "foodservice",
            "foodservice",
            "tables",
            "table.create",
            json!({"code":"TABLE_1","title":"Contract table","seats":4}),
        ),
        (
            "scheduling",
            "scheduling",
            "resources",
            "resource.create",
            json!({"title":"Contract resource","kind":"staff","timezone":"Asia/Jakarta"}),
        ),
        (
            "accommodation",
            "accommodation",
            "properties",
            "property.create",
            json!({"title":"Contract property","timezone":"Asia/Jakarta","checkInTime":"14:00","checkOutTime":"11:00"}),
        ),
        (
            "assets",
            "assets",
            "assets",
            "create_asset",
            json!({"code":"ASSET-1","name":"Contract asset","category":"equipment","ownership":"business_owned","condition":"good"}),
        ),
        (
            "subscriptions",
            "subscriptions",
            "subscriptions",
            "create_subscription",
            json!({"contactId":contact,"planCode":"PLAN-1","planName":"Contract membership","billingInterval":"monthly","unitPrice":10000,"currency":"IDR","quantity":1,"initialEntitlements":[]}),
        ),
        (
            "attendance",
            "attendance",
            "sessions",
            "create_session",
            json!({"title":"Contract session","sessionCode":"SESSION-1","startTime":"2030-01-01T10:00:00Z","endTime":"2030-01-01T11:00:00Z","capacity":10}),
        ),
        (
            "laundry",
            "laundry",
            "orders",
            "create_order",
            json!({"orderCode":"L-1","kind":"item","currency":"IDR","lines":[{"itemRef":"shirt","quantity":2,"unit":"piece","unitPriceMinor":1000}]}),
        ),
        (
            "field",
            "field",
            "jobs",
            "create_job",
            json!({"job":{"jobId":"contract-job-1","customerRef":"customer-1","siteRef":"site-main","summary":"Synthetic service","priority":"normal","appointmentRef":"appointment-1"}}),
        ),
        (
            "fulfillment",
            "fulfillment",
            "jobs",
            "create_job",
            json!({"jobCode":"F-1","kind":"dispatch","title":"Contract dispatch"}),
        ),
        (
            "education",
            "edu",
            "enrollments",
            "admit_applicant",
            json!({"enrollment":{"enrollmentId":"contract-enrollment-1","studentRef":"student-1","classRef":"class-main","termRef":"term-current"}}),
        ),
    ];
    for (module, namespace, table, action, values) in cases {
        let target = format!("{module}.{action}");
        let mission=post(&app,"/api/v1/cockpit/missions",json!({"tenant_id":"acme-retail","mission_code":format!("contract-{module}"),"title":target,"description":"Reviewed native contract","auto_launch":true,"steps":[{"step_index":0,"step_name":target,"tool_name":target,"payload_input":values,"depends_on":[]}]}),&alice).await;
        let id = mission["id"].as_str().unwrap();
        post(
            &app,
            &format!("/api/v1/cockpit/missions/{id}/steps/0/approve?tenant=acme-retail"),
            json!({}),
            &alice,
        )
        .await;
        let path = format!("/api/v1/cockpit/missions/{id}/run-next?tenant=acme-retail");
        let receipt = post(&app, &path, json!({"stepIndex":0}), &alice).await;
        assert_eq!(receipt["status"], "touchdown", "{target}: {receipt}");
        assert_eq!(
            receipt["steps"][0]["payload_output"]["verified"], true,
            "{target}: {receipt}"
        );
        assert_eq!(
            post(&app, &path, json!({"stepIndex":0}), &alice).await,
            receipt
        );
        let statement = format!(
            "SELECT count(*) FROM archava_{namespace}.{table} WHERE tenant_id='acme-retail'"
        );
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(statement.as_str()))
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
        assert_eq!(count, 1, "{target}");
    }
}
