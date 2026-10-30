//! Shared SSR serving helpers for the persistent launcher and fixture preview.
//!
//! The HTTP host owns authority and database readiness. This module only joins
//! that already-constructed host router to the Leptos document shell and the
//! generated browser payload, keeping the preview and persistent entrypoints
//! on the same shipped UI surface.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use archava_host::AppState;
use axum::Router;
use leptos::config::LeptosOptions;
use leptos_axum::LeptosRoutes;
use tower_http::services::ServeDir;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::App;

/// Put the host's response-header policy on a router that does not come from
/// [`archava_host::router`].
///
/// The host router layers the policy over its own routes, and `merge` keeps
/// each route's layer stack: the Leptos document and `/pkg` assets were joined
/// beside it and therefore answered with no CSP, HSTS, `nosniff` or
/// `Referrer-Policy` (audit H10). `default-src 'none'` would blank the shell,
/// so the document policy that admits `/pkg` and the hydration bootstrap is
/// used instead of the JSON one.
fn with_security_headers<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.layer(axum::middleware::from_fn(
        archava_host::document_security_headers,
    ))
}

/// Build the complete application router for a ready host state.
pub fn build_app(state: AppState, site_addr: SocketAddr, pkg_dir: &Path) -> Router {
    let site_root = pkg_dir
        .parent()
        .expect("the wasm package directory must have a site root")
        .to_path_buf();
    let options = LeptosOptions::builder()
        .output_name("archava-slice")
        .site_root(site_root.display().to_string())
        .site_pkg_dir("pkg")
        .site_addr(site_addr)
        .build();
    let routes = leptos_axum::generate_route_list(App);
    let shell_options = options.clone();
    let slice_router: Router<LeptosOptions> = Router::new()
        .nest_service("/pkg", ServeDir::new(pkg_dir))
        .route(
            "/assets/fonts/dm-sans.woff2",
            axum::routing::get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "font/woff2")],
                    include_bytes!("../../../static/assets/fonts/dm-sans.woff2").as_slice(),
                )
            }),
        )
        .route(
            "/assets/fonts/jetbrains-mono.woff2",
            axum::routing::get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "font/woff2")],
                    include_bytes!("../../../static/assets/fonts/jetbrains-mono.woff2").as_slice(),
                )
            }),
        )
        .leptos_routes(&options, routes, move || shell(shell_options.clone()));

    with_security_headers(slice_router)
        .with_state(options)
        .merge(archava_host::router(state))
        // One span per request, for the API and the document alike. Without a
        // subscriber these events are discarded (which is why the launcher
        // installs one), and with one the log line carries method, path,
        // status and latency — the request-level evidence an operator needs
        // when `/readyz` flips to 503.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
}

/// Resolve and validate the generated browser payload.
pub fn wasm_package_dir() -> Result<PathBuf, String> {
    let site_root = std::env::var_os("ARCHAVA_SITE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .expect("the server must run with a working directory")
                .join("target/site")
        });
    let pkg = site_root.join("pkg");
    for name in ["archava-slice.js", "archava-slice_bg.wasm"] {
        if !pkg.join(name).is_file() {
            return Err(format!(
                "missing {}/{}: build wasm bundle first (cargo leptos or wasm-pack)",
                pkg.display(),
                name
            ));
        }
    }
    Ok(pkg)
}

/// The one document shell both SSR and hydration agree on.
fn shell(options: LeptosOptions) -> impl leptos::IntoView {
    use leptos::hydration::HydrationScripts;
    use leptos::prelude::*;

    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <title>"Archava | Business workspace"</title>
                <link rel="stylesheet" href="/workspace.css" />
                <HydrationScripts options />
            </head>
            <body>
                <App />
            </body>
        </html>
    }
}

/// Wait for an operator stop without terminating in-flight requests.
pub async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install a SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => {},
            _ = term.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::get;
    use tower::ServiceExt;

    /// H10: the document router answered with no security headers because the
    /// policy lived on the host router and `merge` kept each route's layer
    /// stack. The helper is what `build_app` puts over the Leptos routes; this
    /// pins that the layer reaches a route it did not originate from.
    #[tokio::test]
    async fn the_document_router_carries_the_shared_header_policy() {
        let app = with_security_headers(Router::new().route("/app", get(|| async { "shell" })));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/app")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("the document route answers");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
        assert_eq!(response.headers()["cache-control"], "no-store");
        let policy = response.headers()["content-security-policy"]
            .to_str()
            .expect("ASCII policy");
        assert!(
            policy.contains("default-src 'self'"),
            "the JSON policy would blank the document: {policy}"
        );
        assert!(
            policy.contains("'wasm-unsafe-eval'"),
            "the wasm bundle needs the eval allowance: {policy}"
        );
    }
}
