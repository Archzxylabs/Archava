//! The login screen — the slice flow's front door.
//!
//! The form posts the host's REAL `/login` route (the same one the integration
//! suites prove); nothing about authority is decided here. On success the
//! host's response carries the CSRF value and has already set the HttpOnly
//! session cookie — the script records the CSRF value and who/where for the
//! header marker, then walks into `/contacts`. A refusal renders the server's
//! single reason verbatim: an invalid password, a busy store and a deactivated
//! account are distinguishable facts, and the UI must not blur them.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;

use crate::session::{self, Held};

/// The `/` route: explains what the slice does and signs people in.
#[component]
pub fn Login() -> impl IntoView {
    // The session facts restore in `App` (post-render, one effect) — the
    // login page itself reads the restored cell through normal reactivity.
    let username = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    // The tenant choice the flow admits: prefilled, editable, and checked by
    // the server on every route it touches — picking a tenant the grants do
    // not reach is a refusal this screen will render.
    let tenant = RwSignal::new(session::DEFAULT_TENANT.to_string());
    let (state, set_state) = signal(LoginState::Idle);
    let navigate = use_navigate();

    // A held session does not need this screen: a reload that lands here
    // with facts restores straight into the flow. The read is tracked — the
    // restore in `App` runs first (creation order) and the change it makes
    // is what fires this navigation — so an SSR-rendered page never
    // navigates (it has no facts) and a signed-out visit never loops.
    let restore_navigate = navigate.clone();
    let session = expect_context::<crate::session::SessionSignal>();
    Effect::new(move |_| {
        if session.get().is_some() {
            restore_navigate("/app/contacts", Default::default());
        }
    });

    let on_submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        let username = username.get_untracked();
        let password = password.get_untracked();
        let tenant = tenant.get_untracked();
        let navigate = navigate.clone();
        spawn_local(async move {
            if username.is_empty() || password.is_empty() || tenant.is_empty() {
                set_state.set(LoginState::FillEverything);
                return;
            }
            let effective_username = if !username.contains('@') {
                format!("{username}@archava.test")
            } else {
                username.clone()
            };
            set_state.set(LoginState::Busy);
            let body = serde_json::json!({ "username": effective_username, "password": password });
            match session::api_fetch("POST", "/login", Some(body.to_string())).await {
                Ok(answer) if answer.status == 200 => {
                    let Some(csrf) = answer.body["csrf"].as_str().map(str::to_owned) else {
                        set_state.set(LoginState::Broken);
                        return;
                    };
                    session::write_session_facts(&csrf, &tenant, &effective_username);
                    session::persist_session(Held {
                        csrf,
                        tenant: tenant.clone(),
                        principal: effective_username,
                    });
                    navigate("/app/contacts", Default::default());
                    set_state.set(LoginState::Idle);
                }
                Ok(answer) => set_state.set(LoginState::Refused {
                    reason: answer.reason().unwrap_or("unknown_refusal").to_string(),
                    message: answer
                        .message()
                        .unwrap_or("The sign-in was refused.")
                        .to_string(),
                }),
                Err(error) => set_state.set(LoginState::Unreachable(error.to_string())),
            }
        })
    };

    view! {
        <section class="login">
            <h1 data-testid="login-heading">"Sign in"</h1>
            <p class="login-subheading">"Your contacts, follow-ups, and daily work in one place."</p>
            <form on:submit=on_submit>
                <label>
                    "Username"
                    <input
                        data-testid="login-username"
                        placeholder="name@archava.test"
                        value=username
                        autocomplete="username"
                        on:input=move |ev| username.set(event_target_value(&ev))
                    />
                </label>
                <label>
                    "Password"
                    <input
                        data-testid="login-password"
                        type="password"
                        placeholder="••••••••••••••••"
                        autocomplete="current-password"
                        on:input=move |ev| password.set(event_target_value(&ev))
                    />
                </label>
                <label>
                    "Tenant"
                    <input
                        data-testid="login-tenant"
                        placeholder="acme-retail"
                        value=tenant
                        autocomplete="off"
                        on:input=move |ev| tenant.set(event_target_value(&ev))
                    />
                </label>
                <button type="submit" data-testid="login-submit">"Sign in →"</button>
            </form>
            {move || match state.get() {
                LoginState::Idle => view! { <p class="status" data-testid="login-status"></p> }.into_any(),
                LoginState::Busy =>
                    view! { <p class="status" data-testid="login-status">"Signing in…"</p> }.into_any(),
                LoginState::FillEverything =>
                    view! { <p class="refusal" data-testid="login-status">"Fill in both fields."</p> }.into_any(),
                LoginState::Refused { reason, message } => view! {
                    <p class="refusal" data-testid="login-status">
                        <strong>"Refused: "</strong>
                        <code>{reason}</code>" — "{message}
                    </p>
                }
                .into_any(),
                LoginState::Unreachable(why) => view! {
                    <p class="refusal" data-testid="login-status">{why}</p>
                }
                .into_any(),
                LoginState::Broken => view! {
                    <p class="refusal" data-testid="login-status">
                        "The answered body was not the login contract the host documents."
                    </p>
                }
                .into_any(),
            }}
            {
                // The fixture cast, so a human driving this by hand knows the
                // accounts that exist. The BROWSER PROOF exercises bob's
                // missing-write-grant path through this very cast.
                view! { <p class="hint" data-testid="login-cast">
                    "Fixture accounts: alice, bob, carol, dave (or @archava.test) — password: correct horse battery staple"
                </p> }
            }
        </section>
    }
}

/// The login page's transient states, exactly as the statuses above render.
#[derive(Clone, Debug)]
enum LoginState {
    Idle,
    Busy,
    FillEverything,
    Refused { reason: String, message: String },
    Unreachable(String),
    Broken,
}
