//! Tax on a supplier document, posted as input tax (TAX.md stage D). A claim
//! may carry an input tax code of its company's book and the tax amount the
//! supplier printed; the goods total stays the net of goods. The document
//! version stores the code version that applies on the document date, the
//! entered tax and the computed one. The bill source of a matched, taxed
//! document posts goods received not invoiced (net) and input tax against the
//! payable (gross); its void reverses all three. A claim without tax is
//! captured, matched and posted as before.
use super::purchase_sources::PurchaseScenario;
use super::tax_sales::guard_message;
use super::*;

async fn today(s: &PurchaseScenario) -> String {
    sqlx::query_scalar("SELECT (clock_timestamp() AT TIME ZONE 'UTC')::date::text")
        .fetch_one(&s.f.admin_pool)
        .await
        .unwrap()
}
/// A tax code version entered by alice and confirmed by bob (the fixture's
/// book requires a second person).
async fn entered_code(
    s: &PurchaseScenario,
    code: &str,
    kind: &str,
    rate: (i64, i64),
    account: &Value,
    from: &str,
) -> Value {
    ok(&s.app,&s.alice,json!({"type":"tax_code.create","bookId":s.book["bookId"],"code":code,"kind":kind,
        "rate":{"numerator":rate.0,"denominator":rate.1},
        "controlAccount":{"accountId":account["resourceId"],"expectedAccountRevision":account["revision"]},
        "effectiveFrom":from,"reason":"Entered by the company from its own tax advice"})).await
}
async fn confirmed_code(
    s: &PurchaseScenario,
    code: &str,
    kind: &str,
    rate: (i64, i64),
    account: &Value,
    from: &str,
) -> Value {
    let entered = entered_code(s, code, kind, rate, account, from).await;
    ok(&s.app,&s.bob,json!({"type":"tax_code.confirm","id":entered["resourceId"],"expectedRevision":1,"reason":"Checked against the company's tax advice"})).await
}
async fn input_account(s: &PurchaseScenario, code: &str) -> Value {
    account(&s.app, &s.alice, &s.book, code, "asset", "operating").await
}
/// A confirmed purchase order of `quantity` units at 7,000, all accepted
/// without a landed fee, with its receipt source posted (inventory against
/// goods received not invoiced).
async fn received(s: &PurchaseScenario, quantity: i64, code: &str) -> (Value, Uuid) {
    let po = s.draft_order(quantity).await;
    s.purchase(json!({"operation":"order.confirm","id":po["id"],"expectedRevision":1,"reason":"Confirm under the reviewed purchase policy"})).await;
    let line = s.order_line(&po).await;
    let receipt = s.purchase(json!({"operation":"order.receive","id":po["id"],"expectedRevision":2,"reason":"All goods accepted","lines":[{"lineId":line,"accepted":quantity,"rejected":0}]})).await;
    s.post(&s.reviewed(&receipt, code, &[0, 1]).await).await;
    (po, line)
}
/// `document.capture` for all `quantity` units of the order at 7,000, with or
/// without the two tax members.
fn claim(
    s: &PurchaseScenario,
    po: &Value,
    line: Uuid,
    reference: &str,
    date: &str,
    quantity: i64,
    tax: Option<(&str, i64)>,
) -> Value {
    let mut command = json!({"operation":"document.capture","orderId":po["id"],"expectedOrderRevision":3,"supplierId":s.supplier["id"],"vendorReference":reference,"documentDate":date,"currency":s.currency,"declaredTotal":quantity*7000,"lines":[{"lineId":line,"quantity":quantity,"unitCost":7000}]});
    if let Some((code, amount)) = tax {
        command["taxCode"] = json!(code);
        command["taxMinor"] = json!(amount);
    }
    command
}
async fn attempt(s: &PurchaseScenario, command: Value) -> (StatusCode, Value) {
    call(
        &s.app,
        "POST",
        "/erp/purchasing/commands",
        input(command),
        Some(&s.alice),
    )
    .await
}
fn matching(document: &Value, revision: i64) -> Value {
    json!({"operation":"document.match","id":document["id"],"expectedRevision":revision,"expectedOrderRevision":3,"reason":"Exact accepted-goods claim"})
}
fn voiding(document: &Value, revision: i64) -> Value {
    json!({"operation":"document.void","id":document["id"],"expectedRevision":revision,"reason":"Correct the original supplier claim"})
}
/// The purchasing workspace of the company: documents and input tax codes.
async fn workspace(s: &PurchaseScenario) -> Value {
    let (st, v) = call(
        &s.app,
        "GET",
        &format!(
            "/erp/purchasing/workspace?tenant=acme&companyId={}",
            s.company["companyId"].as_str().unwrap()
        ),
        Value::Null,
        Some(&s.alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v
}
async fn document_row(s: &PurchaseScenario, document: &Value) -> Value {
    workspace(s).await["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == document["id"])
        .cloned()
        .unwrap()
}
/// What the bridge stored for one purchasing command: effect, goods, tax and
/// the tax control account with its revision.
type Captured = (String, i64, i64, Option<String>, Option<i32>);
async fn captured(s: &PurchaseScenario, command: &Value) -> Captured {
    sqlx::query_as("SELECT effect,goods_minor,tax_minor,tax_account_id::text,tax_account_revision FROM archava_accounting.purchase_sources WHERE id=$1::uuid")
        .bind(command["requestId"].as_str().unwrap())
        .fetch_one(&s.f.admin_pool)
        .await
        .unwrap()
}
/// The captured source as the Accounting workspace lists it.
async fn listed(s: &PurchaseScenario, command: &Value) -> Value {
    let (st, v) = call(
        &s.app,
        "GET",
        &format!(
            "/erp/accounting/workspace?tenant=acme&companyId={}&collection=purchase_sources&id={}",
            s.company["companyId"].as_str().unwrap(),
            command["requestId"].as_str().unwrap()
        ),
        Value::Null,
        Some(&s.alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["purchase_sources"][0].clone()
}
fn reference(account: &Value) -> Value {
    json!({"accountId":account["resourceId"],"expectedAccountRevision":account["revision"]})
}
/// `purchase_source.draft` naming exactly these reviewed accounts.
fn draft_with(source: &Value, code: &str, accounts: &[&Value]) -> Value {
    let accounts: Vec<Value> = accounts.iter().map(|a| reference(a)).collect();
    input(
        json!({"type":"purchase_source.draft","sourceId":source["requestId"],"code":code,"accounts":accounts,"reason":"Review captured goods, tax, source date and account roles"}),
    )
}
/// Drafts a taxed bill or void with its three accounts and has bob post it.
async fn post_taxed(
    s: &PurchaseScenario,
    source: &Value,
    code: &str,
    tax_account: &Value,
) -> Value {
    let request = draft_with(source, code, &[&s.accounts[1], tax_account, &s.accounts[2]]);
    let (st, v) = send(&s.app, &s.alice, &request).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    s.post(&v["receipt"]).await;
    v["receipt"].clone()
}
/// The lines of one journal: account code, debit, credit, in order.
async fn lines(s: &PurchaseScenario, journal: &Value) -> Vec<(String, i64, i64)> {
    sqlx::query_as("SELECT account_code,debit_minor,credit_minor FROM archava_accounting.journal_lines WHERE journal_id=$1::uuid ORDER BY ordinal")
        .bind(journal["resourceId"].as_str().unwrap())
        .fetch_all(&s.f.admin_pool)
        .await
        .unwrap()
}
fn row(code: &str, debit: i64, credit: i64) -> (String, i64, i64) {
    (code.to_owned(), debit, credit)
}
async fn whole_report(s: &PurchaseScenario) -> Value {
    let (st, v) = call(
        &s.app,
        "GET",
        &format!(
            "/erp/accounting/reports?tenant=acme&bookId={}&from=2000-01-01&to=2100-12-31",
            s.book["bookId"].as_str().unwrap()
        ),
        Value::Null,
        Some(&s.alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v
}
/// Closing debit less credit of one account in the report's trial balance.
fn closing(report: &Value, code: &str) -> String {
    report["trialBalance"]["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["code"] == code)
        .map(|a| a["closingNetMinor"].as_str().unwrap().to_owned())
        .unwrap_or_else(|| "0".into())
}
async fn payable_report(s: &PurchaseScenario) -> Value {
    subledger::accepted(&s.app, &s.alice, &s.book, "payable", "2100-12-31").await
}
async fn close_blockers(s: &PurchaseScenario) -> (bool, Vec<(String, i64)>) {
    let (st, v) = call(
        &s.app,
        "GET",
        &format!(
            "/erp/accounting/close-readiness?tenant=acme&periodId={}",
            s.period["resourceId"].as_str().unwrap()
        ),
        Value::Null,
        Some(&s.alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    (
        v["ready"].as_bool().unwrap(),
        v["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| {
                (
                    b["reason"].as_str().unwrap().to_owned(),
                    b["count"].as_i64().unwrap(),
                )
            })
            .collect(),
    )
}
async fn count(s: &PurchaseScenario, table: &str) -> i64 {
    let statement = format!("SELECT count(*) FROM {table}");
    sqlx::query_scalar(sqlx::AssertSqlSafe(statement.as_str()))
        .fetch_one(&s.f.admin_pool)
        .await
        .unwrap()
}

/// **Witness: on an IDR book with a `whole_idr` policy, a supplier claim for
/// goods of 140,000 with an 11% input code and the supplier's tax of 15,400 is
/// stored with the code version, the entered and the computed tax; matching is
/// decided on goods alone; the bill source is captured as goods 14,000,000 and
/// tax 1,540,000 on the tax code's control account; the draft needs exactly
/// the three accounts and is posted by a second person; goods received not
/// invoiced is cleared by the net, input tax is +1,540,000 and the payable
/// −15,540,000; the trial balance balances, the payable control comparison
/// has no difference and aging shows the gross. The unposted taxed bill blocks
/// the period close. A supplier payment above the gross is refused and one in
/// full of the gross clears the payable. The bridge guards are complete after
/// the functions were replaced, with unchanged signatures.**
#[tokio::test]
async fn a_taxed_supplier_bill_posts_goods_input_tax_and_a_gross_payable_and_is_paid_in_gross() {
    let s = PurchaseScenario::new(true).await;
    // The replaced functions kept their identity: the readiness checks that
    // compare each trigger's function by OID hold through the serving role,
    // and the host of this fixture started on them. No overload was created.
    assert!(sqlx::query_scalar::<_, bool>(
        "SELECT archava_accounting.purchase_guards_complete() AND archava_accounting.payable_guards_complete()"
    )
    .fetch_one(&s.f.runtime_pool)
    .await
    .unwrap());
    let _: Router = app(&s.f).await;
    let signatures: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT p.proname::text,pg_get_function_identity_arguments(p.oid),pg_get_function_result(p.oid) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace \
         WHERE (n.nspname='archava_accounting' AND p.proname IN ('purchase_effect','purchase_effect_tax','guard_purchase_source','capture_purchase_source','purchase_legs','guard_purchase_journal_source','payable_balance')) \
            OR (n.nspname='archava_purchasing' AND p.proname IN ('guard_supplier_document','guard_supplier_document_tax','assert_supplier_document')) ORDER BY 1",
    )
    .fetch_all(&s.f.admin_pool)
    .await
    .unwrap();
    let own = |a: &str, b: &str, c: &str| (a.to_owned(), b.to_owned(), c.to_owned());
    assert_eq!(
        signatures,
        vec![
            own("assert_supplier_document", "", "trigger"),
            own("capture_purchase_source", "", "trigger"),
            own("guard_purchase_journal_source", "", "trigger"),
            own("guard_purchase_source", "", "trigger"),
            own("guard_supplier_document", "", "trigger"),
            own("guard_supplier_document_tax", "", "trigger"),
            own(
                "payable_balance",
                "wanted_tenant text, wanted_source uuid, excluded uuid",
                "jsonb"
            ),
            own(
                "purchase_effect",
                "wanted_tenant text, wanted_request uuid",
                "jsonb"
            ),
            own(
                "purchase_effect_tax",
                "wanted_tenant text, wanted_request uuid",
                "TABLE(tax numeric, tax_code_id uuid, bill_request uuid)"
            ),
            own(
                "purchase_legs",
                "wanted_tenant text, wanted_source uuid",
                "TABLE(role text, debit_minor bigint, credit_minor bigint)"
            ),
        ]
    );

    let date = today(&s).await;
    let vat_account = input_account(&s, "VAT-IN").await;
    let vat = confirmed_code(&s, "VAT-IN", "input", (11, 100), &vat_account, "2020-01-01").await;
    // The supplier documents screen reads the company's input codes from the
    // Purchasing workspace: code, version, rate, base and range, no account
    // and no person.
    let read = workspace(&s).await;
    assert_eq!(
        read["taxCodes"],
        json!([{"code":"VAT-IN","version":1,"rate":{"numerator":11,"denominator":100},"base":{"numerator":1,"denominator":1},
            "effectiveFrom":"2020-01-01","effectiveTo":null,"enteredByCompany":true,"verifiedByArchava":false}])
    );
    assert!(read["taxNotice"]
        .as_str()
        .unwrap()
        .contains("Archava does not verify tax rates or rules"));

    let (po, line) = received(&s, 20, "RECEIPT").await;
    let document = s
        .purchase(claim(
            &s,
            &po,
            line,
            "TAXED-BILL",
            &date,
            20,
            Some(("VAT-IN", 15_400)),
        ))
        .await;
    let shown = document_row(&s, &document).await;
    assert_eq!(shown["declaredTotal"], 140_000, "the goods total stays net");
    assert_eq!(
        shown["tax"],
        json!({"codeId":vat["resourceId"],"code":"VAT-IN","version":1,"kind":"input",
            "rate":{"numerator":11,"denominator":100},"base":{"numerator":1,"denominator":1},
            "enteredMinor":15_400,"computedMinor":15_400,"differenceMinor":0,"totalMinor":155_400,
            "enteredByCompany":true,"verifiedByArchava":false})
    );
    // Capturing a claim posts nothing.
    assert_eq!(
        count(&s, "archava_accounting.purchase_sources").await,
        1,
        "the receipt only"
    );

    // Matching compares goods quantities and net amounts; tax plays no part.
    let bill = s.purchase(matching(&document, 1)).await;
    let matched = document_row(&s, &document).await;
    assert_eq!(matched["state"], "matched");
    assert_eq!(matched["matchEvidence"]["declaredTotal"], 140_000);
    assert_eq!(matched["matchEvidence"]["computedTotal"], 140_000);
    assert_eq!(matched["tax"], shown["tax"]);
    let account_id = vat_account["resourceId"].as_str().unwrap().to_owned();
    assert_eq!(
        captured(&s, &bill).await,
        (
            "bill".into(),
            14_000_000,
            1_540_000,
            Some(account_id.clone()),
            Some(1)
        )
    );
    let source = listed(&s, &bill).await;
    assert_eq!(source["goodsMinor"], "14000000");
    assert_eq!(source["taxMinor"], "1540000");
    assert_eq!(source["taxAccountId"], vat_account["resourceId"]);
    assert_eq!(source["taxAccountRevision"], 1);
    assert_eq!(source["sourceState"], "pending");
    // The payable is not recognised before the bill is posted, and the
    // unposted taxed bill blocks the close.
    assert_eq!(
        close_blockers(&s).await,
        (false, vec![("unposted_purchase_sources".to_owned(), 1)])
    );

    // The draft names exactly goods received not invoiced, the tax control
    // account and the payable. Two accounts, or a wrong third one, are refused
    // with the reason and nothing is written.
    let journals = count(&s, "archava_accounting.journals").await;
    for accounts in [
        vec![&s.accounts[1], &s.accounts[2]],
        vec![&s.accounts[1], &s.accounts[0], &s.accounts[2]],
        vec![&s.accounts[1], &vat_account],
    ] {
        let (st, v) = send(&s.app, &s.alice, &draft_with(&bill, "BILL", &accounts)).await;
        assert_refused_as(st, &v, "purchase_source_tax_accounts_required");
    }
    assert_eq!(count(&s, "archava_accounting.journals").await, journals);
    let journal = post_taxed(&s, &bill, "BILL", &vat_account).await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![
            row("GRNI", 14_000_000, 0),
            row("VAT-IN", 1_540_000, 0),
            row("PAYABLE", 0, 15_540_000)
        ]
    );
    assert_eq!(listed(&s, &bill).await["sourceState"], "posted");

    let report = whole_report(&s).await;
    assert_eq!(report["trialBalance"]["balanced"], true);
    assert_eq!(closing(&report, "INVENTORY"), "14000000");
    assert_eq!(closing(&report, "GRNI"), "0", "cleared by the net of goods");
    assert_eq!(closing(&report, "VAT-IN"), "1540000");
    assert_eq!(closing(&report, "PAYABLE"), "-15540000");
    assert_eq!(report["sourceCoverage"]["capturedCount"], 2);
    assert_eq!(report["sourceCoverage"]["postedCount"], 2);
    assert_eq!(close_blockers(&s).await, (true, vec![]));

    // The payable subledger is in gross: the control comparison has no
    // difference and aging shows what is owed to the supplier.
    let payable = payable_report(&s).await;
    assert_eq!(payable["postedSourceControlsAgree"], true, "{payable}");
    assert_eq!(payable["positionControlDifferenceMinor"], "0");
    let control = payable["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["code"] == "PAYABLE")
        .unwrap();
    assert_eq!(control["expectedMinor"], "15540000");
    assert_eq!(control["actualMinor"], "15540000");
    assert_eq!(control["differenceMinor"], "0");
    assert_eq!(payable["positions"][0]["balanceMinor"], "15540000");
    assert_eq!(payable["totals"]["netOutstandingMinor"], "15540000");

    // Supplier payment through the existing flow: the limit is the gross.
    let cash = account(&s.app, &s.alice, &s.book, "BANK", "asset", "cash").await;
    let binding = bank::bank_account(&s.app, &s.alice, &s.book, &cash).await;
    ok(&s.app,&s.alice,json!({"type":"bank_statement.import","bankAccountId":binding["resourceId"],"account":bank::account_ref(&cash),"statement":{"reference":"TAXED-SUPPLIER-MOVEMENTS","fromDate":date,"toDate":date,"currency":"IDR","openingMinor":"40000000","closingMinor":"8919999","rows":[
        {"externalRef":"OVER","date":date,"amountMinor":"-15540001","description":"One unit above the gross"},
        {"externalRef":"FULL","date":date,"amountMinor":"-15540000","description":"The gross of the taxed bill"}]},"reason":"Review actual synthetic bank statement evidence"})).await;
    let (_, data) = bank::bank_workspace(&s.app, &s.alice, &s.company, "bank_rows").await;
    let bank_row = |external: &str| {
        data["bank_rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["externalRef"] == external)
            .unwrap()["id"]
            .clone()
    };
    let payment = |code: &str, bank_row: Value, amount: &str| json!({"type":"payable_payment.draft","sourceId":bill["requestId"],"expectedDocumentRevision":2,"bankRowId":bank_row,"code":code,"payableAccount":bank::account_ref(&s.accounts[2]),"cashAccount":bank::account_ref(&cash),"amountMinor":amount,"reason":"Review actual supplier bank withdrawal and original bill"});
    let (_, bills) = bank::bank_workspace(&s.app, &s.alice, &s.company, "payables").await;
    assert_eq!(bills["payables"][0]["balance"]["originalMinor"], "15540000");
    assert_eq!(bills["payablesSummary"]["recognizedMinor"], "15540000");
    assert_eq!(bills["payablesSummary"]["availableMinor"], "15540000");
    let (st, v) = send(
        &s.app,
        &s.alice,
        &input(payment("PAY-OVER", bank_row("OVER"), "15540001")),
    )
    .await;
    assert_refused_as(st, &v, "supplier_payment_exceeds_bill");
    let paid = ok(
        &s.app,
        &s.alice,
        payment("PAY-FULL", bank_row("FULL"), "15540000"),
    )
    .await;
    s.post(&paid).await;
    let (_, bills) = bank::bank_workspace(&s.app, &s.alice, &s.company, "payables").await;
    assert_eq!(bills["payablesSummary"]["postedPaymentMinor"], "15540000");
    assert_eq!(bills["payablesSummary"]["outstandingMinor"], "0");
    let report = whole_report(&s).await;
    assert_eq!(report["trialBalance"]["balanced"], true);
    assert_eq!(closing(&report, "PAYABLE"), "0");
    assert_eq!(closing(&report, "VAT-IN"), "1540000");
    assert_eq!(closing(&report, "BANK"), "-15540000");
    let payable = payable_report(&s).await;
    assert_eq!(payable["postedSourceControlsAgree"], true, "{payable}");
    assert_eq!(payable["totals"]["netOutstandingMinor"], "0");
    // A paid bill cannot be voided, taxed or not.
    let (st, v) = attempt(&s, voiding(&document, 2)).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
}

/// **Witness: an entered tax that differs from the computed one is accepted
/// and is the amount posted; the read shows computed, entered and the
/// difference. A document with a 0% code and tax 0 keeps its code's account on
/// the source and posts with two legs. A claim without tax in the same book is
/// captured, listed and drafted exactly as before.**
#[tokio::test]
async fn the_entered_tax_is_posted_beside_the_computed_one_and_zero_tax_has_no_tax_leg() {
    let s = PurchaseScenario::new(true).await;
    let date = today(&s).await;
    let vat_account = input_account(&s, "VAT-IN").await;
    let zero_account = input_account(&s, "VAT-ZERO").await;
    confirmed_code(&s, "VAT-IN", "input", (11, 100), &vat_account, "2020-01-01").await;
    confirmed_code(&s, "ZERO", "input", (0, 1), &zero_account, "2020-01-01").await;

    // The supplier printed 15,399; Archava computes 15,400.
    let (po, line) = received(&s, 20, "RECEIPT-A").await;
    let document = s
        .purchase(claim(
            &s,
            &po,
            line,
            "PRINTED-LESS",
            &date,
            20,
            Some(("VAT-IN", 15_399)),
        ))
        .await;
    let shown = document_row(&s, &document).await;
    assert_eq!(shown["tax"]["computedMinor"], 15_400);
    assert_eq!(shown["tax"]["enteredMinor"], 15_399);
    assert_eq!(shown["tax"]["differenceMinor"], -1);
    assert_eq!(shown["tax"]["totalMinor"], 155_399);
    let stored: (i64, i64) = sqlx::query_as("SELECT tax_minor,tax_computed_minor FROM archava_purchasing.supplier_documents WHERE id=$1::uuid")
        .bind(document["id"].as_str().unwrap())
        .fetch_one(&s.f.admin_pool)
        .await
        .unwrap();
    assert_eq!(stored, (15_399, 15_400));
    let bill = s.purchase(matching(&document, 1)).await;
    assert_eq!(captured(&s, &bill).await.2, 1_539_900, "the entered amount");
    let journal = post_taxed(&s, &bill, "BILL-A", &vat_account).await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![
            row("GRNI", 14_000_000, 0),
            row("VAT-IN", 1_539_900, 0),
            row("PAYABLE", 0, 15_539_900)
        ]
    );

    // A 0% code: the document has a code and tax 0. The source keeps the
    // code's control account and has no tax line.
    let (po, line) = received(&s, 2, "RECEIPT-B").await;
    let document = s
        .purchase(claim(
            &s,
            &po,
            line,
            "ZERO-RATED",
            &date,
            2,
            Some(("ZERO", 0)),
        ))
        .await;
    let shown = document_row(&s, &document).await;
    assert_eq!(shown["tax"]["code"], "ZERO");
    assert_eq!(shown["tax"]["computedMinor"], 0);
    assert_eq!(shown["tax"]["enteredMinor"], 0);
    assert_eq!(shown["tax"]["totalMinor"], 14_000);
    let bill = s.purchase(matching(&document, 1)).await;
    assert_eq!(
        captured(&s, &bill).await,
        (
            "bill".into(),
            1_400_000,
            0,
            Some(zero_account["resourceId"].as_str().unwrap().to_owned()),
            Some(1)
        )
    );
    let (st, v) = send(
        &s.app,
        &s.alice,
        &draft_with(
            &bill,
            "BILL-B3",
            &[&s.accounts[1], &zero_account, &s.accounts[2]],
        ),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v.get("detail").is_none(), "a plain conflict: {v}");
    let journal = s.reviewed(&bill, "BILL-B", &[1, 2]).await;
    s.post(&journal).await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![row("GRNI", 1_400_000, 0), row("PAYABLE", 0, 1_400_000)]
    );

    // No tax members: nothing about the claim, its source or its draft changes.
    let (po, line) = received(&s, 2, "RECEIPT-C").await;
    let document = s
        .purchase(claim(&s, &po, line, "UNTAXED", &date, 2, None))
        .await;
    let shown = document_row(&s, &document).await;
    assert!(shown.get("tax").is_none(), "{shown}");
    assert!(shown["versions"][0].get("tax").is_none(), "{shown}");
    let bill = s.purchase(matching(&document, 1)).await;
    assert_eq!(
        captured(&s, &bill).await,
        ("bill".into(), 1_400_000, 0, None, None)
    );
    let source = listed(&s, &bill).await;
    for key in ["taxMinor", "taxAccountId", "taxAccountRevision"] {
        assert!(source.get(key).is_none(), "{key}: {source}");
    }
    let journal = s.reviewed(&bill, "BILL-C", &[1, 2]).await;
    s.post(&journal).await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![row("GRNI", 1_400_000, 0), row("PAYABLE", 0, 1_400_000)]
    );

    let report = whole_report(&s).await;
    assert_eq!(report["trialBalance"]["balanced"], true);
    assert_eq!(closing(&report, "GRNI"), "0");
    assert_eq!(closing(&report, "VAT-IN"), "1539900");
    assert_eq!(closing(&report, "VAT-ZERO"), "0");
    assert_eq!(closing(&report, "PAYABLE"), "-18339900");
    let payable = payable_report(&s).await;
    assert_eq!(payable["postedSourceControlsAgree"], true, "{payable}");
    assert_eq!(payable["totals"]["netOutstandingMinor"], "18339900");
}

/// **Witness: on a USD book under a `minor` policy a 10.75% code on goods of
/// USD 70.00 computes 7.525, rounded once half away from zero to 7.53; the
/// source is captured in minor units unchanged (7000 and 753) and posts with
/// three legs; the payable control reconciles in gross.**
#[tokio::test]
async fn a_usd_supplier_bill_posts_its_tax_in_minor_units_with_one_rounding() {
    let s = PurchaseScenario::new_in("USD", true).await;
    let date = today(&s).await;
    let tax_account = input_account(&s, "GST-IN").await;
    confirmed_code(&s, "GST", "input", (43, 400), &tax_account, "2020-01-01").await;
    let (po, line) = received(&s, 1, "RECEIPT").await;
    // The supplier rounded down to 7.52.
    let document = s
        .purchase(claim(
            &s,
            &po,
            line,
            "USD-TAXED",
            &date,
            1,
            Some(("GST", 752)),
        ))
        .await;
    let shown = document_row(&s, &document).await;
    assert_eq!(shown["currency"], "USD");
    assert_eq!(shown["declaredTotal"], 7_000);
    assert_eq!(shown["tax"]["computedMinor"], 753, "752.5 rounds half away");
    assert_eq!(shown["tax"]["enteredMinor"], 752);
    assert_eq!(shown["tax"]["differenceMinor"], -1);
    assert_eq!(shown["tax"]["totalMinor"], 7_752);
    let bill = s.purchase(matching(&document, 1)).await;
    assert_eq!(
        captured(&s, &bill).await,
        (
            "bill".into(),
            7_000,
            752,
            Some(tax_account["resourceId"].as_str().unwrap().to_owned()),
            Some(1)
        ),
        "not multiplied by 100"
    );
    let journal = post_taxed(&s, &bill, "BILL", &tax_account).await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![
            row("GRNI", 7_000, 0),
            row("GST-IN", 752, 0),
            row("PAYABLE", 0, 7_752)
        ]
    );
    let report = whole_report(&s).await;
    assert_eq!(report["trialBalance"]["balanced"], true);
    assert_eq!(closing(&report, "GRNI"), "0");
    assert_eq!(closing(&report, "GST-IN"), "752");
    assert_eq!(closing(&report, "PAYABLE"), "-7752");
    let payable = payable_report(&s).await;
    assert_eq!(payable["book"]["currency"], "USD");
    assert_eq!(payable["postedSourceControlsAgree"], true, "{payable}");
    assert_eq!(payable["positions"][0]["balanceMinor"], "7752");
}

/// **Witness: a void of a taxed bill is captured with the negated goods and
/// tax on the bill's tax account and reverses all three legs. A corrected
/// version may change the tax amount and the code; each version keeps its own
/// snapshot, and the new bill posts the new tax. After the last void the
/// ledger is exactly zero on input tax and the payable, and goods received
/// not invoiced holds only the receipt. A corrected version may also drop the
/// tax.**
#[tokio::test]
async fn a_correction_changes_the_tax_and_a_void_reverses_goods_tax_and_payable_to_zero() {
    let s = PurchaseScenario::new(true).await;
    let date = today(&s).await;
    let vat_account = input_account(&s, "VAT-IN").await;
    let other_account = input_account(&s, "VAT-OTHER").await;
    confirmed_code(&s, "VAT-IN", "input", (11, 100), &vat_account, "2020-01-01").await;
    confirmed_code(
        &s,
        "VAT-12",
        "input",
        (12, 100),
        &other_account,
        "2020-01-01",
    )
    .await;
    let vat_id = vat_account["resourceId"].as_str().unwrap().to_owned();
    let other_id = other_account["resourceId"].as_str().unwrap().to_owned();

    let (po, line) = received(&s, 20, "RECEIPT").await;
    let document = s
        .purchase(claim(
            &s,
            &po,
            line,
            "CORRECTED",
            &date,
            20,
            Some(("VAT-IN", 15_400)),
        ))
        .await;
    let first = s.purchase(matching(&document, 1)).await;
    post_taxed(&s, &first, "BILL-1", &vat_account).await;

    // The void of the posted bill: negated goods and tax, the bill's account.
    let void = s.purchase(voiding(&document, 2)).await;
    assert_eq!(
        captured(&s, &void).await,
        (
            "void".into(),
            -14_000_000,
            -1_540_000,
            Some(vat_id.clone()),
            Some(1)
        )
    );
    let (st, v) = send(
        &s.app,
        &s.alice,
        &draft_with(&void, "VOID-1", &[&s.accounts[1], &s.accounts[2]]),
    )
    .await;
    assert_refused_as(st, &v, "purchase_source_tax_accounts_required");
    let journal = post_taxed(&s, &void, "VOID-1", &vat_account).await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![
            row("PAYABLE", 15_540_000, 0),
            row("GRNI", 0, 14_000_000),
            row("VAT-IN", 0, 1_540_000)
        ]
    );
    let report = whole_report(&s).await;
    assert_eq!(closing(&report, "GRNI"), "-14000000", "the receipt only");
    assert_eq!(closing(&report, "VAT-IN"), "0");
    assert_eq!(closing(&report, "PAYABLE"), "0");
    let payable = payable_report(&s).await;
    assert_eq!(payable["postedSourceControlsAgree"], true, "{payable}");
    assert_eq!(payable["totals"]["netOutstandingMinor"], "0");

    // The corrected claim: another code and the amount the supplier reissued.
    let revised = s.purchase(json!({"operation":"document.revise","id":document["id"],"expectedRevision":3,"expectedOrderRevision":3,"documentDate":date,"currency":"IDR","declaredTotal":140_000,"lines":[{"lineId":line,"quantity":20,"unitCost":7000}],"reason":"Supplier reissued the document with another tax","taxCode":"VAT-12","taxMinor":16_799})).await;
    assert_eq!(revised["revision"], 4);
    let shown = document_row(&s, &document).await;
    assert_eq!(shown["tax"]["code"], "VAT-12");
    assert_eq!(shown["tax"]["computedMinor"], 16_800);
    assert_eq!(shown["tax"]["enteredMinor"], 16_799);
    assert_eq!(shown["tax"]["differenceMinor"], -1);
    // Every version keeps the tax it was captured with.
    let versions: Vec<(i32, String, Option<String>, Option<i64>)> = sqlx::query_as("SELECT revision,snapshot->>'state',snapshot->>'tax_code',(snapshot->>'tax_minor')::bigint FROM archava_purchasing.supplier_document_versions WHERE document_id=$1::uuid ORDER BY revision")
        .bind(document["id"].as_str().unwrap())
        .fetch_all(&s.f.admin_pool)
        .await
        .unwrap();
    let version = |revision: i32, state: &str, code: &str, tax: i64| {
        (revision, state.to_owned(), Some(code.to_owned()), Some(tax))
    };
    assert_eq!(
        versions,
        vec![
            version(1, "captured", "VAT-IN", 15_400),
            version(2, "matched", "VAT-IN", 15_400),
            version(3, "void", "VAT-IN", 15_400),
            version(4, "captured", "VAT-12", 16_799),
        ]
    );
    assert_eq!(shown["versions"][0]["tax"]["code"], "VAT-12");
    assert_eq!(shown["versions"][0]["tax"]["enteredMinor"], 16_799);
    assert_eq!(shown["versions"][1]["tax"]["code"], "VAT-IN");
    assert_eq!(shown["versions"][1]["tax"]["enteredMinor"], 15_400);

    let second = s.purchase(matching(&document, 4)).await;
    assert_eq!(
        captured(&s, &second).await,
        (
            "bill".into(),
            14_000_000,
            1_679_900,
            Some(other_id.clone()),
            Some(1)
        )
    );
    let journal = post_taxed(&s, &second, "BILL-2", &other_account).await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![
            row("GRNI", 14_000_000, 0),
            row("VAT-OTHER", 1_679_900, 0),
            row("PAYABLE", 0, 15_679_900)
        ]
    );
    // The first bill is recognised at zero after its posted void; the second
    // is owed in gross.
    let (_, bills) = bank::bank_workspace(&s.app, &s.alice, &s.company, "payables").await;
    assert_eq!(bills["payables"].as_array().unwrap().len(), 2);
    assert_eq!(bills["payablesSummary"]["recognizedMinor"], "15679900");
    assert_eq!(bills["payablesSummary"]["outstandingMinor"], "15679900");

    // The second void takes the second bill's account and returns everything
    // the bills posted to exactly zero.
    let void = s.purchase(voiding(&document, 5)).await;
    assert_eq!(
        captured(&s, &void).await,
        (
            "void".into(),
            -14_000_000,
            -1_679_900,
            Some(other_id),
            Some(1)
        )
    );
    post_taxed(&s, &void, "VOID-2", &other_account).await;
    let report = whole_report(&s).await;
    assert_eq!(report["trialBalance"]["balanced"], true);
    assert_eq!(closing(&report, "INVENTORY"), "14000000");
    assert_eq!(closing(&report, "GRNI"), "-14000000", "the receipt only");
    assert_eq!(closing(&report, "VAT-IN"), "0");
    assert_eq!(closing(&report, "VAT-OTHER"), "0");
    assert_eq!(closing(&report, "PAYABLE"), "0");
    let (_, bills) = bank::bank_workspace(&s.app, &s.alice, &s.company, "payables").await;
    assert_eq!(bills["payablesSummary"]["recognizedMinor"], "0");
    assert_eq!(bills["payablesSummary"]["outstandingMinor"], "0");
    assert_eq!(close_blockers(&s).await, (true, vec![]));

    // A corrected version without the tax members is a claim without tax.
    s.purchase(json!({"operation":"document.revise","id":document["id"],"expectedRevision":6,"expectedOrderRevision":3,"documentDate":date,"currency":"IDR","declaredTotal":140_000,"lines":[{"lineId":line,"quantity":20,"unitCost":7000}],"reason":"The supplier's document carries no tax after all"})).await;
    let shown = document_row(&s, &document).await;
    assert!(shown.get("tax").is_none(), "{shown}");
    let plain = s.purchase(matching(&document, 7)).await;
    assert_eq!(
        captured(&s, &plain).await,
        ("bill".into(), 14_000_000, 0, None, None)
    );
}

/// One direct change of a supplier document as the serving role, rolled back.
async fn direct_document(
    s: &PurchaseScenario,
    document: &Value,
    assignment: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = archava_adapters::rls::TenantTransaction::begin(
        &s.f.runtime_pool,
        "archava_purchasing",
        "purchasing",
        archava_core::tenant::TenantId::new("acme").unwrap(),
    )
    .await
    .unwrap();
    let statement = format!(
        "UPDATE archava_purchasing.supplier_documents SET {assignment},revision=revision+1 WHERE id=$1::uuid"
    );
    let outcome = sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
        .bind(document["id"].as_str().unwrap())
        .execute(tx.checked_executor().await.unwrap())
        .await;
    tx.rollback().await.unwrap();
    outcome.map(|done| assert_eq!(done.rows_affected(), 1))
}
/// One direct insert of a purchase source as the serving role, copying a
/// captured source with the stated tax and tax account. The guard runs before
/// the primary key is checked, so a row the guard accepts ends as a duplicate.
async fn direct_source(
    s: &PurchaseScenario,
    command: &Value,
    tax: i64,
    account: Option<&str>,
) -> Result<(), sqlx::Error> {
    let mut tx = archava_adapters::rls::TenantTransaction::begin(
        &s.f.runtime_pool,
        "archava_accounting",
        "accounting",
        archava_core::tenant::TenantId::new("acme").unwrap(),
    )
    .await
    .unwrap();
    let outcome = sqlx::query("INSERT INTO archava_accounting.purchase_sources(tenant_id,id,book_id,origin_id,order_id,document_id,source_revision,effect,goods_minor,landed_fee_minor,source_snapshot,actor_id,occurred_at,source_date,tax_minor,tax_account_id,tax_account_revision) \
        SELECT tenant_id,id,book_id,origin_id,order_id,document_id,source_revision,effect,goods_minor,landed_fee_minor,source_snapshot,actor_id,occurred_at,source_date,$2,$3::uuid,CASE WHEN $3 IS NULL THEN NULL ELSE 1 END \
        FROM archava_accounting.purchase_sources WHERE id=$1::uuid")
        .bind(command["requestId"].as_str().unwrap())
        .bind(tax)
        .bind(account)
        .execute(tx.checked_executor().await.unwrap())
        .await;
    tx.rollback().await.unwrap();
    outcome.map(|_| ())
}

/// **Witness: a claim is refused with a specific reason, and nothing is
/// written, for an output code, an unconfirmed code, an unknown code, a
/// document date no confirmed version applies on, one tax member without the
/// other and an entered tax above the goods total. Matching a taxed claim is
/// refused while the tax control account is inactive and released by
/// reactivating it, and refused when that account is an account of the
/// purchase policy. The database, not the adapter, is the authority: as the
/// serving role a document whose snapshot, computed tax or bound disagrees
/// with its tax code cannot be stored, and a source whose tax or tax account
/// disagrees with its document cannot be inserted.**
#[tokio::test]
async fn taxed_claims_and_bills_are_refused_with_reasons_and_the_database_holds_the_rules() {
    let s = PurchaseScenario::new(true).await;
    let date = today(&s).await;
    let vat_account = input_account(&s, "VAT-IN").await;
    let spare = input_account(&s, "VAT-SPARE").await;
    let output = account(
        &s.app,
        &s.alice,
        &s.book,
        "VAT-OUT",
        "liability",
        "operating",
    )
    .await;
    confirmed_code(&s, "VAT-IN", "input", (11, 100), &vat_account, "2020-01-01").await;
    confirmed_code(&s, "SPARE", "input", (11, 100), &spare, "2020-01-01").await;
    confirmed_code(&s, "SALES", "output", (11, 100), &output, "2020-01-01").await;
    entered_code(
        &s,
        "PENDING",
        "input",
        (11, 100),
        &vat_account,
        "2020-01-01",
    )
    .await;
    // An input code may use any operating asset account, also the inventory
    // account of the purchase policy.
    confirmed_code(
        &s,
        "STOCK",
        "input",
        (11, 100),
        &s.accounts[0],
        "2020-01-01",
    )
    .await;

    let (po, line) = received(&s, 20, "RECEIPT").await;
    let documents = count(&s, "archava_purchasing.supplier_documents").await;
    let commands = count(&s, "archava_purchasing.commands").await;
    let taxed = |code: &'static str, amount: i64, day: &str| {
        claim(&s, &po, line, "REFUSED", day, 20, Some((code, amount)))
    };
    for (command, reason) in [
        (
            taxed("SALES", 15_400, &date),
            "supplier_document_tax_code_not_input",
        ),
        (
            taxed("PENDING", 15_400, &date),
            "supplier_document_tax_code_not_confirmed",
        ),
        (
            taxed("NOPE", 15_400, &date),
            "supplier_document_tax_code_unknown",
        ),
        (
            taxed("VAT-IN", 15_400, "2019-12-31"),
            "supplier_document_tax_code_not_applicable",
        ),
        (
            taxed("VAT-IN", 140_001, &date),
            "supplier_document_tax_amount_out_of_bounds",
        ),
    ] {
        let (st, v) = attempt(&s, command).await;
        assert_refused_as(st, &v, reason);
    }
    for missing in ["taxCode", "taxMinor"] {
        let mut command = taxed("VAT-IN", 15_400, &date);
        command.as_object_mut().unwrap().remove(missing);
        let (st, v) = attempt(&s, command).await;
        assert_refused_as(st, &v, "supplier_document_tax_code_and_amount_together");
    }
    // A negative amount or a rate in the body is not a command at all.
    let (st, v) = attempt(&s, taxed("VAT-IN", -1, &date)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    let mut forged = taxed("VAT-IN", 15_400, &date);
    forged["taxRate"] = json!({"numerator":1,"denominator":100});
    let (st, v) = attempt(&s, forged).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(
        count(&s, "archava_purchasing.supplier_documents").await,
        documents
    );
    assert_eq!(count(&s, "archava_purchasing.commands").await, commands);
    // The whole goods total as tax is the bound and is accepted.
    let bound = s
        .purchase(claim(
            &s,
            &po,
            line,
            "AT-THE-BOUND",
            &date,
            20,
            Some(("VAT-IN", 140_000)),
        ))
        .await;
    s.purchase(voiding(&bound, 1)).await;

    // The control account of SPARE is deactivated after the claim was
    // captured. Matching would capture a bill whose tax cannot be posted: it
    // is refused with its reason and nothing is written.
    let stale = s
        .purchase(claim(
            &s,
            &po,
            line,
            "STALE-ACCOUNT",
            &date,
            20,
            Some(("SPARE", 15_400)),
        ))
        .await;
    let off = ok(&s.app,&s.alice,json!({"type":"account.set_active","id":spare["resourceId"],"expectedRevision":1,"active":false,"reason":"Retired control account"})).await;
    let sources = count(&s, "archava_accounting.purchase_sources").await;
    let commands = count(&s, "archava_purchasing.commands").await;
    let (st, v) = attempt(&s, matching(&stale, 1)).await;
    assert_refused_as(st, &v, "supplier_bill_tax_control_account_changed");
    assert_eq!(document_row(&s, &stale).await["state"], "captured");
    assert_eq!(document_row(&s, &stale).await["revision"], 1);
    assert_eq!(
        count(&s, "archava_accounting.purchase_sources").await,
        sources
    );
    assert_eq!(count(&s, "archava_purchasing.commands").await, commands);
    assert_eq!(
        count(&s, "archava_purchasing.supplier_document_allocations").await,
        0
    );
    // Reactivating the account releases the claim; the source stores the
    // revision the account has at capture and is reviewed against it.
    let on = ok(&s.app,&s.alice,json!({"type":"account.set_active","id":spare["resourceId"],"expectedRevision":off["revision"],"active":true,"reason":"Restored control account"})).await;
    assert_eq!(on["revision"], 3);
    let released = s.purchase(matching(&stale, 1)).await;
    assert_eq!(
        captured(&s, &released).await,
        (
            "bill".into(),
            14_000_000,
            1_540_000,
            Some(spare["resourceId"].as_str().unwrap().to_owned()),
            Some(3)
        )
    );
    let journal = post_taxed(&s, &released, "RELEASED", &on).await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![
            row("GRNI", 14_000_000, 0),
            row("VAT-SPARE", 1_540_000, 0),
            row("PAYABLE", 0, 15_540_000)
        ]
    );

    // A tax code whose control account is the policy's inventory account: the
    // tax would not be a separate line.
    let (po2, line2) = received(&s, 2, "RECEIPT-2").await;
    let mixed = s
        .purchase(claim(
            &s,
            &po2,
            line2,
            "POLICY-ACCOUNT",
            &date,
            2,
            Some(("STOCK", 1_540)),
        ))
        .await;
    let (st, v) = attempt(&s, matching(&mixed, 1)).await;
    assert_refused_as(st, &v, "supplier_bill_tax_control_account_in_policy");
    assert_eq!(document_row(&s, &mixed).await["state"], "captured");

    // Direct SQL as the serving role. A captured document keeps the snapshot
    // of the version that applies on its date, the computed tax of that rule
    // and the bound; a matched one keeps its tax.
    let refusal = |outcome: Result<(), sqlx::Error>| guard_message(&outcome.unwrap_err());
    let check = |message: &str| ("23514".to_owned(), message.to_owned());
    for (assignment, message) in [
        ("tax_rate_numerator=12", "Supplier document tax snapshot differs from the tax code version that applies on the document date"),
        ("tax_code_version=2", "Supplier document tax snapshot differs from the tax code version that applies on the document date"),
        ("tax_computed_minor=1", "Supplier document computed tax must follow the stored tax rule"),
        ("tax_minor=declared_total+1", "A supplier document tax amount cannot be negative or exceed its goods total"),
        ("tax_code='SALES'", "A supplier document needs an input tax code"),
        ("tax_code='PENDING'", "The supplier document tax code is entered but not confirmed"),
        ("tax_code='NOPE'", "Supplier document tax code is not a tax code of the company book"),
        ("document_date=DATE '2019-12-31'", "No confirmed version of the supplier document tax code applies on the document date"),
        ("tax_code=NULL", "A supplier document tax code and its tax amount are stated together"),
    ] {
        assert_eq!(
            refusal(direct_document(&s, &mixed, assignment).await),
            check(message),
            "{assignment}"
        );
    }
    assert_eq!(
        refusal(direct_document(&s, &stale, "tax_minor=1,state='void'").await),
        check("Supplier document tax changes only with a corrected claim version")
    );
    let plain = s
        .purchase(claim(&s, &po2, line2, "PLAIN", &date, 2, None))
        .await;
    assert_eq!(
        refusal(direct_document(&s, &plain, "tax_minor=1").await),
        check("A supplier document tax code and its tax amount are stated together")
    );

    // A source whose tax or tax account is not its document's cannot be
    // inserted; the captured values pass the guard and end as a duplicate.
    let spare_id = spare["resourceId"].as_str().unwrap();
    let vat_id = vat_account["resourceId"].as_str().unwrap();
    let differs = check("Purchase source tax differs from the tax stored on its supplier document");
    for (tax, account) in [
        (1_540_001, Some(spare_id)),
        (0, Some(spare_id)),
        (0, None),
        (1_540_000, None),
        (1_540_000, Some(vat_id)),
    ] {
        assert_eq!(
            refusal(direct_source(&s, &released, tax, account).await),
            differs,
            "{tax} {account:?}"
        );
    }
    // The stored revision is the account's at capture (3): the copy names 1.
    assert_eq!(
        refusal(direct_source(&s, &released, 1_540_000, Some(spare_id)).await),
        differs
    );
    let void = s.purchase(voiding(&stale, 2)).await;
    assert_eq!(
        refusal(direct_source(&s, &void, -1_540_000, Some(vat_id)).await),
        check("A purchase void retains the tax control account of its supplier bill")
    );
    assert_eq!(
        refusal(direct_source(&s, &void, -1_540_001, Some(spare_id)).await),
        differs
    );
    let untaxed = s.purchase(matching(&plain, 1)).await;
    assert_eq!(
        refusal(direct_source(&s, &untaxed, 100, Some(vat_id)).await),
        differs
    );
    assert_eq!(
        refusal(direct_source(&s, &untaxed, 0, Some(vat_id)).await),
        differs
    );
    let duplicate = direct_source(&s, &untaxed, 0, None).await.unwrap_err();
    assert_eq!(guard_message(&duplicate).0, "23505", "{duplicate}");

    // The guards and the host are still complete.
    assert!(sqlx::query_scalar::<_, bool>(
        "SELECT archava_accounting.purchase_guards_complete() AND archava_accounting.payable_guards_complete()"
    )
    .fetch_one(&s.f.runtime_pool)
    .await
    .unwrap());
}
