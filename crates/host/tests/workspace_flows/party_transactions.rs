use super::*;
async fn fixture() -> (Fixture, Router, Value, Value, Value) {
    let (f, app, s) = setup().await;
    for actor in ["alice", "reader", "assigned", "service-seat"] {
        sqlx::query("INSERT INTO archava_organizations.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme',$1,$2,ARRAY['parties.read','parties.write','organizations.read','organizations.write'],'all')").bind(actor).bind(if actor=="service-seat"{"service"}else{"human"}).execute(&f.admin_pool).await.unwrap();
    }
    let o = org(
        &app,
        &s,
        "create_organization",
        json!({"name":"Shared business"}),
    )
    .await;
    let p=org(&app,&s,"party.create_from_organization",json!({"organizationId":o["organizationId"],"expectedSourceRevision":1,"customer":true,"supplier":true})).await;
    let product=submit(&app,&s,"commerce",json!({"operation":"product.create","sku":"PARTY-ITEM","title":"Party item","unit":"unit","trackStock":true,"price":15000})).await;
    submit(&app,&s,"commerce",json!({"operation":"stock.adjust","productId":product["id"],"quantity":10,"unitCostMinor":5000,"reason":"Synthetic opening goods"})).await;
    (f, app, s, p, product)
}
async fn org(app: &Router, s: &Value, kind: &str, data: Value) -> Value {
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
fn sales(p: &Value, product: &Value, revision: i64) -> Value {
    json!({"operation":"order.create_from_party","partyId":p["partyId"],"expectedPartyRevision":revision,"lines":[{"productId":product["id"],"quantity":2}]})
}
fn supplier(p: &Value, revision: i64) -> Value {
    json!({"operation":"supplier.create-from-party","code":"SHARED","partyId":p["partyId"],"expectedPartyRevision":revision})
}
fn input(command: Value) -> Value {
    json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":command})
}
async fn raw(app: &Router, s: &Value, domain: &str, b: &Value) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        &format!("/erp/{domain}/commands"),
        b.clone(),
        Some(s),
    )
    .await
}
async fn roles(app: &Router, s: &Value, p: &Value, revision: i64, customer: bool, supplier: bool) {
    org(app,s,"party.set_roles",json!({"id":p["partyId"],"expectedRevision":revision,"customer":customer,"supplier":supplier,"reason":"Reviewed role change"})).await;
}
fn row(workspace: &Value, collection: &str, id: &Value) -> Value {
    workspace[collection]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == *id)
        .unwrap()
        .clone()
}
async fn purchase(app: &Router, s: &Value, supplier: &Value, product: &Value) -> (Value, Value) {
    let offer=submit(app,s,"purchasing",json!({"operation":"offer.create","supplierId":supplier["id"],"productId":product["id"],"unitCost":6000})).await;
    let order=submit(app,s,"purchasing",json!({"operation":"order.create","supplierId":supplier["id"],"lines":[{"offerId":offer["id"],"quantity":3}]})).await;
    (offer, order)
}
#[tokio::test]
async fn party_transactions_share_one_master_and_keep_history_after_roles_and_read_access_end() {
    let (f, app, s, p, product) = fixture().await;
    let sale = submit(&app, &s, "commerce", sales(&p, &product, 1)).await;
    let supplier = submit(&app, &s, "purchasing", supplier(&p, 1)).await;
    let (_, po) = purchase(&app, &s, &supplier, &product).await;
    roles(&app, &s, &p, 1, false, false).await;
    sqlx::query("UPDATE archava_organizations.memberships SET permissions=ARRAY['parties.write'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
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
    let (_, w) = snapshot(&app, &s, "commerce").await;
    let sale = row(&w, "orders", &sale["id"]);
    let (st, b) =
        order_invoice_call(&app, &s, &order_invoice_command(&sale, "PARTY-INVOICE")).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    submit(&app,&s,"purchasing",json!({"operation":"order.confirm","id":po["id"],"expectedRevision":1,"reason":"Original commitment"})).await;
    let (_, w) = snapshot(&app, &s, "purchasing").await;
    let po = row(&w, "orders", &po["id"]);
    submit(&app,&s,"purchasing",json!({"operation":"order.receive","id":po["id"],"expectedRevision":2,"reason":"Goods checked","lines":[{"lineId":po["lines"][0]["id"],"accepted":3,"rejected":0}]})).await;
    let (_, w) = snapshot(&app, &s, "billing").await;
    let invoice = &w["invoices"][0];
    for record in [&sale, invoice] {
        assert_eq!(record["customerPartyId"], p["partyId"]);
        assert_eq!(record["customerPartyRevision"], 1);
        assert!(record["customerContactId"].is_null());
    }
    assert_eq!(invoice["customer"], "Shared business");
    assert_eq!(po["supplierPartyId"], p["partyId"]);
    assert_eq!(po["supplierPartyRevision"], 1);
    assert!(po["supplierContactId"].is_null());
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_purchasing.receipts WHERE tenant_id='acme'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}
#[tokio::test]
async fn party_transaction_replay_rechecks_read_scope_but_preserves_the_original_role_and_version()
{
    let (f, app, s, p, product) = fixture().await;
    let commands = [
        ("commerce", input(sales(&p, &product, 1))),
        ("purchasing", input(supplier(&p, 1))),
    ];
    for (domain, b) in &commands {
        assert_eq!(raw(&app, &s, domain, b).await.0, StatusCode::OK);
    }
    roles(&app, &s, &p, 1, false, false).await;
    for (domain, b) in &commands {
        let (st, out) = raw(&app, &s, domain, b).await;
        assert_eq!(st, StatusCode::OK, "{out}");
        let r = out.get("receipt").unwrap_or(&out);
        if *domain == "purchasing" {
            assert_eq!(r["replayed"], true);
        } else {
            assert!(r["id"].is_string());
        }
    }
    for (domain, cmd) in [
        ("commerce", sales(&p, &product, 2)),
        ("purchasing", supplier(&p, 2)),
    ] {
        assert_eq!(
            raw(&app, &s, domain, &input(cmd)).await.0,
            StatusCode::CONFLICT
        );
    }
    sqlx::query("UPDATE archava_organizations.memberships SET permissions=ARRAY['parties.write'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    for (domain, b) in &commands {
        assert_eq!(raw(&app, &s, domain, b).await.0, StatusCode::FORBIDDEN);
        let path = format!(
            "/erp/{domain}/commands/status?tenant=acme&requestId={}",
            b["requestId"].as_str().unwrap()
        );
        assert_eq!(
            call(&app, "GET", &path, Value::Null, Some(&s)).await.0,
            StatusCode::OK
        );
    }
    sqlx::query("UPDATE archava_organizations.memberships SET permissions=ARRAY['parties.read'],scope='assigned' WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    sqlx::query(
        "UPDATE archava_organizations.parties SET owner_id='someone-else' WHERE tenant_id='acme'",
    )
    .execute(&f.admin_pool)
    .await
    .unwrap_err();
    // Reader's assigned scope cannot resolve Alice's party, even with transaction write grants.
    sqlx::query("UPDATE archava_organizations.memberships SET scope='assigned' WHERE principal_id='assigned'").execute(&f.admin_pool).await.unwrap();
    let assigned = login(&app, "assigned").await;
    assert!(matches!(
        raw(&app, &assigned, "commerce", &input(sales(&p, &product, 2)))
            .await
            .0,
        StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
    ));
}
#[tokio::test]
async fn party_supplier_requires_explicit_refresh_before_new_offers_orders_or_reactivation() {
    let (_f, app, s, p, product) = fixture().await;
    let sup = submit(&app, &s, "purchasing", supplier(&p, 1)).await;
    let (offer, old) = purchase(&app, &s, &sup, &product).await;
    roles(&app, &s, &p, 1, true, false).await;
    let po = json!({"operation":"order.create","supplierId":sup["id"],"lines":[{"offerId":offer["id"],"quantity":1}]});
    assert_eq!(
        raw(&app, &s, "purchasing", &input(po.clone())).await.0,
        StatusCode::CONFLICT
    );
    roles(&app, &s, &p, 2, true, true).await;
    for cmd in [
        po.clone(),
        json!({"operation":"offer.create","supplierId":sup["id"],"productId":product["id"],"unitCost":6500}),
    ] {
        assert_eq!(
            raw(&app, &s, "purchasing", &input(cmd)).await.0,
            StatusCode::CONFLICT
        );
    }
    submit(&app,&s,"purchasing",json!({"operation":"supplier.set-active","id":sup["id"],"expectedRevision":1,"active":false,"reason":"Suspend locally"})).await;
    let activate = json!({"operation":"supplier.set-active","id":sup["id"],"expectedRevision":2,"active":true,"reason":"Resume locally"});
    assert_eq!(
        raw(&app, &s, "purchasing", &input(activate)).await.0,
        StatusCode::CONFLICT
    );
    let refresh = input(
        json!({"operation":"supplier.refresh-party","id":sup["id"],"expectedRevision":2,"expectedPartyRevision":3,"reason":"Reviewed current party"}),
    );
    let (st, b) = raw(&app, &s, "purchasing", &refresh).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    submit(&app,&s,"purchasing",json!({"operation":"supplier.set-active","id":sup["id"],"expectedRevision":3,"active":true,"reason":"Resume reviewed source"})).await;
    let newer = submit(&app, &s, "purchasing", po).await;
    let (_, w) = snapshot(&app, &s, "purchasing").await;
    assert_eq!(row(&w, "orders", &old["id"])["supplierPartyRevision"], 1);
    assert_eq!(row(&w, "orders", &newer["id"])["supplierPartyRevision"], 3);
    assert_eq!(
        raw(&app, &s, "purchasing", &refresh).await.0,
        StatusCode::OK
    );
}
#[tokio::test]
async fn party_transaction_rejects_stale_missing_forged_and_foreign_sources_without_writes() {
    let (f, app, s, p, product) = fixture().await;
    for (domain, cmd) in [
        ("commerce", sales(&p, &product, 2)),
        ("purchasing", supplier(&p, 2)),
    ] {
        assert_eq!(
            raw(&app, &s, domain, &input(cmd.clone())).await.0,
            StatusCode::CONFLICT
        );
        let mut missing = cmd.clone();
        missing["partyId"] = json!(Uuid::new_v4());
        assert_eq!(
            raw(&app, &s, domain, &input(missing)).await.0,
            StatusCode::NOT_FOUND
        );
        for key in [
            "customerName",
            "name",
            "ownerId",
            "supplierPartyRevision",
            "permissions",
        ] {
            let mut forged = cmd.clone();
            forged[key] = json!("forged");
            assert_eq!(
                raw(&app, &s, domain, &input(forged)).await.0,
                StatusCode::BAD_REQUEST
            );
        }
        let mut foreign = input(cmd);
        foreign["tenant"] = json!("foreign");
        assert_eq!(
            raw(&app, &s, domain, &foreign).await.0,
            StatusCode::FORBIDDEN
        );
    }
    let n:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM archava_commerce.orders)+(SELECT count(*) FROM archava_purchasing.suppliers)").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(n, 0);
}
#[tokio::test]
async fn published_party_read_is_sufficient_without_raw_crm_or_party_write_access() {
    let (f, app, s, p, product) = fixture().await;
    sqlx::query("UPDATE archava_organizations.memberships SET permissions=ARRAY['parties.read'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    submit(&app, &s, "commerce", sales(&p, &product, 1)).await;
    let sup = submit(&app, &s, "purchasing", supplier(&p, 1)).await;
    purchase(&app, &s, &sup, &product).await;
    for actor in ["reader", "service-seat"] {
        let peer = login(&app, actor).await;
        assert_eq!(
            raw(&app, &peer, "commerce", &input(sales(&p, &product, 1)))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            raw(&app, &peer, "purchasing", &input(supplier(&p, 1)))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
}
#[tokio::test]
async fn party_transaction_database_guards_reject_repointing_and_partial_version_pairs() {
    let (f, app, s, p, product) = fixture().await;
    let sale = submit(&app, &s, "commerce", sales(&p, &product, 1)).await;
    let sup = submit(&app, &s, "purchasing", supplier(&p, 1)).await;
    let (_, po) = purchase(&app, &s, &sup, &product).await;
    for (table, column, id) in [
        ("archava_commerce.orders", "customer", &sale["id"]),
        ("archava_purchasing.suppliers", "supplier", &sup["id"]),
        ("archava_purchasing.orders", "supplier", &po["id"]),
    ] {
        let id = Uuid::parse_str(id.as_str().unwrap()).unwrap();
        for value in ["NULL", "2"] {
            let sql=format!("UPDATE {table} SET {column}_party_revision={value},revision=revision+1 WHERE id=$1");
            let e = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                .bind(id)
                .execute(&f.admin_pool)
                .await
                .unwrap_err();
            assert!(
                matches!(
                    e.as_database_error().and_then(|e| e.code()).as_deref(),
                    Some("23514")
                ),
                "{e}"
            );
        }
    }
}
#[tokio::test]
async fn party_order_and_role_change_serialize_to_a_valid_published_version() {
    let (_f, app, s, p, product) = fixture().await;
    let sale = input(sales(&p, &product, 1));
    let role = json!({"tenant":"acme","mutation":{"requestId":Uuid::new_v4(),"type":"party.set_roles","data":{"id":p["partyId"],"expectedRevision":1,"customer":false,"supplier":true,"reason":"Stop customer role"}}});
    let (a, b) = tokio::join!(
        raw(&app, &s, "commerce", &sale),
        call(&app, "POST", "/erp/organizations/commands", role, Some(&s))
    );
    assert_eq!(b.0, StatusCode::OK, "{}", b.1);
    assert!(
        matches!(a.0, StatusCode::OK | StatusCode::CONFLICT),
        "{}",
        a.1
    );
    let (_, w) = snapshot(&app, &s, "commerce").await;
    for row in w["orders"].as_array().unwrap() {
        assert_eq!(row["customerPartyRevision"], 1);
        assert_eq!(row["customerPartyId"], p["partyId"]);
    }
}
#[tokio::test]
async fn party_sales_creation_uses_registered_tool_after_explicit_mission_approval() {
    let (_f, app, s, p, product) = fixture().await;
    let mut payload = sales(&p, &product, 1);
    payload.as_object_mut().unwrap().remove("operation");
    let body = json!({"tenant_id":"acme","mission_code":"party-sale","title":"Reviewed party sale","description":"Synthetic published master","auto_launch":true,"steps":[{"step_index":0,"step_name":"Create party sale","tool_name":"commerce.order.create_from_party","payload_input":payload,"depends_on":[]}]});
    let (st, m) = call(&app, "POST", "/api/v1/cockpit/missions", body, Some(&s)).await;
    assert_eq!(st, StatusCode::CREATED, "{m}");
    let base = format!("/api/v1/cockpit/missions/{}", m["id"].as_str().unwrap());
    let run = format!("{base}/run-next?tenant=acme");
    assert_eq!(
        call(&app, "POST", &run, json!({"stepIndex":0}), Some(&s))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{base}/steps/0/approve?tenant=acme"),
            json!({}),
            Some(&s)
        )
        .await
        .0,
        StatusCode::OK
    );
    let (st, b) = call(&app, "POST", &run, json!({"stepIndex":0}), Some(&s)).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert_eq!(b["status"], "touchdown");
    let (_, w) = snapshot(&app, &s, "commerce").await;
    assert_eq!(w["orders"][0]["customerPartyId"], p["partyId"]);
}

#[tokio::test]
async fn party_transaction_upgrade_preserves_legacy_documents_without_automatic_linking() {
    let f = archava_pg_fixture::provision_pre_party_transaction_profile().await;
    assert_eq!(f.organizations_ledger.len(), 3);
    assert_eq!(f.commerce_ledger.len(), 7);
    assert_eq!(f.purchasing_ledger.len(), 3);
    assert_eq!(f.billing_ledger.len(), 6);
    sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES('alice','alice@archava.test',$1,true,1)").bind(HASH).execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope,role,capability) VALUES('acme','alice','human',ARRAY['catalog.read','catalog.write','orders.read','orders.write'],'all','owner','enterprise')").execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_purchasing.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','alice','human',ARRAY['suppliers.read','suppliers.write'],'all')").execute(&f.admin_pool).await.unwrap();
    // This is the real prior schema. Current-host readiness must reject it;
    // seed the old entry paths through their tenant-bound production adapters.
    let additional = f.erp_readiness();
    let ready = AppState::try_new_all(
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
    .await;
    match ready {
        Err(error) => {
            let message = format!("{error:?}");
            assert!(
                message.contains("UnsatisfiedPolicyGate")
                    && (message.contains("organizations") || message.contains("purchasing")),
                "{message}"
            );
        }
        Ok(_) => panic!("current host must refuse the pre-company schema"),
    }
    let principal = archava_domain::erp::purchasing::PurchasePrincipal::human("alice");
    let mut tx = archava_adapters::rls::TenantTransaction::begin(
        &f.runtime_pool,
        "archava_commerce",
        "Commerce",
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
    let command=serde_json::from_value(json!({"operation":"product.create","sku":"LEGACY","title":"Legacy product","unit":"unit","trackStock":false,"price":1000})).unwrap();
    let product = archava_adapters::erp::commerce::record(
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
    let command=serde_json::from_value(json!({"operation":"order.create","customerName":"Unlinked buyer","lines":[{"productId":product.id,"quantity":1}]})).unwrap();
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
    let mut tx = archava_adapters::rls::TenantTransaction::begin(
        &f.runtime_pool,
        "archava_purchasing",
        "purchasing",
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
    let mutation=archava_domain::erp::purchasing::PurchaseMutation::parse(&json!({"requestId":Uuid::new_v4(),"command":{"operation":"supplier.create","code":"LEGACY","name":"Unlinked supplier"}})).unwrap();
    archava_adapters::erp::purchasing::record_purchase_command(
        tx.checked_executor().await.unwrap(),
        "acme",
        &principal,
        &context,
        &"c".repeat(64),
        &mutation,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    archava_pg_fixture::apply_pending_party_transaction_migrations(&f.owner_pool).await;
    archava_pg_fixture::apply_pending_party_transaction_migrations(&f.owner_pool).await;
    let preserved:bool=sqlx::query_scalar("SELECT (SELECT customer_name='Unlinked buyer' AND customer_party_id IS NULL AND customer_party_revision IS NULL AND customer_contact_id IS NULL FROM archava_commerce.orders WHERE tenant_id='acme') AND (SELECT name='Unlinked supplier' AND supplier_party_id IS NULL AND supplier_party_revision IS NULL AND supplier_contact_id IS NULL FROM archava_purchasing.suppliers WHERE tenant_id='acme')").fetch_one(&f.admin_pool).await.unwrap();
    assert!(preserved);
}

#[tokio::test]
async fn party_supplier_refresh_and_new_purchase_serialize_without_mixed_versions() {
    let (_f, app, s, p, product) = fixture().await;
    let sup = submit(&app, &s, "purchasing", supplier(&p, 1)).await;
    let (offer, _) = purchase(&app, &s, &sup, &product).await;
    roles(&app, &s, &p, 1, false, true).await;
    let refresh = input(
        json!({"operation":"supplier.refresh-party","id":sup["id"],"expectedRevision":1,"expectedPartyRevision":2,"reason":"Reviewed current master"}),
    );
    let purchase = input(
        json!({"operation":"order.create","supplierId":sup["id"],"lines":[{"offerId":offer["id"],"quantity":1}]}),
    );
    let (a, b) = tokio::join!(
        raw(&app, &s, "purchasing", &refresh),
        raw(&app, &s, "purchasing", &purchase)
    );
    assert_eq!(a.0, StatusCode::OK, "{}", a.1);
    assert!(
        matches!(b.0, StatusCode::OK | StatusCode::CONFLICT),
        "{}",
        b.1
    );
    if b.0 == StatusCode::OK {
        let (_, w) = snapshot(&app, &s, "purchasing").await;
        let id = &b.1.get("receipt").unwrap_or(&b.1)["id"];
        assert_eq!(row(&w, "orders", id)["supplierPartyRevision"], 2);
    }
}
