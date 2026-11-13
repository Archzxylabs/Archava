//! The assistant desk — the visible assistant panel, backed by the host's
//! real conversation route (`POST /api/crm/{tenant}/conversation`).
//!
//! The planner is deterministic and the answers are served from the CRM: the
//! overview counts, the open worklist, the pipeline, contact lists and
//! summaries all come from the same SQL the product reads use. The panel is a
//! product surface, not a mock: it shows the reply text, the reply's source
//! (`crm` or `none`), and the typed data behind it.
//!
//! ## Execution
//!
//! `complete follow-up` only PREPARES: the assistant answers with a
//! `completion_draft` carrying the follow-up's id and revision and
//! `requiresApproval: true`. The desk renders that as a review with one
//! explicit confirmation; confirming sends the typed `followup.complete`
//! command through `/crm/commands`, the host re-derives authority, writes the
//! effect and the receipt, and the desk re-reads the worklist. Nothing the
//! planner says is executed without that human confirmation, and the desk
//! never retries under a fresh idempotency key after an unknown outcome.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;

use crate::alive::AliveSignal;
use crate::draft::uuid_string;
use crate::pending::{self, PendingMarker};
use crate::session::{self, Answer, FetchError};

const OVERVIEW: &str = "show crm overview";
const WORKLIST: &str = "list follow-ups";
const PIPELINE: &str = "list opportunities";
const CONTACTS: &str = "list contacts";

fn completion_receipt(body: &serde_json::Value, key: &str, command: &serde_json::Value) -> bool {
    crate::receipt::crm(body, key, Some("followup"))
        && body["receipt"]["id"] == command["id"]
        && command["expectedRevision"]
            .as_i64()
            .and_then(|n| n.checked_add(1))
            == body["receipt"]["revision"].as_i64()
}

#[cfg(test)]
mod receipt_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn completion_acknowledgement_binds_the_original_key_resource_and_next_revision() {
        let key = "ad24fbe6-0000-4000-8000-000000000001";
        let id = "ad24fbe6-0000-4000-8000-000000000002";
        let command = json!({"id":id,"expectedRevision":4});
        let receipt = json!({"decision":"receipt","request_id":key,"receipt":{"kind":"followup","id":id,"revision":5}});
        assert!(completion_receipt(&receipt, key, &command));
        for wrong in [
            json!({"id":key,"expectedRevision":4}),
            json!({"id":id,"expectedRevision":3}),
        ] {
            assert!(!completion_receipt(&receipt, key, &wrong));
        }
        assert!(!completion_receipt(&receipt, "wrong-key", &command));
        assert!(!completion_receipt(
            &json!({"decision":"receipt","requestId":"wrong-request","receipt":{"id":"wrong-resource","revision":1}}),
            key,
            &command
        ));
        assert!(!completion_receipt(
            &json!({"decision":"receipt"}),
            key,
            &command
        ));
    }
}

#[derive(Clone, Debug, PartialEq)]
struct QueueItem {
    id: String,
    contact_id: String,
    contact_name: String,
    summary: String,
    due_at: String,
}

#[derive(Clone, Debug, PartialEq)]
struct PipeItem {
    id: String,
    contact_id: String,
    title: String,
    stage: String,
    revision: i64,
}

#[derive(Clone, Debug, PartialEq)]
struct ContactItem {
    id: String,
    full_name: String,
    revision: i64,
}

#[derive(Clone, Debug, PartialEq)]
enum Payload {
    Overview {
        contacts: String,
        open: String,
        overdue: String,
        upcoming: String,
        completed: String,
        pipeline: String,
        active: String,
    },
    Worklist(Vec<QueueItem>),
    Pipeline(Vec<PipeItem>),
    Contacts(Vec<ContactItem>),
    /// A prepared completion: the review the human confirms.
    Completion {
        followup_id: String,
        revision: i64,
        summary: String,
        contact_id: String,
        contact_name: String,
    },
    Summary {
        contact_id: String,
        full_name: String,
        revision: i64,
    },
    Other(String),
    Empty,
}

#[derive(Clone, Debug, PartialEq)]
struct Reply {
    text: String,
    source: String,
    /// The planner identity the reply reports (`model` / `providerId`) —
    /// the panel shows it so an answer is never anonymous.
    planner: String,
    data: Payload,
}

#[derive(Clone)]
enum DeskState {
    Waiting,
    Asking,
    Answered(Reply),
    Refused { reason: String, message: String },
    Unreachable(String),
}

#[derive(Clone)]
enum ActionState {
    Idle,
    Busy,
    Recorded(String),
    Refused { reason: String, message: String },
    Unknown(String),
}

/// The `/desk` route: the assistant panel.
#[component]
pub fn Desk() -> impl IntoView {
    let session = expect_context::<session::SessionSignal>();
    let (state, set_state) = signal(DeskState::Waiting);
    let (action, set_action) = signal(ActionState::Idle);
    let prompt = RwSignal::new(String::new());
    let pending = RwSignal::new(None::<(String, String)>);
    let blocked = RwSignal::new(None::<String>);
    // The utterance the panel is currently answering — re-asked after a
    // confirmed write so the data region reflects the committed effect.
    let utterance = RwSignal::new(OVERVIEW.to_string());

    let ask = move |utterance_text: String, followup_id: Option<String>| {
        set_state.set_if_alive(DeskState::Asking);
        utterance.set_if_alive(utterance_text.clone());
        let tenant = session::held().map(|held| held.tenant).unwrap_or_default();
        let target = format!(
            "/api/crm/{}/conversation",
            crate::contacts::query_encode(&tenant)
        );
        let mut body = serde_json::json!({ "utterance": utterance_text });
        if let Some(id) = followup_id {
            body["followupId"] = serde_json::Value::String(id);
        }
        let body = body.to_string();
        spawn_local(async move {
            match session::api_fetch("POST", &target, Some(body)).await {
                Ok(Answer { status: 200, body }) => {
                    let text = body["text"].as_str().unwrap_or_default().to_string();
                    let source = body["source"].as_str().unwrap_or("none").to_string();
                    let payload = parse_payload(&body["data"]);
                    let planner = format!(
                        "{} / {}",
                        body["planner"]["model"].as_str().unwrap_or("—"),
                        body["planner"]["providerId"].as_str().unwrap_or("—"),
                    );
                    set_state.set_if_alive(DeskState::Answered(Reply {
                        text,
                        source,
                        planner,
                        data: payload,
                    }));
                }
                Ok(Answer { status, body }) => {
                    // The conversation route answers failures in the house
                    // envelope `{error:{message}}` (the command route's
                    // `{reason,message}` is a different contract); surface the
                    // status code when it names no reason.
                    set_state.set_if_alive(DeskState::Refused {
                        reason: format!("http {status}"),
                        message: body["error"]["message"]
                            .as_str()
                            .or_else(|| body["message"].as_str())
                            .unwrap_or("The assistant refused.")
                            .to_string(),
                    });
                }
                Err(FetchError::NoAnswer(why)) => {
                    set_state.set_if_alive(DeskState::Unreachable(why))
                }
            }
        });
    };

    // One typed write (the human's confirmation of a prepared completion).
    // Same idempotency discipline as the record screen: an unknown outcome
    // keeps the key, so the retry is the SAME command.
    let send = move |_what: &'static str, command: serde_json::Value| {
        if matches!(action.get_untracked(), ActionState::Busy) {
            return;
        }
        let Some(held) = session::held() else {
            return;
        };
        let canonical = command.to_string();
        let key = match pending.get_untracked() {
            Some((key, stored)) if stored == canonical => key,
            _ if blocked.get_untracked().is_some() => return,
            _ => {
                let key = uuid_string();
                pending.set_if_alive(Some((key.clone(), canonical)));
                key
            }
        };
        let marker = PendingMarker::with_domain(
            key.clone(),
            held.tenant.clone(),
            format!(
                "desk:{}:{}",
                command["id"].as_str().unwrap_or_default(),
                command["expectedRevision"].as_i64().unwrap_or(0)
            ),
            held.principal.clone(),
        );
        if pending::write(&marker).is_err() {
            set_action.set_if_alive(ActionState::Refused {
                reason: "pending_unavailable".into(),
                message: "Could not preserve the action reference. Completion was not sent.".into(),
            });
            return;
        }
        blocked.set_if_alive(Some(key.clone()));
        let body = serde_json::json!({
            "tenant": held.tenant,
            "requestId": key,
            "command": command.clone(),
        })
        .to_string();
        set_action.set_if_alive(ActionState::Busy);
        spawn_local(async move {
            match session::api_fetch("POST", "/crm/commands", Some(body)).await {
                Ok(Answer { status: 200, body }) if completion_receipt(&body, &key, &command) => {
                    pending::clear_by_request_id(&held.tenant, &held.principal, &key);
                    blocked.set_if_alive(None);
                    pending.set_if_alive(None);
                    let receipt = body["receipt"]["id"]
                        .as_str()
                        .unwrap_or("recorded")
                        .to_string();
                    set_action.set_if_alive(ActionState::Recorded(format!(
                        "Recorded. Receipt: {receipt} (revision {})",
                        body["receipt"]["revision"]
                    )));
                    // The committed effect is what the panel re-reads.
                    if let Some(text) = utterance.try_get_untracked() {
                        ask(text, None);
                    }
                }
                Ok(_) => {
                    set_action.set_if_alive(ActionState::Unknown("A matching completion receipt was not received. Check the original reference before retrying.".into()));
                }
                Err(FetchError::NoAnswer(why)) => {
                    set_action.set_if_alive(ActionState::Unknown(format!(
                        "{why} — retry sends the same request id, never a second command"
                    )));
                }
            }
        });
    };

    let recover = move |_| {
        let Some(held) = session::held() else {
            return;
        };
        let Some((key, raw)) = pending.get_untracked() else {
            return;
        };
        let Ok(command) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return;
        };
        set_action.set_if_alive(ActionState::Busy);
        spawn_local(async move {
            let path = format!(
                "/crm/commands/status?tenant={}&requestId={}",
                crate::contacts::query_encode(&held.tenant),
                key
            );
            match session::api_fetch("GET", &path, None).await {
                Ok(Answer {status:200, body}) if completion_receipt(&body, &key, &command) => {
                    pending::clear_by_request_id(&held.tenant, &held.principal, &key);
                    blocked.set_if_alive(None); pending.set_if_alive(None);
                    set_action.set_if_alive(ActionState::Recorded(format!("Recorded. Receipt: {} (revision {})",body["receipt"]["id"].as_str().unwrap_or_default(),body["receipt"]["revision"])));
                    ask(WORKLIST.to_string(), None);
                }
                _ => set_action.set_if_alive(ActionState::Unknown("No matching committed receipt is visible. Keep this reference or review and retry the original completion.".into())),
            }
        });
    };

    Effect::new(move |_| {
        if session.get().is_none() {
            pending.set_if_alive(None);
            blocked.set_if_alive(None);
            return;
        }
        if let Some(held) = session::held() {
            if let Some(marker) = pending::read_family(&held.tenant, "desk", &held.principal) {
                let parts: Vec<_> = marker.domain.split(':').collect();
                if let ["desk", id, revision] = parts.as_slice() {
                    if let Ok(revision) = revision.parse::<i64>() {
                        pending.set_if_alive(Some((marker.request_id.clone(), serde_json::json!({"operation":"followup.complete","id":id,"expectedRevision":revision}).to_string())));
                        blocked.set_if_alive(Some(marker.request_id));
                        set_action.set_if_alive(ActionState::Unknown(
                            "An earlier completion needs a status check.".into(),
                        ));
                    }
                }
            }
        }
        ask(OVERVIEW.to_string(), None);
    });

    view! {
        <section class="desk">
            <h1 data-testid="desk-heading">"Work with Ava"</h1>
            <crate::agent::AgentPanel/>
            <h2>"CRM command shortcuts"</h2>
            <p class="hint">
                "Answers are served from the CRM; a prepared completion is"
                " executed only after the confirmation below."
            </p>
            <div class="asks">
                <button data-testid="desk-ask-overview" on:click=move |_| ask(OVERVIEW.to_string(), None)>
                    "Overview"
                </button>
                <button data-testid="desk-ask-worklist" on:click=move |_| ask(WORKLIST.to_string(), None)>
                    "Worklist"
                </button>
                <button data-testid="desk-ask-pipeline" on:click=move |_| ask(PIPELINE.to_string(), None)>
                    "Pipeline"
                </button>
                <button data-testid="desk-ask-contacts" on:click=move |_| ask(CONTACTS.to_string(), None)>
                    "Contacts"
                </button>
            </div>
            <form on:submit=move |event| {
                event.prevent_default();
                let text = prompt.get_untracked();
                if text.trim().is_empty() {
                    return;
                }
                ask(text, None);
            }>
                <label>"Ask " <input data-testid="desk-prompt" value=prompt
                    placeholder="e.g. list follow-ups"
                    on:input=move |ev| prompt.set_if_alive(event_target_value(&ev)) /></label>
                <button data-testid="desk-send">"Send"</button>
            </form>

            {move || match state.get() {
                DeskState::Waiting | DeskState::Asking => view! {
                    <p class="status" data-testid="desk-status">"Asking…"</p>
                }
                .into_any(),
                DeskState::Unreachable(why) => view! {
                    <p class="refusal" data-testid="desk-status">{why}</p>
                }
                .into_any(),
                DeskState::Refused { reason, message } => view! {
                    <p class="refusal" data-testid="desk-status">
                        <strong>"Refused: "</strong><code>{reason}</code>" — "{message}
                    </p>
                }
                .into_any(),
                DeskState::Answered(reply) => view! {
                    <p class="reply" data-testid="desk-reply">{reply.text.clone()}</p>
                    <p class="hint">
                        "source: "<code data-testid="desk-source">{reply.source.clone()}</code>
                        " · planner: "<code data-testid="desk-planner">{reply.planner.clone()}</code>
                    </p>
                    {match reply.data.clone() {
                        Payload::Overview { contacts, open, overdue, upcoming, completed, pipeline, active } => view! {
                            <dl data-testid="desk-overview">
                                <dt>"Contacts"</dt><dd data-testid="desk-overview-contacts">{contacts}</dd>
                                <dt>"Open"</dt><dd>{open}</dd>
                                <dt>"Overdue"</dt><dd>{overdue}</dd>
                                <dt>"Upcoming"</dt><dd>{upcoming}</dd>
                                <dt>"Completed"</dt><dd>{completed}</dd>
                                <dt>"Pipeline"</dt><dd data-testid="desk-overview-pipeline">{pipeline}</dd>
                                <dt>"Active"</dt><dd>{active}</dd>
                            </dl>
                        }.into_any(),
                        Payload::Worklist(items) => view! {
                            <ul data-testid="desk-worklist">
                                {items.into_iter().map(|item| {
                                    let complete_id = item.id.clone();
                                    view! {
                                        <li data-testid="desk-worklist-item">
                                            {item.summary.clone()}
                                            " — due "{item.due_at.clone()}
                                            " — "{item.contact_name.clone()}
                                            <RecordLink id=item.contact_id.clone() />
                                            <button
                                                data-testid="desk-prepare-complete"
                                                data-followup=item.id.clone()
                                                on:click=move |_| {
                                                    ask("complete follow-up".to_string(), Some(complete_id.clone()));
                                                }
                                            >
                                                "Complete"
                                            </button>
                                        </li>
                                    }
                                }).collect::<Vec<_>>()}
                            </ul>
                        }.into_any(),
                        Payload::Pipeline(items) => view! {
                            <ul data-testid="desk-pipeline">
                                {items.into_iter().map(|item| view! {
                                    <li data-testid="desk-pipeline-item" data-opportunity=item.id.clone()>
                                        {item.title.clone()}
                                        " — stage "{item.stage.clone()}
                                        " — rev "{item.revision.to_string()}
                                        <RecordLink id=item.contact_id.clone() />
                                    </li>
                                }).collect::<Vec<_>>()}
                            </ul>
                        }.into_any(),
                        Payload::Contacts(items) => view! {
                            <ul data-testid="desk-contacts">
                                {items.into_iter().map(|item| view! {
                                    <li data-testid="desk-contact-item">
                                        {item.full_name.clone()}
                                        " — rev "{item.revision.to_string()}
                                        <RecordLink id=item.id.clone() />
                                    </li>
                                }).collect::<Vec<_>>()}
                            </ul>
                        }.into_any(),
                        Payload::Completion { followup_id, revision, summary, contact_id, contact_name } => {
                            let id = followup_id.clone();
                            view! {
                                <div class="completion" data-testid="desk-completion">
                                    <p>
                                        "Prepared: " <strong>{summary}</strong>
                                        " for "{contact_name}
                                        " (revision "{revision.to_string()}")"
                                    </p>
                                    <p class="hint">"requiresApproval: true — nothing is written until you confirm."</p>
                                    <button
                                        data-testid="desk-confirm-complete"
                                        data-followup=followup_id.clone()
                                        disabled=move || blocked.get().is_some() || matches!(action.get(), ActionState::Busy)
                                        on:click=move |_| {
                                            // The committed effect is what the panel
                                            // re-reads: the worklist without the item.
                                            utterance.set_if_alive(WORKLIST.to_string());
                                            send(
                                                "followup.complete",
                                                serde_json::json!({
                                                    "operation": "followup.complete",
                                                    "id": id,
                                                    "expectedRevision": revision,
                                                }),
                                            );
                                        }
                                    >
                                        "Confirm completion"
                                    </button>
                                    <RecordLink id=contact_id />
                                </div>
                            }.into_any()
                        }
                        Payload::Summary { contact_id, full_name, revision } => view! {
                            <dl data-testid="desk-summary">
                                <dt>"Contact"</dt><dd data-testid="desk-summary-name">{full_name}</dd>
                                <dt>"Revision"</dt><dd>{revision.to_string()}</dd>
                            </dl>
                            <RecordLink id=contact_id />
                        }.into_any(),
                        Payload::Other(kind) => view! {
                            <p class="hint" data-testid="desk-data-kind">"data: "{kind}</p>
                        }.into_any(),
                        Payload::Empty => view! { <p></p> }.into_any(),
                    }}
                }.into_any(),
            }}

            {move || match action.get() {
                ActionState::Idle => view! { <p></p> }.into_any(),
                ActionState::Busy => view! {
                    <p class="status" data-testid="desk-action">"Submitting…"</p>
                }.into_any(),
                ActionState::Recorded(text) => view! {
                    <p class="status" data-testid="desk-action">{text}</p>
                }.into_any(),
                ActionState::Refused { reason, message } => view! {
                    <p class="refusal" data-testid="desk-action">
                        <strong>"Refused: "</strong><code>{reason}</code>" — "{message}
                    </p>
                }.into_any(),
                ActionState::Unknown(why) => view! {
                    <p class="refusal" data-testid="desk-action">{why}</p>
                }.into_any(),
            }}
            {move || blocked.get().map(|key| view! {
                <div class="recovery-panel" data-testid="desk-pending">
                    <p>"Completion reference: "<code>{key}</code></p>
                    <button data-testid="desk-recover" disabled=move || matches!(action.get(),ActionState::Busy) on:click=recover>"Check completion status"</button>
                    <button data-testid="desk-retry" disabled=move || pending.get().is_none() || matches!(action.get(),ActionState::Busy)
                        on:click=move |_| { if let Some((_,raw))=pending.get_untracked() { if let Ok(command)=serde_json::from_str(&raw) { send("followup.complete",command); } } }>"Retry original completion"</button>
                </div>
            })}
        </section>
    }
}

/// The typed data behind a reply. Unknown/absent data is `Empty`/`Other`; the
/// reply text still renders, which is what the contract promises.
fn parse_payload(value: &serde_json::Value) -> Payload {
    match value["kind"].as_str() {
        Some("overview") => {
            let snapshot = &value["snapshot"];
            let count = |node: &serde_json::Value| {
                node.as_i64()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "—".to_string())
            };
            Payload::Overview {
                contacts: count(&snapshot["contacts"]),
                open: count(&snapshot["followups"]["open"]),
                overdue: count(&snapshot["followups"]["overdue"]),
                upcoming: count(&snapshot["followups"]["upcoming"]),
                completed: count(&snapshot["followups"]["completed"]),
                pipeline: count(&snapshot["pipeline"]["total"]),
                active: count(&snapshot["pipeline"]["active"]),
            }
        }
        Some("followups") => Payload::Worklist(
            value["items"]
                .as_array()
                .map(|rows| {
                    rows.iter()
                        .filter_map(|row| {
                            Some(QueueItem {
                                id: row["id"].as_str()?.to_string(),
                                contact_id: row["contactId"].as_str()?.to_string(),
                                contact_name: row["fullName"].as_str()?.to_string(),
                                summary: row["summary"].as_str()?.to_string(),
                                due_at: row["dueAt"].as_str()?.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),
        Some("opportunities") => Payload::Pipeline(
            value["items"]
                .as_array()
                .map(|rows| {
                    rows.iter()
                        .filter_map(|row| {
                            Some(PipeItem {
                                id: row["id"].as_str()?.to_string(),
                                contact_id: row["contactId"].as_str()?.to_string(),
                                title: row["title"].as_str()?.to_string(),
                                stage: row["stage"].as_str()?.to_string(),
                                revision: row["revision"].as_i64()?,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),
        Some("contacts") => Payload::Contacts(
            value["items"]
                .as_array()
                .map(|rows| {
                    rows.iter()
                        .filter_map(|row| {
                            Some(ContactItem {
                                id: row["id"].as_str()?.to_string(),
                                full_name: row["fullName"].as_str()?.to_string(),
                                revision: row["revision"].as_i64()?,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),
        Some("completion_draft") => {
            let task = &value["task"];
            match (
                task["followup"]["id"].as_str(),
                task["followup"]["revision"].as_i64(),
                task["followup"]["summary"].as_str(),
                task["contact"]["id"].as_str(),
                task["contact"]["fullName"].as_str(),
            ) {
                (Some(id), Some(revision), Some(summary), Some(contact_id), Some(contact_name)) => {
                    Payload::Completion {
                        followup_id: id.to_string(),
                        revision,
                        summary: summary.to_string(),
                        contact_id: contact_id.to_string(),
                        contact_name: contact_name.to_string(),
                    }
                }
                _ => Payload::Other("completion_draft".to_string()),
            }
        }
        Some("summary") => {
            let contact = &value["contact"];
            match (
                contact["id"].as_str(),
                contact["fullName"].as_str(),
                contact["revision"].as_i64(),
            ) {
                (Some(id), Some(name), Some(revision)) => Payload::Summary {
                    contact_id: id.to_string(),
                    full_name: name.to_string(),
                    revision,
                },
                _ => Payload::Other("summary".to_string()),
            }
        }
        Some(kind) => Payload::Other(kind.to_string()),
        None => Payload::Empty,
    }
}

/// A link to the record screen for one contact, inside the router context.
#[component]
fn RecordLink(id: String) -> impl IntoView {
    let navigate = use_navigate();
    view! {
        <button
            data-testid="desk-open-record"
            data-contact=id.clone()
            on:click=move |_event| {
                let target = format!("/app/contact?id={id}");
                navigate(&target, Default::default());
            }
        >
            "Open record"
        </button>
    }
}
