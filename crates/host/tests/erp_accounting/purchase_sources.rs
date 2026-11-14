//! Real HTTP/non-owner PostgreSQL purchase capture and separate financial review.
use super::*;
pub(super) struct PurchaseScenario {
    pub(super) f: Fixture,
    pub(super) app: Router,
    pub(super) alice: Value,
    pub(super) bob: Value,
    pub(super) company: Value,
    pub(super) book: Value,
    pub(super) period: Value,
    pub(super) accounts: Vec<Value>,
    pub(super) supplier: Value,
    pub(super) offer: Value,
    /// The book's currency and the source unit its bridges accept.
    pub(super) currency: String,
    pub(super) unit: &'static str,
}
impl PurchaseScenario {
    pub(super) async fn new(policy: bool) -> Self {
        Self::new_in("IDR", policy).await
    }
    /// The same scenario over a book in another currency.
    pub(super) async fn new_in(currency: &str, policy: bool) -> Self {
        let mut s = Self::trading().await;
        s.open_book(currency).await;
        s.stock_item().await;
        if policy {
            ok(&s.app, &s.alice, s.policy(0)).await;
        }
        s
    }
    /// A company that can buy and sell but has no book yet, so it trades in IDR.
    pub(super) async fn trading() -> Self {
        let (f, app, alice, bob, company) = setup().await;
        for actor in ["alice", "bob"] {
            sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope,role,capability) VALUES('acme',$1,'human',ARRAY['catalog.read','catalog.write','stock.read','stock.write','orders.read','orders.write'],'all','owner','enterprise')").bind(actor).execute(&f.admin_pool).await.unwrap();
            sqlx::query("INSERT INTO archava_purchasing.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme',$1,'human',$2,'all')").bind(actor).bind(archava_domain::erp::purchasing::PERMISSION_VOCABULARY).execute(&f.admin_pool).await.unwrap();
        }
        let mut s = Self {
            f,
            app,
            alice,
            bob,
            company,
            book: Value::Null,
            period: Value::Null,
            accounts: Vec::new(),
            supplier: Value::Null,
            offer: Value::Null,
            currency: "IDR".into(),
            unit: "whole_idr",
        };
        s.supplier=s.purchase(json!({"operation":"supplier.create","code":"SOURCE-SUPPLIER","name":"Explicit supplier master"})).await;
        s
    }
    /// Creates the company's book with the accounts and period the purchase bridge needs.
    pub(super) async fn open_book(&mut self, currency: &str) {
        let (status, v) = send(&self.app, &self.alice, &create(&self.company, currency)).await;
        assert_eq!(status, StatusCode::OK, "{v}");
        let book = v["receipt"].clone();
        self.period=ok(&self.app,&self.alice,json!({"type":"period.open","bookId":book["bookId"],"code":"SOURCE","startDate":"2000-01-01","endDate":"2100-12-31","reason":"Owned purchase source window"})).await;
        for (role, class) in [
            ("INVENTORY", "asset"),
            ("GRNI", "liability"),
            ("PAYABLE", "liability"),
            ("FEE-ACCRUAL", "liability"),
        ] {
            self.accounts
                .push(account(&self.app, &self.alice, &book, role, class, "operating").await);
        }
        self.book = book;
        self.currency = currency.into();
        self.unit = if currency == "IDR" {
            "whole_idr"
        } else {
            "minor"
        };
    }
    /// A tracked product and its supplier offer, in the company's current trade currency.
    pub(super) async fn stock_item(&mut self) {
        let product=self.trade(json!({"operation":"product.create_for_company","companyId":self.company["companyId"],"expectedCompanyRevision":1,"sku":"PURCHASE-SOURCE","title":"Counted received goods","unit":"unit","trackStock":true,"price":15000})).await;
        self.offer=self.purchase(json!({"operation":"offer.create","supplierId":self.supplier["id"],"productId":product["id"],"unitCost":7000})).await;
    }
    pub(super) fn policy(&self, revision: i32) -> Value {
        self.policy_in(revision, self.unit)
    }
    pub(super) fn policy_in(&self, revision: i32, unit: &str) -> Value {
        let mappings:Vec<_>=["inventory","goods_received_not_invoiced","payable","landed_fee_accrual"].into_iter().zip(&self.accounts).map(|(role,a)|json!({"role":role,"accountId":a["resourceId"],"expectedAccountRevision":1})).collect();
        json!({"type":"purchase_policy.set","bookId":self.book["bookId"],"expectedRevision":revision,"currencyCatalogue":"SIX-2026-09-17","sourceUnit":unit,"mappings":mappings,"reason":"Explicit future source unit contract"})
    }
    pub(super) async fn trade(&self, command: Value) -> Value {
        let (status, v) = call(
            &self.app,
            "POST",
            "/erp/commerce/commands",
            input(command),
            Some(&self.alice),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        v["receipt"].clone()
    }
    pub(super) async fn purchase(&self, command: Value) -> Value {
        let (status, v) = call(
            &self.app,
            "POST",
            "/erp/purchasing/commands",
            input(command),
            Some(&self.alice),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        v["receipt"].clone()
    }
    pub(super) async fn draft_order(&self, quantity: i64) -> Value {
        self.purchase(json!({"operation":"order.create-for-company","companyId":self.company["companyId"],"expectedCompanyRevision":1,"supplierId":self.supplier["id"],"lines":[{"offerId":self.offer["id"],"quantity":quantity}]})).await
    }
    pub(super) async fn order_line(&self, order: &Value) -> Uuid {
        sqlx::query_scalar(
            "SELECT id FROM archava_purchasing.lines WHERE tenant_id='acme' AND order_id=$1",
        )
        .bind(Uuid::parse_str(order["id"].as_str().unwrap()).unwrap())
        .fetch_one(&self.f.admin_pool)
        .await
        .unwrap()
    }
    pub(super) async fn receive(&self) -> (Value, Value, Uuid) {
        let po = self.draft_order(3).await;
        self.purchase(json!({"operation":"order.confirm","id":po["id"],"expectedRevision":1,"reason":"Confirm reviewed source policy"})).await;
        let line = self.order_line(&po).await;
        let receipt=self.purchase(json!({"operation":"order.receive","id":po["id"],"expectedRevision":2,"reason":"Two accepted, one rejected","landedFeeMinor":101,"allocationBasis":"value","lines":[{"lineId":line,"accepted":2,"rejected":1}]})).await;
        (po, receipt, line)
    }
    async fn source(&self, id: &Value) -> Value {
        let (status,v)=call(&self.app,"GET",&format!("/erp/accounting/workspace?tenant=acme&companyId={}&collection=purchase_sources&id={}",self.company["companyId"].as_str().unwrap(),id.as_str().unwrap()),Value::Null,Some(&self.alice)).await;
        assert_eq!(status, StatusCode::OK, "{v}");
        v["detail"]["record"].clone()
    }
    pub(super) async fn reviewed(&self, source: &Value, code: &str, roles: &[usize]) -> Value {
        let accounts:Vec<_>=roles.iter().map(|i|json!({"accountId":self.accounts[*i]["resourceId"],"expectedAccountRevision":1})).collect();
        ok(&self.app,&self.alice,json!({"type":"purchase_source.draft","sourceId":source["requestId"],"code":code,"accounts":accounts,"reason":"Review original goods, fee, source date and account roles"})).await
    }
    pub(super) async fn post(&self, j: &Value) {
        ok(&self.app,&self.bob,json!({"type":"journal.post","id":j["resourceId"],"expectedRevision":1,"reason":"Independent review of the immutable purchase source"})).await;
    }
}
#[tokio::test]
async fn purchase_receipt_matched_bill_void_post_exact_original_costs_with_independent_review() {
    let s = PurchaseScenario::new(true).await;
    let (po, r, line) = s.receive().await;
    let source = s.source(&r["requestId"]).await;
    assert_eq!(source["goodsMinor"], "1400000");
    assert_eq!(source["landedFeeMinor"], "10100");
    assert_eq!(source["policyRevision"], 1);
    let receipt_j = s.reviewed(&r, "RECEIPT", &[0, 1, 3]).await;
    let (status,_)=send(&s.app,&s.alice,&input(json!({"type":"journal.post","id":receipt_j["resourceId"],"expectedRevision":1,"reason":"Author cannot independently approve"}))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    s.post(&receipt_j).await;
    let document=s.purchase(json!({"operation":"document.capture","orderId":po["id"],"expectedOrderRevision":3,"supplierId":s.supplier["id"],"vendorReference":"SOURCE-BILL","documentDate":"2026-11-08","currency":"IDR","declaredTotal":14000,"lines":[{"lineId":line,"quantity":2,"unitCost":7000}]})).await;
    let bill=s.purchase(json!({"operation":"document.match","id":document["id"],"expectedRevision":1,"expectedOrderRevision":3,"reason":"Exact accepted-goods claim"})).await;
    assert_eq!(s.source(&bill["requestId"]).await["goodsMinor"], "1400000");
    let j = s.reviewed(&bill, "SUPPLIER-BILL", &[1, 2]).await;
    s.post(&j).await;
    let void=s.purchase(json!({"operation":"document.void","id":document["id"],"expectedRevision":2,"reason":"Correct the original supplier claim"})).await;
    assert_eq!(s.source(&void["requestId"]).await["goodsMinor"], "-1400000");
    let j = s.reviewed(&void, "SUPPLIER-VOID", &[1, 2]).await;
    s.post(&j).await;
    let totals:Vec<(String,String)>=sqlx::query_as("SELECT a.code,sum(l.debit_minor-l.credit_minor)::text FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id JOIN archava_accounting.accounts a ON a.tenant_id=l.tenant_id AND a.id=l.account_id WHERE l.tenant_id='acme' AND j.state='posted' GROUP BY a.code ORDER BY a.code").fetch_all(&s.f.admin_pool).await.unwrap();
    assert_eq!(
        totals,
        vec![
            ("FEE-ACCRUAL".into(), "-10100".into()),
            ("GRNI".into(), "-1400000".into()),
            ("INVENTORY".into(), "1410100".into()),
            ("PAYABLE".into(), "0".into())
        ]
    );
    let (status, v) = call(
        &s.app,
        "GET",
        &format!(
            "/erp/accounting/reports?tenant=acme&bookId={}&from=2000-01-01&to=2100-12-31",
            s.book["bookId"].as_str().unwrap()
        ),
        Value::Null,
        Some(&s.alice),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["sourceCoverage"]["capturedCount"], 3);
    assert_eq!(v["sourceCoverage"]["postedCount"], 3);
    assert_eq!(v["sourceCoverage"]["bankReconciliationIncluded"], false);
    ok(&s.app,&s.alice,json!({"type":"period.close","id":s.period["resourceId"],"expectedRevision":1,"reason":"All captured sources reviewed, full bank close remains separate"})).await;
}
#[tokio::test]
async fn purchase_policy_does_not_import_prior_confirmations_and_retains_original_mapping_revision()
{
    let s = PurchaseScenario::new(false).await;
    let old = s.draft_order(1).await;
    s.purchase(json!({"operation":"order.confirm","id":old["id"],"expectedRevision":1,"reason":"Original confirmation before explicit setup"})).await;
    ok(&s.app, &s.alice, s.policy(0)).await;
    let line = s.order_line(&old).await;
    s.purchase(json!({"operation":"order.receive","id":old["id"],"expectedRevision":2,"reason":"Legacy commitment remains outside implicit source import","lines":[{"lineId":line,"accepted":1,"rejected":0}]})).await;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_accounting.purchase_sources")
        .fetch_one(&s.f.admin_pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    let po = s.draft_order(2).await;
    s.purchase(json!({"operation":"order.confirm","id":po["id"],"expectedRevision":1,"reason":"Pin the first reviewed policy"})).await;
    let line = s.order_line(&po).await;
    s.purchase(json!({"operation":"order.receive","id":po["id"],"expectedRevision":2,"reason":"First part","lines":[{"lineId":line,"accepted":1,"rejected":0}]})).await;
    let mut updated = s.policy(1);
    let new = account(
        &s.app,
        &s.alice,
        &s.book,
        "NEW-INVENTORY",
        "asset",
        "operating",
    )
    .await;
    updated["mappings"][0]["accountId"] = new["resourceId"].clone();
    ok(&s.app, &s.alice, updated).await;
    let last=s.purchase(json!({"operation":"order.receive","id":po["id"],"expectedRevision":3,"reason":"Second part retains its original book roles","lines":[{"lineId":line,"accepted":1,"rejected":0}]})).await;
    let source = s.source(&last["requestId"]).await;
    assert_eq!(source["policyRevision"], 1);
    assert_eq!(
        source["policy"]["mappings"][0]["accountId"],
        s.accounts[0]["resourceId"]
    );
    assert_eq!(source["goodsMinor"], "700000");
}
#[tokio::test]
async fn purchase_source_review_replay_current_authority_and_reserved_journal_namespace_are_enforced(
) {
    let s = PurchaseScenario::new(true).await;
    let (_, r, _) = s.receive().await;
    let command = json!({"type":"purchase_source.draft","sourceId":r["requestId"],"code":"ONE-SOURCE","accounts":[{"accountId":s.accounts[0]["resourceId"],"expectedAccountRevision":1},{"accountId":s.accounts[1]["resourceId"],"expectedAccountRevision":1},{"accountId":s.accounts[3]["resourceId"],"expectedAccountRevision":1}],"reason":"Exact reviewed source"});
    let body = input(command.clone());
    let (status, first) = send(&s.app, &s.alice, &body).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let (status, replay) = send(&s.app, &s.alice, &body).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(
        first["receipt"]["resourceId"],
        replay["receipt"]["resourceId"]
    );
    let (status, _) = send(&s.app, &s.bob, &body).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = send(&s.app, &s.alice, &input(command)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=array_remove(permissions,'journals.write') WHERE tenant_id='acme' AND principal_id='alice'").execute(&s.f.admin_pool).await.unwrap();
    let (status, _) = send(&s.app, &s.alice, &body).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_accounting.purchase_journal_sources")
            .fetch_one(&s.f.admin_pool)
            .await
            .unwrap();
    assert_eq!(n, 1);
    let mut forged = draft(
        &s.book,
        "FORGED-PURCHASE",
        vec![
            line(&s.accounts[0], "100", "0"),
            line(&s.accounts[2], "0", "100"),
        ],
    );
    forged["command"]["sourceRef"] = json!(format!(
        "purchasing:{}:FORGED-PURCHASE",
        r["requestId"].as_str().unwrap()
    ));
    let (status, _) = send(&s.app, &s.bob, &forged).await;
    assert_eq!(status, StatusCode::CONFLICT);
}
#[tokio::test]
async fn purchase_sources_prevent_premature_close_and_database_forgery_and_guard_tampering() {
    let s = PurchaseScenario::new(true).await;
    let (_, r, _) = s.receive().await;
    let (status,_)=send(&s.app,&s.alice,&input(json!({"type":"period.close","id":s.period["resourceId"],"expectedRevision":1,"reason":"A pending receipt cannot disappear from close"}))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let mut tx = s.f.admin_pool.begin().await.unwrap();
    assert!(sqlx::query(
        "UPDATE archava_accounting.purchase_sources SET goods_minor=goods_minor+1"
    )
    .execute(&mut *tx)
    .await
    .is_err());
    tx.rollback().await.unwrap();
    let source = s.source(&r["requestId"]).await;
    assert_eq!(source["sourceState"], "pending");
    let mut tx = s.f.admin_pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE archava_purchasing.commands DISABLE TRIGGER capture_financial_purchase_source",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    let complete: bool = sqlx::query_scalar("SELECT archava_accounting.purchase_guards_complete()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(!complete);
    tx.rollback().await.unwrap();
    let _: Router = app(&s.f).await;
}

pub(super) async fn wait_sql(f: &Fixture, query: &str) -> i32 {
    for _ in 0..200 {
        let pid: Option<i32> = sqlx::query_scalar(sqlx::AssertSqlSafe(query))
            .fetch_optional(&f.admin_pool)
            .await
            .unwrap();
        if let Some(pid) = pid {
            return pid;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("Expected the actual concurrent request to reach its database fence");
}
#[tokio::test]
async fn purchase_first_policy_and_confirmation_and_later_update_use_company_before_book_fences() {
    let s = PurchaseScenario::new(false).await;
    let po = s.draft_order(1).await;
    let mut held = s.f.admin_pool.begin().await.unwrap();
    sqlx::query("SELECT archava_accounting.lock_purchase_company('acme',$1)")
        .bind(Uuid::parse_str(s.company["companyId"].as_str().unwrap()).unwrap())
        .execute(&mut *held)
        .await
        .unwrap();
    let app = s.app.clone();
    let alice = s.alice.clone();
    let policy = input(s.policy(0));
    let setup = tokio::spawn(async move { send(&app, &alice, &policy).await });
    let pid=wait_sql(&s.f,"SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'SELECT archava_accounting.lock_purchase_company%'").await;
    let premature:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND relation='archava_accounting.books'::regclass AND mode='RowShareLock')").bind(pid).fetch_one(&s.f.admin_pool).await.unwrap();
    assert!(
        !premature,
        "Policy setup must not hold a book SHARE lock while waiting for its company fence"
    );
    held.commit().await.unwrap();
    let (status, value) = setup.await.unwrap();
    assert_eq!(status, StatusCode::OK, "{value}");
    // Confirmation obtains the company fence but waits for the existing book.
    let mut held = s.f.admin_pool.begin().await.unwrap();
    sqlx::query(
        "SELECT id FROM archava_accounting.books WHERE tenant_id='acme' AND id=$1 FOR UPDATE",
    )
    .bind(Uuid::parse_str(s.book["bookId"].as_str().unwrap()).unwrap())
    .execute(&mut *held)
    .await
    .unwrap();
    let app = s.app.clone();
    let alice = s.alice.clone();
    let body = input(
        json!({"operation":"order.confirm","id":po["id"],"expectedRevision":1,"reason":"Confirmation keeps its current policy while a later update waits"}),
    );
    let confirm = tokio::spawn(async move {
        call(&app, "POST", "/erp/purchasing/commands", body, Some(&alice)).await
    });
    wait_sql(&s.f,"SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'UPDATE archava_purchasing.orders%'").await;
    let app = s.app.clone();
    let alice = s.alice.clone();
    let policy = input(s.policy(1));
    let update = tokio::spawn(async move { send(&app, &alice, &policy).await });
    wait_sql(&s.f,"SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'SELECT archava_accounting.lock_purchase_company%'").await;
    held.commit().await.unwrap();
    let (status, value) = confirm.await.unwrap();
    assert_eq!(status, StatusCode::OK, "{value}");
    let (status, value) = update.await.unwrap();
    assert_eq!(status, StatusCode::OK, "{value}");
    let revision:i32=sqlx::query_scalar("SELECT policy_revision FROM archava_accounting.purchase_origins WHERE tenant_id='acme' AND order_id=$1").bind(Uuid::parse_str(po["id"].as_str().unwrap()).unwrap()).fetch_one(&s.f.admin_pool).await.unwrap();
    assert_eq!(revision, 1);
    let revision: i32 = sqlx::query_scalar(
        "SELECT revision FROM archava_accounting.purchase_policies WHERE tenant_id='acme'",
    )
    .fetch_one(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(revision, 2);
}
#[tokio::test]
async fn purchase_policy_pins_owner_dependencies_and_financial_sources_remain_permission_scoped() {
    let s = PurchaseScenario::new(true).await;
    sqlx::query("INSERT INTO archava_owner.owner_grants(tenant_id,principal_id,permissions) VALUES('acme','alice',ARRAY['config.read','config.apply'])").execute(&s.f.admin_pool).await.unwrap();
    for module in ["accounting", "commerce", "purchasing"] {
        let (status, v) = call(
            &s.app,
            "POST",
            "/own/plan",
            json!({"tenant":"acme","request":{"baseRevision":0,"disable":[module]}}),
            Some(&s.alice),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        assert_eq!(v["plan"]["blocked"]["reason"], "mandatory", "{v}");
        assert_eq!(v["plan"]["blocked"]["module"], module);
    }
    let (_, r, _) = s.receive().await;
    let reader = login(&s.app, "reader").await;
    let (status, _) = send(&s.app, &reader, &input(s.policy(1))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=array_remove(permissions,'books.read') WHERE tenant_id='acme' AND principal_id='alice'").execute(&s.f.admin_pool).await.unwrap();
    let (status, _) = call(
        &s.app,
        "GET",
        &format!(
            "/erp/accounting/workspace?tenant=acme&companyId={}&collection=purchase_sources&id={}",
            s.company["companyId"].as_str().unwrap(),
            r["requestId"].as_str().unwrap()
        ),
        Value::Null,
        Some(&s.alice),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// Trade currency stage 4: on a USD book the purchase bridge posts a received
/// order and its supplier bill at the order's own minor amounts, and the
/// payable control comparison reconciles.
#[tokio::test]
async fn a_usd_book_posts_purchase_receipts_and_bills_in_minor_units_unchanged() {
    let s = PurchaseScenario::new_in("USD", false).await;
    // Whole Rupiah is not a unit of a USD book; the minor unit is.
    let (status, refused) = send(&s.app, &s.alice, &input(s.policy_in(0, "whole_idr"))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    ok(&s.app, &s.alice, s.policy(0)).await;
    let (st, workspace) = snapshot(&s.app, &s.alice, &s.company).await;
    assert_eq!(st, StatusCode::OK, "{workspace}");
    assert_eq!(workspace["purchasePolicy"]["sourceUnit"], "minor");
    let notice = workspace["purchasePostingNotice"].as_str().unwrap();
    assert!(
        notice.contains("USD minor-unit purchase policy") && !notice.contains("whole-IDR"),
        "{notice}"
    );
    // Three ordered at USD 70.00, two accepted, USD 1.01 landed fee.
    let (po, r, line) = s.receive().await;
    let source = s.source(&r["requestId"]).await;
    assert_eq!(source["goodsMinor"], "14000", "not 1400000");
    assert_eq!(source["landedFeeMinor"], "101");
    let (unit, key): (String, String) = sqlx::query_as(
        "SELECT o.policy_snapshot->>'source_unit',(SELECT string_agg(k,',') FROM jsonb_object_keys(s.source_snapshot->'lines'->0) k WHERE k LIKE 'unitCost%') \
         FROM archava_accounting.purchase_sources s JOIN archava_accounting.purchase_origins o ON o.tenant_id=s.tenant_id AND o.id=s.origin_id",
    )
    .fetch_one(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!((unit.as_str(), key.as_str()), ("minor", "unitCostMinor"));
    let receipt_j = s.reviewed(&r, "RECEIPT", &[0, 1, 3]).await;
    s.post(&receipt_j).await;
    let document=s.purchase(json!({"operation":"document.capture","orderId":po["id"],"expectedOrderRevision":3,"supplierId":s.supplier["id"],"vendorReference":"USD-BILL","documentDate":"2026-11-08","currency":"USD","declaredTotal":14000,"lines":[{"lineId":line,"quantity":2,"unitCost":7000}]})).await;
    let bill=s.purchase(json!({"operation":"document.match","id":document["id"],"expectedRevision":1,"expectedOrderRevision":3,"reason":"Exact accepted-goods claim"})).await;
    assert_eq!(s.source(&bill["requestId"]).await["goodsMinor"], "14000");
    let j = s.reviewed(&bill, "SUPPLIER-BILL", &[1, 2]).await;
    s.post(&j).await;
    let totals:Vec<(String,String)>=sqlx::query_as("SELECT a.code,sum(l.debit_minor-l.credit_minor)::text FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id JOIN archava_accounting.accounts a ON a.tenant_id=l.tenant_id AND a.id=l.account_id WHERE l.tenant_id='acme' AND j.state='posted' GROUP BY a.code ORDER BY a.code").fetch_all(&s.f.admin_pool).await.unwrap();
    assert_eq!(
        totals,
        vec![
            ("FEE-ACCRUAL".into(), "-101".into()),
            ("GRNI".into(), "0".into()),
            ("INVENTORY".into(), "14101".into()),
            ("PAYABLE".into(), "-14000".into())
        ]
    );
    let payable = subledger::accepted(&s.app, &s.alice, &s.book, "payable", "2100-12-31").await;
    assert_eq!(payable["book"]["currency"], "USD");
    assert_eq!(payable["totals"]["netOutstandingMinor"], "14000");
    assert_eq!(payable["positionControlDifferenceMinor"], "0");
    assert_eq!(payable["controls"][0]["differenceMinor"], "0");
    assert_eq!(payable["postedSourceControlsAgree"], true, "{payable}");
    // Voiding the bill reverses the same unconverted amount.
    let void=s.purchase(json!({"operation":"document.void","id":document["id"],"expectedRevision":2,"reason":"Correct the original supplier claim"})).await;
    assert_eq!(s.source(&void["requestId"]).await["goodsMinor"], "-14000");
    let j = s.reviewed(&void, "SUPPLIER-VOID", &[1, 2]).await;
    s.post(&j).await;
    let payable = subledger::accepted(&s.app, &s.alice, &s.book, "payable", "2100-12-31").await;
    assert_eq!(payable["totals"]["netOutstandingMinor"], "0");
    assert_eq!(payable["postedSourceControlsAgree"], true, "{payable}");
    ok(&s.app,&s.alice,json!({"type":"period.close","id":s.period["resourceId"],"expectedRevision":1,"reason":"All captured sources reviewed"})).await;
}

/// The unit must fit the book: the `minor` unit is not valid on an IDR book,
/// and the unit of an existing policy cannot be changed.
#[tokio::test]
async fn the_minor_purchase_unit_is_refused_on_an_idr_book() {
    let s = PurchaseScenario::new(false).await;
    let (status, refused) = send(&s.app, &s.alice, &input(s.policy_in(0, "minor"))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    ok(&s.app, &s.alice, s.policy(0)).await;
    let (status, refused) = send(&s.app, &s.alice, &input(s.policy_in(1, "minor"))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
}

/// A company that has a purchase order cannot open a USD book (Accounting
/// 015). The capture guard behind that refusal stays: with the state planted
/// by the database owner, a Rupiah order confirms outside the ledger while the
/// USD book has no purchase policy, and with one its confirmation is refused
/// as a conflict, no origin is recorded and the order stays a draft.
#[tokio::test]
async fn a_planted_idr_purchase_order_under_a_usd_book_is_refused_not_captured() {
    let mut s = PurchaseScenario::trading().await;
    s.stock_item().await;
    let earlier = s.draft_order(1).await;
    let later = s.draft_order(1).await;
    let (status, refused) = send(&s.app, &s.alice, &create(&s.company, "USD")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    let books: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_accounting.books")
        .fetch_one(&s.f.admin_pool)
        .await
        .unwrap();
    assert_eq!(books, 0, "the refused book is not written");
    book_currency_guard(&s.f, false).await;
    s.open_book("USD").await;
    book_currency_guard(&s.f, true).await;
    s.purchase(json!({"operation":"order.confirm","id":earlier["id"],"expectedRevision":1,"reason":"No purchase policy yet, so nothing is captured"})).await;
    ok(&s.app, &s.alice, s.policy(0)).await;
    let (status, refused) = call(
        &s.app,
        "POST",
        "/erp/purchasing/commands",
        input(json!({"operation":"order.confirm","id":later["id"],"expectedRevision":1,"reason":"Rupiah order against a USD book"})),
        Some(&s.alice),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    let (currency, confirmed, revision): (String, bool, i32) = sqlx::query_as(
        "SELECT currency,confirmed_at IS NOT NULL,revision FROM archava_purchasing.orders WHERE id=$1",
    )
    .bind(Uuid::parse_str(later["id"].as_str().unwrap()).unwrap())
    .fetch_one(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!((currency.as_str(), confirmed, revision), ("IDR", false, 1));
    // The order confirmed before the policy is received without a source.
    let line = s.order_line(&earlier).await;
    s.purchase(json!({"operation":"order.receive","id":earlier["id"],"expectedRevision":2,"reason":"Outside the captured sources","lines":[{"lineId":line,"accepted":1,"rejected":0}]})).await;
    let rows: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM archava_accounting.purchase_origins),(SELECT count(*) FROM archava_accounting.purchase_sources)",
    )
    .fetch_one(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(rows, (0, 0));
}
