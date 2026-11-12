//! Real HTTP/forced-RLS original costs, financial review and full operating rollback.
use super::{purchase_sources::PurchaseScenario, *};
struct StockScenario {
    p: PurchaseScenario,
    cogs: Value,
    new_cogs: Value,
    new_inventory: Value,
    product: Uuid,
    buyer: Value,
}
impl StockScenario {
    async fn new(policy: bool) -> Self {
        let p = PurchaseScenario::new(true).await;
        sqlx::query("UPDATE archava_organizations.memberships SET permissions=permissions||ARRAY['parties.read','parties.write'] WHERE tenant_id='acme' AND principal_id IN('alice','bob')").execute(&p.f.admin_pool).await.unwrap();
        sqlx::query("UPDATE archava_commerce.memberships SET permissions=permissions||ARRAY['returns.write'] WHERE tenant_id='acme' AND principal_id IN('alice','bob')").execute(&p.f.admin_pool).await.unwrap();
        let cogs = account(
            &p.app,
            &p.alice,
            &p.book,
            "COGS-OLD",
            "cost_of_goods_sold",
            "operating",
        )
        .await;
        let new_cogs = account(
            &p.app,
            &p.alice,
            &p.book,
            "COGS-NEW",
            "cost_of_goods_sold",
            "operating",
        )
        .await;
        let new_inventory =
            account(&p.app, &p.alice, &p.book, "INV-NEW", "asset", "operating").await;
        let product: Uuid =
            sqlx::query_scalar("SELECT product_id FROM archava_purchasing.offers WHERE id=$1")
                .bind(Uuid::parse_str(p.offer["id"].as_str().unwrap()).unwrap())
                .fetch_one(&p.f.admin_pool)
                .await
                .unwrap();
        let (st, org) = call(&p.app,"POST","/erp/organizations/commands",json!({"tenant":"acme","mutation":{"requestId":Uuid::new_v4(),"type":"create_organization","data":{"name":"Stock buyer","legalName":"Explicit stock buyer"}}}),Some(&p.alice)).await;
        assert_eq!(st, StatusCode::OK, "{org}");
        let (st, party) = call(&p.app,"POST","/erp/organizations/commands",json!({"tenant":"acme","mutation":{"requestId":Uuid::new_v4(),"type":"party.create_from_organization","data":{"organizationId":org["receipt"]["organizationId"],"expectedSourceRevision":1,"customer":true,"supplier":false}}}),Some(&p.alice)).await;
        assert_eq!(st, StatusCode::OK, "{party}");
        let s = Self {
            p,
            cogs,
            new_cogs,
            new_inventory,
            product,
            buyer: party["receipt"].clone(),
        };
        if policy {
            ok(&s.p.app, &s.p.alice, s.policy(0, &s.cogs)).await;
        }
        s
    }
    fn policy(&self, revision: i32, account: &Value) -> Value {
        json!({"type":"stock_policy.set","bookId":self.p.book["bookId"],"expectedRevision":revision,"currencyCatalogue":"SIX-2026-09-17","sourceUnit":"whole_idr","cogsAccount":{"accountId":account["resourceId"],"expectedAccountRevision":account["revision"]},"reason":"Explicit future original-cost recognition"})
    }
    async fn receive(&self, quantity: i64, fee: i64, new_mapping: bool) -> Value {
        if new_mapping {
            let mut policy = self.p.policy(1);
            policy["mappings"][0]["accountId"] = self.new_inventory["resourceId"].clone();
            ok(&self.p.app, &self.p.alice, policy).await;
        }
        let po = self.p.draft_order(quantity).await;
        self.p.purchase(json!({"operation":"order.confirm","id":po["id"],"expectedRevision":1,"reason":"Original acquisition mapping"})).await;
        let line = self.p.order_line(&po).await;
        let received=self.p.purchase(json!({"operation":"order.receive","id":po["id"],"expectedRevision":2,"landedFeeMinor":fee,"allocationBasis":"value","lines":[{"lineId":line,"accepted":quantity,"rejected":0}],"reason":"Actual accepted inventory cost"})).await;
        let inventory = if new_mapping {
            &self.new_inventory
        } else {
            &self.p.accounts[0]
        };
        let accounts: Vec<_> = [inventory, &self.p.accounts[1], &self.p.accounts[3]]
            .iter()
            .map(|a| json!({"accountId":a["resourceId"],"expectedAccountRevision":1}))
            .collect();
        let journal=ok(&self.p.app,&self.p.alice,json!({"type":"purchase_source.draft","sourceId":received["requestId"],"code":if new_mapping{"ACQUIRE-NEW"}else{"ACQUIRE-OLD"},"accounts":accounts,"reason":"Review immutable acquisition, separately post"})).await;
        self.p.post(&journal).await;
        received
    }
    async fn trade(&self, command: Value) -> Value {
        let (st, v) = call(
            &self.p.app,
            "POST",
            "/erp/commerce/commands",
            input(command),
            Some(&self.p.alice),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        let mut receipt = v["receipt"].clone();
        receipt["requestId"] = v["requestId"].clone();
        receipt
    }
    async fn sell(&self, quantity: i64) -> (Value, Value, Uuid) {
        let sale=self.trade(json!({"operation":"order.create_for_company","companyId":self.p.company["companyId"],"expectedCompanyRevision":1,"partyId":self.buyer["partyId"],"expectedPartyRevision":1,"lines":[{"productId":self.product,"quantity":quantity}]})).await;
        self.trade(json!({"operation":"order.reserve","id":sale["id"],"expectedRevision":1,"holdSeconds":600})).await;
        let done = self
            .trade(json!({"operation":"order.fulfill","id":sale["id"],"expectedRevision":2}))
            .await;
        let line =
            sqlx::query_scalar("SELECT id FROM archava_commerce.order_lines WHERE order_id=$1")
                .bind(Uuid::parse_str(sale["id"].as_str().unwrap()).unwrap())
                .fetch_one(&self.p.f.admin_pool)
                .await
                .unwrap();
        (sale, done, line)
    }
    async fn source(&self, source: &Value) -> Value {
        let (st, v) = call(
            &self.p.app,
            "GET",
            &format!(
                "/erp/accounting/workspace?tenant=acme&companyId={}&collection=stock_sources&id={}",
                self.p.company["companyId"].as_str().unwrap(),
                source["requestId"].as_str().unwrap()
            ),
            Value::Null,
            Some(&self.p.alice),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        assert!(v["selected"]["record"].get("segments").is_none());
        assert!(v["selected"]["record"].get("source_snapshot").is_none());
        v["detail"]["record"].clone()
    }
    fn draft(&self, source: &Value, code: &str) -> Value {
        input(
            json!({"type":"stock_source.draft","sourceId":source["id"],"code":code,"accounts":source["legs"].as_array().unwrap().iter().map(|l|json!({"accountId":l["accountId"],"expectedAccountRevision":1})).collect::<Vec<_>>(),"reason":"Review exact original account allocations"}),
        )
    }
    async fn digest(&self) -> Vec<(String, String)> {
        let mut digests = Vec::new();
        for (schema, tables) in [
            (
                "archava_commerce",
                &[
                    "orders",
                    "order_lines",
                    "stock_balances",
                    "commands",
                    "audit",
                    "movements",
                    "inventory_uses",
                    "inventory_return_costs",
                    "returns",
                ][..],
            ),
            (
                "archava_accounting",
                &["stock_sources", "stock_layer_origins"][..],
            ),
        ] {
            for table in tables {
                let q=format!("SELECT md5(coalesce(string_agg(to_jsonb(t)::text,'|' ORDER BY to_jsonb(t)::text),'')) FROM {schema}.{table} t");
                let hash = sqlx::query_scalar(sqlx::AssertSqlSafe(q.as_str()))
                    .fetch_one(&self.p.f.admin_pool)
                    .await
                    .unwrap();
                digests.push((format!("{schema}.{table}"), hash));
            }
        }
        digests
    }
}
#[tokio::test]
async fn actual_stock_costs_restock_original_accounts_after_policy_change_and_post_separately() {
    let s = StockScenario::new(true).await;
    s.receive(1, 1, false).await;
    s.receive(2, 100, true).await;
    let (sale, first, line) = s.sell(2).await;
    let first_source = s.source(&first).await;
    assert_eq!(first_source["amountMinor"], "1405100");
    assert_eq!(first_source["legs"].as_array().unwrap().len(), 3);
    assert_eq!(first_source["policyRevision"], 1);
    assert_eq!(first_source["originalCostPortionCount"], 2);
    assert_eq!(first_source["sourceState"], "pending");
    ok(&s.p.app, &s.p.alice, s.policy(1, &s.new_cogs)).await;
    let returned=s.trade(json!({"operation":"order.return","id":sale["id"],"expectedRevision":3,"lineId":line,"quantity":1,"disposition":"restock","reason":"Original recognized stock cost after remapping"})).await;
    let returned_source = s.source(&returned).await;
    assert_eq!(returned_source["amountMinor"], "-700100");
    assert_eq!(returned_source["effect"], "restock_return");
    let accounts: Vec<_> = returned_source["legs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["accountId"].clone())
        .collect();
    assert!(accounts.contains(&s.cogs["resourceId"]));
    assert!(accounts.contains(&s.p.accounts[0]["resourceId"]));
    assert!(!accounts.contains(&s.new_cogs["resourceId"]));
    assert_eq!(
        s.source(&first).await,
        first_source,
        "Returning must not rewrite the original financial snapshot"
    );
    let (resale, resold, resale_line) = s.sell(2).await;
    let resold_source = s.source(&resold).await;
    assert_eq!(resold_source["amountMinor"], "1405100");
    assert!(resold_source["legs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["accountId"] == s.new_cogs["resourceId"]));
    let discarded=s.trade(json!({"operation":"order.return","id":resale["id"],"expectedRevision":3,"lineId":resale_line,"quantity":1,"disposition":"discard","reason":"No physical inventory recovery"})).await;
    let discard = s.source(&discarded).await;
    assert_eq!(discard["amountMinor"], "0");
    assert_eq!(discard["legs"], json!([]));
    assert_eq!(discard["sourceState"], "no_monetary_effect");
    let close = input(
        json!({"type":"period.close","id":s.p.period["resourceId"],"expectedRevision":1,"reason":"Captured stock costs must be posted first"}),
    );
    assert_eq!(
        send(&s.p.app, &s.p.alice, &close).await.0,
        StatusCode::CONFLICT
    );
    let mut first_journal = Value::Null;
    for (source, code) in [
        (&first_source, "STOCK-SOLD"),
        (&returned_source, "STOCK-RETURN"),
        (&resold_source, "STOCK-RESOLD"),
    ] {
        let request = s.draft(source, code);
        let (st, v) = send(&s.p.app, &s.p.alice, &request).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        assert_eq!(v["receipt"]["sourceId"], source["id"]);
        let journal = &v["receipt"];
        assert_eq!(
            send(&s.p.app, &s.p.alice, &action("journal.post", journal))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        s.p.post(journal).await;
        if first_journal.is_null() {
            first_journal = journal.clone();
        }
    }
    let totals:Vec<(String,String)>=sqlx::query_as("SELECT a.code,sum(l.debit_minor-l.credit_minor)::text FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id JOIN archava_accounting.accounts a ON a.tenant_id=l.tenant_id AND a.id=l.account_id WHERE j.state='posted' GROUP BY a.code ORDER BY a.code COLLATE \"C\"").fetch_all(&s.p.f.admin_pool).await.unwrap();
    assert_eq!(
        totals,
        vec![
            ("COGS-NEW".into(), "1405100".into()),
            ("COGS-OLD".into(), "705000".into()),
            ("FEE-ACCRUAL".into(), "-10100".into()),
            ("GRNI".into(), "-2100000".into()),
            ("INV-NEW".into(), "0".into()),
            ("INVENTORY".into(), "0".into())
        ]
    );
    let report = report(&s.p.app, &s.p.alice, &s.p.book).await;
    assert_eq!(report["profitAndLoss"]["costOfGoodsSoldMinor"], "2110100");
    assert_eq!(report["trialBalance"]["balanced"], true);
    assert_eq!(report["sourceCoverage"]["capturedCount"], 6);
    assert_eq!(report["sourceCoverage"]["postedCount"], 5);
    assert_eq!(report["sourceCoverage"]["noMonetaryEffectCount"], 1);
    assert_eq!(report["sourceCoverage"]["unpostedCount"], 0);
    assert_eq!(
        report["sourceCoverage"]["sourceModules"],
        json!(["billing", "purchasing", "commerce"])
    );
    let reverse = input(
        json!({"type":"journal.reverse","id":first_journal["resourceId"],"expectedRevision":2,"code":"UNBOUND-REVERSE","date":"2026-11-08","sourceRef":"Refuse independent source reversal","reason":"Stock correction must start from its actual operation"}),
    );
    assert_eq!(
        send(&s.p.app, &s.p.bob, &reverse).await.0,
        StatusCode::CONFLICT
    );
    let (st, v) = send(&s.p.app, &s.p.alice, &close).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let before = s.digest().await;
    let (st,v)=call(&s.p.app,"POST","/erp/commerce/commands",input(json!({"operation":"order.return","id":sale["id"],"expectedRevision":4,"lineId":line,"quantity":1,"disposition":"restock","reason":"Closed source period refuses all operating writes"})),Some(&s.p.alice)).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert_eq!(s.digest().await, before);
}
#[tokio::test]
async fn stock_source_api_replay_refuses_forged_money_wrong_accounts_and_revoked_authority() {
    let s = StockScenario::new(true).await;
    s.receive(1, 1, false).await;
    let (_, sale, _) = s.sell(1).await;
    let source = s.source(&sale).await;
    let mut wrong = s.draft(&source, "WRONG");
    wrong["command"]["accounts"][0]["accountId"] = s.new_inventory["resourceId"].clone();
    assert_eq!(
        send(&s.p.app, &s.p.alice, &wrong).await.0,
        StatusCode::CONFLICT
    );
    let mut forged = s.draft(&source, "FORGED-MONEY");
    forged["command"]["amountMinor"] = json!("1");
    assert_eq!(
        send(&s.p.app, &s.p.alice, &forged).await.0,
        StatusCode::BAD_REQUEST
    );
    let mut stale = s.draft(&source, "STALE");
    stale["command"]["accounts"][0]["expectedAccountRevision"] = json!(999);
    assert_eq!(
        send(&s.p.app, &s.p.alice, &stale).await.0,
        StatusCode::CONFLICT
    );
    let mut manual = draft(
        &s.p.book,
        "FORGED-STOCK",
        vec![
            line(&s.cogs, "100", "0"),
            line(&s.p.accounts[0], "0", "100"),
        ],
    );
    manual["command"]["sourceRef"] = json!(format!(
        "stock-cost:{}:FORGED-STOCK",
        source["id"].as_str().unwrap()
    ));
    assert_eq!(
        send(&s.p.app, &s.p.alice, &manual).await.0,
        StatusCode::BAD_REQUEST
    );
    let body = s.draft(&source, "EXACT-SOURCE");
    let (st, first) = send(&s.p.app, &s.p.alice, &body).await;
    assert_eq!(st, StatusCode::OK, "{first}");
    let (st, replay) = send(&s.p.app, &s.p.alice, &body).await;
    assert_eq!(st, StatusCode::OK, "{replay}");
    assert_eq!(replay["receipt"]["replayed"], true);
    assert_eq!(
        replay["receipt"]["resourceId"],
        first["receipt"]["resourceId"]
    );
    assert_eq!(
        send(&s.p.app, &s.p.bob, &body).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(&s.p.app, &s.p.alice, &s.draft(&source, "DUPLICATE"))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let (st, v) = status(&s.p.app, &s.p.alice, &body).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["receipt"]["sourceId"], source["id"]);
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=array_remove(permissions,'journals.write') WHERE tenant_id='acme' AND principal_id='alice'").execute(&s.p.f.admin_pool).await.unwrap();
    assert_eq!(
        send(&s.p.app, &s.p.alice, &body).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(&s.p.app, &s.p.alice, &body).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&s.p.app, &s.p.bob, &input(s.policy(0, &s.new_cogs)))
            .await
            .0,
        StatusCode::CONFLICT
    );
    for q in [
        "UPDATE archava_accounting.stock_sources SET amount_minor=amount_minor+1",
        "DELETE FROM archava_accounting.stock_layer_origins",
    ] {
        assert!(sqlx::query(q).execute(&s.p.f.admin_pool).await.is_err());
    }
    let mut tx = s.p.f.admin_pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE archava_commerce.commands DISABLE TRIGGER capture_financial_stock_source",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    let complete: bool = sqlx::query_scalar("SELECT archava_accounting.stock_guards_complete()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(!complete);
    tx.rollback().await.unwrap();
    let _: Router = app(&s.p.f).await;
}
#[tokio::test]
async fn enabling_stock_policy_never_invents_historical_cogs_for_an_old_fulfillment() {
    let s = StockScenario::new(false).await;
    s.receive(1, 1, false).await;
    let (order, fulfilled, line) = s.sell(1).await;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_accounting.stock_sources WHERE id=$1")
            .bind(Uuid::parse_str(fulfilled["requestId"].as_str().unwrap()).unwrap())
            .fetch_one(&s.p.f.admin_pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    ok(&s.p.app, &s.p.alice, s.policy(0, &s.cogs)).await;
    let before = s.digest().await;
    let (st,v)=call(&s.p.app,"POST","/erp/commerce/commands",input(json!({"operation":"order.return","id":order["id"],"expectedRevision":3,"lineId":line,"quantity":1,"disposition":"restock","reason":"Old fulfillment needs explicit financial opening review"})),Some(&s.p.alice)).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert_eq!(s.digest().await, before);
}
#[tokio::test]
async fn stock_policy_alone_pins_owner_dependencies_without_a_purchase_or_invoice_policy() {
    let (f, app, a, _b, c) = setup().await;
    let (st, v) = send(&app, &a, &create(&c, "IDR")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let book = &v["receipt"];
    let cogs = account(&app, &a, book, "COGS", "cost_of_goods_sold", "operating").await;
    let policy = input(
        json!({"type":"stock_policy.set","bookId":book["bookId"],"expectedRevision":0,"currencyCatalogue":"SIX-2026-09-17","sourceUnit":"whole_idr","cogsAccount":{"accountId":cogs["resourceId"],"expectedAccountRevision":1},"reason":"Explicit stock-only policy dependency review"}),
    );
    let (st, v) = send(&app, &a, &policy).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let count:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM archava_accounting.purchase_policies)+(SELECT count(*) FROM archava_accounting.invoice_policies)").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(count, 0);
    sqlx::query("INSERT INTO archava_owner.owner_grants(tenant_id,principal_id,permissions) VALUES('acme','alice',ARRAY['config.read','config.apply'])").execute(&f.admin_pool).await.unwrap();
    for module in ["accounting", "commerce", "purchasing"] {
        let (st, v) = call(
            &app,
            "POST",
            "/own/plan",
            json!({"tenant":"acme","request":{"baseRevision":0,"disable":[module]}}),
            Some(&a),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        assert_eq!(v["plan"]["blocked"]["reason"], "mandatory", "{v}");
        assert_eq!(v["plan"]["blocked"]["module"], module);
    }
    let w = snapshot(&app, &a, &c).await.1;
    assert_eq!(w["stockPolicy"]["revision"], 1);
    assert_eq!(
        w["stockPolicy"]["cogsAccount"]["accountId"],
        cogs["resourceId"]
    );
    assert_eq!(w["journals"], json!([]));
}
#[tokio::test]
async fn stock_first_policy_and_fulfillment_lock_company_before_book_and_capture_one_revision() {
    use super::purchase_sources::wait_sql;
    let s = StockScenario::new(false).await;
    s.receive(1, 1, false).await;
    let mut held = s.p.f.admin_pool.begin().await.unwrap();
    sqlx::query("SELECT archava_accounting.lock_purchase_company('acme',$1)")
        .bind(Uuid::parse_str(s.p.company["companyId"].as_str().unwrap()).unwrap())
        .execute(&mut *held)
        .await
        .unwrap();
    let app = s.p.app.clone();
    let actor = s.p.alice.clone();
    let policy = input(s.policy(0, &s.cogs));
    let setup = tokio::spawn(async move { send(&app, &actor, &policy).await });
    let pid=wait_sql(&s.p.f,"SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'SELECT archava_accounting.lock_purchase_company%'").await;
    let early:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND relation='archava_accounting.books'::regclass AND mode='RowShareLock')").bind(pid).fetch_one(&s.p.f.admin_pool).await.unwrap();
    assert!(!early);
    held.commit().await.unwrap();
    let (st, v) = setup.await.unwrap();
    assert_eq!(st, StatusCode::OK, "{v}");
    let order=s.trade(json!({"operation":"order.create_for_company","companyId":s.p.company["companyId"],"expectedCompanyRevision":1,"partyId":s.buyer["partyId"],"expectedPartyRevision":1,"lines":[{"productId":s.product,"quantity":1}]})).await;
    s.trade(json!({"operation":"order.reserve","id":order["id"],"expectedRevision":1,"holdSeconds":600})).await;
    let mut held = s.p.f.admin_pool.begin().await.unwrap();
    sqlx::query(
        "SELECT id FROM archava_accounting.books WHERE tenant_id='acme' AND id=$1 FOR UPDATE",
    )
    .bind(Uuid::parse_str(s.p.book["bookId"].as_str().unwrap()).unwrap())
    .execute(&mut *held)
    .await
    .unwrap();
    let app = s.p.app.clone();
    let actor = s.p.alice.clone();
    let body = input(json!({"operation":"order.fulfill","id":order["id"],"expectedRevision":2}));
    let fulfill = tokio::spawn(async move {
        call(&app, "POST", "/erp/commerce/commands", body, Some(&actor)).await
    });
    wait_sql(&s.p.f,"SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'UPDATE archava_commerce.orders%'").await;
    let app = s.p.app.clone();
    let actor = s.p.alice.clone();
    let body = input(s.policy(1, &s.new_cogs));
    let update = tokio::spawn(async move { send(&app, &actor, &body).await });
    wait_sql(&s.p.f,"SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'SELECT archava_accounting.lock_purchase_company%'").await;
    held.commit().await.unwrap();
    let (st, fulfilled) = fulfill.await.unwrap();
    assert_eq!(st, StatusCode::OK, "{fulfilled}");
    let (st, v) = update.await.unwrap();
    assert_eq!(st, StatusCode::OK, "{v}");
    let source = s.source(&fulfilled).await;
    assert_eq!(source["policyRevision"], 1);
    assert!(source["legs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["accountId"] == s.cogs["resourceId"]));
    assert!(!source["legs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["accountId"] == s.new_cogs["resourceId"]));
    assert_eq!(
        snapshot(&s.p.app, &s.p.alice, &s.p.company).await.1["stockPolicy"]["revision"],
        2
    );
}
