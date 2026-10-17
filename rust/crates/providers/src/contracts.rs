//! The provider runtime's wire contract, transcribed from
//! `packages/provider-runtime/src/index.ts`.
//!
//! # The schemas, one by one
//!
//! `budgetIdSchema` — `^[a-z][a-z0-9_-]{0,79}$`:
//! `index.ts` line 4. A bucket id is also the RLS scope value
//! (`current_setting('archava.provider_bucket')`), so its charset is a
//! security property, not a style choice.
//!
//! `providerPolicySchema` — strict object; ids share the charset above with
//! dots allowed; `dailyLimit` 1..=1000, `minimumIntervalMs` 0..=60000,
//! `leaseMs` 1000..=120000 (`index.ts` lines 5–22). The limits ALSO live in
//! `001_provider_attempts.sql`'s CHECK constraints — two independent parsers,
//! so a Rust acceptance of a policy the SQL refuses fails closed at INSERT
//! time.
//!
//! `failureCodeSchema` — the ten fixed codes, in enum order
//! (`index.ts` lines 23–36).
//!
//! `completionSchema` — discriminated by `state`: `succeeded`/`failed`, and
//! every completion carries `httpStatus` (nullable, 100..=599) (`index.ts`
//! lines 37–55).
//!
//! `Admission` — `allowed: true`, or a refusal with one of five fixed codes
//! and a `retryAfterMs` (`index.ts` lines 56–66).
//!
//! The parse RESULTS must match the TS parser's accept/reject set; the error
//! prose may differ (they are not serialized).

/// A bucket id that passed validation.
///
/// Constructed only through [`ProviderPolicy::new`]/`validate_*`; carrying the
/// validated bytes forward instead of re-checking at every read keeps the RLS
/// setting write and the table joins honest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthoredPolicyField {
    pub(crate) value: String,
}

impl AuthoredPolicyField {
    pub fn as_str(&self) -> &str {
        &self.value
    }
}

impl core::fmt::Display for AuthoredPolicyField {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.value)
    }
}

/// What a provider invocation can name when it fails.
///
/// TS: `failureCodeSchema` (`index.ts` line 23–36) — an arbitrary Rust
/// error's `.code` may be `None`, which the runtime maps to
/// [`FailureCode::ProviderFailed`] exactly like the TS `safeParse` miss.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureCode {
    ProviderFailed,
    Timeout,
    ProviderUnavailable,
    InvalidResponse,
    RateLimited,
    PlanMismatch,
    SyntheticOnly,
    BudgetExhausted,
    Busy,
    Interrupted,
}

impl FailureCode {
    /// The wire spelling, identical to the TS union's literals in order.
    pub fn code(&self) -> &'static str {
        match self {
            FailureCode::ProviderFailed => "provider_failed",
            FailureCode::Timeout => "timeout",
            FailureCode::ProviderUnavailable => "provider_unavailable",
            FailureCode::InvalidResponse => "invalid_response",
            FailureCode::RateLimited => "rate_limited",
            FailureCode::PlanMismatch => "plan_mismatch",
            FailureCode::SyntheticOnly => "synthetic_only",
            FailureCode::BudgetExhausted => "budget_exhausted",
            FailureCode::Busy => "busy",
            FailureCode::Interrupted => "interrupted",
        }
    }

    /// The fixed order the SQL CHECK constraint spells, as a stable index.
    pub fn rank(&self) -> u8 {
        match self {
            FailureCode::ProviderFailed => 0,
            FailureCode::Timeout => 1,
            FailureCode::ProviderUnavailable => 2,
            FailureCode::InvalidResponse => 3,
            FailureCode::RateLimited => 4,
            FailureCode::PlanMismatch => 5,
            FailureCode::SyntheticOnly => 6,
            FailureCode::BudgetExhausted => 7,
            FailureCode::Busy => 8,
            FailureCode::Interrupted => 9,
        }
    }
}

impl core::str::FromStr for FailureCode {
    type Err = ParseFailureCodeError;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        match code {
            "provider_failed" => Ok(Self::ProviderFailed),
            "timeout" => Ok(Self::Timeout),
            "provider_unavailable" => Ok(Self::ProviderUnavailable),
            "invalid_response" => Ok(Self::InvalidResponse),
            "rate_limited" => Ok(Self::RateLimited),
            "plan_mismatch" => Ok(Self::PlanMismatch),
            "synthetic_only" => Ok(Self::SyntheticOnly),
            "budget_exhausted" => Ok(Self::BudgetExhausted),
            "busy" => Ok(Self::Busy),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(ParseFailureCodeError(())),
        }
    }
}

/// The error for a failure-code literal outside the closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseFailureCodeError(());

impl core::fmt::Display for ParseFailureCodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Invalid failure code")
    }
}

impl core::error::Error for ParseFailureCodeError {}

/// The refusal codes `Admission` may carry.
///
/// TS: `'daily_limit' | 'busy' | 'cooldown' | 'restore_hold' |
/// 'duplicate_attempt'` (`index.ts` lines 58–62).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionRefusal {
    DailyLimit,
    Busy,
    Cooldown,
    RestoreHold,
    DuplicateAttempt,
}

impl AdmissionRefusal {
    /// The wire spelling, identical to the TS union's literals in order.
    pub fn code(&self) -> &'static str {
        match self {
            AdmissionRefusal::DailyLimit => "daily_limit",
            AdmissionRefusal::Busy => "busy",
            AdmissionRefusal::Cooldown => "cooldown",
            AdmissionRefusal::RestoreHold => "restore_hold",
            AdmissionRefusal::DuplicateAttempt => "duplicate_attempt",
        }
    }
}

/// The invariant policy limits, split out so the SQL CHECK constraints and
/// the Rust validator are demonstrably the same numbers.
pub struct PolicyLimits;

impl PolicyLimits {
    pub const DAILY_LIMIT: std::ops::RangeInclusive<i32> = 1..=1000;
    pub const MINIMUM_INTERVAL_MS: std::ops::RangeInclusive<i32> = 0..=60000;
    pub const LEASE_MS: std::ops::RangeInclusive<i32> = 1000..=120000;
}

/// Alias kept for the lib re-export list: the constraints as one surface.
pub use PolicyLimits as DEFAULT_POLICY_LIMITS;

/// A validated provider policy.
///
/// TS: `providerPolicySchema` (`index.ts` lines 5–22, `.strict()`) — the
/// parse result contract is the accept/reject set; the prose of an error may
/// differ because this type is not serialized, and no caller can construct an
/// invalid one (`pub` fields are deliberately absent).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderPolicy {
    pub(crate) bucket_id: AuthoredPolicyField,
    pub(crate) provider_id: AuthoredPolicyField,
    pub(crate) model: AuthoredPolicyField,
    pub(crate) daily_limit: i32,
    pub(crate) minimum_interval_ms: i32,
    pub(crate) lease_ms: i32,
}

impl ProviderPolicy {
    /// Validate and construct, or name the first field that refused.
    pub fn new(
        bucket_id: &str,
        provider_id: &str,
        model: &str,
        daily_limit: i32,
        minimum_interval_ms: i32,
        lease_ms: i32,
    ) -> Result<Self, PolicyError> {
        let bucket_id = validate_budget_id(bucket_id)?;
        let provider_id =
            validate_provider_or_model_id(provider_id).map_err(|()| PolicyError::ProviderId)?;
        let model = validate_provider_or_model_id(model).map_err(|()| PolicyError::Model)?;
        if !PolicyLimits::DAILY_LIMIT.contains(&daily_limit) {
            return Err(PolicyError::DailyLimit);
        }
        if !PolicyLimits::MINIMUM_INTERVAL_MS.contains(&minimum_interval_ms) {
            return Err(PolicyError::MinimumIntervalMs);
        }
        if !PolicyLimits::LEASE_MS.contains(&lease_ms) {
            return Err(PolicyError::LeaseMs);
        }
        Ok(Self {
            bucket_id,
            provider_id,
            model,
            daily_limit,
            minimum_interval_ms,
            lease_ms,
        })
    }

    pub fn bucket_id(&self) -> &str {
        &self.bucket_id.value
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id.value
    }

    pub fn model(&self) -> &str {
        &self.model.value
    }

    pub fn daily_limit(&self) -> i32 {
        self.daily_limit
    }

    pub fn minimum_interval_ms(&self) -> i32 {
        self.minimum_interval_ms
    }

    pub fn lease_ms(&self) -> i32 {
        self.lease_ms
    }
}

/// The policy field that refused validation, mirroring the TS strict
/// schema's per-field failure surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyError {
    BucketId,
    ProviderId,
    Model,
    DailyLimit,
    MinimumIntervalMs,
    LeaseMs,
}

impl core::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let field = match self {
            PolicyError::BucketId => "bucketId",
            PolicyError::ProviderId => "providerId",
            PolicyError::Model => "model",
            PolicyError::DailyLimit => "dailyLimit",
            PolicyError::MinimumIntervalMs => "minimumIntervalMs",
            PolicyError::LeaseMs => "leaseMs",
        };
        write!(f, "Invalid policy field: {field}")
    }
}

impl core::error::Error for PolicyError {}

/// `^[a-z][a-z0-9_-]{0,79}$`, byte-checked without a regex dependency —
/// the whole charset is ASCII, so a hand loop is exact.
pub fn validate_budget_id(value: &str) -> Result<AuthoredPolicyField, PolicyError> {
    let bytes = value.as_bytes();
    let head = bytes.first().copied();
    if !head.is_some_and(|b| b.is_ascii_lowercase()) {
        return Err(PolicyError::BucketId);
    }
    let rest_ok = bytes[1..]
        .iter()
        .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if !rest_ok || value.len() > 80 {
        return Err(PolicyError::BucketId);
    }
    Ok(AuthoredPolicyField {
        value: value.to_string(),
    })
}

/// The shared charset for `providerId` and `model`:
/// `^[a-z][a-z0-9_.-]{0,79}$` (`index.ts` lines 6–7/10–12).
fn validate_provider_or_model_id(value: &str) -> Result<AuthoredPolicyField, ()> {
    let bytes = value.as_bytes();
    let head = bytes.first().copied();
    if !head.is_some_and(|b| b.is_ascii_lowercase()) {
        return Err(());
    }
    let rest_ok = bytes[1..].iter().all(|&b| {
        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.' || b == b'-'
    });
    if !rest_ok || value.len() > 80 {
        return Err(());
    }
    Ok(AuthoredPolicyField {
        value: value.to_string(),
    })
}

/// A provider invocation's failure, as the TS `invoke` error's `.code`.
///
/// The runtime never lets the error's own message/display reach the ledger —
/// only this optional closed code survives (`index.ts` lines 111–120).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvokeError {
    pub failure_code: Option<FailureCode>,
}

impl InvokeError {
    pub fn new(failure_code: Option<FailureCode>) -> Self {
        Self { failure_code }
    }
}

/// The authoritative outcome recorded for one attempt.
///
/// TS: `completionSchema` (`index.ts` lines 37–55). A completion is the ONLY
/// result ever written under the pattern this module keeps — no payload,
/// no identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    Succeeded {
        http_status: Option<u16>,
    },
    Failed {
        failure_code: FailureCode,
        http_status: Option<u16>,
    },
}

impl Completion {
    /// The TS `state` discriminator and its per-arm fields, as SQL values:
    /// `(state, failure_code, http_status)`.
    ///
    /// The HTTP status is bounded HERE, at the single point status values
    /// enter SQL — the column's own CHECK constraint is the second parser
    /// (`001_provider_attempts.sql` bounds `http_status` to 100..=599 the
    /// same way), and every caller funnels through this conversion, so an
    /// out-of-window status records as `null` (the TS safeParse miss) rather
    /// than reaching the table.
    pub fn as_sql_args(&self) -> (&'static str, Option<&'static str>, Option<i32>) {
        let bounded = |status: Option<u16>| match status {
            Some(status) => validate_http_status(status),
            None => None,
        };
        match self {
            Completion::Succeeded { http_status } => ("succeeded", None, bounded(*http_status)),
            Completion::Failed {
                failure_code,
                http_status,
            } => ("failed", Some(failure_code.code()), bounded(*http_status)),
        }
    }
}

/// The reserve decision.
///
/// TS: `Admission` (`index.ts` lines 56–66) — every refusal names its
/// `retryAfterMs`, computed by the ledger, never guessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Allowed,
    Refused {
        code: AdmissionRefusal,
        retry_after_ms: i64,
    },
}

/// An `$`-style interval/status check helper: 100..=599.
///
/// TS: the completion parser bounds every `httpStatus` to 100–599 both at
/// the safeParse and at the column's CHECK constraint.
pub(crate) fn validate_http_status(value: u16) -> Option<i32> {
    if (100..=599).contains(&value) {
        Some(i32::from(value))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole accept/reject set of `budgetIdSchema` matters: the bucket id
    /// is the RLS scope value, so a charset leak is an injection surface.
    #[test]
    fn budget_id_follows_the_frozen_charset() {
        for (value, ok) in [
            ("a", true),
            ("test-budget", true),
            ("a1", true),
            ("a_b-c", true),
            ("", false),
            ("Upper", false),
            ("1abc", false),
            ("-abc", false),
            ("a b", false),
            ("a/b", false),
            ("é", false),
            ("a…", false),
        ] {
            assert_eq!(
                validate_budget_id(value).is_ok(),
                ok,
                "budget id {value:?} should be {ok}"
            );
        }
        let long_ok = format!("a{}", "x".repeat(79));
        assert!(validate_budget_id(&long_ok).is_ok(), "80 chars ok");
        let long_bad = format!("a{}", "x".repeat(80));
        assert!(validate_budget_id(&long_bad).is_err(), "81 chars refused");
    }

    /// `providerPolicySchema`'s numeric bounds are the SQL CHECK doubles:
    /// construct-at-RNG boundaries on each side of each edge.
    #[test]
    fn policy_bounds_fall_on_the_frozen_edges() {
        let base = ("test-budget", "p", "m", 30, 15000, 60000);
        let ok = |t: (&str, &str, &str, i32, i32, i32)| {
            ProviderPolicy::new(t.0, t.1, t.2, t.3, t.4, t.5).is_ok()
        };
        assert!(ok(("a", "b", "c", 1, 0, 1000)));
        assert!(ok(("a", "b", "c", 1000, 60000, 120000)));
        assert!(!ok(("a", "b", "c", 0, 0, 1000)));
        assert!(!ok(("a", "b", "c", 1001, 0, 1000)));
        assert!(!ok(("a", "b", "c", 30, -1, 1000)));
        assert!(!ok(("a", "b", "c", 30, 60001, 1000)));
        assert!(!ok(("a", "b", "c", 30, 0, 999)));
        assert!(!ok(("a", "b", "c", 30, 0, 120001)));
        assert!(
            PolicyError::DailyLimit
                == ProviderPolicy::new(base.0, base.1, base.2, 0, 0, 1000).unwrap_err()
        );
        assert!(
            PolicyError::LeaseMs
                == ProviderPolicy::new(base.0, base.1, base.2, 30, 0, 999).unwrap_err()
        );
    }

    /// The failure-code union is closed — an unknown string parses to an
    /// error, every spelled code parses to itself and its wire spelling
    /// round-trips.
    #[test]
    fn failure_codes_round_trip_in_the_fixed_order() {
        let spellings = [
            "provider_failed",
            "timeout",
            "provider_unavailable",
            "invalid_response",
            "rate_limited",
            "plan_mismatch",
            "synthetic_only",
            "budget_exhausted",
            "busy",
            "interrupted",
        ];
        for (rank, spelling) in spellings.iter().enumerate() {
            let code: FailureCode = spelling.parse().expect("a spelled code parses");
            assert_eq!(code.rank(), rank as u8, "rank follows the SQL CHECK order");
            assert_eq!(code.code(), *spelling, "wire spelling round-trips");
        }
        assert!("failure".parse::<FailureCode>().is_err());
        assert!("".parse::<FailureCode>().is_err());
    }

    /// HTTP statuses are stored only inside 100..=599, mirror of the column
    /// CHECK; the conversion is used by `Completion::as_sql_args`.
    #[test]
    fn http_status_bounds_match_the_column_constraint() {
        assert_eq!(validate_http_status(99), None);
        assert_eq!(validate_http_status(100), Some(100));
        assert_eq!(validate_http_status(599), Some(599));
        assert!(validate_http_status(600).is_none());
    }
}
