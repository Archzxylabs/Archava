//! `archava-capacity` — the capacity scheduling math, ported from TypeScript.
//!
//! This crate is R02's third deliverable. PRD_FULL_RUST.md:656 names R02 as
//! "Pricing, deployment/assembly planning dan capacity math"; the pricing and
//! assembly crates are already in this workspace, and this is the third row.
//!
//! The source of truth is `packages/capacity-pg/src/repository.ts`:
//!
//! | baseline | port |
//! |---|---|
//! | `nextBusinessDay` (`:108-115`) | [`calendar::next_business_day`] |
//! | `countWorkingDays` (`:96-106`) | [`calendar::count_working_days`] |
//! | `calculateSchedule` (`:117-231`) | [`schedule::calculate_schedule`] |
//!
//! ## What this crate is, and what it is not
//!
//! It is the *math*. It takes a pool's capacity, the allocations already
//! running, a unit count and a desired start, and returns a schedule. It does
//! not talk to PostgreSQL, does not parse ISO strings, and does not decide
//! anything about a job's state — all of that is the repository and service
//! layers above it, whose behavior is deliberately not reproduced here (see
//! [`error`] for why only one of the baseline's eight error codes is modelled).
//!
//! ## No date crate
//!
//! `chrono`, `jiff` and `time` are all absent, by design. The baseline has no
//! date library either — it is `new Date()`, `getUTCDay()` and
//! `+ 24 * 60 * 60 * 1000`. The port models an instant as a signed `i64`
//! millisecond count and implements the three calendar primitives over it. See
//! `Cargo.toml` for the two reasons (the baseline's arithmetic, and R01's
//! wasm gate) and `calendar.rs` for each primitive.
//!
//! ## How fidelity is checked
//!
//! Not by reading the code and agreeing with it. The reference values in
//! `tests/reference.json` were produced by running the baseline's own
//! `calculateSchedule` over a 37-case fixture set, transcript into a committed
//! generator — `tools/rust/capacity-probe.mjs` — so the suite asserts what
//! TypeScript *actually returned*, including the cases where that is surprising.
//! Regenerate with `node tools/rust/capacity-probe.mjs`. The one fixture that
//! depends on the wall clock (`no-desired-start`, which has no desired start and
//! so reads `new Date()`) is excluded from the comparison loop and the reason is
//! recorded in `tests/capacity.rs` rather than papered over with a tolerance.

pub mod calendar;
pub mod error;
pub mod schedule;

pub use calendar::{
    count_working_days, next_business_day, CalendarError, Instant, MAX_HORIZON_DAYS,
    MAX_SUPPORTED_MILLIS, MIN_SUPPORTED_MILLIS, MS_PER_DAY,
};
pub use error::{CapacityError, CapacityErrorKind};
pub use schedule::{
    calculate_schedule, ActiveAllocation, PoolCapacity, ScheduleResult, CAPACITY_EPSILON,
    MAX_ITERATIONS,
};
