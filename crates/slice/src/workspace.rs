//! Shared product navigation and the user's local appearance preference.

use leptos::prelude::*;
use leptos_router::hooks::use_location;

use crate::session::{SessionMarker, SessionSignal};

/// Small, consistent line icons. Kept in the app so navigation needs no icon
/// font, third-party runtime, or network request.
#[component]
pub(crate) fn WorkspaceIcon(#[prop(into)] name: String) -> impl IntoView {
    let path = match name.as_str() {
        "home" => "m3 10 9-7 9 7 M5 9v11h5v-6h4v6h5V9",
        "contacts" => "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2 M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8 M17 4a4 4 0 0 1 0 8 M22 21v-2a4 4 0 0 0-3-3.87",
        "erp" => "M3 7h18v14H3z M8 7V3h8v4 M3 12h18 M10 12v3h4v-3",
        "desk" => "M4 4h16v12H9l-5 4V4z M8 8h8 M8 12h5",
        "owner" => "M4 7h16 M4 17h16 M9 4v6 M15 14v6",
        "recover" => "M3 11a9 9 0 1 1 2 7 M3 4v7h7 M12 7v5l3 2",
        "sun" => "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8 M12 2v2 M12 20v2 M2 12h2 M20 12h2 M5 5l1.5 1.5 M17.5 17.5 19 19 M5 19l1.5-1.5 M17.5 6.5 19 5",
        "arrow" => "M5 12h14 M13 6l6 6-6 6",
        "followup" => "M8 3v4 M16 3v4 M3 10h18 M3 5h18v16H3z M12 13v5 M9.5 15.5h5",
        "mail" => "M3 5h18v14H3z M3 5l9 7 9-7",
        "organization" => "M4 21V3h12v18 M16 10h4v11 M8 7h4 M8 11h4 M8 15h4 M2 21h20",
        _ => "M12 3v18 M3 12h18",
    };
    view! {
        <svg class="workspace-icon" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
            <path d=path />
        </svg>
    }
}

#[component]
fn WorkspaceNavigation(close_menu: Callback<()>) -> impl IntoView {
    let pathname = use_location().pathname;
    let groups = [
        ("Workspace", vec![("/app/home", "Home", "home")]),
        ("CRM", vec![("/app/contacts", "Contacts", "contacts")]),
        ("ERP", vec![("/app/erp", "Operations", "erp")]),
        (
            "Tools",
            vec![
                ("/app/desk", "Assistant", "desk"),
                ("/app/owner", "Settings", "owner"),
            ],
        ),
    ];
    view! {
        <nav class="workspace-navigation" aria-label="Workspace">
            {groups.into_iter().map(|(group, links)| view! {
                <div class="navigation-group">
                    <p class="navigation-label">{group}</p>
                    {links.into_iter().map(|(href, label, section)| {
                        let is_current = move || {
                            let path = pathname.get();
                            match section {
                                "contacts" => path.starts_with("/app/contact") || path == "/app/draft",
                                _ => path == href,
                            }
                        };
                        view! {
                            <a href=href data-testid=format!("nav-{section}") class:current=is_current
                                aria-current=move || is_current().then_some("page")
                                on:click=move |_| close_menu.run(())>
                                <WorkspaceIcon name=section />
                                <span>{label}</span>
                                <span class="nav-current-dot" aria-hidden="true"></span>
                            </a>
                        }
                    }).collect_view()}
                </div>
            }).collect_view()}
        </nav>
    }
}

#[component]
fn WorkspaceBrand() -> impl IntoView {
    view! {
        <a class="app-brand" href="/app/home" aria-label="Archava home">
            <svg class="brand-mark" width="27" height="30" viewBox="0 0 30 32" fill="none" aria-hidden="true">
                <path d="m14 5 9 21" stroke="currentColor" stroke-width="7" stroke-linecap="round" />
                <path d="m8 18-4 8" stroke="currentColor" stroke-width="7" stroke-linecap="round" />
            </svg>
            <span>"ARCHAVA"</span>
        </a>
    }
}

#[component]
pub fn Workspace(children: Children) -> impl IntoView {
    let dark = RwSignal::new(false);
    let menu_open = RwSignal::new(false);
    let pathname = use_location().pathname;
    let session = expect_context::<SessionSignal>();
    let close_menu = Callback::new(move |()| menu_open.set(false));
    let page_label = move || match pathname.get().as_str() {
        "/app/home" => "Home",
        "/app/contacts" => "Contacts",
        "/app/contact" => "Contact details",
        "/app/draft" => "Follow-up",
        "/app/erp" => "Operations",
        "/app/desk" => "Assistant",
        "/app/owner" => "Settings",
        "/app/recover" => "Action recovery",
        "/app" => "Sign in",
        _ => "Workspace",
    };

    Effect::new(move |_| {
        let _ = pathname.get();
        menu_open.set(false);
    });
    Effect::new(move |_| {
        #[cfg(target_arch = "wasm32")]
        if let Some(storage) =
            web_sys::window().and_then(|window| window.local_storage().ok().flatten())
        {
            dark.set(
                storage
                    .get_item("archava.appearance")
                    .ok()
                    .flatten()
                    .as_deref()
                    == Some("dark"),
            );
        }
    });
    let toggle_theme = move |_| {
        let next = !dark.get_untracked();
        dark.set(next);
        #[cfg(target_arch = "wasm32")]
        if let Some(storage) =
            web_sys::window().and_then(|window| window.local_storage().ok().flatten())
        {
            let _ = storage.set_item("archava.appearance", if next { "dark" } else { "light" });
        }
    };

    view! {
        <div class="archava-app" data-theme=move || if dark.get() { "dark" } else { "light" }>
            <a class="skip-link" href="#workspace-content">"Skip to content"</a>
            <aside class="app-sidebar" aria-label="Main navigation">
                <WorkspaceBrand />
                <WorkspaceNavigation close_menu=close_menu />
                <div class="sidebar-bottom">
                    <a class="sidebar-recover" href="/app/recover"><WorkspaceIcon name="recover" /><span>"Action recovery"</span></a>
                    <div class="workspace-identity">
                        <span class="workspace-emblem" aria-hidden="true">"A"</span>
                        <div><small>"Workspace"</small><strong>{move || session.get().map_or("Sign in to continue".into(), |held| held.tenant)}</strong></div>
                    </div>
                </div>
            </aside>
            <div class="app-frame">
                <header class="app-topbar">
                    <div class="mobile-brand"><WorkspaceBrand /></div>
                    <div class="page-breadcrumb"><span>"Workspace"</span><span aria-hidden="true">"/"</span><strong>{page_label}</strong></div>
                    <details class="mobile-navigation" open=move || menu_open.get()
                        on:keydown=move |event| { if event.key() == "Escape" { menu_open.set(false); } }>
                        <summary aria-label="Workspace menu" aria-expanded=move || menu_open.get().to_string()
                            on:click=move |event| { event.prevent_default(); menu_open.update(|open| *open = !*open); }>
                            <span class="menu-lines" aria-hidden="true"></span>"Menu"
                        </summary>
                        <WorkspaceNavigation close_menu=close_menu />
                    </details>
                    <div class="topbar-actions">
                        <button type="button" class="appearance-toggle" aria-pressed=move || dark.get().to_string()
                            aria-label=move || if dark.get() { "Switch to light appearance" } else { "Switch to dark appearance" }
                            on:click=toggle_theme>
                            <WorkspaceIcon name="sun" />
                            <span>{move || if dark.get() { "Light mode" } else { "Dark mode" }}</span>
                        </button>
                        <SessionMarker />
                    </div>
                </header>
                <main class="workspace-content" id="workspace-content" tabindex="-1">{children()}</main>
                <footer class="workspace-footer"><span>"Archava workspace"</span><a href="/app/recover">"Check a previous action"</a></footer>
            </div>
        </div>
    }
}
