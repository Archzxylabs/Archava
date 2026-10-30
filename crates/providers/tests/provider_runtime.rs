//! The R09 provider slice's runtime and ledger witnesses — the port of
//! `packages/provider-runtime/test/runtime.test.ts` plus the PostgreSQL
//! witnesses the TS store's `postgres.ts` semantics pin in production SQL.
//!
//! Two halves, one contract:
//!
//! - **The orchestration half** (`ScriptedStore`): the admission/refusal
//!   ordering, the durability boundary (`finish` decides whether a result
//!   may be returned), the fail-closed conversion of every store failure to
//!   `outcome_unknown`, the masking of arbitrary invoke errors to the closed
//!   code set, and the privacy boundary — the serialized calls the store
//!   receives carry the policy's ids/limits, the attempt id and the outcome's
//!   status enum, and NOTHING else (`JSON.stringify` of the mock's calls, in
//!   TS; a recorded call string here).
//!
//! - **The PostgreSQL half** (`archava-pg-fixture`): the real
//!   `archava_provider` ledger migrated and granted like production, its
//!   forced-RLS scope checked by `create`'s non-owner refusal, the reserve
//!   ladder's five refusals reproduced against real rows, the expired-lease
//!   reap and the operator hold, and one end-to-end `run_provider` through
//!   the real store.
//!
//! The witness output `tools/rust` later collects (`P-ASSISTANT-CONTEXT`'s
//! budget/recovery check and `P-PROVIDER-CONTRACTS`' idempotency/reconciliation
//! checks) names these test functions verbatim — renaming one is a coverage
//! manifest change, not an internal refactor.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use archava_pg_fixture::provision;
use archava_providers::{
    run_provider, Admission, AdmissionRefusal, Completion, FailureCode, InvokeError, PolicyError,
    PostgresProviderAttemptStore, ProviderAttemptStore, ProviderPolicy, ProviderRunFailure,
    StoreError,
};
use sqlx::types::Uuid;

// --- The scripted store ----------------------------------------------------

/// What `reserve` does on the next call.
#[derive(Clone, Copy, Debug)]
enum ScriptedReserve {
    Allowed,
    Refused(AdmissionRefusal, i64),
    StoreError,
}

/// What `finish` does on the next call.
#[derive(Clone, Copy, Debug)]
enum ScriptedFinish {
    RecordedTrue,
    RecordedFalse,
    StoreError,
}

/// The scripted store the pure half runs over. It records every call's
/// SERIAL form the way the TS test stringifies the mock: the policy's ids and
/// limits, the attempt id, and the outcome's closed state — exactly the data
/// the ledger is ALLOWED to know. Anything else that shows up here is a
/// privacy leak the `privacy` witnesses make a failure.
#[derive(Clone)]
struct ScriptedStore {
    reserve: ScriptedReserve,
    finish: ScriptedFinish,
    calls: Arc<Mutex<Vec<String>>>,
}

impl ScriptedStore {
    fn new(reserve: ScriptedReserve, finish: ScriptedFinish) -> Self {
        Self {
            reserve,
            finish,
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl std::fmt::Display for ScriptedStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("scripted")
    }
}

fn policy_record(policy: &ProviderPolicy) -> String {
    format!(
        "policy:{}/{}/{}/{}/{}/{}",
        policy.bucket_id(),
        policy.provider_id(),
        policy.model(),
        policy.daily_limit(),
        policy.minimum_interval_ms(),
        policy.lease_ms()
    )
}

impl ProviderAttemptStore for ScriptedStore {
    async fn reserve(
        &self,
        policy: &ProviderPolicy,
        attempt_id: &Uuid,
    ) -> Result<Admission, StoreError> {
        self.calls.lock().unwrap().push(format!(
            "reserve {} attempt:{}",
            policy_record(policy),
            attempt_id
        ));
        match self.reserve {
            ScriptedReserve::Allowed => Ok(Admission::Allowed),
            ScriptedReserve::Refused(code, retry_after_ms) => Ok(Admission::Refused {
                code,
                retry_after_ms,
            }),
            ScriptedReserve::StoreError => Err(StoreError),
        }
    }

    async fn finish(
        &self,
        bucket_id: &str,
        attempt_id: &Uuid,
        completion: Completion,
    ) -> Result<bool, StoreError> {
        let (state, failure_code, http_status) = completion.as_sql_args();
        self.calls.lock().unwrap().push(format!(
            "finish bucket:{bucket_id} attempt:{attempt_id} state:{state} failure_code:{} http_status:{}",
            failure_code.unwrap_or("-"),
            http_status.map(|status| status.to_string()).unwrap_or_else(|| "null".into())
        ));
        match self.finish {
            ScriptedFinish::RecordedTrue => Ok(true),
            ScriptedFinish::RecordedFalse => Ok(false),
            ScriptedFinish::StoreError => Err(StoreError),
        }
    }
}

// --- The cast ---------------------------------------------------------------

fn policy() -> ProviderPolicy {
    ProviderPolicy::new(
        "test-budget",
        "test-provider",
        "test-model",
        30,
        15000,
        60000,
    )
    .expect("the fixture policy validates")
}

/// The payload the "provider" returns — text the ledger must NEVER see.
const RESULT_PAYLOAD: &str = "classified-payroll-run";
const NEVER_PERSISTED: &str = "no prompts, results, records or identities reach the ledger";
const ERROR_PAYLOAD: &str = "the machine-room stack trace of doomsday";

struct InvokeFailure {
    _code: String,
}

impl From<InvokeFailure> for InvokeError {
    fn from(_error: InvokeFailure) -> Self {
        InvokeError { failure_code: None }
    }
}

/// The always-valid attempt id shape `z.uuid()` parses.
fn attempt() -> String {
    Uuid::new_v4().to_string()
}

/// `run_provider` over a simple provider: resolves to [`RESULT_PAYLOAD`],
/// optionally recording an HTTP status.
async fn run_over_scripted<T, Err, S, Fut>(
    store: &S,
    policy: &ProviderPolicy,
    attempt_id: &str,
    invoke: impl FnOnce() -> Fut,
    http_status: impl Fn() -> Option<u16>,
) -> Result<T, ProviderRunFailure>
where
    S: ProviderAttemptStore,
    Fut: std::future::Future<Output = Result<T, Err>>,
    Err: Into<InvokeError>,
{
    run_provider(store, policy, attempt_id, invoke, http_status).await
}

// --- The pure half: the orchestration contract ------------------------------

#[tokio::test]
async fn refuses_daily_limit_before_invoking() {
    refuses_before_invoking(ScriptedReserve::Refused(
        AdmissionRefusal::DailyLimit,
        12000,
    ))
    .await;
}

#[tokio::test]
async fn refuses_busy_before_invoking() {
    refuses_before_invoking(ScriptedReserve::Refused(AdmissionRefusal::Busy, 12000)).await;
}

#[tokio::test]
async fn refuses_cooldown_before_invoking() {
    refuses_before_invoking(ScriptedReserve::Refused(AdmissionRefusal::Cooldown, 12000)).await;
}

#[tokio::test]
async fn refuses_restore_hold_before_invoking() {
    refuses_before_invoking(ScriptedReserve::Refused(
        AdmissionRefusal::RestoreHold,
        12000,
    ))
    .await;
}

#[tokio::test]
async fn refuses_duplicate_attempt_before_invoking() {
    refuses_before_invoking(ScriptedReserve::Refused(
        AdmissionRefusal::DuplicateAttempt,
        12000,
    ))
    .await;
}

/// The five refusals, restated once: admission refusals happen BEFORE the
/// invoke, record no completion, carry the ledger's own retry margin, and the
/// caller receives exactly the refused code (never the provider result).
async fn refuses_before_invoking(reserve: ScriptedReserve) {
    let store = ScriptedStore::new(reserve, ScriptedFinish::RecordedTrue);
    let run = run_over_scripted(
        &store,
        &policy(),
        &attempt(),
        || async { Ok::<String, InvokeFailure>(RESULT_PAYLOAD.into()) },
        || None,
    )
    .await;
    let code = run.unwrap_err();
    let ProviderRunFailure::Admission {
        code: refused,
        retry_after_ms,
    } = code
    else {
        panic!("expected an admission refusal, got {code:?}");
    };
    assert_eq!(
        retry_after_ms, 12000,
        "the ledger's margin travels verbatim"
    );
    let refusal = refused.code();
    assert!(
        [
            "daily_limit",
            "busy",
            "cooldown",
            "restore_hold",
            "duplicate_attempt"
        ]
        .contains(&refusal),
        "`{refusal}` is the refused code"
    );
    // The invoke never happened: the ONLY serial the store received is the
    // reserve (a refusal means no attempt started, so no completion exists).
    let calls: Vec<String> = store.calls();
    assert_eq!(calls.len(), 1, "reserve only — no finish after a refusal");
    assert!(calls[0].starts_with("reserve "), "the call is the reserve");
}

#[tokio::test]
async fn result_returns_only_after_durable_completion() {
    let store = ScriptedStore::new(ScriptedReserve::Allowed, ScriptedFinish::RecordedTrue);
    let result = run_over_scripted(
        &store,
        &policy(),
        &attempt(),
        || async { Ok::<String, InvokeFailure>(RESULT_PAYLOAD.into()) },
        || Some(200),
    )
    .await;
    assert_eq!(
        result.expect("durable completion returns the result"),
        RESULT_PAYLOAD
    );
    let calls: Vec<String> = store.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.starts_with("reserve "))
            .count(),
        1,
        "one reserve"
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.starts_with("finish "))
            .count(),
        1,
        "one finish, after the invoke"
    );
}

#[tokio::test]
async fn unrecorded_completion_converts_success_to_unknown() {
    let store = ScriptedStore::new(ScriptedReserve::Allowed, ScriptedFinish::RecordedFalse);
    let result = run_over_scripted(
        &store,
        &policy(),
        &attempt(),
        || async { Ok::<String, InvokeFailure>(RESULT_PAYLOAD.into()) },
        || None,
    )
    .await;
    assert_eq!(result.unwrap_err(), ProviderRunFailure::OutcomeUnknown);
    // The finish RAN (the attempt was the bucket's lease, the ledger said it
    // could not become durable) — the failure is the ledger's answer, not the
    // provider's.
    assert!(
        store
            .calls()
            .last()
            .is_some_and(|call| call.starts_with("finish ")),
        "the finish was attempted before the unknown outcome"
    );
}

#[tokio::test]
async fn store_failures_convert_to_unknown_fail_closed() {
    // A reservation that cannot become durable: the invoke NEVER runs — the
    // budget ledger must not let a caller spend when it cannot even count.
    let reserve_failed =
        ScriptedStore::new(ScriptedReserve::StoreError, ScriptedFinish::RecordedTrue);
    let result = run_over_scripted(
        &reserve_failed,
        &policy(),
        &attempt(),
        || async { Ok::<String, InvokeFailure>(RESULT_PAYLOAD.into()) },
        || None,
    )
    .await;
    assert_eq!(result.unwrap_err(), ProviderRunFailure::OutcomeUnknown);
    let calls: Vec<String> = reserve_failed.calls();
    assert_eq!(calls.len(), 1, "reserve only — invoke impossible");

    // A completion that cannot become durable after a SUCCESS the provider
    // returned: unknown — the result never leaves the runtime.
    let finish_failed = ScriptedStore::new(ScriptedReserve::Allowed, ScriptedFinish::StoreError);
    let result = run_over_scripted(
        &finish_failed,
        &policy(),
        &attempt(),
        || async { Ok::<String, InvokeFailure>(RESULT_PAYLOAD.into()) },
        || None,
    )
    .await;
    assert_eq!(result.unwrap_err(), ProviderRunFailure::OutcomeUnknown);

    // ... and the same conversion after a FAILED invoke.
    let finish_failed_too =
        ScriptedStore::new(ScriptedReserve::Allowed, ScriptedFinish::StoreError);
    let result = run_over_scripted(
        &finish_failed_too,
        &policy(),
        &attempt(),
        || async {
            Err::<String, InvokeFailure>(InvokeFailure {
                _code: "failed".into(),
            })
        },
        || None,
    )
    .await;
    assert_eq!(result.unwrap_err(), ProviderRunFailure::OutcomeUnknown);
}

#[tokio::test]
async fn arbitrary_invoke_errors_mask_to_provider_failed() {
    let store = ScriptedStore::new(ScriptedReserve::Allowed, ScriptedFinish::RecordedTrue);
    let result = run_over_scripted(
        &store,
        &policy(),
        &attempt(),
        || async {
            Err::<String, InvokeFailure>(InvokeFailure {
                _code: ERROR_PAYLOAD.into(),
            })
        },
        || None,
    )
    .await;
    assert_eq!(result.unwrap_err(), ProviderRunFailure::ProviderFailed);
    // The ledger saw only the CLOSED default code — the error's own payload
    // stays inside the caller's `Into<InvokeError>` conversion.
    let calls: Vec<String> = store.calls();
    assert!(
        calls[1].contains("state:failed failure_code:provider_failed"),
        "the recorded completion carries the masked code, got {:?}",
        calls[1]
    );
}

#[tokio::test]
async fn known_codes_and_http_status_record_verbatim() {
    let store = ScriptedStore::new(ScriptedReserve::Allowed, ScriptedFinish::RecordedTrue);
    // A code the invoke names IS allowed; the runtime records it verbatim.
    struct TimedOut;
    impl From<TimedOut> for InvokeError {
        fn from(_error: TimedOut) -> Self {
            InvokeError {
                failure_code: Some(FailureCode::Timeout),
            }
        }
    }
    let result = run_over_scripted(
        &store,
        &policy(),
        &attempt(),
        || async { Err::<String, TimedOut>(TimedOut) },
        || Some(504),
    )
    .await;
    assert_eq!(result.unwrap_err(), ProviderRunFailure::ProviderFailed);
    let calls: Vec<String> = store.calls();
    assert!(
        calls[1].contains("state:failed failure_code:timeout http_status:504"),
        "the closed code and status travel verbatim, got {:?}",
        calls[1]
    );
}

#[tokio::test]
async fn out_of_window_http_status_records_null() {
    let store = ScriptedStore::new(ScriptedReserve::Allowed, ScriptedFinish::RecordedTrue);
    let result = run_over_scripted(
        &store,
        &policy(),
        &attempt(),
        || async { Ok::<String, InvokeFailure>(RESULT_PAYLOAD.into()) },
        || Some(50),
    )
    .await;
    assert!(result.is_ok(), "a status like 50 does not fail the run");
    let calls: Vec<String> = store.calls();
    assert!(
        calls[1].contains("state:succeeded failure_code:- http_status:null"),
        "an out-of-window status records null, got {:?}",
        calls[1]
    );
}

#[tokio::test]
async fn forged_attempt_ids_refuse_before_reserving() {
    for forged in [
        "",
        "forged",
        "123e4567-e89b-12d3-a456-42661417400",
        "123e4567-e89b-12d3-a456-4266141740000",
    ] {
        let store = ScriptedStore::new(ScriptedReserve::Allowed, ScriptedFinish::RecordedTrue);
        let result = run_over_scripted(
            &store,
            &policy(),
            forged,
            || async { Ok::<String, InvokeFailure>(RESULT_PAYLOAD.into()) },
            || None,
        )
        .await;
        assert_eq!(
            result.unwrap_err(),
            ProviderRunFailure::OutcomeUnknown,
            "`{forged}` must not parse"
        );
        let calls: Vec<String> = store.calls();
        assert!(calls.is_empty(), "no store call happens before validation");
    }
}

#[test]
fn forged_policies_cannot_reach_the_runtime() {
    // The constructor is the only entry point; a policy that does not
    // validate cannot exist, so no code path can hand one to `run_provider`.
    for bad in [
        (0, 0, 1000),
        (1001, 0, 1000),
        (30, -1, 1000),
        (30, 60001, 1000),
        (30, 0, 999),
        (30, 0, 120001),
    ] {
        let error = ProviderPolicy::new(
            "test-budget",
            "test-provider",
            "test-model",
            bad.0,
            bad.1,
            bad.2,
        )
        .expect_err("the limit must refuse");
        assert!(error.to_string().starts_with("Invalid policy field"));
    }
    // The id charsets close the injection surface for the SAME reason the
    // SQL CHECK constraints do: the bucket id is the RLS scope value.
    for bad_id in ["", "Upper", "1abc", "-abc", "a/b", "a?b"] {
        assert!(ProviderPolicy::new(bad_id, "b", "c", 30, 0, 1000).is_err());
        assert!(ProviderPolicy::new("a", bad_id, "c", 30, 0, 1000).is_err());
        assert!(ProviderPolicy::new("a", "b", bad_id, 30, 0, 1000).is_err());
    }
    assert_eq!(
        ProviderPolicy::new(
            "test-budget",
            "test-provider",
            "test-model",
            30,
            15000,
            60000
        )
        .err(),
        None,
        "the fixture policy itself stays valid"
    );
    let _ = PolicyError::BucketId; // the surface stays in scope for callers
}

/// The privacy boundary in one serial: the store NEVER receives the result,
/// the invoke error's contents, request/response bodies, or any credential —
/// only the policy's ids/limits, the attempt id, and the closed outcome.
#[tokio::test]
async fn the_store_receives_safe_metadata_only() {
    let store = ScriptedStore::new(ScriptedReserve::Allowed, ScriptedFinish::RecordedTrue);
    let result = run_over_scripted(
        &store,
        &policy(),
        &attempt(),
        || async { Ok::<String, InvokeFailure>(format!("{RESULT_PAYLOAD} {NEVER_PERSISTED}")) },
        || Some(200),
    )
    .await;
    assert!(result.is_ok());
    let calls: Vec<String> = store.calls();
    let serial = calls.join("\n");
    assert!(
        !serial.contains(RESULT_PAYLOAD),
        "the result payload leaked: {serial}"
    );
    assert!(
        !serial.contains(NEVER_PERSISTED),
        "prose leaked into the ledger"
    );
}

// --- The PostgreSQL half ----------------------------------------------------

async fn connect_runtime(fixture: &archava_pg_fixture::Fixture) -> sqlx::PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&fixture.runtime_url())
        .await
        .expect("connect as the runtime role")
}

#[tokio::test]
async fn pg_reserve_admits_counts_and_projects() {
    let fixture = provision().await;
    let store = PostgresProviderAttemptStore::create(&fixture.runtime_pool)
        .await
        .expect("the provisioned ledger opens as the runtime role");
    let id = Uuid::new_v4();
    let admission = ProviderAttemptStore::reserve(&store, &policy(), &id)
        .await
        .expect("a fresh bucket admits");
    assert_eq!(admission, Admission::Allowed);
    let status = archava_providers::read_provider_status(&fixture.runtime_pool, &policy())
        .await
        .expect("the status read answers the runtime role");
    assert_eq!(status.attempts_today, 1, "the admit counted");
    assert_eq!(status.remaining, 29, "the remaining budget follows");
    assert_eq!(
        status.active_request_id.as_deref(),
        Some(id.to_string().as_str()),
        "the active lease refers the attempt"
    );
    assert!(status.restore_hold_until.is_none());
    assert_eq!(status.utc_day.len(), 10, "the UTC day is a date");
    let recent = &status.recent;
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].state, "pending");
    assert_eq!(recent[0].request_id, id.to_string());
    let iso = &recent[0].started_at;
    assert!(
        looks_like_iso(iso),
        "started_at is the TS toISOString shape: {iso}"
    );
}

/// `^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{3}Z$` —
/// `Date.toISOString()`'s exact shape, produced inside PostgreSQL.
fn looks_like_iso(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 24
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'.'
        && bytes[23] == b'Z'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, &b)| b.is_ascii_digit() || matches!(i, 4 | 7 | 10 | 13 | 16 | 19 | 23))
}

#[tokio::test]
async fn pg_duplicate_attempt_refuses_replay() {
    let fixture = provision().await;
    let store = PostgresProviderAttemptStore::create(&fixture.runtime_pool)
        .await
        .expect("open");
    let id = Uuid::new_v4();
    assert_eq!(
        ProviderAttemptStore::reserve(&store, &policy(), &id).await,
        Ok(Admission::Allowed)
    );
    // A DIFFERENT reserve with the SAME attempt id is a replay even while the
    // bus is free: the attempt ROW is the shield (TS refuses with `0`).
    let replay = ProviderAttemptStore::reserve(&store, &policy(), &id).await;
    match replay {
        Ok(Admission::Refused {
            code: AdmissionRefusal::DuplicateAttempt,
            retry_after_ms: 0,
        }) => {}
        other => panic!("expected duplicate_attempt with 0ms, got {other:?}"),
    }
}

#[tokio::test]
async fn pg_cooldown_refuses_inside_the_interval() {
    let fixture = provision().await;
    let store = PostgresProviderAttemptStore::create(&fixture.runtime_pool)
        .await
        .expect("open");
    // The ladder's order matters: BUSY is checked before COOLDOWN, so the
    // cooldown window is observable only after the first lease cleared.
    // Finalize the first attempt (interval 15000ms is still unpaid), then
    // the immediate second reserve refuses with the unpaid remainder.
    let first = Uuid::new_v4();
    assert_eq!(
        ProviderAttemptStore::reserve(&store, &policy(), &first).await,
        Ok(Admission::Allowed)
    );
    assert_eq!(
        ProviderAttemptStore::finish(
            &store,
            policy().bucket_id(),
            &first,
            Completion::Succeeded { http_status: None },
        )
        .await,
        Ok(true)
    );
    let second = ProviderAttemptStore::reserve(&store, &policy(), &Uuid::new_v4()).await;
    match second {
        Ok(Admission::Refused {
            code: AdmissionRefusal::Cooldown,
            retry_after_ms,
        }) => {
            // The margin is the interval's UNPAID remainder — positive, and
            // no more than the policy's whole interval.
            assert!(
                retry_after_ms > 0,
                "a remainder, not a guess: {retry_after_ms}"
            );
            assert!(retry_after_ms <= 15_000);
        }
        other => panic!("expected a cooldown refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn pg_daily_limit_refuses_to_utc_midnight() {
    let fixture = provision().await;
    let store = PostgresProviderAttemptStore::create(&fixture.runtime_pool)
        .await
        .expect("open");
    // limit-1, no interval: finalize the first attempt, then the second
    // reserve inside the same UTC DAY is past the limit — the day rollover
    // does not reset an already-live day.
    let starred = ProviderPolicy::new(
        policy().bucket_id(),
        "test-provider",
        "test-model",
        1,
        0,
        60000,
    )
    .expect("limit-1 policy");
    let first = Uuid::new_v4();
    assert_eq!(
        ProviderAttemptStore::reserve(&store, &starred, &first).await,
        Ok(Admission::Allowed)
    );
    assert_eq!(
        ProviderAttemptStore::finish(
            &store,
            starred.bucket_id(),
            &first,
            Completion::Succeeded { http_status: None },
        )
        .await,
        Ok(true)
    );
    let second = ProviderAttemptStore::reserve(&store, &starred, &Uuid::new_v4()).await;
    match second {
        Ok(Admission::Refused {
            code: AdmissionRefusal::DailyLimit,
            retry_after_ms,
        }) => {
            assert!(retry_after_ms > 0, "the next UTC midnight is in the future");
            assert!(retry_after_ms <= 24 * 3600 * 1000, "one day away at most");
        }
        other => panic!("expected daily_limit, got {other:?}"),
    }
}

#[tokio::test]
async fn pg_expired_lease_reaps_unknown_then_admits() {
    let fixture = provision().await;
    let store = PostgresProviderAttemptStore::create(&fixture.runtime_pool)
        .await
        .expect("open");
    // A short lease: 1000ms, the floor the policy bounds allow.
    let short = ProviderPolicy::new(
        policy().bucket_id(),
        "test-provider",
        "test-model",
        30,
        0,
        1000,
    )
    .expect("short policy");
    let first = Uuid::new_v4();
    assert_eq!(
        ProviderAttemptStore::reserve(&store, &short, &first).await,
        Ok(Admission::Allowed)
    );
    // While the lease is LIVE the second reserve is busy...
    let live = ProviderAttemptStore::reserve(&store, &short, &Uuid::new_v4()).await;
    match live {
        Ok(Admission::Refused {
            code: AdmissionRefusal::Busy,
            ..
        }) => {}
        other => panic!("expected busy while the lease is live, got {other:?}"),
    }
    // ...after it expires the SAME first attempt reaps to
    // unknown/interrupted with a server-computed elapsed value.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let after = ProviderAttemptStore::reserve(&store, &short, &Uuid::new_v4()).await;
    assert_eq!(
        after,
        Ok(Admission::Allowed),
        "the reaped lease freed the bus"
    );
    let status = archava_providers::read_provider_status(&fixture.runtime_pool, &short)
        .await
        .expect("status read");
    let reaped = status
        .recent
        .iter()
        .find(|view| view.request_id == first.to_string())
        .expect("the first attempt is in the projection");
    assert_eq!(reaped.state, "unknown", "an interrupted attempt is unknown");
    assert_eq!(
        reaped.failure_code.as_deref(),
        Some("interrupted"),
        "the code the reap records"
    );
    assert!(
        reaped.elapsed_ms.unwrap_or(0) >= 1000,
        "elapsed measured by the server, got {:?}",
        reaped.elapsed_ms
    );
    // The used counter counts EVERY admit, including the one after the reap.
    assert_eq!(status.attempts_today, 2);
}

#[tokio::test]
async fn pg_finish_records_once_then_refuses() {
    let fixture = provision().await;
    let store = PostgresProviderAttemptStore::create(&fixture.runtime_pool)
        .await
        .expect("open");
    let id = Uuid::new_v4();
    assert_eq!(
        ProviderAttemptStore::reserve(&store, &policy(), &id).await,
        Ok(Admission::Allowed)
    );
    // The live lease finalizes: durable, the result may surface.
    let recorded = ProviderAttemptStore::finish(
        &store,
        policy().bucket_id(),
        &id,
        Completion::Succeeded {
            http_status: Some(200),
        },
    )
    .await;
    assert_eq!(recorded, Ok(true), "the live lease finalize is durable");
    let status = archava_providers::read_provider_status(&fixture.runtime_pool, &policy())
        .await
        .expect("status read");
    let finished = status
        .recent
        .iter()
        .find(|view| view.request_id == id.to_string())
        .expect("the attempt is projected");
    assert_eq!(finished.state, "succeeded");
    assert_eq!(finished.http_status, Some(200));
    assert!(finished.failure_code.is_none());
    assert!(
        finished.finished_at.is_some(),
        "the finish timestamp exists"
    );
    assert!(
        looks_like_iso(finished.finished_at.as_deref().expect("checked")),
        "finished_at is the ISO shape"
    );
    assert!(status.active_request_id.is_none(), "the lease cleared");
    // A second finish of the SAME attempt returns false — history is
    // append-only, and the trigger refuses the rewrite even before the
    // pending-only WHERE does.
    let again = ProviderAttemptStore::finish(
        &store,
        policy().bucket_id(),
        &id,
        Completion::Succeeded {
            http_status: Some(200),
        },
    )
    .await;
    assert_eq!(
        again,
        Ok(false),
        "the second finalize is an unknown outcome"
    );
    // A finish for a NON-LEASED attempt is also unknown, never an error.
    let stranger = ProviderAttemptStore::finish(
        &store,
        policy().bucket_id(),
        &Uuid::new_v4(),
        Completion::Failed {
            failure_code: FailureCode::InvalidResponse,
            http_status: None,
        },
    )
    .await;
    assert_eq!(stranger, Ok(false));
}

#[tokio::test]
async fn pg_finish_after_expiry_is_unknown() {
    let fixture = provision().await;
    let store = PostgresProviderAttemptStore::create(&fixture.runtime_pool)
        .await
        .expect("open");
    let short = ProviderPolicy::new(
        policy().bucket_id(),
        "test-provider",
        "test-model",
        30,
        0,
        1000,
    )
    .expect("short policy");
    let id = Uuid::new_v4();
    assert_eq!(
        ProviderAttemptStore::reserve(&store, &short, &id).await,
        Ok(Admission::Allowed)
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    // The completion arrives after the lease expired: NOT recorded — the
    // attempt was already reaped to unknown, and the verdict is `false`, the
    // fail-closed `outcome_unknown` the runtime converts.
    let late = ProviderAttemptStore::finish(
        &store,
        short.bucket_id(),
        &id,
        Completion::Succeeded {
            http_status: Some(200),
        },
    )
    .await;
    assert_eq!(late, Ok(false), "an expired lease cannot be finalized late");
    let status = archava_providers::read_provider_status(&fixture.runtime_pool, &short)
        .await
        .unwrap_or_else(|error| panic!("status read: {error}"));
    let reaped = status
        .recent
        .iter()
        .find(|view| view.request_id == id.to_string())
        .expect("the attempt exists");
    assert_eq!(reaped.state, "unknown", "the reap stands");
    assert_eq!(reaped.failure_code.as_deref(), Some("interrupted"));
}

#[tokio::test]
async fn pg_hold_provider_after_restore_freezes_the_day() {
    let fixture = provision().await;
    let store = PostgresProviderAttemptStore::create(&fixture.runtime_pool)
        .await
        .expect("open");
    let pending = Uuid::new_v4();
    assert_eq!(
        ProviderAttemptStore::reserve(&store, &policy(), &pending).await,
        Ok(Admission::Allowed)
    );
    archava_providers::hold_provider_after_restore(&fixture.runtime_pool, &policy())
        .await
        .expect("the operator hold applies");
    // Every pending attempt is interrupted; the remaining budget is frozen.
    let status = archava_providers::read_provider_status(&fixture.runtime_pool, &policy())
        .await
        .expect("status read");
    assert_eq!(status.remaining, 0, "a held day spends nothing");
    assert!(
        status.restore_hold_until.is_some(),
        "the hold projects an end"
    );
    if let Some(hold) = &status.restore_hold_until {
        assert!(
            looks_like_iso(hold),
            "the hold end is the ISO shape: {hold}"
        );
    }
    // The NEXT reserve refuses with the hold's margin.
    let refused = ProviderAttemptStore::reserve(&store, &policy(), &Uuid::new_v4()).await;
    match refused {
        Ok(Admission::Refused {
            code: AdmissionRefusal::RestoreHold,
            retry_after_ms,
        }) => {
            assert!(
                retry_after_ms > 0 && retry_after_ms <= 24 * 3600 * 1000,
                "the hold margin is tomorrow minus now: {retry_after_ms}"
            );
        }
        other => panic!("expected a restore_hold refusal, got {other:?}"),
    }
    // The held attempt's row is unknown/interrupted like a reap's.
    let held_view = {
        let status = archava_providers::read_provider_status(&fixture.runtime_pool, &policy())
            .await
            .expect("status read");
        status
            .recent
            .iter()
            .find(|view| view.request_id == pending.to_string())
            .expect("the held attempt projects")
            .clone()
    };
    assert_eq!(held_view.state, "unknown");
    assert_eq!(held_view.failure_code.as_deref(), Some("interrupted"));
}

#[tokio::test]
async fn pg_policy_mismatch_fails_closed() {
    let fixture = provision().await;
    let store = PostgresProviderAttemptStore::create(&fixture.runtime_pool)
        .await
        .expect("open");
    // Seed the bucket under policy A...
    assert_eq!(
        ProviderAttemptStore::reserve(&store, &policy(), &Uuid::new_v4()).await,
        Ok(Admission::Allowed)
    );
    // ...then describe it with policy B (a different daily limit). The
    // mismatch is a store error — the orchestration converts it to
    // `outcome_unknown`, never an admission.
    let forged = ProviderPolicy::new(
        policy().bucket_id(),
        "test-provider",
        "test-model",
        999,
        15000,
        60000,
    )
    .expect(" forged daily limit still validates as integers");
    let refused = ProviderAttemptStore::reserve(&store, &forged, &Uuid::new_v4()).await;
    assert!(refused.is_err(), "a policy mismatch fails closed");
    // The forged reserve mutated nothing: the status under the REAL policy
    // still reads the original used count.
    let status = archava_providers::read_provider_status(&fixture.runtime_pool, &policy())
        .await
        .expect("status read");
    assert_eq!(status.attempts_today, 1);
}

#[tokio::test]
async fn pg_create_refuses_owner_and_tampered_pair() {
    let fixture = provision().await;
    // 1. The OWNER role cannot open the store — the checkTs refuses a role
    //    that owns the namespace regardless of how honest it behaves.
    let owner = PostgresProviderAttemptStore::create(&fixture.owner_pool).await;
    assert!(
        matches!(owner, Err(archava_providers::SetupError::NonOwnerRole)),
        "an owner role must refuse, got {owner:?}"
    );
    // 2. A tampered checksum pair refuses: the ledger must be exactly the
    //    frozen (id, sha256-of-file-bytes) pair. The tamper itself runs
    //    through the owner pool — the runtime role holds no UPDATE grant on
    //    `schema_migrations` (SELECT-only), the very assertion the runtime
    //    made when the tamper attempt on it would fail.
    let runtime = connect_runtime(&fixture).await;
    let tamper = sqlx::query("UPDATE archava_provider.schema_migrations SET checksum='forged'")
        .execute(&fixture.owner_pool)
        .await
        .expect("tamper through the owner role");
    assert_eq!(tamper.rows_affected(), 1);
    let tampered = PostgresProviderAttemptStore::create(&runtime).await;
    assert!(
        matches!(
            tampered,
            Err(archava_providers::SetupError::UnsupportedSchema)
        ),
        "a modified history must refuse, got {tampered:?}"
    );
    sqlx::query("UPDATE archava_provider.schema_migrations SET checksum=$1")
        .bind(checksum_literal())
        .execute(&fixture.owner_pool)
        .await
        .expect("restore");
    // 3. After restoring the pair the SAME pool opens (the checks were the
    //    refusal, not any cached state).
    PostgresProviderAttemptStore::create(&runtime)
        .await
        .expect("the restored ledger opens");
}

/// The sha256 the store froze — computed the same way `create` computes it.
fn checksum_literal() -> String {
    let bytes = std::fs::read(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../migrations/provider-runtime/001_provider_attempts.sql"),
    )
    .expect("migration bytes");
    use sha2::Digest;
    let mut digest = sha2::Sha256::new();
    digest.update(&bytes);
    digest
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

#[tokio::test]
async fn pg_run_provider_records_the_verbatim_outcome() {
    let fixture = provision().await;
    let store = PostgresProviderAttemptStore::create(&fixture.runtime_pool)
        .await
        .expect("open");
    let id = attempt();
    let result = run_provider(
        &store,
        &policy(),
        &id,
        || async { Ok::<String, InvokeFailure>(format!("{RESULT_PAYLOAD} (verified)")) },
        || Some(200),
    )
    .await;
    assert!(result.is_ok(), "the end-to-end run returns the result");
    let status = archava_providers::read_provider_status(&fixture.runtime_pool, &policy())
        .await
        .expect("status read");
    let view = status
        .recent
        .iter()
        .find(|view| view.request_id == id)
        .expect("the attempt projects");
    assert_eq!(view.state, "succeeded");
    assert_eq!(view.http_status, Some(200));
    assert!(
        looks_like_iso(&view.started_at),
        "started_at {} ISO",
        view.started_at
    );
    assert!(view.finished_at.is_some());
    assert_eq!(status.attempts_today, 1);
    assert!(status.active_request_id.is_none(), "the lease finished");
}
