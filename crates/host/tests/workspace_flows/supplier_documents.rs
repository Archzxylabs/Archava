use super::*;
async fn purchase(
    accepted: i64,
    rejected: i64,
) -> (Fixture, Router, Value, Value, Value, Value, Value) {
    let (f, app, s, a, _b, party) = setup_trade().await;
    sqlx::query("UPDATE archava_purchasing.memberships SET permissions=permissions||ARRAY['documents.read','documents.write'] WHERE principal_id IN ('alice','assigned')").execute(&f.admin_pool).await.unwrap();
    let p = product(&app, &s, &a, "DOCUMENT-GOODS").await;
    let supplier=submit(&app,&s,"purchasing",json!({"operation":"supplier.create-from-party","code":"DOCUMENT-VENDOR","partyId":party["partyId"],"expectedPartyRevision":1})).await;
    let offer=submit(&app,&s,"purchasing",json!({"operation":"offer.create","supplierId":supplier["id"],"productId":p["id"],"unitCost":6000})).await;
    let po=submit(&app,&s,"purchasing",json!({"operation":"order.create-for-company","companyId":a["companyId"],"expectedCompanyRevision":1,"supplierId":supplier["id"],"lines":[{"offerId":offer["id"],"quantity":5}]})).await;
    submit(&app,&s,"purchasing",json!({"operation":"order.confirm","id":po["id"],"expectedRevision":1,"reason":"Approved commitment"})).await;
    let (_, w) = read(&app, &s, "purchasing", Some(&a)).await;
    let mut order = w["orders"][0].clone();
    if accepted + rejected > 0 {
        submit(&app,&s,"purchasing",json!({"operation":"order.receive","id":po["id"],"expectedRevision":2,"reason":"Inspected goods","lines":[{"lineId":order["lines"][0]["id"],"accepted":accepted,"rejected":rejected}]})).await;
        let (_, w) = read(&app, &s, "purchasing", Some(&a)).await;
        order = w["orders"][0].clone();
    }
    (f, app, s, a, p, supplier, order)
}
fn claim(order: &Value, reference: &str, quantity: i64, cost: i64, total: i64) -> Value {
    json!({"operation":"document.capture","orderId":order["id"],"expectedOrderRevision":order["revision"],"supplierId":order["supplierId"],"vendorReference":reference,"documentDate":"2026-11-07","currency":"IDR","declaredTotal":total,"lines":[{"lineId":order["lines"][0]["id"],"quantity":quantity,"unitCost":cost}]})
}
fn reconcile(d: &Value, order: &Value, revision: i64) -> Value {
    json!({"operation":"document.match","id":d["id"],"expectedRevision":revision,"expectedOrderRevision":order["revision"],"reason":"Review vendor claim against accepted goods"})
}
async fn document(app: &Router, s: &Value, a: &Value, id: &Value) -> Value {
    let (st, w) = read(app, s, "purchasing", Some(a)).await;
    assert_eq!(st, StatusCode::OK, "{w}");
    w["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| &d["id"] == id)
        .unwrap()
        .clone()
}
async fn invariant(f: &Fixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('movements',(SELECT jsonb_agg(to_jsonb(x) ORDER BY to_jsonb(x)::text) FROM archava_commerce.movements x),'layers',(SELECT jsonb_agg(to_jsonb(x) ORDER BY to_jsonb(x)::text) FROM archava_commerce.inventory_layers x),'stock',(SELECT jsonb_agg(to_jsonb(x) ORDER BY product_id) FROM archava_commerce.stock_balances x),'invoices',(SELECT count(*) FROM archava_billing.invoices),'payments',(SELECT count(*) FROM archava_billing.gateway_links))").fetch_one(&f.admin_pool).await.unwrap()
}
#[tokio::test]
async fn supplier_documents_match_void_release_and_recover_without_posting_or_stock_effects() {
    let (f, app, s, a, _p, _vendor, order) = purchase(5, 0).await;
    let before = invariant(&f).await;
    let d = submit(
        &app,
        &s,
        "purchasing",
        claim(&order, "INV-001", 5, 6000, 30000),
    )
    .await;
    let request = Uuid::new_v4();
    let body = json!({"tenant":"acme","requestId":request,"command":reconcile(&d,&order,1)});
    let (st, receipt) = call(
        &app,
        "POST",
        "/erp/purchasing/commands",
        body.clone(),
        Some(&s),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{receipt}");
    let matched = document(&app, &s, &a, &d["id"]).await;
    assert_eq!(matched["state"], "matched");
    assert_eq!(matched["activeAllocatedQuantity"], 5);
    assert_eq!(matched["accountingStatus"], "not_inferred");
    assert_eq!(matched["matchEvidence"]["reasons"], json!([]));
    let other = submit(
        &app,
        &s,
        "purchasing",
        claim(&order, "INV-002", 5, 6000, 30000),
    )
    .await;
    submit(&app, &s, "purchasing", reconcile(&other, &order, 1)).await;
    assert_eq!(document(&app, &s, &a, &other["id"]).await["state"], "held");
    submit(&app,&s,"purchasing",json!({"operation":"document.void","id":d["id"],"expectedRevision":2,"reason":"Vendor claim withdrawn"})).await;
    submit(&app, &s, "purchasing", reconcile(&other, &order, 2)).await;
    assert_eq!(
        document(&app, &s, &a, &other["id"]).await["activeAllocatedQuantity"],
        5
    );
    let (st, replayed) = call(&app, "POST", "/erp/purchasing/commands", body, Some(&s)).await;
    assert_eq!(st, StatusCode::OK, "{replayed}");
    assert_eq!(replayed["receipt"]["revision"], 2);
    assert_eq!(replayed["receipt"]["replayed"], true);
    let (st, status) = call(
        &app,
        "GET",
        &format!("/erp/purchasing/commands/status?tenant=acme&requestId={request}"),
        Value::Null,
        Some(&s),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{status}");
    assert_eq!(status["receipt"]["revision"], 2);
    assert_eq!(
        raw(
            &app,
            &s,
            "purchasing",
            claim(&order, " inv-001 ", 1, 6000, 6000)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(before, invariant(&f).await);
    let history = document(&app, &s, &a, &d["id"]).await;
    assert_eq!(history["versions"].as_array().unwrap().len(), 3);
    assert_eq!(history["versions"][1]["state"], "matched");
}
#[tokio::test]
async fn supplier_documents_hold_price_total_rejection_and_correct_explicitly() {
    let (f, app, s, a, _p, _vendor, order) = purchase(3, 2).await;
    let d = submit(
        &app,
        &s,
        "purchasing",
        claim(&order, "INV-HOLD", 5, 7000, 35001),
    )
    .await;
    submit(&app, &s, "purchasing", reconcile(&d, &order, 1)).await;
    let held = document(&app, &s, &a, &d["id"]).await;
    assert_eq!(held["state"], "held");
    assert_eq!(
        held["matchEvidence"]["reasons"].as_array().unwrap().len(),
        3
    );
    assert_eq!(held["activeAllocatedQuantity"], 0);
    assert_eq!(held["matchEvidence"]["lines"][0]["accepted"], 3);
    assert_eq!(held["matchEvidence"]["lines"][0]["rejected"], 2);
    submit(&app,&s,"purchasing",json!({"operation":"document.revise","id":d["id"],"expectedRevision":2,"expectedOrderRevision":order["revision"],"documentDate":"2026-11-07","currency":"IDR","declaredTotal":18000,"lines":[{"lineId":order["lines"][0]["id"],"quantity":3,"unitCost":6000}],"reason":"Supplier corrected rejected goods and price"})).await;
    // Historical commitments can still be reconciled after published identity deactivation.
    org_command(&app,&s,"company.set_active",json!({"id":a["companyId"],"expectedRevision":1,"active":false,"reason":"No new commitments"})).await;
    submit(&app, &s, "purchasing", reconcile(&d, &order, 3)).await;
    assert_eq!(document(&app, &s, &a, &d["id"]).await["state"], "matched");
    assert_eq!(raw(&app,&s,"purchasing",json!({"operation":"document.revise","id":d["id"],"expectedRevision":4,"expectedOrderRevision":order["revision"],"documentDate":"2026-11-07","currency":"IDR","declaredTotal":1,"lines":[{"lineId":order["lines"][0]["id"],"quantity":1,"unitCost":1}],"reason":"Cannot silently edit a matched claim"})).await.0,StatusCode::CONFLICT);
    let n:i64=sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.supplier_document_allocations WHERE released_revision IS NULL").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(n, 1);
}
#[tokio::test]
async fn supplier_documents_recheck_authority_scope_and_receipt_access() {
    let (f, app, s, a, _p, _vendor, order) = purchase(5, 0).await;
    let input = claim(&order, "INV-AUTH", 1, 6000, 6000);
    let request = Uuid::new_v4();
    let body = json!({"tenant":"acme","requestId":request,"command":input});
    let (st, d) = call(
        &app,
        "POST",
        "/erp/purchasing/commands",
        body.clone(),
        Some(&s),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{d}");
    sqlx::query("UPDATE archava_organizations.memberships SET permissions=array_remove(permissions,'companies.read') WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    assert_eq!(
        read(&app, &s, "purchasing", Some(&a)).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/erp/purchasing/commands",
            body.clone(),
            Some(&s)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        raw(&app, &s, "purchasing", reconcile(&d["receipt"], &order, 1))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/erp/purchasing/commands/status?tenant=acme&requestId={request}"),
            Value::Null,
            Some(&s)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE archava_organizations.memberships SET permissions=permissions||ARRAY['companies.read'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    sqlx::query("UPDATE archava_purchasing.memberships SET permissions=array_remove(permissions,'documents.write') WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    assert_eq!(
        call(&app, "POST", "/erp/purchasing/commands", body, Some(&s))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let assigned = login(&app, "assigned").await;
    assert_eq!(
        raw(
            &app,
            &assigned,
            "purchasing",
            reconcile(&d["receipt"], &order, 1)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let mut wrong = claim(&order, "INV-WRONG", 1, 6000, 6000);
    wrong["supplierId"] = json!(Uuid::new_v4());
    assert_eq!(
        raw(&app, &assigned, "purchasing", wrong).await.0,
        StatusCode::NOT_FOUND
    );
    let (_, view) = read(&app, &assigned, "purchasing", Some(&a)).await;
    assert_eq!(view["documents"], json!([]));
}
#[tokio::test]
async fn supplier_documents_competing_claims_cannot_double_allocate_accepted_goods() {
    let (f, app, s, a, _p, _vendor, order) = purchase(5, 0).await;
    let d1 = submit(
        &app,
        &s,
        "purchasing",
        claim(&order, "INV-RACE-1", 5, 6000, 30000),
    )
    .await;
    let d2 = submit(
        &app,
        &s,
        "purchasing",
        claim(&order, "INV-RACE-2", 5, 6000, 30000),
    )
    .await;
    let (c1, c2) = tokio::join!(
        raw(&app, &s, "purchasing", reconcile(&d1, &order, 1)),
        raw(&app, &s, "purchasing", reconcile(&d2, &order, 1))
    );
    assert_eq!(c1.0, StatusCode::OK, "{:?}", c1);
    assert_eq!(c2.0, StatusCode::OK, "{:?}", c2);
    let x = document(&app, &s, &a, &d1["id"]).await;
    let y = document(&app, &s, &a, &d2["id"]).await;
    assert_eq!(
        [&x, &y].iter().filter(|d| d["state"] == "matched").count(),
        1
    );
    assert_eq!([&x, &y].iter().filter(|d| d["state"] == "held").count(), 1);
    let total:i64=sqlx::query_scalar("SELECT sum(quantity)::bigint FROM archava_purchasing.supplier_document_allocations WHERE released_revision IS NULL").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(total, 5);
}
#[tokio::test]
async fn supplier_documents_database_refuses_identity_capacity_and_fabricated_history() {
    let (f, app, s, a, _p, _vendor, order) = purchase(5, 0).await;
    let d = submit(
        &app,
        &s,
        "purchasing",
        claim(&order, "INV-DB", 5, 6000, 30000),
    )
    .await;
    submit(&app, &s, "purchasing", reconcile(&d, &order, 1)).await;
    let id = Uuid::parse_str(d["id"].as_str().unwrap()).unwrap();
    let mut tx = f.admin_pool.begin().await.unwrap();
    assert!(sqlx::query("UPDATE archava_purchasing.supplier_documents SET vendor_reference='FAKE',revision=revision+1 WHERE id=$1").bind(id).execute(&mut *tx).await.is_err());
    tx.rollback().await.unwrap();
    let mut tx = f.admin_pool.begin().await.unwrap();
    assert!(sqlx::query("UPDATE archava_purchasing.supplier_document_allocations SET quantity=quantity+1 WHERE document_id=$1").bind(id).execute(&mut *tx).await.is_err());
    tx.rollback().await.unwrap();
    let mut tx = f.admin_pool.begin().await.unwrap();
    sqlx::query("UPDATE archava_purchasing.supplier_documents SET state='void',revision=revision+1 WHERE id=$1").bind(id).execute(&mut *tx).await.unwrap();
    assert!(tx.commit().await.is_err());
    assert_eq!(document(&app, &s, &a, &d["id"]).await["state"], "matched");
}

#[tokio::test]
async fn supplier_documents_upgrade_preserves_real_prior_purchase_receipt_and_stock_twice() {
    let f = archava_pg_fixture::provision_pre_supplier_documents_profile().await;
    assert_eq!(f.purchasing_ledger.len(), 5);
    sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope,role,capability) VALUES('acme','alice','human',ARRAY['catalog.read','catalog.write','stock.read','stock.write','orders.read','orders.write'],'all','owner','enterprise')").execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_purchasing.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','alice','human',ARRAY['suppliers.read','suppliers.write','offers.read','offers.write','orders.read','orders.write','receipts.read','receipts.write','audit.read'],'all')").execute(&f.admin_pool).await.unwrap();
    let principal = archava_domain::erp::purchasing::PurchasePrincipal::human("alice");
    let mut tx = archava_adapters::rls::TenantTransaction::begin(
        &f.runtime_pool,
        "archava_purchasing",
        "Purchasing",
        archava_core::tenant::TenantId::new("acme").unwrap(),
    )
    .await
    .unwrap();
    let context = archava_adapters::erp::commerce_kernel::resolve_commerce_context(
        tx.checked_executor().await.unwrap(),
        "acme",
        &principal,
    )
    .await
    .unwrap();
    let p=archava_adapters::erp::commerce::record(tx.checked_executor().await.unwrap(),"acme",&principal,&context,Uuid::new_v4(),&"a".repeat(64),&serde_json::from_value(json!({"operation":"product.create","sku":"BEFORE-DOCUMENTS","title":"Historical goods","unit":"unit","trackStock":true,"price":9000})).unwrap()).await.unwrap();
    async fn old(
        c: &mut sqlx::PgConnection,
        command: Value,
    ) -> archava_domain::erp::purchasing::PurchaseReceipt {
        let principal = archava_domain::erp::purchasing::PurchasePrincipal::human("alice");
        let context =
            archava_adapters::erp::purchasing::resolve_purchasing_context(c, "acme", &principal)
                .await
                .unwrap();
        let m = archava_domain::erp::purchasing::PurchaseMutation::parse(
            &json!({"requestId":Uuid::new_v4(),"command":command}),
        )
        .unwrap();
        archava_adapters::erp::purchasing::record_purchase_command(
            c,
            "acme",
            &principal,
            &context,
            &"b".repeat(64),
            &m,
        )
        .await
        .unwrap()
    }
    let supplier = old(
        tx.checked_executor().await.unwrap(),
        json!({"operation":"supplier.create","code":"OLD-DOC-VENDOR","name":"Historical vendor"}),
    )
    .await;
    let offer=old(tx.checked_executor().await.unwrap(),json!({"operation":"offer.create","supplierId":supplier.id,"productId":p.id,"unitCost":6000})).await;
    let po=old(tx.checked_executor().await.unwrap(),json!({"operation":"order.create","supplierId":supplier.id,"lines":[{"offerId":offer.id,"quantity":2}]})).await;
    old(tx.checked_executor().await.unwrap(),json!({"operation":"order.confirm","id":po.id,"expectedRevision":1,"reason":"Historical approval"})).await;
    let line: Uuid = sqlx::query_scalar(
        "SELECT id FROM archava_purchasing.lines WHERE tenant_id='acme' AND order_id=$1",
    )
    .bind(Uuid::parse_str(&po.id).unwrap())
    .fetch_one(tx.checked_executor().await.unwrap())
    .await
    .unwrap();
    old(tx.checked_executor().await.unwrap(),json!({"operation":"order.receive","id":po.id,"expectedRevision":2,"reason":"Historical accepted receipt","lines":[{"lineId":line,"accepted":2,"rejected":0}]})).await;
    tx.commit().await.unwrap();
    let statement="SELECT jsonb_build_object('suppliers',(SELECT jsonb_agg(to_jsonb(x) ORDER BY id) FROM archava_purchasing.suppliers x),'offers',(SELECT jsonb_agg(to_jsonb(x) ORDER BY id) FROM archava_purchasing.offers x),'orders',(SELECT jsonb_agg(to_jsonb(x) ORDER BY id) FROM archava_purchasing.orders x),'lines',(SELECT jsonb_agg(to_jsonb(x) ORDER BY id) FROM archava_purchasing.lines x),'receipts',(SELECT jsonb_agg(to_jsonb(x) ORDER BY request_id) FROM archava_purchasing.receipts x),'commands',(SELECT jsonb_agg(to_jsonb(x) ORDER BY request_id) FROM archava_purchasing.commands x),'stock',(SELECT jsonb_agg(to_jsonb(x) ORDER BY product_id) FROM archava_commerce.stock_balances x))";
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
    let documents: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.supplier_documents")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(documents, 0);
    let mut tx = archava_adapters::rls::TenantTransaction::begin(
        &f.runtime_pool,
        "archava_purchasing",
        "Purchasing",
        archava_core::tenant::TenantId::new("acme").unwrap(),
    )
    .await
    .unwrap();
    let context = archava_adapters::erp::purchasing::resolve_purchasing_context(
        tx.checked_executor().await.unwrap(),
        "acme",
        &principal,
    )
    .await
    .unwrap();
    let status = archava_adapters::erp::purchasing::command_status(
        tx.checked_executor().await.unwrap(),
        "acme",
        &principal,
        &context,
        Uuid::parse_str(&po.request_id).unwrap(),
    )
    .await
    .unwrap();
    let archava_domain::erp::purchasing::PurchaseCommandStatus::Committed { receipt } = status
    else {
        panic!("Historical PO receipt lost")
    };
    assert_eq!(receipt.id, po.id);
    assert_eq!(receipt.revision, 1);
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn supplier_documents_later_receipt_requires_new_review_and_replay_keeps_held_history() {
    let (_f, app, s, a, _p, _vendor, order) = purchase(0, 0).await;
    let d = submit(
        &app,
        &s,
        "purchasing",
        claim(&order, "INV-EARLY", 5, 6000, 30000),
    )
    .await;
    let request = Uuid::new_v4();
    let body = json!({"tenant":"acme","requestId":request,"command":reconcile(&d,&order,1)});
    assert_eq!(
        call(
            &app,
            "POST",
            "/erp/purchasing/commands",
            body.clone(),
            Some(&s)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(document(&app, &s, &a, &d["id"]).await["state"], "held");
    submit(&app,&s,"purchasing",json!({"operation":"order.receive","id":order["id"],"expectedRevision":2,"reason":"Goods arrived later","lines":[{"lineId":order["lines"][0]["id"],"accepted":5,"rejected":0}]})).await;
    let (st, replay) = call(&app, "POST", "/erp/purchasing/commands", body, Some(&s)).await;
    assert_eq!(st, StatusCode::OK, "{replay}");
    assert_eq!(replay["receipt"]["revision"], 2);
    assert_eq!(document(&app, &s, &a, &d["id"]).await["state"], "held");
    assert_eq!(
        raw(&app, &s, "purchasing", reconcile(&d, &order, 2))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let (_, w) = read(&app, &s, "purchasing", Some(&a)).await;
    submit(&app, &s, "purchasing", reconcile(&d, &w["orders"][0], 2)).await;
    let current = document(&app, &s, &a, &d["id"]).await;
    assert_eq!(current["state"], "matched");
    assert_eq!(current["versions"][1]["state"], "held");
}
