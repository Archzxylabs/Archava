//! C03's owner surface: inspect → plan → preview → confirm → applied →
//! history/restore, driven entirely by the host's persisted lifecycle.
//!
//! The page never decides what a plan means. It submits a typed request
//! (base revision + enable/disable + terminology), renders the host's plan
//! (changes, readiness/cost, explicit blocked result), and confirms THE EXACT
//! PLAN by hash + base revision. A changed base or catalogue refuses the
//! confirmation and forces a replan. Restore is previewed as a plan too and
//! lands as a new revision.

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::{json, Value};

use crate::draft::uuid_string;
use crate::session::{self, Answer, FetchError};

#[derive(Clone, Debug, PartialEq)]
enum Load {
    Loading,
    Ready(Value),
    Refused { reason: String, message: String },
    Unreachable(String),
}

#[derive(Clone, Debug, PartialEq)]
enum Action {
    Idle,
    Busy,
    Applied(String),
    Refused { reason: String, message: String },
    Offline(String),
}

fn refusal(answer: &Answer) -> Action {
    Action::Refused {
        reason: answer.reason().unwrap_or("refused").to_string(),
        message: answer.body["message"]
            .as_str()
            .unwrap_or("The host refused the request.")
            .to_string(),
    }
}

fn change_text(change: &Value) -> String {
    match change["kind"].as_str() {
        Some("enable") => format!("Enable {}", change["module"].as_str().unwrap_or("?")),
        Some("disable") => format!("Disable {}", change["module"].as_str().unwrap_or("?")),
        Some("relabelStage") => format!(
            "Label {} → {}",
            change["stage"].as_str().unwrap_or("?"),
            change["label"].as_str().unwrap_or("?")
        ),
        _ => change.to_string(),
    }
}

fn effect_text(effect: &Value) -> String {
    format!(
        "{}: {}, {}",
        effect["module"].as_str().unwrap_or("?"),
        effect["readiness"].as_str().unwrap_or("?"),
        effect["cost"].as_str().unwrap_or("?")
    )
}

#[component]
pub fn Owner() -> impl IntoView {
    let session = expect_context::<session::SessionSignal>();
    let load = RwSignal::new(Load::Loading);
    let history = RwSignal::new(Vec::<Value>::new());
    let desired = RwSignal::new(Vec::<String>::new());
    let plan = RwSignal::new(None::<Value>);
    // `Some(target)` means the plan is a restore preview.
    let plan_mode = RwSignal::new(None::<i64>);
    let action = RwSignal::new(Action::Idle);

    let refresh = move || {
        spawn_local(async move {
            let Some(held) = session::held() else {
                return;
            };
            let tenant = held.tenant;
            match session::api_fetch("GET", &format!("/own/config?tenant={tenant}"), None).await {
                Ok(Answer { status: 200, body }) => {
                    let enabled: Vec<String> = body["modules"]
                        .as_array()
                        .map(|modules| {
                            modules
                                .iter()
                                .filter(|module| module["enabled"] == true)
                                .filter_map(|module| module["id"].as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default();
                    desired.set(enabled);
                    load.set(Load::Ready(body));
                }
                Ok(answer) => load.set(Load::Refused {
                    reason: answer.reason().unwrap_or("refused").to_string(),
                    message: answer.body["message"]
                        .as_str()
                        .unwrap_or("The owner configuration could not be read.")
                        .to_string(),
                }),
                Err(FetchError::NoAnswer(why)) => load.set(Load::Unreachable(why)),
            }
            match session::api_fetch("GET", &format!("/own/history?tenant={tenant}"), None).await {
                Ok(Answer { status: 200, body }) => {
                    history.set(body["revisions"].as_array().cloned().unwrap_or_default());
                }
                _ => history.set(Vec::new()),
            }
        });
    };

    // Restore the facts AFTER the session is available (the SSR pass cannot
    // see it, exactly like every other page).
    Effect::new(move |_| {
        if session.get().is_some() {
            refresh();
        }
    });

    let preview = move |restore_target: Option<i64>| {
        let Some(held) = session::held() else {
            return;
        };
        let tenant = held.tenant;
        action.set(Action::Busy);
        spawn_local(async move {
            let (path, body) = match restore_target {
                Some(target) => (
                    "/own/restore/plan".to_string(),
                    json!({"tenant": tenant, "targetRevision": target}),
                ),
                None => {
                    let Load::Ready(config) = load.get_untracked() else {
                        return;
                    };
                    let base = config["config"]["revision"].as_i64().unwrap_or(0);
                    let current: Vec<String> = config["modules"]
                        .as_array()
                        .map(|modules| {
                            modules
                                .iter()
                                .filter(|module| module["enabled"] == true)
                                .filter_map(|module| module["id"].as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default();
                    let wanted = desired.get_untracked();
                    let enable: Vec<String> = wanted
                        .iter()
                        .filter(|id| !current.contains(id))
                        .cloned()
                        .collect();
                    let disable: Vec<String> = current
                        .iter()
                        .filter(|id| !wanted.contains(id))
                        .cloned()
                        .collect();
                    (
                        "/own/plan".to_string(),
                        json!({
                            "tenant": tenant,
                            "request": {
                                "baseRevision": base,
                                "enable": enable,
                                "disable": disable,
                            }
                        }),
                    )
                }
            };
            match session::api_fetch("POST", &path, Some(body.to_string())).await {
                Ok(Answer { status: 200, body }) => {
                    plan.set(Some(body["plan"].clone()));
                    plan_mode.set(restore_target);
                    action.set(Action::Idle);
                }
                Ok(answer) => action.set(refusal(&answer)),
                Err(FetchError::NoAnswer(why)) => action.set(Action::Offline(why)),
            }
        });
    };

    let confirm = move || {
        let Some(held) = session::held() else {
            return;
        };
        let Some(planned) = plan.get_untracked() else {
            return;
        };
        let tenant = held.tenant;
        let hash = planned["planHash"].as_str().unwrap_or_default().to_string();
        let base = planned["baseRevision"].as_i64().unwrap_or_default();
        let request_id = uuid_string();
        let mode = plan_mode.get_untracked();
        action.set(Action::Busy);
        spawn_local(async move {
            let body = match mode {
                Some(target) => json!({
                    "tenant": tenant,
                    "requestId": request_id,
                    "targetRevision": target,
                    "confirmation": {"planHash": hash, "baseRevision": base},
                }),
                None => {
                    let Load::Ready(config) = load.get_untracked() else {
                        return;
                    };
                    let current: Vec<String> = config["modules"]
                        .as_array()
                        .map(|modules| {
                            modules
                                .iter()
                                .filter(|module| module["enabled"] == true)
                                .filter_map(|module| module["id"].as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default();
                    let wanted = desired.get_untracked();
                    json!({
                        "tenant": tenant,
                        "requestId": request_id,
                        "request": {
                            "baseRevision": base,
                            "enable": wanted.iter().filter(|id| !current.contains(id)).cloned().collect::<Vec<_>>(),
                            "disable": current.iter().filter(|id| !wanted.contains(id)).cloned().collect::<Vec<_>>(),
                        },
                        "confirmation": {"planHash": hash, "baseRevision": base},
                    })
                }
            };
            let path = if mode.is_some() {
                "/own/restore"
            } else {
                "/own/apply"
            };
            match session::api_fetch("POST", path, Some(body.to_string())).await {
                Ok(Answer { status: 200, body }) => {
                    let revision = body["receipt"]["revision"]
                        .as_i64()
                        .map(|revision| revision.to_string())
                        .unwrap_or_else(|| "?".to_string());
                    let outcome = body["receipt"]["outcome"].as_str().unwrap_or("apply");
                    plan.set(None);
                    plan_mode.set(None);
                    action.set(Action::Applied(format!(
                        "{outcome} committed revision {revision}"
                    )));
                    refresh();
                }
                Ok(answer) => {
                    // The plan the user saw is no longer confirmable.
                    plan.set(None);
                    plan_mode.set(None);
                    action.set(refusal(&answer));
                }
                Err(FetchError::NoAnswer(why)) => action.set(Action::Offline(why)),
            }
        });
    };

    view! {
        <section>
            <h1>"Owner configuration"</h1>
            <p class="hint">
                "Inspect the installed modules, plan a change, preview its real effects, "
                "confirm the exact plan, then restore a prior revision if needed."
            </p>

            <p data-testid="owner-state">
                {move || match load.get() {
                    Load::Loading => "Loading the owner configuration…".to_string(),
                    Load::Ready(config) => format!(
                        "Configuration revision {} · catalogue {} · pricing {}",
                        config["config"]["revision"].as_i64().unwrap_or(0),
                        config["catalogueVersion"].as_str().unwrap_or("?"),
                        config["pricingVersion"].as_str().unwrap_or("?"),
                    ),
                    Load::Refused { reason, .. } => format!("Refused: {reason}"),
                    Load::Unreachable(why) => format!("Unreachable: {why}"),
                }}
            </p>
            {move || match load.get() {
                Load::Refused { reason, message } => view! {
                    <p class="refusal" data-testid="owner-refusal">
                        <strong>"Refused: "</strong><code>{reason}</code>" — "{message}
                    </p>
                }.into_any(),
                _ => view! { <p></p> }.into_any(),
            }}

            {move || match load.get() {
                Load::Ready(config) => {
                    let can_apply = config["grants"]["apply"] == true;
                    let current_revision = config["config"]["revision"].as_i64().unwrap_or(0);
                    let modules = config["modules"].as_array().cloned().unwrap_or_default();
                    let rows = modules.into_iter().map(|module| {
                        let id = module["id"].as_str().unwrap_or("?").to_string();
                        let mandatory = module["mandatory"] == true;
                        let toggle_id = id.clone();
                        let state_id = id.clone();
                        let label_id = id.clone();
                        view! {
                            <li data-testid=format!("owner-module-{id}")>
                                <strong>{id.clone()}</strong>
                                " — "
                                {if mandatory { "mandatory" } else { "optional" }}
                                ", "
                                {module["readiness"].as_str().unwrap_or("?").to_string()}
                                ", "
                                {module["cost"].as_str().unwrap_or("?").to_string()}
                                " · "
                                <span>
                                    {move || if desired.get().contains(&state_id) { "enabled" } else { "disabled" }}
                                </span>
                                " "
                                <button
                                    data-testid=format!("owner-toggle-{toggle_id}")
                                    disabled=move || mandatory || !can_apply
                                    on:click=move |_| {
                                        let mut next = desired.get_untracked();
                                        if let Some(position) = next.iter().position(|held| held == &toggle_id) {
                                            next.remove(position);
                                        } else {
                                            next.push(toggle_id.clone());
                                        }
                                        desired.set(next);
                                        plan.set(None);
                                        plan_mode.set(None);
                                    }
                                >
                                    {move || if desired.get().contains(&label_id) { "Disable" } else { "Enable" }}
                                </button>
                            </li>
                        }
                    }).collect_view();
                    let disabled = !can_apply;
                    view! {
                        <ul data-testid="owner-modules">{rows}</ul>
                        {(!can_apply).then(|| view! {
                            <p class="refusal" data-testid="owner-read-only">
                                "This principal has read access only; changes require config.apply."
                            </p>
                        })}
                        <button
                            data-testid="owner-preview"
                            disabled=move || disabled
                            on:click=move |_| preview(None)
                        >
                            "Preview change"
                        </button>
                        <span class="hint">
                            "base revision " {current_revision.to_string()}
                        </span>
                    }.into_any()
                }
                _ => view! { <p></p> }.into_any(),
            }}

            {move || match action.get() {
                Action::Idle => view! { <p></p> }.into_any(),
                Action::Busy => view! { <p class="status" data-testid="owner-status">"Submitting…"</p> }.into_any(),
                Action::Applied(text) => view! { <p class="status" data-testid="owner-status">{text}</p> }.into_any(),
                Action::Refused { reason, message } => view! {
                    <p class="refusal" data-testid="owner-status">
                        <strong>"Refused: "</strong><code>{reason}</code>" — "{message}
                    </p>
                }.into_any(),
                Action::Offline(why) => view! {
                    <p class="refusal" data-testid="owner-status">{why}</p>
                }.into_any(),
            }}

            {move || match plan.get() {
                None => view! { <p></p> }.into_any(),
                Some(planned) => {
                    let blocked = &planned["blocked"];
                    let changes: Vec<String> = planned["changes"]
                        .as_array()
                        .map(|rows| rows.iter().map(change_text).collect())
                        .unwrap_or_default();
                    let effects: Vec<String> = planned["effects"]
                        .as_array()
                        .map(|rows| rows.iter().map(effect_text).collect())
                        .unwrap_or_default();
                    let is_blocked = !blocked.is_null();
                    let base = planned["baseRevision"].as_i64().unwrap_or(0);
                    let revision = planned["config"]["revision"].as_i64().unwrap_or(0);
                    let mode_label = match plan_mode.get() {
                        Some(target) => format!("Restore revision {target}"),
                        None => format!("Apply revision {revision}"),
                    };
                    view! {
                        <div data-testid="owner-plan">
                            <h2>{mode_label}</h2>
                            <p class="hint">
                                "plan " <code>{planned["planHash"].as_str().unwrap_or("?").to_string()}</code>
                                " · base revision " {base.to_string()}
                            </p>
                            {if is_blocked {
                                let reason = blocked["reason"].as_str().unwrap_or("blocked").to_string();
                                let detail = blocked["detail"].as_str().unwrap_or("").to_string();
                                view! {
                                    <p class="refusal" data-testid="owner-blocked">
                                        <strong>"Blocked: "</strong><code>{reason}</code>" — "{detail}
                                    </p>
                                }.into_any()
                            } else {
                                view! { <p></p> }.into_any()
                            }}
                            <h3>"Changes"</h3>
                            <ul data-testid="owner-changes">
                                {changes.into_iter().map(|line| view! { <li>{line}</li> }).collect_view()}
                            </ul>
                            <h3>"Effects"</h3>
                            <ul data-testid="owner-effects">
                                {effects.into_iter().map(|line| view! { <li>{line}</li> }).collect_view()}
                            </ul>
                            <button
                                data-testid="owner-confirm"
                                disabled=move || is_blocked
                                on:click=move |_| confirm()
                            >
                                "Confirm and apply"
                            </button>
                        </div>
                    }.into_any()
                }
            }}

            <h2>"History"</h2>
            <ul data-testid="owner-history">
                {move || history.get().into_iter().map(|row| {
                    let revision = row["revision"].as_i64().unwrap_or(0);
                    let restore_revision = revision;
                    view! {
                        <li data-testid=format!("owner-revision-{revision}")>
                            <strong>"r" {revision.to_string()}</strong>
                            " · " {row["outcome"].as_str().unwrap_or("?").to_string()}
                            " · " {row["actor"].as_str().unwrap_or("?").to_string()}
                            " · " {row["createdAt"].as_str().unwrap_or("").to_string()}
                            " · " <code>{row["planHash"].as_str().unwrap_or("?").to_string()}</code>
                            " "
                            <button
                                data-testid=format!("owner-restore-{revision}")
                                on:click=move |_| preview(Some(restore_revision))
                            >
                                "Preview restore"
                            </button>
                        </li>
                    }
                }).collect_view()}
            </ul>
        </section>
    }
}
