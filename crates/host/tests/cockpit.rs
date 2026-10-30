//! Real router + non-owner PostgreSQL regression tests for durable mission commands.
use archava_host::{router, AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

const KEY: &[u8] = &[7u8; 32];
const PASSWORD: &str = "correct horse battery staple";
const HASH: &str =
    "scrypt$32768$8$3$OZtInKxSaSZqbEptj2fvUg$jU_gx2rQW-69EYpbRoMKGSgclhV-mO7gXVF-QYvSXVo";
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
fn ledger() -> Ledger {
    Ledger {
        namespace: "archava_commerce",
        ledger: "Commerce",
        tables: TABLES,
    }
}
async fn state(f: &Fixture, pool: sqlx::PgPool) -> AppState {
    AppState::try_new(pool, ledger(), &f.commerce_ledger, &f.identity_ledger, KEY)
        .await
        .unwrap()
}
async fn setup() -> (Fixture, AppState, Router, Value) {
    let f = provision().await;
    for (id, tenant, capability, scope) in [
        ("alice", "acme", "enterprise", "all"),
        ("viewer", "acme", "assist", "all"),
        ("assigned", "acme", "act", "assigned"),
        ("mallory", "other", "enterprise", "all"),
    ] {
        sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES($1,$2,$3,true,1)").bind(id).bind(format!("{id}@archava.test")).bind(HASH).execute(&f.admin_pool).await.unwrap();
        sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope,active,role,capability) VALUES($1,$2,'human','{}',$3,true,'owner',$4)").bind(tenant).bind(id).bind(scope).bind(capability).execute(&f.admin_pool).await.unwrap();
    }
    let state = state(&f, f.runtime_pool.clone()).await;
    let app = router(state.clone());
    let session = login(&app, "alice").await;
    (f, state, app, session)
}
async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    session: Option<&Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header("x-archava-client", "cli");
    if let Some(s) = session {
        request = request
            .header(
                "authorization",
                format!("Bearer {}", s["token"].as_str().unwrap()),
            )
            .header("x-archava-csrf", s["csrf"].as_str().unwrap());
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
async fn login(app: &Router, user: &str) -> Value {
    let (status, value) = call(
        app,
        "POST",
        "/login",
        json!({"username":format!("{user}@archava.test"),"password":PASSWORD}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    value
}
fn mission() -> Value {
    json!({"tenant_id":"acme", "mission_code":"AUDIT-01", "title":"Durable mission", "description":"Synthetic regression fixture", "auto_launch":true, "steps":[{"step_index":0,"step_name":"First","tool_name":"test.first"},{"step_index":1,"step_name":"Second","tool_name":"test.second","depends_on":[0]}]})
}
fn report() -> Value {
    json!({"agent_role":"test-worker","success":true,"high_signal_summary":"Synthetic result","output_payload":{"value":42},"token_cost":10})
}
async fn create(app: &Router, session: &Value) -> String {
    let (status, body) = call(
        app,
        "POST",
        "/api/v1/cockpit/missions",
        mission(),
        Some(session),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().unwrap().to_owned()
}
async fn db_status(f: &Fixture, id: &str) -> (String, String) {
    sqlx::query_as("SELECT m.status,s.status FROM archava_cockpit.missions m JOIN archava_cockpit.flight_steps s ON s.mission_id=m.id WHERE m.id=$1::uuid AND s.step_index=0").bind(id).fetch_one(&f.admin_pool).await.unwrap()
}
async fn step_events(f: &Fixture, id: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM archava_cockpit.flight_events WHERE mission_id=$1::uuid AND event_type='step_completed'").bind(id).fetch_one(&f.admin_pool).await.unwrap()
}
#[tokio::test]
async fn failed_checkpoint_rolls_back_step_event_and_cache_then_retry_commits() {
    let (f, state, app, session) = setup().await;
    let id = create(&app, &session).await;
    sqlx::raw_sql("CREATE FUNCTION archava_cockpit.reject_checkpoint() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic persistence failure'; END $$; CREATE TRIGGER reject_checkpoint BEFORE INSERT OR UPDATE ON archava_cockpit.checkpoints FOR EACH ROW EXECUTE FUNCTION archava_cockpit.reject_checkpoint();").execute(&f.admin_pool).await.unwrap();
    let path = format!("/api/v1/cockpit/missions/{id}/steps/0/complete?tenant=acme");
    let (status, _) = call(&app, "POST", &path, report(), Some(&session)).await;
    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "failed persistence must not acknowledge success"
    );
    assert_eq!(
        db_status(&f, &id).await,
        ("in_flight".into(), "pending".into())
    );
    assert_eq!(step_events(&f, &id).await, 0);
    assert_eq!(
        state
            .cockpit()
            .get_mission(id.parse().unwrap())
            .unwrap()
            .steps[0]
            .status,
        archava_cockpit::StepStatus::Pending
    );
    sqlx::raw_sql("DROP TRIGGER reject_checkpoint ON archava_cockpit.checkpoints")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(
        call(&app, "POST", &path, report(), Some(&session)).await.0,
        StatusCode::OK
    );
    assert_eq!(step_events(&f, &id).await, 1);
}
#[tokio::test]
async fn cold_restart_dependency_replay_conflict_and_concurrency_are_durable() {
    let (f, _state, app, session) = setup().await;
    let id = create(&app, &session).await;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(f.runtime_options())
        .await
        .unwrap();
    let restarted = router(state(&f, pool.clone()).await);
    let read = format!("/api/v1/cockpit/missions/{id}?tenant=acme");
    assert_eq!(
        call(&restarted, "GET", &read, Value::Null, Some(&session))
            .await
            .0,
        StatusCode::OK
    );
    let first = format!("/api/v1/cockpit/missions/{id}/steps/0/complete?tenant=acme");
    let second = format!("/api/v1/cockpit/missions/{id}/steps/1/complete?tenant=acme");
    assert_eq!(
        call(&restarted, "POST", &second, report(), Some(&session))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(&restarted, "POST", &first, report(), Some(&session))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&restarted, "POST", &first, report(), Some(&session))
            .await
            .0,
        StatusCode::OK
    );
    let mut changed = report();
    changed["output_payload"] = json!({"value":99});
    assert_eq!(
        call(&restarted, "POST", &first, changed, Some(&session))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let (a, b) = tokio::join!(
        call(&restarted, "POST", &second, report(), Some(&session)),
        call(&restarted, "POST", &second, report(), Some(&session))
    );
    assert_eq!(a.0, StatusCode::OK, "{}", a.1);
    assert_eq!(b.0, StatusCode::OK, "{}", b.1);
    assert_eq!(step_events(&f, &id).await, 2);
    let cold = router(state(&f, pool.clone()).await);
    let (status, result) = call(&cold, "GET", &read, Value::Null, Some(&session)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["status"], "touchdown");
    assert_eq!(result["steps"][0]["payload_output"]["value"], 42);
    assert_eq!(
        call(&cold, "POST", &first, report(), Some(&session))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(step_events(&f, &id).await, 2);
    pool.close().await;
}
#[tokio::test]
async fn read_write_scope_csrf_revocation_and_invalid_indices_fail_closed() {
    let (f, _state, app, session) = setup().await;
    let id = create(&app, &session).await;
    let read = format!("/api/v1/cockpit/missions/{id}?tenant=acme");
    assert_eq!(
        call(&app, "GET", &read, Value::Null, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    for user in ["assigned", "mallory"] {
        let other = login(&app, user).await;
        assert_eq!(
            call(&app, "GET", &read, Value::Null, Some(&other)).await.0,
            StatusCode::FORBIDDEN,
            "{user}"
        );
    }
    let viewer = login(&app, "viewer").await;
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/cockpit/missions",
            mission(),
            Some(&viewer)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut wrong = session.clone();
    wrong["csrf"] = json!("invalid-csrf");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/cockpit/missions",
            mission(),
            Some(&wrong)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut invalid = mission();
    invalid["steps"][0]["step_index"] = json!(u64::MAX);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/cockpit/missions",
            invalid,
            Some(&session)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    sqlx::query("UPDATE archava_commerce.memberships SET active=false WHERE principal_id='alice'")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(
        call(&app, "GET", &read, Value::Null, Some(&session))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/cockpit/missions/{id}/steps/0/complete?tenant=acme"),
            report(),
            Some(&session)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(step_events(&f, &id).await, 0);
}
#[tokio::test]
async fn failed_auto_launch_and_abort_never_publish_uncommitted_status() {
    let (f, state, app, session) = setup().await;
    sqlx::raw_sql("CREATE FUNCTION archava_cockpit.reject_launch() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.status='in_flight' THEN RAISE EXCEPTION 'synthetic launch failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_launch BEFORE INSERT OR UPDATE ON archava_cockpit.missions FOR EACH ROW EXECUTE FUNCTION archava_cockpit.reject_launch();").execute(&f.admin_pool).await.unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/cockpit/missions",
            mission(),
            Some(&session)
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_cockpit.missions")
        .fetch_one(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    sqlx::raw_sql("DROP TRIGGER reject_launch ON archava_cockpit.missions")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    let id = create(&app, &session).await;
    sqlx::raw_sql("CREATE FUNCTION archava_cockpit.reject_abort() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.status='aborted' THEN RAISE EXCEPTION 'synthetic abort failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_abort BEFORE UPDATE ON archava_cockpit.missions FOR EACH ROW EXECUTE FUNCTION archava_cockpit.reject_abort();").execute(&f.admin_pool).await.unwrap();
    let path = format!("/api/v1/cockpit/missions/{id}/abort?tenant=acme");
    let body = json!({"reason":"Synthetic cancellation"});
    assert_eq!(
        call(&app, "POST", &path, body.clone(), Some(&session))
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(db_status(&f, &id).await.0, "in_flight");
    assert_eq!(
        state
            .cockpit()
            .get_mission(id.parse().unwrap())
            .unwrap()
            .status,
        archava_cockpit::FlightStatus::InFlight
    );
    sqlx::raw_sql("DROP TRIGGER reject_abort ON archava_cockpit.missions")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    assert_eq!(
        call(&app, "POST", &path, body.clone(), Some(&session))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "POST", &path, body, Some(&session)).await.0,
        StatusCode::OK
    );
    assert_eq!(db_status(&f, &id).await.0, "aborted");
}
