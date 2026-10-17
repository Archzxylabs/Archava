//! A disposable, owned PostgreSQL instance for the Rust integration suites.
//!
//! ## Why this crate exists (RF02)
//!
//! The tenant-context tests used to read `ARCHAVA_TEST_DATABASE_URL` with a
//! fallback to a hand-seeded cluster at a hardcoded `/tmp/pgdata` — a database
//! someone else provisioned, seeded once by a script, and left running. Three
//! defects followed from that one choice, and all three are audit findings:
//!
//! 1. **The tests were not reproducible.** A fresh checkout had no database, so
//!    `cargo test` failed with a connection error whose exit code was
//!    indistinguishable from a regression. The file even documented this as
//!    deliberate ("a missing database is a configuration error, not a test
//!    failure") — which meant the suite could not fail for its real reason.
//! 2. **Seeded state drifted.** `rls_scopes_reads_to_the_declared_tenant`
//!    inserted two products per run and asserted `count = 1` per tenant, which
//!    only holds on a database whose products table was truncated first. On a
//!    long-lived `/tmp/pgdata` it eventually fails for accumulation, not for
//!    behavior.
//! 3. **The owner seed was shared mutable state.** Any process on the machine
//!    could point at the same cluster; the "owner" role in the test URL was a
//!    trust-auth superuser on a world-reachable socket directory.
//!
//! This crate replaces all of that with what the mandate asks for: an owned,
//! disposable fixture that the test itself provisions — `initdb` into a unique
//! temp directory, a random free port, its own roles, the REAL commerce
//! migrations applied by a real migration-owner role, and a teardown that stops
//! the server and deletes the directory. Nothing is shared with any other
//! process, nothing survives the run, and a machine without PostgreSQL
//! binaries fails loudly at `initdb` with the missing tool named — never a
//! silent skip, because a skipped integration test and a passing one are the
//! same row in a report.
//!
//! ## What it provisions, and why each piece is what it is
//!
//! - **`postgres` (superuser, admin pool)** — for fixture administration only:
//!   creating roles and databases. Never used by a test that asserts authority,
//!   because a superuser bypasses RLS by definition.
//! - **`archava_owner` (owner pool)** — the role that applies the migrations
//!   and therefore OWNS every table in `archava_commerce`. It exists so a test
//!   can prove the cohort refuses exactly this role
//!   (`TenantContextError::NonOwnerRuntime`): FORCE ROW LEVEL SECURITY makes
//!   the owner subject to the policy, and the non-owner check refuses it
//!   before it can open anything.
//! - **`archava_runtime` (runtime pool, and a max-1-connection pool)** — the
//!   role the host runs as: NOSUPERUSER, NOBYPASSRLS, no table ownership, and
//!   only the grants `grantCommerceRuntime` gives. The single-connection pool
//!   is what makes the leak test deterministic: with `max_connections(1)`,
//!   two sequential transactions MUST run on the same physical connection, so
//!   "the tenant setting is gone before the second begin" is proven on the
//!   connection itself rather than probabilistically on a pool.
//!
//! The migrations applied are the REAL ones from
//! `packages/commerce-pg/migrations/`, read from the repository at run time and
//! applied with the same semantics as `packages/commerce-pg/src/migrate.ts` —
//! advisory lock, history check that refuses modified history, per-migration
//! `schema_migrations` row inserted with the sha256 of the file bytes. A
//! fixture with hand-written schema drift would test a database nobody ships.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tokio::sync::Semaphore;

/// The migration files this fixture applies, in apply order.
///
/// Frozen as the same list `packages/commerce-pg/src/migrate.ts:5-9` declares.
/// A new commerce migration belongs in BOTH lists — this one so the fixture
/// keeps applying what the baseline applies, and that one because it is the
/// migrator of record. `assert_startup_readiness` then compares the applied
/// `schema_migrations` against these same files, so the fixture and the
/// readiness port can never hash different inputs.
pub const COMMERCE_MIGRATIONS: &[&str] = &[
    "001_catalog_orders_stock",
    "002_managed_menu_products",
    "003_purchase_receipts",
    "004_bound_approvals",
];

/// The identity ledger's migrations, in apply order — `packages/identity-pg`.
///
/// RA01's sessions live here rather than in the commerce namespace, exactly as
/// the baseline keeps them: identity is its own schema with its own
/// `schema_migrations` history, its own non-owner check and its own runtime
/// grants. The fixture applies both ledgers because the host's authority path
/// needs both: a session row (identity) and the membership it is resolved
/// against (commerce).
pub const IDENTITY_MIGRATIONS: &[&str] = &["001_identity"];

/// The CRM ledger's migrations, in apply order — `packages/crm-pg`.
///
/// §1.4B's vertical slice reads contacts and the sales pipeline and writes a
/// reviewed lead through the command path, so the slice's database is the real
/// `archava_crm` schema: its own namespace, its own `schema_migrations`
/// history, its own non-owner check and its own runtime grants
/// (`grantCrmRuntime`, ported statement for statement in `provision`). The
/// host's startup readiness asserts this ledger exactly like the other two —
/// data the server can serve must be data the server has verified.
pub const CRM_MIGRATIONS: &[&str] = &[
    "001_crm_contacts",
    "002_sales_pipeline",
    "003_native_approval",
];

/// The purchasing ledger's migrations, in apply order —
/// `packages/purchasing-pg` (Archxy 2's ERP namespace, PACKET-000): the R06
/// cohort needs the real `archava_purchasing` schema next to
/// `archava_commerce`, because the receipt-completion triggers reconcile the
/// two ledgers inside ONE transaction.
pub const PURCHASING_MIGRATIONS: &[&str] = &["001_suppliers_orders_receipts"];

/// The foodservice ledger's migration, in apply order —
/// `packages/foodservice-pg/migrations/001_tables_recipes_tickets.sql`
/// (Archxy 2's ERP namespace, PACKET-001): a single migration above
/// `archava_foodservice` next to the other cohort schemas.
pub const FOOD_MIGRATIONS: &[&str] = &["001_tables_recipes_tickets"];

/// The namespace and ledger label the commerce schema uses.
pub const NAMESPACE: &str = "archava_commerce";
pub const LEDGER: &str = "Commerce";

const ADMIN_USER: &str = "postgres";
const OWNER_USER: &str = "archava_owner";
const RUNTIME_USER: &str = "archava_runtime";
const DATABASE: &str = "archava_test";
const DIR_PREFIX: &str = "archava-pg-fixture-";

/// One provisioned, running, isolated PostgreSQL instance.
///
/// Dropped values stop the server and delete the directory — every test
/// provisions its own through [`provision`] rather than sharing one.
///
/// ## Why there is deliberately no `shared()`
///
/// The first version of this crate had a process-wide singleton, and it failed
/// in the instructive way: `#[tokio::test]` gives every test its OWN runtime,
/// and a `PgPool` holds background tasks pinned to the runtime that created
/// it. The first test provisioned the fixture on its runtime; when that test
/// finished, its runtime shut down and every later test using those pools got
/// `A Tokio 1.x context was found, but it is being shutdown` and `PoolTimedOut`.
/// A shared async resource cannot outlive the runtime of whoever created it,
/// and the test harness retires runtimes constantly.
///
/// Provisioning per test costs under a second (initdb with durability off,
/// measured at ~0.5s including the commerce migrations) and buys three things
/// the singleton could not: no cross-test state at all (the checksum-tampering
/// test does not need a lock, because it tampers with its own ledger), a
/// failure that fails exactly one test instead of cascading through a poisoned
/// singleton, and a teardown proven on every test run rather than once.
///
/// ## Why provisioning is serialized
///
/// A process-wide semaphore (runtime-agnostic `tokio::sync::Semaphore`, so it
/// works across the per-test runtimes) admits one provision at a time. Two
/// concurrent provisions could race twice: `free_port()` drops its listener
/// before postgres binds the port, and `clean_stale_dirs()` could delete a
/// sibling's directory that exists but has no `postmaster.pid` yet. Holding
/// the permit for the whole of [`provision`] makes both windows empty.
/// Cross-process overlap (two cargo runs at once) is handled by the age gate
/// in `clean_stale_dirs` plus the pid liveness check.
pub struct Fixture {
    /// Superuser pool. Fixture administration only — never authority evidence.
    pub admin_pool: PgPool,
    /// The table-owner pool. Exists so tests can prove it is REFUSED.
    pub owner_pool: PgPool,
    /// The runtime pool: the role the host runs as.
    pub runtime_pool: PgPool,
    /// The runtime role on ONE physical connection (`max_connections(1)`),
    /// which is what makes the no-leak read-back deterministic.
    pub single_pool: PgPool,
    /// The port the instance listens on (loopback only).
    pub port: u16,
    /// `(id, sha256)` of each applied migration, in apply order — the exact
    /// input `assert_startup_readiness` expects.
    pub commerce_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_identity` — the input the host's identity
    /// readiness check expects (RA01).
    pub identity_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_crm` — the input the host's CRM readiness
    /// check expects (§1.4B: the slice serves contacts and pipeline from that
    /// schema, so startup asserts its history too).
    pub crm_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_purchasing` — the ERP cohort ledger
    /// (Archxy 2's ERP namespace, PACKET-000; the shared fixture is the
    /// integration writer's single-writer file, the packet record rides in
    /// `docs/rust/archxy2/INTEGRATION_REQUESTS.md` PACKET-000).
    pub purchasing_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_foodservice` — the stock cohort's second
    /// ERP ledger (PACKET-001): tickets, recipes and the menus whose menu
    /// products live IN `archava_commerce`.
    pub foodservice_ledger: Vec<(String, String)>,
    dir: PathBuf,
}

impl Fixture {
    /// A connection URL for the runtime role.
    ///
    /// A test that must prove something SURVIVES A RESTART (RA01: "sessions
    /// survive restart") cannot reuse [`Fixture::runtime_pool`] for the
    /// post-restart state: a `PgPool` is a live process resource, and handing
    /// the same pool to a second `AppState` would share the very connection
    /// state the test claims to have thrown away. A restart means new
    /// connections, and the URL is the honest way to ask for them.
    pub fn runtime_url(&self) -> String {
        format!(
            "postgres://{RUNTIME_USER}:archava@127.0.0.1:{}/{}",
            self.port, DATABASE
        )
    }

    /// The directory the instance's data and log live in. Public to the
    /// lifecycle: an operator (or a restarted server in attach mode) names
    /// the running instance by this directory, and the runtime prints it in
    /// its startup facts line.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// One provision at a time per process — see [`Fixture`] for the two races
/// this closes. `const_new` keeps it a static, so there is exactly one gate
/// regardless of how many tests reach `provision` first.
static PROVISION_SLOTS: Semaphore = Semaphore::const_new(1);

/// A fresh command for a PostgreSQL binary, after proving the binary runs.
///
/// The probe uses its OWN `Command` — this is load-bearing, and the first
/// version of this function got it wrong in the most instructive way possible:
/// it appended `--version` to the very `Command` it returned, so every real
/// invocation became `initdb --version -D …`. PostgreSQL prints its version
/// and exits 0 — success! — having created nothing, and the fixture then spent
/// 60 seconds polling a server that no `pg_ctl … start` had ever launched
/// ("did not become ready in 60s", with `<no log>` because `pg_ctl -l` never
/// ran either). A probe that contaminates the thing it probes is worse than no
/// probe, so the returned command is built here from scratch, with the probe's
/// arguments on a separate process.
///
/// The loud failure is the point (RF02): "initdb not found — install a
/// PostgreSQL server package" is a setup error an operator can act on in one
/// read. A `#[ignore]`-style skip would put a green row in the report for a
/// suite that never ran.
fn tool(name: &str) -> Command {
    let probe = Command::new(name)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "`{name}` is not runnable ({error}). The Rust integration suites need the \
                 PostgreSQL server binaries (initdb/pg_ctl) on PATH — install the server \
                 package (Debian/Ubuntu: `apt-get install postgresql`), never a mock."
            )
        });
    assert!(
        probe.status.success(),
        "`{name} --version` exited {:?}: {}",
        probe.status.code(),
        String::from_utf8_lossy(&probe.stderr)
    );
    let mut command = Command::new(name);
    // stdin/stderr stay piped so a failure can quote the tool's own words.
    command.stdin(Stdio::null()).stderr(Stdio::piped());
    command
}

/// A unique directory for this instance, and the stale ones removed first.
///
/// The directory name embeds this process's id (`archava-pg-fixture-<pid>-<nanos>`),
/// and that embedded pid is the **lifecycle guard**: it is created atomically
/// with the directory, before `initdb` runs, so the ownership claim cannot be
/// lost to a crash between "directory exists" and "claim written". The reaper
/// below reads it back out of the name to decide whether a directory's owner is
/// still alive — see [`creator_pid_from_name`] and [`reap_verdict`].
fn instance_dir() -> PathBuf {
    clean_stale_dirs();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{DIR_PREFIX}{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create fixture directory");
    dir
}

/// The verdict for one candidate directory in [`clean_stale_dirs`].
///
/// Split out from the reaper so the decision is testable with a controlled
/// clock and a controlled liveness probe — the mandate forbids "waiting five
/// minutes" or "destroying someone else's fixture" as a way to test this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReapVerdict {
    /// Younger than the age gate: assume a provision is in flight and keep it.
    KeepTooYoung,
    /// A named process (the creator or the running postmaster) is alive.
    KeepLive,
    /// Ownership could not be established (no parseable creator pid): refuse to
    /// guess, because a missing/unreadable pid is not proof the directory is
    /// safe to delete.
    KeepUnknown,
    /// Positively proven dead (the creator pid parsed and that process is gone).
    Remove,
}

/// Decides whether one fixture directory may be reaped.
///
/// Three inputs, and the order they are consulted in is the safety argument:
///
/// 1. **Age.** Anything younger than `min_age` is kept unconditionally — it may
///    be a provision in flight whose `postmaster.pid` does not exist yet.
/// 2. **Liveness.** If the creator (from the directory name) OR the postmaster
///    (from `data/postmaster.pid`) is alive, the directory is kept. A live
///    instance older than the age gate is another process's fixture, not a leak.
/// 3. **Positive death proof.** Only when the creator pid *parsed* and that
///    process is gone is the directory removed. A directory whose creator pid
///    cannot be parsed (`KeepUnknown`) is kept: the mandate is explicit that a
///    missing/unreadable pid is not evidence that deletion is safe.
fn reap_verdict(
    age: Duration,
    creator_pid: Option<u32>,
    postmaster_pid: Option<u32>,
    min_age: Duration,
    proc_alive: impl Fn(u32) -> bool,
) -> ReapVerdict {
    if age < min_age {
        return ReapVerdict::KeepTooYoung;
    }
    if creator_pid.is_some_and(&proc_alive) || postmaster_pid.is_some_and(&proc_alive) {
        return ReapVerdict::KeepLive;
    }
    match creator_pid {
        None => ReapVerdict::KeepUnknown,
        // The creator parsed and is proven gone. Even if the postmaster pid was
        // missing or unreadable, the process that owned this instance is dead,
        // so the instance cannot be live.
        Some(_) => ReapVerdict::Remove,
    }
}

/// The creator process id embedded in a fixture directory name.
///
/// The name is `<DIR_PREFIX><pid>-<nanos>`; the first `-`-separated field after
/// the prefix is the pid of the process that called [`provision`]. Returns
/// `None` for anything that does not parse — a foreign directory that merely
/// shares the prefix — which the reaper treats as "keep" rather than "delete".
fn creator_pid_from_name(name: &str) -> Option<u32> {
    let rest = name.strip_prefix(DIR_PREFIX)?;
    rest.split('-').next()?.parse().ok()
}

/// The running postmaster's pid, read from **`<data>/postmaster.pid`**.
///
/// This is the RF04 correction. The server is started with
/// `-D <fixture>/data`, so PostgreSQL writes its pid file inside the *data*
/// directory. The previous version read `<fixture>/postmaster.pid` (one level
/// too high), never found it, treated every old directory as dead, and deleted
/// live fixtures. `None` when the file is absent or unparsable — and per the
/// reaper's rule, `None` is never treated as proof of death.
fn postmaster_pid_from(data: &Path) -> Option<u32> {
    let contents = std::fs::read_to_string(data.join("postmaster.pid")).ok()?;
    contents.lines().next()?.trim().parse().ok()
}

/// Whether a process id names a live process.
///
/// On Linux this is a `/proc/<pid>` existence check — the same signal the
/// previous reaper used. On a platform without `/proc` the answer is
/// conservatively `true`: without a way to prove a process is *gone*, the only
/// safe verdict is "keep it", because a leaked directory is recoverable and a
/// deleted live instance is not.
#[cfg(target_os = "linux")]
fn proc_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(not(target_os = "linux"))]
fn proc_alive(_pid: u32) -> bool {
    true
}

/// Remove directories left by crashed runs, never a live instance's and never
/// an in-flight one's.
///
/// The safety argument lives in [`reap_verdict`], which this function drives
/// with real inputs: age from the directory's mtime, the creator pid from the
/// directory name (the lifecycle guard), and the postmaster pid from
/// `data/postmaster.pid` (the RF04 path correction). A directory is removed
/// only on positive proof that its owning process is gone.
const STALE_MIN_AGE: Duration = Duration::from_secs(300);

fn clean_stale_dirs() {
    #[cfg(target_os = "linux")]
    {
        let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name.starts_with(DIR_PREFIX) || !entry.path().is_dir() {
                continue;
            }
            // An unreadable mtime is not evidence of age; keep the directory.
            let Some(age) = entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|modified| modified.elapsed().ok())
            else {
                continue;
            };
            let creator = creator_pid_from_name(name);
            let postmaster = postmaster_pid_from(&entry.path().join("data"));
            let verdict = reap_verdict(age, creator, postmaster, STALE_MIN_AGE, proc_alive);
            if verdict == ReapVerdict::Remove {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
}

/// A guard that stops and removes what a [`provision`] call created if
/// provisioning fails after the server has started.
///
/// ## Why this exists (RF04's second half)
///
/// `provision` panics with context on every failure, which is right for a test
/// fixture — but a panic between `pg_ctl start` and the `Fixture` value being
/// returned would leak a *running* server and its ~50MB data directory, because
/// `Fixture::drop` never runs for a value that was never constructed. The
/// mandate names exactly this: *"protect in-flight provisioning and cleanup
/// failure after server start with a lifecycle guard created before the
/// resource can leak."*
///
/// So the guard is created immediately after the directory and port exist —
/// before `initdb` — and it disarms only when the returned [`Fixture`] takes
/// over cleanup responsibility. Any early exit (panic or unwind) stops the
/// server if one was started and removes the directory.
struct ProvisioningGuard {
    dir: PathBuf,
    /// Set once `pg_ctl start` has succeeded, so the guard knows a server may
    /// be running and must be stopped before the directory is removed.
    started: bool,
    /// Cleared by [`ProvisioningGuard::disarm`] when the [`Fixture`] takes over
    /// cleanup. While armed the guard owns the directory; once disarmed its
    /// `Drop` is a no-op and must not touch the directory the Fixture now owns.
    armed: bool,
}

impl ProvisioningGuard {
    /// Hand cleanup responsibility to the constructed [`Fixture`].
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ProvisioningGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if self.started {
            let _ = stop_server(&self.dir.join("data"));
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Best-effort `pg_ctl stop -m immediate` on a data directory.
///
/// Shared by [`ProvisioningGuard::drop`] and [`Fixture::drop`]. A non-zero
/// exit is not an error here: stopping an already-dead (or never-started)
/// server is a no-op, and the caller removes the directory either way.
fn stop_server(data: &Path) -> std::io::Result<()> {
    let _stop = Command::new("pg_ctl")
        .args([
            "-D",
            data.to_str().expect("utf-8 data dir"),
            "-m",
            "immediate",
            "-w",
            "-t",
            "30",
            "stop",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    Ok(())
}

/// A free TCP port on loopback. The listener is dropped before postgres binds
/// the port, which is the usual small race — acceptable here because the
/// instance is per-process and a bind failure at start is a loud panic, never
/// a silent fallback to another database.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind a loopback port")
        .local_addr()
        .expect("read local address")
        .port()
}

/// Provision one disposable instance for one caller.
///
/// Serialized process-wide by [`PROVISION_SLOTS`] (see [`Fixture`] for why
/// both the per-test lifetime and the serialization exist). Every step panics
/// with context on failure. This is deliberately not a `Result`: there is no
/// caller that could recover mid-test, and a `Result` would invite
/// `let Ok(x) = ... else { return }` — the silent skip this fixture exists to
/// make impossible.
pub async fn provision() -> Fixture {
    // Held for the whole provisioning window and released when this future
    // completes or unwinds. `acquire` returns `Result` only because a
    // semaphore can be closed — this one never is, hence `expect` rather than
    // a fallback: a closed gate would mean someone disabled serialization,
    // and silently provisioning anyway would reintroduce the races it exists
    // to close.
    let _slot = PROVISION_SLOTS
        .acquire()
        .await
        .expect("provisioning semaphore is never closed");
    let dir = instance_dir();
    let data = dir.join("data");
    let log = dir.join("postgres.log");
    let port = free_port();
    // The lifecycle guard, created BEFORE initdb so any failure after this
    // point — including a panic once the server is already running — stops and
    // removes what this provision created. Disarmed only when the returned
    // `Fixture` takes over cleanup. See `ProvisioningGuard`.
    let mut guard = ProvisioningGuard {
        dir: dir.clone(),
        started: false,
        armed: true,
    };

    // 1. initdb: trust auth on loopback + unix socket, no durability (this is
    //    a disposable instance, and fsync off makes initdb and every test
    //    transaction measurably faster).
    let initdb = tool("initdb")
        .args([
            "-D",
            data.to_str().expect("utf-8 data dir"),
            "-U",
            ADMIN_USER,
            "-E",
            "UTF8",
            "--auth-local=trust",
            "--auth-host=trust",
            "--no-sync",
        ])
        .output()
        .expect("run initdb");
    assert!(
        initdb.status.success(),
        "initdb failed: {}",
        String::from_utf8_lossy(&initdb.stderr)
    );
    // An exit code is not an effect. `initdb --version` exits 0 having created
    // nothing — which is exactly what happened the first time this fixture ran,
    // and what this assertion makes impossible to reintroduce: success is a
    // directory that exists with content, not a status alone.
    let populated = std::fs::read_dir(&data)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false);
    assert!(
        populated,
        "initdb reported success but {} was not created (or is empty)",
        data.display()
    );

    // 2. Start on the chosen port, loopback only, durability off, socket in the
    //    instance directory (short path — unix socket paths cap near 108 bytes).
    let options = format!(
        "-p {port} -k {} -c listen_addresses=127.0.0.1 \
         -c fsync=off -c synchronous_commit=off -c full_page_writes=off",
        dir.display()
    );
    let start = tool("pg_ctl")
        .args([
            "-D",
            data.to_str().expect("utf-8 data dir"),
            "-l",
            log.to_str().expect("utf-8 log path"),
            "-o",
            &options,
            "-w",
            "-t",
            "60",
            "start",
        ])
        .output()
        .expect("run pg_ctl start");
    assert!(
        start.status.success(),
        "pg_ctl start failed: {}\nlog: {}",
        String::from_utf8_lossy(&start.stderr),
        std::fs::read_to_string(&log).unwrap_or_else(|_| "<no log>".to_string())
    );
    // A server is now running; from here until the Fixture is returned the
    // guard is responsible for stopping it if anything fails.
    guard.started = true;

    let maintenance_url = url(ADMIN_USER, "postgres", port);
    wait_ready(&maintenance_url, &log).await;

    // Roles and CREATE DATABASE run on the maintenance database (roles are
    // cluster-wide; you cannot create the database you are connected to).
    let maintenance_pool = connect(&maintenance_url, 1).await;

    // 3. Roles. `archava_owner` applies migrations and therefore owns the
    //    tables; `archava_runtime` is the host role: no superuser, no bypass,
    //    no ownership — exactly the shape the non-owner check demands.
    //    (The `AssertSqlSafe` wrappers below audit the same thing at every
    //    site: every interpolated value is a `const` declared in this file —
    //    no caller, environment variable, or file content reaches these
    //    format strings.)
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE ROLE {OWNER_USER} LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE"
    )))
    .execute(&maintenance_pool)
    .await
    .expect("create owner role");
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE ROLE {RUNTIME_USER} LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE"
    )))
    .execute(&maintenance_pool)
    .await
    .expect("create runtime role");
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE DATABASE {DATABASE} OWNER {OWNER_USER} TEMPLATE template0"
    )))
    .execute(&maintenance_pool)
    .await
    .expect("create test database");

    // The superuser pool tests see — seeding that bypasses RLS, and the
    // superuser-readiness refusal — connects to `archava_test` itself. A
    // superuser on the maintenance database cannot even name
    // `archava_commerce`: every statement against the ledger would fail with
    // `schema does not exist`, which is how this was first wired.
    let admin_pool = connect(&url(ADMIN_USER, DATABASE, port), 2).await;

    // 4. Migrate as the owner role, with migrate.ts's semantics. Both ledgers:
    //    commerce (the tenant tables and, since RA01, the bound approvals) and
    //    identity (the users and sessions the authority path authenticates
    //    against). The owner role creates both, which is what makes the
    //    runtime role a non-owner of both — the readiness gates refuse on
    //    exactly that property being absent.
    let owner_url = url(OWNER_USER, DATABASE, port);
    let owner_pool = connect(&owner_url, 1).await;
    let commerce_ledger = migrate_commerce(&owner_pool).await;
    let identity_ledger = migrate_ledger(
        &owner_pool,
        "archava_identity",
        "archava_identity_migrations",
    )
    .await;
    // §1.4B: the slice's own ledger — contacts, pipeline, and their history,
    // applied by the same owner role so the runtime role is a non-owner of
    // this schema exactly as it is of the other two.
    let crm_ledger = migrate_ledger(&owner_pool, "archava_crm", "archava_crm_migrations").await;
    // Archxy 2 ERP namespace (PACKET-000): the purchasing ledger migrates under
    // the same owner role, with the same migrate.ts semantics, so the runtime
    // role is a non-owner of this schema exactly as it is of the others. The
    // cross-ledger FOREIGN KEY into `archava_commerce.products` means this must
    // run AFTER the commerce migration (it does — order above).
    let purchasing_ledger = migrate_ledger(
        &owner_pool,
        "archava_purchasing",
        "archava_purchasing_migrations",
    )
    .await;
    // Archxy 2 ERP namespace (PACKET-001): the foodservice ledger migrates
    // under the same owner role; its recipes JOIN `archava_commerce.products`
    // and its ticket flow claims commerce commands, so it runs after the
    // commerce and purchasing migrations (order above).
    let foodservice_ledger = migrate_ledger(
        &owner_pool,
        "archava_foodservice",
        "archava_foodservice_migrations",
    )
    .await;

    // 5. Runtime grants — the port of grantCommerceRuntime
    //    (`packages/commerce-pg/src/migrate.ts:59-76`), statement for statement.
    //    Including `lock_membership`: migration 001 defines it and REVOKEs it
    //    from PUBLIC, so without this grant the runtime role could not call the
    //    membership lock the baseline's kernel relies on.
    //
    //    Issued on `admin_pool`, which by construction is connected to
    //    `archava_test` — grants against `archava_commerce` run where the
    //    schema exists, which is the whole reason the pool above is not the
    //    maintenance-database connection.
    let grants = [
        format!("GRANT USAGE ON SCHEMA {NAMESPACE} TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON {NAMESPACE}.schema_migrations,{NAMESPACE}.memberships TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON {NAMESPACE}.products,{NAMESPACE}.orders,\
             {NAMESPACE}.allocations,{NAMESPACE}.commands TO {RUNTIME_USER}"
        ),
        format!("GRANT SELECT,UPDATE ON {NAMESPACE}.stock_balances TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT,INSERT,UPDATE,DELETE ON {NAMESPACE}.order_lines TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT ON {NAMESPACE}.movements,{NAMESPACE}.returns,\
             {NAMESPACE}.audit TO {RUNTIME_USER}"
        ),
        // RA01: the runtime reads approval requests, creates them, stamps the
        // approver onto them and consumes them — but never deletes them, and
        // the trigger in migration 004 refuses every transition back.
        format!("GRANT SELECT,INSERT,UPDATE ON {NAMESPACE}.approvals TO {RUNTIME_USER}"),
        format!(
            "GRANT EXECUTE ON FUNCTION {NAMESPACE}.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        // RA01 identity: the runtime authenticates (reads users, upserts login
        // buckets, inserts sessions) and nothing more. No UPDATE on users means
        // no in-band password change path exists yet; no DELETE on sessions
        // means the only session removal is expiry/idle pruning the baseline
        // itself performs.
        format!("GRANT USAGE ON SCHEMA archava_identity TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_identity.schema_migrations,archava_identity.users TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE,DELETE ON archava_identity.sessions,\
             archava_identity.login_buckets TO {RUNTIME_USER}"
        ),
        format!("GRANT INSERT ON archava_identity.events TO {RUNTIME_USER}"),
        // §1.4B CRM grants — the port of grantCrmRuntime
        // (`packages/crm-pg/src/migrate.ts:64-78`), statement for statement.
        // The slice's read path SELECTs contacts and opportunities; its
        // reviewed-command effect INSERTs a lead's contact and pipeline row;
        // memberships and history are read-only to the runtime, and
        // `lock_membership` exists because migration 001 REVOKEs it from
        // PUBLIC like its commerce twin.
        format!("GRANT USAGE ON SCHEMA archava_crm TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_crm.schema_migrations,archava_crm.memberships TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_crm.contacts,archava_crm.followups,\
             archava_crm.commands TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT ON archava_crm.activities,archava_crm.audit TO {RUNTIME_USER}"
        ),
        format!("GRANT SELECT,INSERT,UPDATE ON archava_crm.opportunities TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT,INSERT ON archava_crm.opportunity_transitions TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_crm.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        // Archxy 2 ERP (PACKET-000) — the port of grantPurchasingRuntime
        // (`packages/purchasing-pg/src/migrate.ts`), statement for statement:
        // history and memberships read-only; supplier/offer/order/line draft
        // writing; receipts and history append-only (`INSERT` only, the
        // immutability triggers refuse the rest anyway); `lock_membership`
        // executes because migration 001 REVOKEs it from PUBLIC like its
        // siblings.
        format!("GRANT USAGE ON SCHEMA archava_purchasing TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_purchasing.schema_migrations,\
             archava_purchasing.memberships TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_purchasing.suppliers,\
             archava_purchasing.offers,archava_purchasing.orders,\
             archava_purchasing.lines,archava_purchasing.commands TO {RUNTIME_USER}"
        ),
        format!("GRANT DELETE ON archava_purchasing.lines TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT,INSERT ON archava_purchasing.receipts,\
             archava_purchasing.receipt_lines,archava_purchasing.audit TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_purchasing.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        // Archxy 2 ERP (PACKET-001) — the port of grantFoodServiceRuntime
        // (`packages/foodservice-pg/src/migrate.ts`), statement for statement:
        // menus/tables/tickets draft writing, recipe versions editable while
        // unsealed, components and history append-only, `lock_membership`
        // executes because migration 001 REVOKEs it from PUBLIC.
        format!("GRANT USAGE ON SCHEMA archava_foodservice TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_foodservice.schema_migrations,\
             archava_foodservice.memberships TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_foodservice.tables,\
             archava_foodservice.menu_items,archava_foodservice.tickets,\
             archava_foodservice.commands,archava_foodservice.recipe_versions TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT ON archava_foodservice.recipe_components,\
             archava_foodservice.ticket_lines,archava_foodservice.audit TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_foodservice.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
    ];
    for statement in grants {
        sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .execute(&admin_pool)
            .await
            .unwrap_or_else(|error| panic!("grant failed: {statement}\n{error}"));
    }

    let runtime_url = url(RUNTIME_USER, DATABASE, port);
    let runtime_pool = connect(&runtime_url, 4).await;
    let single_pool = connect(&runtime_url, 1).await;
    // Everything that can fail has now succeeded. Disarm the guard so its
    // `Drop` stops touching the directory, and hand cleanup to the `Fixture`.
    guard.disarm();
    Fixture {
        admin_pool,
        owner_pool,
        runtime_pool,
        single_pool,
        port,
        commerce_ledger,
        identity_ledger,
        crm_ledger,
        purchasing_ledger,
        foodservice_ledger,
        dir,
    }
}

fn url(user: &str, database: &str, port: u16) -> String {
    format!("postgres://{user}@127.0.0.1:{port}/{database}")
}

async fn connect(url: &str, max_connections: u32) -> PgPool {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(url)
        .await
        .unwrap_or_else(|error| panic!("connect to fixture at {url}: {error}"))
}

/// Wait until the server accepts connections, quoting the log on timeout.
async fn wait_ready(admin_url: &str, log: &Path) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if PgPoolOptions::new()
            .max_connections(1)
            .connect(admin_url)
            .await
            .is_ok()
        {
            return;
        }
        if Instant::now() > deadline {
            panic!(
                "fixture postgres did not become ready in 60s\nlog: {}",
                std::fs::read_to_string(log).unwrap_or_else(|_| "<no log>".to_string())
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Apply the commerce migrations exactly as `migrate.ts` does.
///
/// The same four steps, in the same order: advisory lock (so two test binaries
/// provisioning at once cannot interleave), schema + ledger table, history
/// check that REFUSES modified history, then apply-and-record per migration.
/// The checksums recorded here are `archava_adapters::rls::migration_checksum`
/// of the file bytes — the same function the readiness check recomputes.
async fn migrate_commerce(pool: &PgPool) -> Vec<(String, String)> {
    migrate_ledger(pool, NAMESPACE, "archava_commerce_migrations").await
}

/// Apply one ledger's migrations with `migrate.ts`'s semantics.
///
/// Parameterized over the namespace and the frozen file list so the identity
/// ledger gets the exact treatment the commerce one has always had — one
/// implementation of "refuses modified history" rather than two that can
/// drift. The advisory lock label differs per ledger so the two cannot
/// interleave when both run in one provisioning transaction sequence.
/// The package that owns one namespace's migration directory.
///
/// Three namespaces, three owners, and getting this wrong is a fixture that
/// panics reading a file that does not exist: `archava_crm`'s migrations live
/// in `packages/crm-pg/`, not in the commerce package that once owned every
/// ledger this fixture applied. Resolved by explicit namespace rather than by
/// "not identity means commerce" so a fourth ledger cannot silently inherit
/// the wrong directory.
fn migrations_package(namespace: &str) -> &'static str {
    match namespace {
        "archava_identity" => "identity-pg",
        "archava_crm" => "crm-pg",
        // The Archxy 2 ERP cohorts' migrations live in the packages they were
        // frozen in; no other ledger may inherit one of these directories.
        "archava_purchasing" => "purchasing-pg",
        "archava_foodservice" => "foodservice-pg",
        _ => "commerce-pg",
    }
}

/// The `(id, checksum)` pairs a namespace's frozen migration list produces
/// from the migration files as they exist RIGHT NOW — the input
/// [`migrate_ledger`] records and `assert_startup_readiness` re-verifies.
///
/// Extracted from `migrate_ledger`'s inline mapping so the two readers of the
/// migration files (a fresh apply and a restart's re-derivation) cannot drift:
/// the whole point of the pair list is that it is derivable from files, not
/// from whatever one process happened to store. The namespace selects the
/// frozen file list as well as the migrations directory.
pub fn ledger_expectations(namespace: &str) -> Vec<(String, String)> {
    let migrations = match namespace {
        "archava_identity" => IDENTITY_MIGRATIONS,
        "archava_crm" => CRM_MIGRATIONS,
        "archava_purchasing" => PURCHASING_MIGRATIONS,
        "archava_foodservice" => FOOD_MIGRATIONS,
        _ => COMMERCE_MIGRATIONS,
    };
    let package = migrations_package(namespace);
    let migrations_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../../packages/{package}/migrations"));
    migrations
        .iter()
        .map(|id| {
            let path = migrations_dir.join(format!("{id}.sql"));
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|error| panic!("read migration {}: {error}", path.display()));
            (
                id.to_string(),
                archava_adapters::rls::migration_checksum(&bytes),
            )
        })
        .collect()
}

/// Attach to a provisioned-and-still-running instance whose creator process
/// died without dropping it.
///
/// This is the RESTART primitive the slice's browser proof needs: a fixture
/// `Drop`ped normally stops its server and deletes its data, and a service
/// restart must NOT pay that price — the recovery contract is "the robot's
/// receipts survive a service restart because the database is durable", and
/// this is how a second process takes over an instance the first one lost
/// (`kill -9` skips every drop guard, leaving the postmaster and its data
/// directory alive; the reaper at [`instance_dir`] keeps exactly such a
/// directory, because the postmaster pid in `data/postmaster.pid` is alive).
///
/// The attach does NOT re-run migrations or re-issue grants: the instance's
/// history was gated when the first process provisioned it (`migrate_ledger`
/// refuses modified history; the re-derivation below would read the same
/// files) — attaching only opens the pools the serving state needs and
/// re-derives the ledger expectations from the same frozen file lists. The
/// runtime role and its grants are per-cluster facts, made by `provision`.
///
/// An attach takes over the lifecycle exactly like the original provision:
/// dropping the returned [`Fixture`] stops the server and removes the
/// directory. Ungraceful attachment points (the directory absent, no
/// `postmaster.pid`) are refused in one line, so an operator cannot silently
/// serve from something that was never an instance.
pub async fn attach(data_dir: &Path, port: u16) -> Fixture {
    assert!(
        data_dir.join("postmaster.pid").exists(),
        "attach refused: {} does not look like a running instance (no postmaster.pid)",
        data_dir.display()
    );
    // For the drop-guard's lifecycle bookkeeping: understood as owning the
    // instance from here on, the server stops and the directory is removed
    // when this Fixture is dropped — what an operator restarting a service
    // means by "owning" it.
    let dir = data_dir
        .parent()
        .expect("a data dir always sits in one fixture dir")
        .to_path_buf();

    let admin_pool = connect(&url(ADMIN_USER, DATABASE, port), 2).await;
    let owner_pool = connect(&url(OWNER_USER, DATABASE, port), 1).await;
    let runtime_url = url(RUNTIME_USER, DATABASE, port);
    let runtime_pool = connect(&runtime_url, 4).await;
    let single_pool = connect(&runtime_url, 1).await;

    Fixture {
        admin_pool,
        owner_pool,
        runtime_pool,
        single_pool,
        port,
        commerce_ledger: ledger_expectations(NAMESPACE),
        identity_ledger: ledger_expectations("archava_identity"),
        crm_ledger: ledger_expectations("archava_crm"),
        purchasing_ledger: ledger_expectations("archava_purchasing"),
        foodservice_ledger: ledger_expectations("archava_foodservice"),
        dir,
    }
}

async fn migrate_ledger(pool: &PgPool, namespace: &str, lock_label: &str) -> Vec<(String, String)> {
    let package = migrations_package(namespace);
    let migrations_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../../packages/{package}/migrations"));
    // The recorded pairs come from the SAME derivation an attach re-runs —
    // one definition, so a restart always recomputes what a fresh apply
    // recorded.
    let ledger = ledger_expectations(namespace);

    let mut transaction = pool.begin().await.expect("begin migration transaction");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(lock_label)
        .execute(&mut *transaction)
        .await
        .expect("advisory migration lock");
    // `namespace` reaches identifier positions below. Both call sites pass
    // crate-level constants; `AssertSqlSafe` marks the interpolation as a
    // reviewed fact rather than an unchecked string.
    let create_schema = format!("CREATE SCHEMA IF NOT EXISTS {namespace}");
    sqlx::query(sqlx::AssertSqlSafe(create_schema.as_str()))
        .execute(&mut *transaction)
        .await
        .expect("create schema");
    let create_history = format!(
        "CREATE TABLE IF NOT EXISTS {namespace}.schema_migrations(\
         id text PRIMARY KEY, checksum text NOT NULL, \
         applied_at timestamptz NOT NULL DEFAULT now())"
    );
    sqlx::query(sqlx::AssertSqlSafe(create_history.as_str()))
        .execute(&mut *transaction)
        .await
        .expect("create schema_migrations");

    let history_query =
        format!("SELECT id,checksum FROM {namespace}.schema_migrations ORDER BY id");
    let existing: Vec<(String, String)> =
        sqlx::query_as(sqlx::AssertSqlSafe(history_query.as_str()))
            .fetch_all(&mut *transaction)
            .await
            .expect("read existing migration history");
    assert!(
        existing.len() <= ledger.len()
            && existing
                .iter()
                .zip(&ledger)
                .all(|((eid, esum), (id, sum))| eid == id && esum == sum),
        "Unsupported or modified {namespace} migration history \
         (a fresh fixture cannot have history — this means the migrations dir changed)"
    );

    for (id, checksum) in ledger.iter().skip(existing.len()) {
        let path = migrations_dir.join(format!("{id}.sql"));
        let sql = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read migration {}: {error}", path.display()));
        // `raw_sql` runs the file over the simple protocol, which supports the
        // multiple statements and `$$`-quoted plpgsql bodies the migrations
        // contain; the prepared-statement API cannot. The `AssertSqlSafe`
        // audit: these bytes come from `packages/{package}/migrations/*.sql`
        // in this repository, read by absolute path from a frozen file list —
        // the same input `migrateCommerce` runs, and the checksums recorded
        // below are of these same bytes.
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql.as_str()))
            .execute(&mut *transaction)
            .await
            .unwrap_or_else(|error| panic!("apply migration {namespace}/{id}: {error}"));
        let record =
            format!("INSERT INTO {namespace}.schema_migrations(id,checksum) VALUES($1,$2)");
        sqlx::query(sqlx::AssertSqlSafe(record.as_str()))
            .bind(id)
            .bind(checksum)
            .execute(&mut *transaction)
            .await
            .expect("record migration");
    }
    transaction.commit().await.expect("commit migrations");
    ledger
}

impl Drop for Fixture {
    /// Stop the server and delete the directory — the whole point of
    /// "disposable". A leaked instance would hold the port and the data until
    /// someone noticed; this makes the cost of forgetting zero.
    fn drop(&mut self) {
        if let Err(error) = stop_server(&self.dir.join("data")) {
            eprintln!("pg-fixture: pg_ctl stop failed during drop: {error}");
        }
        if let Err(error) = std::fs::remove_dir_all(&self.dir) {
            eprintln!(
                "pg-fixture: could not remove {}: {error}",
                self.dir.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A live process id for the liveness probes: this test's own pid.
    fn live_pid() -> u32 {
        std::process::id()
    }

    /// A process id that is almost certainly dead: a high value with no `/proc`
    /// entry. Used only through the injected probe, never as a real signal.
    const DEAD_PID: u32 = u32::MAX - 7;

    fn live(_: u32) -> bool {
        true
    }

    fn dead(_: u32) -> bool {
        false
    }

    // --- reap_verdict: the decision, on a controlled clock and probe --------

    /// The RF04 headline: a LIVE fixture older than the age gate must be kept.
    /// Before the fix the reaper read the wrong pid path, saw no pid, concluded
    /// "not live", and deleted it.
    #[test]
    fn a_live_fixture_older_than_the_age_gate_is_preserved() {
        let verdict = reap_verdict(
            Duration::from_secs(600),
            Some(live_pid()),
            Some(live_pid()),
            STALE_MIN_AGE,
            live,
        );
        assert_eq!(verdict, ReapVerdict::KeepLive);
    }

    /// A young directory is kept unconditionally: it may be an in-flight
    /// provision whose `postmaster.pid` does not exist yet.
    #[test]
    fn an_inflight_provision_is_preserved_regardless_of_pids() {
        let verdict = reap_verdict(
            Duration::from_secs(1),
            Some(DEAD_PID),
            Some(DEAD_PID),
            STALE_MIN_AGE,
            dead,
        );
        assert_eq!(verdict, ReapVerdict::KeepTooYoung);
    }

    /// A genuinely dead fixture — old, creator gone, postmaster gone — is
    /// reaped. This is the leak the age gate exists to reclaim.
    #[test]
    fn a_dead_fixture_is_reaped() {
        let verdict = reap_verdict(
            Duration::from_secs(600),
            Some(DEAD_PID),
            Some(DEAD_PID),
            STALE_MIN_AGE,
            dead,
        );
        assert_eq!(verdict, ReapVerdict::Remove);
    }

    /// A live postmaster must win even if the creator pid reads as dead (its
    /// process may have exited while the server it spawned keeps running).
    #[test]
    fn a_live_postmaster_without_a_live_creator_is_preserved() {
        let verdict = reap_verdict(
            Duration::from_secs(600),
            Some(DEAD_PID),
            Some(live_pid()),
            STALE_MIN_AGE,
            |pid| pid == live_pid(),
        );
        assert_eq!(verdict, ReapVerdict::KeepLive);
    }

    /// A missing/unreadable creator pid is NOT proof the directory is safe to
    /// delete. The mandate is explicit on this point, so the verdict is "keep"
    /// rather than "remove" — a leak is recoverable, a deleted live instance is
    /// not.
    #[test]
    fn a_missing_creator_pid_is_not_proof_of_death() {
        let verdict = reap_verdict(
            Duration::from_secs(600),
            None,
            Some(DEAD_PID),
            STALE_MIN_AGE,
            dead,
        );
        assert_eq!(verdict, ReapVerdict::KeepUnknown);
    }

    /// Both pids missing, past the age gate: still kept, for the same reason.
    #[test]
    fn no_pids_at_all_is_kept_not_deleted() {
        let verdict = reap_verdict(Duration::from_secs(600), None, None, STALE_MIN_AGE, dead);
        assert_eq!(verdict, ReapVerdict::KeepUnknown);
    }

    // --- The parsers the verdict depends on --------------------------------

    /// The creator pid is embedded in the directory name.
    #[test]
    fn the_creator_pid_is_parsed_from_the_directory_name() {
        assert_eq!(
            creator_pid_from_name("archava-pg-fixture-4242-1712345678901234567"),
            Some(4242)
        );
        // A foreign directory that merely shares the prefix parses to None and
        // is therefore kept, never deleted.
        assert_eq!(
            creator_pid_from_name("archava-pg-fixture-notanumber-1"),
            None
        );
        assert_eq!(creator_pid_from_name("some-other-dir-4242-1"), None);
    }

    /// The postmaster pid is read from **`data/postmaster.pid`** — the RF04
    /// correction. A filesystem canary proves the path, so a regression that
    /// reads the old `<fixture>/postmaster.pid` location fails here.
    #[test]
    fn the_postmaster_pid_is_read_from_the_data_directory() {
        let canary = std::env::temp_dir().join(format!(
            "archava-pg-fixture-canary-{}-{}",
            std::process::id(),
            "rf04"
        ));
        let data = canary.join("data");
        std::fs::create_dir_all(&data).expect("create canary");
        // The real layout: the pid file lives inside `data/`.
        let mine = live_pid();
        std::fs::write(data.join("postmaster.pid"), format!("{mine}\n123\n"))
            .expect("write canary pid");

        assert_eq!(postmaster_pid_from(&data), Some(mine));

        // Reading the OLD (wrong) location finds nothing — this is what made
        // the reaper treat every live fixture as dead.
        assert_eq!(postmaster_pid_from(&canary), None);

        let _ = std::fs::remove_dir_all(&canary);
    }

    /// The `Fixture::drop` and the guard share one stop path, so a server is
    /// stopped identically whether teardown is normal or a failure unwind.
    #[test]
    fn stop_server_targets_the_data_directory() {
        // A data directory that does not exist: `pg_ctl stop` fails, but the
        // helper returns Ok (best-effort) rather than panicking — teardown must
        // never be the thing that aborts a cleanup.
        let missing = std::env::temp_dir().join("archava-pg-fixture-definitely-missing");
        assert!(stop_server(&missing).is_ok());
    }
}
