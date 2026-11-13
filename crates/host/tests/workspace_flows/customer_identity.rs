use super::*;

async fn setup_customers() -> (Fixture, Router, Value) {
    let (fixture, app, session) = setup().await;
    for actor in ["alice", "reader", "assigned", "service-seat"] {
        sqlx::query("INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme',$1,$2,$3,$4)")
            .bind(actor).bind(if actor=="service-seat"{"service"}else{"human"})
            .bind(if actor=="reader" {vec!["contacts.read"]}else{vec!["contacts.read","contacts.write"]})
            .bind(if actor=="assigned"{"assigned"}else{"all"}).execute(&fixture.admin_pool).await.unwrap();
    }
    (fixture, app, session)
}
async fn contact(app: &Router, session: &Value, tenant: &str, name: &str, email: &str) -> Value {
    let (status,body)=call(app,"POST","/crm/commands",json!({"tenant":tenant,"requestId":Uuid::new_v4(),"command":{"operation":"contact.create","data":{"fullName":name,"email":email}}}),Some(session)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["receipt"].clone()
}
async fn product(app: &Router, session: &Value) -> Value {
    submit(app,session,"commerce",json!({"operation":"product.create","sku":"CUSTOMER-SOURCE","title":"Customer-linked item","unit":"unit","trackStock":false,"price":12500})).await
}
fn body(contact: &Value, product: &Value) -> Value {
    json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"order.create_from_contact","contactId":contact["id"],"expectedContactRevision":contact["revision"],"lines":[{"productId":product["id"],"quantity":2}]}})
}
async fn create(app: &Router, session: &Value, body: &Value) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/erp/commerce/commands",
        body.clone(),
        Some(session),
    )
    .await
}
async fn rename(app: &Router, session: &Value, contact: &Value, name: &str) -> (StatusCode, Value) {
    call(app,"POST","/crm/commands",json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"contact.update","id":contact["id"],"expectedRevision":contact["revision"],"data":{"fullName":name}}}),Some(session)).await
}
async fn fulfill(app: &Router, session: &Value, order: &Value) {
    submit(app,session,"commerce",json!({"operation":"order.reserve","id":order["id"],"expectedRevision":1,"holdSeconds":300})).await;
    submit(
        app,
        session,
        "commerce",
        json!({"operation":"order.fulfill","id":order["id"],"expectedRevision":2}),
    )
    .await;
}

#[tokio::test]
async fn customer_identity_survives_same_names_renaming_fulfillment_invoice_and_replay() {
    let (f, app, session) = setup_customers().await;
    let first = contact(
        &app,
        &session,
        "acme",
        "Same customer",
        "first@example.test",
    )
    .await;
    let second = contact(
        &app,
        &session,
        "acme",
        "Same customer",
        "second@example.test",
    )
    .await;
    let product = product(&app, &session).await;
    let input = body(&second, &product);
    let (status, answer) = create(&app, &session, &input).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let order = &answer["receipt"];
    let (status, renamed) = rename(&app, &session, &second, "Renamed customer").await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    let (status, replayed) = create(&app, &session, &input).await;
    assert_eq!(status, StatusCode::OK, "{replayed}");
    assert_eq!(replayed["receipt"], *order);
    // Invoice conversion uses authorized order history, not current CRM profile access.
    sqlx::query("UPDATE archava_crm.memberships SET permissions=ARRAY['contacts.write']::text[] WHERE tenant_id='acme' AND principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    fulfill(&app, &session, order).await;
    let source = json!({"id":order["id"],"revision":3});
    let (status, invoice) = order_invoice_call(
        &app,
        &session,
        &order_invoice_command(&source, "INV-CUSTOMER"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{invoice}");
    let (_, orders) = snapshot(&app, &session, "commerce").await;
    assert_eq!(orders["orders"].as_array().unwrap().len(), 1);
    assert_eq!(orders["orders"][0]["customerContactId"], second["id"]);
    assert_ne!(orders["orders"][0]["customerContactId"], first["id"]);
    let (_, invoices) = snapshot(&app, &session, "billing").await;
    assert_eq!(invoices["invoices"][0]["customerContactId"], second["id"]);
    assert_eq!(invoices["invoices"][0]["customerContactRevision"], 1);
    assert_eq!(invoices["invoices"][0]["customer"], "Same customer");
    let mut tx = f.runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    let stored:Value=sqlx::query_scalar("SELECT json_build_object('contact',c.id,'currentName',c.full_name,'currentRevision',c.revision,'orderName',o.customer_name,'orderContactRevision',o.customer_contact_revision,'invoiceName',i.customer_ref,'invoiceContactRevision',i.customer_contact_revision,'sameIdentity',o.customer_contact_id=i.customer_contact_id) FROM archava_commerce.orders o JOIN archava_billing.invoices i ON i.tenant_id=o.tenant_id AND i.source_order_id=o.id JOIN archava_crm.contacts c ON c.tenant_id=o.tenant_id AND c.id=o.customer_contact_id WHERE o.tenant_id='acme' AND o.id=$1")
        .bind(Uuid::parse_str(order["id"].as_str().unwrap()).unwrap()).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        stored,
        json!({"contact":second["id"],"currentName":"Renamed customer","currentRevision":2,"orderName":"Same customer","orderContactRevision":1,"invoiceName":"Same customer","invoiceContactRevision":1,"sameIdentity":true})
    );
}

#[tokio::test]
async fn customer_identity_refuses_stale_missing_and_forged_sources_atomically() {
    let (f, app, session) = setup_customers().await;
    let customer = contact(
        &app,
        &session,
        "acme",
        "Original customer",
        "stale@example.test",
    )
    .await;
    let product = product(&app, &session).await;
    assert_eq!(
        rename(&app, &session, &customer, "New name").await.0,
        StatusCode::OK
    );
    let mut input = body(&customer, &product);
    assert_eq!(create(&app, &session, &input).await.0, StatusCode::CONFLICT);
    input["command"]["contactId"] = json!(Uuid::new_v4());
    assert_eq!(
        create(&app, &session, &input).await.0,
        StatusCode::NOT_FOUND
    );
    input = body(&customer, &product);
    input["command"]["customerName"] = json!("Forged");
    assert_eq!(
        create(&app, &session, &input).await.0,
        StatusCode::BAD_REQUEST
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_commerce.orders")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let claims: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_commerce.commands WHERE request_id=$1")
            .bind(Uuid::parse_str(input["requestId"].as_str().unwrap()).unwrap())
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(claims, 0);
}

#[tokio::test]
async fn customer_identity_rechecks_contact_read_and_scope_on_creation_and_replay() {
    let (f, app, session) = setup_customers().await;
    let customer = contact(
        &app,
        &session,
        "acme",
        "Scoped customer",
        "scope@example.test",
    )
    .await;
    let product = product(&app, &session).await;
    let input = body(&customer, &product);
    let assigned = login(&app, "assigned").await;
    assert_eq!(
        create(&app, &assigned, &input).await.0,
        StatusCode::NOT_FOUND
    );
    let own = contact(
        &app,
        &assigned,
        "acme",
        "Assigned customer",
        "assigned@example.test",
    )
    .await;
    assert_eq!(
        create(&app, &assigned, &body(&own, &product)).await.0,
        StatusCode::OK
    );
    sqlx::query("UPDATE archava_crm.memberships SET permissions=ARRAY['contacts.read']::text[] WHERE tenant_id='acme' AND principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    let (status, saved) = create(&app, &session, &input).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    sqlx::query("UPDATE archava_crm.memberships SET permissions=ARRAY['contacts.write']::text[] WHERE tenant_id='acme' AND principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    assert_eq!(
        create(&app, &session, &input).await.0,
        StatusCode::FORBIDDEN
    );
    let (status, status_doc) = call(
        &app,
        "GET",
        &format!(
            "/erp/commerce/commands/status?tenant=acme&requestId={}",
            input["requestId"].as_str().unwrap()
        ),
        Value::Null,
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{status_doc}");
    assert_eq!(status_doc["receipt"]["id"], saved["receipt"]["id"]);
    let reader = login(&app, "reader").await;
    assert_eq!(
        create(&app, &reader, &body(&customer, &product)).await.0,
        StatusCode::FORBIDDEN
    );
    let service = login(&app, "service-seat").await;
    assert_eq!(
        create(&app, &service, &body(&customer, &product)).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn customer_identity_never_resolves_another_tenants_contact() {
    let (f, app, session) = setup_customers().await;
    sqlx::query("INSERT INTO archava_crm.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('other','alice','human',ARRAY['contacts.read','contacts.write'],'all')").execute(&f.admin_pool).await.unwrap();
    let other = contact(&app, &session, "other", "Other buyer", "other@example.test").await;
    let product = product(&app, &session).await;
    assert_eq!(
        create(&app, &session, &body(&other, &product)).await.0,
        StatusCode::NOT_FOUND
    );
    let (_, orders) = snapshot(&app, &session, "commerce").await;
    assert_eq!(orders["orders"], json!([]));
}

#[tokio::test]
async fn customer_identity_database_prevents_repointing_and_false_snapshots() {
    let (f, app, session) = setup_customers().await;
    let customer = contact(
        &app,
        &session,
        "acme",
        "Guard customer",
        "guard@example.test",
    )
    .await;
    let other = contact(
        &app,
        &session,
        "acme",
        "Other customer",
        "guard2@example.test",
    )
    .await;
    let product = product(&app, &session).await;
    let (status, saved) = create(&app, &session, &body(&customer, &product)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let order = &saved["receipt"];
    let mut tx = f.runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    let changed=sqlx::query("UPDATE archava_commerce.orders SET customer_contact_id=$1,revision=revision+1 WHERE tenant_id='acme' AND id=$2")
        .bind(Uuid::parse_str(other["id"].as_str().unwrap()).unwrap()).bind(Uuid::parse_str(order["id"].as_str().unwrap()).unwrap()).execute(&mut *tx).await;
    assert!(changed.is_err());
    tx.rollback().await.unwrap();
    let mut tx = f.runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    let forged=sqlx::query("INSERT INTO archava_commerce.orders(tenant_id,id,owner_id,customer_name,total_estimate,customer_contact_id,customer_contact_revision) VALUES('acme',$1,'alice','Forged name',0,$2,1)")
        .bind(Uuid::new_v4()).bind(Uuid::parse_str(customer["id"].as_str().unwrap()).unwrap()).execute(&mut *tx).await;
    assert!(forged.is_err());
    tx.rollback().await.unwrap();
    fulfill(&app, &session, order).await;
    let mut tx = f.runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    let forged=sqlx::query("INSERT INTO archava_billing.invoices(tenant_id,id,invoice_code,customer_ref,currency,gross_minor,outstanding_minor,consumer_ref,owner_id,source_order_id,source_order_revision,customer_contact_id,customer_contact_revision) VALUES('acme',$1,'INV-FALSE-CUSTOMER','Guard customer','IDR',25000,25000,$2,'alice',$3,3,$4,1)")
        .bind(Uuid::new_v4()).bind(format!("commerce:{}",order["id"].as_str().unwrap())).bind(Uuid::parse_str(order["id"].as_str().unwrap()).unwrap()).bind(Uuid::parse_str(other["id"].as_str().unwrap()).unwrap()).execute(&mut *tx).await;
    assert!(forged.is_err());
    tx.rollback().await.unwrap();
    let source = json!({"id":order["id"],"revision":3});
    let (status, invoice) = order_invoice_call(
        &app,
        &session,
        &order_invoice_command(&source, "INV-TRUE-CUSTOMER"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{invoice}");
    let mut tx = f.runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    let changed=sqlx::query("UPDATE archava_billing.invoices SET customer_contact_id=NULL,customer_contact_revision=NULL WHERE tenant_id='acme' AND id=$1")
        .bind(Uuid::parse_str(invoice["invoiceId"].as_str().unwrap()).unwrap()).execute(&mut *tx).await;
    assert!(changed.is_err());
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn customer_identity_retains_unicode_names_and_does_not_backfill_manual_customers() {
    let (_f, app, session) = setup_customers().await;
    let name = "客".repeat(160);
    let customer = contact(&app, &session, "acme", &name, "unicode@example.test").await;
    let product = product(&app, &session).await;
    let (status, saved) = create(&app, &session, &body(&customer, &product)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    submit(&app,&session,"commerce",json!({"operation":"order.create","customerName":name,"lines":[{"productId":product["id"],"quantity":1}]})).await;
    let (_, orders) = snapshot(&app, &session, "commerce").await;
    let rows = orders["orders"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows.iter()
            .filter(|row| row["customerContactId"] == customer["id"])
            .count(),
        1
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row["customerContactId"].is_null())
            .count(),
        1
    );
    assert!(rows.iter().all(|row| row["customerName"] == name));
}

#[tokio::test]
async fn customer_identity_concurrent_rename_preserves_the_reviewed_snapshot_or_refuses() {
    let (_f, app, session) = setup_customers().await;
    let customer = contact(&app, &session, "acme", "Before race", "race@example.test").await;
    let product = product(&app, &session).await;
    let input = body(&customer, &product);
    let (creation, update) = tokio::join!(
        create(&app, &session, &input),
        rename(&app, &session, &customer, "After race")
    );
    assert_eq!(update.0, StatusCode::OK, "{update:?}");
    let (_, orders) = snapshot(&app, &session, "commerce").await;
    match creation.0 {
        StatusCode::OK => {
            assert_eq!(orders["orders"][0]["customerName"], "Before race");
            assert_eq!(orders["orders"][0]["customerContactRevision"], 1);
        }
        StatusCode::CONFLICT => assert_eq!(orders["orders"], json!([])),
        _ => panic!("Unexpected race result: {creation:?}"),
    }
}

#[tokio::test]
async fn customer_identity_registered_mission_requires_approval_and_uses_the_selected_record() {
    let (_f, app, session) = setup_customers().await;
    let customer = contact(
        &app,
        &session,
        "acme",
        "Mission customer",
        "mission@example.test",
    )
    .await;
    let product = product(&app, &session).await;
    let input = json!({"tenant_id":"acme","mission_code":"customer-order","title":"Order for existing customer","description":"Reviewed customer identity","auto_launch":true,"steps":[{"step_index":0,"step_name":"Create linked order","tool_name":"commerce.order.create_from_contact","payload_input":{"contactId":customer["id"],"expectedContactRevision":1,"lines":[{"productId":product["id"],"quantity":2}]},"depends_on":[]}]});
    let (status, mission) = call(
        &app,
        "POST",
        "/api/v1/cockpit/missions",
        input,
        Some(&session),
    )
    .await;
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
    let (_, before) = snapshot(&app, &session, "commerce").await;
    assert_eq!(before["orders"], json!([]));
    let (status, approved) = call(
        &app,
        "POST",
        &format!("{base}/steps/0/approve?tenant=acme"),
        json!({}),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    let (status, result) = call(&app, "POST", &path, json!({"stepIndex":0}), Some(&session)).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["status"], "touchdown");
    assert_eq!(result["steps"][0]["payload_output"]["verified"], true);
    let (status, replayed) =
        call(&app, "POST", &path, json!({"stepIndex":0}), Some(&session)).await;
    assert_eq!(status, StatusCode::OK, "{replayed}");
    assert_eq!(result, replayed);
    let (_, after) = snapshot(&app, &session, "commerce").await;
    assert_eq!(after["orders"].as_array().unwrap().len(), 1);
    assert_eq!(after["orders"][0]["customerContactId"], customer["id"]);
}
