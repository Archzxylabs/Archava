//! A bounded, session-scoped CRM overview. Counts describe the first page
//! returned by the contacts endpoint, never invented workspace-wide totals.

use std::collections::BTreeSet;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::contact_context::initials;
use crate::contacts::query_encode;
use crate::session::{self, Answer, SessionSignal};
use crate::workspace::WorkspaceIcon;

#[derive(Clone)]
struct DirectoryContact {
    id: String,
    name: String,
    email: Option<String>,
    organization: Option<String>,
    owner: String,
}

#[derive(Clone)]
struct DirectoryPage {
    contacts: Vec<DirectoryContact>,
    more: bool,
}

impl DirectoryPage {
    fn parse(body: &serde_json::Value) -> Option<Self> {
        let contacts = body["items"]
            .as_array()?
            .iter()
            .map(|item| {
                Some(DirectoryContact {
                    id: item["id"].as_str()?.into(),
                    name: item["fullName"].as_str()?.into(),
                    email: item["email"]
                        .as_str()
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned),
                    organization: item["organization"]
                        .as_str()
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned),
                    owner: item["ownerId"].as_str()?.into(),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        // An unexpected cursor shape is an invalid response, not the last page.
        let more = match &body["next"] {
            serde_json::Value::Null => false,
            serde_json::Value::String(cursor) if !cursor.is_empty() => true,
            _ => return None,
        };
        Some(Self { contacts, more })
    }
}

#[derive(Clone)]
enum OverviewState {
    SignedOut,
    Loading,
    Ready(DirectoryPage),
    Unavailable(String),
}

#[component]
fn DirectoryOverview(page: DirectoryPage) -> impl IntoView {
    let total = page.contacts.len();
    let with_email = page
        .contacts
        .iter()
        .filter(|contact| contact.email.is_some())
        .count();
    let with_organization = page
        .contacts
        .iter()
        .filter(|contact| contact.organization.is_some())
        .count();
    let organizations = page
        .contacts
        .iter()
        .filter_map(|contact| contact.organization.as_deref())
        .collect::<BTreeSet<_>>()
        .len();
    let assigned = page
        .contacts
        .iter()
        .filter(|contact| !contact.owner.is_empty())
        .count();
    let coverage = [
        ("Email address", with_email),
        ("Organization", with_organization),
        ("Record owner", assigned),
    ];
    let stats = [
        ("contacts", "Contacts in view", total),
        ("organization", "Organizations in view", organizations),
        ("mail", "Contacts with email", with_email),
    ];
    view! {
        <div class="overview-stats" aria-label="Contact page summary">
            {stats.into_iter().map(|(_icon, label, count)| view! {
                <div class="overview-stat"><strong data-testid="home-stat">{count}</strong><span>{label}</span></div>
            }).collect_view()}
        </div>
        <div class="overview-scope" data-testid="home-scope">
            <span class="scope-dot" aria-hidden="true"></span>
            {if page.more { "First 20 accessible contacts · more contacts available" } else { "First page of accessible contacts" }}
            <span class="scope-detail">"Counts describe this view."</span>
        </div>
        <div class="overview-grid">
            <div class="overview-primary">
                <section class="directory-panel" aria-labelledby="directory-heading">
                    <div class="panel-heading"><div><h2 id="directory-heading">"People & relationships"</h2><p class="panel-description">"The context for your next conversation starts here."</p></div><a href="/app/contacts">"All contacts"<WorkspaceIcon name="arrow" /></a></div>
                    {if total == 0 {
                        view! {
                            <div class="dashboard-empty"><WorkspaceIcon name="contacts" /><h3>"Start with your first contact"</h3><p>"Add a person to keep their details, notes, and next steps together."</p><a class="action-link" href="/app/contacts">"Open contacts"</a></div>
                        }.into_any()
                    } else {
                        let remaining = total.saturating_sub(5);
                        view! {
                            <ul class="dashboard-contacts">
                                {page.contacts.into_iter().take(5).map(|contact| {
                                    let href = format!("/app/contact?id={}", query_encode(&contact.id));
                                    let subtitle = contact.email.unwrap_or_else(|| contact.organization.clone().unwrap_or("Open contact details".into()));
                                    view! {
                                        <li><a href=href data-testid="home-contact">
                                            <span class="directory-initials" aria-hidden="true">{initials(&contact.name)}</span>
                                            <span class="directory-person"><strong>{contact.name}</strong><small>{subtitle}</small></span>
                                            <span class="directory-organization"><small>"Organization"</small><span>{contact.organization.unwrap_or("Not added".into())}</span></span>
                                            <span class="directory-owner"><small>"Owner"</small><span>{contact.owner}</span></span>
                                            <WorkspaceIcon name="arrow" />
                                        </a></li>
                                    }
                                }).collect_view()}
                            </ul>
                            {(remaining > 0).then(|| view! { <p class="directory-remaining">{format!("{remaining} more in this view. ")}<a href="/app/contacts">"Browse the directory"</a></p> })}
                        }.into_any()
                    }}
                </section>
                <div class="workspace-paths">
                    <a class="assistant-path" href="/app/desk"><WorkspaceIcon name="desk" /><h2>"Think it through."</h2><p>"Bring your next question to the workspace assistant."</p><span>"Open assistant"<WorkspaceIcon name="arrow" /></span></a>
                    <a class="operations-path" href="/app/erp"><div class="path-caption"><WorkspaceIcon name="erp" /><span class="preview-badge">"Operations"</span></div><h2>"Run your daily work."</h2><p>"Manage products, orders, purchases, invoices and support in your workspace."</p><span>"Open operations"<WorkspaceIcon name="arrow" /></span></a>
                </div>
            </div>
            <aside class="overview-secondary" aria-label="Workspace context">
                <section class="coverage-panel" aria-labelledby="coverage-heading">
                    <div class="panel-heading"><h2 id="coverage-heading">"A fuller picture"</h2></div>
                    <p class="panel-description">"Details recorded for the contacts in this view."</p>
                    <div class="coverage-list">
                        {coverage.into_iter().map(|(label, count)| {
                            let percentage = if total == 0 { 0.0 } else { count as f64 / total as f64 * 100.0 };
                            view! {
                                <div class="coverage-row"><div><span>{label}</span><span>{format!("{count} / {total}")}</span></div><div class="coverage-track" role="meter" aria-label=label aria-valuemin="0" aria-valuemax="100" aria-valuenow=percentage.to_string() aria-valuetext=format!("{count} of {total} contacts")><span style=format!("width: {percentage}%")></span></div></div>
                            }
                        }).collect_view()}
                    </div>
                </section>
                <section class="quick-actions" aria-labelledby="next-move-heading">
                    <div class="panel-heading"><h2 id="next-move-heading">"Keep work moving"</h2></div>
                    <a href="/app/recover"><span class="shortcut-icon"><WorkspaceIcon name="recover" /></span><span><strong>"Check a previous action"</strong><small>"Find the result before trying again."</small></span><WorkspaceIcon name="arrow" /></a>
                    <a href="/app/owner"><span class="shortcut-icon"><WorkspaceIcon name="owner" /></span><span><strong>"Shape your workspace"</strong><small>"Review your modules and configuration."</small></span><WorkspaceIcon name="arrow" /></a>
                </section>
            </aside>
        </div>
    }
}

#[component]
pub fn Home() -> impl IntoView {
    let session = expect_context::<SessionSignal>();
    let state = RwSignal::new(OverviewState::SignedOut);
    let refresh = RwSignal::new(0_u64);
    let generation = RwSignal::new(0_u64);
    Effect::new(move |_| {
        let held = session.get();
        let _ = refresh.get();
        generation.update(|value| *value = value.wrapping_add(1));
        let ticket = generation.get_untracked();
        let Some(held) = held else {
            state.set(OverviewState::SignedOut);
            return;
        };
        state.set(OverviewState::Loading);
        let target = format!("/contacts?tenant={}&limit=20", query_encode(&held.tenant));
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
            state.set(match result {
                Ok(Answer { status: 200, body }) => DirectoryPage::parse(&body).map(OverviewState::Ready).unwrap_or_else(|| OverviewState::Unavailable("The contact overview could not be read. Open Contacts or try refreshing.".into())),
                Ok(answer) => OverviewState::Unavailable(answer.message().unwrap_or("Your current access does not allow this contact overview.").into()),
                Err(_) => OverviewState::Unavailable("The overview is temporarily unavailable. Try refreshing in a moment.".into()),
            });
        });
    });
    view! {
        <section class="home" data-testid="home">
            <header class="page-heading home-heading">
                <div><h1>"Your workspace."</h1><p class="hint">"People, context, and the next step."</p></div>
                <div class="page-actions"><button type="button" data-testid="home-refresh" disabled=move || matches!(state.get(), OverviewState::Loading) on:click=move |_| refresh.update(|value| *value = value.wrapping_add(1))><WorkspaceIcon name="recover" />"Refresh"</button><a class="action-link" href="/app/contacts">"Open contacts"<WorkspaceIcon name="arrow" /></a></div>
            </header>
            <div aria-live="polite" aria-busy=move || matches!(state.get(), OverviewState::Loading).to_string()>
                {move || match state.get() {
                    OverviewState::SignedOut => view! { <div class="dashboard-panel overview-message"><h2>"Welcome to your workspace"</h2><p>"Sign in to see the contacts available to you."</p><a class="action-link" href="/app">"Sign in"</a></div> }.into_any(),
                    OverviewState::Loading => view! { <p class="status overview-loading" role="status">"Loading your workspace…"</p> }.into_any(),
                    OverviewState::Unavailable(message) => view! { <div class="dashboard-panel overview-message"><h2>"Overview unavailable"</h2><p class="refusal" data-testid="home-refusal">{message}</p><a href="/app/contacts">"Open contacts"</a></div> }.into_any(),
                    OverviewState::Ready(page) => view! { <DirectoryOverview page=page /> }.into_any(),
                }}
            </div>
        </section>
    }
}
