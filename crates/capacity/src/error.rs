//! `CapacityError` — the one failure the scheduling math can report.
//!
//! The baseline's error type is a class carrying a `code` from an eight-member
//! union and an optional message (`packages/capacity/src/contracts.ts:3-19`):
//!
//! ```ts
//! export class CapacityError extends Error {
//!   constructor(
//!     public readonly code:
//!       | 'forbidden' | 'not_found' | 'conflict' | 'invalid_request'
//!       | 'unauthorized' | 'pool_archived' | 'invalid_state'
//!       | 'insufficient_capacity',
//!     message?: string,
//!   ) {
//!     super(message ?? code)
//!     this.name = 'CapacityError'
//!   }
//! }
//! ```
//!
//! `calculateSchedule` reaches exactly one of those eight. It throws
//! `new CapacityError('insufficient_capacity')` twice — once at the entry
//! (`repository.ts:124`, when the pool's weekly capacity does not divide into a
//! positive daily burn) and once at the exit (`repository.ts:209`, when the
//! 1000-iteration loop gives up). Both are constructed with **no message
//! argument**, so `super(message ?? code)` makes the message the code itself.
//!
//! So this port models one variant rather than eight, and states the reason
//! the other seven are absent rather than leaving the omission to be inferred:
//! they are thrown by the repository, service and API layers
//! (`repository.ts:70,297,319,340,405,436,440,477,501`, `service.ts:44,53`,
//! `tools.ts:29`), none of which is scheduling math. Carrying unreachable
//! variants would be a statement this crate cannot back — an enum member that
//! no code path constructs is a claim the port decided something it did not
//! decide, and it would be dead weight the moment someone read it as
//! "the other seven are the service layer's". When the port grows a caller for
//! those paths, the variants arrive with the code that throws them.

use std::fmt;

/// Every failure this crate can report.
///
/// A two-variant enum rather than a unit struct, deliberately: the baseline's
/// failure mode is *coded*, and when this port is promoted the variants that
/// arrive must join a shape already built to carry a code, not be retrofitted
/// into a message-only error. [`CapacityErrorKind::code`] is where the
/// wire-visible string lives, so a future variant's string is one match arm
/// away rather than a rewrite of `Display`.
///
/// [`InvalidRequest`] is the port's own addition at the scheduling boundary: the
/// baseline's code union already names `invalid_request` (error.rs:10) but
/// `calculateSchedule` itself validates nothing, so NaN / Infinity / non-positive
/// values flow straight into the search and either produce garbage or burn the
/// 1000-iteration loop without progress. This port refuses them at the entry
/// instead. [`InsufficientCapacity`] remains the only code `calculateSchedule`
/// itself produces — a genuinely unsatisfiable schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapacityErrorKind {
    /// No schedule exists that satisfies the request.
    ///
    /// Reached from two places, and they mean different things to an operator
    /// even though they report the same code:
    ///
    /// * a pool whose `weeklyCapacity / 5` is not greater than zero. A pool
    ///   that cannot burn anything per day can never finish a job, so there is
    ///   no schedule to search for. `maxConcurrentJobs: 0` reaches this too —
    ///   not through the burn rate but because `concurrentJobsToday + 1 > 0` is
    ///   true on the first candidate day, so the loop refuses every option and
    ///   exhausts itself.
    /// * the search ran 1000 candidate starts and found none valid. The
    ///   `unsatisfiable-concurrency` fixture above returns from here.
    ///
    /// The baseline does not distinguish them and neither does this port: the
    /// `insufficient_capacity` code is the whole contract, and a caller that
    /// needs the difference must add it to the baseline first.
    InsufficientCapacity,

    /// The request itself is malformed: a non-finite (NaN / Infinity) rate or
    /// unit count, a non-positive burn rate, zero concurrency, or non-positive
    /// units. The baseline schema rejects some of these at its boundary
    /// (`z.number().positive()`) but `calculateSchedule` itself validates
    /// nothing, so this port guards the entry before any arithmetic runs.
    InvalidRequest,
}

impl CapacityErrorKind {
    /// The code as the baseline spells it, for a caller matching on the string
    /// the TypeScript throws.
    pub const fn code(self) -> &'static str {
        match self {
            CapacityErrorKind::InsufficientCapacity => "insufficient_capacity",
            CapacityErrorKind::InvalidRequest => "invalid_request",
        }
    }
}

/// The error returned by [`calculate_schedule`](crate::calculate_schedule).
///
/// Serializes to the baseline's shape — a `Display` of `"insufficient_capacity"`
/// is what `new CapacityError('insufficient_capacity').message` evaluates to,
/// so a caller logging this error sees what a caller logging the TypeScript one
/// sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CapacityError {
    /// Which refusal this is. A struct field rather than the bare kind so
    /// adding a message later does not change the type's arity at every call
    /// site.
    pub kind: CapacityErrorKind,
}

impl CapacityError {
    /// The refusal `calculateSchedule` throws for an unsatisfiable request.
    pub const fn insufficient_capacity() -> Self {
        Self {
            kind: CapacityErrorKind::InsufficientCapacity,
        }
    }

    /// The port's refusal for a malformed request — NaN / Infinity input,
    /// non-positive burn rate, zero concurrency, or non-positive units.
    pub const fn invalid_request() -> Self {
        Self {
            kind: CapacityErrorKind::InvalidRequest,
        }
    }

    /// The baseline's `code`, as this port spells it.
    pub const fn code(self) -> &'static str {
        self.kind.code()
    }
}

impl fmt::Display for CapacityError {
    /// The message, which for this variant *is* the code — `super(message ??
    /// code)` with no message supplied.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind.code())
    }
}

impl std::error::Error for CapacityError {}

/// A calendar refusal is a malformed request from the scheduling layer's point
/// of view.
///
/// RA07 routes the calendar primitives' controlled errors
/// ([`CalendarError`](crate::calendar::CalendarError)) into the capacity error
/// channel so [`calculate_schedule`](crate::calculate_schedule) can propagate
/// them with `?` without inventing a ninth baseline code. An out-of-domain
/// timestamp, an unrepresentable day-step, or a span past the horizon are all
/// "the request itself is malformed", which is exactly what
/// [`CapacityErrorKind::InvalidRequest`] means — the baseline's
/// `invalid_request` code, reached through a guard the baseline does not have.
impl From<crate::calendar::CalendarError> for CapacityError {
    fn from(_: crate::calendar::CalendarError) -> Self {
        CapacityError::invalid_request()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The message the baseline produces when no message is passed, which is
    /// what both `calculateSchedule` throw sites do.
    #[test]
    fn display_is_the_code_when_no_message_is_supplied() {
        let error = CapacityError::insufficient_capacity();
        assert_eq!(error.to_string(), "insufficient_capacity");
        assert_eq!(error.code(), "insufficient_capacity");
    }

    /// `std::error::Error` is implemented, so this composes with `?` and
    /// `Box<dyn Error>` the way any other library error does.
    #[test]
    fn is_a_std_error() {
        let error: Box<dyn std::error::Error> = Box::new(CapacityError::insufficient_capacity());
        assert_eq!(error.to_string(), "insufficient_capacity");
    }
}
