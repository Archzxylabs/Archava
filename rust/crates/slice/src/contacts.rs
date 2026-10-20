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

use crate::draft::uuid_string;
use crate::pending::{self, PendingMarker};
use crate::session::{self, Answer, FetchError};

/// Percent-encode a query value (RFC 3986 unreserved bytes pass through) so
/// a contact named "A&B iter%" survives the URL round-trip its draft link
/// makes: a raw `%` or `&` in a name would otherwise rewrite the query string
/// the draft page parses.
pub(crate) fn query_encode(value: &str) -> String {
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

/// The new-contact form's outcome. `Blocked` is an UNRESOLVED earlier
/// submission: the marker survived, so a fresh intent would be a second
/// command under a new key.
#[derive(Clone)]
enum CreateState {
    Idle,
    Busy,
    Recorded { id: String },
    Refused { reason: String, message: String },
    Unknown(String),
    Blocked { request_id: String },
}

/// The `/contacts` route.
#[component]
pub fn Contacts() -> impl IntoView {
    let session = expect_context::<session::SessionSignal>();
    let navigate = use_navigate();
    let search = RwSignal::new(String::new());
    let (state, set_state) = signal(ContactsState::Waiting);
    // The cursor that opens the NEXT page, exactly as the server returned it,
    // and the cursor that opened THIS page. Two values, because "where am I"
    // and "where can I go" are different facts: the next-page action needs
    // the first, the first-page action needs the second.
    let next_cursor = RwSignal::new(None::<String>);
    let page_cursor = RwSignal::new(None::<String>);

    // One loader, shared by hydration, the Refresh button and the page
    // actions. The filter is a VALUE the list query carries verbatim — the
    // server escapes it, and this page never widens it. The URL is built with
    // percent-encoding: a literal `Ada&limit=1` used to become two query
    // parameters, so the UI showed a contact the encoded API search never
    // returned (the audit's C06 repro).
    let load = move |search_term: String, after: Option<String>, set_hiatus: bool| {
        if set_hiatus {
            set_state.set(ContactsState::Loading);
        }
        let tenant = session::held().map(|held| held.tenant).unwrap_or_default();
        let mut target = format!("/contacts?tenant={}", query_encode(&tenant));
        if !search_term.is_empty() {
            target.push_str(&format!("&search={}", query_encode(&search_term)));
        }
        if let Some(cursor) = after.as_ref() {
            target.push_str(&format!("&after={}", query_encode(cursor)));
        }
        spawn_local(async move {
            match session::api_fetch("GET", &target, None).await {
                Ok(Answer { status: 200, body }) => {
                    let Some(items) = body["items"].as_array() else {
                        next_cursor.set(None);
                        page_cursor.set(None);
                        set_state.set(ContactsState::Broken);
                        return;
                    };
                    let rows: Vec<ContactRow> =
                        items.iter().filter_map(ContactRow::parse).collect();
                    let complete = rows.len() == items.len();
                    next_cursor.set(body["next"].as_str().map(str::to_owned));
                    page_cursor.set(after);
                    set_state.set(if complete {
                        ContactsState::Loaded { rows }
                    } else {
                        ContactsState::Broken
                    });
                }
                Ok(Answer { status, body }) => {
                    next_cursor.set(None);
                    page_cursor.set(None);
                    set_state.set(ContactsState::Refused {
                        reason: body["reason"]
                            .as_str()
                            .unwrap_or(&format!("http {status}"))
                            .to_string(),
                        message: body["message"]
                            .as_str()
                            .unwrap_or("The contact list was refused.")
                            .to_string(),
                    })
                }
                Err(FetchError::NoAnswer(why)) => {
                    next_cursor.set(None);
                    page_cursor.set(None);
                    set_state.set(ContactsState::Unreachable(why));
                }
            }
        })
    };

    // Reload when the session changes: a signed-out state refuses the list,
    // a signed-in state loads its page once — never on every keystroke.
    Effect::new(move || {
        if session.get().is_none() {
            next_cursor.set(None);
            page_cursor.set(None);
            set_state.set(ContactsState::SignedOut);
            return;
        }
        load(String::new(), None, true);
    });

    // The new-contact form: a real `contact.create` over `/crm/commands`.
    // A submission that can commit persists the app-owned marker FIRST (the
    // C05 discipline): a storage refusal blocks the send, an unknown outcome
    // keeps both the key and the marker, and an unresolved marker on mount
    // blocks a fresh attempt with a link to the recovery page.
    let new_name = RwSignal::new(String::new());
    let new_email = RwSignal::new(String::new());
    let new_organization = RwSignal::new(String::new());
    let (create_state, set_create_state) = signal(CreateState::Idle);
    let create_key = RwSignal::new(None::<String>);

    Effect::new(move |_| {
        if session.get().is_none() {
            return;
        }
        let Some(held) = session::held() else {
            return;
        };
        if let Some(marker) = pending::read(&held.tenant, &held.principal) {
            set_create_state.set(CreateState::Blocked {
                request_id: marker.request_id,
            });
        }
    });

    let create = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        if matches!(create_state.get_untracked(), CreateState::Blocked { .. }) {
            return;
        }
        let Some(held) = session::held() else {
            return;
        };
        let key = match create_key.get_untracked() {
            Some(key) => key,
            None => {
                let key = uuid_string();
                create_key.set(Some(key.clone()));
                key
            }
        };
        let marker = PendingMarker::new(key.clone(), held.tenant.clone(), held.principal.clone());
        if let Err(why) = pending::write(&marker) {
            set_create_state.set(CreateState::Refused {
                reason: "pending_unavailable".to_string(),
                message: format!("The pending marker could not be stored: {why}"),
            });
            return;
        }
        let name = new_name.get_untracked();
        let email = new_email.get_untracked();
        let organization = new_organization.get_untracked();
        let optional = |value: String| {
            if value.trim().is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::Value::String(value)
            }
        };
        let body = serde_json::json!({
            "tenant": held.tenant,
            "requestId": key,
            "command": {
                "operation": "contact.create",
                "data": {
                    "fullName": name,
                    "email": optional(email),
                    "organization": optional(organization),
                },
            },
        })
        .to_string();
        set_create_state.set(CreateState::Busy);
        let tenant = held.tenant.clone();
        spawn_local(async move {
            match session::api_fetch("POST", "/crm/commands", Some(body)).await {
                Ok(Answer { status: 200, body }) if body["decision"] == "receipt" => {
                    pending::clear(&tenant);
                    create_key.set(None);
                    let id = body["receipt"]["id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    set_create_state.set(CreateState::Recorded { id });
                    load(String::new(), None, false);
                }
                Ok(Answer { status, body }) => {
                    pending::clear(&tenant);
                    create_key.set(None);
                    set_create_state.set(CreateState::Refused {
                        reason: body["reason"]
                            .as_str()
                            .unwrap_or(&format!("http {status}"))
                            .to_string(),
                        message: body["message"]
                            .as_str()
                            .unwrap_or("The contact was refused.")
                            .to_string(),
                    });
                }
                Err(FetchError::NoAnswer(why)) => {
                    // The command may have committed. Keep the key AND the
                    // marker: a retry must be the SAME command.
                    set_create_state.set(CreateState::Unknown(why));
                }
            }
        });
    };

    let refresh = move |_event: leptos::ev::MouseEvent| {
        // Refresh restarts at the first page: the filter in the box is the
        // query, and a cursor belongs to the query it came from.
        load(search.get_untracked(), None, true);
    };

    // The next page: the server's own cursor, carried verbatim.
    let next_page = move |_event: leptos::ev::MouseEvent| {
        let Some(cursor) = next_cursor.get_untracked() else {
            return;
        };
        load(search.get_untracked(), Some(cursor), true);
    };

    // Back to the first page.
    let first_page = move |_event: leptos::ev::MouseEvent| {
        load(search.get_untracked(), None, true);
    };

    view! {
        <section class="contacts">
            <h1 data-testid="contacts-heading">"Contacts"</h1>
            <div class="search">
                <label>
                    "Filter by name "
                    <input
                        data-testid="contacts-search"
                        value=search
                        on:input=move |ev| {
                            let value = event_target_value(&ev);
                            search.set(value);
                            // A new filter invalidates the page position: the
                            // cursor belonged to the previous query, and
                            // carrying it into another one would page through
                            // rows the new filter does not match. The list
                            // stays on screen; the next-page action goes away
                            // until the new filter is applied.
                            next_cursor.set(None);
                            page_cursor.set(None);
                        }
                    />
                </label>
                <button data-testid="contacts-refresh" on:click=refresh>"Refresh"</button>
            </div>

            <form class="create" data-testid="contact-create-form" on:submit=create>
                <h2>"New contact"</h2>
                <label>
                    "Name "
                    <input data-testid="contact-new-name" value=new_name
                        on:input=move |ev| new_name.set(event_target_value(&ev)) />
                </label>
                <label>
                    "Email "
                    <input data-testid="contact-new-email" value=new_email
                        on:input=move |ev| new_email.set(event_target_value(&ev)) />
                </label>
                <label>
                    "Organization "
                    <input data-testid="contact-new-org" value=new_organization
                        on:input=move |ev| new_organization.set(event_target_value(&ev)) />
                </label>
                <button data-testid="contact-create"
                    disabled=move || matches!(create_state.get(), CreateState::Blocked { .. })>
                    "Create"
                </button>
                {let create_navigate = navigate.clone();
                move || match create_state.get() {
                    CreateState::Idle => view! { <p></p> }.into_any(),
                    CreateState::Busy => view! {
                        <p class="status" data-testid="contact-create-status">"Creating…"</p>
                    }
                    .into_any(),
                    CreateState::Recorded { id } => {
                        let target = format!("/app/contact?id={id}");
                        let navigate = create_navigate.clone();
                        view! {
                            <p class="status" data-testid="contact-create-status">
                                "Recorded. Receipt: " <code>{id.clone()}</code>
                            </p>
                            <button data-testid="contact-create-open"
                                on:click=move |_| navigate(&target, Default::default())>
                                "Open the new record"
                            </button>
                        }
                        .into_any()
                    }
                    CreateState::Refused { reason, message } => view! {
                        <p class="refusal" data-testid="contact-create-status">
                            <strong>"Refused: "</strong><code>{reason}</code>" — "{message}
                        </p>
                    }
                    .into_any(),
                    CreateState::Unknown(why) => view! {
                        <p class="refusal" data-testid="contact-create-status">
                            {why}" — retry sends the same request id, never a second command"
                        </p>
                    }
                    .into_any(),
                    CreateState::Blocked { request_id } => {
                        let href = format!("/app/recover?id={request_id}");
                        view! {
                            <p class="refusal" data-testid="contact-create-status">
                                "An earlier submission is unresolved (request " <code>{request_id}</code>
                                "). Resolve it before creating another contact: "
                                <a data-testid="contact-create-recover" href=href>"the recovery page"</a>
                            </p>
                        }
                        .into_any()
                    }
                }}
            </form>
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
                ContactsState::Loaded { rows } => {
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
                                        let navigate_open = navigate.clone();
                                        let row_id = row.id.clone();
                                        let row_revision = row.revision;
                                        let row_name = query_encode(&row.full_name);
                                        let open_id = row_id.clone();
                                        view! {
                                            <tr data-testid="contact-row">
                                                <td>{row.full_name}</td>
                                                <td>{row.email.unwrap_or_default()}</td>
                                                <td>{row.owner_id}</td>
                                                <td>{row.revision.to_string()}</td>
                                                <td>
                                                    <button
                                                        data-testid="contact-open"
                                                        on:click=move |_event| {
                                                            let target = format!("/app/contact?id={open_id}");
                                                            navigate_open(&target, Default::default());
                                                        }
                                                    >
                                                        "Open"
                                                    </button>
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
                        <div class="paging">
                            // The page actions are real: the cursor the
                            // server returned opens the next page, and the
                            // first-page action goes back. Before the audit
                            // (C06) the cursor was only NAMED in a sentence —
                            // the page could not move.
                            <button
                                data-testid="contacts-next-page"
                                disabled=move || next_cursor.get().is_none()
                                on:click=next_page
                            >
                                "Next page"
                            </button>
                            <button
                                data-testid="contacts-first-page"
                                disabled=move || page_cursor.get().is_none()
                                on:click=first_page
                            >
                                "First page"
                            </button>
                            <p class="hint" data-testid="contacts-page-note">
                                {move || match next_cursor.get() {
                                    Some(cursor) => {
                                        format!("a further page exists after {cursor}")
                                    }
                                    None => "this page is the last".to_string(),
                                }}
                            </p>
                        </div>
                    }
                }
                .into_any(),
            }}
        </section>
    }
}
