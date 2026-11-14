//! Showing and entering a trade amount by its record's currency
//! (`docs/architecture/MULTI_CURRENCY.md`, rules 4 and 6).
//!
//! A trade amount is the integer a product, order, offer, purchase order,
//! supplier document or invoice stores. Its unit follows the record's
//! currency: an IDR record holds whole Rupiah, a record in any other currency
//! holds that currency's ISO minor unit. Ledger amounts are a different
//! contract (always book minor units) and are not formatted here.
//!
//! Pure and wasm-safe: the host (mail, checkout) and the slice (screens) share
//! this one rule.

use super::accounting::minor_units;

/// The currency of a record without a company or without a book.
pub const IDR: &str = "IDR";

/// Why an entered trade amount was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeAmountError {
    /// Not a plain number in the currency's notation.
    Malformed,
    /// More fraction digits than the currency has.
    TooManyDecimals,
    /// A negative amount where only zero or more is accepted.
    Negative,
    /// Does not fit the stored integer.
    Overflow,
    /// The currency is not in the pinned ISO catalogue.
    UnknownCurrency,
}

/// Fraction digits a trade amount in `currency` is shown and entered with:
/// none for IDR (whole Rupiah), the ISO exponent for any other catalogued
/// currency, `None` for an unknown code.
pub fn trade_decimals(currency: &str) -> Option<u8> {
    if currency == IDR {
        Some(0)
    } else {
        minor_units(currency)
    }
}

fn all_digits(digits: &str) -> bool {
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

fn grouped(digits: &str) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// Places the decimal point `decimals` digits from the right of an unsigned
/// digit string. Never drops a digit; leading zeros are reduced to one.
fn placed(digits: &str, decimals: u8) -> String {
    let decimals = usize::from(decimals);
    let trimmed = digits.trim_start_matches('0');
    let padded = format!("{trimmed:0>width$}", width = decimals + 1);
    if decimals == 0 {
        return padded;
    }
    let point = padded.len() - decimals;
    format!("{}.{}", &padded[..point], &padded[point..])
}

/// The bare number of a trade amount, without a currency code: `150000` for
/// IDR 150000 (no grouping), `19.99` for USD 1999, `1500` for JPY 1500,
/// `1.250` for KWD 1250, with a leading `-` when negative. `None` for an
/// unknown currency.
pub fn trade_decimal(currency: &str, amount: i64) -> Option<String> {
    let decimals = trade_decimals(currency)?;
    let sign = if amount < 0 { "-" } else { "" };
    Some(format!(
        "{sign}{}",
        placed(&amount.unsigned_abs().to_string(), decimals)
    ))
}

/// A trade amount as text.
///
/// - IDR: `Rp 150.000`, dot-grouped whole Rupiah; a negative amount is
///   `−Rp 150.000` (U+2212). This is the output every IDR screen had before
///   other currencies existed and must not change.
/// - Any other catalogued currency: `<CODE> <major>[.<minor>]` with the ISO
///   number of fraction digits and no grouping: `USD 19.99`, `JPY 1500`,
///   `KWD 1.250`; negative `USD -19.99`. This matches the ledger display.
/// - Unknown code: `<CODE> <integer> minor units`, so no digit is dropped or
///   reinterpreted.
pub fn format_trade(currency: &str, amount: i64) -> String {
    if currency == IDR {
        return format!(
            "{}Rp {}",
            if amount < 0 { "−" } else { "" },
            grouped(&amount.unsigned_abs().to_string())
        );
    }
    match trade_decimal(currency, amount) {
        Some(number) => format!("{currency} {number}"),
        None => format!("{currency} {amount} minor units"),
    }
}

/// [`format_trade`] for an unsigned amount carried as a decimal digit string
/// of any length (exact inventory values exceed `i64` in principle). `None`
/// when `digits` is not a non-empty run of ASCII digits.
pub fn format_trade_digits(currency: &str, digits: &str) -> Option<String> {
    if !all_digits(digits) {
        return None;
    }
    Some(if currency == IDR {
        format!("Rp {}", grouped(digits))
    } else {
        match trade_decimals(currency) {
            Some(decimals) => format!("{currency} {}", placed(digits, decimals)),
            None => format!("{currency} {digits} minor units"),
        }
    })
}

/// Reads an entered trade amount into the record's stored integer.
///
/// - IDR: a whole number of Rupiah, parsed exactly as the IDR forms always
///   did (surrounding whitespace ignored, no separators, no fraction).
/// - Any other catalogued currency: `[-]digits[.digits]` in major units, with
///   at most the ISO number of fraction digits; the result is minor units
///   (`19.99` USD is `1999`, `19.9` is `1990`, `1500` JPY is `1500`, `1.25`
///   KWD is `1250`). No grouping separators, no comma, no exponent.
///
/// The sign is returned as entered; use [`parse_trade_unsigned`] where a
/// negative amount is not accepted.
pub fn parse_trade(currency: &str, input: &str) -> Result<i64, TradeAmountError> {
    let input = input.trim();
    if currency == IDR {
        return input.parse::<i64>().map_err(|error| match error.kind() {
            std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow => {
                TradeAmountError::Overflow
            }
            _ => TradeAmountError::Malformed,
        });
    }
    let decimals = trade_decimals(currency).ok_or(TradeAmountError::UnknownCurrency)?;
    let (negative, unsigned) = match input.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, input),
    };
    let (whole, fraction) = match unsigned.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (unsigned, None),
    };
    if !all_digits(whole) || fraction.is_some_and(|f| !all_digits(f)) {
        return Err(TradeAmountError::Malformed);
    }
    let fraction = fraction.unwrap_or_default();
    if fraction.len() > usize::from(decimals) {
        return Err(TradeAmountError::TooManyDecimals);
    }
    let scale = 10_i64
        .checked_pow(u32::from(decimals))
        .ok_or(TradeAmountError::Overflow)?;
    let whole: i64 = whole.parse().map_err(|_| TradeAmountError::Overflow)?;
    let fraction: i64 = format!("{fraction:0<width$}", width = usize::from(decimals))
        .parse()
        .unwrap_or(0);
    let magnitude = whole
        .checked_mul(scale)
        .and_then(|v| v.checked_add(fraction))
        .ok_or(TradeAmountError::Overflow)?;
    Ok(if negative { -magnitude } else { magnitude })
}

/// [`parse_trade`] that refuses a negative amount.
pub fn parse_trade_unsigned(currency: &str, input: &str) -> Result<i64, TradeAmountError> {
    match parse_trade(currency, input)? {
        amount if amount < 0 => Err(TradeAmountError::Negative),
        amount => Ok(amount),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idr_keeps_the_whole_rupiah_display_every_screen_already_had() {
        assert_eq!(format_trade("IDR", 150_000), "Rp 150.000");
        assert_eq!(format_trade("IDR", 0), "Rp 0");
        assert_eq!(format_trade("IDR", 999), "Rp 999");
        assert_eq!(format_trade("IDR", 1_000), "Rp 1.000");
        assert_eq!(format_trade("IDR", 1_234_567), "Rp 1.234.567");
        assert_eq!(format_trade("IDR", -15_000), "−Rp 15.000");
        assert_eq!(
            format_trade("IDR", i64::MIN),
            "−Rp 9.223.372.036.854.775.808"
        );
        assert_eq!(
            format_trade_digits("IDR", "1000000000000000000").as_deref(),
            Some("Rp 1.000.000.000.000.000.000")
        );
        assert_eq!(format_trade_digits("IDR", "0").as_deref(), Some("Rp 0"));
        assert_eq!(trade_decimal("IDR", 150_000).as_deref(), Some("150000"));
    }

    #[test]
    fn other_currencies_show_their_iso_decimals_without_grouping() {
        assert_eq!(format_trade("USD", 1_999), "USD 19.99");
        assert_eq!(format_trade("USD", 5), "USD 0.05");
        assert_eq!(format_trade("USD", 0), "USD 0.00");
        assert_eq!(format_trade("USD", 123_456_789), "USD 1234567.89");
        assert_eq!(format_trade("USD", -1_999), "USD -19.99");
        assert_eq!(format_trade("USD", -5), "USD -0.05");
        assert_eq!(format_trade("JPY", 1_500), "JPY 1500");
        assert_eq!(format_trade("JPY", -1_500), "JPY -1500");
        assert_eq!(format_trade("KWD", 1_250), "KWD 1.250");
        assert_eq!(format_trade("KWD", 7), "KWD 0.007");
        assert_eq!(format_trade("USD", i64::MAX), "USD 92233720368547758.07");
        assert_eq!(format_trade("USD", i64::MIN), "USD -92233720368547758.08");
        assert_eq!(trade_decimal("USD", 1_999).as_deref(), Some("19.99"));
        assert_eq!(trade_decimal("KWD", -1_250).as_deref(), Some("-1.250"));
        assert_eq!(
            format_trade_digits("USD", "100000000000000000000").as_deref(),
            Some("USD 1000000000000000000.00")
        );
        assert_eq!(
            format_trade_digits("USD", "007").as_deref(),
            Some("USD 0.07")
        );
        assert_eq!(format_trade_digits("JPY", "0").as_deref(), Some("JPY 0"));
    }

    #[test]
    fn an_unknown_code_never_drops_or_moves_a_digit() {
        assert_eq!(trade_decimals("ZZZ"), None);
        assert_eq!(format_trade("ZZZ", 1_999), "ZZZ 1999 minor units");
        assert_eq!(format_trade("ZZZ", -1_999), "ZZZ -1999 minor units");
        assert_eq!(format_trade("", 5), " 5 minor units");
        assert_eq!(format_trade("idr", 5), "idr 5 minor units");
        assert_eq!(trade_decimal("ZZZ", 1), None);
        assert_eq!(
            format_trade_digits("ZZZ", "1999").as_deref(),
            Some("ZZZ 1999 minor units")
        );
        for bad in ["", "-1", "1.0", "1 000", "abc"] {
            assert_eq!(format_trade_digits("USD", bad), None, "{bad:?}");
            assert_eq!(format_trade_digits("IDR", bad), None, "{bad:?}");
        }
    }

    #[test]
    fn idr_input_stays_whole_rupiah() {
        assert_eq!(parse_trade("IDR", "150000"), Ok(150_000));
        assert_eq!(parse_trade("IDR", " 150000 "), Ok(150_000));
        assert_eq!(parse_trade("IDR", "0"), Ok(0));
        assert_eq!(parse_trade("IDR", "-5000"), Ok(-5_000));
        for bad in ["", "150.000", "150000.00", "1,5", "Rp 5", "1e3"] {
            assert_eq!(
                parse_trade("IDR", bad),
                Err(TradeAmountError::Malformed),
                "{bad:?}"
            );
        }
        assert_eq!(
            parse_trade("IDR", "9223372036854775808"),
            Err(TradeAmountError::Overflow)
        );
        assert_eq!(
            parse_trade_unsigned("IDR", "-1"),
            Err(TradeAmountError::Negative)
        );
        assert_eq!(parse_trade_unsigned("IDR", "150000"), Ok(150_000));
    }

    #[test]
    fn other_currencies_are_entered_in_major_units_and_stored_in_minor_units() {
        assert_eq!(parse_trade("USD", "19.99"), Ok(1_999));
        assert_eq!(parse_trade("USD", "19.9"), Ok(1_990));
        assert_eq!(parse_trade("USD", "19"), Ok(1_900));
        assert_eq!(parse_trade("USD", " 0.05 "), Ok(5));
        assert_eq!(parse_trade("USD", "0019.99"), Ok(1_999));
        assert_eq!(parse_trade("USD", "-19.99"), Ok(-1_999));
        assert_eq!(parse_trade("USD", "-0.00"), Ok(0));
        assert_eq!(parse_trade("JPY", "1500"), Ok(1_500));
        assert_eq!(parse_trade("KWD", "1.25"), Ok(1_250));
        assert_eq!(parse_trade("KWD", "1.250"), Ok(1_250));
        assert_eq!(parse_trade("KWD", "0.007"), Ok(7));
        assert_eq!(parse_trade("USD", "92233720368547758.07"), Ok(i64::MAX));
        for (currency, input) in [("USD", "19.999"), ("JPY", "1500.0"), ("KWD", "1.2500")] {
            assert_eq!(
                parse_trade(currency, input),
                Err(TradeAmountError::TooManyDecimals),
                "{currency} {input}"
            );
        }
        for bad in [
            "", "-", ".", "19.", ".99", "1,99", "1 999", "1e3", "+19.99", "--1", "1.2.3", "USD 5",
            "١٩",
        ] {
            assert_eq!(
                parse_trade("USD", bad),
                Err(TradeAmountError::Malformed),
                "{bad:?}"
            );
        }
        for overflow in [
            "92233720368547758.08",
            "99999999999999999999",
            "9223372036854775807",
        ] {
            assert_eq!(
                parse_trade("USD", overflow),
                Err(TradeAmountError::Overflow),
                "{overflow}"
            );
        }
        assert_eq!(
            parse_trade("ZZZ", "1"),
            Err(TradeAmountError::UnknownCurrency)
        );
        assert_eq!(
            parse_trade_unsigned("USD", "-0.01"),
            Err(TradeAmountError::Negative)
        );
        assert_eq!(parse_trade_unsigned("USD", "0.01"), Ok(1));
    }

    #[test]
    fn what_is_shown_can_be_entered_again() {
        for (currency, amount) in [
            ("USD", 1_999),
            ("JPY", 1_500),
            ("KWD", 1_250),
            ("IDR", 150_000),
        ] {
            let shown = trade_decimal(currency, amount).unwrap();
            assert_eq!(parse_trade(currency, &shown), Ok(amount), "{currency}");
        }
    }
}
