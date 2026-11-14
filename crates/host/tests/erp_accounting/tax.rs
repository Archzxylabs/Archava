//! Tax codes (TAX.md stage A): a company's own codes, entered by one person,
//! confirmed by a reviewer, with versions and date ranges. Nothing uses a code
//! yet, so these tests stop at the stored rows.
use super::*;

/// A book without independent posting: the person who enters a tax code may
/// confirm it. The default synthetic book (`book`) requires a second person.
async fn single_person_book(app: &Router, a: &Value, c: &Value) -> Value {
    let mut request = create(c, "USD");
    request["command"]["independentPosting"] = json!(false);
    let (st, v) = send(app, a, &request).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["receipt"].clone()
}
fn fraction(numerator: i64, denominator: i64) -> Value {
    json!({"numerator":numerator,"denominator":denominator})
}
fn tax_code(
    book: &Value,
    code: &str,
    kind: &str,
    rate: Value,
    account: &Value,
    from: &str,
) -> Value {
    input(
        json!({"type":"tax_code.create","bookId":book["bookId"],"code":code,"kind":kind,"rate":rate,
        "controlAccount":{"accountId":account["resourceId"],"expectedAccountRevision":account["revision"]},
        "effectiveFrom":from,"reason":"Entered by the company from its own tax advice"}),
    )
}
fn retire(code: &Value, revision: i64, to: &str) -> Value {
    input(
        json!({"type":"tax_code.retire","id":code["resourceId"],"expectedRevision":revision,"effectiveTo":to,"reason":"The company's rate changes"}),
    )
}
fn withdraw(code: &Value, revision: i64) -> Value {
    input(
        json!({"type":"tax_code.retire","id":code["resourceId"],"expectedRevision":revision,"reason":"Entered by mistake"}),
    )
}
fn confirm(code: &Value, revision: i64) -> Value {
    input(
        json!({"type":"tax_code.confirm","id":code["resourceId"],"expectedRevision":revision,"reason":"Checked against the company's tax advice"}),
    )
}
/// Entered by one person and confirmed by another; the confirmation receipt.
async fn confirmed(app: &Router, creator: &Value, reviewer: &Value, request: &Value) -> Value {
    let (st, entered) = send(app, creator, request).await;
    assert_eq!(st, StatusCode::OK, "{entered}");
    assert_eq!(entered["receipt"]["taxCodeState"], "entered", "{entered}");
    let (st, v) = send(app, reviewer, &confirm(&entered["receipt"], 1)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["receipt"]["taxCodeState"], "confirmed", "{v}");
    v["receipt"].clone()
}
async fn listed(app: &Router, s: &Value, c: &Value, query: &str) -> (StatusCode, Value) {
    call(
        app,
        "GET",
        &format!(
            "/erp/accounting/workspace?tenant=acme&companyId={}{query}",
            c["companyId"].as_str().unwrap()
        ),
        Value::Null,
        Some(s),
    )
    .await
}
fn row<'a>(workspace: &'a Value, id: &Value) -> &'a Value {
    workspace["tax_codes"]
        .as_array()
        .expect("the workspace lists tax codes")
        .iter()
        .find(|row| row["id"] == id["resourceId"])
        .expect("the tax code is listed")
}
async fn stored(f: &Fixture) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM archava_accounting.tax_codes")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap()
}
/// One more human with exactly the named accounting permissions.
async fn member(f: &Fixture, actor: &str, permissions: &[&str]) {
    sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES($1,$2,$3,true,1)")
        .bind(actor).bind(format!("{actor}@archava.test")).bind(HASH).execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_organizations.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme',$1,'human',ARRAY['organizations.read','organizations.write','companies.read','companies.write'],'all')")
        .bind(actor).execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_accounting.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme',$1,'human',$2,'all')")
        .bind(actor).bind(permissions).execute(&f.admin_pool).await.unwrap();
}

/// **Witness: on a book with independent posting a code entered by one person
/// does not apply until another person confirms it; the creator cannot confirm
/// it; a confirmed code is listed by date, retired once and replaced by a new
/// version; and every command replays its original receipt.**
#[tokio::test]
async fn tax_codes_are_entered_confirmed_by_a_second_person_listed_retired_and_versioned() {
    let (f, app, alice, bob, c) = setup().await;
    let book = book(&app, &alice, &c).await;
    let output = account(&app, &alice, &book, "VAT-OUT", "liability", "operating").await;
    let input_account = account(&app, &alice, &book, "VAT-IN", "asset", "operating").await;

    // The readiness function is part of the host's startup check, which the
    // router of `setup` already passed; the serving role can ask it directly.
    let complete: bool = sqlx::query_scalar("SELECT archava_accounting.tax_guards_complete()")
        .fetch_one(&f.runtime_pool)
        .await
        .unwrap();
    assert!(complete);

    let (st, empty) = listed(&app, &alice, &c, "").await;
    assert_eq!(st, StatusCode::OK, "{empty}");
    assert_eq!(empty["tax_codes"], json!([]));
    let notice = empty["taxNotice"].as_str().unwrap();
    assert!(notice.contains("Archava does not verify tax rates or rules"));
    assert!(notice.contains("does not apply until a reviewer confirms it"));

    let request = tax_code(
        &book,
        "VAT",
        "output",
        fraction(11, 100),
        &output,
        "2026-01-01",
    );
    let (st, created) = send(&app, &alice, &request).await;
    assert_eq!(st, StatusCode::OK, "{created}");
    assert_eq!(created["decision"], "receipt");
    let first = created["receipt"].clone();
    assert_eq!(first["type"], "tax_code.create");
    assert_eq!(first["resourceKind"], "tax_code");
    assert_eq!(first["revision"], 1);
    assert_eq!(first["bookId"], book["bookId"]);
    // The receipt says the code is only entered.
    assert_eq!(first["taxCodeState"], "entered");

    // The same request again answers with the original receipt and no new row.
    let (st, replay) = send(&app, &alice, &request).await;
    assert_eq!(st, StatusCode::OK, "{replay}");
    assert_eq!(replay["decision"], "replayed");
    assert_eq!(replay["receipt"]["replayed"], true);
    for key in [
        "requestId",
        "resourceId",
        "bookId",
        "revision",
        "type",
        "taxCodeState",
    ] {
        assert_eq!(replay["receipt"][key], first[key], "{key}");
    }
    assert_eq!(stored(&f).await, 1);
    // The same request id with other content is not a replay.
    let mut altered = request.clone();
    altered["command"]["rate"] = fraction(12, 100);
    assert_eq!(send(&app, &alice, &altered).await.0, StatusCode::CONFLICT);

    // Entered is not confirmed: the code applies on no date.
    let (_, w) = listed(&app, &alice, &c, "&asOf=2026-06-01").await;
    let entered = row(&w, &first);
    assert_eq!(entered["state"], "entered");
    assert_eq!(entered["applies"], false);
    assert_eq!(entered["createdBy"], "alice");
    assert_eq!(entered["reviewedBy"], Value::Null);
    assert_eq!(entered["confirmedAt"], Value::Null);

    // The person who entered it cannot confirm it on this book.
    let (st, v) = send(&app, &alice, &confirm(&first, 1)).await;
    assert_refused_as(st, &v, "tax_code_independent_reviewer_required");
    let (_, w) = listed(&app, &alice, &c, "&asOf=2026-06-01").await;
    assert_eq!(row(&w, &first)["state"], "entered");
    // A stale revision is a plain conflict.
    assert_eq!(
        send(&app, &bob, &confirm(&first, 2)).await.0,
        StatusCode::CONFLICT
    );
    // A second person confirms it, and the confirmation replays.
    let confirmation = confirm(&first, 1);
    let (st, done) = send(&app, &bob, &confirmation).await;
    assert_eq!(st, StatusCode::OK, "{done}");
    assert_eq!(done["decision"], "receipt");
    assert_eq!(done["receipt"]["type"], "tax_code.confirm");
    assert_eq!(done["receipt"]["resourceId"], first["resourceId"]);
    assert_eq!(done["receipt"]["revision"], 2);
    assert_eq!(done["receipt"]["taxCodeState"], "confirmed");
    let (st, replay) = send(&app, &bob, &confirmation).await;
    assert_eq!(st, StatusCode::OK, "{replay}");
    assert_eq!(replay["decision"], "replayed");
    for key in [
        "requestId",
        "resourceId",
        "revision",
        "type",
        "taxCodeState",
    ] {
        assert_eq!(replay["receipt"][key], done["receipt"][key], "{key}");
    }
    let (st, held) = status(&app, &bob, &done["receipt"]).await;
    assert_eq!(st, StatusCode::OK, "{held}");
    assert_eq!(held["decision"], "committed");
    // Another person cannot replay the reviewer's request as their own.
    assert_eq!(
        send(&app, &alice, &confirmation).await.0,
        StatusCode::CONFLICT
    );
    // A confirmed code is not confirmed again, at either revision.
    for revision in [1, 2] {
        let (st, v) = send(&app, &bob, &confirm(&first, revision)).await;
        assert_refused_as(st, &v, "tax_code_not_awaiting_confirmation");
    }

    let mut scaled = tax_code(
        &book,
        "VAT-PURCHASE",
        "input",
        fraction(12, 100),
        &input_account,
        "2026-01-01",
    );
    scaled["command"]["base"] = fraction(11, 12);
    let second = confirmed(&app, &bob, &alice, &scaled).await;
    // A zero rate is a legitimate code.
    let zero = confirmed(
        &app,
        &alice,
        &bob,
        &tax_code(
            &book,
            "ZERO",
            "output",
            fraction(0, 100),
            &output,
            "2026-01-01",
        ),
    )
    .await;

    let (st, w) = listed(&app, &alice, &c, "&asOf=2026-06-01").await;
    assert_eq!(st, StatusCode::OK, "{w}");
    assert_eq!(w["tax_codes"].as_array().unwrap().len(), 3);
    let out = row(&w, &first);
    assert_eq!(out["code"], "VAT");
    assert_eq!(out["version"], 1);
    assert_eq!(out["kind"], "output");
    assert_eq!(out["state"], "confirmed");
    assert_eq!(out["rate"], fraction(11, 100));
    assert_eq!(out["base"], fraction(1, 1));
    assert_eq!(out["controlAccount"]["accountId"], output["resourceId"]);
    assert_eq!(out["controlAccount"]["code"], "VAT-OUT");
    assert_eq!(out["controlAccount"]["class"], "liability");
    assert_eq!(out["effectiveFrom"], "2026-01-01");
    assert_eq!(out["effectiveTo"], Value::Null);
    assert_eq!(out["createdBy"], "alice");
    assert_eq!(out["reviewedBy"], "bob");
    assert!(out["confirmedAt"].as_str().is_some());
    assert_eq!(out["appliesOn"], "2026-06-01");
    assert_eq!(out["applies"], true);
    assert_eq!(out["enteredByCompany"], true);
    assert_eq!(out["verifiedByArchava"], false);
    let purchase = row(&w, &second);
    assert_eq!(purchase["kind"], "input");
    assert_eq!(purchase["rate"], fraction(12, 100));
    assert_eq!(purchase["base"], fraction(11, 12));
    assert_eq!(purchase["controlAccount"]["class"], "asset");
    assert_eq!(purchase["createdBy"], "bob");
    assert_eq!(purchase["reviewedBy"], "alice");
    assert_eq!(row(&w, &zero)["rate"], fraction(0, 100));
    // Before its first day a confirmed code does not apply.
    let (_, before) = listed(&app, &alice, &c, "&asOf=2025-12-31").await;
    assert_eq!(row(&before, &first)["applies"], false);
    // Without a date the answer names the date it used.
    let (_, today) = listed(&app, &alice, &c, "&collection=tax_codes").await;
    assert!(row(&today, &first)["appliesOn"].as_str().is_some());
    assert_eq!(
        listed(&app, &alice, &c, "&asOf=2026-02-30").await.0,
        StatusCode::BAD_REQUEST
    );
    // One version opens as a record with its stored snapshot.
    let (st, opened) = listed(
        &app,
        &alice,
        &c,
        &format!(
            "&collection=tax_codes&id={}",
            first["resourceId"].as_str().unwrap()
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{opened}");
    assert_eq!(opened["detail"]["record"]["code"], "VAT");
    assert_eq!(opened["selected"]["record"]["rate_numerator"], 11);
    assert_eq!(opened["selected"]["record"]["state"], "confirmed");

    // A second version cannot start while the first is open-ended, or on the
    // first one's last day. Retiring a confirmed code needs no second person,
    // but it needs the last day.
    let next = |from: &str| tax_code(&book, "VAT", "output", fraction(12, 100), &output, from);
    let (st, v) = send(&app, &alice, &next("2027-01-01")).await;
    assert_refused_as(st, &v, "tax_code_ranges_overlap");
    let (st, v) = send(&app, &alice, &withdraw(&first, 2)).await;
    assert_refused_as(st, &v, "tax_code_last_day_required");
    let (st, v) = send(&app, &alice, &retire(&first, 2, "2025-12-31")).await;
    assert_refused_as(st, &v, "tax_code_ends_before_start");
    let (st, retired) = send(&app, &alice, &retire(&first, 2, "2026-12-31")).await;
    assert_eq!(st, StatusCode::OK, "{retired}");
    assert_eq!(retired["receipt"]["type"], "tax_code.retire");
    assert_eq!(retired["receipt"]["resourceId"], first["resourceId"]);
    assert_eq!(retired["receipt"]["revision"], 3);
    assert_eq!(retired["receipt"]["taxCodeState"], "retired");
    for revision in [2, 3] {
        let (st, v) = send(&app, &alice, &retire(&first, revision, "2027-06-30")).await;
        assert_refused_as(st, &v, "tax_code_already_closed");
    }
    let (st, v) = send(&app, &bob, &confirm(&first, 3)).await;
    assert_refused_as(st, &v, "tax_code_not_awaiting_confirmation");
    let (st, v) = send(&app, &alice, &next("2026-12-31")).await;
    assert_refused_as(st, &v, "tax_code_ranges_overlap");
    let (st, v) = send(&app, &alice, &next("2025-06-01")).await;
    assert_refused_as(st, &v, "tax_code_ranges_overlap");
    let newer = confirmed(&app, &alice, &bob, &next("2027-01-01")).await;
    assert_ne!(newer["resourceId"], first["resourceId"]);

    let (_, last_day) = listed(&app, &alice, &c, "&asOf=2026-12-31").await;
    assert_eq!(row(&last_day, &first)["applies"], true);
    assert_eq!(row(&last_day, &first)["state"], "retired");
    assert_eq!(row(&last_day, &first)["effectiveTo"], "2026-12-31");
    assert_eq!(row(&last_day, &first)["revision"], 3);
    assert_eq!(row(&last_day, &newer)["applies"], false);
    let (_, next_day) = listed(&app, &alice, &c, "&asOf=2027-01-01").await;
    assert_eq!(row(&next_day, &first)["applies"], false);
    assert_eq!(row(&next_day, &newer)["applies"], true);
    assert_eq!(row(&next_day, &newer)["version"], 2);
    assert_eq!(row(&next_day, &newer)["rate"], fraction(12, 100));
    // The first version kept what it was entered with.
    assert_eq!(row(&next_day, &first)["rate"], fraction(11, 100));

    let versions: Vec<(i32, i32, String, String, Option<String>, i32)> = sqlx::query_as("SELECT version,revision,state,created_by,reviewed_by,rate_numerator FROM archava_accounting.tax_codes WHERE code='VAT' ORDER BY version")
        .fetch_all(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(
        versions,
        vec![
            (
                1,
                3,
                "retired".into(),
                "alice".into(),
                Some("bob".into()),
                11
            ),
            (
                2,
                2,
                "confirmed".into(),
                "alice".into(),
                Some("bob".into()),
                12
            )
        ]
    );
    // Each stored change has exactly one audit event by its actor.
    let events: Vec<(String, String, i32)> = sqlx::query_as("SELECT operation,actor_id,revision FROM archava_accounting.audit WHERE resource_kind='tax_code' AND resource_id=$1::uuid ORDER BY revision")
        .bind(first["resourceId"].as_str().unwrap())
        .fetch_all(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(
        events,
        vec![
            ("tax_code.create".into(), "alice".into(), 1),
            ("tax_code.confirm".into(), "bob".into(), 2),
            ("tax_code.retire".into(), "alice".into(), 3)
        ]
    );
}

/// **Witness: on a book without independent posting the creator confirms their
/// own code in a second step; an entered code can be withdrawn, after which it
/// neither applies nor blocks its range or its kind; and a code whose control
/// account changed or was deactivated before review cannot be confirmed.**
#[tokio::test]
async fn single_person_books_confirm_their_own_codes_and_entered_codes_can_be_withdrawn() {
    let (f, app, alice, bob, c) = setup().await;
    let book = single_person_book(&app, &alice, &c).await;
    let liability = account(&app, &alice, &book, "VAT-OUT", "liability", "operating").await;
    let asset = account(&app, &alice, &book, "VAT-IN", "asset", "operating").await;

    // Creation never confirms, on any book.
    let request = tax_code(
        &book,
        "VAT",
        "output",
        fraction(11, 100),
        &liability,
        "2026-01-01",
    );
    let (st, entered) = send(&app, &alice, &request).await;
    assert_eq!(st, StatusCode::OK, "{entered}");
    let entered = entered["receipt"].clone();
    assert_eq!(entered["taxCodeState"], "entered");
    let (_, w) = listed(&app, &alice, &c, "&asOf=2026-06-01").await;
    assert_eq!(row(&w, &entered)["applies"], false);
    let (st, own) = send(&app, &alice, &confirm(&entered, 1)).await;
    assert_eq!(st, StatusCode::OK, "{own}");
    assert_eq!(own["receipt"]["taxCodeState"], "confirmed");
    let (_, w) = listed(&app, &alice, &c, "&asOf=2026-06-01").await;
    assert_eq!(row(&w, &entered)["applies"], true);
    assert_eq!(row(&w, &entered)["createdBy"], "alice");
    assert_eq!(row(&w, &entered)["reviewedBy"], "alice");

    // An entered version holds its range until it is withdrawn.
    let mistaken = tax_code(
        &book,
        "GST",
        "input",
        fraction(10, 100),
        &asset,
        "2026-01-01",
    );
    let (st, wrong) = send(&app, &alice, &mistaken).await;
    assert_eq!(st, StatusCode::OK, "{wrong}");
    let wrong = wrong["receipt"].clone();
    let corrected = tax_code(
        &book,
        "GST",
        "output",
        fraction(10, 100),
        &liability,
        "2026-01-01",
    );
    let (st, v) = send(&app, &alice, &corrected).await;
    assert_refused_as(st, &v, "tax_code_kind_changed");
    let same_kind = tax_code(
        &book,
        "GST",
        "input",
        fraction(9, 100),
        &asset,
        "2026-01-01",
    );
    let (st, v) = send(&app, &alice, &same_kind).await;
    assert_refused_as(st, &v, "tax_code_ranges_overlap");
    // Anyone with tax.manage withdraws it; a last day, if sent, is not stored.
    let withdrawal = retire(&wrong, 1, "2026-12-31");
    let (st, gone) = send(&app, &bob, &withdrawal).await;
    assert_eq!(st, StatusCode::OK, "{gone}");
    assert_eq!(gone["receipt"]["type"], "tax_code.retire");
    assert_eq!(gone["receipt"]["revision"], 2);
    assert_eq!(gone["receipt"]["taxCodeState"], "withdrawn");
    let (_, w) = listed(&app, &alice, &c, "&asOf=2026-06-01").await;
    let withdrawn = row(&w, &wrong);
    assert_eq!(withdrawn["state"], "withdrawn");
    assert_eq!(withdrawn["applies"], false);
    assert_eq!(withdrawn["effectiveTo"], Value::Null);
    assert_eq!(withdrawn["reviewedBy"], Value::Null);
    assert!(withdrawn["withdrawnAt"].as_str().is_some());
    // A withdrawn version is closed for good.
    let (st, v) = send(&app, &alice, &confirm(&wrong, 2)).await;
    assert_refused_as(st, &v, "tax_code_not_awaiting_confirmation");
    for body in [withdraw(&wrong, 2), retire(&wrong, 2, "2026-12-31")] {
        let (st, v) = send(&app, &alice, &body).await;
        assert_refused_as(st, &v, "tax_code_already_closed");
    }
    // The same range, and the other kind, are free again.
    let (st, replaced) = send(&app, &alice, &corrected).await;
    assert_eq!(st, StatusCode::OK, "{replaced}");
    let replaced = replaced["receipt"].clone();
    let (st, v) = send(&app, &alice, &confirm(&replaced, 1)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, w) = listed(&app, &alice, &c, "&asOf=2026-06-01").await;
    assert_eq!(row(&w, &replaced)["version"], 2);
    assert_eq!(row(&w, &replaced)["kind"], "output");
    assert_eq!(row(&w, &replaced)["applies"], true);
    assert_eq!(row(&w, &wrong)["applies"], false);

    // The control account is checked again at confirmation.
    let moving = account(&app, &alice, &book, "VAT-MOVING", "liability", "operating").await;
    let (st, pending) = send(
        &app,
        &alice,
        &tax_code(
            &book,
            "LATE",
            "output",
            fraction(5, 100),
            &moving,
            "2026-01-01",
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{pending}");
    let pending = pending["receipt"].clone();
    let off = ok(
        &app,
        &alice,
        json!({"type":"account.set_active","id":moving["resourceId"],"expectedRevision":1,"active":false,"reason":"Retired control account"}),
    )
    .await;
    let (st, v) = send(&app, &alice, &confirm(&pending, 1)).await;
    assert_refused_as(st, &v, "tax_code_control_account_changed");
    // Active again, but no longer at the revision the code was entered with.
    ok(
        &app,
        &alice,
        json!({"type":"account.set_active","id":moving["resourceId"],"expectedRevision":off["revision"],"active":true,"reason":"Restored control account"}),
    )
    .await;
    let (st, v) = send(&app, &alice, &confirm(&pending, 1)).await;
    assert_refused_as(st, &v, "tax_code_control_account_changed");
    let (_, w) = listed(&app, &alice, &c, "").await;
    assert_eq!(row(&w, &pending)["state"], "entered");
    assert_eq!(row(&w, &pending)["controlAccount"]["revision"], 3);
    // The way out is to withdraw it and enter it again.
    let (st, v) = send(&app, &alice, &withdraw(&pending, 1)).await;
    assert_eq!(st, StatusCode::OK, "{v}");

    let states: Vec<(String, i32, String)> = sqlx::query_as(
        "SELECT code,version,state FROM archava_accounting.tax_codes ORDER BY code,version",
    )
    .fetch_all(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(
        states,
        vec![
            ("GST".into(), 1, "withdrawn".into()),
            ("GST".into(), 2, "confirmed".into()),
            ("LATE".into(), 1, "withdrawn".into()),
            ("VAT".into(), 1, "confirmed".into())
        ]
    );
}

/// **Witness: each rule of a tax code is refused with its own reason, and a
/// refused command stores nothing.**
#[tokio::test]
async fn tax_codes_refuse_wrong_accounts_overlaps_other_books_and_missing_authority() {
    let (f, app, alice, bob, c) = setup().await;
    let book = book(&app, &alice, &c).await;
    let liability = account(&app, &alice, &book, "VAT-OUT", "liability", "operating").await;
    let asset = account(&app, &alice, &book, "VAT-IN", "asset", "operating").await;
    let cash = account(&app, &alice, &book, "BANK", "asset", "cash").await;
    let unclassified = account(&app, &alice, &book, "ACCRUAL", "liability", "none").await;
    let revenue = account(&app, &alice, &book, "REVENUE", "revenue", "operating").await;
    let closed = account(&app, &alice, &book, "OLD-VAT", "liability", "operating").await;
    let deactivated = ok(
        &app,
        &alice,
        json!({"type":"account.set_active","id":closed["resourceId"],"expectedRevision":1,"active":false,"reason":"Retired control account"}),
    )
    .await;

    for (why, code, body) in [
        (
            "an output code on an asset account",
            "tax_code_control_account_wrong_class",
            tax_code(
                &book,
                "BAD-1",
                "output",
                fraction(11, 100),
                &asset,
                "2026-01-01",
            ),
        ),
        (
            "an input code on a liability account",
            "tax_code_control_account_wrong_class",
            tax_code(
                &book,
                "BAD-2",
                "input",
                fraction(11, 100),
                &liability,
                "2026-01-01",
            ),
        ),
        (
            "an input code on a cash account",
            "tax_code_control_account_wrong_class",
            tax_code(
                &book,
                "BAD-3",
                "input",
                fraction(11, 100),
                &cash,
                "2026-01-01",
            ),
        ),
        (
            "an output code on a liability outside operating activities",
            "tax_code_control_account_wrong_class",
            tax_code(
                &book,
                "BAD-4",
                "output",
                fraction(11, 100),
                &unclassified,
                "2026-01-01",
            ),
        ),
        (
            "an output code on a revenue account",
            "tax_code_control_account_wrong_class",
            tax_code(
                &book,
                "BAD-5",
                "output",
                fraction(11, 100),
                &revenue,
                "2026-01-01",
            ),
        ),
        (
            "an account version that is no longer current",
            "tax_code_control_account_changed",
            tax_code(
                &book,
                "BAD-6",
                "output",
                fraction(11, 100),
                &closed,
                "2026-01-01",
            ),
        ),
        (
            "an inactive account at its current version",
            "tax_code_control_account_changed",
            tax_code(
                &book,
                "BAD-7",
                "output",
                fraction(11, 100),
                &deactivated,
                "2026-01-01",
            ),
        ),
    ] {
        let (st, v) = send(&app, &alice, &body).await;
        assert_refused_as(st, &v, code);
        assert_eq!(stored(&f).await, 0, "{why} must store nothing");
    }

    // Shape and bounds are refused before anything is read.
    for (field, value) in [
        ("rate", fraction(101, 100)),
        ("rate", fraction(-1, 100)),
        ("rate", fraction(1, 0)),
        ("rate", fraction(1, 1_000_001)),
        ("rate", json!(0.11)),
        ("rate", json!("11%")),
        ("base", fraction(0, 1)),
        ("base", fraction(13, 12)),
        ("kind", json!("withholding")),
        ("code", json!("vat")),
        ("effectiveFrom", json!("2026-02-30")),
        ("reviewedBy", json!("bob")),
        ("createdBy", json!("bob")),
        ("state", json!("confirmed")),
    ] {
        let mut body = tax_code(
            &book,
            "BOUNDS",
            "output",
            fraction(11, 100),
            &liability,
            "2026-01-01",
        );
        body["command"][field] = value;
        let (st, v) = send(&app, &alice, &body).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{field}: {v}");
        assert_eq!(v["reason"], "invalid_submission", "{field}: {v}");
    }
    let mut named = confirm(&json!({"resourceId":Uuid::new_v4()}), 1);
    named["command"]["reviewedBy"] = json!("bob");
    assert_eq!(send(&app, &alice, &named).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(stored(&f).await, 0);

    let vat = confirmed(
        &app,
        &alice,
        &bob,
        &tax_code(
            &book,
            "VAT",
            "output",
            fraction(11, 100),
            &liability,
            "2026-03-01",
        ),
    )
    .await;
    // The same code again while the first version is open-ended.
    let (st, v) = send(
        &app,
        &alice,
        &tax_code(
            &book,
            "VAT",
            "output",
            fraction(12, 100),
            &liability,
            "2026-06-01",
        ),
    )
    .await;
    assert_refused_as(st, &v, "tax_code_ranges_overlap");
    // A code keeps its kind.
    let (st, v) = send(
        &app,
        &alice,
        &tax_code(
            &book,
            "VAT",
            "input",
            fraction(11, 100),
            &asset,
            "2027-01-01",
        ),
    )
    .await;
    assert_refused_as(st, &v, "tax_code_kind_changed");
    // The last day cannot precede the first.
    let (st, v) = send(&app, &alice, &retire(&vat, 2, "2026-02-28")).await;
    assert_refused_as(st, &v, "tax_code_ends_before_start");
    // A stale revision is a plain conflict; an unknown code is not found.
    assert_eq!(
        send(&app, &alice, &retire(&vat, 1, "2026-12-31")).await.0,
        StatusCode::CONFLICT
    );
    for body in [
        retire(&json!({"resourceId":Uuid::new_v4()}), 1, "2026-12-31"),
        confirm(&json!({"resourceId":Uuid::new_v4()}), 1),
    ] {
        assert_eq!(send(&app, &alice, &body).await.0, StatusCode::NOT_FOUND);
    }
    // The first day itself is a valid last day.
    let (st, v) = send(&app, &alice, &retire(&vat, 2, "2026-03-01")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(stored(&f).await, 1);

    // A book kept in an external system takes no tax code.
    let other = company(&app, &alice, "EXTERNAL").await;
    let mut external = create(&other, "JPY");
    external["command"]["code"] = json!("EXTERNAL");
    external["command"]["sourceAuthority"] = json!("external");
    external["command"]["externalSystem"] = json!("EXISTING-BOOK");
    let (st, external) = send(&app, &alice, &external).await;
    assert_eq!(st, StatusCode::OK, "{external}");
    let (st, v) = send(
        &app,
        &alice,
        &tax_code(
            &external["receipt"],
            "VAT",
            "output",
            fraction(11, 100),
            &liability,
            "2026-01-01",
        ),
    )
    .await;
    assert_refused_as(st, &v, "tax_code_native_book_required");
    assert_eq!(stored(&f).await, 1);

    // Authority: entering, confirming and retiring need tax.manage and
    // tax.read; listing needs tax.read.
    let (st, waiting) = send(
        &app,
        &alice,
        &tax_code(
            &book,
            "WAITING",
            "output",
            fraction(11, 100),
            &liability,
            "2026-01-01",
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{waiting}");
    let waiting = waiting["receipt"].clone();
    member(&f, "tax-viewer", &["books.read", "tax.read"]).await;
    member(&f, "tax-writer", &["books.read", "tax.manage"]).await;
    let reader = login(&app, "reader").await;
    let viewer = login(&app, "tax-viewer").await;
    let writer = login(&app, "tax-writer").await;
    for (who, session) in [
        ("reader", &reader),
        ("tax-viewer", &viewer),
        ("tax-writer", &writer),
    ] {
        for body in [
            tax_code(
                &book,
                "DENIED",
                "output",
                fraction(11, 100),
                &liability,
                "2026-01-01",
            ),
            confirm(&waiting, 1),
            withdraw(&waiting, 1),
        ] {
            let (st, v) = send(&app, session, &body).await;
            assert_eq!(st, StatusCode::FORBIDDEN, "{who}: {v}");
        }
    }
    assert_eq!(stored(&f).await, 2);
    let (st, seen) = listed(&app, &viewer, &c, "").await;
    assert_eq!(st, StatusCode::OK, "{seen}");
    assert_eq!(seen["tax_codes"].as_array().unwrap().len(), 2);
    assert_eq!(row(&seen, &waiting)["state"], "entered");
    for (who, session) in [("reader", &reader), ("tax-writer", &writer)] {
        let (st, v) = listed(&app, session, &c, "&collection=tax_codes").await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{who}: {v}");
        let (st, v) = listed(&app, session, &c, "").await;
        assert_eq!(st, StatusCode::OK, "{who}: {v}");
        assert!(v.get("tax_codes").is_none(), "{who}: {v}");
        assert!(v.get("taxNotice").is_none(), "{who}: {v}");
        assert!(
            !v["collections"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == "tax_codes"),
            "{who}"
        );
    }
    // The receipt of a tax command is read with the same authority.
    assert_eq!(status(&app, &alice, &waiting).await.0, StatusCode::OK);

    // Migration 016 widened the permission vocabulary without granting it:
    // a membership holds only what an administrator gave it.
    let gained: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_accounting.memberships WHERE principal_id IN ('reader','tax-viewer') AND permissions @> ARRAY['tax.manage']::text[]")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(gained, 0);
    let reader_permissions: Vec<String> = sqlx::query_scalar(
        "SELECT permissions FROM archava_accounting.memberships WHERE principal_id='reader'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(reader_permissions, ["books.read", "reports.read"]);
    assert!(sqlx::query("UPDATE archava_accounting.memberships SET permissions=permissions||ARRAY['tax.write'] WHERE principal_id='reader'")
        .execute(&f.admin_pool)
        .await
        .is_err());
}

/// **Witness: a stored tax code cannot be edited or deleted with direct SQL,
/// by the serving role or the owner, cannot be planted or confirmed without
/// its reviewed command, and is invisible to another tenant.**
#[tokio::test]
async fn stored_tax_codes_are_immutable_undeletable_and_tenant_bound() {
    let (f, app, alice, bob, c) = setup().await;
    let book = book(&app, &alice, &c).await;
    let liability = account(&app, &alice, &book, "VAT-OUT", "liability", "operating").await;
    let code = |name: &str, from: &str| {
        tax_code(&book, name, "output", fraction(11, 100), &liability, from)
    };
    let (st, entered) = send(&app, &alice, &code("NEW", "2026-01-01")).await;
    assert_eq!(st, StatusCode::OK, "{entered}");
    let open = confirmed(&app, &alice, &bob, &code("VAT", "2026-01-01")).await;
    let retired = confirmed(&app, &alice, &bob, &code("OLD", "2025-01-01")).await;
    let (st, v) = send(&app, &alice, &retire(&retired, 2, "2025-12-31")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, withdrawn) = send(&app, &alice, &code("GONE", "2026-01-01")).await;
    assert_eq!(st, StatusCode::OK, "{withdrawn}");
    let (st, v) = send(&app, &alice, &withdraw(&withdrawn["receipt"], 1)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let before: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(t) ORDER BY t.id) FROM archava_accounting.tax_codes t",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();

    let entered_id = entered["receipt"]["resourceId"].as_str().unwrap();
    let open_id = open["resourceId"].as_str().unwrap();
    let retired_id = retired["resourceId"].as_str().unwrap();
    let withdrawn_id = withdrawn["receipt"]["resourceId"].as_str().unwrap();
    const IMMUTABLE: &str =
        "A stored tax code is immutable; it can only be confirmed, withdrawn or retired";
    const CLOSED: &str = "A withdrawn or retired tax code cannot change again";
    for (statement, target, expected) in [
        ("UPDATE archava_accounting.tax_codes SET rate_numerator=12 WHERE id=$1::uuid", open_id, IMMUTABLE),
        ("UPDATE archava_accounting.tax_codes SET rate_numerator=12 WHERE id=$1::uuid", entered_id, IMMUTABLE),
        ("UPDATE archava_accounting.tax_codes SET control_account_revision=2 WHERE id=$1::uuid", open_id, IMMUTABLE),
        ("UPDATE archava_accounting.tax_codes SET reviewed_by='alice' WHERE id=$1::uuid", open_id, IMMUTABLE),
        ("UPDATE archava_accounting.tax_codes SET state='retired',effective_to=DATE '2026-12-31',retired_at=clock_timestamp(),revision=3,rate_numerator=12 WHERE id=$1::uuid", open_id, IMMUTABLE),
        ("UPDATE archava_accounting.tax_codes SET state='retired',effective_to=DATE '2025-12-31',retired_at=clock_timestamp(),revision=3 WHERE id=$1::uuid", open_id, "A tax code cannot end before it starts"),
        ("UPDATE archava_accounting.tax_codes SET state='retired',retired_at=clock_timestamp(),revision=3 WHERE id=$1::uuid", open_id, "Retiring a confirmed tax code needs its last day"),
        ("UPDATE archava_accounting.tax_codes SET state='entered',reviewed_by=NULL,confirmed_at=NULL,revision=1 WHERE id=$1::uuid", open_id, IMMUTABLE),
        ("UPDATE archava_accounting.tax_codes SET state='confirmed',confirmed_at=clock_timestamp() WHERE id=$1::uuid", open_id, "Only an entered tax code can be confirmed"),
        // An entered code cannot skip its review, be confirmed by its creator
        // on this book, by someone without tax authority, or with other edits.
        ("UPDATE archava_accounting.tax_codes SET state='retired',effective_to=DATE '2026-12-31',retired_at=clock_timestamp(),revision=2 WHERE id=$1::uuid", entered_id, IMMUTABLE),
        ("UPDATE archava_accounting.tax_codes SET state='confirmed',reviewed_by='alice',confirmed_at=clock_timestamp(),revision=2 WHERE id=$1::uuid", entered_id, "A book with independent posting needs a tax code reviewer other than the person who entered it"),
        ("UPDATE archava_accounting.tax_codes SET state='confirmed',reviewed_by='reader',confirmed_at=clock_timestamp(),revision=2 WHERE id=$1::uuid", entered_id, "A tax code requires a current human member with tax authority over the book"),
        ("UPDATE archava_accounting.tax_codes SET state='confirmed',reviewed_by='bob',confirmed_at=clock_timestamp(),revision=2,rate_numerator=12 WHERE id=$1::uuid", entered_id, IMMUTABLE),
        ("UPDATE archava_accounting.tax_codes SET state='confirmed',reviewed_by='bob',confirmed_at=clock_timestamp(),revision=2,created_by='bob' WHERE id=$1::uuid", entered_id, IMMUTABLE),
        ("UPDATE archava_accounting.tax_codes SET effective_to=DATE '2026-06-30' WHERE id=$1::uuid", retired_id, CLOSED),
        ("UPDATE archava_accounting.tax_codes SET state='confirmed',effective_to=NULL,retired_at=NULL,revision=2 WHERE id=$1::uuid", retired_id, CLOSED),
        ("UPDATE archava_accounting.tax_codes SET state='entered',withdrawn_at=NULL,revision=1 WHERE id=$1::uuid", withdrawn_id, CLOSED),
        ("UPDATE archava_accounting.tax_codes SET state='confirmed',reviewed_by='bob',confirmed_at=clock_timestamp(),withdrawn_at=NULL WHERE id=$1::uuid", withdrawn_id, "Only an entered tax code can be confirmed"),
        ("DELETE FROM archava_accounting.tax_codes WHERE id=$1::uuid", open_id, "Tax code history cannot be deleted"),
    ] {
        // The database owner meets the guard itself.
        let error = sqlx::query(statement)
            .bind(target)
            .execute(&f.admin_pool)
            .await
            .expect_err(statement);
        assert!(error.to_string().contains(expected), "{statement}: {error}");
        // The serving role, inside its tenant, is refused as well.
        let mut tx = f.runtime_pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = sqlx::query(statement)
            .bind(target)
            .execute(&mut *tx)
            .await
            .expect_err(statement);
        // It has no DELETE privilege; an UPDATE reaches the same guard.
        let wanted = if statement.starts_with("DELETE") {
            "permission denied"
        } else {
            expected
        };
        assert!(error.to_string().contains(wanted), "{statement}: {error}");
    }
    // The serving role holds no DELETE privilege at all.
    let may_delete: bool = sqlx::query_scalar(
        "SELECT has_table_privilege(current_user,'archava_accounting.tax_codes','DELETE')",
    )
    .fetch_one(&f.runtime_pool)
    .await
    .unwrap();
    assert!(!may_delete);

    // Direct inserts meet the same guards as the command.
    let insert = "INSERT INTO archava_accounting.tax_codes(tenant_id,id,book_id,code,version,kind,rate_numerator,rate_denominator,base_numerator,base_denominator,control_account_id,control_account_revision,effective_from,created_by,state,reviewed_by,confirmed_at,revision) VALUES('acme',gen_random_uuid(),$1::uuid,$2,$3,'output',$4,100,1,1,$5::uuid,1,DATE '2030-01-01',$6,$7,$8,CASE WHEN $8 IS NULL THEN NULL ELSE clock_timestamp() END,CASE WHEN $7='confirmed' THEN 2 ELSE 1 END)";
    let book_id = book["bookId"].as_str().unwrap();
    let account_id = liability["resourceId"].as_str().unwrap();
    for (code, version, numerator, creator, state, reviewer, expected) in [
        ("PLANTED", 1, 101, "alice", "entered", None, "A tax rate must be between zero and one and its base fraction above zero and at most one"),
        ("PLANTED", 2, 11, "alice", "entered", None, "Tax code versions must append in order"),
        ("PLANTED", 1, 11, "reader", "entered", None, "A tax code requires a current human member with tax authority over the book"),
        ("PLANTED", 1, 11, "service-seat", "entered", None, "A tax code requires a current human member with tax authority over the book"),
        // A code cannot be born confirmed, even naming a second person.
        ("PLANTED", 1, 11, "alice", "confirmed", Some("bob"), "A tax code starts entered, unconfirmed and open-ended at revision one"),
        ("VAT", 2, 11, "alice", "entered", None, "Tax code date ranges of one code must not overlap"),
    ] {
        let error = sqlx::query(insert)
            .bind(book_id).bind(code).bind(version).bind(numerator).bind(account_id).bind(creator).bind(state).bind(reviewer)
            .execute(&f.admin_pool)
            .await
            .expect_err(expected);
        assert!(error.to_string().contains(expected), "{expected}: {error}");
    }
    // A row that passes every row guard still needs its own reviewed command:
    // without the audit event and receipt the transaction cannot commit.
    let mut tx = f.admin_pool.begin().await.unwrap();
    sqlx::query(insert)
        .bind(book_id)
        .bind("PLANTED")
        .bind(1)
        .bind(11)
        .bind(account_id)
        .bind("alice")
        .bind("entered")
        .bind(None::<&str>)
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = tx.commit().await.expect_err("a planted tax code");
    assert!(
        error
            .to_string()
            .contains("Financial audit and receipt are incomplete")
            || error.to_string().contains(
                "A tax code change must carry the reviewed command of the person it names"
            ),
        "{error}"
    );
    // The same holds for a confirmation written in SQL in a second person's
    // name: the row guard passes, the commit does not.
    let mut tx = f.admin_pool.begin().await.unwrap();
    sqlx::query("UPDATE archava_accounting.tax_codes SET state='confirmed',reviewed_by='bob',confirmed_at=clock_timestamp(),revision=2 WHERE id=$1::uuid")
        .bind(entered_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = tx.commit().await.expect_err("a planted confirmation");
    assert!(
        error
            .to_string()
            .contains("Financial audit and receipt are incomplete")
            || error.to_string().contains(
                "A tax code change must carry the reviewed command of the person it names"
            ),
        "{error}"
    );
    let after: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(t) ORDER BY t.id) FROM archava_accounting.tax_codes t",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(after, before);

    for (tenant, visible) in [("acme", 4_i64), ("another-tenant", 0)] {
        let mut tx = f.runtime_pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('archava.tenant_id',$1,true)")
            .bind(tenant)
            .execute(&mut *tx)
            .await
            .unwrap();
        let seen: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_accounting.tax_codes")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(seen, visible, "{tenant}");
    }
}

/// **Witness: two people entering the same code at the same time store one
/// version (the other is refused as an overlap, not stored beside it), and two
/// people confirming the same entered code confirm it once.**
#[tokio::test]
async fn concurrent_versions_and_confirmations_of_one_code_do_not_double() {
    let (f, app, alice, bob, c) = setup().await;
    let book = single_person_book(&app, &alice, &c).await;
    let liability = account(&app, &alice, &book, "VAT-OUT", "liability", "operating").await;
    for round in 0..4 {
        let code = format!("RACE-{round}");
        let first = tax_code(
            &book,
            &code,
            "output",
            fraction(11, 100),
            &liability,
            "2026-01-01",
        );
        let second = tax_code(
            &book,
            &code,
            "output",
            fraction(12, 100),
            &liability,
            "2026-07-01",
        );
        let (one, two) = tokio::join!(send(&app, &alice, &first), send(&app, &bob, &second));
        let (won, lost) = if one.0 == StatusCode::OK {
            (one, two)
        } else {
            (two, one)
        };
        assert_eq!(won.0, StatusCode::OK, "{}", won.1);
        assert_refused_as(lost.0, &lost.1, "tax_code_ranges_overlap");
        let rows: i64 =
            sqlx::query_scalar("SELECT count(*) FROM archava_accounting.tax_codes WHERE code=$1")
                .bind(&code)
                .fetch_one(&f.admin_pool)
                .await
                .unwrap();
        assert_eq!(rows, 1, "{code}");

        let entered = won.1["receipt"].clone();
        let (by_alice, by_bob) = (confirm(&entered, 1), confirm(&entered, 1));
        let (one, two) = tokio::join!(send(&app, &alice, &by_alice), send(&app, &bob, &by_bob));
        let (won, lost) = if one.0 == StatusCode::OK {
            (one, two)
        } else {
            (two, one)
        };
        assert_eq!(won.0, StatusCode::OK, "{}", won.1);
        assert_refused_as(lost.0, &lost.1, "tax_code_not_awaiting_confirmation");
        let confirmations: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_accounting.audit WHERE operation='tax_code.confirm' AND resource_id=$1::uuid")
            .bind(entered["resourceId"].as_str().unwrap())
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
        assert_eq!(confirmations, 1, "{code}");
    }
}

/// **Witness: the host refuses to start when a tax guard is missing.**
#[tokio::test]
async fn the_host_is_not_ready_without_the_tax_guards() {
    let (f, _app, _alice, _bob, _c) = setup().await;
    for trigger in [
        "tax_code_guard",
        "tax_code_complete",
        "tax_code_claim_complete",
    ] {
        let toggle = |state: &str| {
            format!("ALTER TABLE archava_accounting.tax_codes {state} TRIGGER {trigger}")
        };
        sqlx::query(sqlx::AssertSqlSafe(toggle("DISABLE").as_str()))
            .execute(&f.admin_pool)
            .await
            .unwrap();
        let complete: bool = sqlx::query_scalar("SELECT archava_accounting.tax_guards_complete()")
            .fetch_one(&f.runtime_pool)
            .await
            .unwrap();
        assert!(!complete, "{trigger}");
        let state = AppState::try_new_all(
            f.runtime_pool.clone(),
            ledger(),
            AllReadiness {
                commerce: &f.commerce_ledger,
                identity: &f.identity_ledger,
                crm: &f.crm_ledger,
                owner: &f.owner_ledger,
                purchasing: &f.purchasing_ledger,
                additional_erp: &f.erp_readiness(),
            },
            &[7; 32],
        )
        .await;
        assert!(state.is_err(), "{trigger}");
        sqlx::query(sqlx::AssertSqlSafe(toggle("ENABLE").as_str()))
            .execute(&f.admin_pool)
            .await
            .unwrap();
    }
    let _: Router = app(&f).await;
}
