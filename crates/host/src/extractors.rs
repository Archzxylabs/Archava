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
//! `Path` and `Form` are no longer unwired: `cockpit.rs` takes typed path ids
//! and `feed.rs` takes a profile slug. So the rule is no longer "forbidden" but
//! "guarded" — every extractor must be the `Result<…, Rejection>` spelling,
//! because axum's default rejection for `Path`, `Query` and `Json` can name the
//! caller's own input. The tripwire below covers the whole crate, records the
//! modules that still carry bare extractors, and fails when that debt grows —
//! or when it shrinks without the row being deleted.
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

/// Every module in this crate, as source text.
///
/// `check-structure.mjs` fails when a `crates/host/src/**/*.rs` file is missing
/// from this list, so a new module cannot escape the rules below by not being
/// named here. `extractors.rs` is excluded because it is this file.
const MODULES: &[(&str, &str)] = &[
    ("agent/bindings.rs", include_str!("agent/bindings.rs")),
    ("agent/delegation.rs", include_str!("agent/delegation.rs")),
    ("agent/disclosure.rs", include_str!("agent/disclosure.rs")),
    (
        "agent/disclosure/fields.rs",
        include_str!("agent/disclosure/fields.rs"),
    ),
    (
        "agent/disclosure/tests.rs",
        include_str!("agent/disclosure/tests.rs"),
    ),
    ("agent/mod.rs", include_str!("agent/mod.rs")),
    ("agent/lookups.rs", include_str!("agent/lookups.rs")),
    ("erp/accounting.rs", include_str!("erp/accounting.rs")),
    ("agent/planner.rs", include_str!("agent/planner.rs")),
    ("agent/read.rs", include_str!("agent/read.rs")),
    ("agent/runner.rs", include_str!("agent/runner.rs")),
    ("agent/schema.rs", include_str!("agent/schema.rs")),
    ("assistant/bridge.rs", include_str!("assistant/bridge.rs")),
    ("assistant/mod.rs", include_str!("assistant/mod.rs")),
    (
        "assistant/sql_host.rs",
        include_str!("assistant/sql_host.rs"),
    ),
    ("authority.rs", include_str!("authority.rs")),
    ("business.rs", include_str!("business.rs")),
    ("workflows.rs", include_str!("workflows.rs")),
    ("feed/actions.rs", include_str!("feed/actions.rs")),
    ("ingestion/commit.rs", include_str!("ingestion/commit.rs")),
    ("erp/projections.rs", include_str!("erp/projections.rs")),
    ("cockpit.rs", include_str!("cockpit.rs")),
    ("erp/crm.rs", include_str!("erp/crm.rs")),
    ("decision.rs", include_str!("decision.rs")),
    ("feed.rs", include_str!("feed.rs")),
    ("identity.rs", include_str!("identity.rs")),
    ("ingestion.rs", include_str!("ingestion.rs")),
    ("lib.rs", include_str!("lib.rs")),
    ("mail.rs", include_str!("mail.rs")),
    ("metrics.rs", include_str!("metrics.rs")),
    ("orbz.rs", include_str!("orbz.rs")),
    ("own.rs", include_str!("own.rs")),
    ("own/disclosure.rs", include_str!("own/disclosure.rs")),
    ("payments.rs", include_str!("payments.rs")),
    ("presence.rs", include_str!("presence.rs")),
    ("presence/media.rs", include_str!("presence/media.rs")),
    ("preview.rs", include_str!("preview.rs")),
    ("router.rs", include_str!("router.rs")),
    ("security.rs", include_str!("security.rs")),
    ("webhooks.rs", include_str!("webhooks.rs")),
    ("erp/accommodation.rs", include_str!("erp/accommodation.rs")),
    ("erp/assets.rs", include_str!("erp/assets.rs")),
    ("erp/attendance.rs", include_str!("erp/attendance.rs")),
    ("erp/billing.rs", include_str!("erp/billing.rs")),
    ("erp/commerce.rs", include_str!("erp/commerce.rs")),
    (
        "erp/field_services.rs",
        include_str!("erp/field_services.rs"),
    ),
    ("erp/foodservice.rs", include_str!("erp/foodservice.rs")),
    (
        "erp/formal_education.rs",
        include_str!("erp/formal_education.rs"),
    ),
    ("erp/fulfillment.rs", include_str!("erp/fulfillment.rs")),
    ("erp/laundry.rs", include_str!("erp/laundry.rs")),
    ("erp/mod.rs", include_str!("erp/mod.rs")),
    ("erp/organizations.rs", include_str!("erp/organizations.rs")),
    ("erp/parties.rs", include_str!("erp/parties.rs")),
    ("erp/companies.rs", include_str!("erp/companies.rs")),
    ("erp/projects.rs", include_str!("erp/projects.rs")),
    ("erp/proposals.rs", include_str!("erp/proposals.rs")),
    ("erp/purchasing.rs", include_str!("erp/purchasing.rs")),
    ("erp/scheduling.rs", include_str!("erp/scheduling.rs")),
    ("erp/subscriptions.rs", include_str!("erp/subscriptions.rs")),
    ("erp/support.rs", include_str!("erp/support.rs")),
    ("erp/workspace.rs", include_str!("erp/workspace.rs")),
    ("routes/auth.rs", include_str!("routes/auth.rs")),
    ("routes/cockpit.rs", include_str!("routes/cockpit.rs")),
    ("routes/commands.rs", include_str!("routes/commands.rs")),
    ("routes/crm.rs", include_str!("routes/crm.rs")),
    ("routes/erp.rs", include_str!("routes/erp.rs")),
    ("routes/mod.rs", include_str!("routes/mod.rs")),
    ("routes/system.rs", include_str!("routes/system.rs")),
    ("routes/webhooks.rs", include_str!("routes/webhooks.rs")),
];

/// Modules that still wire a bare — reflection-prone — extractor, and how many
/// of each spelling they carry: `(module, Path, Query, Json)`.
///
/// The recorded numbers must match exactly in both directions. A module that
/// grows a bare extractor fails this test, and a module that reaches zero fails
/// until its row is deleted. That is the ratchet: no new bare extractor can
/// land anywhere in this crate, and the remaining debt is enumerated rather than
/// implied. The modules listed here are the extraction backlog, in the order a
/// security pass should take them.
const EXTRACTOR_DEBT: &[(&str, usize, usize, usize)] = &[];

/// Counts the bare and guarded spellings of the three extractors that reflect.
///
/// A guarded occurrence (`Result<Query<…>, QueryRejection>`) never matches the
/// bare pattern, because the colon is followed by `Result`, so the counts are
/// disjoint and exact.
fn extractor_counts(source: &str) -> (usize, usize, usize) {
    (
        source.matches(": Path<").count(),
        source.matches(": Query<").count(),
        source.matches(": Json<").count(),
    )
}

/// No module may wire an extractor whose default rejection can name the
/// caller's own input, except the recorded debt — which can only shrink.
///
/// This reads every module's source at compile time, so it cannot drift from
/// what the crate actually declares. `Form` stays absolutely forbidden: no
/// module takes a form body, and a shipping handler would need its rejection
/// path written first.
#[test]
fn no_new_bare_extractor_is_wired_anywhere_in_the_crate() {
    for (name, source) in MODULES {
        assert!(
            !source.contains(": Form<") && !source.contains("Form("),
            "{name} wires axum's `Form` extractor. Its default rejection reflects \
             the caller's submission back at it. Type it as \
             `Result<Form<T>, FormRejection>` and map the error to a fixed \
             message and status first."
        );

        let counted = extractor_counts(source);
        let expected = EXTRACTOR_DEBT
            .iter()
            .find(|(module, ..)| module == name)
            .map(|(_, path, query, json)| (*path, *query, *json))
            .unwrap_or((0, 0, 0));
        assert_eq!(
            counted, expected,
            "{name} changed its bare-extractor count. Expected \
             (Path, Query, Json) = {expected:?}, found {counted:?}. A new bare \
             extractor must be written as `Result<Extractor<T>, Rejection>` with \
             a fixed refusal instead. A module that reached (0, 0, 0) must have \
             its `EXTRACTOR_DEBT` row deleted, so the debt cannot outlive the \
             fix."
        );
    }

    for (module, ..) in EXTRACTOR_DEBT {
        assert!(
            MODULES.iter().any(|(name, _)| name == module),
            "{module} is recorded as extractor debt but is not in MODULES, so the \
             debt row is not measuring anything."
        );
    }
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
