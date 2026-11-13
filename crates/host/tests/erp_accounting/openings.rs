//! Declared opening balances: a reviewed lump on a policy-named control
//! account, explained by the control comparison instead of being a difference.
use super::*;
use invoice_sources::SourceFixture;

const OPENED: &str = "2026-11-01";
const LATER: &str = "2100-12-31";

fn account_ref(account: &Value) -> Value {
    json!({"accountId":account["resourceId"],"expectedAccountRevision":account["revision"]})
}
pub(super) fn opening(
    book: &Value,
    control: &str,
    code: &str,
    account: &Value,
    offset: &Value,
    amount: &str,
) -> Value {
    input(
        json!({"type":"opening_balance.draft","bookId":book["bookId"],"control":control,"code":code,"date":OPENED,
        "controlAccount":account_ref(account),"offsetAccount":account_ref(offset),"amountMinor":amount,
        "reason":"Reviewed opening balance from the prior books"}),
    )
}
/// The control row of `account` in a subledger or inventory report.
pub(super) fn control_of<'a>(report: &'a Value, account: &Value) -> &'a Value {
    report["controls"]
        .as_array()
        .expect("the report lists its control accounts")
        .iter()
        .find(|row| row["accountId"] == account["resourceId"])
        .expect("the account is a listed control account")
}
fn amounts(row: &Value) -> [&str; 4] {
    [
        "expectedMinor",
        "openingMinor",
        "actualMinor",
        "differenceMinor",
    ]
    .map(|key| row[key].as_str().unwrap())
}
async fn journals(s: &SourceFixture) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM archava_accounting.journals")
        .fetch_one(&s.f.admin_pool)
        .await
        .unwrap()
}

/// **Witness: a declared opening receivable is an explained part of the
/// control balance once it is independently posted, a generic journal on the
/// same account is still a difference, and reversing the opening removes it.**
#[tokio::test]
async fn a_declared_opening_receivable_is_explained_by_the_control_comparison() {
    let s = SourceFixture::new().await;
    s.enable().await;
    let equity = account(
        &s.app,
        &s.alice,
        &s.book,
        "OPEN-EQUITY",
        "equity",
        "financing",
    )
    .await;
    let before = subledger::accepted(&s.app, &s.alice, &s.book, "receivable", LATER).await;
    assert_eq!(amounts(control_of(&before, &s.ar)), ["0", "0", "0", "0"]);

    // The declaration is a draft journal: nothing is recognized yet.
    let request = opening(&s.book, "receivable", "OPEN-AR", &s.ar, &equity, "250000");
    let (st, drafted) = send(&s.app, &s.alice, &request).await;
    assert_eq!(st, StatusCode::OK, "{drafted}");
    let journal = drafted["receipt"].clone();
    assert_eq!(journal["resourceKind"], "journal");
    assert_eq!(journal["revision"], 1);
    let (st, replay) = send(&s.app, &s.alice, &request).await;
    assert_eq!(st, StatusCode::OK, "{replay}");
    assert_eq!(replay["receipt"]["resourceId"], journal["resourceId"]);
    let stored: (String, String, i64, String, String) = sqlx::query_as("SELECT o.control,j.source_ref,o.amount_minor,j.state,o.created_by FROM archava_accounting.opening_balances o JOIN archava_accounting.journals j ON j.tenant_id=o.tenant_id AND j.id=o.id")
        .fetch_one(&s.f.admin_pool)
        .await
        .unwrap();
    assert_eq!(
        stored,
        (
            "receivable".into(),
            "opening:receivable:OPEN-AR".into(),
            250_000,
            "draft".into(),
            "alice".into()
        )
    );
    let draft_only = subledger::accepted(&s.app, &s.alice, &s.book, "receivable", LATER).await;
    assert_eq!(
        amounts(control_of(&draft_only, &s.ar)),
        ["0", "0", "0", "0"]
    );

    // Posting stays independent: the declarer cannot post their own opening.
    assert_eq!(
        send(&s.app, &s.alice, &action("journal.post", &journal))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (st, posted) = send(&s.app, &s.bob, &action("journal.post", &journal)).await;
    assert_eq!(st, StatusCode::OK, "{posted}");
    let opened = subledger::accepted(&s.app, &s.alice, &s.book, "receivable", LATER).await;
    assert_eq!(
        amounts(control_of(&opened, &s.ar)),
        ["250000", "250000", "250000", "0"]
    );
    assert_eq!(opened["postedSourceControlsAgree"], true, "{opened}");
    assert_eq!(opened["positionControlDifferenceMinor"], "0");
    assert_eq!(opened["declaredOpeningMinor"], "250000");
    // No document stands behind the lump, so nothing is aged.
    assert_eq!(opened["totals"]["netOutstandingMinor"], "0");
    assert_eq!(opened["positions"], json!([]));
    // The cutoff applies to the opening's book date.
    let earlier = subledger::accepted(&s.app, &s.alice, &s.book, "receivable", "2026-10-31").await;
    assert_eq!(amounts(control_of(&earlier, &s.ar)), ["0", "0", "0", "0"]);

    // A generic journal on the control account is still unexplained.
    post(
        &s.app,
        &s.alice,
        &s.bob,
        &s.book,
        "MANUAL-AR",
        vec![line(&s.ar, "100", "0"), line(&equity, "0", "100")],
    )
    .await;
    let adjusted = subledger::accepted(&s.app, &s.alice, &s.book, "receivable", LATER).await;
    assert_eq!(
        amounts(control_of(&adjusted, &s.ar)),
        ["250000", "250000", "250100", "100"]
    );
    assert_eq!(adjusted["postedSourceControlsAgree"], false);

    // Reversing the posted opening takes it out of the expected balance.
    let reverse = input(
        json!({"type":"journal.reverse","id":journal["resourceId"],"expectedRevision":2,"code":"OPEN-AR-REV","date":"2026-11-02","sourceRef":"Withdraw the declared opening receivable","reason":"The prior books were restated"}),
    );
    let (st, reversal) = send(&s.app, &s.alice, &reverse).await;
    assert_eq!(st, StatusCode::OK, "{reversal}");
    let (st, v) = send(
        &s.app,
        &s.bob,
        &action("journal.post", &reversal["receipt"]),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let withdrawn = subledger::accepted(&s.app, &s.alice, &s.book, "receivable", LATER).await;
    assert_eq!(
        amounts(control_of(&withdrawn, &s.ar)),
        ["0", "0", "100", "100"]
    );
    assert_eq!(withdrawn["declaredOpeningMinor"], "0");
    // Before the reversal's date the opening still stood.
    let between = subledger::accepted(&s.app, &s.alice, &s.book, "receivable", OPENED).await;
    assert_eq!(
        amounts(control_of(&between, &s.ar)),
        ["250000", "250000", "250000", "0"]
    );
}

/// **Witness: an opening cannot be declared on an arbitrary account, against a
/// non-equity account, by a generic journal, or without authority, and a
/// stored declaration cannot be changed or read across tenants.**
#[tokio::test]
async fn opening_declarations_refuse_wrong_accounts_forged_references_and_edits() {
    let s = SourceFixture::new().await;
    s.enable().await;
    let equity = account(
        &s.app,
        &s.alice,
        &s.book,
        "OPEN-EQUITY",
        "equity",
        "financing",
    )
    .await;
    let start = journals(&s).await;
    for (why, body) in [
        (
            "an account that is not the policy's receivable account",
            opening(
                &s.book,
                "receivable",
                "OPEN-BAD-1",
                &s.clearing,
                &equity,
                "1000",
            ),
        ),
        (
            "a non-equity offset account",
            opening(
                &s.book,
                "receivable",
                "OPEN-BAD-2",
                &s.ar,
                &s.revenue,
                "1000",
            ),
        ),
        (
            "a payable opening when the book declares no payable account",
            opening(&s.book, "payable", "OPEN-BAD-3", &s.ar, &equity, "1000"),
        ),
        (
            "a generic journal that takes the reserved reference",
            input(
                json!({"type":"journal.draft","bookId":s.book["bookId"],"code":"OPEN-FAKE","date":OPENED,"sourceRef":"opening:receivable:OPEN-FAKE","memo":"Not a declaration","lines":[line(&s.ar,"1000","0"),line(&equity,"0","1000")],"reason":"A generic journal must not pose as an opening"}),
            ),
        ),
    ] {
        let (st, v) = send(&s.app, &s.alice, &body).await;
        assert_eq!(st, StatusCode::CONFLICT, "{why}: {v}");
        assert_eq!(journals(&s).await, start, "{why} must write nothing");
    }
    for (field, value) in [
        ("amountMinor", json!("0")),
        ("amountMinor", json!("-5")),
        ("control", json!("cash")),
    ] {
        let mut body = opening(&s.book, "receivable", "OPEN-BAD-4", &s.ar, &equity, "1000");
        body["command"][field] = value;
        let (st, v) = send(&s.app, &s.alice, &body).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{field}: {v}");
    }
    let reader = login(&s.app, "reader").await;
    let (st, v) = send(
        &s.app,
        &reader,
        &opening(&s.book, "receivable", "OPEN-READER", &s.ar, &equity, "1000"),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(journals(&s).await, start);

    // A stored declaration is immutable, even for the database owner role.
    let (st, drafted) = send(
        &s.app,
        &s.alice,
        &opening(&s.book, "receivable", "OPEN-AR", &s.ar, &equity, "1000"),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{drafted}");
    for statement in [
        "UPDATE archava_accounting.opening_balances SET amount_minor=1",
        "DELETE FROM archava_accounting.opening_balances",
    ] {
        assert!(sqlx::query(statement)
            .execute(&s.f.admin_pool)
            .await
            .is_err());
    }
    // The serving role sees it only inside its own tenant.
    for (tenant, visible) in [("acme", 1_i64), ("another-tenant", 0)] {
        let mut tx = s.f.runtime_pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('archava.tenant_id',$1,true)")
            .bind(tenant)
            .execute(&mut *tx)
            .await
            .unwrap();
        let seen: i64 =
            sqlx::query_scalar("SELECT count(*) FROM archava_accounting.opening_balances")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(seen, visible, "{tenant}");
    }
    let complete: bool = sqlx::query_scalar("SELECT archava_accounting.opening_guards_complete()")
        .fetch_one(&s.f.runtime_pool)
        .await
        .unwrap();
    assert!(complete);
    // A draft opening can be voided like any draft journal.
    let (st, v) = send(
        &s.app,
        &s.alice,
        &action("journal.void", &drafted["receipt"]),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}
