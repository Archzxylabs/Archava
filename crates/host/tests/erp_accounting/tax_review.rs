//! Findings of the independent review of the tax money path (2026-11-14) and
//! the rules decided after it (TAX.md, "Decisions taken after review"): one
//! UTC calendar for every tax date, readiness over the tax guards outside
//! Accounting, pinned search paths, a tax control account that is never a
//! posting policy account, the ledger bound at capture, and a reversal that
//! is not drafted before the document it reverses is posted.
use super::invoice_sources::SourceFixture;
use super::tax_sales::{
    confirmed_code, fulfilled, guard_message, invoice_row, order, output_account, product,
};
use super::*;

/// The UTC date now, and a time zone whose calendar date differs from it at
/// this moment with the date it has there. One of the two candidates always
/// differs: UTC+14 from 10:00 UTC on, UTC-12 before 12:00 UTC.
async fn another_calendar(f: &Fixture) -> (String, String, String) {
    sqlx::query_as(
        "SELECT (now() AT TIME ZONE 'UTC')::date::text,z,(now() AT TIME ZONE z)::date::text \
         FROM (VALUES ('Pacific/Kiritimati'),('Etc/GMT+12')) v(z) \
         WHERE (now() AT TIME ZONE z)::date<>(now() AT TIME ZONE 'UTC')::date LIMIT 1",
    )
    .fetch_one(&f.runtime_pool)
    .await
    .unwrap()
}
/// The host over fresh serving-role connections whose session `TimeZone` is
/// `zone`. The driver starts every connection with `TimeZone=UTC`, whatever
/// the role, the database or the connection options say, so the host's own
/// sessions are in UTC; a session is in another zone only after a `SET`, which
/// is what each connection of this pool runs first.
async fn app_in_zone(f: &Fixture, zone: &str) -> Router {
    let statement = format!("SET TimeZone='{zone}'");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .after_connect(move |connection, _| {
            let statement = statement.clone();
            Box::pin(async move {
                sqlx::query(sqlx::AssertSqlSafe(statement))
                    .execute(connection)
                    .await
                    .map(|_| ())
            })
        })
        .connect_with(f.runtime_options())
        .await
        .unwrap();
    let shown: String = sqlx::query_scalar("SHOW TimeZone")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(shown, zone);
    router(
        AppState::try_new_all(
            pool,
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
        .await
        .unwrap(),
    )
}
async fn retire(s: &SourceFixture, code: &Value, last_day: &str) -> Value {
    ok(&s.app,&s.alice,json!({"type":"tax_code.retire","id":code["resourceId"],"expectedRevision":2,"effectiveTo":last_day,"reason":"The company's rate changes"})).await
}
/// One direct invoice insert as the serving role for a 30,000 exclusive
/// order, with the snapshot of `version` (the code's receipt, its code text,
/// version number and rate), in a session whose `TimeZone` is `zone`, created
/// `shifted` away from the clock (an interval). The deferred provenance check
/// runs before the transaction is rolled back.
async fn planted_invoice(
    s: &SourceFixture,
    order: &Value,
    version: (&Value, &str, i32, i32),
    zone: Option<&str>,
    shifted: &str,
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
    let tax = 300 * i64::from(version.3);
    let attempt = async {
        if let Some(zone) = zone {
            sqlx::query("SELECT set_config('TimeZone',$1,true)")
                .bind(zone)
                .execute(&mut *connection)
                .await?;
        }
        sqlx::query("INSERT INTO archava_billing.invoices(tenant_id,id,invoice_code,customer_ref,currency,status,gross_minor,outstanding_minor,consumer_ref,owner_id,created_at,source_order_id,source_order_revision,customer_party_id,customer_party_revision,company_id,company_revision,company_code,company_name,tax_minor,tax_code_id,tax_code,tax_code_version,tax_kind,tax_rate_numerator,tax_rate_denominator,tax_base_numerator,tax_base_denominator,price_includes_tax) \
            SELECT o.tenant_id,$1,$2,o.customer_name,o.currency,'draft',$3,$3,'commerce:'||o.id::text,'alice',clock_timestamp()+$4::text::interval,o.id,o.revision,o.customer_party_id,o.customer_party_revision,o.company_id,o.company_revision,o.company_code,o.company_name,\
            $5,$6::uuid,$7,$8,'output',$9,100,1,1,false FROM archava_commerce.orders o WHERE o.id=$10::uuid")
            .bind(invoice).bind(format!("DIRECT-{invoice}").chars().take(40).collect::<String>()).bind(30_000 + tax).bind(shifted)
            .bind(tax).bind(version.0["resourceId"].as_str().unwrap()).bind(version.1).bind(version.2).bind(version.3)
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

/// **Witness (S1): every tax date is the UTC calendar date. With a session
/// time zone whose date differs from the UTC date, the order guard, the
/// invoice guard, the billing adapter and the tax code list still decide by
/// the UTC date; and a taxed invoice cannot be inserted with a `created_at`
/// that is not its own transaction's clock, so a direct insert as the serving
/// role cannot back-date an invoice to reach a retired version.**
#[tokio::test]
async fn tax_dates_are_utc_dates_and_a_taxed_invoice_cannot_be_back_dated() {
    let s = SourceFixture::new().await;
    let (utc, zone, there) = another_calendar(&s.f).await;
    assert_ne!(utc, there);
    // Both calendars agree up to `last` and from `next` on; between them the
    // UTC date and the session's date fall on different sides.
    let (last, next) = if utc < there {
        (utc.clone(), there.clone())
    } else {
        (there.clone(), utc.clone())
    };
    let (before,): (String,) = sqlx::query_as("SELECT ((now() AT TIME ZONE 'UTC')::date-3)::text")
        .fetch_one(&s.f.runtime_pool)
        .await
        .unwrap();
    let output = output_account(&s).await;
    let first = confirmed_code(
        &s,
        "VAT",
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
    let early = confirmed_code(
        &s,
        "EARLY",
        "output",
        (11, 100),
        (1, 1),
        &output,
        "2020-01-01",
    )
    .await;
    let item = product(&s, "TEA", 15_000).await;
    let direct = fulfilled(&s, order(&s, &item, 2, Some(("VAT", false)))).await;
    let through = fulfilled(&s, order(&s, &item, 2, Some(("VAT", false)))).await;
    let stale = fulfilled(&s, order(&s, &item, 2, Some(("OLD", false)))).await;
    // VAT changes its rate between the two calendars' dates; OLD ended three
    // days ago in every calendar; EARLY ends on `last` and LATE starts on `next`.
    retire(&s, &first, &last).await;
    let second = confirmed_code(&s, "VAT", "output", (12, 100), (1, 1), &output, &next).await;
    retire(&s, &old, &before).await;
    retire(&s, &early, &last).await;
    confirmed_code(&s, "LATE", "output", (11, 100), (1, 1), &output, &next).await;
    // The version of VAT that applies on the UTC date, and the one that
    // applies on the session's date.
    let (on_utc, in_session) = if utc == next {
        ((&second, "VAT", 2, 12), (&first, "VAT", 1, 11))
    } else {
        ((&first, "VAT", 1, 11), (&second, "VAT", 2, 12))
    };
    let (code_on_utc, code_in_session) = if utc == next {
        ("LATE", "EARLY")
    } else {
        ("EARLY", "LATE")
    };
    let not_applicable = (
        "23514".to_owned(),
        "No confirmed version of the invoice tax code applies on the invoice date".to_owned(),
    );

    // The invoice guard, in a session of the other calendar.
    planted_invoice(&s, &direct, on_utc, Some(&zone), "0")
        .await
        .expect("the version of the UTC date is the invoice's version in any session time zone");
    let refused = planted_invoice(&s, &direct, in_session, Some(&zone), "0")
        .await
        .expect_err("the version of the session's date is not the invoice's version");
    assert_eq!(guard_message(&refused), not_applicable);

    // The host on connections of the other calendar: the order guard, the
    // adapter that takes the invoice's version, and the two lists.
    let app = app_in_zone(&s.f, &zone).await;
    let alice = login(&app, "alice").await;
    let command = |body: Value| json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":body});
    let (st, v) = call(
        &app,
        "POST",
        "/erp/commerce/commands",
        command(order(&s, &item, 1, Some((code_on_utc, false)))),
        Some(&alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = call(
        &app,
        "POST",
        "/erp/commerce/commands",
        command(order(&s, &item, 1, Some((code_in_session, false)))),
        Some(&alice),
    )
    .await;
    assert_refused_as(st, &v, "order_tax_code_not_applicable");
    let (st, invoice) = call(
        &app,
        "POST",
        "/erp/billing/commands",
        json!({"tenant":"acme","mutation":{"commandId":Uuid::new_v4(),"type":"create_invoice_from_order","invoiceCode":"IN-ZONE","orderId":through["id"],"expectedOrderRevision":3}}),
        Some(&alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{invoice}");
    let row = invoice_row(&s, &invoice).await;
    assert_eq!(row["tax"]["codeId"], on_utc.0["resourceId"], "{row}");
    assert_eq!(row["tax"]["version"], on_utc.2, "{row}");
    assert_eq!(row["taxMinor"], 300 * i64::from(on_utc.3), "{row}");
    let company = s.company["companyId"].as_str().unwrap();
    let (st, sales) = call(
        &app,
        "GET",
        &format!("/erp/commerce/workspace?tenant=acme&companyId={company}"),
        Value::Null,
        Some(&alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{sales}");
    let offered: Vec<(&str, i64, &str)> = sales["taxCodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["code"].as_str().unwrap(),
                c["version"].as_i64().unwrap(),
                c["appliesOn"].as_str().unwrap(),
            )
        })
        .collect();
    let mut expected = vec![
        (code_on_utc, 1, utc.as_str()),
        ("VAT", i64::from(on_utc.2), utc.as_str()),
    ];
    expected.sort();
    assert_eq!(offered, expected, "{sales}");
    let (st, listed) = call(
        &app,
        "GET",
        &format!("/erp/accounting/workspace?tenant=acme&companyId={company}&collection=tax_codes"),
        Value::Null,
        Some(&alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{listed}");
    for code in listed["tax_codes"].as_array().unwrap() {
        assert_eq!(code["appliesOn"], utc.as_str(), "{code}");
        let on_the_utc_date = code["state"] != "entered"
            && code["effectiveFrom"].as_str().unwrap() <= utc.as_str()
            && code["effectiveTo"]
                .as_str()
                .is_none_or(|to| to >= utc.as_str());
        assert_eq!(code["applies"], on_the_utc_date, "{code}");
    }

    // Back-dating. OLD applied three days ago and no longer does: an invoice
    // created now is refused for it, and so is one that claims to have been
    // created then (or later than now), whatever version it names.
    let refused = planted_invoice(&s, &stale, (&old, "OLD", 1, 10), None, "0")
        .await
        .expect_err("OLD does not apply today");
    assert_eq!(guard_message(&refused), not_applicable);
    for shifted in ["-4 days", "-1 hour", "-10 seconds", "10 seconds", "1 day"] {
        for (order, version) in [(&stale, (&old, "OLD", 1, 10)), (&direct, on_utc)] {
            let refused = planted_invoice(&s, order, version, None, shifted)
                .await
                .expect_err("a taxed invoice is created at the clock of its transaction");
            assert_eq!(
                guard_message(&refused),
                (
                    "23514".to_owned(),
                    "A taxed invoice is created at the time of its own transaction".to_owned()
                ),
                "{shifted}"
            );
        }
    }
    assert_eq!(
        super::tax_sales::count(&s.f, "archava_billing.invoices").await,
        1
    );
}

/// **Witness (S5): the provenance guard that recomputes an invoice's tax from
/// its order, and the two other invoice guards that were created without one,
/// run with a pinned `search_path`; no function of the tax path is left
/// without one; and an invoice from an order is still created, taxed or not.**
#[tokio::test]
async fn the_invoice_provenance_guards_run_with_a_pinned_search_path() {
    let s = SourceFixture::new().await;
    for function in [
        "archava_billing.assert_order_invoice()",
        "archava_billing.guard_invoice_source()",
        "archava_billing.guard_customer_contact()",
    ] {
        let config: Option<Vec<String>> =
            sqlx::query_scalar("SELECT proconfig FROM pg_proc WHERE oid=$1::regprocedure")
                .bind(function)
                .fetch_one(&s.f.admin_pool)
                .await
                .unwrap();
        assert_eq!(
            config,
            Some(vec!["search_path=pg_catalog".to_owned()]),
            "{function}"
        );
    }
    // Of the trigger functions on the tables of the tax path, the only ones
    // left without a pinned search path are older than the tax migrations and
    // Billing 005 to 011 (Billing 001 to 004, Purchasing 001) and take no part
    // in a tax decision.
    let unpinned: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT p.oid::regprocedure::text FROM pg_trigger t JOIN pg_proc p ON p.oid=t.tgfoid \
         WHERE NOT t.tgisinternal AND t.tgrelid IN ('archava_billing.invoices'::regclass,'archava_billing.invoice_lines'::regclass,\
          'archava_billing.invoice_corrections'::regclass,'archava_purchasing.supplier_documents'::regclass,'archava_accounting.tax_codes'::regclass,\
          'archava_accounting.invoice_sources'::regclass,'archava_accounting.purchase_sources'::regclass) \
         AND NOT EXISTS(SELECT 1 FROM unnest(p.proconfig) c WHERE c LIKE 'search_path=%') ORDER BY 1",
    )
    .fetch_all(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(
        unpinned,
        [
            "archava_billing.guard_gateway_invoice()",
            "archava_billing.guard_invoice_retention()",
            "archava_billing.guard_invoice_status()",
            "archava_billing.immutable_evidence()",
            "archava_purchasing.immutable_record()"
        ]
    );

    let output = output_account(&s).await;
    confirmed_code(
        &s,
        "VAT",
        "output",
        (11, 100),
        (1, 1),
        &output,
        "2020-01-01",
    )
    .await;
    let item = product(&s, "TEA", 15_000).await;
    let taxed = fulfilled(&s, order(&s, &item, 2, Some(("VAT", false)))).await;
    let plain = fulfilled(&s, order(&s, &item, 2, None)).await;
    let invoice = super::tax_sales::invoiced(&s, "PINNED-TAXED", &taxed).await;
    assert_eq!(invoice["outstandingMinor"], 33_300);
    let invoice = super::tax_sales::invoiced(&s, "PINNED-PLAIN", &plain).await;
    assert_eq!(invoice["outstandingMinor"], 30_000);
    // The guard still refuses a forged row, under the pinned path.
    let forged = planted_invoice(
        &s,
        &plain,
        (&json!({"resourceId":Uuid::new_v4()}), "VAT", 1, 11),
        None,
        "0",
    )
    .await
    .expect_err("an untaxed order invoiced with tax");
    assert_eq!(guard_message(&forged).0, "23514");
}

use super::purchase_sources::PurchaseScenario;
use super::tax_purchases as buys;

async fn tax_code_guard(f: &Fixture, enabled: bool) {
    let statement = format!(
        "ALTER TABLE archava_accounting.tax_codes {} TRIGGER tax_code_guard",
        if enabled { "ENABLE" } else { "DISABLE" }
    );
    sqlx::query(sqlx::AssertSqlSafe(statement))
        .execute(&f.admin_pool)
        .await
        .unwrap();
}
fn tax_code_request(book: &Value, code: &str, kind: &str, account: &Value) -> Value {
    input(
        json!({"type":"tax_code.create","bookId":book["bookId"],"code":code,"kind":kind,
        "rate":{"numerator":11,"denominator":100},
        "controlAccount":{"accountId":account["resourceId"],"expectedAccountRevision":account["revision"]},
        "effectiveFrom":"2020-01-01","reason":"Entered by the company from its own tax advice"}),
    )
}
/// A tax code entered by alice and confirmed by bob with the tax code guard
/// switched off by the database owner: the only way left to store a code
/// whose control account a posting policy names.
async fn planted_code(
    (f, app, alice, bob, book): (&Fixture, &Router, &Value, &Value, &Value),
    code: &str,
    kind: &str,
    account: &Value,
) -> Value {
    tax_code_guard(f, false).await;
    let (st, entered) = send(app, alice, &tax_code_request(book, code, kind, account)).await;
    assert_eq!(st, StatusCode::OK, "{entered}");
    let confirmed = ok(app,bob,json!({"type":"tax_code.confirm","id":entered["receipt"]["resourceId"],"expectedRevision":1,"reason":"Checked against the company's tax advice"})).await;
    tax_code_guard(f, true).await;
    confirmed
}
/// The posting policy of `table` and its reviewed audit snapshot, rewritten by
/// a superuser so its mappings spell every account id in upper case. The
/// policy commands store an id in canonical lower case, so only stored data
/// from elsewhere (an import, a restore, a repair) can carry another spelling.
/// For purchases it is planted after the goods were received: the stock layer
/// guard of Accounting 005 compares the inventory account as text too and
/// refuses a tracked receipt under such a policy.
async fn spelled_in_upper_case(f: &Fixture, table: &str, kind: &str) {
    let mut connection = f.admin_pool.acquire().await.unwrap();
    for statement in [
        "BEGIN".to_owned(),
        "SET LOCAL session_replication_role=replica".to_owned(),
        format!("UPDATE archava_accounting.{table} p SET mappings=(SELECT jsonb_agg(jsonb_set(v,'{{accountId}}',to_jsonb(upper(v->>'accountId'))) ORDER BY n) FROM jsonb_array_elements(p.mappings) WITH ORDINALITY x(v,n))"),
        format!("UPDATE archava_accounting.audit a SET snapshot=jsonb_set(a.snapshot,'{{record}}',(SELECT to_jsonb(p) FROM archava_accounting.{table} p WHERE p.tenant_id=a.tenant_id AND p.id=a.resource_id)) WHERE a.resource_kind='{kind}'"),
        // An order confirmed earlier pinned the snapshot; it is the policy its bills are held to.
        "UPDATE archava_accounting.purchase_origins o SET policy_snapshot=(SELECT a.snapshot->'record' FROM archava_accounting.audit a WHERE a.tenant_id=o.tenant_id AND a.resource_kind='purchase_policy' AND a.resource_id=o.policy_id AND a.revision=o.policy_revision)".to_owned(),
        "COMMIT".to_owned(),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(statement))
            .execute(&mut *connection)
            .await
            .unwrap();
    }
}
fn upper(mut policy: Value) -> Value {
    for mapping in policy["mappings"].as_array_mut().unwrap() {
        mapping["accountId"] = json!(mapping["accountId"].as_str().unwrap().to_uppercase());
    }
    policy
}

/// **Witness (P1, at capture, purchases): a posting policy whose stored
/// mappings spell an account id in upper case still names that account. A
/// supplier bill whose tax code has the policy's inventory account as its
/// control account is refused at capture with the policy-account reason, and
/// nothing is written.**
#[tokio::test]
async fn a_policy_account_spelled_in_upper_case_is_still_the_tax_control_account() {
    // Purchases: the inventory account is the control account of STOCK.
    let s = PurchaseScenario::new(false).await;
    let inventory = s.accounts[0]["resourceId"].as_str().unwrap();
    let stored = || async {
        sqlx::query_scalar::<_, String>(
            "SELECT mappings->0->>'accountId' FROM archava_accounting.purchase_policies",
        )
        .fetch_one(&s.f.admin_pool)
        .await
        .unwrap()
    };
    // The command stores the canonical spelling whatever it was given.
    ok(&s.app, &s.alice, upper(s.policy(0))).await;
    assert_eq!(stored().await, inventory);
    let po = s.draft_order(2).await;
    s.purchase(json!({"operation":"order.confirm","id":po["id"],"expectedRevision":1,"reason":"Confirm under the reviewed purchase policy"})).await;
    let line = s.order_line(&po).await;
    s.purchase(json!({"operation":"order.receive","id":po["id"],"expectedRevision":2,"reason":"All goods accepted","lines":[{"lineId":line,"accepted":2,"rejected":0}]})).await;
    spelled_in_upper_case(&s.f, "purchase_policies", "purchase_policy").await;
    assert_eq!(stored().await, inventory.to_uppercase());
    let pinned: String = sqlx::query_scalar(
        "SELECT policy_snapshot->'mappings'->0->>'accountId' FROM archava_accounting.purchase_origins",
    )
    .fetch_one(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(pinned, inventory.to_uppercase());
    planted_code(
        (&s.f, &s.app, &s.alice, &s.bob, &s.book),
        "STOCK",
        "input",
        &s.accounts[0],
    )
    .await;
    let date = buys::today(&s).await;
    let mixed = s
        .purchase(buys::claim(
            &s,
            &po,
            line,
            "UPPER-CASE",
            &date,
            2,
            Some(("STOCK", 1_540)),
        ))
        .await;
    let sources: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_accounting.purchase_sources")
            .fetch_one(&s.f.admin_pool)
            .await
            .unwrap();
    let (st, v) = call(
        &s.app,
        "POST",
        "/erp/purchasing/commands",
        input(buys::matching(&mixed, 1)),
        Some(&s.alice),
    )
    .await;
    assert_refused_as(st, &v, "supplier_bill_tax_control_account_in_policy");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM archava_accounting.purchase_sources")
            .fetch_one(&s.f.admin_pool)
            .await
            .unwrap(),
        sources
    );
}

/// **Witness (P1, at capture, sales): the same for an invoice whose tax code
/// has the policy's receivable account, spelled in upper case in the stored
/// policy, as its control account.**
#[tokio::test]
async fn an_invoice_policy_account_spelled_in_upper_case_is_still_the_tax_control_account() {
    let s = SourceFixture::new().await;
    let policy = json!({"type":"invoice_policy.set","bookId":s.book["bookId"],"expectedRevision":0,"currencyCatalogue":"SIX-2026-09-17","sourceUnit":"whole_idr","mappings":[
        {"role":"receivable","accountId":s.ar["resourceId"],"expectedAccountRevision":1},
        {"role":"revenue","accountId":s.revenue["resourceId"],"expectedAccountRevision":1},
        {"role":"payment_clearing","accountId":s.clearing["resourceId"],"expectedAccountRevision":1}],"reason":"Explicit future invoice unit contract"});
    ok(&s.app, &s.alice, upper(policy)).await;
    spelled_in_upper_case(&s.f, "invoice_policies", "invoice_policy").await;
    let stored: String = sqlx::query_scalar(
        "SELECT mappings->0->>'accountId' FROM archava_accounting.invoice_policies",
    )
    .fetch_one(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(stored, s.ar["resourceId"].as_str().unwrap().to_uppercase());
    planted_code(
        (&s.f, &s.app, &s.alice, &s.bob, &s.book),
        "VAT",
        "output",
        &s.ar,
    )
    .await;
    let item = product(&s, "TEA", 15_000).await;
    let o = fulfilled(&s, order(&s, &item, 2, Some(("VAT", false)))).await;
    let invoice = super::tax_sales::invoiced(&s, "UPPER-CASE", &o).await;
    let (st, v) = super::tax_sales::bill(&s,json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Issue against a policy account"})).await;
    assert_refused_as(st, &v, "invoice_tax_control_account_in_policy");
    assert_eq!(
        super::tax_sales::count(&s.f, "archava_accounting.invoice_sources").await,
        0
    );
    assert_eq!(invoice_row(&s, &invoice).await["status"], "draft");
}

/// **Witness (P1, at the source): a tax code is not created or confirmed with
/// a control account that the book's invoice, purchase or stock posting
/// policy names, and a posting policy is not created or revised to name the
/// control account of an entered, confirmed or retired tax code version of
/// its book. A withdrawn version does not count.**
#[tokio::test]
async fn a_tax_control_account_and_a_posting_policy_account_exclude_each_other() {
    let s = PurchaseScenario::new(true).await;
    let codes = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM archava_accounting.tax_codes")
            .fetch_one(&s.f.admin_pool)
            .await
            .unwrap()
    };
    // A tax code on a purchase policy account: the inventory account for an
    // input code, the payable account for an output code.
    for (code, kind, account) in [
        ("STOCK", "input", &s.accounts[0]),
        ("OWED", "output", &s.accounts[2]),
    ] {
        let (st, v) = send(
            &s.app,
            &s.alice,
            &tax_code_request(&s.book, code, kind, account),
        )
        .await;
        assert_refused_as(st, &v, "tax_code_control_account_in_posting_policy");
    }
    assert_eq!(codes().await, 0);

    // A purchase policy on the control account of an entered tax code.
    let vat_in = buys::input_account(&s, "VAT-IN").await;
    let (st, entered) = send(
        &s.app,
        &s.alice,
        &tax_code_request(&s.book, "VAT-IN", "input", &vat_in),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{entered}");
    let entered = entered["receipt"].clone();
    let mut revised = s.policy(1);
    revised["mappings"][0]["accountId"] = vat_in["resourceId"].clone();
    let (st, v) = send(&s.app, &s.alice, &input(revised.clone())).await;
    assert_refused_as(st, &v, "posting_policy_account_is_tax_control_account");
    // The same account spelled in upper case is the same account.
    let (st, v) = send(&s.app, &s.alice, &input(upper(revised.clone()))).await;
    assert_refused_as(st, &v, "posting_policy_account_is_tax_control_account");
    // An invoice policy on it: an input tax account and a receivable account
    // are both operating assets.
    let revenue = account(&s.app, &s.alice, &s.book, "REVENUE", "revenue", "operating").await;
    let clearing = account(&s.app, &s.alice, &s.book, "CLEARING", "asset", "none").await;
    let receivable = account(&s.app, &s.alice, &s.book, "AR", "asset", "operating").await;
    let invoice_policy = |ar: &Value| {
        input(
            json!({"type":"invoice_policy.set","bookId":s.book["bookId"],"expectedRevision":0,"currencyCatalogue":"SIX-2026-09-17","sourceUnit":"whole_idr","mappings":[
        {"role":"receivable","accountId":ar["resourceId"],"expectedAccountRevision":1},
        {"role":"revenue","accountId":revenue["resourceId"],"expectedAccountRevision":1},
        {"role":"payment_clearing","accountId":clearing["resourceId"],"expectedAccountRevision":1}],"reason":"Explicit future invoice unit contract"}),
        )
    };
    let (st, v) = send(&s.app, &s.alice, &invoice_policy(&vat_in)).await;
    assert_refused_as(st, &v, "posting_policy_account_is_tax_control_account");
    let (st, v) = send(&s.app, &s.alice, &invoice_policy(&receivable)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // …and a tax code on an invoice policy account.
    let (st, v) = send(
        &s.app,
        &s.alice,
        &tax_code_request(&s.book, "ON-AR", "input", &receivable),
    )
    .await;
    assert_refused_as(st, &v, "tax_code_control_account_in_posting_policy");

    // Confirmation checks again. The owner plants a purchase policy that names
    // the entered code's account; the code can then not be confirmed.
    sqlx::query(
        "ALTER TABLE archava_accounting.purchase_policies DISABLE TRIGGER purchase_policy_guard",
    )
    .execute(&s.f.admin_pool)
    .await
    .unwrap();
    let planted = ok(&s.app, &s.alice, revised.clone()).await;
    sqlx::query(
        "ALTER TABLE archava_accounting.purchase_policies ENABLE TRIGGER purchase_policy_guard",
    )
    .execute(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(planted["revision"], 2);
    let (st, v) = send(&s.app,&s.bob,&input(json!({"type":"tax_code.confirm","id":entered["resourceId"],"expectedRevision":1,"reason":"Checked against the company's tax advice"}))).await;
    assert_refused_as(st, &v, "tax_code_control_account_in_posting_policy");
    // A policy revision that keeps naming the account is refused as well; one
    // that names the inventory account again is accepted, and the code confirms.
    let mut kept = s.policy(2);
    kept["mappings"][0]["accountId"] = vat_in["resourceId"].clone();
    let (st, v) = send(&s.app, &s.alice, &input(kept.clone())).await;
    assert_refused_as(st, &v, "posting_policy_account_is_tax_control_account");
    ok(&s.app, &s.alice, s.policy(2)).await;
    ok(&s.app,&s.bob,json!({"type":"tax_code.confirm","id":entered["resourceId"],"expectedRevision":1,"reason":"Checked against the company's tax advice"})).await;
    // A confirmed and then a retired version still hold the account…
    let mut again = s.policy(3);
    again["mappings"][0]["accountId"] = vat_in["resourceId"].clone();
    let (st, v) = send(&s.app, &s.alice, &input(again.clone())).await;
    assert_refused_as(st, &v, "posting_policy_account_is_tax_control_account");
    ok(&s.app,&s.alice,json!({"type":"tax_code.retire","id":entered["resourceId"],"expectedRevision":2,"effectiveTo":"2020-12-31","reason":"The company's rate changes"})).await;
    let (st, v) = send(&s.app, &s.alice, &input(again)).await;
    assert_refused_as(st, &v, "posting_policy_account_is_tax_control_account");
    // …a withdrawn one does not.
    let spare = buys::input_account(&s, "VAT-SPARE").await;
    let (st, withdrawn) = send(
        &s.app,
        &s.alice,
        &tax_code_request(&s.book, "SPARE", "input", &spare),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{withdrawn}");
    let mut free = s.policy(3);
    free["mappings"][0]["accountId"] = spare["resourceId"].clone();
    let (st, v) = send(&s.app, &s.alice, &input(free.clone())).await;
    assert_refused_as(st, &v, "posting_policy_account_is_tax_control_account");
    ok(&s.app,&s.alice,json!({"type":"tax_code.retire","id":withdrawn["receipt"]["resourceId"],"expectedRevision":1,"reason":"Entered by mistake"})).await;
    ok(&s.app, &s.alice, free).await;

    // The stock policy: its account is a cost of goods sold account, which no
    // tax code can use through the commands, so the rule is the database's.
    let cogs = account(
        &s.app,
        &s.alice,
        &s.book,
        "COGS",
        "cost_of_goods_sold",
        "operating",
    )
    .await;
    planted_code(
        (&s.f, &s.app, &s.alice, &s.bob, &s.book),
        "ON-COGS",
        "input",
        &cogs,
    )
    .await;
    let (st, v) = send(&s.app,&s.alice,&input(json!({"type":"stock_policy.set","bookId":s.book["bookId"],"expectedRevision":0,"currencyCatalogue":"SIX-2026-09-17","sourceUnit":"whole_idr","cogsAccount":{"accountId":cogs["resourceId"],"expectedAccountRevision":cogs["revision"]},"reason":"Explicit future original-cost recognition"}))).await;
    assert_refused_as(st, &v, "posting_policy_account_is_tax_control_account");
    let other = account(
        &s.app,
        &s.alice,
        &s.book,
        "COGS-2",
        "cost_of_goods_sold",
        "operating",
    )
    .await;
    ok(&s.app,&s.alice,json!({"type":"stock_policy.set","bookId":s.book["bookId"],"expectedRevision":0,"currencyCatalogue":"SIX-2026-09-17","sourceUnit":"whole_idr","cogsAccount":{"accountId":other["resourceId"],"expectedAccountRevision":other["revision"]},"reason":"Explicit future original-cost recognition"})).await;
    tax_code_guard(&s.f, false).await;
    let (st, on_stock) = send(
        &s.app,
        &s.alice,
        &tax_code_request(&s.book, "ON-COGS-2", "input", &other),
    )
    .await;
    tax_code_guard(&s.f, true).await;
    assert_eq!(st, StatusCode::OK, "{on_stock}");
    let (st, v) = send(&s.app,&s.bob,&input(json!({"type":"tax_code.confirm","id":on_stock["receipt"]["resourceId"],"expectedRevision":1,"reason":"Checked against the company's tax advice"}))).await;
    assert_refused_as(st, &v, "tax_code_control_account_in_posting_policy");
}

/// **Witness (P4): the reversal of a document is not reviewed before the
/// document itself is posted. The void of a supplier bill cannot be drafted
/// while the bill source has no posted journal, and a correction or a void of
/// an invoice cannot be drafted while its issue source has none; after the
/// original is posted both are drafted and posted as before. An issue without
/// an amount never has a journal and holds nothing back.**
#[tokio::test]
async fn a_reversal_is_not_drafted_before_its_original_document_is_posted() {
    let s = PurchaseScenario::new(true).await;
    let (po, line) = buys::received(&s, 2, "RECEIPT").await;
    let date = buys::today(&s).await;
    let document = s
        .purchase(buys::claim(&s, &po, line, "VOIDED-EARLY", &date, 2, None))
        .await;
    let bill = s.purchase(buys::matching(&document, 1)).await;
    let void = s.purchase(json!({"operation":"document.void","id":document["id"],"expectedRevision":2,"reason":"Correct the original supplier claim"})).await;
    let roles = |source: &Value, code: &str| {
        let accounts: Vec<_> = [1, 2].iter().map(|i: &usize| json!({"accountId":s.accounts[*i]["resourceId"],"expectedAccountRevision":1})).collect();
        input(
            json!({"type":"purchase_source.draft","sourceId":source["requestId"],"code":code,"accounts":accounts,"reason":"Review original goods, source date and account roles"}),
        )
    };
    let journals = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM archava_accounting.journals WHERE source_ref LIKE 'purchasing:%' AND code LIKE 'VOID%'")
            .fetch_one(&s.f.admin_pool)
            .await
            .unwrap()
    };
    let (st, v) = send(&s.app, &s.alice, &roles(&void, "VOID-EARLY")).await;
    assert_refused_as(st, &v, "purchase_void_before_bill_posted");
    // A drafted bill is not a posted bill.
    let (st, drafted) = send(&s.app, &s.alice, &roles(&bill, "BILL")).await;
    assert_eq!(st, StatusCode::OK, "{drafted}");
    let (st, v) = send(&s.app, &s.alice, &roles(&void, "VOID-EARLY")).await;
    assert_refused_as(st, &v, "purchase_void_before_bill_posted");
    assert_eq!(journals().await, 0);
    s.post(&drafted["receipt"]).await;
    let (st, reversed) = send(&s.app, &s.alice, &roles(&void, "VOID")).await;
    assert_eq!(st, StatusCode::OK, "{reversed}");
    s.post(&reversed["receipt"]).await;
    let payable: String = sqlx::query_scalar("SELECT coalesce(sum(l.debit_minor-l.credit_minor),0)::text FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id WHERE j.state='posted' AND l.account_id=$1::uuid")
        .bind(s.accounts[2]["resourceId"].as_str().unwrap())
        .fetch_one(&s.f.admin_pool)
        .await
        .unwrap();
    assert_eq!(payable, "0");

    // Sales.
    let s = SourceFixture::new().await;
    s.enable().await;
    let invoice = s.invoice("EARLY-CREDIT").await;
    let issue = s.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Issue"})).await;
    let credit = s.bill(json!({"type":"append_correction","invoiceId":invoice["invoiceId"],"amountMinor":-5000,"reason":"Reviewed customer credit"})).await;
    let (st, v) = send(&s.app, &s.alice, &s.source_draft(&credit, "CREDIT-EARLY")).await;
    assert_refused_as(st, &v, "invoice_correction_before_issue_posted");
    let second = s.invoice("EARLY-VOID").await;
    let second_issue = s.bill(json!({"type":"issue_invoice","invoiceId":second["invoiceId"],"expectedRevision":1,"reason":"Issue"})).await;
    let void = s.bill(json!({"type":"void_invoice","invoiceId":second["invoiceId"],"expectedRevision":2,"reason":"Issued in error"})).await;
    let (st, v) = send(&s.app, &s.alice, &s.source_draft(&void, "VOID-EARLY")).await;
    assert_refused_as(st, &v, "invoice_correction_before_issue_posted");
    // A drafted issue is not a posted issue.
    let (st, drafted) = send(&s.app, &s.alice, &s.source_draft(&second_issue, "ISSUE-2")).await;
    assert_eq!(st, StatusCode::OK, "{drafted}");
    let (st, v) = send(&s.app, &s.alice, &s.source_draft(&void, "VOID-EARLY")).await;
    assert_refused_as(st, &v, "invoice_correction_before_issue_posted");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM archava_accounting.journal_sources")
            .fetch_one(&s.f.admin_pool)
            .await
            .unwrap(),
        1
    );
    let (st, posted) = send(&s.app, &s.bob, &action("journal.post", &drafted["receipt"])).await;
    assert_eq!(st, StatusCode::OK, "{posted}");
    s.post_source(&void, "VOID").await;
    s.post_source(&issue, "ISSUE").await;
    s.post_source(&credit, "CREDIT").await;
    // An issue of zero has no journal to post; its correction is drafted.
    let zero = s.invoice_many_amount("ZERO", 0, 1, 1).await;
    s.bill(json!({"type":"issue_invoice","invoiceId":zero["invoiceId"],"expectedRevision":1,"reason":"Zero-value issue"})).await;
    let debit = s.bill(json!({"type":"append_correction","invoiceId":zero["invoiceId"],"amountMinor":700,"reason":"Reviewed customer charge"})).await;
    s.post_source(&debit, "ZERO-DEBIT").await;
}

/// **Witness (P3): a taxed supplier document whose goods and tax together are
/// more than the ledger can post is refused when the claim is captured and
/// when it is revised, with its own reason, instead of being stored and then
/// failing at the match. The largest total the ledger posts is accepted, and
/// an untaxed claim is bounded as before.**
#[tokio::test]
async fn a_taxed_claim_beyond_the_ledger_bound_is_refused_at_capture() {
    let s = PurchaseScenario::new(true).await;
    let vat_in = buys::input_account(&s, "VAT-IN").await;
    buys::confirmed_code(&s, "VAT-IN", "input", (11, 100), &vat_in, "2020-01-01").await;
    let (po, line) = buys::received(&s, 2, "RECEIPT").await;
    let date = buys::today(&s).await;
    // 85,000,000,000,000 whole Rupiah of goods post as 8.5e15 minor units,
    // inside the ledger bound of 9e15; with 9,350,000,000,000 of tax the
    // payable would be 9.435e15.
    let large = |reference: &str, tax: Option<i64>| {
        let mut command = json!({"operation":"document.capture","orderId":po["id"],"expectedOrderRevision":3,"supplierId":s.supplier["id"],"vendorReference":reference,"documentDate":date,"currency":"IDR","declaredTotal":85_000_000_000_000_i64,"lines":[{"lineId":line,"quantity":85_000,"unitCost":1_000_000_000}]});
        if let Some(tax) = tax {
            command["taxCode"] = json!("VAT-IN");
            command["taxMinor"] = json!(tax);
        }
        command
    };
    let attempt = |command: Value| async {
        call(
            &s.app,
            "POST",
            "/erp/purchasing/commands",
            input(command),
            Some(&s.alice),
        )
        .await
    };
    let documents = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM archava_purchasing.supplier_documents")
            .fetch_one(&s.f.admin_pool)
            .await
            .unwrap()
    };
    let (st, v) = attempt(large("BEYOND", Some(9_350_000_000_000))).await;
    assert_refused_as(st, &v, "supplier_document_total_beyond_ledger_bound");
    let (st, v) = attempt(large("ONE-BEYOND", Some(5_000_000_000_001))).await;
    assert_refused_as(st, &v, "supplier_document_total_beyond_ledger_bound");
    assert_eq!(documents().await, 0);
    // Exactly the bound, and the same goods without tax, are captured.
    let (st, at_bound) = attempt(large("AT-THE-BOUND", Some(5_000_000_000_000))).await;
    assert_eq!(st, StatusCode::OK, "{at_bound}");
    let (st, untaxed) = attempt(large("UNTAXED", None)).await;
    assert_eq!(st, StatusCode::OK, "{untaxed}");
    // A revision is held to the same bound.
    let small = s
        .purchase(buys::claim(
            &s,
            &po,
            line,
            "REVISED",
            &date,
            2,
            Some(("VAT-IN", 1_540)),
        ))
        .await;
    let revise = |tax: i64| json!({"operation":"document.revise","id":small["id"],"expectedRevision":1,"expectedOrderRevision":3,"documentDate":date,"currency":"IDR","declaredTotal":85_000_000_000_000_i64,"lines":[{"lineId":line,"quantity":85_000,"unitCost":1_000_000_000}],"taxCode":"VAT-IN","taxMinor":tax,"reason":"Corrected supplier claim"});
    let (st, v) = attempt(revise(9_350_000_000_000)).await;
    assert_refused_as(st, &v, "supplier_document_total_beyond_ledger_bound");
    let (st, v) = attempt(revise(5_000_000_000_000)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // The database holds the rule for the serving role as well.
    let mut tx = archava_adapters::rls::TenantTransaction::begin(
        &s.f.runtime_pool,
        "archava_purchasing",
        "purchasing",
        archava_core::tenant::TenantId::new("acme").unwrap(),
    )
    .await
    .unwrap();
    let error = sqlx::query("UPDATE archava_purchasing.supplier_documents SET tax_minor=tax_minor+1,revision=revision+1 WHERE id=$1::uuid")
        .bind(at_bound["receipt"]["id"].as_str().unwrap())
        .execute(tx.checked_executor().await.unwrap())
        .await
        .expect_err("one unit beyond the bound");
    assert_eq!(
        guard_message(&error),
        (
            "23514".to_owned(),
            "A taxed supplier document total exceeds the amount the ledger can post".to_owned()
        )
    );
    tx.rollback().await.unwrap();
}

/// **Witness (S4, P2): the host is ready only while the tax guards outside
/// Accounting are installed and enabled: the invoice tax guard, the correction
/// tax guard, the invoice tax identifier guard, the two deferred provenance
/// triggers that recompute an invoice's tax from its order, the sales order
/// tax guard and the supplier document tax guard.**
#[tokio::test]
async fn the_host_is_not_ready_without_the_tax_guards_outside_accounting() {
    let (f, _app, _alice, _bob, _c) = setup().await;
    let ready = || async {
        AppState::try_new_all(
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
        .await
    };
    for (table, trigger) in [
        ("archava_billing.invoices", "invoice_tax_guard"),
        (
            "archava_billing.invoice_corrections",
            "correction_tax_guard",
        ),
        ("archava_billing.invoices", "invoice_tax_identifiers_guard"),
        ("archava_billing.invoices", "source_invoice_complete"),
        (
            "archava_billing.invoice_lines",
            "source_invoice_lines_complete",
        ),
        ("archava_commerce.orders", "a_tax_sales"),
        (
            "archava_purchasing.supplier_documents",
            "supplier_document_tax_guard",
        ),
    ] {
        let toggle = |state: &str| format!("ALTER TABLE {table} {state} TRIGGER {trigger}");
        sqlx::query(sqlx::AssertSqlSafe(toggle("DISABLE")))
            .execute(&f.owner_pool)
            .await
            .unwrap();
        let failure = match ready().await {
            Ok(_) => panic!("the host is still ready with {trigger} disabled"),
            Err(failure) => format!("{failure:?}"),
        };
        assert!(
            failure.contains("invoice source guard coverage is incomplete"),
            "{trigger}: {failure}"
        );
        let complete: bool =
            sqlx::query_scalar("SELECT archava_accounting.tax_document_guards_complete()")
                .fetch_one(&f.runtime_pool)
                .await
                .unwrap();
        assert!(!complete, "{trigger}");
        sqlx::query(sqlx::AssertSqlSafe(toggle("ENABLE")))
            .execute(&f.owner_pool)
            .await
            .unwrap();
    }
    // A guard that fires only as a replica, or one re-pointed at another
    // function, is not the guard either.
    sqlx::query("ALTER TABLE archava_commerce.orders ENABLE REPLICA TRIGGER a_tax_sales")
        .execute(&f.owner_pool)
        .await
        .unwrap();
    assert!(ready().await.is_err());
    sqlx::query("ALTER TABLE archava_commerce.orders ENABLE TRIGGER a_tax_sales")
        .execute(&f.owner_pool)
        .await
        .unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT archava_accounting.tax_document_guards_complete() AND archava_accounting.tax_guards_complete()")
            .fetch_one(&f.runtime_pool)
            .await
            .unwrap()
    );
    let _: Router = app(&f).await;
}
