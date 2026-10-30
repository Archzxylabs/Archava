//! Rust-side tests for the port.
//!
//! These assert the *stated* behaviour of `packages/reference/src/rates.ts`
//! before the parity harness compares the two engines. A parity run is only
//! evidence if the port is independently shown to implement the documented
//! rules, so the arithmetic, the exclusive-`to` window, and the `None` cases
//! are each pinned here from the baseline's own comments.

use archava_rates::*;

const GARDEN: &str = "garden-twin";
const DELUXE: &str = "deluxe-valley";
const TREETOP: &str = "treetop-suite";
const VILLA: &str = "valley-pool-villa";

#[test]
fn a_base_rate_is_charged_on_a_weeknight_without_uplift() {
    // 2026-10-05 is a Monday, so it is not a weekend night.
    let rate = nightly_rate(GARDEN, "2026-10-05").expect("garden twin has a Monday rate");
    assert_eq!(rate.amount_minor, 850_000);
    assert_eq!(rate.currency, "IDR");
    assert_eq!(rate.night, "2026-10-05");
}

#[test]
fn the_weekend_uplift_is_added_then_stepped_not_multiplied() {
    // The baseline adds: round(850_000 * 15 / 100) = 127_500, then
    // round_to_step(977_500, 1_000) = 978_000. Multiplying would give
    // 977_499.9999999999 and a down-round on the tie, which is exactly the
    // defect the baseline's own comment records.
    let saturday = nightly_rate(GARDEN, "2026-10-10").expect("garden twin has a Saturday rate");
    assert_eq!(saturday.amount_minor, 978_000);

    let friday = nightly_rate(GARDEN, "2026-10-09").expect("garden twin has a Friday rate");
    assert_eq!(friday.amount_minor, 978_000);

    // The deluxe's tie lands the same way as the garden twin's, which the
    // baseline's float arithmetic could not guarantee.
    let deluxe = nightly_rate(DELUXE, "2026-10-10").expect("deluxe has a Saturday rate");
    // round(1_250_000 * 15 / 100) = 187_500; 1_437_500 -> 1_438_000.
    assert_eq!(deluxe.amount_minor, 1_438_000);
}

#[test]
fn only_the_two_stated_nights_carry_the_uplift() {
    for night in REFERENCE_NIGHTS {
        let rate = nightly_rate(GARDEN, night).expect("every fixture night has a rate");
        if is_weekend_night(night) {
            assert_eq!(
                rate.amount_minor, 978_000,
                "{night} should carry the uplift"
            );
        } else {
            assert_eq!(
                rate.amount_minor, 850_000,
                "{night} should not carry the uplift"
            );
        }
    }
    assert!(!is_weekend_night("2026-10-05"));
    assert!(!is_weekend_night("2026-10-11"));
}

#[test]
fn an_unknown_unit_or_night_is_absent_rather_than_zero() {
    assert!(nightly_rate("penthouse", "2026-10-05").is_none());
    assert!(nightly_rate(GARDEN, "2026-10-04").is_none());
    // '2026-10-11' is the checkout: a legal bound, but not a night.
    assert!(nightly_rate(GARDEN, "2026-10-11").is_none());
    assert!(rooms_built_of("penthouse").is_none());
    assert!(nightly_availability("penthouse", "2026-10-05").is_none());
}

#[test]
fn a_closed_night_sells_nothing_and_a_partial_night_reports_its_remainder() {
    // The garden wing is closed for a wedding party on the 8th.
    let closed = nightly_availability(GARDEN, "2026-10-08").expect("the night exists");
    assert!(!closed.available);
    assert_eq!(closed.units_left, 0);

    // A group booking left two of the deluxe's six rooms on the 9th.
    let partial = nightly_availability(DELUXE, "2026-10-09").expect("the night exists");
    assert!(partial.available);
    assert_eq!(partial.units_left, 2);

    // An untouched night reports what was built.
    let open = nightly_availability(DELUXE, "2026-10-05").expect("the night exists");
    assert!(open.available);
    assert_eq!(open.units_left, 6);
}

#[test]
fn the_window_end_is_exclusive_and_must_be_a_stated_bound() {
    // The canonical six-night stay: check-in 5th, checkout 11th.
    let nights = nights_between("2026-10-05", "2026-10-11");
    assert_eq!(nights, REFERENCE_NIGHTS.to_vec());

    // A checkout on a date the bounds do not name prices nothing.
    assert!(nights_between("2026-10-05", "2026-10-12").is_empty());
    assert!(nights_between("2026-10-05", "2026-11-01").is_empty());
    // A window that ends where it begins, or before, covers no nights.
    assert!(nights_between("2026-10-06", "2026-10-06").is_empty());
    assert!(nights_between("2026-10-09", "2026-10-07").is_empty());
    // One night's stay.
    assert_eq!(
        nights_between("2026-10-05", "2026-10-06"),
        vec!["2026-10-05"]
    );
}

#[test]
fn a_stay_prices_every_night_and_taxes_the_subtotal() {
    let quote = stay_quote(GARDEN, "2026-10-05", "2026-10-11").expect("the full window prices");
    assert_eq!(quote.nights.len(), 6);
    // Four weeknights at 850_000 plus two weekend nights at 978_000.
    let expected_subtotal = 4 * 850_000 + 2 * 978_000;
    assert_eq!(quote.subtotal_minor, expected_subtotal);
    // PPN 11%, rounded.
    let expected_tax = (expected_subtotal * 11) / 100;
    assert_eq!(quote.tax_minor, expected_tax);
    assert_eq!(quote.total_minor, expected_subtotal + expected_tax);
}

#[test]
fn an_unavailable_stay_still_has_a_price() {
    // The villa is closed on the 10th, so it is not sellable across the window,
    // but its price is still a real question.
    assert!(
        stay_quote(VILLA, "2026-10-05", "2026-10-11").is_some(),
        "the price of an unsellable stay is still asked"
    );

    let availability = stay_availability(VILLA, "2026-10-05", "2026-10-11").expect("reported");
    assert!(
        !availability.every_night_available,
        "the closed villa is not sellable"
    );
}

#[test]
fn bookable_excludes_closed_units_and_cheapest_orders_by_total() {
    // The treetop is closed on the 7th and the villa on the 10th, and the
    // garden twin is closed on *both* the 8th and 9th, so across the whole
    // six-night window only the deluxe is sellable every night.
    let bookable = bookable_stays("2026-10-05", "2026-10-11");
    let ids: Vec<&str> = bookable.iter().map(|stay| stay.unit_id.as_str()).collect();
    assert_eq!(ids, vec![DELUXE]);

    let cheapest = cheapest_bookable("2026-10-05", "2026-10-11");
    let totals: Vec<i64> = cheapest.iter().map(|stay| stay.total_minor).collect();
    assert_eq!(totals.len(), 1);
    assert_eq!(cheapest[0].unit_id, DELUXE);

    // A window that avoids the treetop's only closed night (the 7th) sells it,
    // which is what makes the closed-night list the interesting shape: it is
    // per-night, not per-unit.
    let around_closure = bookable_stays("2026-10-08", "2026-10-11");
    let around_ids: Vec<&str> = around_closure
        .iter()
        .map(|stay| stay.unit_id.as_str())
        .collect();
    assert!(
        around_ids.contains(&TREETOP),
        "the treetop is sellable when its closed night is outside the window, got {around_ids:?}"
    );
    // The garden twin is closed on two nights of that window, so it is absent
    // from it despite being available on the earlier nights.
    assert!(
        !around_ids.contains(&GARDEN),
        "the garden twin is closed on two nights of this window, got {around_ids:?}"
    );
}

#[test]
fn the_stated_window_is_bookable_for_at_least_one_unit() {
    // The demo's own listing page must not be empty. The baseline's comment
    // records that an unnamed bound once emptied every listing page, so this is
    // the regression that guards it.
    assert!(!bookable_stays(REFERENCE_WINDOW_FROM, REFERENCE_WINDOW_TO).is_empty());
}

#[test]
fn a_sellable_stay_serializes_with_the_quotes_own_fields_flattened() {
    // The baseline builds each entry as `{ ...quote, availability }`, so the
    // money fields sit at the top level and `availability` comes after them. The
    // first port nested the quote under a `quote` key instead, which the parity
    // harness reported as 42 divergent bookable windows — and which no unit test
    // would have caught, because a nested quote is a perfectly reasonable shape
    // until something outside the crate reads it.
    //
    // This asserts on the serialized text, not the struct, for the same reason
    // `lib.rs`'s bridge tests do: the key order and the flatness are the wire
    // contract, and a struct comparison would pass on either layout.
    let text = serde_json::to_string(&cheapest_bookable("2026-10-05", "2026-10-11"))
        .expect("a sellable stay serializes");
    assert!(
        text.contains("\"totalMinor\":8742360"),
        "the total should be a top-level key, got {text}"
    );
    assert!(
        text.contains("\"availability\":{"),
        "availability should follow the quote's fields, got {text}"
    );
    assert!(
        !text.contains("\"quote\""),
        "the quote must not be nested under a `quote` key, got {text}"
    );
}
