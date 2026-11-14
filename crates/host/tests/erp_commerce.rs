//! Native commerce journeys over the real router and a non-owner forced-RLS database.
mod common;
use archava_host::{router, AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::{http::StatusCode, Router};
use common::{call, login, HASH};
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
async fn setup() -> (Fixture, Router, Value) {
    setup_fixture(provision().await).await
}
async fn setup_fixture(f: Fixture) -> (Fixture, Router, Value) {
    for (user, tenant, scope, permissions) in [
        (
            "alice",
            "acme",
            "all",
            vec![
                "catalog.read",
                "catalog.write",
                "stock.read",
                "stock.write",
                "orders.read",
                "orders.write",
                "returns.write",
                "audit.read",
            ],
        ),
        (
            "viewer",
            "acme",
            "all",
            vec!["catalog.read", "stock.read", "orders.read"],
        ),
        (
            "assigned",
            "acme",
            "assigned",
            vec!["catalog.read", "orders.read", "orders.write"],
        ),
        (
            "mallory",
            "other",
            "all",
            vec![
                "catalog.read",
                "catalog.write",
                "orders.read",
                "orders.write",
            ],
        ),
    ] {
        sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES($1,$2,$3,true,1)").bind(user).bind(format!("{user}@archava.test")).bind(HASH).execute(&f.admin_pool).await.unwrap();
        sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope,active,role,capability) VALUES($1,$2,'human',$3,$4,true,'owner','enterprise')").bind(tenant).bind(user).bind(permissions).bind(scope).execute(&f.admin_pool).await.unwrap();
    }
    let state = AppState::try_new(
        f.runtime_pool.clone(),
        Ledger {
            namespace: "archava_commerce",
            ledger: "Commerce",
            tables: TABLES,
        },
        &f.commerce_ledger,
        &f.identity_ledger,
        &[7u8; 32],
    )
    .await
    .unwrap();
    let app = router(state);
    let session = login(&app, "alice").await;
    (f, app, session)
}
fn submission(command: Value) -> Value {
    json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":command})
}
async fn command(app: &Router, session: &Value, command: Value) -> Value {
    let (status, body) = call(
        app,
        "POST",
        "/erp/commerce/commands",
        submission(command),
        Some(session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["receipt"].clone()
}
async fn product(app: &Router, session: &Value, sku: &str, tracked: bool) -> String {
    command(app,session,json!({"operation":"product.create","sku":sku,"title":format!("Product {sku}"),"unit":"unit","trackStock":tracked,"price":10000})).await["id"].as_str().unwrap().into()
}
async fn snapshot(app: &Router, session: &Value) -> Value {
    let (status, body) = call(
        app,
        "GET",
        "/erp/commerce/workspace?tenant=acme",
        Value::Null,
        Some(session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}
fn stock(view: &Value, id: &str) -> i64 {
    view["products"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id)
        .unwrap()["quantity"]
        .as_i64()
        .unwrap()
}
#[tokio::test]
async fn physical_order_reservation_fulfillment_partial_return_and_recovery() {
    let (f, app, session) = setup().await;
    let p = product(&app, &session, "PHYSICAL", true).await;
    command(&app,&session,json!({"operation":"stock.adjust","productId":p,"quantity":10,"reason":"Opening inventory count"})).await;
    let created=command(&app,&session,json!({"operation":"order.create","customerName":"Synthetic Customer","lines":[{"productId":p,"quantity":2}]})).await;
    let order = created["id"].as_str().unwrap();
    command(
        &app,
        &session,
        json!({"operation":"order.reserve","id":order,"expectedRevision":1,"holdSeconds":600}),
    )
    .await;
    command(
        &app,
        &session,
        json!({"operation":"order.fulfill","id":order,"expectedRevision":2}),
    )
    .await;
    let view = snapshot(&app, &session).await;
    assert_eq!(stock(&view, &p), 8);
    let row = view["orders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == order)
        .unwrap();
    assert_eq!(row["state"], "fulfilled");
    assert_eq!(row["total"], 20000);
    let line = row["lines"][0]["id"].as_str().unwrap();
    let input = submission(
        json!({"operation":"order.return","id":order,"expectedRevision":3,"lineId":line,"quantity":1,"disposition":"restock","reason":"Returned sealed item"}),
    );
    for _ in 0..2 {
        assert_eq!(
            call(
                &app,
                "POST",
                "/erp/commerce/commands",
                input.clone(),
                Some(&session)
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let view = snapshot(&app, &session).await;
    assert_eq!(stock(&view, &p), 9);
    let status_path = format!(
        "/erp/commerce/commands/status?tenant=acme&requestId={}",
        input["requestId"].as_str().unwrap()
    );
    let (status, receipt) = call(&app, "GET", &status_path, Value::Null, Some(&session)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["status"], "committed");
    assert_eq!(receipt["receipt"]["revision"], 4);
    command(&app,&session,json!({"operation":"order.return","id":order,"expectedRevision":4,"lineId":line,"quantity":1,"disposition":"restock","reason":"Remaining item returned"})).await;
    let view = snapshot(&app, &session).await;
    assert_eq!(stock(&view, &p), 10);
    assert_eq!(view["orders"][0]["state"], "returned");
    let exact_links: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_commerce.inventory_layers l JOIN archava_commerce.inventory_return_costs r ON r.tenant_id=l.tenant_id AND r.id=l.return_cost_id JOIN archava_commerce.inventory_uses u ON u.tenant_id=r.tenant_id AND u.id=r.source_use_id WHERE l.tenant_id='acme' AND u.order_id=$1 AND l.source_request_id=r.request_id AND l.product_id=r.product_id AND l.original_quantity=r.quantity AND l.total_cost_minor IS NOT DISTINCT FROM r.total_cost_minor")
        .bind(Uuid::parse_str(order).unwrap()).fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(exact_links, 2, "Each actual returned portion has its exact original use reference; replay creates none extra");
}

#[tokio::test]
async fn return_provenance_readiness_refuses_disabled_guards_and_exposed_cost_writers() {
    let (f, _, _) = setup().await;
    let open = || {
        AppState::try_new(
            f.runtime_pool.clone(),
            Ledger {
                namespace: "archava_commerce",
                ledger: "Commerce",
                tables: TABLES,
            },
            &f.commerce_ledger,
            &f.identity_ledger,
            &[7u8; 32],
        )
    };
    assert!(open().await.is_ok());
    sqlx::query(
        "ALTER TABLE archava_commerce.inventory_layers DISABLE TRIGGER return_layer_provenance",
    )
    .execute(&f.owner_pool)
    .await
    .unwrap();
    assert!(
        open().await.is_err(),
        "Disabled provenance must refuse current-profile startup"
    );
    sqlx::query(
        "ALTER TABLE archava_commerce.inventory_layers ENABLE TRIGGER return_layer_provenance",
    )
    .execute(&f.owner_pool)
    .await
    .unwrap();
    sqlx::query("GRANT EXECUTE ON FUNCTION archava_commerce.cost_movement() TO archava_runtime")
        .execute(&f.owner_pool)
        .await
        .unwrap();
    assert!(
        open().await.is_err(),
        "Runtime access to the private cost writer must refuse startup"
    );
    sqlx::query("REVOKE EXECUTE ON FUNCTION archava_commerce.cost_movement() FROM archava_runtime")
        .execute(&f.owner_pool)
        .await
        .unwrap();
    assert!(open().await.is_ok());
}

#[tokio::test]
async fn existing_return_layers_upgrade_without_guessed_origins_and_remain_consumable() {
    let (f, app, session) =
        setup_fixture(archava_pg_fixture::provision_pre_return_provenance_profile().await).await;
    let p = product(&app, &session, "PRIOR-RETURN", true).await;
    command(&app,&session,json!({"operation":"stock.adjust","productId":p,"quantity":2,"reason":"Prior operational stock, not a financial opening"})).await;
    let sale=command(&app,&session,json!({"operation":"order.create","customerName":"Prior return customer","lines":[{"productId":p,"quantity":2}]})).await;
    command(
        &app,
        &session,
        json!({"operation":"order.reserve","id":sale["id"],"expectedRevision":1,"holdSeconds":600}),
    )
    .await;
    command(
        &app,
        &session,
        json!({"operation":"order.fulfill","id":sale["id"],"expectedRevision":2}),
    )
    .await;
    let line: Uuid =
        sqlx::query_scalar("SELECT id FROM archava_commerce.order_lines WHERE order_id=$1")
            .bind(Uuid::parse_str(sale["id"].as_str().unwrap()).unwrap())
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    command(&app,&session,json!({"operation":"order.return","id":sale["id"],"expectedRevision":3,"lineId":line,"quantity":1,"disposition":"restock","reason":"Prior return lacks an explicit return-layer reference"})).await;
    let before: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(l) ORDER BY id) FROM archava_commerce.inventory_layers l",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    let upgraded = archava_pg_fixture::apply_pending_commerce_migrations(&f.owner_pool).await;
    assert!(f.commerce_ledger.len() < upgraded.len());
    assert_eq!(
        upgraded.len(),
        archava_pg_fixture::migration_count("archava_commerce")
    );
    assert_eq!(
        archava_pg_fixture::apply_pending_commerce_migrations(&f.owner_pool).await,
        upgraded
    );
    let after:Value=sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(l)-'return_cost_id' ORDER BY id) FROM archava_commerce.inventory_layers l").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(before,after,"Upgrade preserves every original field and attaches no guessed historical account/cost origin");
    let state = AppState::try_new(
        f.runtime_pool.clone(),
        Ledger {
            namespace: "archava_commerce",
            ledger: "Commerce",
            tables: TABLES,
        },
        &upgraded,
        &f.identity_ledger,
        &[7u8; 32],
    )
    .await
    .unwrap();
    let app = router(state);
    command(&app,&session,json!({"operation":"order.return","id":sale["id"],"expectedRevision":4,"lineId":line,"quantity":1,"disposition":"restock","reason":"New return now binds its exact original use"})).await;
    let counts:(i64,i64)=sqlx::query_as("SELECT count(*) FILTER(WHERE return_cost_id IS NULL),count(*) FILTER(WHERE return_cost_id IS NOT NULL) FROM archava_commerce.inventory_layers WHERE source_kind='return'").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(counts, (1, 1));
    let next=command(&app,&session,json!({"operation":"order.create","customerName":"Consume preserved legacy stock","lines":[{"productId":p,"quantity":2}]})).await;
    command(
        &app,
        &session,
        json!({"operation":"order.reserve","id":next["id"],"expectedRevision":1,"holdSeconds":600}),
    )
    .await;
    command(
        &app,
        &session,
        json!({"operation":"order.fulfill","id":next["id"],"expectedRevision":2}),
    )
    .await;
    assert_eq!(stock(&snapshot(&app, &session).await, &p), 0);
    let unknown:i64=sqlx::query_scalar("SELECT count(*) FROM archava_commerce.inventory_layers WHERE source_kind='return' AND total_cost_minor IS NULL").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(
        unknown, 2,
        "Physical reuse preserves unknown cost instead of manufacturing a zero financial balance"
    );
}
#[tokio::test]
async fn services_can_reserve_and_complete_without_inventing_stock_allocations() {
    let (f, app, session) = setup().await;
    let p = product(&app, &session, "SERVICE", false).await;
    let order=command(&app,&session,json!({"operation":"order.create","customerName":"Service Customer","lines":[{"productId":p,"quantity":1}]})).await["id"].as_str().unwrap().to_owned();
    command(
        &app,
        &session,
        json!({"operation":"order.reserve","id":order,"expectedRevision":1,"holdSeconds":600}),
    )
    .await;
    command(
        &app,
        &session,
        json!({"operation":"order.fulfill","id":order,"expectedRevision":2}),
    )
    .await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_commerce.allocations")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(
        snapshot(&app, &session).await["orders"][0]["state"],
        "fulfilled"
    );
}
#[tokio::test]
async fn command_identity_current_grants_tenant_and_assignment_are_enforced() {
    let (f, app, session) = setup().await;
    let input = submission(
        json!({"operation":"product.create","sku":"ONCE","title":"Once","unit":"unit","trackStock":true,"price":100}),
    );
    let (a, first) = call(
        &app,
        "POST",
        "/erp/commerce/commands",
        input.clone(),
        Some(&session),
    )
    .await;
    assert_eq!(a, StatusCode::OK, "{first}");
    let (b, second) = call(
        &app,
        "POST",
        "/erp/commerce/commands",
        input.clone(),
        Some(&session),
    )
    .await;
    assert_eq!(b, StatusCode::OK);
    assert_eq!(first, second);
    let mut changed = input.clone();
    changed["command"]["price"] = json!(200);
    assert_eq!(
        call(
            &app,
            "POST",
            "/erp/commerce/commands",
            changed,
            Some(&session)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let p = first["receipt"]["id"].as_str().unwrap();
    let viewer = login(&app, "viewer").await;
    let adjustment = submission(
        json!({"operation":"stock.adjust","productId":p,"quantity":100,"reason":"Must be refused"}),
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/erp/commerce/commands",
            adjustment,
            Some(&viewer)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let other = login(&app, "mallory").await;
    assert_eq!(
        call(
            &app,
            "GET",
            "/erp/commerce/workspace?tenant=acme",
            Value::Null,
            Some(&other)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let order=command(&app,&session,json!({"operation":"order.create","customerName":"Private Customer","lines":[{"productId":p,"quantity":1}]})).await;
    let assigned = login(&app, "assigned").await;
    assert_eq!(
        snapshot(&app, &assigned).await["orders"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(call(&app,"POST","/erp/commerce/commands",submission(json!({"operation":"order.cancel","id":order["id"],"expectedRevision":1,"reason":"Not mine"})),Some(&assigned)).await.0,StatusCode::FORBIDDEN);
    sqlx::query("UPDATE archava_commerce.memberships SET permissions=ARRAY['catalog.read'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            "/erp/commerce/commands",
            input,
            Some(&session)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_commerce.products")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
#[tokio::test]
async fn concurrent_reservations_cannot_oversell_and_failure_leaves_no_command() {
    let (f, app, session) = setup().await;
    let p = product(&app, &session, "SCARCE", true).await;
    command(
        &app,
        &session,
        json!({"operation":"stock.adjust","productId":p,"quantity":10,"reason":"Count"}),
    )
    .await;
    let a=command(&app,&session,json!({"operation":"order.create","customerName":"A","lines":[{"productId":p,"quantity":8}]})).await;
    let b=command(&app,&session,json!({"operation":"order.create","customerName":"B","lines":[{"productId":p,"quantity":8}]})).await;
    let first = submission(
        json!({"operation":"order.reserve","id":a["id"],"expectedRevision":1,"holdSeconds":600}),
    );
    let second = submission(
        json!({"operation":"order.reserve","id":b["id"],"expectedRevision":1,"holdSeconds":600}),
    );
    let (x, y) = tokio::join!(
        call(
            &app,
            "POST",
            "/erp/commerce/commands",
            first.clone(),
            Some(&session)
        ),
        call(
            &app,
            "POST",
            "/erp/commerce/commands",
            second.clone(),
            Some(&session)
        )
    );
    assert!(
        matches!(
            (x.0, y.0),
            (StatusCode::OK, StatusCode::CONFLICT) | (StatusCode::CONFLICT, StatusCode::OK)
        ),
        "{x:?} {y:?}"
    );
    let held:i64=sqlx::query_scalar("SELECT coalesce(sum(quantity),0)::bigint FROM archava_commerce.allocations WHERE state='held'").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(held, 8);
    let failed = if x.0 == StatusCode::CONFLICT {
        &first
    } else {
        &second
    };
    let recorded: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_commerce.commands WHERE request_id=$1::uuid",
    )
    .bind(failed["requestId"].as_str().unwrap())
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(recorded, 0);
}

// --- Trade currency (docs/architecture/MULTI_CURRENCY.md, stage 1) ---------

/// The whole mounted router, with alice able to register companies and
/// parties, create books and trade.
async fn trade_setup() -> (Fixture, Router, Value) {
    let f = provision().await;
    sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES('alice','alice@archava.test',$1,true,1)").bind(HASH).execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_organizations.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','alice','human',ARRAY['organizations.read','organizations.write','companies.read','companies.write','parties.read','parties.write'],'all')").execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_accounting.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','alice','human',$1,'all')").bind(archava_domain::erp::accounting::PERMISSIONS).execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','alice','human',$1,'all')").bind(archava_adapters::erp::COMMERCE_PERMISSION_VOCABULARY).execute(&f.admin_pool).await.unwrap();
    let state = AppState::try_new_all(
        f.runtime_pool.clone(),
        Ledger {
            namespace: "archava_commerce",
            ledger: "Commerce",
            tables: TABLES,
        },
        archava_host::router::AllReadiness {
            commerce: &f.commerce_ledger,
            identity: &f.identity_ledger,
            crm: &f.crm_ledger,
            owner: &f.owner_ledger,
            purchasing: &f.purchasing_ledger,
            additional_erp: &f.erp_readiness(),
        },
        &[7u8; 32],
    )
    .await
    .unwrap();
    let app = router(state);
    let session = login(&app, "alice").await;
    (f, app, session)
}
async fn organization_command(app: &Router, session: &Value, kind: &str, data: Value) -> Value {
    let (status, body) = call(
        app,
        "POST",
        "/erp/organizations/commands",
        json!({"tenant":"acme","mutation":{"requestId":Uuid::new_v4(),"type":kind,"data":data}}),
        Some(session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["receipt"].clone()
}
/// A reviewed managed company; returns its id.
async fn trade_company(app: &Router, session: &Value, code: &str) -> String {
    let organization = organization_command(
        app,
        session,
        "create_organization",
        json!({"name":format!("Company {code}"),"legalName":format!("Company {code} Official")}),
    )
    .await;
    organization_command(
        app,
        session,
        "company.create_from_organization",
        json!({"code":code,"organizationId":organization["organizationId"],"expectedSourceRevision":1,"reason":"Explicit synthetic legal entity"}),
    )
    .await["companyId"]
        .as_str()
        .unwrap()
        .into()
}
/// A reviewed customer party; returns its id.
async fn trade_buyer(app: &Router, session: &Value) -> String {
    let organization = organization_command(
        app,
        session,
        "create_organization",
        json!({"name":"Currency buyer","legalName":"Currency Buyer Official"}),
    )
    .await;
    organization_command(
        app,
        session,
        "party.create_from_organization",
        json!({"organizationId":organization["organizationId"],"expectedSourceRevision":1,"customer":true,"supplier":false}),
    )
    .await["partyId"]
        .as_str()
        .unwrap()
        .into()
}
async fn trade_book(app: &Router, session: &Value, company: &str, currency: &str) {
    let (status, body) = call(
        app,
        "POST",
        "/erp/accounting/commands",
        json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"type":"book.create","companyId":company,"expectedCompanyRevision":1,"code":format!("BOOK-{currency}"),"name":"Explicit synthetic official book","currency":currency,"sourceAuthority":"native","independentPosting":true,"reason":"Reviewed book setup; no historical import"}}),
        Some(session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
fn book_request(company: &str, currency: &str) -> Value {
    json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"type":"book.create","companyId":company,"expectedCompanyRevision":1,"code":format!("BOOK-{currency}"),"name":"Explicit synthetic official book","currency":currency,"sourceAuthority":"native","independentPosting":true,"reason":"Reviewed book setup; no historical import"}})
}
async fn books(f: &Fixture, company: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT currency FROM archava_accounting.books WHERE company_id=$1::uuid ORDER BY currency",
    )
    .bind(company)
    .fetch_all(&f.admin_pool)
    .await
    .unwrap()
}
/// `book.create` in `currency` is refused as a conflict and writes nothing.
async fn book_refused(f: &Fixture, app: &Router, session: &Value, company: &str, currency: &str) {
    let request = book_request(company, currency);
    let (status, body) = call(
        app,
        "POST",
        "/erp/accounting/commands",
        request.clone(),
        Some(session),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(books(f, company).await, Vec::<String>::new());
    let recorded: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_accounting.commands WHERE request_id=$1::uuid",
    )
    .bind(request["requestId"].as_str().unwrap())
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(recorded, 0, "the refused book leaves no command");
}
/// A non-IDR book for a company that already trades. The book guard
/// (Accounting 015) refuses that through every runtime path, so the database
/// owner disables it to plant the state the older guards still defend against.
async fn planted_book(f: &Fixture, app: &Router, session: &Value, company: &str, currency: &str) {
    sqlx::query("ALTER TABLE archava_accounting.books DISABLE TRIGGER a_currency_book")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    trade_book(app, session, company, currency).await;
    sqlx::query("ALTER TABLE archava_accounting.books ENABLE TRIGGER a_currency_book")
        .execute(&f.admin_pool)
        .await
        .unwrap();
}
async fn company_product(app: &Router, session: &Value, company: &str, sku: &str) -> String {
    command(app,session,json!({"operation":"product.create_for_company","companyId":company,"expectedCompanyRevision":1,"sku":sku,"title":format!("Product {sku}"),"unit":"unit","trackStock":false,"price":1999})).await["id"].as_str().unwrap().into()
}
fn company_order(company: &str, buyer: &str, products: &[&str]) -> Value {
    let lines: Vec<Value> = products
        .iter()
        .map(|product| json!({"productId":product,"quantity":2}))
        .collect();
    json!({"operation":"order.create_for_company","companyId":company,"expectedCompanyRevision":1,"partyId":buyer,"expectedPartyRevision":1,"lines":lines})
}
async fn company_snapshot(app: &Router, session: &Value, company: &str) -> Value {
    let (status, body) = call(
        app,
        "GET",
        &format!("/erp/commerce/workspace?tenant=acme&companyId={company}"),
        Value::Null,
        Some(session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}
/// The `currency` the workspace read returns for one product or order.
fn read_currency(view: &Value, collection: &str, id: &str) -> String {
    view[collection]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["id"] == id)
        .unwrap_or_else(|| panic!("{collection} {id} is missing from {view}"))["currency"]
        .as_str()
        .unwrap()
        .into()
}
async fn stored_currency(f: &Fixture, table: &str, id: &str) -> String {
    let statement = format!("SELECT currency FROM archava_commerce.{table} WHERE id=$1::uuid");
    sqlx::query_scalar(sqlx::AssertSqlSafe(statement.as_str()))
        .bind(id)
        .fetch_one(&f.admin_pool)
        .await
        .unwrap()
}
fn refused_check(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>) -> String {
    let error = result.expect_err("the database must refuse this row");
    let database = error.as_database_error().expect("a database refusal");
    assert_eq!(database.code().as_deref(), Some("23514"), "{database}");
    database.message().to_owned()
}
/// A direct product insert carrying the company's current reviewed snapshot.
async fn insert_company_product(
    f: &Fixture,
    company: &str,
    sku: &str,
    currency: &str,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    // The stock-balance row an accepted product creates is written under
    // forced RLS, so the direct insert names its tenant like the runtime does.
    let mut transaction = f.admin_pool.begin().await?;
    sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
        .execute(&mut *transaction)
        .await?;
    let inserted=sqlx::query("INSERT INTO archava_commerce.products(tenant_id,id,sku,title,unit,track_stock,price,company_id,company_revision,company_code,company_name,currency) SELECT c.tenant_id,$1,$2,'Direct row','unit',false,100,c.id,c.revision,c.code,c.legal_name,$3 FROM archava_organizations.companies c WHERE c.tenant_id='acme' AND c.id=$4::uuid")
        .bind(Uuid::new_v4())
        .bind(sku)
        .bind(currency)
        .bind(company)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(inserted)
}

#[tokio::test]
async fn company_products_and_sales_orders_take_the_book_currency() {
    let (f, app, session) = trade_setup().await;
    let company = trade_company(&app, &session, "USDCO").await;
    let buyer = trade_buyer(&app, &session).await;
    trade_book(&app, &session, &company, "USD").await;
    // Before the first record exists the read already names the currency a
    // new product or order of this company is created in, so the form can
    // label and read its price in it.
    let empty = company_snapshot(&app, &session, &company).await;
    assert_eq!(empty["tradeCurrency"], "USD");
    assert_eq!(empty["products"], json!([]));

    let item = company_product(&app, &session, &company, "USD-ITEM").await;
    let order = command(&app, &session, company_order(&company, &buyer, &[&item])).await;
    let order_id = order["id"].as_str().unwrap();
    assert_eq!(stored_currency(&f, "products", &item).await, "USD");
    assert_eq!(stored_currency(&f, "orders", order_id).await, "USD");
    let view = company_snapshot(&app, &session, &company).await;
    assert_eq!(read_currency(&view, "products", &item), "USD");
    assert_eq!(read_currency(&view, "orders", order_id), "USD");
    let listed = view["orders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["id"] == order_id)
        .unwrap();
    // Amounts are untouched: 2 x 1999 in the record's own unit.
    assert_eq!(listed["total"], 3998);

    // The write paths read the non-IDR rows back instead of refusing them.
    command(&app,&session,json!({"operation":"product.set","id":item,"expectedRevision":1,"price":2499,"active":true})).await;
    command(
        &app,
        &session,
        json!({"operation":"order.reserve","id":order_id,"expectedRevision":1,"holdSeconds":300}),
    )
    .await;
    command(
        &app,
        &session,
        json!({"operation":"order.fulfill","id":order_id,"expectedRevision":2}),
    )
    .await;
    let view = company_snapshot(&app, &session, &company).await;
    assert_eq!(read_currency(&view, "products", &item), "USD");
    assert_eq!(read_currency(&view, "orders", order_id), "USD");
    assert_eq!(stored_currency(&f, "orders", order_id).await, "USD");
}

#[tokio::test]
async fn records_without_a_company_or_a_book_stay_idr() {
    let (f, app, session) = trade_setup().await;
    let unowned = product(&app, &session, "NO-COMPANY", false).await;
    let unowned_order=command(&app,&session,json!({"operation":"order.create","customerName":"Walk-in","lines":[{"productId":unowned,"quantity":1}]})).await;
    let unowned_order = unowned_order["id"].as_str().unwrap();
    assert_eq!(stored_currency(&f, "products", &unowned).await, "IDR");
    assert_eq!(stored_currency(&f, "orders", unowned_order).await, "IDR");
    let view = snapshot(&app, &session).await;
    assert_eq!(view["tradeCurrency"], "IDR");
    assert_eq!(read_currency(&view, "products", &unowned), "IDR");
    assert_eq!(read_currency(&view, "orders", unowned_order), "IDR");

    let company = trade_company(&app, &session, "NOBOOK").await;
    let buyer = trade_buyer(&app, &session).await;
    let item = company_product(&app, &session, &company, "NOBOOK-ITEM").await;
    let order = command(&app, &session, company_order(&company, &buyer, &[&item])).await;
    let order_id = order["id"].as_str().unwrap();
    assert_eq!(stored_currency(&f, "products", &item).await, "IDR");
    assert_eq!(stored_currency(&f, "orders", order_id).await, "IDR");
    let view = company_snapshot(&app, &session, &company).await;
    assert_eq!(view["tradeCurrency"], "IDR");
    assert_eq!(read_currency(&view, "products", &item), "IDR");
    assert_eq!(read_currency(&view, "orders", order_id), "IDR");
}

#[tokio::test]
async fn the_database_refuses_a_currency_other_than_the_company_book() {
    let (f, app, session) = trade_setup().await;
    let named: Vec<(String, String)> = sqlx::query_as("SELECT conrelid::regclass::text,conname::text FROM pg_constraint WHERE contype='c' AND conrelid IN ('archava_commerce.products'::regclass,'archava_commerce.orders'::regclass) AND pg_get_constraintdef(oid) LIKE '%currency%' ORDER BY 1")
        .fetch_all(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(
        named,
        vec![
            (
                "archava_commerce.orders".to_owned(),
                "sales_currency_code".to_owned()
            ),
            (
                "archava_commerce.products".to_owned(),
                "product_currency_code".to_owned()
            ),
        ]
    );

    let company = trade_company(&app, &session, "GUARDED").await;
    let buyer = trade_buyer(&app, &session).await;
    // Before the book exists the company trades in IDR, and only in IDR.
    let message = refused_check(insert_company_product(&f, &company, "EARLY-USD", "USD").await);
    assert_eq!(message, "Currency differs from the company book");
    let legacy = company_product(&app, &session, &company, "LEGACY-IDR").await;
    // A company with a product cannot open a USD book; the mixed state the
    // line guard below defends against is planted by the database owner.
    book_refused(&f, &app, &session, &company, "USD").await;
    planted_book(&f, &app, &session, &company, "USD").await;

    let message = refused_check(insert_company_product(&f, &company, "WRONG-IDR", "IDR").await);
    assert_eq!(message, "Currency differs from the company book");
    refused_check(insert_company_product(&f, &company, "WRONG-JPY", "JPY").await);
    // The column default is IDR, which this company may no longer use.
    refused_check(sqlx::query("INSERT INTO archava_commerce.products(tenant_id,id,sku,title,unit,track_stock,price,company_id,company_revision,company_code,company_name) SELECT c.tenant_id,$1,'DEFAULTED','Direct row','unit',false,100,c.id,c.revision,c.code,c.legal_name FROM archava_organizations.companies c WHERE c.tenant_id='acme' AND c.id=$2::uuid").bind(Uuid::new_v4()).bind(&company).execute(&f.admin_pool).await);
    let accepted = insert_company_product(&f, &company, "RIGHT-USD", "USD")
        .await
        .expect("the book currency is accepted");
    assert_eq!(accepted.rows_affected(), 1);
    // A product without a company is IDR whatever books exist.
    let message=refused_check(sqlx::query("INSERT INTO archava_commerce.products(tenant_id,id,sku,title,unit,track_stock,price,currency) VALUES('acme',$1,'UNOWNED-USD','Direct row','unit',false,100,'USD')").bind(Uuid::new_v4()).execute(&f.admin_pool).await);
    assert_eq!(message, "Currency differs from the company book");
    // The format check replaced the IDR-only one.
    refused_check(sqlx::query("INSERT INTO archava_commerce.products(tenant_id,id,sku,title,unit,track_stock,price,currency) VALUES('acme',$1,'LOWERCASE','Direct row','unit',false,100,'idr')").bind(Uuid::new_v4()).execute(&f.admin_pool).await);
    // An order header obeys the same rule as a product.
    let message=refused_check(sqlx::query("INSERT INTO archava_commerce.orders(tenant_id,id,owner_id,customer_name,total_estimate,currency) VALUES('acme',$1,'alice','Direct order',0,'USD')").bind(Uuid::new_v4()).execute(&f.admin_pool).await);
    assert_eq!(message, "Currency differs from the company book");

    // One order, one currency: the planted IDR product of the same company
    // cannot be added to its USD order.
    let item = company_product(&app, &session, &company, "USD-ITEM").await;
    let order = command(&app, &session, company_order(&company, &buyer, &[&item])).await;
    let message=refused_check(sqlx::query("INSERT INTO archava_commerce.order_lines(tenant_id,id,order_id,product_id,sku,title,unit,track_stock,quantity,price,product_revision) SELECT p.tenant_id,$1,$2::uuid,p.id,p.sku,p.title,p.unit,p.track_stock,1,p.price,p.revision FROM archava_commerce.products p WHERE p.tenant_id='acme' AND p.id=$3::uuid").bind(Uuid::new_v4()).bind(order["id"].as_str().unwrap()).bind(&legacy).execute(&f.admin_pool).await);
    assert_eq!(message, "Sales line is priced in another currency");
    let lines: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_commerce.order_lines WHERE order_id=$1::uuid",
    )
    .bind(order["id"].as_str().unwrap())
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(lines, 1);
}

#[tokio::test]
async fn a_non_idr_book_is_refused_once_the_company_trades() {
    let (f, app, session) = trade_setup().await;
    let company = trade_company(&app, &session, "TRADED").await;
    let buyer = trade_buyer(&app, &session).await;
    // (a) One product is enough.
    let before = company_product(&app, &session, &company, "BEFORE-BOOK").await;
    book_refused(&f, &app, &session, &company, "USD").await;
    // (b) So is a sales order, and a zero-decimal currency is no exception.
    let early = command(&app, &session, company_order(&company, &buyer, &[&before])).await;
    let early = early["id"].as_str().unwrap();
    book_refused(&f, &app, &session, &company, "USD").await;
    book_refused(&f, &app, &session, &company, "JPY").await;

    // The company keeps trading in IDR and can still open an IDR book.
    trade_book(&app, &session, &company, "IDR").await;
    assert_eq!(books(&f, &company).await, vec!["IDR".to_owned()]);
    let after = company_product(&app, &session, &company, "AFTER-BOOK").await;
    let later = command(
        &app,
        &session,
        company_order(&company, &buyer, &[&before, &after]),
    )
    .await;
    assert_eq!(stored_currency(&f, "products", &before).await, "IDR");
    assert_eq!(stored_currency(&f, "products", &after).await, "IDR");
    assert_eq!(stored_currency(&f, "orders", early).await, "IDR");
    assert_eq!(
        stored_currency(&f, "orders", later["id"].as_str().unwrap()).await,
        "IDR"
    );
    let view = company_snapshot(&app, &session, &company).await;
    assert_eq!(read_currency(&view, "products", &before), "IDR");
    assert_eq!(read_currency(&view, "orders", early), "IDR");
    command(
        &app,
        &session,
        json!({"operation":"order.reserve","id":early,"expectedRevision":1,"holdSeconds":300}),
    )
    .await;
    // Currency never changes after insert.
    let message = refused_check(
        sqlx::query("UPDATE archava_commerce.products SET currency='USD',revision=revision+1 WHERE id=$1::uuid")
            .bind(&before)
            .execute(&f.admin_pool)
            .await,
    );
    assert!(message.contains("immutable"), "{message}");

    // A company that has not traded opens its USD book as before.
    let fresh = trade_company(&app, &session, "FRESH").await;
    trade_book(&app, &session, &fresh, "USD").await;
    assert_eq!(books(&f, &fresh).await, vec!["USD".to_owned()]);
}

/// A company product insert and a non-IDR book serialise on the company row,
/// in both orders: a committed IDR product never coexists with a USD book.
#[tokio::test]
async fn a_product_insert_and_a_non_idr_book_serialise_on_the_company() {
    use std::time::Duration;
    let (f, app, session) = trade_setup().await;
    let pause = |millis| tokio::time::sleep(Duration::from_millis(millis));

    // The product is written first but not yet committed. The book waits for
    // it (an unlocked check would not see the row and would accept the book)
    // and is refused once it commits.
    let company = trade_company(&app, &session, "PRODUCT-FIRST").await;
    let mut pending = f.admin_pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
        .execute(&mut *pending)
        .await
        .unwrap();
    sqlx::query("INSERT INTO archava_commerce.products(tenant_id,id,sku,title,unit,track_stock,price,company_id,company_revision,company_code,company_name,currency) SELECT c.tenant_id,$1,'PENDING-IDR','Direct row','unit',false,100,c.id,c.revision,c.code,c.legal_name,'IDR' FROM archava_organizations.companies c WHERE c.tenant_id='acme' AND c.id=$2::uuid")
        .bind(Uuid::new_v4())
        .bind(&company)
        .execute(&mut *pending)
        .await
        .unwrap();
    let request = book_request(&company, "USD");
    let ((status, body), ()) = tokio::join!(
        call(
            &app,
            "POST",
            "/erp/accounting/commands",
            request,
            Some(&session)
        ),
        async {
            pause(700).await;
            pending.commit().await.unwrap();
        }
    );
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(books(&f, &company).await, Vec::<String>::new());

    // The book is written first and held before its commit by an uncommitted
    // row that claims its book code. The product waits for the book (an
    // unlocked insert would commit in IDR) and is then created in USD.
    let company = trade_company(&app, &session, "BOOK-FIRST").await;
    let blocker = trade_company(&app, &session, "BLOCKER").await;
    let mut held = f.admin_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role='replica'")
        .execute(&mut *held)
        .await
        .unwrap();
    sqlx::query("INSERT INTO archava_accounting.books(tenant_id,id,company_id,company_revision,company_code,company_name,owner_id,code,name,currency,minor_units,currency_catalogue,source_authority,independent_posting) SELECT c.tenant_id,$1,c.id,c.revision,c.code,c.legal_name,'alice','BOOK-USD','Uncommitted code holder','USD',2,'SIX-2026-09-17','native',true FROM archava_organizations.companies c WHERE c.tenant_id='acme' AND c.id=$2::uuid")
        .bind(Uuid::new_v4())
        .bind(&blocker)
        .execute(&mut *held)
        .await
        .unwrap();
    let request = book_request(&company, "USD");
    let item = submission(
        json!({"operation":"product.create_for_company","companyId":company,"expectedCompanyRevision":1,"sku":"RACED","title":"Raced product","unit":"unit","trackStock":false,"price":1999}),
    );
    let ((book_status, book_body), (item_status, item_body), ()) = tokio::join!(
        call(
            &app,
            "POST",
            "/erp/accounting/commands",
            request,
            Some(&session)
        ),
        async {
            pause(500).await;
            call(&app, "POST", "/erp/commerce/commands", item, Some(&session)).await
        },
        async {
            pause(1200).await;
            held.rollback().await.unwrap();
        }
    );
    assert_eq!(book_status, StatusCode::OK, "{book_body}");
    assert_eq!(item_status, StatusCode::OK, "{item_body}");
    assert_eq!(books(&f, &company).await, vec!["USD".to_owned()]);
    assert_eq!(
        stored_currency(&f, "products", item_body["receipt"]["id"].as_str().unwrap()).await,
        "USD"
    );

    // Unsynchronised: whichever commits first, the pair is never an IDR
    // product beside a USD book.
    for round in 0..4 {
        let company = trade_company(&app, &session, &format!("RACE-{round}")).await;
        let mut request = book_request(&company, "USD");
        request["command"]["code"] = json!(format!("RACE-{round}"));
        let item = submission(
            json!({"operation":"product.create_for_company","companyId":company,"expectedCompanyRevision":1,"sku":format!("RACE-{round}"),"title":"Raced product","unit":"unit","trackStock":false,"price":1999}),
        );
        let ((book_status, book_body), (item_status, item_body)) = tokio::join!(
            call(
                &app,
                "POST",
                "/erp/accounting/commands",
                request,
                Some(&session)
            ),
            call(&app, "POST", "/erp/commerce/commands", item, Some(&session))
        );
        let currencies: Vec<String> = sqlx::query_scalar(
            "SELECT currency FROM archava_commerce.products WHERE company_id=$1::uuid",
        )
        .bind(&company)
        .fetch_all(&f.admin_pool)
        .await
        .unwrap();
        let outcome = (
            book_status,
            item_status,
            books(&f, &company).await,
            currencies,
        );
        let product_first = (
            StatusCode::CONFLICT,
            StatusCode::OK,
            vec![],
            vec!["IDR".to_owned()],
        );
        let book_first = (
            StatusCode::OK,
            StatusCode::OK,
            vec!["USD".to_owned()],
            vec!["USD".to_owned()],
        );
        // The product read "no book" and the database refused its IDR row.
        let book_first_product_refused = (
            StatusCode::OK,
            StatusCode::CONFLICT,
            vec!["USD".to_owned()],
            vec![],
        );
        assert!(
            outcome == product_first
                || outcome == book_first
                || outcome == book_first_product_refused,
            "{outcome:?} {book_body} {item_body}"
        );
    }
}
