//! Tax on a company sales order and on the invoice made from it (TAX.md stage
//! B). The order names a confirmed output tax code of its company's book and a
//! price basis; the invoice stores the version that applies on its own date
//! with the net, tax and gross computed from it, and splits its corrections
//! with that stored rule. How a taxed invoice posts its tax to the ledger
//! (stage C) is in `tax_posting.rs`, which shares the helpers below.
use super::invoice_sources::SourceFixture;
use super::*;
use archava_domain::erp::tax::{Fraction, TaxRule};

/// Today, yesterday and tomorrow in the UTC calendar: the dates a tax code
/// applies on (TAX.md, decisions taken after review).
pub(super) async fn days(f: &Fixture) -> (String, String, String) {
    sqlx::query_as("SELECT d::text,(d-1)::text,(d+1)::text FROM (SELECT (now() AT TIME ZONE 'UTC')::date AS d) utc")
        .fetch_one(&f.runtime_pool)
        .await
        .unwrap()
}
/// A tax code version entered by alice; `confirmed_code` has bob confirm it
/// (the fixture's book requires a second person).
pub(super) async fn entered_code(
    s: &SourceFixture,
    code: &str,
    kind: &str,
    rate: (i64, i64),
    base: (i64, i64),
    account: &Value,
    from: &str,
) -> Value {
    ok(&s.app,&s.alice,json!({"type":"tax_code.create","bookId":s.book["bookId"],"code":code,"kind":kind,
        "rate":{"numerator":rate.0,"denominator":rate.1},"base":{"numerator":base.0,"denominator":base.1},
        "controlAccount":{"accountId":account["resourceId"],"expectedAccountRevision":account["revision"]},
        "effectiveFrom":from,"reason":"Entered by the company from its own tax advice"})).await
}
pub(super) async fn confirmed_code(
    s: &SourceFixture,
    code: &str,
    kind: &str,
    rate: (i64, i64),
    base: (i64, i64),
    account: &Value,
    from: &str,
) -> Value {
    let entered = entered_code(s, code, kind, rate, base, account, from).await;
    ok(&s.app,&s.bob,json!({"type":"tax_code.confirm","id":entered["resourceId"],"expectedRevision":1,"reason":"Checked against the company's tax advice"})).await
}
async fn retire(s: &SourceFixture, code: &Value, last_day: &str) -> Value {
    ok(&s.app,&s.alice,json!({"type":"tax_code.retire","id":code["resourceId"],"expectedRevision":2,"effectiveTo":last_day,"reason":"The company's rate changes"})).await
}
pub(super) async fn output_account(s: &SourceFixture) -> Value {
    account(
        &s.app,
        &s.alice,
        &s.book,
        "VAT-OUT",
        "liability",
        "operating",
    )
    .await
}
pub(super) async fn trade(s: &SourceFixture, command: Value) -> (StatusCode, Value) {
    call(
        &s.app,
        "POST",
        "/erp/commerce/commands",
        json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":command}),
        Some(&s.alice),
    )
    .await
}
pub(super) async fn traded(s: &SourceFixture, command: Value) -> Value {
    let (st, v) = trade(s, command).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["receipt"].clone()
}
pub(super) async fn product(s: &SourceFixture, sku: &str, price: i64) -> Value {
    traded(s,json!({"operation":"product.create_for_company","companyId":s.company["companyId"],"expectedCompanyRevision":1,"sku":sku,"title":sku,"unit":"unit","trackStock":false,"price":price})).await
}
/// A company order of `quantity` × `item`, naming `tax` (code, prices include tax) or none.
pub(super) fn order(
    s: &SourceFixture,
    item: &Value,
    quantity: i64,
    tax: Option<(&str, bool)>,
) -> Value {
    let mut command = json!({"operation":"order.create_for_company","companyId":s.company["companyId"],"expectedCompanyRevision":1,"partyId":s.party["partyId"],"expectedPartyRevision":1,"lines":[{"productId":item["id"],"quantity":quantity}]});
    if let Some((code, inclusive)) = tax {
        command["taxCode"] = json!(code);
        command["priceIncludesTax"] = json!(inclusive);
    }
    command
}
pub(super) async fn fulfilled(s: &SourceFixture, command: Value) -> Value {
    let o = traded(s, command).await;
    traded(
        s,
        json!({"operation":"order.reserve","id":o["id"],"expectedRevision":1,"holdSeconds":300}),
    )
    .await;
    traded(
        s,
        json!({"operation":"order.fulfill","id":o["id"],"expectedRevision":2}),
    )
    .await;
    o
}
pub(super) async fn bill(s: &SourceFixture, mut command: Value) -> (StatusCode, Value) {
    command["commandId"] = json!(Uuid::new_v4());
    call(
        &s.app,
        "POST",
        "/erp/billing/commands",
        json!({"tenant":"acme","mutation":command}),
        Some(&s.alice),
    )
    .await
}
pub(super) async fn invoice_from(
    s: &SourceFixture,
    code: &str,
    order: &Value,
) -> (StatusCode, Value) {
    bill(s,json!({"type":"create_invoice_from_order","invoiceCode":code,"orderId":order["id"],"expectedOrderRevision":3})).await
}
pub(super) async fn invoiced(s: &SourceFixture, code: &str, order: &Value) -> Value {
    let (st, v) = invoice_from(s, code, order).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v
}
pub(super) async fn workspace(s: &SourceFixture, domain: &str, id: &Value) -> Value {
    let (st, v) = call(
        &s.app,
        "GET",
        &format!(
            "/erp/{domain}/workspace?tenant=acme&companyId={}&id={}",
            s.company["companyId"].as_str().unwrap(),
            id.as_str().unwrap()
        ),
        Value::Null,
        Some(&s.alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v
}
/// The invoice as the Billing workspace read returns it.
pub(super) async fn invoice_row(s: &SourceFixture, invoice: &Value) -> Value {
    workspace(s, "billing", &invoice["invoiceId"]).await["invoices"][0].clone()
}
async fn order_row(s: &SourceFixture, order: &Value) -> Value {
    workspace(s, "commerce", &order["id"]).await["orders"][0].clone()
}
pub(super) async fn correct(s: &SourceFixture, invoice: &Value, amount: i64) -> Value {
    s.bill(json!({"type":"append_correction","invoiceId":invoice["invoiceId"],"amountMinor":amount,"reason":"Reviewed correction of a taxed invoice"})).await
}
/// The stored amounts of an invoice: gross, corrections, tax, correction tax,
/// outstanding, and the sum of the tax parts its correction rows carry.
pub(super) async fn stored(s: &SourceFixture, invoice: &Value) -> (i64, i64, i64, i64, i64, i64) {
    sqlx::query_as("SELECT i.gross_minor,i.correction_minor,i.tax_minor,i.correction_tax_minor,i.outstanding_minor,(SELECT coalesce(sum(c.tax_minor),0)::bigint FROM archava_billing.invoice_corrections c WHERE c.invoice_id=i.id) FROM archava_billing.invoices i WHERE i.id=$1::uuid")
        .bind(invoice["invoiceId"].as_str().unwrap()).fetch_one(&s.f.admin_pool).await.unwrap()
}
pub(super) async fn count(f: &Fixture, table: &str) -> i64 {
    let q = format!("SELECT count(*) FROM {table}");
    sqlx::query_scalar(sqlx::AssertSqlSafe(q.as_str()))
        .fetch_one(&f.admin_pool)
        .await
        .unwrap()
}
fn rule(rate: (i32, i32), base: (i32, i32)) -> TaxRule {
    TaxRule::new(
        Fraction {
            numerator: rate.0,
            denominator: rate.1,
        },
        Fraction {
            numerator: base.0,
            denominator: base.1,
        },
    )
    .unwrap()
}
pub(super) fn guard_message(error: &sqlx::Error) -> (String, String) {
    let database = error.as_database_error().expect("a database refusal");
    (
        database.code().unwrap_or_default().into_owned(),
        database.message().to_owned(),
    )
}

/// **Witness: an exclusive 11% code adds tax to the invoice of a 2 × 15,000
/// IDR order (net 30,000, tax 3,300, gross and outstanding 33,300); the
/// invoice stores the code version, rate, base and basis; the reads return
/// them; 12% of 11/12 of the price is the same tax; an untaxed order of the
/// same company is invoiced as before; a correction is split into net and
/// tax, ten corrections of one unit keep `net + tax = gross`, and a full
/// reversal brings net, tax and gross to exactly zero.**
#[tokio::test]
async fn an_exclusive_tax_code_adds_tax_to_the_invoice_and_corrections_split_with_the_stored_rate()
{
    let s = SourceFixture::new().await;
    let account = output_account(&s).await;
    let vat = confirmed_code(
        &s,
        "VAT",
        "output",
        (11, 100),
        (1, 1),
        &account,
        "2020-01-01",
    )
    .await;
    confirmed_code(
        &s,
        "VAT-DPP",
        "output",
        (12, 100),
        (11, 12),
        &account,
        "2020-01-01",
    )
    .await;
    let (today, ..) = days(&s.f).await;

    // The Sales screen reads the output codes that apply today from the
    // Commerce workspace, with the notice that the company entered them.
    let (st, sales) = call(
        &s.app,
        "GET",
        &format!(
            "/erp/commerce/workspace?tenant=acme&companyId={}",
            s.company["companyId"].as_str().unwrap()
        ),
        Value::Null,
        Some(&s.alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{sales}");
    assert_eq!(
        sales["taxCodes"],
        json!([
            {"code":"VAT","version":1,"rate":{"numerator":11,"denominator":100},"base":{"numerator":1,"denominator":1},
             "effectiveFrom":"2020-01-01","effectiveTo":null,"appliesOn":today,"enteredByCompany":true,"verifiedByArchava":false},
            {"code":"VAT-DPP","version":1,"rate":{"numerator":12,"denominator":100},"base":{"numerator":11,"denominator":12},
             "effectiveFrom":"2020-01-01","effectiveTo":null,"appliesOn":today,"enteredByCompany":true,"verifiedByArchava":false}
        ])
    );
    assert!(sales["taxNotice"]
        .as_str()
        .unwrap()
        .contains("Archava does not verify tax rates or rules"));
    // Without a company there is no book and no code to offer.
    let (st, legacy) = call(
        &s.app,
        "GET",
        "/erp/commerce/workspace?tenant=acme",
        Value::Null,
        Some(&s.alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{legacy}");
    assert_eq!(legacy["taxCodes"], json!([]));

    let item = product(&s, "TEA", 15_000).await;
    // The checkout document shows an order total. An order that names a tax
    // code has no tax amount until it is invoiced, so it is not signed; an
    // untaxed draft of the same company is, with its total.
    let draft = traded(&s, order(&s, &item, 2, Some(("VAT", false)))).await;
    let (st, v) = call(
        &s.app,
        "POST",
        "/embed/checkout/sign",
        json!({"tenant":"acme","orderId":draft["id"],"amount":30_000,"currency":"IDR","item":"Tea"}),
        Some(&s.alice),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"], "checkout_order_taxed", "{v}");
    let untaxed_draft = traded(&s, order(&s, &item, 2, None)).await;
    let (st, v) = call(
        &s.app,
        "POST",
        "/embed/checkout/sign",
        json!({"tenant":"acme","orderId":untaxed_draft["id"],"amount":30_000,"currency":"IDR","item":"Tea"}),
        Some(&s.alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["url"].as_str().unwrap().contains("amount=30000"), "{v}");

    let o = fulfilled(&s, order(&s, &item, 2, Some(("VAT", false)))).await;
    // The order keeps its priced total and names the code and the basis only.
    let row = order_row(&s, &o).await;
    assert_eq!(row["total"], 30_000);
    assert_eq!(row["taxCode"], "VAT");
    assert_eq!(row["priceIncludesTax"], false);

    let invoice = invoiced(&s, "TAX-EXCL", &o).await;
    assert_eq!(invoice["status"], "draft");
    assert_eq!(invoice["outstandingMinor"], 33_300);
    let row = invoice_row(&s, &invoice).await;
    assert_eq!(row["gross"], 33_300);
    assert_eq!(row["outstanding"], 33_300);
    assert_eq!(row["taxMinor"], 3_300);
    assert_eq!(row["netMinor"], 30_000);
    assert_eq!(row["correctionTaxMinor"], 0);
    assert_eq!(
        row["tax"],
        json!({"codeId":vat["resourceId"],"code":"VAT","version":1,"kind":"output",
            "rate":{"numerator":11,"denominator":100},"base":{"numerator":1,"denominator":1},
            "priceIncludesTax":false,"enteredByCompany":true,"verifiedByArchava":false})
    );
    // The lines are the order's priced lines; tax is a document amount.
    assert_eq!(row["lines"].as_array().unwrap().len(), 1);
    assert_eq!(row["lines"][0]["total"], 30_000);
    assert_eq!(row["lines"][0]["unitAmount"], 15_000);
    let billing = workspace(&s, "billing", &invoice["invoiceId"]).await;
    assert!(billing["taxNotice"]
        .as_str()
        .unwrap()
        .contains("Archava does not verify tax rates or rules"));

    // 12% of eleven twelfths of the price is the same tax on the same order.
    let scaled = fulfilled(&s, order(&s, &item, 2, Some(("VAT-DPP", false)))).await;
    let scaled = invoiced(&s, "TAX-DPP", &scaled).await;
    let row = invoice_row(&s, &scaled).await;
    assert_eq!(
        (&row["gross"], &row["taxMinor"], &row["netMinor"]),
        (&json!(33_300), &json!(3_300), &json!(30_000))
    );
    assert_eq!(
        row["tax"]["rate"],
        json!({"numerator":12,"denominator":100})
    );
    assert_eq!(row["tax"]["base"], json!({"numerator":11,"denominator":12}));

    // An order without a tax code is invoiced exactly as before.
    let plain = fulfilled(&s, order(&s, &item, 2, None)).await;
    let row = order_row(&s, &plain).await;
    assert!(row["taxCode"].is_null() && row["priceIncludesTax"].is_null());
    let plain = invoiced(&s, "NO-TAX", &plain).await;
    assert_eq!(plain["outstandingMinor"], 30_000);
    let row = invoice_row(&s, &plain).await;
    assert_eq!(row["gross"], 30_000);
    assert_eq!(row["taxMinor"], 0);
    assert_eq!(row["netMinor"], 30_000);
    assert!(row["tax"].is_null());
    s.bill(json!({"type":"issue_invoice","invoiceId":plain["invoiceId"],"expectedRevision":1,"reason":"Untaxed invoice"})).await;
    let corrected = correct(&s, &plain, -1_000).await;
    assert_eq!(corrected["outstandingMinor"], 29_000);
    assert_eq!(stored(&s, &plain).await, (30_000, -1_000, 0, 0, 29_000, 0));

    // Corrections of the taxed invoice: the amount is what the customer owes
    // less, and it is split with the invoice's stored rate.
    let issued=s.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Issue the taxed invoice"})).await;
    assert_eq!(issued["outstandingMinor"], 33_300);
    let partial = correct(&s, &invoice, -11_100).await;
    assert_eq!(partial["outstandingMinor"], 22_200);
    assert_eq!(
        stored(&s, &invoice).await,
        (33_300, -11_100, 3_300, -1_100, 22_200, -1_100)
    );
    let part: i64 = sqlx::query_scalar(
        "SELECT tax_minor FROM archava_billing.invoice_corrections WHERE id=$1::uuid",
    )
    .bind(partial["correctionId"].as_str().unwrap())
    .fetch_one(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(part, -1_100);
    let row = invoice_row(&s, &invoice).await;
    assert_eq!(row["correction"], -11_100);
    assert_eq!(row["correctionTaxMinor"], -1_100);
    // The original snapshot is unchanged by a correction.
    assert_eq!(row["taxMinor"], 3_300);
    assert_eq!(row["netMinor"], 30_000);

    // Ten corrections of one unit: after each one the tax is the tax contained
    // in the corrected total, the rows add up to it and net + tax = gross.
    let eleven = rule((11, 100), (1, 1));
    for step in 1..=10_i64 {
        correct(&s, &invoice, -1).await;
        let (gross, correction, tax, correction_tax, outstanding, rows) =
            stored(&s, &invoice).await;
        let total = gross + correction;
        assert_eq!(total, 22_200 - step);
        assert_eq!(outstanding, total);
        assert_eq!(tax + correction_tax, eleven.contained(total).unwrap());
        assert_eq!(rows, correction_tax, "the rows carry the invoice's tax");
        let net = total - (tax + correction_tax);
        assert_eq!(net + tax + correction_tax, total);
    }
    // 22,190 contains 2,199 of tax: the ten units moved the tax by one.
    assert_eq!(
        stored(&s, &invoice).await,
        (33_300, -11_110, 3_300, -1_101, 22_190, -1_101)
    );

    // Reversing everything that is left: net, tax and gross are exactly zero.
    let reversed = correct(&s, &invoice, -22_190).await;
    assert_eq!(reversed["outstandingMinor"], 0);
    assert_eq!(
        stored(&s, &invoice).await,
        (33_300, -33_300, 3_300, -3_300, 0, -3_300)
    );
    // No policy on this book: nothing was captured for the ledger.
    assert_eq!(count(&s.f, "archava_accounting.invoice_sources").await, 0);

    // The database's integer arithmetic is the domain's, for both bases.
    for (rate, base) in [((11, 100), (1, 1)), ((12, 100), (11, 12)), ((1, 3), (2, 3))] {
        let tax_rule = rule(rate, base);
        for amount in [
            0_i64,
            1,
            4,
            5,
            50,
            999,
            30_000,
            33_300,
            1_234_567_891,
            -1,
            -999,
            -33_300,
        ] {
            let (on_net, in_gross): (i64, i64) = sqlx::query_as(
                "SELECT archava_billing.tax_on_net($1,$2,$3,$4,$5),archava_billing.tax_in_gross($1,$2,$3,$4,$5)",
            )
            .bind(amount)
            .bind(rate.0)
            .bind(rate.1)
            .bind(base.0)
            .bind(base.1)
            .fetch_one(&s.f.runtime_pool)
            .await
            .unwrap();
            assert_eq!(
                on_net,
                tax_rule.exclusive(amount).unwrap().0,
                "{rate:?} {amount}"
            );
            assert_eq!(
                in_gross,
                tax_rule.contained(amount).unwrap(),
                "{rate:?} {amount}"
            );
        }
    }
}

/// **Witness: an inclusive 11% code extracts the tax from the same order
/// (gross 30,000, net 27,027, tax 2,973); a payment in full settles the
/// invoice; a correction and a full reversal split with the stored rate; and
/// the invoice mail states net, tax and total with the company notice.**
#[tokio::test]
async fn an_inclusive_tax_code_extracts_tax_and_the_invoice_is_paid_corrected_and_mailed() {
    let mut s = SourceFixture::new().await;
    // The same database behind a router that has an owned mail provider.
    let mock = mail::Mock::start().await;
    s.app = router(
        AppState::try_new_all(
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
        .unwrap()
        .with_email_registry(mock.registry()),
    );
    let account = output_account(&s).await;
    let vat = confirmed_code(
        &s,
        "VAT",
        "output",
        (11, 100),
        (1, 1),
        &account,
        "2020-01-01",
    )
    .await;
    let item = product(&s, "TEA", 15_000).await;

    let o = fulfilled(&s, order(&s, &item, 2, Some(("VAT", true)))).await;
    assert_eq!(order_row(&s, &o).await["priceIncludesTax"], true);
    let invoice = invoiced(&s, "TAX-INCL", &o).await;
    assert_eq!(invoice["outstandingMinor"], 30_000);
    let row = invoice_row(&s, &invoice).await;
    // 30,000 / 1.11 = 27,027.03, so the net is 27,027 and the tax the rest.
    assert_eq!(row["gross"], 30_000);
    assert_eq!(row["netMinor"], 27_027);
    assert_eq!(row["taxMinor"], 2_973);
    assert_eq!(row["outstanding"], 30_000);
    assert_eq!(row["tax"]["codeId"], vat["resourceId"]);
    assert_eq!(row["tax"]["priceIncludesTax"], true);

    s.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Issue the taxed invoice"})).await;

    // The mail states the net, the tax with its code and rate, and the total.
    let request = Uuid::new_v4();
    let (st, sent) = call(
        &s.app,
        "POST",
        &format!(
            "/erp/billing/invoices/{}/send-email?tenant=acme",
            invoice["invoiceId"].as_str().unwrap()
        ),
        json!({"requestId":request,"recipient":"customer@example.com","subject":"Your invoice","message":"Thank you."}),
        Some(&s.alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{sent}");
    assert_eq!(sent["state"], "accepted");
    let (text, html): (String, Option<String>) = sqlx::query_as(
        "SELECT payload_text,payload_html FROM archava_billing.email_outbox WHERE tenant_id='acme' AND request_id=$1",
    )
    .bind(request)
    .fetch_one(&s.f.admin_pool)
    .await
    .unwrap();
    assert!(text.contains("Total Due: 30000 IDR"), "{text}");
    assert!(text.contains("- TEA (x2): 30000 IDR"), "{text}");
    assert!(
        text.contains("\nNet: 27027 IDR\nTax (VAT 11%, included in the prices): 2973 IDR\nTotal: 30000 IDR\nTax codes are entered by the company; Archava does not verify tax rates or rules.\n"),
        "{text}"
    );
    let html = html.unwrap();
    assert!(html.contains("<li>Net: 27027 IDR</li>"), "{html}");
    assert!(
        html.contains("<li>Tax (VAT 11%, included in the prices): 2973 IDR</li>"),
        "{html}"
    );
    assert!(
        html.contains("<li>Total: <strong>30000 IDR</strong></li>"),
        "{html}"
    );
    assert_eq!(mock.sent(), 1);

    // A payment in full settles the taxed invoice: payments work on what is owed.
    let paid = s.payment(&invoice, "succeeded", 30_000).await;
    assert_eq!(paid["status"], "settled");
    assert_eq!(paid["outstandingMinor"], 0);
    let row = invoice_row(&s, &invoice).await;
    assert_eq!(row["status"], "settled");
    assert_eq!(row["settled"], 30_000);
    assert_eq!(row["taxMinor"], 2_973);

    // A second inclusive invoice: a partial correction, then a full reversal.
    let second = fulfilled(&s, order(&s, &item, 2, Some(("VAT", true)))).await;
    let second = invoiced(&s, "TAX-INCL-2", &second).await;
    s.bill(json!({"type":"issue_invoice","invoiceId":second["invoiceId"],"expectedRevision":1,"reason":"Issue"})).await;
    correct(&s, &second, -10_000).await;
    // 20,000 / 1.11 = 18,018.02: tax 1,982, so the correction carried -991.
    assert_eq!(
        stored(&s, &second).await,
        (30_000, -10_000, 2_973, -991, 20_000, -991)
    );
    correct(&s, &second, -20_000).await;
    assert_eq!(
        stored(&s, &second).await,
        (30_000, -30_000, 2_973, -2_973, 0, -2_973)
    );
    assert_eq!(count(&s.f, "archava_accounting.invoice_sources").await, 0);
}

/// **Witness: a company whose book is in USD is taxed in minor units, and the
/// rounding is half away from zero once per document: 3 × 3.33 is 9.99, 11% of
/// it is 1.0989, stored as 110 minor units.**
#[tokio::test]
async fn a_usd_invoice_is_taxed_in_minor_units_with_one_rounding_per_document() {
    let s = SourceFixture::new_in("USD").await;
    let account = output_account(&s).await;
    confirmed_code(
        &s,
        "SALES-TAX",
        "output",
        (11, 100),
        (1, 1),
        &account,
        "2020-01-01",
    )
    .await;
    let item = product(&s, "MUG", 333).await;
    let o = fulfilled(&s, order(&s, &item, 3, Some(("SALES-TAX", false)))).await;
    assert_eq!(order_row(&s, &o).await["currency"], "USD");
    let invoice = invoiced(&s, "USD-TAX", &o).await;
    assert_eq!(invoice["outstandingMinor"], 1_109);
    let row = invoice_row(&s, &invoice).await;
    assert_eq!(row["currency"], "USD");
    assert_eq!(row["netMinor"], 999);
    assert_eq!(row["taxMinor"], 110);
    assert_eq!(row["gross"], 1_109);
    // Rounding each of the three lines instead would have given 3 × 37 = 111.
    assert_eq!(rule((11, 100), (1, 1)).exclusive(333).unwrap().0 * 3, 111);

    // The same order with prices that include the tax: 9.99 contains 0.99.
    let inclusive = fulfilled(&s, order(&s, &item, 3, Some(("SALES-TAX", true)))).await;
    let inclusive = invoiced(&s, "USD-TAX-INCL", &inclusive).await;
    let row = invoice_row(&s, &inclusive).await;
    assert_eq!(
        (&row["gross"], &row["netMinor"], &row["taxMinor"]),
        (&json!(999), &json!(900), &json!(99))
    );
}

/// **Witness: an order is refused, with its specific reason and without a
/// row, for an input code, an entered but unconfirmed code, a code retired
/// before today, a code that starts tomorrow, an unknown code and a price
/// basis without a code; no order command without a company takes a tax
/// code; and an invoice is refused, never created untaxed, when no confirmed
/// version of the order's code applies on the invoice date.**
#[tokio::test]
async fn orders_and_invoices_refuse_tax_codes_that_do_not_apply_with_specific_reasons() {
    let s = SourceFixture::new().await;
    let (today, yesterday, tomorrow) = days(&s.f).await;
    let output = output_account(&s).await;
    let input_account = account(&s.app, &s.alice, &s.book, "VAT-IN", "asset", "operating").await;
    let vat = confirmed_code(
        &s,
        "VAT",
        "output",
        (11, 100),
        (1, 1),
        &output,
        "2020-01-01",
    )
    .await;
    confirmed_code(
        &s,
        "VAT-IN",
        "input",
        (11, 100),
        (1, 1),
        &input_account,
        "2020-01-01",
    )
    .await;
    entered_code(
        &s,
        "PENDING",
        "output",
        (11, 100),
        (1, 1),
        &output,
        "2020-01-01",
    )
    .await;
    let old = confirmed_code(
        &s,
        "OLD",
        "output",
        (10, 100),
        (1, 1),
        &output,
        "2020-01-01",
    )
    .await;
    retire(&s, &old, &yesterday).await;
    confirmed_code(&s, "NEXT", "output", (12, 100), (1, 1), &output, &tomorrow).await;
    let item = product(&s, "TEA", 15_000).await;
    let orders = count(&s.f, "archava_commerce.orders").await;

    for (code, reason) in [
        ("VAT-IN", "order_tax_code_not_output"),
        ("PENDING", "order_tax_code_not_confirmed"),
        ("OLD", "order_tax_code_not_applicable"),
        ("NEXT", "order_tax_code_not_applicable"),
        ("NOPE", "order_tax_code_unknown"),
    ] {
        for inclusive in [false, true] {
            let (st, v) = trade(&s, order(&s, &item, 2, Some((code, inclusive)))).await;
            assert_refused_as(st, &v, reason);
        }
    }
    // A code and its price basis are stated together.
    let mut basis_only = order(&s, &item, 2, None);
    basis_only["priceIncludesTax"] = json!(false);
    let (st, v) = trade(&s, basis_only).await;
    assert_refused_as(st, &v, "order_tax_code_and_basis_together");
    let mut code_only = order(&s, &item, 2, None);
    code_only["taxCode"] = json!("VAT");
    let (st, v) = trade(&s, code_only).await;
    assert_refused_as(st, &v, "order_tax_code_and_basis_together");
    // A malformed code is not a command at all.
    let (st, v) = trade(&s, order(&s, &item, 2, Some(("vat", false)))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["reason"], "invalid_submission", "{v}");

    // No order command without a company has a tax member…
    for command in [
        json!({"operation":"order.create","customerName":"Walk-in","lines":[{"productId":item["id"],"quantity":1}],"taxCode":"VAT","priceIncludesTax":false}),
        json!({"operation":"order.create_from_party","partyId":s.party["partyId"],"expectedPartyRevision":1,"lines":[{"productId":item["id"],"quantity":1}],"taxCode":"VAT","priceIncludesTax":false}),
    ] {
        let (st, v) = trade(&s, command).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
        assert_eq!(v["reason"], "invalid_submission", "{v}");
    }
    assert_eq!(count(&s.f, "archava_commerce.orders").await, orders);
    // …and the database refuses the row itself, under its own reason.
    let direct = sqlx::query("INSERT INTO archava_commerce.orders(tenant_id,id,owner_id,customer_name,total_estimate,tax_code,price_includes_tax) VALUES('acme',$1,'alice','Walk-in',0,'VAT',false)")
        .bind(Uuid::new_v4()).execute(&s.f.admin_pool).await.expect_err("a tax code needs a company");
    assert_eq!(
        guard_message(&direct),
        (
            "23514".to_owned(),
            "A sales order tax code needs a company".to_owned()
        )
    );
    assert_eq!(
        archava_domain::erp::refusals::for_guard("A sales order tax code needs a company")
            .unwrap()
            .code,
        "order_tax_code_needs_company"
    );

    // The order was created while VAT applied. The code is then retired as of
    // yesterday, so no version applies on the invoice date: the invoice is
    // refused and nothing is created untaxed.
    let o = fulfilled(&s, order(&s, &item, 2, Some(("VAT", false)))).await;
    retire(&s, &vat, &yesterday).await;
    let (st, v) = invoice_from(&s, "TOO-LATE", &o).await;
    assert_refused_as(st, &v, "invoice_tax_code_not_applicable");
    assert_eq!(count(&s.f, "archava_billing.invoices").await, 0);
    assert_eq!(count(&s.f, "archava_billing.invoice_lines").await, 0);
    // A new order can no longer name the retired code either.
    let (st, v) = trade(&s, order(&s, &item, 2, Some(("VAT", false)))).await;
    assert_refused_as(st, &v, "order_tax_code_not_applicable");

    // The company enters and confirms the version that applies from today;
    // the invoice takes that version, not the one the order was created under.
    let next = confirmed_code(&s, "VAT", "output", (12, 100), (1, 1), &output, &today).await;
    let invoice = invoiced(&s, "NEW-RATE", &o).await;
    assert_eq!(invoice["outstandingMinor"], 33_600);
    let row = invoice_row(&s, &invoice).await;
    assert_eq!(row["tax"]["codeId"], next["resourceId"]);
    assert_eq!(row["tax"]["version"], 2);
    assert_eq!(
        row["tax"]["rate"],
        json!({"numerator":12,"denominator":100})
    );
    assert_eq!(row["taxMinor"], 3_600);

    // Retiring that version later does not change the invoice (rule 6).
    retire(&s, &next, &today).await;
    let row = invoice_row(&s, &invoice).await;
    assert_eq!(row["tax"]["version"], 2);
    assert_eq!(row["taxMinor"], 3_600);
    assert_eq!(row["gross"], 33_600);
}

/// One direct invoice insert as the serving role, copying the order's
/// identity like the adapter does, with the stated amounts and snapshot; the
/// deferred provenance check runs before the transaction is rolled back.
async fn direct_invoice(
    s: &SourceFixture,
    order: &Value,
    gross: i64,
    tax: i64,
    snapshot: Option<(&Value, i32, i32, bool)>,
) -> Result<(), sqlx::Error> {
    let mut tx = archava_adapters::rls::TenantTransaction::begin(
        &s.f.runtime_pool,
        "archava_billing",
        "billing",
        archava_core::tenant::TenantId::new("acme").unwrap(),
    )
    .await
    .unwrap();
    let connection = tx.checked_executor().await.unwrap();
    let invoice = Uuid::new_v4();
    let attempt = async {
        sqlx::query("INSERT INTO archava_billing.invoices(tenant_id,id,invoice_code,customer_ref,currency,status,gross_minor,outstanding_minor,consumer_ref,owner_id,source_order_id,source_order_revision,customer_party_id,customer_party_revision,company_id,company_revision,company_code,company_name,tax_minor,tax_code_id,tax_code,tax_code_version,tax_kind,tax_rate_numerator,tax_rate_denominator,tax_base_numerator,tax_base_denominator,price_includes_tax) \
            SELECT o.tenant_id,$1,$2,o.customer_name,o.currency,'draft',$3,$3,'commerce:'||o.id::text,'alice',o.id,o.revision,o.customer_party_id,o.customer_party_revision,o.company_id,o.company_revision,o.company_code,o.company_name,\
            $4,$5::uuid,$6,$7,$8,$9,$10,$11,$12,$13 FROM archava_commerce.orders o WHERE o.id=$14::uuid")
            .bind(invoice).bind(format!("DIRECT-{invoice}").chars().take(40).collect::<String>()).bind(gross).bind(tax)
            .bind(snapshot.map(|t| t.0["resourceId"].as_str().unwrap().to_owned()))
            .bind(snapshot.map(|_| "VAT")).bind(snapshot.map(|_| 1_i32)).bind(snapshot.map(|_| "output"))
            .bind(snapshot.map(|t| t.1)).bind(snapshot.map(|t| t.2)).bind(snapshot.map(|_| 1_i32)).bind(snapshot.map(|_| 1_i32))
            .bind(snapshot.map(|t| t.3))
            .bind(order["id"].as_str().unwrap())
            .execute(&mut *connection).await?;
        sqlx::query("INSERT INTO archava_billing.invoice_lines(tenant_id,id,invoice_id,description,quantity,unit_minor,line_total_minor,recorded_by,source_order_line_id) \
            SELECT l.tenant_id,gen_random_uuid(),$1,l.title,l.quantity,l.price,l.quantity::bigint*l.price,'alice',l.id FROM archava_commerce.order_lines l WHERE l.order_id=$2::uuid")
            .bind(invoice).bind(order["id"].as_str().unwrap()).execute(&mut *connection).await?;
        sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
            .execute(&mut *connection)
            .await?;
        Ok(())
    }
    .await;
    tx.rollback().await.unwrap();
    attempt
}

/// **Witness: the database, not the adapter, is the authority on an invoice's
/// tax. As the serving role, a direct insert is refused when the tax differs
/// from the arithmetic, when the gross does not carry it, when the snapshot
/// is not the order's tax code or not a version that applies, and when an
/// untaxed invoice is made from a taxed order or the reverse; the exact row
/// the adapter writes is accepted. A stored snapshot cannot be changed and a
/// correction cannot carry an arbitrary tax.**
#[tokio::test]
async fn the_database_refuses_an_invoice_whose_tax_disagrees_with_its_order_or_the_arithmetic() {
    let s = SourceFixture::new().await;
    let account = output_account(&s).await;
    let vat = confirmed_code(
        &s,
        "VAT",
        "output",
        (11, 100),
        (1, 1),
        &account,
        "2020-01-01",
    )
    .await;
    let item = product(&s, "TEA", 15_000).await;
    let exclusive = fulfilled(&s, order(&s, &item, 2, Some(("VAT", false)))).await;
    let inclusive = fulfilled(&s, order(&s, &item, 2, Some(("VAT", true)))).await;
    let untaxed = fulfilled(&s, order(&s, &item, 2, None)).await;
    let arithmetic = (
        "23514".to_owned(),
        "Invoice tax must match the source order tax code and its arithmetic".to_owned(),
    );
    let provenance = (
        "P0001".to_owned(),
        "Invoice must match the fulfilled source order snapshot".to_owned(),
    );
    let version = (
        "23514".to_owned(),
        "No confirmed version of the invoice tax code applies on the invoice date".to_owned(),
    );

    // The rows the adapter writes pass every guard.
    direct_invoice(&s, &exclusive, 33_300, 3_300, Some((&vat, 11, 100, false)))
        .await
        .expect("the exact exclusive invoice");
    direct_invoice(&s, &inclusive, 30_000, 2_973, Some((&vat, 11, 100, true)))
        .await
        .expect("the exact inclusive invoice");
    direct_invoice(&s, &untaxed, 30_000, 0, None)
        .await
        .expect("the exact untaxed invoice");

    for (label, attempt, expected) in [
        (
            "one unit too much tax",
            direct_invoice(&s, &exclusive, 33_301, 3_301, Some((&vat, 11, 100, false))).await,
            &arithmetic,
        ),
        (
            "tax not added to the gross",
            direct_invoice(&s, &exclusive, 30_000, 3_300, Some((&vat, 11, 100, false))).await,
            &provenance,
        ),
        (
            "an inclusive order taxed as exclusive",
            direct_invoice(&s, &inclusive, 33_300, 3_300, Some((&vat, 11, 100, false))).await,
            &arithmetic,
        ),
        (
            "an inclusive order with a rounded-down tax",
            direct_invoice(&s, &inclusive, 30_000, 2_972, Some((&vat, 11, 100, true))).await,
            &arithmetic,
        ),
        (
            "a taxed order invoiced without tax",
            direct_invoice(&s, &exclusive, 30_000, 0, None).await,
            &arithmetic,
        ),
        (
            "an untaxed order invoiced with tax",
            direct_invoice(&s, &untaxed, 33_300, 3_300, Some((&vat, 11, 100, false))).await,
            &arithmetic,
        ),
        (
            "a rate the code version does not have",
            direct_invoice(&s, &exclusive, 33_000, 3_000, Some((&vat, 10, 100, false))).await,
            &version,
        ),
    ] {
        let error = attempt.expect_err(label);
        assert_eq!(&guard_message(&error), expected, "{label}");
    }
    assert_eq!(count(&s.f, "archava_billing.invoices").await, 0);

    // A stored invoice: its snapshot is immutable, its correction tax follows
    // the rule, and a correction row carries exactly the invoice's change.
    let invoice = invoiced(&s, "STORED", &exclusive).await;
    s.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Issue"})).await;
    let id = invoice["invoiceId"].as_str().unwrap();
    for (statement, message) in [
        ("UPDATE archava_billing.invoices SET tax_minor=0 WHERE id=$1::uuid", "Invoice tax snapshot is immutable"),
        ("UPDATE archava_billing.invoices SET tax_rate_numerator=1 WHERE id=$1::uuid", "Invoice tax snapshot is immutable"),
        ("UPDATE archava_billing.invoices SET price_includes_tax=true WHERE id=$1::uuid", "Invoice tax snapshot is immutable"),
        ("UPDATE archava_billing.invoices SET correction_tax_minor=-1 WHERE id=$1::uuid", "Invoice correction tax must follow the stored tax rule"),
        ("UPDATE archava_billing.invoices SET correction_minor=-11100,outstanding_minor=22200 WHERE id=$1::uuid", "Invoice correction tax must follow the stored tax rule"),
        ("INSERT INTO archava_billing.invoice_corrections(tenant_id,id,invoice_id,amount_minor,reason,recorded_by,tax_minor) VALUES('acme',gen_random_uuid(),$1::uuid,-100,'Direct','alice',-10)", "Correction tax must equal the change in its invoice tax"),
    ] {
        let mut tx = archava_adapters::rls::TenantTransaction::begin(
            &s.f.runtime_pool,
            "archava_billing",
            "billing",
            archava_core::tenant::TenantId::new("acme").unwrap(),
        )
        .await
        .unwrap();
        let error = sqlx::query(statement)
            .bind(id)
            .execute(tx.checked_executor().await.unwrap())
            .await
            .expect_err(statement);
        assert_eq!(
            guard_message(&error),
            ("23514".to_owned(), message.to_owned()),
            "{statement}"
        );
        tx.rollback().await.unwrap();
    }
    // A manual invoice cannot carry a tax snapshot.
    let mut tx = archava_adapters::rls::TenantTransaction::begin(
        &s.f.runtime_pool,
        "archava_billing",
        "billing",
        archava_core::tenant::TenantId::new("acme").unwrap(),
    )
    .await
    .unwrap();
    let error = sqlx::query("INSERT INTO archava_billing.invoices(tenant_id,id,invoice_code,customer_ref,currency,status,gross_minor,outstanding_minor,consumer_ref,owner_id,tax_minor,tax_code_id,tax_code,tax_code_version,tax_kind,tax_rate_numerator,tax_rate_denominator,tax_base_numerator,tax_base_denominator,price_includes_tax) VALUES('acme',gen_random_uuid(),'MANUAL-TAX','Walk-in','IDR','draft',111,111,'manual','alice',11,$1::uuid,'VAT',1,'output',11,100,1,1,false)")
        .bind(vat["resourceId"].as_str().unwrap())
        .execute(tx.checked_executor().await.unwrap())
        .await
        .expect_err("a manual invoice cannot carry tax");
    assert_eq!(
        guard_message(&error),
        (
            "23514".to_owned(),
            "Only an invoice made from a company sales order can carry tax".to_owned()
        )
    );
    tx.rollback().await.unwrap();
    // The stored invoice is as it was.
    assert_eq!(stored(&s, &invoice).await, (33_300, 0, 3_300, 0, 33_300, 0));
}

/// The owned mail provider of `invoice_email.rs`, for the one mail witness here.
mod mail {
    use archava_host::mail::{EmailIntegration, EmailRegistry};
    use archava_providers::resend::ResendClient;
    use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
    use serde_json::{json, Value};
    use std::sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    };

    pub(super) struct Mock {
        origin: String,
        sent: Arc<AtomicU32>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for Mock {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    async fn accept(
        State(sent): State<Arc<AtomicU32>>,
        Json(_): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        sent.fetch_add(1, Ordering::SeqCst);
        (StatusCode::OK, Json(json!({"id":"resend_msg_tax_1"})))
    }
    impl Mock {
        pub(super) async fn start() -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("http://{}/", listener.local_addr().unwrap());
            let sent = Arc::new(AtomicU32::new(0));
            let app = Router::new()
                .route("/emails", post(accept))
                .with_state(sent.clone());
            let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            Self { origin, sent, task }
        }
        pub(super) fn registry(&self) -> EmailRegistry {
            EmailRegistry::new(vec![EmailIntegration {
                id: "resend-primary".into(),
                tenant: archava_core::tenant::TenantId::new("acme").unwrap(),
                from_address: "Acme Billing <billing@acme.com>".into(),
                client: ResendClient::for_loopback(
                    &self.origin,
                    "re_synthetic_owned_email_key_12345".into(),
                )
                .unwrap(),
            }])
            .unwrap()
        }
        pub(super) fn sent(&self) -> u32 {
            self.sent.load(Ordering::SeqCst)
        }
    }
}
