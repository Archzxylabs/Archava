//! The tax report (TAX.md stage E, rule 9): output and input tax of posted
//! sources per tax code version for a period of source dates, beside the
//! posted movement of each tax control account, with captured-but-unposted
//! sources and never-captured taxed documents listed apart from the totals.
use super::invoice_sources::SourceFixture;
use super::purchase_sources::PurchaseScenario;
use super::tax_posting as sales;
use super::tax_purchases as buys;
use super::tax_sales::{confirmed_code, correct, output_account, product};
use super::*;

async fn read(
    app: &Router,
    session: Option<&Value>,
    book: &Value,
    from: &str,
    to: &str,
) -> (StatusCode, Value) {
    call(
        app,
        "GET",
        &format!(
            "/erp/accounting/tax-report?tenant=acme&bookId={}&from={from}&to={to}",
            book["bookId"].as_str().unwrap()
        ),
        Value::Null,
        session,
    )
    .await
}
async fn whole(app: &Router, session: &Value, book: &Value) -> Value {
    let (st, v) = read(app, Some(session), book, "2000-01-01", "2100-12-31").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v
}
/// One code row: code, version, kind, rate, base, sources, taxable base, tax.
type CodeRow = (
    String,
    i64,
    String,
    (i64, i64),
    (i64, i64),
    i64,
    String,
    String,
);
fn codes(report: &Value) -> Vec<CodeRow> {
    report["codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let text = |key: &str| c[key].as_str().unwrap().to_owned();
            let fraction = |key: &str| {
                (
                    c[key]["numerator"].as_i64().unwrap(),
                    c[key]["denominator"].as_i64().unwrap(),
                )
            };
            (
                text("code"),
                c["version"].as_i64().unwrap(),
                text("kind"),
                fraction("rate"),
                fraction("base"),
                c["sourceCount"].as_i64().unwrap(),
                text("taxableBaseMinor"),
                text("taxMinor"),
            )
        })
        .collect()
}
#[allow(clippy::too_many_arguments)]
fn code(
    code: &str,
    version: i64,
    kind: &str,
    rate: (i64, i64),
    base: (i64, i64),
    sources: i64,
    taxable: &str,
    tax: &str,
) -> CodeRow {
    (
        code.into(),
        version,
        kind.into(),
        rate,
        base,
        sources,
        taxable.into(),
        tax.into(),
    )
}
/// One control row: account code, kind, reported tax, movement, difference.
fn controls(report: &Value) -> Vec<(String, String, String, String, String)> {
    report["controls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let text = |key: &str| c[key].as_str().unwrap().to_owned();
            (
                text("code"),
                text("kind"),
                text("reportedTaxMinor"),
                text("movementMinor"),
                text("differenceMinor"),
            )
        })
        .collect()
}
fn control(
    code: &str,
    kind: &str,
    reported: &str,
    movement: &str,
    difference: &str,
) -> (String, String, String, String, String) {
    (
        code.into(),
        kind.into(),
        reported.into(),
        movement.into(),
        difference.into(),
    )
}
fn nothing() -> Value {
    json!({"sourceCount":0,"taxableBaseMinor":"0","taxMinor":"0"})
}
/// The same company and book as a purchase scenario: purchasing grants, a
/// supplier, the purchase accounts, a tracked product with its offer and the
/// purchase policy.
async fn buying(s: SourceFixture) -> PurchaseScenario {
    for actor in ["alice", "bob"] {
        sqlx::query("INSERT INTO archava_purchasing.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme',$1,'human',$2,'all')").bind(actor).bind(archava_domain::erp::purchasing::PERMISSION_VOCABULARY).execute(&s.f.admin_pool).await.unwrap();
    }
    let mut p = PurchaseScenario {
        f: s.f,
        app: s.app,
        alice: s.alice,
        bob: s.bob,
        company: s.company,
        book: s.book,
        period: s.period,
        accounts: Vec::new(),
        supplier: Value::Null,
        offer: Value::Null,
        currency: s.currency,
        unit: s.unit,
    };
    p.supplier = p.purchase(json!({"operation":"supplier.create","code":"TAX-SUPPLIER","name":"Explicit supplier master"})).await;
    for (role, class) in [
        ("INVENTORY", "asset"),
        ("GRNI", "liability"),
        ("PAYABLE", "liability"),
        ("FEE-ACCRUAL", "liability"),
    ] {
        p.accounts
            .push(account(&p.app, &p.alice, &p.book, role, class, "operating").await);
    }
    p.stock_item().await;
    ok(&p.app, &p.alice, p.policy(0)).await;
    p
}

/// **Witness: on one IDR book a taxed sale (11%, exclusive, 2 × 15,000) with
/// a credit of 11,100, a sale under a 0% code, an untaxed sale and a supplier
/// bill for goods of 140,000 whose printed tax (15,401) differs from the
/// computed one are posted. The tax report of the period shows, per tax code
/// version, the taxable base and the tax in ledger minor units: output
/// 2,000,000 / 220,000, the 0% code 3,000,000 / 0, input 14,000,000 /
/// 1,540,100; the untaxed sale is not in it. Output tax 220,000, input tax
/// 1,540,100, difference −1,320,100. Every tax control account moves by what
/// the report attributes to it. A captured taxed invoice that is not posted is
/// pending and not in the totals. A range that ends before the source date has
/// nothing. A manual journal on the output tax account is exactly that
/// account's difference. The read needs `tax.read` and `reports.read`, a
/// session, a valid range and a book in scope.**
#[tokio::test]
async fn the_tax_report_ties_posted_output_and_input_tax_to_the_control_accounts() {
    let s = SourceFixture::new().await;
    let vat_out = output_account(&s).await;
    let vat_zero = account(
        &s.app,
        &s.alice,
        &s.book,
        "VAT-ZERO",
        "liability",
        "operating",
    )
    .await;
    confirmed_code(
        &s,
        "VAT",
        "output",
        (11, 100),
        (1, 1),
        &vat_out,
        "2020-01-01",
    )
    .await;
    confirmed_code(
        &s,
        "ZERO",
        "output",
        (0, 100),
        (1, 1),
        &vat_zero,
        "2020-01-01",
    )
    .await;
    s.enable().await;
    let item = product(&s, "TEA", 15_000).await;

    // Before anything is captured: the confirmed codes' accounts, all zero.
    let empty = whole(&s.app, &s.alice, &s.book).await;
    assert_eq!(empty["codes"], json!([]));
    assert_eq!(
        empty["totals"],
        json!({"outputTaxMinor":"0","inputTaxMinor":"0","differenceMinor":"0"})
    );
    assert_eq!(
        controls(&empty),
        vec![
            control("VAT-OUT", "output", "0", "0", "0"),
            control("VAT-ZERO", "output", "0", "0", "0")
        ]
    );
    assert_eq!(empty["postedSourceControlsAgree"], true);

    // The taxed sale, captured: pending, not in the totals.
    let taxed = sales::taxed_invoice(&s, "TAXED", &item, 2, Some(("VAT", false))).await;
    let issued = sales::issue(&s, &taxed).await;
    let pending = whole(&s.app, &s.alice, &s.book).await;
    assert_eq!(pending["codes"], json!([]));
    assert_eq!(pending["totals"]["outputTaxMinor"], "0");
    assert_eq!(
        pending["pending"],
        json!({"output":{"sourceCount":1,"taxableBaseMinor":"3000000","taxMinor":"330000"},"input":nothing()})
    );
    assert_eq!(pending["postedSourceControlsAgree"], true);
    sales::post_taxed(&s, &issued, "TAXED-ISSUE", &vat_out).await;
    // A credit of 11,100 carries 1,100 of tax.
    let credit = correct(&s, &taxed, -11_100).await;
    sales::post_taxed(&s, &credit, "TAXED-CREDIT", &vat_out).await;
    // A sale under the 0% code and an untaxed sale: two accounts each.
    let zero = sales::taxed_invoice(&s, "ZERO-RATED", &item, 2, Some(("ZERO", false))).await;
    let zero_issue = sales::issue(&s, &zero).await;
    s.post_source(&zero_issue, "ZERO-ISSUE").await;
    let plain = sales::taxed_invoice(&s, "PLAIN", &item, 2, None).await;
    let plain_issue = sales::issue(&s, &plain).await;
    s.post_source(&plain_issue, "PLAIN-ISSUE").await;
    // A second taxed sale stays captured and unposted.
    let waiting = sales::taxed_invoice(&s, "WAITING", &item, 2, Some(("VAT", false))).await;
    sales::issue(&s, &waiting).await;

    // The purchase side of the same book.
    let p = buying(s).await;
    let date = buys::today(&p).await;
    let vat_in = buys::input_account(&p, "VAT-IN").await;
    buys::confirmed_code(&p, "VAT-IN", "input", (11, 100), &vat_in, "2020-01-01").await;
    let (po, line) = buys::received(&p, 20, "RECEIPT").await;
    let document = p
        .purchase(buys::claim(
            &p,
            &po,
            line,
            "PRINTED",
            &date,
            20,
            Some(("VAT-IN", 15_401)),
        ))
        .await;
    let bill = p.purchase(buys::matching(&document, 1)).await;
    buys::post_taxed(&p, &bill, "BILL", &vat_in).await;

    let (st, report) = read(&p.app, Some(&p.alice), &p.book, &date, &date).await;
    assert_eq!(st, StatusCode::OK, "{report}");
    assert_eq!(report["book"]["currency"], "IDR");
    assert_eq!(
        (&report["from"], &report["to"]),
        (&json!(date), &json!(date))
    );
    assert_eq!(report["periodDate"], "source_date");
    assert!(report["observedAt"].is_string());
    assert_eq!(
        codes(&report),
        vec![
            code(
                "VAT",
                1,
                "output",
                (11, 100),
                (1, 1),
                2,
                "2000000",
                "220000"
            ),
            code("ZERO", 1, "output", (0, 100), (1, 1), 1, "3000000", "0"),
            code(
                "VAT-IN",
                1,
                "input",
                (11, 100),
                (1, 1),
                1,
                "14000000",
                "1540100"
            ),
        ]
    );
    assert_eq!(
        report["totals"],
        json!({"outputTaxMinor":"220000","inputTaxMinor":"1540100","differenceMinor":"-1320100"})
    );
    assert_eq!(
        controls(&report),
        vec![
            control("VAT-IN", "input", "1540100", "1540100", "0"),
            control("VAT-OUT", "output", "220000", "220000", "0"),
            control("VAT-ZERO", "output", "0", "0", "0"),
        ]
    );
    assert_eq!(report["postedSourceControlsAgree"], true);
    assert_eq!(
        report["pending"],
        json!({"output":{"sourceCount":1,"taxableBaseMinor":"3000000","taxMinor":"330000"},"input":nothing()})
    );
    assert_eq!(
        report["outsideLedger"]["invoices"],
        json!({"count":0,"taxMinor":"0"})
    );
    assert_eq!(
        report["outsideLedger"]["supplierDocuments"],
        json!({"count":0,"taxMinor":"0"})
    );
    // No compliance claim.
    assert_eq!(report["enteredByCompany"], true);
    assert_eq!(report["verifiedByArchava"], false);
    let notice = report["notice"].as_str().unwrap();
    assert!(notice.contains("Archava does not verify tax rates or rules"));
    assert!(notice.contains("not a tax return"));
    // The report ties to the ledger: the trial balance holds the same amounts.
    let (st, ledger) = call(
        &p.app,
        "GET",
        &format!(
            "/erp/accounting/reports?tenant=acme&bookId={}&from={date}&to={date}",
            p.book["bookId"].as_str().unwrap()
        ),
        Value::Null,
        Some(&p.alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{ledger}");
    let closing = |code: &str| {
        ledger["trialBalance"]["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["code"] == code)
            .map(|a| a["closingNetMinor"].as_str().unwrap().to_owned())
            .unwrap()
    };
    assert_eq!(closing("VAT-OUT"), "-220000");
    assert_eq!(closing("VAT-IN"), "1540100");

    // A range that ends before the source date has no source and no movement.
    let before: String = sqlx::query_scalar("SELECT ($1::text::date-1)::text")
        .bind(&date)
        .fetch_one(&p.f.admin_pool)
        .await
        .unwrap();
    let (st, earlier) = read(&p.app, Some(&p.alice), &p.book, "2000-01-01", &before).await;
    assert_eq!(st, StatusCode::OK, "{earlier}");
    assert_eq!(earlier["codes"], json!([]));
    assert_eq!(
        earlier["totals"],
        json!({"outputTaxMinor":"0","inputTaxMinor":"0","differenceMinor":"0"})
    );
    assert_eq!(
        earlier["pending"],
        json!({"output":nothing(),"input":nothing()})
    );
    assert!(controls(&earlier)
        .iter()
        .all(|c| c.2 == "0" && c.3 == "0" && c.4 == "0"));

    // A voided bill takes its base and tax out again.
    let void = p.purchase(json!({"operation":"document.void","id":document["id"],"expectedRevision":2,"reason":"Correct the original supplier claim"})).await;
    let unposted = whole(&p.app, &p.alice, &p.book).await;
    assert_eq!(
        unposted["pending"]["input"],
        json!({"sourceCount":1,"taxableBaseMinor":"-14000000","taxMinor":"-1540100"})
    );
    assert_eq!(unposted["totals"]["inputTaxMinor"], "1540100");
    buys::post_taxed(&p, &void, "VOID", &vat_in).await;
    let voided = whole(&p.app, &p.alice, &p.book).await;
    assert_eq!(
        codes(&voided)[2],
        code("VAT-IN", 1, "input", (11, 100), (1, 1), 2, "0", "0")
    );
    assert_eq!(
        voided["totals"],
        json!({"outputTaxMinor":"220000","inputTaxMinor":"0","differenceMinor":"220000"})
    );
    assert_eq!(
        controls(&voided)[0],
        control("VAT-IN", "input", "0", "0", "0")
    );
    assert_eq!(voided["postedSourceControlsAgree"], true);

    // A manual journal on the output tax account is that account's difference.
    let equity = account(&p.app, &p.alice, &p.book, "EQUITY", "equity", "financing").await;
    post(
        &p.app,
        &p.alice,
        &p.bob,
        &p.book,
        "MANUAL-TAX",
        vec![
            super::line(&equity, "500", "0"),
            super::line(&vat_out, "0", "500"),
        ],
    )
    .await;
    let adjusted = whole(&p.app, &p.alice, &p.book).await;
    assert_eq!(
        controls(&adjusted),
        vec![
            control("VAT-IN", "input", "0", "0", "0"),
            control("VAT-OUT", "output", "220000", "220500", "500"),
            control("VAT-ZERO", "output", "0", "0", "0"),
        ]
    );
    assert_eq!(adjusted["postedSourceControlsAgree"], false);
    assert_eq!(adjusted["totals"]["outputTaxMinor"], "220000");

    // Refusals.
    let (st, _) = read(&p.app, None, &p.book, &date, &date).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    for (from, to) in [("2026-02-30", "2026-03-01"), ("2026-03-02", "2026-03-01")] {
        let (st, v) = read(&p.app, Some(&p.alice), &p.book, from, to).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
        assert_eq!(v["reason"], "invalid_query");
    }
    let path = format!(
        "/erp/accounting/tax-report?tenant=acme&bookId={}",
        p.book["bookId"].as_str().unwrap()
    );
    for suffix in [
        "&from=2026-01-01",
        "&from=2026-01-01&to=2026-01-31&actorId=bob",
    ] {
        let (st, v) = call(
            &p.app,
            "GET",
            &format!("{path}{suffix}"),
            Value::Null,
            Some(&p.alice),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    }
    let unknown = json!({"bookId":Uuid::new_v4()});
    let (st, v) = read(&p.app, Some(&p.alice), &unknown, &date, &date).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
    // `reader` holds books.read and reports.read but not tax.read.
    let reader = login(&p.app, "reader").await;
    let (st, v) = read(&p.app, Some(&reader), &p.book, &date, &date).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    // A principal whose scope does not reach the company's book.
    let assigned = login(&p.app, "assigned").await;
    let (st, v) = read(&p.app, Some(&assigned), &p.book, &date, &date).await;
    assert!(
        matches!(st, StatusCode::FORBIDDEN | StatusCode::NOT_FOUND),
        "{st} {v}"
    );
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=array_remove(permissions,'reports.read') WHERE tenant_id='acme' AND principal_id='alice'")
        .execute(&p.f.admin_pool)
        .await
        .unwrap();
    let (st, v) = read(&p.app, Some(&p.alice), &p.book, &date, &date).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
}

/// **Witness: a taxed invoice issued before the book had an invoice policy and
/// a taxed supplier document matched for an order confirmed before the book
/// had a purchase policy have no captured source. The tax report counts them
/// as outside the ledger, with their tax in the document's own unit (whole
/// Rupiah), and adds nothing to the codes, the totals or pending. A draft and
/// a voided invoice are not counted. Setting the policy later captures
/// nothing of them.**
#[tokio::test]
async fn taxed_documents_that_were_never_captured_are_listed_outside_the_ledger() {
    let s = SourceFixture::new().await;
    let vat_out = output_account(&s).await;
    confirmed_code(
        &s,
        "VAT",
        "output",
        (11, 100),
        (1, 1),
        &vat_out,
        "2020-01-01",
    )
    .await;
    let item = product(&s, "TEA", 15_000).await;
    let early = sales::taxed_invoice(&s, "EARLY", &item, 2, Some(("VAT", false))).await;
    sales::issue(&s, &early).await;
    correct(&s, &early, -11_100).await;
    // A draft and an issued-then-voided invoice are not outside-ledger tax.
    sales::taxed_invoice(&s, "DRAFT", &item, 2, Some(("VAT", false))).await;
    let gone = sales::taxed_invoice(&s, "GONE", &item, 2, Some(("VAT", false))).await;
    sales::issue(&s, &gone).await;
    s.bill(json!({"type":"void_invoice","invoiceId":gone["invoiceId"],"expectedRevision":2,"reason":"Issued in error"})).await;
    let report = whole(&s.app, &s.alice, &s.book).await;
    assert_eq!(
        report["outsideLedger"],
        json!({"unit":"document","currency":"IDR",
            "unitNote":"Amounts of documents outside the ledger are in the unit the documents store: whole Rupiah for IDR, the currency's minor unit otherwise. They are not in ledger minor units.",
            "invoices":{"count":1,"taxMinor":"2200"},"supplierDocuments":{"count":0,"taxMinor":"0"}})
    );
    assert_eq!(report["codes"], json!([]));
    assert_eq!(
        report["totals"],
        json!({"outputTaxMinor":"0","inputTaxMinor":"0","differenceMinor":"0"})
    );
    assert_eq!(
        report["pending"],
        json!({"output":nothing(),"input":nothing()})
    );
    s.enable().await;
    let later = whole(&s.app, &s.alice, &s.book).await;
    assert_eq!(
        later["outsideLedger"]["invoices"],
        json!({"count":1,"taxMinor":"2200"})
    );
    assert_eq!(later["codes"], json!([]));
    // Outside the range by the day it was issued.
    let (st, earlier) = read(&s.app, Some(&s.alice), &s.book, "2000-01-01", "2000-12-31").await;
    assert_eq!(st, StatusCode::OK, "{earlier}");
    assert_eq!(
        earlier["outsideLedger"]["invoices"],
        json!({"count":0,"taxMinor":"0"})
    );

    // The purchase side: the order is confirmed before the policy exists.
    let p = PurchaseScenario::new(false).await;
    let date = buys::today(&p).await;
    let vat_in = buys::input_account(&p, "VAT-IN").await;
    buys::confirmed_code(&p, "VAT-IN", "input", (11, 100), &vat_in, "2020-01-01").await;
    let po = p.draft_order(20).await;
    p.purchase(json!({"operation":"order.confirm","id":po["id"],"expectedRevision":1,"reason":"Confirmed before a purchase policy exists"})).await;
    let line = p.order_line(&po).await;
    p.purchase(json!({"operation":"order.receive","id":po["id"],"expectedRevision":2,"reason":"All goods accepted","lines":[{"lineId":line,"accepted":20,"rejected":0}]})).await;
    ok(&p.app, &p.alice, p.policy(0)).await;
    let document = p
        .purchase(buys::claim(
            &p,
            &po,
            line,
            "EARLY",
            &date,
            20,
            Some(("VAT-IN", 15_401)),
        ))
        .await;
    let captured = whole(&p.app, &p.alice, &p.book).await;
    assert_eq!(
        captured["outsideLedger"]["supplierDocuments"],
        json!({"count":0,"taxMinor":"0"}),
        "a claim that is not matched is not a bill"
    );
    p.purchase(buys::matching(&document, 1)).await;
    let sources: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_accounting.purchase_sources")
            .fetch_one(&p.f.admin_pool)
            .await
            .unwrap();
    assert_eq!(sources, 0);
    let report = whole(&p.app, &p.alice, &p.book).await;
    assert_eq!(
        report["outsideLedger"]["supplierDocuments"],
        json!({"count":1,"taxMinor":"15401"})
    );
    assert_eq!(
        report["outsideLedger"]["invoices"],
        json!({"count":0,"taxMinor":"0"})
    );
    assert_eq!(report["codes"], json!([]));
    assert_eq!(report["totals"]["inputTaxMinor"], "0");
    assert_eq!(
        report["pending"],
        json!({"output":nothing(),"input":nothing()})
    );
    assert_eq!(
        controls(&report),
        vec![control("VAT-IN", "input", "0", "0", "0")]
    );
}

/// **Witness: on a USD book under the `minor` source unit the report is in
/// cents with the documents' one rounding: 3 × 3.33 at 11% is a base of 9.99
/// and tax of 1.10; a credit of 1.11 carries 0.11; a supplier bill for goods
/// of 1,400.00 with printed tax of 154.01 reports exactly that.**
#[tokio::test]
async fn a_usd_book_reports_its_tax_in_minor_units() {
    let s = SourceFixture::new_in("USD").await;
    let vat_out = output_account(&s).await;
    confirmed_code(
        &s,
        "SALES-TAX",
        "output",
        (11, 100),
        (1, 1),
        &vat_out,
        "2020-01-01",
    )
    .await;
    s.enable().await;
    let item = product(&s, "MUG", 333).await;
    let invoice = sales::taxed_invoice(&s, "USD-TAX", &item, 3, Some(("SALES-TAX", false))).await;
    let issued = sales::issue(&s, &invoice).await;
    sales::post_taxed(&s, &issued, "USD-ISSUE", &vat_out).await;
    let credit = correct(&s, &invoice, -111).await;
    sales::post_taxed(&s, &credit, "USD-CREDIT", &vat_out).await;
    let report = whole(&s.app, &s.alice, &s.book).await;
    assert_eq!(report["book"]["currency"], "USD");
    assert_eq!(
        codes(&report),
        vec![code(
            "SALES-TAX",
            1,
            "output",
            (11, 100),
            (1, 1),
            2,
            "899",
            "99"
        )]
    );
    assert_eq!(
        report["totals"],
        json!({"outputTaxMinor":"99","inputTaxMinor":"0","differenceMinor":"99"})
    );
    assert_eq!(
        controls(&report),
        vec![control("VAT-OUT", "output", "99", "99", "0")]
    );
    assert_eq!(report["postedSourceControlsAgree"], true);

    let p = PurchaseScenario::new_in("USD", true).await;
    let date = buys::today(&p).await;
    let vat_in = buys::input_account(&p, "VAT-IN").await;
    buys::confirmed_code(&p, "VAT-IN", "input", (11, 100), &vat_in, "2020-01-01").await;
    let (po, line) = buys::received(&p, 20, "RECEIPT").await;
    let document = p
        .purchase(buys::claim(
            &p,
            &po,
            line,
            "USD-BILL",
            &date,
            20,
            Some(("VAT-IN", 15_401)),
        ))
        .await;
    let bill = p.purchase(buys::matching(&document, 1)).await;
    buys::post_taxed(&p, &bill, "BILL", &vat_in).await;
    let report = whole(&p.app, &p.alice, &p.book).await;
    assert_eq!(
        codes(&report),
        vec![code(
            "VAT-IN",
            1,
            "input",
            (11, 100),
            (1, 1),
            1,
            "140000",
            "15401"
        )]
    );
    assert_eq!(
        report["totals"],
        json!({"outputTaxMinor":"0","inputTaxMinor":"15401","differenceMinor":"-15401"})
    );
    assert_eq!(
        controls(&report),
        vec![control("VAT-IN", "input", "15401", "15401", "0")]
    );
    assert_eq!(report["postedSourceControlsAgree"], true);
}
