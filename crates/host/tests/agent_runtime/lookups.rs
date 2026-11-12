use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
async fn command(app: &Router, s: &Value, domain: &str, input: Value) -> Value {
    let body = if domain == "support" {
        json!({"tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4(),"command":input}})
    } else {
        json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"command":input})
    };
    post(app, &format!("/erp/{domain}/commands"), body, s).await["receipt"].clone()
}
async fn products(app: &Router, s: &Value, n: usize) {
    for i in 0..n {
        command(app,s,"commerce",json!({"operation":"product.create","sku":format!("PAD-{i}"),"title":format!("Recent unrelated goods {i}"),"unit":"unit","trackStock":false,"price":100})).await;
    }
}
async fn target(app: &Router, s: &Value) -> Value {
    command(app,s,"commerce",json!({"operation":"product.create","sku":"FAR-TARGET","title":"Old target %_& goods","unit":"unit","trackStock":false,"price":10000})).await
}
fn approve(plan: &Value) -> Value {
    json!({"tenant":"acme-retail","goalId":plan["goalId"],"planHash":plan["goal"]["plan_hash"],"confirmed":true})
}
async fn model_app(state: AppState, model: Router) -> (Router, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, model).await.unwrap() });
    let planner =
        archava_providers::agent::GeminiPlanner::new(&endpoint, "owned-lookup-model", "owned-key")
            .unwrap();
    let app = archava_host::router(state.with_agent_config(archava_host::agent::AgentConfig {
        planner: Some(planner),
    }));
    share_all_model_data(&app).await;
    (app, task)
}
fn wire(answer: Value) -> axum::Json<Value> {
    axum::Json(
        json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":answer.to_string()}]}}]}),
    )
}
#[tokio::test]
async fn model_reads_named_product_beyond_one_hundred_rows_and_rechecks_exact_target_before_execution(
) {
    use axum::{extract::State, routing::post as route_post, Json};
    let (f, state, app, s, _) = setup().await;
    let p = target(&app, &s).await;
    products(&app, &s, 110).await;
    let (st, w) = common::call(
        &app,
        "GET",
        "/erp/commerce/workspace?tenant=acme-retail",
        Value::Null,
        Some(&s),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(w["products"].as_array().unwrap().len(), 100);
    assert!(!w["products"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["id"] == p["id"]));
    async fn model(State(calls): State<Arc<AtomicUsize>>, Json(body): Json<Value>) -> Json<Value> {
        calls.fetch_add(1, Ordering::SeqCst);
        let input: Value =
            serde_json::from_str(body["contents"][0]["parts"][0]["text"].as_str().unwrap())
                .unwrap();
        if input["phase"] == "route" {
            return wire(json!({"modules":["commerce"]}));
        }
        let c = &input["authorizedContext"];
        let src = c["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["module"] == "commerce")
            .unwrap();
        if c.get("lookupResults").is_none() {
            assert_eq!(src["coverage"]["products"]["included"], 25);
            assert_eq!(src["coverage"]["products"]["wholeCollection"], false);
            return wire(
                json!({"summary":"Find the named record beyond this bounded page","questions":[],"steps":[],"lookups":[{"module":"commerce","collection":"products","query":"Old target %_& goods"}]}),
            );
        }
        let p = &src["data"]["products"][0];
        assert_eq!(p["title"], "Old target %_& goods");
        assert_eq!(src["data"]["products"].as_array().unwrap().len(), 1);
        wire(
            json!({"summary":"Review price 12000 for the exact named product","questions":[],"steps":[{"title":"Update selected price","tool":"commerce.product.set","input":{"id":p["id"],"expectedRevision":p["revision"],"price":12000,"active":true},"dependsOn":[]}]}),
        )
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let (app, task) = model_app(
        state,
        Router::new()
            .route("/v1beta/models/{model}", route_post(model))
            .with_state(calls.clone()),
    )
    .await;
    let plan = post(
        &app,
        "/api/agent/turns",
        turn("Commerce: set the price of Old target %_& goods to 12000."),
        &s,
    )
    .await;
    assert_eq!(plan["planner"]["planningRounds"], 2);
    assert_eq!(plan["planner"]["readQueries"], 1);
    assert_eq!(plan["goal"]["plan"]["steps"][0]["input"]["id"], p["id"]);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    command(&app,&s,"commerce",json!({"operation":"product.create","sku":"EVEN-LATER","title":"Push the first page again","unit":"unit","trackStock":false,"price":5})).await;
    let done = post(&app, "/api/agent/approve", approve(&plan), &s).await;
    assert_eq!(done["goal"]["state"], "completed", "{done}");
    post(&app, "/api/agent/approve", approve(&plan), &s).await;
    let row: (i32, i32) = sqlx::query_as(
        "SELECT price,revision FROM archava_commerce.products WHERE sku='FAR-TARGET'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(row, (12000, 2));
    task.abort();
    let _ = task.await;
}
#[tokio::test]
async fn explicit_action_loads_old_id_and_detects_a_real_change_instead_of_page_movement() {
    let (f, _state, app, s, _) = setup().await;
    let p = target(&app, &s).await;
    products(&app, &s, 105).await;
    let mut t = turn("Review this exact commerce price change.");
    t["action"] = json!({"tool":"commerce.product.set","values":{"id":p["id"],"expectedRevision":1,"price":15000,"active":true}});
    let plan = post(&app, "/api/agent/turns", t, &s).await;
    assert_eq!(
        plan["goal"]["context"]["lookupResults"][0]["matchingRowsInSourcePage"],
        1
    );
    command(&app,&s,"commerce",json!({"operation":"product.set","id":p["id"],"expectedRevision":1,"price":11000,"active":true})).await;
    let done = post(&app, "/api/agent/approve", approve(&plan), &s).await;
    assert_eq!(done["goal"]["state"], "needs_review", "{done}");
    let price: i32 =
        sqlx::query_scalar("SELECT price FROM archava_commerce.products WHERE sku='FAR-TARGET'")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(price, 11000);
}
#[tokio::test]
async fn lookup_protocol_refuses_writes_network_targets_and_invented_ids_before_business_effects() {
    use axum::{routing::post as route_post, Json};
    let (f, state, app, s, _) = setup().await;
    target(&app, &s).await;
    async fn model(Json(body): Json<Value>) -> Json<Value> {
        let input: Value =
            serde_json::from_str(body["contents"][0]["parts"][0]["text"].as_str().unwrap())
                .unwrap();
        if input["phase"] == "route" {
            return wire(json!({"modules":["commerce"]}));
        }
        let text = input["goal"].as_str().unwrap();
        let lookup = if text.contains("network") {
            json!({"module":"commerce","collection":"products","query":"Old","url":"https://example.com"})
        } else {
            json!({"module":"commerce","collection":"products","id":"00000000-0000-4000-8000-000000000099"})
        };
        wire(json!({"summary":"Untrusted lookup","questions":[],"steps":[],"lookups":[lookup]}))
    }
    let (app, task) = model_app(
        state,
        Router::new().route("/v1beta/models/{model}", route_post(model)),
    )
    .await;
    for text in ["Commerce network lookup", "Commerce invented lookup"] {
        let (st, b) = common::call(&app, "POST", "/api/agent/turns", turn(text), Some(&s)).await;
        assert_eq!(st, StatusCode::BAD_GATEWAY, "{b}");
        assert!(
            matches!(
                b["reason"].as_str(),
                Some("invalid_agent_lookup" | "agent_lookup_id_not_grounded")
            ),
            "{b}"
        );
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_commerce.products")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let goals: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_owner.agent_goals")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(goals, 0);
    task.abort();
    let _ = task.await;
}
#[tokio::test]
async fn selected_company_cannot_review_a_different_companys_record_even_when_both_are_readable() {
    let (f, _state, app, s, _) = setup().await;
    async fn company(app: &Router, s: &Value, code: &str) -> Value {
        let o=post(app,"/erp/organizations/commands",json!({"tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4(),"type":"create_organization","data":{"name":code,"legalName":format!("{code} official")}}}),s).await;
        post(app,"/erp/organizations/commands",json!({"tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4(),"type":"company.create_from_organization","data":{"organizationId":o["receipt"]["organizationId"],"expectedSourceRevision":1,"code":code,"reason":"Explicit current company"}}}),s).await["receipt"]["companyId"].clone()
    }
    let a = company(&app, &s, "LOOKUP-A").await;
    let b = company(&app, &s, "LOOKUP-B").await;
    let p=command(&app,&s,"commerce",json!({"operation":"product.create_for_company","companyId":b,"expectedCompanyRevision":1,"sku":"OTHER-COMPANY","title":"Same name","unit":"unit","trackStock":false,"price":900})).await;
    let mut t = turn("Review this commerce update in company A.");
    t["companyId"] = a.clone();
    t["action"] = json!({"tool":"commerce.product.set","values":{"id":p["id"],"expectedRevision":1,"price":1,"active":true}});
    let (st, r) = common::call(&app, "POST", "/api/agent/turns", t, Some(&s)).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{r}");
    assert_eq!(r["reason"], "agent_reference_not_visible");
    let (st, w) = common::call(
        &app,
        "GET",
        &format!(
            "/erp/commerce/workspace?tenant=acme-retail&companyId={}&id={}",
            a.as_str().unwrap(),
            p["id"].as_str().unwrap()
        ),
        Value::Null,
        Some(&s),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(w["products"], json!([]));
    let price: i32 =
        sqlx::query_scalar("SELECT price FROM archava_commerce.products WHERE sku='OTHER-COMPANY'")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(price, 900);
}

#[tokio::test]
async fn support_search_finds_literal_unicode_target_beyond_first_page_and_preserves_assignment_scope(
) {
    use axum::{routing::post as route_post, Json};
    let (f, state, app, s, bob) = setup().await;
    for i in 0..35 {
        command(&app,&s,"support",json!({"operation":"case.create","requesterName":"Owned caller","subject":format!("Ticket %_&/日本語 {i:03}"),"description":"Literal search sample","priority":"normal","dueAt":null})).await;
    }
    let (st, all) = common::call(
        &app,
        "GET",
        "/erp/support/cases?tenant=acme-retail&limit=100",
        Value::Null,
        Some(&s),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let target = all["items"].as_array().unwrap().last().unwrap().clone();
    let title = target["subject"].as_str().unwrap();
    async fn model(Json(body): Json<Value>) -> Json<Value> {
        let input: Value =
            serde_json::from_str(body["contents"][0]["parts"][0]["text"].as_str().unwrap())
                .unwrap();
        let c = &input["authorizedContext"];
        if c.get("lookupResults").is_none() {
            let title = input["goal"]
                .as_str()
                .unwrap()
                .strip_prefix("Support: start ")
                .unwrap();
            return wire(
                json!({"summary":"Find the exact ticket","questions":[],"steps":[],"lookups":[{"module":"support","collection":"items","query":title}]}),
            );
        }
        let src = c["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["module"] == "support")
            .unwrap();
        assert_eq!(src["data"]["items"].as_array().unwrap().len(), 1);
        let p = &src["data"]["items"][0];
        wire(
            json!({"summary":"Start the selected support ticket","questions":[],"steps":[{"title":"Start ticket","tool":"support.case.start","input":{"id":p["id"],"expectedRevision":p["revision"],"reason":"Review exact literal target"},"dependsOn":[]}]}),
        )
    }
    let (app, task) = model_app(
        state,
        Router::new().route("/v1beta/models/{model}", route_post(model)),
    )
    .await;
    let plan = post(
        &app,
        "/api/agent/turns",
        turn(&format!("Support: start {title}")),
        &s,
    )
    .await;
    assert_eq!(
        plan["goal"]["plan"]["steps"][0]["input"]["id"],
        target["id"]
    );
    let done = post(&app, "/api/agent/approve", approve(&plan), &s).await;
    assert_eq!(done["goal"]["state"], "completed", "{done}");
    sqlx::query("UPDATE archava_support.memberships SET scope='assigned' WHERE principal_id='bob'")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    let (st, w) = common::call(
        &app,
        "GET",
        &format!(
            "/erp/support/cases?tenant=acme-retail&search={}",
            title
                .bytes()
                .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                })
                .collect::<String>()
        ),
        Value::Null,
        Some(&bob),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");
    assert_eq!(w["items"], json!([]));
    let mut t = turn("Review starting the other operator's support ticket.");
    t["action"] = json!({"tool":"support.case.start","values":{"id":target["id"],"expectedRevision":2,"reason":"Not my assigned case"}});
    let (st, w) = common::call(&app, "POST", "/api/agent/turns", t, Some(&bob)).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{w}");
    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn large_authorized_workspace_is_byte_bounded_and_exact_review_guards_still_execute() {
    let (f, _state, app, s, _) = setup().await;
    for n in 0..30 {
        post(&app,"/erp/organizations/commands",json!({"tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4(),"type":"create_organization","data":{"name":format!("{} {n}","界".repeat(49)),"legalName":"社".repeat(50),"billingAddress":"住".repeat(160),"taxId":"T".repeat(48),"industry":"業".repeat(32),"website":format!("https://example.test/{}","x".repeat(170))}}}),&s).await;
    }
    let (st, raw) = common::call(
        &app,
        "GET",
        "/erp/organizations/workspace?tenant=acme-retail",
        Value::Null,
        Some(&s),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{raw}");
    assert!(
        raw.to_string().len() > 32_768,
        "The real workspace must exceed the unchanged source allowance"
    );
    let mut input = turn("Prepare organization work in this populated workspace");
    input["action"] = json!({"tool":"organizations.create_organization","values":{"name":"Reviewed bounded workspace organization"}});
    let plan = post(&app, "/api/agent/turns", input, &s).await;
    assert_eq!(plan["goal"]["state"], "planned");
    let source = plan["goal"]["context"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["module"] == "organizations")
        .unwrap();
    assert!(source["data"].to_string().len() <= 32_768);
    assert_eq!(source["byteBudget"]["limited"], true);
    assert_eq!(
        source["coverage"]["organizations"]["wholeCollection"],
        false
    );
    assert_eq!(source["coverage"]["organizations"]["limitedByBytes"], true);
    let included = source["data"]["organizations"].as_array().unwrap();
    let target = raw["organizations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| !included.iter().any(|i| i["id"] == r["id"]))
        .unwrap();
    // An authorized exact reference outside the byte-bounded view remains usable.
    let mut update = turn("Review this exact organization change");
    update["action"] = json!({"tool":"organizations.update_organization","values":{"id":target["id"],"revision":target["revision"],"name":"Exact reviewed change"}});
    let review = post(&app, "/api/agent/turns", update, &s).await;
    assert_eq!(review["goal"]["state"], "planned");
    assert!(review["goal"]["plan"]["steps"][0]["guards"]
        .as_array()
        .unwrap()
        .iter()
        .any(|g| g["id"] == target["id"]));
    let done = post(&app, "/api/agent/approve", approve(&review), &s).await;
    assert_eq!(done["goal"]["state"], "completed", "{done}");
    assert_eq!(
        done["goal"]["outcome"]["steps"][0]["payload_output"]["verified"],
        true
    );
    post(&app, "/api/agent/approve", approve(&review), &s).await;
    let current: (String, i32) =
        sqlx::query_as("SELECT name,revision FROM archava_organizations.organizations WHERE id=$1")
            .bind(Uuid::parse_str(target["id"].as_str().unwrap()).unwrap())
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(current, ("Exact reviewed change".into(), 2));
}
