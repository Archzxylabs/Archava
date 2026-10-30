//! The recovery page — the authorized answer to "did it record?".
//!
//! `GET /crm/commands/status` answers `committed` with the receipt, or
//! `unconfirmed` — and the contract's own rule is what this page's copy
//! carries: absence is not proof of failure. A key the server has never
//! heard of and a key committed-but-unreadable are ONE unconfirmed answer,
//! so re-submitting the SAME command is safe either way.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_query_map;

use crate::session::{self, Answer, FetchError};

/// The page's possible states.
#[derive(Clone)]
enum RecoverState {
    Idle,
    Busy,
    Committed(Answer),
    Unconfirmed { request_id: String },
    Refused { reason: String, message: String },
    Unreachable(String),
}

/// The `/recover` route. It answers whatever id the caller brings — its own
/// lost response, a teammate's, a browser restart's. An `?id=` in the URL
/// prefills the field (the lost-response flow builds the link from the
/// draft's request id before the browser could see any answer).
#[component]
pub fn Recover() -> impl IntoView {
    let query = use_query_map();
    let prefilled = query.get_untracked().get("id").unwrap_or_default();
    let request_id = RwSignal::new(prefilled);
    let (state, set_state) = signal(RecoverState::Idle);

    let run_lookup = move |request_id: String| {
        let request_id = request_id.trim().to_string();
        if request_id.is_empty() {
            set_state.set(RecoverState::Refused {
                reason: "empty_request_id".to_string(),
                message: "Paste the request id the receipt panel carried (or a draft's own)."
                    .to_string(),
            });
            return;
        }
        set_state.set(RecoverState::Busy);
        spawn_local(async move {
            let tenant = session::held().map(|held| held.tenant).unwrap_or_default();
            let target = format!("/crm/commands/status?tenant={tenant}&requestId={request_id}");
            match session::api_fetch("GET", &target, None).await {
                Ok(Answer { status: 200, body })
                    if crate::receipt::crm(&body, &request_id, None) =>
                {
                    // The outcome this browser could not see is now KNOWN,
                    // and it is the pending operation's own key: the marker
                    // has served its purpose and must not keep blocking the
                    // workspace's next attempt (C05). A receipt for a
                    // DIFFERENT request id leaves the marker alone.
                    if let Some(held) = session::held() {
                        crate::pending::clear_by_request_id(
                            &held.tenant,
                            &held.principal,
                            &request_id,
                        );
                    }
                    set_state.set(RecoverState::Committed(Answer { status: 200, body }));
                }
                Ok(Answer { status: 200, body }) if body["status"] == "unconfirmed" => {
                    set_state.set(RecoverState::Unconfirmed {
                        request_id: body["requestId"]
                            .as_str()
                            .unwrap_or(&request_id)
                            .to_string(),
                    });
                }
                Ok(Answer { status, body }) if (400..500).contains(&status) => {
                    set_state.set(RecoverState::Refused {
                        reason: body["reason"]
                            .as_str()
                            .unwrap_or(&format!("http {status}"))
                            .to_string(),
                        message: body["message"]
                            .as_str()
                            .unwrap_or("The recovery read was refused.")
                            .to_string(),
                    })
                }
                Ok(_) => set_state.set(RecoverState::Unreachable(
                    "A matching receipt was not received. Keep this reference and check again."
                        .into(),
                )),
                Err(FetchError::NoAnswer(why)) => set_state.set(RecoverState::Unreachable(why)),
            }
        });
    };

    // An `?id=` the caller brings is an OPEN recovery: the page answers it
    // without demanding a click. The lookup runs post-render (effects run
    // after the initial render), untracked — once per mount, only when the
    // panel is still untouched.
    Effect::new(move |_| {
        let prefill = request_id.get_untracked();
        let untouched = matches!(state.get_untracked(), RecoverState::Idle);
        if !prefill.trim().is_empty() && untouched {
            run_lookup(prefill);
        }
    });

    let recover = move |_event: leptos::ev::MouseEvent| run_lookup(request_id.get_untracked());

    view! {
        <section class="recover">
            <h1 data-testid="recover-heading">"Recover a receipt"</h1>
            <p>
                "Answers with the recorded receipt when the command committed — "
                "and with “unconfirmed” when the key has no visible committed row "
                "(absence is not proof of failure)."
            </p>
            <div class="recover-form">
                <label>
                    "Request id "
                    <input
                        data-testid="recover-request-id"
                        value=request_id
                        on:input=move |ev| request_id.set(event_target_value(&ev))
                    />
                </label>
                <button data-testid="recover-submit" on:click=recover>"Look up"</button>
            </div>
            {move || match state.get() {
                RecoverState::Idle => view! {
                    <p class="status" data-testid="recover-status"></p>
                }
                .into_any(),
                RecoverState::Busy => view! {
                    <p class="status" data-testid="recover-status">"Looking up…"</p>
                }
                .into_any(),
                RecoverState::Committed(answer) => view! {
                    <p class="receipt" data-testid="recover-status">
                        "Committed. Receipt: "
                        <code>{answer.body["receipt"].to_string()}</code>
                        <span data-testid="recover-receipt-request-id">{answer.body["requestId"].to_string()}</span>
                    </p>
                }
                .into_any(),
                RecoverState::Unconfirmed { request_id } => view! {
                    <p class="status" data-testid="recover-status">
                        {format!(
                            "Unconfirmed ({request_id}): absent is not proof of failure — \
                             re-submitting the SAME command under the SAME key is safe."
                        )}
                    </p>
                }
                .into_any(),
                RecoverState::Unreachable(why) => view! {
                    <p class="refusal" data-testid="recover-status">{why}</p>
                }
                .into_any(),
                RecoverState::Refused { reason, message } => view! {
                    <p class="refusal" data-testid="recover-status">
                        <strong>"Refused: "</strong>
                        <code>{reason}</code>" — "{message}
                    </p>
                }
                .into_any(),
            }}
        </section>
    }
}
