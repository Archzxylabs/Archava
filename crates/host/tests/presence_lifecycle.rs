//! Real router + disposable Postgres regression tests for presence voice lifecycle.

use archava_host::{router, AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

const KEY: &[u8] = &[9u8; 32];
const PASSWORD: &str = "correct horse battery staple";
const HASH: &str =
    "scrypt$32768$8$3$OZtInKxSaSZqbEptj2fvUg$jU_gx2rQW-69EYpbRoMKGSgclhV-mO7gXVF-QYvSXVo";

fn ledger() -> Ledger {
    Ledger {
        namespace: "archava_commerce",
        ledger: "Commerce",
        tables: &["memberships"],
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
        ("mallory", "other", "enterprise", "all"),
    ] {
        sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES($1,$2,$3,true,1)")
            .bind(id)
            .bind(format!("{id}@archava.test"))
            .bind(HASH)
            .execute(&f.admin_pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope,active,role,capability) VALUES($1,$2,'human','{}',$3,true,'owner',$4)")
            .bind(tenant)
            .bind(id)
            .bind(scope)
            .bind(capability)
            .execute(&f.admin_pool)
            .await
            .unwrap();
    }

    // A real commerce order lets the checkout-signing regression prove that
    // authorization is bound to persisted quote data, not just membership.
    let product_id = Uuid::new_v4();
    let order_id = Uuid::parse_str("00000000-0000-4000-8000-000000000042").unwrap();
    let line_id = Uuid::new_v4();
    let mut tx = f.admin_pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('archava.tenant_id', 'acme', true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO archava_commerce.products
         (tenant_id,id,sku,title,unit,track_stock,price,currency)
         VALUES ('acme',$1,'CHECKOUT-TEST','Checkout test product','unit',false,100,'IDR')",
    )
    .bind(product_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO archava_commerce.orders
         (tenant_id,id,owner_id,customer_name,total_estimate,currency)
         VALUES ('acme',$1,'alice','Checkout customer',100,'IDR')",
    )
    .bind(order_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO archava_commerce.order_lines
         (tenant_id,id,order_id,product_id,sku,title,unit,track_stock,quantity,price,product_revision)
         VALUES ('acme',$1,$2,$3,'CHECKOUT-TEST','Checkout test product','unit',false,1,100,1)",
    )
    .bind(line_id)
    .bind(order_id)
    .bind(product_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    sqlx::query(
        "UPDATE archava_commerce.memberships
         SET permissions = ARRAY['orders.read']::text[]
         WHERE tenant_id = 'acme' AND principal_id = 'alice'",
    )
    .execute(&f.admin_pool)
    .await
    .unwrap();

    let state = state(&f, f.runtime_pool.clone())
        .await
        .with_presence_registry(archava_host::presence::PresenceRegistry::for_loopback());
    let app = router(state.clone());
    let session = login(&app, "alice").await;
    (f, state, app, session)
}

async fn login(app: &Router, user: &str) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"username": format!("{user}@archava.test"), "password": PASSWORD})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    let mut payload: Value = serde_json::from_slice(&body).unwrap();
    payload["cookie"] = json!(cookie);
    payload
}

#[tokio::test]
async fn test_presence_session_lifecycle_and_turn_and_degrade() {
    let (_f, _state, app, session) = setup().await;
    let cookie = session["cookie"].as_str().unwrap();
    let csrf = session["csrf"].as_str().unwrap();

    // 0. Mutating request without CSRF must be refused with FORBIDDEN
    let no_csrf_req = Request::builder()
        .method("POST")
        .uri("/presence/sessions")
        .header("cookie", cookie)
        .header("content-type", "application/json")
        .body(Body::from(json!({ "mode": "voice" }).to_string()))
        .unwrap();
    let no_csrf_res = app.clone().oneshot(no_csrf_req).await.unwrap();
    assert_eq!(no_csrf_res.status(), StatusCode::FORBIDDEN);

    // 1. Start presence session with valid CSRF
    let start_req = Request::builder()
        .method("POST")
        .uri("/presence/sessions")
        .header("cookie", cookie)
        .header("x-archava-csrf", csrf)
        .header("content-type", "application/json")
        .body(Body::from(json!({ "mode": "voice" }).to_string()))
        .unwrap();

    let start_res = app.clone().oneshot(start_req).await.unwrap();
    let start_status = start_res.status();
    let start_bytes = to_bytes(start_res.into_body(), 16 * 1024).await.unwrap();
    let start_text = String::from_utf8_lossy(&start_bytes);
    assert_eq!(
        start_status,
        StatusCode::CREATED,
        "Failed start: {start_text}"
    );
    let start_json: Value = serde_json::from_str(&start_text).unwrap();

    let session_id = start_json["session"]["session_id"].as_str().unwrap();
    let token = start_json["token"].as_str().unwrap();
    assert_eq!(start_json["session"]["current_mode"], "voice");
    assert!(!token.is_empty());

    // 2. Query active session (GET does not require CSRF)
    let get_req = Request::builder()
        .method("GET")
        .uri(format!("/presence/sessions/{session_id}"))
        .header("cookie", cookie)
        .body(Body::empty())
        .unwrap();
    let get_res = app.clone().oneshot(get_req).await.unwrap();
    assert_eq!(get_res.status(), StatusCode::OK);

    // 3. Process transcript turn with valid CSRF
    let turn_req = Request::builder()
        .method("POST")
        .uri(format!("/presence/sessions/{session_id}/turn"))
        .header("cookie", cookie)
        .header("x-archava-csrf", csrf)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "frame_id": "f-1",
                "sequence_number": 1,
                "speaker": "user",
                "text": "Berapa total revenue bulan ini?",
                "is_final": true
            })
            .to_string(),
        ))
        .unwrap();

    let turn_res = app.clone().oneshot(turn_req).await.unwrap();
    assert_eq!(turn_res.status(), StatusCode::OK);
    let turn_bytes = to_bytes(turn_res.into_body(), 16 * 1024).await.unwrap();
    let turn_json: Value = serde_json::from_slice(&turn_bytes).unwrap();

    assert_eq!(turn_json["sequence_number"], 2);
    assert_eq!(turn_json["speaker"], "ava");
    assert!(turn_json["text"]
        .as_str()
        .unwrap()
        .contains("Berapa total revenue bulan ini?"));

    // 4. Degrade to Chat mode with valid CSRF
    let degrade_req = Request::builder()
        .method("POST")
        .uri(format!("/presence/sessions/{session_id}/degrade"))
        .header("cookie", cookie)
        .header("x-archava-csrf", csrf)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "reason": "user muted audio" }).to_string(),
        ))
        .unwrap();

    let degrade_res = app.clone().oneshot(degrade_req).await.unwrap();
    assert_eq!(degrade_res.status(), StatusCode::OK);
    let degrade_bytes = to_bytes(degrade_res.into_body(), 16 * 1024).await.unwrap();
    let degrade_json: Value = serde_json::from_slice(&degrade_bytes).unwrap();

    assert_eq!(degrade_json["current_mode"], "chat");
    assert_eq!(degrade_json["degraded"], true);

    // 5. The participant can explicitly close the session and release its
    // capacity slot; the route is a CSRF-protected mutation.
    let close_req = Request::builder()
        .method("DELETE")
        .uri(format!("/presence/sessions/{session_id}"))
        .header("cookie", cookie)
        .header("x-archava-csrf", csrf)
        .body(Body::empty())
        .unwrap();
    let close_res = app.clone().oneshot(close_req).await.unwrap();
    assert_eq!(close_res.status(), StatusCode::OK);

    let after_close_req = Request::builder()
        .method("GET")
        .uri(format!("/presence/sessions/{session_id}"))
        .header("cookie", cookie)
        .body(Body::empty())
        .unwrap();
    let after_close_res = app.clone().oneshot(after_close_req).await.unwrap();
    assert_eq!(after_close_res.status(), StatusCode::NOT_FOUND);

    // 6. Unauthenticated request refused
    let unauth_req = Request::builder()
        .method("GET")
        .uri(format!("/presence/sessions/{session_id}"))
        .body(Body::empty())
        .unwrap();
    let unauth_res = app.clone().oneshot(unauth_req).await.unwrap();
    assert_eq!(unauth_res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn review_gemini_foreign_tenant_session_override_is_refused() {
    let (_f, _state, app, alice) = setup().await;
    let mallory = login(&app, "mallory").await;

    let start = |cookie: String, csrf: String, tenant: &str, session_id: &str| {
        Request::builder()
            .method("POST")
            .uri("/presence/sessions")
            .header("cookie", cookie)
            .header("x-archava-csrf", csrf)
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "tenant_id": tenant,
                    "session_id": session_id,
                    "mode": "voice"
                })
                .to_string(),
            ))
            .unwrap()
    };

    let alice_cookie = alice["cookie"].as_str().unwrap().to_string();
    let alice_csrf = alice["csrf"].as_str().unwrap().to_string();
    let mallory_cookie = mallory["cookie"].as_str().unwrap().to_string();
    let mallory_csrf = mallory["csrf"].as_str().unwrap().to_string();

    // Alice registers session under acme
    let first = app
        .clone()
        .oneshot(start(
            alice_cookie.clone(),
            alice_csrf.clone(),
            "acme",
            "review-victim-session",
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::CREATED);

    // Mallory (tenant 'other') tries to claim tenant 'acme' -> refused FORBIDDEN
    let takeover_foreign_tenant = app
        .clone()
        .oneshot(start(
            mallory_cookie.clone(),
            mallory_csrf.clone(),
            "acme",
            "review-victim-session",
        ))
        .await
        .unwrap();
    assert_eq!(
        takeover_foreign_tenant.status(),
        StatusCode::FORBIDDEN,
        "Mallory cannot create or replace session for a foreign tenant"
    );

    // Mallory tries to claim the same session ID under her own tenant 'other' -> collision CONFLICT
    let takeover_collision = app
        .clone()
        .oneshot(start(
            mallory_cookie.clone(),
            mallory_csrf.clone(),
            "other",
            "review-victim-session",
        ))
        .await
        .unwrap();
    assert_eq!(
        takeover_collision.status(),
        StatusCode::CONFLICT,
        "Session collision must be refused across all active sessions"
    );

    let get = |cookie: String| {
        Request::builder()
            .method("GET")
            .uri("/presence/sessions/review-victim-session")
            .header("cookie", cookie)
            .body(Body::empty())
            .unwrap()
    };

    // Original owner Alice can read
    let alice_read = app
        .clone()
        .oneshot(get(alice_cookie.clone()))
        .await
        .unwrap()
        .status();
    assert_eq!(alice_read, StatusCode::OK);

    // Foreign reader Mallory is refused
    let mallory_read = app
        .clone()
        .oneshot(get(mallory_cookie.clone()))
        .await
        .unwrap()
        .status();
    assert_eq!(mallory_read, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn checkout_signature_mint_requires_membership_in_the_signed_tenant() {
    let (_f, _state, app, alice) = setup().await;
    let mallory = login(&app, "mallory").await;

    let sign = |session: &Value, tenant: &str| {
        Request::builder()
            .method("POST")
            .uri("/embed/checkout/sign")
            .header("cookie", session["cookie"].as_str().unwrap())
            .header("x-archava-csrf", session["csrf"].as_str().unwrap())
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "tenant": tenant,
                    "orderId": "00000000-0000-4000-8000-000000000042",
                    "amount": 100,
                    "currency": "IDR",
                    "item": "Signed checkout"
                })
                .to_string(),
            ))
            .unwrap()
    };

    let allowed = app.clone().oneshot(sign(&alice, "acme")).await.unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);

    let mut mismatched = sign(&alice, "acme");
    *mismatched.body_mut() = Body::from(
        json!({
            "tenant": "acme",
            "orderId": "00000000-0000-4000-8000-000000000042",
            "amount": 999,
            "currency": "IDR",
            "item": "Signed checkout"
        })
        .to_string(),
    );
    let mismatch = app.clone().oneshot(mismatched).await.unwrap();
    assert_eq!(mismatch.status(), StatusCode::CONFLICT);

    let missing = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/embed/checkout/sign")
                .header("cookie", alice["cookie"].as_str().unwrap())
                .header("x-archava-csrf", alice["csrf"].as_str().unwrap())
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "tenant": "acme",
                        "orderId": "00000000-0000-4000-8000-000000000099",
                        "amount": 100,
                        "currency": "IDR"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let refused = app.clone().oneshot(sign(&mallory, "acme")).await.unwrap();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn review_revocation_stops_subsequent_presence_operations() {
    let (f, _state, app, session) = setup().await;
    let cookie = session["cookie"].as_str().unwrap().to_string();
    let csrf = session["csrf"].as_str().unwrap().to_string();

    // 1. Alice starts session
    let start_req = Request::builder()
        .method("POST")
        .uri("/presence/sessions")
        .header("cookie", &cookie)
        .header("x-archava-csrf", &csrf)
        .header("content-type", "application/json")
        .body(Body::from(json!({ "mode": "voice" }).to_string()))
        .unwrap();

    let start_res = app.clone().oneshot(start_req).await.unwrap();
    assert_eq!(start_res.status(), StatusCode::CREATED);
    let start_bytes = to_bytes(start_res.into_body(), 16 * 1024).await.unwrap();
    let start_json: Value = serde_json::from_slice(&start_bytes).unwrap();
    let session_id = start_json["session"]["session_id"].as_str().unwrap();

    // 2. Revoke Alice's membership
    sqlx::query(
        "UPDATE archava_commerce.memberships SET active=false WHERE principal_id='alice' AND tenant_id='acme'",
    )
    .execute(&f.admin_pool)
    .await
    .unwrap();

    // 3. Subsequent GET must be refused with FORBIDDEN
    let get_req = Request::builder()
        .method("GET")
        .uri(format!("/presence/sessions/{session_id}"))
        .header("cookie", &cookie)
        .body(Body::empty())
        .unwrap();
    let get_res = app.clone().oneshot(get_req).await.unwrap();
    assert_eq!(
        get_res.status(),
        StatusCode::FORBIDDEN,
        "Revoked member cannot read session"
    );

    // 4. Subsequent turn must be refused with FORBIDDEN
    let turn_req = Request::builder()
        .method("POST")
        .uri(format!("/presence/sessions/{session_id}/turn"))
        .header("cookie", &cookie)
        .header("x-archava-csrf", &csrf)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "frame_id": "f-1",
                "sequence_number": 1,
                "speaker": "user",
                "text": "Berapa total revenue?",
                "is_final": true
            })
            .to_string(),
        ))
        .unwrap();
    let turn_res = app.clone().oneshot(turn_req).await.unwrap();
    assert_eq!(
        turn_res.status(),
        StatusCode::FORBIDDEN,
        "Revoked member cannot execute turn"
    );

    // 5. Subsequent degrade must be refused with FORBIDDEN
    let degrade_req = Request::builder()
        .method("POST")
        .uri(format!("/presence/sessions/{session_id}/degrade"))
        .header("cookie", &cookie)
        .header("x-archava-csrf", &csrf)
        .header("content-type", "application/json")
        .body(Body::from(json!({ "reason": "offline" }).to_string()))
        .unwrap();
    let degrade_res = app.clone().oneshot(degrade_req).await.unwrap();
    assert_eq!(
        degrade_res.status(),
        StatusCode::FORBIDDEN,
        "Revoked member cannot degrade session"
    );
}

#[tokio::test]
async fn test_presence_unconfigured_default_returns_service_unavailable() {
    let f = provision().await;
    sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES($1,$2,$3,true,1)")
        .bind("alice")
        .bind("alice@archava.test")
        .bind(HASH)
        .execute(&f.admin_pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope,active,role,capability) VALUES($1,$2,'human','{}','all',true,'owner','enterprise')")
        .bind("acme")
        .bind("alice")
        .execute(&f.admin_pool)
        .await
        .unwrap();

    // Default state has PresenceRegistry::default() with no voice provider configured
    let state = state(&f, f.runtime_pool.clone()).await;
    let app = router(state.clone());
    let session = login(&app, "alice").await;

    let cookie = session["cookie"].as_str().unwrap().to_string();
    let csrf = session["csrf"].as_str().unwrap().to_string();

    let start_req = Request::builder()
        .method("POST")
        .uri("/presence/sessions")
        .header("cookie", &cookie)
        .header("x-archava-csrf", &csrf)
        .header("content-type", "application/json")
        .body(Body::from(json!({ "tenant_id": "acme" }).to_string()))
        .unwrap();
    let res = app.clone().oneshot(start_req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "Unconfigured voice provider must return 503 Service Unavailable"
    );
    let bytes = to_bytes(res.into_body(), 1024 * 64).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"], "presence_unconfigured");
}

#[tokio::test]
async fn test_presence_session_id_validation_and_room_collision_prevention() {
    let (_f, _state, app, session) = setup().await;
    let cookie = session["cookie"].as_str().unwrap();
    let csrf = session["csrf"].as_str().unwrap();

    // 1. Invalid session_id characters (spaces, semicolons, path traversal) refused with 400 Bad Request
    for bad_id in ["invalid id", "semi;colon", "../../traversal", ""] {
        let req = Request::builder()
            .method("POST")
            .uri("/presence/sessions")
            .header("cookie", cookie)
            .header("x-archava-csrf", csrf)
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "tenant_id": "acme",
                    "session_id": bad_id
                })
                .to_string(),
            ))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::BAD_REQUEST,
            "session_id '{bad_id}' must be rejected with 400 Bad Request"
        );
    }

    // 2. Overlong session_id (>64 chars) refused with 400 Bad Request
    let overlong_id = "a".repeat(65);
    let req = Request::builder()
        .method("POST")
        .uri("/presence/sessions")
        .header("cookie", cookie)
        .header("x-archava-csrf", csrf)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tenant_id": "acme",
                "session_id": overlong_id
            })
            .to_string(),
        ))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::BAD_REQUEST,
        "Overlong session_id must be rejected with 400 Bad Request"
    );

    // 3. Valid session_id with allowed chars (alphanumeric, dash, underscore) succeeds
    let req = Request::builder()
        .method("POST")
        .uri("/presence/sessions")
        .header("cookie", cookie)
        .header("x-archava-csrf", csrf)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tenant_id": "acme",
                "session_id": "valid-session_123"
            })
            .to_string(),
        ))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let bytes = to_bytes(res.into_body(), 16 * 1024).await.unwrap();
    let val: Value = serde_json::from_slice(&bytes).unwrap();
    let room_name = val["session"]["room_name"].as_str().unwrap();
    assert!(room_name.starts_with("archava-acme-"));
}
