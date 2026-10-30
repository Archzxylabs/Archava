use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;
pub const PASSWORD: &str = "correct horse battery staple";
pub const HASH: &str =
    "scrypt$32768$8$3$OZtInKxSaSZqbEptj2fvUg$jU_gx2rQW-69EYpbRoMKGSgclhV-mO7gXVF-QYvSXVo";
pub async fn call(
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
pub async fn login(app: &Router, user: &str) -> Value {
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
