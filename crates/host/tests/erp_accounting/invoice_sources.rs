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
}
impl SourceFixture {
    pub(super) async fn new() -> Self {
        let (f, app, alice, bob, company) = setup().await;
        sqlx::query("UPDATE archava_organizations.memberships SET permissions=permissions||ARRAY['parties.read','parties.write'] WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
        sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','alice','human',$1,'all')").bind(archava_adapters::erp::COMMERCE_PERMISSION_VOCABULARY).execute(&f.admin_pool).await.unwrap();
        sqlx::query("INSERT INTO archava_billing.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('acme','alice','human',$1,'all')").bind(archava_domain::erp::billing::BILLING_PERMISSIONS).execute(&f.admin_pool).await.unwrap();
        sqlx::query("INSERT INTO archava_owner.owner_grants(tenant_id,principal_id,permissions) VALUES('acme','alice',ARRAY['config.read','config.apply'])").execute(&f.admin_pool).await.unwrap();
        let (st, body) = send(&app, &alice, &create(&company, "IDR")).await;
        assert_eq!(st, StatusCode::OK, "{body}");
        let book = body["receipt"].clone();
        let ar = account(&app, &alice, &book, "AR", "asset", "operating").await;
        let revenue = account(&app, &alice, &book, "REVENUE", "revenue", "operating").await;
        let clearing = account(&app, &alice, &book, "CLEARING", "asset", "none").await;
        let period=ok(&app,&alice,json!({"type":"period.open","bookId":book["bookId"],"code":"SOURCE-TEST","startDate":"2000-01-01","endDate":"2100-12-31","reason":"Synthetic period covering source clock without a fixed test date"})).await;
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
            book,
            ar,
            revenue,
            clearing,
            period,
            party,
        }
    }
    async fn org(app: &Router, s: &Value, kind: &str, data: Value) -> Value {
        let(st,v)=call(app,"POST","/erp/organizations/commands",json!({"tenant":"acme","mutation":{"requestId":Uuid::new_v4(),"type":kind,"data":data}}),Some(s)).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        v["receipt"].clone()
    }
    fn policy(&self, revision: i32) -> Value {
        input(
            json!({"type":"invoice_policy.set","bookId":self.book["bookId"],"expectedRevision":revision,"currencyCatalogue":"SIX-2026-09-17","sourceUnit":"whole_idr","mappings":[
            {"role":"receivable","accountId":self.ar["resourceId"],"expectedAccountRevision":1},
            {"role":"revenue","accountId":self.revenue["resourceId"],"expectedAccountRevision":1},
            {"role":"payment_clearing","accountId":self.clearing["resourceId"],"expectedAccountRevision":1}],"reason":"Explicit future invoice unit contract: whole Rupiah, exact factor 100, clearing is not bank cash"}),
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
                currency: "IDR",
            },
        )
        .unwrap();
        self.bill(json!({"type":"record_provider_event","invoiceId":invoice,"providerId":"owned-bank","providerEventId":event,"outcome":outcome,"amountMinor":amount,"currency":"IDR","signature":signature})).await
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
