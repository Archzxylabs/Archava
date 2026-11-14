use super::*;

async fn setup_suppliers() -> (Fixture, Router, Value) {
    let (f, app, session) = setup().await;
    for actor in ["alice", "reader", "assigned", "service-seat"] {
        sqlx::query("INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme',$1,$2,ARRAY['contacts.read','contacts.write'], 'all')")
            .bind(actor).bind(if actor=="service-seat"{"service"}else{"human"}).execute(&f.admin_pool).await.unwrap();
    }
    (f, app, session)
}
async fn contact(app: &Router, session: &Value, name: &str) -> Value {
    let (status, body)=call(app,"POST","/crm/commands",json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"contact.create","data":{"fullName":name,"email":"supplier@example.test"}}}),Some(session)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["receipt"].clone()
}
async fn rename(app: &Router, session: &Value, source: &Value, revision: i64, name: &str) {
    let (status, body)=call(app,"POST","/crm/commands",json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"contact.update","id":source["id"],"expectedRevision":revision,"data":{"fullName":name}}}),Some(session)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
fn creation(source: &Value, code: &str) -> Value {
    json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"supplier.create-from-contact","code":code,"contactId":source["id"],"expectedContactRevision":source["revision"]}})
}
fn refresh(supplier: &Value, revision: i64, source_revision: i64) -> Value {
    json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"supplier.refresh-contact","id":supplier["id"],"expectedRevision":revision,"expectedContactRevision":source_revision,"reason":"Reviewed supplier profile change"}})
}
async fn send(app: &Router, session: &Value, input: &Value) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/erp/purchasing/commands",
        input.clone(),
        Some(session),
    )
    .await
}
async fn create(app: &Router, session: &Value, source: &Value, code: &str) -> Value {
    let (status, body) = send(app, session, &creation(source, code)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body.get("receipt").unwrap_or(&body).clone()
}
async fn offer(app: &Router, session: &Value, supplier: &Value) -> (Value, Value) {
    let product=submit(app,session,"commerce",json!({"operation":"product.create","sku":"SUPPLIER-IDENTITY","title":"Supplier identity item","unit":"unit","price":15000,"trackStock":true})).await;
    let offer=submit(app,session,"purchasing",json!({"operation":"offer.create","supplierId":supplier["id"],"productId":product["id"],"unitCost":6000})).await;
    (product, offer)
}
async fn order(app: &Router, session: &Value, supplier: &Value, offer: &Value) -> Value {
    submit(app,session,"purchasing",json!({"operation":"order.create","supplierId":supplier["id"],"lines":[{"offerId":offer["id"],"quantity":5}]})).await
}

#[tokio::test]
async fn supplier_identity_refresh_preserves_purchase_receipt_history_and_shared_customer_role() {
    let (f, app, session) = setup_suppliers().await;
    let other = contact(&app, &session, "Same supplier").await;
    let source = contact(&app, &session, "Same supplier").await;
    let input = creation(&source, "LINKED");
    let (status, answer) = send(&app, &session, &input).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let supplier = answer.get("receipt").unwrap_or(&answer).clone();
    let (product, offer) = offer(&app, &session, &supplier).await;
    let old = order(&app, &session, &supplier, &offer).await;
    let customer_order=submit(&app,&session,"commerce",json!({"operation":"order.create_from_contact","contactId":source["id"],"expectedContactRevision":1,"lines":[{"productId":product["id"],"quantity":1}]})).await;
    rename(&app, &session, &source, 1, "Reviewed supplier").await;
    let update = refresh(&supplier, 1, 2);
    assert_eq!(send(&app, &session, &update).await.0, StatusCode::OK);
    rename(&app, &session, &source, 2, "Later CRM name").await;
    for command in [&input, &update] {
        assert_eq!(send(&app, &session, command).await.0, StatusCode::OK);
    }
    let new = order(&app, &session, &supplier, &offer).await;
    submit(&app,&session,"purchasing",json!({"operation":"order.confirm","id":old["id"],"expectedRevision":1,"reason":"Receive original purchase"})).await;
    let (_, before) = snapshot(&app, &session, "purchasing").await;
    let old_row = before["orders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == old["id"])
        .unwrap();
    submit(&app,&session,"purchasing",json!({"operation":"order.receive","id":old["id"],"expectedRevision":2,"reason":"Delivery checked","lines":[{"lineId":old_row["lines"][0]["id"],"accepted":4,"rejected":1}]})).await;
    let (_, after) = snapshot(&app, &session, "purchasing").await;
    let current = &after["suppliers"][0];
    assert_eq!(current["name"], "Reviewed supplier");
    assert_eq!(current["supplierContactRevision"], 2);
    assert_eq!(current["supplierContactId"], source["id"]);
    assert_ne!(current["supplierContactId"], other["id"]);
    for (id, name, revision, state) in [
        (&old["id"], "Same supplier", 1, "received"),
        (&new["id"], "Reviewed supplier", 2, "draft"),
    ] {
        let row = after["orders"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| &row["id"] == id)
            .unwrap();
        assert_eq!(row["supplierName"], name);
        assert_eq!(row["supplierContactRevision"], revision);
        assert_eq!(row["supplierContactId"], source["id"]);
        assert_eq!(row["state"], state);
    }
    let (_, commerce) = snapshot(&app, &session, "commerce").await;
    let customer = commerce["orders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == customer_order["id"])
        .unwrap();
    assert_eq!(customer["customerContactId"], source["id"]);
    assert_eq!(customer["customerContactRevision"], 1);
    let stock:i64=sqlx::query_scalar("SELECT quantity::bigint FROM archava_commerce.stock_balances WHERE tenant_id='acme' AND product_id=$1").bind(Uuid::parse_str(product["id"].as_str().unwrap()).unwrap()).fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(stock, 4);
    let received:i64=sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.receipts r JOIN archava_purchasing.orders o ON o.tenant_id=r.tenant_id AND o.id=r.order_id WHERE o.supplier_name='Same supplier' AND o.supplier_contact_revision=1").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(received, 1);
}

#[tokio::test]
async fn supplier_identity_requires_current_crm_read_on_new_commands_and_replay() {
    let (f, app, session) = setup_suppliers().await;
    let source = contact(&app, &session, "Supplier").await;
    let input = creation(&source, "AUTH");
    sqlx::query("UPDATE archava_crm.memberships SET permissions=ARRAY['contacts.read'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    let (status, _) = send(&app, &session, &input).await;
    assert_eq!(status, StatusCode::OK);
    sqlx::query("UPDATE archava_crm.memberships SET permissions=ARRAY['contacts.write'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    assert_eq!(send(&app, &session, &input).await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        send(&app, &session, &creation(&source, "DENIED")).await.0,
        StatusCode::FORBIDDEN
    );
    // Status reveals this actor's committed receipt, not a fresh CRM profile read.
    let path = format!(
        "/erp/purchasing/commands/status?tenant=acme&requestId={}",
        input["requestId"].as_str().unwrap()
    );
    assert_eq!(
        call(&app, "GET", &path, json!({}), Some(&session)).await.0,
        StatusCode::OK
    );
    for actor in ["reader", "assigned", "service-seat"] {
        let peer = login(&app, actor).await;
        assert_eq!(
            send(&app, &peer, &creation(&source, &actor.to_uppercase()))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    sqlx::query("UPDATE archava_crm.memberships SET permissions=ARRAY['contacts.read'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    sqlx::query("UPDATE archava_purchasing.memberships SET permissions=ARRAY['suppliers.read'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    assert_eq!(send(&app, &session, &input).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn supplier_identity_refuses_stale_missing_forged_and_unlinked_sources_atomically() {
    let (f, app, session) = setup_suppliers().await;
    let source = contact(&app, &session, "Before").await;
    rename(&app, &session, &source, 1, "After").await;
    let input = creation(&source, "STALE");
    assert_eq!(send(&app, &session, &input).await.0, StatusCode::CONFLICT);
    let mut missing = input.clone();
    missing["command"]["contactId"] = json!(Uuid::new_v4());
    assert_eq!(
        send(&app, &session, &missing).await.0,
        StatusCode::NOT_FOUND
    );
    for key in ["name", "ownerId", "role"] {
        let mut forged = input.clone();
        forged["command"][key] = json!("forged");
        assert_eq!(
            send(&app, &session, &forged).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    let linked = create(
        &app,
        &session,
        &json!({"id":source["id"],"revision":2}),
        "CURRENT",
    )
    .await;
    assert_eq!(
        send(&app, &session, &refresh(&linked, 1, 2)).await.0,
        StatusCode::CONFLICT
    );
    rename(&app, &session, &source, 2, "Third").await;
    assert_eq!(
        send(&app, &session, &refresh(&linked, 1, 2)).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(&app, &session, &refresh(&linked, 2, 3)).await.0,
        StatusCode::CONFLICT
    );
    let manual = submit(
        &app,
        &session,
        "purchasing",
        json!({"operation":"supplier.create","code":"MANUAL","name":"Third"}),
    )
    .await;
    assert_eq!(
        send(&app, &session, &refresh(&manual, 1, 3)).await.0,
        StatusCode::CONFLICT
    );
    let commands: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.commands")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(commands, 2);
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.audit")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(audits, 2);
}

#[tokio::test]
async fn supplier_identity_enforces_tenant_and_assigned_contact_scope() {
    let (f, app, session) = setup_suppliers().await;
    let source = contact(&app, &session, "Hidden owner").await;
    sqlx::query("UPDATE archava_crm.memberships SET scope='assigned' WHERE principal_id='alice'")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    // Synthetic opening record owned by a different real member.
    let foreign = Uuid::new_v4();
    sqlx::query("INSERT INTO archava_crm.contacts(tenant_id,id,full_name,owner_id) VALUES('acme',$1,'Peer supplier','assigned')").bind(foreign).execute(&f.admin_pool).await.unwrap();
    assert_eq!(
        send(
            &app,
            &session,
            &creation(&json!({"id":foreign,"revision":1}), "HIDDEN")
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(&app, &session, &creation(&source, "OWN")).await.0,
        StatusCode::OK
    );
    let another = Uuid::new_v4();
    sqlx::query("INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('other','alice','human',ARRAY['contacts.read'],'all')").execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_crm.contacts(tenant_id,id,full_name,owner_id) VALUES('other',$1,'Cross tenant','alice')").bind(another).execute(&f.admin_pool).await.unwrap();
    assert_eq!(
        send(
            &app,
            &session,
            &creation(&json!({"id":another,"revision":1}), "CROSS")
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn supplier_identity_database_guards_reject_repointing_and_false_snapshots() {
    let (f, app, session) = setup_suppliers().await;
    let source = contact(&app, &session, "Original").await;
    let other = contact(&app, &session, "Other").await;
    let supplier = create(&app, &session, &source, "GUARD").await;
    let (_, offer) = offer(&app, &session, &supplier).await;
    let order = order(&app, &session, &supplier, &offer).await;
    let supplier_id = Uuid::parse_str(supplier["id"].as_str().unwrap()).unwrap();
    let order_id = Uuid::parse_str(order["id"].as_str().unwrap()).unwrap();
    for query in ["UPDATE archava_purchasing.suppliers SET name='Forged',revision=revision+1 WHERE id=$1","UPDATE archava_purchasing.suppliers SET supplier_contact_revision=NULL,revision=revision+1 WHERE id=$1"] {
        assert!(sqlx::query(query).bind(supplier_id).execute(&f.admin_pool).await.is_err());
    }
    assert!(sqlx::query("UPDATE archava_purchasing.suppliers SET supplier_contact_id=$2,revision=revision+1 WHERE id=$1").bind(supplier_id).bind(Uuid::parse_str(other["id"].as_str().unwrap()).unwrap()).execute(&f.admin_pool).await.is_err());
    assert!(sqlx::query("UPDATE archava_purchasing.orders SET supplier_contact_revision=2,revision=revision+1 WHERE id=$1").bind(order_id).execute(&f.admin_pool).await.is_err());
    let mut tx = f.admin_pool.begin().await.unwrap();
    let bad=sqlx::query("INSERT INTO archava_purchasing.suppliers(tenant_id,id,code,name,supplier_contact_id,supplier_contact_revision) VALUES('acme',$1,'FORGED','Wrong',$2,1)").bind(Uuid::new_v4()).bind(Uuid::parse_str(source["id"].as_str().unwrap()).unwrap()).execute(&mut *tx).await;
    assert!(bad.is_err());
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn supplier_identity_supports_unicode_without_guessing_links_for_manual_names() {
    let (_f, app, session) = setup_suppliers().await;
    let name = "供".repeat(160);
    let source = contact(&app, &session, &name).await;
    let supplier = create(&app, &session, &source, "UNICODE").await;
    let (_, offer) = offer(&app, &session, &supplier).await;
    let old = order(&app, &session, &supplier, &offer).await;
    submit(&app,&session,"purchasing",json!({"operation":"order.confirm","id":old["id"],"expectedRevision":1,"reason":"Unicode snapshot"})).await;
    submit(
        &app,
        &session,
        "purchasing",
        json!({"operation":"supplier.create","code":"MANUAL","name":name}),
    )
    .await;
    let (_, rows) = snapshot(&app, &session, "purchasing").await;
    assert_eq!(
        rows["suppliers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["supplierContactId"].is_null())
            .count(),
        1
    );
    assert!(rows["suppliers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["name"] == name));
}

#[tokio::test]
async fn supplier_identity_refresh_and_purchase_creation_are_concurrent_and_coherent() {
    let (_f, app, session) = setup_suppliers().await;
    let source = contact(&app, &session, "Before race").await;
    let supplier = create(&app, &session, &source, "RACE").await;
    let (_, offer) = offer(&app, &session, &supplier).await;
    rename(&app, &session, &source, 1, "After race").await;
    let update = refresh(&supplier, 1, 2);
    let (updated, purchase) = tokio::join!(
        send(&app, &session, &update),
        order(&app, &session, &supplier, &offer)
    );
    assert_eq!(updated.0, StatusCode::OK, "{updated:?}");
    let (_, rows) = snapshot(&app, &session, "purchasing").await;
    let row = rows["orders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == purchase["id"])
        .unwrap();
    assert!(
        row["supplierName"] == "Before race" && row["supplierContactRevision"] == 1
            || row["supplierName"] == "After race" && row["supplierContactRevision"] == 2,
        "{row}"
    );
}

#[tokio::test]
async fn supplier_identity_duplicate_code_race_refuses_once_and_contact_can_have_multiple_codes() {
    let (_f, app, session) = setup_suppliers().await;
    let source = contact(&app, &session, "Supplier").await;
    let first = creation(&source, "SAME");
    let second = creation(&source, "SAME");
    let (a, b) = tokio::join!(send(&app, &session, &first), send(&app, &session, &second));
    assert!(
        a.0 == StatusCode::OK && b.0 == StatusCode::CONFLICT
            || b.0 == StatusCode::OK && a.0 == StatusCode::CONFLICT,
        "{a:?} {b:?}"
    );
    create(&app, &session, &source, "ANOTHER").await;
    let (_, rows) = snapshot(&app, &session, "purchasing").await;
    assert_eq!(rows["suppliers"].as_array().unwrap().len(), 2);
    assert!(rows["suppliers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["supplierContactId"] == source["id"]));
}

#[tokio::test]
async fn supplier_identity_mission_commands_require_approval_and_replay_once() {
    let (_f, app, session) = setup_suppliers().await;
    let source = contact(&app, &session, "Mission supplier").await;
    for (code, operation, payload) in [
        (
            "supplier-create",
            "purchasing.supplier.create-from-contact",
            json!({"code":"MISSION","contactId":source["id"],"expectedContactRevision":1}),
        ),
        (
            "supplier-refresh",
            "purchasing.supplier.refresh-contact",
            json!({}),
        ),
    ] {
        let payload = if code == "supplier-refresh" {
            rename(&app, &session, &source, 1, "Reviewed mission supplier").await;
            let (_, rows) = snapshot(&app, &session, "purchasing").await;
            refresh(&rows["suppliers"][0], 1, 2)["command"]
                .as_object()
                .unwrap()
                .iter()
                .filter(|(key, _)| key.as_str() != "operation")
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<serde_json::Map<_, _>>()
                .into()
        } else {
            payload
        };
        let (status,mission)=call(&app,"POST","/api/v1/cockpit/missions",json!({"tenant_id":"acme","mission_code":code,"title":"Reviewed supplier change","description":"Explicit CRM identity","auto_launch":true,"steps":[{"step_index":0,"step_name":"Apply reviewed change","tool_name":operation,"payload_input":payload,"depends_on":[]}]}),Some(&session)).await;
        assert_eq!(status, StatusCode::CREATED, "{mission}");
        let base = format!(
            "/api/v1/cockpit/missions/{}",
            mission["id"].as_str().unwrap()
        );
        let path = format!("{base}/run-next?tenant=acme");
        assert_eq!(
            call(&app, "POST", &path, json!({"stepIndex":0}), Some(&session))
                .await
                .0,
            StatusCode::CONFLICT
        );
        let (_, before) = snapshot(&app, &session, "purchasing").await;
        if code == "supplier-create" {
            assert_eq!(before["suppliers"], json!([]));
        } else {
            assert_eq!(before["suppliers"][0]["revision"], 1);
        }
        assert_eq!(
            call(
                &app,
                "POST",
                &format!("{base}/steps/0/approve?tenant=acme"),
                json!({}),
                Some(&session)
            )
            .await
            .0,
            StatusCode::OK
        );
        let (status, result) =
            call(&app, "POST", &path, json!({"stepIndex":0}), Some(&session)).await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(result["status"], "touchdown");
        assert_eq!(result["steps"][0]["payload_output"]["verified"], true);
        assert_eq!(
            call(&app, "POST", &path, json!({"stepIndex":0}), Some(&session))
                .await
                .1,
            result
        );
    }
    let (_, rows) = snapshot(&app, &session, "purchasing").await;
    assert_eq!(rows["suppliers"].as_array().unwrap().len(), 1);
    assert_eq!(rows["suppliers"][0]["revision"], 2);
    assert_eq!(rows["suppliers"][0]["name"], "Reviewed mission supplier");
}
