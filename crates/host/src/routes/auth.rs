//! Authentication and session lifecycle routes (`/login`, `/logout`).

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};

use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::identity::IdentityError;
use crate::router::{authenticate, session_token, AppState};
use crate::security::{
    expired_session_cookies, is_command_line_client, is_secure_from_peer, session_cookie,
};

/// The single bucket every login shares when the embedder did not install
/// `ConnectInfo`. Named so an operator reading `login_buckets` sees the
/// degradation instead of mistaking one row for per-client limiting.
pub const NO_CONNECT_INFO_SOURCE: &str = "no-connect-info";

/// The source a login attempt is bucketed under (F-09).
///
/// `Identity::login` keeps one bucket per source, so 60 failures from one peer
/// can no longer close sign-in for every other peer — the audit's cheap global
/// DoS. The source is the transport peer address that
/// `into_make_service_with_connect_info::<SocketAddr>()` installs; an embedder
/// that serves the router without it falls back to [`NO_CONNECT_INFO_SOURCE`],
/// which reproduces the old shared bucket instead of turning a login into a 500.
pub struct LoginSource {
    /// The rate-limit bucket key.
    pub bucket: String,
    /// The peer that must be a trusted hop before its `X-Forwarded-Proto` can
    /// select the secure cookie (audit H10). `None` keeps the embedding/test
    /// posture, where no direct public caller is known.
    pub peer: Option<IpAddr>,
}

impl FromRequestParts<AppState> for LoginSource {
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(address)| address.ip());
        let bucket = peer
            .map(|address| address.to_string())
            .unwrap_or_else(|| NO_CONNECT_INFO_SOURCE.to_string());
        Ok(Self { bucket, peer })
    }
}

/// `POST /login`. Identity only — no tenant, no role, no grant: a session is
/// proof of *who*, and everything about *what they may do* is resolved per
/// request from commerce memberships.
#[derive(Debug, Deserialize)]
pub struct LoginSubmission {
    pub username: String,
    pub password: String,
}

/// The answer to a login.
#[derive(Debug, Serialize)]
pub struct LoginOutcome {
    /// `authenticated`, `rejected`, or `refused`.
    pub decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The session token: present exactly once, in this response, and only for
    /// a caller that identifies itself as a command-line client
    /// (`x-archava-client: cli`). A browser receives it in the HttpOnly cookie
    /// instead — a body its own script could read would undo that. Only the HMAC
    /// digest is stored server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// The CSRF value bound to this token — send it back as `x-archava-csrf`
    /// on every mutation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub csrf: Option<String>,
}

impl LoginOutcome {
    pub(crate) fn base(decision: &str) -> Self {
        Self {
            decision: decision.to_string(),
            reason: None,
            message: None,
            token: None,
            csrf: None,
        }
    }
}

/// The logout route's body. A separate contract from `CommandOutcome`: the
/// decision vocabulary there enumerates command outcomes, and "signed out" is
/// not one of them. The refusal shape is identical (decision/reason/message),
/// so a caller reads one contract either way.
#[derive(Debug, Serialize)]
pub struct LogoutOutcome {
    /// `signed_out` on success; the refusal vocabulary otherwise
    /// (`unauthenticated`, `refused`).
    pub decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Mounts the session authentication route family.
pub fn router() -> Router<AppState> {
    Router::new()
        // R09 FIX: /login accepts POST for authentication; GET redirects to sovereign workspace login (/app)
        // preventing 405 Method Not Allowed on browser redirects or bookmarks.
        .route(
            "/login",
            post(login).get(|| async { axum::response::Redirect::temporary("/app") }),
        )
        // The logout route: the server-side end of a session (C02). Without
        // it, "sign out" only cleared browser state and the credential kept
        // reading rows — the audit's reproduced defect.
        .route("/logout", post(logout))
}

/// The login route: exchange username + password for a session.
///
/// Identity only. No tenant and no membership is consulted here — a user may
/// hold memberships in several tenants (or none), and which one a command
/// targets is decided per request by the membership it can prove. Failures
/// are single-reason on purpose: invalid credentials is one answer whether
/// the username is unknown, the password wrong, the account deactivated or
/// the session insert racing a deactivation.
pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    source: LoginSource,
    submission: Result<Json<LoginSubmission>, JsonRejection>,
) -> (StatusCode, HeaderMap, Json<LoginOutcome>) {
    let Json(submission) = match submission {
        Ok(submission) => submission,
        Err(rejected) => {
            tracing::warn!(error = %rejected, "refused a login submission at the boundary");
            return (
                rejected.status(),
                HeaderMap::new(),
                Json(LoginOutcome {
                    reason: Some("malformed_submission".to_string()),
                    message: Some("The submission could not be processed.".to_string()),
                    ..LoginOutcome::base("rejected")
                }),
            );
        }
    };
    match state
        .identity()
        .login(&submission.username, &submission.password, &source.bucket)
        .await
    {
        Ok(token) => {
            let csrf = state.identity().csrf(&token);
            // The browser path holds the token in an HttpOnly cookie — `Secure`
            // and `__Host-` prefixed when the request arrived over TLS — and
            // keeps the CSRF value in the body, where the slice's own script
            // reads it and echoes it as the mutation header. A refusal never
            // sets a cookie.
            let mut response_headers = HeaderMap::new();
            response_headers.insert(
                header::SET_COOKIE,
                session_cookie(&token, is_secure_from_peer(source.peer, &headers))
                    .parse()
                    .expect("a well-formed cookie attribute list"),
            );
            // F-07: a browser never needs the raw token in the body — reading it
            // there is exactly the leak the HttpOnly cookie prevents. A scripted
            // client without a cookie jar asks for it by name, and a cross-site
            // page cannot set that header without a preflight this host never
            // approves.
            let body_token = is_command_line_client(&headers).then_some(token);
            (
                StatusCode::OK,
                response_headers,
                Json(LoginOutcome {
                    token: body_token,
                    csrf: Some(csrf),
                    ..LoginOutcome::base("authenticated")
                }),
            )
        }
        Err(error) => {
            let (status, outcome) = refused_login_outcome(&error);
            (status, HeaderMap::new(), Json(outcome))
        }
    }
}

/// The logout route (C02): revoke the server session, THEN close the cookie.
///
/// The order is the whole point. The session row is deleted first — the
/// server's authority — and the cookie expiry is only sent afterwards. The
/// reverse order would let a failed revocation look like a signed-out
/// browser while the credential kept working, which is exactly the defect
/// the 19 Oktober audit reproduced: "sign out" cleared UI state and the old
/// cookie still read twenty authorized contact rows.
///
/// A CSRF failure refuses BEFORE anything is revoked, so a forged cross-site
/// request cannot close somebody else's session, and a session that is
/// already gone is `401 unauthenticated` rather than a success that closed
/// nothing. Other live sessions of the same user are untouched: this revokes
/// the presented token, nothing else.
pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> (StatusCode, HeaderMap, Json<LogoutOutcome>) {
    let session = match authenticate(&state, &headers, true).await {
        Ok(session) => session,
        Err((status, Json(outcome))) => {
            return (
                status,
                HeaderMap::new(),
                Json(LogoutOutcome {
                    decision: outcome.decision,
                    reason: outcome.reason,
                    message: outcome.message,
                }),
            )
        }
    };
    // The authenticated session is what proves the caller may close THIS
    // token; the token itself is what gets revoked.
    let _ = session;
    let Some(token) = session_token(&headers) else {
        // `authenticate` only succeeds for a presented token, so this is
        // unreachable in practice; answer honestly rather than assume.
        return (
            StatusCode::UNAUTHORIZED,
            HeaderMap::new(),
            Json(LogoutOutcome {
                decision: "unauthenticated".to_string(),
                reason: Some("unauthenticated".to_string()),
                message: Some("There is no session to close.".to_string()),
            }),
        );
    };
    match state.identity().revoke(&token).await {
        Ok(true) => (
            StatusCode::OK,
            expire_session_cookie(),
            Json(LogoutOutcome {
                decision: "signed_out".to_string(),
                reason: None,
                message: Some("The session is closed.".to_string()),
            }),
        ),
        Ok(false) => (
            // The session disappeared between the authentication and the
            // revocation (another tab signed out first). The caller's goal is
            // still met — no session answers for that token any more — so
            // this closes the cookie too rather than pretending otherwise.
            StatusCode::OK,
            expire_session_cookie(),
            Json(LogoutOutcome {
                decision: "signed_out".to_string(),
                reason: None,
                message: Some("The session is closed.".to_string()),
            }),
        ),
        Err(error) => {
            tracing::error!(error = %error, "identity store unavailable while revoking a session");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                HeaderMap::new(),
                Json(LogoutOutcome {
                    decision: "refused".to_string(),
                    reason: Some("identity_unavailable".to_string()),
                    message: Some("The session could not be closed right now.".to_string()),
                }),
            )
        }
    }
}

/// The expired-cookie headers the logout route sends — both names, because the
/// jar may hold either (a session minted before, and one minted after, the
/// deployment gained a TLS terminator in front of it).
pub fn expire_session_cookie() -> HeaderMap {
    let mut headers = HeaderMap::new();
    for cookie in expired_session_cookies() {
        headers.append(
            header::SET_COOKIE,
            cookie.parse().expect("a well-formed cookie attribute list"),
        );
    }
    headers
}

/// The login route's refusal mapping, as a pure function so it has in-crate
/// unit tests (RN07/W-PW03): the load-bearing property is WHICH status each
/// identity failure produces — an overload must be distinguishable from a
/// wrong password, and a code path that once folded `Busy` into
/// `invalid_credentials` was exactly such a confusion, fixed by asserting the
/// boundaries here where they cost nothing to run.
///
/// `IdentityError` has no `PartialEq` (deadlock-free comparisons of errors
/// are not meaningful), so matching is exhaustive by construction.
pub fn refused_login_outcome(error: &IdentityError) -> (StatusCode, LoginOutcome) {
    let (status, reason, message) = match error {
        IdentityError::InvalidCredentials => (
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "The username or password is incorrect.",
        ),
        IdentityError::RateLimited => (
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Too many login attempts; try again later.",
        ),
        IdentityError::Busy => (
            // Deliberately NOT 401: a credential-shaped answer for a correct
            // password under load tells its user the password was wrong, and
            // the natural retry then burns the account's rate budget chasing
            // a credential problem that does not exist.
            StatusCode::SERVICE_UNAVAILABLE,
            "busy",
            "Too many simultaneous verifications; try again.",
        ),
        IdentityError::Unavailable | IdentityError::Unauthenticated => (
            // Operator-facing detail goes to the log, never the response: the
            // caller's answer is `503 refused`, and an error naming the query
            // or the constraint is exactly what must not be reflected (D09).
            StatusCode::SERVICE_UNAVAILABLE,
            "identity",
            "",
        ),
    };
    (
        status,
        LoginOutcome {
            reason: Some(reason.to_string()),
            message: (!message.is_empty()).then(|| message.to_string()),
            ..LoginOutcome::base("refused")
        },
    )
}
