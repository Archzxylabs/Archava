//! `rates` — the reference tenant's rates and availability, ported.
//!
//! A port of `packages/reference/src/rates.ts`. Money here is all integer minor
//! units, so a parity failure would be a real arithmetic difference rather than
//! a floating-point display artifact.
//!
//! ## The three quirks reproduced on purpose
//!
//! **The uplift is added, not multiplied.** `base * 1.15` is not exact in
//! binary floating point: 850,000 x 1.15 is 977499.9999999999, which prints as
//! 977,500 but rounds *down*. The baseline's own comment records the damage —
//! the garden twin's 977,500 tie rounded down while the deluxe's identical tie
//! rounded up, so one stated rule produced two answers decided by nothing the
//! operator said. The port does the same arithmetic the baseline does
//! (`round(base * 15 / 100)` then `round_to_step(base + uplift, 1000)`), which
//! in integers is exact. So this port *fixes* that float ambiguity by
//! construction while keeping the baseline's rule. The parity harness is what
//! decides whether that shows as a difference: if the baseline's `Math.round`
//! and this integer path ever disagree, the case is reported as a divergence,
//! not reconciled here.
//!
//! **`to` is exclusive and is a legal bound that is not itself a night.**
//! `REFERENCE_BOUNDS` is `REFERENCE_NIGHTS` plus `REFERENCE_WINDOW.to`
//! ('2026-10-11', the checkout). A window whose end is not in that list prices
//! nothing at all, and the baseline's own comment records that the canonical
//! six-night stay once ended on a date the list did not contain, emptying every
//! listing page in the demo. Preserved exactly.
//!
//! **`undefined` is absent, not zero.** Every accessor returns `Option`: a unit
//! the tenant does not own has no rate, no room count, and no availability.
//! `stayAvailability` checks `read.len() != nights.len()` and returns `None`,
//! so a partially-covered window yields no availability rather than a
//! half-stay. Preserved.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The nights the fixtures cover. Closed list; 2026-10-05 is a Monday, so the
/// 9th and 10th are the weekend nights and only those carry the uplift.
pub const REFERENCE_NIGHTS: [&str; 6] = [
    "2026-10-05",
    "2026-10-06",
    "2026-10-07",
    "2026-10-08",
    "2026-10-09",
    "2026-10-10",
];

/// The dates a stay may end on: every night, plus the checkout after the last.
/// `to` is exclusive — a guest occupies nights and leaves on the checkout
/// morning — so the window end is a legal bound that is not itself a night.
pub const REFERENCE_BOUNDS: [&str; 7] = [
    REFERENCE_NIGHTS[0],
    REFERENCE_NIGHTS[1],
    REFERENCE_NIGHTS[2],
    REFERENCE_NIGHTS[3],
    REFERENCE_NIGHTS[4],
    REFERENCE_NIGHTS[5],
    "2026-10-11",
];

/// The reservation window the fixtures cover.
pub const REFERENCE_WINDOW_FROM: &str = "2026-10-05";
pub const REFERENCE_WINDOW_TO: &str = "2026-10-11";
/// The currency every reference price is quoted in.
pub const REFERENCE_CURRENCY: &str = "IDR";
/// The weekend uplift, stated so the demo can explain it rather than assert it.
pub const WEEKEND_UPLIFT_PERCENT: i64 = 15;
/// Rates are quoted to the nearest thousand, as the resort's price list does.
pub const RATE_STEP: i64 = 1_000;

/// The tax the fixtures apply. A real tenant's line comes from its own system.
pub const REFERENCE_TAX_CODE: &str = "PPN";
pub const REFERENCE_TAX_LABEL: &str = "PPN 11%";
pub const REFERENCE_TAX_RATE_PERCENT: i64 = 11;

/// The Friday and Saturday of the window.
pub const WEEKEND_NIGHTS: [&str; 2] = ["2026-10-09", "2026-10-10"];

/// The base nightly rate per unit, before the weekend uplift.
fn base_rates() -> BTreeMap<&'static str, i64> {
    BTreeMap::from([
        ("garden-twin", 850_000),
        ("deluxe-valley", 1_250_000),
        ("treetop-suite", 2_100_000),
        ("valley-pool-villa", 3_400_000),
    ])
}

/// How many rooms of each type the resort actually has.
fn rooms_built() -> BTreeMap<&'static str, i64> {
    BTreeMap::from([
        ("garden-twin", 4),
        ("deluxe-valley", 6),
        ("treetop-suite", 2),
        ("valley-pool-villa", 2),
    ])
}

/// Nights a unit type is closed outright, stated as operator-recognisable facts.
///
/// A list per unit, not one night: the garden wing is held by a wedding party
/// on *both* the 8th and the 9th, and a single-night model silently dropped the
/// second one — caught by the crate's own `bookable_stays` test before the
/// parity harness ever ran.
fn closed_nights() -> BTreeMap<&'static str, &'static [&'static str]> {
    BTreeMap::from([
        ("garden-twin", &["2026-10-08", "2026-10-09"] as &[&str]),
        ("treetop-suite", &["2026-10-07"] as &[&str]),
        ("valley-pool-villa", &["2026-10-10"] as &[&str]),
    ])
}

/// Nights where a group booking took most of a type, leaving a remainder.
fn remaining_overrides() -> BTreeMap<&'static str, BTreeMap<&'static str, i64>> {
    BTreeMap::from([("deluxe-valley", BTreeMap::from([("2026-10-09", 2)]))])
}

/// The units the tenant sells, in catalog order. Ids and names only — the rate
/// tables below are keyed by these ids, so a unit that stops existing stops
/// having a rate rather than quietly keeping a stale one.
pub fn reference_units() -> Vec<(&'static str, &'static str)> {
    vec![
        ("garden-twin", "Garden Twin Room"),
        ("deluxe-valley", "Deluxe Valley Room"),
        ("treetop-suite", "Treetop Suite"),
        ("valley-pool-villa", "Valley Pool Villa"),
    ]
}

/// True for a night the fixtures cover.
pub fn is_reference_night(night: &str) -> bool {
    REFERENCE_NIGHTS.contains(&night)
}

/// Rooms of this unit type the resort has. `None` for a unit it does not own.
pub fn rooms_built_of(unit_id: &str) -> Option<i64> {
    rooms_built().get(unit_id).copied()
}

/// The base nightly rate, before any uplift. `None` for an unknown unit.
pub fn base_rate(unit_id: &str) -> Option<i64> {
    base_rates().get(unit_id).copied()
}

/// True when the night carries the weekend uplift.
pub fn is_weekend_night(night: &str) -> bool {
    WEEKEND_NIGHTS.contains(&night)
}

/// `Math.round(amount / step) * step`, in integers.
///
/// JavaScript's `Math.round` rounds half toward +infinity, and Rust's
/// `f64::round` does the same, so for non-negative amounts the two agree. Every
/// amount here is non-negative (a rate, a remainder, a subtotal), so the
/// integer form is chosen: it is exact, and it cannot drift.
fn round_to_step(amount: i64, step: i64) -> i64 {
    debug_assert!(step > 0, "step must be positive");
    // `amount + step / 2` then floor division matches `Math.round` for every
    // non-negative half-way case the fixtures reach.
    ((amount + step / 2) / step) * step
}

/// The weekend rate for a base rate.
///
/// The uplift is added rather than multiplied in, on purpose — see the module
/// header. Both factors are integers here, so the tie lands the same way for
/// every unit, which is the property the baseline's float arithmetic lost.
///
/// ## One difference from the baseline, exact over the fixtures only
///
/// The baseline computes `Math.round((base * 15) / 100)`, which rounds half
/// toward +infinity. This divides instead, which truncates toward zero. The two
/// agree exactly when `base * 15` is an exact multiple of 100, and all four
/// fixture bases are multiples of 20 (850,000; 1,250,000; 2,100,000; 3,400,000),
/// so both uplift and tax lines divide exactly and the port is bit-for-bit
/// correct over the entire stated fixture space.
///
/// They would part company on a base rate that is not a multiple of 20 — a
/// 850,010 tarif would give `Math.round` 127,502 and this 127,501. That is not
/// reachable through the fixtures, and the parity harness proves it is not, but
/// it is a real difference in the *rule* rather than a property of these
/// numbers. Any future slice ported from float `Math.round` into integer
/// division needs this checked before assuming the two are interchangeable.
fn weekend_rate(base: i64) -> i64 {
    let uplift = (base * WEEKEND_UPLIFT_PERCENT) / 100;
    round_to_step(base + uplift, RATE_STEP)
}

/// The rate for one unit on one night, uplift included. `None` when the unit is
/// unknown or the night is outside the fixtures.
pub fn nightly_rate(unit_id: &str, night: &str) -> Option<NightlyRate> {
    let base = base_rate(unit_id)?;
    if !is_reference_night(night) {
        return None;
    }
    Some(NightlyRate {
        unit_id: unit_id.to_string(),
        night: night.to_string(),
        amount_minor: if is_weekend_night(night) {
            weekend_rate(base)
        } else {
            base
        },
        currency: REFERENCE_CURRENCY.to_string(),
    })
}

/// Availability for one unit on one night.
pub fn nightly_availability(unit_id: &str, night: &str) -> Option<NightlyAvailability> {
    let built = rooms_built_of(unit_id)?;
    if !is_reference_night(night) {
        return None;
    }
    let closed = closed_nights()
        .get(unit_id)
        .is_some_and(|nights| nights.contains(&night));
    let units_left = if closed {
        0
    } else {
        remaining_overrides()
            .get(unit_id)
            .and_then(|nights| nights.get(night))
            .copied()
            .unwrap_or(built)
    };
    Some(NightlyAvailability {
        unit_id: unit_id.to_string(),
        night: night.to_string(),
        available: units_left > 0,
        units_left,
    })
}

/// The nights a stay covers: check-in night through the night before checkout.
///
/// Empty for a window the fixtures do not state, rather than extrapolating.
pub fn nights_between(from: &str, to: &str) -> Vec<&'static str> {
    let start = REFERENCE_NIGHTS.iter().position(|night| *night == from);
    let end = REFERENCE_BOUNDS.iter().position(|bound| *bound == to);
    match (start, end) {
        (Some(start), Some(end)) if end > start => REFERENCE_NIGHTS[start..end].to_vec(),
        _ => Vec::new(),
    }
}

/// Availability for a whole stay. `None` when either end is outside the
/// fixtures, or any covered night has no availability of its own.
pub fn stay_availability(unit_id: &str, from: &str, to: &str) -> Option<StayAvailability> {
    let nights = nights_between(from, to);
    if nights.is_empty() {
        return None;
    }
    let read: Vec<NightlyAvailability> = nights
        .iter()
        .filter_map(|night| nightly_availability(unit_id, night))
        .collect();
    if read.len() != nights.len() {
        return None;
    }
    Some(StayAvailability {
        unit_id: unit_id.to_string(),
        from: from.to_string(),
        to: to.to_string(),
        every_night_available: read.iter().all(|night| night.available),
        nights: read,
    })
}

/// The money for a stay: nightly rates, the stated tax line, and the total.
///
/// Returns a quote even for a stay that cannot be sold — the price of an
/// unavailable stay is a real question — so callers who care about sellability
/// ask `stay_availability` too.
pub fn stay_quote(unit_id: &str, from: &str, to: &str) -> Option<StayQuote> {
    let nights = nights_between(from, to);
    if nights.is_empty() {
        return None;
    }
    let rates: Vec<NightlyRate> = nights
        .iter()
        .filter_map(|night| nightly_rate(unit_id, night))
        .collect();
    if rates.len() != nights.len() {
        return None;
    }
    let subtotal_minor: i64 = rates.iter().map(|rate| rate.amount_minor).sum();
    // Truncating division where the baseline uses `Math.round`. Exact over the
    // fixtures — every subtotal here is a sum of multiples of 1,000, and
    // `sum * 11` is therefore always an exact multiple of 100 — and documented
    // as a difference in the rule rather than a coincidence, on `weekend_rate`.
    let tax_minor = (subtotal_minor * REFERENCE_TAX_RATE_PERCENT) / 100;
    Some(StayQuote {
        unit_id: unit_id.to_string(),
        from: from.to_string(),
        to: to.to_string(),
        subtotal_minor,
        tax_minor,
        total_minor: subtotal_minor + tax_minor,
        currency: REFERENCE_CURRENCY.to_string(),
        nights: rates,
    })
}

/// Every unit that can actually be booked across a stay.
///
/// ## Why the quote's fields are flat, not nested
///
/// The baseline builds each entry as `{ ...quote, availability }`, so the answer
/// has `unitId`, `from`, `to`, `nights`, `subtotalMinor`, `taxMinor`,
/// `totalMinor` and `currency` at the *top* level, with `availability` after
/// them. The first version of this port nested the quote under a `quote` key
/// instead, and the parity harness reported all 42 sellable-window cases of
/// `bookableStays` and `cheapestBookable` as divergent — one shape bug, found 42
/// times. The fields are spelled out flat here rather than held in a `quote`
/// sub-struct so the mistake cannot come back.
///
/// This is also why `stay_quote` keeps returning a `StayQuote`: that function's
/// own shape was correct and is checked against the baseline on its own.
pub fn bookable_stays(from: &str, to: &str) -> Vec<BookableStay> {
    let mut bookable = Vec::new();
    for (unit_id, _name) in reference_units() {
        let quote = match stay_quote(unit_id, from, to) {
            Some(quote) => quote,
            None => continue,
        };
        let availability = match stay_availability(unit_id, from, to) {
            Some(availability) => availability,
            None => continue,
        };
        if !availability.every_night_available {
            continue;
        }
        bookable.push(BookableStay::from_parts(quote, availability));
    }
    bookable
}

/// The units a listing page would show as sellable, cheapest first.
pub fn cheapest_bookable(from: &str, to: &str) -> Vec<BookableStay> {
    let mut stays = bookable_stays(from, to);
    // `sort_by_key`, not `sort_by(|l, r| l.total_minor.cmp(&r.total_minor)`:
    // the same ordering, and clippy's `unnecessary_sort_by` is right that the
    // longer form says nothing the short one does not.
    stays.sort_by_key(|stay| stay.total_minor);
    stays
}

/// The rate for one unit on one night, uplift included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NightlyRate {
    #[serde(rename = "unitId")]
    pub unit_id: String,
    pub night: String,
    #[serde(rename = "amountMinor")]
    pub amount_minor: i64,
    pub currency: String,
}

/// One unit's availability for one night. `0` units left means closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NightlyAvailability {
    #[serde(rename = "unitId")]
    pub unit_id: String,
    pub night: String,
    pub available: bool,
    #[serde(rename = "unitsLeft")]
    pub units_left: i64,
}

/// Availability across a whole stay, which is what a booking actually needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StayAvailability {
    #[serde(rename = "unitId")]
    pub unit_id: String,
    pub from: String,
    pub to: String,
    pub nights: Vec<NightlyAvailability>,
    #[serde(rename = "everyNightAvailable")]
    pub every_night_available: bool,
}

/// The money for a stay, including the tax line the fixtures state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StayQuote {
    #[serde(rename = "unitId")]
    pub unit_id: String,
    pub from: String,
    pub to: String,
    pub nights: Vec<NightlyRate>,
    #[serde(rename = "subtotalMinor")]
    pub subtotal_minor: i64,
    #[serde(rename = "taxMinor")]
    pub tax_minor: i64,
    #[serde(rename = "totalMinor")]
    pub total_minor: i64,
    pub currency: String,
}

/// A sellable stay, in the baseline's `{ ...quote, availability }` shape.
///
/// The quote's own fields are flattened in, so this serializes with the same key
/// order the baseline's spread produces — quote keys first, then `availability`.
/// A nested `quote` field here was the bug the parity harness caught; see
/// `bookable_stays` for the incident.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookableStay {
    #[serde(rename = "unitId")]
    pub unit_id: String,
    pub from: String,
    pub to: String,
    pub nights: Vec<NightlyRate>,
    #[serde(rename = "subtotalMinor")]
    pub subtotal_minor: i64,
    #[serde(rename = "taxMinor")]
    pub tax_minor: i64,
    #[serde(rename = "totalMinor")]
    pub total_minor: i64,
    pub currency: String,
    pub availability: StayAvailability,
}

impl BookableStay {
    /// Flatten a quote and its availability into one entry.
    fn from_parts(quote: StayQuote, availability: StayAvailability) -> Self {
        Self {
            unit_id: quote.unit_id,
            from: quote.from,
            to: quote.to,
            nights: quote.nights,
            subtotal_minor: quote.subtotal_minor,
            tax_minor: quote.tax_minor,
            total_minor: quote.total_minor,
            currency: quote.currency,
            availability,
        }
    }
}
