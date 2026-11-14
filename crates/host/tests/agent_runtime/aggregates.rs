//! Aggregate lookups: whole-set totals by SQL, their scope, allowlist,
//! disclosure and planner budget, checked against direct SQL and against the
//! request bodies an owned provider fixture actually received.
use super::*;
use archava_adapters::erp::aggregates::{self as store, CompanyScope, Refusal};
use archava_core::tenant::TenantId;
use archava_domain::erp::purchasing::PurchasePrincipal;
use axum::{extract::State, routing::post as route_post, Json};
use std::sync::Arc;
use tokio::sync::Mutex;

const FROM: &str = "2026-11-01";
const TO: &str = "2026-11-30";
/// Every seeded customer name starts with this; no other record does.
const NAME: &str = "Zebra Customer";

#[derive(Clone)]
struct Recorder {
    calls: Arc<Mutex<Vec<Value>>>,
    schemas: Arc<Mutex<Vec<Value>>>,
    answers: Arc<Vec<Value>>,
}
/// An owned provider that records every request and answers from a script.
struct Script {
    app: Router,
    task: tokio::task::JoinHandle<()>,
    calls: Arc<Mutex<Vec<Value>>>,
    schemas: Arc<Mutex<Vec<Value>>>,
}
impl Drop for Script {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn scripted(state: AppState, answers: Vec<Value>) -> Script {
    async fn wire(State(state): State<Recorder>, Json(body): Json<Value>) -> Json<Value> {
        let input: Value =
            serde_json::from_str(body["contents"][0]["parts"][0]["text"].as_str().unwrap())
                .unwrap();
        state
            .schemas
            .lock()
            .await
            .push(body["generationConfig"]["responseJsonSchema"].clone());
        let index = {
            let mut calls = state.calls.lock().await;
            calls.push(input);
            calls.len() - 1
        };
        let answer = &state.answers[index.min(state.answers.len() - 1)];
        Json(
            json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":answer.to_string()}]}}]}),
        )
    }
    let calls = Arc::new(Mutex::new(Vec::new()));
    let schemas = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let fixture = Router::new()
        .route("/v1beta/models/{model}", route_post(wire))
        .with_state(Recorder {
            calls: calls.clone(),
            schemas: schemas.clone(),
            answers: Arc::new(answers),
        });
    let task = tokio::spawn(async move { axum::serve(listener, fixture).await.unwrap() });
    let planner = archava_providers::agent::GeminiPlanner::new(
        &endpoint,
        "owned-aggregate-model",
        "owned-aggregate-key",
    )
    .unwrap();
    let app = archava_host::router(state.with_agent_config(archava_host::agent::AgentConfig {
        planner: Some(planner),
    }));
    Script {
        app,
        task,
        calls,
        schemas,
    }
}
fn answer(summary: &str) -> Value {
    json!({"summary":summary,"questions":[],"steps":[]})
}
fn reads(lookups: Value) -> Value {
    json!({"summary":"Read the totals first.","questions":[],"steps":[],"lookups":lookups})
}
fn aggregate(module: &str, collection: &str, aggregate: Value) -> Value {
    json!({"module":module,"collection":collection,"aggregate":aggregate})
}
/// The owner's AI sharing selection, through the public review lifecycle.
async fn share(app: &Router, modules: Value, classes: Value, companies: Value, workspace: bool) {
    let alice = common::login(app, "alice").await;
    let (st, config) = common::call(
        app,
        "GET",
        "/own/config?tenant=acme-retail",
        Value::Null,
        Some(&alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{config}");
    let request = json!({"baseRevision":config["config"]["revision"],"aiDisclosure":{"kind":"replace","policy":{"providerBinding":config["aiProvider"]["binding"],"modules":modules,"dataClasses":classes,"companies":companies,"includeWorkspaceRecords":workspace}}});
    let preview = post(
        app,
        "/own/plan",
        json!({"tenant":"acme-retail","request":request.clone()}),
        &alice,
    )
    .await;
    assert!(preview["plan"]["blocked"].is_null(), "{preview}");
    post(app,"/own/apply",json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"request":request,"confirmation":{"baseRevision":config["config"]["revision"],"planHash":preview["plan"]["planHash"]},"disclosureApproval":preview["disclosureApproval"]}),&alice).await;
}
fn all_classes() -> Value {
    json!([
        "conversation_text",
        "record_references",
        "business_facts",
        "names",
        "contact_details",
        "notes",
        "financial_amounts"
    ])
}
async fn company(app: &Router, s: &Value, code: &str) -> Value {
    let o=post(app,"/erp/organizations/commands",json!({"tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4(),"type":"create_organization","data":{"name":format!("Company {code}"),"legalName":format!("Company {code} Official")}}}),s).await;
    post(app,"/erp/organizations/commands",json!({"tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4(),"type":"company.create_from_organization","data":{"code":code,"organizationId":o["receipt"]["organizationId"],"expectedSourceRevision":1,"reason":"Synthetic aggregate scope"}}}),s).await["receipt"]["companyId"].clone()
}
/// A company whose book, and therefore whose trade currency, is USD.
async fn usd_company(app: &Router, s: &Value, code: &str) -> Value {
    let (st, config) = common::call(
        app,
        "GET",
        "/own/config?tenant=acme-retail",
        Value::Null,
        Some(s),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{config}");
    if !config["config"]["modules"]
        .to_string()
        .contains("\"accounting\"")
    {
        let request = json!({"baseRevision":config["config"]["revision"],"enable":["accounting"]});
        let preview = post(
            app,
            "/own/plan",
            json!({"tenant":"acme-retail","request":request.clone()}),
            s,
        )
        .await;
        post(app,"/own/apply",json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"request":request,"confirmation":{"baseRevision":config["config"]["revision"],"planHash":preview["plan"]["planHash"]}}),s).await;
    }
    let id = company(app, s, code).await;
    post(app,"/erp/accounting/commands",json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"command":{"type":"book.create","companyId":id,"expectedCompanyRevision":1,"code":format!("BOOK-{code}"),"name":"Synthetic USD book","currency":"USD","sourceAuthority":"native","independentPosting":false,"reason":"Opened before any trade record"}}),s).await;
    id
}
struct Seed {
    idr: Value,
    usd: Value,
}
impl Seed {
    fn idr(&self) -> &str {
        self.idr.as_str().unwrap()
    }
    fn usd(&self) -> &str {
        self.usd.as_str().unwrap()
    }
}
/// 219 invoices of this tenant across two companies, two currencies, four
/// statuses, three months and two owners, plus another tenant's.
///
/// - IDR company: 150 issued in November for 60 customers (more than a page
///   and more than the group limit), 6 on the UTC month boundaries, 8 drafts,
///   5 settled, 4 void.
/// - USD company: 30 issued in November for 7 customers, 4 in October.
/// - No company (IDR): 12 issued in November, half of them owned by bob.
/// - Tenant `other-co`: 5 issued in November with amounts that would show.
async fn seed(f: &Fixture, app: &Router, alice: &Value) -> Seed {
    let idr = company(app, alice, "AGG-IDR").await;
    let usd = usd_company(app, alice, "AGG-USD").await;
    let insert = "INSERT INTO archava_billing.invoices(tenant_id,id,invoice_code,customer_ref,currency,status,gross_minor,settled_minor,outstanding_minor,consumer_ref,owner_id,issued_at,settled_at,voided_at,company_id,company_revision,company_code,company_name) \
        SELECT $1,gen_random_uuid(),$2||'-'||n,$3||' '||lpad((n % $4)::text,2,'0'),$5,$6,$7+n,CASE WHEN $6='settled' THEN $7+n ELSE $8 END,CASE WHEN $6='settled' THEN 0 ELSE $7+n-$8 END,'seed',CASE WHEN $9 AND n % 2=0 THEN 'bob' ELSE $10 END,\
          CASE WHEN $6='draft' THEN NULL ELSE $11::text::timestamptz+(n||' seconds')::interval END,CASE WHEN $6='settled' THEN $11::text::timestamptz+(n||' seconds')::interval END,CASE WHEN $6='void' THEN $11::text::timestamptz+(n||' seconds')::interval END,\
          c.id,c.revision,c.code,c.legal_name \
        FROM generate_series(1,$12::int) n LEFT JOIN archava_organizations.companies c ON c.tenant_id=$1 AND c.id=$13::text::uuid";
    #[allow(clippy::too_many_arguments)]
    async fn rows(
        f: &Fixture,
        insert: &str,
        tenant: &str,
        code: &str,
        customers: i32,
        currency: &str,
        status: &str,
        base: i64,
        settled: i64,
        split_owner: bool,
        owner: &str,
        at: &str,
        count: i32,
        company: Option<&str>,
    ) {
        sqlx::query(sqlx::AssertSqlSafe(insert))
            .bind(tenant)
            .bind(code)
            .bind(NAME)
            .bind(customers)
            .bind(currency)
            .bind(status)
            .bind(base)
            .bind(settled)
            .bind(split_owner)
            .bind(owner)
            .bind(at)
            .bind(count)
            .bind(company)
            .execute(&f.admin_pool)
            .await
            .unwrap_or_else(|e| panic!("seed {code}: {e}"));
    }
    let (a, b) = (idr.as_str().unwrap(), usd.as_str().unwrap());
    let t = "acme-retail";
    rows(
        f,
        insert,
        t,
        "A-NOV",
        60,
        "IDR",
        "issued",
        7_310_000,
        1_000,
        false,
        "alice",
        "2026-11-10T08:00:00Z",
        150,
        Some(a),
    )
    .await;
    // The UTC calendar bounds: the last second of October and the first of
    // December are outside; the first and last seconds of November inside.
    rows(
        f,
        insert,
        t,
        "A-OCT-END",
        60,
        "IDR",
        "issued",
        5_000_000,
        0,
        false,
        "alice",
        "2026-10-31T23:59:57Z",
        2,
        Some(a),
    )
    .await;
    rows(
        f,
        insert,
        t,
        "A-NOV-START",
        60,
        "IDR",
        "issued",
        5_100_000,
        0,
        false,
        "alice",
        "2026-10-31T23:59:59Z",
        1,
        Some(a),
    )
    .await;
    rows(
        f,
        insert,
        t,
        "A-NOV-END",
        60,
        "IDR",
        "issued",
        5_200_000,
        0,
        false,
        "alice",
        "2026-11-30T23:59:58Z",
        1,
        Some(a),
    )
    .await;
    rows(
        f,
        insert,
        t,
        "A-DEC-START",
        60,
        "IDR",
        "issued",
        5_300_000,
        0,
        false,
        "alice",
        "2026-11-30T23:59:59Z",
        2,
        Some(a),
    )
    .await;
    rows(
        f,
        insert,
        t,
        "A-DRAFT",
        60,
        "IDR",
        "draft",
        6_000_000,
        0,
        false,
        "alice",
        "2026-11-12T08:00:00Z",
        8,
        Some(a),
    )
    .await;
    rows(
        f,
        insert,
        t,
        "A-SETTLED",
        60,
        "IDR",
        "settled",
        6_100_000,
        0,
        false,
        "alice",
        "2026-11-12T08:00:00Z",
        5,
        Some(a),
    )
    .await;
    rows(
        f,
        insert,
        t,
        "A-VOID",
        60,
        "IDR",
        "void",
        6_200_000,
        0,
        false,
        "alice",
        "2026-11-12T08:00:00Z",
        4,
        Some(a),
    )
    .await;
    rows(
        f,
        insert,
        t,
        "B-NOV",
        7,
        "USD",
        "issued",
        41_900,
        100,
        false,
        "alice",
        "2026-11-11T08:00:00Z",
        30,
        Some(b),
    )
    .await;
    rows(
        f,
        insert,
        t,
        "B-OCT",
        7,
        "USD",
        "issued",
        51_900,
        100,
        false,
        "alice",
        "2026-10-11T08:00:00Z",
        4,
        Some(b),
    )
    .await;
    rows(
        f,
        insert,
        t,
        "L-NOV",
        3,
        "IDR",
        "issued",
        2_500_000,
        500,
        true,
        "alice",
        "2026-11-13T08:00:00Z",
        12,
        None,
    )
    .await;
    sqlx::query("INSERT INTO archava_billing.memberships(tenant_id,principal_id,kind,permissions,scope,active) VALUES('other-co','mallory','human',ARRAY['billing.read'],'all',true)").execute(&f.admin_pool).await.unwrap();
    rows(
        f,
        insert,
        "other-co",
        "X-NOV",
        2,
        "IDR",
        "issued",
        990_000_000_000,
        0,
        false,
        "mallory",
        "2026-11-10T08:00:00Z",
        5,
        None,
    )
    .await;
    Seed { idr, usd }
}
/// `(currency, rows, sum, distinct customers)` straight from the table.
async fn direct(f: &Fixture, column: &str, predicate: &str) -> Vec<(String, i64, i64, i64)> {
    let statement = format!("SELECT currency,count(*),sum({column})::bigint,count(DISTINCT customer_ref) FROM archava_billing.invoices WHERE tenant_id='acme-retail' AND {predicate} GROUP BY currency ORDER BY currency");
    sqlx::query_as(sqlx::AssertSqlSafe(statement.as_str()))
        .fetch_all(&f.admin_pool)
        .await
        .unwrap()
}
const NOVEMBER: &str = "status='issued' AND (issued_at AT TIME ZONE 'UTC')::date BETWEEN '2026-11-01' AND '2026-11-30'";
fn amounts(section: &Value) -> i64 {
    section["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["amount"].as_i64().unwrap())
        .sum::<i64>()
        + section["other"]["amount"].as_i64().unwrap_or(0)
}
fn group_rows(section: &Value) -> i64 {
    section["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["rows"].as_i64().unwrap())
        .sum::<i64>()
        + section["other"]["rows"].as_i64().unwrap_or(0)
}

#[tokio::test]
async fn a_totals_question_is_answered_from_the_whole_filtered_set_per_currency_with_stated_coverage(
) {
    let (f, state, app, alice, _) = setup().await;
    let seed = seed(&f, &app, &alice).await;
    let november = json!({"status":"issued","dateFrom":FROM,"dateTo":TO});
    let mut one_company = november.clone();
    one_company["companyId"] = seed.idr.clone();
    let m = scripted(
        state,
        vec![
            reads(json!([
                aggregate("billing","invoices",json!({"measure":"sum","field":"outstanding","groupBy":"customer","filters":november})),
                aggregate("billing","invoices",json!({"measure":"sum","field":"outstanding","groupBy":"customer","filters":one_company})),
                aggregate("billing","invoices",json!({"measure":"count","groupBy":"status"})),
                aggregate("billing","invoices",json!({"measure":"sum","field":"gross","groupBy":"month","filters":{"companyId":seed.usd}})),
                aggregate("billing","invoices",json!({"measure":"sum","field":"tax","filters":november})),
                {"module":"billing","collection":"invoices","query":"A-NOV-1"}
            ])),
            answer("Customers owe the amounts listed per currency for November."),
        ],
    )
    .await;
    share_all_model_data(&m.app).await;
    let plan = post(
        &m.app,
        "/api/agent/turns",
        turn("How much do customers still owe us on invoices this month, by customer?"),
        &alice,
    )
    .await;
    assert_eq!(plan["goal"]["state"], "completed", "{plan}");
    assert_eq!(plan["planner"]["planningRounds"], 2);
    assert_eq!(plan["planner"]["readQueries"], 6);
    let calls = m.calls.lock().await;
    assert_eq!(calls.len(), 2);
    // The first round is told which aggregates exist; none has been run.
    assert!(calls[0]["authorizedContext"]
        .get("aggregateResults")
        .is_none());
    assert_eq!(
        calls[0]["readExpansion"]["aggregates"],
        json!([{"module":"billing","collection":"invoices","measures":["count","sum"],"sumFields":["gross","outstanding","tax"],"groupBy":["status","customer","month"],"filters":["status","dateFrom","dateTo","companyId"],"dateField":"issued","filterValues":{"status":["draft","issued","settled","void"]}}])
    );
    let results = calls[1]["authorizedContext"]["aggregateResults"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(results.len(), 5);

    // 1. Outstanding by customer, issued in November: every row, per currency.
    let expected = direct(&f, "outstanding_minor", NOVEMBER).await;
    assert_eq!(expected.len(), 2, "{expected:?}");
    let by_customer = &results[0];
    let covered: i64 = expected.iter().map(|e| e.1).sum();
    assert_eq!(
        covered,
        150 + 2 + 30 + 12,
        "150 + both November bounds + USD + unassigned"
    );
    assert_eq!(by_customer["rowsCovered"], covered);
    assert_eq!(by_customer["complete"], true);
    assert_eq!(by_customer["incompleteReason"], Value::Null);
    assert_eq!(
        by_customer["filter"],
        json!({"status":"issued","dateField":"issued","dateFrom":FROM,"dateTo":TO,"calendar":"UTC calendar dates, both bounds inclusive","companyId":null})
    );
    assert_eq!(
        by_customer["scope"],
        json!({"companies":{"approvedForAiSharing":2},"recordsWithoutCompany":true,"ownRecordsOnly":false})
    );
    let sections = by_customer["sections"].as_array().unwrap();
    assert_eq!(
        sections.len(),
        2,
        "one section per currency, never a mixed total"
    );
    assert!(by_customer.get("total").is_none());
    for (section, (currency, rows, total, customers)) in sections.iter().zip(&expected) {
        assert_eq!(section["currency"], *currency);
        assert_eq!(section["rows"], *rows);
        assert_eq!(
            section["total"], *total,
            "{currency} equals the direct SQL sum"
        );
        assert_eq!(section["groupCount"], *customers);
        assert_eq!(
            amounts(section),
            *total,
            "{currency} groups and fold add up"
        );
        assert_eq!(group_rows(section), *rows);
    }
    assert_eq!(sections[0]["unit"], "whole IDR");
    assert_eq!(sections[1]["unit"], "minor units of USD");
    assert_eq!(
        sections[0]["totalFormatted"],
        archava_domain::erp::trade_money::format_trade("IDR", expected[0].2)
    );
    assert!(sections[1]["totalFormatted"]
        .as_str()
        .unwrap()
        .starts_with("USD "));
    // Two currencies share the 50 groups: 25 each, the rest folded.
    assert_eq!(sections[0]["groups"].as_array().unwrap().len(), 25);
    assert_eq!(sections[0]["other"]["groups"], expected[0].3 - 25);
    assert_eq!(sections[1]["groups"].as_array().unwrap().len(), 7);
    assert_eq!(sections[1]["other"], Value::Null);
    let ordered = sections[0]["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["amount"].as_i64().unwrap())
        .collect::<Vec<_>>();
    assert!(
        ordered.windows(2).all(|pair| pair[0] >= pair[1]),
        "{ordered:?}"
    );
    // Another tenant's rows are in no figure.
    assert!(expected[0].2 < 990_000_000_000);

    // 2. One company, one currency: more than 50 customers fold into "other".
    let folded = &results[1];
    let expected = direct(
        &f,
        "outstanding_minor",
        &format!("{NOVEMBER} AND company_id='{}'", seed.idr()),
    )
    .await;
    assert_eq!(expected.len(), 1);
    let (_, rows, total, customers) = expected[0].clone();
    assert_eq!((rows, customers), (152, 60));
    assert_eq!(folded["rowsCovered"], rows);
    assert_eq!(folded["filter"]["companyId"], seed.idr);
    assert_eq!(folded["scope"]["companies"], "selected company");
    assert_eq!(folded["scope"]["recordsWithoutCompany"], false);
    let section = &folded["sections"][0];
    assert_eq!(folded["sections"].as_array().unwrap().len(), 1);
    assert_eq!(section["groups"].as_array().unwrap().len(), 50);
    assert_eq!(section["groupCount"], 60);
    assert_eq!(section["other"]["groups"], 10);
    assert_eq!(section["total"], total);
    assert_eq!(amounts(section), total, "the fold keeps the total exact");
    assert_eq!(group_rows(section), rows);
    assert!(section["other"]["amount"].as_i64().unwrap() > 0);
    // A page read of the same collection covers fewer rows than the aggregate.
    let (st, page) = common::call(
        &app,
        "GET",
        &format!(
            "/erp/billing/workspace?tenant=acme-retail&companyId={}",
            seed.idr()
        ),
        Value::Null,
        Some(&alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{page}");
    let page_rows = page["invoices"].as_array().unwrap().len() as i64;
    assert_eq!(page_rows, 100);
    assert!(page_rows < rows);

    // 3. A count has no currency: every status of every shared row.
    let by_status = &results[2];
    let statuses: Vec<(String, i64)> = sqlx::query_as("SELECT status,count(*) FROM archava_billing.invoices WHERE tenant_id='acme-retail' GROUP BY status ORDER BY count(*) DESC,status")
        .fetch_all(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(by_status["rowsCovered"], 219);
    assert_eq!(by_status["sections"].as_array().unwrap().len(), 1);
    assert!(by_status["sections"][0].get("currency").is_none());
    assert!(by_status["sections"][0].get("total").is_none());
    assert_eq!(
        by_status["sections"][0]["groups"],
        json!(statuses
            .iter()
            .map(|(status, rows)| json!({"label":status,"rows":rows}))
            .collect::<Vec<_>>())
    );

    // 4. Gross by calendar month of issue, USD company: minor units.
    let by_month = &results[3];
    let months: Vec<(String, i64, i64)> = sqlx::query_as("SELECT to_char(issued_at AT TIME ZONE 'UTC','YYYY-MM'),count(*),sum(gross_minor)::bigint FROM archava_billing.invoices WHERE tenant_id='acme-retail' AND company_id=$1::uuid GROUP BY 1 ORDER BY 3 DESC")
        .bind(seed.usd())
        .fetch_all(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(months.len(), 2);
    assert_eq!(by_month["sections"][0]["currency"], "USD");
    assert_eq!(by_month["sections"][0]["unit"], "minor units of USD");
    assert_eq!(
        by_month["sections"][0]["groups"],
        json!(months
            .iter()
            .map(|(month, rows, total)| json!({"label":month,"rows":rows,"amount":total,"formatted":archava_domain::erp::trade_money::format_trade("USD", *total)}))
            .collect::<Vec<_>>())
    );

    // 5. An ungrouped sum is one total per currency.
    let tax = &results[4];
    assert_eq!(tax["rowsCovered"], covered);
    assert_eq!(
        tax["sections"],
        json!([{"rows":164,"currency":"IDR","unit":"whole IDR","total":0,"totalFormatted":"Rp 0"},{"rows":30,"currency":"USD","unit":"minor units of USD","total":0,"totalFormatted":"USD 0.00"}])
    );

    // Both calls could still be followed by a read, so both schemas offer the
    // aggregate member; the last call of a turn offers no lookup at all (see
    // `aggregates_share_the_lookup_budget_and_the_last_round_offers_no_read`).
    let schemas = m.schemas.lock().await;
    for schema in schemas.iter() {
        assert!(schema["properties"]["lookups"]["items"]["properties"]["aggregate"].is_object());
    }
    assert_eq!(calls[1]["readExpansion"]["canRequestAnotherRead"], true);
    assert_eq!(calls[1]["readExpansion"]["remainingReadRequests"], 2);
    drop(schemas);
    // Page evidence stays distinguishable from aggregate evidence.
    assert_eq!(
        calls[1]["authorizedContext"]["lookupResults"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    drop(calls);

    // The answer states its coverage in the host's words, from the read.
    let coverage = by_customer["coverage"].as_str().unwrap();
    assert!(
        coverage.starts_with(&format!("Billing invoices: sum of outstanding by customer over all {covered} matching rows (status issued, issued {FROM}..{TO} (UTC calendar dates, inclusive)); scope: records of the 2 companies approved for AI sharing and records without a company; read at 20")),
        "{coverage}"
    );
    let text = plan["text"].as_str().unwrap();
    assert!(text.starts_with("Customers owe the amounts listed per currency for November."));
    assert!(
        text.contains("Evidence (computed by Archava, not by the model):"),
        "{text}"
    );
    assert!(text.contains(coverage), "{text}");
    assert_eq!(
        plan["goal"]["context"]["aggregateResults"][0]["rowsCovered"],
        covered
    );
}

async fn run_as(
    f: &Fixture,
    tenant: &str,
    actor: &str,
    request: &store::Request<'_>,
) -> Result<store::Outcome, store::AggregateError> {
    let (namespace, ledger) = match request.module {
        "billing" => ("archava_billing", "billing"),
        "commerce" => ("archava_commerce", "commerce"),
        _ => ("archava_purchasing", "purchasing"),
    };
    let mut tx = archava_adapters::rls::TenantTransaction::begin(
        &f.runtime_pool,
        namespace,
        ledger,
        TenantId::new(tenant).unwrap(),
    )
    .await
    .unwrap();
    let outcome = store::run(
        tx.checked_executor().await.unwrap(),
        tenant,
        &PurchasePrincipal::human(actor),
        request,
    )
    .await;
    tx.commit().await.unwrap();
    outcome
}
fn everything() -> CompanyScope {
    CompanyScope {
        companies: None,
        unassigned: true,
    }
}
fn count<'a>(module: &'a str, collection: &'a str, scope: CompanyScope) -> store::Request<'a> {
    store::Request {
        module,
        collection,
        sum: None,
        group_by: None,
        filters: &[],
        scope,
    }
}
async fn rows_as(f: &Fixture, tenant: &str, actor: &str, scope: CompanyScope) -> i64 {
    run_as(f, tenant, actor, &count("billing", "invoices", scope))
        .await
        .unwrap()
        .rows
}
fn refused(outcome: Result<store::Outcome, store::AggregateError>) -> Refusal {
    match outcome {
        Err(store::AggregateError::Refused(refusal)) => refusal,
        other => panic!("expected a refusal, got {other:?}"),
    }
}
async fn sql(f: &Fixture, statement: &str) {
    sqlx::query(sqlx::AssertSqlSafe(statement))
        .execute(&f.admin_pool)
        .await
        .unwrap_or_else(|e| panic!("{statement}: {e}"));
}
async fn page_rows(app: &Router, s: &Value, company: Option<&str>) -> (StatusCode, usize) {
    let path = match company {
        Some(id) => format!("/erp/billing/workspace?tenant=acme-retail&companyId={id}"),
        None => "/erp/billing/workspace?tenant=acme-retail".to_string(),
    };
    let (status, body) = common::call(app, "GET", &path, Value::Null, Some(s)).await;
    (status, body["invoices"].as_array().map_or(0, Vec::len))
}

/// The aggregate is checked at the adapter, where it is enforced: the same
/// permission, row scope, company access and tenant as the page read.
#[tokio::test]
async fn an_aggregate_never_counts_a_row_the_page_read_would_not_show() {
    let (f, state, app, alice, bob) = setup().await;
    let seed = seed(&f, &app, &alice).await;
    let id = |text: &str| Uuid::parse_str(text).unwrap();
    // Full access: every row of this tenant and none of the other tenant's.
    assert_eq!(rows_as(&f, "acme-retail", "alice", everything()).await, 219);
    assert_eq!(rows_as(&f, "acme-retail", "bob", everything()).await, 219);
    assert_eq!(rows_as(&f, "other-co", "mallory", everything()).await, 5);
    let filters = [("status", "issued")];
    let outstanding = run_as(
        &f,
        "acme-retail",
        "alice",
        &store::Request {
            module: "billing",
            collection: "invoices",
            sum: Some("outstanding"),
            group_by: None,
            filters: &filters,
            scope: everything(),
        },
    )
    .await
    .unwrap();
    let expected = direct(&f, "outstanding_minor", "status='issued'").await;
    assert_eq!(
        outstanding
            .sections
            .iter()
            .map(|s| (s.currency.clone().unwrap(), s.rows, s.total.unwrap() as i64))
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|(currency, rows, total, _)| (currency.clone(), *rows, *total))
            .collect::<Vec<_>>()
    );
    assert!(outstanding.filter.every_owner);
    // A principal of another tenant, or of none, has no membership here.
    assert_eq!(
        refused(
            run_as(
                &f,
                "acme-retail",
                "mallory",
                &count("billing", "invoices", everything())
            )
            .await
        ),
        Refusal::NotPermitted
    );
    assert_eq!(
        refused(
            run_as(
                &f,
                "other-co",
                "alice",
                &count("billing", "invoices", everything())
            )
            .await
        ),
        Refusal::NotPermitted
    );

    // The caller's selection only narrows.
    let only = |companies: Vec<Uuid>, unassigned| CompanyScope {
        companies: Some(companies),
        unassigned,
    };
    assert_eq!(
        rows_as(
            &f,
            "acme-retail",
            "alice",
            only(vec![id(seed.usd())], false)
        )
        .await,
        34
    );
    assert_eq!(
        rows_as(&f, "acme-retail", "alice", only(vec![], true)).await,
        12
    );
    assert_eq!(
        rows_as(&f, "acme-retail", "alice", only(vec![], false)).await,
        0
    );
    assert_eq!(
        rows_as(
            &f,
            "acme-retail",
            "alice",
            only(vec![Uuid::new_v4()], false)
        )
        .await,
        0
    );
    let company = [("companyId", seed.idr())];
    let one = |scope| store::Request {
        module: "billing",
        collection: "invoices",
        sum: None,
        group_by: None,
        filters: &company,
        scope,
    };
    assert_eq!(
        run_as(&f, "acme-retail", "alice", &one(everything()))
            .await
            .unwrap()
            .rows,
        173
    );
    assert_eq!(
        refused(
            run_as(
                &f,
                "acme-retail",
                "alice",
                &one(only(vec![id(seed.usd())], true))
            )
            .await
        ),
        Refusal::CompanyNotReadable,
        "a company filter outside the caller's selection is refused, not widened"
    );

    // Company access, as `company_trade::resolve` gives it to the page read:
    // an Organizations scope of `assigned` shows only companies bob owns.
    sql(
        &f,
        "UPDATE archava_organizations.memberships SET scope='assigned' WHERE principal_id='bob'",
    )
    .await;
    assert_eq!(page_rows(&app, &bob, None).await, (StatusCode::OK, 12));
    assert_eq!(
        page_rows(&app, &bob, Some(seed.idr())).await,
        (StatusCode::NOT_FOUND, 0)
    );
    assert_eq!(rows_as(&f, "acme-retail", "bob", everything()).await, 12);
    assert_eq!(
        rows_as(
            &f,
            "acme-retail",
            "bob",
            only(vec![id(seed.idr()), id(seed.usd())], true)
        )
        .await,
        12,
        "naming a company does not make it readable"
    );
    assert_eq!(
        refused(run_as(&f, "acme-retail", "bob", &one(everything())).await),
        Refusal::CompanyNotReadable
    );
    // Without `companies.read` no company row counts either.
    sql(&f, "UPDATE archava_organizations.memberships SET scope='all',permissions=array_remove(permissions,'companies.read') WHERE principal_id='bob'").await;
    assert_eq!(
        page_rows(&app, &bob, Some(seed.usd())).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(rows_as(&f, "acme-retail", "bob", everything()).await, 12);
    assert_eq!(
        refused(run_as(&f, "acme-retail", "bob", &one(everything())).await),
        Refusal::CompanyNotReadable
    );
    sql(&f, "UPDATE archava_organizations.memberships SET permissions=array_append(permissions,'companies.read') WHERE principal_id='bob'").await;
    assert_eq!(rows_as(&f, "acme-retail", "bob", everything()).await, 219);

    // Row scope: an `assigned` billing membership sees only its own invoices.
    sql(&f, "UPDATE archava_billing.memberships SET scope='assigned' WHERE tenant_id='acme-retail' AND principal_id='bob'").await;
    let own: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_billing.invoices WHERE tenant_id='acme-retail' AND owner_id='bob'")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(own, 6);
    assert_eq!(page_rows(&app, &bob, None).await, (StatusCode::OK, 6));
    let scoped = run_as(
        &f,
        "acme-retail",
        "bob",
        &count("billing", "invoices", everything()),
    )
    .await
    .unwrap();
    assert_eq!(scoped.rows, 6);
    assert!(
        !scoped.filter.every_owner,
        "the evidence says whose rows these are"
    );

    // Permission: without `billing.read`, or without an active membership,
    // nothing is computed.
    sql(&f, "UPDATE archava_billing.memberships SET scope='all',permissions=ARRAY['billing.write'] WHERE tenant_id='acme-retail' AND principal_id='bob'").await;
    assert_eq!(page_rows(&app, &bob, None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        refused(
            run_as(
                &f,
                "acme-retail",
                "bob",
                &count("billing", "invoices", everything())
            )
            .await
        ),
        Refusal::NotPermitted
    );
    // Through the planner the same user is refused with a reason and the
    // provider request carries no figure and no seeded name.
    let m = scripted(
        state,
        vec![
            reads(json!([aggregate(
                "billing",
                "invoices",
                json!({"measure":"sum","field":"outstanding","groupBy":"customer"})
            )])),
            answer("I cannot read invoices under the current access."),
        ],
    )
    .await;
    share_all_model_data(&m.app).await;
    let plan = post(
        &m.app,
        "/api/agent/turns",
        turn("How much do customers owe on invoices?"),
        &bob,
    )
    .await;
    assert_eq!(plan["goal"]["state"], "completed", "{plan}");
    let calls = m.calls.lock().await;
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["readExpansion"]["aggregates"], json!([]));
    assert_eq!(
        calls[1]["authorizedContext"]["aggregateResults"],
        json!([{"kind":"aggregate","refused":true,"reason":"aggregate_module_unavailable","message":"This business area is not among the sources available to this turn under the current access and configuration.","module":"billing","collection":"invoices"}])
    );
    for call in calls.iter() {
        let text = call.to_string();
        assert!(!text.contains(NAME), "{text}");
        assert!(!text.contains("sections"), "{text}");
    }
    drop(calls);
    assert!(!plan["text"].as_str().unwrap().contains("Evidence"));
    sql(&f, "UPDATE archava_billing.memberships SET active=false WHERE tenant_id='acme-retail' AND principal_id='bob'").await;
    assert_eq!(
        refused(
            run_as(
                &f,
                "acme-retail",
                "bob",
                &count("billing", "invoices", everything())
            )
            .await
        ),
        Refusal::NotPermitted
    );
}

async fn command(app: &Router, s: &Value, domain: &str, input: Value) -> Value {
    post(
        app,
        &format!("/erp/{domain}/commands"),
        json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"command":input}),
        s,
    )
    .await["receipt"]
        .clone()
}

#[tokio::test]
async fn sales_and_purchase_orders_aggregate_under_their_own_read_permission_and_scope() {
    let (f, _state, app, alice, bob) = setup().await;
    let product = stock(&app, &alice, "AGG-GOODS").await;
    let product = product["receipt"]["id"].clone();
    for (customer, quantity) in [
        ("Zebra Customer 01", 2),
        ("Zebra Customer 01", 3),
        ("Zebra Customer 02", 5),
    ] {
        command(&app,&alice,"commerce",json!({"operation":"order.create","customerName":customer,"lines":[{"productId":product,"quantity":quantity}]})).await;
    }
    let mine = command(&app,&bob,"commerce",json!({"operation":"order.create","customerName":"Zebra Customer 03","lines":[{"productId":product,"quantity":7}]})).await;
    command(&app,&bob,"commerce",json!({"operation":"order.cancel","id":mine["id"],"expectedRevision":1,"reason":"Synthetic cancelled order"})).await;
    let (st, purchasing) = common::call(
        &app,
        "GET",
        "/erp/purchasing/workspace?tenant=acme-retail",
        Value::Null,
        Some(&alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{purchasing}");
    let offer = &purchasing["offers"][0];
    for quantity in [4, 6] {
        command(&app,&alice,"purchasing",json!({"operation":"order.create","supplierId":offer["supplierId"],"lines":[{"offerId":offer["id"],"quantity":quantity}]})).await;
    }
    // Sales orders: total by customer, exact against the table.
    let by_customer = store::Request {
        module: "commerce",
        collection: "orders",
        sum: Some("total"),
        group_by: Some("customer"),
        filters: &[],
        scope: everything(),
    };
    let outcome = run_as(&f, "acme-retail", "alice", &by_customer)
        .await
        .unwrap();
    let expected: Vec<(String, i64, i64)> = sqlx::query_as("SELECT customer_name,count(*),sum(total_estimate)::bigint FROM archava_commerce.orders WHERE tenant_id='acme-retail' AND managed_by IS NULL GROUP BY customer_name ORDER BY 3 DESC")
        .fetch_all(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(expected.len(), 3);
    assert_eq!(outcome.rows, 4);
    assert_eq!(outcome.filter.date, "created");
    assert_eq!(outcome.sections.len(), 1);
    assert_eq!(outcome.sections[0].currency.as_deref(), Some("IDR"));
    assert_eq!(
        outcome.sections[0]
            .groups
            .iter()
            .map(|g| (g.label.clone().unwrap(), g.rows, g.total.unwrap() as i64))
            .collect::<Vec<_>>(),
        expected
    );
    let state = [("state", "cancelled")];
    let cancelled = run_as(
        &f,
        "acme-retail",
        "alice",
        &store::Request {
            module: "commerce",
            collection: "orders",
            sum: None,
            group_by: Some("state"),
            filters: &state,
            scope: everything(),
        },
    )
    .await
    .unwrap();
    assert_eq!(cancelled.rows, 1);
    assert_eq!(
        cancelled.sections[0].groups[0].label.as_deref(),
        Some("cancelled")
    );
    assert_eq!(cancelled.sections[0].currency, None);
    // Purchase orders: total by supplier.
    let by_supplier = store::Request {
        module: "purchasing",
        collection: "orders",
        sum: Some("total"),
        group_by: Some("supplier"),
        filters: &[],
        scope: everything(),
    };
    let outcome = run_as(&f, "acme-retail", "alice", &by_supplier)
        .await
        .unwrap();
    let (rows, total): (i64, i64) = sqlx::query_as("SELECT count(*),sum(total_estimate)::bigint FROM archava_purchasing.orders WHERE tenant_id='acme-retail'")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!((rows, total), (2, 50_000));
    assert_eq!(outcome.rows, rows);
    assert_eq!(outcome.sections[0].total, Some(i128::from(total)));
    assert_eq!(outcome.sections[0].groups.len(), 1);
    assert_eq!(
        outcome.sections[0].groups[0].label.as_deref(),
        Some("Agent synthetic supplier")
    );
    // An `assigned` commerce membership sees its own orders, in both ledgers:
    // the commerce scope narrows purchasing too, as in the page read.
    sql(
        &f,
        "UPDATE archava_commerce.memberships SET scope='assigned' WHERE principal_id='bob'",
    )
    .await;
    assert_eq!(
        run_as(
            &f,
            "acme-retail",
            "bob",
            &count("commerce", "orders", everything())
        )
        .await
        .unwrap()
        .rows,
        1
    );
    assert_eq!(
        run_as(
            &f,
            "acme-retail",
            "bob",
            &count("purchasing", "orders", everything())
        )
        .await
        .unwrap()
        .rows,
        0
    );
    let (st, page) = common::call(
        &app,
        "GET",
        "/erp/commerce/workspace?tenant=acme-retail",
        Value::Null,
        Some(&bob),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{page}");
    assert_eq!(page["orders"].as_array().unwrap().len(), 1);
    // Without `orders.read` neither collection is aggregated.
    sql(&f, "UPDATE archava_commerce.memberships SET permissions=array_remove(permissions,'orders.read') WHERE principal_id='bob'").await;
    sql(&f, "UPDATE archava_purchasing.memberships SET permissions=array_remove(permissions,'orders.read') WHERE principal_id='bob'").await;
    assert_eq!(
        refused(
            run_as(
                &f,
                "acme-retail",
                "bob",
                &count("commerce", "orders", everything())
            )
            .await
        ),
        Refusal::NotPermitted
    );
    assert_eq!(
        refused(
            run_as(
                &f,
                "acme-retail",
                "bob",
                &count("purchasing", "orders", everything())
            )
            .await
        ),
        Refusal::NotPermitted
    );
    assert_eq!(
        run_as(
            &f,
            "acme-retail",
            "alice",
            &count("commerce", "orders", everything())
        )
        .await
        .unwrap()
        .rows,
        4
    );
}

fn refusal(reason: &str, message: &str, collection: Option<&str>) -> Value {
    let mut out = json!({"kind":"aggregate","refused":true,"reason":reason,"message":message,"module":"billing"});
    if let Some(collection) = collection {
        out["collection"] = json!(collection);
    }
    out
}
/// No seeded name, no total and no result section may be in this value: a
/// whole provider request, or the aggregate results on their own.
fn assert_no_aggregate_data(value: &Value, totals: &[i64]) {
    let text = value.to_string();
    assert!(!text.contains(NAME), "a seeded name leaked: {text}");
    for total in totals {
        assert!(!text.contains(&total.to_string()), "{total} leaked: {text}");
    }
    // The instructions name these members; the data must not carry them.
    let data = if value.get("authorizedContext").is_some() {
        value["authorizedContext"].to_string()
    } else {
        text
    };
    assert!(!data.contains("\"sections\""), "{data}");
    assert!(!data.contains("rowsCovered"), "{data}");
}

#[tokio::test]
async fn anything_outside_the_allowlist_is_refused_with_its_reason_and_discloses_nothing() {
    let (f, state, app, alice, _) = setup().await;
    let _seed = seed(&f, &app, &alice).await;
    let totals = direct(&f, "outstanding_minor", "true")
        .await
        .into_iter()
        .map(|row| row.2)
        .collect::<Vec<_>>();
    let m = scripted(
        state,
        vec![
            reads(json!([
                aggregate("billing", "payment_events", json!({"measure":"count"})),
                aggregate(
                    "billing",
                    "invoices",
                    json!({"measure":"sum","field":"settled"})
                ),
                aggregate(
                    "billing",
                    "invoices",
                    json!({"measure":"average","field":"gross"})
                ),
                aggregate(
                    "billing",
                    "invoices",
                    json!({"measure":"count","groupBy":"owner"})
                ),
                aggregate(
                    "billing",
                    "invoices",
                    json!({"measure":"count","filters":{"ownerId":"alice"}})
                ),
                aggregate(
                    "billing",
                    "invoices",
                    json!({"measure":"count","filters":{"status":"paid"}})
                ),
                aggregate(
                    "billing",
                    "invoices",
                    json!({"measure":"count","filters":{"companyId":Uuid::new_v4()}})
                ),
                aggregate(
                    "commerce",
                    "orders",
                    json!({"measure":"sum","field":"total"})
                )
            ])),
            answer("None of those totals is available."),
        ],
    )
    .await;
    share_all_model_data(&m.app).await;
    let plan = post(
        &m.app,
        "/api/agent/turns",
        turn("Give me every invoice total you can."),
        &alice,
    )
    .await;
    assert_eq!(plan["goal"]["state"], "completed", "{plan}");
    let calls = m.calls.lock().await;
    assert_eq!(calls.len(), 2);
    let results = &calls[1]["authorizedContext"]["aggregateResults"];
    assert_eq!(
        results,
        &json!([
            refusal("aggregate_collection_not_listed","No aggregate is offered for this module and collection. Use one listed in readExpansion.aggregates.",None),
            refusal("aggregate_measure_not_listed","The measure must be count, or sum with one of the collection's listed sumFields.",Some("invoices")),
            refusal("aggregate_measure_not_listed","The measure must be count, or sum with one of the collection's listed sumFields.",Some("invoices")),
            refusal("aggregate_dimension_not_listed","This collection cannot be grouped by that dimension. Use one of its listed groupBy values or none.",Some("invoices")),
            refusal("aggregate_filter_not_listed","This collection does not offer that filter. Use only its listed filters, each at most once.",Some("invoices")),
            refusal("aggregate_filter_invalid","A filter value is not valid: dates are YYYY-MM-DD with dateFrom not after dateTo, the state must be one of the listed values and companyId an observed company id.",Some("invoices")),
            refusal("aggregate_company_not_shared","That company is not among the companies approved for AI data sharing.",Some("invoices")),
            {"kind":"aggregate","refused":true,"reason":"aggregate_module_unavailable","message":"This business area is not among the sources available to this turn under the current access and configuration.","module":"commerce","collection":"orders"}
        ])
    );
    assert_no_aggregate_data(results, &totals);
    drop(calls);
    assert!(!plan["text"].as_str().unwrap().contains("Evidence"));
    assert_no_aggregate_data(&plan["goal"]["context"]["aggregateResults"], &totals);
    // A member outside the closed shape is not a refusal the model can read
    // around: the turn itself is malformed and no second request is sent.
    let malformed = scripted(
        cold_state(&f).await,
        vec![reads(json!([aggregate(
            "billing",
            "invoices",
            json!({"measure":"count","sql":"SELECT sum(gross_minor) FROM archava_billing.invoices"})
        )]))],
    )
    .await;
    share_all_model_data(&malformed.app).await;
    let (status, error) = common::call(
        &malformed.app,
        "POST",
        "/api/agent/turns",
        turn("Give me the invoice total another way."),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{error}");
    assert_eq!(error["reason"], "invalid_agent_lookup");
    assert_eq!(malformed.calls.lock().await.len(), 1);
    // An aggregate is one kind of lookup, not an addition to a search.
    let mixed = scripted(
        cold_state(&f).await,
        vec![reads(json!([
            {"module":"billing","collection":"invoices","query":"A-NOV","aggregate":{"measure":"count"}}
        ]))],
    )
    .await;
    share_all_model_data(&mixed.app).await;
    let (status, error) = common::call(
        &mixed.app,
        "POST",
        "/api/agent/turns",
        turn("Search and total the invoices at once."),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{error}");
    assert_eq!(error["reason"], "invalid_agent_lookup");
}

/// `(modules, data classes, companies, records without a company)`.
type Sharing = (Value, Value, Value, bool);
/// One turn under a sharing selection the owner approves for this provider
/// profile (`None`: everything); returns the recorded provider requests.
async fn ask(f: &Fixture, session: &Value, sharing: Option<Sharing>, lookups: Value) -> Vec<Value> {
    let m = scripted(
        cold_state(f).await,
        vec![reads(lookups), answer("Answered from what is shared.")],
    )
    .await;
    match sharing {
        Some((modules, classes, companies, workspace)) => {
            share(&m.app, modules, classes, companies, workspace).await
        }
        None => share_all_model_data(&m.app).await,
    }
    let plan = post(
        &m.app,
        "/api/agent/turns",
        turn("What do the invoice totals look like?"),
        session,
    )
    .await;
    assert_eq!(plan["goal"]["state"], "completed", "{plan}");
    let calls = m.calls.lock().await.clone();
    assert_eq!(calls.len(), 2);
    calls
}
fn by(group: &str, measure: Value) -> Value {
    let mut request = measure;
    request["groupBy"] = json!(group);
    aggregate("billing", "invoices", request)
}
fn sum_outstanding() -> Value {
    json!({"measure":"sum","field":"outstanding"})
}

#[tokio::test]
async fn an_unshared_module_or_data_category_keeps_names_and_amounts_from_the_provider() {
    let (f, _state, app, alice, _) = setup().await;
    let seed = seed(&f, &app, &alice).await;
    let both = json!([seed.idr, seed.usd]);
    let mut totals = Vec::new();
    for column in ["outstanding_minor", "gross_minor"] {
        for predicate in ["true", "status='issued'", NOVEMBER] {
            totals.extend(
                direct(&f, column, predicate)
                    .await
                    .into_iter()
                    .map(|row| row.2),
            );
        }
    }

    // 1. Billing is not shared: not offered, refused, and nothing of it sent.
    let calls = ask(
        &f,
        &alice,
        Some((json!(["crm"]), all_classes(), both.clone(), true)),
        json!([
            by("customer", sum_outstanding()),
            aggregate("billing", "invoices", json!({"measure":"count"}))
        ]),
    )
    .await;
    assert_eq!(calls[0]["readExpansion"]["aggregates"], json!([]));
    let message =
        "The workspace owner's AI data sharing selection does not include this business area.";
    assert_eq!(
        calls[1]["authorizedContext"]["aggregateResults"],
        json!([
            refusal("aggregate_module_not_shared", message, Some("invoices")),
            refusal("aggregate_module_not_shared", message, Some("invoices"))
        ])
    );
    for call in &calls {
        assert_no_aggregate_data(call, &totals);
        assert!(
            !call.to_string().contains("A-NOV-"),
            "no invoice code either"
        );
    }

    // 2. Amounts are shared, names are not: the customer dimension is refused
    //    while status and month work, and no customer name is sent at all.
    let calls = ask(
        &f,
        &alice,
        Some((
            json!(["billing"]),
            json!(["conversation_text", "business_facts", "financial_amounts"]),
            both.clone(),
            true,
        )),
        json!([
            by("customer", sum_outstanding()),
            by("status", sum_outstanding()),
            by("month", json!({"measure":"count"}))
        ]),
    )
    .await;
    assert_eq!(
        calls[0]["readExpansion"]["aggregates"][0]["groupBy"],
        json!(["status", "month"])
    );
    assert_eq!(
        calls[0]["readExpansion"]["aggregates"][0]["sumFields"],
        json!(["gross", "outstanding", "tax"])
    );
    let results = calls[1]["authorizedContext"]["aggregateResults"]
        .as_array()
        .unwrap();
    let mut names_refused = refusal(
        "aggregate_data_class_not_shared",
        "The workspace owner's AI data sharing selection does not include a data category this aggregate would disclose. Choose a measure or dimension that needs only shared categories.",
        Some("invoices"),
    );
    names_refused["dataClass"] = json!("names");
    assert_eq!(results[0], names_refused);
    assert_eq!(results[1]["rowsCovered"], 219);
    assert_eq!(
        results[1]["sections"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (
                s["currency"].as_str().unwrap().to_owned(),
                s["total"].as_i64().unwrap()
            ))
            .collect::<Vec<_>>(),
        direct(&f, "outstanding_minor", "true")
            .await
            .into_iter()
            .map(|row| (row.0, row.2))
            .collect::<Vec<_>>()
    );
    assert_eq!(results[2]["rowsCovered"], 219);
    assert_eq!(results[2]["sections"][0]["groups"][0]["label"], "2026-11");
    // Without record references the company identifier is not echoed.
    assert!(results[1]["filter"].get("companyId").is_none());
    assert_eq!(results[1]["filter"]["companyFilterApplied"], false);
    for call in &calls {
        assert!(!call.to_string().contains(NAME), "{call}");
    }

    // 3. Names are shared, amounts are not: a sum is refused; a count by
    //    customer carries labels and row counts and no amount.
    let calls = ask(
        &f,
        &alice,
        Some((
            json!(["billing"]),
            json!(["conversation_text", "business_facts", "names"]),
            both.clone(),
            true,
        )),
        json!([
            by("customer", sum_outstanding()),
            by(
                "customer",
                json!({"measure":"count","filters":{"companyId":seed.usd}})
            )
        ]),
    )
    .await;
    assert_eq!(
        calls[0]["readExpansion"]["aggregates"][0]["measures"],
        json!(["count"])
    );
    assert_eq!(
        calls[0]["readExpansion"]["aggregates"][0]["sumFields"],
        json!([])
    );
    let results = calls[1]["authorizedContext"]["aggregateResults"]
        .as_array()
        .unwrap();
    assert_eq!(results[0]["reason"], "aggregate_data_class_not_shared");
    assert_eq!(results[0]["dataClass"], "financial_amounts");
    assert_eq!(results[1]["rowsCovered"], 34);
    let groups = results[1]["sections"][0]["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 7);
    assert!(groups
        .iter()
        .all(|g| g["label"].as_str().unwrap().starts_with(NAME) && g.get("amount").is_none()));
    for call in &calls {
        let text = call.to_string();
        for total in &totals {
            assert!(!text.contains(&total.to_string()), "{total}: {text}");
        }
        assert!(
            !text.contains("\"amount\"") && !text.contains("totalFormatted"),
            "{text}"
        );
    }

    // 4. Statuses and dates are not shared: nothing is offered and even a
    //    plain count is refused.
    let calls = ask(
        &f,
        &alice,
        Some((
            json!(["billing"]),
            json!(["conversation_text", "names", "financial_amounts"]),
            both,
            true,
        )),
        json!([aggregate("billing", "invoices", json!({"measure":"count"}))]),
    )
    .await;
    assert_eq!(calls[0]["readExpansion"]["aggregates"], json!([]));
    assert_eq!(
        calls[1]["authorizedContext"]["aggregateResults"][0]["dataClass"],
        "business_facts"
    );
    assert_no_aggregate_data(&calls[1]["authorizedContext"]["aggregateResults"], &totals);
}

#[tokio::test]
async fn a_shared_aggregate_reaches_the_provider_exactly_and_only_for_the_approved_companies() {
    let (f, _state, app, alice, _) = setup().await;
    let seed = seed(&f, &app, &alice).await;
    // Everything is shared: the request carries exactly the aggregate result.
    let november = json!({"status":"issued","dateFrom":FROM,"dateTo":TO,"companyId":seed.usd});
    let calls = ask(
        &f,
        &alice,
        None,
        json!([aggregate(
            "billing",
            "invoices",
            json!({"measure":"sum","field":"outstanding","groupBy":"customer","filters":november})
        )]),
    )
    .await;
    let results = calls[1]["authorizedContext"]["aggregateResults"]
        .as_array()
        .unwrap();
    assert_eq!(results.len(), 1);
    let observed = results[0]["observedAt"].as_str().unwrap();
    assert_eq!(observed.len(), 20);
    assert!(
        observed.starts_with("20") && observed.ends_with('Z'),
        "{observed}"
    );
    let customers: Vec<(String, i64, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT customer_ref,count(*),sum(outstanding_minor)::bigint FROM archava_billing.invoices WHERE tenant_id='acme-retail' AND company_id='{}' AND {NOVEMBER} GROUP BY customer_ref ORDER BY 3 DESC,2 DESC,1", seed.usd()).as_str()))
        .fetch_all(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(customers.len(), 7);
    let total: i64 = customers.iter().map(|c| c.2).sum();
    let usd = |amount: i64| archava_domain::erp::trade_money::format_trade("USD", amount);
    assert_eq!(
        results[0],
        json!({
            "kind":"aggregate",
            "module":"billing",
            "collection":"invoices",
            "measure":{"kind":"sum","field":"outstanding"},
            "groupBy":"customer",
            "filter":{"status":"issued","dateField":"issued","dateFrom":FROM,"dateTo":TO,"calendar":"UTC calendar dates, both bounds inclusive","companyId":seed.usd},
            "scope":{"companies":"selected company","recordsWithoutCompany":false,"ownRecordsOnly":false},
            "rowsCovered":30,
            "complete":true,
            "incompleteReason":null,
            "observedAt":observed,
            "groupLimit":50,
            "sections":[{
                "rows":30,
                "currency":"USD",
                "unit":"minor units of USD",
                "total":total,
                "totalFormatted":usd(total),
                "groupCount":7,
                "groups":customers.iter().map(|(label, rows, amount)| json!({"label":label,"rows":rows,"amount":amount,"formatted":usd(*amount)})).collect::<Vec<_>>(),
                "other":null
            }],
            "coverage":format!("Billing invoices: sum of outstanding by customer over all 30 matching rows (status issued, issued {FROM}..{TO} (UTC calendar dates, inclusive)); scope: records of the selected company; read at {observed}."),
            "note":"The whole filtered set was aggregated in the database. Amounts of different currencies are separate sections and must never be added together; each section states its unit."
        })
    );
    // Only the USD company is approved: the same unfiltered count now covers
    // that company and the records without a company, and says so. (A turn
    // without a company reaches the provider only when unassigned records
    // are shared, so they are always part of such a turn's scope.)
    let calls = ask(
        &f,
        &alice,
        Some((json!(["billing"]), all_classes(), json!([seed.usd]), true)),
        json!([
            aggregate("billing", "invoices", json!({"measure":"count"})),
            aggregate(
                "billing",
                "invoices",
                json!({"measure":"count","filters":{"companyId":seed.idr}})
            )
        ]),
    )
    .await;
    let results = calls[1]["authorizedContext"]["aggregateResults"]
        .as_array()
        .unwrap();
    assert_eq!(results[0]["rowsCovered"], 34 + 12);
    assert_eq!(
        results[0]["scope"],
        json!({"companies":{"approvedForAiSharing":1},"recordsWithoutCompany":true,"ownRecordsOnly":false})
    );
    assert!(results[0]["coverage"]
        .as_str()
        .unwrap()
        .contains("over all 46 matching rows (no state or date filter); scope: records of the 1 company approved for AI sharing and records without a company; read at"));
    assert_eq!(results[1]["reason"], "aggregate_company_not_shared");
    // A company-scoped turn counts its company only.
    let m = scripted(
        cold_state(&f).await,
        vec![
            reads(json!([aggregate(
                "billing",
                "invoices",
                json!({"measure":"sum","field":"gross"})
            )])),
            answer("The company's gross total."),
        ],
    )
    .await;
    share_all_model_data(&m.app).await;
    let mut input = turn("What is the invoice gross total of this company?");
    input["companyId"] = seed.usd.clone();
    let plan = post(&m.app, "/api/agent/turns", input, &alice).await;
    assert_eq!(plan["goal"]["state"], "completed", "{plan}");
    let calls = m.calls.lock().await;
    let result = &calls[1]["authorizedContext"]["aggregateResults"][0];
    let expected = direct(&f, "gross_minor", &format!("company_id='{}'", seed.usd())).await;
    assert_eq!(result["rowsCovered"], 34);
    assert_eq!(result["scope"]["companies"], "selected company");
    assert_eq!(
        result["sections"],
        json!([{"rows":34,"currency":"USD","unit":"minor units of USD","total":expected[0].2,"totalFormatted":usd(expected[0].2)}])
    );
}

#[tokio::test]
async fn aggregates_share_the_lookup_budget_and_the_last_round_offers_no_read() {
    let (f, state, app, alice, _) = setup().await;
    let _seed = seed(&f, &app, &alice).await;
    let count_by = |group: &str| by(group, json!({"measure":"count"}));
    let page = |query: &str| json!({"module":"billing","collection":"invoices","query":query});
    // Three rounds: an aggregate, then a page read, then the answer.
    let m = scripted(
        state,
        vec![
            reads(json!([count_by("status")])),
            reads(json!([page("A-NOV-1")])),
            answer("219 invoices in all."),
        ],
    )
    .await;
    share_all_model_data(&m.app).await;
    let plan = post(
        &m.app,
        "/api/agent/turns",
        turn("How many invoices are there by status?"),
        &alice,
    )
    .await;
    assert_eq!(plan["goal"]["state"], "completed", "{plan}");
    assert_eq!(plan["planner"]["planningRounds"], 3);
    assert_eq!(plan["planner"]["providerCalls"], 3);
    assert_eq!(plan["planner"]["readQueries"], 2);
    let calls = m.calls.lock().await;
    let schemas = m.schemas.lock().await;
    assert_eq!((calls.len(), schemas.len()), (3, 3));
    for (round, schema) in schemas.iter().enumerate() {
        assert_eq!(
            schema["properties"]["lookups"].is_object(),
            round < 2,
            "round {}",
            round + 1
        );
    }
    assert!(!schemas[2].to_string().contains("aggregate"));
    assert_eq!(calls[2]["readExpansion"]["canRequestAnotherRead"], false);
    // The aggregate is recomputed for every later round and stays beside the
    // page evidence, each under its own key.
    for call in &calls[1..] {
        assert_eq!(
            call["authorizedContext"]["aggregateResults"][0]["rowsCovered"],
            219
        );
    }
    assert_eq!(
        calls[2]["authorizedContext"]["lookupResults"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        calls[2]["authorizedContext"]["lookupResults"][0]["collection"],
        "invoices"
    );
    drop((calls, schemas));
    let text = plan["text"].as_str().unwrap();
    assert!(
        text.contains("- Billing invoices: count by status over all 219 matching rows (no state or date filter); scope: records of the 2 companies approved for AI sharing and records without a company; read at 20"),
        "{text}"
    );

    // More than eight lookups in one answer, of either kind, is refused.
    let nine = json!([
        count_by("status"),
        count_by("month"),
        count_by("customer"),
        by("status", sum_outstanding()),
        by("month", sum_outstanding()),
        page("A-NOV-1"),
        page("A-NOV-2"),
        page("A-NOV-3"),
        page("A-NOV-4")
    ]);
    let over = scripted(cold_state(&f).await, vec![reads(nine)]).await;
    share_all_model_data(&over.app).await;
    let (status, error) = common::call(
        &over.app,
        "POST",
        "/api/agent/turns",
        turn("Total the invoices every way."),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{error}");
    assert_eq!(error["reason"], "agent_lookup_budget_exceeded");
    assert_eq!(over.calls.lock().await.len(), 1);
    // The cap is cumulative across rounds: five aggregates, then four pages.
    let staged = scripted(
        cold_state(&f).await,
        vec![
            reads(json!([
                count_by("status"),
                count_by("month"),
                count_by("customer"),
                by("status", sum_outstanding()),
                by("month", sum_outstanding())
            ])),
            reads(json!([
                page("A-NOV-1"),
                page("A-NOV-2"),
                page("A-NOV-3"),
                page("A-NOV-4")
            ])),
            answer("Unreachable."),
        ],
    )
    .await;
    share_all_model_data(&staged.app).await;
    let (status, error) = common::call(
        &staged.app,
        "POST",
        "/api/agent/turns",
        turn("Total the invoices in stages."),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{error}");
    assert_eq!(error["reason"], "agent_lookup_budget_exceeded");
    assert_eq!(
        staged.calls.lock().await.len(),
        2,
        "no third provider request"
    );
    assert_eq!(
        staged.calls.lock().await[1]["readExpansion"]["remainingReadRequests"],
        3
    );
    // An identical aggregate asked again adds nothing and is not served again.
    let repeated = scripted(
        cold_state(&f).await,
        vec![reads(json!([count_by("status")]))],
    )
    .await;
    share_all_model_data(&repeated.app).await;
    let (status, error) = common::call(
        &repeated.app,
        "POST",
        "/api/agent/turns",
        turn("Count the invoices by status again and again."),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{error}");
    assert_eq!(error["reason"], "agent_lookup_budget_exceeded");
    assert_eq!(error["planningBudget"]["newRequests"], 0);
    assert_eq!(repeated.calls.lock().await.len(), 2);
    assert!(!error.to_string().contains("rowsCovered"));
}

#[tokio::test]
async fn the_planner_is_told_the_selected_companys_trade_currency_and_unit() {
    let (f, state, app, alice, _) = setup().await;
    let idr = company(&app, &alice, "UNIT-IDR").await;
    let usd = usd_company(&app, &alice, "UNIT-USD").await;
    let m = scripted(
        state,
        vec![json!({"summary":"A price is needed.","questions":["Which price should the product have?"],"steps":[]})],
    )
    .await;
    share_all_model_data(&m.app).await;
    for company in [Some(&usd), Some(&idr), None] {
        let mut input = turn("Prepare a new catalog product for this company.");
        if let Some(company) = company {
            input["companyId"] = company.clone();
        }
        let plan = post(&m.app, "/api/agent/turns", input, &alice).await;
        assert_eq!(plan["goal"]["state"], "clarifying", "{plan}");
    }
    let calls = m.calls.lock().await;
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls[0]["authorizedContext"]["tradeCurrency"],
        json!({"currency":"USD","unit":"minor","unitLabel":"minor units of USD","fractionDigits":2,"example":"the integer 1999 is USD 19.99","rule":"Every integer money field of this company's trade records (price, unitCost, unitCostMinor, landedFeeMinor, declaredTotal, taxMinor, unitMinor, amountMinor, totals) is in this unit. Never convert between currencies and never scale an amount by 100 unless this unit requires it."})
    );
    assert_eq!(calls[0]["authorizedContext"]["companyId"], usd);
    let rupiah = &calls[1]["authorizedContext"]["tradeCurrency"];
    assert_eq!(rupiah["currency"], "IDR");
    assert_eq!(rupiah["unit"], "whole_idr");
    assert_eq!(rupiah["unitLabel"], "whole IDR");
    assert_eq!(rupiah["fractionDigits"], 0);
    assert_eq!(rupiah["example"], "the integer 150000 is Rp 150.000");
    // Without a company in scope there is no company currency to state; the
    // general rule is still part of the instructions and of each money field.
    assert!(calls[2]["authorizedContext"].get("tradeCurrency").is_none());
    for call in calls.iter() {
        assert!(call["instructions"].as_str().unwrap().contains("Integer money fields are whole Rupiah for an IDR record and the currency's ISO minor unit for any other currency (1999 is USD 19.99)"));
        let price = call["actionContracts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["tool"] == "commerce.product.create_for_company")
            .unwrap()["inputSchema"]["properties"]["price"]["description"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            price.contains("whole Rupiah when the currency is IDR")
                && price.contains("ISO minor unit"),
            "{price}"
        );
    }
    drop(calls);
    // The stored context of the goal keeps what the planner was told.
    let stored: Value = sqlx::query_scalar("SELECT context->'tradeCurrency' FROM archava_owner.agent_goals WHERE context->>'companyId'=$1")
        .bind(usd.as_str().unwrap())
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(stored["currency"], "USD");
}
