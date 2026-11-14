//! Real router + disposable PostgreSQL financial controls. Synthetic books are explicit.
#[path = "erp_accounting/bank.rs"]
mod bank;
#[path = "erp_accounting/bank_periods.rs"]
mod bank_periods;
#[path = "erp_accounting/clearing.rs"]
mod clearing;
#[path = "erp_accounting/close.rs"]
mod close;
mod common;
#[path = "erp_accounting/invoice_sources.rs"]
mod invoice_sources;
#[path = "erp_accounting/openings.rs"]
mod openings;
#[path = "erp_accounting/payables.rs"]
mod payables;
#[path = "erp_accounting/purchase_sources.rs"]
mod purchase_sources;
#[path = "erp_accounting/stock_sources.rs"]
mod stock_sources;
#[path = "erp_accounting/subledger.rs"]
mod subledger;
#[path = "erp_accounting/tax.rs"]
mod tax;
#[path = "erp_accounting/tax_documents.rs"]
mod tax_documents;
#[path = "erp_accounting/tax_posting.rs"]
mod tax_posting;
#[path = "erp_accounting/tax_purchases.rs"]
mod tax_purchases;
#[path = "erp_accounting/tax_report.rs"]
mod tax_report;
#[path = "erp_accounting/tax_sales.rs"]
mod tax_sales;
#[path = "erp_accounting/terms.rs"]
mod terms;
use archava_host::{router, router::AllReadiness, AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::{http::StatusCode, Router};
use common::{call, login, HASH};
use serde_json::{json, Value};
use uuid::Uuid;

fn ledger() -> Ledger {
    Ledger {
        namespace: "archava_commerce",
        ledger: "Commerce",
        tables: &[
            "memberships",
            "products",
            "stock_balances",
            "orders",
            "order_lines",
            "allocations",
            "commands",
            "movements",
            "returns",
            "audit",
            "approvals",
        ],
    }
}
async fn app(f: &Fixture) -> Router {
    router(
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
        .unwrap(),
    )
}
async fn setup() -> (Fixture, Router, Value, Value, Value) {
    let f = provision().await;
    for actor in ["alice", "bob", "reader", "assigned", "service-seat"] {
        sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES($1,$2,$3,true,1)").bind(actor).bind(format!("{actor}@archava.test")).bind(HASH).execute(&f.admin_pool).await.unwrap();
        sqlx::query("INSERT INTO archava_organizations.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme',$1,'human',ARRAY['organizations.read','organizations.write','companies.read','companies.write'],$2)").bind(actor).bind(if actor=="assigned"{"assigned"}else{"all"}).execute(&f.admin_pool).await.unwrap();
        let grants = if actor == "reader" {
            vec!["books.read", "reports.read"]
        } else {
            archava_domain::erp::accounting::PERMISSIONS.to_vec()
        };
        sqlx::query("INSERT INTO archava_accounting.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme',$1,$2,$3,$4)").bind(actor).bind(if actor=="service-seat"{"service"}else{"human"}).bind(grants).bind(if actor=="assigned"{"assigned"}else{"all"}).execute(&f.admin_pool).await.unwrap();
    }
    let app = app(&f).await;
    let a = login(&app, "alice").await;
    let b = login(&app, "bob").await;
    let c = company(&app, &a, "A").await;
    (f, app, a, b, c)
}
async fn company(app: &Router, s: &Value, code: &str) -> Value {
    let (status,o)=call(app,"POST","/erp/organizations/commands",json!({"tenant":"acme","mutation":{"requestId":Uuid::new_v4(),"type":"create_organization","data":{"name":format!("Company {code}"),"legalName":format!("Company {code} Official")}}}),Some(s)).await;
    assert_eq!(status, StatusCode::OK, "{o}");
    let (status,c)=call(app,"POST","/erp/organizations/commands",json!({"tenant":"acme","mutation":{"requestId":Uuid::new_v4(),"type":"company.create_from_organization","data":{"code":code,"organizationId":o["receipt"]["organizationId"],"expectedSourceRevision":1,"reason":"Explicit synthetic legal entity"}}}),Some(s)).await;
    assert_eq!(status, StatusCode::OK, "{c}");
    c["receipt"].clone()
}
fn input(command: Value) -> Value {
    json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":command})
}
/// A refusal whose database guard is catalogued
/// (`config/refusal-reasons.v1.json`): the same 409 `conflict` as any other
/// refusal, which now also names its reason.
fn assert_refused_as(status: StatusCode, body: &Value, code: &str) {
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["decision"], "refused", "{body}");
    assert_eq!(body["reason"], "conflict", "{body}");
    assert_eq!(body["detail"]["code"], code, "{body}");
    let refusal = archava_domain::erp::refusals::for_code(code).expect("a catalogued code");
    assert_eq!(body["message"], refusal.message.as_str(), "{body}");
    assert_eq!(
        body["detail"]["message"],
        refusal.message.as_str(),
        "{body}"
    );
    assert_eq!(
        body["detail"]["messageId"],
        refusal.message_id.as_str(),
        "{body}"
    );
}
async fn send(app: &Router, s: &Value, body: &Value) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/erp/accounting/commands",
        body.clone(),
        Some(s),
    )
    .await
}
async fn ok(app: &Router, s: &Value, command: Value) -> Value {
    let (st, v) = send(app, s, &input(command)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["receipt"].clone()
}
fn create(c: &Value, currency: &str) -> Value {
    input(
        json!({"type":"book.create","companyId":c["companyId"],"expectedCompanyRevision":1,"code":format!("BOOK-{currency}"),"name":"Explicit synthetic official book","currency":currency,"sourceAuthority":"native","independentPosting":true,"reason":"Reviewed book setup; no historical import"}),
    )
}
/// Lets the database owner switch the book currency guard (Accounting 015) off
/// and on. A non-IDR book for a company that already trades is refused through
/// every runtime path, so tests of the guards behind it plant that state.
async fn book_currency_guard(f: &Fixture, enabled: bool) {
    let statement = format!(
        "ALTER TABLE archava_accounting.books {} TRIGGER a_currency_book",
        if enabled { "ENABLE" } else { "DISABLE" }
    );
    sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
        .execute(&f.admin_pool)
        .await
        .unwrap();
}
async fn book(app: &Router, a: &Value, c: &Value) -> Value {
    let (st, v) = send(app, a, &create(c, "USD")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["receipt"].clone()
}
async fn account(app: &Router, a: &Value, b: &Value, code: &str, class: &str, cash: &str) -> Value {
    ok(app,a,json!({"type":"account.create","bookId":b["bookId"],"code":code,"name":code,"class":class,"cashRole":cash,"reason":"Reviewed account classification"})).await
}
async fn period(app: &Router, a: &Value, b: &Value) -> Value {
    ok(app,a,json!({"type":"period.open","bookId":b["bookId"],"code":"2026-11","startDate":"2026-11-01","endDate":"2026-11-30","reason":"Open test financial period"})).await
}
fn line(account: &Value, d: &str, c: &str) -> Value {
    json!({"accountId":account["resourceId"],"expectedAccountRevision":account["revision"],"debitMinor":d,"creditMinor":c,"memo":"Source control amount"})
}
fn draft(b: &Value, code: &str, lines: Vec<Value>) -> Value {
    input(
        json!({"type":"journal.draft","bookId":b["bookId"],"code":code,"date":"2026-11-07","sourceRef":format!("Reviewed source {code}"),"memo":code,"lines":lines,"reason":"Review synthetic source to book control"}),
    )
}
fn action(op: &str, r: &Value) -> Value {
    input(
        json!({"type":op,"id":r["resourceId"],"expectedRevision":r["revision"],"reason":"Explicit financial review"}),
    )
}
async fn post(
    app: &Router,
    a: &Value,
    b: &Value,
    book: &Value,
    code: &str,
    lines: Vec<Value>,
) -> Value {
    let (st, r) = send(app, a, &draft(book, code, lines)).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (st, v) = send(app, b, &action("journal.post", &r["receipt"])).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["receipt"].clone()
}
async fn report(app: &Router, s: &Value, b: &Value) -> Value {
    let (st, r) = call(
        app,
        "GET",
        &format!(
            "/erp/accounting/reports?tenant=acme&bookId={}&from=2026-11-01&to=2026-11-30",
            b["bookId"].as_str().unwrap()
        ),
        Value::Null,
        Some(s),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    r
}
async fn snapshot(app: &Router, s: &Value, c: &Value) -> (StatusCode, Value) {
    call(
        app,
        "GET",
        &format!(
            "/erp/accounting/workspace?tenant=acme&companyId={}",
            c["companyId"].as_str().unwrap()
        ),
        Value::Null,
        Some(s),
    )
    .await
}
async fn status(app: &Router, s: &Value, r: &Value) -> (StatusCode, Value) {
    call(
        app,
        "GET",
        &format!(
            "/erp/accounting/commands/status?tenant=acme&requestId={}",
            r["requestId"].as_str().unwrap()
        ),
        Value::Null,
        Some(s),
    )
    .await
}

#[tokio::test]
async fn explicit_books_pin_source_company_currency_and_no_implicit_historical_posting() {
    let (f, app, a, _b, c) = setup().await;
    assert_eq!(snapshot(&app, &a, &c).await.1["books"], json!([]));
    let request = create(&c, "IDR");
    let (st, r) = send(&app, &a, &request).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let w = snapshot(&app, &a, &c).await.1;
    assert_eq!(w["books"][0]["currency"], "IDR");
    assert_eq!(w["books"][0]["minorUnits"], 2);
    assert_eq!(w["journals"], json!([]));
    assert_eq!(
        send(&app, &a, &request).await.1["receipt"]["replayed"],
        true
    );
    assert_eq!(
        send(&app, &a, &create(&c, "JPY")).await.0,
        StatusCode::CONFLICT
    );
    let other = company(&app, &a, "EXTERNAL").await;
    let mut external = create(&other, "JPY");
    external["command"]["code"] = json!("EXTERNAL");
    external["command"]["sourceAuthority"] = json!("external");
    external["command"]["externalSystem"] = json!("EXISTING-BOOK");
    let (st, e) = send(&app, &a, &external).await;
    assert_eq!(st, StatusCode::OK, "{e}");
    assert_eq!(
        snapshot(&app, &a, &other).await.1["books"][0]["externalConnectorReady"],
        false
    );
    let(st,_)=send(&app,&a,&input(json!({"type":"account.create","bookId":e["receipt"]["bookId"],"code":"CASH","name":"Cash","class":"asset","cashRole":"cash","reason":"Cannot post native into external book"}))).await;
    assert_eq!(st, StatusCode::CONFLICT);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_accounting.journals")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

/// Each of the four company trade tables refuses a non-IDR book on its own. A
/// sales order, purchase order or invoice cannot exist without a product through
/// the commands, so the owner plants one bare row per company.
#[tokio::test]
async fn a_non_idr_book_is_refused_once_the_company_owns_a_trade_record() {
    let (f, app, a, _b, untraded) = setup().await;
    let planted = [
        ("PRODUCT", "INSERT INTO archava_commerce.products(tenant_id,id,sku,title,unit,track_stock,price,company_id,company_revision,company_code,company_name) SELECT c.tenant_id,$1,'PLANTED','Planted product','unit',false,100,c.id,c.revision,c.code,c.legal_name FROM archava_organizations.companies c WHERE c.id=$2"),
        ("SALE", "INSERT INTO archava_commerce.orders(tenant_id,id,owner_id,customer_name,total_estimate,company_id,company_revision,company_code,company_name) SELECT c.tenant_id,$1,'alice','Planted sale',0,c.id,c.revision,c.code,c.legal_name FROM archava_organizations.companies c WHERE c.id=$2"),
        ("PURCHASE", "INSERT INTO archava_purchasing.orders(tenant_id,id,owner_id,supplier_id,supplier_code,supplier_name,total_estimate,company_id,company_revision,company_code,company_name) SELECT c.tenant_id,$1,'alice',$1,'PLANTED','Planted supplier',0,c.id,c.revision,c.code,c.legal_name FROM archava_organizations.companies c WHERE c.id=$2"),
        ("INVOICE", "INSERT INTO archava_billing.invoices(tenant_id,id,invoice_code,customer_ref,currency,consumer_ref,owner_id,company_id,company_revision,company_code,company_name) SELECT c.tenant_id,$1,'PLANTED','Planted customer','IDR','planted','alice',c.id,c.revision,c.code,c.legal_name FROM archava_organizations.companies c WHERE c.id=$2"),
    ];
    for (code, statement) in planted {
        let c = company(&app, &a, code).await;
        let id = Uuid::parse_str(c["companyId"].as_str().unwrap()).unwrap();
        // Replica mode skips the unrelated triggers and foreign keys of a bare row.
        let mut owner = f.admin_pool.begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role='replica'")
            .execute(&mut *owner)
            .await
            .unwrap();
        let rows = sqlx::query(statement)
            .bind(Uuid::new_v4())
            .bind(id)
            .execute(&mut *owner)
            .await
            .unwrap()
            .rows_affected();
        assert_eq!(rows, 1, "{code}");
        owner.commit().await.unwrap();

        for currency in ["USD", "JPY"] {
            let mut refused = create(&c, currency);
            refused["command"]["code"] = json!(format!("{code}-{currency}"));
            let (st, v) = send(&app, &a, &refused).await;
            assert_eq!(st, StatusCode::CONFLICT, "{code} {currency} {v}");
            assert_refused_as(st, &v, "book_currency_conflicts_with_existing_trade");
            assert!(
                v["message"]
                    .as_str()
                    .unwrap()
                    .starts_with("This company already has products, orders or invoices in IDR"),
                "{v}"
            );
            let written: (i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM archava_accounting.books WHERE company_id=$1),(SELECT count(*) FROM archava_accounting.commands WHERE request_id=$2::uuid)")
                .bind(id)
                .bind(refused["requestId"].as_str().unwrap())
                .fetch_one(&f.admin_pool)
                .await
                .unwrap();
            assert_eq!(written, (0, 0), "{code} {currency}");
        }
        // The command's reason above is the catalogue entry of this guard message.
        let direct = sqlx::query("INSERT INTO archava_accounting.books(tenant_id,id,company_id,company_revision,company_code,company_name,owner_id,code,name,currency,minor_units,currency_catalogue,source_authority,independent_posting) SELECT c.tenant_id,$1,c.id,c.revision,c.code,c.legal_name,'alice','DIRECT','Direct row','USD',2,'SIX-2026-09-17','native',true FROM archava_organizations.companies c WHERE c.id=$2")
            .bind(Uuid::new_v4())
            .bind(id)
            .execute(&f.admin_pool)
            .await
            .expect_err("the database refuses the book");
        let direct = direct.as_database_error().expect("a database refusal");
        assert_eq!(direct.code().as_deref(), Some("23514"), "{direct}");
        assert_eq!(
            direct.message(),
            "A company that already trades in IDR cannot open a book in another currency"
        );

        let mut idr = create(&c, "IDR");
        idr["command"]["code"] = json!(format!("{code}-IDR"));
        let (st, v) = send(&app, &a, &idr).await;
        assert_eq!(st, StatusCode::OK, "{code} {v}");
        assert_eq!(
            snapshot(&app, &a, &c).await.1["books"][0]["currency"],
            "IDR"
        );
    }
    // A company without trade records opens its non-IDR book as before.
    let (st, v) = send(&app, &a, &create(&untraded, "USD")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // A book never changes afterwards, so the insert is the only path.
    for statement in [
        "UPDATE archava_accounting.books SET currency='EUR'",
        "DELETE FROM archava_accounting.books",
    ] {
        let refused = sqlx::query(statement)
            .execute(&f.admin_pool)
            .await
            .expect_err("books are immutable");
        assert_eq!(
            refused.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
}

#[tokio::test]
async fn posted_purchase_and_sale_controls_produce_exact_trial_profit_balance_and_cash_reports() {
    let (_f, app, a, b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    period(&app, &a, &book).await;
    let cash = account(&app, &a, &book, "BANK", "asset", "cash").await;
    let equity = account(&app, &a, &book, "EQUITY", "equity", "financing").await;
    let stock = account(&app, &a, &book, "INVENTORY", "asset", "operating").await;
    let ap = account(&app, &a, &book, "AP", "liability", "operating").await;
    let ar = account(&app, &a, &book, "AR", "asset", "operating").await;
    let sales = account(&app, &a, &book, "SALES", "revenue", "operating").await;
    let cost = account(&app, &a, &book, "COGS", "cost_of_goods_sold", "operating").await;
    post(
        &app,
        &a,
        &b,
        &book,
        "OPEN",
        vec![line(&cash, "1000000", "0"), line(&equity, "0", "1000000")],
    )
    .await;
    post(
        &app,
        &a,
        &b,
        &book,
        "BUY",
        vec![line(&stock, "1000000", "0"), line(&ap, "0", "1000000")],
    )
    .await;
    post(
        &app,
        &a,
        &b,
        &book,
        "PAY",
        vec![line(&ap, "1000000", "0"), line(&cash, "0", "1000000")],
    )
    .await;
    post(
        &app,
        &a,
        &b,
        &book,
        "SHIP",
        vec![line(&cost, "400000", "0"), line(&stock, "0", "400000")],
    )
    .await;
    post(
        &app,
        &a,
        &b,
        &book,
        "SELL",
        vec![line(&ar, "600000", "0"), line(&sales, "0", "600000")],
    )
    .await;
    post(
        &app,
        &a,
        &b,
        &book,
        "COLLECT",
        vec![line(&cash, "600000", "0"), line(&ar, "0", "600000")],
    )
    .await;
    let r = report(&app, &a, &book).await;
    assert_eq!(r["trialBalance"]["debitMinor"], "4600000");
    assert_eq!(r["trialBalance"]["balanced"], true);
    assert_eq!(r["profitAndLoss"]["netProfitMinor"], "200000");
    assert_eq!(r["balanceSheet"]["assetsMinor"], "1200000");
    assert_eq!(r["balanceSheet"]["totalEquityMinor"], "1200000");
    assert_eq!(r["balanceSheet"]["balanced"], true);
    assert_eq!(r["cashFlow"]["operatingMinor"], "-400000");
    assert_eq!(r["cashFlow"]["financingMinor"], "1000000");
    assert_eq!(r["cashFlow"]["netChangeMinor"], "600000");
    assert_eq!(r["cashFlow"]["unclassifiedJournals"], 0);
    assert_eq!(r["coverage"]["postedJournalCount"], 6);
    assert!(r["observedAt"].is_string());
    // This is a GL control oracle. Operational AP/AR source bridges remain independently tested work.
}

#[tokio::test]
async fn independent_posting_replay_current_grants_scope_and_company_revocation_are_enforced() {
    let (f, app, a, b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    period(&app, &a, &book).await;
    let cash = account(&app, &a, &book, "BANK", "asset", "cash").await;
    let equity = account(&app, &a, &book, "EQUITY", "equity", "financing").await;
    let (st, r) = send(
        &app,
        &a,
        &draft(
            &book,
            "ONE",
            vec![line(&cash, "100", "0"), line(&equity, "0", "100")],
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let posting = action("journal.post", &r["receipt"]);
    assert_eq!(send(&app, &a, &posting).await.0, StatusCode::FORBIDDEN);
    let (x, y) = tokio::join!(send(&app, &b, &posting), send(&app, &b, &posting));
    assert_eq!(x.0, StatusCode::OK, "{}", x.1);
    assert_eq!(y.0, StatusCode::OK, "{}", y.1);
    assert_ne!(x.1["receipt"]["replayed"], y.1["receipt"]["replayed"]);
    assert_eq!(
        report(&app, &a, &book).await["coverage"]["postedJournalCount"],
        1
    );
    assert_eq!(status(&app, &b, &posting).await.1["decision"], "committed");
    for actor in ["assigned", "service-seat"] {
        let peer = login(&app, actor).await;
        assert!(matches!(
            snapshot(&app, &peer, &c).await.0,
            StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
        ));
    }
    let reader = login(&app, "reader").await;
    assert_eq!(snapshot(&app, &reader, &c).await.0, StatusCode::OK);
    assert_eq!(send(&app, &reader, &posting).await.0, StatusCode::FORBIDDEN);
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=ARRAY['books.read'] WHERE principal_id='bob'").execute(&f.admin_pool).await.unwrap();
    assert_eq!(send(&app, &b, &posting).await.0, StatusCode::FORBIDDEN);
    assert_eq!(status(&app, &b, &posting).await.0, StatusCode::FORBIDDEN);
    sqlx::query("UPDATE archava_organizations.memberships SET permissions=ARRAY['organizations.read'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    assert_eq!(snapshot(&app, &a, &c).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn journal_correction_keeps_original_immutable_and_reverses_in_a_new_open_period() {
    let (f, app, a, b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    let p = period(&app, &a, &book).await;
    let cash = account(&app, &a, &book, "BANK", "asset", "cash").await;
    let equity = account(&app, &a, &book, "EQUITY", "equity", "financing").await;
    let original = post(
        &app,
        &a,
        &b,
        &book,
        "OPEN",
        vec![line(&cash, "12345", "0"), line(&equity, "0", "12345")],
    )
    .await;
    let close = action("period.close", &p);
    assert_eq!(send(&app, &a, &close).await.0, StatusCode::OK);
    assert_eq!(
        send(
            &app,
            &a,
            &draft(
                &book,
                "CLOSED",
                vec![line(&cash, "1", "0"), line(&equity, "0", "1")]
            )
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    ok(&app,&a,json!({"type":"period.open","bookId":book["bookId"],"code":"2026-12","startDate":"2026-12-01","endDate":"2026-12-31","reason":"Correction period"})).await;
    let reverse = input(
        json!({"type":"journal.reverse","id":original["resourceId"],"expectedRevision":2,"code":"REV-OPEN","date":"2026-12-01","sourceRef":"Reviewed reversal OPEN","reason":"Correct mistaken opening without erasing it"}),
    );
    let (st, r) = send(&app, &a, &reverse).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(
        send(&app, &b, &action("journal.post", &r["receipt"]))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        report(&app, &a, &book).await["balanceSheet"]["assetsMinor"],
        "12345"
    );
    let (st, r) = call(
        &app,
        "GET",
        &format!(
            "/erp/accounting/reports?tenant=acme&bookId={}&from=2026-11-01&to=2026-12-31",
            book["bookId"].as_str().unwrap()
        ),
        Value::Null,
        Some(&a),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["balanceSheet"]["assetsMinor"], "0");
    assert_eq!(
        send(&app, &a, &action("journal.void", &original)).await.0,
        StatusCode::CONFLICT
    );
    let mut again = reverse.clone();
    again["requestId"] = json!(Uuid::new_v4());
    again["command"]["code"] = json!("REV-AGAIN");
    again["command"]["sourceRef"] = json!("Second reversal");
    assert_eq!(send(&app, &a, &again).await.0, StatusCode::CONFLICT);
    let state: String =
        sqlx::query_scalar("SELECT state FROM archava_accounting.journals WHERE id=$1")
            .bind(Uuid::parse_str(original["resourceId"].as_str().unwrap()).unwrap())
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(state, "posted");
    let reader = login(&app, "reader").await;
    let reopen = input(
        json!({"type":"period.reopen","id":p["resourceId"],"expectedRevision":2,"reason":"Reviewed controlled reopening"}),
    );
    assert_eq!(send(&app, &reader, &reopen).await.0, StatusCode::FORBIDDEN);
    assert_eq!(send(&app, &a, &reopen).await.0, StatusCode::OK);
}

#[tokio::test]
async fn stale_accounts_unbalanced_lines_overlap_and_drafts_block_financial_finalization() {
    let (_f, app, a, b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    let p = period(&app, &a, &book).await;
    let cash = account(&app, &a, &book, "BANK", "asset", "cash").await;
    let equity = account(&app, &a, &book, "EQUITY", "equity", "financing").await;
    let (st, overlap) = send(&app,&a,&input(json!({"type":"period.open","bookId":book["bookId"],"code":"OVERLAP","startDate":"2026-11-30","endDate":"2026-12-31","reason":"Overlap refuses"}))).await;
    assert_refused_as(st, &overlap, "period_overlaps_existing");
    assert_eq!(
        send(
            &app,
            &a,
            &draft(
                &book,
                "BAD",
                vec![line(&cash, "100", "0"), line(&equity, "0", "99")]
            )
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (st, r) = send(
        &app,
        &a,
        &draft(
            &book,
            "STALE",
            vec![line(&cash, "100", "0"), line(&equity, "0", "100")],
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    // A draft journal blocks the close, and the refusal says so; sending the
    // same request again is refused for the same reason.
    let close = action("period.close", &p);
    for _ in 0..2 {
        let (st, refused) = send(&app, &a, &close).await;
        assert_refused_as(st, &refused, "period_close_draft_journals");
    }
    let mut inactive = action("account.set_active", &cash);
    inactive["command"]["active"] = json!(false);
    assert_eq!(send(&app, &a, &inactive).await.0, StatusCode::OK);
    assert_eq!(
        send(&app, &b, &action("journal.post", &r["receipt"]))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(&app, &a, &action("journal.void", &r["receipt"]))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        send(&app, &a, &action("period.close", &p)).await.0,
        StatusCode::OK
    );
    assert_eq!(
        report(&app, &a, &book).await["coverage"]["postedJournalCount"],
        0
    );
}

#[tokio::test]
async fn totals_exceeding_javascript_integer_precision_are_exact_strings_in_all_reports() {
    let (_f, app, a, b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    period(&app, &a, &book).await;
    let cash = account(&app, &a, &book, "BANK", "asset", "cash").await;
    let equity = account(&app, &a, &book, "EQUITY", "equity", "financing").await;
    for n in 0..3 {
        post(
            &app,
            &a,
            &b,
            &book,
            &format!("LARGE-{n}"),
            vec![
                line(&cash, "9000000000000000", "0"),
                line(&equity, "0", "9000000000000000"),
            ],
        )
        .await;
    }
    let r = report(&app, &a, &book).await;
    assert_eq!(r["trialBalance"]["debitMinor"], "27000000000000000");
    assert_eq!(r["balanceSheet"]["assetsMinor"], "27000000000000000");
    assert_eq!(r["cashFlow"]["netChangeMinor"], "27000000000000000");
}

#[tokio::test]
async fn financial_history_forced_tenant_policies_and_runtime_writer_fences_cannot_be_bypassed() {
    let (f, app, a, b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    period(&app, &a, &book).await;
    let cash = account(&app, &a, &book, "BANK", "asset", "cash").await;
    let equity = account(&app, &a, &book, "EQUITY", "equity", "financing").await;
    post(
        &app,
        &a,
        &b,
        &book,
        "OPEN",
        vec![line(&cash, "100", "0"), line(&equity, "0", "100")],
    )
    .await;
    for q in [
        "UPDATE archava_accounting.books SET currency='JPY',minor_units=0",
        "UPDATE archava_accounting.accounts SET class='expense',revision=revision+1",
        "UPDATE archava_accounting.journals SET memo='Rewritten',revision=revision+1",
        "UPDATE archava_accounting.journal_lines SET debit_minor=debit_minor+1",
        "UPDATE archava_accounting.audit SET reason='Rewritten'",
        "UPDATE archava_accounting.commands SET fingerprint='Rewritten'",
        "UPDATE archava_accounting.periods SET state='closed',revision=revision+1",
        "DELETE FROM archava_accounting.journals",
    ] {
        let e = sqlx::query(q)
            .execute(&f.admin_pool)
            .await
            .expect_err("database refuses changing financial history without its full protocol");
        assert_eq!(
            e.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("23514"),
            "{q}: {e}"
        );
    }
    let mut tx = f.runtime_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL archava.tenant_id='foreign'")
        .execute(&mut *tx)
        .await
        .unwrap();
    for table in archava_adapters::erp::accounting::TABLES {
        let q = format!("SELECT count(*) FROM archava_accounting.{table}");
        let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(q.as_str()))
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(n, 0, "{table} must be tenant fenced");
    }
    tx.rollback().await.unwrap();
    let covered: bool = sqlx::query_scalar("SELECT archava_control.write_guards_complete()")
        .fetch_one(&f.runtime_pool)
        .await
        .unwrap();
    assert!(covered);
    for table in archava_adapters::erp::accounting::TABLES
        .iter()
        .filter(|t| **t != "memberships")
    {
        let mut tx = f.runtime_pool.begin().await.unwrap();
        sqlx::query("SET LOCAL archava.tenant_id='acme'")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SET LOCAL archava.writer_token='invalid-fixture-token'")
            .execute(&mut *tx)
            .await
            .unwrap();
        let q=format!("INSERT INTO archava_accounting.{table} SELECT * FROM archava_accounting.{table} WHERE false");
        let e = sqlx::query(sqlx::AssertSqlSafe(q.as_str()))
            .execute(&mut *tx)
            .await
            .expect_err("stale writer refuses even a zero-row statement");
        assert_eq!(
            e.as_database_error().and_then(|e| e.code()).as_deref(),
            Some(if *table == "stock_layer_origins" {
                "42501"
            } else {
                "57P03"
            }),
            "{table}: {e}"
        );
        tx.rollback().await.unwrap();
    }
    sqlx::query("DROP TRIGGER archava_runtime_write_fence ON archava_accounting.journals")
        .execute(&f.owner_pool)
        .await
        .unwrap();
    let refused = AppState::try_new_all(
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
    .unwrap_err();
    assert!(
        format!("{refused:?}").contains("fence coverage is incomplete"),
        "{refused:?}"
    );
    sqlx::query("SELECT archava_control.install_runtime_write_guards()")
        .execute(&f.owner_pool)
        .await
        .unwrap();
    assert_eq!(
        report(&app, &a, &book).await["balanceSheet"]["assetsMinor"],
        "100"
    );
}

#[tokio::test]
async fn actual_previous_profile_upgrades_books_and_writer_control_without_posting_existing_trade()
{
    let mut f = archava_pg_fixture::provision_pre_accounting_profile().await;
    assert!(f.accounting_ledger.is_empty());
    let exists: bool =
        sqlx::query_scalar("SELECT to_regclass('archava_accounting.books') IS NOT NULL")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert!(!exists);
    let previous_control: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_control.schema_migrations")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(previous_control, 1);
    sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES('alice','alice@archava.test',$1,true,1)").bind(HASH).execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_organizations.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','alice','human',ARRAY['organizations.read','organizations.write','companies.read','companies.write'],'all')").execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope,role,capability) VALUES('acme','alice','human',ARRAY['catalog.read','catalog.write'],'all','owner','enterprise')").execute(&f.admin_pool).await.unwrap();
    let old_app = app(&f).await;
    let owner = login(&old_app, "alice").await;
    let old_company = company(&old_app, &owner, "EXISTING").await;
    let(st,product)=call(&old_app,"POST","/erp/commerce/commands",json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":{"operation":"product.create_for_company","companyId":old_company["companyId"],"expectedCompanyRevision":1,"sku":"PRE-BOOK","title":"Existing product must stay unposted","unit":"unit","trackStock":false,"price":15000}}),Some(&owner)).await;
    assert_eq!(st, StatusCode::OK, "{product}");
    let original_products: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(p) ORDER BY p.id) FROM archava_commerce.products p",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    let before:Value=sqlx::query_scalar("SELECT jsonb_build_object('products',(SELECT count(*) FROM archava_commerce.products),'purchases',(SELECT count(*) FROM archava_purchasing.orders),'invoices',(SELECT count(*) FROM archava_billing.invoices),'memberships',(SELECT count(*) FROM archava_organizations.memberships))").fetch_one(&f.admin_pool).await.unwrap();
    f.accounting_ledger =
        archava_pg_fixture::apply_pending_accounting_migrations(&f.owner_pool).await;
    assert_eq!(
        archava_pg_fixture::apply_pending_accounting_migrations(&f.owner_pool).await,
        f.accounting_ledger
    );
    let after:Value=sqlx::query_scalar("SELECT jsonb_build_object('products',(SELECT count(*) FROM archava_commerce.products),'purchases',(SELECT count(*) FROM archava_purchasing.orders),'invoices',(SELECT count(*) FROM archava_billing.invoices),'memberships',(SELECT count(*) FROM archava_organizations.memberships))").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(before, after);
    assert_eq!(after["products"], 1);
    let upgraded_products: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(p) ORDER BY p.id) FROM archava_commerce.products p",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(original_products, upgraded_products);
    let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM archava_accounting.books),(SELECT count(*) FROM archava_accounting.journals),(SELECT count(*) FROM archava_accounting.memberships)").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(counts, (0, 0, 0));
    let _app = app(&f).await;
}

#[tokio::test]
async fn financial_search_exact_records_keyset_pages_and_cross_company_lines_keep_their_scope() {
    let (_f, app, a, _b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    period(&app, &a, &book).await;
    for n in 0..105 {
        account(
            &app,
            &a,
            &book,
            &format!("ITEM-{n:03}"),
            "asset",
            "operating",
        )
        .await;
    }
    let target=ok(&app,&a,json!({"type":"account.create","bookId":book["bookId"],"code":"LITERAL","name":"Old target %_& goods","class":"asset","cashRole":"operating","reason":"Literal search target"})).await;
    let prefix = format!(
        "/erp/accounting/workspace?tenant=acme&companyId={}&collection=accounts",
        c["companyId"].as_str().unwrap()
    );
    let (st, first) = call(
        &app,
        "GET",
        &format!("{prefix}&limit=2"),
        Value::Null,
        Some(&a),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{first}");
    assert_eq!(first["accounts"].as_array().unwrap().len(), 2);
    let meta = first["collections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "accounts")
        .unwrap();
    assert_eq!(meta["hasMore"], true);
    let (st, next) = call(
        &app,
        "GET",
        &format!(
            "{prefix}&limit=2&cursor={}",
            meta["nextCursor"].as_str().unwrap()
        ),
        Value::Null,
        Some(&a),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{next}");
    for r in next["accounts"].as_array().unwrap() {
        assert!(!first["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == r["id"]));
    }
    let (st, found) = call(
        &app,
        "GET",
        &format!("{prefix}&search=Old%20target%20%25_%26%20goods"),
        Value::Null,
        Some(&a),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{found}");
    assert_eq!(found["accounts"].as_array().unwrap().len(), 1);
    assert_eq!(found["accounts"][0]["id"], target["resourceId"]);
    let (st, exact) = call(
        &app,
        "GET",
        &format!("{prefix}&id={}", target["resourceId"].as_str().unwrap()),
        Value::Null,
        Some(&a),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{exact}");
    assert_eq!(exact["detail"]["record"]["name"], "Old target %_& goods");
    let other = company(&app, &a, "B").await;
    let mut request = create(&other, "USD");
    request["command"]["code"] = json!("OTHER-BOOK");
    let (st, r) = send(&app, &a, &request).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let foreign = account(&app, &a, &r["receipt"], "FOREIGN", "equity", "financing").await;
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("{prefix}&id={}", foreign["resourceId"].as_str().unwrap()),
            Value::Null,
            Some(&a)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &app,
            &a,
            &draft(
                &book,
                "MIXED",
                vec![line(&target, "1", "0"), line(&foreign, "0", "1")]
            )
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn report_aggregates_include_posted_journals_beyond_every_workspace_page() {
    let (_f, app, a, b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    period(&app, &a, &book).await;
    let cash = account(&app, &a, &book, "BANK", "asset", "cash").await;
    let equity = account(&app, &a, &book, "EQUITY", "equity", "financing").await;
    for n in 0..105 {
        post(
            &app,
            &a,
            &b,
            &book,
            &format!("ROW-{n:03}"),
            vec![line(&cash, "123", "0"), line(&equity, "0", "123")],
        )
        .await;
    }
    let w = snapshot(&app, &a, &c).await.1;
    assert_eq!(w["journals"].as_array().unwrap().len(), 100);
    assert_eq!(
        w["collections"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "journals")
            .unwrap()["hasMore"],
        true
    );
    let r = report(&app, &a, &book).await;
    assert_eq!(r["coverage"]["postedJournalCount"], 105);
    assert_eq!(r["coverage"]["allPostedJournalsInFilter"], true);
    assert_eq!(r["trialBalance"]["debitMinor"], "12915");
    assert_eq!(r["balanceSheet"]["assetsMinor"], "12915");
}

#[tokio::test]
async fn inactive_operating_companies_can_finish_correct_and_close_their_existing_financial_book() {
    let (_f, app, a, b, c) = setup().await;
    let book = book(&app, &a, &c).await;
    let p = period(&app, &a, &book).await;
    let cash = account(&app, &a, &book, "BANK", "asset", "cash").await;
    let equity = account(&app, &a, &book, "EQUITY", "equity", "financing").await;
    let (st, existing) = send(
        &app,
        &a,
        &draft(
            &book,
            "EXISTING",
            vec![line(&cash, "100", "0"), line(&equity, "0", "100")],
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{existing}");
    let (st, withdraw) = send(
        &app,
        &a,
        &draft(
            &book,
            "WITHDRAW",
            vec![line(&cash, "20", "0"), line(&equity, "0", "20")],
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{withdraw}");
    let other = company(&app, &a, "NO-BOOK").await;
    for company in [&c, &other] {
        let(st,r)=call(&app,"POST","/erp/organizations/commands",json!({"tenant":"acme","mutation":{"requestId":Uuid::new_v4(),"type":"company.set_active","data":{"id":company["companyId"],"expectedRevision":1,"active":false,"reason":"Stop new operating business while closing the books"}}}),Some(&a)).await;
        assert_eq!(st, StatusCode::OK, "{r}");
    }
    let w = snapshot(&app, &a, &c).await.1;
    assert_eq!(w["companyActive"], false);
    assert!(w["notice"].as_str().unwrap().contains("existing book"));
    assert_eq!(
        send(&app, &a, &action("journal.void", &withdraw["receipt"]))
            .await
            .0,
        StatusCode::OK
    );
    let (st, posted) = send(&app, &b, &action("journal.post", &existing["receipt"])).await;
    assert_eq!(st, StatusCode::OK, "{posted}");
    let adjustment = post(
        &app,
        &a,
        &b,
        &book,
        "CLOSING-ADJUSTMENT",
        vec![line(&cash, "1", "0"), line(&equity, "0", "1")],
    )
    .await;
    let reversal=ok(&app,&a,json!({"type":"journal.reverse","id":adjustment["resourceId"],"expectedRevision":2,"code":"REV-ADJUSTMENT","date":"2026-11-08","sourceRef":"Reviewed historical correction","reason":"Correct a closing adjustment after operating deactivation"})).await;
    assert_eq!(
        send(&app, &b, &action("journal.post", &reversal)).await.0,
        StatusCode::OK
    );
    assert_eq!(
        report(&app, &a, &book).await["balanceSheet"]["assetsMinor"],
        "100"
    );
    assert_eq!(
        send(&app, &a, &action("period.close", &p)).await.0,
        StatusCode::OK
    );
    let reopened=ok(&app,&a,json!({"type":"period.reopen","id":p["resourceId"],"expectedRevision":2,"reason":"Authorized historical period review"})).await;
    assert_eq!(reopened["revision"], 3);
    let mut forbidden = create(&other, "USD");
    forbidden["command"]["expectedCompanyRevision"] = json!(2);
    forbidden["command"]["code"] = json!("INACTIVE-BOOK");
    assert_eq!(send(&app, &a, &forbidden).await.0, StatusCode::CONFLICT);
}
