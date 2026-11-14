//! A taxed sales invoice posts its tax to the ledger (TAX.md stage C). The
//! invoice bridge captures the tax part of every effect beside its amount, with
//! the control account of the tax code version stored on the invoice; the
//! reviewed draft has three legs: receivable (gross), revenue (net) and the tax
//! control account (tax). A source without tax is captured and drafted as before.
use super::invoice_sources::SourceFixture;
use super::tax_sales::{
    confirmed_code, correct, count, fulfilled, guard_message, invoice_row, invoiced, order,
    output_account, product, stored,
};
use super::*;

/// What the bridge stored for one billing command: effect, amount, tax, and
/// the tax control account with its revision.
type Captured = (String, i64, i64, Option<String>, Option<i32>);
async fn captured(s: &SourceFixture, command: &Value) -> Captured {
    sqlx::query_as("SELECT effect,amount_minor,tax_minor,tax_account_id::text,tax_account_revision FROM archava_accounting.invoice_sources WHERE id=$1::uuid")
        .bind(command["commandId"].as_str().unwrap())
        .fetch_one(&s.f.admin_pool)
        .await
        .unwrap()
}
fn reference(account: &Value) -> Value {
    json!({"accountId":account["resourceId"],"expectedAccountRevision":account["revision"]})
}
/// `invoice_source.draft` naming exactly these reviewed accounts.
fn draft_with(source: &Value, code: &str, accounts: &[&Value]) -> Value {
    let accounts: Vec<Value> = accounts.iter().map(|a| reference(a)).collect();
    input(
        json!({"type":"invoice_source.draft","sourceId":source["commandId"],"code":code,"accounts":accounts,"reason":"Review captured amount, tax, source date and account roles"}),
    )
}
/// Drafts a taxed source with its three accounts and has bob post it.
pub(super) async fn post_taxed(
    s: &SourceFixture,
    source: &Value,
    code: &str,
    tax_account: &Value,
) -> Value {
    let request = draft_with(source, code, &[&s.ar, &s.revenue, tax_account]);
    let (st, v) = send(&s.app, &s.alice, &request).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, posted) = send(&s.app, &s.bob, &action("journal.post", &v["receipt"])).await;
    assert_eq!(st, StatusCode::OK, "{posted}");
    v["receipt"].clone()
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
/// The posted lines of one journal: account code, debit, credit, in order.
async fn lines(s: &SourceFixture, journal: &Value) -> Vec<(String, i64, i64)> {
    sqlx::query_as("SELECT account_code,debit_minor,credit_minor FROM archava_accounting.journal_lines WHERE journal_id=$1::uuid ORDER BY ordinal")
        .bind(journal["resourceId"].as_str().unwrap())
        .fetch_all(&s.f.admin_pool)
        .await
        .unwrap()
}
fn row(code: &str, debit: i64, credit: i64) -> (String, i64, i64) {
    (code.to_owned(), debit, credit)
}
pub(super) async fn issue(s: &SourceFixture, invoice: &Value) -> Value {
    s.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Issue the taxed invoice"})).await
}
/// A fulfilled order of 2 × `item` under `tax`, invoiced as a draft.
pub(super) async fn taxed_invoice(
    s: &SourceFixture,
    code: &str,
    item: &Value,
    quantity: i64,
    tax: Option<(&str, bool)>,
) -> Value {
    let o = fulfilled(s, order(s, item, quantity, tax)).await;
    invoiced(s, code, &o).await
}
/// The source row as the Accounting workspace list returns it.
async fn listed(s: &SourceFixture, source: &Value) -> Value {
    let (st, v) = call(
        &s.app,
        "GET",
        &format!(
            "/erp/accounting/workspace?tenant=acme&companyId={}&collection=invoice_sources",
            s.company["companyId"].as_str().unwrap()
        ),
        Value::Null,
        Some(&s.alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["invoice_sources"]
        .as_array()
        .unwrap_or_else(|| panic!("{v}"))
        .iter()
        .find(|r| r["id"] == source["commandId"])
        .cloned()
        .unwrap_or_else(|| panic!("{v}"))
}
async fn close_blockers(s: &SourceFixture) -> (bool, Vec<(String, i64)>) {
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
async fn utc_today(s: &SourceFixture) -> String {
    sqlx::query_scalar("SELECT (now() AT TIME ZONE 'UTC')::date::text")
        .fetch_one(&s.f.runtime_pool)
        .await
        .unwrap()
}

/// **Witness: on an IDR book with a `whole_idr` policy, an exclusive 11% code
/// on 2 × 15,000 is captured as 3,330,000 with 330,000 of tax on the tax
/// code's control account; the draft needs the three accounts and is posted by
/// a second person; receivable is +3,330,000, revenue −3,000,000 and output tax
/// −330,000; the trial balance balances, the receivable control comparison has
/// no difference and profit and loss shows revenue 3,000,000. The unposted
/// taxed source blocks the period close. A payment in full then clears the
/// receivable in gross against clearing. An untaxed invoice of the same book
/// still drafts with exactly two accounts. The bridge guards are complete
/// after the functions were replaced.**
#[tokio::test]
async fn a_taxed_invoice_posts_receivable_revenue_and_output_tax_and_is_paid_in_gross() {
    let s = SourceFixture::new().await;
    // The replaced functions kept their identity: the readiness check that
    // compares each trigger's function by OID holds through the serving role,
    // and the host of this fixture started on it. No overload was created.
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT archava_accounting.source_guards_complete()")
            .fetch_one(&s.f.runtime_pool)
            .await
            .unwrap()
    );
    let signatures: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT p.proname::text,pg_get_function_identity_arguments(p.oid),pg_get_function_result(p.oid) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace \
         WHERE (n.nspname='archava_accounting' AND p.proname IN ('invoice_effect','invoice_effect_tax','guard_invoice_source','capture_invoice_source','guard_journal_source')) \
            OR (n.nspname='archava_billing' AND p.proname='guard_invoice_tax') ORDER BY 1",
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
            own("guard_invoice_tax", "", "trigger"),
            own("guard_journal_source", "", "trigger"),
            own(
                "invoice_effect",
                "wanted_tenant text, wanted_request uuid",
                "TABLE(effect text, whole_idr numeric)"
            ),
            own(
                "invoice_effect_tax",
                "wanted_tenant text, wanted_request uuid",
                "numeric"
            ),
        ]
    );

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
    s.enable().await;
    let item = product(&s, "TEA", 15_000).await;

    // A taxed invoice is created and issued on a book that posts invoices.
    let invoice = taxed_invoice(&s, "TAX-POST", &item, 2, Some(("VAT", false))).await;
    assert_eq!(invoice["outstandingMinor"], 33_300);
    let issued = issue(&s, &invoice).await;
    assert_eq!(issued["status"], "issued");
    assert_eq!(
        captured(&s, &issued).await,
        (
            "issue".into(),
            3_330_000,
            330_000,
            Some(vat_account["resourceId"].as_str().unwrap().to_owned()),
            Some(1)
        )
    );
    // The workspace row carries the tax and its account for the review.
    let listed_row = listed(&s, &issued).await;
    assert_eq!(listed_row["amountMinor"], "3330000");
    assert_eq!(listed_row["taxMinor"], "330000");
    assert_eq!(listed_row["taxAccountId"], vat_account["resourceId"]);
    assert_eq!(listed_row["taxAccountRevision"], 1);
    assert_eq!(listed_row["sourceState"], "pending");

    // The unposted taxed source blocks the close, as any other source does.
    assert_eq!(
        close_blockers(&s).await,
        (false, vec![("unposted_invoice_sources".to_owned(), 1)])
    );
    assert_eq!(
        send(&s.app, &s.alice, &action("period.close", &s.period))
            .await
            .0,
        StatusCode::CONFLICT
    );

    // The draft of a taxed source needs its three accounts.
    let (st, v) = send(&s.app, &s.alice, &s.source_draft(&issued, "TWO")).await;
    assert_refused_as(st, &v, "invoice_source_tax_accounts_required");
    let wrong = draft_with(&issued, "WRONG", &[&s.ar, &s.revenue, &s.clearing]);
    let (st, v) = send(&s.app, &s.alice, &wrong).await;
    assert_refused_as(st, &v, "invoice_source_tax_accounts_required");
    let no_revenue = draft_with(&issued, "NO-REVENUE", &[&s.ar, &s.clearing, &vat_account]);
    let (st, v) = send(&s.app, &s.alice, &no_revenue).await;
    assert_refused_as(st, &v, "invoice_source_tax_accounts_required");
    assert_eq!(count(&s.f, "archava_accounting.journals").await, 0);

    let request = draft_with(&issued, "ISSUE", &[&s.ar, &s.revenue, &vat_account]);
    let (st, drafted) = send(&s.app, &s.alice, &request).await;
    assert_eq!(st, StatusCode::OK, "{drafted}");
    assert_eq!(drafted["receipt"]["sourceId"], issued["commandId"]);
    assert_eq!(
        lines(&s, &drafted["receipt"]).await,
        vec![
            row("AR", 3_330_000, 0),
            row("REVENUE", 0, 3_000_000),
            row("VAT-OUT", 0, 330_000),
        ]
    );
    assert_eq!(
        close_blockers(&s).await,
        (
            false,
            vec![
                ("draft_journals".to_owned(), 1),
                ("unposted_invoice_sources".to_owned(), 1)
            ]
        )
    );
    // Independent posting: the person who drafted cannot post.
    assert_eq!(
        send(
            &s.app,
            &s.alice,
            &action("journal.post", &drafted["receipt"])
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (st, posted) = send(&s.app, &s.bob, &action("journal.post", &drafted["receipt"])).await;
    assert_eq!(st, StatusCode::OK, "{posted}");
    assert_eq!(close_blockers(&s).await, (true, vec![]));
    assert_eq!(listed(&s, &issued).await["sourceState"], "posted");

    let r = s.reports().await;
    assert_eq!(closing(&r, "AR"), "3330000");
    assert_eq!(closing(&r, "REVENUE"), "-3000000");
    assert_eq!(closing(&r, "VAT-OUT"), "-330000");
    assert_eq!(r["trialBalance"]["balanced"], true);
    assert_eq!(r["trialBalance"]["debitMinor"], "3330000");
    assert_eq!(r["profitAndLoss"]["revenueMinor"], "3000000");
    assert_eq!(r["profitAndLoss"]["netProfitMinor"], "3000000");
    assert_eq!(r["balanceSheet"]["assetsMinor"], "3330000");
    assert_eq!(r["balanceSheet"]["liabilitiesMinor"], "330000");
    assert_eq!(r["balanceSheet"]["balanced"], true);
    assert_eq!(r["sourceCoverage"]["postedCount"], 1);
    assert_eq!(r["sourceCoverage"]["unpostedCount"], 0);
    // The receivable subledger and its control account agree in gross.
    let today = utc_today(&s).await;
    let receivable = subledger::accepted(&s.app, &s.alice, &s.book, "receivable", &today).await;
    assert_eq!(receivable["totals"]["netOutstandingMinor"], "3330000");
    assert_eq!(receivable["controls"].as_array().unwrap().len(), 1);
    assert_eq!(receivable["controls"][0]["expectedMinor"], "3330000");
    assert_eq!(receivable["controls"][0]["actualMinor"], "3330000");
    assert_eq!(receivable["controls"][0]["differenceMinor"], "0");

    // An untaxed invoice of the same book: captured without tax, drafted with
    // exactly two accounts; a third account is refused.
    let plain = taxed_invoice(&s, "PLAIN", &item, 2, None).await;
    let plain_issue = issue(&s, &plain).await;
    assert_eq!(
        captured(&s, &plain_issue).await,
        ("issue".into(), 3_000_000, 0, None, None)
    );
    let plain_row = listed(&s, &plain_issue).await;
    for key in ["taxMinor", "taxAccountId", "taxAccountRevision"] {
        assert!(plain_row.get(key).is_none(), "{key}: {plain_row}");
    }
    let three = draft_with(&plain_issue, "THREE", &[&s.ar, &s.revenue, &vat_account]);
    let (st, v) = send(&s.app, &s.alice, &three).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v.get("detail").is_none(), "{v}");
    let plain_journal = s.post_source(&plain_issue, "PLAIN-ISSUE").await;
    assert_eq!(
        lines(&s, &plain_journal).await,
        vec![row("AR", 3_000_000, 0), row("REVENUE", 0, 3_000_000)]
    );

    // A payment in full of the taxed invoice: receivable against clearing in
    // gross, two accounts, no tax.
    let paid = s.payment(&invoice, "succeeded", 33_300).await;
    assert_eq!(paid["status"], "settled");
    assert_eq!(
        captured(&s, &paid).await,
        ("collection".into(), 3_330_000, 0, None, None)
    );
    let collection = s.post_source(&paid, "COLLECTION").await;
    assert_eq!(
        lines(&s, &collection).await,
        vec![row("CLEARING", 3_330_000, 0), row("AR", 0, 3_330_000)]
    );
    let r = s.reports().await;
    // Only the untaxed invoice is still owed.
    assert_eq!(closing(&r, "AR"), "3000000");
    assert_eq!(closing(&r, "CLEARING"), "3330000");
    assert_eq!(closing(&r, "REVENUE"), "-6000000");
    assert_eq!(closing(&r, "VAT-OUT"), "-330000");
    assert_eq!(r["trialBalance"]["balanced"], true);
    let receivable = subledger::accepted(&s.app, &s.alice, &s.book, "receivable", &today).await;
    assert_eq!(receivable["totals"]["netOutstandingMinor"], "3000000");
    assert_eq!(receivable["controls"][0]["differenceMinor"], "0");
    assert_eq!(
        send(&s.app, &s.alice, &action("period.close", &s.period))
            .await
            .0,
        StatusCode::OK
    );
}

/// **Witness: prices that include the tax, and 12% of eleven twelfths of the
/// price, are captured with exactly the tax the invoice stores times the unit
/// factor, and post it; a 0% code is captured with tax 0 and drafts with two
/// legs and no tax leg.**
#[tokio::test]
async fn inclusive_scaled_and_zero_rate_codes_post_the_tax_their_invoice_stores() {
    let s = SourceFixture::new().await;
    let vat_account = output_account(&s).await;
    for (code, rate, base) in [
        ("VAT", (11, 100), (1, 1)),
        ("VAT-DPP", (12, 100), (11, 12)),
        ("ZERO", (0, 100), (1, 1)),
    ] {
        confirmed_code(&s, code, "output", rate, base, &vat_account, "2020-01-01").await;
    }
    s.enable().await;
    let item = product(&s, "TEA", 15_000).await;
    let account_id = vat_account["resourceId"].as_str().unwrap().to_owned();

    // Inclusive: 30,000 contains 2,973 of tax.
    let inclusive = taxed_invoice(&s, "INCL", &item, 2, Some(("VAT", true))).await;
    let inclusive_issue = issue(&s, &inclusive).await;
    let (gross, _, tax, ..) = stored(&s, &inclusive).await;
    assert_eq!((gross, tax), (30_000, 2_973));
    assert_eq!(
        captured(&s, &inclusive_issue).await,
        (
            "issue".into(),
            gross * 100,
            tax * 100,
            Some(account_id.clone()),
            Some(1)
        )
    );
    let journal = post_taxed(&s, &inclusive_issue, "INCL", &vat_account).await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![
            row("AR", 3_000_000, 0),
            row("REVENUE", 0, 2_702_700),
            row("VAT-OUT", 0, 297_300),
        ]
    );

    // 12% of 11/12 of 30,000 is 3,300, the same tax as 11% of it.
    let scaled = taxed_invoice(&s, "DPP", &item, 2, Some(("VAT-DPP", false))).await;
    let scaled_issue = issue(&s, &scaled).await;
    let (gross, _, tax, ..) = stored(&s, &scaled).await;
    assert_eq!((gross, tax), (33_300, 3_300));
    assert_eq!(
        captured(&s, &scaled_issue).await,
        (
            "issue".into(),
            gross * 100,
            tax * 100,
            Some(account_id.clone()),
            Some(1)
        )
    );
    let journal = post_taxed(&s, &scaled_issue, "DPP", &vat_account).await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![
            row("AR", 3_330_000, 0),
            row("REVENUE", 0, 3_000_000),
            row("VAT-OUT", 0, 330_000),
        ]
    );

    // A 0% code: the invoice is taxed with nothing. The source keeps the
    // code's control account, has tax 0 and needs no tax leg.
    let zero = taxed_invoice(&s, "ZERO-RATE", &item, 2, Some(("ZERO", false))).await;
    let zero_issue = issue(&s, &zero).await;
    assert_eq!(stored(&s, &zero).await, (30_000, 0, 0, 0, 30_000, 0));
    assert_eq!(
        captured(&s, &zero_issue).await,
        ("issue".into(), 3_000_000, 0, Some(account_id), Some(1))
    );
    let three = draft_with(
        &zero_issue,
        "ZERO-THREE",
        &[&s.ar, &s.revenue, &vat_account],
    );
    let (st, v) = send(&s.app, &s.alice, &three).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let journal = s.post_source(&zero_issue, "ZERO").await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![row("AR", 3_000_000, 0), row("REVENUE", 0, 3_000_000)]
    );
    // A correction of the 0% invoice carries no tax either.
    let credit = correct(&s, &zero, -10_000).await;
    assert_eq!(captured(&s, &credit).await.2, 0);
    let journal = s.post_source(&credit, "ZERO-CREDIT").await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![row("REVENUE", 1_000_000, 0), row("AR", 0, 1_000_000)]
    );

    let r = s.reports().await;
    assert_eq!(closing(&r, "AR"), "8330000");
    assert_eq!(closing(&r, "REVENUE"), "-7702700");
    assert_eq!(closing(&r, "VAT-OUT"), "-627300");
    assert_eq!(r["trialBalance"]["balanced"], true);
    assert_eq!(r["profitAndLoss"]["revenueMinor"], "7702700");
    assert_eq!(r["sourceCoverage"]["unpostedCount"], 0);
}

/// **Witness: a USD book with a `minor` policy posts 9.99 of revenue and 1.10
/// of tax unchanged: the unit factor is 1 for the gross and for the tax.**
#[tokio::test]
async fn a_usd_book_posts_tax_in_minor_units_unchanged() {
    let s = SourceFixture::new_in("USD").await;
    let vat_account = output_account(&s).await;
    confirmed_code(
        &s,
        "SALES-TAX",
        "output",
        (11, 100),
        (1, 1),
        &vat_account,
        "2020-01-01",
    )
    .await;
    s.enable().await;
    let item = product(&s, "MUG", 333).await;
    let invoice = taxed_invoice(&s, "USD-TAX", &item, 3, Some(("SALES-TAX", false))).await;
    assert_eq!(invoice["outstandingMinor"], 1_109);
    let issued = issue(&s, &invoice).await;
    assert_eq!(
        captured(&s, &issued).await,
        (
            "issue".into(),
            1_109,
            110,
            Some(vat_account["resourceId"].as_str().unwrap().to_owned()),
            Some(1)
        )
    );
    let journal = post_taxed(&s, &issued, "USD-ISSUE", &vat_account).await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![
            row("AR", 1_109, 0),
            row("REVENUE", 0, 999),
            row("VAT-OUT", 0, 110),
        ]
    );
    // A credit of 1.11 carries 0.11 of tax.
    let credit = correct(&s, &invoice, -111).await;
    assert_eq!(
        (captured(&s, &credit).await.1, captured(&s, &credit).await.2),
        (-111, -11)
    );
    let journal = post_taxed(&s, &credit, "USD-CREDIT", &vat_account).await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![
            row("REVENUE", 100, 0),
            row("VAT-OUT", 11, 0),
            row("AR", 0, 111),
        ]
    );
    let r = s.reports().await;
    assert_eq!(closing(&r, "AR"), "998");
    assert_eq!(closing(&r, "REVENUE"), "-899");
    assert_eq!(closing(&r, "VAT-OUT"), "-99");
    assert_eq!(r["trialBalance"]["balanced"], true);
    assert_eq!(r["profitAndLoss"]["revenueMinor"], "899");
    let today = utc_today(&s).await;
    let receivable = subledger::accepted(&s.app, &s.alice, &s.book, "receivable", &today).await;
    assert_eq!(receivable["totals"]["netOutstandingMinor"], "998");
    assert_eq!(receivable["controls"][0]["differenceMinor"], "0");
}

/// **Witness: a partial correction and a full reversal of a taxed invoice post
/// three legs each, with the tax part Billing stored on each correction, and
/// bring receivable, revenue and output tax back to exactly zero; a void of a
/// second taxed invoice reverses what its issue posted.**
#[tokio::test]
async fn corrections_and_a_void_of_a_taxed_invoice_reverse_net_and_tax_to_zero() {
    let s = SourceFixture::new().await;
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
    s.enable().await;
    let item = product(&s, "TEA", 15_000).await;
    let account_id = vat_account["resourceId"].as_str().unwrap().to_owned();

    let invoice = taxed_invoice(&s, "TAX-CREDIT", &item, 2, Some(("VAT", false))).await;
    let issued = issue(&s, &invoice).await;
    post_taxed(&s, &issued, "ISSUE", &vat_account).await;

    let partial = correct(&s, &invoice, -11_100).await;
    assert_eq!(
        captured(&s, &partial).await,
        (
            "correction".into(),
            -1_110_000,
            -110_000,
            Some(account_id.clone()),
            Some(1)
        )
    );
    // Two accounts are not enough for a correction that carries tax.
    let (st, v) = send(&s.app, &s.alice, &s.source_draft(&partial, "CREDIT-TWO")).await;
    assert_refused_as(st, &v, "invoice_source_tax_accounts_required");
    let journal = post_taxed(&s, &partial, "CREDIT", &vat_account).await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![
            row("REVENUE", 1_000_000, 0),
            row("VAT-OUT", 110_000, 0),
            row("AR", 0, 1_110_000),
        ]
    );
    let r = s.reports().await;
    assert_eq!(closing(&r, "AR"), "2220000");
    assert_eq!(closing(&r, "REVENUE"), "-2000000");
    assert_eq!(closing(&r, "VAT-OUT"), "-220000");

    // A correction of one unit: 22,199 contains 2,200 of tax (22,199 / 1.11 =
    // 19,999.1), so this unit is all net and has two legs.
    let unit = correct(&s, &invoice, -1).await;
    assert_eq!(
        captured(&s, &unit).await,
        (
            "correction".into(),
            -100,
            0,
            Some(account_id.clone()),
            Some(1)
        )
    );
    let journal = s.post_source(&unit, "UNIT").await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![row("REVENUE", 100, 0), row("AR", 0, 100)]
    );

    // The full reversal of what is left carries the rest of the tax.
    let rest = correct(&s, &invoice, -22_199).await;
    assert_eq!(rest["outstandingMinor"], 0);
    assert_eq!(
        stored(&s, &invoice).await,
        (33_300, -33_300, 3_300, -3_300, 0, -3_300)
    );
    assert_eq!(
        captured(&s, &rest).await,
        (
            "correction".into(),
            -2_219_900,
            -220_000,
            Some(account_id.clone()),
            Some(1)
        )
    );
    let journal = post_taxed(&s, &rest, "REVERSAL", &vat_account).await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![
            row("REVENUE", 1_999_900, 0),
            row("VAT-OUT", 220_000, 0),
            row("AR", 0, 2_219_900),
        ]
    );
    let r = s.reports().await;
    for code in ["AR", "REVENUE", "VAT-OUT"] {
        assert_eq!(closing(&r, code), "0", "{code}");
    }
    assert_eq!(r["profitAndLoss"]["revenueMinor"], "0");
    assert_eq!(r["balanceSheet"]["liabilitiesMinor"], "0");
    assert_eq!(r["trialBalance"]["balanced"], true);

    // A second taxed invoice is issued and voided: the void reverses the
    // gross, the net and the tax the issue posted.
    let second = taxed_invoice(&s, "TAX-VOID", &item, 2, Some(("VAT", true))).await;
    let second_issue = issue(&s, &second).await;
    post_taxed(&s, &second_issue, "ISSUE-2", &vat_account).await;
    let void = s.bill(json!({"type":"void_invoice","invoiceId":second["invoiceId"],"expectedRevision":2,"reason":"Cancel the taxed invoice"})).await;
    assert_eq!(
        captured(&s, &void).await,
        (
            "void".into(),
            -3_000_000,
            -297_300,
            Some(account_id),
            Some(1)
        )
    );
    let journal = post_taxed(&s, &void, "VOID", &vat_account).await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![
            row("REVENUE", 2_702_700, 0),
            row("VAT-OUT", 297_300, 0),
            row("AR", 0, 3_000_000),
        ]
    );
    let r = s.reports().await;
    for code in ["AR", "REVENUE", "VAT-OUT"] {
        assert_eq!(closing(&r, code), "0", "{code}");
    }
    assert_eq!(r["trialBalance"]["balanced"], true);
    assert_eq!(r["sourceCoverage"]["unpostedCount"], 0);
    let today = utc_today(&s).await;
    let receivable = subledger::accepted(&s.app, &s.alice, &s.book, "receivable", &today).await;
    assert_eq!(receivable["totals"]["netOutstandingMinor"], "0");
    assert_eq!(receivable["controls"][0]["differenceMinor"], "0");
    assert_eq!(
        send(&s.app, &s.alice, &action("period.close", &s.period))
            .await
            .0,
        StatusCode::OK
    );
}

/// One direct insert of a correction source as the serving role, copying the
/// issue source like the capture does, with the stated tax and tax account.
async fn direct_source(
    s: &SourceFixture,
    correction: &Value,
    amount: i64,
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
    let attempt = sqlx::query("INSERT INTO archava_accounting.invoice_sources(tenant_id,id,book_id,policy_id,policy_revision,policy_snapshot,invoice_id,invoice_code,invoice_revision,effect,amount_minor,actor_id,occurred_at,source_date,tax_minor,tax_account_id,tax_account_revision) \
        SELECT o.tenant_id,c.request_id,o.book_id,o.policy_id,o.policy_revision,o.policy_snapshot,o.invoice_id,o.invoice_code,(c.receipt->>'revision')::integer,'correction',$2,c.actor_id,(c.receipt->>'executedAt')::timestamptz,((c.receipt->>'executedAt')::timestamptz AT TIME ZONE 'UTC')::date,$3,$4::uuid,CASE WHEN $4 IS NULL THEN NULL ELSE 1 END \
        FROM archava_billing.commands c JOIN archava_accounting.invoice_sources o ON o.tenant_id=c.tenant_id AND o.invoice_id=(c.receipt->>'invoiceId')::uuid AND o.effect='issue' WHERE c.request_id=$1::uuid")
        .bind(correction["commandId"].as_str().unwrap())
        .bind(amount)
        .bind(tax)
        .bind(account)
        .execute(tx.checked_executor().await.unwrap())
        .await;
    tx.rollback().await.unwrap();
    attempt.map(|done| assert_eq!(done.rows_affected(), 1))
}

/// **Witness: the capture refuses, and rolls the billing command back, while
/// the tax control account is inactive (the invoice stays a draft and no
/// source is stored; reactivating the account releases it), when the tax control
/// account is an account of the posting policy, and when the tax code is not a
/// confirmed output code of the book. A taxed invoice issued before the book
/// had a policy is never captured, nor are its corrections and payments. The
/// database, not the capture, is the authority: as the serving role a source
/// whose tax or tax account disagrees with the invoice cannot be inserted.**
#[tokio::test]
async fn capture_refuses_a_stale_tax_account_and_the_database_refuses_a_source_with_another_tax() {
    let s = SourceFixture::new().await;
    let vat_account = output_account(&s).await;
    let spare = account(
        &s.app,
        &s.alice,
        &s.book,
        "VAT-OLD",
        "liability",
        "operating",
    )
    .await;
    let vat = confirmed_code(
        &s,
        "VAT",
        "output",
        (11, 100),
        (1, 1),
        &vat_account,
        "2020-01-01",
    )
    .await;
    let old = confirmed_code(&s, "OLD", "output", (11, 100), (1, 1), &spare, "2020-01-01").await;
    let item = product(&s, "TEA", 15_000).await;

    // Issued before the book posts invoices: not captured then or later.
    let early = taxed_invoice(&s, "EARLY", &item, 2, Some(("VAT", false))).await;
    issue(&s, &early).await;
    assert_eq!(count(&s.f, "archava_accounting.invoice_sources").await, 0);
    // A draft made before the policy is captured when it is issued under it.
    let waiting = taxed_invoice(&s, "WAITING", &item, 2, Some(("VAT", false))).await;
    s.enable().await;
    correct(&s, &early, -11_100).await;
    s.payment(&early, "succeeded", 22_200).await;
    assert_eq!(count(&s.f, "archava_accounting.invoice_sources").await, 0);
    let waiting_issue = issue(&s, &waiting).await;
    assert_eq!(captured(&s, &waiting_issue).await.2, 330_000);

    // The control account of OLD is deactivated after the code was confirmed.
    // The code still applies, so the order and the draft invoice are made;
    // issuing is refused with its reason and nothing is written.
    let stale = taxed_invoice(&s, "STALE", &item, 2, Some(("OLD", false))).await;
    let off = ok(&s.app,&s.alice,json!({"type":"account.set_active","id":spare["resourceId"],"expectedRevision":1,"active":false,"reason":"Retired control account"})).await;
    let sources = count(&s.f, "archava_accounting.invoice_sources").await;
    let commands = count(&s.f, "archava_billing.commands").await;
    let attempt = json!({"type":"issue_invoice","invoiceId":stale["invoiceId"],"expectedRevision":1,"reason":"Issue with a retired tax account"});
    let (st, v) = super::tax_sales::bill(&s, attempt.clone()).await;
    assert_refused_as(st, &v, "invoice_tax_control_account_changed");
    assert_eq!(invoice_row(&s, &stale).await["status"], "draft");
    assert_eq!(
        count(&s.f, "archava_accounting.invoice_sources").await,
        sources
    );
    assert_eq!(count(&s.f, "archava_billing.commands").await, commands);
    // Reactivating the account releases the draft. The account has a new
    // revision by now; the source stores the revision it has at capture, and
    // the draft is reviewed and posted against it with its three legs.
    let on = ok(&s.app,&s.alice,json!({"type":"account.set_active","id":spare["resourceId"],"expectedRevision":off["revision"],"active":true,"reason":"Restored control account"})).await;
    assert_eq!(on["revision"], 3);
    let (st, released) = super::tax_sales::bill(&s, attempt).await;
    assert_eq!(st, StatusCode::OK, "{released}");
    assert_eq!(invoice_row(&s, &stale).await["status"], "issued");
    assert_eq!(
        captured(&s, &released).await,
        (
            "issue".into(),
            3_330_000,
            330_000,
            Some(spare["resourceId"].as_str().unwrap().to_owned()),
            Some(3)
        )
    );
    let journal = post_taxed(&s, &released, "RELEASED", &on).await;
    assert_eq!(
        lines(&s, &journal).await,
        vec![
            row("AR", 3_330_000, 0),
            row("REVENUE", 0, 3_000_000),
            row("VAT-OLD", 0, 330_000),
        ]
    );
    // A new version of the code with another account serves later invoices.
    let (today,): (String,) = sqlx::query_as("SELECT current_date::text")
        .fetch_one(&s.f.runtime_pool)
        .await
        .unwrap();
    let (yesterday,): (String,) = sqlx::query_as("SELECT (current_date-1)::text")
        .fetch_one(&s.f.runtime_pool)
        .await
        .unwrap();
    ok(&s.app,&s.alice,json!({"type":"tax_code.retire","id":old["resourceId"],"expectedRevision":2,"effectiveTo":yesterday,"reason":"Its control account changed"})).await;
    confirmed_code(&s, "OLD", "output", (11, 100), (1, 1), &vat_account, &today).await;
    let renewed = taxed_invoice(&s, "RENEWED", &item, 2, Some(("OLD", false))).await;
    let renewed_issue = issue(&s, &renewed).await;
    assert_eq!(
        captured(&s, &renewed_issue).await,
        (
            "issue".into(),
            3_330_000,
            330_000,
            Some(vat_account["resourceId"].as_str().unwrap().to_owned()),
            Some(1)
        )
    );

    // A tax control account can never be the policy's receivable or revenue
    // account through the commands: an output code needs a liability account,
    // the policy an asset and a revenue account, and an account keeps its class.
    for (code, reviewed) in [("ON-AR", &s.ar), ("ON-REVENUE", &s.revenue)] {
        let (st, v) = send(&s.app,&s.alice,&input(json!({"type":"tax_code.create","bookId":s.book["bookId"],"code":code,"kind":"output",
            "rate":{"numerator":11,"denominator":100},"controlAccount":reference(reviewed),"effectiveFrom":"2020-01-01","reason":"A tax code on a policy account"}))).await;
        assert_refused_as(st, &v, "tax_code_control_account_wrong_class");
    }
    // The capture guard holds the rule on its own. The database owner plants
    // what no command can store, with the tax code guards switched off.
    let planted = taxed_invoice(&s, "PLANTED", &item, 2, Some(("VAT", false))).await;
    let attempt = json!({"type":"issue_invoice","invoiceId":planted["invoiceId"],"expectedRevision":1,"reason":"Issue against a planted tax code"});
    for (plant, reason) in [
        (
            format!(
                "UPDATE archava_accounting.tax_codes SET control_account_id='{}' WHERE id=$1::uuid",
                s.revenue["resourceId"].as_str().unwrap()
            ),
            "invoice_tax_control_account_in_policy",
        ),
        (
            format!(
                "UPDATE archava_accounting.tax_codes SET control_account_id='{}' WHERE id=$1::uuid",
                s.ar["resourceId"].as_str().unwrap()
            ),
            "invoice_tax_control_account_in_policy",
        ),
        (
            format!(
                "UPDATE archava_accounting.tax_codes SET control_account_id='{}',kind='input' WHERE id=$1::uuid",
                vat_account["resourceId"].as_str().unwrap()
            ),
            "invoice_tax_code_not_postable",
        ),
    ] {
        for statement in [
            "ALTER TABLE archava_accounting.tax_codes DISABLE TRIGGER USER".to_owned(),
            plant,
            "ALTER TABLE archava_accounting.tax_codes ENABLE TRIGGER USER".to_owned(),
        ] {
            let done = sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
                .bind(vat["resourceId"].as_str().unwrap())
                .execute(&s.f.admin_pool)
                .await
                .unwrap_or_else(|e| panic!("{statement}: {e}"));
            assert!(
                !statement.starts_with("UPDATE") || done.rows_affected() == 1,
                "{statement}"
            );
        }
        let (st, v) = super::tax_sales::bill(&s, attempt.clone()).await;
        assert_refused_as(st, &v, reason);
        assert_eq!(invoice_row(&s, &planted).await["status"], "draft");
    }
    for statement in [
        "ALTER TABLE archava_accounting.tax_codes DISABLE TRIGGER USER".to_owned(),
        format!(
            "UPDATE archava_accounting.tax_codes SET control_account_id='{}',kind='output' WHERE id=$1::uuid",
            vat_account["resourceId"].as_str().unwrap()
        ),
        "ALTER TABLE archava_accounting.tax_codes ENABLE TRIGGER USER".to_owned(),
    ] {
        let done = sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .bind(vat["resourceId"].as_str().unwrap())
            .execute(&s.f.admin_pool)
            .await
            .unwrap_or_else(|e| panic!("{statement}: {e}"));
        assert!(
            !statement.starts_with("UPDATE") || done.rows_affected() == 1,
            "{statement}"
        );
    }
    let planted_issue = issue(&s, &planted).await;
    assert_eq!(captured(&s, &planted_issue).await.2, 330_000);

    // Direct SQL as the serving role. A correction is appended while the
    // capture trigger is off, so its command has no source yet; the source is
    // then inserted by hand.
    sqlx::query(
        "ALTER TABLE archava_billing.commands DISABLE TRIGGER capture_financial_invoice_source",
    )
    .execute(&s.f.owner_pool)
    .await
    .unwrap();
    let uncaptured = correct(&s, &waiting, -11_100).await;
    sqlx::query(
        "ALTER TABLE archava_billing.commands ENABLE TRIGGER capture_financial_invoice_source",
    )
    .execute(&s.f.owner_pool)
    .await
    .unwrap();
    let account_id = vat_account["resourceId"].as_str().unwrap();
    let differs = (
        "23514".to_owned(),
        "Financial source tax differs from the tax stored on its invoice".to_owned(),
    );
    let retained = (
        "23514".to_owned(),
        "Invoice corrections retain their original posting policy".to_owned(),
    );
    for (label, tax, tax_account, expected) in [
        ("one unit less tax", -109_900, Some(account_id), &differs),
        (
            "the tax of the document, not of the ledger unit",
            -1_100,
            Some(account_id),
            &differs,
        ),
        (
            "no tax on a taxed correction",
            0,
            Some(account_id),
            &differs,
        ),
        (
            "tax with the other sign",
            110_000,
            Some(account_id),
            &differs,
        ),
        (
            "another tax account",
            -110_000,
            Some(s.clearing["resourceId"].as_str().unwrap()),
            &retained,
        ),
    ] {
        let error = direct_source(&s, &uncaptured, -1_110_000, tax, tax_account)
            .await
            .expect_err(label);
        assert_eq!(&guard_message(&error), expected, "{label}");
    }
    // Tax without its account is not a storable row at all.
    let error = direct_source(&s, &uncaptured, -1_110_000, -110_000, None)
        .await
        .expect_err("tax without a tax account");
    assert_eq!(guard_message(&error).1, retained.1);
    // The exact row the capture writes is accepted.
    direct_source(&s, &uncaptured, -1_110_000, -110_000, Some(account_id))
        .await
        .expect("the exact taxed correction source");
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT archava_accounting.source_guards_complete()")
            .fetch_one(&s.f.runtime_pool)
            .await
            .unwrap()
    );
    // The early invoice never reached the ledger.
    let early_sources: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_accounting.invoice_sources WHERE invoice_id=$1::uuid",
    )
    .bind(early["invoiceId"].as_str().unwrap())
    .fetch_one(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(early_sources, 0);
}
