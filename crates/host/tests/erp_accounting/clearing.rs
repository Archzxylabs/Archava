//! Actual verified invoice source -> bank clearing, without re-recognizing AR/revenue.
use super::*;
use invoice_sources::SourceFixture;
struct Scenario {
    s: SourceFixture,
    invoice: Value,
    collection: Value,
    cash: Value,
    fee: Value,
    rows: Vec<Value>,
    date: String,
}
impl Scenario {
    async fn new(post_collection: bool) -> Self {
        let s = SourceFixture::new().await;
        s.enable().await;
        let invoice = s.invoice("ACTUAL-CLEARING").await;
        let issue=s.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Actual issued source before real verified collection"})).await;
        s.post_source(&issue, "ISSUE").await;
        let collection = s.payment(&invoice, "succeeded", 30000).await;
        if post_collection {
            s.post_source(&collection, "COLLECTION").await;
        }
        let cash = account(&s.app, &s.alice, &s.book, "BANK", "asset", "cash").await;
        let fee = account(
            &s.app,
            &s.alice,
            &s.book,
            "GATEWAY-FEE",
            "expense",
            "operating",
        )
        .await;
        let binding = bank::bank_account(&s.app, &s.alice, &s.book, &cash).await;
        let date: String =
            sqlx::query_scalar("SELECT (clock_timestamp() AT TIME ZONE 'UTC')::date::text")
                .fetch_one(&s.f.admin_pool)
                .await
                .unwrap();
        ok(&s.app,&s.alice,json!({"type":"bank_statement.import","bankAccountId":binding["resourceId"],"account":bank::account_ref(&cash),"statement":{"reference":"ACTUAL-CLEARING","fromDate":date,"toDate":date,"currency":"IDR","openingMinor":"0","closingMinor":"1970000","rows":[{"externalRef":"NET-COLLECTION","date":date,"amountMinor":"2980000","description":"Gross3000000 less explicit20000 fee"},{"externalRef":"ACTUAL-REFUND","date":date,"amountMinor":"-1010000","description":"Refund1000000 plus explicit10000 fee"}]},"reason":"Review actual owned provider bank evidence"})).await;
        let (_, workspace) = bank::bank_workspace(&s.app, &s.alice, &s.company, "bank_rows").await;
        let mut rows = workspace["bank_rows"].as_array().unwrap().clone();
        rows.sort_by_key(|r| r["ordinal"].as_i64().unwrap());
        Self {
            s,
            invoice,
            collection,
            cash,
            fee,
            rows,
            date,
        }
    }
    fn settlement(&self, source: &Value, row: usize, code: &str, gross: &str, fee: &str) -> Value {
        let mut command = json!({"type":"clearing_settlement.draft","sourceId":source["commandId"],"bankRowId":self.rows[row]["id"],"code":code,"clearingAccount":bank::account_ref(&self.s.clearing),"cashAccount":bank::account_ref(&self.cash),"grossMinor":gross,"feeMinor":fee,"reason":"Review original verified source, explicit fee and actual net bank evidence"});
        if fee != "0" {
            command["feeAccount"] = bank::account_ref(&self.fee)
        }
        command
    }
    async fn post(&self, j: &Value) -> Value {
        ok(&self.s.app,&self.s.bob,json!({"type":"journal.post","id":j["resourceId"],"expectedRevision":1,"reason":"Independent source and bank review"})).await
    }
    async fn counts(&self) -> (i64, i64, i64, i64) {
        sqlx::query_as("SELECT (SELECT count(*) FROM archava_accounting.journals),(SELECT count(*) FROM archava_accounting.clearing_settlements),(SELECT count(*) FROM archava_accounting.commands),(SELECT count(*) FROM archava_accounting.audit)").fetch_one(&self.s.f.admin_pool).await.unwrap()
    }
}
#[tokio::test]
async fn clearing_actual_collection_fee_and_refund_move_cash_once_without_double_ar_or_revenue() {
    let s = Scenario::new(true).await;
    let body = input(s.settlement(&s.collection, 0, "SETTLE", "3000000", "20000"));
    let (st, v) = send(&s.s.app, &s.s.alice, &body).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let draft = &v["receipt"];
    assert_eq!(draft["sourceId"], s.collection["commandId"]);
    assert_eq!(draft["bankRowId"], s.rows[0]["id"]);
    let before = s.s.reports().await;
    assert_eq!(before["profitAndLoss"]["revenueMinor"], "3000000");
    assert_eq!(send(&s.s.app,&s.s.alice,&input(json!({"type":"journal.post","id":draft["resourceId"],"expectedRevision":1,"reason":"Cannot self-approve independent money posting"}))).await.0,StatusCode::FORBIDDEN);
    let posted = s.post(draft).await;
    assert_eq!(
        send(
            &s.s.app,
            &s.s.alice,
            &bank::match_row(&s.rows[0], &posted, 2, &s.cash, "2980000")
        )
        .await
        .0,
        StatusCode::OK
    );
    let refund = s.s.payment(&s.invoice, "refunded", 10000).await;
    s.s.post_source(&refund, "REFUND-SOURCE").await;
    let returned = ok(
        &s.s.app,
        &s.s.alice,
        s.settlement(&refund, 1, "REFUND-BANK", "1000000", "10000"),
    )
    .await;
    let returned = s.post(&returned).await;
    assert_eq!(
        send(
            &s.s.app,
            &s.s.alice,
            &bank::match_row(&s.rows[1], &returned, 2, &s.cash, "1010000")
        )
        .await
        .0,
        StatusCode::OK
    );
    let credit=s.s.bill(json!({"type":"append_correction","invoiceId":s.invoice["invoiceId"],"amountMinor":-10000,"reason":"Actual customer credit after verified partial refund"})).await;
    s.s.post_source(&credit, "CREDIT").await;
    let reports = s.s.reports().await;
    assert_eq!(reports["profitAndLoss"]["revenueMinor"], "2000000");
    assert_eq!(reports["profitAndLoss"]["expenseMinor"], "30000");
    assert_eq!(reports["profitAndLoss"]["netProfitMinor"], "1970000");
    assert_eq!(reports["balanceSheet"]["assetsMinor"], "1970000");
    assert_eq!(reports["balanceSheet"]["balanced"], true);
    assert_eq!(reports["cashFlow"]["operatingMinor"], "1970000");
    assert_eq!(reports["cashFlow"]["unclassifiedMinor"], "0");
    let rows = &reports["trialBalance"]["accounts"];
    for (code, expected) in [
        ("AR", "0"),
        ("CLEARING", "0"),
        ("BANK", "1970000"),
        ("GATEWAY-FEE", "30000"),
    ] {
        assert_eq!(
            rows.as_array()
                .unwrap()
                .iter()
                .find(|r| r["code"] == code)
                .unwrap()["closingNetMinor"],
            expected
        );
    }
    let counts = s.counts().await;
    let (st, replay) = send(&s.s.app, &s.s.alice, &body).await;
    assert_eq!(st, StatusCode::OK, "{replay}");
    assert_eq!(replay["receipt"]["replayed"], true);
    assert_eq!(s.counts().await, counts);
    for j in [&posted, &returned] {
        assert_eq!(send(&s.s.app,&s.s.alice,&input(json!({"type":"journal.reverse","id":j["resourceId"],"expectedRevision":2,"code":"NO-IMAGINARY-RETURN","date":s.date,"sourceRef":"A ledger reversal is not an actual bank event","reason":"Preserve actual settled cash"}))).await.0,StatusCode::CONFLICT);
    }
}
#[tokio::test]
async fn clearing_refuses_unposted_wrong_original_accounts_direction_and_stale_or_revoked_reviews()
{
    let s = Scenario::new(false).await;
    let counts = s.counts().await;
    assert_eq!(
        send(
            &s.s.app,
            &s.s.alice,
            &input(s.settlement(&s.collection, 0, "UNPOSTED", "3000000", "20000"))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(s.counts().await, counts);
    s.s.post_source(&s.collection, "COLLECTION").await;
    for (field, value) in [
        ("bankRowId", s.rows[1]["id"].clone()),
        ("clearingAccount", bank::account_ref(&s.s.ar)),
        ("feeAccount", bank::account_ref(&s.s.revenue)),
        ("cashAccount", bank::account_ref(&s.s.ar)),
    ] {
        let mut command = s.settlement(&s.collection, 0, "INVALID", "3000000", "20000");
        command[field] = value;
        let counts = s.counts().await;
        assert_eq!(
            send(&s.s.app, &s.s.alice, &input(command)).await.0,
            StatusCode::CONFLICT
        );
        assert_eq!(s.counts().await, counts);
    }
    let body = input(s.settlement(&s.collection, 0, "VALID", "3000000", "20000"));
    let (st, v) = send(&s.s.app, &s.s.alice, &body).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=array_remove(permissions,'bank.read') WHERE tenant_id='acme' AND principal_id='alice'").execute(&s.s.f.admin_pool).await.unwrap();
    assert_eq!(
        send(&s.s.app, &s.s.alice, &body).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(&s.s.app, &s.s.alice, &body).await.0,
        StatusCode::FORBIDDEN
    );
    let (_, journals) = bank::bank_workspace(&s.s.app, &s.s.alice, &s.s.company, "journals").await;
    assert!(!journals
        .to_string()
        .contains(s.rows[0]["id"].as_str().unwrap()));
    assert_eq!(
        bank::bank_workspace(&s.s.app, &s.s.alice, &s.s.company, "clearing_settlements")
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}
#[tokio::test]
async fn clearing_racing_gross_reservations_have_one_winner_and_void_releases_both_capacities() {
    let s = Scenario::new(true).await;
    let left = input(s.settlement(&s.collection, 0, "RACE-A", "2000000", "20000"));
    let right = input(s.settlement(&s.collection, 0, "RACE-B", "2000000", "20000"));
    let (left, right) = tokio::join!(
        send(&s.s.app, &s.s.alice, &left),
        send(&s.s.app, &s.s.alice, &right)
    );
    let mut statuses = vec![left.0.as_u16(), right.0.as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    let winner = if left.0 == StatusCode::OK {
        &left.1["receipt"]
    } else {
        &right.1["receipt"]
    };
    ok(&s.s.app,&s.s.alice,json!({"type":"journal.void","id":winner["resourceId"],"expectedRevision":1,"reason":"Release unposted verified-source and bank reservations"})).await;
    let replacement = ok(
        &s.s.app,
        &s.s.alice,
        s.settlement(&s.collection, 0, "REPLACEMENT", "3000000", "20000"),
    )
    .await;
    s.post(&replacement).await;
    let(check,reserved):(bool,String)=sqlx::query_as("SELECT archava_accounting.clearing_guards_complete(),archava_accounting.clearing_row_reserved('acme',$1)::text").bind(Uuid::parse_str(s.rows[0]["id"].as_str().unwrap()).unwrap()).fetch_one(&s.s.f.admin_pool).await.unwrap();
    assert!(check);
    assert_eq!(reserved, "2980000");
}

#[tokio::test]
async fn clearing_refund_and_supplier_payment_race_for_the_same_actual_withdrawal_capacity() {
    let s = Scenario::new(true).await;
    sqlx::query("INSERT INTO archava_purchasing.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','alice','human',$1,'all')")
        .bind(archava_domain::erp::purchasing::PERMISSION_VOCABULARY).execute(&s.s.f.admin_pool).await.unwrap();
    sqlx::query("UPDATE archava_commerce.memberships SET role='owner',capability='enterprise' WHERE tenant_id='acme' AND principal_id='alice'").execute(&s.s.f.admin_pool).await.unwrap();
    let mut accounts = Vec::new();
    for (code, class) in [
        ("INV", "asset"),
        ("GRNI", "liability"),
        ("AP", "liability"),
        ("FEE-ACCRUAL", "liability"),
    ] {
        accounts.push(account(&s.s.app, &s.s.alice, &s.s.book, code, class, "operating").await);
    }
    let mappings: Vec<_> = [
        "inventory",
        "goods_received_not_invoiced",
        "payable",
        "landed_fee_accrual",
    ]
    .into_iter()
    .zip(&accounts)
    .map(|(role, a)| {
        let mut value = bank::account_ref(a);
        value["role"] = json!(role);
        value
    })
    .collect();
    ok(&s.s.app,&s.s.alice,json!({"type":"purchase_policy.set","bookId":s.s.book["bookId"],"expectedRevision":0,"currencyCatalogue":"SIX-2026-09-17","sourceUnit":"whole_idr","mappings":mappings,"reason":"Actual supplier source shares the same company cash with a customer refund"})).await;
    async fn mutation(s: &Scenario, module: &str, command: Value) -> Value {
        let (st, v) = call(
            &s.s.app,
            "POST",
            &format!("/erp/{module}/commands"),
            input(command),
            Some(&s.s.alice),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        v["receipt"].clone()
    }
    let supplier=mutation(&s,"purchasing",json!({"operation":"supplier.create","code":"AP-CLEARING-RACE","name":"Actual supplier sharing the bank row"})).await;
    let product=mutation(&s,"commerce",json!({"operation":"product.create_for_company","companyId":s.s.company["companyId"],"expectedCompanyRevision":1,"sku":"AP-CLEARING-RACE","title":"Actual supplier goods for shared evidence","unit":"unit","trackStock":true,"price":15000})).await;
    let offer=mutation(&s,"purchasing",json!({"operation":"offer.create","supplierId":supplier["id"],"productId":product["id"],"unitCost":10000})).await;
    let order=mutation(&s,"purchasing",json!({"operation":"order.create-for-company","companyId":s.s.company["companyId"],"expectedCompanyRevision":1,"supplierId":supplier["id"],"lines":[{"offerId":offer["id"],"quantity":1}]})).await;
    mutation(&s,"purchasing",json!({"operation":"order.confirm","id":order["id"],"expectedRevision":1,"reason":"Capture actual original purchase policy"})).await;
    let line: Uuid = sqlx::query_scalar(
        "SELECT id FROM archava_purchasing.lines WHERE tenant_id='acme' AND order_id=$1",
    )
    .bind(Uuid::parse_str(order["id"].as_str().unwrap()).unwrap())
    .fetch_one(&s.s.f.admin_pool)
    .await
    .unwrap();
    let receipt=mutation(&s,"purchasing",json!({"operation":"order.receive","id":order["id"],"expectedRevision":2,"lines":[{"lineId":line,"accepted":1,"rejected":0}],"reason":"Actual accepted supplier stock"})).await;
    let doc=mutation(&s,"purchasing",json!({"operation":"document.capture","orderId":order["id"],"expectedOrderRevision":3,"supplierId":supplier["id"],"vendorReference":"AP-CLEARING-RACE","documentDate":s.date,"currency":"IDR","declaredTotal":10000,"lines":[{"lineId":line,"quantity":1,"unitCost":10000}]})).await;
    let bill=mutation(&s,"purchasing",json!({"operation":"document.match","id":doc["id"],"expectedRevision":1,"expectedOrderRevision":3,"reason":"Match actual bill before supplier payment"})).await;
    for (source, code, roles) in [(&receipt, "RECEIVED", [0, 1]), (&bill, "PAYABLE", [1, 2])] {
        let refs: Vec<_> = roles
            .into_iter()
            .map(|i| bank::account_ref(&accounts[i]))
            .collect();
        let j=ok(&s.s.app,&s.s.alice,json!({"type":"purchase_source.draft","sourceId":source["requestId"],"code":code,"accounts":refs,"reason":"Review actual shared-book source"})).await;
        s.post(&j).await;
    }
    let refund = s.s.payment(&s.invoice, "refunded", 10000).await;
    s.s.post_source(&refund, "REFUND-SOURCE").await;
    let payable = |code: &str, amount: &str| json!({"type":"payable_payment.draft","sourceId":bill["requestId"],"expectedDocumentRevision":2,"bankRowId":s.rows[1]["id"],"code":code,"payableAccount":bank::account_ref(&accounts[2]),"cashAccount":bank::account_ref(&s.cash),"amountMinor":amount,"reason":"Review a supplier payment sharing actual bank evidence"});
    let left = input(payable("SUPPLIER-RACE", "600000"));
    let right = input(s.settlement(&refund, 1, "CUSTOMER-REFUND-RACE", "600000", "0"));
    let (left, right) = tokio::join!(
        send(&s.s.app, &s.s.alice, &left),
        send(&s.s.app, &s.s.alice, &right)
    );
    let mut statuses = vec![left.0.as_u16(), right.0.as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 409], "AP={left:?} clearing={right:?}");
    let winner = if left.0 == StatusCode::OK {
        &left.1["receipt"]
    } else {
        &right.1["receipt"]
    };
    let posted = s.post(winner).await;
    let (st, matched) = send(
        &s.s.app,
        &s.s.alice,
        &bank::match_row(&s.rows[1], &posted, 2, &s.cash, "600000"),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{matched}");
    let available: String =
        sqlx::query_scalar("SELECT archava_accounting.bank_unreserved('acme',$1)::text")
            .bind(Uuid::parse_str(s.rows[1]["id"].as_str().unwrap()).unwrap())
            .fetch_one(&s.s.f.admin_pool)
            .await
            .unwrap();
    assert_eq!(available, "410000");
    let other = if left.0 == StatusCode::OK {
        s.settlement(&refund, 1, "OTHER-REFUND", "400000", "0")
    } else {
        payable("OTHER-SUPPLIER", "400000")
    };
    let other = ok(&s.s.app, &s.s.alice, other).await;
    s.post(&other).await;
    ok(&s.s.app,&s.s.alice,json!({"type":"bank_allocation.reverse","id":matched["receipt"]["resourceId"],"expectedRevision":1,"reason":"Restore original reservation without releasing posted cash capacity"})).await;
    let available: String =
        sqlx::query_scalar("SELECT archava_accounting.bank_unreserved('acme',$1)::text")
            .bind(Uuid::parse_str(s.rows[1]["id"].as_str().unwrap()).unwrap())
            .fetch_one(&s.s.f.admin_pool)
            .await
            .unwrap();
    assert_eq!(available, "10000");
}
