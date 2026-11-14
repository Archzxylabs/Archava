use super::*;
async fn setup_trade() -> (Fixture, Router, Value, Value, Value, Value) {
    let (f, app, s) = setup().await;
    for actor in ["alice", "assigned", "reader"] {
        sqlx::query("INSERT INTO archava_organizations.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme',$1,'human',$2,'all')").bind(actor).bind(if actor=="reader"{vec!["organizations.read"]}else{vec!["organizations.read","organizations.write","parties.read","parties.write","companies.read","companies.write","audit.read"]}).execute(&f.admin_pool).await.unwrap();
    }
    sqlx::query(
        "UPDATE archava_commerce.memberships SET scope='all' WHERE principal_id='assigned'",
    )
    .execute(&f.admin_pool)
    .await
    .unwrap();
    let other = login(&app, "assigned").await;
    let a = company(&app, &s, "ALPHA").await;
    let b = company(&app, &other, "BETA").await;
    let o = organization(&app, &s, "Trading partner").await;
    let party=org_command(&app,&s,"party.create_from_organization",json!({"organizationId":o["organizationId"],"expectedSourceRevision":1,"customer":true,"supplier":true})).await;
    (f, app, s, a, b, party)
}
async fn org_command(app: &Router, s: &Value, kind: &str, data: Value) -> Value {
    let (st, b) = call(
        app,
        "POST",
        "/erp/organizations/commands",
        json!({"tenant":"acme","mutation":{"requestId":Uuid::new_v4(),"type":kind,"data":data}}),
        Some(s),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{b}");
    b["receipt"].clone()
}
async fn organization(app: &Router, s: &Value, name: &str) -> Value {
    org_command(
        app,
        s,
        "create_organization",
        json!({"name":name,"legalName":format!("{name} Official")}),
    )
    .await
}
async fn company(app: &Router, s: &Value, code: &str) -> Value {
    let o = organization(app, s, code).await;
    org_command(app,s,"company.create_from_organization",json!({"organizationId":o["organizationId"],"expectedSourceRevision":1,"code":code,"reason":"Declare managed company"})).await
}
async fn product(app: &Router, s: &Value, c: &Value, sku: &str) -> Value {
    submit(app,s,"commerce",json!({"operation":"product.create_for_company","companyId":c["companyId"],"expectedCompanyRevision":1,"sku":sku,"title":sku,"unit":"unit","trackStock":true,"price":15000})).await
}
fn sale(c: &Value, p: &Value, party: &Value) -> Value {
    json!({"operation":"order.create_for_company","companyId":c["companyId"],"expectedCompanyRevision":1,"partyId":party["partyId"],"expectedPartyRevision":1,"lines":[{"productId":p["id"],"quantity":2}]})
}
async fn read(app: &Router, s: &Value, domain: &str, c: Option<&Value>) -> (StatusCode, Value) {
    let company = c
        .map(|c| format!("&companyId={}", c["companyId"].as_str().unwrap()))
        .unwrap_or_default();
    call(
        app,
        "GET",
        &format!("/erp/{domain}/workspace?tenant=acme{company}"),
        Value::Null,
        Some(s),
    )
    .await
}
async fn raw(app: &Router, s: &Value, domain: &str, cmd: Value) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        &format!("/erp/{domain}/commands"),
        json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":cmd}),
        Some(s),
    )
    .await
}
#[tokio::test]
async fn company_trade_keeps_purchase_stock_sales_cost_and_invoice_in_one_company() {
    let (f, app, s, a, b, party) = setup_trade().await;
    let pa = product(&app, &s, &a, "ALPHA-ITEM").await;
    let pb = product(&app, &s, &b, "BETA-ITEM").await;
    let supplier=submit(&app,&s,"purchasing",json!({"operation":"supplier.create-from-party","code":"PARTNER","partyId":party["partyId"],"expectedPartyRevision":1})).await;
    let offer=submit(&app,&s,"purchasing",json!({"operation":"offer.create","supplierId":supplier["id"],"productId":pa["id"],"unitCost":6000})).await;
    let po=submit(&app,&s,"purchasing",json!({"operation":"order.create-for-company","companyId":a["companyId"],"expectedCompanyRevision":1,"supplierId":supplier["id"],"lines":[{"offerId":offer["id"],"quantity":5}]})).await;
    submit(&app,&s,"purchasing",json!({"operation":"order.confirm","id":po["id"],"expectedRevision":1,"reason":"Approve Alpha purchase"})).await;
    let (st, w) = read(&app, &s, "purchasing", Some(&a)).await;
    assert_eq!(st, StatusCode::OK, "{w}");
    let order = w["orders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == po["id"])
        .unwrap();
    submit(&app,&s,"purchasing",json!({"operation":"order.receive","id":po["id"],"expectedRevision":2,"reason":"Receive Alpha goods","lines":[{"lineId":order["lines"][0]["id"],"accepted":5,"rejected":0}]})).await;
    let sale = submit(&app, &s, "commerce", sale(&a, &pa, &party)).await;
    submit(
        &app,
        &s,
        "commerce",
        json!({"operation":"order.reserve","id":sale["id"],"expectedRevision":1,"holdSeconds":300}),
    )
    .await;
    submit(
        &app,
        &s,
        "commerce",
        json!({"operation":"order.fulfill","id":sale["id"],"expectedRevision":2}),
    )
    .await;
    let (st, w) = read(&app, &s, "commerce", Some(&a)).await;
    assert_eq!(st, StatusCode::OK, "{w}");
    let order = w["orders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == sale["id"])
        .unwrap();
    assert_eq!(order["companyId"], a["companyId"]);
    assert_eq!(order["fulfillmentCostMinor"], "12000");
    assert_eq!(w["products"][0]["quantity"], 3);
    let (st, r) =
        order_invoice_call(&app, &s, &order_invoice_command(order, "ALPHA-INVOICE")).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (st, w) = read(&app, &s, "billing", Some(&a)).await;
    assert_eq!(st, StatusCode::OK, "{w}");
    assert_eq!(w["invoices"][0]["companyId"], a["companyId"]);
    assert_eq!(w["invoices"][0]["gross"], 30000);
    for domain in ["commerce", "purchasing", "billing"] {
        let (st, w) = read(&app, &s, domain, None).await;
        assert_eq!(st, StatusCode::OK, "{w}");
        assert!(w[if domain == "billing" {
            "invoices"
        } else {
            "orders"
        }]
        .as_array()
        .unwrap()
        .is_empty());
    }
    let (st, w) = read(&app, &s, "commerce", Some(&b)).await;
    assert_eq!(st, StatusCode::OK, "{w}");
    assert_eq!(w["products"].as_array().unwrap().len(), 1);
    assert_eq!(w["products"][0]["id"], pb["id"]);
    assert_eq!(w["products"][0]["quantity"], 0);
    assert!(w["orders"].as_array().unwrap().is_empty());
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_commerce.movements WHERE company_id IS NOT NULL",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(n, 2);
}
#[tokio::test]
async fn company_trade_refuses_cross_company_and_legacy_paths_without_consuming_other_stock() {
    let (_f, app, s, a, b, party) = setup_trade().await;
    let pa = product(&app, &s, &a, "A-ITEM").await;
    let pb = product(&app, &s, &b, "B-ITEM").await;
    submit(&app,&s,"commerce",json!({"operation":"stock.adjust_for_company","companyId":a["companyId"],"expectedCompanyRevision":1,"productId":pa["id"],"quantity":10,"unitCostMinor":5000,"reason":"Alpha opening goods"})).await;
    assert!(raw(&app, &s, "commerce", sale(&b, &pa, &party))
        .await
        .0
        .is_client_error());
    assert!(raw(&app,&s,"commerce",json!({"operation":"order.create","customerName":"Bypass","lines":[{"productId":pa["id"],"quantity":1}]})).await.0.is_client_error());
    assert!(raw(
        &app,
        &s,
        "commerce",
        json!({"operation":"stock.adjust","productId":pa["id"],"quantity":1,"reason":"Bypass"})
    )
    .await
    .0
    .is_client_error());
    let order = submit(&app, &s, "commerce", sale(&b, &pb, &party)).await;
    assert_eq!(raw(&app,&s,"commerce",json!({"operation":"order.reserve","id":order["id"],"expectedRevision":1,"holdSeconds":300})).await.0,StatusCode::CONFLICT);
    let (st, w) = read(&app, &s, "commerce", Some(&a)).await;
    assert_eq!(st, StatusCode::OK, "{w}");
    assert_eq!(w["products"][0]["quantity"], 10);
}

#[tokio::test]
async fn company_history_survives_inactivity_but_new_business_requires_review() {
    let (_f, app, s, a, _b, party) = setup_trade().await;
    let p = product(&app, &s, &a, "HISTORY").await;
    submit(&app,&s,"commerce",json!({"operation":"stock.adjust_for_company","companyId":a["companyId"],"expectedCompanyRevision":1,"productId":p["id"],"quantity":5,"unitCostMinor":1000,"reason":"Reviewed opening count"})).await;
    let order = submit(&app, &s, "commerce", sale(&a, &p, &party)).await;
    submit(&app,&s,"commerce",json!({"operation":"order.reserve","id":order["id"],"expectedRevision":1,"holdSeconds":300})).await;
    org_command(&app,&s,"company.set_active",json!({"id":a["companyId"],"expectedRevision":1,"active":false,"reason":"Stop new business; finish commitments"})).await;
    assert_eq!(
        raw(&app, &s, "commerce", sale(&a, &p, &party)).await.0,
        StatusCode::CONFLICT
    );
    let mut updated = sale(&a, &p, &party);
    updated["expectedCompanyRevision"] = json!(2);
    assert_eq!(
        raw(&app, &s, "commerce", updated).await.0,
        StatusCode::CONFLICT
    );
    submit(
        &app,
        &s,
        "commerce",
        json!({"operation":"order.fulfill","id":order["id"],"expectedRevision":2}),
    )
    .await;
    let (_, w) = read(&app, &s, "commerce", Some(&a)).await;
    assert_eq!(w["orders"][0]["companyRevision"], 1);
    assert_eq!(w["products"][0]["quantity"], 3);
    let source = &w["orders"][0];
    let cmd = order_invoice_command(source, "HISTORICAL-INVOICE");
    assert_eq!(order_invoice_call(&app, &s, &cmd).await.0, StatusCode::OK);
    let (_, bill) = read(&app, &s, "billing", Some(&a)).await;
    assert_eq!(bill["invoices"][0]["companyRevision"], 1);
    assert_eq!(bill["invoices"][0]["companyName"], "ALPHA Official");
    submit(&app,&s,"commerce",json!({"operation":"order.return","id":order["id"],"expectedRevision":3,"lineId":source["lines"][0]["id"],"quantity":1,"disposition":"restock","reason":"Accepted historical return"})).await;
    assert_eq!(
        read(&app, &s, "commerce", Some(&a)).await.1["products"][0]["quantity"],
        4
    );
    org_command(&app,&s,"company.set_active",json!({"id":a["companyId"],"expectedRevision":2,"active":true,"reason":"Resume managed company"})).await;
    assert_eq!(
        raw(&app, &s, "commerce", sale(&a, &p, &party)).await.0,
        StatusCode::CONFLICT
    );
    let mut fresh = sale(&a, &p, &party);
    fresh["expectedCompanyRevision"] = json!(3);
    assert_eq!(raw(&app, &s, "commerce", fresh).await.0, StatusCode::OK);
}

#[tokio::test]
async fn company_scope_and_revocation_apply_to_reads_mutation_replay_and_status() {
    let (f, app, s, a, _b, party) = setup_trade().await;
    let p = product(&app, &s, &a, "ACCESS").await;
    let order = submit(&app, &s, "commerce", sale(&a, &p, &party)).await;
    let checkout = json!({"tenant":"acme","orderId":order["id"],"amount":30000,"currency":"IDR","item":"Owned company checkout"});
    assert_eq!(
        call(
            &app,
            "POST",
            "/embed/checkout/sign",
            checkout.clone(),
            Some(&s)
        )
        .await
        .0,
        StatusCode::OK
    );
    let assigned = login(&app, "assigned").await;
    sqlx::query("UPDATE archava_organizations.memberships SET scope='assigned' WHERE principal_id='assigned'").execute(&f.admin_pool).await.unwrap();
    for actor in [&assigned, &login(&app, "reader").await] {
        for domain in ["commerce", "purchasing", "billing"] {
            assert!(read(&app, actor, domain, Some(&a))
                .await
                .0
                .is_client_error());
        }
    }
    let key = Uuid::new_v4();
    let command = sale(&a, &p, &party);
    let body = json!({"tenant":"acme","requestId":key,"command":command});
    assert_eq!(
        call(
            &app,
            "POST",
            "/erp/commerce/commands",
            body.clone(),
            Some(&s)
        )
        .await
        .0,
        StatusCode::OK
    );
    let status = format!("/erp/commerce/commands/status?tenant=acme&requestId={key}");
    assert_eq!(
        call(&app, "GET", &status, Value::Null, Some(&s)).await.0,
        StatusCode::OK
    );
    sqlx::query("UPDATE archava_organizations.memberships SET permissions=array_remove(permissions,'companies.read') WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    assert_eq!(
        read(&app, &s, "commerce", Some(&a)).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "POST", "/erp/commerce/commands", body, Some(&s))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "GET", &status, Value::Null, Some(&s)).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(raw(&app,&s,"commerce",json!({"operation":"order.cancel","id":order["id"],"expectedRevision":1,"reason":"Access revoked"})).await.0,StatusCode::FORBIDDEN);
    assert_eq!(
        call(&app, "POST", "/embed/checkout/sign", checkout, Some(&s))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    // Unassigned legacy workspace stays usable and contains no company document.
    let (st, w) = read(&app, &s, "commerce", None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(w["orders"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn database_refuses_company_reassignment_and_cross_company_lines() {
    let (f, app, s, a, b, party) = setup_trade().await;
    let pa = product(&app, &s, &a, "DB-A").await;
    let pb = product(&app, &s, &b, "DB-B").await;
    let order = submit(&app, &s, "commerce", sale(&a, &pa, &party)).await;
    let error=sqlx::query("UPDATE archava_commerce.products SET company_id=$2,company_code='BETA',company_name='BETA Official',revision=revision+1 WHERE id=$1")
        .bind(Uuid::parse_str(pa["id"].as_str().unwrap()).unwrap()).bind(Uuid::parse_str(b["companyId"].as_str().unwrap()).unwrap()).execute(&f.admin_pool).await.unwrap_err();
    assert!(
        error.to_string().contains("company identity is immutable"),
        "{error}"
    );
    let error =
        sqlx::query("UPDATE archava_commerce.order_lines SET product_id=$2 WHERE order_id=$1")
            .bind(Uuid::parse_str(order["id"].as_str().unwrap()).unwrap())
            .bind(Uuid::parse_str(pb["id"].as_str().unwrap()).unwrap())
            .execute(&f.admin_pool)
            .await
            .unwrap_err();
    assert!(error.to_string().contains("another company"), "{error}");
    let error=sqlx::query("UPDATE archava_commerce.orders SET company_id=NULL,company_revision=NULL,company_code=NULL,company_name=NULL,revision=revision+1 WHERE id=$1")
        .bind(Uuid::parse_str(order["id"].as_str().unwrap()).unwrap()).execute(&f.admin_pool).await.unwrap_err();
    assert!(
        error.to_string().contains("company identity is immutable"),
        "{error}"
    );
    let (_, w) = read(&app, &s, "commerce", Some(&a)).await;
    assert_eq!(w["orders"][0]["lines"][0]["productId"], pa["id"]);
    assert_eq!(w["products"][0]["companyId"], a["companyId"]);
}

#[tokio::test]
async fn concurrent_company_reservations_cannot_oversell_or_borrow_another_company() {
    let (_f, app, s, a, b, party) = setup_trade().await;
    let pa = product(&app, &s, &a, "RACE-A").await;
    let pb = product(&app, &s, &b, "RACE-B").await;
    for (c, p) in [(&a, &pa), (&b, &pb)] {
        submit(&app,&s,"commerce",json!({"operation":"stock.adjust_for_company","companyId":c["companyId"],"expectedCompanyRevision":1,"productId":p["id"],"quantity":5,"reason":"Synthetic independent opening stock"})).await;
    }
    let mut orders = Vec::new();
    for (c, p) in [(&a, &pa), (&a, &pa), (&b, &pb)] {
        let mut cmd = sale(c, p, &party);
        cmd["lines"][0]["quantity"] = json!(4);
        orders.push(submit(&app, &s, "commerce", cmd).await);
    }
    let reserve = |n: usize| json!({"operation":"order.reserve","id":orders[n]["id"],"expectedRevision":1,"holdSeconds":300});
    let (ra, rb, rc) = tokio::join!(
        raw(&app, &s, "commerce", reserve(0)),
        raw(&app, &s, "commerce", reserve(1)),
        raw(&app, &s, "commerce", reserve(2))
    );
    assert_eq!(
        usize::from(ra.0 == StatusCode::OK) + usize::from(rb.0 == StatusCode::OK),
        1
    );
    assert_eq!(rc.0, StatusCode::OK, "{rc:?}");
    assert!([ra.0, rb.0].contains(&StatusCode::CONFLICT));
    for c in [&a, &b] {
        let (_, w) = read(&app, &s, "commerce", Some(c)).await;
        assert_eq!(w["products"][0]["quantity"], 5);
        assert_eq!(w["products"][0]["available"], 1);
    }
}

#[tokio::test]
async fn company_trade_upgrade_preserves_legacy_identity_and_original_receipts() {
    let f = archava_pg_fixture::provision_pre_company_trade_profile().await;
    assert_eq!(f.commerce_ledger.len(), 8);
    assert_eq!(f.purchasing_ledger.len(), 4);
    assert_eq!(f.billing_ledger.len(), 7);
    sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope,role,capability) VALUES('acme','alice','human',ARRAY['catalog.read','catalog.write','orders.read','orders.write'],'all','owner','enterprise')").execute(&f.admin_pool).await.unwrap();
    let mut tx = archava_adapters::rls::TenantTransaction::begin(
        &f.runtime_pool,
        "archava_commerce",
        "Commerce",
        archava_core::tenant::TenantId::new("acme").unwrap(),
    )
    .await
    .unwrap();
    let principal = archava_domain::erp::purchasing::PurchasePrincipal::human("alice");
    let context = archava_adapters::erp::commerce_kernel::resolve_commerce_context(
        tx.checked_executor().await.unwrap(),
        "acme",
        &principal,
    )
    .await
    .unwrap();
    let command=serde_json::from_value(json!({"operation":"product.create","sku":"BEFORE-COMPANY","title":"Unassigned historical goods","unit":"unit","trackStock":false,"price":1000})).unwrap();
    let p = archava_adapters::erp::commerce::record(
        tx.checked_executor().await.unwrap(),
        "acme",
        &principal,
        &context,
        Uuid::new_v4(),
        &"a".repeat(64),
        &command,
    )
    .await
    .unwrap();
    let command=serde_json::from_value(json!({"operation":"order.create","customerName":"Historical customer","lines":[{"productId":p.id,"quantity":1}]})).unwrap();
    archava_adapters::erp::commerce::record(
        tx.checked_executor().await.unwrap(),
        "acme",
        &principal,
        &context,
        Uuid::new_v4(),
        &"b".repeat(64),
        &command,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let statement="SELECT json_build_object('products',(SELECT json_agg(to_jsonb(p)-'company_id'-'company_revision'-'company_code'-'company_name' ORDER BY id) FROM archava_commerce.products p),'orders',(SELECT json_agg(to_jsonb(o)-'company_id'-'company_revision'-'company_code'-'company_name'-'tax_code'-'price_includes_tax' ORDER BY id) FROM archava_commerce.orders o),'commands',(SELECT json_agg(to_jsonb(c) ORDER BY request_id) FROM archava_commerce.commands c))";
    let before: Value = sqlx::query_scalar(statement)
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    archava_pg_fixture::apply_pending_party_transaction_migrations(&f.owner_pool).await;
    archava_pg_fixture::apply_pending_party_transaction_migrations(&f.owner_pool).await;
    let after: Value = sqlx::query_scalar(statement)
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(before, after);
    // The later sales-tax columns are added empty: an upgrade taxes no order.
    let taxed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_commerce.orders \
         WHERE (to_jsonb(orders)->>'tax_code') IS NOT NULL \
         OR (to_jsonb(orders)->>'price_includes_tax') IS NOT NULL",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(taxed, 0);
    let assigned: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_commerce.products WHERE company_id IS NOT NULL",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(assigned, 0);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_organizations.companies")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn company_gateway_service_finalizes_bound_creation_without_human_company_grants() {
    use archava_adapters::{
        erp::{billing::resolve_billing_context, payments},
        rls::TenantTransaction,
    };
    use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
    let (f, app, s, a, _b, party) = setup_trade().await;
    let p = product(&app, &s, &a, "GATEWAY-COMPANY").await;
    submit(&app,&s,"commerce",json!({"operation":"stock.adjust_for_company","companyId":a["companyId"],"expectedCompanyRevision":1,"productId":p["id"],"quantity":2,"reason":"Owned service-boundary goods"})).await;
    let order = submit(&app, &s, "commerce", sale(&a, &p, &party)).await;
    submit(&app,&s,"commerce",json!({"operation":"order.reserve","id":order["id"],"expectedRevision":1,"holdSeconds":300})).await;
    submit(
        &app,
        &s,
        "commerce",
        json!({"operation":"order.fulfill","id":order["id"],"expectedRevision":2}),
    )
    .await;
    let (_, w) = read(&app, &s, "commerce", Some(&a)).await;
    let source = w["orders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == order["id"])
        .unwrap();
    let (st, receipt) =
        order_invoice_call(&app, &s, &order_invoice_command(source, "COMPANY-GATEWAY")).await;
    assert_eq!(st, StatusCode::OK, "{receipt}");
    let invoice = Uuid::parse_str(receipt["invoiceId"].as_str().unwrap()).unwrap();
    submit(&app,&s,"billing",json!({"type":"issue_invoice","invoiceId":invoice,"expectedRevision":1,"reason":"Issue service-boundary invoice"})).await;
    sqlx::query("INSERT INTO archava_billing.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','company-gateway','service',ARRAY['billing.read','billing.write'],'all')").execute(&f.admin_pool).await.unwrap();
    let tenant = archava_core::tenant::TenantId::new("acme").unwrap();
    let human = PurchasePrincipal::human("alice");
    let service = PurchasePrincipal {
        kind: PrincipalKind::Service,
        id: "company-gateway".into(),
        on_behalf_of: None,
    };
    let key = Uuid::new_v4();
    let mut tx = TenantTransaction::begin(
        &f.runtime_pool,
        "archava_billing",
        "billing",
        tenant.clone(),
    )
    .await
    .unwrap();
    let c = tx.checked_executor().await.unwrap();
    let access = resolve_billing_context(c, "acme", &human).await.unwrap();
    let (link, invoke) = payments::request_link(
        c,
        "acme",
        &human,
        &access,
        payments::LinkRequest {
            request_id: key,
            invoice_id: invoice,
            integration_id: "owned-company-gateway",
            service_id: &service.id,
        },
    )
    .await
    .unwrap();
    assert!(invoke);
    tx.commit().await.unwrap();
    // The remote invocation was authorized and claimed before this revocation.
    sqlx::query("UPDATE archava_organizations.memberships SET permissions=array_remove(permissions,'companies.read') WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    let mut tx = TenantTransaction::begin(
        &f.runtime_pool,
        "archava_billing",
        "billing",
        tenant.clone(),
    )
    .await
    .unwrap();
    let c = tx.checked_executor().await.unwrap();
    let access = resolve_billing_context(c, "acme", &service).await.unwrap();
    let binding = payments::binding(
        c,
        "acme",
        &service,
        &access,
        "owned-company-gateway",
        &link.order_id,
    )
    .await
    .unwrap();
    let finished = payments::finish_creation(
        c,
        "acme",
        &service,
        &access,
        &binding,
        Some("https://example.invalid/owned-checkout"),
    )
    .await
    .unwrap();
    assert_eq!(finished.state, "open");
    tx.commit().await.unwrap();
    let status = format!("/erp/billing/payments/status?tenant=acme&requestId={key}");
    assert_eq!(
        call(&app, "GET", &status, Value::Null, Some(&s)).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE archava_organizations.memberships SET permissions=array_append(permissions,'companies.read') WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    let (st, recovered) = call(&app, "GET", &status, Value::Null, Some(&s)).await;
    assert_eq!(st, StatusCode::OK, "{recovered}");
    assert_eq!(recovered["link"]["id"], link.id);
    assert_eq!(recovered["link"]["state"], "open");
    let service_company_memberships:i64=sqlx::query_scalar("SELECT count(*) FROM archava_organizations.memberships WHERE principal_id='company-gateway'").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(service_company_memberships, 0);
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_billing.gateway_links WHERE invoice_id=$1",
    )
    .bind(invoice)
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(rows, 1);
}

#[path = "supplier_documents.rs"]
mod supplier_documents;
