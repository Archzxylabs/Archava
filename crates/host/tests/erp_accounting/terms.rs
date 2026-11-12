//! Real immutable source identity, explicit due conditions, CAS and close authority.
use super::*;
use invoice_sources::SourceFixture;

struct Scenario {
    s: SourceFixture,
    source: Value,
    date: String,
    due: String,
}
async fn collection(
    app: &Router,
    session: &Value,
    company: &Value,
    name: &str,
    id: Option<&str>,
) -> (StatusCode, Value) {
    let mut path = format!(
        "/erp/accounting/workspace?tenant=acme&companyId={}&collection={name}",
        company["companyId"].as_str().unwrap()
    );
    if let Some(id) = id {
        path.push_str(&format!("&id={id}"));
    }
    call(app, "GET", &path, Value::Null, Some(session)).await
}
impl Scenario {
    async fn new() -> Self {
        let s = SourceFixture::new().await;
        s.enable().await;
        let invoice = s.invoice("ACTUAL-DUE-TERMS").await;
        let source=s.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Original issued invoice before explicit due terms"})).await;
        let (date,due):(String,String)=sqlx::query_as("SELECT source_date::text,(source_date+30)::text FROM archava_accounting.invoice_sources WHERE id=$1")
            .bind(Uuid::parse_str(source["commandId"].as_str().unwrap()).unwrap()).fetch_one(&s.f.admin_pool).await.unwrap();
        Self {
            s,
            source,
            date,
            due,
        }
    }
    fn command(&self, revision: i32, condition: Value) -> Value {
        json!({"type":"invoice_terms.set","sourceId":self.source["commandId"],"expectedDocumentRevision":2,"expectedRevision":revision,"effectiveFrom":self.date,"condition":condition,"reason":"Explicit original customer due agreement under current authority"})
    }
    async fn counts(&self) -> (i64, i64, i64, i64) {
        sqlx::query_as("SELECT (SELECT count(*) FROM archava_accounting.document_terms),(SELECT count(*) FROM archava_accounting.journals),(SELECT count(*) FROM archava_accounting.commands),(SELECT count(*) FROM archava_accounting.audit)")
            .fetch_one(&self.s.f.admin_pool).await.unwrap()
    }
}
#[tokio::test]
async fn due_terms_sql_claims_tenant_isolation_scopes_and_guard_metadata_fail_closed() {
    let s = Scenario::new().await;
    let receipt = ok(
        &s.s.app,
        &s.s.alice,
        s.command(0, json!({"kind":"unknown"})),
    )
    .await;
    for tenant in [None, Some("other")] {
        let mut tx = s.s.f.runtime_pool.begin().await.unwrap();
        if let Some(tenant) = tenant {
            sqlx::query("SELECT set_config('archava.tenant_id',$1,true)")
                .bind(tenant)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        let rows: i64 =
            sqlx::query_scalar("SELECT count(*) FROM archava_accounting.document_terms")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(rows, 0);
        tx.rollback().await.unwrap();
    }
    let before = s.counts().await;
    let mut forged = s.s.f.admin_pool.begin().await.unwrap();
    sqlx::query("INSERT INTO archava_accounting.document_terms(tenant_id,id,book_id,request_id,document_kind,source_id,document_id,document_revision,revision,effective_from,due_kind,due_date,created_by,reason) VALUES('acme',$1,$2,$3,'receivable',$4,$5,2,2,$6::text::date,'unknown',NULL,'alice','No corresponding immutable receipt or audit')")
        .bind(Uuid::new_v4()).bind(Uuid::parse_str(s.s.book["bookId"].as_str().unwrap()).unwrap()).bind(Uuid::new_v4()).bind(Uuid::parse_str(s.source["commandId"].as_str().unwrap()).unwrap()).bind(Uuid::parse_str(receipt["documentTerms"]["documentId"].as_str().unwrap()).unwrap()).bind(&s.date).execute(&mut *forged).await.unwrap();
    assert!(forged.commit().await.is_err());
    assert_eq!(s.counts().await, before);
    for actor in ["assigned", "service-seat"] {
        let session = login(&s.s.app, actor).await;
        let (status, _) = send(
            &s.s.app,
            &session,
            &input(s.command(1, json!({"kind":"unknown"}))),
        )
        .await;
        assert!(
            matches!(status, StatusCode::FORBIDDEN | StatusCode::NOT_FOUND),
            "{actor}: {status}"
        );
    }
    assert_eq!(s.counts().await, before);
    sqlx::query("ALTER TABLE archava_accounting.document_terms DISABLE TRIGGER document_terms_claim_complete").execute(&s.s.f.admin_pool).await.unwrap();
    let complete: bool = sqlx::query_scalar("SELECT archava_accounting.terms_guards_complete()")
        .fetch_one(&s.s.f.runtime_pool)
        .await
        .unwrap();
    assert!(!complete);
    let state = AppState::try_new_all(
        s.s.f.runtime_pool.clone(),
        ledger(),
        AllReadiness {
            commerce: &s.s.f.commerce_ledger,
            identity: &s.s.f.identity_ledger,
            crm: &s.s.f.crm_ledger,
            owner: &s.s.f.owner_ledger,
            purchasing: &s.s.f.purchasing_ledger,
            additional_erp: &s.s.f.erp_readiness(),
        },
        &[7; 32],
    )
    .await;
    assert!(state.is_err());
}

#[tokio::test]
async fn due_terms_actual_source_preserves_money_versions_replay_and_explicit_unknown() {
    let s = Scenario::new().await;
    let before = s.counts().await;
    let body = input(s.command(0, json!({"kind":"due_on","date":s.due})));
    let (st, v) = send(&s.s.app, &s.s.alice, &body).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let first = &v["receipt"];
    assert_eq!(first["resourceKind"], "document_terms");
    assert_eq!(first["sourceId"], s.source["commandId"]);
    assert_eq!(
        first["documentTerms"]["condition"],
        json!({"kind":"due_on","date":s.due})
    );
    assert_eq!(first["documentTerms"]["termsRevision"], 1);
    assert_eq!(first["documentTerms"]["documentRevision"], 2);
    let after = s.counts().await;
    assert_eq!(after.0, before.0 + 1);
    assert_eq!(after.1, before.1);
    assert_eq!(
        send(&s.s.app, &s.s.alice, &body).await.1["receipt"]["replayed"],
        true
    );
    assert_eq!(s.counts().await, after);
    let (st, unknown) = send(
        &s.s.app,
        &s.s.alice,
        &input(s.command(1, json!({"kind":"unknown"}))),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{unknown}");
    assert_eq!(
        unknown["receipt"]["documentTerms"]["condition"],
        json!({"kind":"unknown"})
    );
    assert_eq!(unknown["receipt"]["revision"], 2);
    let counts = s.counts().await;
    let (st, replayed) = send(&s.s.app, &s.s.alice, &body).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(replayed["receipt"]["resourceId"], first["resourceId"]);
    assert_eq!(
        replayed["receipt"]["documentTerms"]["condition"]["date"],
        s.due
    );
    assert_eq!(s.counts().await, counts);
    assert_eq!(
        send(
            &s.s.app,
            &s.s.alice,
            &input(s.command(1, json!({"kind":"due_on","date":s.due})))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(s.counts().await, counts);
    let mut changed = body.clone();
    changed["command"]["condition"] = json!({"kind":"unknown"});
    assert_eq!(
        send(&s.s.app, &s.s.alice, &changed).await.0,
        StatusCode::CONFLICT
    );
    assert!(sqlx::query(
        "UPDATE archava_accounting.document_terms SET due_kind='unknown',due_date=NULL WHERE id=$1"
    )
    .bind(Uuid::parse_str(first["resourceId"].as_str().unwrap()).unwrap())
    .execute(&s.s.f.admin_pool)
    .await
    .is_err());
    assert_eq!(s.counts().await, counts);
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=array_remove(permissions,'terms.write') WHERE principal_id='alice'").execute(&s.s.f.admin_pool).await.unwrap();
    assert_eq!(
        send(
            &s.s.app,
            &s.s.alice,
            &input(s.command(2, json!({"kind":"unknown"})))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(s.counts().await, counts);
}
#[tokio::test]
async fn due_terms_original_source_current_document_and_open_effective_period_are_required() {
    let s = Scenario::new().await;
    let before = s.counts().await;
    let mut stale = s.command(0, json!({"kind":"due_on","date":s.due}));
    stale["expectedDocumentRevision"] = json!(1);
    assert_eq!(
        send(&s.s.app, &s.s.alice, &input(stale)).await.0,
        StatusCode::CONFLICT
    );
    let mut wrong = s.command(0, json!({"kind":"unknown"}));
    wrong["type"] = json!("supplier_terms.set");
    assert_eq!(
        send(&s.s.app, &s.s.alice, &input(wrong)).await.0,
        StatusCode::NOT_FOUND
    );
    let earlier: String = sqlx::query_scalar("SELECT ($1::text::date-1)::text")
        .bind(&s.date)
        .fetch_one(&s.s.f.admin_pool)
        .await
        .unwrap();
    let mut backdated = s.command(0, json!({"kind":"unknown"}));
    backdated["effectiveFrom"] = json!(earlier);
    assert_eq!(
        send(&s.s.app, &s.s.alice, &input(backdated)).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(s.counts().await, before);
    s.s.post_source(&s.source, "POST-ORIGINAL").await;
    ok(&s.s.app,&s.s.alice,json!({"type":"period.close","id":s.s.period["resourceId"],"expectedRevision":1,"reason":"All original captured sources are posted"})).await;
    let closed = s.counts().await;
    let body = input(s.command(0, json!({"kind":"unknown"})));
    assert_eq!(
        send(&s.s.app, &s.s.alice, &body).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(s.counts().await, closed);
    ok(&s.s.app,&s.s.alice,json!({"type":"period.reopen","id":s.s.period["resourceId"],"expectedRevision":2,"reason":"Authorized explicit reopen before due-term history change"})).await;
    let (st, v) = send(&s.s.app, &s.s.alice, &body).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["receipt"]["revision"], 1);
}

#[tokio::test]
async fn due_terms_supplier_source_races_have_one_version_and_need_explicit_payable_read() {
    let p = purchase_sources::PurchaseScenario::new(true).await;
    let (po, _receipt, line) = p.receive().await;
    let date: String =
        sqlx::query_scalar("SELECT (clock_timestamp() AT TIME ZONE 'UTC')::date::text")
            .fetch_one(&p.f.admin_pool)
            .await
            .unwrap();
    let document=p.purchase(json!({"operation":"document.capture","orderId":po["id"],"expectedOrderRevision":3,"supplierId":p.supplier["id"],"vendorReference":"TERMS-SUPPLIER","documentDate":date,"currency":"IDR","declaredTotal":14000,"lines":[{"lineId":line,"quantity":2,"unitCost":7000}]})).await;
    let bill=p.purchase(json!({"operation":"document.match","id":document["id"],"expectedRevision":1,"expectedOrderRevision":3,"reason":"Actual accepted original supplier bill for due-date review"})).await;
    let(date,due):(String,String)=sqlx::query_as("SELECT source_date::text,(source_date+15)::text FROM archava_accounting.purchase_sources WHERE id=$1")
        .bind(Uuid::parse_str(bill["requestId"].as_str().unwrap()).unwrap()).fetch_one(&p.f.admin_pool).await.unwrap();
    let make = || {
        input(
            json!({"type":"supplier_terms.set","sourceId":bill["requestId"],"expectedDocumentRevision":2,"expectedRevision":0,"effectiveFrom":date,"condition":{"kind":"due_on","date":due},"reason":"Review explicit original supplier due agreement"}),
        )
    };
    let left = make();
    let right = make();
    let journals: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_accounting.journals")
        .fetch_one(&p.f.admin_pool)
        .await
        .unwrap();
    let (left_result, right_result) = tokio::join!(
        send(&p.app, &p.alice, &left),
        send(&p.app, &p.alice, &right)
    );
    let mut statuses = [left_result.0.as_u16(), right_result.0.as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 409], "{left_result:?} {right_result:?}");
    let winner = if left_result.0 == StatusCode::OK {
        &left_result.1["receipt"]
    } else {
        &right_result.1["receipt"]
    };
    assert_eq!(winner["documentTerms"]["documentKind"], "payable");
    assert_eq!(winner["documentTerms"]["documentId"], document["id"]);
    let (st, page) = collection(
        &p.app,
        &p.alice,
        &p.company,
        "supplier_term_sources",
        Some(bill["requestId"].as_str().unwrap()),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{page}");
    assert_eq!(page["supplier_term_sources"][0]["documentRevision"], 2);
    assert_eq!(page["supplier_term_sources"][0]["termsRevision"], 1);
    assert_eq!(
        page["supplier_term_sources"][0]["latestCondition"],
        json!({"kind":"due_on","date":due})
    );
    let (st, payable) = collection(
        &p.app,
        &p.alice,
        &p.company,
        "payables",
        Some(bill["requestId"].as_str().unwrap()),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{payable}");
    assert_eq!(payable["payables"][0]["termsKnown"], true);
    assert_eq!(payable["payables"][0]["dueDate"], due);
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=array_remove(permissions,'terms.read') WHERE principal_id='alice'").execute(&p.f.admin_pool).await.unwrap();
    let (st, hidden) = collection(
        &p.app,
        &p.alice,
        &p.company,
        "payables",
        Some(bill["requestId"].as_str().unwrap()),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{hidden}");
    assert!(hidden["payables"][0]["termsKnown"].is_null());
    assert!(hidden["payables"][0]["dueTerms"].is_null());
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=permissions||ARRAY['terms.read'] WHERE principal_id='alice'").execute(&p.f.admin_pool).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM archava_accounting.document_terms")
            .fetch_one(&p.f.admin_pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM archava_accounting.journals")
            .fetch_one(&p.f.admin_pool)
            .await
            .unwrap(),
        journals
    );
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=array_remove(permissions,'payables.read') WHERE principal_id='alice'").execute(&p.f.admin_pool).await.unwrap();
    assert_eq!(
        collection(&p.app, &p.alice, &p.company, "supplier_term_sources", None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (st, history) = collection(&p.app, &p.alice, &p.company, "document_terms", None).await;
    assert_eq!(st, StatusCode::OK, "{history}");
    assert_eq!(history["document_terms"], json!([]));
    assert_eq!(
        collection(
            &p.app,
            &p.alice,
            &p.company,
            "document_terms",
            winner["resourceId"].as_str()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let mut denied = make();
    denied["command"]["expectedRevision"] = json!(1);
    assert_eq!(
        send(&p.app, &p.alice, &denied).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM archava_accounting.document_terms")
            .fetch_one(&p.f.admin_pool)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn due_terms_private_source_and_history_pages_follow_current_read_authority() {
    let s = Scenario::new().await;
    let source_id = s.source["commandId"].as_str().unwrap();
    let (st, page) = collection(
        &s.s.app,
        &s.s.alice,
        &s.s.company,
        "invoice_term_sources",
        Some(source_id),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{page}");
    let original = &page["invoice_term_sources"][0];
    assert_eq!(original["documentRevision"], 2);
    assert_eq!(original["termsRevision"], 0);
    assert!(original["latestCondition"].is_null());
    assert_eq!(original["sourceDate"], s.date);
    let (st, saved) = send(
        &s.s.app,
        &s.s.alice,
        &input(s.command(0, json!({"kind":"due_on","date":s.due}))),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{saved}");
    let receipt = &saved["receipt"];
    let (st, page) = collection(
        &s.s.app,
        &s.s.alice,
        &s.s.company,
        "invoice_term_sources",
        Some(source_id),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{page}");
    assert_eq!(page["invoice_term_sources"][0]["termsRevision"], 1);
    assert_eq!(
        page["invoice_term_sources"][0]["latestCondition"],
        receipt["documentTerms"]["condition"]
    );
    let (st, history) = collection(
        &s.s.app,
        &s.s.alice,
        &s.s.company,
        "document_terms",
        receipt["resourceId"].as_str(),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{history}");
    assert_eq!(
        history["document_terms"][0]["documentId"],
        receipt["documentTerms"]["documentId"]
    );
    assert_eq!(history["document_terms"][0]["revision"], 1);
    assert!(history["document_terms"][0]["recordedAt"].is_string());
    assert_eq!(
        history["detail"]["record"]["condition"],
        receipt["documentTerms"]["condition"]
    );
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=array_remove(permissions,'terms.read') WHERE principal_id='alice'").execute(&s.s.f.admin_pool).await.unwrap();
    for name in [
        "invoice_term_sources",
        "supplier_term_sources",
        "document_terms",
    ] {
        assert_eq!(
            collection(&s.s.app, &s.s.alice, &s.s.company, name, None)
                .await
                .0,
            StatusCode::FORBIDDEN,
            "{name}"
        );
    }
    let (st, page) = snapshot(&s.s.app, &s.s.alice, &s.s.company).await;
    assert_eq!(st, StatusCode::OK, "{page}");
    for name in [
        "invoice_term_sources",
        "supplier_term_sources",
        "document_terms",
    ] {
        assert!(page.get(name).is_none(), "{name}");
    }
    assert_eq!(
        status(
            &s.s.app,
            &s.s.alice,
            &json!({"requestId":receipt["requestId"]})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}
