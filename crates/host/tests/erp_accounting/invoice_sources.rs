use super::*;
use archava_domain::erp::billing::{provider_event_digest, PaymentEventClaim};
use hmac::{Hmac, Mac};
use sha2::Sha256;

pub(super) struct SourceFixture {
    pub(super) f: Fixture,
    pub(super) app: Router,
    pub(super) alice: Value,
    pub(super) bob: Value,
    pub(super) company: Value,
    pub(super) book: Value,
    pub(super) ar: Value,
    pub(super) revenue: Value,
    pub(super) clearing: Value,
    pub(super) period: Value,
    pub(super) party: Value,
    /// The book's currency and the source unit its bridges accept.
    pub(super) currency: String,
    pub(super) unit: &'static str,
}
impl SourceFixture {
    pub(super) async fn new() -> Self {
        Self::new_in("IDR").await
    }
    /// The same fixture over a book in another currency.
    pub(super) async fn new_in(currency: &str) -> Self {
        let mut s = Self::trading().await;
        s.open_book(currency).await;
        s
    }
    /// A company that can sell and bill but has no book yet, so it trades in IDR.
    pub(super) async fn trading() -> Self {
        let (f, app, alice, bob, company) = setup().await;
        sqlx::query("UPDATE archava_organizations.memberships SET permissions=permissions||ARRAY['parties.read','parties.write'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
        sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','alice','human',$1,'all')").bind(archava_adapters::erp::COMMERCE_PERMISSION_VOCABULARY).execute(&f.admin_pool).await.unwrap();
        sqlx::query("INSERT INTO archava_billing.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','alice','human',$1,'all')").bind(archava_domain::erp::billing::BILLING_PERMISSIONS).execute(&f.admin_pool).await.unwrap();
        sqlx::query("INSERT INTO archava_owner.owner_grants(tenant_id,principal_id,permissions) VALUES('acme','alice',ARRAY['config.read','config.apply'])").execute(&f.admin_pool).await.unwrap();
        let org = Self::org(
            &app,
            &alice,
            "create_organization",
            json!({"name":"Source buyer","legalName":"Source Buyer Official"}),
        )
        .await;
        let party=Self::org(&app,&alice,"party.create_from_organization",json!({"organizationId":org["organizationId"],"expectedSourceRevision":1,"customer":true,"supplier":false})).await;
        Self {
            f,
            app,
            alice,
            bob,
            company,
            book: Value::Null,
            ar: Value::Null,
            revenue: Value::Null,
            clearing: Value::Null,
            period: Value::Null,
            party,
            currency: "IDR".into(),
            unit: "whole_idr",
        }
    }
    /// Creates the company's book with the accounts and period the invoice bridge needs.
    pub(super) async fn open_book(&mut self, currency: &str) {
        let (app, alice) = (&self.app, &self.alice);
        let (st, body) = send(app, alice, &create(&self.company, currency)).await;
        assert_eq!(st, StatusCode::OK, "{body}");
        let book = body["receipt"].clone();
        self.ar = account(app, alice, &book, "AR", "asset", "operating").await;
        self.revenue = account(app, alice, &book, "REVENUE", "revenue", "operating").await;
        self.clearing = account(app, alice, &book, "CLEARING", "asset", "none").await;
        self.period=ok(app,alice,json!({"type":"period.open","bookId":book["bookId"],"code":"SOURCE-TEST","startDate":"2000-01-01","endDate":"2100-12-31","reason":"Synthetic period covering source clock without a fixed test date"})).await;
        self.book = book;
        self.currency = currency.into();
        self.unit = if currency == "IDR" {
            "whole_idr"
        } else {
            "minor"
        };
    }
    async fn org(app: &Router, s: &Value, kind: &str, data: Value) -> Value {
        let(st,v)=call(app,"POST","/erp/organizations/commands",json!({"tenant":"acme","mutation":{"requestId":Uuid::new_v4(),"type":kind,"data":data}}),Some(s)).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        v["receipt"].clone()
    }
    fn policy(&self, revision: i32) -> Value {
        self.policy_in(revision, self.unit)
    }
    fn policy_in(&self, revision: i32, unit: &str) -> Value {
        input(
            json!({"type":"invoice_policy.set","bookId":self.book["bookId"],"expectedRevision":revision,"currencyCatalogue":"SIX-2026-09-17","sourceUnit":unit,"mappings":[
            {"role":"receivable","accountId":self.ar["resourceId"],"expectedAccountRevision":1},
            {"role":"revenue","accountId":self.revenue["resourceId"],"expectedAccountRevision":1},
            {"role":"payment_clearing","accountId":self.clearing["resourceId"],"expectedAccountRevision":1}],"reason":"Explicit future invoice unit contract: exact unit factor, clearing is not bank cash"}),
        )
    }
    pub(super) async fn enable(&self) -> Value {
        let (st, v) = send(&self.app, &self.alice, &self.policy(0)).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        v["receipt"].clone()
    }
    async fn trade(&self, command: Value) -> Value {
        let (st, v) = call(
            &self.app,
            "POST",
            "/erp/commerce/commands",
            json!({"tenant":"acme","requestId":Uuid::new_v4(),"command":command}),
            Some(&self.alice),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        v["receipt"].clone()
    }
    pub(super) async fn bill(&self, mut command: Value) -> Value {
        command["commandId"] = json!(Uuid::new_v4());
        let (st, v) = call(
            &self.app,
            "POST",
            "/erp/billing/commands",
            json!({"tenant":"acme","mutation":command}),
            Some(&self.alice),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        v
    }
    pub(super) async fn invoice(&self, code: &str) -> Value {
        self.invoice_amount(code, 15000, 2).await
    }
    async fn invoice_amount(&self, code: &str, price: i64, quantity: i64) -> Value {
        self.invoice_many_amount(code, price, quantity, 1).await
    }
    pub(super) async fn invoice_many_amount(
        &self,
        code: &str,
        price: i64,
        quantity: i64,
        count: usize,
    ) -> Value {
        let mut lines = Vec::new();
        for n in 0..count {
            let p=self.trade(json!({"operation":"product.create_for_company","companyId":self.company["companyId"],"expectedCompanyRevision":1,"sku":format!("{code}-{n}"),"title":code,"unit":"unit","trackStock":false,"price":price})).await;
            lines.push(json!({"productId":p["id"],"quantity":quantity}));
        }
        let o=self.trade(json!({"operation":"order.create_for_company","companyId":self.company["companyId"],"expectedCompanyRevision":1,"partyId":self.party["partyId"],"expectedPartyRevision":1,"lines":lines})).await;
        self.trade(json!({"operation":"order.reserve","id":o["id"],"expectedRevision":1,"holdSeconds":300})).await;
        self.trade(json!({"operation":"order.fulfill","id":o["id"],"expectedRevision":2}))
            .await;
        self.bill(json!({"type":"create_invoice_from_order","invoiceCode":code,"orderId":o["id"],"expectedOrderRevision":3})).await
    }
    pub(super) fn source_draft(&self, source: &Value, code: &str) -> Value {
        let counter = if source["action"] == "record_provider_event" {
            &self.clearing
        } else {
            &self.revenue
        };
        input(
            json!({"type":"invoice_source.draft","sourceId":source["commandId"],"code":code,"accounts":[{"accountId":self.ar["resourceId"],"expectedAccountRevision":1},{"accountId":counter["resourceId"],"expectedAccountRevision":1}],"reason":"Review captured amount, source date and original policy account roles"}),
        )
    }
    pub(super) async fn post_source(&self, source: &Value, code: &str) -> Value {
        let (st, v) = send(&self.app, &self.alice, &self.source_draft(source, code)).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        let (st, posted) = send(&self.app, &self.bob, &action("journal.post", &v["receipt"])).await;
        assert_eq!(st, StatusCode::OK, "{posted}");
        posted["receipt"].clone()
    }
    async fn count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM archava_accounting.invoice_sources")
            .fetch_one(&self.f.admin_pool)
            .await
            .unwrap()
    }
    pub(super) async fn reports(&self) -> Value {
        let (st, v) = call(
            &self.app,
            "GET",
            &format!(
                "/erp/accounting/reports?tenant=acme&bookId={}&from=2000-01-01&to=2100-12-31",
                self.book["bookId"].as_str().unwrap()
            ),
            Value::Null,
            Some(&self.alice),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        v
    }
    pub(super) async fn payment(&self, invoice: &Value, outcome: &str, amount: i64) -> Value {
        let event = Uuid::new_v4().to_string();
        let invoice = invoice["invoiceId"].as_str().unwrap();
        let mut mac = Hmac::<Sha256>::new_from_slice(&[7; 32]).unwrap();
        mac.update(b"billing-provider-secret\0acme\0owned-bank");
        let key = format!("{:x}", mac.finalize().into_bytes());
        let signature = provider_event_digest(
            key.as_bytes(),
            &PaymentEventClaim {
                tenant_id: "acme",
                invoice_id: invoice,
                provider_id: "owned-bank",
                provider_event_id: &event,
                outcome,
                amount_minor: amount,
                currency: &self.currency,
            },
        )
        .unwrap();
        self.bill(json!({"type":"record_provider_event","invoiceId":invoice,"providerId":"owned-bank","providerEventId":event,"outcome":outcome,"amountMinor":amount,"currency":self.currency,"signature":signature})).await
    }
}

#[tokio::test]
async fn real_invoice_sources_post_exact_ar_revenue_clearing_correction_refund_and_void() {
    let s = SourceFixture::new().await;
    let legacy = s.invoice("LEGACY").await;
    s.bill(json!({"type":"issue_invoice","invoiceId":legacy["invoiceId"],"expectedRevision":1,"reason":"Before explicit capture policy"})).await;
    assert_eq!(s.count().await, 0);
    s.enable().await;
    s.bill(json!({"type":"append_correction","invoiceId":legacy["invoiceId"],"amountMinor":-100,"reason":"Legacy remains opening/import work"})).await;
    assert_eq!(s.count().await, 0);
    let invoice = s.invoice("FUTURE").await;
    // Operating authority can capture a source without arbitrary financial grants.
    sqlx::query(
        "UPDATE archava_accounting.memberships SET active=false WHERE principal_id='alice'",
    )
    .execute(&s.f.admin_pool)
    .await
    .unwrap();
    let issue=s.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Issue frozen operating amount"})).await;
    assert_eq!(s.count().await, 1);
    sqlx::query("UPDATE archava_accounting.memberships SET active=true WHERE principal_id='alice'")
        .execute(&s.f.admin_pool)
        .await
        .unwrap();
    let coverage = s.reports().await;
    assert_eq!(coverage["sourceCoverage"]["unpostedCount"], 1);
    let stored: (String, i64) =
        sqlx::query_as("SELECT effect,amount_minor FROM archava_accounting.invoice_sources")
            .fetch_one(&s.f.admin_pool)
            .await
            .unwrap();
    assert_eq!(stored, ("issue".into(), 3_000_000));
    assert_eq!(
        send(&s.app, &s.alice, &action("period.close", &s.period))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let request = s.source_draft(&issue, "ISSUE");
    let (st, draft) = send(&s.app, &s.alice, &request).await;
    assert_eq!(st, StatusCode::OK, "{draft}");
    assert_eq!(draft["receipt"]["sourceId"], issue["commandId"]);
    assert_eq!(
        send(&s.app, &s.alice, &request).await.1["receipt"]["replayed"],
        true
    );
    assert_eq!(
        send(&s.app, &s.alice, &s.source_draft(&issue, "DOUBLE"))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(&s.app, &s.alice, &action("journal.post", &draft["receipt"]))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (st, posted) = send(&s.app, &s.bob, &action("journal.post", &draft["receipt"])).await;
    assert_eq!(st, StatusCode::OK, "{posted}");
    let correction=s.bill(json!({"type":"append_correction","invoiceId":invoice["invoiceId"],"amountMinor":-5000,"reason":"Reviewed customer credit"})).await;
    s.post_source(&correction, "CREDIT").await;
    let r = s.reports().await;
    assert_eq!(r["profitAndLoss"]["revenueMinor"], "2500000");
    assert_eq!(r["balanceSheet"]["assetsMinor"], "2500000");
    let paid = s.payment(&invoice, "succeeded", 25000).await;
    s.post_source(&paid, "COLLECTION").await;
    let r = s.reports().await;
    assert_eq!(r["cashFlow"]["netChangeMinor"], "0");
    let ar = r["trialBalance"]["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["code"] == "AR")
        .unwrap();
    assert_eq!(ar["closingNetMinor"], "0");
    let refund = s.payment(&invoice, "refunded", 25000).await;
    s.post_source(&refund, "REFUND").await;
    let void=s.bill(json!({"type":"void_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":5,"reason":"Cancel after the verified refund"})).await;
    s.post_source(&void, "VOID").await;
    assert_eq!(s.count().await, 5);
    let r = s.reports().await;
    assert_eq!(r["sourceCoverage"]["postedCount"], 5);
    assert_eq!(r["sourceCoverage"]["unpostedCount"], 0);
    assert_eq!(r["balanceSheet"]["assetsMinor"], "0");
    assert_eq!(r["profitAndLoss"]["revenueMinor"], "0");
    assert_eq!(r["trialBalance"]["balanced"], true);
    assert_eq!(
        send(&s.app, &s.alice, &action("period.close", &s.period))
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn source_policy_versions_scope_recovery_and_owner_toggle_preserve_capture() {
    let s = SourceFixture::new().await;
    let policy = s.enable().await;
    let invoice = s.invoice("SOURCE-RECOVERY").await;
    let issue=s.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Capture at policy version one"})).await;
    let (st, v) = send(&s.app, &s.alice, &s.policy(1)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["receipt"]["revision"], 2);
    assert_eq!(
        send(&s.app, &s.alice, &s.policy(1)).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(policy["resourceKind"], "invoice_policy");
    let correction=s.bill(json!({"type":"append_correction","invoiceId":invoice["invoiceId"],"amountMinor":-1,"reason":"Retain original account roles"})).await;
    let versions: Vec<i32> = sqlx::query_scalar(
        "SELECT policy_revision FROM archava_accounting.invoice_sources ORDER BY id",
    )
    .fetch_all(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(versions, vec![1, 1]);
    let request = s.source_draft(&issue, "WITHDRAW");
    let (st, v) = send(&s.app, &s.alice, &request).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        send(&s.app, &s.alice, &action("journal.void", &v["receipt"]))
            .await
            .0,
        StatusCode::OK
    );
    s.post_source(&issue, "REVIEW-AGAIN").await;
    s.post_source(&correction, "CORRECTION").await;
    for module in ["billing", "accounting"] {
        let (st, p) = call(
            &s.app,
            "POST",
            "/own/plan",
            json!({"tenant":"acme","request":{"baseRevision":0,"disable":[module]}}),
            Some(&s.alice),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{p}");
        assert_eq!(p["plan"]["blocked"]["reason"], "mandatory", "{p}");
        assert_eq!(p["plan"]["blocked"]["module"], module, "{p}");
    }
    let reader = login(&s.app, "reader").await;
    assert_eq!(
        send(&s.app, &reader, &s.source_draft(&issue, "NO-AUTHORITY"))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query(
        "UPDATE archava_organizations.memberships SET active=false WHERE principal_id='alice'",
    )
    .execute(&s.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(
        send(&s.app, &s.alice, &request).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn source_draft_race_has_one_live_result_and_closed_source_period_rolls_back_invoice() {
    let s = SourceFixture::new().await;
    s.enable().await;
    let invoice = s.invoice("SOURCE-RACE").await;
    let issue=s.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Source race"})).await;
    let one = s.source_draft(&issue, "RACE-ONE");
    let two = s.source_draft(&issue, "RACE-TWO");
    let (a, b) = tokio::join!(send(&s.app, &s.alice, &one), send(&s.app, &s.alice, &two));
    assert!(
        (a.0 == StatusCode::OK && b.0 == StatusCode::CONFLICT)
            || (b.0 == StatusCode::OK && a.0 == StatusCode::CONFLICT),
        "{a:?} {b:?}"
    );
    let winner = if a.0 == StatusCode::OK { a.1 } else { b.1 };
    assert_eq!(
        send(&s.app, &s.bob, &action("journal.post", &winner["receipt"]))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        send(&s.app, &s.alice, &action("period.close", &s.period))
            .await
            .0,
        StatusCode::OK
    );
    let id = Uuid::new_v4();
    let(st,v)=call(&s.app,"POST","/erp/billing/commands",json!({"tenant":"acme","mutation":{"type":"append_correction","commandId":id,"invoiceId":invoice["invoiceId"],"amountMinor":-1,"reason":"Closed source period must refuse atomically"}}),Some(&s.alice)).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let rows:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM archava_billing.commands WHERE request_id=$1),(SELECT count(*) FROM archava_billing.invoice_corrections)").bind(id).fetch_one(&s.f.admin_pool).await.unwrap();
    assert_eq!(rows, (0, 0));
    assert_eq!(s.count().await, 1);
    let mut reopen = action("period.reopen", &s.period);
    reopen["command"]["expectedRevision"] = json!(2);
    assert_eq!(send(&s.app, &s.alice, &reopen).await.0, StatusCode::OK);
    let reverse = input(
        json!({"type":"journal.reverse","id":winner["receipt"]["resourceId"],"expectedRevision":2,"code":"UNBOUND-REVERSE","date":"2026-11-07","sourceRef":"Cannot bypass the source credit workflow","reason":"Attempt unsupported source reversal"}),
    );
    assert_eq!(
        send(&s.app, &s.alice, &reverse).await.0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn source_policy_rejects_unit_account_claims_and_handles_zero_and_large_amounts_explicitly() {
    let s = SourceFixture::new().await;
    let mut forged = s.policy(0);
    forged["command"]["sourceUnit"] = json!("iso_minor");
    assert_eq!(
        send(&s.app, &s.alice, &forged).await.0,
        StatusCode::BAD_REQUEST
    );
    let mut forged = s.policy(0);
    forged["command"]["approved"] = json!(true);
    assert_eq!(
        send(&s.app, &s.alice, &forged).await.0,
        StatusCode::BAD_REQUEST
    );
    let mut forged = s.policy(0);
    forged["command"]["mappings"][0]["expectedAccountRevision"] = json!(2);
    assert_eq!(
        send(&s.app, &s.alice, &forged).await.0,
        StatusCode::CONFLICT
    );
    let cash = account(&s.app, &s.alice, &s.book, "ACTUAL-BANK", "asset", "cash").await;
    let mut forged = s.policy(0);
    forged["command"]["mappings"][2]["accountId"] = cash["resourceId"].clone();
    assert_eq!(
        send(&s.app, &s.alice, &forged).await.0,
        StatusCode::CONFLICT,
        "Provider collections must not pretend to be settled bank cash"
    );
    s.enable().await;
    let invoice = s.invoice_amount("SOURCE-ZERO", 0, 1).await;
    let zero=s.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Explicit zero-value commercial source"})).await;
    assert_eq!(s.count().await, 1);
    assert_eq!(
        send(&s.app, &s.alice, &s.source_draft(&zero, "ZERO-JOURNAL"))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let r = s.reports().await;
    assert_eq!(r["sourceCoverage"]["noMonetaryEffectCount"], 1);
    assert_eq!(r["sourceCoverage"]["unpostedCount"], 0);
    let other = company(&s.app, &s.alice, "OTHER").await;
    let mut book_request = create(&other, "IDR");
    book_request["command"]["code"] = json!("BOOK-OTHER");
    assert_eq!(
        send(&s.app, &s.alice, &book_request).await.0,
        StatusCode::OK
    );
    let path = format!(
        "/erp/accounting/workspace?tenant=acme&companyId={}&collection=invoice_sources&id={}",
        other["companyId"].as_str().unwrap(),
        zero["commandId"].as_str().unwrap()
    );
    assert_eq!(
        call(&s.app, "GET", &path, Value::Null, Some(&s.alice))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let large = s
        .invoice_many_amount("SOURCE-TOO-LARGE", 1000000000, 10000, 10)
        .await;
    let id = Uuid::new_v4();
    let(st,v)=call(&s.app,"POST","/erp/billing/commands",json!({"tenant":"acme","mutation":{"type":"issue_invoice","commandId":id,"invoiceId":large["invoiceId"],"expectedRevision":1,"reason":"Exceeds exact book posting bound"}}),Some(&s.alice)).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert_eq!(s.count().await, 1);
    let state: String =
        sqlx::query_scalar("SELECT status FROM archava_billing.invoices WHERE id=$1")
            .bind(Uuid::parse_str(large["invoiceId"].as_str().unwrap()).unwrap())
            .fetch_one(&s.f.admin_pool)
            .await
            .unwrap();
    assert_eq!(state, "draft");
    assert_eq!(
        send(&s.app, &s.alice, &action("period.close", &s.period))
            .await
            .0,
        StatusCode::OK
    );
    let changed =
        sqlx::query("UPDATE archava_accounting.invoice_sources SET amount_minor=100 WHERE id=$1")
            .bind(Uuid::parse_str(zero["commandId"].as_str().unwrap()).unwrap())
            .execute(&s.f.admin_pool)
            .await;
    assert!(
        changed.is_err(),
        "Captured sources are immutable even to direct SQL updates"
    );
}

#[tokio::test]
async fn reusable_financial_adapter_commits_the_source_link_receipt_and_audit_atomically() {
    use archava_adapters::{erp::accounting as store, rls::TenantTransaction};
    use archava_domain::erp::{accounting, purchasing::PurchasePrincipal};
    let s = SourceFixture::new().await;
    s.enable().await;
    let invoice = s.invoice("ADAPTER-SOURCE").await;
    let issue=s.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Reusable adapter source input"})).await;
    let body = s.source_draft(&issue, "ADAPTER-DRAFT");
    let mutation =
        accounting::parse(&json!({"requestId":body["requestId"],"command":body["command"]}))
            .unwrap();
    let tenant = archava_core::tenant::TenantId::new("acme").unwrap();
    let mut tx = TenantTransaction::begin(
        &s.f.runtime_pool,
        "archava_accounting",
        "accounting",
        tenant,
    )
    .await
    .unwrap();
    let conn = tx.checked_executor().await.unwrap();
    let p = PurchasePrincipal::human("alice");
    let access = store::access(conn, "acme", &p).await.unwrap();
    let receipt = store::record(conn, "acme", &p, &access, &"a".repeat(64), &mutation)
        .await
        .unwrap();
    assert_eq!(receipt.source_id.as_deref(), issue["commandId"].as_str());
    tx.commit()
        .await
        .expect("Source, journal, immutable audit and matching source receipt commit together");
    let rows:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM archava_accounting.journal_sources),(SELECT count(*) FROM archava_accounting.journals),(SELECT count(*) FROM archava_accounting.audit WHERE operation='invoice_source.draft')").fetch_one(&s.f.admin_pool).await.unwrap();
    assert_eq!(rows, (1, 1, 1));
}

#[tokio::test]
async fn startup_refuses_disabled_invoice_capture_and_source_audit_guards() {
    let s = SourceFixture::new().await;
    for (table, trigger) in [
        (
            "archava_billing.commands",
            "capture_financial_invoice_source",
        ),
        (
            "archava_accounting.journal_sources",
            "source_link_claim_complete",
        ),
        ("archava_accounting.periods", "invoice_source_close_guard"),
    ] {
        let disable = format!("ALTER TABLE {table} DISABLE TRIGGER {trigger}");
        sqlx::query(sqlx::AssertSqlSafe(disable.as_str()))
            .execute(&s.f.owner_pool)
            .await
            .unwrap();
        let ready = AppState::try_new_all(
            s.f.runtime_pool.clone(),
            ledger(),
            AllReadiness {
                commerce: &s.f.commerce_ledger,
                identity: &s.f.identity_ledger,
                crm: &s.f.crm_ledger,
                owner: &s.f.owner_ledger,
                purchasing: &s.f.purchasing_ledger,
                additional_erp: &s.f.erp_readiness(),
            },
            &[7; 32],
        )
        .await;
        let failure = ready.expect_err("Missing source capture/control must fail startup");
        assert!(
            format!("{failure:?}").contains("invoice source guard coverage is incomplete"),
            "{failure:?}"
        );
        let enable = format!("ALTER TABLE {table} ENABLE TRIGGER {trigger}");
        sqlx::query(sqlx::AssertSqlSafe(enable.as_str()))
            .execute(&s.f.owner_pool)
            .await
            .unwrap();
    }
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT archava_accounting.source_guards_complete()")
            .fetch_one(&s.f.runtime_pool)
            .await
            .unwrap()
    );
    let mut reserved = draft(
        &s.book,
        "MANUAL-SOURCE-CLAIM",
        vec![line(&s.ar, "100", "0"), line(&s.revenue, "0", "100")],
    );
    reserved["command"]["sourceRef"] = json!("billing:unbound-claim");
    assert_eq!(
        send(&s.app, &s.alice, &reserved).await.0,
        StatusCode::BAD_REQUEST
    );
}

/// Trade currency stages 3 and 4: an invoice made from a fulfilled order of a
/// company whose book is USD is in USD with the order's amounts unchanged, and
/// under the `minor` unit it is captured and posted at exactly that amount.
#[tokio::test]
async fn an_invoice_from_a_non_idr_order_takes_the_order_currency() {
    let x = SourceFixture::new_in("USD").await;
    let before = x.invoice_amount("USD-BEFORE", 1999, 2).await;
    let id = Uuid::parse_str(before["invoiceId"].as_str().unwrap()).unwrap();
    let (currency, gross, order_currency): (String, i64, String) = sqlx::query_as(
        "SELECT i.currency,i.gross_minor,o.currency FROM archava_billing.invoices i \
         JOIN archava_commerce.orders o ON o.tenant_id=i.tenant_id AND o.id=i.source_order_id \
         WHERE i.id=$1",
    )
    .bind(id)
    .fetch_one(&x.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(
        (currency.as_str(), gross, order_currency.as_str()),
        ("USD", 3998, "USD")
    );
    x.bill(json!({"type":"issue_invoice","invoiceId":before["invoiceId"],"expectedRevision":1,"reason":"Issued before any policy"})).await;
    assert_eq!(x.count().await, 0, "no source is captured without a policy");

    // Whole Rupiah is not a unit of a USD book; the minor unit is.
    let (status, refused) = send(&x.app, &x.alice, &x.policy_in(0, "whole_idr")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    x.enable().await;
    let (st, snapshot) = snapshot(&x.app, &x.alice, &x.company).await;
    assert_eq!(st, StatusCode::OK, "{snapshot}");
    assert_eq!(snapshot["invoicePolicy"]["sourceUnit"], "minor");
    let notice = snapshot["invoicePostingNotice"].as_str().unwrap();
    assert!(
        notice.contains("USD minor-unit") && !notice.contains("whole-IDR"),
        "{notice}"
    );

    let invoice = x.invoice_amount("USD-INV", 1999, 2).await;
    let issue=x.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Issue under the minor unit"})).await;
    let stored: Vec<(String, i64, String)> = sqlx::query_as(
        "SELECT effect,amount_minor,policy_snapshot->>'source_unit' FROM archava_accounting.invoice_sources",
    )
    .fetch_all(&x.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(
        stored,
        vec![("issue".into(), 3998, "minor".into())],
        "USD 39.98 is 3998 ledger minor units, not 399800"
    );
    x.post_source(&issue, "USD-ISSUE").await;
    let lines: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT a.code,l.debit_minor,l.credit_minor FROM archava_accounting.journal_lines l \
         JOIN archava_accounting.accounts a ON a.tenant_id=l.tenant_id AND a.id=l.account_id ORDER BY a.code",
    )
    .fetch_all(&x.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(
        lines,
        vec![("AR".into(), 3998, 0), ("REVENUE".into(), 0, 3998)]
    );
    let r = x.reports().await;
    assert_eq!(r["profitAndLoss"]["revenueMinor"], "3998");
    assert_eq!(r["balanceSheet"]["assetsMinor"], "3998");
    assert_eq!(r["sourceCoverage"]["postedCount"], 1);
    assert_eq!(r["sourceCoverage"]["unpostedCount"], 0);
    let receivable =
        subledger::accepted(&x.app, &x.alice, &x.book, "receivable", "2100-12-31").await;
    assert_eq!(receivable["book"]["currency"], "USD");
    assert_eq!(receivable["totals"]["netOutstandingMinor"], "3998");
    assert_eq!(receivable["positionControlDifferenceMinor"], "0");
    assert_eq!(receivable["controls"][0]["differenceMinor"], "0");
    assert_eq!(receivable["postedSourceControlsAgree"], true);

    // A correction and a verified collection keep the unit of the issue.
    let correction=x.bill(json!({"type":"append_correction","invoiceId":invoice["invoiceId"],"amountMinor":-98,"reason":"Reviewed customer credit"})).await;
    x.post_source(&correction, "USD-CREDIT").await;
    let paid = x.payment(&invoice, "succeeded", 3900).await;
    x.post_source(&paid, "USD-COLLECTION").await;
    let amounts: Vec<(String, i64)> = sqlx::query_as(
        "SELECT effect,amount_minor FROM archava_accounting.invoice_sources ORDER BY occurred_at",
    )
    .fetch_all(&x.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(
        amounts,
        vec![
            ("issue".into(), 3998),
            ("correction".into(), -98),
            ("collection".into(), 3900)
        ]
    );
    let r = x.reports().await;
    assert_eq!(r["profitAndLoss"]["revenueMinor"], "3900");
    assert_eq!(r["trialBalance"]["balanced"], true);
    let settled = subledger::accepted(&x.app, &x.alice, &x.book, "receivable", "2100-12-31").await;
    assert_eq!(settled["totals"]["netOutstandingMinor"], "0");
    assert_eq!(settled["postedSourceControlsAgree"], true);

    // The earlier invoice was issued before the policy and is never imported.
    x.bill(json!({"type":"append_correction","invoiceId":before["invoiceId"],"amountMinor":-1,"reason":"Outside the captured sources"})).await;
    assert_eq!(x.count().await, 3);

    // The replaced functions keep the triggers startup readiness identifies.
    for check in [
        "source_guards_complete",
        "purchase_guards_complete",
        "stock_guards_complete",
    ] {
        let query = format!("SELECT archava_accounting.{check}()");
        assert!(
            sqlx::query_scalar::<_, bool>(sqlx::AssertSqlSafe(query.as_str()))
                .fetch_one(&x.f.runtime_pool)
                .await
                .unwrap(),
            "{check}"
        );
    }
    let _: Router = app(&x.f).await;
    let checks: Vec<String> = sqlx::query_scalar(
        "SELECT conname::text FROM pg_constraint WHERE contype='c' AND pg_get_constraintdef(oid) LIKE '%source_unit%' ORDER BY 1",
    )
    .fetch_all(&x.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(
        checks,
        [
            "invoice_policy_source_unit",
            "purchase_policy_source_unit",
            "stock_policy_source_unit"
        ]
    );
}

/// A zero-decimal currency: 3 x JPY 1500 is 4500 ledger minor units. A factor
/// of 100 here would post JPY 450,000.
#[tokio::test]
async fn a_zero_decimal_book_posts_minor_amounts_unchanged() {
    let x = SourceFixture::new_in("JPY").await;
    let minor_units: i16 = sqlx::query_scalar("SELECT minor_units FROM archava_accounting.books")
        .fetch_one(&x.f.admin_pool)
        .await
        .unwrap();
    assert_eq!(minor_units, 0);
    x.enable().await;
    let invoice = x.invoice_amount("JPY-INV", 1500, 3).await;
    let issue=x.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Issue in a zero-decimal currency"})).await;
    let stored: (String, i64) =
        sqlx::query_as("SELECT effect,amount_minor FROM archava_accounting.invoice_sources")
            .fetch_one(&x.f.admin_pool)
            .await
            .unwrap();
    assert_eq!(stored, ("issue".into(), 4500));
    x.post_source(&issue, "JPY-ISSUE").await;
    let r = x.reports().await;
    assert_eq!(r["profitAndLoss"]["revenueMinor"], "4500");
    assert_eq!(r["balanceSheet"]["assetsMinor"], "4500");
    assert_eq!(r["trialBalance"]["balanced"], true);
    let receivable =
        subledger::accepted(&x.app, &x.alice, &x.book, "receivable", "2100-12-31").await;
    assert_eq!(receivable["book"]["minorUnits"], 0);
    assert_eq!(receivable["totals"]["netOutstandingMinor"], "4500");
    assert_eq!(receivable["postedSourceControlsAgree"], true);
    // The ledger bound applies to the unconverted amount: the total an IDR book
    // refuses at factor 100 is within the bound at factor 1.
    let large = x
        .invoice_many_amount("JPY-LARGE", 1000000000, 10000, 10)
        .await;
    x.bill(json!({"type":"issue_invoice","invoiceId":large["invoiceId"],"expectedRevision":1,"reason":"Within the exact book posting bound"})).await;
    let amount: i64 = sqlx::query_scalar(
        "SELECT amount_minor FROM archava_accounting.invoice_sources WHERE invoice_id=$1",
    )
    .bind(Uuid::parse_str(large["invoiceId"].as_str().unwrap()).unwrap())
    .fetch_one(&x.f.admin_pool)
    .await
    .unwrap();
    assert_eq!(amount, 100_000_000_000_000);
}

/// The unit must fit the book: the `minor` unit is not valid on an IDR book.
#[tokio::test]
async fn the_minor_unit_is_refused_on_an_idr_book() {
    let s = SourceFixture::new().await;
    let (status, refused) = send(&s.app, &s.alice, &s.policy_in(0, "minor")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    s.enable().await;
    let (st, snapshot) = snapshot(&s.app, &s.alice, &s.company).await;
    assert_eq!(st, StatusCode::OK, "{snapshot}");
    assert_eq!(snapshot["invoicePolicy"]["sourceUnit"], "whole_idr");
    assert!(snapshot["invoicePostingNotice"]
        .as_str()
        .unwrap()
        .contains("whole-IDR unit contract"));
    // The unit of an existing policy cannot be changed either.
    let (status, refused) = send(&s.app, &s.alice, &s.policy_in(1, "minor")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
}

/// A company that traded in IDR before it created a USD book. Its IDR invoice
/// cannot be issued while the book has an invoice policy: the issue is refused
/// as a conflict and nothing is captured. An IDR invoice issued before the
/// policy stays outside the ledger and can still be corrected.
#[tokio::test]
async fn an_idr_invoice_of_a_company_with_a_later_usd_book_is_refused_not_captured() {
    let mut x = SourceFixture::trading().await;
    let issued = x.invoice_amount("IDR-ISSUED", 15000, 2).await;
    let draft = x.invoice_amount("IDR-DRAFT", 15000, 2).await;
    x.open_book("USD").await;
    x.bill(json!({"type":"issue_invoice","invoiceId":issued["invoiceId"],"expectedRevision":1,"reason":"No policy yet, so nothing is captured"})).await;
    x.enable().await;
    let command = Uuid::new_v4();
    let (st,v)=call(&x.app,"POST","/erp/billing/commands",json!({"tenant":"acme","mutation":{"type":"issue_invoice","commandId":command,"invoiceId":draft["invoiceId"],"expectedRevision":1,"reason":"Rupiah invoice against a USD book"}}),Some(&x.alice)).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let (state, currency): (String, String) =
        sqlx::query_as("SELECT status,currency FROM archava_billing.invoices WHERE id=$1")
            .bind(Uuid::parse_str(draft["invoiceId"].as_str().unwrap()).unwrap())
            .fetch_one(&x.f.admin_pool)
            .await
            .unwrap();
    assert_eq!((state.as_str(), currency.as_str()), ("draft", "IDR"));
    let recorded: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_billing.commands WHERE request_id=$1")
            .bind(command)
            .fetch_one(&x.f.admin_pool)
            .await
            .unwrap();
    assert_eq!(recorded, 0, "the refused issue leaves no command");
    assert_eq!(x.count().await, 0);
    x.bill(json!({"type":"append_correction","invoiceId":issued["invoiceId"],"amountMinor":-100,"reason":"Issued before the policy, still correctable"})).await;
    assert_eq!(x.count().await, 0);
    // New trade is in USD and is captured.
    let invoice = x.invoice_amount("USD-NEW", 1999, 1).await;
    x.bill(json!({"type":"issue_invoice","invoiceId":invoice["invoiceId"],"expectedRevision":1,"reason":"Book currency invoice"})).await;
    let amount: i64 =
        sqlx::query_scalar("SELECT amount_minor FROM archava_accounting.invoice_sources")
            .fetch_one(&x.f.admin_pool)
            .await
            .unwrap();
    assert_eq!(amount, 1999);
}
