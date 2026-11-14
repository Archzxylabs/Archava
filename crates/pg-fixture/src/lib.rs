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

pub use archava_adapters::writer::{connect as connect_writer, WriterLease};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tokio::sync::Semaphore;

/// One ledger of `migrations/manifest.v1.json`: its schema, the directory its
/// files live in under `migrations/`, and its migration ids in apply order.
struct ManifestLedger {
    namespace: String,
    directory: String,
    ids: Vec<String>,
}

/// The migration manifest, the same list the Ops installer applies and the
/// production server embeds.
///
/// The fixture used to hold its own frozen list per ledger and its own
/// schema-to-directory table. Both are read from the manifest instead, so a
/// new migration reaches the installer, the server and every integration suite
/// from one entry. `assert_startup_readiness` still compares the applied
/// `schema_migrations` against checksums of these same files.
fn manifest() -> &'static [ManifestLedger] {
    static MANIFEST: std::sync::OnceLock<Vec<ManifestLedger>> = std::sync::OnceLock::new();
    MANIFEST.get_or_init(|| {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../migrations/manifest.v1.json"))
                .expect("migrations/manifest.v1.json is valid JSON");
        manifest["ledgers"]
            .as_array()
            .expect("the migration manifest lists ledgers")
            .iter()
            .map(|ledger| {
                let namespace = ledger["namespace"]
                    .as_str()
                    .expect("a manifest ledger names its namespace");
                let mut directory = None;
                let ids = ledger["migrations"]
                    .as_array()
                    .expect("a manifest ledger lists migrations")
                    .iter()
                    .map(|migration| {
                        let id = migration["id"].as_str().expect("a migration has an id");
                        let path = migration["path"].as_str().expect("a migration has a path");
                        let found = path
                            .strip_prefix("migrations/")
                            .and_then(|rest| rest.strip_suffix(&format!("/{id}.sql")))
                            .unwrap_or_else(|| {
                                panic!("{namespace}: {path} is not migrations/<directory>/{id}.sql")
                            });
                        assert!(
                            directory.get_or_insert(found) == &found,
                            "{namespace}: migrations are spread over more than one directory"
                        );
                        id.to_owned()
                    })
                    .collect();
                ManifestLedger {
                    namespace: namespace.to_owned(),
                    directory: directory
                        .unwrap_or_else(|| panic!("{namespace}: no migrations in the manifest"))
                        .to_owned(),
                    ids,
                }
            })
            .collect()
    })
}

/// One ledger of the manifest. An unknown namespace is a fixture bug, and
/// panics rather than borrowing another ledger's files.
fn manifest_ledger(namespace: &str) -> &'static ManifestLedger {
    manifest()
        .iter()
        .find(|ledger| ledger.namespace == namespace)
        .unwrap_or_else(|| panic!("{namespace} is not in migrations/manifest.v1.json"))
}

/// How many migrations the manifest declares for one ledger.
pub fn migration_count(namespace: &str) -> usize {
    manifest_ledger(namespace).ids.len()
}

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
    /// Fixed writer identity shared only by pools owned by this fixture process.
    pub writer_lease: WriterLease,
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
    /// `docs/archive/rust/archxy2/INTEGRATION_REQUESTS.md` PACKET-000).
    pub purchasing_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_foodservice` — the stock cohort's second
    /// ERP ledger (PACKET-001): tickets, recipes and the menus whose menu
    /// products live IN `archava_commerce`.
    pub foodservice_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_organizations`, `archava_proposals` and
    /// `archava_projects` — the R04 cohorts' ledgers (PACKET-003).
    pub organizations_ledger: Vec<(String, String)>,
    pub accounting_ledger: Vec<(String, String)>,
    pub proposals_ledger: Vec<(String, String)>,
    pub projects_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_scheduling` — the R05 reservations
    /// cohort (PACKET-004; the occupancy kernel rides migration 002).
    pub scheduling_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_accommodation` — the R05 stays cohort
    /// (PACKET-005).
    pub accommodation_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_assets` — the R05 custody cohort
    /// (PACKET-006).
    pub assets_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_billing` — the R07 settlement cohort
    /// (PACKET-007), the eight operational tables the balance CHECK and
    /// the guard triggers defend.
    pub billing_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_subscriptions` — the R07 entitlements
    /// cohort (PACKET-008), the six operational tables the quota CHECK,
    /// the usage-idempotency UNIQUE constraint and the immutability
    /// triggers defend.
    pub subscriptions_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_attendance` — the R07 attendance
    /// cohort (PACKET-009), the six operational tables the roster/ticket
    /// UNIQUE constraints, the capacity CHECK and the immutability
    /// triggers defend.
    pub attendance_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_support` — the R07 support cohort
    /// (PACKET-010), the six operational tables the deferred completeness
    /// trigger and the history/command guards defend.
    pub support_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_laundry` — the R08 laundry cohort.
    pub laundry_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_field` — the R08 field services cohort.
    pub field_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_fulfillment` — the R08 fulfillment cohort.
    pub fulfillment_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_edu` — the R08 formal education cohort.
    pub edu_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_owner` — the owner-configuration
    /// revisions, receipts, audit and grants (C03).
    pub owner_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_provider` — the R09 provider attempt
    /// ledger (buckets, pending→finalized attempts, restore holds), through
    /// whose forced-RLS scope the provider budget is a real PostgreSQL
    /// transaction, not a client counter.
    pub provider_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_cockpit` — autonomous flight missions, steps, events and checkpoints.
    pub cockpit_ledger: Vec<(String, String)>,
    /// The same ledger for `archava_orbz` — semantic knowledge nodes, episodic streams and trajectories.
    pub orbz_ledger: Vec<(String, String)>,
    dir: PathBuf,
}

impl Fixture {
    /// All non-primary ERP ledgers mounted by `AppState::try_new_erp`.
    ///
    /// The host owns the table descriptors; the fixture owns the checksum
    /// vectors. Returning the pair keeps integration tests from accidentally
    /// checking only the purchasing ledger while still avoiding a dependency
    /// from the fixture crate back into the host crate.
    pub fn erp_readiness(&self) -> Vec<(&'static str, &[(String, String)])> {
        let mut ledgers: Vec<(&'static str, &[(String, String)])> = vec![
            ("archava_foodservice", &self.foodservice_ledger),
            ("archava_organizations", &self.organizations_ledger),
            ("archava_proposals", &self.proposals_ledger),
            ("archava_projects", &self.projects_ledger),
            ("archava_scheduling", &self.scheduling_ledger),
            ("archava_accommodation", &self.accommodation_ledger),
            ("archava_assets", &self.assets_ledger),
            ("archava_billing", &self.billing_ledger),
            ("archava_subscriptions", &self.subscriptions_ledger),
            ("archava_attendance", &self.attendance_ledger),
            ("archava_support", &self.support_ledger),
            ("archava_laundry", &self.laundry_ledger),
            ("archava_field", &self.field_ledger),
            ("archava_fulfillment", &self.fulfillment_ledger),
            ("archava_edu", &self.edu_ledger),
            ("archava_cockpit", &self.cockpit_ledger),
            ("archava_orbz", &self.orbz_ledger),
        ];
        if self.accounting_ledger.len() == migration_count("archava_accounting") {
            ledgers.push(("archava_accounting", &self.accounting_ledger));
        }
        ledgers
    }

    /// A connection URL for the runtime role.
    ///
    /// A test that must prove something SURVIVES A RESTART (RA01: "sessions
    /// survive restart") cannot reuse [`Fixture::runtime_pool`] for the
    /// post-restart state: a `PgPool` is a live process resource, and handing
    /// the same pool to a second `AppState` would share the very connection
    /// state the test claims to have thrown away. A restart means new
    /// connections, and the URL is the honest way to ask for them.
    pub fn runtime_options(&self) -> PgConnectOptions {
        self.writer_lease.connection_options(
            self.runtime_url()
                .parse()
                .expect("fixture connection options"),
        )
    }

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

    /// A connection URL for the owner role — the role that applies migrations.
    ///
    /// The upgrade rehearsal needs a SECOND owner connection distinct from
    /// [`Fixture::owner_pool`] for the same reason
    /// [`Fixture::runtime_url`] exists for a restart: the rehearsal treats
    /// "the process that opened this database" and "the process upgrading it"
    /// as two separate live resources, and sharing a `PgPool` would share the
    /// very connection state the rehearsal claims to have handed off.
    pub fn owner_url(&self) -> String {
        format!(
            "postgres://{OWNER_USER}:archava@127.0.0.1:{}/{}",
            self.port, DATABASE
        )
    }
}

/// Apply the commerce ledger's remaining migrations as the owner role.
///
/// R10's upgrade primitive: called on a fixture provisioned by
/// [`provision_prior_commerce_profile`], it records the exact continuation a
/// real profile upgrade performs — the prefix history is accepted by
/// [`migrate_ledger`]'s checksummed-history assertion and only pending
/// migrations run. A "first call applies pending files; a second call is a
/// no-op" idempotence run is part of the rehearsal callers make, which is why
/// this is exposed as its own function rather than smuggled into provision.
pub async fn apply_pending_commerce_migrations(owner_pool: &PgPool) -> Vec<(String, String)> {
    let history = migrate_commerce(owner_pool).await;
    let control: bool = sqlx::query_scalar(
        "SELECT to_regprocedure('archava_control.install_runtime_write_guards()') IS NOT NULL",
    )
    .fetch_one(owner_pool)
    .await
    .expect("control protocol lookup");
    if control {
        sqlx::query("SELECT archava_control.install_runtime_write_guards()")
            .execute(owner_pool)
            .await
            .expect("refresh new-table write guards");
    }
    history
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
///
/// The *base* of that directory is chosen by [`choose_socket_base`], never
/// assumed: the cluster binds its Unix socket inside this directory (`-k`), so
/// the base decides whether the cluster can start at all.
fn instance_dir(port: u16) -> PathBuf {
    clean_stale_dirs();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let name = format!("{DIR_PREFIX}{}-{nanos}", std::process::id());
    let forced = std::env::var_os(SOCKET_DIR_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let base = choose_socket_base(&std::env::temp_dir(), forced.as_deref(), &name, port)
        .unwrap_or_else(|error| panic!("{error}"));
    let dir = base.join(name);
    std::fs::create_dir_all(&dir).expect("create fixture directory");
    dir
}

/// The longest Unix-domain socket path the kernel can bind.
///
/// `sockaddr_un.sun_path` is 108 bytes **including** the terminating NUL, so
/// 107 bytes is the largest usable path. PostgreSQL creates the socket as
/// `<socket dir>/.s.PGSQL.<port>`, which leaves a socket *directory* budget of
/// `107 - len("/.s.PGSQL.65535")` = 92 bytes.
///
/// Named, and checked, because of the audit's A5 finding — what happens when it
/// is not: `initdb` succeeds, `pg_ctl start` fails with the uninformative
/// `could not start server`, and every suite reports `0 passed / N failed` in
/// seconds without naming the socket or the fix.
pub const SUN_PATH_MAX: usize = 107;

/// Where instances go when `temp_dir()` cannot hold a bindable socket path.
///
/// `/tmp` is the one directory whose length is a constant of the Linux layout,
/// so the fallback makes the base choice independent of how long `TMPDIR`
/// happens to be.
const FALLBACK_SOCKET_BASE: &str = "/tmp";

/// Override for the socket base, under the same name the TypeScript half of
/// this fixture uses (`tools/library/pg-fixture.mjs`): one knob, both languages.
///
/// Deliberately *not* a silent substitute — a forced directory that does not
/// fit is reported (see [`SocketPathTooLong`]) rather than quietly replaced,
/// because the override exists so a diagnosis can be reproduced exactly.
const SOCKET_DIR_ENV: &str = "ARCHAVA_PG_SOCKET_DIR";

/// The path PostgreSQL binds inside `socket_dir` for `port`.
///
/// Public because the length invariant is the fixture's contract rather than an
/// internal detail: a caller asserts on the same path the server was started
/// with, so "the check passed" and "the server bound it" cannot drift apart.
pub fn socket_path(socket_dir: &Path, port: u16) -> PathBuf {
    socket_dir.join(format!(".s.PGSQL.{port}"))
}

/// The byte length of a path as the kernel counts it.
///
/// `sun_path` is a byte array, so the comparison must be in bytes and not
/// characters: a non-ASCII `TMPDIR` is longer on the wire than it looks.
fn path_bytes(path: &Path) -> usize {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().len()
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().len()
    }
}

/// Whether the socket PostgreSQL would bind at `socket_dir` fits in `sun_path`.
fn socket_fits(socket_dir: &Path, port: u16) -> bool {
    path_bytes(&socket_path(socket_dir, port)) <= SUN_PATH_MAX
}

/// Why a socket directory cannot be used: the whole diagnosis in one value.
///
/// A5's failure mode was not "the path was long" — it was that the long path
/// surfaced as `pg_ctl: could not start server` and every test was reported as
/// failed. So the failure carries the socket path, its byte length, the
/// `temp_dir()` value that produced it, and the knob that changes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketPathTooLong {
    /// The path PostgreSQL would have tried to bind.
    socket: PathBuf,
    /// `temp_dir()` at decision time — the value that is usually the cause.
    temp_dir: PathBuf,
    /// The forced base (from [`SOCKET_DIR_ENV`]) when that is what failed.
    forced: Option<PathBuf>,
}

impl SocketPathTooLong {
    /// The socket path PostgreSQL could not bind.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Its length in bytes — the number that exceeded [`SUN_PATH_MAX`].
    pub fn socket_len(&self) -> usize {
        path_bytes(&self.socket)
    }
}

impl std::fmt::Display for SocketPathTooLong {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            out,
            "pg-fixture: the PostgreSQL unix socket path does not fit in \
             sockaddr_un.sun_path ({SUN_PATH_MAX} bytes max)\n  socket path : {} ({} bytes)\n  \
             socket dir  : {}\n  temp_dir()  : {} ({} bytes)",
            self.socket.display(),
            self.socket_len(),
            self.socket.parent().unwrap_or(Path::new("")).display(),
            self.temp_dir.display(),
            path_bytes(&self.temp_dir),
        )?;
        if let Some(forced) = &self.forced {
            write!(out, "\n  forced by {SOCKET_DIR_ENV}: {}", forced.display())?;
        }
        write!(
            out,
            "\nfix: set {SOCKET_DIR_ENV} to a short directory (e.g. \
             {SOCKET_DIR_ENV}={FALLBACK_SOCKET_BASE}) or run with \
             TMPDIR={FALLBACK_SOCKET_BASE} — the fixture already falls back to \
             {FALLBACK_SOCKET_BASE} on its own, so reaching this message means the path was \
             forced longer than the fallback can hold"
        )
    }
}

/// Chooses the base directory for one instance so its socket path is bindable.
///
/// One mechanism per cluster, not two: the instance always binds a Unix socket
/// inside its own directory (`-k`), and the *directory* is chosen to make that
/// socket fit. `temp_dir()` is preferred — a caller who set `TMPDIR` meant it —
/// but `temp_dir()` is never *trusted* to be short: when its path would
/// overflow `sun_path`, [`FALLBACK_SOCKET_BASE`] is used instead. An explicit
/// [`SOCKET_DIR_ENV`] wins outright and is never substituted, so a forced long
/// directory produces this diagnosis instead of a silent repair.
fn choose_socket_base(
    temp_dir: &Path,
    forced: Option<&Path>,
    instance_name: &str,
    port: u16,
) -> Result<PathBuf, SocketPathTooLong> {
    let socket_dir = |base: &Path| base.join(instance_name);
    let failure = |base: &Path, is_forced: bool| SocketPathTooLong {
        socket: socket_path(&socket_dir(base), port),
        temp_dir: temp_dir.to_path_buf(),
        forced: is_forced.then(|| base.to_path_buf()),
    };

    if let Some(forced) = forced {
        return if socket_fits(&socket_dir(forced), port) {
            Ok(forced.to_path_buf())
        } else {
            Err(failure(forced, true))
        };
    }
    for base in [temp_dir, Path::new(FALLBACK_SOCKET_BASE)] {
        if socket_fits(&socket_dir(base), port) {
            return Ok(base.to_path_buf());
        }
    }
    Err(failure(Path::new(FALLBACK_SOCKET_BASE), false))
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
        for base in reap_bases() {
            clean_stale_dir(&base);
        }
    }
}

/// Every base directory an instance can have been created in.
///
/// [`choose_socket_base`] picks between `temp_dir()`, the `/tmp` fallback, and —
/// when set — the forced [`SOCKET_DIR_ENV`], so the reaper has to sweep all of
/// them. Sweeping only `temp_dir()` would orphan every directory created under a
/// base that is no longer selected, which is the opposite of what a reaper is
/// for. Deduplicated in preference order, so a `TMPDIR=/tmp` run does not scan
/// the same directory twice.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn reap_bases_from(temp_dir: PathBuf, forced: Option<PathBuf>) -> Vec<PathBuf> {
    let mut bases: Vec<PathBuf> = Vec::with_capacity(3);
    for base in forced
        .into_iter()
        .chain(std::iter::once(temp_dir))
        .chain(std::iter::once(PathBuf::from(FALLBACK_SOCKET_BASE)))
    {
        if !bases.contains(&base) {
            bases.push(base);
        }
    }
    bases
}

/// The bases this process would reap, read from the live environment.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn reap_bases() -> Vec<PathBuf> {
    reap_bases_from(
        std::env::temp_dir(),
        std::env::var_os(SOCKET_DIR_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
    )
}

/// Reaps the stale instances in one base directory.
///
/// Split out of [`clean_stale_dirs`] so that "which bases" (a list now, not a
/// single directory) and "how a candidate is judged" stay separate: the loop
/// above adds bases without re-deriving the safety argument below, and the
/// verdict itself still comes from [`reap_verdict`] on real inputs.
#[cfg(target_os = "linux")]
fn clean_stale_dir(base: &Path) {
    let Ok(entries) = std::fs::read_dir(base) else {
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
    let stop = Command::new("pg_ctl")
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
        .output();
    if let Err(error) = stop {
        // Teardown is best-effort. Provisioning still fails loudly when the
        // required PostgreSQL tools are absent; cleanup must not turn a test
        // failure into a second panic merely because pg_ctl is unavailable.
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(());
        }
        return Err(error);
    }
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
    provision_with_commerce_usize(migration_count("archava_commerce")).await
}

/// The for-rehearsal prior profile: "a database an earlier supported release
/// created", pinned through 003_purchase_receipts.
///
/// MIG-03 demands the upgrade story be proven for fresh AND for every
/// supported prior profile. On the Rust side the prior supported profile is
/// the ledger before `004_bound_approvals`: every other ledger migrates as today, and the commerce
/// ledger applies only through `003_purchase_receipts`. The fixture stays
/// otherwise identical — same roles, same frozen lists, same history
/// semantics — so an upgrade rehearsal against it exercises the REAL
/// continuation path (history prefix → full ledger), not a mock.
pub async fn provision_prior_commerce_profile() -> Fixture {
    provision_with_commerce_usize(3).await
}

/// A stock ledger immediately before durable acquisition costing existed.
pub async fn provision_pre_costing_commerce_profile() -> Fixture {
    provision_with_commerce_usize(4).await
}

/// An existing costing install before runtime cost-writer privilege reduction.
pub async fn provision_pre_private_cost_writers_profile() -> Fixture {
    provision_with_commerce_usize(5).await
}

/// Existing company-owned costing before exact return-layer/use provenance.
pub async fn provision_pre_return_provenance_profile() -> Fixture {
    provision_with_commerce_usize(9).await
}

/// The provision body, parameterized over how far into the commerce ledger the
/// "prior profile" window reaches. `provision` and
/// [`provision_prior_commerce_profile`] are its only callers.
async fn provision_with_commerce_usize(commerce_through: usize) -> Fixture {
    provision_profiles(
        commerce_through,
        migration_count("archava_organizations"),
        true,
        true,
    )
    .await
}

/// Owned upgrade fixture before verified CRM affiliation sources existed.
pub async fn provision_pre_verified_affiliations_profile() -> Fixture {
    provision_profiles(migration_count("archava_commerce"), 1, false, true).await
}

/// Latest pre-integration profile: published parties, legacy transaction ledgers.
pub async fn provision_pre_party_transaction_profile() -> Fixture {
    provision_profiles(7, 3, false, true).await
}

/// Profile before company-owned trade dimensions were mounted.
pub async fn provision_pre_company_trade_profile() -> Fixture {
    provision_profiles(8, 4, true, true).await
}

/// Previous accepted company-trade profile, before operational supplier documents.
pub async fn provision_pre_supplier_documents_profile() -> Fixture {
    provision_profiles(
        migration_count("archava_commerce"),
        migration_count("archava_organizations"),
        true,
        false,
    )
    .await
}

/// Latest profile before managed-company registration was mounted.
pub async fn provision_pre_company_profile() -> Fixture {
    provision_profiles(migration_count("archava_commerce"), 3, true, true).await
}

/// Upgrade transaction ledgers in dependency order using the migration owner.
pub async fn apply_pending_party_transaction_migrations(owner_pool: &PgPool) {
    apply_pending_organization_migrations(owner_pool).await;
    apply_pending_commerce_migrations(owner_pool).await;
    migrate_ledger(
        owner_pool,
        "archava_purchasing",
        "archava_purchasing_migrations",
    )
    .await;
    migrate_ledger(owner_pool, "archava_billing", "archava_billing_migrations").await;
    grant_supplier_document_runtime(owner_pool).await;
    // New dependent tables must enter the serving fence after the entire
    // upgrade, including supplier documents created after Organizations.
    sqlx::query("SELECT archava_control.install_runtime_write_guards()")
        .execute(owner_pool)
        .await
        .expect("refresh all upgraded trade writer guards");
}

/// Apply the current organization history prefix using the real owner path.
pub async fn apply_pending_organization_migrations(owner_pool: &PgPool) -> Vec<(String, String)> {
    let ledger = migrate_ledger(
        owner_pool,
        "archava_organizations",
        "archava_organizations_migrations",
    )
    .await;
    grant_party_runtime(owner_pool).await;
    grant_company_runtime(owner_pool).await;
    let control: bool = sqlx::query_scalar(
        "SELECT to_regprocedure('archava_control.install_runtime_write_guards()') IS NOT NULL",
    )
    .fetch_one(owner_pool)
    .await
    .expect("control protocol lookup");
    if control {
        sqlx::query("SELECT archava_control.install_runtime_write_guards()")
            .execute(owner_pool)
            .await
            .expect("refresh new-table write guards");
    }

    ledger
}

async fn grant_supplier_document_runtime(pool: &PgPool) {
    let present: bool = sqlx::query_scalar(
        "SELECT to_regclass('archava_purchasing.supplier_documents') IS NOT NULL",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    if present {
        for statement in [format!("GRANT SELECT,INSERT,UPDATE ON archava_purchasing.supplier_documents,archava_purchasing.supplier_document_allocations TO {RUNTIME_USER}"),format!("GRANT SELECT,INSERT ON archava_purchasing.supplier_document_versions TO {RUNTIME_USER}")] { sqlx::query(sqlx::AssertSqlSafe(statement.as_str())).execute(pool).await.unwrap(); }
    }
}

pub async fn apply_pending_accounting_migrations(owner_pool: &PgPool) -> Vec<(String, String)> {
    let ledger = migrate_ledger(
        owner_pool,
        "archava_accounting",
        "archava_accounting_migrations",
    )
    .await;
    grant_accounting_runtime(owner_pool).await;
    migrate_ledger(owner_pool, "archava_control", "archava_control_migrations").await;
    sqlx::query("SELECT archava_control.install_runtime_write_guards()")
        .execute(owner_pool)
        .await
        .expect("refresh accounting writer guards");
    ledger
}
async fn grant_accounting_runtime(pool: &PgPool) {
    let present: bool =
        sqlx::query_scalar("SELECT to_regclass('archava_accounting.books') IS NOT NULL")
            .fetch_one(pool)
            .await
            .unwrap();
    if present {
        for statement in [
        format!("GRANT USAGE ON SCHEMA archava_accounting TO {RUNTIME_USER}"),
        format!("GRANT SELECT ON archava_accounting.schema_migrations,archava_accounting.memberships TO {RUNTIME_USER}"),
        format!("GRANT SELECT,INSERT ON archava_accounting.journal_lines,archava_accounting.audit,archava_accounting.invoice_sources,archava_accounting.journal_sources TO {RUNTIME_USER}"),
        format!("GRANT SELECT,INSERT,UPDATE ON archava_accounting.books,archava_accounting.accounts,archava_accounting.periods,archava_accounting.journals,archava_accounting.commands,archava_accounting.invoice_policies TO {RUNTIME_USER}"),
        format!("GRANT EXECUTE ON FUNCTION archava_accounting.lock_membership(text,text) TO {RUNTIME_USER}"),
        format!("GRANT EXECUTE ON FUNCTION archava_accounting.source_guards_complete() TO {RUNTIME_USER}"),
    ] {sqlx::query(sqlx::AssertSqlSafe(statement.as_str())).execute(pool).await.unwrap();}
        let purchase_present: bool = sqlx::query_scalar(
            "SELECT to_regclass('archava_accounting.purchase_policies') IS NOT NULL",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if purchase_present {
            for statement in [
                format!("GRANT SELECT,INSERT,UPDATE ON archava_accounting.purchase_policies TO {RUNTIME_USER}"),
                format!("GRANT SELECT,INSERT ON archava_accounting.purchase_origins,archava_accounting.purchase_sources,archava_accounting.purchase_journal_sources TO {RUNTIME_USER}"),
                format!("GRANT EXECUTE ON FUNCTION archava_accounting.lock_purchase_company(text,uuid),archava_accounting.purchase_guards_complete() TO {RUNTIME_USER}"),
            ] {sqlx::query(sqlx::AssertSqlSafe(statement.as_str())).execute(pool).await.unwrap();}
        }
        let stock_present: bool = sqlx::query_scalar(
            "SELECT to_regclass('archava_accounting.stock_policies') IS NOT NULL",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if stock_present {
            for statement in [
                format!("GRANT SELECT ON archava_accounting.stock_layer_origins TO {RUNTIME_USER}"),
                format!("GRANT SELECT,INSERT,UPDATE ON archava_accounting.stock_policies TO {RUNTIME_USER}"),
                format!("GRANT SELECT,INSERT ON archava_accounting.stock_sources,archava_accounting.stock_journal_sources TO {RUNTIME_USER}"),
                format!("GRANT EXECUTE ON FUNCTION archava_accounting.stock_guards_complete() TO {RUNTIME_USER}"),
            ] {sqlx::query(sqlx::AssertSqlSafe(statement.as_str())).execute(pool).await.unwrap();}
        }
        let bank_present: bool = sqlx::query_scalar(
            "SELECT to_regclass('archava_accounting.bank_accounts') IS NOT NULL",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if bank_present {
            for statement in [
                format!("GRANT SELECT,INSERT ON archava_accounting.bank_accounts,archava_accounting.bank_statements,archava_accounting.bank_rows TO {RUNTIME_USER}"),
                format!("GRANT EXECUTE ON FUNCTION archava_accounting.bank_evidence_guards_complete() TO {RUNTIME_USER}"),
            ] {sqlx::query(sqlx::AssertSqlSafe(statement.as_str())).execute(pool).await.unwrap();}
        }
        let matching_present: bool = sqlx::query_scalar(
            "SELECT to_regclass('archava_accounting.bank_allocations') IS NOT NULL",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if matching_present {
            for statement in [
                format!("GRANT SELECT,INSERT ON archava_accounting.bank_allocations,archava_accounting.bank_unmatches TO {RUNTIME_USER}"),
                format!("GRANT EXECUTE ON FUNCTION archava_accounting.bank_matching_guards_complete() TO {RUNTIME_USER}"),
            ] {sqlx::query(sqlx::AssertSqlSafe(statement.as_str())).execute(pool).await.unwrap();}
        }
        let periods_present: bool = sqlx::query_scalar(
            "SELECT to_regclass('archava_accounting.bank_policies') IS NOT NULL",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if periods_present {
            for statement in [
                format!("GRANT SELECT,INSERT,UPDATE ON archava_accounting.bank_policies TO {RUNTIME_USER}"),
                format!("GRANT SELECT,INSERT ON archava_accounting.bank_reconciliations TO {RUNTIME_USER}"),
                format!("GRANT EXECUTE ON FUNCTION archava_accounting.bank_period_guards_complete() TO {RUNTIME_USER}"),
            ] {sqlx::query(sqlx::AssertSqlSafe(statement.as_str())).execute(pool).await.unwrap();}
        }
        let payables_present: bool = sqlx::query_scalar(
            "SELECT to_regclass('archava_accounting.payable_settlements') IS NOT NULL",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if payables_present {
            for statement in [
                format!("GRANT SELECT,INSERT ON archava_accounting.payable_settlements TO {RUNTIME_USER}"),
                format!("GRANT EXECUTE ON FUNCTION archava_accounting.payable_guards_complete() TO {RUNTIME_USER}"),
            ] {sqlx::query(sqlx::AssertSqlSafe(statement.as_str())).execute(pool).await.unwrap();}
        }
        let clearing_present: bool = sqlx::query_scalar(
            "SELECT to_regclass('archava_accounting.clearing_settlements') IS NOT NULL",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if clearing_present {
            for statement in [
                format!("GRANT SELECT,INSERT ON archava_accounting.clearing_settlements TO {RUNTIME_USER}"),
                format!("GRANT EXECUTE ON FUNCTION archava_accounting.clearing_guards_complete() TO {RUNTIME_USER}"),
            ] {sqlx::query(sqlx::AssertSqlSafe(statement.as_str())).execute(pool).await.unwrap();}
        }
        let terms_present: bool = sqlx::query_scalar(
            "SELECT to_regclass('archava_accounting.document_terms') IS NOT NULL",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if terms_present {
            for statement in [
                format!("GRANT SELECT,INSERT ON archava_accounting.document_terms TO {RUNTIME_USER}"),
                format!("GRANT EXECUTE ON FUNCTION archava_accounting.terms_guards_complete() TO {RUNTIME_USER}"),
            ] {sqlx::query(sqlx::AssertSqlSafe(statement.as_str())).execute(pool).await.unwrap();}
        }
        let openings_present: bool = sqlx::query_scalar(
            "SELECT to_regclass('archava_accounting.opening_balances') IS NOT NULL",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if openings_present {
            for statement in [
                format!("GRANT SELECT,INSERT ON archava_accounting.opening_balances TO {RUNTIME_USER}"),
                format!("GRANT EXECUTE ON FUNCTION archava_accounting.opening_guards_complete() TO {RUNTIME_USER}"),
            ] {sqlx::query(sqlx::AssertSqlSafe(statement.as_str())).execute(pool).await.unwrap();}
        }
        let tax_present: bool =
            sqlx::query_scalar("SELECT to_regclass('archava_accounting.tax_codes') IS NOT NULL")
                .fetch_one(pool)
                .await
                .unwrap();
        if tax_present {
            for statement in [
                format!("GRANT SELECT,INSERT,UPDATE ON archava_accounting.tax_codes TO {RUNTIME_USER}"),
                format!("GRANT EXECUTE ON FUNCTION archava_accounting.tax_guards_complete() TO {RUNTIME_USER}"),
            ] {sqlx::query(sqlx::AssertSqlSafe(statement.as_str())).execute(pool).await.unwrap();}
        }
        // Accounting 019: readiness over the tax guards outside Accounting.
        let tax_documents_present: bool = sqlx::query_scalar(
            "SELECT to_regprocedure('archava_accounting.tax_document_guards_complete()') IS NOT NULL",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if tax_documents_present {
            let statement = format!("GRANT EXECUTE ON FUNCTION archava_accounting.tax_document_guards_complete() TO {RUNTIME_USER}");
            sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
                .execute(pool)
                .await
                .unwrap();
        }
    }
}

async fn grant_company_runtime(pool: &PgPool) {
    let present: bool =
        sqlx::query_scalar("SELECT to_regclass('archava_organizations.companies') IS NOT NULL")
            .fetch_one(pool)
            .await
            .unwrap();
    if present {
        let statement = format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_organizations.companies TO {RUNTIME_USER}"
        );
        sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .execute(pool)
            .await
            .unwrap();
        let statement =
            format!("GRANT SELECT,INSERT ON archava_organizations.company_audit TO {RUNTIME_USER}");
        sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .execute(pool)
            .await
            .unwrap();
    }
}

async fn grant_party_runtime(pool: &PgPool) {
    let present: bool =
        sqlx::query_scalar("SELECT to_regclass('archava_organizations.parties') IS NOT NULL")
            .fetch_one(pool)
            .await
            .unwrap();
    if present {
        let statement = format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_organizations.parties TO {RUNTIME_USER}"
        );
        sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .execute(pool)
            .await
            .unwrap();
        let statement =
            format!("GRANT SELECT,INSERT ON archava_organizations.party_audit TO {RUNTIME_USER}");
        sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .execute(pool)
            .await
            .unwrap();
    }
}

async fn provision_profiles(
    commerce_through: usize,
    organizations_through: usize,
    party_transactions: bool,
    supplier_documents: bool,
) -> Fixture {
    provision_profile_options(
        commerce_through,
        organizations_through,
        party_transactions,
        supplier_documents,
        true,
    )
    .await
}

pub async fn provision_pre_accounting_profile() -> Fixture {
    provision_profile_options(
        migration_count("archava_commerce"),
        migration_count("archava_organizations"),
        true,
        true,
        false,
    )
    .await
}

async fn provision_profile_options(
    commerce_through: usize,
    organizations_through: usize,
    party_transactions: bool,
    supplier_documents: bool,
    accounting: bool,
) -> Fixture {
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
    // The port comes first: the socket lives inside the instance directory
    // (`-k`), so the port is part of the socket path and the length check has to
    // run on the exact path that will be bound, not an approximation of it (A5).
    let port = free_port();
    let dir = instance_dir(port);
    let data = dir.join("data");
    let log = dir.join("postgres.log");
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
    //    Collation is pinned to `C`: without it the cluster inherits the
    //    machine's locale, and a text `ORDER BY` returns `INVENTORY` before
    //    `INV-NEW` under en_US but after it under C — the same test then
    //    passes on one machine and fails on another. Character
    //    classification (`lc-ctype`) is left to the environment on purpose.
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
            "--lc-collate=C",
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
    //    instance directory. `-k` is the ONE socket mechanism for this cluster:
    //    `instance_dir` chose the directory so that `<dir>/.s.PGSQL.<port>` fits
    //    in `sockaddr_un.sun_path` (107 bytes), and reports a named error before
    //    initdb when it cannot — never `pg_ctl: could not start server` (A5).
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
    let organizations_ledger = migrate_ledger_prefix(
        &owner_pool,
        "archava_organizations",
        "archava_organizations_migrations",
        organizations_through,
    )
    .await;
    // Current party-linked ledgers require the Organizations registry. The old
    // affiliation profile retains its matching pre-party dependent histories.
    let commerce_through = if organizations_through < 3 || !party_transactions {
        commerce_through.min(7)
    } else {
        commerce_through
    };
    let commerce_through = if organizations_through < 4 {
        commerce_through.min(8)
    } else {
        commerce_through
    };
    // Customer-linked Commerce orders depend on the CRM contact identity schema.
    let commerce_ledger = if commerce_through == migration_count("archava_commerce") {
        migrate_commerce(&owner_pool).await
    } else {
        migrate_commerce_upto(&owner_pool, commerce_through).await
    };
    // Archxy 2 ERP namespace (PACKET-000): the purchasing ledger migrates under
    // the same owner role, with the same migrate.ts semantics, so the runtime
    // role is a non-owner of this schema exactly as it is of the others. The
    // cross-ledger FOREIGN KEY into `archava_commerce.products` means this must
    // run AFTER the commerce migration (it does — order above).
    let purchasing_ledger = migrate_ledger_prefix(
        &owner_pool,
        "archava_purchasing",
        "archava_purchasing_migrations",
        if organizations_through < 3 || !party_transactions {
            3
        } else if organizations_through < 4 || commerce_through < 9 {
            4
        } else if !supplier_documents {
            5
        } else {
            usize::MAX
        },
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
    // Archxy 2 ERP namespace (PACKET-003): the three R04 ledgers migrate under
    // the same owner role from their own packages; ordering after the commerce
    // family keeps every ledger's advisory lock label distinct.
    let proposals_ledger = migrate_ledger(
        &owner_pool,
        "archava_proposals",
        "archava_proposals_migrations",
    )
    .await;
    let projects_ledger = migrate_ledger(
        &owner_pool,
        "archava_projects",
        "archava_projects_migrations",
    )
    .await;
    // Archxy 2 ERP namespace (PACKET-004): the scheduling ledger applies BOTH
    // migrations under the same owner role; 002 carries the shared occupancy
    // kernel and the accommodation entry-point RPC.
    let scheduling_ledger = migrate_ledger(
        &owner_pool,
        "archava_scheduling",
        "archava_scheduling_migrations",
    )
    .await;
    // (PACKET-005) the accommodation ledger migrates AFTER scheduling: its
    // rooms reference `archava_scheduling.resources` and its stays sit inside
    // the scheduling occupancy kernel.
    let accommodation_ledger = migrate_ledger(
        &owner_pool,
        "archava_accommodation",
        "archava_accommodation_migrations",
    )
    .await;
    // (PACKET-006) the assets ledger, its own namespace with no shared kernel.
    let assets_ledger =
        migrate_ledger(&owner_pool, "archava_assets", "archava_assets_migrations").await;
    // Archxy 2 ERP namespace (PACKET-007): the billing ledger migrates
    // under the same owner role, after the R05 family — no cross-schema
    // FOREIGN KEY exists, but the namespace's one printed apply order
    // holds (the R07 area's first subject).
    let billing_ledger = migrate_ledger_prefix(
        &owner_pool,
        "archava_billing",
        "archava_billing_migrations",
        if organizations_through < 3 || !party_transactions {
            6
        } else if organizations_through < 4 || commerce_through < 9 {
            7
        } else {
            usize::MAX
        },
    )
    .await;
    // Source capture references the current company-owned Billing schema.
    // Historical profiles retain their earlier surface until explicit upgrade.
    let accounting_ledger = if accounting
        && organizations_through >= 4
        && billing_ledger.len() == migration_count("archava_billing")
    {
        migrate_ledger_prefix(
            &owner_pool,
            "archava_accounting",
            "archava_accounting_migrations",
            if purchasing_ledger.len() != migration_count("archava_purchasing") {
                3
            } else if commerce_through < 10 {
                4
            } else {
                usize::MAX
            },
        )
        .await
    } else {
        Vec::new()
    };
    // Archxy 2 ERP namespace (PACKET-008): the subscriptions ledger
    // migrates under the same owner role, after the settlement sibling —
    // no cross-schema FOREIGN KEY exists, but the namespace's one printed
    // apply order holds.
    let subscriptions_ledger = migrate_ledger(
        &owner_pool,
        "archava_subscriptions",
        "archava_subscriptions_migrations",
    )
    .await;
    // Archxy 2 ERP namespace (PACKET-009): the attendance ledger migrates
    // under the same owner role, after the entitlements sibling — no
    // cross-schema FOREIGN KEY exists, but the namespace's one printed
    // apply order holds.
    let attendance_ledger = migrate_ledger(
        &owner_pool,
        "archava_attendance",
        "archava_attendance_migrations",
    )
    .await;
    // Archxy 2 ERP namespace (PACKET-010): the support ledger migrates under
    // the same owner role, after the attendance sibling.
    let support_ledger =
        migrate_ledger(&owner_pool, "archava_support", "archava_support_migrations").await;
    // R08: laundry cohort.
    let laundry_ledger =
        migrate_ledger(&owner_pool, "archava_laundry", "archava_laundry_migrations").await;
    // R08: field services cohort.
    let field_ledger =
        migrate_ledger(&owner_pool, "archava_field", "archava_field_migrations").await;
    // R08: fulfillment cohort.
    let fulfillment_ledger = migrate_ledger(
        &owner_pool,
        "archava_fulfillment",
        "archava_fulfillment_migrations",
    )
    .await;
    // R08: formal education cohort.
    let edu_ledger = migrate_ledger(&owner_pool, "archava_edu", "archava_edu_migrations").await;
    // R09's provider slice
    // R09's provider slice: the attempt ledger migrates under the same owner
    // role, one UTC-anchored schema next to the domain ledgers. The runtime
    // role must stay a NON-owner of `archava_provider` — `create`'s role
    // check refuses an owner exactly like the domain ledgers' checks.
    let provider_ledger = migrate_ledger(
        &owner_pool,
        "archava_provider",
        "archava_provider_migrations",
    )
    .await;
    // C03: the owner-configuration ledger, migrated by the same owner role so
    // the runtime is a non-owner here exactly as it is elsewhere.
    let owner_ledger =
        migrate_ledger(&owner_pool, "archava_owner", "archava_owner_migrations").await;
    // Cockpit & Orbz ledgers: migrated under owner role with forced RLS
    let cockpit_ledger =
        migrate_ledger(&owner_pool, "archava_cockpit", "archava_cockpit_migrations").await;
    let orbz_ledger = migrate_ledger(&owner_pool, "archava_orbz", "archava_orbz_migrations").await;
    let _control_ledger = if accounting && organizations_through >= 4 {
        migrate_ledger(&owner_pool, "archava_control", "archava_control_migrations").await
    } else {
        migrate_ledger_prefix(
            &owner_pool,
            "archava_control",
            "archava_control_migrations",
            1,
        )
        .await
    };

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
        //
        // The table only exists once 004 has applied, so the PRIOR-PROFILE
        // window (R10's rehearsal fixture: the ledger through 003 only) skips
        // this grant instead of failing on a table its frozen window does not
        // create yet — the upgrade's continuation run (the FULL window) is
        // what brings both the table and the grant. The full-window grant
        // list stays exactly the frozen baseline's list.
        format!("GRANT SELECT,INSERT,UPDATE ON {NAMESPACE}.approvals TO {RUNTIME_USER}"),
        format!(
            "GRANT EXECUTE ON FUNCTION {NAMESPACE}.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        // RA01 identity: the runtime authenticates (reads users, upserts login
        // buckets, inserts sessions) and nothing more. No UPDATE on users means
        // no in-band password change path exists yet; no DELETE on sessions
        // means the only session removal is expiry/idle pruning the baseline
        // itself performs.
        format!("GRANT SELECT ON archava_commerce.inventory_layers,archava_commerce.inventory_uses,archava_commerce.inventory_return_costs TO {RUNTIME_USER}"),
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
        // Archxy 2 ERP (PACKET-003) — the port of grantOrganizationsRuntime
        // (`packages/organizations-pg/src/migrate.ts`), statement for
        // statement: memberships read, organizations/affiliations/commands/
        // audit rw, `lock_membership` executes.
        format!("GRANT USAGE ON SCHEMA archava_organizations TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_organizations.schema_migrations,\
             archava_organizations.memberships TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_organizations.organizations,\
             archava_organizations.affiliations,archava_organizations.commands,\
             archava_organizations.audit TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_organizations.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        // Archxy 2 ERP (PACKET-003) — the port of grantProposalsRuntime:
        // proposals/scope versions/change requests rw, snapshots-editable
        // scope items, audit rw, `lock_membership` executes.
        format!("GRANT USAGE ON SCHEMA archava_proposals TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_proposals.schema_migrations,\
             archava_proposals.memberships TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_proposals.proposals,\
             archava_proposals.scope_versions,archava_proposals.change_requests,\
             archava_proposals.commands,archava_proposals.audit TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE,DELETE ON archava_proposals.scope_items TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_proposals.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        // Archxy 2 ERP (PACKET-003) — the port of grantProjectsRuntime:
        // projects/tasks/milestones rw, audit INSERT-only, `lock_membership`
        // executes.
        format!("GRANT USAGE ON SCHEMA archava_projects TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_projects.schema_migrations,\
             archava_projects.memberships TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_projects.projects,archava_projects.tasks,\
             archava_projects.milestones,archava_projects.commands TO {RUNTIME_USER}"
        ),
        format!("GRANT SELECT,INSERT ON archava_projects.audit TO {RUNTIME_USER}"),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_projects.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        // Archxy 2 ERP (PACKET-004) — the port of grantSchedulingRuntime:
        // resources/reservations/commands rw, the shared occupancy kernel
        // rw (the accommodation cohort reserves through it), audit insert.
        format!("GRANT USAGE ON SCHEMA archava_scheduling TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_scheduling.schema_migrations,\
             archava_scheduling.memberships TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_scheduling.resources,\
             archava_scheduling.reservations,archava_scheduling.commands TO {RUNTIME_USER}"
        ),
        format!("GRANT SELECT,INSERT ON archava_scheduling.audit TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_scheduling.occupancy TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_scheduling.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        // Archxy 2 ERP (PACKET-005) — the port of grantAccommodationRuntime:
        // properties/stay-nights insert, room types/rooms/stays/commands rw,
        // audit insert, the membership and property locks, and the
        // create-accommodation-resource RPC the scheduling migration owns.
        format!("GRANT USAGE ON SCHEMA archava_accommodation TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_accommodation.schema_migrations,\
             archava_accommodation.memberships TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT ON archava_accommodation.properties,\
             archava_accommodation.stay_nights TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_accommodation.room_types,\
             archava_accommodation.rooms,archava_accommodation.stays,\
             archava_accommodation.commands TO {RUNTIME_USER}"
        ),
        format!("GRANT SELECT,INSERT ON archava_accommodation.audit TO {RUNTIME_USER}"),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_accommodation.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_accommodation.lock_property(text,uuid) TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_scheduling.create_accommodation_resource(text,uuid,text,text) TO {RUNTIME_USER}"
        ),
        // Archxy 2 ERP (PACKET-006) — the port of grantAssetsRuntime: schema
        // usage, memberships read, assets/custody/maintenance/commands/audit
        // rw, and the membership lock.
        format!("GRANT USAGE ON SCHEMA archava_assets TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_assets.schema_migrations,\
             archava_assets.memberships TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_assets.assets,\
             archava_assets.custody_events,archava_assets.maintenance_records,\
             archava_assets.commands,archava_assets.audit TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_assets.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        // Archxy 2 ERP namespace (PACKET-007): the billing grants — the port
        // of grantBillingRuntime (`packages/billing-pg/src/migrate.ts`),
        // statement for statement: schema usage, the history+membership
        // read, the invoice/command writes (the mutation surface's UPDATE
        // + INSERT tables), the append-only child inserts, the audit
        // insert, and the membership lock's SECURITY DEFINER function.
        format!("GRANT USAGE ON SCHEMA archava_billing TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_billing.schema_migrations,\
             archava_billing.memberships,archava_billing.gateway_runtime TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_billing.invoices,\
             archava_billing.commands,archava_billing.gateway_links,archava_billing.email_outbox TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT ON archava_billing.invoice_lines,\
             archava_billing.invoice_corrections,archava_billing.payment_attempts,\
             archava_billing.payment_events,archava_billing.audit,archava_billing.gateway_observations TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_billing.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        // Archxy 2 ERP namespace (PACKET-008): the subscriptions grants —
        // the port of grantSubscriptionsRuntime
        // (`packages/subscriptions-pg/src/migrate.ts`), statement for
        // statement: schema usage, the history+membership read, the
        // subscription/entitlement/usage/command/audit writes, and the
        // membership lock's SECURITY DEFINER function.
        format!("GRANT USAGE ON SCHEMA archava_subscriptions TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_subscriptions.schema_migrations,\
             archava_subscriptions.memberships TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_subscriptions.subscriptions,\
             archava_subscriptions.entitlements,archava_subscriptions.usage_events,\
             archava_subscriptions.commands,archava_subscriptions.audit TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_subscriptions.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        // Archxy 2 ERP namespace (PACKET-009): the attendance grants — the
        // port of grantAttendanceRuntime
        // (`packages/attendance-pg/src/migrate.ts`), statement for
        // statement: schema usage, the history+membership read, the
        // session/roster/event/command/audit writes, and the membership
        // lock's SECURITY DEFINER function.
        format!("GRANT USAGE ON SCHEMA archava_attendance TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_attendance.schema_migrations,\
             archava_attendance.memberships TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_attendance.sessions,\
             archava_attendance.rosters,archava_attendance.check_in_events,\
             archava_attendance.commands,archava_attendance.audit TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_attendance.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        // Archxy 2 ERP namespace (PACKET-010): the support grants — the port
        // of grantSupportRuntime (`packages/support-pg/src/migrate.ts`),
        // statement for statement.
        format!("GRANT USAGE ON SCHEMA archava_support TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_support.schema_migrations,\
             archava_support.memberships TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_support.cases,\
             archava_support.commands TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT ON archava_support.notes,\
             archava_support.transitions,archava_support.audit TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_support.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        // R08 laundry cohort grants.
        format!("GRANT USAGE ON SCHEMA archava_laundry TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_laundry.schema_migrations,\
             archava_laundry.memberships TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_laundry.orders,\
             archava_laundry.commands TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT ON archava_laundry.order_lines,\
             archava_laundry.process_events,archava_laundry.findings,archava_laundry.audit TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_laundry.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        // R08 field services cohort grants.
        format!("GRANT USAGE ON SCHEMA archava_field TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_field.schema_migrations,\
             archava_field.memberships TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_field.jobs,\
             archava_field.commands TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT ON archava_field.visits,\
             archava_field.work_lines,archava_field.signoffs,archava_field.audit TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_field.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        // R08 fulfillment cohort grants.
        format!("GRANT USAGE ON SCHEMA archava_fulfillment TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_fulfillment.schema_migrations,\
             archava_fulfillment.memberships TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_fulfillment.jobs,\
             archava_fulfillment.commands TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT ON archava_fulfillment.job_lines,\
             archava_fulfillment.findings,archava_fulfillment.custody_events,archava_fulfillment.audit TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_fulfillment.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        format!("GRANT USAGE ON SCHEMA archava_edu TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_edu.schema_migrations,\
             archava_edu.memberships TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_edu.enrollments TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT SELECT,INSERT ON archava_edu.sessions,\
             archava_edu.results,archava_edu.reviews,archava_edu.commands,archava_edu.audit TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_edu.lock_membership(text,text) TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_edu.visible_enrollment(text,text,text,archava_edu.enrollments) TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_edu.results_published(text) TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_edu.assert_transition(text,text) TO {RUNTIME_USER}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION archava_edu.assert_score(numeric,numeric) TO {RUNTIME_USER}"
        ),
        // R09 provider slice — the port of grantProviderRuntime
        // (`packages/provider-runtime/src/postgres.ts`), statement for
        // statement, mirrored by
        // `archava_providers::store::grant_provider_runtime`: usage, history
        // read-only, and draft writing on buckets/attempts. `schema_migrations`
        // stays SELECT-only so only the migration path records history, and
        // the forced-RLS scope (`archava.provider_bucket`) makes every write
        // bucket-local regardless of the grant's breadth.
        // C03 owner configuration: grants and revisions are read-only to the
        // runtime; revisions, receipts and audit are append-only (no UPDATE,
        // no DELETE). RLS still scopes every row to the session tenant.
        format!("GRANT USAGE ON SCHEMA archava_owner TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT ON archava_owner.schema_migrations,             archava_owner.owner_grants,archava_owner.config_revisions,             archava_owner.config_audit TO {RUNTIME_USER}"
        ),
        format!("GRANT SELECT,INSERT,UPDATE ON archava_owner.reviewed_workflows TO {RUNTIME_USER}"),
        format!("GRANT SELECT,INSERT ON archava_owner.agent_threads,archava_owner.agent_turns,archava_owner.agent_preferences TO {RUNTIME_USER}"),
        format!("GRANT SELECT,INSERT,UPDATE ON archava_owner.agent_goals TO {RUNTIME_USER}"),
        format!("GRANT SELECT,INSERT,DELETE ON archava_owner.agent_work TO {RUNTIME_USER}"),
        format!("GRANT SELECT,INSERT ON archava_owner.agent_model_attempts TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT,INSERT ON archava_owner.config_revisions,             archava_owner.apply_receipts,archava_owner.config_audit TO {RUNTIME_USER}"
        ),
        format!("GRANT USAGE ON SCHEMA archava_provider TO {RUNTIME_USER}"),
        format!("GRANT SELECT ON archava_provider.schema_migrations TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON archava_provider.buckets,\
             archava_provider.attempts TO {RUNTIME_USER}"
        ),
        format!("GRANT USAGE ON SCHEMA archava_cockpit TO {RUNTIME_USER}"),
        format!("GRANT SELECT ON archava_cockpit.schema_migrations TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT,INSERT,UPDATE,DELETE ON archava_cockpit.missions,\
             archava_cockpit.flight_steps,archava_cockpit.flight_events,\
             archava_cockpit.checkpoints TO {RUNTIME_USER}"
        ),
        format!("GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA archava_cockpit TO {RUNTIME_USER}"),
        format!("GRANT USAGE ON SCHEMA archava_orbz TO {RUNTIME_USER}"),
        format!("GRANT SELECT ON archava_orbz.schema_migrations TO {RUNTIME_USER}"),
        format!(
            "GRANT SELECT,INSERT,UPDATE,DELETE ON archava_orbz.nodes,\
             archava_orbz.episodes,archava_orbz.trajectories TO {RUNTIME_USER}"
        ),
        format!("GRANT USAGE ON SCHEMA archava_control TO {RUNTIME_USER}"),
        format!("GRANT SELECT ON archava_control.schema_migrations TO {RUNTIME_USER}"),
        format!("GRANT EXECUTE ON FUNCTION archava_control.take_serving_writer(text,boolean),archava_control.require_serving_writer(),archava_control.drain_serving_writer(bigint,text),archava_control.write_guards_complete() TO {RUNTIME_USER}"),
    ];
    // 004-bound approvals is the upgrade's OWN surface: a prior-profile
    // window stands in for "before the current identity", so its newest
    // grant intentionally does not exist there yet. The full window keeps
    // the statement list byte-identical to the frozen baseline's port.
    let approvals_grant = format!("{NAMESPACE}.approvals");
    let grants: Vec<String> = grants
        .into_iter()
        .filter(|statement| commerce_through >= 4 || !statement.contains(approvals_grant.as_str()))
        .filter(|statement| {
            commerce_through >= 5 || !statement.contains("archava_commerce.inventory_")
        })
        .collect();
    for statement in grants {
        sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .execute(&admin_pool)
            .await
            .unwrap_or_else(|error| panic!("grant failed: {statement}\n{error}"));
    }

    let runtime_url = url(RUNTIME_USER, DATABASE, port);
    let options: PgConnectOptions = runtime_url.parse().expect("runtime connection options");
    let (runtime_pool, writer_lease) = connect_writer(options.clone(), 4, false)
        .await
        .expect("fixed fixture writer");
    let single_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(writer_lease.connection_options(options))
        .await
        .expect("single-connection fixture writer");
    // Everything that can fail has now succeeded. Disarm the guard so its
    // `Drop` stops touching the directory, and hand cleanup to the `Fixture`.
    guard.disarm();
    grant_party_runtime(&owner_pool).await;
    grant_company_runtime(&owner_pool).await;
    grant_supplier_document_runtime(&owner_pool).await;
    grant_accounting_runtime(&owner_pool).await;
    Fixture {
        admin_pool,
        owner_pool,
        runtime_pool,
        writer_lease,
        single_pool,
        port,
        commerce_ledger,
        identity_ledger,
        crm_ledger,
        purchasing_ledger,
        foodservice_ledger,
        organizations_ledger,
        accounting_ledger,
        proposals_ledger,
        projects_ledger,
        scheduling_ledger,
        accommodation_ledger,
        assets_ledger,
        billing_ledger,
        subscriptions_ledger,
        attendance_ledger,
        support_ledger,
        laundry_ledger,
        field_ledger,
        fulfillment_ledger,
        edu_ledger,
        provider_ledger,
        owner_ledger,
        cockpit_ledger,
        orbz_ledger,
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
    let ledger = migrate_ledger(pool, NAMESPACE, "archava_commerce_migrations").await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "GRANT EXECUTE ON FUNCTION archava_commerce.return_layer_guards_complete() TO {RUNTIME_USER}"
    )))
    .execute(pool)
    .await
    .expect("grant return provenance readiness");
    ledger
}

/// The commerce ledger applied only through a prefix of the frozen list.
///
/// This is the R10 rehearsal primitive for a PRIOR PROFILE database (MIG-03:
/// "fresh + every supported prior profile"): the current release identity adds
/// `004_bound_approvals` on top of the migrated-before history, so a fixture
/// standing in for "a prior supported profile's install" must hold the ledger
/// through `003_purchase_receipts` and NOT 004. The window is a count, not an
/// id, so a caller cannot name an id the frozen list does not contain, and the
/// recorded history is still a strict prefix of the frozen ledger — which is
/// exactly what makes the later full apply a legal continuation rather than a
/// rewrite: [`migrate_ledger`]'s history assertion consumes that prefix.
///
/// A `take` beyond the frozen list is refused in one line, the same way a
/// caller asking for history the package never shipped is refused.
async fn migrate_commerce_upto(pool: &PgPool, take: usize) -> Vec<(String, String)> {
    const LABEL: &str = "archava_commerce_migrations";
    let package = migrations_package(NAMESPACE);
    let migrations_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../migrations/{package}"));
    let ledger = ledger_expectations(NAMESPACE);
    assert!(
        take <= ledger.len(),
        "commerce migration window {take} exceeds the frozen ledger of {len}",
        len = ledger.len()
    );

    let mut transaction = pool.begin().await.expect("begin migration transaction");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(LABEL)
        .execute(&mut *transaction)
        .await
        .expect("advisory migration lock");
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE SCHEMA IF NOT EXISTS {NAMESPACE}"
    )))
    .execute(&mut *transaction)
    .await
    .expect("create schema");
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE TABLE IF NOT EXISTS {NAMESPACE}.schema_migrations(\
         id text PRIMARY KEY, checksum text NOT NULL, \
         applied_at timestamptz NOT NULL DEFAULT now())"
    )))
    .execute(&mut *transaction)
    .await
    .expect("create schema_migrations");

    let history_query =
        format!("SELECT id,checksum FROM {NAMESPACE}.schema_migrations ORDER BY id");
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
        "Unsupported or modified {NAMESPACE} migration history"
    );
    let applied = existing.len();
    assert!(
        applied <= take,
        "commerce history already reached {applied} migrations, past the rehearsal window {take}"
    );

    for (id, checksum) in ledger.iter().skip(applied).take(take - applied) {
        let path = migrations_dir.join(format!("{id}.sql"));
        let sql = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read migration {}: {error}", path.display()));
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql.as_str()))
            .execute(&mut *transaction)
            .await
            .unwrap_or_else(|error| panic!("apply migration {NAMESPACE}/{id}: {error}"));
        let record =
            format!("INSERT INTO {NAMESPACE}.schema_migrations(id,checksum) VALUES($1,$2)");
        sqlx::query(sqlx::AssertSqlSafe(record.as_str()))
            .bind(id)
            .bind(checksum)
            .execute(&mut *transaction)
            .await
            .expect("record migration");
    }
    transaction.commit().await.expect("commit migrations");
    ledger.iter().take(take).cloned().collect()
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
    &manifest_ledger(namespace).directory
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
    let migrations = &manifest_ledger(namespace).ids;
    let migrations_dir = migrations_dir(namespace);
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
    let options: PgConnectOptions = runtime_url.parse().expect("runtime connection options");
    let (runtime_pool, writer_lease) = connect_writer(options.clone(), 4, true)
        .await
        .expect("fixed fixture writer");
    let single_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(writer_lease.connection_options(options))
        .await
        .expect("single-connection fixture writer");

    Fixture {
        admin_pool,
        owner_pool,
        runtime_pool,
        writer_lease,
        single_pool,
        port,
        commerce_ledger: ledger_expectations(NAMESPACE),
        identity_ledger: ledger_expectations("archava_identity"),
        crm_ledger: ledger_expectations("archava_crm"),
        purchasing_ledger: ledger_expectations("archava_purchasing"),
        foodservice_ledger: ledger_expectations("archava_foodservice"),
        organizations_ledger: ledger_expectations("archava_organizations"),
        accounting_ledger: ledger_expectations("archava_accounting"),
        proposals_ledger: ledger_expectations("archava_proposals"),
        projects_ledger: ledger_expectations("archava_projects"),
        scheduling_ledger: ledger_expectations("archava_scheduling"),
        accommodation_ledger: ledger_expectations("archava_accommodation"),
        assets_ledger: ledger_expectations("archava_assets"),
        billing_ledger: ledger_expectations("archava_billing"),
        subscriptions_ledger: ledger_expectations("archava_subscriptions"),
        attendance_ledger: ledger_expectations("archava_attendance"),
        support_ledger: ledger_expectations("archava_support"),
        laundry_ledger: ledger_expectations("archava_laundry"),
        field_ledger: ledger_expectations("archava_field"),
        fulfillment_ledger: ledger_expectations("archava_fulfillment"),
        edu_ledger: ledger_expectations("archava_edu"),
        provider_ledger: ledger_expectations("archava_provider"),
        owner_ledger: ledger_expectations("archava_owner"),
        cockpit_ledger: ledger_expectations("archava_cockpit"),
        orbz_ledger: ledger_expectations("archava_orbz"),
        dir,
    }
}

/// The canonical directory for one namespace's SQL migrations. Both apply
/// and readiness re-derivation read `migrations/<ledger>` from the repository
/// root, so a restart cannot silently recompute a different input.
fn migrations_dir(namespace: &str) -> std::path::PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    root.join(format!("migrations/{}", migrations_package(namespace)))
}

async fn migrate_ledger(pool: &PgPool, namespace: &str, lock_label: &str) -> Vec<(String, String)> {
    migrate_ledger_prefix(pool, namespace, lock_label, usize::MAX).await
}

async fn migrate_ledger_prefix(
    pool: &PgPool,
    namespace: &str,
    lock_label: &str,
    through: usize,
) -> Vec<(String, String)> {
    let migrations_dir = migrations_dir(namespace);
    // The recorded pairs come from the SAME derivation an attach re-runs —
    // one definition, so a restart always recomputes what a fresh apply
    // recorded.
    let ledger = ledger_expectations(namespace)
        .into_iter()
        .take(through)
        .collect::<Vec<_>>();

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
            .unwrap_or_else(|error| panic!("apply migration {namespace}/{id}: {error:?}"));
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

    // --- A5: the socket path must fit in sun_path ---------------------------
    //
    // The decision is tested here without touching the environment: `TMPDIR` is
    // process-global, so a test that set it would race every other test in this
    // binary. The real cluster is exercised end to end in `tests/socket_dir.rs`.

    /// A `temp_dir()` long enough that PostgreSQL's socket cannot bind in it.
    /// 105 bytes is the length the audit measured on the kanban scratch path.
    fn long_temp_dir() -> PathBuf {
        Path::new("/tmp").join("a".repeat(100))
    }

    /// A directory name in the fixture's own format, so every check below runs
    /// on the name a real instance would actually get.
    fn instance_name() -> String {
        format!("{DIR_PREFIX}{}-1792622950458050376", std::process::id())
    }

    /// The A5 headline, on the decision itself: a long `temp_dir()` must not be
    /// used — its socket would not bind — and the base must fall back to `/tmp`.
    #[test]
    fn a_long_temp_dir_falls_back_to_a_bindable_base() {
        let (long, name) = (long_temp_dir(), instance_name());
        assert!(
            !socket_fits(&long.join(&name), 5432),
            "premise: {} must overflow the socket path limit",
            long.display()
        );

        let base =
            choose_socket_base(&long, None, &name, 5432).expect("a bindable fallback exists");

        assert_eq!(base, PathBuf::from(FALLBACK_SOCKET_BASE));
        assert!(socket_fits(&base.join(&name), 5432));
    }

    /// The fallback is conditional, not unconditional: a `temp_dir()` that does
    /// fit is still preferred, because a caller who set `TMPDIR` meant it.
    #[test]
    fn a_short_temp_dir_is_still_preferred() {
        let short = Path::new("/tmp").join("fixture-tests");
        let name = instance_name();
        assert_eq!(
            choose_socket_base(&short, None, &name, 5432),
            Ok(short.clone())
        );
    }

    /// The limit is checked against the real socket file name, at the boundary:
    /// one byte over must be refused, exactly at the limit must be accepted.
    #[test]
    fn the_limit_is_checked_against_the_real_socket_name() {
        assert_eq!(
            socket_path(Path::new("/tmp/x"), 5432),
            PathBuf::from("/tmp/x/.s.PGSQL.5432")
        );

        let suffix = "/.s.PGSQL.5432".len();
        let at_limit = PathBuf::from(format!(
            "/tmp/{}",
            "a".repeat(SUN_PATH_MAX - "/tmp/".len() - suffix)
        ));
        assert_eq!(path_bytes(&socket_path(&at_limit, 5432)), SUN_PATH_MAX);
        assert!(socket_fits(&at_limit, 5432));

        let over = PathBuf::from(format!("{}.", at_limit.display()));
        assert_eq!(path_bytes(&socket_path(&over, 5432)), SUN_PATH_MAX + 1);
        assert!(!socket_fits(&over, 5432));
    }

    /// `sun_path` is a byte array, so the limit counts bytes and not characters:
    /// a non-ASCII `TMPDIR` that looks short can still overflow it. This one is
    /// 50 characters (comfortably "under 107") and 109 bytes.
    #[test]
    fn the_limit_counts_bytes_not_characters() {
        let wide = PathBuf::from(format!("/tmp/{}", "é".repeat(45)));
        assert_eq!(wide.to_string_lossy().chars().count(), 50);
        assert_eq!(path_bytes(&wide), 95);
        assert!(!socket_fits(&wide, 5432));
    }

    /// A forced directory is never silently replaced, and its failure carries the
    /// whole diagnosis: the socket path, its length, the 107-byte limit, the
    /// `temp_dir()` value, and the knob that caused it.
    #[test]
    fn a_forced_long_socket_dir_is_reported_not_repaired() {
        let name = instance_name();
        let forced = long_temp_dir().join("forced");
        let error = choose_socket_base(Path::new("/tmp"), Some(&forced), &name, 5432)
            .expect_err("a forced directory that does not fit must be an error");

        assert_eq!(error.forced.as_deref(), Some(forced.as_path()));
        assert!(error.socket_len() > SUN_PATH_MAX);

        let message = error.to_string();
        let socket_path_text = error.socket.display().to_string();
        for needle in [
            "sockaddr_un.sun_path",
            "107 bytes max",
            "temp_dir()",
            SOCKET_DIR_ENV,
            socket_path_text.as_str(),
        ] {
            assert!(message.contains(needle), "missing {needle:?} in: {message}");
        }
    }

    /// The reaper must sweep every base the fixture can create in, or a
    /// directory is orphaned the moment the selection changes.
    #[test]
    fn the_reaper_sweeps_every_base_the_fixture_can_create_in() {
        let long = long_temp_dir();
        let fallback = PathBuf::from(FALLBACK_SOCKET_BASE);
        assert_eq!(
            reap_bases_from(long.clone(), None),
            vec![long.clone(), fallback.clone()]
        );

        let forced = PathBuf::from("/tmp/forced");
        assert_eq!(
            reap_bases_from(long.clone(), Some(forced.clone())),
            vec![forced, long.clone(), fallback.clone()]
        );

        // `TMPDIR=/tmp` is one directory, not two: no double scan, no double reap.
        assert_eq!(reap_bases_from(fallback.clone(), None), vec![fallback]);
    }

    /// The base moved, the NAME did not: a directory created by the new base is
    /// still parseable by the reaper's lifecycle guard.
    #[test]
    fn the_instance_name_keeps_the_creator_pid_contract() {
        assert_eq!(
            creator_pid_from_name(&instance_name()),
            Some(std::process::id())
        );
    }
}
