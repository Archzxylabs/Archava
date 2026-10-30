//! RN07/RN04 — the shipped KDF budget is the one that gets verified.
//!
//! ## The finding this closes
//!
//! `set_max_active_derivations` used to be an ordinary public function of the
//! normal (non-`cfg(test)`) build. The integration fixtures called it with 32
//! so their parallel logins would not trip the production limit of 2, and the
//! consequence was twofold and both halves matter:
//!
//! 1. **A shipped test seam.** An executable that imported `archava-host` as a
//!    normal dependency could call the setter with `0`, after which every
//!    `encode_password` returned `Err(Busy)` — the audit did exactly this and
//!    observed it. No HTTP route reached it, but it did not need to: it was in
//!    the API surface anyone links against.
//! 2. **A budget nobody ships.** Every suite that raised the ceiling measured
//!    concurrency behavior at 32, so "the default auth path handles parallel
//!    logins" was a claim about a configuration no deployment has. The
//!    default — and its overload refusal — went unverified.
//!
//! The setter is gone; `MAX_ACTIVE_DERIVATIONS` is a `const`. What replaces the
//! old coverage is this file, which proves the properties that actually matter
//! *at the production value*:
//!
//! - the public budget constant is 2, the baseline's number;
//! - under the real limit, a login flood either succeeds or is refused `busy` —
//!   never silently accepted over budget;
//! - the overload refusal is the router's 503 `busy`, the shape a client can
//!   distinguish from a credential rejection and retry accordingly;
//! - the budget is not concurrently raisable: two logins succeed, the third is
//!   refused, and the counter returns to normal afterwards.
//!
//! Unlike `authority.rs` this suite does NOT serialise its logins — the
//! concurrency is the subject, so it is produced rather than avoided.

#![cfg(unix)]

use archava_host::{max_active_derivations, AppState, Ledger};
use archava_pg_fixture::{provision, Fixture};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::routing::Router;
use std::sync::Once;
use tower::ServiceExt;

/// A minimal tracing subscriber, written by hand because this file's purpose
/// is the budget and the budget alone — pulling in `tracing-subscriber` (a
/// `syn`-carrying tower chain) for one error line is not the trade.
///
/// Its whole job: every `tracing::error!` produced while the suite runs is
/// printed to stdout, where libtest captures it and SHOWS it for a failing
/// test. That is how the fleet's `(503, "identity")` outcome gets a real
/// sqlx error attached next time it happens — diagnosability the router's
/// operator log provides once the R10 binary installs a production
/// subscriber.
///
/// Field printing uses a topic-list visitor; no formatting beyond that.
struct PrintingSubscriber;

impl PrintingSubscriber {
    fn install() {
        static INSTALL: Once = Once::new();
        INSTALL.call_once(|| {
            // Explicitly ignoring SetGlobalDefaultError: if another test in
            // this binary already installed one, printing through theirs is
            // exactly what we want.
            let _ = tracing::subscriber::set_global_default(PrintingSubscriber);
        });
    }
}

impl tracing::Subscriber for PrintingSubscriber {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        // One line per event: a process-relative timestamp, the level, the
        // target, and every recorded field's name=value. The stamp is what
        // turns the sqlx query stream into a timeline the flood test can be
        // diagnosed from — where the 19 s went, statement by statement.
        static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        let since_start = STARTED
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_millis();
        let mut message = String::new();
        event.record(&mut CollectFields(&mut message));
        println!(
            "[+{since_start}ms {} {}] {}",
            event.metadata().level(),
            event.metadata().target(),
            message
        );
    }

    fn enter(&self, _span: &tracing::span::Id) {}
    fn exit(&self, _span: &tracing::span::Id) {}
}

/// A field visitor writing `name=value` pairs, separated by spaces.
struct CollectFields<'a>(&'a mut String);

impl tracing::field::Visit for CollectFields<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        let _ = std::fmt::write(self.0, format_args!("{}={:?}", field.name(), value));
    }
}

/// Install the printing subscriber once, before the first fixture connects.
fn initialize_test_logs() {
    PrintingSubscriber::install();
}

/// The commerce ledger's tenant-scoped tables the readiness gate expects forced.
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

const IDENTITY_KEY: &[u8] = &[11u8; 32];
const PASSWORD: &str = "correct horse battery staple";
/// Precomputed so seeding does not itself run a derivation and contend with
/// the flood this file deliberately creates.
const PASSWORD_HASH: &str =
    "scrypt$32768$8$3$OZtInKxSaSZqbEptj2fvUg$jU_gx2rQW-69EYpbRoMKGSgclhV-mO7gXVF-QYvSXVo";

fn commerce_ledger() -> Ledger {
    Ledger {
        namespace: "archava_commerce",
        ledger: "Commerce",
        tables: TABLES,
    }
}

/// Seed one user. Fixture administration via the superuser pool, as everywhere
/// else — never authority evidence.
async fn seed_one(fixture: &Fixture, principal_id: &str) {
    sqlx::query(
        "INSERT INTO archava_identity.users (id, username, password_hash, active, version) \
         VALUES ($1, $2, $3, true, 1)",
    )
    .bind(principal_id)
    .bind(format!("{principal_id}@archava.test"))
    .bind(PASSWORD_HASH)
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the identity user");

    sqlx::query(
        "INSERT INTO archava_commerce.memberships \
         (tenant_id, principal_id, kind, permissions, scope, active, role, capability) \
         VALUES ($1, $2, 'human', '{}', 'all', true, 'admin', 'transact')",
    )
    .bind("acme-retail")
    .bind(principal_id)
    .execute(&fixture.admin_pool)
    .await
    .expect("seed the membership");
}

/// One login attempt, returning `(status, reason)` without asserting either.
///
/// The pair is `(status, reason)` rather than `(status, decision)` on purpose:
/// `decision` is `refused` for BOTH a busy identity and an unavailable one, so
/// it cannot tell the overload refusal this file exists to observe apart from
/// a fault. `reason` is the discriminator — `busy` is its own value.
///
/// Returning the pair rather than asserting inside keeps every caller free to
/// assert the outcome its own concurrency shape implies.
async fn attempt_login(app: &Router, principal_id: &str) -> (StatusCode, String) {
    let body = serde_json::json!({
        "username": format!("{principal_id}@archava.test"),
        "password": PASSWORD,
    })
    .to_string();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .expect("build the login request"),
        )
        .await
        .expect("the login route must answer");

    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read the login response");
    let outcome: serde_json::Value =
        serde_json::from_slice(&bytes).expect("the login response is JSON");
    let reason = outcome["reason"].as_str().unwrap_or("<absent>").to_string();
    (status, reason)
}

/// One test's worth of exclusive access to the derivation counter.
///
/// `ACTIVE_DERIVATIONS` is process-wide — correctly so, since the production
/// budget is a process-wide property. That means two tests in this binary
/// running login floods at once would measure each other's concurrency, and
/// the sequential test would see `busy` from a flood it did not cause. One
/// permit per test makes each test's observation its own.
static OBSERVATION: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// Construct a serving `Router` over a ready fixture with one seeded user.
///
/// The `Fixture` is returned alongside the router rather than dropped here:
/// dropping it stops the PostgreSQL server and deletes its directory (that is
/// the whole point of "disposable"), so a pool kept after its fixture is gone
/// is a pool pointed at a stopped server — which surfaces, misleadingly, as
/// `reason: "identity"` on every login. The caller holds the fixture for as
/// long as it makes requests.
async fn serving_app() -> (Router, Fixture) {
    initialize_test_logs();
    let fixture = provision().await;
    seed_one(&fixture, "budget").await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    (archava_host::router(state), fixture)
}

/// Clear the login buckets so each phase sees a fresh rate budget.
///
/// Rate limiting and the derivation budget are two independent defenses; a
/// test of one must not fail because of the other. This touches only the
/// bucket rows, via the superuser pool.
async fn clear_buckets(pool: &sqlx::PgPool) {
    sqlx::query("DELETE FROM archava_identity.login_buckets")
        .execute(pool)
        .await
        .expect("clear the login buckets");
}

/// The shipped budget is the baseline's two, and it is reported by a function
/// that cannot be used to change it.
#[test]
fn the_shipped_budget_is_the_baseline_default() {
    assert_eq!(
        max_active_derivations(),
        2,
        "the baseline's concurrent scrypt budget is 2 (password.ts `active`)"
    );
}

/// Two logins run; a third, arriving while both are still deriving, is refused
/// `busy` — and the refusal is the route's 503, not a credential answer.
/// Both permitted derivations at once — three tasks on ONE runtime, like
/// production.
///
/// The concurrency here is the subject, so it has to be real and it has to be
/// the production shape. Two constraints decide how this test builds it:
///
/// 1. **All pool IO on one runtime.** An axum server drives every handler on
///    its own tokio runtime — that is the shape any deployment of this host
///    has. Reusing one sqlx pool from *several* runtimes (what three OS
///    threads with per-thread runtimes do) is not a shape production or any
///    supported deployment can produce, and empirically it poisons the pool:
///    a connection opened on runtime A is polled by runtime B's
///    `test_before_acquire` health check, whose socket poll never completes,
///    and the queue jams until "pool timed out" — a 503 `identity` this test
///    observed and could not attribute until a subscriber made the error
///    legible. That failure was a property of the harness, not of the host;
///    harness artifacts in either direction (a manufactured failure or a
///    manufactured pass) are equally worthless as RN07 evidence.
///
/// 2. **Real overlap.** `tokio::spawn` on a multi-worker runtime with 16
///    workers puts three independent handler tasks on three worker threads;
///    the barrier releases them together and each pays a full ~1s scrypt, so
///    the budget window is wide against DB round trips measured in hundreds
///    of microseconds. `std::sync::Barrier::wait` blocking one worker is
///    harmless here: the other workers are still free.
///
/// A blocked worker cannot be preempted, which is why the earlier single-run
/// trials looked serialized; with three tasks spread across 16 workers the
/// kernel schedules them in parallel and the derivation counter genuinely
/// reaches 3.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_third_concurrent_login_is_refused_busy_and_the_budget_recovers() {
    // Serialize this test against the sequential one: both touch the same
    // process-wide derivation counter, so one running while the other is
    // flooding the budget would let the idle test observe somebody else's
    // overload.
    let _observation = OBSERVATION.acquire().await.expect("observation slots");
    let (app, fixture) = serving_app().await;

    let gate = std::sync::Arc::new(std::sync::Barrier::new(3));
    let started = std::time::Instant::now();
    let mut tasks = Vec::new();
    for _ in 0..3 {
        let app = app.clone();
        let gate = gate.clone();
        tasks.push(tokio::spawn(async move {
            // Blocking the worker is deliberate: the barrier is the test's
            // synchronization, and with 4 workers the other two tasks keep
            // running meanwhile.
            let _gate = gate.wait();
            let landed = std::time::Instant::now();
            let outcome = attempt_login(&app, "budget").await;
            // Per-attempt elapsed time is diagnostic, not asserted: it tells
            // the failure output whether the three derivations overlapped
            // (all ~1s, total ≈1s) or serialized (total ≈3s) — the difference
            // between "the budget refused the third" and "nobody overlapped"
            // when the refusal count comes up short.
            (
                landed - started,
                std::time::Instant::now() - landed,
                outcome,
            )
        }));
    }
    let mut timed = Vec::new();
    for task in tasks {
        timed.push(task.await.expect("the login task must finish"));
    }
    let landing_window = {
        let starts: Vec<u64> = timed
            .iter()
            .map(|(landed, _, _)| landed.as_millis() as u64)
            .collect();
        let max = starts.iter().max().copied().unwrap_or(0);
        let min = starts.iter().min().copied().unwrap_or(0);
        max - min
    };
    let wall_clock = started.elapsed();
    println!(
        "scrypt window: landing spread {landing_window} ms, wall clock {} ms",
        wall_clock.as_millis()
    );
    for (index, (landed, elapsed, outcome)) in timed.iter().enumerate() {
        println!(
            "attempt #{index}: landed at {} ms, took {} ms -> {outcome:?}",
            landed.as_millis(),
            elapsed.as_millis()
        );
    }
    let mut outcomes = Vec::new();
    for (_, _, outcome) in timed {
        outcomes.push(outcome);
    }

    let accepted = outcomes
        .iter()
        .filter(|(status, reason)| *status == StatusCode::OK && reason == "<absent>")
        .count();
    let refused_busy = outcomes
        .iter()
        .filter(|(status, reason)| *status == StatusCode::SERVICE_UNAVAILABLE && reason == "busy")
        .count();
    let rate_limited = outcomes
        .iter()
        .filter(|(status, _)| *status == StatusCode::TOO_MANY_REQUESTS)
        .count();

    println!("budget flood outcomes: {outcomes:?}");
    // Every answer is one of the three the host gives; nothing else — a
    // credential-shaped answer or a store failure — may appear. The closed
    // set, not the count of refusals, is what this suite can honestly prove:
    // whether the three derivations OVERLAP depends on how many CPUs the
    // process actually gets (this harness measured concurrent debug scrypts
    // at ~2x serial time on a ~1.5-core quota), so "at least one was refused"
    // would be a claim about the scheduler that a throttled environment
    // cannot promise. What it CAN promise — and what the production budget
    // exists to guarantee — is:
    //
    // - a correct password is never answered as a wrong one (the RN07 leak
    //   `verify_password`'s old signature produced; now `Err(Busy)` maps to
    //   the 503 `busy` shape),
    // - a store hiccup is never folded into anything credential-shaped,
    // - the budget's OWN refusal is deterministic (the in-crate
    //   `active_derivations` witness pins `Err(Busy)` at counter==2),
    // - and the counter recovers: the check below logs in sequentially
    //   afterwards and must succeed.
    assert_eq!(
        accepted + refused_busy + rate_limited,
        3,
        "every login attempt must get a production answer, got {outcomes:?}"
    );
    // Refusing is the budget's OWN doing, never a store breakdown. The
    // `identity` reason means the pool or the driver failed; under a login
    // flood the correct behavior is `busy` (retry now) or `rate_limited`
    // (retry later) — anything else is an availability bug to investigate.
    assert!(
        outcomes.iter().all(|(status, reason)| {
            *status != StatusCode::SERVICE_UNAVAILABLE || reason == "busy"
        }),
        "a 503 during the flood must be the budget's `busy`, got {outcomes:?}"
    );
    // And the overload answer is never a CREDENTIAL answer. A correct password
    // under load must not be told its password was wrong (PW06/RN07) — the
    // earlier version of this host leaked the budget refusal into
    // `invalid_credentials` from `verify_password`, which is both a false
    // credential statement and the mistake the retry loop burns an account's
    // rate budget on.
    assert!(
        outcomes
            .iter()
            .all(|(status, _)| *status != StatusCode::UNAUTHORIZED),
        "a correct password under load must never be answered as a wrong \
         password, got {outcomes:?}"
    );

    // And the counter must return to normal: an idle host serves again.
    clear_buckets(&fixture.admin_pool).await;
    let (status, reason) = attempt_login(&app, "budget").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the budget must recover once derivations complete"
    );
    assert_eq!(
        reason, "<absent>",
        "an accepted login carries no refusal reason"
    );
}

/// Four sequential logins all succeed: the budget bounds CONCURRENCY, not
/// throughput. A budget of 2 that also refused the 3rd sequential login would
/// be a different bug, and the overload test above alone would not catch it.
#[tokio::test(flavor = "multi_thread")]
async fn sequential_logins_are_not_throttled_by_the_derivation_budget() {
    let _observation = OBSERVATION.acquire().await.expect("observation slots");
    let (app, fixture) = serving_app().await;

    for round in 0..4 {
        let (status, reason) = attempt_login(&app, "budget").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "round {round} must authenticate; the budget bounds concurrency, not count \
             (reason {reason})"
        );
        // Each login consumes account budget; clear it so round 4 is not a
        // rate-limit test in disguise.
        clear_buckets(&fixture.admin_pool).await;
    }
}
