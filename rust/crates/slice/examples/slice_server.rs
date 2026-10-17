//! The slice's executable: the REAL serving stack the browser proof drives,
//! plus the operator commands (`run` / `stop`) that own its lifecycle.
//!
//! One process does three things a fixture cannot mirror:
//!
//! - it serves the host's routes with a SOCIALLY real PostgreSQL — a
//!   provisioned instance where `archava_runtime` is a non-owner of every
//!   RLS-forced ledger, exactly as the integration suites demand, seeded with
//!   the same cast (fixture-only credentials; never real secrets);
//! - it serves the Leptos SSR shell plus the `pkg/` assets the browser
//!   hydrates — the SHIPPED UI surface, built ahead of run time by the wasm
//!   bindgen step (the operator check below refuses to start without it,
//!   which is the loud failure, not a silent empty page);
//! - it takes a RESTART: `run` against a stopped-with-`kill`-9 instance whose
//!   PostgreSQL survived carries its receipts forward — set
//!   `ARCHAVA_SLICE_ATTACH` (the JSON the first process printed at startup) to
//!   attach to that instance instead of provisioning, and the routes that
//!   answer after the restart answer from the SAME data.
//!
//! Run it release-mode: real scrypt derivations on the login route cost
//! seconds in a debug build and tens of milliseconds here.
//!
//! ```text
//! cargo run --release -p archava-slice --example slice_server   # run
//! cargo run --release -p archava-slice --example slice_server stop
//! ```

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use archava_host::{AppState, Ledger};
use archava_pg_fixture::{attach, provision, Fixture};
use archava_slice::App;
use axum::Router;
use leptos::config::LeptosOptions;
use leptos_axum::LeptosRoutes;
use tower_http::services::ServeDir;

/// The commerce ledger this host serves alongside CRM — the same table list
/// as `authority.rs` and `crm.rs`, so the readiness gate checks the same
/// RLS-forced surface in serving as in the suites.
const TABLES: &[&str] = &[
    "memberships",
    "products",
    "stock_balances",
    "orders",
    "order_lines",
    "allocations",
    "commands",
    "movements",
    "returns",
    "audit",
    "approvals",
];

/// A 32-byte identity HMAC key — the minimum `Identity::open` accepts. A
/// fixture-only secret: what a real deployment would hold is generated
/// outside the repository and never printed.
const IDENTITY_KEY: &[u8] = &[7u8; 32];

/// A precomputed scrypt hash of the fixture password ("correct horse battery
/// staple", the same string every suite uses) — one hash serves every user,
/// and no credential checker sees a differentiated password hash here.
const PASSWORD_HASH: &str =
    "scrypt$32768$8$3$OZtInKxSaSZqbEptj2fvUg$jU_gx2rQW-69EYpbRoMKGSgclhV-mO7gXVF-QYvSXVo";

/// The commerce ledger this host serves.
fn commerce_ledger() -> Ledger {
    Ledger {
        namespace: "archava_commerce",
        ledger: "Commerce",
        tables: TABLES,
    }
}

/// The browser-proof cast: the slice's CRM principals, in the vocabulary the
/// memberships table carries. A strict subset of `crm.rs`'s cast (no service
/// principals — the service path has no HTTP surface, the suites prove it
/// through the library API) plus one cross-tenant reader.
///
/// - `alice` — full read/write, scope `all`, owner of the contacts the flow
///   drafts against. The positive path's every step.
/// - `bob` — reads only. The missing-write-grant refusal (`role_not_allowed`).
/// - `carol` — write AND reads, scope `assigned`, owning one contact. The
///   out-of-scope refusal when the (URL-forgeable) draft names someone
///   else's contact.
/// - `dave` — only `followups.write`. The missing-read-grant refusal on the
///   list and on receipt recovery.
/// - `revoked` — alice's shape with `active = false`: authenticated, then
///   refused membership-level on every CRM read.
/// - `another` — globex-only reader. The cross-tenant refusal: same tenant
///   field, absent grant.
const CAST: &[CrmCastRow] = &[
    (
        "alice",
        "human",
        &[
            "contacts.read",
            "contacts.write",
            "followups.read",
            "followups.write",
        ],
        "all",
        true,
        &["acme-retail"],
    ),
    (
        "bob",
        "human",
        &["contacts.read", "followups.read"],
        "all",
        true,
        &["acme-retail"],
    ),
    (
        "carol",
        "human",
        &["contacts.read", "followups.read", "followups.write"],
        "assigned",
        true,
        &["acme-retail"],
    ),
    (
        "dave",
        "human",
        &["followups.write"],
        "all",
        true,
        &["acme-retail"],
    ),
    (
        "revoked",
        "human",
        &[
            "contacts.read",
            "contacts.write",
            "followups.read",
            "followups.write",
        ],
        "all",
        false,
        &["acme-retail"],
    ),
    (
        "another",
        "human",
        &["contacts.read"],
        "all",
        true,
        &["globex-corp"],
    ),
];

/// One cast row: `(principal, kind, permissions, scope, active, tenants)`.
type CrmCastRow = (
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static str,
    bool,
    &'static [&'static str],
);

/// The contacts, fixed ids so the browser-proof evidence can name rows
/// exactly. `owner_id` carries the assigned-scope binding.
type CrmContactRow = (
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
    &'static str,
);

const CONTACTS: &[CrmContactRow] = &[
    (
        "acme-retail",
        "2c3e5a10-1111-4111-8111-0000000000a1",
        "Ada Lovelace",
        Some("ada@lovelace.test"),
        "alice",
    ),
    (
        "acme-retail",
        "2c3e5a10-1111-4111-8111-0000000000a2",
        "Grace Hopper",
        Some("grace@hopper.test"),
        "alice",
    ),
    (
        "acme-retail",
        "2c3e5a10-1111-4111-8111-0000000000b1",
        "Carol Onwire",
        None,
        "carol",
    ),
    (
        "acme-retail",
        "2c3e5a10-1111-4111-8111-0000000000b2",
        "Dave Outstation",
        None,
        "carol",
    ),
    (
        "globex-corp",
        "2c3e5a10-1111-4111-8111-0000000000d1",
        "Globex Lead",
        None,
        "another",
    ),
];

#[tokio::main]
async fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("stop") => stop(),
        Some("help") | Some("--help") => usage(),
        // No argument, or `run`: the serve loop.
        None | Some("run") => run().await,
        Some(other) => {
            eprintln!("unknown command {other:?}");
            usage();
        }
    }
}

fn usage() {
    eprintln!("usage: archava-slice run | archava-slice stop");
    std::process::exit(2);
}

/// The pidfile of a running slice server. `run` writes it, `stop` reads it —
/// the run/stop pair is an OPERATOR COMMAND, and a file is the one channel
/// both sides of a restart can rely on.
fn pidfile() -> PathBuf {
    PathBuf::from("/tmp/opencode/archava-slice.pid")
}

fn stop() {
    let pid = std::fs::read_to_string(pidfile())
        .unwrap_or_else(|_| panic!("no running slice server: {} is absent", pidfile().display()));
    let pid = pid.trim();
    std::process::Command::new("kill")
        .args(["-TERM", pid])
        .status()
        .expect("run kill -TERM");
    println!("sent SIGTERM to slice server pid {pid}");
}

/// Port selection: fixed by default (the operator's URl is stable across a
/// restart), overridable for parallel proofs. Unprivileged range, loopback
/// only.
const DEFAULT_PORT: u16 = 8347;

async fn run() {
    let port = std::env::var("SLICE_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);

    // Attach mode carries the facts a restart needs: the instance's port and
    // data directory, printed by the first process verbatim.
    let fixture: Option<Fixture> = match std::env::var("ARCHAVA_SLICE_ATTACH") {
        Ok(json) => {
            let restart: AttachInfo = serde_json::from_str(&json)
                .expect("ARCHAVA_SLICE_ATTACH must be {\"port\", \"data\"}");
            Some(attach(Path::new(&restart.data), restart.port).await)
        }
        Err(_) => None,
    };

    // Provision on first run; the fixture's whole lifecycle, from initdb to
    // `pg_ctl stop` in the Drop, is the fixture crate's business.
    let new_instance = match fixture {
        Some(fixture) => fixture,
        None => provision().await,
    };

    // Seed only when this process CREATED the instance: an attached instance
    // already holds its cast and contacts — and, critically, its receipts,
    // which is the fact the restart proof reads back.
    if std::env::var("ARCHAVA_SLICE_ATTACH").is_err() {
        seed(&new_instance).await;
    }

    let state = AppState::try_new_crm(
        new_instance.runtime_pool.clone(),
        commerce_ledger(),
        &new_instance.commerce_ledger,
        &new_instance.identity_ledger,
        IDENTITY_KEY,
        &new_instance.crm_ledger,
    )
    .await
    .expect("a servable fixture must construct serving state");

    // The pkg assets must EXIST before the routes that reference them serve:
    // an operator starting this without the bindgen step gets the loud
    // refusal and the one command that fixes it.
    let pkg_dir = assert_wasm_assets();

    let site_addr: SocketAddr = format!("127.0.0.1:{port}")
        .parse()
        .expect("the fixed loopback address");

    let options = LeptosOptions::builder()
        .output_name("archava-slice")
        .site_root("target/site")
        .site_pkg_dir("pkg")
        .site_addr(site_addr)
        .build();
    let routes = leptos_axum::generate_route_list(App);
    let shell_options = options.clone();
    let slice_router: Router<LeptosOptions> = Router::new()
        .nest_service("/pkg", ServeDir::new(&pkg_dir))
        .leptos_routes(&options, routes, move || shell(shell_options.clone()));

    let app: Router<()> = slice_router
        .with_state(options)
        .merge(archava_host::router(state));

    let listener = tokio::net::TcpListener::bind(site_addr)
        .await
        .unwrap_or_else(|error| panic!("bind {site_addr}: {error}"));

    // The facts a RESTART needs, printed verbatim: attach consumes this shape
    // (serde) and the operator saves it verbatim, so the two ends agree by
    // construction rather than by substring luck. `data` is the PGDATA level
    // — `<dir>/data` — the path attach() asserts `postmaster.pid` in; `dir()`
    // alone is the fixture level and attaching it is refused on purpose.
    // Only the PRIME prints them: an attached server keeps no facts of its
    // own (its graceful stop deletes the instance), and a facts line in an
    // attach log is the fingerprint of a wrong re-provision, which probes
    // assert against.
    println!("slice server ready on http://{site_addr}/app — CRM ledger sealed, receipts durable",);
    if std::env::var("ARCHAVA_SLICE_ATTACH").is_err() {
        println!(
            "restart facts: ARCHAVA_SLICE_ATTACH={{\"port\":{},\"data\":\"{}\"}}",
            new_instance.port,
            new_instance.dir().join("data").display()
        );
    }

    // The pidfile: the operator stop command's one handle on this process.
    std::fs::create_dir_all(pidfile().parent().expect("a pidfile needs a parent"))
        .expect("create the pidfile directory");
    std::fs::write(pidfile(), std::process::id().to_string()).expect("write the pidfile");

    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("serve the slice");

    // The pidfile must not point at a stopped process.
    let _ = std::fs::remove_file(pidfile());
    // Dropping the fixture here (its own Drop) stops the relay PostgreSQL and
    // deletes its directory — the fixture lifecycle's whole point. In attach
    // mode that means OUR predecessor's instance, whose receipts live only as
    // long as an operator keeps the fixture — the disposable-instance deal.
    drop(new_instance);
}

/// The SSR shell — the full document both platforms agree on, per the 0.8
/// template: the hydration scripts live in the head, the router in the body.
fn shell(options: LeptosOptions) -> impl leptos::IntoView {
    use leptos::hydration::HydrationScripts;
    use leptos::prelude::*;

    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <title>"Archava — CRM slice"</title>
                <HydrationScripts options />
            </head>
            <body>
                <App />
            </body>
        </html>
    }
}

/// SIGINT and SIGTERM take the server down GRACEFULLY: in-flight axum
/// requests finish, the pool drains, and only then does the fixture's Drop
/// stop the PostgreSQL. A hard kill is what the PROOF's restart uses — that
/// is the point of the attach mode above.
async fn shutdown_signal() {
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

/// The bindgen outputs the hydration shell references. Their absence means
/// the wasm step did not run — refuse with the command, never serve an
/// empty shell and ask the browser to fail quietly.
fn assert_wasm_assets() -> PathBuf {
    let cwd = std::env::current_dir().expect("the server must run with a working directory");
    let pkg = cwd.join("target").join("site").join("pkg");
    for name in ["archava-slice.js", "archava-slice_bg.wasm"] {
        assert!(
            pkg.join(name).is_file(),
            "missing {pkg:?}/{name}: run the wasm build first \
             (cargo build -p archava-slice --target wasm32-unknown-unknown --lib --features hydrate; \
             wasm-bindgen --out-dir target/site/pkg --out-name archava-slice --target web --no-typescript \
             target/wasm32-unknown-unknown/release/archava_slice.wasm)"
        );
    }
    pkg
}

/// Seed the serving database with the fixture cast. A direct copy of the
/// integration suite's seeding stops at the rows the BROWSER proof needs;
/// the escaped-name and duplicate-format rows stay suite-only (their
/// witnesses read SQL, not a screen).
async fn seed(fixture: &Fixture) {
    for (id, kind, permissions, scope, active, tenants) in CAST {
        sqlx::query(
            "INSERT INTO archava_identity.users (id, username, password_hash, active, version) \
             VALUES ($1, $2, $3, true, 1)",
        )
        .bind(*id)
        .bind(format!("{id}@archava.test"))
        .bind(PASSWORD_HASH)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the identity user");

        for tenant in *tenants {
            sqlx::query(
                "INSERT INTO archava_crm.memberships \
                 (tenant_id, principal_id, kind, permissions, scope, active) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(tenant)
            .bind(*id)
            .bind(kind)
            .bind(*permissions)
            .bind(*scope)
            .bind(*active)
            .execute(&fixture.admin_pool)
            .await
            .expect("seed the CRM membership");
        }
    }

    for (tenant, id, full_name, email, owner) in CONTACTS {
        sqlx::query(
            "INSERT INTO archava_crm.contacts \
             (tenant_id, id, full_name, email, organization, owner_id) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(tenant)
        .bind(sqlx::types::Uuid::parse_str(id).expect("a fixture uuid"))
        .bind(full_name)
        .bind(email)
        .bind(None::<String>)
        .bind(owner)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the contact");
    }
}

/// What `run` prints and `ARCHAVA_SLICE_ATTACH` carries — the two ends of a
/// restart agree on this shape, serde-checked at both ends, not on a
/// substring.
#[derive(Debug, serde::Deserialize)]
struct AttachInfo {
    port: u16,
    data: String,
}
