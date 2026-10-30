//! Native Tier 1 workspace reads, cross-ledger journeys and private mission execution.
mod common;
use archava_host::{router, router::AllReadiness, AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::{http::StatusCode, Router};
use common::{call, login, HASH};
use serde_json::{json, Value};
use uuid::Uuid;
async fn setup() -> (Fixture, Router, Value) {
    let f = provision().await;
    for user in ["alice", "reader", "assigned", "service-seat"] {
        sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES($1,$2,$3,true,1)").bind(user).bind(format!("{user}@archava.test")).bind(HASH).execute(&f.admin_pool).await.unwrap();
        for (namespace, all) in [
            (
                "archava_commerce",
                vec![
                    "catalog.read",
                    "catalog.write",
                    "stock.read",
                    "stock.write",
                    "orders.read",
                    "orders.write",
                    "returns.write",
                    "valuation.read",
                    "valuation.write",
                    "audit.read",
                ],
            ),
            (
                "archava_purchasing",
                vec![
                    "suppliers.read",
                    "suppliers.write",
                    "offers.read",
                    "offers.write",
                    "orders.read",
                    "orders.write",
                    "receipts.read",
                    "receipts.write",
                    "audit.read",
                ],
            ),
            (
                "archava_billing",
                vec![
                    "billing.read",
                    "billing.write",
                    "billing.settle",
                    "audit.read",
                ],
            ),
            (
                "archava_support",
                vec![
                    "cases.read",
                    "cases.write",
                    "cases.assign",
                    "notes.read",
                    "notes.write",
                    "audit.read",
                ],
            ),
        ] {
            let grants: Vec<&str> = all
                .into_iter()
                .filter(|p| user != "reader" || p.ends_with(".read"))
                .collect();
            let query=format!("INSERT INTO {namespace}.memberships(tenant_id,principal_id,kind,permissions,scope,active) VALUES('acme',$1,$2,$3,$4,true)");
            sqlx::query(sqlx::AssertSqlSafe(query.as_str()))
                .bind(user)
                .bind(if user == "service-seat" {
                    "service"
                } else {
                    "human"
                })
                .bind(grants)
                .bind(if user == "assigned" {
                    "assigned"
                } else {
                    "all"
                })
                .execute(&f.admin_pool)
                .await
                .unwrap();
        }
        sqlx::query("UPDATE archava_commerce.memberships SET role='owner',capability=$2 WHERE principal_id=$1").bind(user).bind(if user=="reader"{"assist"}else{"enterprise"}).execute(&f.admin_pool).await.unwrap();
    }
    let additional = f.erp_readiness();
    let state = AppState::try_new_all(
        f.runtime_pool.clone(),
        Ledger {
            namespace: "archava_commerce",
            ledger: "Commerce",
            tables: &[
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
    .unwrap();
    let app = router(state);
    let session = login(&app, "alice").await;
    (f, app, session)
}
async fn submit(app: &Router, session: &Value, domain: &str, mut command: Value) -> Value {
    let id = Uuid::new_v4();
    let body = if domain == "billing" {
        command["commandId"] = json!(id);
        json!({"tenant":"acme","mutation":command})
    } else if domain == "support" {
        json!({"tenant":"acme","mutation":{"requestId":id,"command":command}})
    } else {
        json!({"tenant":"acme","requestId":id,"command":command})
    };
    let (s, b) = call(
        app,
        "POST",
        &format!("/erp/{domain}/commands"),
        body,
        Some(session),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{domain}: {b}");
    if b["receipt"].is_object() {
        b["receipt"].clone()
    } else {
        b
    }
}
async fn snapshot(app: &Router, session: &Value, domain: &str) -> (StatusCode, Value) {
    call(
        app,
        "GET",
        &format!("/erp/{domain}/workspace?tenant=acme"),
        Value::Null,
        Some(session),
    )
    .await
}
#[tokio::test]
async fn procurement_to_stock_invoice_and_support_are_real_workspace_records() {
    let (_f, app, session) = setup().await;
    let product=submit(&app,&session,"commerce",json!({"operation":"product.create","sku":"BUY","title":"Purchased goods","unit":"unit","trackStock":true,"price":10000})).await;
    let supplier = submit(
        &app,
        &session,
        "purchasing",
        json!({"operation":"supplier.create","code":"SUP","name":"Synthetic supplier"}),
    )
    .await;
    let offer=submit(&app,&session,"purchasing",json!({"operation":"offer.create","supplierId":supplier["id"],"productId":product["id"],"unitCost":6000})).await;
    let order=submit(&app,&session,"purchasing",json!({"operation":"order.create","supplierId":supplier["id"],"lines":[{"offerId":offer["id"],"quantity":5}]})).await;
    let (s, view) = snapshot(&app, &session, "purchasing").await;
    assert_eq!(s, StatusCode::OK, "{view}");
    assert_eq!(view["orders"][0]["total"], 30000);
    submit(&app,&session,"purchasing",json!({"operation":"order.confirm","id":order["id"],"expectedRevision":1,"reason":"Approved procurement"})).await;
    let receipt=submit(&app,&session,"purchasing",json!({"operation":"order.receive","id":order["id"],"expectedRevision":2,"reason":"Delivery inspection","lines":[{"lineId":view["orders"][0]["lines"][0]["id"],"accepted":4,"rejected":1}]})).await;
    assert!(receipt["id"].is_string());
    let (s, view) = snapshot(&app, &session, "commerce").await;
    assert_eq!(s, StatusCode::OK, "{view}");
    assert_eq!(view["products"][0]["quantity"], 4);
    assert_eq!(view["products"][0]["available"], 4);
    let invoice=submit(&app,&session,"billing",json!({"type":"create_invoice","invoiceCode":"INV-TEST","customerRef":"Synthetic buyer","consumerRef":order["id"],"currency":"IDR","lines":[{"description":"Goods","quantity":2,"unitMinor":10000}]})).await;
    submit(&app,&session,"billing",json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":invoice["revision"],"reason":"Issued to customer"})).await;
    let (s, view) = snapshot(&app, &session, "billing").await;
    assert_eq!(s, StatusCode::OK, "{view}");
    assert_eq!(view["invoices"][0]["outstanding"], 20000);
    assert_eq!(view["invoices"][0]["settled"], 0);
    assert_eq!(view["invoices"][0]["status"], "issued");
    let case=submit(&app,&session,"support",json!({"operation":"case.create","requesterName":"Synthetic buyer","subject":"Delivery question","description":"Please confirm delivery","priority":"normal"})).await;
    let started=submit(&app,&session,"support",json!({"operation":"case.start","id":case["id"],"expectedRevision":case["revision"],"reason":"Reviewing order"})).await;
    submit(&app,&session,"support",json!({"operation":"case.resolve","id":case["id"],"expectedRevision":started["revision"],"resolution":"Delivery confirmed"})).await;
    let (s, body) = call(
        &app,
        "GET",
        &format!(
            "/erp/support/cases/detail?tenant=acme&id={}",
            case["id"].as_str().unwrap()
        ),
        Value::Null,
        Some(&session),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    assert_eq!(body["record"]["state"], "resolved");
    assert!(body["transitions"].as_array().unwrap().len() >= 3);
}
#[tokio::test]
async fn workspace_projection_honors_kind_scope_revocation_and_tenant() {
    let (f, app, session) = setup().await;
    submit(&app,&session,"billing",json!({"type":"create_invoice","invoiceCode":"PRIVATE","customerRef":"Private buyer","consumerRef":"contract","currency":"IDR","lines":[{"description":"Service","quantity":1,"unitMinor":50}]})).await;
    let assigned = login(&app, "assigned").await;
    let (s, view) = snapshot(&app, &assigned, "billing").await;
    assert_eq!(s, StatusCode::OK, "{view}");
    assert!(view["invoices"].as_array().unwrap().is_empty());
    let reader = login(&app, "reader").await;
    for domain in ["commerce", "purchasing", "billing"] {
        let (s, b) = snapshot(&app, &reader, domain).await;
        assert_eq!(s, StatusCode::OK, "{domain}: {b}");
        let path = format!("/erp/{domain}/workspace?tenant=foreign");
        assert_eq!(
            call(&app, "GET", &path, Value::Null, Some(&reader)).await.0,
            StatusCode::FORBIDDEN
        );
    }
    let service = login(&app, "service-seat").await;
    assert_eq!(
        snapshot(&app, &service, "billing").await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE archava_billing.memberships SET active=false WHERE principal_id='reader'")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(
        snapshot(&app, &reader, "billing").await.0,
        StatusCode::FORBIDDEN
    );
}
#[tokio::test]
async fn mission_creation_replay_private_listing_real_tools_and_step_retry() {
    let (f, app, session) = setup().await;
    let create = json!({"tenant_id":"acme","mission_code":Uuid::new_v4(),"title":"Business overview","description":"Read-only business observations","auto_launch":true,"steps":[{"step_index":0,"step_name":"Sales","tool_name":"commerce.workspace.read","payload_input":{},"depends_on":[]},{"step_index":1,"step_name":"Invoices","tool_name":"billing.workspace.read","payload_input":{},"depends_on":[0]}]});
    let (s, mission) = call(
        &app,
        "POST",
        "/api/v1/cockpit/missions",
        create.clone(),
        Some(&session),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{mission}");
    let id = mission["id"].as_str().unwrap();
    let (s, replay) = call(
        &app,
        "POST",
        "/api/v1/cockpit/missions",
        create.clone(),
        Some(&session),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{replay}");
    assert_eq!(replay["id"], mission["id"]);
    let mut changed = create;
    changed["title"] = json!("Different");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/cockpit/missions",
            changed,
            Some(&session)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let reader = login(&app, "reader").await;
    let (s, list) = call(
        &app,
        "GET",
        "/api/v1/cockpit/missions?tenant=acme",
        Value::Null,
        Some(&reader),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(list["items"].as_array().unwrap().is_empty());
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/cockpit/missions/{id}?tenant=acme"),
            Value::Null,
            Some(&reader)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let path = format!("/api/v1/cockpit/missions/{id}/run-next?tenant=acme");
    assert_eq!(
        call(&app, "POST", &path, json!({"stepIndex":1}), Some(&session))
            .await
            .0,
        StatusCode::CONFLICT
    );
    for _ in 0..2 {
        let (s, b) = call(&app, "POST", &path, json!({"stepIndex":0}), Some(&session)).await;
        assert_eq!(s, StatusCode::OK, "{b}");
        assert_eq!(b["steps"][0]["status"], "passed");
        assert_eq!(b["steps"][1]["status"], "pending");
        assert_eq!(
            b["steps"][0]["payload_output"]["tool"],
            "commerce.workspace.read"
        );
    }
    sqlx::query("UPDATE archava_billing.memberships SET active=false WHERE principal_id='alice'")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(
        call(&app, "POST", &path, json!({"stepIndex":1}), Some(&session))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE archava_billing.memberships SET active=true WHERE principal_id='alice'")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    let (s, b) = call(&app, "POST", &path, json!({"stepIndex":1}), Some(&session)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["status"], "touchdown");
    let events:i64=sqlx::query_scalar("SELECT count(*) FROM archava_cockpit.flight_events WHERE mission_id=$1 AND event_type='step_completed'").bind(Uuid::parse_str(id).unwrap()).fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(events, 2);
}

async fn receive_costed(
    app: &Router,
    session: &Value,
    product: &Value,
    code: &str,
    quantity: i64,
    unit_cost: i64,
    fee: i64,
) -> Value {
    let supplier = submit(
        app,
        session,
        "purchasing",
        json!({"operation":"supplier.create","code":code,"name":format!("Supplier {code}")}),
    )
    .await;
    let offer=submit(app,session,"purchasing",json!({"operation":"offer.create","supplierId":supplier["id"],"productId":product["id"],"unitCost":unit_cost})).await;
    let order=submit(app,session,"purchasing",json!({"operation":"order.create","supplierId":supplier["id"],"lines":[{"offerId":offer["id"],"quantity":quantity}]})).await;
    submit(app,session,"purchasing",json!({"operation":"order.confirm","id":order["id"],"expectedRevision":1,"reason":"Confirmed pricing"})).await;
    let (s, view) = snapshot(app, session, "purchasing").await;
    assert_eq!(s, StatusCode::OK, "{view}");
    let row = view["orders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == order["id"])
        .unwrap();
    submit(app,session,"purchasing",json!({"operation":"order.receive","id":order["id"],"expectedRevision":2,"reason":"Inspected delivery","lines":[{"lineId":row["lines"][0]["id"],"accepted":quantity,"rejected":0}],"landedFeeMinor":fee,"allocationBasis":"value"})).await
}
async fn fulfill_costed(app: &Router, session: &Value, product: &Value, quantity: i64) -> Value {
    let order=submit(app,session,"commerce",json!({"operation":"order.create","customerName":"Costing buyer","lines":[{"productId":product["id"],"quantity":quantity}]})).await;
    submit(app,session,"commerce",json!({"operation":"order.reserve","id":order["id"],"expectedRevision":1,"holdSeconds":900})).await;
    submit(
        app,
        session,
        "commerce",
        json!({"operation":"order.fulfill","id":order["id"],"expectedRevision":2}),
    )
    .await;
    let (s, view) = snapshot(app, session, "commerce").await;
    assert_eq!(s, StatusCode::OK, "{view}");
    view["orders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == order["id"])
        .unwrap()
        .clone()
}
#[tokio::test]
async fn landed_cost_fifo_partial_restock_discard_and_restocked_fifo_conserve_every_rupiah() {
    let (_f, app, session) = setup().await;
    let product=submit(&app,&session,"commerce",json!({"operation":"product.create","sku":"FIFO","title":"FIFO goods","unit":"unit","trackStock":true,"price":20})).await;
    receive_costed(&app, &session, &product, "LOT-A", 2, 3, 1).await; // 7, unit intervals 4 + 3.
    receive_costed(&app, &session, &product, "LOT-B", 3, 5, 2).await; // 17, intervals 6 + 6 + 5.
    let order = fulfill_costed(&app, &session, &product, 4).await;
    assert_eq!(order["fulfillmentCostMinor"], "19");
    let line = order["lines"][0]["id"].clone();
    for (qty, disposition) in [(1, "restock"), (1, "discard"), (2, "restock")] {
        let (_, view) = snapshot(&app, &session, "commerce").await;
        let current = view["orders"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == order["id"])
            .unwrap();
        submit(&app,&session,"commerce",json!({"operation":"order.return","id":order["id"],"expectedRevision":current["revision"],"lineId":line,"quantity":qty,"disposition":disposition,"reason":"Return inspection"})).await;
    }
    let (_, view) = snapshot(&app, &session, "commerce").await;
    let current = &view["products"][0];
    assert_eq!(current["quantity"], 4);
    assert_eq!(current["inventoryValueMinor"], "21");
    assert_eq!(current["unknownCostQuantity"], 0);
    let returned = view["orders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == order["id"])
        .unwrap();
    assert_eq!(returned["restockedCostMinor"], "16");
    assert_eq!(returned["state"], "returned");
    let next = fulfill_costed(&app, &session, &product, 2).await;
    assert_eq!(
        next["fulfillmentCostMinor"], "9",
        "Remaining original lot (5) is older than restocked lot (4)"
    );
    let (_, view) = snapshot(&app, &session, "commerce").await;
    assert_eq!(view["products"][0]["quantity"], 2);
    assert_eq!(view["products"][0]["inventoryValueMinor"], "12");
}
#[tokio::test]
async fn unknown_cost_never_becomes_zero_and_valuation_permissions_are_independent() {
    let (f, app, session) = setup().await;
    let product=submit(&app,&session,"commerce",json!({"operation":"product.create","sku":"UNKNOWN","title":"Historic cost missing","unit":"unit","trackStock":true,"price":20})).await;
    submit(&app,&session,"commerce",json!({"operation":"stock.adjust","productId":product["id"],"quantity":3,"reason":"Historical count"})).await;
    let (_, view) = snapshot(&app, &session, "commerce").await;
    assert!(view["products"][0]["inventoryValueMinor"].is_null());
    assert_eq!(view["products"][0]["unknownCostQuantity"], 3);
    let order = fulfill_costed(&app, &session, &product, 1).await;
    assert!(order["fulfillmentCostMinor"].is_null());
    sqlx::query("UPDATE archava_commerce.memberships SET permissions=array_remove(permissions,'valuation.read') WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    let (_, view) = snapshot(&app, &session, "commerce").await;
    assert!(view["products"][0].get("inventoryValueMinor").is_none());
    assert!(view["orders"][0].get("fulfillmentCostMinor").is_none());
    sqlx::query("UPDATE archava_commerce.memberships SET permissions=array_remove(permissions,'valuation.write') WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    let(s,_)=call(&app,"POST","/erp/commerce/commands",json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"stock.adjust","productId":product["id"],"quantity":1,"unitCostMinor":0,"reason":"Known zero requires valuation authority"}}),Some(&session)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_cost_ledger_failure_rolls_back_fulfillment_stock_audit_and_receipt() {
    let (f, app, session) = setup().await;
    let product=submit(&app,&session,"commerce",json!({"operation":"product.create","sku":"ATOMIC-COST","title":"Atomic cost","unit":"unit","trackStock":true,"price":20})).await;
    receive_costed(&app, &session, &product, "ATOMIC", 3, 5, 1).await;
    let order=submit(&app,&session,"commerce",json!({"operation":"order.create","customerName":"Failure test","lines":[{"productId":product["id"],"quantity":2}]})).await;
    submit(&app,&session,"commerce",json!({"operation":"order.reserve","id":order["id"],"expectedRevision":1,"holdSeconds":900})).await;
    sqlx::raw_sql("CREATE FUNCTION archava_commerce.reject_cost_test() RETURNS trigger LANGUAGE plpgsql AS $$BEGIN RAISE EXCEPTION 'synthetic cost ledger outage';END$$; CREATE TRIGGER reject_cost_test BEFORE INSERT ON archava_commerce.inventory_uses FOR EACH ROW EXECUTE FUNCTION archava_commerce.reject_cost_test()").execute(&f.admin_pool).await.unwrap();
    let body = json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"order.fulfill","id":order["id"],"expectedRevision":2}});
    let (s, _) = call(
        &app,
        "POST",
        "/erp/commerce/commands",
        body.clone(),
        Some(&session),
    )
    .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    let (_, view) = snapshot(&app, &session, "commerce").await;
    assert_eq!(view["products"][0]["quantity"], 3);
    assert_eq!(view["products"][0]["inventoryValueMinor"], "16");
    assert_eq!(view["orders"][0]["state"], "reserved");
    let command_rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_commerce.commands WHERE request_id=$1")
            .bind(Uuid::parse_str(body["requestId"].as_str().unwrap()).unwrap())
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(command_rows, 0);
    sqlx::raw_sql("DROP TRIGGER reject_cost_test ON archava_commerce.inventory_uses")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    for _ in 0..2 {
        assert_eq!(
            call(
                &app,
                "POST",
                "/erp/commerce/commands",
                body.clone(),
                Some(&session)
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let (_, view) = snapshot(&app, &session, "commerce").await;
    assert_eq!(view["products"][0]["quantity"], 1);
    assert_eq!(view["products"][0]["inventoryValueMinor"], "5");
    assert_eq!(view["orders"][0]["fulfillmentCostMinor"], "11");
}

#[tokio::test]
async fn zero_value_goods_require_a_quantity_basis_and_keep_frozen_purchase_prices() {
    let (f, app, session) = setup().await;
    let mut products = Vec::new();
    for sku in ["ZERO-A", "ZERO-B"] {
        products.push(submit(&app,&session,"commerce",json!({"operation":"product.create","sku":sku,"title":sku,"unit":"unit","trackStock":true,"price":20})).await);
    }
    let supplier = submit(
        &app,
        &session,
        "purchasing",
        json!({"operation":"supplier.create","code":"FREE","name":"Promotional supplier"}),
    )
    .await;
    let mut offers = Vec::new();
    for p in &products {
        offers.push(submit(&app,&session,"purchasing",json!({"operation":"offer.create","supplierId":supplier["id"],"productId":p["id"],"unitCost":0})).await);
    }
    let order=submit(&app,&session,"purchasing",json!({"operation":"order.create","supplierId":supplier["id"],"lines":[{"offerId":offers[0]["id"],"quantity":1},{"offerId":offers[1]["id"],"quantity":2}]})).await;
    submit(&app,&session,"purchasing",json!({"operation":"order.confirm","id":order["id"],"expectedRevision":1,"reason":"Frozen zero-cost promotion"})).await;
    for offer in &offers {
        submit(&app,&session,"purchasing",json!({"operation":"offer.set-cost","id":offer["id"],"expectedRevision":1,"reason":"Future quote changed","unitCost":100})).await;
    }
    let (_, view) = snapshot(&app, &session, "purchasing").await;
    let rows = view["orders"][0]["lines"].as_array().unwrap();
    let lines = rows
        .iter()
        .map(|l| json!({"lineId":l["id"],"accepted":l["quantity"],"rejected":0}))
        .collect::<Vec<_>>();
    let mut body = json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"order.receive","id":order["id"],"expectedRevision":2,"reason":"Promotional goods received","lines":lines,"landedFeeMinor":1,"allocationBasis":"value"}});
    assert_eq!(
        call(
            &app,
            "POST",
            "/erp/purchasing/commands",
            body.clone(),
            Some(&session)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let receipts: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.receipts")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(receipts, 0, "invalid allocation writes no receipt");
    body["command"]["allocationBasis"] = json!("quantity");
    for _ in 0..2 {
        let (s, b) = call(
            &app,
            "POST",
            "/erp/purchasing/commands",
            body.clone(),
            Some(&session),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{b}");
    }
    let (_, view) = snapshot(&app, &session, "commerce").await;
    let a = view["products"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == products[0]["id"])
        .unwrap();
    let b = view["products"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == products[1]["id"])
        .unwrap();
    assert_eq!(a["quantity"], 1);
    assert_eq!(a["inventoryValueMinor"], "0");
    assert_eq!(a["unknownCostQuantity"], 0);
    assert_eq!(b["quantity"], 2);
    assert_eq!(b["inventoryValueMinor"], "1");
}
#[tokio::test]
async fn inventory_values_above_javascript_safe_integer_travel_as_exact_decimal_strings() {
    let (_f, app, session) = setup().await;
    let product=submit(&app,&session,"commerce",json!({"operation":"product.create","sku":"LARGE-VALUE","title":"Exact valuation","unit":"unit","trackStock":true,"price":20})).await;
    submit(&app,&session,"commerce",json!({"operation":"stock.adjust","productId":product["id"],"quantity":1000000000,"unitCostMinor":1000000000,"reason":"Known opening acquisition basis"})).await;
    let (_, view) = snapshot(&app, &session, "commerce").await;
    assert_eq!(
        view["products"][0]["inventoryValueMinor"],
        "1000000000000000000"
    );
    assert_eq!(view["products"][0]["unknownCostQuantity"], 0);
}

#[tokio::test]
async fn concurrent_fulfillment_uses_disjoint_fifo_intervals() {
    let (_f, app, session) = setup().await;
    let product=submit(&app,&session,"commerce",json!({"operation":"product.create","sku":"PARALLEL-COST","title":"Concurrent FIFO","unit":"unit","trackStock":true,"price":20})).await;
    receive_costed(&app, &session, &product, "CONCURRENT", 10, 3, 1).await;
    let mut orders = Vec::new();
    for name in ["First", "Second"] {
        let order=submit(&app,&session,"commerce",json!({"operation":"order.create","customerName":name,"lines":[{"productId":product["id"],"quantity":4}]})).await;
        submit(&app,&session,"commerce",json!({"operation":"order.reserve","id":order["id"],"expectedRevision":1,"holdSeconds":900})).await;
        orders.push(order);
    }
    let a = json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"order.fulfill","id":orders[0]["id"],"expectedRevision":2}});
    let b = json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"order.fulfill","id":orders[1]["id"],"expectedRevision":2}});
    let (first, second) = tokio::join!(
        call(&app, "POST", "/erp/commerce/commands", a, Some(&session)),
        call(&app, "POST", "/erp/commerce/commands", b, Some(&session))
    );
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(second.0, StatusCode::OK, "{}", second.1);
    let (_, view) = snapshot(&app, &session, "commerce").await;
    assert_eq!(view["products"][0]["quantity"], 2);
    assert_eq!(view["products"][0]["inventoryValueMinor"], "6");
    let costs = view["orders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            o["fulfillmentCostMinor"]
                .as_str()
                .unwrap()
                .parse::<i64>()
                .unwrap()
        })
        .sum::<i64>();
    assert_eq!(
        costs, 25,
        "8 units consume the first indivisible fee Rupiah exactly once"
    );
}

#[tokio::test]
async fn runtime_cannot_attach_inventory_cost_writer_to_a_temporary_trigger() {
    let (f, app, session) = setup().await;
    let product=submit(&app,&session,"commerce",json!({"operation":"product.create","sku":"COST-PRIVILEGE","title":"Cost privilege witness","unit":"unit","trackStock":true,"price":1})).await;
    let mut tx = f.runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("CREATE TEMP TABLE forged_cost(tenant_id text,product_id uuid,request_id uuid,order_id uuid,kind text,quantity int,unit_cost_minor bigint,landed_fee_minor bigint)").execute(&mut *tx).await.unwrap();
    let attached=sqlx::query("CREATE TRIGGER forged_cost_write AFTER INSERT ON forged_cost FOR EACH ROW EXECUTE FUNCTION archava_commerce.cost_movement()")
        .execute(&mut *tx).await;
    if let Err(error) = attached {
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("42501"),
            "{error}"
        );
        tx.rollback().await.unwrap();
    } else {
        sqlx::query("INSERT INTO forged_cost VALUES('acme',$1,$2,NULL,'adjust',7,1,0)")
            .bind(Uuid::parse_str(product["id"].as_str().unwrap()).unwrap())
            .bind(Uuid::new_v4())
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    let invented: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_commerce.inventory_layers WHERE tenant_id='acme'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(invented,0,"A runtime-owned temporary trigger must not invent acquisition layers without physical stock movements");
}

#[tokio::test]
async fn stale_serving_process_cannot_write_after_writer_takeover() {
    let (f, app, session) = setup().await;
    sqlx::query("INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','alice','human','{contacts.read,contacts.write}','all')")
        .execute(&f.admin_pool).await.unwrap();
    let contact_body = |name: &str| json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"contact.create","data":{"fullName":name}}});
    let (status, body) = call(
        &app,
        "POST",
        "/crm/commands",
        contact_body("Before takeover"),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut mission = json!({"tenant_id":"acme","mission_code":Uuid::new_v4(),"title":"Before takeover","description":"Authorized read mission","auto_launch":true,"steps":[{"step_index":0,"step_name":"Sales","tool_name":"commerce.workspace.read","payload_input":{},"depends_on":[]}]});
    let (status, body) = call(
        &app,
        "POST",
        "/api/v1/cockpit/missions",
        mission.clone(),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    // Mirror the operational lease transition through an explicit administrator,
    // without giving the serving process any refreshed token.
    sqlx::raw_sql("CREATE SCHEMA IF NOT EXISTS archava_control;CREATE TABLE IF NOT EXISTS archava_control.writer_lease(lease_name text PRIMARY KEY,epoch bigint NOT NULL,owner_token text NOT NULL,drained boolean NOT NULL DEFAULT false)")
        .execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_control.writer_lease(lease_name,epoch,owner_token,drained) VALUES('archava-serving',1,$1,false) ON CONFLICT(lease_name) DO UPDATE SET epoch=writer_lease.epoch+1,owner_token=EXCLUDED.owner_token,drained=false")
        .bind(Uuid::new_v4().to_string()).execute(&f.admin_pool).await.unwrap();
    let (status,answer)=call(&app,"POST","/erp/commerce/commands",json!({"tenant":"acme","requestId":Uuid::new_v4().to_string(),"command":{"operation":"product.create","sku":"STALE-WRITER","title":"Must not be acknowledged","unit":"unit","trackStock":true,"price":1}}),Some(&session)).await;
    assert!(
        status.is_server_error(),
        "Stale serving writer acknowledged {status}: {answer}"
    );
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_commerce.products WHERE sku='STALE-WRITER'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(rows, 0);
    mission["mission_code"] = json!(Uuid::new_v4());
    for (path, body, actor) in [
        (
            "/crm/commands",
            contact_body("Must not save"),
            Some(&session),
        ),
        ("/api/v1/cockpit/missions", mission, Some(&session)),
        (
            "/login",
            json!({"username":"alice@archava.test","password":common::PASSWORD}),
            None,
        ),
    ] {
        let (status, answer) = call(&app, "POST", path, body, actor).await;
        assert!(
            status.is_server_error(),
            "Stale {path} acknowledged {status}: {answer}"
        );
    }
    let effects:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM archava_crm.contacts),(SELECT count(*) FROM archava_cockpit.missions),(SELECT count(*) FROM archava_identity.sessions)")
        .fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(
        effects,
        (1, 1, 1),
        "stale CRM, mission and login requests must not add durable effects"
    );
    use tower::ServiceExt;
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/readyz")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn assigned_billing_cannot_mutate_or_recover_an_invoice_outside_its_current_scope() {
    let (f, app, alice) = setup().await;
    let invoice=submit(&app,&alice,"billing",json!({"type":"create_invoice","invoiceCode":"SCOPE-FOREIGN","customerRef":"Scope witness","consumerRef":"scope","currency":"IDR","lines":[{"description":"Service","quantity":1,"unitMinor":100}]})).await;
    let assigned = login(&app, "assigned").await;
    let attack = json!({"tenant":"acme","mutation":{"type":"issue_invoice","commandId":Uuid::new_v4(),"invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Foreign invoice"}});
    let (status, body) = call(
        &app,
        "POST",
        "/erp/billing/commands",
        attack,
        Some(&assigned),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "assigned mutation exposed foreign invoice: {body}"
    );
    let state: String = sqlx::query_scalar(
        "SELECT status FROM archava_billing.invoices WHERE invoice_code='SCOPE-FOREIGN'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(state, "draft");
    let own=submit(&app,&assigned,"billing",json!({"type":"create_invoice","invoiceCode":"SCOPE-OWN","customerRef":"Own scope","consumerRef":"scope","currency":"IDR","lines":[{"description":"Service","quantity":1,"unitMinor":100}]})).await;
    submit(&app,&assigned,"billing",json!({"type":"issue_invoice","invoiceId":own["invoiceId"],"expectedRevision":1,"reason":"Own invoice"})).await;
    sqlx::query("UPDATE archava_billing.memberships SET scope='all' WHERE tenant_id='acme' AND principal_id='assigned'").execute(&f.admin_pool).await.unwrap();
    let key = Uuid::new_v4();
    let request = json!({"tenant":"acme","mutation":{"type":"record_payment_attempt","commandId":key,"invoiceId":invoice["invoiceId"],"amountMinor":1}});
    let (status, body) = call(
        &app,
        "POST",
        "/erp/billing/commands",
        request.clone(),
        Some(&assigned),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    sqlx::query("UPDATE archava_billing.memberships SET scope='assigned' WHERE tenant_id='acme' AND principal_id='assigned'").execute(&f.admin_pool).await.unwrap();
    let (status, body) = call(
        &app,
        "POST",
        "/erp/billing/commands",
        request,
        Some(&assigned),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "replay must recheck current invoice scope: {body}"
    );
    let (status, body) = call(
        &app,
        "GET",
        &format!("/erp/billing/commands/status?tenant=acme&requestId={key}"),
        Value::Null,
        Some(&assigned),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "unknown", "{body}");
    assert!(body.get("receipt").is_none());
}
