//! The Indonesia tax pack on documents (`docs/modules/TAX_INDONESIA_PACK.md`):
//! a taxed invoice keeps the tax ids of its seller and buyer as they were at
//! creation (Billing 011), takes one externally issued tax invoice reference
//! that can only be replaced with a reason, and the tax report lists the
//! posted taxed documents of a period with a CSV export. Every value is text
//! the company entered; nothing is verified, generated or sent anywhere.
use super::invoice_sources::SourceFixture;
use super::tax_posting as sales;
use super::tax_purchases as buys;
use super::tax_report::buying;
use super::tax_sales::{
    confirmed_code, correct, fulfilled, guard_message, invoice_row, invoiced, order,
    output_account, product,
};
use super::*;
use axum::body::{to_bytes, Body};
use axum::http::Request;
use tower::ServiceExt;

const SELLER: &str = "01.234.567.8-999.000";
const BUYER: &str = "02.345.678.9-012.000";
/// A customer name with a comma and a quote, as a CSV must carry it.
const BUYER_NAME: &str = "Toko \"Maju\", PT";
const REFERENCE: &str = "010.000-26.00000001";

async fn org(s: &SourceFixture, kind: &str, data: Value) -> Value {
    let (st, v) = call(
        &s.app,
        "POST",
        "/erp/organizations/commands",
        json!({"tenant":"acme","mutation":{"requestId":Uuid::new_v4(),"type":kind,"data":data}}),
        Some(&s.alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["receipt"].clone()
}
/// A customer organization with a tax id and the party made from it; the
/// fixture's orders then name this party.
async fn buyer(s: &mut SourceFixture) -> Value {
    let o = org(
        s,
        "create_organization",
        json!({"name":BUYER_NAME,"legalName":"PT Toko Maju","taxId":BUYER}),
    )
    .await;
    s.party = org(s,"party.create_from_organization",json!({"organizationId":o["organizationId"],"expectedSourceRevision":1,"customer":true,"supplier":false})).await;
    o
}
/// Gives the fixture's company a tax id the reviewed way: on its source
/// organization (`organization_revision` is its current one), then refreshed
/// into the company at `company_revision`.
async fn seller_tax_id(
    s: &SourceFixture,
    tax_id: &str,
    organization_revision: i32,
    company_revision: i32,
) {
    let source: String = sqlx::query_scalar("SELECT source_organization_id::text FROM archava_organizations.companies WHERE id=$1::uuid")
        .bind(s.company["companyId"].as_str().unwrap()).fetch_one(&s.f.admin_pool).await.unwrap();
    org(s,"update_organization",json!({"id":source,"revision":organization_revision,"name":"Company A","legalName":"Company A Official","taxId":tax_id})).await;
    org(s,"company.refresh_source",json!({"id":s.company["companyId"],"expectedRevision":company_revision,"expectedSourceRevision":organization_revision+1,"reason":"Reviewed tax id of the company"})).await;
}
/// A billing command under a chosen command id, so it can be sent again.
async fn billing(
    s: &SourceFixture,
    session: &Value,
    id: Uuid,
    mut command: Value,
) -> (StatusCode, Value) {
    command["commandId"] = json!(id);
    call(
        &s.app,
        "POST",
        "/erp/billing/commands",
        json!({"tenant":"acme","mutation":command}),
        Some(session),
    )
    .await
}
fn record(invoice: &Value, revision: i64, reference: &str, code: Option<&str>) -> Value {
    let mut command = json!({"type":"record_tax_reference","invoiceId":invoice["invoiceId"],"expectedRevision":revision,"reference":reference});
    if let Some(code) = code {
        command["transactionCode"] = json!(code);
    }
    command
}
fn replace(invoice: &Value, revision: i64, reference: &str, code: Option<&str>) -> Value {
    let mut command = record(invoice, revision, reference, code);
    command["type"] = json!("replace_tax_reference");
    command["reason"] = json!("The first number was mistyped");
    command
}
/// Operation, revision and reason of every audit row of an invoice, in order.
async fn audit(s: &SourceFixture, invoice: &Value) -> Vec<(String, i32, String)> {
    sqlx::query_as("SELECT operation,revision,reason FROM archava_billing.audit WHERE invoice_id=$1::uuid ORDER BY revision,occurred_at")
        .bind(invoice["invoiceId"].as_str().unwrap()).fetch_all(&s.f.admin_pool).await.unwrap()
}
/// What the serving role is told when it runs `statement` on one invoice.
async fn runtime_refusal(s: &SourceFixture, statement: &str, invoice: &Value) -> (String, String) {
    let mut tx = s.f.runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query(sqlx::AssertSqlSafe(statement))
        .bind(invoice["invoiceId"].as_str().unwrap())
        .execute(&mut *tx)
        .await
        .expect_err(statement);
    guard_message(&error)
}

/// **Witness: an invoice made from a taxed order stores the tax id of its
/// company and of its customer party as they are at that moment, and keeps
/// them when either is changed later; an invoice made afterwards stores the
/// new ones, and an untaxed invoice stores none. An issued taxed invoice
/// takes one tax invoice reference: refused on a draft, on an untaxed
/// invoice, on a voided invoice, for a caller without `billing.settle`, and a
/// second time without the replace command; a replay returns the original
/// receipt; a replacement needs a reason and leaves the value it replaced in
/// the immutable audit trail; a settled invoice can still be replaced. The
/// serving role cannot alter the stored tax ids, remove or forge a reference,
/// or delete an audit row.**
#[tokio::test]
async fn a_taxed_invoice_keeps_its_tax_ids_and_takes_one_replaceable_tax_reference() {
    let mut s = SourceFixture::new().await;
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
    let customer = buyer(&mut s).await;
    // Orders are made while the company is at its first revision.
    let first = fulfilled(&s, order(&s, &item, 2, Some(("VAT", false)))).await;
    let second = fulfilled(&s, order(&s, &item, 2, Some(("VAT", false)))).await;
    let third = fulfilled(&s, order(&s, &item, 2, Some(("VAT", false)))).await;
    let untaxed = fulfilled(&s, order(&s, &item, 2, None)).await;
    seller_tax_id(&s, SELLER, 1, 1).await;

    let taxed = invoiced(&s, "TAXED", &first).await;
    let row = invoice_row(&s, &taxed).await;
    assert_eq!(row["sellerTaxId"], SELLER, "{row}");
    assert_eq!(row["buyerTaxId"], BUYER, "{row}");
    assert!(row["taxReference"].is_null(), "{row}");
    let plain = invoiced(&s, "PLAIN", &untaxed).await;
    let plain_row = invoice_row(&s, &plain).await;
    assert!(plain_row["sellerTaxId"].is_null() && plain_row["buyerTaxId"].is_null());
    assert!(plain_row["taxReference"].is_null() && plain_row["tax"].is_null());

    // The party and the company change their tax ids; the invoice does not.
    org(&s,"update_organization",json!({"id":customer["organizationId"],"revision":1,"name":BUYER_NAME,"legalName":"PT Toko Maju","taxId":"09.999.999.9-999.000"})).await;
    org(&s,"party.refresh_source",json!({"id":s.party["partyId"],"expectedRevision":1,"expectedSourceRevision":2,"reason":"Reviewed tax id of the customer"})).await;
    seller_tax_id(&s, "08.888.888.8-888.000", 2, 2).await;
    let kept = invoice_row(&s, &taxed).await;
    assert_eq!(
        (&kept["sellerTaxId"], &kept["buyerTaxId"]),
        (&json!(SELLER), &json!(BUYER))
    );
    assert_eq!(kept, row);
    let later = invoiced(&s, "LATER", &second).await;
    let later_row = invoice_row(&s, &later).await;
    assert_eq!(later_row["sellerTaxId"], "08.888.888.8-888.000");
    assert_eq!(later_row["buyerTaxId"], "09.999.999.9-999.000");

    // A draft takes no reference.
    let (st, v) = billing(
        &s,
        &s.alice,
        Uuid::new_v4(),
        record(&taxed, 1, REFERENCE, Some("01")),
    )
    .await;
    assert_refused_as(st, &v, "invoice_tax_reference_not_issued");
    sales::issue(&s, &taxed).await;
    // A stale revision is a plain conflict; a malformed reference is invalid.
    let (st, v) = billing(
        &s,
        &s.alice,
        Uuid::new_v4(),
        record(&taxed, 1, REFERENCE, Some("01")),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v.get("detail").is_none(), "{v}");
    for (member, value) in [
        ("reference", json!("   ")),
        ("reference", json!("x".repeat(101))),
        ("transactionCode", json!("12345678901")),
        ("verified", json!(true)),
    ] {
        let mut forged = record(&taxed, 2, REFERENCE, Some("01"));
        forged[member] = value;
        let (st, v) = billing(&s, &s.alice, Uuid::new_v4(), forged).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{member} {v}");
    }

    let request = Uuid::new_v4();
    let (st, v) = billing(
        &s,
        &s.alice,
        request,
        record(&taxed, 2, REFERENCE, Some("01")),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["action"], "record_tax_reference");
    assert_eq!(v["invoiceId"], taxed["invoiceId"]);
    assert_eq!(v["status"], "issued");
    assert_eq!(v["revision"], 3);
    assert_eq!(v["outstandingMinor"], 33_300);
    assert_eq!(v["idempotencyReplay"], false);
    // The same command again is the original receipt, and writes nothing.
    let (st, replayed) = billing(
        &s,
        &s.alice,
        request,
        record(&taxed, 2, REFERENCE, Some("01")),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{replayed}");
    assert_eq!(replayed["idempotencyReplay"], true);
    assert_eq!(
        (
            &replayed["revision"],
            &replayed["executedAt"],
            &replayed["commandId"]
        ),
        (&v["revision"], &v["executedAt"], &v["commandId"])
    );
    let recorded = invoice_row(&s, &taxed).await;
    assert_eq!(
        recorded["taxReference"],
        json!({"reference":REFERENCE,"transactionCode":"01","version":1,"enteredByCompany":true,"verifiedByArchava":false})
    );
    assert_eq!(recorded["revision"], 3);
    // Amounts, status and the stored tax ids are what they were.
    for key in [
        "gross",
        "outstanding",
        "taxMinor",
        "sellerTaxId",
        "buyerTaxId",
        "tax",
    ] {
        assert_eq!(recorded[key], row[key], "{key}");
    }
    assert_eq!(recorded["status"], "issued");
    assert_eq!(recorded["outstanding"], 33_300);

    // Set once: a second recording is refused and names the replace command.
    let (st, v) = billing(
        &s,
        &s.alice,
        Uuid::new_v4(),
        record(&taxed, 3, "ANOTHER", None),
    )
    .await;
    assert_refused_as(st, &v, "invoice_tax_reference_already_recorded");
    // Replacing a value with itself is nothing to keep.
    let (st, v) = billing(
        &s,
        &s.alice,
        Uuid::new_v4(),
        replace(&taxed, 3, REFERENCE, Some("01")),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    // A replacement without a reason is not a command.
    let mut unreasoned = replace(&taxed, 3, "010.000-26.00000002", None);
    unreasoned.as_object_mut().unwrap().remove("reason");
    let (st, v) = billing(&s, &s.alice, Uuid::new_v4(), unreasoned).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    let (st, v) = billing(
        &s,
        &s.alice,
        Uuid::new_v4(),
        replace(&taxed, 3, "010.000-26.00000002", None),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        (&v["action"], &v["revision"]),
        (&json!("replace_tax_reference"), &json!(4))
    );
    assert_eq!(
        invoice_row(&s, &taxed).await["taxReference"],
        json!({"reference":"010.000-26.00000002","transactionCode":null,"version":2,"enteredByCompany":true,"verifiedByArchava":false})
    );
    // The audit trail keeps every value: what was recorded, and what a
    // replacement replaced.
    let trail = audit(&s, &taxed).await;
    assert_eq!(
        trail[2..],
        [
            (
                "record_tax_reference".to_owned(),
                3,
                "Recorded tax invoice reference \"010.000-26.00000001\" (transaction code \"01\")".to_owned()
            ),
            (
                "replace_tax_reference".to_owned(),
                4,
                "Replaced tax invoice reference \"010.000-26.00000001\" (transaction code \"01\") with \"010.000-26.00000002\" (no transaction code): The first number was mistyped".to_owned()
            ),
        ]
    );
    assert_eq!(trail.len(), 4, "{trail:?}");

    // An untaxed invoice takes none; an invoice without a reference has
    // nothing to replace; a caller without billing.settle is refused.
    sales::issue(&s, &plain).await;
    let (st, v) = billing(
        &s,
        &s.alice,
        Uuid::new_v4(),
        record(&plain, 2, REFERENCE, None),
    )
    .await;
    assert_refused_as(st, &v, "invoice_tax_reference_untaxed");
    sales::issue(&s, &later).await;
    let (st, v) = billing(
        &s,
        &s.alice,
        Uuid::new_v4(),
        replace(&later, 2, REFERENCE, None),
    )
    .await;
    assert_refused_as(st, &v, "invoice_tax_reference_missing");
    sqlx::query("INSERT INTO archava_billing.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','bob','human',ARRAY['billing.read','billing.write'],'all')").execute(&s.f.admin_pool).await.unwrap();
    let (st, v) = billing(
        &s,
        &s.bob,
        Uuid::new_v4(),
        record(&later, 2, REFERENCE, None),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    let (st, v) = call(
        &s.app,
        "POST",
        "/erp/billing/commands",
        json!({"tenant":"acme","mutation":record(&later, 2, REFERENCE, None)}),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "{v}");
    assert!(invoice_row(&s, &later).await["taxReference"].is_null());

    // A settled invoice still carries, and can replace, its reference.
    let paid = s.payment(&taxed, "succeeded", 33_300).await;
    assert_eq!(paid["status"], "settled", "{paid}");
    let revision = invoice_row(&s, &taxed).await["revision"].as_i64().unwrap();
    let (st, v) = billing(
        &s,
        &s.alice,
        Uuid::new_v4(),
        replace(&taxed, revision, "010.000-26.00000003", Some("02")),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "settled");
    assert_eq!(invoice_row(&s, &taxed).await["taxReference"]["version"], 3);
    // A voided invoice takes none.
    let another = invoiced(&s, "VOIDED", &third).await;
    sales::issue(&s, &another).await;
    s.bill(json!({"type":"void_invoice","invoiceId":another["invoiceId"],"expectedRevision":2,"reason":"Issued by mistake"})).await;
    let (st, v) = billing(
        &s,
        &s.alice,
        Uuid::new_v4(),
        record(&another, 3, REFERENCE, None),
    )
    .await;
    assert_refused_as(st, &v, "invoice_tax_reference_not_issued");

    // The serving role, by direct SQL.
    let refused = |message: &str| ("23514".to_owned(), message.to_owned());
    for (statement, target, message) in [
        ("UPDATE archava_billing.invoices SET seller_tax_id='00' WHERE id=$1::uuid", &taxed, "Invoice tax identifier snapshot is immutable"),
        ("UPDATE archava_billing.invoices SET buyer_tax_id=NULL WHERE id=$1::uuid", &taxed, "Invoice tax identifier snapshot is immutable"),
        ("UPDATE archava_billing.invoices SET tax_reference=NULL,tax_transaction_code=NULL,tax_reference_version=0,revision=revision+1 WHERE id=$1::uuid", &taxed, "A recorded tax invoice reference cannot be removed"),
        ("UPDATE archava_billing.invoices SET tax_reference='FORGED',tax_reference_version=1,revision=revision+1 WHERE id=$1::uuid", &taxed, "The invoice already has a tax invoice reference"),
        ("UPDATE archava_billing.invoices SET tax_reference='FORGED',tax_reference_version=tax_reference_version+1 WHERE id=$1::uuid", &taxed, "A tax invoice reference changes alone, one version at a time"),
        ("UPDATE archava_billing.invoices SET tax_reference='FORGED',tax_reference_version=2,revision=revision+1 WHERE id=$1::uuid", &later, "The invoice has no tax invoice reference to replace"),
        ("UPDATE archava_billing.invoices SET tax_reference='FORGED',tax_reference_version=1,revision=revision+1 WHERE id=$1::uuid", &another, "A tax invoice reference can be recorded only on an issued or settled invoice"),
    ] {
        assert_eq!(runtime_refusal(&s, statement, target).await, refused(message), "{statement}");
    }
    // An untaxed invoice cannot be given identifiers or a reference at all.
    for statement in [
        "UPDATE archava_billing.invoices SET tax_reference='FORGED',tax_reference_version=1,revision=revision+1 WHERE id=$1::uuid",
        "UPDATE archava_billing.invoices SET seller_tax_id='00' WHERE id=$1::uuid",
    ] {
        let (state, _) = runtime_refusal(&s, statement, &plain).await;
        assert_eq!(state, "23514", "{statement}");
    }
    // The history is the audit trail: the serving role cannot delete or edit
    // a row of it, and the owner's edit is refused by the table itself.
    for statement in [
        "DELETE FROM archava_billing.audit WHERE invoice_id=$1::uuid AND operation='replace_tax_reference'",
        "UPDATE archava_billing.audit SET reason='rewritten' WHERE invoice_id=$1::uuid AND operation='record_tax_reference'",
    ] {
        let (state, message) = runtime_refusal(&s, statement, &taxed).await;
        assert_eq!(state, "42501", "{statement}: {message}");
    }
    let owner = sqlx::query("DELETE FROM archava_billing.audit WHERE invoice_id=$1::uuid AND operation='replace_tax_reference'")
        .bind(taxed["invoiceId"].as_str().unwrap())
        .execute(&s.f.admin_pool)
        .await
        .expect_err("audit history is immutable");
    assert_eq!(
        guard_message(&owner).1,
        "Billing audit history is immutable"
    );
    assert_eq!(audit(&s, &taxed).await.len(), 6);
    assert_eq!(invoice_row(&s, &taxed).await["sellerTaxId"], SELLER);
}

async fn documents(
    app: &Router,
    session: Option<&Value>,
    book: &Value,
    query: &str,
) -> (StatusCode, Value) {
    call(
        app,
        "GET",
        &format!(
            "/erp/accounting/tax-report/documents?tenant=acme&bookId={}&{query}",
            book["bookId"].as_str().unwrap()
        ),
        Value::Null,
        session,
    )
    .await
}
/// The CSV download: status, content type, content disposition and the bytes.
async fn download(
    app: &Router,
    session: &Value,
    book: &Value,
    from: &str,
    to: &str,
) -> (StatusCode, String, String, Vec<u8>) {
    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/erp/accounting/tax-report/documents.csv?tenant=acme&bookId={}&from={from}&to={to}",
            book["bookId"].as_str().unwrap()
        ))
        .header("x-archava-client", "cli")
        .header(
            "authorization",
            format!("Bearer {}", session["token"].as_str().unwrap()),
        )
        .header("x-archava-csrf", session["csrf"].as_str().unwrap())
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    let (status, kind, disposition) = (
        response.status(),
        header("content-type"),
        header("content-disposition"),
    );
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (status, kind, disposition, bytes.to_vec())
}

/// **Witness: on one IDR book a taxed sale (2 × 15,000 at 11%, exclusive) to
/// a customer whose name holds a comma and a quote is issued, posted and
/// given a tax invoice reference, credited by 11,100 and the credit posted;
/// a supplier bill for goods of 140,000 with a printed tax of 15,401 is
/// matched and posted. The documents listing of the period returns exactly
/// those three sources, each with its document code, the date the report
/// uses, the counterparty, the tax id the invoice stored, the tax code
/// version, the taxable amount and the tax in ledger minor units, and the
/// reference. A second taxed sale that is captured and not posted, an untaxed
/// posted sale and a period that ends before the source date are not in it.
/// The listing is paged by source. The CSV of the same period is, byte for
/// byte, the disclaimer line, the header and those three rows with RFC 4180
/// quoting, served as `text/csv` under a file name made of the book code and
/// the period. Both reads need `tax.read` and `reports.read`, a session, a
/// valid range and a book in scope.**
#[tokio::test]
async fn the_documents_listing_and_its_csv_give_the_posted_taxed_sources_of_a_period() {
    let mut s = SourceFixture::new().await;
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
    s.enable().await;
    let item = product(&s, "TEA", 15_000).await;
    buyer(&mut s).await;
    // The orders and the whole purchase side run while the company is at its
    // first revision; the company's tax id is reviewed in afterwards, before
    // any invoice is created.
    let sold = fulfilled(&s, order(&s, &item, 2, Some(("VAT", false)))).await;
    let unposted = fulfilled(&s, order(&s, &item, 2, Some(("VAT", false)))).await;
    let untaxed = fulfilled(&s, order(&s, &item, 2, None)).await;
    let (ar, revenue, clearing, party) = (
        s.ar.clone(),
        s.revenue.clone(),
        s.clearing.clone(),
        s.party.clone(),
    );

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
    let bill_source: Uuid = sqlx::query_scalar("SELECT id FROM archava_accounting.purchase_sources WHERE effect='bill' AND document_id=$1::uuid")
        .bind(document["id"].as_str().unwrap()).fetch_one(&p.f.admin_pool).await.unwrap();

    let s = SourceFixture {
        f: p.f,
        app: p.app,
        alice: p.alice,
        bob: p.bob,
        company: p.company,
        book: p.book,
        ar,
        revenue,
        clearing,
        period: p.period,
        party,
        currency: p.currency,
        unit: p.unit,
    };
    seller_tax_id(&s, SELLER, 1, 1).await;
    let taxed = invoiced(&s, "INV-TAXED", &sold).await;
    let issued = sales::issue(&s, &taxed).await;
    sales::post_taxed(&s, &issued, "TAXED-ISSUE", &vat_out).await;
    let (st, v) = billing(
        &s,
        &s.alice,
        Uuid::new_v4(),
        record(&taxed, 2, REFERENCE, Some("01")),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let credit = correct(&s, &taxed, -11_100).await;
    sales::post_taxed(&s, &credit, "TAXED-CREDIT", &vat_out).await;
    // Captured and not posted; and untaxed and posted.
    let waiting = invoiced(&s, "INV-WAITING", &unposted).await;
    sales::issue(&s, &waiting).await;
    let plain = invoiced(&s, "INV-PLAIN", &untaxed).await;
    let plain_issue = sales::issue(&s, &plain).await;
    s.post_source(&plain_issue, "PLAIN-ISSUE").await;

    let source = |command: &Value| Uuid::parse_str(command["commandId"].as_str().unwrap()).unwrap();
    let sale = |id: Uuid, effect: &str, base: &str, tax: &str| {
        json!({"sourceId":id,"date":date,"side":"output","effect":effect,"documentKind":"invoice","documentId":taxed["invoiceId"],
            "documentCode":"INV-TAXED","counterpartyName":BUYER_NAME,"counterpartyTaxId":BUYER,"companyTaxId":SELLER,
            "taxCode":"VAT","taxCodeVersion":1,"rate":{"numerator":11,"denominator":100},"base":{"numerator":1,"denominator":1},
            "taxableBaseMinor":base,"taxMinor":tax,"taxReference":REFERENCE,"taxTransactionCode":"01"})
    };
    // The order of the listing: source date, then source id.
    let mut expected = [
        (
            source(&issued),
            sale(source(&issued), "issue", "3000000", "330000"),
        ),
        (
            source(&credit),
            sale(source(&credit), "correction", "-1000000", "-110000"),
        ),
        (
            bill_source,
            json!({"sourceId":bill_source,"date":date,"side":"input","effect":"bill","documentKind":"supplier_document","documentId":document["id"],
                "documentCode":"PRINTED","counterpartyName":"Explicit supplier master","counterpartyTaxId":null,"companyTaxId":null,
                "taxCode":"VAT-IN","taxCodeVersion":1,"rate":{"numerator":11,"denominator":100},"base":{"numerator":1,"denominator":1},
                "taxableBaseMinor":"14000000","taxMinor":"1540100","taxReference":null,"taxTransactionCode":null}),
        ),
    ];
    expected.sort_by_key(|(id, _)| *id);
    let rows: Vec<Value> = expected.iter().map(|(_, row)| row.clone()).collect();

    let range = format!("from={date}&to={date}");
    let (st, listing) = documents(&s.app, Some(&s.alice), &s.book, &range).await;
    assert_eq!(st, StatusCode::OK, "{listing}");
    assert_eq!(listing["documents"], json!(rows), "{listing}");
    assert_eq!(
        listing["page"],
        json!({"limit":100,"hasMore":false,"nextCursor":null})
    );
    assert_eq!(listing["book"]["code"], "BOOK-IDR");
    assert_eq!(
        (&listing["from"], &listing["to"]),
        (&json!(date), &json!(date))
    );
    assert_eq!(listing["periodDate"], "source_date");
    assert!(listing["observedAt"].is_string());
    assert_eq!(listing["enteredByCompany"], true);
    assert_eq!(listing["verifiedByArchava"], false);
    let notice = listing["notice"].as_str().unwrap();
    assert!(notice.contains("not a tax return"), "{notice}");
    assert!(
        notice.contains("not in any tax authority's import format"),
        "{notice}"
    );
    // The rows are the sources the report adds up.
    let (st, report) = call(
        &s.app,
        "GET",
        &format!(
            "/erp/accounting/tax-report?tenant=acme&bookId={}&{range}",
            s.book["bookId"].as_str().unwrap()
        ),
        Value::Null,
        Some(&s.alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{report}");
    assert_eq!(
        report["totals"],
        json!({"outputTaxMinor":"220000","inputTaxMinor":"1540100","differenceMinor":"-1320100"})
    );

    // Paged by source: two, then the third after the second.
    let (st, page) = documents(&s.app, Some(&s.alice), &s.book, &format!("{range}&limit=2")).await;
    assert_eq!(st, StatusCode::OK, "{page}");
    assert_eq!(page["documents"], json!(rows[..2]));
    assert_eq!(
        page["page"],
        json!({"limit":2,"hasMore":true,"nextCursor":expected[1].0})
    );
    let (st, rest) = documents(
        &s.app,
        Some(&s.alice),
        &s.book,
        &format!("{range}&limit=2&cursor={}", expected[1].0),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{rest}");
    assert_eq!(rest["documents"], json!(rows[2..]));
    assert_eq!(
        rest["page"],
        json!({"limit":2,"hasMore":false,"nextCursor":null})
    );
    // Out of range: a period that ends before the source date, or starts after it.
    for outside in [
        "from=2000-01-01&to=2000-12-31",
        "from=2100-01-01&to=2100-12-31",
    ] {
        let (st, none) = documents(&s.app, Some(&s.alice), &s.book, outside).await;
        assert_eq!(st, StatusCode::OK, "{none}");
        assert_eq!(none["documents"], json!([]), "{outside}");
    }

    // The CSV, byte for byte.
    let line = |row: &Value, tail: &str| -> String {
        let base = match row["effect"].as_str().unwrap() {
            "issue" => "30000.00,3300.00",
            "correction" => "-10000.00,-1100.00",
            _ => "140000.00,15401.00",
        };
        match row["side"].as_str().unwrap() {
            "output" => format!("output,invoice,{},INV-TAXED,{date},\"Toko \"\"Maju\"\", PT\",{BUYER},{SELLER},VAT,1,11/100,1/1,{base},IDR,{tail}\r\n", row["effect"].as_str().unwrap()),
            _ => format!("input,supplier_document,bill,PRINTED,{date},Explicit supplier master,,,VAT-IN,1,11/100,1/1,{base},IDR,,\r\n"),
        }
    };
    let mut csv = String::from("\"Draft export of data your company entered in Archava. It is not a tax return and is not in any tax authority's import format. Archava does not verify tax codes, rates, tax ids or tax invoice references.\"\r\n");
    csv.push_str("Side,Document type,Effect,Document code,Date,Counterparty,Counterparty tax id,Company tax id,Tax code,Tax code version,Rate,Base,Taxable base,Tax,Currency,Tax invoice reference,Transaction code\r\n");
    for row in &rows {
        csv.push_str(&line(row, &format!("{REFERENCE},01")));
    }
    let (st, kind, disposition, bytes) = download(&s.app, &s.alice, &s.book, &date, &date).await;
    assert_eq!(st, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
    assert_eq!(kind, "text/csv; charset=utf-8");
    assert_eq!(
        disposition,
        format!("attachment; filename=\"tax-documents-BOOK-IDR-{date}-to-{date}.csv\"")
    );
    assert_eq!(String::from_utf8(bytes).unwrap(), csv);
    for forbidden in ["faktur", "coretax", "efaktur"] {
        assert!(
            !csv.to_lowercase().contains(forbidden)
                && !disposition.to_lowercase().contains(forbidden)
        );
    }
    // An empty period is the two heading lines.
    let (st, _, _, bytes) = download(&s.app, &s.alice, &s.book, "2000-01-01", "2000-12-31").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(String::from_utf8(bytes).unwrap().matches("\r\n").count(), 2);

    // Refusals, for both reads.
    let (st, v) = documents(&s.app, None, &s.book, &range).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "{v}");
    for (invalid, reason) in [
        ("from=2026-02-30&to=2026-03-01".to_owned(), "invalid_query"),
        (format!("from={date}&to=2000-01-01"), "invalid_query"),
        (format!("{range}&limit=0"), "invalid_query"),
        (format!("{range}&limit=101"), "invalid_query"),
        // A missing, unreadable or unknown parameter is not a query at all.
        (format!("from={date}"), "malformed_submission"),
        (format!("{range}&cursor=not-a-uuid"), "malformed_submission"),
        (format!("{range}&format=efaktur"), "malformed_submission"),
    ] {
        let (st, v) = documents(&s.app, Some(&s.alice), &s.book, &invalid).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{invalid} {v}");
        assert_eq!(v["reason"], reason, "{invalid} {v}");
    }
    let (st, _, _, bytes) = download(&s.app, &s.alice, &s.book, &date, "2000-01-01").await;
    assert_eq!(
        st,
        StatusCode::BAD_REQUEST,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    // Without both grants: the reader of the fixture has reports.read only.
    let reader = login(&s.app, "reader").await;
    let (st, v) = documents(&s.app, Some(&reader), &s.book, &range).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    let (st, kind, _, _) = download(&s.app, &reader, &s.book, &date, &date).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    assert!(kind.starts_with("application/json"), "{kind}");
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=ARRAY['books.read','tax.read'] WHERE principal_id='reader'").execute(&s.f.admin_pool).await.unwrap();
    let (st, v) = documents(&s.app, Some(&reader), &s.book, &range).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    // A book that does not exist, and one outside the caller's scope.
    let missing = json!({"bookId":Uuid::new_v4()});
    let (st, v) = documents(&s.app, Some(&s.alice), &missing, &range).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
    let assigned = login(&s.app, "assigned").await;
    let (st, v) = documents(&s.app, Some(&assigned), &s.book, &range).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
    let (st, _, _, _) = download(&s.app, &assigned, &s.book, &date, &date).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}
