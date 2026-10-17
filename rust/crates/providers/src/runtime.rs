//! The fail-closed provider run orchestration — the port of
//! `packages/provider-runtime/src/index.ts`'s `runProvider`.
//!
//! The three contracts the baseline pins (`packages/provider-runtime/test/
//! runtime.test.ts`, one witness each, ported in `tests/provider_runtime.rs`):
//!
//! 1. **Refusals happen before the invoke, without a finish.** An admission
//!    refusal only exists because the LEDGER said no; calling the provider
//!    anyway would bypass the budget the admission is for.
//! 2. **The result is returned only after a durable completion.** `finish`
//!    returning `false` (this attempt is not the bucket's active lease —
//!    expired, reaped, already finished) converts a provider SUCCESS into an
//!    `outcome_unknown`: the model output is not released, because the
//!    ledger, not the caller, decides which invocation was real.
//! 3. **Store failures convert to `outcome_unknown`, fail closed.** A
//!    reservation that could not be durably recorded must not invoke (a
//!    budget double-spend is a silent cost; a lost budget is a loud error),
//!    and a completion that could not be recorded must not release the
//!    result.
//!
//! And the privacy boundary: no prompt, result, identity or credential is
//! ever passed to the store — the parameters carry the policy's ids and
//! limits, the outcome's status enum, and nothing else.

use core::future::Future;

use sqlx::types::Uuid;

use crate::contracts::{Admission, Completion, FailureCode, InvokeError, ProviderPolicy};

/// A store that could not answer durably.
///
/// The TS layer has no distinct type (`store.reserve` throwing propagates to
/// `runProvider`'s catch and becomes `outcome_unknown`); the Rust trait makes
/// it explicit so the scripted stores in the tests can model each failure
/// mode at the right layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Provider attempt store could not answer durably")]
pub struct StoreError;

/// The store surface `runProvider` needs — `reserve` then `finish`, nothing
/// else. The Postgres store implements it; the tests' scripted stores
/// implement the same two methods. Object safety is not a goal: the runtime
/// is generic, because a `dyn` dispatch boundary here would suggest the store
/// is swappable at runtime rather than composed once.
pub trait ProviderAttemptStore: Send + Sync {
    /// Reserve one attempt's slot in the budget. Refusals carry the ledger's
    /// own retry margin.
    ///
    /// TS: `reserve(policy, attemptId): Promise<Admission>` (`index.ts`
    /// lines 71–75).
    ///
    /// The futures carry explicit `Send` bounds (the trait is implemented by
    /// a pool-backed store whose futures cross task boundaries in the host,
    /// and clippy's desugaring note is recorded here): a scripted store for
    /// the witnesses implements the same two methods.
    fn reserve(
        &self,
        policy: &ProviderPolicy,
        attempt_id: &Uuid,
    ) -> impl std::future::Future<Output = Result<Admission, StoreError>> + Send;

    /// Finalize the authoritative outcome. `Ok(false)` means this attempt is
    /// NOT the bucket's active lease — an unknown outcome, fail closed.
    ///
    /// TS: `finish(bucketId, attemptId, completion): Promise<boolean>`
    /// (`index.ts` lines 72–75).
    fn finish(
        &self,
        bucket_id: &str,
        attempt_id: &Uuid,
        completion: Completion,
    ) -> impl std::future::Future<Output = Result<bool, StoreError>> + Send;
}

/// How a run can fail — the baseline's two error classes flattened into one
/// enum whose variants keep the two classes' fixed messages.
///
/// TS: `ProviderAdmissionError` (a ledger refusal, retry advice attached) and
/// `ProviderRunError` (`provider_failed`: the invoke failed and was recorded;
/// `outcome_unknown`: the ledger could not prove which outcome was
/// authoritative, so everything fails closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProviderRunFailure {
    /// The refusal the ledger itself issued.
    /// TS: `ProviderAdmissionError(code, retryAfterMs)`.
    #[error("Provider request is temporarily limited.")]
    Admission {
        code: crate::contracts::AdmissionRefusal,
        retry_after_ms: i64,
    },
    /// The invoke failed AND the failure was durably recorded under an
    /// allowed closed code. TS: `ProviderRunError('provider_failed')`.
    #[error("Provider result could not be verified.")]
    ProviderFailed,
    /// The ledger could not prove which outcome was authoritative — the
    /// reservation or the completion was not durable. TS:
    /// `ProviderRunError('outcome_unknown')`.
    #[error("Provider result could not be verified.")]
    OutcomeUnknown,
}

impl ProviderRunFailure {
    /// The boundary-visible code — the admission refusal's code, or the two
    /// run-error codes. No other information is carried: a caller cannot
    /// recover the masked invoke error's contents from this type, matching
    /// the TS error classes' fixed messages.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            ProviderRunFailure::Admission { code, .. } => Some(code.code()),
            ProviderRunFailure::ProviderFailed => Some("provider_failed"),
            ProviderRunFailure::OutcomeUnknown => Some("outcome_unknown"),
        }
    }

    /// The retry margin (admission refusals only; the other two outcomes are
    /// not retry advice).
    pub fn retry_after_ms(&self) -> Option<i64> {
        match self {
            ProviderRunFailure::Admission { retry_after_ms, .. } => Some(*retry_after_ms),
            _ => None,
        }
    }
}

/// Reserve → invoke → finalize, the whole orchestration.
///
/// TS: `runProvider` (`index.ts` lines 89–128). Every step below is the
/// baseline's order, no reordering:
///
/// 1. Validate the attempt id — BEFORE any store call, so a forged field
///    never reaches the ledger and never spends a reserve.
/// 2. Reserve. A refusal: named error, no invoke, no finish. A store
///    failure: `OutcomeUnknown`, no invoke.
/// 3. Invoke. An error: record `failed` under the invoke's optional closed
///    code (a missing code records `provider_failed`) with the status
///    getter's bounded value; if that recording is not durable,
///    `OutcomeUnknown` — never re-invoke, never release the error.
/// 4. Invoke success: record `succeeded` with the status getter's bounded
///    value; if that recording is not durable (including `finish` →
///    `false`), the result is NOT returned — `OutcomeUnknown`.
/// 5. Return the result only after a durable success.
///
/// The masked error's own payload (`Err: Into<InvokeError>`) is the caller's
/// place to keep whatever they want; the ledger sees the closed code only.
pub async fn run_provider<T, Err, S, Fut>(
    store: &S,
    policy: &ProviderPolicy,
    attempt_id: &str,
    invoke: impl FnOnce() -> Fut,
    http_status: impl Fn() -> Option<u16>,
) -> Result<T, ProviderRunFailure>
where
    S: ProviderAttemptStore,
    Fut: Future<Output = Result<T, Err>>,
    Err: Into<InvokeError>,
{
    // Step 1: the id parses strictly before the store sees anything (TS:
    // `z.uuid().parse(options.attemptId)` — and its `forge` witness expects
    // the refusal to happen before `store.reserve` is called).
    let attempt_uuid =
        validate_attempt_id(attempt_id).map_err(|_| ProviderRunFailure::OutcomeUnknown)?;
    // The policy was validated at construction (`ProviderPolicy::new`); the
    // runtime re-derives nothing, because a policy exists only in its valid
    // form.
    let bound_status = validate_status(http_status());
    // Step 2: reserve.
    let admission = match store.reserve(policy, &attempt_uuid).await {
        Ok(admission) => admission,
        // The reservation is not durable famous truth: do NOT invoke.
        Err(_) => return Err(ProviderRunFailure::OutcomeUnknown),
    };
    match admission {
        Admission::Refused {
            code,
            retry_after_ms,
        } => {
            return Err(ProviderRunFailure::Admission {
                code,
                retry_after_ms,
            });
        }
        Admission::Allowed => {}
    }
    // Step 3: the invoke happens ONLY after a durable reservation.
    let result = invoke().await;
    match result {
        Ok(value) => {
            // Step 4: a success is returned ONLY after durable completion.
            let recorded = store
                .finish(
                    policy.bucket_id(),
                    &attempt_uuid,
                    Completion::Succeeded {
                        http_status: bound_status,
                    },
                )
                .await;
            match recorded {
                Ok(true) => Ok(value),
                Ok(false) | Err(_) => Err(ProviderRunFailure::OutcomeUnknown),
            }
        }
        Err(err) => {
            // The failure is masked: the ledger sees the closed code only,
            // and the caller receives exactly `ProviderFailed` — not the
            // invoke error's contents.
            let into: InvokeError = err.into();
            let recorded = store
                .finish(
                    policy.bucket_id(),
                    &attempt_uuid,
                    Completion::Failed {
                        failure_code: into.failure_code.unwrap_or(FailureCode::ProviderFailed),
                        http_status: bound_status,
                    },
                )
                .await;
            match recorded {
                Ok(true) => Err(ProviderRunFailure::ProviderFailed),
                Ok(false) | Err(_) => Err(ProviderRunFailure::OutcomeUnknown),
            }
        }
    }
    // NO unreachable code path returns a result without a durable completion:
    // both Ok branches above are the only `Ok(_)` exits, and both align with
    // the two error leaves' conditions.
}

/// The status getter's value, bounded the way the TS `safeParse` bounds it:
/// anything outside 100..=599 records `null`, never a fabricated status.
fn validate_status(value: Option<u16>) -> Option<u16> {
    match value {
        Some(status) if (100..=599).contains(&status) => Some(status),
        _ => None,
    }
}

/// `z.uuid()` parity: the strict 8-4-4-4-12 hex form, case-insensitive, no
/// braced/URN shapes. `zod`'s `z.uuid()` accepts exactly this form and the
/// `attempts.attempt_id` column is `uuid`. Returns the typed value for binds.
fn validate_attempt_id(attempt_id: &str) -> Result<Uuid, ()> {
    const GROUP_LENS: [usize; 5] = [8, 4, 4, 4, 12];
    if !attempt_id.is_ascii() {
        return Err(());
    }
    let groups: Vec<&[u8]> = attempt_id.as_bytes().split(|&b| b == b'-').collect();
    if groups.len() != 5 {
        return Err(());
    }
    for (group, expected) in groups.iter().zip(GROUP_LENS) {
        if group.len() != expected || !group.iter().all(|&b| b.is_ascii_hexdigit()) {
            return Err(());
        }
    }
    Uuid::parse_str(attempt_id).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The attempt id parse is the strict 8-4-4-4-12 hex form — the same
    /// accept/reject set as `z.uuid()`: hyphenated hex only.
    #[test]
    fn attempt_id_parses_strictly() {
        let valid = "123e4567-e89b-12d3-a456-426614174000";
        assert!(validate_attempt_id(valid).is_ok());
        let upper = "123E4567-E89B-12D3-A456-426614174000";
        assert!(validate_attempt_id(upper).is_ok(), "hex case-insensitive");
        for malformed in [
            "",
            "forged",
            "123e4567-e89b-12d3-a456-42661417400",
            "123e4567-e89b-12d3-a456-4266141740000",
            "123e4567_e89b-12d3-a456-426614174000",
            "123e4567-e89b-12d3-a456-426614174000-extra",
            "123g4567-e89b-12d3-a456-426614174000",
            "{123e4567-e89b-12d3-a456-426614174000}",
        ] {
            assert!(
                validate_attempt_id(malformed).is_err(),
                "{malformed:?} should not parse"
            );
        }
    }
}
