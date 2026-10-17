//! The contacts page — the real, session-scoped read.
//!
//! The table is what `GET /contacts` answered for THIS session: the rows the
//! membership's scope admits, with the revisions the draft will bind to. A
//! merchant with no contacts-read grant gets the host's refusal rendered —
//! the absence of a grant is NOT an empty list, and the UI keeps those facts
//! apart by showing the refusal's reason.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;

use crate::session::{self, Answer, FetchError};

/// Percent-encode a query value (RFC 3986 unreserved bytes pass through) so
/// a contact named "A&B iter%" survives the URL round-trip its draft link
/// makes: a raw `%` or `&` in a name would otherwise rewrite the query string
/// the draft page parses.
fn query_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.extend(format!("%{byte:02X}").chars()),
        }
    }
    out
}

/// One row of the contact page, exactly the fields `GET /contacts`
/// returns.
#[derive(Clone, Debug, PartialEq)]
struct ContactRow {
    id: String,
    full_name: String,
    email: Option<String>,
    owner_id: String,
    revision: i64,
}

impl ContactRow {
    fn parse(value: &serde_json::Value) -> Option<Self> {
        Some(ContactRow {
            id: value["id"].as_str()?.to_string(),
            full_name: value["fullName"].as_str()?.to_string(),
            email: value["email"].as_str().map(str::to_owned),
            owner_id: value["ownerId"].as_str()?.to_string(),
            revision: value["revision"].as_i64()?,
        })
    }
}

/// The page's possible states. `Loaded` carries the cursor only when the
/// server said a further page exists.
#[derive(Clone)]
enum ContactsState {
    /// Hydration has not finished the first load (or nothing to load yet).
    Waiting,
    Loading,
    Loaded {
        rows: Vec<ContactRow>,
        next: Option<String>,
    },
    Refused {
        reason: String,
        message: String,
    },
    Unreachable(String),
    /// No session (or one whose facts hydration could not restore): the
    /// login page is the answer, not an empty table.
    SignedOut,
    /// The answer did not carry the contract's array shape.
    Broken,
}

/// The `/contacts` route.
#[component]
pub fn Contacts() -> impl IntoView {
    let session = expect_context::<session::SessionSignal>();
    let navigate = use_navigate();
    let search = RwSignal::new(String::new());
    let (state, set_state) = signal(ContactsState::Waiting);

    // One loader, shared by hydration and the Refresh button. The filter is a
    // VALUE the list query carries verbatim — the server escapes it, and this
    // page never widens it.
    let load = move |search_term: String, set_hiatus: bool| {
        if set_hiatus {
            set_state.set(ContactsState::Loading);
        }
        let tenant = session::held().map(|held| held.tenant).unwrap_or_default();
        let target = if search_term.is_empty() {
            format!("/contacts?tenant={tenant}")
        } else {
            format!("/contacts?tenant={tenant}&search={search_term}")
        };
        spawn_local(async move {
            match session::api_fetch("GET", &target, None).await {
                Ok(Answer { status: 200, body }) => {
                    let Some(items) = body["items"].as_array() else {
                        set_state.set(ContactsState::Broken);
                        return;
                    };
                    let rows: Vec<ContactRow> =
                        items.iter().filter_map(ContactRow::parse).collect();
                    let complete = rows.len() == items.len();
                    set_state.set(if complete {
                        ContactsState::Loaded {
                            rows,
                            next: body["next"].as_str().map(str::to_owned),
                        }
                    } else {
                        ContactsState::Broken
                    });
                }
                Ok(Answer { status, body }) => set_state.set(ContactsState::Refused {
                    reason: body["reason"]
                        .as_str()
                        .unwrap_or(&format!("http {status}"))
                        .to_string(),
                    message: body["message"]
                        .as_str()
                        .unwrap_or("The contact list was refused.")
                        .to_string(),
                }),
                Err(FetchError::NoAnswer(why)) => set_state.set(ContactsState::Unreachable(why)),
            }
        })
    };

    // Reload when the session changes: a signed-out state refuses the list,
    // a signed-in state loads its page once — never on every keystroke.
    Effect::new(move || {
        if session.get().is_none() {
            set_state.set(ContactsState::SignedOut);
            return;
        }
        load(String::new(), true);
    });

    let refresh = move |_event: leptos::ev::MouseEvent| load(search.get_untracked(), true);

    view! {
        <section class="contacts">
            <h1 data-testid="contacts-heading">"Contacts"</h1>
            <div class="search">
                <label>
                    "Filter by name "
                    <input
                        data-testid="contacts-search"
                        value=search
                        on:input=move |ev| search.set(event_target_value(&ev))
                    />
                </label>
                <button data-testid="contacts-refresh" on:click=refresh>"Refresh"</button>
            </div>
            {move || match state.get() {
                ContactsState::Waiting | ContactsState::Loading => view! {
                    <p class="status" data-testid="contacts-status">"Loading…"</p>
                }
                .into_any(),
                ContactsState::SignedOut => view! {
                    <p class="refusal" data-testid="contacts-status">
                        "Sign in first — the list is a session-scoped read."
                    </p>
                }
                .into_any(),
                ContactsState::Broken => view! {
                    <p class="refusal" data-testid="contacts-status">
                        "The answer carried rows this contract does not declare."
                    </p>
                }
                .into_any(),
                ContactsState::Unreachable(why) => view! {
                    <p class="refusal" data-testid="contacts-status">{why}</p>
                }
                .into_any(),
                ContactsState::Refused { reason, message } => view! {
                    <p class="refusal" data-testid="contacts-status">
                        <strong>"Refused: "</strong>
                        <code>{reason}</code>" — "{message}
                    </p>
                }
                .into_any(),
                ContactsState::Loaded { rows, next } => {
                    let count = rows.len();
                    view! {
                        <p class="status" data-testid="contacts-count">
                            {format!("{count} contact(s) in the tenant page")}
                        </p>
                        <table>
                            <thead>
                                <tr>
                                    <th>"Name"</th>
                                    <th>"Email"</th>
                                    <th>"Owner"</th>
                                    <th>"Revision"</th>
                                    <th></th>
                                </tr>
                            </thead>
                            <tbody>
                                {rows
                                    .into_iter()
                                    .map(|row| {
                                        let navigate = navigate.clone();
                                        let row_id = row.id.clone();
                                        let row_revision = row.revision;
                                        let row_name = query_encode(&row.full_name);
                                        view! {
                                            <tr data-testid="contact-row">
                                                <td>{row.full_name}</td>
                                                <td>{row.email.unwrap_or_default()}</td>
                                                <td>{row.owner_id}</td>
                                                <td>{row.revision.to_string()}</td>
                                                <td>
                                                    <button
                                                        data-testid="contact-draft"
                                                        on:click=move |_event| {
                                                            let target = format!(
                                                                "/app/draft?contact={row_id}&revision={row_revision}&name={row_name}"
                                                            );
                                                            navigate(&target, Default::default());
                                                        }
                                                    >
                                                        "Draft"
                                                    </button>
                                                </td>
                                            </tr>
                                        }
                                    })
                                    .collect::<Vec<_>>()}
                            </tbody>
                        </table>
                        <p class="hint" data-testid="contacts-page-note">
                            {next
                                .map(|cursor| format!("a further page exists after {cursor}"))
                                .unwrap_or_else(|| "this page is the last".to_string())}
                        </p>
                    }
                }
                .into_any(),
            }}
        </section>
    }
}
