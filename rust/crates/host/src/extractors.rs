//! D09's R02 follow-up: the path, query and form extractors.
//!
//! The `router.rs` module tests cover the `Json` extractor, because `Json` was
//! where D09's reflection bug actually lived. Axum's `Path`, `Query` and `Form`
//! extractors have equivalent default rejections, and D09 names the consequence
//! explicitly: *"every extraction path needs the same treatment, not just `Json`.
//! `Path` is the dangerous one — it reflects a URL path segment, and the R02
//! routes are tenant-scoped by path."*
//!
//! `Query` is now genuinely wired: `GET /commands/status` takes its parameters
//! that way, and `router.rs` routes its rejection through `query_rejection()` —
//! the same fixed-message, logged-detail shape `Json` uses — with a negative
//! test (`a_malformed_status_query_is_refused_without_reflection`) that proves
//! the hostile parameter never comes back. The tripwire below therefore no
//! longer forbids `Query<`; it asserts the *shape* of its use instead, so a bare
//! `Query<` extractor — the reflection-prone spelling — still cannot land
//! unnoticed.
//!
//! `Path` and `Form` remain unwired, and the routes remain static, so that gap
//! is still closed **vacuously** rather than fixed — which is why the tripwire
//! for the person adding the first `/tenants/{tenant}/...` route stays.
//!
//! Two kinds of assertion live here, and they are deliberately different in kind:
//!
//! 1. Behavioral ones that run **today**: an unmatched path does not come back
//!    reflected; a malformed submission never echoes. These are the properties
//!    a caller can observe right now, on a pool that never connects.
//! 2. A structural one that reads the router's own source and is meant to change
//!    the moment someone adds a path or form extractor — at which point the
//!    rejection handler has to be written first. A test whose message tells you
//!    what to do when it fails is doing more work than one that just fails.
//!
//! The gate-level negatives — unknown action, wrong role, under-capability,
//! L5-without-approval, declined confirmation, forged tenant against a live
//! session — are **not** here: they now sit behind the membership lookup, which
//! needs PostgreSQL, so they moved with RA01 to `tests/authority.rs`, driven
//! against the real database through the real router. What stays here is what
//! the ordering still guarantees without one.
//!
//! It is its own module rather than a `mod tests` addition because
//! `router.rs` is past the maintainability ceiling, and because `include_str!`
//! makes this the one place in the crate that reads the router's own source —
//! which is a thing a unit test inside that file would find awkward to do.
//!
//! It lives in-crate (`#[cfg(test)]`) rather than in `tests/` because
//! `AppState::new` is `pub(crate)`: the only constructor an external
//! caller can reach is `AppState::try_new`, which refuses to build state on a
//! pool that has not passed startup readiness. These cases never touch the
//! database — they run on a pool that never connects, which is exactly
//! what `try_new` would refuse — so they run inside the crate, where the
//! test-only constructor exists for precisely that ordering proof.

use crate::identity::Identity;
use crate::{router, AppState, Ledger};
use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use axum::routing::Router;
use tower::ServiceExt;

/// The commerce ledger with every tenant-scoped table, `approvals` included.
fn ledger() -> Ledger {
    Ledger {
        namespace: "archava_commerce",
        ledger: "Commerce",
        tables: &[
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
        ],
    }
}

/// Builds a router on a pool that never connects.
///
/// Every case here is refused before the handler reaches the database, which is
/// what makes them runnable without one — the ordering is the property.
fn router_that_never_connects() -> Router {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://archava:archava@127.0.0.1:1/archava_test")
        .expect("a lazy pool must construct without connecting");
    let identity = Identity::unchecked(pool.clone(), &[7u8; 32]);
    router(AppState::new(pool, ledger(), identity))
}

/// Drives a request through the real router.
///
/// This pool never connects: every case below is refused before the handler
/// touches the database, so none is needed.
async fn call(uri: &str, body: Option<&str>) -> (StatusCode, String) {
    let router = router_that_never_connects();

    let mut builder = Request::builder()
        .method(if body.is_some() { "POST" } else { "GET" })
        .uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let request = builder.body(body.unwrap_or_default().to_string()).unwrap();

    let response = router.oneshot(request).await.expect("router must answer");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body must be readable");
    (status, String::from_utf8_lossy(&bytes).to_string())
}

/// A path the router does not declare must not be handed back.
///
/// Axum answers an unmatched path with its own 404, which says nothing about the
/// path that missed. If a future router implementation echoed the path into a
/// body — a "not found: /contacts/../etc/passwd" — it would be the same
/// reflection primitive D09 removed from the `Json` rejection, arriving through
/// the URL instead of the body, and it would be reachable unauthenticated.
#[tokio::test]
async fn an_unmatched_path_is_not_reflected() {
    for path in [
        "/contacts/../etc/passwd",
        "/tenants/acme-retail/contacts",
        "/contacts/%2e%2e%2fetc%2fpasswd",
        // Percent-encoded, because a raw `<` is not a valid URI character and
        // could never be sent — asserting on it would be asserting that a client
        // which cannot connect is not reflected.
        "/contacts/%3Cscript%3Ealert(1)%3C%2Fscript%3E",
    ] {
        let response = router_that_never_connects()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .body(String::new())
                    .expect("a test request must build"),
            )
            .await
            .expect("the router must answer");
        let status = response.status();
        let body =
            String::from_utf8_lossy(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .to_string();

        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{path} was answered {status}, expected the router's own 404"
        );
        for fragment in ["../etc/passwd", "acme-retail", "<script>", "%2e%2e"] {
            assert!(
                !body.contains(fragment),
                "{path} came back reflected ({fragment:?}) in: {body}"
            );
        }
    }
}

/// The next path or form extractor must not be wired until its rejection path
/// is written — and a `Query` extractor may not be wired *without* one.
///
/// This reads the router's own source at compile time. That is the point: it
/// cannot drift from what the router actually declares.
///
/// For `Path` and `Form` the rule is absolute (both spellings, bare and fully
/// qualified, because matching only one is how a grep-shaped test gets walked
/// past). For `Query` the extractor is now sanctioned — `GET /commands/status`
/// uses it — so the rule becomes structural: every `Query<` must be the `Result`
/// spelling that carries a `QueryRejection`, which is the only shape
/// `query_rejection()` can map. A bare `Query<` in a handler signature would
/// fall back to axum's reflective default, and this assertion is what says so.
#[test]
fn no_path_or_form_extractor_is_wired_without_a_refusal_handler() {
    const SOURCE: &str = include_str!("router.rs");

    let unwired = [
        ("Path<", "`axum::extract::Path`"),
        ("extract::Path", "`axum::extract::Path`, fully qualified"),
        ("Form<", "`axum::extract::Form`"),
        ("extract::Form", "`axum::extract::Form`, fully qualified"),
    ];

    for (needle, named) in unwired {
        assert!(
            !SOURCE.contains(needle),
            "{named} now appears in router.rs. D09's R02 follow-up is live: axum's \
             default rejection for this extractor reflects the caller's own input \
             back at it, and this host's routes are unauthenticated. Route the \
             rejection through the same `rejection()` shape the `Json` extractor \
             uses — fixed message, detail logged server-side, no part of the input \
             in the response — and delete this assertion only once that handler \
             has a negative test of its own."
        );
    }

    // `Query` may appear only as `Result<Query<…>, QueryRejection>` — never the
    // bare extractor, whose default rejection echoes the query string. Counting
    // the spellings is exact: any bare `Query<` makes the total exceed the
    // guarded ones, and the message names the handler to reach for.
    let all = SOURCE.matches("Query<").count();
    let guarded = SOURCE.matches("Result<Query<").count();
    assert_eq!(
        all, guarded,
        "a bare `Query<` extractor is wired in router.rs. Its default rejection \
         reflects the caller's own query string back at it. Type it as \
         `Result<Query<T>, QueryRejection>` and map the error through \
         `query_rejection()` — fixed message, detail logged server-side — the \
         way `GET /commands/status` does."
    );
}

// --- Extraction ordering without a database ------------------------------------
//
// With RA01 the authority chain runs extraction → session → membership → gate.
// The session and membership steps need PostgreSQL; extraction does not. So
// what remains provable here is the *front* of the chain: hostile input dies at
// deserialization, and no refusal at either cold stage echoes it back.

/// An invalid tenant is refused during extraction, before any handler code
/// runs — and before the session check, which is the ordering that keeps a
/// malformed body from reaching identity work at all.
#[tokio::test]
async fn an_invalid_tenant_is_refused_at_the_entrypoint() {
    let (status, _) = call(
        "/commands",
        Some(&submission("../etc/passwd", "product.read")),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a tenant that cannot be represented must be refused at extraction, \
         not handed to a handler"
    );
}

/// A submission carrying the pre-RA01 authority fields — role, capability,
/// human_approved — must parse (they are unknown fields, dropped silently) and
/// then be refused for having no session. It must not 422: a 422 here would
/// mean the body was still being read as an authority claim, and an accepted
/// one would mean worse.
#[tokio::test]
async fn legacy_authority_fields_are_dropped_not_deserialized() {
    let body = serde_json::json!({
        "tenant": "acme-retail",
        "role": "owner",
        "capability": "enterprise",
        "action_id": "admin.config.update",
        "request_id": "7d9f4c2e-1b3a-4c7d-9e8f-0a1b2c3d4e5f",
        "revision": 1,
        "human_approved": true,
    })
    .to_string();
    let (status, response) = call("/commands", Some(&body)).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "unknown fields must be dropped rather than honoured or rejected: {response}"
    );
}

/// A refusal at either cold stage never names the tenant.
///
/// The 422 case is the hostile-tenant body; the 401 case is everything else —
/// with no session, the tenant is never consulted, so echoing it would be a
/// disclosure of nothing the caller just sent. The 403 case (a real session
/// against a tenant it has no membership in) needs PostgreSQL and lives in
/// `tests/authority.rs`; it asserts the same property where it first becomes
/// observable.
#[tokio::test]
async fn a_boundary_refusal_never_names_the_tenant() {
    for body in [
        submission("../etc/passwd", "product.read"),
        submission("acme-retail", "product.read"),
        submission("acme-retail", "admin.config.update"),
    ] {
        let (status, response) = call("/commands", Some(&body)).await;
        assert!(
            status == StatusCode::UNAUTHORIZED || status == StatusCode::UNPROCESSABLE_ENTITY,
            "expected a cold refusal, got {status}: {response}"
        );
        assert!(
            !response.contains("tenant"),
            "a refusal must not name the tenant field: {response}"
        );
    }
}

/// A JSON body that is not a submission at all comes back as the fixed
/// `malformed_submission`, not as axum's parse error — which quotes the
/// offending bytes verbatim, including whatever secrets a caller might have
/// pasted into a malformed request.
#[tokio::test]
async fn an_unparseable_body_is_not_reflected() {
    let (status, body) = call("/commands", Some("{\"tenant\": ")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "got: {body}");
    assert!(
        !body.contains("tenant"),
        "the parse refusal named the field: {body}"
    );
    assert!(
        body.contains("malformed_submission"),
        "the caller needs a stable reason, got: {body}"
    );
}

/// Builds a fully valid JSON command submission body.
///
/// `request_id` and `revision` are present because they are required fields:
/// without them the body would 422 on a missing key and never reach the stage
/// under test. The `role` and `capability` keys are present on purpose — they
/// are the fields the pre-RA01 body used to carry as authority, so every test
/// using this helper is also asserting they are dropped rather than read.
fn submission(tenant: &str, action_id: &str) -> String {
    serde_json::json!({
        "tenant": tenant,
        "action_id": action_id,
        "request_id": "7d9f4c2e-1b3a-4c7d-9e8f-0a1b2c3d4e5f",
        "revision": 1,
        "role": "owner",
        "capability": "enterprise",
    })
    .to_string()
}
