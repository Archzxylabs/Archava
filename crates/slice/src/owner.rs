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
use crate::pending::{self, PendingMarker};
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
    Refused {
        reason: String,
        message: String,
    },
    Offline(String),
    /// An operation whose key is still in storage: the host answered "no such
    /// operation", so the key is the right one to retry with.
    Pending(String),
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

/// The operation id this intent already owns, or a fresh one.
///
/// An intent is the user's decision to apply ONE plan. A click, a retry after
/// the plan was refused and a reload are all the same intent, so the key is
/// taken from the marker and reused. Minting it per click is what made a lost
/// response unrecoverable: the reload minted a second key, the first
/// operation's outcome became unaskable, and a retry could commit twice.
///
/// A marker recorded by ANOTHER account or workspace is never adopted —
/// adopting it would let this browser read (or replay) someone else's
/// operation, which is the same capability leak the host now refuses with
/// `request_id_reused`.
fn reuse_or_mint(held: Option<PendingMarker>, principal: &str, tenant: &str) -> String {
    match held {
        Some(marker) if marker.principal == principal && marker.tenant == tenant => {
            marker.request_id
        }
        _ => uuid_string(),
    }
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
    // The key of the operation this browser believes is unresolved, if any.
    let pending_id = RwSignal::new(None::<String>);

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

    // A marker in storage is an operation whose outcome this browser never
    // saw. Ask the host with the ORIGINAL key: that question — not a reload
    // and not a fresh intent — is what makes a lost response recoverable.
    let recover = move || {
        spawn_local(async move {
            let Some(held) = session::held() else {
                return;
            };
            let Some(marker) = pending::read_scoped(&held.tenant, "owner", &held.principal) else {
                return;
            };
            pending_id.set(Some(marker.request_id.clone()));
            let path = format!(
                "/own/status?tenant={}&requestId={}",
                held.tenant, marker.request_id
            );
            match session::api_fetch("GET", &path, None).await {
                // The host committed it. The outcome is known, so the key is
                // spent and the page reports ONE effect, not a second attempt.
                Ok(Answer { status: 200, body })
                    if crate::receipt::owner(&body, &marker.request_id) =>
                {
                    pending::clear_by_request_id(&held.tenant, &held.principal, &marker.request_id);
                    pending_id.set(None);
                    let revision = body["receipt"]["revision"].as_i64().unwrap_or(0);
                    let outcome = body["receipt"]["outcome"].as_str().unwrap_or("apply");
                    action.set(Action::Applied(format!(
                        "recovered {outcome} committed revision {revision}"
                    )));
                    refresh();
                }
                // The host has no such operation: it never committed, so the
                // key stays the right one to retry with.
                Ok(Answer { status: 200, body })
                    if body["decision"].as_str() == Some("unknown") =>
                {
                    action.set(Action::Pending(marker.request_id.clone()));
                }
                Ok(answer) => action.set(refusal(&answer)),
                Err(FetchError::NoAnswer(why)) => action.set(Action::Offline(why)),
            }
        });
    };

    // Restore the facts AFTER the session is available (the SSR pass cannot
    // see it, exactly like every other page).
    Effect::new(move |_| {
        if session.get().is_some() {
            refresh();
            recover();
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
        let principal = held.principal;
        let hash = planned["planHash"].as_str().unwrap_or_default().to_string();
        let base = planned["baseRevision"].as_i64().unwrap_or_default();
        // ONE key per intent, reused across clicks, retries and reloads.
        let existing = pending::read_scoped(&tenant, "owner", &principal);
        let resuming = existing.is_some();
        let request_id = reuse_or_mint(existing, &principal, &tenant);
        // Record it BEFORE the request leaves. A submission that cannot record
        // its own identity is a submission nobody could ever recover, so it
        // does not happen (see `pending`'s module docs).
        let marker = PendingMarker::with_domain(
            request_id.clone(),
            tenant.clone(),
            "owner",
            principal.clone(),
        );
        if let Err(why) = pending::write(&marker) {
            action.set(Action::Offline(why));
            return;
        }
        pending_id.set(Some(request_id.clone()));
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
                Ok(Answer { status: 200, body }) if crate::receipt::owner(&body, &request_id) => {
                    // Committed: the outcome is known, so the marker is spent.
                    pending::clear_by_request_id(&tenant, &principal, &request_id);
                    pending_id.set(None);
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
                Ok(answer) if !(400..500).contains(&answer.status) => {
                    // 5xx is the other half of a lost response: the host may
                    // have committed and then failed to answer, so the key is
                    // NOT spent — reloading is the only way to find out.
                    action.set(Action::Offline(format!(
                        "the host answered {} — operation {request_id} may be committed; reload to recover its outcome.",
                        answer.status
                    )))
                }
                Ok(answer) if resuming => {
                    action.set(Action::Offline(format!("The retry was refused ({}). Original reference {request_id} is retained; check its status or review the original plan again.",answer.status)));
                }
                Ok(answer) => {
                    // A 4xx refusal wrote nothing (the host is transactional),
                    // so the key is spent and the plan is no longer confirmable.
                    pending::clear_by_request_id(&tenant, &principal, &request_id);
                    pending_id.set(None);
                    plan.set(None);
                    plan_mode.set(None);
                    action.set(refusal(&answer));
                }
                Err(FetchError::NoAnswer(why)) => {
                    // Unknown: the transport died and the server may well have
                    // committed it. The marker stays — that key is the only
                    // thing that can answer the question on reload.
                    action.set(Action::Offline(format!(
                        "{why} — operation {request_id} is still pending; reload to recover its outcome."
                    )))
                }
            }
        });
    };

    let review_section = NodeRef::<leptos::html::Section>::new();
    Effect::new(move |_| {
        if plan.get().is_some() {
            #[cfg(target_arch = "wasm32")]
            if let Some(element) = review_section.get() {
                element.scroll_into_view();
                let _ = element.focus();
            }
        }
    });

    view! {
        <section class="owner">
            <header class="page-heading">
            <h1>"Workspace settings"</h1>
            <p class="hint">
                "Configure your modules, review each change, and keep a clear history."
            </p>

            </header>
            <div class="settings-layout">
                <nav class="settings-navigation" aria-label="Settings sections">
                    <a href="#settings-modules">"Installed modules"</a>
                    <a href="#settings-review">"Review changes"</a>
                    <a href="#settings-history">"Revision history"</a>
                    <p>"Changes are previewed before they are applied."</p>
                </nav>
                <div class="settings-content">
                <section class="settings-section" id="settings-modules" tabindex="-1" aria-labelledby="settings-modules-heading">
                <h2 id="settings-modules-heading">"Installed modules"</h2>
            <p class="hint" data-testid="owner-state">
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
                                <div class="module-description"><strong>{id.clone()}</strong><p>
                                " — "
                                {if mandatory { "mandatory" } else { "optional" }}
                                ", "
                                {module["readiness"].as_str().unwrap_or("?").to_string()}
                                ", "
                                {module["cost"].as_str().unwrap_or("?").to_string()}
                                </p></div>
                                <span class="module-state">{move || if desired.get().contains(&state_id) { "enabled" } else { "disabled" }}</span>
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

                </section>
                <section class="settings-section settings-review" id="settings-review" node_ref=review_section tabindex="-1" aria-labelledby="settings-review-heading">
                    <h2 id="settings-review-heading">"Review changes"</h2>
                    <p class="hint">"Preview your selection to see its changes and effects before confirming."</p>
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
                Action::Pending(id) => view! {
                    <p class="status" data-testid="owner-status">
                        "Operation " <code>{id}</code>
                        " has no recorded outcome — the host never committed it. Retrying reuses this id."
                    </p>
                }.into_any(),
            }}

            {move || match pending_id.get() {
                None => view! { <p></p> }.into_any(),
                Some(id) => view! {
                    <p class="hint" data-testid="owner-pending">
                        "pending operation " <code data-testid="owner-pending-id">{id}</code>
                        " — a retry reuses this id instead of sending a second operation."
                    </p>
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

                </section>
                <section class="settings-section" id="settings-history" tabindex="-1" aria-labelledby="settings-history-heading">
            <h2 id="settings-history-heading">"Revision history"</h2>
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
                </div>
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::{reuse_or_mint, PendingMarker};

    fn marker(request_id: &str, tenant: &str, principal: &str) -> PendingMarker {
        PendingMarker {
            request_id: request_id.to_string(),
            tenant: tenant.to_string(),
            domain: "owner".to_string(),
            principal: principal.to_string(),
            created_at: "2026-10-22T00:00:00.000Z".to_string(),
        }
    }

    /// The whole defect: a reload must ask about the SAME operation, not mint
    /// a new one. The marker is the intent's memory of its own key.
    #[test]
    fn an_intent_keeps_the_key_its_marker_recorded() {
        let held = marker("8b1c4f0e-0000-4000-8000-000000000001", "acme", "alice");
        assert_eq!(
            reuse_or_mint(Some(held), "alice", "acme"),
            "8b1c4f0e-0000-4000-8000-000000000001"
        );
    }

    /// A key recorded by another account or workspace is never adopted: doing
    /// so would let this browser read (or replay) a foreign operation, which
    /// the host answers with `request_id_reused`.
    #[test]
    fn a_marker_from_another_account_is_never_adopted() {
        let foreign = marker("8b1c4f0e-0000-4000-8000-000000000002", "acme", "mallory");
        assert_ne!(
            reuse_or_mint(Some(foreign), "alice", "acme"),
            "8b1c4f0e-0000-4000-8000-000000000002"
        );

        let other_workspace = marker("8b1c4f0e-0000-4000-8000-000000000003", "globex", "alice");
        assert_ne!(
            reuse_or_mint(Some(other_workspace), "alice", "acme"),
            "8b1c4f0e-0000-4000-8000-000000000003"
        );
    }
}
