//! The three calendar primitives the baseline actually uses.
//!
//! Everything here is a direct translation of three functions from
//! `packages/capacity-pg/src/repository.ts`:
//!
//! | baseline | port | line |
//! |---|---|---|
//! | `new Date(ms)` + `getUTCDay()` | [`weekday`] | `:100`, `:110`, `:160` |
//! | `cur.getTime() + 24 * 60 * 60 * 1000` | [`MS_PER_DAY`] | `:103`, `:111`, `:163`, `:172` |
//! | `nextBusinessDay` | [`next_business_day`] | `:108-115` |
//!
//! ## Why milliseconds, and why signed
//!
//! JavaScript's `Date` is a signed 64-bit millisecond count from the Unix
//! epoch. The baseline reads it with `getTime()`, compares the integers, and
//! steps them by whole days. So this port does exactly the same:
//! [`Instant`] is an `i64` millisecond count, and every primitive below is
//! integer arithmetic on it.
//!
//! It is **signed** for a reason, not a default. The baseline accepts a
//! `desiredStartAt` from `instantSchema`, which is `z.iso.datetime({ offset:
//! true })` — a real calendar date that may predate 1970. The
//! `allocation-in-the-past` fixture in the differential suite (2020-01-06) and
//! any pre-epoch input both produce negative `getTime()` values, and a
//! comparison like `a.start < dayEnd` must order them correctly against a
//! positive candidate start. An unsigned type would wrap every past timestamp
//! into the far future and make a past allocation look like a 290-million-year
//! blocker, which the loop would then chase for 1000 iterations and refuse.
//!
//! ## The one place the baseline's arithmetic is not the arithmetic it looks like
//!
//! [`next_business_day`] contains the subtlety of this whole port, and it is a
//! single line that is easy to "clean up" into a bug:
//!
//! ```js
//! cur = new Date(cur.getTime() + 24 * 60 * 60 * 1000)
//! cur.setUTCHours(0, 0, 0, 0)   // <-- only inside the weekend loop
//! ```
//!
//! `setUTCHours(0,0,0,0)` runs **only when a weekend was actually skipped**. A
//! caller that passes a weekday date with a time-of-day — `07:00`, `13:45:30.500`
//! — gets that *time of day preserved*, not normalized to midnight. Two of the
//! differential suite's fixtures depend on it:
//!
//! * `offset-tz-negative` (`2030-01-07T00:00:00-05:00`) normalizes to
//!   `2030-01-07T05:00:00.000Z`, a Monday with a 05:00 time-of-day, and the
//!   returned start and finish both carry `05:00`.
//! * `timestamp-with-time-of-day` (a Tuesday at `13:45:30.500`) likewise keeps
//!   its time on both endpoints.
//!
//! So a port that "helpfully" zeroed the time on every call would produce
//! `2030-01-07T00:00:00.000Z` where the baseline produces
//! `2030-01-07T05:00:00.000Z`. Same weekday, same working-day count, different
//! `scheduledStartAt` — and the capacity verifier asserts the *exact string*
//! (`tools/library/verify-capacity.mjs:583-585`). [`next_business_day`] carries
//! this branch verbatim, and `keeps_the_time_of_day_when_no_weekend_is_crossed`
//! pins it.
//!
//! ## Why the day step is fixed, not calendar-aware
//!
//! `countWorkingDays` and the candidate-day walker both advance by exactly
//! 86,400,000 ms. On the UTC timeline that is a day with no exception, so the
//! arithmetic is exact — but only because every timestamp the baseline accepts
//! is interpreted in UTC. The stepping is deliberately *not* calendar-aware:
//! were it, a DST-observing calendar would disagree with the baseline's naive
//! stepping for any local-zone input, which is precisely the divergence this
//! crate's `Cargo.toml` refuses to take on.

use serde::{Deserialize, Serialize};

/// Milliseconds in a day, spelled as the baseline spells it.
///
/// `24 * 60 * 60 * 1000` in the TypeScript, written here in the same
/// left-to-right order so the two can be compared character by character.
pub const MS_PER_DAY: i64 = 24 * 60 * 60 * 1000;

/// The lower bound of the supported instant domain: JavaScript's `Date` range.
///
/// `new Date(-100_000_000 * 86_400_000)` is the earliest instant a JS `Date`
/// can represent (`-271821-04-20T00:00:00Z`). This port is a faithful
/// transcription of `Date` arithmetic, so its supported domain is the same one:
/// an instant outside it is not a value the baseline could ever have produced,
/// and calendar primitives refuse it rather than guessing.
pub const MIN_SUPPORTED_MILLIS: i64 = -8_640_000_000_000_000;

/// The upper bound of the supported instant domain: JavaScript's `Date` range.
///
/// `new Date(100_000_000 * 86_400_000)` (`275760-09-13T00:00:00Z`). See
/// [`MIN_SUPPORTED_MILLIS`].
pub const MAX_SUPPORTED_MILLIS: i64 = 8_640_000_000_000_000;

/// The largest span any day-walking primitive will iterate over.
///
/// 365,000 days is 1,000 years — a thousand times the widest allocation in the
/// differential fixtures (a 10-year blocker) and far beyond any real scheduling
/// horizon. It is the termination guarantee the mandate requires: a walk over a
/// bounded span is finite by construction, and a span beyond this bound is a
/// malformed request that is refused with a controlled error rather than a loop
/// that spins for `~10^8` iterations. [`count_working_days`] checks it before it
/// steps; [`calculate_schedule`](crate::calculate_schedule) inherits it through
/// [`count_working_days`] and its own 1000-iteration cap.
pub const MAX_HORIZON_DAYS: i64 = 365_000;

/// Everything that can go wrong in calendar arithmetic.
///
/// RA07's subject. The baseline's `Date` arithmetic has no error channel — a
/// `+` that overflows a JS number silently becomes `Infinity` and the loop
/// comparing against it simply stops — but the port models instants as `i64`
/// milliseconds, where a checked add can genuinely overflow, and the previous
/// `plus_days` answered that overflow by returning the *unchanged* instant,
/// which made [`count_working_days`]'s `while current < finish` loop spin
/// forever (`count_working_days(i64::MAX - 1, i64::MAX)` never advanced). Each
/// failure below is therefore a controlled refusal, never a stale value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CalendarError {
    /// A millisecond count outside the supported [`Instant`] domain.
    TimestampOutOfRange { millis: i64 },
    /// A day-step whose result is not representable as an `i64` count of
    /// milliseconds.
    Overflow,
    /// A span wider than [`MAX_HORIZON_DAYS`], refused before any walking
    /// starts so the walk is bounded by construction.
    HorizonExceeded { days: i64 },
}

impl CalendarError {
    /// The stable wire code, matching the capacity error vocabulary a caller
    /// already handles (`invalid_request` is the closest baseline code).
    pub const fn code(self) -> &'static str {
        "invalid_request"
    }
}

impl std::fmt::Display for CalendarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TimestampOutOfRange { millis } => write!(
                f,
                "instant {millis}ms is outside the supported calendar domain \
                 [{MIN_SUPPORTED_MILLIS}, {MAX_SUPPORTED_MILLIS}]"
            ),
            Self::Overflow => write!(f, "calendar day-step overflowed the instant range"),
            Self::HorizonExceeded { days } => write!(
                f,
                "calendar span of {days} days exceeds the supported horizon of \
                 {MAX_HORIZON_DAYS} days"
            ),
        }
    }
}

impl std::error::Error for CalendarError {}

/// One millisecond count from the Unix epoch.
///
/// A newtype rather than a bare `i64` so the primitives below cannot be called
/// with an unrelated millisecond count (a duration, an elapsed time, a
/// timestamp the caller half-computed) by accident.
///
/// Signed; see the module documentation for why. Ordering is the underlying
/// integer's, so it matches JavaScript's `getTime()` comparison exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Instant(pub i64);

impl Instant {
    /// From a millisecond count. Infallible — there is no ill-formed
    /// [`Instant`], only a misleading one, which is the caller's business.
    pub const fn from_millis(millis: i64) -> Self {
        Self(millis)
    }

    /// The raw millisecond count JavaScript's `getTime()` would return.
    pub const fn as_millis(self) -> i64 {
        self.0
    }

    /// Whether this instant is inside the supported [`Instant`] domain.
    ///
    /// The domain is JavaScript's `Date` range (see
    /// [`MIN_SUPPORTED_MILLIS`]/[`MAX_SUPPORTED_MILLIS`]). The public calendar
    /// primitives validate their inputs against it so an out-of-domain instant
    /// is a controlled refusal rather than a silent walk off the end of the
    /// representable range.
    pub const fn in_supported_domain(self) -> bool {
        self.0 >= MIN_SUPPORTED_MILLIS && self.0 <= MAX_SUPPORTED_MILLIS
    }

    /// This instant advanced by `days` whole days, or a controlled error.
    ///
    /// The baseline's `new Date(d.getTime() + 24 * 60 * 60 * 1000)`, with the
    /// multiplier pulled out so a caller stepping several days at once reads
    /// the same arithmetic. This is RA07's fix: the previous version answered a
    /// checked-add overflow by returning `self` unchanged, which turned
    /// [`count_working_days`]'s `while current < finish` loop into one that
    /// never advanced (`count_working_days(i64::MAX - 1, i64::MAX)` hung). Now
    /// an unrepresentable step is [`CalendarError::Overflow`], never a stale
    /// instant.
    ///
    /// The arithmetic is checked rather than saturating on purpose: saturation
    /// is the same "return a plausible-looking wrong value" failure in
    /// slow motion, and a caller cannot tell a saturated step from a real one.
    /// Note this refuses only true `i64` overflow; staying inside the supported
    /// *domain* is the public entrypoints' job — [`next_business_day`] and
    /// [`count_working_days`] both validate theirs — because a day-step that
    /// merely overshoots the domain by a fraction of a day is a legitimate
    /// final iteration, not an error.
    pub fn try_plus_days(self, days: i64) -> Result<Self, CalendarError> {
        let step = days
            .checked_mul(MS_PER_DAY)
            .ok_or(CalendarError::Overflow)?;
        let advanced = self.0.checked_add(step).ok_or(CalendarError::Overflow)?;
        Ok(Self(advanced))
    }

    /// `getUTCDay()`: 0 is Sunday through 6 is Saturday.
    ///
    /// # The algorithm, and why the shift is 4
    ///
    /// Weekday-from-epoch-days is a classic civil-from-days problem, and the
    /// standard solution is Hinnant's `civil_from_days` inverse: shift the
    /// epoch to a known Monday, divide, and take the remainder. Here the shift
    /// is written as a named constant rather than inlined, so the reasoning is
    /// auditable at the expression:
    ///
    /// ```text
    /// days = floor(millis / MS_PER_DAY)   // days since 1970-01-01, a Thursday
    /// weekday = (days + 4) rem 7          // the epoch's own weekday, added
    /// ```
    ///
    /// The `4` is **1970-01-01's weekday**, which is a Thursday, numbered 4
    /// under `getUTCDay`'s Sunday=0. Adding a day number to a known day's
    /// weekday and reducing mod 7 is the whole mapping: day 0 → 4, day 1 → 5,
    /// day 3 → 0 (Sunday), and so on round. `weekday_from_epoch_days` asserts
    /// this against dates whose weekday is independently known, including a
    /// pre-epoch one, so a wrong constant cannot pass by agreeing only at the
    /// anchor.
    ///
    /// Two properties this relies on, both verified rather than assumed:
    ///
    /// * **Floor division, not truncation.** The baseline's millisecond count
    ///   is negative before 1970, and JavaScript's `Date` floor-divides those
    ///   into whole days. Rust's `/` truncates toward zero, so `-1 / 7` is `0`
    ///   in Rust and `-1` in the floor sense. The 2020 fixture is positive and
    ///   unaffected, but the arithmetic must be right for negative input too or
    ///   a past allocation's weekday would be off by one and the loop would
    ///   mis-schedule it.
    /// * **The epoch really is a Thursday.** 1970-01-01 was a Thursday, and the
    ///   `+4` lands on Sunday=0. `weekday_of_epoch_days` tests both this and
    ///   the negative case against dates whose weekday is independently known.
    pub fn weekday(self) -> u8 {
        weekday_from_days(self.epoch_days())
    }

    /// Whole days since the epoch, floored.
    ///
    /// The baseline gets this by reading `getTime()` of a date already stepped
    /// a whole number of days; here it is floor division because the input may
    /// be negative. `f64` division is avoided as well — the baseline never
    /// divides a millisecond count in floating point, and `f64` loses exactness
    /// above 2^53 ms (~285,000 years) for no gain.
    const fn epoch_days(self) -> i64 {
        let quotient = self.0 / MS_PER_DAY;
        let remainder = self.0 % MS_PER_DAY;
        if remainder < 0 {
            quotient - 1
        } else {
            quotient
        }
    }

    /// Whether this instant falls on a Saturday or Sunday.
    ///
    /// UTC, as `getUTCDay()` is — the baseline never reads a local-zone weekday
    /// at all, so this port has no local-zone arithmetic to be ambiguous about.
    /// Both transcribed call sites (the weekend skip in [`next_business_day`],
    /// the filter in [`count_working_days`]) ask this same question, which is
    /// why there is one function for it rather than one per call site.
    pub fn is_weekend(self) -> bool {
        matches!(self.weekday(), 0 | 6)
    }
}

/// The weekday of a day number, where day 0 is 1970-01-01 (a Thursday).
///
/// `0` is Sunday, matching `getUTCDay()`.
fn weekday_from_days(days: i64) -> u8 {
    // The epoch's own weekday. Day 0 is 1970-01-01, a Thursday, which is
    // `getUTCDay() === 4`; adding each day number to that and reducing mod 7
    // is the whole mapping.
    const EPOCH_WEEKDAY: i64 = 4;
    // `rem_euclid` rather than `%` so a negative day number (any date before
    // 1970) yields a weekday in 0..=6 rather than a negative one — which is
    // what JavaScript's `%` gives for free but Rust's does not.
    (days + EPOCH_WEEKDAY).rem_euclid(7) as u8
}

/// The baseline's `nextBusinessDay`, transcribed line for line.
///
/// ```js
/// function nextBusinessDay(date) {
///   let cur = new Date(date.getTime())
///   while (cur.getUTCDay() === 0 || cur.getUTCDay() === 6) {
///     cur = new Date(cur.getTime() + 24 * 60 * 60 * 1000)
///     cur.setUTCHours(0, 0, 0, 0)
///   }
///   return cur
/// }
/// ```
///
/// Note what this does *not* do: it does not call `setUTCHours` on a date that
/// is already a weekday. That is the whole subtlety the module header explains,
/// and it is why the normalization sits in the loop body and not at the entry.
/// A weekday input is returned with its time-of-day intact.
///
/// Takes [`Instant`] **by value**, matching the baseline's
/// `new Date(date.getTime())` copy — the caller's instant is never advanced.
///
/// RA07: the input is validated against the supported domain, and each day-step
/// is a checked [`try_plus_days`], so an out-of-domain instant is
/// [`CalendarError::TimestampOutOfRange`] and an unrepresentable step is
/// [`CalendarError::Overflow`] — never a silent no-op that could spin. A
/// weekday input is returned unchanged after validation; a weekend input walks
/// at most two days to the following Monday, so this terminates by
/// construction.
pub fn next_business_day(instant: Instant) -> Result<Instant, CalendarError> {
    if !instant.in_supported_domain() {
        return Err(CalendarError::TimestampOutOfRange {
            millis: instant.as_millis(),
        });
    }
    let mut current = instant;
    while current.is_weekend() {
        current = current.try_plus_days(1)?;
        // `setUTCHours(0, 0, 0, 0)`, which is *inside* the loop: it zeroes the
        // time-of-day only when a weekend day was actually stepped over. A
        // Monday-at-05:00 input returns 05:00, not midnight.
        current = Instant::from_millis(at_midnight(current.as_millis()));
    }
    // RA07/RN05: the RESULT must be in the domain too, and this is not
    // redundant with the entry check. A weekend instant near the top of the
    // supported range steps *forward* onto Monday — past the ceiling — so
    // `next_business_day(MAX_SUPPORTED_MILLIS)` used to answer
    // `Ok(8640000172800000)`, above the `8640000000000000` maximum this very
    // function validated against. The input was legal and the arithmetic was
    // legal; what was missing was that the OUTPUT is a domain value every
    // caller then treats as one — `schedule` feeds it straight back into
    // `count_working_days` and into stored `start_at`/`finish_at` columns. An
    // out-of-domain answer is refused here, once, at the boundary that created
    // it, rather than by however many callers remember to re-check.
    if !current.in_supported_domain() {
        return Err(CalendarError::TimestampOutOfRange {
            millis: current.as_millis(),
        });
    }
    Ok(current)
}

/// The baseline's `setUTCHours(0, 0, 0, 0)`: zero the time-of-day.
///
/// Milliseconds within the current UTC day are dropped, leaving the day's first
/// instant. Expressed as a subtraction from the day number rather than a
/// re-derivation so it stays obviously the same operation the baseline performs
/// on an already-existing date object.
fn at_midnight(millis: i64) -> i64 {
    let days = Instant::from_millis(millis).epoch_days();
    days.saturating_mul(MS_PER_DAY)
}

/// The baseline's `countWorkingDays`, transcribed line for line.
///
/// ```js
/// function countWorkingDays(start, finish) {
///   let count = 0
///   let cur = new Date(start.getTime())
///   while (cur.getTime() < finish.getTime()) {
///     if (cur.getUTCDay() !== 0 && cur.getUTCDay() !== 6) count++
///     cur = new Date(cur.getTime() + 24 * 60 * 60 * 1000)
///   }
///   return Math.max(1, count)
/// }
/// ```
///
/// The loop condition is strict `<`, so a zero-length span counts nothing and
/// is floored to 1 by the trailing `Math.max(1, ...)`. That floor is what makes
/// the division in the schedule's per-allocation daily rate
/// (`remainingUnits / wDays`) total, and it is why a one-day allocation yields
/// `dailyUnits = min(dailyBurnRate, required)` rather than `Infinity`.
///
/// ## RA07: the bounds that make this terminate
///
/// The transcription above is faithful, and faithful transcription of this loop
/// is exactly what hung: `while current < finish` only terminates if `current`
/// *advances*, and the old `plus_days` returned the instant unchanged on
/// overflow — so `count_working_days(i64::MAX - 1, i64::MAX)` spun forever.
/// Two checks now run before the loop, and both are the controlled errors the
/// mandate asks for rather than a timeout on the caller:
///
/// 1. **Domain.** Both endpoints must be inside the supported [`Instant`]
///    domain, so the extreme `i64` values that triggered the hang are refused
///    up front as [`CalendarError::TimestampOutOfRange`].
/// 2. **Horizon.** The span must not exceed [`MAX_HORIZON_DAYS`], so the walk
///    is bounded by construction ([`CalendarError::HorizonExceeded`]) instead
///    of relying on the loop to terminate.
///
/// A `finish` before `start` is **not** an error: the baseline's loop simply
/// never runs and `Math.max(1, 0)` floors to `1`, and the differential fixtures
/// rely on that. The horizon is therefore only consulted for a non-empty span.
pub fn count_working_days(start: Instant, finish: Instant) -> Result<u32, CalendarError> {
    if !start.in_supported_domain() {
        return Err(CalendarError::TimestampOutOfRange {
            millis: start.as_millis(),
        });
    }
    if !finish.in_supported_domain() {
        return Err(CalendarError::TimestampOutOfRange {
            millis: finish.as_millis(),
        });
    }
    // Both endpoints are in-domain, so their difference cannot overflow `i64`
    // (it is at most `2 * MAX_SUPPORTED_MILLIS`), and the ceiling of that
    // difference in days is the exact number of loop iterations ahead.
    if finish >= start {
        let span = finish.as_millis() - start.as_millis();
        // Ceiling division written out (not `div_ceil`, which is unstable for
        // signed integers): whole days plus one for the partial day. This is
        // exactly the number of loop iterations ahead.
        let span_days = span / MS_PER_DAY + i64::from(span % MS_PER_DAY != 0);
        if span_days > MAX_HORIZON_DAYS {
            return Err(CalendarError::HorizonExceeded { days: span_days });
        }
    }
    let mut count = 0u32;
    let mut current = start;
    while current < finish {
        if !current.is_weekend() {
            count += 1;
        }
        current = current.try_plus_days(1)?;
    }
    // `Math.max(1, count)`. A zero-length span reports 1.
    Ok(count.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Milliseconds for `days` days after the epoch, as the baseline's own
    /// arithmetic would produce it.
    fn days_after_epoch(days: i64) -> i64 {
        days * MS_PER_DAY
    }

    /// 2030-01-07, the Monday every capacity fixture is expressed relative to.
    ///
    /// Its day number is derived by the same arithmetic JavaScript uses, not
    /// assumed: `Date.UTC(2030, 0, 7)` is 21,921 days after the epoch, and
    /// `getUTCDay()` on it is 1 (Monday). Both are checked in the probe that
    /// generated the differential suite's reference values.
    const MONDAY_2030: i64 = 21_921;

    /// The dates whose weekdays are named independently in the differential
    /// suite's fixtures. 2030-01-07 is a Monday — the verifier's own comment
    /// says so, and every fixture in `tools/library/verify-capacity.mjs:372`
    /// is expressed relative to it.
    #[test]
    fn weekday_of_epoch_days() {
        // 1970-01-01 is the epoch and a Thursday.
        assert_eq!(
            Instant::from_millis(0).weekday(),
            4,
            "epoch day is a Thursday"
        );
        // 2030-01-07, the Monday every capacity fixture uses.
        assert_eq!(
            Instant::from_millis(days_after_epoch(MONDAY_2030)).weekday(),
            1,
            "2030-01-07 is a Monday"
        );
        // The surrounding days of that week, so an off-by-one in the shift
        // cannot pass by agreeing only at the anchor.
        for (day_offset, expected) in [
            (0i64, 1u8), // Mon 2030-01-07
            (1, 2),      // Tue
            (2, 3),      // Wed
            (3, 4),      // Thu
            (4, 5),      // Fri
            (5, 6),      // Sat 2030-01-12
            (6, 0),      // Sun 2030-01-13
            (7, 1),      // next Mon 2030-01-14
        ] {
            assert_eq!(
                Instant::from_millis(days_after_epoch(MONDAY_2030 + day_offset)).weekday(),
                expected,
                "day offset {day_offset} from 2030-01-07"
            );
        }
    }

    #[test]
    fn weekday_is_floored_for_pre_epoch_days() {
        // 1969-12-31, the day before the epoch, is a Wednesday. Truncating
        // division would put day -1 at weekday 6 (Saturday) instead of 3.
        assert_eq!(Instant::from_millis(-MS_PER_DAY).weekday(), 3);
        // And one millisecond before midnight, still the same day.
        assert_eq!(Instant::from_millis(-1).weekday(), 3);
        // A known pre-epoch weekday further back: 1900-01-01 was a Monday.
        assert_eq!(
            Instant::from_millis(days_after_epoch(-25_567)).weekday(),
            1,
            "1900-01-01 is a Monday"
        );
    }

    /// `nextBusinessDay` leaves a weekday's time-of-day alone — the one line
    /// of the baseline most likely to be "cleaned up" into a bug.
    #[test]
    fn keeps_the_time_of_day_when_no_weekend_is_crossed() {
        // 2030-01-07T00:00:00-05:00 is 2030-01-07T05:00:00Z, a Monday.
        let monday_at_five = Instant::from_millis(days_after_epoch(MONDAY_2030) + 5 * 3_600_000);
        let resolved = next_business_day(monday_at_five).expect("a Monday is in-domain");
        assert_eq!(
            resolved.as_millis(),
            monday_at_five.as_millis(),
            "a weekday input is returned unchanged, time-of-day included"
        );
        // A Tuesday at 13:45:30.500 — the other fixture that depends on this.
        let tuesday_mid_afternoon = Instant::from_millis(
            days_after_epoch(MONDAY_2030 + 1) + 13 * 3_600_000 + 45 * 60_000 + 30_500,
        );
        assert_eq!(
            next_business_day(tuesday_mid_afternoon)
                .expect("a Tuesday is in-domain")
                .as_millis(),
            tuesday_mid_afternoon.as_millis(),
            "a weekday with a time-of-day keeps it"
        );
    }

    #[test]
    fn skips_a_weekend_to_the_following_monday_at_midnight() {
        // Saturday 2030-01-12T00:00Z advances to Monday 2030-01-14T00:00Z, and
        // the time is normalized because a weekend was crossed.
        let saturday = Instant::from_millis(days_after_epoch(MONDAY_2030 + 5));
        assert_eq!(
            next_business_day(saturday)
                .expect("a Saturday is in-domain")
                .as_millis(),
            days_after_epoch(MONDAY_2030 + 7),
            "Saturday advances two days to Monday, at midnight"
        );
        // Sunday likewise.
        let sunday = Instant::from_millis(days_after_epoch(MONDAY_2030 + 6));
        assert_eq!(
            next_business_day(sunday)
                .expect("a Sunday is in-domain")
                .as_millis(),
            days_after_epoch(MONDAY_2030 + 7),
            "Sunday advances one day to Monday, at midnight"
        );
    }

    #[test]
    fn normalizes_the_time_only_when_a_weekend_is_crossed() {
        // A Saturday carrying a time-of-day: the skip runs, so `setUTCHours`
        // runs, so the time is dropped.
        let saturday_late =
            Instant::from_millis(days_after_epoch(MONDAY_2030 + 5) + 20 * 3_600_000);
        assert_eq!(
            next_business_day(saturday_late)
                .expect("a Saturday is in-domain")
                .as_millis(),
            days_after_epoch(MONDAY_2030 + 7),
            "a weekend day's time-of-day is normalized away"
        );
    }

    #[test]
    fn counts_working_days_between_two_mondays() {
        // Mon 2030-01-07 to Wed 2030-01-09: two working days.
        let monday = Instant::from_millis(days_after_epoch(MONDAY_2030));
        let wednesday = Instant::from_millis(days_after_epoch(MONDAY_2030 + 2));
        assert_eq!(count_working_days(monday, wednesday).expect("in-domain"), 2);
        // The same span across a weekend: Friday to Monday is one working day.
        let friday = Instant::from_millis(days_after_epoch(MONDAY_2030 + 4));
        let next_monday = Instant::from_millis(days_after_epoch(MONDAY_2030 + 7));
        assert_eq!(
            count_working_days(friday, next_monday).expect("in-domain"),
            1
        );
        // A zero-length span floors to 1.
        assert_eq!(count_working_days(monday, monday).expect("in-domain"), 1);
        // A full week Monday-to-Monday is five.
        assert_eq!(
            count_working_days(monday, next_monday).expect("in-domain"),
            5
        );
    }

    #[test]
    fn counts_working_days_across_a_pre_epoch_span() {
        // 1900-01-01 (Mon) to 1900-01-05 (Fri) is four working days. This is
        // the case where the floor division matters — a truncating day count
        // would land the same millisecond values on the wrong weekdays.
        let monday_1900 = Instant::from_millis(days_after_epoch(-25_567));
        let friday_1900 = Instant::from_millis(days_after_epoch(-25_563));
        assert_eq!(
            count_working_days(monday_1900, friday_1900).expect("in-domain"),
            4
        );
    }

    // --- RA07: the bounds that make the walk terminate ---------------------

    /// The audit's exact reproducer: `count_working_days(i64::MAX - 1,
    /// i64::MAX)` used to hang forever because `plus_days` returned the
    /// unchanged instant on overflow and the loop never advanced. It must now
    /// return a controlled error immediately.
    #[test]
    fn the_overflow_reproducer_returns_a_controlled_error_instead_of_hanging() {
        let start = Instant::from_millis(i64::MAX - 1);
        let finish = Instant::from_millis(i64::MAX);
        assert_eq!(
            count_working_days(start, finish),
            Err(CalendarError::TimestampOutOfRange {
                millis: i64::MAX - 1
            }),
            "an out-of-domain start must be refused before any walking starts"
        );
    }

    /// Both endpoints outside the domain are refused; the check runs on each
    /// independently so a bad `finish` alone is also caught.
    #[test]
    fn an_out_of_domain_endpoint_is_refused() {
        let monday = Instant::from_millis(days_after_epoch(MONDAY_2030));
        assert!(matches!(
            count_working_days(monday, Instant::from_millis(i64::MAX)),
            Err(CalendarError::TimestampOutOfRange { .. })
        ));
        assert!(matches!(
            count_working_days(Instant::from_millis(i64::MIN), monday),
            Err(CalendarError::TimestampOutOfRange { .. })
        ));
    }

    /// A span just inside the horizon still walks; a span just past it is
    /// refused. This pins the boundary on both sides so the bound cannot drift
    /// into either "hangs" or "refuses valid work".
    #[test]
    fn the_horizon_bounds_a_wide_span_on_both_sides() {
        let start = Instant::from_millis(days_after_epoch(MONDAY_2030));
        // Exactly MAX_HORIZON_DAYS: the largest span the contract allows, so
        // it walks (bounded at 365,000 iterations, a few milliseconds).
        let at_limit = start.try_plus_days(MAX_HORIZON_DAYS).expect("in range");
        assert!(count_working_days(start, at_limit).is_ok());
        // One day past it: refused before any walking starts.
        let past_limit = start.try_plus_days(MAX_HORIZON_DAYS + 1).expect("in range");
        assert_eq!(
            count_working_days(start, past_limit),
            Err(CalendarError::HorizonExceeded {
                days: MAX_HORIZON_DAYS + 1
            })
        );
    }

    /// A `finish` before `start` is the baseline's zero-iteration case and
    /// floors to 1 — not an error, and not a hang. Pinned so a future
    /// "helpful" validation does not turn it into a refusal.
    #[test]
    fn a_reversed_span_floors_to_one_without_hanging() {
        let monday = Instant::from_millis(days_after_epoch(MONDAY_2030));
        let friday = Instant::from_millis(days_after_epoch(MONDAY_2030 + 4));
        assert_eq!(
            count_working_days(friday, monday).expect("reversed is ok"),
            1
        );
    }

    /// The day-step refuses an unrepresentable advance rather than returning
    /// the unchanged instant (the old saturation behavior RA07 removed).
    #[test]
    fn try_plus_days_refuses_an_unrepresentable_step() {
        let near_max = Instant::from_millis(MAX_SUPPORTED_MILLIS);
        // Even inside the supported domain, an enormous step overflows `i64`.
        assert_eq!(
            near_max.try_plus_days(i64::MAX),
            Err(CalendarError::Overflow)
        );
        // A step that lands outside the domain but is representable is allowed
        // here; domain enforcement is the public entrypoints' contract.
        assert!(near_max.try_plus_days(1).is_ok());
    }

    /// RN05: the RESULT of a weekend skip must be in the domain, not just the
    /// input.
    ///
    /// `MAX_SUPPORTED_MILLIS` is a supported instant — the entry check passes
    /// it — and it falls on a weekend, so the skip walks it forward onto the
    /// following Monday, which is past the ceiling. The old function returned
    /// `Ok(8640000172800000)` for this exact input, above the
    /// `8640000000000000` maximum it had validated the input against. Every
    /// caller treats an `Ok` as a domain value.
    #[test]
    fn a_weekend_step_that_leaves_the_domain_is_refused() {
        let at_max = Instant::from_millis(MAX_SUPPORTED_MILLIS);
        assert!(
            at_max.in_supported_domain() && at_max.is_weekend(),
            "this regression needs the ceiling to be a weekend instant"
        );
        assert_eq!(
            next_business_day(at_max),
            Err(CalendarError::TimestampOutOfRange {
                millis: 8_640_000_172_800_000
            }),
            "stepping the ceiling onto Monday must refuse, not return an \
             out-of-domain instant"
        );

        // The last weekday at or below the ceiling is still fine: the fix
        // refuses an out-of-domain RESULT, not a near-boundary input.
        let mut last_weekday = at_max;
        while last_weekday.is_weekend() {
            last_weekday = last_weekday
                .try_plus_days(-1)
                .expect("stepping back from the ceiling stays in range");
        }
        let resolved = next_business_day(last_weekday).expect("a weekday in domain");
        assert!(
            resolved.in_supported_domain(),
            "an in-domain weekday must resolve inside the domain"
        );
    }

    /// RN05's lower bound, symmetric: a pre-epoch weekend steps *backwards*
    /// only in wall-clock terms — it still advances the instant, so the floor
    /// cannot be overshot the way the ceiling can. Pinned so the two bounds
    /// stay a pair rather than one fix and one assumption.
    #[test]
    fn the_floor_of_the_domain_resolves_inside_the_domain() {
        let at_min = Instant::from_millis(MIN_SUPPORTED_MILLIS);
        assert!(at_min.in_supported_domain());
        let resolved = next_business_day(at_min).expect("the floor is resolvable");
        assert!(
            resolved.in_supported_domain(),
            "the floor must resolve inside the domain, got {}",
            resolved.as_millis()
        );
    }

    /// The horizon and the domain are independent refusals, and both fire
    /// before any walking starts.
    #[test]
    fn an_out_of_domain_input_is_refused_before_the_horizon_is_considered() {
        assert_eq!(
            count_working_days(
                Instant::from_millis(MAX_SUPPORTED_MILLIS + 1),
                Instant::from_millis(MAX_SUPPORTED_MILLIS + 2)
            ),
            Err(CalendarError::TimestampOutOfRange {
                millis: MAX_SUPPORTED_MILLIS + 1
            })
        );
    }
}
