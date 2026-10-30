//! Transport hardening: the cookie policy the login route mints under, and the
//! response headers every route carries.
//!
//! Audit A8 (`docs/archive/audits/A8_HARDENING_2026-10-22.md`) found three things here.
//! F-01: the session cookie had no `Secure` attribute and no `__Host-` prefix,
//! because the fixture runs plain HTTP and the old comment deferred the whole
//! question to a hypothetical TLS front. F-02: this host set no
//! `Content-Security-Policy`, `Strict-Transport-Security`,
//! `X-Content-Type-Options` or `Referrer-Policy`, while the TypeScript CRM host
//! (`apps/crm/src/host.ts`) already set all four — so a browser reading a JSON
//! error from here got a weaker header set than the same browser reading one
//! from the CRM. F-07: the raw session token was in the login body for every
//! caller, including browsers that already receive it in an HttpOnly cookie.
//!
//! The policy below is the CRM's, not a new one: `nosniff`, `no-referrer`,
//! `no-store`, `frame-ancestors 'none'`, `base-uri 'none'`, `form-action
//! 'none'`, and a one-year `max-age` HSTS. This host answers JSON only, so its
//! `default-src 'none'` is stricter than the CRM's `'self'` origin — nothing
//! here is meant to be a document.

use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, Request};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;

/// The session cookie over plain HTTP: the fixture's browsers, and the only
/// name a `Secure`-less transport may set. A `__Host-` cookie is defined to be
/// `Secure`, `Path=/` and host-locked, so a script driving `http://127.0.0.1`
/// could never send it back — the two names are how one binary serves both.
pub(crate) const SESSION_COOKIE: &str = "archava_session";

/// The session cookie over TLS: host-locked (no `Domain`), path-locked to `/`
/// and `Secure`, which is exactly the prefix's contract and what keeps a
/// compromised sibling subdomain from overwriting it.
pub(crate) const SECURE_SESSION_COOKIE: &str = "__Host-archava_session";

/// Header a non-browser client sets to ask for the token in the login body.
///
/// F-07: a browser never needs it (the token arrives in an HttpOnly cookie and
/// its own script has the CSRF value), while a scripted client that cannot
/// carry a cookie jar does. Opting in by name is the point: a cross-site page
/// cannot set this header on a `fetch` without a CORS preflight this host never
/// approves, so the adaptive path is not a hole in the cookie policy.
pub(crate) const CLIENT_HEADER: &str = "x-archava-client";

/// The API's content policy: this host serves JSON, never a document.
pub(crate) const CONTENT_SECURITY_POLICY: &str =
    "default-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

/// The policy for the SSR document and the wasm bundle it hydrates.
///
/// `CONTENT_SECURITY_POLICY` forbids everything, which is correct for JSON and
/// would blank the CRM slice: the document loads its module and wasm from
/// `/pkg`, and Leptos emits one inline `<script type="module">` that starts
/// hydration. `'wasm-unsafe-eval'` is what browsers require before they will
/// compile a `WebAssembly` module under a CSP; `'unsafe-inline'` on the style
/// side covers the `style` attributes the views use. Scripts stay restricted to
/// this origin plus the one inline bootstrap the framework writes.
pub(crate) const DOCUMENT_CONTENT_SECURITY_POLICY: &str = "default-src 'self'; \
     script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval'; \
     style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; \
     connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; \
     form-action 'self'; object-src 'none'";

/// The policy for embedded modals/widgets (e.g. `/embed/checkout`).
/// U01 FIX: Allows framing via `frame-ancestors *` so merchant websites can
/// embed checkout modal iframes without browser CSP violations.
pub(crate) const EMBED_CONTENT_SECURITY_POLICY: &str = "default-src 'self'; \
     script-src 'self' 'unsafe-inline'; \
     style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; \
     connect-src 'self'; base-uri 'none'; frame-ancestors *; \
     form-action 'self'; object-src 'none'";

/// One year, the CRM's value — long enough that a client that ever reaches TLS
/// stops being downgradable, short enough to expire in a browser's lifetime.
pub(crate) const STRICT_TRANSPORT_SECURITY: &str = "max-age=31536000";

/// Whether this request arrived over TLS.
///
/// The router is a plain-HTTP `axum` service by design (a TLS terminator sits
/// in front of it), so the scheme is only knowable from the hop that terminated
/// TLS: `X-Forwarded-Proto` first, then the `Origin` a browser sends. Anything
/// unrecognised is plain HTTP, which mints the weaker cookie rather than
/// claiming a `Secure` one the browser would refuse to store.
///
/// Both signals are client-controlled headers, so they are only honoured when
/// the transport peer is a trusted hop — loopback, RFC 1918, link-local or
/// IPv6 unique-local, which is where a terminator on the same host or private
/// network connects from. A direct public caller cannot promote its request to
/// TLS (audit H10). An embedder that serves the router without `ConnectInfo`
/// degrades to the trusted posture, the same fallback the login bucket uses;
/// the shipped launchers install `ConnectInfo`, so the defense applies there.
pub(crate) fn is_secure_from_peer(peer: Option<IpAddr>, headers: &HeaderMap) -> bool {
    if !is_trusted_hop(peer) {
        return false;
    }
    if let Some(forwarded) = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
    {
        return forwarded.trim().eq_ignore_ascii_case("https");
    }
    headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| {
            origin
                .get(..8)
                .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
        })
}

/// Whether `peer` is allowed to describe the scheme of the original request.
///
/// `None` is the embedding/test posture: no peer was installed, so there is no
/// evidence of a direct public caller to distrust. Deployment contracts that
/// need the spoofing defense serve with `ConnectInfo`.
fn is_trusted_hop(peer: Option<IpAddr>) -> bool {
    match peer {
        None => true,
        Some(IpAddr::V4(address)) => {
            address.is_loopback() || address.is_private() || address.is_link_local()
        }
        Some(IpAddr::V6(address)) => {
            address.is_loopback() || address.is_unique_local() || address.is_unicast_link_local()
        }
    }
}

/// The transport peer `into_make_service_with_connect_info` installed, when it
/// did. Shared by both header layers and the login route.
pub(crate) fn peer_ip(request: &Request) -> Option<IpAddr> {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip())
}

/// The carrier for a browser: `HttpOnly` (no script reach), `SameSite=Strict`
/// (no cross-site send), `Path=/`, and `Secure` + `__Host-` under TLS.
///
/// No `Domain` attribute: `__Host-` forbids it, and on the plain name its
/// absence is what makes the cookie host-only.
pub(crate) fn session_cookie(token: &str, secure: bool) -> String {
    let name = if secure {
        SECURE_SESSION_COOKIE
    } else {
        SESSION_COOKIE
    };
    let mut cookie = format!("{name}={token}; HttpOnly; Path=/; SameSite=Strict");
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// The expiry a logout writes. Both names are expired, because the client may
/// hold either — a session minted before a TLS hop and one minted after it are
/// two different cookies in the same jar.
pub(crate) fn expired_session_cookies() -> [String; 2] {
    [
        format!(
            "{SECURE_SESSION_COOKIE}=; HttpOnly; Path=/; SameSite=Strict; Secure; Max-Age=0; \
             Expires=Thu, 01 Jan 1970 00:00:00 GMT"
        ),
        format!(
            "{SESSION_COOKIE}=; HttpOnly; Path=/; SameSite=Strict; Max-Age=0; \
             Expires=Thu, 01 Jan 1970 00:00:00 GMT"
        ),
    ]
}

/// The token in a request's `Cookie` header, from either name.
///
/// Both are accepted on the way in — that is what lets a session survive the
/// deployment adding TLS in front of a host that used to answer plain HTTP —
/// and neither one's value is trusted here: `Identity::authenticate` still
/// checks the digest, the version and the idle window.
pub(crate) fn cookie_token(cookies: &str) -> Option<String> {
    let names = [SECURE_SESSION_COOKIE, SESSION_COOKIE];
    cookies.split(';').find_map(|cookie| {
        let cookie = cookie.trim();
        names.iter().find_map(|name| {
            cookie
                .strip_prefix(name)
                .and_then(|rest| rest.strip_prefix('='))
                .map(str::to_string)
                .filter(|token| !token.is_empty())
        })
    })
}

/// Whether the caller asked for the token in the body (see [`CLIENT_HEADER`]).
pub(crate) fn is_command_line_client(headers: &HeaderMap) -> bool {
    headers
        .get(CLIENT_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("cli"))
}

/// Apply the CRM's header policy to a response. HSTS only over TLS: sending it
/// on a plain-HTTP response teaches a browser to refuse the very scheme the
/// fixture is served on.
pub(crate) fn apply(headers: &mut HeaderMap, secure: bool) {
    apply_policy(headers, secure, CONTENT_SECURITY_POLICY);
}

fn apply_policy(headers: &mut HeaderMap, secure: bool, policy: &'static str) {
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(policy),
    );
    if secure {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static(STRICT_TRANSPORT_SECURITY),
        );
    }
}

/// The layer that puts [`apply`] on every route, refusals included.
///
/// Inside the handler would be one `insert` per return site — and the refusals,
/// the ones a browser is most likely to render as bare text, would be the ones
/// that got missed.
pub async fn security_headers(request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let is_embed = path == "/embed/checkout" || path.starts_with("/embed/");
    let secure = is_secure_from_peer(peer_ip(&request), request.headers());
    let mut response = next.run(request).await;
    if is_embed {
        apply_policy(
            response.headers_mut(),
            secure,
            EMBED_CONTENT_SECURITY_POLICY,
        );
    } else {
        apply(response.headers_mut(), secure);
    }
    response
}

/// The same layer for a document router (the SSR shell and its `/pkg` files).
///
/// Kept beside [`security_headers`] so the two policies cannot drift into one
/// accidental copy: the JSON policy forbids all content, the document policy
/// admits only this origin and the hydration bootstrap.
pub async fn document_security_headers(request: Request, next: Next) -> Response {
    let secure = is_secure_from_peer(peer_ip(&request), request.headers());
    let mut response = next.run(request).await;
    apply_policy(
        response.headers_mut(),
        secure,
        DOCUMENT_CONTENT_SECURITY_POLICY,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F-01: a plain-HTTP login mints the plain name, still `HttpOnly` and
    /// `SameSite=Strict`, and still without `Secure` (a browser would drop it).
    #[test]
    fn a_plain_http_session_mints_the_plain_cookie() {
        let cookie = session_cookie("token-value", false);
        assert_eq!(
            cookie,
            "archava_session=token-value; HttpOnly; Path=/; SameSite=Strict"
        );
        assert!(!cookie.contains("Secure"));
    }

    /// F-01: over TLS the SAME login mints `__Host-`, and the prefix's rules
    /// hold: `Secure`, `Path=/`, no `Domain`.
    #[test]
    fn a_tls_session_mints_the_host_prefixed_cookie() {
        let cookie = session_cookie("token-value", true);
        assert!(
            cookie.starts_with("__Host-archava_session=token-value;"),
            "the TLS cookie must carry the __Host- prefix: {cookie}"
        );
        assert!(
            cookie.contains("; Secure"),
            "and the Secure attribute: {cookie}"
        );
        assert!(cookie.contains("Path=/"), "and Path=/: {cookie}");
        assert!(
            !cookie.contains("Domain"),
            "a __Host- cookie forbids Domain: {cookie}"
        );
    }

    /// F-01: both names are cleared on logout, or a session minted under the
    /// other scheme survives the logout that was supposed to end it.
    #[test]
    fn a_logout_expires_both_cookie_names() {
        let expired = expired_session_cookies();
        assert!(expired[0].starts_with("__Host-archava_session=;"));
        assert!(expired[0].contains("Max-Age=0") && expired[0].contains("Secure"));
        assert!(expired[1].starts_with("archava_session=;"));
        assert!(expired[1].contains("Max-Age=0"));
    }

    /// F-01/F-07: the resolver reads either name, and reads neither out of a
    /// cookie that merely shares a suffix.
    #[test]
    fn the_resolver_reads_either_cookie_name() {
        assert_eq!(
            cookie_token("__Host-archava_session=prefixed; other=1").as_deref(),
            Some("prefixed")
        );
        assert_eq!(
            cookie_token("other=1; archava_session=plain").as_deref(),
            Some("plain")
        );
        assert_eq!(cookie_token("not_archava_session=decoy"), None);
        assert_eq!(cookie_token("archava_session="), None);
        assert_eq!(cookie_token(""), None);
    }

    /// F-02: the policy is the CRM's four headers, and HSTS only where it can
    /// be honoured.
    #[test]
    fn every_response_carries_the_shared_policy_and_hsts_only_over_tls() {
        let mut plain = HeaderMap::new();
        apply(&mut plain, false);
        assert_eq!(plain[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(plain[header::REFERRER_POLICY], "no-referrer");
        assert_eq!(plain[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            plain[header::CONTENT_SECURITY_POLICY],
            CONTENT_SECURITY_POLICY
        );
        assert!(
            !plain.contains_key(header::STRICT_TRANSPORT_SECURITY),
            "HSTS on a plain-HTTP answer would lock the fixture out of itself"
        );

        let mut tls = HeaderMap::new();
        apply(&mut tls, true);
        assert_eq!(tls[header::STRICT_TRANSPORT_SECURITY], "max-age=31536000");
    }

    /// The API policy is not weaker than the CRM's on the two directives that
    /// matter for a JSON endpoint: nothing may be framed, nothing may be a base.
    #[test]
    fn the_api_policy_forbids_framing_and_base_uris() {
        assert!(CONTENT_SECURITY_POLICY.contains("default-src 'none'"));
        assert!(CONTENT_SECURITY_POLICY.contains("frame-ancestors 'none'"));
        assert!(CONTENT_SECURITY_POLICY.contains("base-uri 'none'"));
        assert!(CONTENT_SECURITY_POLICY.contains("form-action 'none'"));
        assert!(!CONTENT_SECURITY_POLICY.contains("unsafe-inline"));
        assert!(!CONTENT_SECURITY_POLICY.contains("unsafe-eval"));
    }

    /// H10: the document policy must admit the hydrating bundle the JSON policy
    /// would forbid; the two are separate constants for exactly that reason.
    #[test]
    fn the_document_policy_admits_the_hydration_bundle() {
        assert!(DOCUMENT_CONTENT_SECURITY_POLICY.contains("default-src 'self'"));
        assert!(DOCUMENT_CONTENT_SECURITY_POLICY.contains("'wasm-unsafe-eval'"));
        assert!(DOCUMENT_CONTENT_SECURITY_POLICY.contains("script-src 'self'"));
        assert!(!DOCUMENT_CONTENT_SECURITY_POLICY.contains("'unsafe-eval'"));
        assert!(CONTENT_SECURITY_POLICY.contains("default-src 'none'"));
    }

    /// U01: the embed policy must allow framing via `frame-ancestors *` so merchant websites
    /// can embed checkout modals without browser CSP blocks, while API and document policies
    /// strictly retain `frame-ancestors 'none'`.
    #[test]
    fn the_embed_policy_allows_framing() {
        assert!(EMBED_CONTENT_SECURITY_POLICY.contains("frame-ancestors *"));
        assert!(CONTENT_SECURITY_POLICY.contains("frame-ancestors 'none'"));
        assert!(DOCUMENT_CONTENT_SECURITY_POLICY.contains("frame-ancestors 'none'"));
    }

    /// F-07: the request's scheme decides the cookie, and only the hop that
    /// terminated TLS can say so.
    #[test]
    fn the_forwarded_scheme_and_the_origin_mark_a_request_secure() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        assert!(is_secure_from_peer(None, &headers));

        headers.insert("x-forwarded-proto", HeaderValue::from_static("http"));
        assert!(!is_secure_from_peer(None, &headers));

        let mut origin = HeaderMap::new();
        origin.insert(header::ORIGIN, HeaderValue::from_static("https://crm.test"));
        assert!(is_secure_from_peer(None, &origin));

        origin.insert(header::ORIGIN, HeaderValue::from_static("http://crm.test"));
        assert!(!is_secure_from_peer(None, &origin));
        assert!(!is_secure_from_peer(None, &HeaderMap::new()));
    }

    /// H10: `X-Forwarded-Proto` and `Origin` are headers a direct caller owns,
    /// so only a loopback/private transport peer may use them to select the
    /// secure cookie. A public peer stays on the plain-HTTP posture.
    #[test]
    fn only_a_trusted_hop_may_claim_tls() {
        use std::net::{Ipv4Addr, Ipv6Addr};

        let public = Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9)));
        let mut forwarded = HeaderMap::new();
        forwarded.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        assert!(
            !is_secure_from_peer(public, &forwarded),
            "a direct public caller must not mint the __Host- cookie"
        );

        let mut origin = HeaderMap::new();
        origin.insert(header::ORIGIN, HeaderValue::from_static("https://crm.test"));
        assert!(
            !is_secure_from_peer(public, &origin),
            "the Origin header is no more trustworthy than the forwarded scheme"
        );

        let private = Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)));
        assert!(is_secure_from_peer(private, &forwarded));
        let loopback = Some(IpAddr::V6(Ipv6Addr::LOCALHOST));
        assert!(is_secure_from_peer(loopback, &forwarded));
        let local = Some(IpAddr::V6(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1)));
        assert!(is_secure_from_peer(local, &forwarded));

        assert!(
            is_secure_from_peer(None, &forwarded),
            "an embedder without ConnectInfo keeps the documented trusted posture"
        );
    }

    /// F-07: only a caller that names itself gets the body token.
    #[test]
    fn the_body_token_needs_the_client_opt_in() {
        let mut headers = HeaderMap::new();
        assert!(!is_command_line_client(&headers));
        headers.insert(CLIENT_HEADER, HeaderValue::from_static("CLI"));
        assert!(is_command_line_client(&headers));
        headers.insert(CLIENT_HEADER, HeaderValue::from_static("browser"));
        assert!(!is_command_line_client(&headers));
    }
}
