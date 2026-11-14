//! Actual source/HTTP/non-owner transactions, including racing reservations.
use super::*;
use purchase_sources::PurchaseScenario;

struct Scenario {
    p: PurchaseScenario,
    bill: Value,
    document: Value,
    cash: Value,
    rows: Vec<Value>,
    date: String,
}
impl Scenario {
    async fn new(post_bill: bool) -> Self {
        let p = PurchaseScenario::new(true).await;
        let (po, receipt, line) = p.receive().await;
        p.post(&p.reviewed(&receipt, "RECEIPT", &[0, 1, 3]).await)
            .await;
        let date: String =
            sqlx::query_scalar("SELECT (clock_timestamp() AT TIME ZONE 'UTC')::date::text")
                .fetch_one(&p.f.admin_pool)
                .await
                .unwrap();
        let document = p.purchase(json!({"operation":"document.capture","orderId":po["id"],"expectedOrderRevision":3,"supplierId":p.supplier["id"],"vendorReference":"ACTUAL-SUPPLIER-BILL","documentDate":date,"currency":"IDR","declaredTotal":14000,"lines":[{"lineId":line,"quantity":2,"unitCost":7000}]})).await;
        let bill = p.purchase(json!({"operation":"document.match","id":document["id"],"expectedRevision":1,"expectedOrderRevision":3,"reason":"Match the accepted original supplier goods"})).await;
        if post_bill {
            p.post(&p.reviewed(&bill, "BILL", &[1, 2]).await).await;
        }
        let cash = account(&p.app, &p.alice, &p.book, "BANK", "asset", "cash").await;
        let binding = bank::bank_account(&p.app, &p.alice, &p.book, &cash).await;
        let statement=ok(&p.app,&p.alice,json!({"type":"bank_statement.import","bankAccountId":binding["resourceId"],"account":bank::account_ref(&cash),"statement":{"reference":"ACTUAL-SUPPLIER-MOVEMENTS","fromDate":date,"toDate":date,"currency":"IDR","openingMinor":"5000000","closingMinor":"1000000","rows":[
            {"externalRef":"PAYMENT-A","date":date,"amountMinor":"-1000000","description":"Actual supplier withdrawal A"},
            {"externalRef":"PAYMENT-B","date":date,"amountMinor":"-1000000","description":"Actual supplier withdrawal B"},
            {"externalRef":"RETURN-A","date":date,"amountMinor":"1000000","description":"Actual supplier return A"},
            {"externalRef":"PAYMENT-C","date":date,"amountMinor":"-3000000","description":"Actual supplier withdrawal C"}]},"reason":"Review actual synthetic bank statement evidence"})).await;
        let (_, data) = bank::bank_workspace(&p.app, &p.alice, &p.company, "bank_rows").await;
        let mut rows = data["bank_rows"].as_array().unwrap().clone();
        rows.sort_by_key(|r| r["ordinal"].as_i64().unwrap());
        assert_eq!(rows.len(), 4);
        assert!(statement["resourceId"].is_string());
        Self {
            p,
            bill,
            document,
            cash,
            rows,
            date,
        }
    }
    fn payment(&self, code: &str, row: usize, amount: &str) -> Value {
        json!({"type":"payable_payment.draft","sourceId":self.bill["requestId"],"expectedDocumentRevision":2,"bankRowId":self.rows[row]["id"],"code":code,"payableAccount":bank::account_ref(&self.p.accounts[2]),"cashAccount":bank::account_ref(&self.cash),"amountMinor":amount,"reason":"Review actual supplier bank withdrawal and original bill"})
    }
    fn returned(&self, original: &Value, code: &str, amount: &str) -> Value {
        json!({"type":"payable_return.draft","originalPaymentId":original["resourceId"],"expectedJournalRevision":2,"bankRowId":self.rows[2]["id"],"code":code,"payableAccount":bank::account_ref(&self.p.accounts[2]),"cashAccount":bank::account_ref(&self.cash),"amountMinor":amount,"reason":"Review actual returned supplier bank payment"})
    }
    async fn summary(&self) -> Value {
        let (st, data) =
            bank::bank_workspace(&self.p.app, &self.p.alice, &self.p.company, "payables").await;
        assert_eq!(st, StatusCode::OK, "{data}");
        assert_eq!(data["payables"].as_array().unwrap().len(), 1);
        assert_eq!(data["payables"][0]["termsKnown"], false);
        data["payablesSummary"].clone()
    }
    async fn post(&self, j: &Value) -> Value {
        ok(&self.p.app,&self.p.bob,json!({"type":"journal.post","id":j["resourceId"],"expectedRevision":1,"reason":"Independent review of actual supplier bank evidence"})).await
    }
    async fn counts(&self) -> (i64, i64, i64, i64) {
        sqlx::query_as("SELECT (SELECT count(*) FROM archava_accounting.journals),(SELECT count(*) FROM archava_accounting.payable_settlements),(SELECT count(*) FROM archava_accounting.commands),(SELECT count(*) FROM archava_accounting.audit)")
            .fetch_one(&self.p.f.admin_pool).await.unwrap()
    }
}

#[tokio::test]
async fn subledger_supplier_cutoff_excludes_later_posted_payments_and_requires_posted_returns() {
    let s = Scenario::new(true).await;
    let future_payable = account(
        &s.p.app,
        &s.p.alice,
        &s.p.book,
        "FUTURE-PAYABLE",
        "liability",
        "operating",
    )
    .await;
    let mut policy = s.p.policy(1);
    policy["mappings"][2]["accountId"] = future_payable["resourceId"].clone();
    ok(&s.p.app, &s.p.alice, policy).await;
    let (tomorrow, after): (String, String) =
        sqlx::query_as("SELECT ($1::text::date+1)::text,($1::text::date+2)::text")
            .bind(&s.date)
            .fetch_one(&s.p.f.admin_pool)
            .await
            .unwrap();
    ok(&s.p.app,&s.p.alice,json!({"type":"bank_statement.import","bankAccountId":s.rows[0]["bankAccountId"],"account":bank::account_ref(&s.cash),"statement":{"reference":"AGING-LATER-CASH","fromDate":tomorrow,"toDate":after,"currency":"IDR","openingMinor":"1000000","closingMinor":"1000000","rows":[
        {"externalRef":"AGING-FUTURE-PAYMENT","date":tomorrow,"amountMinor":"-500000","description":"Actual synthetic withdrawal after report cutoff"},
        {"externalRef":"AGING-FUTURE-RETURN","date":after,"amountMinor":"500000","description":"Actual synthetic returned withdrawal after payment"}]},"reason":"Review explicit later bank movement dates"})).await;
    let (_, rows) = bank::bank_workspace(&s.p.app, &s.p.alice, &s.p.company, "bank_rows").await;
    let later_row = |external: &str| {
        rows["bank_rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["externalRef"] == external)
            .unwrap()["id"]
            .clone()
    };
    let report = |date: String| {
        let s = &s;
        async move { subledger::accepted(&s.p.app, &s.p.alice, &s.p.book, "payable", &date).await }
    };
    let mut command = s.payment("AGING-LATER-PAY", 0, "500000");
    command["bankRowId"] = later_row("AGING-FUTURE-PAYMENT");
    let payment = ok(&s.p.app, &s.p.alice, command).await;
    assert_eq!(
        report(tomorrow.clone()).await["totals"]["netOutstandingMinor"],
        "1400000"
    );
    let posted = s.post(&payment).await;
    assert_eq!(s.summary().await["outstandingMinor"], "900000");
    let historical = report(s.date.clone()).await;
    assert_eq!(historical["totals"]["unknownTermsMinor"], "1400000");
    assert_eq!(historical["postedSourceControlsAgree"], true);
    let paid = report(tomorrow.clone()).await;
    assert_eq!(paid["totals"]["netOutstandingMinor"], "900000");
    assert_eq!(paid["postedSourceControlsAgree"], true);
    let mut command = s.returned(&posted, "AGING-LATER-RETURN", "500000");
    command["bankRowId"] = later_row("AGING-FUTURE-RETURN");
    let returned = ok(&s.p.app, &s.p.alice, command).await;
    assert_eq!(
        report(after.clone()).await["totals"]["netOutstandingMinor"],
        "900000"
    );
    s.post(&returned).await;
    let restored = report(after.clone()).await;
    assert_eq!(restored["totals"]["netOutstandingMinor"], "1400000");
    assert_eq!(restored["postedSourceControlsAgree"], true);
    assert_eq!(restored["controls"].as_array().unwrap().len(), 2);
    let declared = restored["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["accountId"] == future_payable["resourceId"])
        .unwrap();
    assert_eq!(declared["expectedMinor"], "0");
    assert_eq!(declared["actualMinor"], "0");
    assert_eq!(
        report(tomorrow.clone()).await["totals"]["netOutstandingMinor"],
        "900000"
    );
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=array_remove(permissions,'payables.read') WHERE principal_id='alice'").execute(&s.p.f.admin_pool).await.unwrap();
    assert_eq!(
        subledger::read(&s.p.app, &s.p.alice, &s.p.book, "payable", &after, "")
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn subledger_supplier_operating_void_does_not_hide_unposted_financial_correction() {
    let s = Scenario::new(true).await;
    let before = subledger::accepted(&s.p.app, &s.p.alice, &s.p.book, "payable", &s.date).await;
    assert_eq!(before["totals"]["netOutstandingMinor"], "1400000");
    let void=s.p.purchase(json!({"operation":"document.void","id":s.document["id"],"expectedRevision":2,"reason":"Void the original reviewed supplier claim"})).await;
    let unposted = subledger::accepted(&s.p.app, &s.p.alice, &s.p.book, "payable", &s.date).await;
    assert_eq!(unposted["totals"]["netOutstandingMinor"], "1400000");
    assert_eq!(unposted["coverage"]["pendingMonetarySourceCount"], 1);
    s.p.post(&s.p.reviewed(&void, "AGING-VOID", &[1, 2]).await)
        .await;
    let posted = subledger::accepted(&s.p.app, &s.p.alice, &s.p.book, "payable", &s.date).await;
    assert_eq!(posted["totals"]["netOutstandingMinor"], "0");
    assert_eq!(posted["positions"][0]["bucket"], "settled");
    assert_eq!(posted["postedSourceControlsAgree"], true);
    assert_eq!(posted["coverage"]["pendingMonetarySourceCount"], 0);
}

#[tokio::test]
async fn payable_partial_payment_actual_return_and_bank_matching_keep_exact_independent_balances() {
    let s = Scenario::new(true).await;
    let body = input(s.payment("PAY-1", 0, "1000000"));
    let (st, v) = send(&s.p.app, &s.p.alice, &body).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let payment = v["receipt"].clone();
    assert_eq!(payment["sourceId"], s.bill["requestId"]);
    assert_eq!(payment["bankRowId"], s.rows[0]["id"]);
    assert_eq!(s.summary().await["reservedPaymentMinor"], "1000000");
    let before = s.counts().await;
    assert_eq!(
        send(
            &s.p.app,
            &s.p.alice,
            &input(s.payment("OVER-RESERVED", 1, "400001"))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(s.counts().await, before);
    assert_eq!(send(&s.p.app,&s.p.alice,&input(json!({"type":"journal.post","id":payment["resourceId"],"expectedRevision":1,"reason":"Creator cannot independently post"}))).await.0,StatusCode::FORBIDDEN);
    let posted = s.post(&payment).await;
    let summary = s.summary().await;
    assert_eq!(summary["postedPaymentMinor"], "1000000");
    assert_eq!(summary["outstandingMinor"], "400000");
    assert_eq!(summary["reservedPaymentMinor"], "0");
    let (st, replay) = send(&s.p.app, &s.p.alice, &body).await;
    assert_eq!(st, StatusCode::OK, "{replay}");
    assert_eq!(replay["receipt"]["replayed"], true);
    assert_eq!(replay["receipt"]["resourceId"], payment["resourceId"]);
    // A payment cannot be reconciled to another withdrawal with the same amount.
    assert_eq!(
        send(
            &s.p.app,
            &s.p.alice,
            &bank::match_row(&s.rows[1], &posted, 2, &s.cash, "1")
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (st, matched) = send(
        &s.p.app,
        &s.p.alice,
        &bank::match_row(&s.rows[0], &posted, 2, &s.cash, "400000"),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{matched}");
    let (st, more) = send(
        &s.p.app,
        &s.p.alice,
        &bank::match_row(&s.rows[0], &posted, 2, &s.cash, "600000"),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{more}");
    ok(&s.p.app,&s.p.alice,json!({"type":"bank_allocation.reverse","id":matched["receipt"]["resourceId"],"expectedRevision":1,"reason":"Unmatching leaves the posted supplier payment intact"})).await;
    assert_eq!(s.summary().await["postedPaymentMinor"], "1000000");
    let reserved: String =
        sqlx::query_scalar("SELECT archava_accounting.payable_row_reserved('acme',$1)::text")
            .bind(Uuid::parse_str(s.rows[0]["id"].as_str().unwrap()).unwrap())
            .fetch_one(&s.p.f.admin_pool)
            .await
            .unwrap();
    assert_eq!(reserved, "400000");
    let returned = ok(
        &s.p.app,
        &s.p.alice,
        s.returned(&posted, "RETURN-1", "1000000"),
    )
    .await;
    assert_eq!(returned["originalPayablePaymentId"], payment["resourceId"]);
    assert_eq!(s.summary().await["availableMinor"], "400000");
    assert_eq!(
        send(
            &s.p.app,
            &s.p.alice,
            &input(s.payment("DRAFT-RETURN-NO-CAPACITY", 1, "400001"))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(
            &s.p.app,
            &s.p.alice,
            &input(s.returned(&posted, "OVER-RETURN", "1"))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let posted_return = s.post(&returned).await;
    assert_eq!(s.summary().await["availableMinor"], "1400000");
    assert_eq!(
        send(
            &s.p.app,
            &s.p.alice,
            &bank::match_row(&s.rows[2], &posted_return, 2, &s.cash, "1000000")
        )
        .await
        .0,
        StatusCode::OK
    );
    for j in [&posted, &posted_return] {
        assert_eq!(send(&s.p.app,&s.p.alice,&input(json!({"type":"journal.reverse","id":j["resourceId"],"expectedRevision":2,"code":"IMAGINARY-REVERSAL","date":s.date,"sourceRef":"Cannot erase actual bank movement","reason":"A real returned bank transaction is required"}))).await.0,StatusCode::CONFLICT);
    }
    let net:String=sqlx::query_scalar("SELECT sum(l.credit_minor-l.debit_minor)::text FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id WHERE l.tenant_id='acme' AND l.account_id=$1 AND j.state='posted'")
        .bind(Uuid::parse_str(s.p.accounts[2]["resourceId"].as_str().unwrap()).unwrap()).fetch_one(&s.p.f.admin_pool).await.unwrap();
    assert_eq!(net, "1400000");
}

#[tokio::test]
async fn payable_source_review_refusals_draft_void_and_current_authority_roll_back_every_claim() {
    let s = Scenario::new(false).await;
    let before = s.counts().await;
    assert_eq!(
        send(
            &s.p.app,
            &s.p.alice,
            &input(s.payment("UNPOSTED", 0, "500000"))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(s.counts().await, before);
    s.p.post(&s.p.reviewed(&s.bill, "BILL", &[1, 2]).await)
        .await;
    for (field, value) in [
        ("expectedDocumentRevision", json!(1)),
        ("bankRowId", s.rows[2]["id"].clone()),
        ("payableAccount", bank::account_ref(&s.p.accounts[1])),
        ("cashAccount", bank::account_ref(&s.p.accounts[0])),
    ] {
        let mut command = s.payment("INVALID", 0, "1000000");
        command[field] = value;
        let counts = s.counts().await;
        assert_eq!(
            send(&s.p.app, &s.p.alice, &input(command)).await.0,
            StatusCode::CONFLICT
        );
        assert_eq!(s.counts().await, counts);
    }
    let body = input(s.payment("VALID-DRAFT", 0, "1000000"));
    let (st, v) = send(&s.p.app, &s.p.alice, &body).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let request = body["requestId"].as_str().unwrap();
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=array_remove(permissions,'payables.write') WHERE tenant_id='acme' AND principal_id='alice'").execute(&s.p.f.admin_pool).await.unwrap();
    assert_eq!(
        send(&s.p.app, &s.p.alice, &body).await.0,
        StatusCode::FORBIDDEN
    );
    let (st, _) = call(
        &s.p.app,
        "GET",
        &format!("/erp/accounting/commands/status?tenant=acme&requestId={request}"),
        Value::Null,
        Some(&s.p.alice),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=array_append(permissions,'payables.write') WHERE tenant_id='acme' AND principal_id='alice'").execute(&s.p.f.admin_pool).await.unwrap();
    ok(&s.p.app,&s.p.alice,json!({"type":"journal.void","id":v["receipt"]["resourceId"],"expectedRevision":1,"reason":"Void an unposted reservation without inventing cash"})).await;
    assert_eq!(s.summary().await["reservedPaymentMinor"], "0");
    assert_eq!(s.summary().await["availableMinor"], "1400000");
    let reader = login(&s.p.app, "reader").await;
    for collection in ["payables", "payable_settlements"] {
        assert_eq!(
            bank::bank_workspace(&s.p.app, &reader, &s.p.company, collection)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    let (_, visible) = bank::bank_workspace(&s.p.app, &reader, &s.p.company, "journals").await;
    assert!(!visible
        .to_string()
        .contains(s.rows[0]["id"].as_str().unwrap()));
    assert!(visible.get("payablesSummary").is_none());
}

#[tokio::test]
async fn payable_racing_drafts_and_bill_void_cannot_spend_the_same_source_or_bank_capacity() {
    let s = Scenario::new(true).await;
    let left = input(s.payment("RACE-A", 0, "1000000"));
    let right = input(s.payment("RACE-B", 1, "1000000"));
    let (left, right) = tokio::join!(
        send(&s.p.app, &s.p.alice, &left),
        send(&s.p.app, &s.p.alice, &right)
    );
    let mut statuses = vec![left.0.as_u16(), right.0.as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    let counts = s.counts().await;
    let void = input(
        json!({"operation":"document.void","id":s.document["id"],"expectedRevision":2,"reason":"Cannot hide a reserved supplier payment"}),
    );
    assert_eq!(
        call(
            &s.p.app,
            "POST",
            "/erp/purchasing/commands",
            void.clone(),
            Some(&s.p.alice)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(s.counts().await, counts);
    let row:(String,i32)=sqlx::query_as("SELECT state,revision FROM archava_purchasing.supplier_documents WHERE tenant_id='acme' AND id=$1").bind(Uuid::parse_str(s.document["id"].as_str().unwrap()).unwrap()).fetch_one(&s.p.f.admin_pool).await.unwrap();
    assert_eq!(row, ("matched".into(), 2));
    let winner = if left.0 == StatusCode::OK {
        &left.1["receipt"]
    } else {
        &right.1["receipt"]
    };
    let posted = s.post(winner).await;
    assert_eq!(
        call(
            &s.p.app,
            "POST",
            "/erp/purchasing/commands",
            void.clone(),
            Some(&s.p.alice)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let returned = ok(
        &s.p.app,
        &s.p.alice,
        s.returned(&posted, "FULL-RETURN", "1000000"),
    )
    .await;
    assert_eq!(
        call(
            &s.p.app,
            "POST",
            "/erp/purchasing/commands",
            void.clone(),
            Some(&s.p.alice)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    s.post(&returned).await;
    let (st, v) = call(
        &s.p.app,
        "POST",
        "/erp/purchasing/commands",
        void,
        Some(&s.p.alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // The operational void has not posted its source reversal yet.
    assert_eq!(s.summary().await["recognizedMinor"], "1400000");
    let correction = s.p.reviewed(&v["receipt"], "VOID-BILL", &[1, 2]).await;
    s.p.post(&correction).await;
    assert_eq!(s.summary().await["recognizedMinor"], "0");
    assert_eq!(s.summary().await["outstandingMinor"], "0");
}

#[tokio::test]
async fn payable_bank_reservation_blocks_an_unrelated_posted_journal_and_survives_unmatch() {
    let s = Scenario::new(true).await;
    let payment = ok(
        &s.p.app,
        &s.p.alice,
        s.payment("RESERVED-PAYMENT", 0, "1000000"),
    )
    .await;
    let equity = account(
        &s.p.app,
        &s.p.alice,
        &s.p.book,
        "OTHER-EQUITY",
        "equity",
        "financing",
    )
    .await;
    let other=ok(&s.p.app,&s.p.alice,json!({"type":"journal.draft","bookId":s.p.book["bookId"],"code":"UNRELATED-WITHDRAWAL","date":s.date,"sourceRef":"Explicit unrelated source","memo":"Cannot consume supplier-reserved evidence","lines":[line(&equity,"1000000","0"),line(&s.cash,"0","1000000")],"reason":"Unrelated reviewed journal still cannot take reserved bank evidence"})).await;
    let other = s.post(&other).await;
    assert_eq!(
        send(
            &s.p.app,
            &s.p.alice,
            &bank::match_row(&s.rows[0], &other, 2, &s.cash, "1")
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let posted = s.post(&payment).await;
    let (st, matched) = send(
        &s.p.app,
        &s.p.alice,
        &bank::match_row(&s.rows[0], &posted, 2, &s.cash, "1000000"),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{matched}");
    ok(&s.p.app,&s.p.alice,json!({"type":"bank_allocation.reverse","id":matched["receipt"]["resourceId"],"expectedRevision":1,"reason":"Keep the actual payment reserved while correcting a match"})).await;
    assert_eq!(
        send(
            &s.p.app,
            &s.p.alice,
            &bank::match_row(&s.rows[0], &other, 2, &s.cash, "1")
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=array_remove(permissions,'bank.read') WHERE tenant_id='acme' AND principal_id='alice'").execute(&s.p.f.admin_pool).await.unwrap();
    assert_eq!(
        send(
            &s.p.app,
            &s.p.alice,
            &input(s.returned(&posted, "NO-BANK-ACCESS", "1"))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        bank::bank_workspace(&s.p.app, &s.p.alice, &s.p.company, "payable_settlements")
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (_, rows) = bank::bank_workspace(&s.p.app, &s.p.alice, &s.p.company, "journals").await;
    assert!(!rows.to_string().contains(s.rows[0]["id"].as_str().unwrap()));
}

#[tokio::test]
async fn payable_racing_source_void_never_commits_both_or_deadlocks() {
    let s = Scenario::new(true).await;
    let payment = input(s.payment("CONCURRENT-VOID", 0, "500000"));
    let void = input(
        json!({"operation":"document.void","id":s.document["id"],"expectedRevision":2,"reason":"Race the actual source correction against its settlement reservation"}),
    );
    let (paid, voided) = tokio::join!(
        send(&s.p.app, &s.p.alice, &payment),
        call(
            &s.p.app,
            "POST",
            "/erp/purchasing/commands",
            void,
            Some(&s.p.alice)
        )
    );
    assert!(
        matches!(
            (paid.0, voided.0),
            (StatusCode::OK, StatusCode::CONFLICT) | (StatusCode::CONFLICT, StatusCode::OK)
        ),
        "payment={paid:?} void={voided:?}"
    );
    let state: String = sqlx::query_scalar(
        "SELECT state FROM archava_purchasing.supplier_documents WHERE tenant_id='acme' AND id=$1",
    )
    .bind(Uuid::parse_str(s.document["id"].as_str().unwrap()).unwrap())
    .fetch_one(&s.p.f.admin_pool)
    .await
    .unwrap();
    let settlements: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_accounting.payable_settlements")
            .fetch_one(&s.p.f.admin_pool)
            .await
            .unwrap();
    assert_eq!(
        (state.as_str(), settlements),
        if paid.0 == StatusCode::OK {
            ("matched", 1)
        } else {
            ("void", 0)
        }
    );
}

#[tokio::test]
async fn payable_sql_claim_integrity_rls_and_startup_metadata_fail_closed() {
    let s = Scenario::new(true).await;
    let payment = ok(&s.p.app, &s.p.alice, s.payment("SQL-CONTROL", 0, "500000")).await;
    let check:bool=sqlx::query_scalar("SELECT archava_accounting.payable_guards_complete() AND archava_control.write_guards_complete()").fetch_one(&s.p.f.admin_pool).await.unwrap();
    assert!(check);
    sqlx::query(
        "ALTER TABLE archava_accounting.payable_settlements DISABLE TRIGGER payable_claim_complete",
    )
    .execute(&s.p.f.admin_pool)
    .await
    .unwrap();
    let check: bool = sqlx::query_scalar("SELECT archava_accounting.payable_guards_complete()")
        .fetch_one(&s.p.f.admin_pool)
        .await
        .unwrap();
    assert!(!check);
    sqlx::query(
        "ALTER TABLE archava_accounting.payable_settlements ENABLE TRIGGER payable_claim_complete",
    )
    .execute(&s.p.f.admin_pool)
    .await
    .unwrap();
    let error=sqlx::query("UPDATE archava_accounting.payable_settlements SET amount_minor=1 WHERE tenant_id='acme' AND id=$1").bind(Uuid::parse_str(payment["resourceId"].as_str().unwrap()).unwrap()).execute(&s.p.f.admin_pool).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    let mut tx = s.p.f.runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('archava.tenant_id','different-tenant',true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_accounting.payable_settlements")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(n, 0);
    tx.rollback().await.unwrap();
    let mut forged = draft(
        &s.p.book,
        "FORGED-PAYABLE-PREFIX",
        vec![line(&s.p.accounts[2], "1", "0"), line(&s.cash, "0", "1")],
    );
    forged["command"]["date"] = json!(s.date);
    forged["command"]["sourceRef"] = json!(format!(
        "payable:payment:{}:FORGED-PAYABLE-PREFIX",
        s.bill["requestId"].as_str().unwrap()
    ));
    let counts = s.counts().await;
    assert_eq!(
        send(&s.p.app, &s.p.alice, &forged).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(s.counts().await, counts);
}
