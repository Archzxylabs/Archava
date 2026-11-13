use super::bank::{bank_account, match_row, statement};
use super::bank_periods::{check, confirm, empty_statement, policy};
use super::*;

async fn readiness(app: &Router, actor: &Value, period: &Value) -> (StatusCode, Value) {
    call(
        app,
        "GET",
        &format!(
            "/erp/accounting/close-readiness?tenant=acme&periodId={}",
            period["resourceId"].as_str().unwrap()
        ),
        Value::Null,
        Some(actor),
    )
    .await
}
/// The blocker reasons with their counts, in report order.
fn blockers(value: &Value) -> Vec<(String, i64)> {
    value["blockers"]
        .as_array()
        .expect("the report lists its blockers")
        .iter()
        .map(|b| {
            (
                b["reason"].as_str().unwrap().to_string(),
                b["count"].as_i64().unwrap(),
            )
        })
        .collect()
}
/// Reads the report, and proves a report that is not ready means the close is
/// refused. Returns the report.
async fn blocked(app: &Router, a: &Value, period: &Value, expected: &[(&str, i64)]) -> Value {
    let (st, value) = readiness(app, a, period).await;
    assert_eq!(st, StatusCode::OK, "{value}");
    let expected: Vec<(String, i64)> = expected.iter().map(|(r, c)| (r.to_string(), *c)).collect();
    assert_eq!(blockers(&value), expected, "{value}");
    assert_eq!(value["ready"], false, "{value}");
    assert_eq!(value["canClose"], false, "{value}");
    assert_eq!(
        send(app, a, &action("period.close", period)).await.0,
        StatusCode::CONFLICT,
        "a period the report calls not ready must not close"
    );
    value
}

/// **Witness: the close-readiness report names every condition the period's
/// close guards refuse on, and agrees with them.** At each step of a period
/// with a draft journal and a bank account, the report lists what is still
/// outstanding and `period.close` is refused; once the report is ready the
/// same close succeeds. The report writes nothing and respects grants.
#[tokio::test]
async fn close_readiness_names_each_outstanding_condition_and_agrees_with_the_close() {
    let (f, app, a, b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    let period = period(&app, &a, &book).await;
    let cash = account(&app, &a, &book, "BANK", "asset", "cash").await;
    let equity = account(&app, &a, &book, "EQUITY", "equity", "financing").await;

    // An empty period has nothing outstanding; only a closer may close it.
    let (st, empty) = readiness(&app, &a, &period).await;
    assert_eq!(st, StatusCode::OK, "{empty}");
    assert_eq!(empty["ready"], true, "{empty}");
    assert_eq!(empty["canClose"], true);
    assert_eq!(blockers(&empty), []);
    assert_eq!(empty["period"]["state"], "open");
    assert_eq!(empty["period"]["code"], "2026-11");
    let reader = login(&app, "reader").await;
    let (st, read) = readiness(&app, &reader, &period).await;
    assert_eq!(st, StatusCode::OK, "{read}");
    assert_eq!(read["ready"], true);
    assert_eq!(read["canClose"], false, "a reader cannot close");
    let (st, _) = call(
        &app,
        "GET",
        &format!(
            "/erp/accounting/close-readiness?tenant=acme&periodId={}",
            Uuid::new_v4()
        ),
        Value::Null,
        Some(&a),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = call(
        &app,
        "GET",
        &format!(
            "/erp/accounting/close-readiness?tenant=acme&periodId={}",
            period["resourceId"].as_str().unwrap()
        ),
        Value::Null,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    // A draft journal blocks; posting it clears the blocker.
    let (st, drafted) = send(
        &app,
        &a,
        &draft(
            &book,
            "OPENING",
            vec![line(&cash, "500", "0"), line(&equity, "0", "500")],
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{drafted}");
    blocked(&app, &a, &period, &[("draft_journals", 1)]).await;
    let (st, voided) = send(&app, &a, &action("journal.void", &drafted["receipt"])).await;
    assert_eq!(st, StatusCode::OK, "{voided}");
    assert_eq!(readiness(&app, &a, &period).await.1["ready"], true);

    // A registered bank needs an opening basis, then coverage, matching and a
    // fresh independent review.
    let bank = bank_account(&app, &a, &book, &cash).await;
    blocked(&app, &a, &period, &[("banks_without_policy", 1)]).await;
    let policy = policy(&app, &a, &bank, true).await;
    let pending = blocked(&app, &a, &period, &[("bank_reviews_incomplete", 1)]).await;
    assert_eq!(pending["bankReviews"][0]["checkReady"], false);
    assert_eq!(pending["bankReviews"][0]["reviewed"], false);
    assert_eq!(
        pending["bankReviews"][0]["bankPolicyId"],
        policy["resourceId"]
    );
    let (st, r) = send(&app, &a, &statement(&bank, &cash)).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    blocked(
        &app,
        &a,
        &period,
        &[("unmatched_bank_rows", 3), ("bank_reviews_incomplete", 1)],
    )
    .await;
    let rows = snapshot(&app, &a, &c).await.1["bank_rows"]
        .as_array()
        .unwrap()
        .clone();
    for (reference, amount, incoming, code) in [
        ("receipt-A", "15000", true, "IN"),
        ("payment-A", "3000", false, "OUT-A"),
        ("payment-B", "2000", false, "OUT-B"),
    ] {
        let row = rows.iter().find(|r| r["externalRef"] == reference).unwrap();
        let lines = if incoming {
            vec![line(&cash, amount, "0"), line(&equity, "0", amount)]
        } else {
            vec![line(&equity, amount, "0"), line(&cash, "0", amount)]
        };
        let journal = post(&app, &a, &b, &book, code, lines).await;
        let (st, r) = send(
            &app,
            &a,
            &match_row(row, &journal, if incoming { 1 } else { 2 }, &cash, amount),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{r}");
    }
    blocked(&app, &a, &period, &[("bank_reviews_incomplete", 1)]).await;
    let (st, r) = send(
        &app,
        &a,
        &empty_statement(
            &bank,
            &cash,
            "NOV-SECOND",
            "2026-11-16",
            "2026-11-30",
            "10000",
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    // Evidence is complete but nobody has reviewed it yet.
    let unreviewed = blocked(&app, &a, &period, &[("bank_reviews_incomplete", 1)]).await;
    assert_eq!(unreviewed["bankReviews"][0]["checkReady"], true);
    assert_eq!(unreviewed["bankReviews"][0]["reviewed"], false);

    let current = check(&app, &b, &policy, &period).await.1;
    let (st, proof) = send(&app, &b, &confirm(&current)).await;
    assert_eq!(st, StatusCode::OK, "{proof}");

    // Reading the report any number of times changes nothing.
    let rows_before: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM archava_accounting.commands)+(SELECT count(*) FROM archava_accounting.audit)")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    let (st, ready) = readiness(&app, &a, &period).await;
    assert_eq!(st, StatusCode::OK, "{ready}");
    assert_eq!(blockers(&ready), []);
    assert_eq!(ready["ready"], true, "{ready}");
    assert_eq!(ready["canClose"], true);
    assert_eq!(ready["bankReviews"][0]["reviewed"], true);
    let rows_after: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM archava_accounting.commands)+(SELECT count(*) FROM archava_accounting.audit)")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(rows_after, rows_before);

    // Ready means the close is accepted.
    let (st, closed) = send(&app, &a, &action("period.close", &period)).await;
    assert_eq!(st, StatusCode::OK, "{closed}");
    let (st, after) = readiness(&app, &a, &period).await;
    assert_eq!(st, StatusCode::OK, "{after}");
    assert_eq!(after["period"]["state"], "closed");
    assert_eq!(
        after["ready"], false,
        "a closed period is not awaiting a close"
    );
    assert_eq!(after["canClose"], false);
    assert_eq!(blockers(&after), []);
    assert_eq!(after["bankReviews"][0]["reviewed"], true);
}

/// **Witness: an operating source that accounting has not posted is reported
/// as outstanding, and stays outstanding while its journal is only a draft.**
#[tokio::test]
async fn close_readiness_counts_unposted_operating_sources() {
    let s = invoice_sources::SourceFixture::new().await;
    s.enable().await;
    let invoice = s.invoice("FUTURE").await;
    let issue = s.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Issue frozen operating amount"})).await;
    blocked(
        &s.app,
        &s.alice,
        &s.period,
        &[("unposted_invoice_sources", 1)],
    )
    .await;
    let (st, drafted) = send(&s.app, &s.alice, &s.source_draft(&issue, "ISSUE")).await;
    assert_eq!(st, StatusCode::OK, "{drafted}");
    blocked(
        &s.app,
        &s.alice,
        &s.period,
        &[("draft_journals", 1), ("unposted_invoice_sources", 1)],
    )
    .await;
    let (st, posted) = send(&s.app, &s.bob, &action("journal.post", &drafted["receipt"])).await;
    assert_eq!(st, StatusCode::OK, "{posted}");
    let (st, ready) = readiness(&s.app, &s.alice, &s.period).await;
    assert_eq!(st, StatusCode::OK, "{ready}");
    assert_eq!(blockers(&ready), []);
    assert_eq!(ready["ready"], true, "{ready}");
}
