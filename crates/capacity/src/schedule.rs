//! `calculateSchedule` — the capacity search loop, ported.
//!
//! The baseline is `packages/capacity-pg/src/repository.ts:117-231`. It answers
//! one question: given a pool's weekly capacity and the jobs already running in
//! it, when can a new job start? And it answers it by *walking candidate start
//! days forward* until it finds one where every day of the proposed schedule
//! fits under the pool's concurrency and burn limits.
//!
//! This is the whole of R02's third deliverable. `countWorkingDays` and
//! `nextBusinessDay` live in [`crate::calendar`]; the arithmetic helpers
//! ([`math_min`], [`math_round`], [`math_ceil`], [`math_max_1`]) live here
//! because the baseline inlines them.
//!
//! ## What this port deliberately does not decide
//!
//! The baseline's caller reads `activeAllocations` out of PostgreSQL with
//! `SELECT ... WHERE state IN ('scheduled','in_progress') ORDER BY
//! scheduled_finish_at ASC` (repository.ts:440-467, :655-690). The rows are an
//! input here, not something this crate queries. That is the same boundary the
//! assembly planner's port drew, and for the same reason: the scheduling math
//! is the part that has to agree line-for-line with the baseline, and a query
//! is the part that can be re-expressed in whatever data layer the caller has.
//!
//! ## The four arithmetic rules that are easy to get wrong
//!
//! 1. **`Math.max(1, ...)` appears three times** — [`MATH_MAX_1`] floors
//!    `workingDaysNeeded`, [`count_working_days`](crate::calendar::count_working_days)'s
//!    divisor, and `estimatedCalendarDays`. Each is a separate call in the
//!    baseline and each is reproduced separately here.
//! 2. **`Math.round` is half-toward-+Infinity, not half-away-from-zero.**
//!    Rust's `f64::round` disagrees on every negative half, so [`math_round`]
//!    is transcribed rather than called through. Unreachable with a positive
//!    span, but a subtraction's sign is not a property a reader can trust.
//! 3. **The capacity check carries a `+1e-6` tolerance** — [`CAPACITY_EPSILON`].
//!    It is not decoration; see the module-level note on it below.
//! 4. **`nextBusinessDay` is a no-op on weekdays.** It only normalizes to
//!    midnight *inside* the weekend-skip loop, so a weekday input keeps its
//!    time-of-day. Two fixtures depend on this: a `-05:00` offset desired start
//!    surfaces at `05:00`, and a `13:45:30.500` timestamp keeps its
//!    milliseconds.

use crate::calendar::{count_working_days, next_business_day, Instant, MS_PER_DAY};
use crate::error::CapacityError;

/// `Math.min(a, b)`.
///
/// Unlike `Math.max`, JavaScript's `Math.min` has no NaN or signed-zero
/// surprise that this crate reaches: `f64::min` propagates NaN the way
/// `Math.min` does, and a negative zero could only arrive here if
/// `requiredUnits / workingDaysNeeded` were `-0`, which needs a negative
/// `requiredUnits` — zod's `.positive()` rejects that at the boundary. So the
/// method is called directly and this helper exists to keep the `min`/`max`
/// pair visible together in the transcription.
fn math_min(a: f64, b: f64) -> f64 {
    a.min(b)
}

/// `Math.round(x)`: halves round toward **+Infinity**.
///
/// Same function the pricing crate's `jsmath::math_round` implements, repeated
/// here because this crate does not depend on `archava-pricing` — the two
/// crates share a baseline behavior, not a dependency. If they ever diverge it
/// will be visible in `archava-capacity`'s own tests rather than hidden behind
/// a re-export.
fn math_round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let floored = x.floor();
    let rounded = if x - floored >= 0.5 {
        floored + 1.0
    } else {
        floored
    };
    if rounded == 0.0 && x.is_sign_negative() {
        -0.0
    } else {
        rounded
    }
}

/// `Math.ceil(x)` for the one place the baseline uses it.
///
/// Called directly — Rust's `f64::ceil` and JavaScript's `Math.ceil` agree on
/// the whole domain except that `Math.ceil(NaN)` is `NaN` and `f64::ceil` also
/// gives `NaN`, so there is nothing to transcribe.
fn math_ceil(x: f64) -> f64 {
    x.ceil()
}

/// The baseline's repeated `Math.max(1, ...)`.
///
/// A named function rather than an inline `1.0f64.max(x)` because it appears
/// three times in the baseline and each appearance means the same thing: *a
/// span is never less than one*. Naming it says that out loud, and gives the
/// three call sites a single place to point at when a fixture disagrees.
fn math_max_1(x: f64) -> f64 {
    1.0f64.max(x)
}

/// The tolerance on the per-day capacity comparison.
///
/// `usedCapacityToday + neededDailyBurn > dailyBurnRate + 1e-6` — the `+1e-6`
/// is the baseline's answer to floating-point equality, and it is **not**
/// dead code. A pool with `weeklyCapacity: 3` burns `0.6` per day; an
/// allocation of `1` unit over `5` working days burns `1/5 = 0.2`; a job of
/// `0.8` units over `max(1, ceil(0.8/0.6)) = 2` days burns `0.4`. In exact
/// arithmetic `0.2 + 0.4 == 0.6` is a fit. In f64 the sum is
/// `0.6000000000000001` — one ULP over — so the *exact* comparison rejects
/// Monday and pushes the whole schedule to the following week.
///
/// That is the `epsilon-admits-one-ulp-over-capacity` fixture in the
/// differential suite: with the tolerance, the job starts on the desired
/// Monday (2030-01-07); without it, on 2030-01-14. A port that dropped the
/// `+1e-6` would pass every hand-picked "sensible" fixture and still be wrong.
///
/// The companion fixture `epsilon-does-not-admit-a-real-overrun` pins the
/// other side: `0.6` is not a pass-through, and a genuine 40-unit overrun is
/// still rejected.
pub const CAPACITY_EPSILON: f64 = 1e-6;

/// The baseline's `while (loopCount < 1000)` bound.
///
/// Long jobs do not spend this budget. A blocked candidate jumps straight to
/// the `earliestBlockingFinish` of whatever is in the way, so a 2000-hour job
/// over a saturated pool still resolves in a handful of iterations. The cap is
/// load-bearing for exactly one situation: `maxConcurrentJobs: 0`, where every
/// candidate day is rejected on the concurrency check and nothing ever jumps
/// the search forward, so the loop is the only thing that stops it. That is the
/// `unsatisfiable-concurrency` fixture, and it is the reason the cap stays.
pub const MAX_ITERATIONS: u32 = 1000;

/// The pool's two scheduling limits.
///
/// `weeklyCapacity` doubles as `maxConcurrentJobs` here because both come from
/// the same `capacityPoolSchema` row (`contracts.ts`:
/// `weeklyCapacity: z.number().positive().max(1000000)`,
/// `maxConcurrentJobs: z.number().int().positive().max(100)`). The baseline
/// does not validate them — it is handed them as `{ weeklyCapacity: number;
/// maxConcurrentJobs: number }` and computes — so this crate does not either.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoolCapacity {
    /// Units of work the pool burns per week.
    pub weekly_capacity: f64,
    /// How many jobs may run in the pool at once.
    pub max_concurrent_jobs: u32,
}

/// One running job, as the scheduling math needs it.
///
/// The baseline's `CapacityAllocation` is a database row; only four of its
/// fields reach the math, and one derived value is computed from them
/// (`repository.ts:133-147`). With the exception of `id` — which the math
/// carries through without reading — these are that row's scheduling columns,
/// typed to match `capacityAllocationSchema` (`contracts.ts`):
/// `requiredUnits: positiveNumber`, `remainingUnits: nonNegativeNumber`,
/// `scheduledStartAt/scheduledFinishAt: instantSchema`.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveAllocation {
    /// The allocation's identifier, as `capacityAllocationSchema` spells it.
    ///
    /// The scheduling math never reads it — the baseline's projection carries
    /// `id` and then filters on `finish` alone, and this port's projection drops
    /// it for that reason (see [`AllocDetail`]). The field stays on this type
    /// because it is part of the row a caller holds, and it is the difference
    /// between two otherwise-identical allocations; a caller comparing
    /// allocations, or reporting which one blocked a day, needs it.
    pub id: String,
    /// When the job starts (`scheduledStartAt`).
    pub start: Instant,
    /// When the job finishes (`scheduledFinishAt`).
    pub finish: Instant,
    /// `requiredUnits`.
    pub required_units: f64,
    /// `remainingUnits`.
    pub remaining_units: f64,
}

impl ActiveAllocation {
    /// `remainingUnits > 0 ? remainingUnits : requiredUnits`.
    ///
    /// A strict `> 0`, so `remainingUnits` of exactly `0` **and** a negative
    /// value both fall back to `requiredUnits`. Two fixtures pin those
    /// separately (`remaining-zero-falls-back-to-required`,
    /// `remaining-negative-falls-back`) because they are the same code path
    /// with different inputs, and a future change to the comparison should
    /// break one without the other.
    fn effective_units(&self) -> f64 {
        if self.remaining_units > 0.0 {
            self.remaining_units
        } else {
            self.required_units
        }
    }

    /// `dailyUnits`: the burn this allocation contributes per day, capped at
    /// the pool's own daily rate.
    ///
    /// The cap is `Math.min(dailyBurnRate, ...)` — one allocation can never be
    /// charged more than the pool burns in a day, which is what keeps a
    /// 10-year allocation from reading as an infinite daily charge.
    ///
    /// RA07: fallsible, because [`count_working_days`] now is — an allocation
    /// whose span is out of the supported domain or past the horizon is a
    /// malformed request, mapped to [`CapacityError`] by the caller.
    fn daily_units(&self, daily_burn_rate: f64) -> Result<f64, CapacityError> {
        let working_days = count_working_days(self.start, self.finish)?;
        Ok(math_min(
            daily_burn_rate,
            self.effective_units() / working_days as f64,
        ))
    }
}

/// What the baseline's `ScheduleResult` carries.
///
/// `queuePosition`, `scheduledStartAt`, `scheduledFinishAt`, `burnRatePerDay`,
/// `estimatedCalendarDays` and `workingDays` (`repository.ts:87-94`). The two
/// timestamps stay [`Instant`]s rather than becoming formatted strings: the
/// baseline's `.toISOString()` is a serialization concern, and every caller in
/// this crate wants the comparable value underneath it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScheduleResult {
    /// How many jobs are ahead of this one in the queue. `0` when the job
    /// starts on its desired day, `>= 1` when it was pushed later.
    pub queue_position: u32,
    /// `scheduledStartAt`.
    pub scheduled_start: Instant,
    /// `scheduledFinishAt`.
    pub scheduled_finish: Instant,
    /// `burnRatePerDay` — the pool's daily burn, `weeklyCapacity / 5`.
    pub burn_rate_per_day: f64,
    /// `estimatedCalendarDays`.
    pub estimated_calendar_days: u32,
    /// `workingDays` — how many working days the job needs.
    pub working_days: u32,
}

/// An allocation's scheduling dates, reduced to what the day loop reads.
///
/// The baseline builds the same three-field projection at `repository.ts:141-146`
/// (`{ id, start, finish, dailyUnits }`) and then filters on `finish`. This port
/// keeps the three fields the day loop actually reads and drops `id`, because
/// the baseline's `id` is *also* never read — it is written into the projection
/// and then never consulted. TypeScript does not warn about an unused object
/// property, so the baseline can carry it for free; Rust's dead-code lint is a
/// signal it cannot, and honouring it here is what forces the observation into
/// the open: **nothing in the scheduling math identifies an allocation by id.**
///
/// That is a real property of the baseline and not an artifact of the port. A
/// future caller that needs per-allocation attribution in a schedule decision
/// will have to add it to the baseline first — which is the same rule this crate
/// follows for the error codes it does not model.
struct AllocDetail {
    start: Instant,
    finish: Instant,
    daily_units: f64,
}

/// `calculateSchedule`, ported.
///
/// Every line below has a counterpart in `repository.ts:117-231`; the comments
/// name the baseline line where the correspondence is not otherwise obvious.
/// The one departure is structural rather than behavioral: the baseline reads
/// `new Date()` once for the no-`desiredStartAt` case, and this port takes it
/// from [`now`] so the rest of the function stays free of it.
pub fn calculate_schedule(
    pool: PoolCapacity,
    active_allocations: &[ActiveAllocation],
    required_units: f64,
    desired_start_at: Option<Instant>,
) -> Result<ScheduleResult, CapacityError> {
    // Entry validation for the values the baseline never re-checks inside
    // `calculateSchedule`. Zero and negative `weeklyCapacity` are already
    // refused by the `dailyBurnRate <= 0` branch below (matching the baseline's
    // `insufficient_capacity` for `burn-rate-zero-refuses` and
    // `burn-rate-negative-refuses`), and `maxConcurrentJobs: 0` and
    // `requiredUnits: 0` are deliberate baseline successes recorded in
    // `reference.json` (`unsatisfiable-concurrency`, `zero-units`), so those
    // are left to the search itself. Only the non-finite inputs — NaN and
    // Infinity — have no recorded case and are refused here before they can
    // flow into the search and either produce garbage or burn the
    // 1000-iteration loop without progress.
    if !pool.weekly_capacity.is_finite() {
        return Err(CapacityError::invalid_request());
    }
    if !required_units.is_finite() {
        return Err(CapacityError::invalid_request());
    }

    let daily_burn_rate = pool.weekly_capacity / 5.0;

    // repository.ts:127-128. `dailyBurnRate <= 0` refuses zero or negative
    // capacity before the search — this is the branch behind
    // `burn-rate-zero-refuses` (wc=0) and `burn-rate-negative-refuses` (wc=-50)
    // in `reference.json`, both recorded as `insufficient_capacity`.
    if daily_burn_rate <= 0.0 {
        return Err(CapacityError::insufficient_capacity());
    }

    // repository.ts:127-128. `desiredStartAt ? new Date(desiredStartAt) : now`
    // — the only wall-clock read in the whole function. RA07: an out-of-domain
    // desired start is a controlled `invalid_request`, not a silent walk.
    let candidate_start = next_business_day(desired_start_at.unwrap_or_else(now))?;
    let initial_desired_time = candidate_start;

    let working_days_needed = math_max_1(math_ceil(required_units / daily_burn_rate)) as u32;
    let needed_daily_burn = math_min(daily_burn_rate, required_units / working_days_needed as f64);

    // repository.ts:133-147. The map is not memoized across days in the
    // baseline either — it runs once, and `dailyUnits` is carried forward.
    // RA07: `daily_units` is fallible now, so the map collects a `Result` and
    // `?` propagates a malformed allocation's span out of `calculate_schedule`.
    let alloc_details: Vec<AllocDetail> = active_allocations
        .iter()
        .map(|alloc| {
            Ok(AllocDetail {
                start: alloc.start,
                finish: alloc.finish,
                daily_units: alloc.daily_units(daily_burn_rate)?,
            })
        })
        .collect::<Result<_, CapacityError>>()?;

    let mut loop_count: u32 = 0;
    let mut candidate_start = candidate_start;
    let mut candidate_finish = candidate_start;
    let mut found = false;

    while loop_count < MAX_ITERATIONS {
        loop_count += 1;
        // repository.ts:155. Normalizes a weekend to the following Monday;
        // a weekday is already valid and passes through unchanged.
        candidate_start = next_business_day(candidate_start)?;

        // repository.ts:157-164. Collect the job's first `workingDaysNeeded`
        // weekdays, then step one past the last to find `candidateFinish`.
        // RA07: `try_plus_days` refuses an unrepresentable step rather than
        // silently returning the unchanged instant, so this inner walk cannot
        // spin without progress.
        let mut proposed_working_days: Vec<Instant> = Vec::with_capacity(
            usize::try_from(working_days_needed).unwrap_or(propose_working_days_hint()),
        );
        let mut cursor = candidate_start;
        while proposed_working_days.len() < working_days_needed as usize {
            if !cursor.is_weekend() {
                proposed_working_days.push(cursor);
            }
            cursor = cursor.try_plus_days(1)?;
        }
        candidate_finish = next_business_day(cursor)?;

        // repository.ts:167-194. Test every proposed day; stop at the first
        // that fails, keeping the earliest finish that blocked it.
        let mut valid = true;
        let mut earliest_blocking_finish: Option<Instant> = None;
        for day in &proposed_working_days {
            let day_start = *day;
            let day_end = day_start.try_plus_days(1)?;

            let mut concurrent_jobs_today: u32 = 0;
            let mut used_capacity_today: f64 = 0.0;

            for a in &alloc_details {
                // `a.start < dayEnd && a.finish > dayStart` — the overlap
                // test. A half-open interval, so an allocation finishing
                // exactly at `dayStart` does not count for that day.
                if a.start < day_end && a.finish > day_start {
                    concurrent_jobs_today += 1;
                    used_capacity_today += a.daily_units;
                    // `a.finish > dayStart && a.finish < earliestBlockingFinish`
                    // — the second clause is the `!== Infinity` part; an
                    // `Option` here makes it structural instead of sentinel.
                    if a.finish > day_start
                        && earliest_blocking_finish.is_none_or(|earliest| a.finish < earliest)
                    {
                        earliest_blocking_finish = Some(a.finish);
                    }
                }
            }

            // repository.ts:187-193.
            if concurrent_jobs_today + 1 > pool.max_concurrent_jobs
                || used_capacity_today + needed_daily_burn > daily_burn_rate + CAPACITY_EPSILON
            {
                valid = false;
                break;
            }
        }

        if valid {
            found = true;
            break;
        }

        // repository.ts:201-205. Jump to whatever blocked us when that is
        // ahead of the candidate; otherwise step one day.
        let next_start = match earliest_blocking_finish {
            Some(finish) if finish > candidate_start => finish,
            _ => candidate_start.try_plus_days(1)?,
        };
        candidate_start = next_business_day(next_start)?;
    }

    // repository.ts:208-210.
    if !found {
        return Err(CapacityError::insufficient_capacity());
    }

    // repository.ts:212-215. `Math.max(1, Math.round(days))`, and the trailing
    // `as u32` is the whole reason the helpers are `f64`-in/`f64`-out: the
    // baseline computes in one number type and then hands `Math.round`'s result
    // to a caller expecting an integer, so the truncation happens once, at the
    // boundary, and never inside the arithmetic.
    let elapsed_days = candidate_finish.as_millis() - candidate_start.as_millis();
    let estimated_calendar_days =
        math_max_1(math_round(elapsed_days as f64 / MS_PER_DAY as f64)) as u32;

    // repository.ts:217-221. Only when the job was actually pushed.
    let queue_position = if candidate_start > initial_desired_time {
        let blocking = alloc_details
            .iter()
            .filter(|a| a.finish > initial_desired_time)
            .count();
        math_max_1(blocking as f64) as u32
    } else {
        0
    };

    Ok(ScheduleResult {
        queue_position,
        scheduled_start: candidate_start,
        scheduled_finish: candidate_finish,
        burn_rate_per_day: daily_burn_rate,
        estimated_calendar_days,
        working_days: working_days_needed,
    })
}

/// The current instant.
///
/// The baseline's `new Date()` at `repository.ts:126`. Isolated as its own
/// function so the rest of the port never calls it — the differential suite
/// runs every fixture with an explicit `desiredStartAt` precisely so this
/// function is never on its path.
fn now() -> Instant {
    Instant::from_millis(current_time_millis())
}

/// The wall clock, read once, in milliseconds since the epoch.
///
/// `std::time::SystemTime::now()` is the platform's own clock, the same one
/// `new Date()` reads. It is not `Instant::now()` from `std::time` — that type
/// is monotonic and has no epoch, so it cannot answer "what millisecond count
/// is it now". A failed read cannot be represented: an earlier-than-epoch
/// `SystemTime` is a genuine possibility in principle and is folded to the
/// epoch rather than panicking, which is the same tolerance
/// `Duration::as_millis` would give a negative duration.
#[cfg(not(target_arch = "wasm32"))]
fn current_time_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(elapsed) => elapsed.as_millis() as i64,
        Err(_) => 0,
    }
}

/// The wall clock under `wasm32-unknown-unknown`.
///
/// `std::time::SystemTime::now()` panics on that target, so a wasm build of
/// this crate cannot carry it — which is why the wall-clock read is confined to
/// one function behind a `cfg` rather than left inline. R01's wasm gate is the
/// reason this crate is wasm-clean at all; scheduling with no `desiredStartAt`
/// is the one behavior a wasm caller cannot ask for, and it is the host's job
/// to supply a desired start.
#[cfg(target_arch = "wasm32")]
fn current_time_millis() -> i64 {
    0
}

/// The `Vec::with_capacity` argument when `working_days_needed` will not fit a
/// `usize`.
///
/// Unreachable — `workingDaysNeeded` is a count of days derived from
/// `requiredUnits / dailyBurnRate`, and `1e6 / 5` is `200_000`. Kept as a named
/// function so the `unwrap_or` argument reads as what it is rather than as a
/// magic `0`.
fn propose_working_days_hint() -> usize {
    0
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
    /// `Date.UTC(2030, 0, 7)` is 21,921 days after the epoch and
    /// `getUTCDay()` on it is 1 (Monday) — both checked in the probe that
    /// generated the differential suite's reference values.
    const MONDAY_2030: i64 = 21_921;

    /// 2030-01-09, the Wednesday the saturated fixtures land on.
    const WEDNESDAY_2030: i64 = 21_923;

    /// 2030-01-11, the Friday the weekend-boundary fixtures use.
    const FRIDAY_2030: i64 = 21_925;

    /// 2030-01-14, the Monday a Saturday or Sunday desired start normalizes to.
    const NEXT_MONDAY_2030: i64 = 21_928;

    /// 2020-01-06 — an allocation long finished, still scheduled.
    ///
    /// Its epoch millis are **positive**: 2020 is after 1970, so this is not a
    /// pre-epoch case. The fixture is here for the weekend/overflow behavior,
    /// not for the signed arithmetic.
    const PAST_MONDAY_2020: i64 = 18_267;

    /// 2040-01-01, a Sunday — the indefinitely-blocking allocation.
    const FAR_SUNDAY_2040: i64 = 25_567;

    fn at(days: i64) -> Instant {
        Instant::from_millis(days_after_epoch(days))
    }

    fn pool(weekly_capacity: f64, max_concurrent_jobs: u32) -> PoolCapacity {
        PoolCapacity {
            weekly_capacity,
            max_concurrent_jobs,
        }
    }

    fn allocation(
        id: &str,
        start: i64,
        finish: i64,
        required_units: f64,
        remaining_units: f64,
    ) -> ActiveAllocation {
        ActiveAllocation {
            id: id.to_string(),
            start: at(start),
            finish: at(finish),
            required_units,
            remaining_units,
        }
    }

    /// A pool that burns 40 units a day and runs one job at a time.
    fn default_pool() -> PoolCapacity {
        pool(200.0, 1)
    }

    #[test]
    fn an_idle_pool_starts_the_job_on_its_desired_monday() {
        let result = calculate_schedule(default_pool(), &[], 80.0, Some(at(MONDAY_2030)))
            .expect("an idle pool can always schedule");

        assert_eq!(result.scheduled_start, at(MONDAY_2030));
        // 80 units over a 40/day burn needs two working days, so the job runs
        // Monday and Tuesday and finishes Wednesday morning.
        assert_eq!(result.scheduled_finish, at(WEDNESDAY_2030));
        assert_eq!(result.estimated_calendar_days, 2);
        assert_eq!(result.working_days, 2);
        // Nothing pushed it, so no queue position.
        assert_eq!(result.queue_position, 0);
        assert_eq!(result.burn_rate_per_day, 40.0);
    }

    #[test]
    fn a_weekend_desired_start_normalizes_to_the_next_monday() {
        // Saturday and Sunday both roll forward to the following Monday, and
        // midnight comes along — the desired start was midnight, so the
        // time-of-day is not observable here.
        for weekend in [MONDAY_2030 + 5, MONDAY_2030 + 6] {
            let result = calculate_schedule(default_pool(), &[], 40.0, Some(at(weekend)))
                .expect("a weekend start is schedulable");

            assert_eq!(result.scheduled_start, at(NEXT_MONDAY_2030));
            assert_eq!(result.queue_position, 0);
        }
    }

    #[test]
    fn a_time_of_day_survives_a_weekday_desired_start() {
        // `nextBusinessDay` only zeroes the clock inside the weekend-skip loop,
        // so a weekday keeps whatever time it arrived with. 2030-01-07 at
        // 05:00 is what `2030-01-07T00:00:00-05:00` is in UTC.
        let with_time = Instant::from_millis(days_after_epoch(MONDAY_2030) + 5 * 60 * 60 * 1000);
        let result = calculate_schedule(default_pool(), &[], 40.0, Some(with_time))
            .expect("a timed weekday start is schedulable");

        assert_eq!(result.scheduled_start, with_time);
        assert_eq!(
            result.scheduled_finish,
            Instant::from_millis(with_time.as_millis() + MS_PER_DAY)
        );
    }

    #[test]
    fn a_sub_millisecond_desired_start_keeps_its_milliseconds() {
        // `2030-01-08T13:45:30.500Z` — a Tuesday. The port's whole timestamp
        // surface is `i64` milliseconds, so this is exact rather than truncated.
        let tuesday = Instant::from_millis(
            days_after_epoch(MONDAY_2030 + 1) + 13 * 60 * 60 * 1000 + 45 * 60 * 1000 - 30 * 1000
                + 500,
        );
        let result = calculate_schedule(default_pool(), &[], 40.0, Some(tuesday))
            .expect("a sub-second start is schedulable");

        assert_eq!(result.scheduled_start, tuesday);
        assert_eq!(result.estimated_calendar_days, 1);
    }

    #[test]
    fn the_capacity_epsilon_admits_a_sum_one_ulp_over_the_budget() {
        // The epsilon's own fixture. burn = 3/5 = 0.6; the pre-existing job
        // burns 1 unit over 5 working days = 0.2/day; the new job needs 0.8
        // units over max(1, ceil(0.8/0.6)) = 2 days = 0.4/day. In exact
        // arithmetic 0.2 + 0.4 == 0.6 is a fit; in f64 the sum is
        // 0.6000000000000001, one ULP over, and the `+1e-6` is what admits it.
        let epsilon_pool = pool(3.0, 5);
        let allocs = [allocation("ulp", MONDAY_2030, MONDAY_2030 + 5, 1.0, 1.0)];

        let result = calculate_schedule(epsilon_pool, &allocs, 0.8, Some(at(MONDAY_2030)))
            .expect("the one-ulp overrun is within the epsilon");

        assert_eq!(result.scheduled_start, at(MONDAY_2030));
        assert_eq!(result.working_days, 2);
        assert_eq!(result.burn_rate_per_day, 0.6);
    }

    #[test]
    fn the_capacity_epsilon_does_not_admit_a_real_overrun() {
        // The control for the fixture above. burn = 40/day, the pre-existing
        // job takes all of it, and 40 more units cannot fit — so Monday is
        // rejected for real and the job moves to Wednesday.
        let allocs = [allocation("over", MONDAY_2030, WEDNESDAY_2030, 80.0, 80.0)];

        let result = calculate_schedule(default_pool(), &allocs, 40.0, Some(at(MONDAY_2030)))
            .expect("the job fits once the blocker clears");

        assert_eq!(result.scheduled_start, at(WEDNESDAY_2030));
        assert_eq!(result.queue_position, 1);
    }

    #[test]
    fn a_blocking_allocation_pushes_the_job_and_reports_the_queue() {
        // One job already running Monday-to-Wednesday in a pool that allows
        // only one concurrent job. Monday is blocked by concurrency, Wednesday
        // is free, and the queue reports one job ahead.
        let allocs = [allocation("a1", MONDAY_2030, WEDNESDAY_2030, 80.0, 80.0)];

        let result = calculate_schedule(default_pool(), &allocs, 40.0, Some(at(MONDAY_2030)))
            .expect("the job fits after the blocker");

        assert_eq!(result.scheduled_start, at(WEDNESDAY_2030));
        assert_eq!(
            result.scheduled_finish,
            Instant::from_millis(days_after_epoch(WEDNESDAY_2030 + 1))
        );
        assert_eq!(result.queue_position, 1);
    }

    #[test]
    fn the_queue_position_counts_every_allocation_still_finishing() {
        // Three allocations in a pool with room for three at once is a
        // concurrency block, not a capacity block, and the queue counts all
        // three regardless of which one actually blocked the day.
        let allocs = [
            allocation("a1", MONDAY_2030, WEDNESDAY_2030, 80.0, 80.0),
            allocation("a2", MONDAY_2030, WEDNESDAY_2030, 80.0, 80.0),
            allocation("a3", MONDAY_2030, WEDNESDAY_2030, 80.0, 80.0),
        ];
        let result = calculate_schedule(pool(200.0, 3), &allocs, 40.0, Some(at(MONDAY_2030)))
            .expect("the job fits after the blockers");

        assert_eq!(result.scheduled_start, at(WEDNESDAY_2030));
        assert_eq!(result.queue_position, 3);
    }

    #[test]
    fn remaining_units_of_zero_falls_back_to_required_units() {
        // `remainingUnits > 0` is strict, so a zero remaining balance charges
        // the job's full `requiredUnits` — the same as the negative case.
        let zeroed = [allocation("a1", MONDAY_2030, WEDNESDAY_2030, 40.0, 0.0)];
        let negative = [allocation("a1", MONDAY_2030, WEDNESDAY_2030, 40.0, -5.0)];

        let from_zero = calculate_schedule(pool(200.0, 5), &zeroed, 40.0, Some(at(MONDAY_2030)))
            .expect("schedulable");
        let from_negative =
            calculate_schedule(pool(200.0, 5), &negative, 40.0, Some(at(MONDAY_2030)))
                .expect("schedulable");

        assert_eq!(from_zero.scheduled_start, from_negative.scheduled_start);
        assert_eq!(from_zero.scheduled_start, at(WEDNESDAY_2030));
    }

    #[test]
    fn an_allocation_in_the_past_does_not_block() {
        // 2020, more than a decade before the desired start. The overlap test
        // is a half-open interval, so a job that finished long ago shares no
        // day with the new one.
        let allocs = [allocation(
            "old",
            PAST_MONDAY_2020,
            PAST_MONDAY_2020 + 2,
            80.0,
            80.0,
        )];

        let result = calculate_schedule(default_pool(), &allocs, 40.0, Some(at(MONDAY_2030)))
            .expect("a finished job cannot block");

        assert_eq!(result.scheduled_start, at(MONDAY_2030));
        assert_eq!(result.queue_position, 0);
    }

    #[test]
    fn an_indefinitely_blocking_allocation_resolves_in_one_jump() {
        // An allocation running to 2040-01-01 (a Sunday) blocks every day
        // until then. The search does not walk a day at a time — it jumps to
        // `earliestBlockingFinish`, then normalizes the Sunday forward to
        // Monday 2040-01-02. This is what keeps a long block cheap, and it is
        // why the 1000-iteration cap is not what makes the common case finite.
        let allocs = [allocation(
            "forever",
            MONDAY_2030,
            FAR_SUNDAY_2040,
            80.0,
            80.0,
        )];
        let result = calculate_schedule(default_pool(), &allocs, 40.0, Some(at(MONDAY_2030)))
            .expect("the job fits once the century-long blocker clears");

        assert_eq!(result.scheduled_start, at(FAR_SUNDAY_2040 + 1));
        assert_eq!(result.queue_position, 1);
        assert_eq!(result.working_days, 1);
    }

    #[test]
    fn a_huge_job_spans_calendar_days_not_working_days() {
        // 800 units over 40/day is 20 working days. They span four calendar
        // weeks because weekends do not count, and `candidateFinish` is stepped
        // one day past the last working day and then normalized — so the finish
        // is 2030-02-04, a Monday, and the span is 28 calendar days.
        let result = calculate_schedule(default_pool(), &[], 800.0, Some(at(MONDAY_2030)))
            .expect("a huge job is still schedulable");

        assert_eq!(result.working_days, 20);
        assert_eq!(result.scheduled_finish, at(MONDAY_2030 + 28));
        assert_eq!(result.estimated_calendar_days, 28);
    }

    #[test]
    fn a_job_smaller_than_one_day_still_takes_one_working_day() {
        // `max(1, ceil(39/40))` is 1, not 0. Without the floor the loop that
        // collects proposed working days would never terminate.
        let result = calculate_schedule(default_pool(), &[], 39.0, Some(at(MONDAY_2030)))
            .expect("a small job is schedulable");

        assert_eq!(result.working_days, 1);
        assert_eq!(result.estimated_calendar_days, 1);
    }

    #[test]
    fn a_pool_that_cannot_burn_refuses_the_request() {
        // `weeklyCapacity: 0` divides to a daily burn of 0, which is not
        // greater than 0 — refused before the search starts.
        let result = calculate_schedule(pool(0.0, 1), &[], 40.0, Some(at(MONDAY_2030)));

        assert_eq!(result.unwrap_err().code(), "insufficient_capacity");
    }

    #[test]
    fn a_zero_concurrency_limit_exhausts_the_loop() {
        // `maxConcurrentJobs: 0` — `concurrentJobsToday + 1 > 0` is true on
        // every candidate day, so nothing is ever valid and nothing ever jumps
        // the search forward. This is the one case where the
        // 1000-iteration cap is what stops the loop rather than
        // `earliestBlockingFinish`.
        let result = calculate_schedule(pool(200.0, 0), &[], 40.0, Some(at(MONDAY_2030)));

        assert_eq!(result.unwrap_err().code(), "insufficient_capacity");
    }

    #[test]
    fn a_tiny_pool_burns_a_fractional_daily_rate() {
        // 1 unit a week is 0.2/day, so even a 1-unit job needs all five
        // working days and finishes the following Monday.
        let result = calculate_schedule(pool(1.0, 1), &[], 1.0, Some(at(MONDAY_2030)))
            .expect("a tiny pool can still schedule");

        assert_eq!(result.burn_rate_per_day, 0.2);
        assert_eq!(result.working_days, 5);
        assert_eq!(result.scheduled_finish, at(NEXT_MONDAY_2030));
        assert_eq!(result.estimated_calendar_days, 7);
    }

    #[test]
    fn sequential_allocations_report_a_queue_without_a_single_blocker() {
        // Two allocations back to back, Monday-to-Wednesday and
        // Wednesday-to-Friday. A Monday desired start is blocked by the first,
        // so the job moves to Wednesday — which the second allocation then
        // blocks too. Both finish after the desired instant, so the queue
        // counts two jobs ahead even though only one blocked each day.
        let allocs = [
            allocation("a1", MONDAY_2030, WEDNESDAY_2030, 80.0, 80.0),
            allocation("a2", WEDNESDAY_2030, FRIDAY_2030, 80.0, 80.0),
        ];
        let result = calculate_schedule(default_pool(), &allocs, 40.0, Some(at(MONDAY_2030)))
            .expect("the job fits after the sequence");

        assert_eq!(result.scheduled_start, at(FRIDAY_2030));
        assert_eq!(result.queue_position, 2);
    }

    #[test]
    fn a_weekend_spanning_allocation_blocks_the_working_days_after_it() {
        // An allocation from Friday into the next Monday covers no working day
        // of its own, but the new job's Friday is inside its interval, so
        // concurrency blocks and the job moves to the following Monday.
        let allocs = [allocation("wk", FRIDAY_2030, NEXT_MONDAY_2030, 40.0, 40.0)];
        let result = calculate_schedule(default_pool(), &allocs, 40.0, Some(at(FRIDAY_2030)))
            .expect("the job fits after the weekend-spanning blocker");

        assert_eq!(result.scheduled_start, at(NEXT_MONDAY_2030));
        assert_eq!(result.queue_position, 1);
    }

    #[test]
    fn math_min_agrees_with_javascript_on_the_values_the_baseline_uses() {
        // The cap in `daily_units` and `neededDailyBurn` both take the smaller
        // of a rate and a quotient; `f64::min` and `Math.min` agree on these.
        assert_eq!(math_min(40.0, 20.0), 20.0);
        assert_eq!(math_min(40.0, 80.0), 40.0);
        assert_eq!(math_min(0.2, 0.2), 0.2);
    }

    #[test]
    fn math_round_rounds_halves_toward_positive_infinity() {
        // Rust's `round` is half-away-from-zero; JavaScript's is
        // half-toward-+Infinity. `estimatedCalendarDays` is the only caller,
        // and it only sees non-negative spans, but the helper is correct on the
        // whole domain so a future negative span cannot expose the difference.
        assert_eq!(math_round(2.5), 3.0);
        assert_eq!(math_round(-1.5), -1.0);
        assert_eq!(math_round(0.2), 0.0);
        assert_eq!(math_round(-0.2), -0.0);
        assert!(math_round(-0.2).is_sign_negative());
    }

    #[test]
    fn math_max_1_floors_a_span_to_one() {
        // The three `Math.max(1, ...)` floors all reduce to this, and this is
        // the one a `0`-day span would otherwise break.
        assert_eq!(math_max_1(0.0), 1.0);
        assert_eq!(math_max_1(0.4), 1.0);
        assert_eq!(math_max_1(20.0), 20.0);
    }
}
