//! C02 — the logout contract, proven against real HTTP and real PostgreSQL.
//!
//! ## Why this suite exists
//!
//! The 19 Oktober 2026 CRM audit (finding C02) reproduced a signed-out
//! browser that still read twenty authorized contact rows: the sign-out
//! action cleared UI state only, the credential kept working, and the router
//! had no logout route to revoke anything. Clearing a page is not closing a
//! session.
//!
//! Every assertion here is a fact about what the SERVER did:
//!
//! - `POST /logout` revokes the session ROW — the authority — and only then
//!   expires the cookie, so a browser cannot look signed out while the
//!   credential still answers;
//! - after the revocation the old credential cannot read contacts, cannot
//!   mutate, and cannot recover a receipt: three different routes, one
//!   revoked session;
//! - a missing or forged CSRF is refused BEFORE anything is revoked, so a
//!   forged cross-site request cannot close somebody else's session, and the
//!   session survives the refusal (the audit's C02 ask: the CSRF negative is
//!   tested WITH a valid session, not with a missing one);
//! - another live session of the same user is untouched: logout revokes the
//!   presented token and nothing else;
//! - a restart does not resurrect a revoked session: the row is gone, so a
//!   freshly built state still refuses the old token;
//! - logging out with no session is `unauthenticated` — not an empty
//!   dataset and not a success that closed nothing.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use archava_host::{AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::routing::Router;
use tower::ServiceExt;

/// The commerce ledger this host serves alongside CRM — the same table list
/// `crm.rs` uses; the readiness gate checks every name.
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

/// A 32-byte identity HMAC key — the minimum `Identity::open` accepts.
const IDENTITY_KEY: &[u8] = &[11u8; 32];

const ACME: &str = "acme-retail";
const PRINCIPAL: &str = "alice";
const PASSWORD: &str = "correct horse battery staple";
const PASSWORD_HASH: &str =
    "scrypt$32768$8$3$OZtInKxSaSZqbEptj2fvUg$jU_gx2rQW-69EYpbRoMKGSgclhV-mO7gXVF-QYvSXVo";

fn commerce_ledger() -> Ledger {
    Ledger {
        namespace: "archava_commerce",
        ledger: "Commerce",
        tables: TABLES,
    }
}

/// Serving state that has passed both readiness gates.
async fn state(fixture: &Fixture) -> AppState {
    AppState::try_new_crm(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
        &fixture.crm_ledger,
    )
    .await
    .expect("a ready fixture must construct serving state")
}

/// One human principal with the CRM grants this suite exercises, one tenant
/// membership and one contact to read.
async fn seed(fixture: &Fixture) {
    sqlx::query(
        "INSERT INTO archava_identity.users (id, username, password_hash, active, version) \
         VALUES ($1, $2, $3, true, 1)",
    )
    .bind(PRINCIPAL)
    .bind(format!("{PRINCIPAL}@archava.test"))
    .bind(PASSWORD_HASH)
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the identity user");

    sqlx::query(
        "INSERT INTO archava_crm.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active) \
         VALUES ($1, $2, 'human', $3, 'all', true)",
    )
    .bind(ACME)
    .bind(PRINCIPAL)
    .bind(vec![
        "contacts.read",
        "contacts.write",
        "followups.read",
        "followups.write",
        "activities.read",
        "activities.write",
    ])
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the CRM membership");

    sqlx::query(
        "INSERT INTO archava_crm.contacts \
         (tenant_id, id, full_name, email, organization, owner_id, revision) \
         VALUES ($1, $2, 'Ada Lovelace', 'ada@lovelace.test', 'Analytical Engines', $3, 1)",
    )
    .bind(ACME)
    .bind(uuid::Uuid::new_v4())
    .bind(PRINCIPAL)
    .execute(&fixture.admin_pool)
    .await
    .expect("seed one contact");
}

/// A logged-in session: the token and the CSRF value the login body carries.
struct Session {
    token: String,
    csrf: String,
}

/// The login slots: the shipped KDF budget allows two concurrent scrypt
/// derivations, and the host answers `503 busy` beyond that. The suite
/// serialises its logins through the SAME number of slots the production
/// flood gets (RN04/RN07) — it does not raise the limit to make the tests
/// easier.
static LOGIN_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

async fn login(app: &Router) -> Session {
    let _slot = LOGIN_SLOTS.acquire().await.expect("login slots are open");
    let body = serde_json::json!({
        "username": format!("{PRINCIPAL}@archava.test"),
        "password": PASSWORD,
    })
    .to_string();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header("content-type", "application/json")
                .header("x-archava-client", "cli")
                .body(Body::from(body))
                .expect("build the login request"),
        )
        .await
        .expect("the login route must answer");
    assert_eq!(response.status(), StatusCode::OK, "login must succeed");
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read the login response");
    let outcome: serde_json::Value =
        serde_json::from_slice(&bytes).expect("the login response is JSON");
    Session {
        token: outcome["token"]
            .as_str()
            .expect("the login response carries a token")
            .to_string(),
        csrf: outcome["csrf"]
            .as_str()
            .expect("the login response carries a CSRF value")
            .to_string(),
    }
}

/// `POST /login` as if it arrived from `peer`.
///
/// The served router reads the peer out of `ConnectInfo`, which axum installs
/// from `into_make_service_with_connect_info`. A `oneshot` request carries no
/// socket, so the test installs the same extension the transport layer would —
/// the route under test is the real one, only the transport is substituted.
async fn login_from(
    app: &Router,
    peer: IpAddr,
    username: &str,
    password: &str,
) -> (StatusCode, serde_json::Value) {
    let _slot = LOGIN_SLOTS.acquire().await.expect("login slots are open");
    let body = serde_json::json!({ "username": username, "password": password }).to_string();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header("content-type", "application/json")
                .header("x-archava-client", "cli")
                .extension(ConnectInfo(SocketAddr::from((peer, 40_000))))
                .body(Body::from(body))
                .expect("build the login request"),
        )
        .await
        .expect("the login route must answer");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read the login response");
    let outcome = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, outcome)
}

/// Every login bucket row, as the limiter sees it.
async fn login_buckets(fixture: &Fixture) -> Vec<(String, i32)> {
    sqlx::query_as::<_, (String, i32)>(
        "SELECT key, attempts FROM archava_identity.login_buckets ORDER BY key",
    )
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("read the login buckets")
}

/// A GET carrying the session as a bearer token.
async fn get(app: &Router, uri: &str, session: Option<&Session>) -> StatusCode {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(session) = session {
        builder = builder.header("authorization", format!("Bearer {}", session.token));
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).expect("build the request"))
        .await
        .expect("the route must answer");
    response.status()
}

/// `POST /logout` with the session, optionally with a forged CSRF value.
async fn logout(
    app: &Router,
    session: Option<&Session>,
    csrf: Option<&str>,
) -> (StatusCode, Option<String>) {
    let mut builder = Request::builder().method("POST").uri("/logout");
    if let Some(session) = session {
        builder = builder.header("authorization", format!("Bearer {}", session.token));
        if let Some(csrf) = csrf {
            builder = builder.header("x-archava-csrf", csrf);
        }
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).expect("build the request"))
        .await
        .expect("the logout route must answer");
    let status = response.status();
    let cookie = response
        .headers()
        .get("set-cookie")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    (status, cookie)
}

/// The number of live session rows for a token — the server's own authority.
async fn live_session_rows(fixture: &Fixture, token: &str) -> i64 {
    // The token is stored hashed; the digest is computed the same way the
    // identity port computes it, so this asks the table the same question.
    let digest = identity_digest(token);
    let row: (i64,) =
        sqlx::query_as("SELECT count(*) FROM archava_identity.sessions WHERE token_hash = $1")
            .bind(digest)
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("count the session rows");
    row.0
}

/// The identity port's session digest: HMAC-SHA256 over the label and value,
/// hex-encoded. Mirrors `IdentityStore::digest` exactly — the same label
/// separator, the same encoding — so this asks the sessions table the same
/// question the server asks.
fn identity_digest(token: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(IDENTITY_KEY).expect("any key length");
    mac.update(b"session");
    mac.update(b"\0");
    mac.update(token.as_bytes());
    let bytes = mac.finalize().into_bytes();
    let mut hexed = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hexed.push_str(&format!("{byte:02x}"));
    }
    hexed
}

/// **Witness: signing out ends the server session, and the old credential
/// can no longer read, mutate or recover.**
#[tokio::test]
async fn signing_out_revokes_the_session_so_the_old_credential_cannot_read_mutate_or_recover() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app).await;

    // Before: the credential reads the tenant's contact.
    assert_eq!(
        get(&app, &format!("/contacts?tenant={ACME}"), Some(&session)).await,
        StatusCode::OK,
        "a live session reads the contacts"
    );

    let (status, cookie) = logout(&app, Some(&session), Some(&session.csrf)).await;
    assert_eq!(status, StatusCode::OK, "the logout route answers");
    let cookie = cookie.expect("the logout response expires the cookie");
    assert!(
        cookie.contains("archava_session=;") && cookie.contains("Max-Age=0"),
        "the cookie is expired, not merely cleared in script: {cookie}"
    );

    // The authority is gone: no session row answers for that token.
    assert_eq!(
        live_session_rows(&fixture, &session.token).await,
        0,
        "the session row is deleted — the server, not the page, closed it"
    );

    // After: the same credential cannot read, mutate, or recover.
    assert_eq!(
        get(&app, &format!("/contacts?tenant={ACME}"), Some(&session)).await,
        StatusCode::UNAUTHORIZED,
        "the old credential cannot read the contacts"
    );
    assert_eq!(
        get(
            &app,
            &format!(
                "/crm/commands/status?tenant={ACME}&requestId={}",
                uuid::Uuid::new_v4()
            ),
            Some(&session)
        )
        .await,
        StatusCode::UNAUTHORIZED,
        "the old credential cannot recover a receipt"
    );
    let mutation = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/crm/commands")
                .header("authorization", format!("Bearer {}", session.token))
                .header("x-archava-csrf", &session.csrf)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "tenant": ACME,
                        "requestId": uuid::Uuid::new_v4(),
                        "command": {
                            "operation": "followup.create",
                            "contactId": uuid::Uuid::new_v4(),
                            "contactRevision": 1,
                            "summary": "a follow-up after sign-out",
                            "dueAt": "2026-11-01T09:00:00Z",
                        },
                    })
                    .to_string(),
                ))
                .expect("build the mutation request"),
        )
        .await
        .expect("the command route must answer");
    assert_eq!(
        mutation.status(),
        StatusCode::UNAUTHORIZED,
        "the old credential cannot mutate"
    );
}

/// **Witness: a missing or forged CSRF is refused, and the session survives.**
///
/// The audit asked for the CSRF negative to be tested WITH a valid session:
/// the old browser witness used a session-less request, which proves nothing
/// about the gate that protects a live session.
#[tokio::test]
async fn a_missing_or_forged_csrf_logout_is_refused_and_the_session_survives() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app).await;

    let (status, _) = logout(&app, Some(&session), None).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a logout without the CSRF header is refused"
    );
    assert_eq!(
        live_session_rows(&fixture, &session.token).await,
        1,
        "the refused logout revoked nothing"
    );
    assert_eq!(
        get(&app, &format!("/contacts?tenant={ACME}"), Some(&session)).await,
        StatusCode::OK,
        "the session still reads after the refused logout"
    );

    let (status, _) = logout(&app, Some(&session), Some("forged-csrf-value")).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a logout with a forged CSRF value is refused"
    );
    assert_eq!(
        live_session_rows(&fixture, &session.token).await,
        1,
        "the refused logout revoked nothing"
    );
    assert_eq!(
        get(&app, &format!("/contacts?tenant={ACME}"), Some(&session)).await,
        StatusCode::OK,
        "the session still reads after the forged logout"
    );
}

/// **Witness: logout revokes the presented token and no other session.**
#[tokio::test]
async fn logout_leaves_another_live_session_of_the_same_user_alone() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let first = login(&app).await;
    let second = login(&app).await;

    let (status, _) = logout(&app, Some(&first), Some(&first.csrf)).await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(
        get(&app, &format!("/contacts?tenant={ACME}"), Some(&first)).await,
        StatusCode::UNAUTHORIZED,
        "the signed-out session is refused"
    );
    assert_eq!(
        get(&app, &format!("/contacts?tenant={ACME}"), Some(&second)).await,
        StatusCode::OK,
        "the other live session still reads — logout is per token"
    );
}

/// **Witness: a restart does not resurrect a revoked session.**
#[tokio::test]
async fn a_restart_does_not_resurrect_a_revoked_session() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app).await;

    let (status, _) = logout(&app, Some(&session), Some(&session.csrf)).await;
    assert_eq!(status, StatusCode::OK);

    // A fresh serving state over the same database — the restart.
    let restarted = archava_host::router(state(&fixture).await);
    assert_eq!(
        get(
            &restarted,
            &format!("/contacts?tenant={ACME}"),
            Some(&session)
        )
        .await,
        StatusCode::UNAUTHORIZED,
        "the revoked credential stays revoked across a restart"
    );
}

/// **Witness: logging out with no session is unauthenticated, not a success.**
#[tokio::test]
async fn a_logout_without_a_session_is_unauthenticated() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);

    let (status, cookie) = logout(&app, None, None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "there is no session to close, and that is not an empty dataset"
    );
    assert!(
        cookie.is_none(),
        "a refusal does not expire a cookie it never validated"
    );
}

/// **Witness: the logout is recorded in the identity event log.**
#[tokio::test]
async fn the_logout_is_appended_to_the_identity_event_log() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app).await;

    let (status, _) = logout(&app, Some(&session), Some(&session.csrf)).await;
    assert_eq!(status, StatusCode::OK);

    let row: (i64,) = sqlx::query_as(
        "SELECT count(*) FROM archava_identity.events WHERE user_id = $1 AND kind = 'logout'",
    )
    .bind(PRINCIPAL)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("count the logout events");
    assert_eq!(row.0, 1, "the logout is recorded once, against its user");
}

/// **Witness: one source burning its login budget does not close sign-in for
/// another source** (F-09).
///
/// The router used to present one constant source to `Identity::login`, so
/// sixty failed attempts from anywhere refused every user for five minutes —
/// an unauthenticated denial of service that cost an attacker nothing. The
/// budget is per source now; this drives the real route and proves the
/// isolation end to end: the noisy peer is locked out of its own bucket and the
/// quiet peer still signs in.
///
/// The budget is spent by moving the noisy source's own row to the limit, not
/// by replaying sixty refusals: every refusal runs a key derivation (seconds
/// each in a debug build), so sixty of them outlive the limiter's five-minute
/// window - the first draft of this test did exactly that and proved nothing,
/// because the window rolled over before the sixty-first attempt. Two probes
/// name the row instead: a refusal writes the source row, the (account, peer)
/// pair row and the account-wide observation row, the account changes between
/// the probes, and only the source row counts both. Everything after that is
/// the real route, the real limiter and the real rows.
#[tokio::test]
async fn exhausting_one_source_does_not_lock_another_source_out() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);

    let noisy = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7));
    let quiet = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 9));

    // Two refusals from the noisy peer, each against a different account, so
    // the only budget either probe spends is the source's.
    let (status, _) = login_from(&app, noisy, "nobody-one@archava.test", PASSWORD).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "an unknown account is refused"
    );
    let after_one = login_buckets(&fixture).await;
    let (status, _) = login_from(&app, noisy, "nobody-two@archava.test", PASSWORD).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "an unknown account is refused"
    );
    let after_two = login_buckets(&fixture).await;

    let mut source_key = None;
    for (key, attempts) in &after_two {
        if let Some((_, before)) = after_one.iter().find(|(seen, _)| seen == key) {
            if *attempts == *before + 1 {
                source_key = Some(key.clone());
            }
        }
    }
    let source_key = source_key.expect("the source row is the one both probes count");

    // Sixty refusals' worth of budget, written where the limiter reads it.
    // (The limiter refuses once `attempts` passes its source limit of 60.)
    sqlx::query(
        "UPDATE archava_identity.login_buckets SET attempts = 60, window_start = now() \
         WHERE key = $1",
    )
    .bind(&source_key)
    .execute(&fixture.admin_pool)
    .await
    .expect("spend the noisy source's budget");

    // The noisy source has spent its own budget: the right password is refused
    // now, which is the behaviour the limit exists for.
    let account = format!("{PRINCIPAL}@archava.test");
    let (status, _) = login_from(&app, noisy, &account, PASSWORD).await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "the noisy source has spent its own budget"
    );

    // A different source is untouched. Before the fix this answered 429 too,
    // because both peers shared the single `host-router` bucket.
    let (status, outcome) = login_from(&app, quiet, &account, PASSWORD).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a quiet source still signs in while another source is throttled: {outcome}"
    );
    assert!(
        outcome["token"]
            .as_str()
            .is_some_and(|token| !token.is_empty()),
        "the quiet source receives a session token"
    );
}

/// **Witness: a wrong password cannot lock the owner out (H01).**
///
/// The account-wide bucket used to be a gate, and only a success cleared it, so
/// five wrong passwords every fifteen minutes from anywhere refused the real
/// owner indefinitely — the refusal was what prevented the success that would
/// have unlocked the account.
///
/// The fixture spends one real refusal (one key derivation) and then writes the
/// row counts directly, the way the source test above does: replaying six
/// refusals would cost six debug-build derivations and prove the same rows. All
/// three rows written by that refusal are moved to 6, so the account-wide row is
/// at a value that USED to refuse everyone:
///
///   * the same peer is refused (the (account, peer) gate is real), and
///   * a different peer signs in with the right password, which is only
///     possible because the account-wide row is an observation now.
#[tokio::test]
async fn a_wrong_password_locks_the_peer_out_not_the_account() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);

    let attacker = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 20));
    let owner = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 21));
    let account = format!("{PRINCIPAL}@archava.test");

    let (status, _) = login_from(&app, attacker, &account, "wrong password entirely").await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a wrong password is a refusal"
    );

    // Live counts as the limiter reads them: one refusal spent the source, the
    // (account, peer) pair, and the account-wide row.
    let rows = login_buckets(&fixture).await;
    assert_eq!(rows.len(), 3, "one refusal writes three rows: {rows:?}");
    sqlx::query("UPDATE archava_identity.login_buckets SET attempts = 6, window_start = now()")
        .execute(&fixture.admin_pool)
        .await
        .expect("spend the pair's budget");

    let (status, _) = login_from(&app, attacker, &account, PASSWORD).await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "the peer that spent the pair's budget is refused, even with the right password"
    );

    let (status, outcome) = login_from(&app, owner, &account, PASSWORD).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the owner's own peer still signs in while another peer is throttled \
         and the account-wide row is saturated: {outcome}"
    );
}

// --- A8: the transport policy -------------------------------------------------

/// `POST /login` in the shape a BROWSER sends: no client header, so the token
/// must arrive in the cookie and nowhere else (F-07). `forwarded_proto` is what
/// a TLS terminator would have added.
async fn login_like_a_browser(
    app: &Router,
    forwarded_proto: Option<&str>,
) -> (Vec<String>, serde_json::Value) {
    let _slot = LOGIN_SLOTS.acquire().await.expect("login slots are open");
    let body = serde_json::json!({
        "username": format!("{PRINCIPAL}@archava.test"),
        "password": PASSWORD,
    })
    .to_string();
    let mut builder = Request::builder()
        .method("POST")
        .uri("/login")
        .header("content-type", "application/json");
    if let Some(proto) = forwarded_proto {
        builder = builder.header("x-forwarded-proto", proto);
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(Body::from(body))
                .expect("build the login request"),
        )
        .await
        .expect("the login route must answer");
    assert_eq!(response.status(), StatusCode::OK, "login must succeed");
    let cookies = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok().map(str::to_string))
        .collect();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read the login response");
    let outcome = serde_json::from_slice(&bytes).expect("the login response is JSON");
    (cookies, outcome)
}

/// **Witness: a browser-shaped login carries no token in the body, and the
/// cookie it does get is HttpOnly, same-site and path-locked** (F-01/F-07).
///
/// The audit's F-07: the login body shipped the raw session token to every
/// caller, including the browser whose token the HttpOnly cookie exists to keep
/// out of script reach. A page that can read its own session token can replay
/// it anywhere it likes; the cookie flag bought nothing. The token now reaches
/// a browser only through the cookie.
#[tokio::test]
async fn a_browser_login_keeps_the_token_out_of_the_body() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);

    let (cookies, outcome) = login_like_a_browser(&app, None).await;

    assert!(
        outcome.get("token").is_none(),
        "a browser must not read its session token out of the response body: {outcome}"
    );
    assert!(
        outcome["csrf"]
            .as_str()
            .is_some_and(|csrf| !csrf.is_empty()),
        "the browser still receives the CSRF value its script must echo: {outcome}"
    );

    let cookie = cookies
        .iter()
        .find(|cookie| cookie.starts_with("archava_session="))
        .unwrap_or_else(|| panic!("the login must set the session cookie: {cookies:?}"));
    assert!(cookie.contains("HttpOnly"), "no script reach: {cookie}");
    assert!(
        cookie.contains("SameSite=Strict"),
        "no cross-site send: {cookie}"
    );
    assert!(
        cookie.contains("Path=/"),
        "the whole host sees it: {cookie}"
    );
    assert!(
        !cookie.contains("Secure"),
        "a plain-HTTP login must not set a cookie the browser would refuse to store: {cookie}"
    );
}

/// **Witness: the same login over TLS mints `__Host-`, `Secure`, and gets HSTS**
/// (F-01/F-02).
#[tokio::test]
async fn a_tls_login_sets_the_secure_host_cookie() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);

    let (cookies, outcome) = login_like_a_browser(&app, Some("https")).await;

    let cookie = cookies
        .iter()
        .find(|cookie| cookie.starts_with("__Host-archava_session="))
        .unwrap_or_else(|| {
            panic!("a TLS login must mint the __Host- prefixed cookie: {cookies:?}")
        });
    assert!(
        cookie.contains("; Secure"),
        "and the Secure attribute: {cookie}"
    );
    assert!(
        !cookie.contains("Domain"),
        "a __Host- cookie forbids Domain: {cookie}"
    );
    assert!(
        outcome.get("token").is_none(),
        "TLS does not change what the body carries: {outcome}"
    );
}

/// **Witness: every response carries the CRM's header policy — refusals too —
/// and HSTS only where the request arrived over TLS** (F-02).
#[tokio::test]
async fn every_response_carries_the_shared_security_headers() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);

    // A refusal (404) is the shape a browser is most likely to render as bare
    // text, so the layer must cover it, not just the JSON routes.
    for uri in ["/healthz", "/no-such-route"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("build the request"),
            )
            .await
            .expect("the route must answer");
        let headers = response.headers();
        assert_eq!(headers["x-content-type-options"], "nosniff", "uri {uri}");
        assert_eq!(headers["referrer-policy"], "no-referrer", "uri {uri}");
        assert_eq!(headers["cache-control"], "no-store", "uri {uri}");
        assert_eq!(
            headers["content-security-policy"],
            "default-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
            "uri {uri}"
        );
        assert!(
            headers.get("strict-transport-security").is_none(),
            "HSTS on plain HTTP would lock the fixture out of itself: uri {uri}"
        );
    }

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .header("x-forwarded-proto", "https")
                .body(Body::empty())
                .expect("build the request"),
        )
        .await
        .expect("the route must answer");
    assert_eq!(
        response.headers()["strict-transport-security"],
        "max-age=31536000",
        "a TLS-terminated answer carries HSTS"
    );
}

/// **Witness: logout expires BOTH cookie names** (F-01).
///
/// A jar can hold a session minted before the host gained TLS and one minted
/// after; expiring only one leaves the other credential live in the browser
/// that just signed out.
#[tokio::test]
async fn a_logout_expires_both_session_cookie_names() {
    let fixture = provision().await;
    seed(&fixture).await;
    let app = archava_host::router(state(&fixture).await);
    let session = login(&app).await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/logout")
                .header("authorization", format!("Bearer {}", session.token))
                .header("x-archava-csrf", &session.csrf)
                .body(Body::empty())
                .expect("build the logout request"),
        )
        .await
        .expect("the logout route must answer");
    assert_eq!(response.status(), StatusCode::OK);

    let cookies: Vec<String> = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok().map(str::to_string))
        .collect();
    for name in ["__Host-archava_session=", "archava_session="] {
        let cookie = cookies
            .iter()
            .find(|cookie| cookie.starts_with(name))
            .unwrap_or_else(|| panic!("logout must expire {name}: {cookies:?}"));
        assert!(cookie.contains("Max-Age=0"), "uri {name}: {cookie}");
    }
}
