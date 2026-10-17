//! Numeric helpers that replicate JavaScript's `Math` semantics exactly.
//!
//! The pricing engine is an f64 engine by construction: `rounding.ts` divides
//! and multiplies doubles with no decimal library anywhere in the slice, so a
//! port that swapped in `rust_decimal` or integer minor units would inherit
//! different rounding and stop being comparable to the baseline. These helpers
//! exist for the two places where Rust's `f64` methods and JavaScript's `Math`
//! disagree. Both differences are real and both are covered by tests, because
//! a silent divergence here moves money.

/// `Math.max(a, b)`.
///
/// Three behaviors a naive `a.max(b)` gets wrong:
/// 1. Either operand being `NaN` makes the result `NaN`. Rust's `f64::max`
///    propagates the *non*-NaN operand instead, so `0.0_f64.max(NaN)` is `0.0`
///    where `Math.max(0, NaN)` is `NaN`.
/// 2. Ties return the first argument, so `Math.max(0, -0)` is `0` and
///    `Math.max(-0, 0)` is `-0`.
/// 3. `Math.max(0, -5)` is `0`, which the ordering rule already handles.
pub fn math_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if b > a {
        b
    } else {
        a
    }
}

/// `Math.round(x)`: halves round toward **+Infinity**, not away from zero.
///
/// Rust's `f64::round()` is half-away-from-zero, so the two agree on every
/// non-negative input and disagree on every negative half:
/// `Math.round(-1.5)` is `-1` while `(-1.5_f64).round()` is `-2`. The engine
/// only reaches this with non-negative amounts (zod's `.nonnegative()` guards
/// every pricebook money field), but the helper is correct on the whole domain
/// so a future fixture cannot expose the difference.
///
/// The negative-zero case is reproduced too: `Math.round(-0.5)` is `-0`, and
/// `-0 * 50` stays `-0`, which `Object.is` can observe.
pub fn math_round(x: f64) -> f64 {
    if !x.is_finite() {
        // `Math.round(NaN)` is `NaN`, `Math.round(Infinity)` is `Infinity`.
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

/// The rounding increments `rounding.ts` hardcodes.
///
/// These are deliberately NOT read from the pricebook. The TypeScript engine
/// keeps two independent sources: `rounding.ts` fixes these constants for the
/// arithmetic, while `engine.ts` separately reads `currency_rounding` only to
/// report `roundedWithIncrement`. The port reproduces both sources rather than
/// unifying them, so a pricebook whose `currency_rounding` disagrees with these
/// constants produces the same self-contradictory quote the baseline does.
/// See `Region::rounding_increment` for the reporting side.
pub const GLOBAL_ROUNDING_INCREMENT: f64 = 50.0;
pub const ID_ROUNDING_INCREMENT: f64 = 100_000.0;

/// `roundCurrency(region, amount)` — a line-for-line port of `rounding.ts`.
///
/// Rounding happens exactly once, on the final one-time total, and never on
/// line items. Non-finite amounts short-circuit and are returned unchanged,
/// which is how an `Infinity` integration count reaches the output as
/// `Infinity` instead of throwing.
pub fn round_currency(region: RegionCode, amount: f64) -> f64 {
    let increment = match region {
        RegionCode::Global => GLOBAL_ROUNDING_INCREMENT,
        RegionCode::Id => ID_ROUNDING_INCREMENT,
        // `rounding.ts` selects with `region === 'ID' ? id : global`, so an
        // absent region rounds by the global increment. Unreachable in practice
        // — `require_region` rejects it before any amount is rounded.
        RegionCode::Absent => GLOBAL_ROUNDING_INCREMENT,
    };
    if !amount.is_finite() {
        return amount;
    }
    math_round(amount / increment) * increment
}

use crate::pricebook::RegionCode;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_max_propagates_nan_like_javascript() {
        assert!(math_max(0.0, f64::NAN).is_nan());
        assert!(math_max(f64::NAN, 0.0).is_nan());
        assert!(math_max(f64::NAN, f64::NAN).is_nan());
    }

    #[test]
    fn math_max_returns_the_first_argument_on_ties() {
        assert_eq!(math_max(0.0, -0.0), 0.0);
        assert!(math_max(-0.0, 0.0).is_sign_negative());
        assert_eq!(math_max(0.0, -5.0), 0.0);
        assert_eq!(math_max(-5.0, 0.0), 0.0);
    }

    #[test]
    fn math_round_rounds_halves_toward_positive_infinity() {
        assert_eq!(math_round(-1.5), -1.0);
        assert_eq!(math_round(-2.5), -2.0);
        assert_eq!(math_round(2.5), 3.0);
        assert_eq!(math_round(1.5), 2.0);
        assert_eq!(math_round(-0.5), -0.0);
        assert!(math_round(-0.5).is_sign_negative());
        assert_eq!(math_round(-0.2), -0.0);
        assert_eq!(math_round(0.2), 0.0);
    }

    #[test]
    fn math_round_passes_through_non_finite() {
        assert!(math_round(f64::NAN).is_nan());
        assert_eq!(math_round(f64::INFINITY), f64::INFINITY);
        assert_eq!(math_round(f64::NEG_INFINITY), f64::NEG_INFINITY);
    }

    #[test]
    fn rounds_global_amounts_to_the_nearest_fifty_half_up() {
        assert_eq!(round_currency(RegionCode::Global, 1.0), 0.0);
        assert_eq!(round_currency(RegionCode::Global, 24.0), 0.0);
        assert_eq!(round_currency(RegionCode::Global, 25.0), 50.0);
        assert_eq!(round_currency(RegionCode::Global, 26.0), 50.0);
        assert_eq!(round_currency(RegionCode::Global, 75.0), 100.0);
        assert_eq!(round_currency(RegionCode::Global, 8824.0), 8800.0);
        // The documented reference example from `rounding.ts`.
        assert_eq!(round_currency(RegionCode::Global, 8825.0), 8850.0);
        assert_eq!(round_currency(RegionCode::Global, 8851.0), 8850.0);
        assert_eq!(round_currency(RegionCode::Global, 0.4), 0.0);
    }

    #[test]
    fn rounds_indonesian_amounts_to_the_nearest_hundred_thousand() {
        assert_eq!(round_currency(RegionCode::Id, 49_999.0), 0.0);
        assert_eq!(round_currency(RegionCode::Id, 50_000.0), 100_000.0);
        assert_eq!(round_currency(RegionCode::Id, 150_000.0), 200_000.0);
        assert_eq!(round_currency(RegionCode::Id, 149_999.0), 100_000.0);
        // 24730000 / 100000 is 247.3, which lands safely away from a half.
        assert_eq!(round_currency(RegionCode::Id, 24_730_000.0), 24_700_000.0);
    }

    #[test]
    fn negative_and_non_finite_amounts_behave_like_the_baseline() {
        assert_eq!(round_currency(RegionCode::Global, -25.0), -0.0);
        assert!(round_currency(RegionCode::Global, -75.0) == -50.0);
        assert!(round_currency(RegionCode::Global, f64::NAN).is_nan());
        assert_eq!(
            round_currency(RegionCode::Global, f64::INFINITY),
            f64::INFINITY
        );
        assert_eq!(
            round_currency(RegionCode::Id, f64::NEG_INFINITY),
            f64::NEG_INFINITY
        );
    }
}
