//! The draft page — review, then submit the one real command.
//!
//! The form carries the bind the command makes: the contact revision the
//! draft expects. It is an editable field on purpose — the stale-revision
//! scenario is NOT a special error page, it is the user typing a revision a
//! concurrent edit already moved past, and the refusal the server answers
//! with (`revision_changed`) is exactly what the UI shows.
//!
//! The request id is minted once per page load (`crypto.randomUUID`);
//! submitting the unchanged draft again therefore REPLAYS the idempotency key
//! — the duplicate-submission scenario — while the "new attempt" button mints
//! a fresh key (the same draft, a next possible effect).

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_query_map;

use crate::pending::{self, PendingMarker};
use crate::session::{Answer, FetchError};

/// The page's possible states, rendered by the one status block below.
#[derive(Clone)]
enum DraftState {
    /// The form, unsubmitted.
    Idle,
    Busy,
    /// Stored: the server answered with the receipt, and it stays on screen.
    Recorded(Answer),
    /// The SAME command submitted a second time under the SAME key.
    Replayed(Answer),
    /// The bind failed: this draft's expected revision is not the current one.
    Stale(Answer),
    /// The transport died mid-flight: the recording may have happened — the
    /// recovery screen answers, this page does not guess.
    Unreachable(String),
    /// The browser could not record the pending marker, so the submission
    /// was NOT made. A send the application could not recover is the defect
    /// the marker exists to remove; refusing to send is the honest answer.
    Unmarkable(String),
    /// A previous submission's outcome is still unknown, so a FRESH
    /// idempotency key is refused: it would run the operation a second time
    /// under another key, which the server would treat as new work.
    Unresolved,
    /// Some other refusal, verbatim from the host.
    Refused {
        reason: String,
        message: String,
    },
}

/// The `/draft?contact=<uuid>&revision=<n>&name=<name>` route.
#[component]
pub fn Draft() -> impl IntoView {
    let query = use_query_map();
    let query_map = query.get_untracked();
    let contact = query_map.get("contact").unwrap_or_default();
    let contact_name = query_map.get("name").unwrap_or_default();
    let bound_revision = query_map
        .get("revision")
        .and_then(|revision| revision.parse::<i64>().ok())
        .unwrap_or(0);

    let revision_field = RwSignal::new(bound_revision);
    let summary = RwSignal::new(String::new());
    let due_at = RwSignal::new("2027-03-04T10:15:00Z".to_string());
    // The request id is the browser's OWN randomness: neither the SSR pass
    // nor the hydrate morph may carry one (the server would claim origin it
    // does not have). Both start empty — identical text and node shapes —
    // and the id is minted after the first render lands, when the browser
    // side owns the screen.
    let request_id = RwSignal::new(String::new());
    // The pending marker this workspace holds, if any — the application's own
    // memory of a submission whose outcome it does not know. It is restored
    // BEFORE any id is minted: a reload of a draft whose outcome is unknown
    // must come back with the ORIGINAL key. The 19 Oktober audit reloaded
    // exactly that page and watched the request id change, with the original
    // nowhere in the tab, the URL or storage.
    let pending_marker = RwSignal::new(None::<PendingMarker>);
    // Which (workspace, account) the marker was restored for, so a workspace
    // or account change restores THAT one's marker instead of keeping the
    // previous one.
    let restored_for = RwSignal::new(None::<(String, String)>);
    let session = expect_context::<crate::session::SessionSignal>();
    Effect::new(move |_| {
        // The effect depends on the session: the marker is account/workspace
        // bound, so it can only be restored once the session's facts are
        // known — before that, "no marker" would be a guess.
        let Some(held) = session.get() else {
            return;
        };
        if restored_for.get_untracked().as_ref()
            == Some(&(held.tenant.clone(), held.principal.clone()))
        {
            return;
        }
        restored_for.set(Some((held.tenant.clone(), held.principal.clone())));
        match pending::read(&held.tenant, &held.principal) {
            Some(marker) => {
                pending_marker.set(Some(marker.clone()));
                request_id.set(marker.request_id);
            }
            None => {
                pending_marker.set(None);
                request_id.set(uuid_string());
            }
        }
    });
    // The tenant the draft was authored under — the page's own copy,
    // captured after the restore lands the session facts. Sign-out after a
    // draft is authored does not un-author its body: the submit still
    // reaches the host's CSRF gate (exactly the boundary a signed-out,
    // header-less submit must hit) and the gate — not the UI — decides what
    // the missing header means.
    let draft_tenant = RwSignal::new(String::new());
    Effect::new(move |_| {
        if draft_tenant.get_untracked().is_empty() {
            if let Some(held) = crate::session::held() {
                draft_tenant.set(held.tenant);
            }
        }
    });
    let (state, set_state) = signal(DraftState::Idle);

    let submit_contact = contact.clone();
    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();

        let summary = summary.get_untracked();
        let contact_revision = revision_field.get_untracked();
        let minted = request_id.get_untracked();
        let due_at = due_at.get_untracked();
        let contact = submit_contact.clone();

        if summary.trim().is_empty() {
            set_state.set(DraftState::Refused {
                reason: "empty_summary".to_string(),
                message: "A follow-up needs a summary (1–2000 characters).".to_string(),
            });
            return;
        }

        // Whether a receipt is already on screen decides how a second 200
        // rendering: the SAME command under the SAME key is a replay. Read
        // the fact BEFORE the busy write — after `Busy` the answer can never
        // carry it, and every replay would re-render as a fresh record.
        let was_recorded = matches!(
            state.get_untracked(),
            DraftState::Recorded(_) | DraftState::Replayed(_)
        );
        set_state.set(DraftState::Busy);
        let tenant = draft_tenant.get_untracked();
        let principal = crate::session::held()
            .map(|held| held.principal)
            .unwrap_or_default();
        spawn_local(async move {
            // The pending marker goes FIRST. If the browser cannot record it,
            // the submission does not happen: a send whose key the
            // application could not keep is a send it could never recover,
            // which is the defect this marker exists to remove.
            let marker = PendingMarker::new(minted.clone(), tenant.clone(), principal.clone());
            if let Err(why) = pending::write(&marker) {
                set_state.set(DraftState::Unmarkable(why));
                return;
            }
            pending_marker.set(Some(marker));
            let body = serde_json::json!({
                "tenant": draft_tenant.get_untracked(),
                "requestId": minted,
                "command": {
                    "operation": "followup.create",
                    "contactId": contact,
                    "contactRevision": contact_revision,
                    "summary": summary,
                    "dueAt": due_at,
                },
            });
            match crate::session::api_fetch("POST", "/crm/commands", Some(body.to_string())).await {
                Ok(Answer { status: 200, body }) if was_recorded => {
                    // The outcome is KNOWN (the server replayed the stored
                    // receipt), so the pending marker has served its purpose.
                    pending::clear(&tenant);
                    pending_marker.set(None);
                    set_state.set(DraftState::Replayed(Answer { status: 200, body }));
                }
                Ok(answer @ Answer { status: 200, .. }) => {
                    pending::clear(&tenant);
                    pending_marker.set(None);
                    set_state.set(DraftState::Recorded(answer));
                }
                Ok(answer @ Answer { status: 409, .. })
                    if answer.reason() == Some("revision_changed") =>
                {
                    pending::clear(&tenant);
                    pending_marker.set(None);
                    set_state.set(DraftState::Stale(answer));
                }
                Ok(Answer { status, body }) => {
                    // A refusal is a KNOWN outcome: nothing was committed,
                    // so the marker can go.
                    pending::clear(&tenant);
                    pending_marker.set(None);
                    set_state.set(DraftState::Refused {
                        reason: body["reason"]
                            .as_str()
                            .unwrap_or(&format!("http {status}"))
                            .to_string(),
                        message: body["message"]
                            .as_str()
                            .unwrap_or("The submission was refused.")
                            .to_string(),
                    })
                }
                Err(FetchError::NoAnswer(why)) => {
                    // The outcome is UNKNOWN: the marker stays, and the
                    // recovery link carries the original key. This is the
                    // state the marker exists for — a reload must come back
                    // to THIS key, not to a fresh one.
                    set_state.set(DraftState::Unreachable(format!(
                        "{why} — recover with request id {minted}"
                    )));
                }
            }
        });
    };

    view! {
        <section class="draft">
            <h1 data-testid="draft-heading">"Draft a follow-up"</h1>
            {contact.is_empty().then(|| { view! {
                <p class="refusal" data-testid="draft-context">
                    "No contact in the URL — open this page from a contact row."
                </p>
            }})}
            <p>
                "Contact: "
                <strong data-testid="draft-contact-name">{contact_name.clone()}</strong>
                " — id " <code>{contact.clone()}</code>
            </p>
            <form on:submit=submit>
                <label>
                    "Summary "
                    <input
                        data-testid="draft-summary"
                        value=summary
                        on:input=move |ev| summary.set(event_target_value(&ev))
                    />
                </label>
                <br />
                <label>
                    "Due "
                    <input
                        data-testid="draft-due"
                        value=due_at
                        on:input=move |ev| due_at.set(event_target_value(&ev))
                    />
                </label>
                <br />
                <label>
                    "Contact revision (the bind — a moved value conflicts) "
                    <input
                        data-testid="draft-revision"
                        value=revision_field
                        on:input=move |ev| {
                            if let Ok(revision) = event_target_value(&ev).parse::<i64>() {
                                revision_field.set(revision);
                            }
                        }
                    />
                </label>
                <br />
                <p class="hint" data-testid="review-body">
                    "Review: submits followup.create under request id "
                    <code data-testid="review-request-id">{request_id}</code>
                    " in tenant "
                    <code data-testid="review-tenant">{draft_tenant}</code>
                    "."
                </p>
                // The pending marker, named: a submission whose outcome this
                // application does not know, with the key that reaches it.
                {move || {
                    pending_marker.get().map(|marker| {
                        let recover_href = format!("/app/recover?id={}", marker.request_id);
                        view! {
                            <p class="pending" data-testid="draft-pending">
                                "A submission from this workspace has an unknown outcome: request id "
                                <code data-testid="draft-pending-id">{marker.request_id.clone()}</code>
                                " — resolve it before starting another one: "
                                <a data-testid="draft-pending-recover" href=recover_href>
                                    "open recovery with this request id"
                                </a>
                            </p>
                        }
                    })
                }}
                <button type="submit" data-testid="draft-submit">"Apply"</button>
                <button
                    type="button"
                    data-testid="draft-new-key"
                    on:click=move |_event| {
                        // A FRESH key while an operation's outcome is unknown
                        // would run that operation a second time under
                        // another key — and the server would treat it as new
                        // work. The pending operation has to be resolved
                        // first, with its own key.
                        if pending_marker.get_untracked().is_some() {
                            set_state.set(DraftState::Unresolved);
                            return;
                        }
                        // A fresh key starts a fresh attempt: the state the
                        // previous response produced belongs to the PREVIOUS
                        // idempotency key, and keeping it on screen would
                        // attribute its receipt to the new one.
                        request_id.set(uuid_string());
                        set_state.set(DraftState::Idle);
                    }
                >
                    "New attempt (new idempotency key)"
                </button>
            </form>
            {move || match state.get() {
                DraftState::Idle => view! {
                    <p class="status" data-testid="draft-status"></p>
                }
                .into_any(),
                DraftState::Busy => view! {
                    <p class="status" data-testid="draft-status">"Submitting…"</p>
                }
                .into_any(),
                DraftState::Unmarkable(why) => view! {
                    <p class="refusal" data-testid="draft-status">
                        <strong>"The submission was not made. "</strong>
                        {why}
                        " — a send this browser could not record is a send
                         the application could never recover."
                    </p>
                }
                .into_any(),
                DraftState::Unresolved => view! {
                    <p class="refusal" data-testid="draft-status">
                        <strong>"A previous submission is unresolved. "</strong>
                        "Resolve it with its recovery link first — a fresh key
                         would run the operation again."
                    </p>
                }
                .into_any(),
                DraftState::Unreachable(why) => {
                    let recover_href =
                        format!("/app/recover?id={}", request_id.get_untracked());
                    view! {
                        <p class="refusal" data-testid="draft-status">
                            {why}
                            " — "
                            <a data-testid="unreachable-recover" href=recover_href>
                                "open recovery with this request id"
                            </a>
                        </p>
                    }
                }
                .into_any(),
                DraftState::Replayed(answer) => view! {
                    <p class="receipt replayed" data-testid="draft-status">
                        "Replayed (the draft was already recorded): "
                        <code>{answer.body["receipt"].to_string()}</code>
                    </p>
                }
                .into_any(),
                DraftState::Recorded(answer) => view! {
                    <p class="receipt" data-testid="draft-status">
                        "Recorded. Receipt: "
                        <code>{answer.body["receipt"].to_string()}</code>
                        " — recover it from "
                        <a data-testid="recover-link" href="/app/recover">"the recovery page"</a>
                        " with request id "
                        <code>{request_id.get_untracked()}</code> "."
                    </p>
                }
                .into_any(),
                DraftState::Stale(answer) => view! {
                    <p class="refusal" data-testid="draft-status">
                        <strong>"Conflict: the revision this draft bound to moved. "</strong>
                        {answer.message().unwrap_or("Re-read and try again.")}
                    </p>
                }
                .into_any(),
                DraftState::Refused { reason, message } => view! {
                    <p class="refusal" data-testid="draft-status">
                        <strong>"Refused: "</strong>
                        <code>{reason}</code>" — "{message}
                    </p>
                }
                .into_any(),
            }}
        </section>
    }
}

/// A uuid for the idempotency key, from the browser's own randomness; the
/// page invents ids no other way.
#[cfg(target_arch = "wasm32")]
fn uuid_string() -> String {
    let window = web_sys::window().expect("a browser window");
    let crypto = window.crypto().expect("crypto on the browser");
    crypto.random_uuid()
}

/// The server-side variant is consulted only where the SSR/hydrate paths
/// must render the SAME first value (an empty key), and where the cfg'd
/// effect would not compile on the SSR target — the browser is the only
/// place a key is actually minted.
#[cfg(not(target_arch = "wasm32"))]
fn uuid_string() -> String {
    String::new()
}
