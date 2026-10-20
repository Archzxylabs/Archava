//! The record screen — one contact and everything bound to it, with the four
//! write actions the baseline's contracts define beyond the follow-up draft:
//! `contact.update`, `activity.add`, `followup.complete` and the pipeline's
//! `opportunity.create` / `opportunity.move`.
//!
//! Every fact on this page arrives from the host (`GET /crm/records`) and
//! every action leaves as a typed command (`POST /crm/commands`) carrying the
//! revision this page read. The page never re-derives authority, never retries
//! under a fresh idempotency key after an unknown outcome, and renders the
//! host's own refusal reason when a write is refused.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_query_map;

use crate::draft::uuid_string;
use crate::pending::{self, PendingMarker};
use crate::session::{self, Answer, FetchError};

#[derive(Clone, Debug, PartialEq)]
struct Facts {
    id: String,
    full_name: String,
    email: Option<String>,
    organization: Option<String>,
    owner_id: String,
    revision: i64,
}

impl Facts {
    fn parse(value: &serde_json::Value) -> Option<Self> {
        Some(Facts {
            id: value["id"].as_str()?.to_string(),
            full_name: value["fullName"].as_str()?.to_string(),
            email: value["email"].as_str().map(str::to_owned),
            organization: value["organization"].as_str().map(str::to_owned),
            owner_id: value["ownerId"].as_str()?.to_string(),
            revision: value["revision"].as_i64()?,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Activity {
    kind: String,
    summary: String,
    actor_id: String,
    created_at: String,
}

#[derive(Clone, Debug, PartialEq)]
struct Followup {
    id: String,
    summary: String,
    due_at: String,
    status: String,
    revision: i64,
}

#[derive(Clone, Debug, PartialEq)]
struct Opportunity {
    id: String,
    title: String,
    stage: String,
    revision: i64,
    loss_reason: Option<String>,
    transitions: Vec<Transition>,
}

/// One immutable stage-history row (`from` is absent on the initial `new`).
#[derive(Clone, Debug, PartialEq)]
struct Transition {
    from: Option<String>,
    to: String,
    actor_id: String,
    revision: i64,
    occurred_at: String,
}

#[derive(Clone)]
enum RecordState {
    Waiting,
    Loading,
    Loaded {
        facts: Facts,
        activities: Vec<Activity>,
        followups: Vec<Followup>,
        opportunities: Vec<Opportunity>,
    },
    Refused {
        reason: String,
        message: String,
    },
    Unreachable(String),
    SignedOut,
    Broken,
}

/// The last write's outcome, rendered beside the forms. A `Refused` carries
/// the host's own machine-readable reason; `Unknown` keeps the idempotency key
/// alive so a retry is the SAME command.
#[derive(Clone)]
enum ActionState {
    Idle,
    Busy,
    Recorded(String),
    Refused { reason: String, message: String },
    Unknown(String),
}

const STAGES: [&str; 5] = ["new", "qualified", "proposal", "won", "lost"];

/// The `/contact` route: `?id=<uuid>`.
#[component]
pub fn Contact() -> impl IntoView {
    let session = expect_context::<session::SessionSignal>();
    let query = use_query_map();
    let contact_id = RwSignal::new(query.get_untracked().get("id").unwrap_or_default());
    let (state, set_state) = signal(RecordState::Waiting);
    let (action, set_action) = signal(ActionState::Idle);
    // One in-flight write: the key it used and what it was for. A retry of
    // the SAME action after an unknown outcome reuses the key (the server
    // replays instead of executing twice); a changed form mints a new one.
    let pending = RwSignal::new(None::<(String, String)>);
    // An unresolved app-owned marker from an earlier submission (this page,
    // the draft screen or the create form). Reads stay available; a NEW write
    // intent is refused until it is resolved, while the retry of the SAME key
    // remains allowed.
    let blocked = RwSignal::new(None::<String>);

    // The edit form's boxes carry the record's current values once loaded.
    let edit_name = RwSignal::new(String::new());
    let edit_email = RwSignal::new(String::new());
    let edit_organization = RwSignal::new(String::new());
    let activity_kind = RwSignal::new("note".to_string());
    let activity_summary = RwSignal::new(String::new());
    let opportunity_title = RwSignal::new(String::new());
    let move_stage = RwSignal::new("qualified".to_string());
    let move_reason = RwSignal::new(String::new());

    Effect::new(move |_| {
        if session.get().is_none() {
            return;
        }
        let Some(held) = session::held() else {
            return;
        };
        if let Some(marker) = pending::read(&held.tenant, &held.principal) {
            blocked.set(Some(marker.request_id));
        }
    });

    let load = move |hiatus: bool| {
        if hiatus {
            set_state.set(RecordState::Loading);
        }
        let tenant = session::held().map(|held| held.tenant).unwrap_or_default();
        let id = contact_id.get_untracked();
        if id.is_empty() {
            set_state.set(RecordState::Broken);
            return;
        }
        let target = format!(
            "/crm/records?tenant={}&contact={}",
            crate::contacts::query_encode(&tenant),
            crate::contacts::query_encode(&id)
        );
        spawn_local(async move {
            match session::api_fetch("GET", &target, None).await {
                Ok(Answer { status: 200, body }) => {
                    let Some(facts) = Facts::parse(&body["contact"]) else {
                        set_state.set(RecordState::Broken);
                        return;
                    };
                    let parse_list = |key: &str| -> Option<Vec<serde_json::Value>> {
                        body[key].as_array().cloned()
                    };
                    let (Some(activity_rows), Some(followup_rows), Some(opportunity_rows)) = (
                        parse_list("activities"),
                        parse_list("followups"),
                        parse_list("opportunities"),
                    ) else {
                        set_state.set(RecordState::Broken);
                        return;
                    };
                    let activities: Vec<Activity> = activity_rows
                        .iter()
                        .filter_map(|row| {
                            Some(Activity {
                                kind: row["kind"].as_str()?.to_string(),
                                summary: row["summary"].as_str()?.to_string(),
                                actor_id: row["actorId"].as_str()?.to_string(),
                                created_at: row["createdAt"].as_str()?.to_string(),
                            })
                        })
                        .collect();
                    let followups: Vec<Followup> = followup_rows
                        .iter()
                        .filter_map(|row| {
                            Some(Followup {
                                id: row["id"].as_str()?.to_string(),
                                summary: row["summary"].as_str()?.to_string(),
                                due_at: row["dueAt"].as_str()?.to_string(),
                                status: row["status"].as_str()?.to_string(),
                                revision: row["revision"].as_i64()?,
                            })
                        })
                        .collect();
                    let opportunities: Vec<Opportunity> = opportunity_rows
                        .iter()
                        .filter_map(|row| {
                            Some(Opportunity {
                                id: row["id"].as_str()?.to_string(),
                                title: row["title"].as_str()?.to_string(),
                                stage: row["stage"].as_str()?.to_string(),
                                revision: row["revision"].as_i64()?,
                                loss_reason: row["lossReason"].as_str().map(str::to_owned),
                                transitions: row["transitions"]
                                    .as_array()
                                    .map(|rows| {
                                        rows.iter()
                                            .filter_map(|row| {
                                                Some(Transition {
                                                    from: row["from"].as_str().map(str::to_owned),
                                                    to: row["to"].as_str()?.to_string(),
                                                    actor_id: row["actorId"].as_str()?.to_string(),
                                                    revision: row["revision"].as_i64()?,
                                                    occurred_at: row["occurredAt"]
                                                        .as_str()?
                                                        .to_string(),
                                                })
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_default(),
                            })
                        })
                        .collect();
                    // The edit boxes adopt the freshly read facts: a write
                    // bumps the revision, and the next write must carry the
                    // revision this page just learned, not a stale one.
                    edit_name.set(facts.full_name.clone());
                    edit_email.set(facts.email.clone().unwrap_or_default());
                    edit_organization.set(facts.organization.clone().unwrap_or_default());
                    set_state.set(RecordState::Loaded {
                        facts,
                        activities,
                        followups,
                        opportunities,
                    });
                }
                Ok(Answer { status, body }) => set_state.set(RecordState::Refused {
                    reason: body["reason"]
                        .as_str()
                        .unwrap_or(&format!("http {status}"))
                        .to_string(),
                    message: body["message"]
                        .as_str()
                        .unwrap_or("The record was refused.")
                        .to_string(),
                }),
                Err(FetchError::NoAnswer(why)) => set_state.set(RecordState::Unreachable(why)),
            }
        });
    };

    // One typed submission for every action on the page.
    let send = move |what: &'static str, command: serde_json::Value| {
        // A retry of the SAME key is the resolution path; any other intent is
        // a second command while the first may have committed.
        let retry_key = match pending.get_untracked() {
            Some((key, held)) if held == what => Some(key),
            _ => None,
        };
        if let Some(marker_id) = blocked.get_untracked() {
            if retry_key.as_deref() != Some(marker_id.as_str()) {
                set_action.set(ActionState::Refused {
                    reason: "pending_unresolved".to_string(),
                    message: "An earlier submission is unresolved; resolve it on the recovery page before a new write.".to_string(),
                });
                return;
            }
        }
        let key = match retry_key {
            Some(key) => key,
            None => {
                let key = uuid_string();
                pending.set(Some((key.clone(), what.to_string())));
                key
            }
        };
        let Some(held) = session::held() else {
            return;
        };
        let tenant = held.tenant.clone();
        // The marker is app-owned state, written BEFORE the send: a storage
        // refusal blocks the submission instead of sending unrecoverably.
        if blocked.get_untracked().as_deref() != Some(key.as_str()) {
            let marker = PendingMarker::new(key.clone(), tenant.clone(), held.principal.clone());
            if let Err(why) = pending::write(&marker) {
                set_action.set(ActionState::Refused {
                    reason: "pending_unavailable".to_string(),
                    message: format!("The pending marker could not be stored: {why}"),
                });
                return;
            }
        }
        let body = serde_json::json!({
            "tenant": tenant,
            "requestId": key,
            "command": command,
        })
        .to_string();
        set_action.set(ActionState::Busy);
        spawn_local(async move {
            match session::api_fetch("POST", "/crm/commands", Some(body)).await {
                Ok(Answer { status: 200, body }) if body["decision"] == "receipt" => {
                    pending::clear(&tenant);
                    blocked.set(None);
                    pending.set(None);
                    let receipt = body["receipt"]["id"]
                        .as_str()
                        .unwrap_or("recorded")
                        .to_string();
                    set_action.set(ActionState::Recorded(format!(
                        "Recorded. Receipt: {receipt} (revision {})",
                        body["receipt"]["revision"]
                    )));
                    load(true);
                }
                Ok(Answer { status, body }) => {
                    // A known refusal is a known outcome: the key is spent and
                    // the next attempt is a new command.
                    pending.set(None);
                    set_action.set(ActionState::Refused {
                        reason: body["reason"]
                            .as_str()
                            .unwrap_or(&format!("http {status}"))
                            .to_string(),
                        message: body["message"]
                            .as_str()
                            .unwrap_or("The write was refused.")
                            .to_string(),
                    });
                }
                Err(FetchError::NoAnswer(why)) => {
                    // The outcome is UNKNOWN, not failed: keep the key so a
                    // retry is the same command, and say so.
                    set_action.set(ActionState::Unknown(format!(
                        "{why} — retry sends the same request id, never a second command"
                    )));
                }
            }
        });
    };

    Effect::new(move |_| {
        if session.get().is_none() {
            set_state.set(RecordState::SignedOut);
            return;
        }
        load(true);
    });

    view! {
        <section class="contact">
            <h1 data-testid="contact-heading">"Contact record"</h1>
            {move || match state.get() {
                RecordState::Waiting | RecordState::Loading => view! {
                    <p class="status" data-testid="contact-status">"Loading…"</p>
                }
                .into_any(),
                RecordState::SignedOut => view! {
                    <p class="refusal" data-testid="contact-status">
                        "Sign in first — this record is a session-scoped read."
                    </p>
                }
                .into_any(),
                RecordState::Broken => view! {
                    <p class="refusal" data-testid="contact-status">
                        "The answer did not carry the record contract."
                    </p>
                }
                .into_any(),
                RecordState::Unreachable(why) => view! {
                    <p class="refusal" data-testid="contact-status">{why}</p>
                }
                .into_any(),
                RecordState::Refused { reason, message } => view! {
                    <p class="refusal" data-testid="contact-status">
                        <strong>"Refused: "</strong><code>{reason}</code>" — "{message}
                    </p>
                }
                .into_any(),
                RecordState::Loaded { facts, activities, followups, opportunities } => {
                    let revision = facts.revision;
                    let contact_id_value = facts.id.clone();
                    let update_id = facts.id.clone();
                    let activity_id = facts.id.clone();
                    let opportunity_id = facts.id.clone();
                    let move_buttons = opportunities.clone();
                    view! {
                        <dl data-testid="record-facts">
                            <dt>"Name"</dt><dd data-testid="record-name">{facts.full_name.clone()}</dd>
                            <dt>"Email"</dt><dd data-testid="record-email">{facts.email.clone().unwrap_or_default()}</dd>
                            <dt>"Organization"</dt>
                            <dd data-testid="record-organization">
                                {facts.organization.clone().unwrap_or_default()}
                            </dd>
                            <dt>"Owner"</dt><dd>{facts.owner_id.clone()}</dd>
                            <dt>"Revision"</dt><dd data-testid="record-revision">{revision.to_string()}</dd>
                        </dl>

                        {move || match blocked.get() {
                            Some(request_id) => {
                                let href = format!("/app/recover?id={request_id}");
                                view! {
                                    <p class="refusal" data-testid="record-pending">
                                        "An earlier submission is unresolved (request "
                                        <code>{request_id.clone()}</code>
                                        "). A new write is refused until it is resolved: "
                                        <a data-testid="record-pending-recover" href=href>
                                            "the recovery page"
                                        </a>
                                    </p>
                                }
                                .into_any()
                            }
                            None => view! { <p></p> }.into_any(),
                        }}
                        {move || match action.get() {
                            ActionState::Idle => view! { <p></p> }.into_any(),
                            ActionState::Busy => view! {
                                <p class="status" data-testid="record-action">"Submitting…"</p>
                            }
                            .into_any(),
                            ActionState::Recorded(text) => view! {
                                <p class="status" data-testid="record-action">{text}</p>
                            }
                            .into_any(),
                            ActionState::Refused { reason, message } => view! {
                                <p class="refusal" data-testid="record-action">
                                    <strong>"Refused: "</strong><code>{reason}</code>" — "{message}
                                </p>
                            }
                            .into_any(),
                            ActionState::Unknown(why) => view! {
                                <p class="refusal" data-testid="record-action">{why}</p>
                            }
                            .into_any(),
                        }}

                        <h2>"Edit the contact"</h2>
                        <form on:submit=move |event| {
                            event.prevent_default();
                            pending.set(None);
                            send(
                                "contact.update",
                                serde_json::json!({
                                    "operation": "contact.update",
                                    "id": update_id,
                                    "expectedRevision": revision,
                                    "data": {
                                        "fullName": edit_name.get_untracked(),
                                        "email": if edit_email.get_untracked().is_empty() {
                                            serde_json::Value::Null
                                        } else {
                                            serde_json::Value::String(edit_email.get_untracked())
                                        },
                                        "organization": if edit_organization.get_untracked().is_empty() {
                                            serde_json::Value::Null
                                        } else {
                                            serde_json::Value::String(edit_organization.get_untracked())
                                        },
                                    },
                                }),
                            );
                        }>
                            <label>"Name " <input data-testid="record-edit-name" value=edit_name
                                on:input=move |ev| edit_name.set(event_target_value(&ev)) /></label>
                            <label>"Email " <input data-testid="record-edit-email" value=edit_email
                                on:input=move |ev| edit_email.set(event_target_value(&ev)) /></label>
                            <label>"Organization " <input data-testid="record-edit-organization" value=edit_organization
                                on:input=move |ev| edit_organization.set(event_target_value(&ev)) /></label>
                            <button data-testid="record-save">"Save contact"</button>
                        </form>

                        <h2>"Activities"</h2>
                        <ul data-testid="record-activities">
                            {activities
                                .iter()
                                .map(|activity| view! {
                                    <li data-testid="record-activity">
                                        <code>{activity.kind.clone()}</code>
                                        " — "{activity.summary.clone()}
                                        " ("{activity.actor_id.clone()}")"
                                    </li>
                                })
                                .collect::<Vec<_>>()}
                        </ul>
                        <form on:submit=move |event| {
                            event.prevent_default();
                            pending.set(None);
                            send(
                                "activity.add",
                                serde_json::json!({
                                    "operation": "activity.add",
                                    "contactId": activity_id,
                                    "contactRevision": revision,
                                    "kind": activity_kind.get_untracked(),
                                    "summary": activity_summary.get_untracked(),
                                }),
                            );
                        }>
                            <label>"Kind "
                                <select data-testid="record-activity-kind"
                                    on:change=move |ev| activity_kind.set(event_target_value(&ev))>
                                    <option value="note">"note"</option>
                                    <option value="call">"call"</option>
                                    <option value="meeting">"meeting"</option>
                                </select>
                            </label>
                            <label>"Summary " <input data-testid="record-activity-summary"
                                value=activity_summary
                                on:input=move |ev| activity_summary.set(event_target_value(&ev)) /></label>
                            <button data-testid="record-activity-add">"Add activity"</button>
                        </form>

                        <h2>"Follow-ups"</h2>
                        <ul data-testid="record-followups">
                            {followups
                                .iter()
                                .map(|followup| {
                                    let id = followup.id.clone();
                                    let followup_revision = followup.revision;
                                    let open = followup.status == "open";
                                    view! {
                                        <li data-testid="record-followup">
                                            {followup.summary.clone()}
                                            " — due "{followup.due_at.clone()}
                                            " — "{followup.status.clone()}
                                            {open.then(|| view! {
                                                <button
                                                    data-testid="record-followup-complete"
                                                    data-followup=id.clone()
                                                    on:click=move |_event| {
                                                        pending.set(None);
                                                        send(
                                                            "followup.complete",
                                                            serde_json::json!({
                                                                "operation": "followup.complete",
                                                                "id": id,
                                                                "expectedRevision": followup_revision,
                                                            }),
                                                        );
                                                    }
                                                >
                                                    "Complete"
                                                </button>
                                            })}
                                        </li>
                                    }
                                })
                                .collect::<Vec<_>>()}
                        </ul>

                        <h2>"Pipeline"</h2>
                        <ul data-testid="record-opportunities">
                            {move_buttons
                                .iter()
                                .map(|opportunity| {
                                    let id = opportunity.id.clone();
                                    let opportunity_revision = opportunity.revision;
                                    let stage_now = opportunity.stage.clone();
                                    view! {
                                        <li data-testid="record-opportunity">
                                            {opportunity.title.clone()}
                                            " — stage "{opportunity.stage.clone()}
                                            " — rev "{opportunity.revision.to_string()}
                                            {opportunity.loss_reason.clone().map(|reason| view! {
                                                <span> " (lost: "{reason}")"</span>
                                            })}
                                            <ul class="history" data-testid="record-opportunity-history">
                                                {opportunity
                                                    .transitions
                                                    .iter()
                                                    .map(|step| view! {
                                                        <li data-testid="record-history-step">
                                                            "rev "{step.revision.to_string()}
                                                            " — "{step.from.clone().unwrap_or_else(|| "·".to_string())}
                                                            " → "{step.to.clone()}
                                                            " by "{step.actor_id.clone()}
                                                        </li>
                                                    })
                                                    .collect::<Vec<_>>()}
                                            </ul>
                                            <div class="move">
                                                <select data-testid="record-move-stage"
                                                    on:change=move |ev| move_stage.set(event_target_value(&ev))>
                                                    {STAGES
                                                        .iter()
                                                        .map(|stage| view! {
                                                            <option value=*stage selected=*stage == stage_now>
                                                                {*stage}
                                                            </option>
                                                        })
                                                        .collect::<Vec<_>>()}
                                                </select>
                                                <input data-testid="record-move-reason" value=move_reason
                                                    placeholder="loss reason (required for lost)"
                                                    on:input=move |ev| move_reason.set(event_target_value(&ev)) />
                                                <button data-testid="record-move"
                                                    data-opportunity=id.clone()
                                                    on:click=move |_event| {
                                                        pending.set(None);
                                                        let reason = move_reason.get_untracked();
                                                        send(
                                                            "opportunity.move",
                                                            serde_json::json!({
                                                                "operation": "opportunity.move",
                                                                "id": id,
                                                                "expectedRevision": opportunity_revision,
                                                                "stage": move_stage.get_untracked(),
                                                                "reason": if reason.is_empty() {
                                                    serde_json::Value::Null
                                                } else {
                                                    serde_json::Value::String(reason)
                                                },
                                                            }),
                                                        );
                                                    }
                                                >
                                                    "Move"
                                                </button>
                                            </div>
                                        </li>
                                    }
                                })
                                .collect::<Vec<_>>()}
                        </ul>
                        <form on:submit=move |event| {
                            event.prevent_default();
                            pending.set(None);
                            send(
                                "opportunity.create",
                                serde_json::json!({
                                    "operation": "opportunity.create",
                                    "contactId": opportunity_id,
                                    "contactRevision": revision,
                                    "title": opportunity_title.get_untracked(),
                                }),
                            );
                        }>
                            <label>"Title " <input data-testid="record-opportunity-title"
                                value=opportunity_title
                                on:input=move |ev| opportunity_title.set(event_target_value(&ev)) /></label>
                            <button data-testid="record-opportunity-create">"Open opportunity"</button>
                        </form>
                        <p class="hint">
                            "Contact id: "<code data-testid="record-id">{contact_id_value}</code>
                        </p>
                    }
                }
                .into_any(),
            }}
        </section>
    }
}
