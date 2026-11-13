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
    assert_eq!(upgraded.len(), f.commerce_ledger.len() + 1);
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
