//! Actual posted sources, business-date terms and independent GL controls.
use super::*;
use invoice_sources::SourceFixture;

pub(super) async fn read(
    app: &Router,
    session: &Value,
    book: &Value,
    side: &str,
    date: &str,
    extra: &str,
) -> (StatusCode, Value) {
    call(
        app,
        "GET",
        &format!(
            "/erp/accounting/subledger-report?tenant=acme&bookId={}&side={side}&asOf={date}{extra}",
            book["bookId"].as_str().unwrap()
        ),
        Value::Null,
        Some(session),
    )
    .await
}
pub(super) async fn accepted(
    app: &Router,
    session: &Value,
    book: &Value,
    side: &str,
    date: &str,
) -> Value {
    let (st, value) = read(app, session, book, side, date, "").await;
    assert_eq!(st, StatusCode::OK, "{value}");
    value
}
#[tokio::test]
async fn subledger_real_invoice_effects_cutoff_due_history_and_generic_control_difference() {
    let s = SourceFixture::new().await;
    s.enable().await;
    let invoice = s.invoice("AGING-ORIGINAL").await;
    let issue=s.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Original invoice before aging"})).await;
    let(date,before,later,due):(String,String,String,String)=sqlx::query_as("SELECT source_date::text,(source_date-1)::text,(source_date+2)::text,(source_date-10)::text FROM archava_accounting.invoice_sources WHERE id=$1")
        .bind(Uuid::parse_str(issue["commandId"].as_str().unwrap()).unwrap()).fetch_one(&s.f.admin_pool).await.unwrap();
    let pending = accepted(&s.app, &s.alice, &s.book, "receivable", &date).await;
    assert_eq!(pending["positions"], json!([]));
    assert_eq!(pending["coverage"]["pendingMonetarySourceCount"], 1);
    s.post_source(&issue, "AGING-ISSUE").await;
    let historical = accepted(&s.app, &s.alice, &s.book, "receivable", &before).await;
    assert_eq!(historical["totals"]["netOutstandingMinor"], "0");
    let current = accepted(&s.app, &s.alice, &s.book, "receivable", &date).await;
    assert_eq!(current["totals"]["unknownTermsMinor"], "3000000");
    assert_eq!(current["postedSourceControlsAgree"], true);
    let correction=s.bill(json!({"type":"append_correction","invoiceId":invoice["invoiceId"],"amountMinor":-5000,"reason":"Actual reviewed credit not yet posted"})).await;
    let unposted = accepted(&s.app, &s.alice, &s.book, "receivable", &date).await;
    assert_eq!(unposted["totals"]["netOutstandingMinor"], "3000000");
    assert_eq!(unposted["coverage"]["pendingMonetarySourceCount"], 1);
    let terms=ok(&s.app,&s.alice,json!({"type":"invoice_terms.set","sourceId":issue["commandId"],"expectedDocumentRevision":3,"expectedRevision":0,"effectiveFrom":later,"condition":{"kind":"due_on","date":due},"reason":"Explicit later effective date under current reviewed knowledge"})).await;
    let current = accepted(&s.app, &s.alice, &s.book, "receivable", &date).await;
    assert!(current["positions"][0]["terms"].is_null());
    assert_eq!(current["totals"]["unknownTermsMinor"], "3000000");
    let future = accepted(&s.app, &s.alice, &s.book, "receivable", &later).await;
    assert_eq!(future["totals"]["overdue1To30Minor"], "3000000");
    assert_eq!(
        future["positions"][0]["terms"]["termsRevision"],
        terms["revision"]
    );
    assert!(future["positions"][0]["terms"]["recordedAt"].is_string());
    s.post_source(&correction, "AGING-CREDIT").await;
    let paid = s.payment(&invoice, "succeeded", 10000).await;
    s.post_source(&paid, "AGING-COLLECTION").await;
    let refund = s.payment(&invoice, "refunded", 2000).await;
    s.post_source(&refund, "AGING-REFUND").await;
    let settled = accepted(&s.app, &s.alice, &s.book, "receivable", &date).await;
    assert_eq!(settled["totals"]["netOutstandingMinor"], "1700000");
    assert_eq!(settled["postedSourceControlsAgree"], true);
    assert_eq!(settled["coverage"]["pendingMonetarySourceCount"], 0);
    let equity = account(
        &s.app,
        &s.alice,
        &s.book,
        "UNATTRIBUTED",
        "equity",
        "financing",
    )
    .await;
    post(
        &s.app,
        &s.alice,
        &s.bob,
        &s.book,
        "CONTROL-ADJUSTMENT",
        vec![line(&s.ar, "100", "0"), line(&equity, "0", "100")],
    )
    .await;
    let mismatch = accepted(&s.app, &s.alice, &s.book, "receivable", &date).await;
    assert_eq!(mismatch["totals"]["netOutstandingMinor"], "1700000");
    assert_eq!(mismatch["controls"][0]["differenceMinor"], "100");
    assert_eq!(mismatch["postedSourceControlsAgree"], false);
    let new_ar = account(&s.app, &s.alice, &s.book, "NEW-AR", "asset", "operating").await;
    ok(&s.app,&s.alice,json!({"type":"invoice_policy.set","bookId":s.book["bookId"],"expectedRevision":1,"currencyCatalogue":"SIX-2026-09-17","sourceUnit":"whole_idr","mappings":[
        {"role":"receivable","accountId":new_ar["resourceId"],"expectedAccountRevision":1},
        {"role":"revenue","accountId":s.revenue["resourceId"],"expectedAccountRevision":1},
        {"role":"payment_clearing","accountId":s.clearing["resourceId"],"expectedAccountRevision":1}],"reason":"Explicit new mapping applies to future original invoices"})).await;
    let next = s.invoice("AGING-NEW-MAPPING").await;
    let issued=s.bill(json!({"type":"issue_invoice","invoiceId":next["invoiceId"],"expectedRevision":1,"reason":"Future invoice captures the new original control account"})).await;
    let mut reviewed = s.source_draft(&issued, "AGING-NEW-AR");
    reviewed["command"]["accounts"][0]["accountId"] = new_ar["resourceId"].clone();
    let (st, draft) = send(&s.app, &s.alice, &reviewed).await;
    assert_eq!(st, StatusCode::OK, "{draft}");
    assert_eq!(
        send(&s.app, &s.bob, &action("journal.post", &draft["receipt"]))
            .await
            .0,
        StatusCode::OK
    );
    let historical_mapping = accepted(&s.app, &s.alice, &s.book, "receivable", &date).await;
    assert_eq!(
        historical_mapping["totals"]["netOutstandingMinor"],
        "4700000"
    );
    assert_eq!(historical_mapping["controls"].as_array().unwrap().len(), 2);
    for (id, expected, difference) in [
        (&s.ar["resourceId"], "1700000", "100"),
        (&new_ar["resourceId"], "3000000", "0"),
    ] {
        let control = historical_mapping["controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| &r["accountId"] == id)
            .unwrap();
        assert_eq!(control["expectedMinor"], expected);
        assert_eq!(control["differenceMinor"], difference);
    }
    for suffix in ["&limit=0", "&limit=-1", "&limit=101", "&actorId=bob"] {
        assert_eq!(
            read(&s.app, &s.alice, &s.book, "receivable", &date, suffix)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        read(&s.app, &s.alice, &s.book, "invented", &date, "")
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        read(&s.app, &s.alice, &s.book, "receivable", "2026-02-29", "")
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=array_remove(permissions,'terms.read') WHERE principal_id='alice'").execute(&s.f.admin_pool).await.unwrap();
    assert_eq!(
        read(&s.app, &s.alice, &s.book, "receivable", &date, "")
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn subledger_totals_cover_every_original_invoice_beyond_a_page() {
    let s = SourceFixture::new().await;
    s.enable().await;
    let mut date = String::new();
    for n in 0..101 {
        let invoice = if n < 3 {
            s.invoice_many_amount(&format!("AGING-PAGE-{n}"), 1_000_000_000, 10_000, 4)
                .await
        } else {
            s.invoice(&format!("AGING-PAGE-{n}")).await
        };
        let issue=s.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Actual issued document beyond the first report page"})).await;
        s.post_source(&issue, &format!("AGING-POST-{n}")).await;
        if date.is_empty() {
            date = sqlx::query_scalar(
                "SELECT source_date::text FROM archava_accounting.invoice_sources WHERE id=$1",
            )
            .bind(Uuid::parse_str(issue["commandId"].as_str().unwrap()).unwrap())
            .fetch_one(&s.f.admin_pool)
            .await
            .unwrap();
        }
    }
    let (st, first) = read(&s.app, &s.alice, &s.book, "receivable", &date, "&limit=100").await;
    assert_eq!(st, StatusCode::OK, "{first}");
    assert_eq!(first["positions"].as_array().unwrap().len(), 100);
    assert_eq!(first["coverage"]["qualifyingDocumentCount"], 101);
    assert_eq!(first["totals"]["unknownTermsMinor"], "12000000294000000");
    assert_eq!(first["postedSourceControlsAgree"], true);
    let (st, second) = read(
        &s.app,
        &s.alice,
        &s.book,
        "receivable",
        &date,
        &format!(
            "&limit=100&cursor={}",
            first["page"]["nextCursor"].as_str().unwrap()
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{second}");
    assert_eq!(second["positions"].as_array().unwrap().len(), 1);
    assert_eq!(second["totals"], first["totals"]);
    assert_eq!(second["page"]["hasMore"], false);
    for row in first["positions"].as_array().unwrap() {
        assert!(!second["positions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["documentId"] == row["documentId"]));
    }
}
