//! Product effects, current authority, confirmed input, atomic rollback and replay.
mod common;
#[path = "agent_runtime/disclosure.rs"]
mod disclosure;
use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::{http::StatusCode, Router};
use serde_json::{json, Value};
use uuid::Uuid;
const TABLES: &[&str] = &[
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
];
async fn setup() -> (Fixture, AppState, Router, Value, Value) {
    let f = provision().await;
    for actor in ["alice", "bob"] {
        sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES($1,$2,$3,true,1)").bind(actor).bind(format!("{actor}@archava.test")).bind(common::HASH).execute(&f.admin_pool).await.unwrap();
        for module in archava_domain::operator::catalogue()["modules"]
            .as_array()
            .unwrap()
        {
            let namespace = module["namespace"].as_str().unwrap();
            let permissions: Vec<&str> = module["permissions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            let statement=format!("INSERT INTO {namespace}.memberships(tenant_id,principal_id,kind,permissions,scope,active) VALUES('acme-retail',$1,'human',$2,'all',true)");
            sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
                .bind(actor)
                .bind(permissions)
                .execute(&f.admin_pool)
                .await
                .unwrap();
        }
    }
    sqlx::query("UPDATE archava_commerce.memberships SET role='owner',capability='enterprise'")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO archava_owner.owner_grants(tenant_id,principal_id,permissions,active) VALUES('acme-retail','alice',ARRAY['config.read','config.apply'],true)").execute(&f.admin_pool).await.unwrap();
    let state = cold_state(&f).await;
    let app = archava_host::router(state.clone());
    let alice = common::login(&app, "alice").await;
    let bob = common::login(&app, "bob").await;
    (f, state, app, alice, bob)
}
async fn cold_state(f: &Fixture) -> AppState {
    AppState::try_new_erp(
        f.runtime_pool.clone(),
        Ledger {
            namespace: "archava_commerce",
            ledger: "commerce",
            tables: TABLES,
        },
        &f.commerce_ledger,
        &f.identity_ledger,
        &[7; 32],
        &f.purchasing_ledger,
        &f.erp_readiness(),
    )
    .await
    .unwrap()
}
async fn post(app: &Router, path: &str, body: Value, session: &Value) -> Value {
    let (status, body) = common::call(app, "POST", path, body, Some(session)).await;
    assert!(status.is_success(), "{path}: {status} {body}");
    body
}
fn turn(text: &str) -> Value {
    json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"utterance":text})
}
/// Explicit synthetic-owner review through the actual public lifecycle.
async fn share_all_model_data(app: &Router) {
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
    let (st, companies) = common::call(
        app,
        "GET",
        "/erp/organizations/workspace?tenant=acme-retail&collection=companies&limit=100",
        Value::Null,
        Some(&alice),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{companies}");
    let companies = companies["companies"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["id"].as_str())
        .collect::<Vec<_>>();
    let modules = archava_domain::operator::catalogue()["modules"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["id"].as_str())
        .filter(|m| *m != "accounting" || !companies.is_empty())
        .collect::<Vec<_>>();
    let request = json!({"baseRevision":config["config"]["revision"],"aiDisclosure":{"kind":"replace","policy":{"providerBinding":config["aiProvider"]["binding"],"modules":modules,"dataClasses":["conversation_text","record_references","business_facts","names","contact_details","notes","financial_amounts"],"companies":companies,"includeWorkspaceRecords":true}}});
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
async fn stock(app: &Router, session: &Value, sku: &str) -> Value {
    let product=post(app,"/erp/commerce/commands",json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"command":{"operation":"product.create","sku":sku,"title":sku,"unit":"unit","trackStock":true,"price":10000}}),session).await;
    let supplier=post(app,"/erp/purchasing/commands",json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"command":{"operation":"supplier.create","code":format!("S-{sku}"),"name":"Agent synthetic supplier"}}),session).await;
    post(app,"/erp/purchasing/commands",json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"command":{"operation":"offer.create","supplierId":supplier["receipt"]["id"],"productId":product["receipt"]["id"],"unitCost":5000}}),session).await;
    product
}
#[tokio::test]
async fn conversations_are_durable_scoped_idempotent_and_do_not_misroute_scheduling() {
    let (f, _state, app, alice, bob) = setup().await;
    let input = turn("Cari jadwal resource yang bentrok dan ajukan pilihan pemindahan reservasi.");
    let reply = post(&app, "/api/agent/turns", input.clone(), &alice).await;
    // Frozen pre-company TurnInput serialization: old durable request hashes must survive upgrade.
    let legacy_wire = format!(
        r#"{{"tenant":"acme-retail","requestId":"{}","threadId":null,"utterance":{},"goalId":null,"action":null}}"#,
        input["requestId"].as_str().unwrap(),
        serde_json::to_string(&input["utterance"]).unwrap()
    );
    use sha2::{Digest, Sha256};
    assert_eq!(
        reply["inputHash"],
        format!("{:x}", Sha256::digest(legacy_wire.as_bytes()))
    );
    let mut explicit_null = input.clone();
    explicit_null["companyId"] = Value::Null;
    assert_eq!(
        post(&app, "/api/agent/turns", explicit_null, &alice).await["goalId"],
        reply["goalId"]
    );
    assert_eq!(reply["source"], "business");
    assert_eq!(reply["data"]["kind"], "agent_clarification");
    assert!(reply["goal"]["modules"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "scheduling"));
    assert!(!reply["goal"]["modules"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "crm"));
    assert_eq!(
        post(&app, "/api/agent/turns", input.clone(), &alice).await["goalId"],
        reply["goalId"]
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_owner.agent_goals")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let mut changed = input.clone();
    changed["utterance"] = json!("Different goal");
    let (s, _) = common::call(&app, "POST", "/api/agent/turns", changed, Some(&alice)).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let thread = reply["threadId"].clone();
    let mut remember =
        turn("Ingat preferensi pembelian saya: tampilkan biaya sebelum meminta approval.");
    remember["threadId"] = thread.clone();
    post(&app, "/api/agent/turns", remember, &alice).await;
    let mut recall = turn("Apa preferensi pembelian yang baru saya sampaikan?");
    recall["threadId"] = thread;
    let recalled = post(&app, "/api/agent/turns", recall, &alice).await;
    assert_eq!(recalled["data"]["items"].as_array().unwrap().len(), 1);
    let other = post(&app, "/api/agent/turns", turn("Apa preferensi saya?"), &bob).await;
    assert!(other["data"]["items"].as_array().unwrap().is_empty());
    let (s, _) = common::call(
        &app,
        "GET",
        &format!(
            "/api/agent/state?tenant=acme-retail&goalId={}",
            reply["goalId"].as_str().unwrap()
        ),
        json!({}),
        Some(&bob),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    sqlx::query(
        "UPDATE archava_scheduling.memberships SET scope='assigned' WHERE principal_id='alice'",
    )
    .execute(&f.admin_pool)
    .await
    .unwrap();
    let (s, _) = common::call(
        &app,
        "GET",
        &format!(
            "/api/agent/state?tenant=acme-retail&goalId={}",
            reply["goalId"].as_str().unwrap()
        ),
        json!({}),
        Some(&alice),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}
#[tokio::test]
async fn stock_goal_review_approval_execution_and_conversational_report_are_connected() {
    let (f, _state, app, alice, _) = setup().await;
    stock(&app, &alice, "AGENT-STOCK").await;
    let input = turn(
        "Ava, urus kebutuhan stok minggu depan dan mintakan persetujuan sebelum menjalankannya.",
    );
    let plan = post(&app, "/api/agent/turns", input, &alice).await;
    assert_eq!(plan["data"]["kind"], "agent_plan", "{plan}");
    assert_eq!(plan["goal"]["state"], "planned");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.orders")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let mut approve = json!({"tenant":"acme-retail","goalId":plan["goalId"],"planHash":plan["goal"]["plan_hash"],"confirmed":true});
    let mut wrong = approve.clone();
    wrong["planHash"] = json!("wrong");
    let (s, _) = common::call(&app, "POST", "/api/agent/approve", wrong, Some(&alice)).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let result = post(&app, "/api/agent/approve", approve.clone(), &alice).await;
    assert_eq!(result["goal"]["state"], "completed", "{result}");
    assert_eq!(
        result["goal"]["outcome"]["steps"][0]["payload_output"]["verified"],
        true
    );
    approve["confirmed"] = json!(true);
    post(&app, "/api/agent/approve", approve, &alice).await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.orders")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let mut report = turn("Laporkan hasil misi dan pekerjaan yang sudah selesai.");
    report["threadId"] = plan["threadId"].clone();
    report["goalId"] = plan["goalId"].clone();
    let report = post(&app, "/api/agent/turns", report, &alice).await;
    assert_eq!(report["data"]["kind"], "agent_report");
    assert_eq!(report["data"]["verifiedCompletedSteps"], 1);
}
#[tokio::test]
async fn durable_worker_recovers_a_transient_failure_without_persisting_a_bearer() {
    let (f, state, app, alice, _) = setup().await;
    stock(&app, &alice, "AGENT-RECOVER").await;
    let plan = post(
        &app,
        "/api/agent/turns",
        turn("Rencanakan pembelian stok yang menipis."),
        &alice,
    )
    .await;
    sqlx::raw_sql("CREATE FUNCTION archava_purchasing.agent_test_fail() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'owned transient failure'; END $$; CREATE TRIGGER agent_test_fail BEFORE INSERT ON archava_purchasing.orders FOR EACH ROW EXECUTE FUNCTION archava_purchasing.agent_test_fail()").execute(&f.admin_pool).await.unwrap();
    let result=post(&app,"/api/agent/approve",json!({"tenant":"acme-retail","goalId":plan["goalId"],"planHash":plan["goal"]["plan_hash"],"confirmed":true}),&alice).await;
    assert_eq!(result["goal"]["state"], "running", "{result}");
    assert!(result["goal"].get("session_hash").is_none());
    sqlx::query("DROP TRIGGER agent_test_fail ON archava_purchasing.orders")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    drop(state);
    let worker = archava_host::agent::AgentWorker::start(cold_state(&f).await);
    let mut done = false;
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let state: String =
            sqlx::query_scalar("SELECT state FROM archava_owner.agent_goals WHERE id=$1")
                .bind(Uuid::parse_str(plan["goalId"].as_str().unwrap()).unwrap())
                .fetch_one(&f.admin_pool)
                .await
                .unwrap();
        if state == "completed" {
            done = true;
            break;
        }
    }
    worker.shutdown().await;
    assert!(done, "worker did not recover");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.orders")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn configured_model_plans_registered_actions_and_passes_verified_ids_between_steps() {
    use axum::{extract::State, routing::post as route_post, Json};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let (f, state, _app, alice, _) = setup().await;
    let app = archava_host::router(state.clone());
    let product = stock(&app, &alice, "MODEL-PRODUCT").await;
    let product_id = product["receipt"]["id"].as_str().unwrap().to_string();
    let calls = Arc::new(AtomicUsize::new(0));
    async fn model(
        State((product, calls)): State<(String, Arc<AtomicUsize>)>,
        headers: axum::http::HeaderMap,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        assert_eq!(headers["x-goog-api-key"], "owned-planner-key");
        assert_eq!(
            body["generationConfig"]["responseMimeType"],
            "application/json"
        );
        calls.fetch_add(1, Ordering::SeqCst);
        let input: Value =
            serde_json::from_str(body["contents"][0]["parts"][0]["text"].as_str().unwrap())
                .unwrap();
        let answer = if input["phase"] == "route" {
            json!({"modules":["commerce","purchasing"]})
        } else {
            assert!(input["authorizedContext"]["sources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["module"] == "commerce"));
            json!({"summary":"Prepare a supplier and offer for the authorized product.","questions":[],"steps":[
             {"title":"Create the reviewed supplier","tool":"purchasing.supplier.create","input":json!({"code":"MODEL-SUPPLIER","name":"Reviewed model supplier"}),"dependsOn":[]},
             {"title":"Create its offer for the authorized product","tool":"purchasing.offer.create","input":json!({"supplierId":{"$from":{"step":0,"path":["receipt","receipt","id"]}},"productId":product,"unitCost":3500}),"dependsOn":[0]}
            ]})
        };
        Json(
            json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":answer.to_string()}]}}]}),
        )
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let fixture = axum::Router::new()
        .route("/v1beta/models/{model}", route_post(model))
        .with_state((product_id, calls.clone()));
    let task = tokio::spawn(async move { axum::serve(listener, fixture).await.unwrap() });
    let planner = archava_providers::agent::GeminiPlanner::new(
        &endpoint,
        "owned-agent-model",
        "owned-planner-key",
    )
    .unwrap();
    let app = archava_host::router(state.with_agent_config(archava_host::agent::AgentConfig {
        planner: Some(planner),
    }));
    share_all_model_data(&app).await;
    let input = turn("Prepare the agreed procurement work.");
    let plan = post(&app, "/api/agent/turns", input.clone(), &alice).await;
    assert_eq!(plan["planner"]["providerId"], "gemini");
    assert_eq!(plan["goal"]["plan"]["steps"].as_array().unwrap().len(), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    post(&app, "/api/agent/turns", input, &alice).await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "replay must not spend another model request"
    );
    let result=post(&app,"/api/agent/approve",json!({"tenant":"acme-retail","goalId":plan["goalId"],"planHash":plan["goal"]["plan_hash"],"confirmed":true}),&alice).await;
    assert_eq!(result["goal"]["state"], "completed", "{result}");
    let supplier: String = sqlx::query_scalar(
        "SELECT id::text FROM archava_purchasing.suppliers WHERE code='MODEL-SUPPLIER'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.offers WHERE supplier_id=$1::uuid AND unit_cost=3500").bind(supplier).fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(count, 1);
    task.abort();
    let _ = task.await;
}
#[tokio::test]
async fn worker_obeys_parent_session_revocation_and_cannot_be_forged_with_headers() {
    let (f, state, app, alice, _) = setup().await;
    stock(&app, &alice, "AGENT-SESSION").await;
    let plan = post(
        &app,
        "/api/agent/turns",
        turn("Review low stock and plan replenishment."),
        &alice,
    )
    .await;
    // Record approved work without executing it, as if the process stopped at the queue checkpoint.
    let goal = Uuid::parse_str(plan["goalId"].as_str().unwrap()).unwrap();
    let hash: String = sqlx::query_scalar(
        "SELECT token_hash FROM archava_identity.sessions WHERE user_id='alice'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    sqlx::query("UPDATE archava_owner.agent_goals SET state='approved',approved_hash=plan_hash,session_hash=$2 WHERE id=$1").bind(goal).bind(&hash).execute(&f.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO archava_owner.agent_work(tenant_id,goal_id) VALUES('acme-retail',$1)")
        .bind(goal)
        .execute(&f.admin_pool)
        .await
        .unwrap();
    use tower::ServiceExt;
    let forged = axum::http::Request::builder()
        .uri("/api/agent/state?tenant=acme-retail")
        .header("x-archava-agent-execution", format!("{goal}:{hash}"))
        .body(axum::body::Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(forged).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    sqlx::query("DELETE FROM archava_identity.sessions WHERE token_hash=$1")
        .bind(hash)
        .execute(&f.admin_pool)
        .await
        .unwrap();
    let worker = archava_host::agent::AgentWorker::start(state);
    tokio::time::sleep(std::time::Duration::from_millis(1300)).await;
    worker.shutdown().await;
    let status: String =
        sqlx::query_scalar("SELECT state FROM archava_owner.agent_goals WHERE id=$1")
            .bind(goal)
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(status, "awaiting_session");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.orders")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let(s,_)=common::call(&app,"POST","/api/agent/approve",json!({"tenant":"acme-retail","goalId":goal,"planHash":plan["goal"]["plan_hash"],"confirmed":true}),None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn changed_supplier_facts_require_a_new_review_and_cancelled_work_never_runs() {
    let (f, _state, app, alice, _) = setup().await;
    stock(&app, &alice, "AGENT-REVIEW").await;
    let plan = post(
        &app,
        "/api/agent/turns",
        turn("Review low stock and plan replenishment for approval."),
        &alice,
    )
    .await;
    let offer: String =
        sqlx::query_scalar("SELECT id::text FROM archava_purchasing.offers LIMIT 1")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    post(&app,"/erp/purchasing/commands",json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"command":{"operation":"offer.set-cost","id":offer,"expectedRevision":1,"reason":"Updated synthetic supplier quote","unitCost":6500}}),&alice).await;
    let result=post(&app,"/api/agent/approve",json!({"tenant":"acme-retail","goalId":plan["goalId"],"planHash":plan["goal"]["plan_hash"],"confirmed":true}),&alice).await;
    assert_eq!(result["goal"]["state"], "needs_review", "{result}");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.orders")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let mut refresh = turn("Refresh this plan using current supplier facts.");
    refresh["threadId"] = plan["threadId"].clone();
    refresh["goalId"] = plan["goalId"].clone();
    let revised = post(&app, "/api/agent/turns", refresh, &alice).await;
    assert_eq!(revised["goal"]["state"], "planned", "{revised}");
    assert_ne!(revised["goalId"], plan["goalId"]);
    assert_ne!(revised["goal"]["plan_hash"], plan["goal"]["plan_hash"]);
    let finished=post(&app,"/api/agent/approve",json!({"tenant":"acme-retail","goalId":revised["goalId"],"planHash":revised["goal"]["plan_hash"],"confirmed":true}),&alice).await;
    assert_eq!(finished["goal"]["state"], "completed", "{finished}");
    let cost: i64 =
        sqlx::query_scalar("SELECT unit_cost::bigint FROM archava_purchasing.lines LIMIT 1")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(cost, 6500);
    let draft = post(
        &app,
        "/api/agent/turns",
        turn("Prepare a reviewed product"),
        &alice,
    )
    .await;
    assert_eq!(draft["goal"]["state"], "clarifying");
    let cancelled=post(&app,"/api/agent/control",json!({"tenant":"acme-retail","goalId":draft["goalId"],"planHash":draft["goal"]["plan_hash"],"confirmed":true,"action":"cancel"}),&alice).await;
    assert_eq!(cancelled["goal"]["state"], "cancelled");
    let(s,_)=common::call(&app,"POST","/api/agent/approve",json!({"tenant":"acme-retail","goalId":draft["goalId"],"planHash":draft["goal"]["plan_hash"],"confirmed":true}),Some(&alice)).await;
    assert_eq!(s, StatusCode::CONFLICT);
}
#[tokio::test]
async fn worker_routing_scope_does_not_open_foreign_business_rows() {
    let (f, _state, app, alice, _) = setup().await;
    let reply = post(
        &app,
        "/api/agent/turns",
        turn("Review project delivery"),
        &alice,
    )
    .await;
    let mut tx = f.runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('archava.tenant_id','foreign-tenant',true),set_config('archava.agent_worker','on',true)").execute(&mut *tx).await.unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_owner.agent_goals WHERE id=$1")
            .bind(Uuid::parse_str(reply["goalId"].as_str().unwrap()).unwrap())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(count, 0);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn failed_turn_commit_rolls_back_the_plan_and_original_request_can_be_retried() {
    let (f, _state, app, alice, _) = setup().await;
    let input = turn("Review project delivery");
    sqlx::raw_sql("CREATE FUNCTION archava_owner.fail_agent_turn() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'owned failed acknowledgement'; END $$; CREATE TRIGGER fail_agent_turn BEFORE INSERT ON archava_owner.agent_turns FOR EACH ROW EXECUTE FUNCTION archava_owner.fail_agent_turn()").execute(&f.admin_pool).await.unwrap();
    let (status, _) = common::call(
        &app,
        "POST",
        "/api/agent/turns",
        input.clone(),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_owner.agent_goals")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(
        count, 0,
        "a plan must not survive without its recoverable turn"
    );
    let path = format!(
        "/api/agent/state?tenant=acme-retail&requestId={}",
        input["requestId"].as_str().unwrap()
    );
    let (_, status) = common::call(&app, "GET", &path, Value::Null, Some(&alice)).await;
    assert_eq!(status["status"], "not_committed");
    sqlx::query("DROP TRIGGER fail_agent_turn ON archava_owner.agent_turns")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    let response = post(&app, "/api/agent/turns", input.clone(), &alice).await;
    let (_, recovered) = common::call(&app, "GET", &path, Value::Null, Some(&alice)).await;
    assert_eq!(recovered["status"], "committed");
    assert_eq!(recovered["turn"]["response"]["goalId"], response["goalId"]);
    let other = post(
        &app,
        "/api/agent/turns",
        turn("What work have you completed?"),
        &alice,
    )
    .await;
    assert_eq!(other["data"]["verifiedCompletedSteps"], 0);
}

#[tokio::test]
async fn request_status_waits_for_inflight_planning_and_read_only_approval_is_refused() {
    let (f, _state, app, alice, _) = setup().await;
    let request = Uuid::new_v4();
    let mut lock = f.admin_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("agent-turn:acme-retail:alice:{request}"))
        .execute(&mut *lock)
        .await
        .unwrap();
    let path = format!("/api/agent/state?tenant=acme-retail&requestId={request}");
    let (_, status) = common::call(&app, "GET", &path, Value::Null, Some(&alice)).await;
    assert_eq!(status["status"], "pending");
    lock.rollback().await.unwrap();
    let mut input = turn("Prepare a reviewed product");
    input["action"] = json!({"tool":"commerce.product.create","values":{"sku":"READ-ONLY-AGENT","title":"Read only","unit":"unit","trackStock":true,"price":1000}});
    sqlx::query(
        "UPDATE archava_commerce.memberships SET capability='assist' WHERE principal_id='alice'",
    )
    .execute(&f.admin_pool)
    .await
    .unwrap();
    let plan = post(&app, "/api/agent/turns", input, &alice).await;
    let (status,_)=common::call(&app,"POST","/api/agent/approve",json!({"tenant":"acme-retail","goalId":plan["goalId"],"planHash":plan["goal"]["plan_hash"],"confirmed":true}),Some(&alice)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_commerce.products WHERE sku='READ-ONLY-AGENT'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn all_eighteen_business_domains_supply_authorized_agent_context_in_both_languages() {
    let (f, _state, app, alice, _) = setup().await;
    for (module, en, id) in [
        (
            "crm",
            "Review customer follow-up",
            "Tinjau kontak pelanggan",
        ),
        ("commerce", "Review product inventory", "Tinjau stok produk"),
        (
            "purchasing",
            "Review supplier offers",
            "Tinjau penawaran pemasok",
        ),
        (
            "billing",
            "Review invoice collection",
            "Tinjau tagihan penagihan",
        ),
        (
            "organizations",
            "Review organization affiliations",
            "Tinjau afiliasi organisasi",
        ),
        ("proposals", "Review proposal scope", "Tinjau proposal"),
        ("projects", "Review project milestones", "Tinjau proyek"),
        (
            "foodservice",
            "Review restaurant kitchen",
            "Tinjau restoran dan dapur",
        ),
        ("accommodation", "Review hotel rooms", "Tinjau kamar tamu"),
        (
            "scheduling",
            "Review resource reservations",
            "Tinjau jadwal reservasi",
        ),
        (
            "assets",
            "Review asset maintenance",
            "Tinjau perawatan aset",
        ),
        (
            "subscriptions",
            "Review subscription entitlements",
            "Tinjau langganan",
        ),
        (
            "attendance",
            "Review attendance sessions",
            "Tinjau kehadiran peserta",
        ),
        ("laundry", "Review laundry process", "Tinjau cucian laundry"),
        (
            "field",
            "Review field service jobs",
            "Tinjau pekerjaan teknisi",
        ),
        (
            "fulfillment",
            "Review shipment dispatch",
            "Tinjau pengiriman",
        ),
        (
            "education",
            "Review student enrollment",
            "Tinjau siswa dan kelas",
        ),
        ("support", "Review support tickets", "Tinjau tiket dukungan"),
    ] {
        for text in [en, id] {
            let response = post(&app, "/api/agent/turns", turn(text), &alice).await;
            assert_eq!(response["source"], "business", "{module}: {response}");
            assert!(
                response["goal"]["context"]["sources"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|s| s["module"] == module && s["available"] == true),
                "{module}: {response}"
            );
            assert_eq!(response["goal"]["state"], "clarifying");
        }
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_cockpit.missions")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(
        count, 0,
        "a context-only question does not authorize hidden business work"
    );
}

#[tokio::test]
async fn reviewed_native_actions_execute_once_with_verified_receipts_in_all_eighteen_domains() {
    let (f, _state, app, alice, _) = setup().await;
    let contact_body=archava_domain::operator::envelope("crm","acme-retail",&Uuid::new_v4().to_string(),"contact.create",&json!({"data":{"fullName":"Agent domain reference","email":"domain-reference@archava.test"}})).unwrap();
    let contact = post(&app, "/crm/commands", contact_body, &alice).await;
    let cases: Value =
        serde_json::from_str(include_str!("fixtures/agent_native_actions.json")).unwrap();
    for row in cases.as_array().unwrap() {
        let (module, namespace, table, action) = (
            row[0].as_str().unwrap(),
            row[1].as_str().unwrap(),
            row[2].as_str().unwrap(),
            row[3].as_str().unwrap(),
        );
        let mut values = row[4].clone();
        if module == "subscriptions" {
            values["contactId"] = contact["receipt"]["id"].clone();
        }
        let statement = format!(
            "SELECT count(*) FROM archava_{namespace}.{table} WHERE tenant_id='acme-retail'"
        );
        let before: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(statement.as_str()))
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
        let mut input = turn(&format!("Prepare reviewed {module} work"));
        input["action"] = json!({"tool":format!("{module}.{action}"),"values":values});
        let (status, plan) = common::call(
            &app,
            "POST",
            "/api/agent/turns",
            input.clone(),
            Some(&alice),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{module}: {plan}");
        assert_eq!(plan["goal"]["state"], "planned", "{module}: {plan}");
        let approval = json!({"tenant":"acme-retail","goalId":plan["goalId"],"planHash":plan["goal"]["plan_hash"],"confirmed":true});
        let result = post(&app, "/api/agent/approve", approval.clone(), &alice).await;
        assert_eq!(result["goal"]["state"], "completed", "{module}: {result}");
        assert_eq!(
            result["goal"]["outcome"]["steps"][0]["payload_output"]["verified"], true,
            "{module}: {result}"
        );
        post(&app, "/api/agent/turns", input, &alice).await;
        post(&app, "/api/agent/approve", approval, &alice).await;
        let after: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(statement.as_str()))
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
        assert_eq!(
            after,
            before + 1,
            "{module}: replay must not create another business row"
        );
    }
}

#[tokio::test]
async fn paused_agent_work_records_the_actual_mission_state_and_resumes_once() {
    let (f, state, app, alice, _) = setup().await;
    let mut input = turn("Prepare reviewed product");
    input["action"] = json!({"tool":"commerce.product.create","values":{"sku":"AGENT-PAUSE","title":"Paused work","unit":"unit","trackStock":true,"price":1000}});
    let plan = post(&app, "/api/agent/turns", input, &alice).await;
    sqlx::raw_sql("CREATE FUNCTION archava_commerce.agent_pause_fail() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'owned temporary failure'; END $$; CREATE TRIGGER agent_pause_fail BEFORE INSERT ON archava_commerce.products FOR EACH ROW EXECUTE FUNCTION archava_commerce.agent_pause_fail()").execute(&f.admin_pool).await.unwrap();
    let approval = json!({"tenant":"acme-retail","goalId":plan["goalId"],"planHash":plan["goal"]["plan_hash"],"confirmed":true});
    let queued = post(&app, "/api/agent/approve", approval.clone(), &alice).await;
    assert_eq!(queued["goal"]["state"], "running", "{queued}");
    let mut control = approval;
    control["action"] = json!("pause");
    let paused = post(&app, "/api/agent/control", control.clone(), &alice).await;
    assert_eq!(paused["goal"]["state"], "holding");
    assert_eq!(paused["goal"]["outcome"]["status"], "holding_pattern");
    sqlx::query("DROP TRIGGER agent_pause_fail ON archava_commerce.products")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    let worker = archava_host::agent::AgentWorker::start(state);
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_commerce.products WHERE sku='AGENT-PAUSE'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
    control["action"] = json!("resume");
    post(&app, "/api/agent/control", control, &alice).await;
    let mut completed = false;
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let status: String =
            sqlx::query_scalar("SELECT state FROM archava_owner.agent_goals WHERE id=$1")
                .bind(Uuid::parse_str(plan["goalId"].as_str().unwrap()).unwrap())
                .fetch_one(&f.admin_pool)
                .await
                .unwrap();
        if status == "completed" {
            completed = true;
            break;
        }
    }
    worker.shutdown().await;
    assert!(completed, "resumed goal did not finish");
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_commerce.products WHERE sku='AGENT-PAUSE'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn conversational_controls_including_negated_or_quoted_commands_never_authorize_work() {
    let (f, _state, app, alice, _) = setup().await;
    let mut input = turn("Prepare reviewed product");
    input["action"] = json!({"tool":"commerce.product.create","values":{"sku":"AGENT-CONTROL-REVIEW","title":"Requires explicit confirmation","unit":"unit","trackStock":true,"price":1000}});
    let plan = post(&app, "/api/agent/turns", input, &alice).await;
    for text in [
        "Do not resume goal",
        "The customer wrote 'resume goal' but I am only asking about it",
        "Resume goal",
        "Do not cancel goal",
        "Pause goal",
    ] {
        let mut input = turn(text);
        input["goalId"] = plan["goalId"].clone();
        input["threadId"] = plan["threadId"].clone();
        let reply = post(&app, "/api/agent/turns", input, &alice).await;
        assert_eq!(reply["data"]["kind"], "agent_control_review");
        assert_eq!(reply["goal"]["state"], "planned", "{text}: {reply}");
        assert!(reply["goal"]["approved_hash"].is_null());
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM archava_commerce.products WHERE sku='AGENT-CONTROL-REVIEW'",
    )
    .fetch_one(&f.admin_pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_owner.agent_work")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(queued, 0);
}

#[tokio::test]
async fn company_scoped_agent_binds_verified_product_and_stock_and_holds_after_revocation() {
    use axum::{routing::post as route_post, Json};
    let (f, state, app, alice, _) = setup().await;
    let source=post(&app,"/erp/organizations/commands",json!({"tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4(),"type":"create_organization","data":{"name":"Agent company","legalName":"Agent Company Official"}}}),&alice).await;
    let company=post(&app,"/erp/organizations/commands",json!({"tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4(),"type":"company.create_from_organization","data":{"code":"AGENT-CO","organizationId":source["receipt"]["organizationId"],"expectedSourceRevision":1,"reason":"Reviewed owned company"}}}),&alice).await;
    let company = company["receipt"]["companyId"].clone();
    async fn model(Json(body): Json<Value>) -> Json<Value> {
        let input: Value =
            serde_json::from_str(body["contents"][0]["parts"][0]["text"].as_str().unwrap())
                .unwrap();
        let company = &input["authorizedContext"]["companyId"];
        let answer = if input["phase"] == "route" {
            json!({"modules":["commerce"]})
        } else {
            assert!(company.is_string());
            json!({"summary":"Create a reviewed company product and count seven units.","questions":[],"steps":[
                {"title":"Create company product","tool":"commerce.product.create_for_company","input":{"companyId":company,"expectedCompanyRevision":1,"sku":"AGENT-COMPANY-ITEM","title":"Company bound goods","unit":"unit","trackStock":true,"price":2500},"dependsOn":[]},
                {"title":"Count company goods","tool":"commerce.stock.adjust_for_company","input":{"companyId":company,"expectedCompanyRevision":1,"productId":{"$from":{"step":0,"path":["receipt","receipt","id"]}},"quantity":7,"unitCostMinor":1000,"reason":"Reviewed owned opening count"},"dependsOn":[0]}
            ]})
        };
        Json(
            json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":answer.to_string()}]}}]}),
        )
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/v1beta/models/{model}", route_post(model)),
        )
        .await
        .unwrap()
    });
    let planner =
        archava_providers::agent::GeminiPlanner::new(&endpoint, "owned-company-model", "owned-key")
            .unwrap();
    let app = archava_host::router(state.with_agent_config(archava_host::agent::AgentConfig {
        planner: Some(planner),
    }));
    share_all_model_data(&app).await;
    let mut input = turn("Create company stock from my reviewed details.");
    input["companyId"] = company.clone();
    let plan = post(&app, "/api/agent/turns", input, &alice).await;
    assert_eq!(plan["goal"]["context"]["companyId"], company);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_commerce.products")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let approval = json!({"tenant":"acme-retail","goalId":plan["goalId"],"planHash":plan["goal"]["plan_hash"],"confirmed":true});
    let result = post(&app, "/api/agent/approve", approval.clone(), &alice).await;
    assert_eq!(result["goal"]["state"], "completed", "{result}");
    post(&app, "/api/agent/approve", approval, &alice).await;
    let proof:(String,i32,i64)=sqlx::query_as("SELECT p.company_id::text,b.quantity,(SELECT count(*) FROM archava_commerce.movements WHERE product_id=p.id) FROM archava_commerce.products p JOIN archava_commerce.stock_balances b ON b.product_id=p.id AND b.tenant_id=p.tenant_id WHERE p.sku='AGENT-COMPANY-ITEM'").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(proof, (company.as_str().unwrap().to_string(), 7, 1));
    let mut legacy = turn("Create an unassigned product inside a company goal.");
    legacy["companyId"] = company.clone();
    legacy["action"] = json!({"tool":"commerce.product.create","values":{"sku":"MUST-REFUSE","title":"Refused","unit":"unit","trackStock":false,"price":1}});
    assert_eq!(
        common::call(&app, "POST", "/api/agent/turns", legacy, Some(&alice))
            .await
            .0,
        StatusCode::BAD_GATEWAY
    );
    let mut second = turn("Prepare another company product.");
    second["companyId"] = company.clone();
    second["action"] = json!({"tool":"commerce.product.create_for_company","values":{"companyId":company,"expectedCompanyRevision":1,"sku":"HELD-COMPANY-ITEM","title":"Held goods","unit":"unit","trackStock":true,"price":1}});
    let held = post(&app, "/api/agent/turns", second, &alice).await;
    sqlx::query("UPDATE archava_organizations.memberships SET permissions=array_remove(permissions,'companies.read') WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    let (st,_)=common::call(&app,"POST","/api/agent/approve",json!({"tenant":"acme-retail","goalId":held["goalId"],"planHash":held["goal"]["plan_hash"],"confirmed":true}),Some(&alice)).await;
    assert!(st.is_client_error());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_commerce.products")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn supplier_document_agent_executes_reviewed_claim_then_match_once_and_refuses_revoked_approval(
) {
    use axum::{routing::post as route_post, Json};
    let (f, state, app, s, _) = setup().await;
    let o=post(&app,"/erp/organizations/commands",json!({"tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4(),"type":"create_organization","data":{"name":"Supplier document company","legalName":"Supplier Document Official"}}}),&s).await;
    let company=post(&app,"/erp/organizations/commands",json!({"tenant":"acme-retail","mutation":{"requestId":Uuid::new_v4(),"type":"company.create_from_organization","data":{"organizationId":o["receipt"]["organizationId"],"expectedSourceRevision":1,"code":"AVA-DOC","reason":"Reviewed operational company"}}}),&s).await["receipt"]["companyId"].clone();
    async fn command(app: &Router, s: &Value, domain: &str, command: Value) -> Value {
        post(
            app,
            &format!("/erp/{domain}/commands"),
            json!({"tenant":"acme-retail","requestId":Uuid::new_v4(),"command":command}),
            s,
        )
        .await["receipt"]
            .clone()
    }
    let p=command(&app,&s,"commerce",json!({"operation":"product.create_for_company","companyId":company,"expectedCompanyRevision":1,"sku":"AVA-DOC-GOODS","title":"Supplier document goods","unit":"unit","trackStock":true,"price":9000})).await;
    let supplier = command(
        &app,
        &s,
        "purchasing",
        json!({"operation":"supplier.create","code":"AVA-DOC-VENDOR","name":"Owned supplier"}),
    )
    .await;
    let offer=command(&app,&s,"purchasing",json!({"operation":"offer.create","supplierId":supplier["id"],"productId":p["id"],"unitCost":5000})).await;
    let po=command(&app,&s,"purchasing",json!({"operation":"order.create-for-company","companyId":company,"expectedCompanyRevision":1,"supplierId":supplier["id"],"lines":[{"offerId":offer["id"],"quantity":2}]})).await;
    command(&app,&s,"purchasing",json!({"operation":"order.confirm","id":po["id"],"expectedRevision":1,"reason":"Approved purchase"})).await;
    let (st, w) = common::call(
        &app,
        "GET",
        &format!(
            "/erp/purchasing/workspace?tenant=acme-retail&companyId={}",
            company.as_str().unwrap()
        ),
        Value::Null,
        Some(&s),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");
    command(&app,&s,"purchasing",json!({"operation":"order.receive","id":po["id"],"expectedRevision":2,"reason":"Two accepted goods","lines":[{"lineId":w["orders"][0]["lines"][0]["id"],"accepted":2,"rejected":0}]})).await;
    async fn model(Json(body): Json<Value>) -> Json<Value> {
        let input: Value =
            serde_json::from_str(body["contents"][0]["parts"][0]["text"].as_str().unwrap())
                .unwrap();
        if input["phase"] == "route" {
            return Json(
                json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":json!({"modules":["purchasing"]}).to_string()}]}}]}),
            );
        }
        assert!(input["authorizedContext"]["companyId"].is_string());
        let source = input["authorizedContext"]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["module"] == "purchasing")
            .unwrap();
        let po = &source["data"]["orders"][0];
        let plan = json!({"summary":"Capture the supplied unposted goods claim, then check exact accepted goods. No accounting posting or payment.","questions":[],"steps":[
            {"title":"Capture actual supplier claim","tool":"purchasing.document.capture","input":{"orderId":po["id"],"expectedOrderRevision":po["revision"],"supplierId":po["supplierId"],"vendorReference":"AVA-SUPPLIER-DOC","documentDate":"2026-11-07","currency":"IDR","declaredTotal":10000,"lines":[{"lineId":po["lines"][0]["id"],"quantity":2,"unitCost":5000}]},"dependsOn":[]},
            {"title":"Check accepted receipt and exact price","tool":"purchasing.document.match","input":{"id":{"$from":{"step":0,"path":["receipt","receipt","id"]}},"expectedRevision":1,"expectedOrderRevision":po["revision"],"reason":"Review actual supplied vendor claim"},"dependsOn":[0]}
        ]});
        Json(
            json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":plan.to_string()}]}}]}),
        )
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/{*path}", route_post(model)))
            .await
            .unwrap()
    });
    let planner = archava_providers::agent::GeminiPlanner::new(
        &endpoint,
        "owned-supplier-model",
        "owned-key",
    )
    .unwrap();
    let app = archava_host::router(state.with_agent_config(archava_host::agent::AgentConfig {
        planner: Some(planner),
    }));
    share_all_model_data(&app).await;
    let mut input=turn("Purchasing: supplier goods document AVA-SUPPLIER-DOC dated 2026-11-07 claims two units at IDR 5000, total 10000, against the received PO. Capture and check goods only.");
    input["companyId"] = company.clone();
    let plan = post(&app, "/api/agent/turns", input, &s).await;
    assert_eq!(plan["goal"]["plan"]["steps"].as_array().unwrap().len(), 2);
    let before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_purchasing.supplier_documents")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(before, 0);
    let approve = json!({"tenant":"acme-retail","goalId":plan["goalId"],"planHash":plan["goal"]["plan_hash"],"confirmed":true});
    let done = post(&app, "/api/agent/approve", approve.clone(), &s).await;
    assert_eq!(done["goal"]["state"], "completed", "{done}");
    post(&app, "/api/agent/approve", approve, &s).await;
    let proof:(String,i32,i64,i64)=sqlx::query_as("SELECT state,revision,(SELECT count(*) FROM archava_purchasing.supplier_documents),(SELECT sum(quantity)::bigint FROM archava_purchasing.supplier_document_allocations WHERE released_revision IS NULL) FROM archava_purchasing.supplier_documents").fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(proof, ("matched".into(), 2, 1, 2));
    let id: String =
        sqlx::query_scalar("SELECT id::text FROM archava_purchasing.supplier_documents")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    let mut revoke = turn("Review voiding this unposted supplier claim.");
    revoke["companyId"] = company;
    revoke["action"] = json!({"tool":"purchasing.document.void","values":{"id":id,"expectedRevision":2,"reason":"Explicit reviewed withdrawal"}});
    let plan = post(&app, "/api/agent/turns", revoke, &s).await;
    sqlx::query("UPDATE archava_purchasing.memberships SET permissions=array_remove(permissions,'documents.write') WHERE principal_id='alice'").execute(&f.admin_pool).await.unwrap();
    let (status,held)=common::call(&app,"POST","/api/agent/approve",json!({"tenant":"acme-retail","goalId":plan["goalId"],"planHash":plan["goal"]["plan_hash"],"confirmed":true}),Some(&s)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{held}");
    assert_eq!(held["reason"], "agent_context_access_changed");
    let current: (String, i32) =
        sqlx::query_as("SELECT state,revision FROM archava_purchasing.supplier_documents")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(current, ("matched".into(), 2));
    server.abort();
    let _ = server.await;
}

#[path = "agent_runtime/aggregates.rs"]
mod agent_aggregates;
#[path = "agent_runtime/failures.rs"]
mod agent_failures;
#[path = "agent_runtime/lookups.rs"]
mod agent_lookups;
