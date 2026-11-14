#![recursion_limit = "512"]
//! The §1.4B vertical slice — the one real product flow, rendered server-side
//! and hydrated in the browser.
//!
//! ## What this program is
//!
//! A Leptos application whose single job is to make the slice flow *visible*:
//! sign in, see the contacts the session's CRM grants admit, draft a follow-up
//! against a real contact revision, review what will be recorded, submit, and
//! recover the receipt. Nothing here is mock data — every fact the UI shows
//! arrives from the host's own HTTP routes (`/login`, `/contacts`,
//! `/crm/commands`, `/crm/commands/status`), the same surface the integration
//! suites prove [`archava_host`-side](archava_host); the browser decides
//! nothing about authority, because the server re-derives it on every request.
//!
//! ## What hydration buys here
//!
//! The SSR pass renders the shell, the login screen and the blocking copy —
//! a document a browser can paint before any script runs. The hydrate build
//! (`src/hydrate.rs`, the `wasm32` target) then takes the same tree's boots:
//! interactive state (the session, the contact page, the draft) is fetched
//! after hydration, from the session cookie the login route set and the CSRF
//! value the login body carried. A page refresh therefore shows the shell
//! immediately and refills data — the recovery path the browser proof leans
//! on.
//!
//! ## Why the views live in one crate with two targets
//!
//! The same `App` is compiled twice: with the `ssr` feature for the server
//! (`examples/slice_server.rs`) and with `hydrate` for the browser. The
//! compiler guarantees the markup the server emits is the markup the client
//! expects, because it is one program; a shared-views-two-crates layout would
//! let them drift silently.

use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;

mod agent;
mod alive;
pub mod contact;
mod contact_context;
pub mod contacts;
pub mod desk;
pub mod draft;
pub mod erp;
pub mod home;
/// The browser's entry point. Only the wasm compilation exports it; the SSR
/// build never renders a script-side `hydrate`.
#[cfg(target_arch = "wasm32")]
pub mod hydrate;
pub mod login;
pub mod operations;
/// C03: the owner-configuration surface.
pub mod owner;
pub mod pending;
mod receipt;
pub mod recover;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;
pub mod session;
pub mod workspace;

use contact::Contact;
use contacts::Contacts;
use desk::Desk;
use draft::Draft;
use erp::ErpCockpit;
use login::Login;
use owner::Owner;
use recover::Recover;

/// The exact header the host's CSRF gate reads
/// (`CSRF_HEADER` in the host's router; the value it echoes comes from the
/// login body).
pub const CSRF_HEADER: &str = "x-archava-csrf";

/// The response the host's routes answer with when the boundary refused a
/// body — single-reason, from `rejection()`/`query_rejection()`.
pub const MALFORMED_SUBMISSION: &str = "malformed_submission";

/// The whole application: header, route table — the BODY CONTENT only. The
/// document shell (doctype, head, hydration scripts) lives in the shared
/// server module, because the browser's hydrate pass binds to the body the
/// server rendered: an App-owned document would nest one html tree inside
/// another (the doubled markup the first serving run printed).
#[component]
pub fn App() -> impl IntoView {
    session::provide_session();
    // The facts restore AFTER the first render, not before: hydration-time
    // islands adopt the SSR text they find and only rewrite on subsequent
    // reactive changes — the SSR pass cannot see the session (it lives in the
    // browser's cookie and storage), so a PRE-render restore would make the
    // restored value the island's BASELINE, no change would ever fire, and a
    // signed-in reload would display `signed out` forever. From `None` at
    // first render to `Some` in this effect is a real change every island
    // rewrites.
    Effect::new(move |_| session::restore_session());
    provide_context(agent::AgentDraft(RwSignal::new(None)));

    view! {
        // The router wraps the WHOLE body, header included: the session
        // marker owns a sign-out action that navigates when the server has
        // closed the session, and a hook that needs the router context
        // panics outside it ("You cannot call `use_navigate` outside a
        // <Router>"). The shared workspace shell is rendered by both SSR and
        // hydration, so navigation and appearance controls adopt the same tree.
        <Router>
            <workspace::Workspace>
                <Routes fallback=|| view! { <NotFound /> }>
                    <Route path=path!("app") view=Login />
                    <Route path=path!("app/home") view=home::Home />
                    <Route path=path!("app/erp") view=ErpCockpit />
                    <Route path=path!("app/contacts") view=Contacts />
                    <Route path=path!("app/contact") view=Contact />
                    <Route path=path!("app/desk") view=Desk />
                    <Route path=path!("app/draft") view=Draft />
                    <Route path=path!("app/owner") view=Owner />
                    <Route path=path!("app/recover") view=Recover />
                </Routes>
            </workspace::Workspace>
        </Router>
    }
}

/// An unknown path: 404 copy without pretending anything about authority.
#[component]
fn NotFound() -> impl IntoView {
    view! {
        <section>
            <h1>"Nothing here"</h1>
            <p>"This screen does not exist. Use the navigation above."</p>
        </section>
    }
}
