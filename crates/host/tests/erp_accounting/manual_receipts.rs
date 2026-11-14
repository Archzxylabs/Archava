//! Manual customer receipts: a person with `billing.settle` records money
//! received by bank transfer or in cash, and it flows through the existing
//! chain: billing settlement → collection source dated the day the money was
//! received → reviewed journal (clearing against receivable) → clearing
//! settlement against an imported bank row → bank cash. A reversal is the
//! refund side of the same chain. Billing 013 and Accounting 020.
use super::invoice_sources::SourceFixture;
use super::tax_posting::{issue as issue_taxed, post_taxed, taxed_invoice};
use super::tax_sales::{bill, confirmed_code, days, invoice_row, output_account, product};
use super::*;

/// `record_manual_receipt` for `invoice` at `revision`.
fn receipt(invoice: &Value, revision: i64, amount: i64, method: &str, on: &str) -> Value {
    json!({"type":"record_manual_receipt","invoiceId":invoice["invoiceId"],"expectedRevision":revision,"amountMinor":amount,
        "method":method,"receivedOn":on,"reference":format!("REF-{amount}"),"reason":"Seen on the statement and checked against the invoice"})
}
/// `reverse_manual_receipt` of the event `paid` recorded.
fn reversal(invoice: &Value, revision: i64, paid: &Value, amount: i64) -> Value {
    json!({"type":"reverse_manual_receipt","invoiceId":invoice["invoiceId"],"expectedRevision":revision,"receiptEventId":paid["eventId"],
        "amountMinor":amount,"reason":"The transfer was returned by the bank"})
}
/// A company invoice of 2 × 15,000 (30,000 in the invoice's unit), issued at
/// revision 2.
async fn issued(s: &SourceFixture, code: &str) -> (Value, Value) {
    let invoice = s.invoice(code).await;
    let issue = s.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Issue before the customer pays"})).await;
    (invoice, issue)
}
/// The database clock cannot be moved, so an invoice issued `days` ago is
/// planted: as the superuser, with triggers off for this one transaction, the
/// invoice's issue stamp and its captured issue source move back together.
async fn backdate_issue(s: &SourceFixture, invoice: &Value, days: i32) {
    let mut tx = s.f.admin_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role='replica'")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE archava_billing.invoices SET issued_at=issued_at-make_interval(days=>$2) WHERE id=$1::uuid")
        .bind(invoice["invoiceId"].as_str().unwrap())
        .bind(days)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE archava_accounting.invoice_sources SET source_date=source_date-$2,occurred_at=occurred_at-make_interval(days=>$2) WHERE invoice_id=$1::uuid AND effect='issue'")
        .bind(invoice["invoiceId"].as_str().unwrap())
        .bind(days)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
/// Status, settled, outstanding and revision of the stored invoice.
async fn balance(s: &SourceFixture, invoice: &Value) -> (String, i64, i64, i32) {
    sqlx::query_as("SELECT status,settled_minor,outstanding_minor,revision FROM archava_billing.invoices WHERE id=$1::uuid")
        .bind(invoice["invoiceId"].as_str().unwrap())
        .fetch_one(&s.f.admin_pool)
        .await
        .unwrap()
}
/// Effect, amount and source date the bridge captured for one billing command.
async fn captured(s: &SourceFixture, command: &Value) -> (String, i64, String) {
    sqlx::query_as("SELECT effect,amount_minor,source_date::text FROM archava_accounting.invoice_sources WHERE id=$1::uuid")
        .bind(command["commandId"].as_str().unwrap())
        .fetch_one(&s.f.admin_pool)
        .await
        .unwrap()
}
async fn events(s: &SourceFixture, invoice: &Value) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM archava_billing.payment_events WHERE invoice_id=$1::uuid",
    )
    .bind(invoice["invoiceId"].as_str().unwrap())
    .fetch_one(&s.f.admin_pool)
    .await
    .unwrap()
}
/// Drafts a collection or refund source against receivable and clearing and
/// has bob post it. `SourceFixture::source_draft` picks the clearing account
/// by the provider action, so a manual source names its accounts here.
async fn post_money(s: &SourceFixture, source: &Value, code: &str) -> Value {
    let request = input(
        json!({"type":"invoice_source.draft","sourceId":source["commandId"],"code":code,"accounts":[bank::account_ref(&s.ar),bank::account_ref(&s.clearing)],"reason":"Review the manual receipt: amount, received-on date and clearing account"}),
    );
    let (st, v) = send(&s.app, &s.alice, &request).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, posted) = send(&s.app, &s.bob, &action("journal.post", &v["receipt"])).await;
    assert_eq!(st, StatusCode::OK, "{posted}");
    v["receipt"].clone()
}
fn closing(report: &Value, code: &str) -> String {
    report["trialBalance"]["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["code"] == code)
        .map(|a| a["closingNetMinor"].as_str().unwrap().to_owned())
        .unwrap_or_else(|| "0".into())
}
/// A cash account bound as a bank account, with one imported statement row of
/// `amount` (book minor units) dated `date`. Returns the account and the row.
async fn bank_row(s: &SourceFixture, date: &str, amount: &str) -> (Value, Value) {
    let cash = account(&s.app, &s.alice, &s.book, "BANK", "asset", "cash").await;
    let binding = bank::bank_account(&s.app, &s.alice, &s.book, &cash).await;
    ok(&s.app,&s.alice,json!({"type":"bank_statement.import","bankAccountId":binding["resourceId"],"account":bank::account_ref(&cash),"statement":{"reference":"MANUAL-RECEIPT","fromDate":date,"toDate":date,"currency":s.currency,"openingMinor":"0","closingMinor":amount,"rows":[{"externalRef":"TRANSFER-IN","date":date,"amountMinor":amount,"description":"Customer transfer as the bank shows it"}]},"reason":"Statement row for the manually recorded receipt"})).await;
    let (_, workspace) = bank::bank_workspace(&s.app, &s.alice, &s.company, "bank_rows").await;
    (cash, workspace["bank_rows"][0].clone())
}
/// Drafts and posts the clearing settlement of `source` against `row`.
async fn settle(
    s: &SourceFixture,
    source: &Value,
    row: &Value,
    cash: &Value,
    gross: &str,
) -> (StatusCode, Value) {
    let (st, v) = send(&s.app,&s.alice,&input(json!({"type":"clearing_settlement.draft","sourceId":source["commandId"],"bankRowId":row["id"],"code":"SETTLE-MANUAL","clearingAccount":bank::account_ref(&s.clearing),"cashAccount":bank::account_ref(cash),"grossMinor":gross,"feeMinor":"0","reason":"Match the manual receipt to the bank row that shows it"}))).await;
    if st != StatusCode::OK {
        return (st, v);
    }
    send(&s.app,&s.bob,&input(json!({"type":"journal.post","id":v["receipt"]["resourceId"],"expectedRevision":1,"reason":"Independent review of receipt and bank row"}))).await
}
async fn clearing_source(s: &SourceFixture, command: &Value) -> Value {
    let (st, v) = bank::bank_workspace(&s.app, &s.alice, &s.company, "clearing_sources").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["clearing_sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == command["commandId"])
        .cloned()
        .unwrap_or_else(|| panic!("{v}"))
}

/// **Witness: the whole chain on an IDR book. An invoice of 2 × 15,000 issued
/// three days ago takes a manual bank-transfer receipt of the full amount
/// dated yesterday: the invoice is settled with nothing outstanding; the
/// event is a `manual:bank_transfer` event with who, when, method, reference
/// and reason; a collection source of 3,000,000 dated yesterday is captured;
/// its draft posted by a second person clears the receivable against
/// clearing on yesterday's date; a clearing settlement against a bank row
/// dated yesterday moves clearing to bank cash; the receivable position is
/// zero, the control comparison has no difference and the reports balance.**
#[tokio::test]
async fn a_manual_bank_transfer_settles_the_invoice_and_reaches_bank_cash_through_clearing() {
    let s = SourceFixture::new().await;
    s.enable().await;
    let (today, yesterday, _) = days(&s.f).await;
    let (invoice, issue) = issued(&s, "MANUAL-CHAIN").await;
    backdate_issue(&s, &invoice, 3).await;
    s.post_source(&issue, "ISSUE").await;

    let paid = s
        .bill(receipt(&invoice, 2, 30000, "bank_transfer", &yesterday))
        .await;
    assert_eq!(paid["action"], "record_manual_receipt");
    assert_eq!(paid["status"], "settled");
    assert_eq!(paid["outstandingMinor"], 0);
    assert_eq!(paid["revision"], 3);
    assert_eq!(balance(&s, &invoice).await, ("settled".into(), 30000, 0, 3));
    let event: (String, String, String, i64, String, String, String, String, String, String, bool) = sqlx::query_as(
        "SELECT provider_id,provider_event_id,outcome,amount_minor,currency,manual_method,received_on::text,reference,reason,recorded_by,reverses_event_id IS NULL \
         FROM archava_billing.payment_events WHERE id=$1::uuid",
    )
    .bind(paid["eventId"].as_str().unwrap())
    .fetch_one(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(
        event,
        (
            "manual:bank_transfer".into(),
            format!("manual:{}", paid["eventId"].as_str().unwrap()),
            "succeeded".into(),
            30000,
            "IDR".into(),
            "bank_transfer".into(),
            yesterday.clone(),
            "REF-30000".into(),
            "Seen on the statement and checked against the invoice".into(),
            "alice".into(),
            true
        )
    );
    let audit: (String, String) = sqlx::query_as(
        "SELECT operation,reason FROM archava_billing.audit WHERE request_id=$1::uuid",
    )
    .bind(paid["commandId"].as_str().unwrap())
    .fetch_one(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(audit.0, "record_manual_receipt");
    assert!(
        audit
            .1
            .contains("Manual bank_transfer receipt of 30000 minor units")
            && audit.1.contains(&yesterday)
            && audit.1.contains("REF-30000"),
        "{}",
        audit.1
    );

    // The source is dated the day the money was received, not the entry day.
    assert_eq!(
        captured(&s, &paid).await,
        ("collection".into(), 3_000_000, yesterday.clone())
    );
    assert_ne!(yesterday, today);
    let source = clearing_source(&s, &paid).await;
    assert_eq!(source["origin"], "manual");
    assert_eq!(source["method"], "bank_transfer");
    assert_eq!(source["sourceDate"], yesterday.as_str());

    // Unposted, the collection cannot be settled against the bank yet.
    let (cash, row) = bank_row(&s, &yesterday, "3000000").await;
    assert_eq!(
        settle(&s, &paid, &row, &cash, "3000000").await.0,
        StatusCode::CONFLICT
    );
    let journal = post_money(&s, &paid, "COLLECT").await;
    let dated: String = sqlx::query_scalar(
        "SELECT entry_date::text FROM archava_accounting.journals WHERE id=$1::uuid",
    )
    .bind(journal["resourceId"].as_str().unwrap())
    .fetch_one(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(dated, yesterday);
    let report = s.reports().await;
    assert_eq!(closing(&report, "AR"), "0");
    assert_eq!(closing(&report, "CLEARING"), "3000000");
    assert_eq!(report["cashFlow"]["netChangeMinor"], "0");

    let (st, settled) = settle(&s, &paid, &row, &cash, "3000000").await;
    assert_eq!(st, StatusCode::OK, "{settled}");
    let (st, matched) = send(
        &s.app,
        &s.alice,
        &bank::match_row(&row, &settled["receipt"], 2, &cash, "3000000"),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{matched}");
    let report = s.reports().await;
    assert_eq!(closing(&report, "AR"), "0");
    assert_eq!(closing(&report, "CLEARING"), "0");
    assert_eq!(closing(&report, "BANK"), "3000000");
    assert_eq!(report["profitAndLoss"]["revenueMinor"], "3000000");
    assert_eq!(report["trialBalance"]["balanced"], true);
    assert_eq!(report["balanceSheet"]["balanced"], true);
    assert_eq!(report["sourceCoverage"]["unpostedCount"], 0);

    let position = subledger::accepted(&s.app, &s.alice, &s.book, "receivable", &today).await;
    assert_eq!(position["totals"]["netOutstandingMinor"], "0", "{position}");
    assert_eq!(position["postedSourceControlsAgree"], true, "{position}");
    assert_eq!(
        position["controls"][0]["differenceMinor"], "0",
        "{position}"
    );
    assert_eq!(
        position["coverage"]["pendingMonetarySourceCount"], 0,
        "{position}"
    );
    assert_eq!(
        position["positions"].as_array().unwrap().len(),
        1,
        "{position}"
    );
    assert_eq!(position["positions"][0]["balanceMinor"], "0", "{position}");
    assert_eq!(position["positions"][0]["bucket"], "settled", "{position}");
    // Nothing is left to age: every aging bucket of the receivable is zero.
    for (bucket, value) in position["totals"].as_object().unwrap() {
        assert_eq!(value, "0", "{bucket} {position}");
    }
    // The day before the money was received the invoice was still owed.
    let earlier: String = sqlx::query_scalar("SELECT ($1::date-1)::text")
        .bind(&yesterday)
        .fetch_one(&s.f.admin_pool)
        .await
        .unwrap();
    let before = subledger::accepted(&s.app, &s.alice, &s.book, "receivable", &earlier).await;
    assert_eq!(
        before["totals"]["netOutstandingMinor"], "3000000",
        "{before}"
    );
}

/// **Witness: part payments. 10,000 of 30,000 leaves the invoice issued with
/// 20,000 outstanding; 20,001 more is refused as exceeding what is owed and
/// stores nothing; 20,000 settles it; a further receipt on the settled
/// invoice is refused for the same reason. Each receipt is its own collection
/// source.**
#[tokio::test]
async fn part_payments_settle_at_zero_and_a_receipt_beyond_the_balance_is_refused() {
    let s = SourceFixture::new().await;
    s.enable().await;
    let (today, _, _) = days(&s.f).await;
    let (invoice, _) = issued(&s, "MANUAL-PARTS").await;
    let first = s
        .bill(receipt(&invoice, 2, 10000, "bank_transfer", &today))
        .await;
    assert_eq!(first["status"], "issued");
    assert_eq!(first["outstandingMinor"], 20000);
    assert_eq!(
        balance(&s, &invoice).await,
        ("issued".into(), 10000, 20000, 3)
    );

    let (st, v) = bill(&s, receipt(&invoice, 3, 20001, "bank_transfer", &today)).await;
    assert_refused_as(st, &v, "manual_receipt_exceeds_outstanding");
    assert_eq!(
        balance(&s, &invoice).await,
        ("issued".into(), 10000, 20000, 3)
    );
    assert_eq!(events(&s, &invoice).await, 1);
    // A stale revision is a plain conflict, not a reason about money.
    let (st, v) = bill(&s, receipt(&invoice, 2, 20000, "bank_transfer", &today)).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["detail"].is_null(), "{v}");

    let second = s.bill(receipt(&invoice, 3, 20000, "cash", &today)).await;
    assert_eq!(second["status"], "settled");
    assert_eq!(second["outstandingMinor"], 0);
    assert_eq!(balance(&s, &invoice).await, ("settled".into(), 30000, 0, 4));
    let (st, v) = bill(&s, receipt(&invoice, 4, 1, "cash", &today)).await;
    assert_refused_as(st, &v, "manual_receipt_exceeds_outstanding");
    assert_eq!(events(&s, &invoice).await, 2);
    assert_eq!(
        captured(&s, &first).await,
        ("collection".into(), 1_000_000, today.clone())
    );
    assert_eq!(
        captured(&s, &second).await,
        ("collection".into(), 2_000_000, today)
    );
}

/// **Witness: reversals. A full manual receipt is reversed in part (the
/// invoice reopens by that amount and a refund source dated today is
/// captured), a reversal beyond what is left of the receipt is refused, the
/// rest is reversed, and nothing more can be. A gateway event, a reversal and
/// a receipt of another invoice cannot be reversed; an unknown event is not
/// found. The workspace lists every event with its origin, method, dates,
/// reference, recorder and reversed amount. Posted, the sources leave the
/// receivable at what is still owed.**
#[tokio::test]
async fn a_manual_receipt_is_reversed_in_part_and_in_full_and_a_gateway_event_never_is() {
    let s = SourceFixture::new().await;
    s.enable().await;
    let (today, _, _) = days(&s.f).await;
    let (invoice, issue) = issued(&s, "MANUAL-REVERSAL").await;
    s.post_source(&issue, "ISSUE").await;
    let paid = s
        .bill(receipt(&invoice, 2, 30000, "bank_transfer", &today))
        .await;
    assert_eq!(paid["status"], "settled");

    let part = s.bill(reversal(&invoice, 3, &paid, 10000)).await;
    assert_eq!(part["action"], "reverse_manual_receipt");
    assert_eq!(part["status"], "issued");
    assert_eq!(part["outstandingMinor"], 10000);
    assert_eq!(
        balance(&s, &invoice).await,
        ("issued".into(), 20000, 10000, 4)
    );
    assert_eq!(
        captured(&s, &part).await,
        ("refund".into(), -1_000_000, today.clone())
    );
    let stored: (String, String, String, Option<String>, Option<String>, String) = sqlx::query_as(
        "SELECT provider_id,outcome,reverses_event_id::text,received_on::text,reference,reason FROM archava_billing.payment_events WHERE id=$1::uuid",
    )
    .bind(part["eventId"].as_str().unwrap())
    .fetch_one(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(
        stored,
        (
            "manual:bank_transfer".into(),
            "refunded".into(),
            paid["eventId"].as_str().unwrap().to_owned(),
            None,
            None,
            "The transfer was returned by the bank".into()
        )
    );

    let (st, v) = bill(&s, reversal(&invoice, 4, &paid, 20001)).await;
    assert_refused_as(st, &v, "manual_reversal_exceeds_receipt");
    assert_eq!(
        balance(&s, &invoice).await,
        ("issued".into(), 20000, 10000, 4)
    );
    let rest = s.bill(reversal(&invoice, 4, &paid, 20000)).await;
    assert_eq!(rest["outstandingMinor"], 30000);
    assert_eq!(balance(&s, &invoice).await, ("issued".into(), 0, 30000, 5));
    let (st, v) = bill(&s, reversal(&invoice, 5, &paid, 1)).await;
    assert_refused_as(st, &v, "manual_reversal_exceeds_receipt");

    // A reversal is not a receipt; a gateway event is not manual.
    let (st, v) = bill(&s, reversal(&invoice, 5, &part, 1)).await;
    assert_refused_as(st, &v, "manual_reversal_not_manual");
    let gateway = s.payment(&invoice, "succeeded", 5000).await;
    let (st, v) = bill(&s, reversal(&invoice, 6, &gateway, 5000)).await;
    assert_refused_as(st, &v, "manual_reversal_not_manual");
    assert_eq!(
        balance(&s, &invoice).await,
        ("issued".into(), 5000, 25000, 6)
    );
    // A receipt of another invoice, and an event that does not exist.
    let (other, _) = issued(&s, "MANUAL-OTHER").await;
    let elsewhere = s.bill(receipt(&other, 2, 1000, "cash", &today)).await;
    let (st, v) = bill(&s, reversal(&invoice, 6, &elsewhere, 1000)).await;
    assert_refused_as(st, &v, "manual_reversal_not_manual");
    let (st, v) = bill(
        &s,
        reversal(&invoice, 6, &json!({"eventId":Uuid::new_v4()}), 1),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
    assert_eq!(events(&s, &invoice).await, 4);

    // The reopened invoice takes a new receipt.
    let again = s.bill(receipt(&invoice, 6, 5000, "cash", &today)).await;
    assert_eq!(again["outstandingMinor"], 20000);

    let row = invoice_row(&s, &invoice).await;
    assert_eq!(row["issuedOn"], today.as_str(), "{row}");
    let payments = row["payments"].as_array().unwrap();
    assert_eq!(payments.len(), 5, "{row}");
    let first = &payments[0];
    assert_eq!(first["id"], paid["eventId"]);
    assert_eq!(first["origin"], "manual");
    assert_eq!(first["method"], "bank_transfer");
    assert_eq!(first["providerId"], "manual:bank_transfer");
    assert_eq!(first["outcome"], "succeeded");
    assert_eq!(first["amount"], 30000);
    assert_eq!(first["receivedOn"], today.as_str());
    assert_eq!(first["recordedOn"], today.as_str());
    assert_eq!(first["reference"], "REF-30000");
    assert_eq!(
        first["reason"],
        "Seen on the statement and checked against the invoice"
    );
    assert_eq!(first["recordedBy"], "alice");
    assert_eq!(first["reversedAmount"], 30000);
    assert_eq!(first["verifiedByProvider"], false);
    assert!(first["recordedAt"].is_string(), "{first}");
    assert_eq!(payments[1]["origin"], "manual");
    assert_eq!(payments[1]["outcome"], "refunded");
    assert_eq!(payments[1]["reversesEventId"], paid["eventId"]);
    assert!(payments[1]["receivedOn"].is_null() && payments[1]["reference"].is_null());
    assert_eq!(
        payments[1]["reason"],
        "The transfer was returned by the bank"
    );
    assert_eq!(payments[3]["origin"], "gateway");
    assert_eq!(payments[3]["providerId"], "owned-bank");
    assert!(payments[3]["method"].is_null() && payments[3]["reference"].is_null());
    assert_eq!(payments[3]["verifiedByProvider"], true);
    assert_eq!(payments[4]["method"], "cash");
    assert_eq!(payments[4]["reversedAmount"], 0);
    let (st, workspace) = call(
        &s.app,
        "GET",
        &format!(
            "/erp/billing/workspace?tenant=acme&companyId={}",
            s.company["companyId"].as_str().unwrap()
        ),
        Value::Null,
        Some(&s.alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{workspace}");
    assert_eq!(workspace["today"], today.as_str(), "{workspace}");

    // Posted, the money sources net to what was collected and kept.
    for (source, code) in [
        (&paid, "PAID"),
        (&part, "PART"),
        (&rest, "REST"),
        (&again, "AGAIN"),
    ] {
        post_money(&s, source, code).await;
    }
    s.post_source(&gateway, "GATEWAY").await;
    let report = s.reports().await;
    assert_eq!(closing(&report, "AR"), "2000000");
    assert_eq!(closing(&report, "CLEARING"), "1000000");
    assert_eq!(report["trialBalance"]["balanced"], true);
    let position = subledger::accepted(&s.app, &s.alice, &s.book, "receivable", &today).await;
    // The other invoice is issued and unposted; this one owes 20,000.
    assert_eq!(
        position["controls"][0]["differenceMinor"], "0",
        "{position}"
    );
    assert_eq!(
        position["positions"][0]["balanceMinor"], "2000000",
        "{position}"
    );
}

/// **Witness: a taxed invoice (11% added to 2 × 15,000) is settled by a manual
/// receipt of its gross, 33,300. The collection carries no tax and no tax
/// account, clears the gross receivable against clearing, and leaves the tax
/// report exactly as it was.**
#[tokio::test]
async fn a_manual_receipt_of_the_gross_settles_a_taxed_invoice_and_leaves_the_tax_report_alone() {
    let s = SourceFixture::new().await;
    s.enable().await;
    let (today, _, _) = days(&s.f).await;
    let vat_account = output_account(&s).await;
    confirmed_code(
        &s,
        "VAT",
        "output",
        (11, 100),
        (1, 1),
        &vat_account,
        "2020-01-01",
    )
    .await;
    let item = product(&s, "TAXED-RECEIPT", 15000).await;
    let invoice = taxed_invoice(&s, "TAXED-RECEIPT", &item, 2, Some(("VAT", false))).await;
    let issue = issue_taxed(&s, &invoice).await;
    post_taxed(&s, &issue, "TAXED-ISSUE", &vat_account).await;
    let tax_report = || async {
        let (st, mut v) = call(
            &s.app,
            "GET",
            &format!(
                "/erp/accounting/tax-report?tenant=acme&bookId={}&from=2000-01-01&to=2100-12-31",
                s.book["bookId"].as_str().unwrap()
            ),
            Value::Null,
            Some(&s.alice),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        // The moment the report was read is not part of what it reports.
        if let Some(body) = v.as_object_mut() {
            body.remove("observedAt");
            body.remove("generatedAt");
        }
        v
    };
    let before = tax_report().await;

    let (st, v) = bill(&s, receipt(&invoice, 2, 33301, "bank_transfer", &today)).await;
    assert_refused_as(st, &v, "manual_receipt_exceeds_outstanding");
    let paid = s
        .bill(receipt(&invoice, 2, 33300, "bank_transfer", &today))
        .await;
    assert_eq!(paid["status"], "settled");
    let source: (String, i64, i64, Option<String>) = sqlx::query_as(
        "SELECT effect,amount_minor,tax_minor,tax_account_id::text FROM archava_accounting.invoice_sources WHERE id=$1::uuid",
    )
    .bind(paid["commandId"].as_str().unwrap())
    .fetch_one(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(source, ("collection".into(), 3_330_000, 0, None));
    post_money(&s, &paid, "TAXED-COLLECT").await;
    let report = s.reports().await;
    assert_eq!(closing(&report, "AR"), "0");
    assert_eq!(closing(&report, "CLEARING"), "3330000");
    assert_eq!(closing(&report, "VAT-OUT"), "-330000");
    assert_eq!(report["profitAndLoss"]["revenueMinor"], "3000000");
    assert_eq!(report["trialBalance"]["balanced"], true);
    assert_eq!(tax_report().await, before);
}

/// **Witness: a USD book under the `minor` unit. A receipt of 30,000 cents
/// (USD 300.00) is captured as 30,000 book units, unchanged, in USD.**
#[tokio::test]
async fn a_manual_receipt_on_a_usd_book_posts_minor_units_unchanged() {
    let s = SourceFixture::new_in("USD").await;
    s.enable().await;
    let (today, _, _) = days(&s.f).await;
    let (invoice, issue) = issued(&s, "MANUAL-USD").await;
    s.post_source(&issue, "ISSUE").await;
    let first = s
        .bill(receipt(&invoice, 2, 12550, "bank_transfer", &today))
        .await;
    assert_eq!(first["outstandingMinor"], 17450);
    let (st, v) = bill(&s, receipt(&invoice, 3, 17451, "bank_transfer", &today)).await;
    assert_refused_as(st, &v, "manual_receipt_exceeds_outstanding");
    let second = s
        .bill(receipt(&invoice, 3, 17450, "bank_transfer", &today))
        .await;
    assert_eq!(second["status"], "settled");
    let currency: String =
        sqlx::query_scalar("SELECT currency FROM archava_billing.payment_events WHERE id=$1::uuid")
            .bind(first["eventId"].as_str().unwrap())
            .fetch_one(&s.f.admin_pool)
            .await
            .unwrap();
    assert_eq!(currency, "USD");
    assert_eq!(
        captured(&s, &first).await,
        ("collection".into(), 12550, today.clone())
    );
    post_money(&s, &first, "USD-ONE").await;
    post_money(&s, &second, "USD-TWO").await;
    let report = s.reports().await;
    assert_eq!(closing(&report, "AR"), "0");
    assert_eq!(closing(&report, "CLEARING"), "30000");
    assert_eq!(report["trialBalance"]["balanced"], true);
    let (cash, row) = bank_row(&s, &today, "30000").await;
    // One bank row of the whole transfer settles both part receipts.
    let (st, v) = settle(&s, &first, &row, &cash, "12550").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let report = s.reports().await;
    assert_eq!(closing(&report, "CLEARING"), "17450");
    assert_eq!(closing(&report, "BANK"), "12550");
}

/// **Witness: cash follows the same chain. A cash receipt is a `manual:cash`
/// event; posted, it sits in the clearing account and moves no cash; it stays
/// there until a clearing settlement matches it to a statement row of the
/// cash drawer, registered as a bank account with entered rows. There is no
/// evidence-free cash.**
#[tokio::test]
async fn a_cash_receipt_waits_in_clearing_until_it_is_settled_against_a_statement_row() {
    let s = SourceFixture::new().await;
    s.enable().await;
    let (today, _, _) = days(&s.f).await;
    let (invoice, issue) = issued(&s, "MANUAL-CASH").await;
    s.post_source(&issue, "ISSUE").await;
    let paid = s.bill(receipt(&invoice, 2, 30000, "cash", &today)).await;
    assert_eq!(paid["status"], "settled");
    let provider: (String, String) = sqlx::query_as(
        "SELECT provider_id,manual_method FROM archava_billing.payment_events WHERE id=$1::uuid",
    )
    .bind(paid["eventId"].as_str().unwrap())
    .fetch_one(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(provider, ("manual:cash".into(), "cash".into()));
    post_money(&s, &paid, "CASH-COLLECT").await;
    let report = s.reports().await;
    assert_eq!(closing(&report, "AR"), "0");
    assert_eq!(closing(&report, "CLEARING"), "3000000");
    assert_eq!(report["cashFlow"]["netChangeMinor"], "0");
    let source = clearing_source(&s, &paid).await;
    assert_eq!(source["origin"], "manual");
    assert_eq!(source["method"], "cash");
    assert_eq!(
        source["balance"]["availableGrossMinor"], "3000000",
        "{source}"
    );
    // The period cannot hide it: nothing but a settlement empties clearing.
    let settlements: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_accounting.clearing_settlements")
            .fetch_one(&s.f.admin_pool)
            .await
            .unwrap();
    assert_eq!(settlements, 0);

    // The cash drawer is a bank account with a statement row the cashier entered.
    let (drawer, row) = bank_row(&s, &today, "3000000").await;
    let (st, v) = settle(&s, &paid, &row, &drawer, "3000000").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let report = s.reports().await;
    assert_eq!(closing(&report, "CLEARING"), "0");
    assert_eq!(closing(&report, "BANK"), "3000000");
    assert_eq!(report["cashFlow"]["netChangeMinor"], "3000000");
}

/// **Witness: what is refused at entry, each with its reason and nothing
/// stored: a draft and a void invoice, a zero and a negative amount (the
/// parse), a date in the future, a date before the invoice was issued, a date
/// in a closed period (the existing closed-period reason), and an invoice
/// with an open, unpaid gateway link until that link is closed. The link row
/// is planted as the owner: this suite has no payment gateway.**
#[tokio::test]
async fn a_manual_receipt_is_refused_with_its_reason_and_stores_nothing() {
    let s = SourceFixture::new().await;
    s.enable().await;
    let (today, yesterday, tomorrow) = days(&s.f).await;

    let draft = s.invoice("MANUAL-DRAFT").await;
    let (st, v) = bill(&s, receipt(&draft, 1, 30000, "cash", &today)).await;
    assert_refused_as(st, &v, "manual_receipt_not_issued");
    s.bill(json!({"type":"void_invoice","invoiceId":draft["invoiceId"],"expectedRevision":1,"reason":"Never sent"})).await;
    let (st, v) = bill(&s, receipt(&draft, 2, 30000, "cash", &today)).await;
    assert_refused_as(st, &v, "manual_receipt_not_issued");
    assert_eq!(events(&s, &draft).await, 0);

    let (invoice, issue) = issued(&s, "MANUAL-REFUSALS").await;
    for amount in [0, -30000] {
        let (st, v) = bill(&s, receipt(&invoice, 2, amount, "cash", &today)).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
        assert_eq!(v["reason"], "invalid_submission", "{v}");
    }
    let (st, v) = bill(&s, receipt(&invoice, 2, 30000, "cash", &tomorrow)).await;
    assert_refused_as(st, &v, "manual_receipt_future_date");
    let (st, v) = bill(&s, receipt(&invoice, 2, 30000, "cash", &yesterday)).await;
    assert_refused_as(st, &v, "manual_receipt_before_issue");
    for (field, value) in [
        ("receivedOn", json!("2026-02-30")),
        ("method", json!("card")),
        ("reference", json!("  ")),
        ("reason", json!("")),
        ("signature", json!("0".repeat(64))),
    ] {
        let mut command = receipt(&invoice, 2, 30000, "cash", &today);
        command[field] = value;
        let (st, v) = bill(&s, command).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{field} {v}");
    }

    // An open, unpaid online payment: the customer could pay twice.
    let link = Uuid::new_v4();
    sqlx::query("INSERT INTO archava_billing.gateway_links(tenant_id,id,invoice_id,integration_id,service_id,actor_id,request_id,order_id,amount_minor,invoice_revision,state) VALUES('acme',$1,$2::uuid,'merchant','alice','alice',$3,$4,30000,2,'open')")
        .bind(link)
        .bind(invoice["invoiceId"].as_str().unwrap())
        .bind(Uuid::new_v4())
        .bind(format!("archava-{link}"))
        .execute(&s.f.admin_pool)
        .await
        .unwrap();
    let (st, v) = bill(&s, receipt(&invoice, 2, 30000, "cash", &today)).await;
    assert_refused_as(st, &v, "manual_receipt_open_payment_link");
    assert_eq!(balance(&s, &invoice).await, ("issued".into(), 0, 30000, 2));
    assert_eq!(events(&s, &invoice).await, 0);
    // The gateway reported the link failed or expired: it is closed.
    sqlx::query("UPDATE archava_billing.gateway_links SET state='failed',closed_at=clock_timestamp() WHERE id=$1")
        .bind(link)
        .execute(&s.f.admin_pool)
        .await
        .unwrap();

    // A closed period: the receipt would be dated inside it.
    s.post_source(&issue, "ISSUE").await;
    let (st, closed) = send(&s.app, &s.alice, &action("period.close", &s.period)).await;
    assert_eq!(st, StatusCode::OK, "{closed}");
    let commands: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_billing.commands")
        .fetch_one(&s.f.admin_pool)
        .await
        .unwrap();
    let (st, v) = bill(&s, receipt(&invoice, 2, 30000, "cash", &today)).await;
    assert_refused_as(st, &v, "invoice_in_closed_period");
    assert_eq!(balance(&s, &invoice).await, ("issued".into(), 0, 30000, 2));
    assert_eq!(events(&s, &invoice).await, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM archava_billing.commands")
            .fetch_one(&s.f.admin_pool)
            .await
            .unwrap(),
        commands
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM archava_accounting.invoice_sources WHERE effect<>'issue'"
        )
        .fetch_one(&s.f.admin_pool)
        .await
        .unwrap(),
        0
    );
    // Reopened, the same receipt is recorded.
    let (st, reopened) = send(
        &s.app,
        &s.alice,
        &action("period.reopen", &closed["receipt"]),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{reopened}");
    let paid = s.bill(receipt(&invoice, 2, 30000, "cash", &today)).await;
    assert_eq!(paid["status"], "settled");
}

/// **Witness: who may record one. A person without `billing.settle`, a
/// session whose billing membership is a service's, and a service principal
/// calling the command pipeline directly are all refused before anything is
/// written. A replay returns the original receipt and one event. Two
/// concurrent receipts that together exceed what is owed: exactly one is
/// recorded.**
#[tokio::test]
async fn manual_receipts_need_a_person_who_may_settle_and_are_idempotent_and_serial() {
    use archava_domain::erp::billing::{BillingCommand, BillingMutation, ManualMethod};
    use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
    let s = SourceFixture::new().await;
    s.enable().await;
    let (today, _, _) = days(&s.f).await;
    let (invoice, _) = issued(&s, "MANUAL-AUTHORITY").await;

    // bob may read and write invoices but not settle them.
    sqlx::query("INSERT INTO archava_billing.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','bob','human',ARRAY['billing.read','billing.write','audit.read'],'all')").execute(&s.f.admin_pool).await.unwrap();
    // service-seat can log in, but its billing membership is a service's.
    sqlx::query("INSERT INTO archava_billing.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','service-seat','service',$1,'all')").bind(archava_domain::erp::billing::BILLING_PERMISSIONS).execute(&s.f.admin_pool).await.unwrap();
    let seat = login(&s.app, "service-seat").await;
    for session in [&s.bob, &seat] {
        for mut command in [
            receipt(&invoice, 2, 30000, "cash", &today),
            reversal(&invoice, 2, &json!({"eventId":Uuid::new_v4()}), 1),
        ] {
            command["commandId"] = json!(Uuid::new_v4());
            let (st, v) = call(
                &s.app,
                "POST",
                "/erp/billing/commands",
                json!({"tenant":"acme","mutation":command}),
                Some(session),
            )
            .await;
            assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
            assert_eq!(v["reason"], "forbidden", "{v}");
        }
    }
    // The pipeline itself refuses a service principal on the manual arms,
    // whatever its grants: only a provider event may come from a service.
    let service = PurchasePrincipal {
        kind: PrincipalKind::Service,
        id: "service-seat".into(),
        on_behalf_of: None,
    };
    let mut tx = s.f.runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    let access = archava_adapters::erp::billing::resolve_billing_context(&mut tx, "acme", &service)
        .await
        .unwrap();
    assert!(access.holds("billing.settle"));
    for command in [
        BillingCommand::RecordManualReceipt {
            command_id: Uuid::new_v4().to_string(),
            invoice_id: invoice["invoiceId"].as_str().unwrap().into(),
            expected_revision: 2,
            amount_minor: 30000,
            method: ManualMethod::Cash,
            received_on: today.clone(),
            reference: "SERVICE".into(),
            reason: "A service is not a person".into(),
        },
        BillingCommand::ReverseManualReceipt {
            command_id: Uuid::new_v4().to_string(),
            invoice_id: invoice["invoiceId"].as_str().unwrap().into(),
            expected_revision: 2,
            receipt_event_id: Uuid::new_v4().to_string(),
            amount_minor: 1,
            reason: "A service is not a person".into(),
        },
    ] {
        let refused = archava_adapters::erp::billing::record_billing_command(
            &mut tx,
            "acme",
            &service,
            &access,
            "fingerprint",
            &BillingMutation { command },
        )
        .await;
        assert!(
            matches!(
                refused,
                Err(archava_adapters::erp::PurchaseError::Domain(
                    archava_domain::erp::ErpError::Forbidden
                ))
            ),
            "{refused:?}"
        );
    }
    tx.rollback().await.unwrap();
    assert_eq!(balance(&s, &invoice).await, ("issued".into(), 0, 30000, 2));
    assert_eq!(events(&s, &invoice).await, 0);

    // Two receipts of 20,000 at once against 30,000 owed.
    let submit = |command: Value| {
        let (app, session) = (s.app.clone(), s.alice.clone());
        async move {
            call(
                &app,
                "POST",
                "/erp/billing/commands",
                json!({"tenant":"acme","mutation":command}),
                Some(&session),
            )
            .await
        }
    };
    let mut one = receipt(&invoice, 2, 20000, "cash", &today);
    one["commandId"] = json!(Uuid::new_v4());
    let mut two = receipt(&invoice, 2, 20000, "bank_transfer", &today);
    two["commandId"] = json!(Uuid::new_v4());
    let (a, b) = tokio::join!(
        tokio::spawn(submit(one.clone())),
        tokio::spawn(submit(two.clone()))
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    let mut statuses = [a.0, b.0];
    statuses.sort();
    assert_eq!(
        statuses,
        [StatusCode::OK, StatusCode::CONFLICT],
        "{a:?} {b:?}"
    );
    assert_eq!(
        balance(&s, &invoice).await,
        ("issued".into(), 20000, 10000, 3)
    );
    assert_eq!(events(&s, &invoice).await, 1);

    // The winner replays its receipt; nothing is recorded twice.
    let (winner, receipt_body) = if a.0 == StatusCode::OK {
        (one, a.1)
    } else {
        (two, b.1)
    };
    let (st, replay) = submit(winner.clone()).await;
    assert_eq!(st, StatusCode::OK, "{replay}");
    assert_eq!(replay["idempotencyReplay"], true);
    assert_eq!(replay["eventId"], receipt_body["eventId"]);
    assert_eq!(replay["outstandingMinor"], 10000);
    assert_eq!(events(&s, &invoice).await, 1);
    assert_eq!(
        balance(&s, &invoice).await,
        ("issued".into(), 20000, 10000, 3)
    );
    // The same key with another amount is a conflict, not a second receipt.
    let mut changed = winner;
    changed["amountMinor"] = json!(10000);
    let (st, v) = submit(changed).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert_eq!(events(&s, &invoice).await, 1);
}

/// **Witness: the database holds a direct insert by the serving role to the
/// same rules, and the manual facts cannot be altered or removed. The
/// replaced bridge functions kept their signatures, the source readiness
/// still passes, and the receipt readiness sees a disabled guard.**
#[tokio::test]
async fn the_database_guards_manual_events_and_readiness_covers_the_guards() {
    let s = SourceFixture::new().await;
    s.enable().await;
    let (today, _, tomorrow) = days(&s.f).await;
    let (invoice, _) = issued(&s, "MANUAL-GUARD").await;
    let id = invoice["invoiceId"].as_str().unwrap().to_owned();
    // One statement as the serving role, tenant bound, in its own transaction.
    let insert = |columns: &'static str, values: String| {
        let (pool, id) = (s.f.runtime_pool.clone(), id.clone());
        async move {
            let mut tx = pool.begin().await.unwrap();
            sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
                .execute(&mut *tx)
                .await
                .unwrap();
            let statement = format!(
                "INSERT INTO archava_billing.payment_events(tenant_id,id,invoice_id,outcome,currency,recorded_by,{columns}) \
                 VALUES('acme','00000000-0000-4000-8000-0000000000aa','{id}','succeeded','IDR','alice',{values})"
            );
            let result = sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
                .execute(&mut *tx)
                .await;
            let _ = tx.rollback().await;
            result
                .map(|_| ())
                .map_err(|error| super::tax_sales::guard_message(&error))
        }
    };
    let facts =
        "provider_id,provider_event_id,amount_minor,manual_method,received_on,reference,reason";
    let event = "'manual:00000000-0000-4000-8000-0000000000aa'";
    let refused = |message: &str| Err(("23514".to_owned(), message.to_owned()));
    // A manual provider id without the manual facts: the provider-event shape.
    assert_eq!(
        insert(
            "provider_id,provider_event_id,amount_minor",
            "'manual:cash','evt-1',30000".into()
        )
        .await,
        refused("A manual payment event needs its method, reason and recording person")
    );
    // Manual facts under another provider's id, or another event id.
    for values in [
        format!("'stripe-test',{event},30000,'cash','{today}','R','Because'"),
        format!("'manual:bank_transfer',{event},30000,'cash','{today}','R','Because'"),
        format!("'manual:cash','evt-1',30000,'cash','{today}','R','Because'"),
        format!("'manual:cash',{event},30000,'cash',NULL,'R','Because'"),
        format!("'manual:cash',{event},30000,'cash','{today}',NULL,'Because'"),
        format!("'manual:cash',{event},30000,'cash','{today}','R',NULL"),
        format!("'manual:cash',{event},0,'cash','{today}','R','Because'"),
    ] {
        let (code, message) = insert(facts, values.clone()).await.unwrap_err();
        assert_eq!(code, "23514", "{values} {message}");
        assert!(
            message.contains("payment_event_manual_facts"),
            "{values} {message}"
        );
    }
    // Complete facts are still held to the receipt's rules.
    assert_eq!(
        insert(
            facts,
            format!("'manual:cash',{event},30000,'cash','{tomorrow}','R','Because'")
        )
        .await,
        refused("A manual receipt cannot be dated in the future")
    );
    assert_eq!(
        insert(
            facts,
            format!("'manual:cash',{event},30000,'cash','1999-12-31','R','Because'")
        )
        .await,
        refused("A manual receipt cannot be dated before its invoice was issued")
    );
    // Recorded by someone who may not settle, or at another time than now.
    sqlx::query("INSERT INTO archava_billing.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','bob','human',ARRAY['billing.read','billing.write'],'all')").execute(&s.f.admin_pool).await.unwrap();
    for values in [
        format!("'manual:cash',{event},30000,'cash','{today}','R','Because','bob',clock_timestamp()"),
        format!("'manual:cash',{event},30000,'cash','{today}','R','Because','alice',clock_timestamp()-interval '2 days'"),
    ] {
        let (pool, id) = (s.f.runtime_pool.clone(), id.clone());
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
            .execute(&mut *tx)
            .await
            .unwrap();
        let statement = format!(
            "INSERT INTO archava_billing.payment_events(tenant_id,id,invoice_id,outcome,currency,{facts},recorded_by,verified_at) \
             VALUES('acme','00000000-0000-4000-8000-0000000000aa','{id}','succeeded','IDR',{values})"
        );
        let error = sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .execute(&mut *tx)
            .await
            .unwrap_err();
        assert_eq!(
            super::tax_sales::guard_message(&error),
            (
                "23514".to_owned(),
                "A manual payment event needs its method, reason and recording person".to_owned()
            )
        );
    }
    assert_eq!(events(&s, &invoice).await, 0);
    // A reversal must name a manual receipt of the same invoice.
    let gateway = s.payment(&invoice, "succeeded", 5000).await;
    let reversal_of = |target: String, amount: i64| {
        let (pool, id) = (s.f.runtime_pool.clone(), id.clone());
        async move {
            let mut tx = pool.begin().await.unwrap();
            sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
                .execute(&mut *tx)
                .await
                .unwrap();
            let statement = format!(
                "INSERT INTO archava_billing.payment_events(tenant_id,id,invoice_id,outcome,currency,recorded_by,provider_id,provider_event_id,amount_minor,manual_method,reason,reverses_event_id) \
                 VALUES('acme','00000000-0000-4000-8000-0000000000aa','{id}','refunded','IDR','alice','manual:cash','manual:00000000-0000-4000-8000-0000000000aa',{amount},'cash','Because','{target}')"
            );
            let error = sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
                .execute(&mut *tx)
                .await
                .unwrap_err();
            super::tax_sales::guard_message(&error)
        }
    };
    assert_eq!(
        reversal_of(gateway["eventId"].as_str().unwrap().to_owned(), 5000).await,
        (
            "23514".to_owned(),
            "Only a manual receipt of the same invoice can be reversed".to_owned()
        )
    );
    let paid = s.bill(receipt(&invoice, 3, 10000, "cash", &today)).await;
    assert_eq!(
        reversal_of(paid["eventId"].as_str().unwrap().to_owned(), 10001).await,
        (
            "23514".to_owned(),
            "A manual receipt reversal cannot exceed the unreversed amount of its receipt"
                .to_owned()
        )
    );

    // The stored facts cannot be changed or removed: not by the serving role
    // (it holds no such grant) and not by the table owner (the trigger).
    for statement in [
        "UPDATE archava_billing.payment_events SET reference='REWRITTEN' WHERE id=$1::uuid",
        "UPDATE archava_billing.payment_events SET received_on=received_on-1 WHERE id=$1::uuid",
        "UPDATE archava_billing.payment_events SET manual_method=NULL,received_on=NULL,reference=NULL,reason=NULL WHERE id=$1::uuid",
        "DELETE FROM archava_billing.payment_events WHERE id=$1::uuid",
    ] {
        let mut tx = s.f.runtime_pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = sqlx::query(statement)
            .bind(paid["eventId"].as_str().unwrap())
            .execute(&mut *tx)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501"),
            "{statement} {error}"
        );
        drop(tx);
        let mut tx = s.f.owner_pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = sqlx::query(statement)
            .bind(paid["eventId"].as_str().unwrap())
            .execute(&mut *tx)
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("Billing evidence is append-only"),
            "{statement} {error}"
        );
    }
    let kept: (String, String) = sqlx::query_as(
        "SELECT reference,received_on::text FROM archava_billing.payment_events WHERE id=$1::uuid",
    )
    .bind(paid["eventId"].as_str().unwrap())
    .fetch_one(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(kept, ("REF-10000".into(), today));

    // Readiness: the replaced functions kept their identity, and the receipt
    // guards are seen through the serving role.
    let signatures: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT p.proname::text,pg_get_function_identity_arguments(p.oid),pg_get_function_result(p.oid) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace \
         WHERE n.nspname='archava_accounting' AND p.proname IN ('invoice_effect','invoice_effect_date','guard_invoice_source','capture_invoice_source') ORDER BY 1",
    )
    .fetch_all(&s.f.admin_pool)
    .await
    .unwrap();
    let own = |a: &str, b: &str, c: &str| (a.to_owned(), b.to_owned(), c.to_owned());
    assert_eq!(
        signatures,
        vec![
            own("capture_invoice_source", "", "trigger"),
            own("guard_invoice_source", "", "trigger"),
            own(
                "invoice_effect",
                "wanted_tenant text, wanted_request uuid",
                "TABLE(effect text, whole_idr numeric)"
            ),
            own(
                "invoice_effect_date",
                "wanted_tenant text, wanted_request uuid",
                "date"
            ),
        ]
    );
    let ready = |query: &'static str| {
        let pool = s.f.runtime_pool.clone();
        async move {
            sqlx::query_scalar::<_, bool>(query)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    assert!(ready("SELECT archava_accounting.source_guards_complete() AND archava_accounting.clearing_guards_complete() AND archava_accounting.tax_document_guards_complete()").await);
    assert!(ready("SELECT archava_accounting.receipt_guards_complete()").await);
    for (trigger, restore) in [
        ("payment_event_manual_guard", "ENABLE"),
        ("events_immutable", "ENABLE"),
    ] {
        let disable =
            format!("ALTER TABLE archava_billing.payment_events DISABLE TRIGGER {trigger}");
        sqlx::query(sqlx::AssertSqlSafe(disable.as_str()))
            .execute(&s.f.admin_pool)
            .await
            .unwrap();
        assert!(
            !ready("SELECT archava_accounting.receipt_guards_complete()").await,
            "{trigger}"
        );
        let enable =
            format!("ALTER TABLE archava_billing.payment_events {restore} TRIGGER {trigger}");
        sqlx::query(sqlx::AssertSqlSafe(enable.as_str()))
            .execute(&s.f.admin_pool)
            .await
            .unwrap();
        assert!(
            ready("SELECT archava_accounting.receipt_guards_complete()").await,
            "{trigger}"
        );
    }
    // The host refuses to come up while a guard is off.
    sqlx::query(
        "ALTER TABLE archava_billing.payment_events DISABLE TRIGGER payment_event_manual_guard",
    )
    .execute(&s.f.admin_pool)
    .await
    .unwrap();
    assert!(AppState::try_new_all(
        s.f.runtime_pool.clone(),
        ledger(),
        AllReadiness {
            commerce: &s.f.commerce_ledger,
            identity: &s.f.identity_ledger,
            crm: &s.f.crm_ledger,
            owner: &s.f.owner_ledger,
            purchasing: &s.f.purchasing_ledger,
            additional_erp: &s.f.erp_readiness(),
        },
        &[7; 32],
    )
    .await
    .is_err());
}
