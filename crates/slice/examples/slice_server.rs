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

use archava_host::{AllReadiness, AppState, Ledger};
use archava_pg_fixture::{attach, provision, Fixture};
use archava_slice::server::{build_app, shutdown_signal, wasm_package_dir};

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
    "inventory_layers",
    "inventory_uses",
    "inventory_return_costs",
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
            "activities.read",
            "activities.write",
            "followups.read",
            "followups.write",
            "pipeline.read",
            "pipeline.write",
        ],
        "all",
        true,
        &["acme-retail", "bulk-demo"],
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
        Some("cleanup") => cleanup().await,
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
    eprintln!("usage: archava-slice run | archava-slice stop | archava-slice cleanup");
    std::process::exit(2);
}

/// Take ownership of an instance left behind by a deliberate SIGKILL and
/// dispose of it without starting a second HTTP server.
async fn cleanup() {
    let json =
        std::env::var("ARCHAVA_SLICE_ATTACH").expect("cleanup requires ARCHAVA_SLICE_ATTACH");
    let restart: AttachInfo =
        serde_json::from_str(&json).expect("ARCHAVA_SLICE_ATTACH must be {\"port\", \"data\"}");
    let data = Path::new(&restart.data);
    let dir = data
        .parent()
        .expect("the fixture data directory must have a parent");
    assert!(
        dir.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("archava-pg-fixture-")),
        "cleanup refused a non-fixture directory: {}",
        dir.display()
    );
    if data.join("postmaster.pid").exists() {
        let fixture = attach(data, restart.port).await;
        drop(fixture);
    } else if dir.exists() {
        // PostgreSQL may already have stopped and removed postmaster.pid after
        // a failed/aborted proof. The fixture directory is still disposable;
        // remove only the validated fixture path rather than treating the
        // missing pid as a reason to leak it forever.
        std::fs::remove_dir_all(dir).expect("remove stopped fixture directory");
    }
    println!("fixture cleanup complete");
}

/// The pidfile of a running slice server. `run` writes it, `stop` reads it —
/// the run/stop pair is an OPERATOR COMMAND, and a file is the one channel
/// both sides of a restart can rely on.
fn pidfile() -> PathBuf {
    std::env::var_os("ARCHAVA_SLICE_PIDFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/opencode/archava-slice.pid"))
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

    let additional = new_instance.erp_readiness();
    let state = AppState::try_new_all(
        new_instance.runtime_pool.clone(),
        commerce_ledger(),
        AllReadiness {
            commerce: &new_instance.commerce_ledger,
            identity: &new_instance.identity_ledger,
            crm: &new_instance.crm_ledger,
            owner: &new_instance.owner_ledger,
            purchasing: &new_instance.purchasing_ledger,
            additional_erp: &additional,
        },
        IDENTITY_KEY,
    )
    .await
    .expect("a servable fixture must verify every mounted ledger");

    // Explicit owned loopback transport only in this disposable fixture binary.
    // The production executable has no arbitrary endpoint configuration.
    let state = match std::env::var("ARCHAVA_SLICE_PAYMENT_ORIGIN") {
        Ok(origin) => {
            sqlx::query("INSERT INTO archava_billing.memberships(tenant_id,principal_id,kind,permissions,scope,active) VALUES('acme-retail','gateway-fixture','service',ARRAY['billing.read','billing.write'],'all',true) ON CONFLICT DO NOTHING").execute(&new_instance.admin_pool).await.expect("synthetic gateway service");
            let registry = archava_host::payments::PaymentRegistry::new(vec![
                archava_host::payments::PaymentIntegration {
                    id: "fixture-merchant".into(),
                    tenant: serde_json::from_value(serde_json::json!("acme-retail")).unwrap(),
                    service_id: "gateway-fixture".into(),
                    client: archava_providers::midtrans::MidtransClient::for_loopback(
                        &origin,
                        "synthetic-browser-payment-key".into(),
                    )
                    .expect("owned loopback provider"),
                },
            ])
            .expect("fixture registry");
            state.with_payment_registry(registry)
        }
        Err(_) => state,
    };

    // Owned loopback email transport for the disposable browser proof. The
    // production executable only accepts the administrator-managed registry.
    let state = match std::env::var("ARCHAVA_SLICE_EMAIL_ORIGIN") {
        Ok(origin) => {
            let registry = archava_host::mail::EmailRegistry::new(vec![
                archava_host::mail::EmailIntegration {
                    id: "fixture-resend".into(),
                    tenant: serde_json::from_value(serde_json::json!("acme-retail")).unwrap(),
                    from_address: "Archava Fixture <billing@archava.test>".into(),
                    client: archava_providers::resend::ResendClient::for_loopback(
                        &origin,
                        "synthetic-browser-email-key".into(),
                    )
                    .expect("owned loopback email"),
                },
            ])
            .expect("fixture email registry");
            state.with_email_registry(registry)
        }
        Err(_) => state,
    };

    // Owned in-memory presence for the same proof; no LiveKit network call.
    let state = if matches!(
        std::env::var("ARCHAVA_SLICE_PRESENCE_LOOPBACK").as_deref(),
        Ok("1")
    ) {
        state.with_presence_registry(archava_host::presence::PresenceRegistry::for_loopback())
    } else {
        state
    };

    // The pkg assets must EXIST before the routes that reference them serve:
    // an operator starting this without the bindgen step gets the loud
    // refusal and the one command that fixes it.
    let pkg_dir = wasm_package_dir().expect("the Rust browser assets must be built first");

    let site_addr: SocketAddr = format!("127.0.0.1:{port}")
        .parse()
        .expect("the fixed loopback address");

    let app = build_app(state, site_addr, &pkg_dir);

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

    // `into_make_service_with_connect_info` is not decoration: `LoginSource`
    // reads `ConnectInfo<SocketAddr>` to bucket login attempts per peer. With
    // the plain `into_make_service()` every request carried no peer address,
    // so the whole process shared the `no-connect-info` bucket and one noisy
    // client could close sign-in for everyone (audit H02).
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
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

    // Synthetic operator grants for the real native Tier 1 browser journeys.
    for tenant in ["acme-retail", "bulk-demo"] {
        for (principal, read_only) in [("alice", false), ("bob", true)] {
            for (namespace, all) in [
                (
                    "archava_commerce",
                    vec![
                        "catalog.read",
                        "catalog.write",
                        "stock.read",
                        "stock.write",
                        "orders.read",
                        "orders.write",
                        "returns.write",
                        "valuation.read",
                        "valuation.write",
                        "audit.read",
                    ],
                ),
                (
                    "archava_purchasing",
                    vec![
                        "suppliers.read",
                        "suppliers.write",
                        "offers.read",
                        "offers.write",
                        "orders.read",
                        "orders.write",
                        "receipts.read",
                        "receipts.write",
                        "audit.read",
                    ],
                ),
                (
                    "archava_billing",
                    vec![
                        "billing.read",
                        "billing.write",
                        "billing.settle",
                        "audit.read",
                    ],
                ),
                (
                    "archava_support",
                    vec![
                        "cases.read",
                        "cases.write",
                        "cases.assign",
                        "notes.read",
                        "notes.write",
                        "audit.read",
                    ],
                ),
            ] {
                let permissions: Vec<&str> = all
                    .into_iter()
                    .filter(|p| !read_only || p.ends_with(".read"))
                    .collect();
                let query=format!("INSERT INTO {namespace}.memberships(tenant_id,principal_id,kind,permissions,scope,active) VALUES($1,$2,'human',$3,'all',true)");
                sqlx::query(sqlx::AssertSqlSafe(query.as_str()))
                    .bind(tenant)
                    .bind(principal)
                    .bind(permissions)
                    .execute(&fixture.admin_pool)
                    .await
                    .expect("seed Tier 1 grants");
            }
            sqlx::query("UPDATE archava_commerce.memberships SET role='owner',capability=$3 WHERE tenant_id=$1 AND principal_id=$2").bind(tenant).bind(principal).bind(if read_only{"assist"}else{"enterprise"}).execute(&fixture.admin_pool).await.expect("seed mission capability");
        }
    }

    // C03: the owner surface's grants are separate from CRM memberships. Alice
    // manages configuration on both fixture tenants; Bob deliberately has no
    // owner grant, so the browser proof can see the missing-grant refusal.
    for tenant in ["acme-retail", "bulk-demo"] {
        sqlx::query(
            "INSERT INTO archava_owner.owner_grants \
             (tenant_id, principal_id, permissions, active) \
             VALUES ($1, 'alice', ARRAY['config.read','config.apply'], true)",
        )
        .bind(tenant)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the owner grant");
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

    // The pagination and literal-search fixture: one tenant with MORE rows
    // than a page holds, so the browser proof can move between pages for
    // real, and names whose characters a raw query string would eat (`&`,
    // `%`, `+`, `#`). The 19 Oktober audit's C06 repro was a search for
    // `Ada&limit=1`: unencoded, it became a different query and the page
    // showed a row the literal search never returns. These rows make that
    // difference observable on screen.
    for (index, name) in BULK_CONTACT_NAMES.iter().enumerate() {
        let id = format!("2c3e5a10-3333-4333-8333-{:012x}", index + 1);
        sqlx::query(
            "INSERT INTO archava_crm.contacts \
             (tenant_id, id, full_name, email, organization, owner_id) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind("bulk-demo")
        .bind(sqlx::types::Uuid::parse_str(&id).expect("a fixture uuid"))
        .bind(*name)
        .bind(None::<String>)
        .bind(None::<String>)
        .bind("alice")
        .execute(&fixture.admin_pool)
        .await
        .expect("seed the bulk contact");
    }
}

/// The bulk tenant's contact names, in a fixed order so the ids the browser
/// proof reads are stable: 26 rows — more than one page of 20 — with the
/// four names whose characters a query string would otherwise eat.
const BULK_CONTACT_NAMES: &[&str] = &[
    "Bulk Client 01",
    "Bulk Client 02",
    "Bulk Client 03",
    "Bulk Client 04",
    "Bulk Client 05",
    "Bulk Client 06",
    "Bulk Client 07",
    "Bulk Client 08",
    "Bulk Client 09",
    "Bulk Client 10",
    "Bulk Client 11",
    "Bulk Client 12",
    "Bulk Client 13",
    "Bulk Client 14",
    "Bulk Client 15",
    "Bulk Client 16",
    "Bulk Client 17",
    "Bulk Client 18",
    "Bulk Client 19",
    "Bulk Client 20",
    "Bulk Client 21",
    "Ada Lovelace",
    "Ada & Lovelace Ltd",
    "Percent % Name",
    "Plus + Name",
    "Hash # Name",
];

/// What `run` prints and `ARCHAVA_SLICE_ATTACH` carries — the two ends of a
/// restart agree on this shape, serde-checked at both ends, not on a
/// substring.
#[derive(Debug, serde::Deserialize)]
struct AttachInfo {
    port: u16,
    data: String,
}
