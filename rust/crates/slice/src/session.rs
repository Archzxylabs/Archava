//! The browser session — how the UI holds what login delivered, and how it
//! talks to the host's routes.
//!
//! The host's login route records the token in an `archava_session` HttpOnly
//! cookie and delivers the CSRF value in the response body. Two consequences
//! shape this module:
//!
//! - the token is never readable by script, and the business fetches carry no
//!   `Authorization` header at all: same-origin requests attach the cookie,
//!   and the host's session resolution builds on it. A compromised page
//!   cannot read the token out of storage because it was never in storage;
//! - the CSRF value IS persisted (`localStorage`) because the mutation gate's
//!   contract is a header script must be able to echo — that is the
//!   double-submit header, not a leak. A `csrf_required` refusal after a
//!   storage clear names the fix: sign in again.
//!
//! Every business fetch goes to the host's own routes. This module wraps the
//! raw `fetch` plumbing once and returns plain `(status, body)` answers, so
//! the views refuse or render exactly what the server said.
//!
//! The wasm/server split is structural: the browser-only pieces exist only in
//! the hydrate build; the SSR build answers with a "not a browser" failure
//! the views never reach, because interactive fetches are event-driven.

#[cfg(target_arch = "wasm32")]
use std::sync::OnceLock;

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;

/// The outcome of a sign-out attempt, as the marker renders it.
#[derive(Clone, Debug)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) enum SignOut {
    /// The server closed the session; this browser's private state may be
    /// cleared.
    Closed,
    /// The server refused (no session, a CSRF refusal, or the identity store
    /// was unavailable): the local state is KEPT. A UI that claims to be
    /// signed out while the credential still answers is the very defect this
    /// milestone removes, wearing a different hat.
    Refused { reason: String, message: String },
}

/// Ask the server to close this session.
///
/// The server does the revoking: the session row is the authority, and until
/// it is gone the credential still reads and writes. The browser's own state
/// clearing is the LAST step, after the server says so — the same order the
/// host's logout route uses, so the two halves cannot disagree about who
/// closed the session.
#[cfg(target_arch = "wasm32")]
pub(crate) async fn close_server_session() -> SignOut {
    match api_fetch("POST", "/logout", None).await {
        Ok(answer) if answer.status == 200 => SignOut::Closed,
        // "There is no session to close" is ALSO a clean state: the server
        // does not know this credential, so there is nothing left to
        // protect, and a browser that keeps showing a session the server has
        // forgotten is a phantom. A REFUSED logout (403 — a CSRF failure, or
        // 503 — the identity store is unavailable) is different: a session
        // may well still be live, and only the server can say so.
        Ok(answer) if answer.status == 401 => SignOut::Closed,
        Ok(answer) => {
            let reason = answer
                .body
                .get("reason")
                .and_then(|value| value.as_str())
                .unwrap_or("refused")
                .to_string();
            let message = answer
                .body
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("The session could not be closed.")
                .to_string();
            SignOut::Refused { reason, message }
        }
        Err(error) => SignOut::Refused {
            reason: "no_answer".to_string(),
            message: format!("The session could not be closed: {error}"),
        },
    }
}

/// The SSR build never signs out: it has no session and no browser to clear.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn close_server_session() -> SignOut {
    SignOut::Refused {
        reason: "not_a_browser".to_string(),
        message: "Signing out needs the browser.".to_string(),
    }
}

/// The exact header name the host's CSRF gate reads.
///
/// Compiled on the server as well (the shared view tree is one program), but
/// only the wasm fetch plumbing ever reads it.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) const CSRF_HEADER: &str = "x-archava-csrf";

/// What a successful login left behind (browser-side).
#[derive(Clone, Debug)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) struct Held {
    /// The value the login body carried; echoed on every mutation.
    pub csrf: String,
    /// The tenant this session opened on.
    pub tenant: String,
    /// The principal who signed in, for the header marker.
    pub principal: String,
}

/// The one reactive cell the views read; `None` until a login completes.
pub(crate) type SessionSignal = RwSignal<Option<Held>>;

/// The single cell, created by [`App`](crate::App) inside the reactive root
/// and readable by the fetch plumbing afterwards.
///
/// Browser-only by nature: the hydrate build runs ONE reactive root that
/// never disposes, so one lazily-created signal is a singleton. The SSR build
/// MUST NOT share one — its owner is per-request, and a second request would
/// read the first request's already-disposed value (the panic the first
/// serving run proved). See [`provide_session`] for the per-request branch.
#[cfg(target_arch = "wasm32")]
fn session_cell() -> SessionSignal {
    static CELL: OnceLock<SessionSignal> = OnceLock::new();
    *CELL.get_or_init(|| RwSignal::new(None))
}

/// Provide the session cell to the view tree; called once from `App`.
///
/// The server branch creates a FRESH signal for every request — the SSR
/// owner-tree is per-request, so a shared static would be, by request two,
/// a disposed value read by a live closure.
#[cfg(target_arch = "wasm32")]
pub(crate) fn provide_session() {
    provide_context(session_cell());
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn provide_session() {
    provide_context::<SessionSignal>(RwSignal::new(None));
}

/// The server build's no-ops: login handlers only run in a browser.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn persist_session(_held: Held) {}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn forget_session() {}

/// Write a session into the cell after a successful login.
#[cfg(target_arch = "wasm32")]
pub(crate) fn persist_session(held: Held) {
    session_cell().set(Some(held));
}

/// Drop the session (a refusal `csrf_required` after a storage clear — the
/// UI walks the user back to the login flow).
#[cfg(target_arch = "wasm32")]
pub(crate) fn forget_session() {
    session_cell().set(None);
}

/// The session, untracked — for fetch plumbing inside effects.
#[cfg(target_arch = "wasm32")]
pub(crate) fn held() -> Option<Held> {
    session_cell().get_untracked()
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn held() -> Option<Held> {
    None
}

/// Hydration-time restoration. The HttpOnly cookie restores authentication
/// across a refresh and a restart; this restores the celebrated facts the
/// views also need (csrf header echo, tenant, principal name).
#[cfg(target_arch = "wasm32")]
pub(crate) fn restore_session() {
    let Some(storage) = window_storage() else {
        return;
    };
    let csrf = storage
        .get_item("archava_csrf")
        .ok()
        .flatten()
        .unwrap_or_default();
    let tenant = storage
        .get_item("archava_tenant")
        .ok()
        .flatten()
        .unwrap_or_default();
    let principal = storage
        .get_item("archava_principal")
        .ok()
        .flatten()
        .unwrap_or_default();
    if !csrf.is_empty() && !tenant.is_empty() && !principal.is_empty() {
        persist_session(Held {
            csrf,
            tenant,
            principal,
        });
    }
}

/// The server render holds no session facts: the SSR shell renders the signed-
/// out view and hydration refills it.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn restore_session() {}

/// The tenant the login page PRE-FILLS — the checked choice, not the
/// authority: the server refuses any tenant the session's CRM grants do not
/// reach, exactly as it refuses one that never existed. The operator types
/// another tenant in the field to walk the refusal through the visible UI.
pub(crate) const DEFAULT_TENANT: &str = "acme-retail";

/// Persist the celebrated facts a login delivered (csrf/tenant/principal).
/// The HttpOnly cookie was the LOGIN RESPONSE's job; these are the values
/// script is contractually allowed to know.
#[cfg(target_arch = "wasm32")]
pub(crate) fn write_session_facts(csrf: &str, tenant: &str, principal: &str) {
    let Some(storage) = window_storage() else {
        return;
    };
    let _ = storage.set_item("archava_csrf", csrf);
    let _ = storage.set_item("archava_tenant", tenant);
    let _ = storage.set_item("archava_principal", principal);
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_session_facts(_csrf: &str, _tenant: &str, _principal: &str) {}

/// Drop the stored facts (a `csrf_required` refusal after a storage clear).
#[cfg(target_arch = "wasm32")]
pub(crate) fn clear_session_facts() {
    if let Some(storage) = window_storage() {
        let _ = storage.remove_item("archava_csrf");
        let _ = storage.remove_item("archava_tenant");
        let _ = storage.remove_item("archava_principal");
    }
    // The pending markers are this ACCOUNT's private recovery state: the
    // account just left, so they leave with it. A signed-out browser must
    // not keep another account's operation reachable (C05).
    crate::pending::clear_all();
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn clear_session_facts() {}

/// The browser's `localStorage`, when there is a browser.
#[cfg(target_arch = "wasm32")]
fn window_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// One host answer: the status code and the parsed body. `status` is exactly
/// what the route answered; the views map it to refusals or receipts.
#[derive(Clone, Debug)]
pub(crate) struct Answer {
    pub status: u16,
    pub body: serde_json::Value,
}

impl Answer {
    /// The refusal's fixed reason string, when the answer carries one.
    pub(crate) fn reason(&self) -> Option<&str> {
        self.body["reason"].as_str()
    }

    /// The refusal's message string, when the answer carries one.
    pub(crate) fn message(&self) -> Option<&str> {
        self.body["message"].as_str()
    }
}

/// A fetch the browser could not complete: no answer EXISTS to trust.
#[derive(Debug)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) enum FetchError {
    /// The request never completed, or the answer carried no bytes.
    NoAnswer(String),
}

impl core::fmt::Display for FetchError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let FetchError::NoAnswer(why) = self;
        write!(
            formatter,
            "the browser could not complete the request: {why}"
        )
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn api_fetch(
    method: &'static str,
    path: &str,
    body: Option<String>,
) -> Result<Answer, FetchError> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let init = web_sys::RequestInit::new();
    init.set_method(method);
    if let Some(body) = body.as_deref() {
        init.set_body(&wasm_bindgen::JsValue::from_str(body));
    }
    let request = web_sys::Request::new_with_str_and_init(path, &init)
        .map_err(|value| FetchError::NoAnswer(js_string(&value)))?;
    request
        .headers()
        .set("content-type", "application/json")
        .expect("content-type is a fixed, valid header name");
    if let Some(csrf) = held().filter(|_| method != "GET").map(|held| held.csrf) {
        request
            .headers()
            .set(CSRF_HEADER, &csrf)
            .expect("the csrf header name is fixed and valid");
    }

    let window = web_sys::window().ok_or_else(|| FetchError::NoAnswer("no window".into()))?;
    let response = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|value| FetchError::NoAnswer(js_string(&value)))?
        .dyn_into::<web_sys::Response>()
        .expect("fetch always resolves with a Response");
    let status = response.status();
    let text = JsFuture::from(
        response
            .text()
            .map_err(|value| FetchError::NoAnswer(js_string(&value)))?,
    )
    .await
    .map_err(|value| FetchError::NoAnswer(js_string(&value)))?
    .as_string()
    .unwrap_or_default();
    let body: serde_json::Value = if text.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(&text).map_err(|why| {
            FetchError::NoAnswer(format!("the answer is not the JSON contract: {why}"))
        })?
    };
    Ok(Answer { status, body })
}

/// The SSR build never serves data: every business fetch is event-driven in
/// the browser, and this branch exists so the shared view code compiles.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn api_fetch(
    _method: &'static str,
    _path: &str,
    _body: Option<String>,
) -> Result<Answer, FetchError> {
    unreachable!("business fetches are browser events; the SSR shell fetches nothing")
}

#[cfg(target_arch = "wasm32")]
fn js_string(value: &wasm_bindgen::JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| "opaque value".to_string())
}

/// The header one line below shows the signed-in principal and tenant — or
/// the signed-out state, and a link to the login flow. When a session IS
/// held, the marker also carries the sign-out action (a page-level state
/// clear; the server cookie revocation is a later slice item, so the sign-out
/// names what it does exactly).
#[component]
pub(crate) fn SessionMarker() -> impl IntoView {
    let session = expect_context::<SessionSignal>();
    // Why the last sign-out did not close the session, if it did not. Empty
    // means "nothing to report" — the marker must not grow a permanent
    // element for it, or the SSR/hydration node shape would drift. The
    // reason travels as a machine-readable attribute so the browser proof
    // can assert WHICH refusal happened, not merely that text appeared.
    let refusal = RwSignal::new(String::new());
    let refusal_reason = RwSignal::new(String::new());
    let navigate = use_navigate();
    // Shape-stable across the SSR/hydrate boundary: the SSR pass cannot see
    // the session facts (they live in the browser's cookie and storage), so
    // its render — but this reload would render — the marker with the facts
    // restored. A branch on `held` would produce a different NODE SHAPE on
    // the hydrate side than the server rendered, and tachys's hydration
    // would panic on the mismatch (`failed_to_cast_element`) — a full page
    // reload while signed in would never come back. The node spine is
    // therefore constant and only the TEXT island tracks the session.
    view! {
        <span class="session" data-testid="session-marker">
            {move || {
                session
                    .get()
                    .map_or_else(
                        || "signed out".to_owned(),
                        |held| format!("{} @ {}", held.principal, held.tenant),
                    )
            }}
            <button
                class="sign-out"
                data-testid="session-sign-out"
                on:click=move |_event| {
                    // The router's `navigate` is moved into the async block
                    // below, so the click handler hands it over by clone —
                    // the same shape the other views use.
                    let navigate = navigate.clone();
                    spawn_local(async move {
                        // The server closes the session; only then does this
                        // browser forget what it held. A refusal leaves the
                        // session in place and says why — pretending to be
                        // signed out while the credential still works is the
                        // defect the audit reproduced (C02).
                        match crate::session::close_server_session().await {
                            crate::session::SignOut::Closed => {
                                crate::session::forget_session();
                                crate::session::clear_session_facts();
                                refusal.set(String::new());
                                refusal_reason.set(String::new());
                                navigate("/app", Default::default());
                            }
                            crate::session::SignOut::Refused { reason, message } => {
                                refusal.set(message);
                                refusal_reason.set(reason);
                            }
                        }
                    });
                }
            >
                "sign out"
            </button>
            <span
                class="sign-out-refusal"
                data-testid="sign-out-refusal"
                data-reason=move || refusal_reason.get()
            >
                {move || refusal.get()}
            </span>
        </span>
    }
}
