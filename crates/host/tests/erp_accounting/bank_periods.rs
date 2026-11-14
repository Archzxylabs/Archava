use super::bank::{account_ref, bank_account, match_row, statement};
use super::*;

pub(super) async fn policy(app: &Router, actor: &Value, bank: &Value, independent: bool) -> Value {
    ok(app,actor,json!({"type":"bank_policy.set","bankAccountId":bank["resourceId"],"expectedRevision":0,"fromDate":"2026-11-01","independentReview":independent,"reason":"Explicit synthetic opening basis and bank reviewer choice"})).await
}
pub(super) async fn check(
    app: &Router,
    actor: &Value,
    policy: &Value,
    period: &Value,
) -> (StatusCode, Value) {
    call(
        app,
        "GET",
        &format!(
            "/erp/accounting/bank-reconciliation?tenant=acme&bankPolicyId={}&periodId={}",
            policy["resourceId"].as_str().unwrap(),
            period["resourceId"].as_str().unwrap()
        ),
        Value::Null,
        Some(actor),
    )
    .await
}
pub(super) fn confirm(value: &Value) -> Value {
    input(
        json!({"type":"bank_reconciliation.confirm","bankPolicyId":value["bankPolicyId"],"expectedPolicyRevision":value["policyRevision"],"periodId":value["periodId"],"expectedPeriodRevision":value["periodRevision"],"expectedCheckHash":value["check"]["evidenceSha256"],"reason":"Review complete current synthetic statement and posted cash controls"}),
    )
}
pub(super) fn empty_statement(
    bank: &Value,
    cash: &Value,
    reference: &str,
    start: &str,
    end: &str,
    balance: &str,
) -> Value {
    input(
        json!({"type":"bank_statement.import","bankAccountId":bank["resourceId"],"account":account_ref(cash),"statement":{"reference":reference,"fromDate":start,"toDate":end,"currency":"USD","openingMinor":balance,"closingMinor":balance,"rows":[]},"reason":"Complete explicit empty statement interval"}),
    )
}

#[tokio::test]
async fn full_bank_period_requires_coverage_and_independent_current_attestation_before_close() {
    let (f, app, a, b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    let period = period(&app, &a, &book).await;
    let cash = account(&app, &a, &book, "BANK", "asset", "cash").await;
    let equity = account(&app, &a, &book, "EQUITY", "equity", "financing").await;
    let bank = bank_account(&app, &a, &book, &cash).await;
    let policy = policy(&app, &a, &bank, true).await;
    let (st, r) = send(&app, &a, &statement(&bank, &cash)).await;
    assert_eq!(st, StatusCode::OK, "{r}");
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
    let (st, half) = check(&app, &b, &policy, &period).await;
    assert_eq!(st, StatusCode::OK, "{half}");
    assert_eq!(half["check"]["ready"], false);
    assert_eq!(half["check"]["checks"]["completeStatementCoverage"], false);
    assert_eq!(half["check"]["unmatchedCashLegs"], 0);
    assert_eq!(half["check"]["closingLedgerMinor"], "10000");
    assert_eq!(
        send(&app, &a, &action("period.close", &period)).await.0,
        StatusCode::CONFLICT
    );
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
    let current = check(&app, &b, &policy, &period).await.1;
    assert_eq!(current["check"]["ready"], true, "{current}");
    assert_eq!(current["check"]["coveredDays"], 30);
    assert_eq!(current["check"]["statementCount"], 2);
    assert_eq!(current["canConfirm"], true);
    assert_eq!(
        check(&app, &a, &policy, &period).await.1["canConfirm"],
        false
    );
    let request = confirm(&current);
    assert_eq!(send(&app, &a, &request).await.0, StatusCode::FORBIDDEN);
    let (st, proof) = send(&app, &b, &request).await;
    assert_eq!(st, StatusCode::OK, "{proof}");
    assert_eq!(
        proof["receipt"]["bankCheckHash"],
        current["check"]["evidenceSha256"]
    );
    assert_eq!(
        send(&app, &b, &request).await.1["receipt"]["replayed"],
        true
    );
    let (st, closed) = send(&app, &a, &action("period.close", &period)).await;
    assert_eq!(st, StatusCode::OK, "{closed}");
    assert_eq!(check(&app, &b, &policy, &period).await.1["reviewed"], true);
    let matching = snapshot(&app, &a, &c).await.1["bank_allocations"][0].clone();
    assert_eq!(send(&app,&a,&input(json!({"type":"bank_allocation.reverse","id":matching["id"],"expectedRevision":1,"reason":"Closed bank evidence must hold"}))).await.0,StatusCode::CONFLICT);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_accounting.bank_reconciliations")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    let reader = login(&app, "reader").await;
    assert_eq!(
        check(&app, &reader, &policy, &period).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn empty_statement_date_gaps_and_unclaimed_database_attestations_never_close_a_period() {
    let (f, app, a, _b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    let period = period(&app, &a, &book).await;
    let cash = account(&app, &a, &book, "BANK", "asset", "cash").await;
    let bank = bank_account(&app, &a, &book, &cash).await;
    let policy = policy(&app, &a, &bank, false).await;
    for (reference, start, end) in [
        ("FIRST", "2026-11-01", "2026-11-15"),
        ("LAST", "2026-11-17", "2026-11-30"),
    ] {
        let (st, r) = send(
            &app,
            &a,
            &empty_statement(&bank, &cash, reference, start, end, "0"),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{r}");
    }
    let gap = check(&app, &a, &policy, &period).await.1;
    assert_eq!(gap["check"]["coveredDays"], 29);
    assert_eq!(gap["check"]["ready"], false);
    assert_eq!(send(&app, &a, &confirm(&gap)).await.0, StatusCode::CONFLICT);
    assert_eq!(
        send(&app, &a, &action("period.close", &period)).await.0,
        StatusCode::CONFLICT
    );
    let (st, r) = send(
        &app,
        &a,
        &empty_statement(&bank, &cash, "MISSING-DAY", "2026-11-16", "2026-11-16", "0"),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let current = check(&app, &a, &policy, &period).await.1;
    assert_eq!(current["check"]["ready"], true, "{current}");
    let forged=sqlx::query("INSERT INTO archava_accounting.bank_reconciliations(tenant_id,id,book_id,bank_policy_id,period_id,policy_revision,reviewed_period_revision,check_snapshot,created_by) VALUES('acme',$1,$2,$3,$4,1,1,$5,'alice')")
        .bind(Uuid::new_v4()).bind(Uuid::parse_str(book["bookId"].as_str().unwrap()).unwrap()).bind(Uuid::parse_str(policy["resourceId"].as_str().unwrap()).unwrap()).bind(Uuid::parse_str(period["resourceId"].as_str().unwrap()).unwrap()).bind(&current["check"]).execute(&f.admin_pool).await;
    assert_eq!(
        forged
            .unwrap_err()
            .as_database_error()
            .unwrap()
            .code()
            .as_deref(),
        Some("23514")
    );
    assert_eq!(
        send(&app, &a, &action("period.close", &period)).await.0,
        StatusCode::CONFLICT
    );
    let (st, r) = send(&app, &a, &confirm(&current)).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (st, r) = send(&app, &a, &action("period.close", &period)).await;
    assert_eq!(st, StatusCode::OK, "{r}");
}

#[tokio::test]
async fn changed_cash_evidence_requires_a_new_review_and_backdating_cannot_break_a_closed_bank_period(
) {
    let (f, app, a, b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    ok(&app,&a,json!({"type":"period.open","bookId":book["bookId"],"code":"OCT","startDate":"2026-10-01","endDate":"2026-10-31","reason":"Explicit earlier opening period"})).await;
    let period = period(&app, &a, &book).await;
    let cash = account(&app, &a, &book, "BANK", "asset", "cash").await;
    let equity = account(&app, &a, &book, "EQUITY", "equity", "financing").await;
    let bank = bank_account(&app, &a, &book, &cash).await;
    let policy = policy(&app, &a, &bank, true).await;
    let (st, r) = send(
        &app,
        &a,
        &empty_statement(&bank, &cash, "FULL-ZERO", "2026-11-01", "2026-11-30", "0"),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let original = check(&app, &b, &policy, &period).await.1;
    let original_request = confirm(&original);
    let (st, r) = send(&app, &b, &original_request).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let plus = post(
        &app,
        &a,
        &b,
        &book,
        "OFFSET-IN",
        vec![line(&cash, "100", "0"), line(&equity, "0", "100")],
    )
    .await;
    let minus = post(
        &app,
        &a,
        &b,
        &book,
        "OFFSET-OUT",
        vec![line(&equity, "100", "0"), line(&cash, "0", "100")],
    )
    .await;
    let offset = check(&app, &b, &policy, &period).await.1;
    assert_eq!(offset["check"]["closingLedgerMinor"], "0");
    assert_eq!(offset["check"]["unmatchedCashLegs"], 2);
    assert_eq!(offset["check"]["ready"], false);
    assert_eq!(
        send(&app, &a, &action("period.close", &period)).await.0,
        StatusCode::CONFLICT
    );
    for (journal, code) in [(&plus, "REVERSE-IN"), (&minus, "REVERSE-OUT")] {
        let request = input(
            json!({"type":"journal.reverse","id":journal["resourceId"],"expectedRevision":journal["revision"],"code":code,"date":"2026-11-09","sourceRef":format!("Reviewed ledger-only correction {code}"),"reason":"Explicit full reversal without inventing a bank transaction"}),
        );
        let (st, draft) = send(&app, &a, &request).await;
        assert_eq!(st, StatusCode::OK, "{draft}");
        let (st, r) = send(&app, &b, &action("journal.post", &draft["receipt"])).await;
        assert_eq!(st, StatusCode::OK, "{r}");
    }
    let current = check(&app, &b, &policy, &period).await.1;
    assert_eq!(current["check"]["ready"], true, "{current}");
    assert_eq!(current["check"]["unallocatedReversalCashLegs"], 4);
    assert_eq!(current["reviewed"], false);
    assert_ne!(
        current["check"]["evidenceSha256"],
        original["check"]["evidenceSha256"]
    );
    let mut stale = original_request.clone();
    stale["requestId"] = json!(Uuid::new_v4());
    assert_eq!(send(&app, &b, &stale).await.0, StatusCode::CONFLICT);
    assert_eq!(
        send(&app, &b, &original_request).await.1["receipt"]["replayed"],
        true
    );
    assert_eq!(
        send(&app, &a, &action("period.close", &period)).await.0,
        StatusCode::CONFLICT
    );
    let (st, r) = send(&app, &b, &confirm(&current)).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (st, closed) = send(&app, &a, &action("period.close", &period)).await;
    assert_eq!(st, StatusCode::OK, "{closed}");
    let mut historical = draft(
        &book,
        "LATE-OPENING",
        vec![line(&cash, "1", "0"), line(&equity, "0", "1")],
    );
    historical["command"]["date"] = json!("2026-10-31");
    let (st, pending) = send(&app, &a, &historical).await;
    assert_eq!(st, StatusCode::OK, "{pending}");
    assert_eq!(
        send(&app, &b, &action("journal.post", &pending["receipt"]))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(check(&app, &b, &policy, &period).await.1["reviewed"], true);
    let (st, reopened) = send(&app, &a, &action("period.reopen", &closed["receipt"])).await;
    assert_eq!(st, StatusCode::OK, "{reopened}");
    assert_eq!(check(&app, &b, &policy, &period).await.1["reviewed"], false);
    let (st, r) = send(&app, &b, &action("journal.post", &pending["receipt"])).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let changed = check(&app, &b, &policy, &period).await.1;
    assert_eq!(changed["check"]["checks"]["openingAgrees"], false);
    assert_eq!(changed["check"]["ready"], false);
    assert_eq!(
        send(&app, &a, &action("period.close", &reopened["receipt"]))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_accounting.bank_reconciliations")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(count, 2);
}
