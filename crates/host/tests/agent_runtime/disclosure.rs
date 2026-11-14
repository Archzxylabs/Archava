//! Inspect real owned-provider request bytes, not only the saved consent types.
use super::*;
use axum::{extract::State, routing::post as route_post, Json};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
struct Wire {
    calls: Arc<Mutex<Vec<Value>>>,
    schemas: Arc<Mutex<Vec<Value>>>,
    response: Value,
    revoke: Option<(Router, Value)>,
    status: StatusCode,
}
struct Model {
    app: Router,
    task: tokio::task::JoinHandle<()>,
    calls: Arc<Mutex<Vec<Value>>>,
    /// The response schema sent with each call, in call order.
    schemas: Arc<Mutex<Vec<Value>>>,
}
impl Drop for Model {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn model(state: AppState, response: Value, revoke: Option<(Router, Value)>) -> Model {
    model_status(state, response, revoke, StatusCode::OK).await
}
async fn model_status(
    state: AppState,
    response: Value,
    revoke: Option<(Router, Value)>,
    status: StatusCode,
) -> Model {
    async fn wire(State(state): State<Wire>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
        let input: Value =
            serde_json::from_str(body["contents"][0]["parts"][0]["text"].as_str().unwrap())
                .unwrap();
        state
            .schemas
            .lock()
            .await
            .push(body["generationConfig"]["responseJsonSchema"].clone());
        let call_index = {
            let mut calls = state.calls.lock().await;
            calls.push(input);
            calls.len() - 1
        };
        if let Some((app, s)) = &state.revoke {
            if let Some(case_id) = state.response.get("__ownedCaseReassignment") {
                if call_index == 0 {
                    post(app,"/erp/support/commands",json!({"tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4(),"command":{"operation":"case.assign","id":case_id,"expectedRevision":2,"assigneeId":"bob","reason":"Synthetic assignment change during a model request"}}}),s).await;
                }
            } else {
                let (st, c) = common::call(
                    app,
                    "GET",
                    "/own/config?tenant=acme-retail",
                    Value::Null,
                    Some(s),
                )
                .await;
                assert_eq!(st, StatusCode::OK);
                let req = json!({"baseRevision":c["config"]["revision"],"aiDisclosure":{"kind":"revoke"}});
                let p = post(
                    app,
                    "/own/plan",
                    json!({"tenant":"acme-retail","request":req.clone()}),
                    s,
                )
                .await;
                post(app,"/own/apply",json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"request":req,"confirmation":{"baseRevision":c["config"]["revision"],"planHash":p["plan"]["planHash"]},"disclosureApproval":p["disclosureApproval"]}),s).await;
            }
        }
        if state.status != StatusCode::OK {
            return (state.status, Json(state.response));
        }
        let response = state.response["__ownedSequence"]
            .as_array()
            .and_then(|responses| responses.get(call_index))
            .unwrap_or(&state.response);
        (
            StatusCode::OK,
            Json(
                json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":response.to_string()}]}}]}),
            ),
        )
    }
    let calls = Arc::new(Mutex::new(Vec::new()));
    let schemas = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let fixture = Router::new()
        .route("/v1beta/models/{model}", route_post(wire))
        .with_state(Wire {
            calls: calls.clone(),
            schemas: schemas.clone(),
            status,
            response,
            revoke,
        });
    let task = tokio::spawn(async move { axum::serve(listener, fixture).await.unwrap() });
    let app = archava_host::router(
        state.with_agent_config(archava_host::agent::AgentConfig {
            planner: Some(
                archava_providers::agent::GeminiPlanner::new(
                    &endpoint,
                    "owned-privacy-model",
                    "owned-safe-key",
                )
                .unwrap(),
            ),
        }),
    );
    Model {
        app,
        task,
        calls,
        schemas,
    }
}
fn read_response() -> Value {
    json!({"summary":"Reviewed the approved bounded records.","questions":[],"steps":[]})
}
async fn allow(app: &Router, s: &Value, areas: Value, classes: Value, companies: Value) {
    let (st, c) = common::call(
        app,
        "GET",
        "/own/config?tenant=acme-retail",
        Value::Null,
        Some(s),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{c}");
    let request = json!({"baseRevision":c["config"]["revision"],"aiDisclosure":{"kind":"replace","policy":{"providerBinding":c["aiProvider"]["binding"],"modules":areas,"dataClasses":classes,"companies":companies,"includeWorkspaceRecords":true}}});
    let p = post(
        app,
        "/own/plan",
        json!({"tenant":"acme-retail","request":request.clone()}),
        s,
    )
    .await;
    assert!(p["plan"]["blocked"].is_null(), "{p}");
    post(app,"/own/apply",json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"request":request,"confirmation":{"baseRevision":c["config"]["revision"],"planHash":p["plan"]["planHash"]},"disclosureApproval":p["disclosureApproval"]}),s).await;
}
fn limited_classes() -> Value {
    json!(["conversation_text", "record_references", "business_facts"])
}
async fn product(
    app: &Router,
    s: &Value,
    sku: &str,
    title: &str,
    company: Option<&Value>,
) -> Value {
    let mut command = json!({"operation":"product.create","sku":sku,"title":title,"unit":"unit","trackStock":false,"price":77777});
    if let Some(company) = company {
        command["operation"] = json!("product.create_for_company");
        command["companyId"] = company.clone();
        command["expectedCompanyRevision"] = json!(1);
    }
    post(
        app,
        "/erp/commerce/commands",
        json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"command":command}),
        s,
    )
    .await["receipt"]
        .clone()
}
async fn company(app: &Router, s: &Value, code: &str) -> Value {
    let o=post(app,"/erp/organizations/commands",json!({"tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4(),"type":"create_organization","data":{"name":format!("Company {code}"),"legalName":format!("Company {code} Official")}}}),s).await;
    post(app,"/erp/organizations/commands",json!({"tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4(),"type":"company.create_from_organization","data":{"code":code,"organizationId":o["receipt"]["organizationId"],"expectedSourceRevision":1,"reason":"Synthetic scope test"}}}),s).await["receipt"]["companyId"].clone()
}
#[tokio::test]
async fn no_disclosure_sends_zero_provider_requests_and_manual_actions_still_commit() {
    let (f, state, _app, s, _) = setup().await;
    let m = model(state, read_response(), None).await;
    let r = post(&m.app, "/api/agent/turns", turn("Review products."), &s).await;
    assert_eq!(r["planner"]["providerId"], "archava-agent-local");
    let mut input = turn("Prepare a reviewed product.");
    input["action"] = json!({"tool":"commerce.product.create","values":{"sku":"NO-EGRESS","title":"Locally approved goods","unit":"unit","trackStock":false,"price":15000}});
    let r = post(&m.app, "/api/agent/turns", input, &s).await;
    let result=post(&m.app,"/api/agent/approve",json!({"tenant":"acme-retail","goalId":r["goalId"],"planHash":r["goal"]["plan_hash"],"confirmed":true}),&s).await;
    assert_eq!(result["goal"]["state"], "completed", "{result}");
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_commerce.products WHERE sku='NO-EGRESS'")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    assert!(m.calls.lock().await.is_empty());
    let attempts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_owner.agent_model_attempts")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(attempts, 0);
}
#[tokio::test]
async fn actual_provider_payload_withholds_unselected_names_money_modules_and_credentials() {
    let (_f, state, app, s, _) = setup().await;
    let p = product(&app, &s, "PRIVATE-SKU", "PRIVATE-CUSTOMER-GOODS", None).await;
    let m = model(state, read_response(), None).await;
    allow(
        &m.app,
        &s,
        json!(["commerce"]),
        limited_classes(),
        json!([]),
    )
    .await;
    let r = post(
        &m.app,
        "/api/agent/turns",
        turn("Review products sk-proj-owned-sensitive-credential123."),
        &s,
    )
    .await;
    assert_eq!(r["planner"]["providerId"], "gemini");
    let calls = m.calls.lock().await;
    assert_eq!(calls.len(), 1);
    let payload = calls[0].to_string();
    for private in [
        "PRIVATE-SKU",
        "PRIVATE-CUSTOMER-GOODS",
        "77777",
        "sk-proj-owned-sensitive-credential123",
    ] {
        assert!(!payload.contains(private), "{private}: {payload}");
    }
    let row = &calls[0]["authorizedContext"]["sources"][0]["data"]["products"][0];
    assert_eq!(row["id"], p["id"]);
    assert_eq!(row["revision"], 1);
    assert!(row.get("title").is_none());
    assert!(row.get("price").is_none());
    assert!(calls[0]["history"].as_array().unwrap().is_empty());
    assert!(calls[0]["preferences"].as_array().unwrap().is_empty());
    assert!(
        r["goal"]["context"]["sources"]
            .to_string()
            .contains("PRIVATE-CUSTOMER-GOODS"),
        "Local review retains its authorized facts"
    );
}
#[tokio::test]
async fn company_consent_filters_real_organization_records_even_with_workspace_permission() {
    let (_f, state, app, s, _) = setup().await;
    let a = company(&app, &s, "APPROVED").await;
    let b = company(&app, &s, "WITHHELD").await;
    product(&app, &s, "A-GOODS", "Approved company goods", Some(&a)).await;
    product(&app, &s, "B-GOODS", "PRIVATE-OTHER-COMPANY-GOODS", Some(&b)).await;
    let m = model(state, read_response(), None).await;
    allow(
        &m.app,
        &s,
        json!(["commerce", "organizations"]),
        json!([
            "conversation_text",
            "record_references",
            "business_facts",
            "names"
        ]),
        json!([a.clone()]),
    )
    .await;
    let mut input = turn("Review products for the selected company.");
    input["companyId"] = a.clone();
    post(&m.app, "/api/agent/turns", input, &s).await;
    let calls = m.calls.lock().await;
    assert_eq!(calls.len(), 1);
    let payload = calls[0].to_string();
    assert!(payload.contains(a.as_str().unwrap()));
    for private in [
        b.as_str().unwrap(),
        "PRIVATE-OTHER-COMPANY-GOODS",
        "Company WITHHELD Official",
        "B-GOODS",
    ] {
        assert!(!payload.contains(private), "{private}: {payload}");
    }
}
#[tokio::test]
async fn policy_narrowing_never_resends_old_thread_text_or_saved_preferences() {
    let (_f, state, app, s, _) = setup().await;
    product(
        &app,
        &s,
        "PRIVATE-HISTORY-SKU",
        "PRIVATE-HISTORY-TITLE",
        None,
    )
    .await;
    let m = model(state, read_response(), None).await;
    allow(
        &m.app,
        &s,
        json!(["commerce"]),
        json!([
            "conversation_text",
            "record_references",
            "business_facts",
            "names",
            "notes",
            "financial_amounts"
        ]),
        json!([]),
    )
    .await;
    let initial = post(
        &m.app,
        "/api/agent/turns",
        turn("Review products PRIVATE-OLD-UTTERANCE."),
        &s,
    )
    .await;
    let mut remember = turn("Remember: PRIVATE-SAVED-PREFERENCE.");
    remember["threadId"] = initial["threadId"].clone();
    post(&m.app, "/api/agent/turns", remember, &s).await;
    allow(
        &m.app,
        &s,
        json!(["commerce"]),
        limited_classes(),
        json!([]),
    )
    .await;
    let mut followup = turn("Review products again.");
    followup["threadId"] = initial["threadId"].clone();
    post(&m.app, "/api/agent/turns", followup, &s).await;
    let calls = m.calls.lock().await;
    assert_eq!(calls.len(), 2);
    assert!(calls[0].to_string().contains("PRIVATE-HISTORY-TITLE"));
    for private in [
        "PRIVATE-HISTORY-TITLE",
        "PRIVATE-HISTORY-SKU",
        "PRIVATE-OLD-UTTERANCE",
        "PRIVATE-SAVED-PREFERENCE",
    ] {
        assert!(
            !calls[1].to_string().contains(private),
            "{private}: {}",
            calls[1]
        );
    }
}
#[tokio::test]
async fn revocation_during_a_model_request_stops_lookup_and_prevents_a_second_disclosure() {
    let (f, state, app, s, _) = setup().await;
    product(&app, &s, "REVOCATION", "Revocation fixture", None).await;
    let response = json!({"summary":"Read one more record","questions":[],"steps":[],"lookups":[{"module":"commerce","collection":"products","query":"Revocation"}]});
    let m = model(state, response, Some((app, s.clone()))).await;
    allow(
        &m.app,
        &s,
        json!(["commerce"]),
        limited_classes(),
        json!([]),
    )
    .await;
    let (st, r) = common::call(
        &m.app,
        "POST",
        "/api/agent/turns",
        turn("Review products."),
        Some(&s),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{r}");
    assert_eq!(r["reason"], "agent_disclosure_changed");
    assert_eq!(m.calls.lock().await.len(), 1);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_owner.agent_goals")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
#[tokio::test]
async fn a_provider_cannot_expand_into_an_unapproved_module() {
    let (_f, state, _app, s, _) = setup().await;
    let m=model(state,json!({"summary":"Attempt an unapproved financial read","questions":[],"steps":[],"lookups":[{"module":"billing","collection":"invoices","query":"private"}]}),None).await;
    allow(
        &m.app,
        &s,
        json!(["commerce"]),
        limited_classes(),
        json!([]),
    )
    .await;
    let (st, r) = common::call(
        &m.app,
        "POST",
        "/api/agent/turns",
        turn("Review products and invoices."),
        Some(&s),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{r}");
    assert_eq!(r["reason"], "agent_lookup_module_unavailable");
    assert_eq!(m.calls.lock().await.len(), 1);
}
async fn enable_finance(app: &Router, s: &Value) {
    let (st, c) = common::call(
        app,
        "GET",
        "/own/config?tenant=acme-retail",
        Value::Null,
        Some(s),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let request = json!({"baseRevision":c["config"]["revision"],"enable":["accounting"]});
    let p = post(
        app,
        "/own/plan",
        json!({"tenant":"acme-retail","request":request.clone()}),
        s,
    )
    .await;
    post(app,"/own/apply",json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"request":request,"confirmation":{"baseRevision":c["config"]["revision"],"planHash":p["plan"]["planHash"]}}),s).await;
}
async fn finance(app: &Router, s: &Value, command: Value) -> Value {
    post(
        app,
        "/erp/accounting/commands",
        json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"command":command}),
        s,
    )
    .await["receipt"]
        .clone()
}
async fn financial_book(app: &Router, s: &Value, c: &Value) -> Value {
    finance(app,s,json!({"type":"book.create","companyId":c,"expectedCompanyRevision":1,"code":"AVA-IDR","name":"Reviewed Ava financial book","currency":"IDR","sourceAuthority":"native","independentPosting":false,"reason":"Explicit synthetic single-operator book"})).await
}
#[tokio::test]
async fn new_bank_records_stay_withheld_even_when_company_financial_sharing_is_enabled() {
    let (_f, state, app, s, _bob) = setup().await;
    enable_finance(&app, &s).await;
    let c = company(&app, &s, "BANK-PRIVATE").await;
    let book = financial_book(&app, &s, &c).await;
    finance(&app,&s,json!({"type":"period.open","bookId":book["bookId"],"code":"NOV","startDate":"2026-11-01","endDate":"2026-11-30","reason":"Explicit synthetic financial window"})).await;
    let cash=finance(&app,&s,json!({"type":"account.create","bookId":book["bookId"],"code":"CASH","name":"Reviewed cash account","class":"asset","cashRole":"cash","reason":"Explicit synthetic cash classification"})).await;
    let reference = json!({"accountId":cash["resourceId"],"expectedAccountRevision":1});
    let bank=finance(&app,&s,json!({"type":"bank_account.create","bookId":book["bookId"],"code":"PRIVATE-BANK-REGISTRY","name":"Private bank registry marker withheld by default","account":reference,"reason":"Manual owned fixture only"})).await;
    let statement=finance(&app,&s,json!({"type":"bank_statement.import","bankAccountId":bank["resourceId"],"account":reference,"statement":{"reference":"PRIVATE-STATEMENT-MARKER","fromDate":"2026-11-01","toDate":"2026-11-30","currency":"IDR","openingMinor":"0","closingMinor":"7000","rows":[{"externalRef":"PRIVATE-BANK-TRANSACTION-REF","date":"2026-11-09","amountMinor":"10000","description":"Private bank row prose must remain local"},{"externalRef":"PRIVATE-BANK-WITHDRAWAL","date":"2026-11-09","amountMinor":"-3000","description":"Synthetic statement evidence"}]},"reason":"Complete exact owned statement"})).await;
    let (st, local) = common::call(
        &app,
        "GET",
        &format!(
            "/erp/accounting/workspace?tenant=acme-retail&companyId={}&collection=bank_rows",
            c.as_str().unwrap()
        ),
        Value::Null,
        Some(&s),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{local}");
    assert_eq!(local["bank_rows"].as_array().unwrap().len(), 2);
    assert!(local.to_string().contains("PRIVATE-BANK-TRANSACTION-REF"));
    let m = model(state, read_response(), None).await;
    allow(
        &m.app,
        &s,
        json!(["accounting"]),
        json!([
            "conversation_text",
            "record_references",
            "business_facts",
            "financial_amounts"
        ]),
        json!([c.clone()]),
    )
    .await;
    let mut input = turn("Review the currently authorized accounting records.");
    input["companyId"] = c;
    post(&m.app, "/api/agent/turns", input, &s).await;
    let calls = m.calls.lock().await;
    assert_eq!(calls.len(), 1);
    let wire = calls[0].to_string();
    for withheld in [
        "PRIVATE-BANK-REGISTRY",
        "Private bank registry marker",
        "PRIVATE-STATEMENT-MARKER",
        "PRIVATE-BANK-TRANSACTION-REF",
        "Private bank row prose",
        bank["resourceId"].as_str().unwrap(),
        statement["resourceId"].as_str().unwrap(),
    ] {
        assert!(
            !wire.contains(withheld),
            "New bank evidence reached the owned provider: {withheld}"
        );
    }
    let data = &calls[0]["authorizedContext"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["module"] == "accounting")
        .unwrap()["data"];
    assert!(data.get("bank_accounts").is_none());
    assert!(data.get("bank_statements").is_none());
    assert!(data.get("bank_rows").is_none());
}
#[tokio::test]
async fn journal_agent_review_carries_exact_original_lines_and_stale_posting_holds_before_effects()
{
    let (f, _state, app, s, bob) = setup().await;
    enable_finance(&app, &s).await;
    let c = company(&app, &s, "AVA-FINANCE").await;
    let b = financial_book(&app, &s, &c).await;
    let debit=finance(&app,&s,json!({"type":"account.create","bookId":b["bookId"],"code":"INVENTORY","name":"Original inventory account","class":"asset","cashRole":"none","reason":"Synthetic classification"})).await;
    let credit=finance(&app,&s,json!({"type":"account.create","bookId":b["bookId"],"code":"CAPITAL","name":"Original capital account","class":"equity","cashRole":"none","reason":"Synthetic classification"})).await;
    finance(&app,&s,json!({"type":"period.open","bookId":b["bookId"],"code":"2026-11","startDate":"2026-11-01","endDate":"2026-11-30","reason":"Explicit test period"})).await;
    let journal=finance(&app,&s,json!({"type":"journal.draft","bookId":b["bookId"],"code":"EXACT-AVA","date":"2026-11-09","sourceRef":"Explicit synthetic source","memo":"Exact amount for human review","reason":"Review before posting","lines":[{"accountId":debit["resourceId"],"expectedAccountRevision":1,"debitMinor":"705100","creditMinor":"0","memo":"Reviewed debit"},{"accountId":credit["resourceId"],"expectedAccountRevision":1,"debitMinor":"0","creditMinor":"705100","memo":"Reviewed credit"}]})).await;
    let mut input = turn("Prepare the exact journal posting for my review.");
    input["companyId"] = c.clone();
    input["action"] = json!({"tool":"accounting.journal.post","values":{"id":journal["resourceId"],"expectedRevision":1,"reason":"Approve exact reviewed journal"}});
    let r = post(&app, "/api/agent/turns", input, &s).await;
    let guard = r["goal"]["plan"]["steps"][0]["guards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["collection"] == "journals")
        .expect("journal guard");
    assert_eq!(guard["fields"]["lines"][0]["debitMinor"], "705100");
    assert_eq!(
        guard["fields"]["lines"][0]["accountName"],
        "Original inventory account"
    );
    assert_eq!(guard["fields"]["lines"][1]["creditMinor"], "705100");
    finance(&app,&bob,json!({"type":"journal.post","id":journal["resourceId"],"expectedRevision":1,"reason":"Another currently authorized reviewer posts first"})).await;
    let stale=post(&app,"/api/agent/approve",json!({"tenant":"acme-retail","goalId":r["goalId"],"planHash":r["goal"]["plan_hash"],"confirmed":true}),&s).await;
    assert_eq!(stale["goal"]["state"], "needs_review", "{stale}");
    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_accounting.journals WHERE state='posted'")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(rows, 1);
    let balance: String =
        sqlx::query_scalar("SELECT sum(debit_minor)::text FROM archava_accounting.journal_lines")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(balance, "705100");
}
#[tokio::test]
async fn an_owner_financial_sharing_policy_never_mints_financial_read_or_write_grants() {
    let (f, state, app, s, bob) = setup().await;
    enable_finance(&app, &s).await;
    let c = company(&app, &s, "NO-GRANT").await;
    let b = financial_book(&app, &s, &c).await;
    sqlx::query("UPDATE archava_accounting.memberships SET permissions=ARRAY[]::text[] WHERE principal_id='bob'").execute(&f.admin_pool).await.unwrap();
    let m = model(state, read_response(), None).await;
    allow(
        &m.app,
        &s,
        json!(["accounting"]),
        json!([
            "conversation_text",
            "record_references",
            "business_facts",
            "financial_amounts"
        ]),
        json!([c.clone()]),
    )
    .await;
    let mut input = turn("Review accounting records for the selected company.");
    input["companyId"] = c.clone();
    post(&m.app, "/api/agent/turns", input, &bob).await;
    let calls = m.calls.lock().await;
    assert_eq!(calls.len(), 1);
    let source = calls[0]["authorizedContext"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["module"] == "accounting")
        .unwrap();
    assert_eq!(source["available"], false);
    assert!(source.get("data").is_none());
    assert!(!calls[0].to_string().contains(b["bookId"].as_str().unwrap()));
    drop(calls);
    let mut input = turn("Prepare an accounting account.");
    input["companyId"] = c;
    input["action"] = json!({"tool":"accounting.account.create","values":{"bookId":b["bookId"],"code":"FORGED","name":"No added grant","class":"asset","cashRole":"none","reason":"Must refuse"}});
    let (st, r) = common::call(&m.app, "POST", "/api/agent/turns", input, Some(&bob)).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{r}");
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_accounting.accounts WHERE code='FORGED'")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    let permissions: Vec<String> = sqlx::query_scalar(
        "SELECT permissions FROM archava_accounting.memberships WHERE principal_id='bob'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert!(permissions.is_empty());
}

#[tokio::test]
async fn reviewed_accounting_agent_execution_posts_once_and_keeps_independent_posting_authority() {
    let (f, _state, app, alice, bob) = setup().await;
    enable_finance(&app, &alice).await;
    for (n, independent) in [false, true].into_iter().enumerate() {
        let company = company(&app, &alice, &format!("AVA-EXEC-{n}")).await;
        let book = finance(&app,&alice,json!({"type":"book.create","companyId":company,"expectedCompanyRevision":1,"code":format!("AVA-EXEC-{n}"),"name":"Explicit agent execution book","currency":"IDR","sourceAuthority":"native","independentPosting":independent,"reason":"Reviewed explicit posting policy"})).await;
        let debit = finance(&app,&alice,json!({"type":"account.create","bookId":book["bookId"],"code":"INV","name":"Original inventory","class":"asset","cashRole":"none","reason":"Explicit original debit"})).await;
        let credit = finance(&app,&alice,json!({"type":"account.create","bookId":book["bookId"],"code":"CAP","name":"Original capital","class":"equity","cashRole":"none","reason":"Explicit original credit"})).await;
        finance(&app,&alice,json!({"type":"period.open","bookId":book["bookId"],"code":"2026-11","startDate":"2026-11-01","endDate":"2026-11-30","reason":"Reviewed period"})).await;
        let journal = finance(&app,&alice,json!({"type":"journal.draft","bookId":book["bookId"],"code":format!("AVA-EXEC-{n}"),"date":"2026-11-09","sourceRef":"Owned original source","memo":"Exact original money","reason":"Draft before explicit agent review","lines":[{"accountId":debit["resourceId"],"expectedAccountRevision":1,"debitMinor":"705100","creditMinor":"0","memo":"Original debit"},{"accountId":credit["resourceId"],"expectedAccountRevision":1,"debitMinor":"0","creditMinor":"705100","memo":"Original credit"}]})).await;
        let mut input = turn("Prepare exact financial posting for my review.");
        input["companyId"] = company.clone();
        input["action"] = json!({"tool":"accounting.journal.post","values":{"id":journal["resourceId"],"expectedRevision":1,"reason":"Approve original exact financial journal"}});
        let proposed = post(&app, "/api/agent/turns", input.clone(), &alice).await;
        let confirmation = json!({"tenant":"acme-retail","goalId":proposed["goalId"],"planHash":proposed["goal"]["plan_hash"],"confirmed":true});
        let approved = post(&app, "/api/agent/approve", confirmation.clone(), &alice).await;
        let completed = if independent {
            assert_eq!(approved["goal"]["state"], "holding", "{approved}");
            let state: String =
                sqlx::query_scalar("SELECT state FROM archava_accounting.journals WHERE id=$1")
                    .bind(Uuid::parse_str(journal["resourceId"].as_str().unwrap()).unwrap())
                    .fetch_one(&f.admin_pool)
                    .await
                    .unwrap();
            assert_eq!(
                state, "draft",
                "Ava must not bypass the draft author's independent posting policy"
            );
            let (status, _) = common::call(
                &app,
                "POST",
                "/api/agent/approve",
                confirmation.clone(),
                Some(&bob),
            )
            .await;
            assert_eq!(
                status,
                StatusCode::NOT_FOUND,
                "Another reviewer cannot adopt Alice's personal goal"
            );
            input["requestId"] = json!(Uuid::new_v4());
            let independent_review = post(&app, "/api/agent/turns", input.clone(), &bob).await;
            post(&app,"/api/agent/approve",json!({"tenant":"acme-retail","goalId":independent_review["goalId"],"planHash":independent_review["goal"]["plan_hash"],"confirmed":true}),&bob).await
        } else {
            let replay = post(&app, "/api/agent/turns", input.clone(), &alice).await;
            assert_eq!(replay["goalId"], proposed["goalId"]);
            post(&app, "/api/agent/approve", confirmation, &alice).await;
            approved
        };
        assert_eq!(completed["goal"]["state"], "completed", "{completed}");
        assert_eq!(
            completed["goal"]["outcome"]["steps"][0]["payload_output"]["verified"],
            true
        );
        let proof: Value = sqlx::query_scalar("SELECT jsonb_build_object('state',j.state,'revision',j.revision,'postedBy',j.posted_by,'debit',(SELECT sum(debit_minor)::text FROM archava_accounting.journal_lines WHERE journal_id=j.id),'audit',(SELECT count(*) FROM archava_accounting.audit WHERE resource_kind='journal' AND resource_id=j.id)) FROM archava_accounting.journals j WHERE id=$1")
            .bind(Uuid::parse_str(journal["resourceId"].as_str().unwrap()).unwrap())
            .fetch_one(&f.admin_pool).await.unwrap();
        assert_eq!(
            proof,
            json!({"state":"posted","revision":2,"postedBy":if independent {"bob"} else {"alice"},"debit":"705100","audit":2})
        );
    }
    let attempts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_owner.agent_model_attempts")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(attempts, 0, "Typed financial plans never call a provider");
}

#[tokio::test]
async fn provider_failure_is_safe_recoverable_budgeted_and_keeps_manual_work_available() {
    let (f, state, _app, alice, _) = setup().await;
    let m = model_status(
        state,
        json!({"error":{"message":"Response schema rejected: SYNTHETIC-PRIVATE-KEY-AND-PROMPT"}}),
        None,
        StatusCode::BAD_REQUEST,
    )
    .await;
    share_all_model_data(&m.app).await;
    let input = turn("Review products for a bounded read.");
    let (status, body) = common::call(
        &m.app,
        "POST",
        "/api/agent/turns",
        input.clone(),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["reason"], "agent_planner_unavailable");
    assert_eq!(body["failure"], "schema_rejected");
    assert_eq!(body["upstreamStatus"], 400);
    assert!(!body.to_string().contains("SYNTHETIC-PRIVATE"));
    let (status, recovery) = common::call(
        &m.app,
        "GET",
        &format!(
            "/api/agent/state?tenant=acme-retail&requestId={}",
            input["requestId"].as_str().unwrap()
        ),
        Value::Null,
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recovery["status"], "not_committed");
    let (status, recovery) =
        common::call(&m.app, "POST", "/api/agent/turns", input, Some(&alice)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(recovery["reason"], "agent_planning_attempt_needs_recovery");
    assert_eq!(
        m.calls.lock().await.len(),
        1,
        "Recovery must not repeat a paid request"
    );
    for _ in 1..6 {
        let (status, _) = common::call(
            &m.app,
            "POST",
            "/api/agent/turns",
            turn("Review products again."),
            Some(&alice),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }
    let (status, _) = common::call(
        &m.app,
        "POST",
        "/api/agent/turns",
        turn("Review products after the budget."),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(m.calls.lock().await.len(), 6);
    let goals: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_owner.agent_goals")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(
        goals, 0,
        "Failed planning creates no goal or business approval"
    );
    let mut manual = turn("Prepare a product using the manual details.");
    manual["action"] = json!({"tool":"commerce.product.create","values":{"sku":"PROVIDER-DOWN","title":"Reviewed manual fallback","unit":"unit","trackStock":false,"price":1000}});
    let plan = post(&m.app, "/api/agent/turns", manual, &alice).await;
    assert_eq!(plan["planner"]["providerId"], "archava-agent-local");
    let complete=post(&m.app,"/api/agent/approve",json!({"tenant":"acme-retail","goalId":plan["goalId"],"planHash":plan["goal"]["plan_hash"],"confirmed":true}),&alice).await;
    assert_eq!(complete["goal"]["state"], "completed");
    let proof: Value=sqlx::query_scalar("SELECT jsonb_build_object('products',(SELECT count(*) FROM archava_commerce.products WHERE sku='PROVIDER-DOWN'),'attempts',(SELECT count(*) FROM archava_owner.agent_model_attempts))").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(proof, json!({"products":1,"attempts":6}));
    assert_eq!(
        m.calls.lock().await.len(),
        6,
        "Manual work remains available without provider calls"
    );
}

#[tokio::test]
async fn one_binding_format_correction_stays_within_budget_and_never_resends_failed_proposal_text()
{
    let (f, state, app, alice, _) = setup().await;
    let product=post(&app,"/erp/commerce/commands",json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"command":{"operation":"product.create","sku":"REPAIR-SOURCE","title":"Reviewed repair source","unit":"unit","trackStock":true,"price":10000}}),&alice).await;
    let mut bad = json!({"summary":"SYNTHETIC-PRIVATE-FAILED-PROPOSAL","questions":[],"steps":[{"title":"Register supplier","tool":"purchasing.supplier.create","input":{"code":"REPAIR-SUPPLIER","name":"Reviewed supplier"},"dependsOn":[]},{"title":"Register offer","tool":"purchasing.offer.create","input":{"supplierId":{"$from":{"step":1,"path":["receipt","receipt","id"]}},"productId":product["receipt"]["id"],"unitCost":3500},"dependsOn":[0]}]});
    let mut corrected = bad.clone();
    corrected["summary"] = json!("Final valid reviewed proposal");
    corrected["steps"][1]["input"]["supplierId"]["$from"]["step"] = json!(0);
    let m = model(
        state,
        json!({"__ownedSequence":[bad.clone(),corrected]}),
        None,
    )
    .await;
    share_all_model_data(&m.app).await;
    let plan = post(
        &m.app,
        "/api/agent/turns",
        turn("Prepare the reviewed supplier and product offer."),
        &alice,
    )
    .await;
    assert_eq!(plan["goal"]["state"], "planned");
    assert_eq!(plan["planner"]["planningRounds"], 2);
    assert_eq!(plan["planner"]["formatCorrections"], 1);
    let calls = m.calls.lock().await;
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[1]["formatCorrection"]["reason"],
        "invalid_agent_output_binding"
    );
    assert_eq!(calls[1]["formatCorrection"]["bindings"][0]["sourceStep"], 1);
    assert!(!calls[1]
        .to_string()
        .contains("SYNTHETIC-PRIVATE-FAILED-PROPOSAL"));
    drop(calls);
    let before:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM archava_purchasing.suppliers)+(SELECT count(*) FROM archava_purchasing.offers)").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(
        before, 0,
        "Format correction does not approve or execute either candidate"
    );
    let completed=post(&m.app,"/api/agent/approve",json!({"tenant":"acme-retail","goalId":plan["goalId"],"planHash":plan["goal"]["plan_hash"],"confirmed":true}),&alice).await;
    assert_eq!(completed["goal"]["state"], "completed");
    let offer: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.offers WHERE unit_cost=3500")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(offer, 1);
    assert_eq!(m.calls.lock().await.len(), 2);
    // A repeated malformed result has no third round and creates no new goal.
    bad["steps"][0]["input"]["code"] = json!("REPAIR-EXHAUSTED");
    bad["steps"][1]["input"]["supplierId"] = json!({"$from":"SYNTHETIC-PRIVATE-BROKEN-BINDING"});
    let state = cold_state(&f).await;
    let refused = model(state, bad, None).await;
    share_all_model_data(&refused.app).await;
    let (status, body) = common::call(
        &refused.app,
        "POST",
        "/api/agent/turns",
        turn("Prepare another reviewed supplier offer."),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["reason"], "invalid_agent_output_binding");
    assert_eq!(body["formatIssues"][0]["bindingIsObject"], false);
    assert!(!body.to_string().contains("SYNTHETIC-PRIVATE"));
    let calls = refused.calls.lock().await;
    assert!(!calls[1].to_string().contains("SYNTHETIC-PRIVATE"));
    drop(calls);
    assert_eq!(
        refused.calls.lock().await.len(),
        2,
        "Invalid output cannot bypass the two-round limit"
    );
    let goals: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_owner.agent_goals")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(goals, 1, "Only the first final valid proposal was recorded");
}

#[tokio::test]
async fn reassignment_cannot_resend_old_derived_case_text_through_a_parent_goal() {
    let (f, state, app, alice, bob) = setup().await;
    const PRIVATE: &str = "SYNTHETIC-DERIVED-ASSIGNED-CASE-TEXT";
    let created = post(
        &app,
        "/erp/support/commands",
        json!({
            "tenant":"acme-retail", "mutation":{"requestId":Uuid::new_v4(),"command":{
                "operation":"case.create","requesterName":"Synthetic caller",
                "subject":"Assigned continuation scope","description":PRIVATE,
                "priority":"normal","dueAt":null
            }}
        }),
        &bob,
    )
    .await;
    let case_id = created["id"].clone();
    assert!(
        case_id.is_string(),
        "Verify receipt shape before testing scope"
    );
    post(
        &app,
        "/erp/support/commands",
        json!({
            "tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4(),"command":{
                "operation":"case.assign","id":case_id,"expectedRevision":1,
                "assigneeId":"alice","reason":"Assign the synthetic operator"
            }}
        }),
        &bob,
    )
    .await;
    sqlx::query(
        "UPDATE archava_support.memberships SET scope='assigned' WHERE principal_id='alice'",
    )
    .execute(&f.admin_pool)
    .await
    .unwrap();
    let initial = json!({"summary":"Review the assigned ticket","questions":[],"steps":[{
        "title":"Start assigned case","tool":"support.case.start",
        "input":{"id":case_id,"expectedRevision":2,"reason":PRIVATE},"dependsOn":[]
    }]});
    let next = json!({"summary":"Current ticket is unavailable","questions":["Which currently accessible ticket should I use?"],"steps":[]});
    let m = model(state, json!({"__ownedSequence":[initial,next]}), None).await;
    share_all_model_data(&m.app).await;
    let first = post(
        &m.app,
        "/api/agent/turns",
        turn("Support: prepare starting the assigned case."),
        &alice,
    )
    .await;
    assert_eq!(first["goal"]["state"], "planned");
    assert!(m.calls.lock().await[0]["authorizedContext"]
        .to_string()
        .contains(PRIVATE));
    // Domain assignment changes while Alice's memberships/config stay unchanged.
    post(
        &app,
        "/erp/support/commands",
        json!({
            "tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4(),"command":{
                "operation":"case.assign","id":case_id,"expectedRevision":2,
                "assigneeId":"bob","reason":"Hand off the synthetic record"
            }}
        }),
        &bob,
    )
    .await;
    let (status, _) = common::call(
        &app,
        "GET",
        &format!(
            "/erp/support/cases/detail?tenant=acme-retail&id={}",
            case_id.as_str().unwrap()
        ),
        Value::Null,
        Some(&alice),
    )
    .await;
    assert!(matches!(
        status,
        StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
    ));
    let mut follow = turn("Support: review the remaining work from that plan.");
    follow["threadId"] = first["threadId"].clone();
    follow["goalId"] = first["goalId"].clone();
    let (status, body) =
        common::call(&m.app, "POST", "/api/agent/turns", follow, Some(&alice)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let calls = m.calls.lock().await;
    assert_eq!(calls.len(), 2);
    assert!(!calls[1]["authorizedContext"].to_string().contains(PRIVATE));
    assert!(!calls[1]["previousGoal"].to_string().contains(PRIVATE),
        "Current domain access must also constrain derived historical plan fields; actual owned wire previousGoal={}",calls[1]["previousGoal"]);
    let started: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_support.cases WHERE state='in_progress'")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(started, 0, "Neither plan was approved");
}

#[tokio::test]
async fn contract_format_correction_preserves_exact_values_and_never_resends_rejected_fields() {
    let (f, state, app, alice, _) = setup().await;
    let created=post(&app,"/erp/commerce/commands",json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"command":{"operation":"product.create","sku":"CONTRACT-REPAIR","title":"Reviewed contract target","unit":"unit","trackStock":false,"price":1000}}),&alice).await;
    let mut valid = json!({"summary":"Review the exact requested price","questions":[],"steps":[{"title":"Update reviewed price","tool":"commerce.product.set","input":{"id":created["receipt"]["id"],"expectedRevision":1,"price":1400,"active":true},"dependsOn":[]}]});
    let mut invalid = valid.clone();
    invalid["summary"] = json!("PRIVATE-REJECTED-PROPOSAL");
    invalid["steps"][0]["input"]["currency"] = json!("PRIVATE-REJECTED-FIELD");
    let m = model(
        state,
        json!({"__ownedSequence":[invalid,valid.clone()]}),
        None,
    )
    .await;
    share_all_model_data(&m.app).await;
    let plan = post(
        &m.app,
        "/api/agent/turns",
        turn("Commerce: prepare the reviewed price change to 1400."),
        &alice,
    )
    .await;
    assert_eq!(plan["goal"]["state"], "planned");
    assert_eq!(plan["planner"]["planningRounds"], 2);
    assert_eq!(plan["planner"]["formatCorrections"], 1);
    let calls = m.calls.lock().await;
    assert_eq!(calls.len(), 2);
    assert!(!calls[1].to_string().contains("PRIVATE-REJECTED"));
    let contracts = &calls[1]["formatCorrection"]["selectedActionContracts"];
    assert_eq!(contracts.as_array().unwrap().len(), 1);
    assert_eq!(contracts[0]["tool"], "commerce.product.set");
    assert!(contracts[0]["inputSchema"]["properties"]
        .get("currency")
        .is_none());
    drop(calls);
    let price: i32 = sqlx::query_scalar(
        "SELECT price FROM archava_commerce.products WHERE sku='CONTRACT-REPAIR'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(
        price, 1000,
        "A format correction never authorizes a mutation"
    );
    let done=post(&m.app,"/api/agent/approve",json!({"tenant":"acme-retail","goalId":plan["goalId"],"planHash":plan["goal"]["plan_hash"],"confirmed":true}),&alice).await;
    assert_eq!(done["goal"]["state"], "completed");
    let row: (i32, i32) = sqlx::query_as(
        "SELECT price,revision FROM archava_commerce.products WHERE sku='CONTRACT-REPAIR'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(row, (1400, 2));
    // A repeated primitive mismatch has no third round and cannot coerce money
    // through the native form parser or create another approved goal.
    valid["steps"][0]["input"]["expectedRevision"] = json!(2);
    valid["steps"][0]["input"]["price"] = json!("1400");
    let refused = model(cold_state(&f).await, valid, None).await;
    share_all_model_data(&refused.app).await;
    let (status, body) = common::call(
        &refused.app,
        "POST",
        "/api/agent/turns",
        turn("Commerce: prepare another reviewed price change."),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["reason"], "agent_action_fields_invalid");
    assert_eq!(refused.calls.lock().await.len(), 2);
    let goals: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_owner.agent_goals")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(goals, 1);
}

#[tokio::test]
async fn staged_contact_reads_share_three_total_calls_with_routing_and_never_repeat_an_identical_read(
) {
    let (f, state, app, alice, _) = setup().await;
    let body = archava_domain::operator::envelope(
        "crm",
        "acme-retail",
        &Uuid::new_v4().to_string(),
        "contact.create",
        &json!({"data":{"fullName":"Controlled CRM target","organization":"Initial affiliation"}}),
    )
    .unwrap();
    let created = post(&app, "/crm/commands", body, &alice).await;
    let target = created["receipt"]["id"].clone();
    assert!(Uuid::parse_str(target.as_str().unwrap()).unwrap().as_u128() > 200);
    sqlx::query("INSERT INTO archava_crm.contacts(tenant_id,id,full_name,email,organization,owner_id) SELECT 'acme-retail',lpad(to_hex(n),32,'0')::uuid,'Unrelated CRM sample '||n,NULL,'Fixture','alice' FROM generate_series(1,110) AS n").execute(&f.admin_pool).await.unwrap();
    let search = json!({"summary":"Find the named contact outside the first page","questions":[],"steps":[],"lookups":[{"module":"crm","collection":"contacts","query":"Controlled CRM target"}]});
    let detail = json!({"summary":"Inspect the now observed exact target","questions":[],"steps":[],"lookups":[{"module":"crm","collection":"contacts","id":target}]});
    let final_plan = json!({"summary":"Review the current target and requested affiliation","questions":[],"steps":[{"title":"Update the reviewed contact","tool":"crm.contact.update","input":{"id":target,"expectedRevision":1,"data":{"organization":"Reviewed affiliation"}},"dependsOn":[]}]});
    let m = model(
        state,
        json!({"__ownedSequence":[search.clone(),detail.clone(),final_plan]}),
        None,
    )
    .await;
    share_all_model_data(&m.app).await;
    let input = turn("CRM: prepare the affiliation update for Controlled CRM target.");
    let plan = post(&m.app, "/api/agent/turns", input.clone(), &alice).await;
    assert_eq!(plan["goal"]["state"], "planned");
    assert_eq!(plan["planner"]["planningRounds"], 3);
    assert_eq!(plan["planner"]["routingCalls"], 0);
    assert_eq!(plan["planner"]["providerCalls"], 3);
    assert_eq!(plan["planner"]["maximumProviderCalls"], 3);
    assert_eq!(plan["planner"]["readQueries"], 2);
    let calls = m.calls.lock().await;
    assert_eq!(calls.len(), 3);
    assert!(!calls[0]["authorizedContext"]
        .to_string()
        .contains("Controlled CRM target"));
    assert!(calls[1]["authorizedContext"]
        .to_string()
        .contains("Controlled CRM target"));
    assert_eq!(calls[2]["readExpansion"]["canRequestAnotherRead"], false);
    assert_eq!(calls[2]["readExpansion"]["currentPlanningRound"], 3);
    // The schema enforces what `readExpansion` says: a read can be requested
    // while a later call remains, and not on the last one.
    let schemas = m.schemas.lock().await;
    assert_eq!(schemas.len(), 3);
    for (round, schema) in schemas.iter().enumerate() {
        assert_eq!(
            schema["properties"]["lookups"].is_object(),
            round < 2,
            "round {}",
            round + 1
        );
        assert!(schema["properties"]["steps"].is_object());
    }
    drop(schemas);
    assert_eq!(
        calls[2]["authorizedContext"]["lookupResults"][0]["includedRowsInCollection"],
        1
    );
    drop(calls);
    let row: (String, i32) =
        sqlx::query_as("SELECT organization,revision FROM archava_crm.contacts WHERE id=$1")
            .bind(Uuid::parse_str(target.as_str().unwrap()).unwrap())
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(row, ("Initial affiliation".into(), 1));
    let done=post(&m.app,"/api/agent/approve",json!({"tenant":"acme-retail","goalId":plan["goalId"],"planHash":plan["goal"]["plan_hash"],"confirmed":true}),&alice).await;
    assert_eq!(done["goal"]["state"], "completed");
    post(&m.app, "/api/agent/turns", input, &alice).await;
    assert_eq!(m.calls.lock().await.len(), 3);
    let row: (String, i32) =
        sqlx::query_as("SELECT organization,revision FROM archava_crm.contacts WHERE id=$1")
            .bind(Uuid::parse_str(target.as_str().unwrap()).unwrap())
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(row, ("Reviewed affiliation".into(), 2));

    // An actual model-routing call spends one of the same three slots: it does
    // not get an extra planning call, even for a valid next exact read.
    let routed = model(
        cold_state(&f).await,
        json!({"__ownedSequence":[{"modules":["crm"]},search.clone(),detail]}),
        None,
    )
    .await;
    share_all_model_data(&routed.app).await;
    let (status, error) = common::call(
        &routed.app,
        "POST",
        "/api/agent/turns",
        turn("Please assist."),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(error["reason"], "agent_lookup_budget_exceeded");
    assert_eq!(
        routed.calls.lock().await.len(),
        3,
        "Routing plus reads must not cause a fourth model request"
    );
    assert_eq!(routed.calls.lock().await[0]["phase"], "route");
    // Repeating identical successful read input produces no new information and
    // is refused after two calls, despite one unused total slot.
    let repeated = model(cold_state(&f).await, search, None).await;
    share_all_model_data(&repeated.app).await;
    let (status, error) = common::call(
        &repeated.app,
        "POST",
        "/api/agent/turns",
        turn("CRM: inspect Controlled CRM target."),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(error["reason"], "agent_lookup_budget_exceeded");
    assert_eq!(error["planningBudget"]["newRequests"], 0);
    assert_eq!(repeated.calls.lock().await.len(), 2);
    assert!(!error.to_string().contains("Controlled CRM target"));
}

#[tokio::test]
async fn a_later_planning_round_never_resends_a_case_after_assignment_access_is_lost() {
    let (f, state, app, alice, bob) = setup().await;
    const PRIVATE: &str = "SYNTHETIC-CASE-REASSIGNED-IN-FLIGHT";
    let created=post(&app,"/erp/support/commands",json!({"tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4(),"command":{"operation":"case.create","requesterName":"Synthetic caller","subject":"Scoped in-flight case","description":PRIVATE,"priority":"normal","dueAt":null}}}),&bob).await;
    let case_id = created["id"].clone();
    assert!(case_id.is_string());
    post(&app,"/erp/support/commands",json!({"tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4(),"command":{"operation":"case.assign","id":case_id,"expectedRevision":1,"assigneeId":"alice","reason":"Assign synthetic operator"}}}),&bob).await;
    sqlx::query(
        "UPDATE archava_support.memberships SET scope='assigned' WHERE principal_id='alice'",
    )
    .execute(&f.admin_pool)
    .await
    .unwrap();
    post(&app,"/erp/commerce/commands",json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"command":{"operation":"product.create","sku":"SCOPE-ROUND","title":"Scoped refresh goods","unit":"unit","trackStock":false,"price":1000}}),&alice).await;
    let answer = json!({"__ownedCaseReassignment":case_id,"__ownedSequence":[{"summary":"Read the requested commerce target","questions":[],"steps":[],"lookups":[{"module":"commerce","collection":"products","query":"Scoped refresh goods"}]},{"summary":"Review only currently readable facts","questions":[],"steps":[]}]});
    let m = model(state, answer, Some((app.clone(), bob))).await;
    share_all_model_data(&m.app).await;
    let reply = post(
        &m.app,
        "/api/agent/turns",
        turn("Support and Commerce: inspect the current work."),
        &alice,
    )
    .await;
    assert_eq!(reply["planner"]["planningRounds"], 2);
    let (status, _) = common::call(
        &app,
        "GET",
        &format!(
            "/erp/support/cases/detail?tenant=acme-retail&id={}",
            case_id.as_str().unwrap()
        ),
        Value::Null,
        Some(&alice),
    )
    .await;
    assert!(matches!(
        status,
        StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
    ));
    let calls = m.calls.lock().await;
    assert_eq!(calls.len(), 2);
    assert!(calls[0]["authorizedContext"].to_string().contains(PRIVATE));
    assert!(!calls[1]["authorizedContext"].to_string().contains(PRIVATE),"An unchanged membership fingerprint is not current record access; actual second owned context={}",calls[1]["authorizedContext"]);
}
