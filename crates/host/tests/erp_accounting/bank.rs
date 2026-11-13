use super::*;

pub(super) fn account_ref(a: &Value) -> Value {
    json!({"accountId":a["resourceId"],"expectedAccountRevision":a["revision"]})
}
pub(super) async fn bank_account(app: &Router, a: &Value, book: &Value, account: &Value) -> Value {
    ok(app,a,json!({"type":"bank_account.create","bookId":book["bookId"],"code":"MANUAL-BANK","name":"Synthetic bank evidence account","account":account_ref(account),"reason":"Explicit manual profile, not an external bank connection"})).await
}
pub(super) fn statement(bank: &Value, account: &Value) -> Value {
    input(
        json!({"type":"bank_statement.import","bankAccountId":bank["resourceId"],"account":account_ref(account),"statement":{
        "reference":"SYNTHETIC-NOV-FIRST","fromDate":"2026-11-01","toDate":"2026-11-15","currency":"USD","openingMinor":"0","closingMinor":"10000",
        "rows":[{"externalRef":"receipt-A","date":"2026-11-07","amountMinor":"15000","description":"Synthetic deposit"},{"externalRef":"payment-A","date":"2026-11-08","amountMinor":"-3000","description":"Synthetic withdrawal"},{"externalRef":"payment-B","date":"2026-11-09","amountMinor":"-2000","description":"Synthetic withdrawal"}]},
        "reason":"Import all owned synthetic rows and verify the statement control total"}),
    )
}
pub(super) async fn bank_workspace(
    app: &Router,
    session: &Value,
    c: &Value,
    collection: &str,
) -> (StatusCode, Value) {
    call(
        app,
        "GET",
        &format!(
            "/erp/accounting/workspace?tenant=acme&companyId={}&collection={collection}",
            c["companyId"].as_str().unwrap()
        ),
        Value::Null,
        Some(session),
    )
    .await
}

#[tokio::test]
async fn bank_import_is_atomic_exact_evidence_with_replay_and_separate_read_permission() {
    let (f, app, a, b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    period(&app, &a, &book).await;
    let cash = account(&app, &a, &book, "BANK-CASH", "asset", "cash").await;
    let clearing = account(&app, &a, &book, "CLEARING", "asset", "none").await;
    let bad = input(
        json!({"type":"bank_account.create","bookId":book["bookId"],"code":"BAD","name":"Cannot use clearing as bank cash","account":account_ref(&clearing),"reason":"Must retain separate clearing classification"}),
    );
    assert_eq!(send(&app, &a, &bad).await.0, StatusCode::CONFLICT);
    let bank = bank_account(&app, &a, &book, &cash).await;
    let request = statement(&bank, &cash);
    let (st, imported) = send(&app, &a, &request).await;
    assert_eq!(st, StatusCode::OK, "{imported}");
    assert_eq!(
        send(&app, &a, &request).await.1["receipt"]["replayed"],
        true
    );
    assert_eq!(status(&app, &a, &request).await.1["decision"], "committed");
    assert_eq!(status(&app, &b, &request).await.0, StatusCode::FORBIDDEN);
    let mut changed_retry = request.clone();
    changed_retry["command"]["reason"] = json!("Changed request contents must not reuse its key");
    assert_eq!(send(&app, &a, &changed_retry).await.0, StatusCode::CONFLICT);
    let (st, view) = bank_workspace(&app, &a, &c, "bank_statements").await;
    assert_eq!(st, StatusCode::OK, "{view}");
    assert_eq!(view["bank_statements"][0]["closingMinor"], "10000");
    assert_eq!(view["bank_statements"][0]["evidenceOnly"], true);
    let rows = bank_workspace(&app, &a, &c, "bank_rows").await.1;
    assert_eq!(rows["bank_rows"].as_array().unwrap().len(), 3);
    assert!(rows["bank_rows"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["amountMinor"] == "-3000"));
    let journals: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_accounting.journals")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(
        journals, 0,
        "Importing evidence must never create a payment or journal"
    );
    let reader = login(&app, "reader").await;
    let no_bank = snapshot(&app, &reader, &c).await.1;
    assert!(no_bank.get("bank_statements").is_none());
    assert!(no_bank.get("bank_rows").is_none());
    assert_eq!(
        bank_workspace(&app, &reader, &c, "bank_rows").await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=array_remove(permissions,'bank.read') WHERE tenant_id='acme' AND principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    assert_eq!(status(&app, &a, &request).await.0, StatusCode::FORBIDDEN);
    assert_eq!(send(&app, &a, &request).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn bank_import_refuses_bad_totals_currency_overlaps_duplicates_and_late_row_edits() {
    let (f, app, a, _b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    let period = period(&app, &a, &book).await;
    let cash = account(&app, &a, &book, "BANK", "asset", "cash").await;
    let bank = bank_account(&app, &a, &book, &cash).await;
    let request = statement(&bank, &cash);
    for (field, value) in [
        ("closingMinor", json!("9999")),
        ("openingMinor", json!("-0")),
    ] {
        let mut bad = request.clone();
        bad["requestId"] = json!(Uuid::new_v4());
        bad["command"]["statement"][field] = value;
        assert_eq!(send(&app, &a, &bad).await.0, StatusCode::BAD_REQUEST);
        assert_eq!(status(&app, &a, &bad).await.1["decision"], "not_committed");
    }
    let mut bad = request.clone();
    bad["requestId"] = json!(Uuid::new_v4());
    bad["command"]["statement"]["currency"] = json!("EUR");
    assert_eq!(send(&app, &a, &bad).await.0, StatusCode::CONFLICT);
    let (st, imported) = send(&app, &a, &request).await;
    assert_eq!(st, StatusCode::OK, "{imported}");
    let mut overlap = request.clone();
    overlap["requestId"] = json!(Uuid::new_v4());
    overlap["command"]["statement"]["reference"] = json!("OVERLAP");
    assert_eq!(send(&app, &a, &overlap).await.0, StatusCode::CONFLICT);
    let second = input(
        json!({"type":"bank_statement.import","bankAccountId":bank["resourceId"],"account":account_ref(&cash),"statement":{"reference":"SECOND","fromDate":"2026-11-16","toDate":"2026-11-30","currency":"USD","openingMinor":"10000","closingMinor":"11000","rows":[{"externalRef":"receipt-A","date":"2026-11-20","amountMinor":"1000","description":"Previously imported external reference"}]},"reason":"Duplicates across separate statements must roll back"}),
    );
    assert_eq!(send(&app, &a, &second).await.0, StatusCode::CONFLICT);
    assert_eq!(
        status(&app, &a, &second).await.1["decision"],
        "not_committed"
    );
    let count:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM archava_accounting.bank_statements),(SELECT count(*) FROM archava_accounting.bank_rows)").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(count, (1, 3));
    let late=sqlx::query("INSERT INTO archava_accounting.bank_rows(tenant_id,id,book_id,bank_account_id,statement_id,ordinal,external_ref,entry_date,amount_minor,description) VALUES('acme',$1,$2,$3,$4,4,'LATE','2026-11-10',1,'Cannot amend an audited import')")
        .bind(Uuid::new_v4()).bind(Uuid::parse_str(book["bookId"].as_str().unwrap()).unwrap()).bind(Uuid::parse_str(bank["resourceId"].as_str().unwrap()).unwrap()).bind(Uuid::parse_str(imported["receipt"]["resourceId"].as_str().unwrap()).unwrap()).execute(&f.admin_pool).await;
    assert_eq!(
        late.unwrap_err()
            .as_database_error()
            .unwrap()
            .code()
            .as_deref(),
        Some("23514")
    );
    let edited = sqlx::query(
        "UPDATE archava_accounting.bank_rows SET amount_minor=1 WHERE tenant_id='acme'",
    )
    .execute(&f.admin_pool)
    .await;
    assert_eq!(
        edited
            .unwrap_err()
            .as_database_error()
            .unwrap()
            .code()
            .as_deref(),
        Some("23514")
    );
    // Imported unmatched evidence blocks close; no incomplete close is confirmed.
    assert_eq!(
        send(&app, &a, &action("period.close", &period)).await.0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn concurrent_bank_imports_recover_one_original_receipt_without_duplicate_rows() {
    let (f, app, a, _b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    period(&app, &a, &book).await;
    let cash = account(&app, &a, &book, "BANK", "asset", "cash").await;
    let bank = bank_account(&app, &a, &book, &cash).await;
    let request = statement(&bank, &cash);
    let (left, right) = tokio::join!(send(&app, &a, &request), send(&app, &a, &request));
    assert_eq!(left.0, StatusCode::OK, "{left:?}");
    assert_eq!(right.0, StatusCode::OK, "{right:?}");
    assert_eq!(
        left.1["receipt"]["resourceId"],
        right.1["receipt"]["resourceId"]
    );
    assert_ne!(
        left.1["receipt"]["replayed"],
        right.1["receipt"]["replayed"]
    );
    let count:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM archava_accounting.bank_statements),(SELECT count(*) FROM archava_accounting.bank_rows)").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(count, (1, 3));
}

pub(super) fn match_row(
    row: &Value,
    journal: &Value,
    ordinal: i32,
    account: &Value,
    amount: &str,
) -> Value {
    input(
        json!({"type":"bank_allocation.record","bankRowId":row["id"],"journalId":journal["resourceId"],"journalLineOrdinal":ordinal,"expectedJournalRevision":journal["revision"],"account":account_ref(account),"amountMinor":amount,"reason":"Review exact synthetic statement cash evidence against a posted ledger leg"}),
    )
}

#[tokio::test]
async fn bank_matches_require_posted_cash_allow_partial_unmatch_and_preserve_history() {
    let (f, app, a, b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    let period = period(&app, &a, &book).await;
    let cash = account(&app, &a, &book, "BANK", "asset", "cash").await;
    let equity = account(&app, &a, &book, "EQUITY", "equity", "financing").await;
    let bank = bank_account(&app, &a, &book, &cash).await;
    let (st, r) = send(&app, &a, &statement(&bank, &cash)).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let rows = bank_workspace(&app, &a, &c, "bank_rows").await.1;
    let select = |reference: &str| {
        rows["bank_rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["externalRef"] == reference)
            .unwrap()
            .clone()
    };
    let deposit = select("receipt-A");
    let withdrawal = select("payment-A");
    let last = select("payment-B");
    let (st, draft) = send(
        &app,
        &a,
        &draft(
            &book,
            "DEPOSIT",
            vec![line(&cash, "15000", "0"), line(&equity, "0", "15000")],
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{draft}");
    assert_eq!(
        send(
            &app,
            &a,
            &match_row(&deposit, &draft["receipt"], 1, &cash, "15000")
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(&app, &a, &action("journal.post", &draft["receipt"]))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (st, posted) = send(&app, &b, &action("journal.post", &draft["receipt"])).await;
    assert_eq!(st, StatusCode::OK, "{posted}");
    let posted = posted["receipt"].clone();
    assert_eq!(
        send(&app, &a, &match_row(&deposit, &posted, 1, &cash, "15001"))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let part = match_row(&deposit, &posted, 1, &cash, "10000");
    let (st, matched) = send(&app, &a, &part).await;
    assert_eq!(st, StatusCode::OK, "{matched}");
    assert_eq!(send(&app, &a, &part).await.1["receipt"]["replayed"], true);
    let part_rows = bank_workspace(&app, &a, &c, "bank_rows").await.1;
    assert_eq!(
        part_rows["bank_rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == deposit["id"])
            .unwrap()["remainingMinor"],
        "5000"
    );
    let unmatch = action("bank_allocation.reverse", &matched["receipt"]);
    let (st, undo) = send(&app, &b, &unmatch).await;
    assert_eq!(
        undo["receipt"]["originalBankAllocationId"],
        matched["receipt"]["resourceId"]
    );
    assert_eq!(
        status(&app, &b, &unmatch).await.1["receipt"]["originalBankAllocationId"],
        matched["receipt"]["resourceId"]
    );
    assert_eq!(st, StatusCode::OK, "{undo}");
    assert_eq!(
        send(&app, &b, &unmatch).await.1["receipt"]["replayed"],
        true
    );
    assert_eq!(
        send(
            &app,
            &b,
            &action("bank_allocation.reverse", &matched["receipt"])
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (st, final_match) = send(&app, &a, &match_row(&deposit, &posted, 1, &cash, "15000")).await;
    assert_eq!(st, StatusCode::OK, "{final_match}");
    let withdrawal_journal = post(
        &app,
        &a,
        &b,
        &book,
        "WITHDRAWAL",
        vec![line(&equity, "3000", "0"), line(&cash, "0", "3000")],
    )
    .await;
    assert_eq!(
        send(&app, &a, &match_row(&withdrawal, &posted, 1, &cash, "1"))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let (st, r) = send(
        &app,
        &a,
        &match_row(&withdrawal, &withdrawal_journal, 2, &cash, "3000"),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let last_journal = post(
        &app,
        &a,
        &b,
        &book,
        "LAST",
        vec![line(&equity, "2000", "0"), line(&cash, "0", "2000")],
    )
    .await;
    assert_eq!(
        send(&app, &a, &action("period.close", &period)).await.0,
        StatusCode::CONFLICT
    );
    let (st, r) = send(&app, &a, &match_row(&last, &last_journal, 2, &cash, "2000")).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (st, held) = send(&app, &a, &action("period.close", &period)).await;
    assert_eq!(st, StatusCode::CONFLICT, "{held}");
    let totals:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM archava_accounting.journals),(SELECT count(*) FROM archava_accounting.bank_allocations),(SELECT count(*) FROM archava_accounting.bank_unmatches)").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(
        totals,
        (3, 4, 1),
        "Matching adds evidence only and retains every correction"
    );
    let bal = report(&app, &a, &book).await;
    assert_eq!(
        bal["trialBalance"]["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == cash["resourceId"])
            .unwrap()["closingNetMinor"],
        "10000"
    );
}

#[tokio::test]
async fn bank_matching_serializes_racing_allocations_and_refuses_duplicate_cash_use() {
    let (f, app, a, b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    period(&app, &a, &book).await;
    let cash = account(&app, &a, &book, "BANK", "asset", "cash").await;
    let equity = account(&app, &a, &book, "EQUITY", "equity", "financing").await;
    let bank = bank_account(&app, &a, &book, &cash).await;
    let (st, r) = send(&app, &a, &statement(&bank, &cash)).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let rows = bank_workspace(&app, &a, &c, "bank_rows").await.1;
    let deposit = rows["bank_rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["externalRef"] == "receipt-A")
        .unwrap();
    let journal = post(
        &app,
        &a,
        &b,
        &book,
        "DEPOSIT",
        vec![line(&cash, "15000", "0"), line(&equity, "0", "15000")],
    )
    .await;
    let left = match_row(deposit, &journal, 1, &cash, "10000");
    let right = match_row(deposit, &journal, 1, &cash, "10000");
    let (l, r) = tokio::join!(send(&app, &a, &left), send(&app, &b, &right));
    assert!(
        matches!(
            (l.0, r.0),
            (StatusCode::OK, StatusCode::CONFLICT) | (StatusCode::CONFLICT, StatusCode::OK)
        ),
        "{l:?} {r:?}"
    );
    let totals: (i64, i64) = sqlx::query_as(
        "SELECT count(*),sum(amount_minor)::bigint FROM archava_accounting.bank_allocations",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(totals, (1, 10000));
    assert_eq!(
        bank_workspace(&app, &a, &c, "bank_rows").await.1["bank_rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == deposit["id"])
            .unwrap()["remainingMinor"],
        "5000"
    );
}
