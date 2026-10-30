//! A read-only contact pane backed by the existing CRM record endpoint.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::contacts::query_encode;
use crate::session::{self, Answer, SessionSignal};

pub(crate) fn initials(name: &str) -> String {
    let value: String = name
        .split_whitespace()
        .take(2)
        .filter_map(|part| part.chars().next())
        .collect();
    if value.is_empty() {
        "?".into()
    } else {
        value.to_uppercase()
    }
}

#[derive(Clone)]
enum ContextState {
    Empty,
    Loading,
    Ready(serde_json::Value),
    Unavailable(String),
}

#[component]
pub(crate) fn ContactContext(
    selected: RwSignal<Option<String>>,
    reveal: RwSignal<bool>,
) -> impl IntoView {
    let session = expect_context::<SessionSignal>();
    let state = RwSignal::new(ContextState::Empty);
    let generation = RwSignal::new(0_u64);
    let pane = NodeRef::<leptos::html::Aside>::new();

    Effect::new(move |_| {
        if !reveal.get() {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        if web_sys::window()
            .and_then(|window| window.inner_width().ok())
            .and_then(|width| width.as_f64())
            .is_some_and(|width| width <= 760.0)
        {
            if let Some(element) = pane.get() {
                element.scroll_into_view();
                let _ = element.focus();
            }
        }
        reveal.set(false);
    });

    Effect::new(move |_| {
        let contact = selected.get();
        let held = session.get();
        generation.update(|value| *value = value.wrapping_add(1));
        let ticket = generation.get_untracked();
        let (Some(contact), Some(held)) = (contact, held) else {
            state.set(ContextState::Empty);
            return;
        };
        state.set(ContextState::Loading);
        let target = format!(
            "/crm/records?tenant={}&contact={}",
            query_encode(&held.tenant),
            query_encode(&contact),
        );
        spawn_local(async move {
            let result = session::api_fetch("GET", &target, None).await;
            let same_session = session
                .try_get_untracked()
                .flatten()
                .is_some_and(|current| {
                    current.tenant == held.tenant && current.principal == held.principal
                });
            if generation.try_get_untracked() != Some(ticket) || !same_session {
                return;
            }
            match result {
                Ok(Answer { status: 200, body }) => {
                    let record = &body["contact"];
                    let valid = record["id"].as_str() == Some(contact.as_str())
                        && record["fullName"].as_str().is_some()
                        && record["revision"].as_i64().is_some()
                        && body["activities"].is_array()
                        && body["followups"].is_array();
                    state.set(if valid {
                        ContextState::Ready(body)
                    } else {
                        ContextState::Unavailable("Contact details could not be read. Try opening the full record.".into())
                    });
                }
                Ok(answer) => state.set(ContextState::Unavailable(
                    answer.message().unwrap_or("This contact could not be loaded with your current access.").to_string(),
                )),
                Err(_) => state.set(ContextState::Unavailable(
                    "Contact details are temporarily unavailable. Open the full record to try again.".into(),
                )),
            }
        });
    });

    view! {
        <aside class="contact-context" id="selected-contact" node_ref=pane tabindex="-1" aria-label="Selected contact" aria-busy=move || state.with(|value| matches!(value, ContextState::Loading)).to_string()>
            <p class="context-label">"Contact context"</p>
            {move || match state.get() {
                ContextState::Empty => view! {
                    <h2>"Start with a person."</h2>
                    <p class="context-empty">"Choose a contact to see their details, activity, and next steps here."</p>
                }.into_any(),
                ContextState::Loading => view! {
                    <p class="status" role="status">"Loading contact details…"</p>
                }.into_any(),
                ContextState::Unavailable(message) => view! {
                    <p class="refusal" role="status">{message}</p>
                    {selected.get().map(|id| view! {
                        <a class="context-record-link" href=format!("/app/contact?id={}", query_encode(&id))>"Open full record"</a>
                    })}
                }.into_any(),
                ContextState::Ready(body) => {
                    let record = &body["contact"];
                    let id = record["id"].as_str().unwrap_or_default();
                    let name = record["fullName"].as_str().unwrap_or_default().to_string();
                    let organization = record["organization"].as_str().unwrap_or("No organization added").to_string();
                    let email = record["email"].as_str().unwrap_or("No email added").to_string();
                    let owner = record["ownerId"].as_str().unwrap_or_default().to_string();
                    let revision = record["revision"].as_i64().unwrap_or_default();
                    let record_href = format!("/app/contact?id={}", query_encode(id));
                    let draft_href = format!("/app/draft?contact={}&revision={revision}&name={}", query_encode(id), query_encode(&name));
                    let activities = body["activities"].as_array().cloned().unwrap_or_default();
                    let open_followups: Vec<_> = body["followups"].as_array().into_iter().flatten().filter(|item| item["status"].as_str() == Some("open")).cloned().collect();
                    let avatar = initials(&name);
                    view! {
                        <div class="context-identity"><span class="context-initials" aria-hidden="true">{avatar}</span><div><h2 class="context-name" data-testid="context-name">{name}</h2>
                        <p class="context-organization">{organization}</p></div></div>
                        <div class="context-actions">
                            <a class="action-link" data-testid="context-followup" href=draft_href>"Create a follow-up"</a>
                            <a class="context-record-link" data-testid="context-open" href=record_href>"Open full record"</a>
                        </div>
                        <dl class="context-facts">
                            <dt>"Email"</dt><dd>{email}</dd>
                            <dt>"Owner"</dt><dd>{owner}</dd>
                        </dl>
                        <div class="context-details">
                        <section class="context-activity">
                            <h3>"Recent activity"</h3>
                            {if activities.is_empty() {
                                view! { <p class="context-empty">"No activity recorded yet. Add a note from your next conversation in the full record."</p> }.into_any()
                            } else {
                                view! {
                                    <ul>{activities.into_iter().take(3).map(|item| view! {
                                        <li><p>{item["summary"].as_str().unwrap_or_default().to_string()}</p><small>{item["kind"].as_str().unwrap_or_default().to_string()}</small></li>
                                    }).collect_view()}</ul>
                                }.into_any()
                            }}
                        </section>
                        <section class="context-followups">
                            <h3>"Next steps"</h3>
                            {if open_followups.is_empty() {
                                view! { <p class="context-empty">"No open follow-ups for this contact."</p> }.into_any()
                            } else {
                                view! {
                                    <ul>{open_followups.into_iter().take(3).map(|item| view! {
                                        <li><p>{item["summary"].as_str().unwrap_or_default().to_string()}</p><small>"Due "{item["dueAt"].as_str().unwrap_or_default().to_string()}</small></li>
                                    }).collect_view()}</ul>
                                }.into_any()
                            }}
                        </section>
                        </div>
                    }.into_any()
                },
            }}
        </aside>
    }
}
