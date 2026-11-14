//! Tax on documents: integer fractions and the two price bases
//! (`docs/architecture/TAX.md`, rules 4 and 5). A rate and a base fraction are
//! a company's own data; nothing here knows the law of any country.
//!
//! Amounts are whole document units (`i64`), intermediates are `i128`, and no
//! floating point is used.
use super::{accounting::MAX_MINOR, purchasing::ValidationError};
use serde::{Deserialize, Serialize};

/// The largest denominator a stored fraction may have.
pub const MAX_DENOMINATOR: i32 = 1_000_000;

/// Which side of trade a tax code is for.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Tax charged on sales; its control account is a liability.
    Output,
    /// Tax paid on purchases; its control account is an asset.
    Input,
}
impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Output => "output",
            Self::Input => "input",
        }
    }
}

/// `numerator/denominator` of integers; 11% is `11/100`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fraction {
    pub numerator: i32,
    pub denominator: i32,
}
impl Fraction {
    pub const ONE: Self = Self {
        numerator: 1,
        denominator: 1,
    };
    pub fn one() -> Self {
        Self::ONE
    }
    pub fn is_one(self) -> bool {
        self.numerator == self.denominator
    }
    /// The fraction as a percentage in exact decimal digits (`11/100` is
    /// `11%`, `43/400` is `10.75%`). A fraction without a finite decimal
    /// percentage is shown as itself (`1/3`).
    pub fn percent(self) -> String {
        if self.denominator <= 0 || self.numerator < 0 {
            return format!("{}/{}", self.numerator, self.denominator);
        }
        let denominator = i64::from(self.denominator);
        let scaled = i64::from(self.numerator) * 100;
        let mut text = (scaled / denominator).to_string();
        let mut rest = scaled % denominator;
        let mut digits = String::new();
        while rest != 0 && digits.len() < 12 {
            rest *= 10;
            digits.push(char::from(b'0' + (rest / denominator) as u8));
            rest %= denominator;
        }
        if rest != 0 {
            return format!("{}/{}", self.numerator, self.denominator);
        }
        if !digits.is_empty() {
            text.push('.');
            text.push_str(&digits);
        }
        text.push('%');
        text
    }
}

/// A validated rate and base fraction: `0 <= rate <= 1` and `0 < base <= 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaxRule {
    rate: Fraction,
    base: Fraction,
}
impl TaxRule {
    pub fn new(rate: Fraction, base: Fraction) -> Result<Self, ValidationError> {
        let bounded = |f: Fraction, least: i32| {
            (1..=MAX_DENOMINATOR).contains(&f.denominator)
                && (least..=f.denominator).contains(&f.numerator)
        };
        if !bounded(rate, 0) {
            return Err(ValidationError("taxRate"));
        }
        if !bounded(base, 1) {
            return Err(ValidationError("taxBase"));
        }
        Ok(Self { rate, base })
    }
    pub fn rate(self) -> Fraction {
        self.rate
    }
    pub fn base(self) -> Fraction {
        self.base
    }
    /// `base × rate` as one fraction; both parts fit `i128` many times over.
    fn effective(self) -> (i128, i128) {
        (
            i128::from(self.base.numerator) * i128::from(self.rate.numerator),
            i128::from(self.base.denominator) * i128::from(self.rate.denominator),
        )
    }
    /// Prices are net: `tax = round(net × base × rate)`, `gross = net + tax`.
    /// Returns `(tax, gross)`.
    pub fn exclusive(self, net: i64) -> Result<(i64, i64), ValidationError> {
        let (numerator, denominator) = self.effective();
        let tax = half_away(
            i128::from(amount(net)?)
                .checked_mul(numerator)
                .ok_or(ValidationError("taxAmount"))?,
            denominator,
        )?;
        let gross = amount(net.checked_add(tax).ok_or(ValidationError("taxAmount"))?)?;
        Ok((tax, gross))
    }
    /// Prices contain the tax: `net = round(gross / (1 + base × rate))`,
    /// `tax = gross − net`. Returns `(net, tax)`.
    pub fn inclusive(self, gross: i64) -> Result<(i64, i64), ValidationError> {
        let (numerator, denominator) = self.effective();
        let net = half_away(
            i128::from(amount(gross)?)
                .checked_mul(denominator)
                .ok_or(ValidationError("taxAmount"))?,
            denominator + numerator,
        )?;
        Ok((net, gross - net))
    }
}

/// The net, tax and gross of one taxed document, in the document's own unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentTax {
    pub net: i64,
    pub tax: i64,
    pub gross: i64,
}
impl TaxRule {
    /// Taxes a document whose priced lines add up to `total`: the net when the
    /// prices exclude tax (tax is added), the gross when they include it (tax
    /// is extracted). `net + tax = gross` always holds.
    pub fn document(
        self,
        total: i64,
        price_includes_tax: bool,
    ) -> Result<DocumentTax, ValidationError> {
        if price_includes_tax {
            let (net, tax) = self.inclusive(total)?;
            Ok(DocumentTax {
                net,
                tax,
                gross: total,
            })
        } else {
            let (tax, gross) = self.exclusive(total)?;
            Ok(DocumentTax {
                net: total,
                tax,
                gross,
            })
        }
    }
    /// The tax a gross amount contains: `gross − round(gross / (1 + base × rate))`.
    ///
    /// On the gross of a tax-exclusive document this is the tax that was added:
    /// `(net + round(net × k)) / (1 + k)` differs from `net` by less than half
    /// a unit whenever `k > 0`, so it rounds back to `net`.
    pub fn contained(self, gross: i64) -> Result<i64, ValidationError> {
        self.inclusive(gross).map(|(_, tax)| tax)
    }
    /// Splits a correction of a taxed document into its net and tax parts
    /// (rule 6). The correction changes what the customer owes, so it is a
    /// gross amount on either price basis. The tax part is the difference
    /// between the tax contained in the corrected total and the tax the
    /// document carried before: `contained(total_before + amount) −
    /// tax_before`. The parts of all corrections therefore add up to the tax
    /// of the corrected total, and a total of zero has net and tax of zero.
    /// Returns `(net, tax)` with `net + tax = amount`.
    pub fn correction(
        self,
        total_before: i64,
        tax_before: i64,
        correction: i64,
    ) -> Result<(i64, i64), ValidationError> {
        let total = total_before
            .checked_add(correction)
            .ok_or(ValidationError("taxAmount"))?;
        let tax = self
            .contained(total)?
            .checked_sub(tax_before)
            .ok_or(ValidationError("taxAmount"))?;
        let net = correction
            .checked_sub(tax)
            .ok_or(ValidationError("taxAmount"))?;
        Ok((net, tax))
    }
}

/// The sentence for every document that shows a tax code or a tax amount
/// (rule 1).
pub const DOCUMENT_NOTICE: &str =
    "Tax codes are entered by the company; Archava does not verify tax rates or rules.";

/// A tax code as a document names it: `VAT 11%`, or `VAT 12% of 11/12 of the
/// price` when the rate applies to a fraction of the price.
pub fn describe(code: &str, rate: Fraction, base: Fraction) -> String {
    if base.is_one() {
        format!("{code} {}", rate.percent())
    } else {
        format!(
            "{code} {} of {}/{} of the price",
            rate.percent(),
            base.numerator,
            base.denominator
        )
    }
}

/// A document amount; corrections are negative, so both signs are in range.
fn amount(value: i64) -> Result<i64, ValidationError> {
    if (-MAX_MINOR..=MAX_MINOR).contains(&value) {
        Ok(value)
    } else {
        Err(ValidationError("taxAmount"))
    }
}
/// `numerator / denominator` rounded half away from zero; `denominator > 0`.
fn half_away(numerator: i128, denominator: i128) -> Result<i64, ValidationError> {
    let magnitude = numerator
        .unsigned_abs()
        .checked_mul(2)
        .and_then(|twice| twice.checked_add(denominator.unsigned_abs()))
        .ok_or(ValidationError("taxAmount"))?
        / (denominator.unsigned_abs() * 2);
    let magnitude = i64::try_from(magnitude).map_err(|_| ValidationError("taxAmount"))?;
    Ok(if numerator < 0 { -magnitude } else { magnitude })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fraction(numerator: i32, denominator: i32) -> Fraction {
        Fraction {
            numerator,
            denominator,
        }
    }
    fn rule(rate: (i32, i32), base: (i32, i32)) -> TaxRule {
        TaxRule::new(fraction(rate.0, rate.1), fraction(base.0, base.1)).unwrap()
    }
    /// A spread of amounts: small, around rounding boundaries, large, both signs.
    fn amounts() -> Vec<i64> {
        let mut values = vec![
            0,
            1,
            2,
            3,
            4,
            5,
            9,
            10,
            11,
            49,
            50,
            51,
            99,
            100,
            101,
            999,
            1_000,
            1_234,
            9_999,
            10_000,
            123_456,
            1_000_000,
            99_999_999,
            1_234_567_891,
            500_000_000_000,
            4_000_000_000_000_000,
        ];
        // A deterministic pseudo-random tail, so the spread is not hand-picked.
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        for _ in 0..400 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            values.push((state >> 16) as i64 % 4_000_000_000_000_000);
        }
        let negatives: Vec<i64> = values.iter().map(|v| -v).collect();
        values.extend(negatives);
        values
    }
    fn rules() -> Vec<TaxRule> {
        vec![
            rule((0, 1), (1, 1)),
            rule((1, 100), (1, 1)),
            rule((5, 100), (1, 1)),
            rule((11, 100), (1, 1)),
            rule((12, 100), (11, 12)),
            rule((12, 100), (1, 1)),
            rule((43, 400), (1, 1)),
            rule((1, 3), (2, 3)),
            rule((20, 100), (1, 2)),
            rule((999_999, 1_000_000), (999_999, 1_000_000)),
            rule((1, 1), (1, 1)),
        ]
    }

    #[test]
    fn net_plus_tax_is_gross_on_both_price_bases() {
        for rule in rules() {
            for net in amounts() {
                let (tax, gross) = rule.exclusive(net).unwrap();
                assert_eq!(net + tax, gross, "{rule:?} {net}");
                assert_eq!(tax.signum() * net.signum(), tax.signum().abs(), "{net}");
                let (inner_net, inner_tax) = rule.inclusive(net).unwrap();
                assert_eq!(inner_net + inner_tax, net, "{rule:?} {net}");
            }
        }
    }
    #[test]
    fn extracting_the_tax_of_an_added_tax_returns_the_net_within_one_unit() {
        for rule in rules() {
            for net in amounts() {
                let (_, gross) = rule.exclusive(net).unwrap();
                let (back, _) = rule.inclusive(gross).unwrap();
                assert!((back - net).abs() <= 1, "{rule:?} {net} {back}");
            }
        }
        // Whole-unit prices at a plain percentage come back exactly.
        let eleven = rule((11, 100), (1, 1));
        for net in [100, 1_000, 250_000, 1_000_000, -250_000] {
            let (tax, gross) = eleven.exclusive(net).unwrap();
            assert_eq!(eleven.inclusive(gross).unwrap(), (net, tax));
        }
    }
    #[test]
    fn rounding_is_half_away_from_zero_and_symmetric_for_corrections() {
        let half = rule((1, 2), (1, 1));
        assert_eq!(half.exclusive(1).unwrap(), (1, 2));
        assert_eq!(half.exclusive(-1).unwrap(), (-1, -2));
        assert_eq!(half.exclusive(3).unwrap(), (2, 5));
        assert_eq!(half.exclusive(-3).unwrap(), (-2, -5));
        let ten = rule((10, 100), (1, 1));
        assert_eq!(ten.exclusive(4).unwrap(), (0, 4));
        assert_eq!(ten.exclusive(5).unwrap(), (1, 6));
        assert_eq!(ten.exclusive(14).unwrap(), (1, 15));
        assert_eq!(ten.exclusive(15).unwrap(), (2, 17));
        assert_eq!(ten.exclusive(-15).unwrap(), (-2, -17));
        // 11 / 1.1 is exactly 10; 16 / 1.1 is 14.545…, which rounds to 15.
        assert_eq!(ten.inclusive(11).unwrap(), (10, 1));
        assert_eq!(ten.inclusive(16).unwrap(), (15, 1));
        assert_eq!(ten.inclusive(-16).unwrap(), (-15, -1));
        for rule in rules() {
            for value in amounts() {
                let (tax, gross) = rule.exclusive(value).unwrap();
                assert_eq!(rule.exclusive(-value).unwrap(), (-tax, -gross));
                let (net, inner) = rule.inclusive(value).unwrap();
                assert_eq!(rule.inclusive(-value).unwrap(), (-net, -inner));
            }
        }
    }
    #[test]
    fn eleven_percent_and_twelve_percent_of_eleven_twelfths_are_the_same_tax() {
        let plain = rule((11, 100), (1, 1));
        let scaled = rule((12, 100), (11, 12));
        assert_eq!(plain.exclusive(1_000_000).unwrap(), (110_000, 1_110_000));
        assert_eq!(scaled.exclusive(1_000_000).unwrap(), (110_000, 1_110_000));
        assert_eq!(plain.inclusive(1_110_000).unwrap(), (1_000_000, 110_000));
        for value in amounts() {
            assert_eq!(
                plain.exclusive(value).unwrap(),
                scaled.exclusive(value).unwrap(),
                "{value}"
            );
            assert_eq!(
                plain.inclusive(value).unwrap(),
                scaled.inclusive(value).unwrap(),
                "{value}"
            );
        }
    }
    #[test]
    fn a_document_is_taxed_on_its_stated_price_basis() {
        let eleven = rule((11, 100), (1, 1));
        assert_eq!(
            eleven.document(30_000, false).unwrap(),
            DocumentTax {
                net: 30_000,
                tax: 3_300,
                gross: 33_300
            }
        );
        assert_eq!(
            eleven.document(30_000, true).unwrap(),
            DocumentTax {
                net: 27_027,
                tax: 2_973,
                gross: 30_000
            }
        );
        // 3 × 3.33 in minor units: 999 × 0.11 = 109.89, which rounds to 110.
        assert_eq!(
            eleven.document(999, false).unwrap(),
            DocumentTax {
                net: 999,
                tax: 110,
                gross: 1_109
            }
        );
        for rule in rules() {
            for total in amounts() {
                for inclusive in [false, true] {
                    let Ok(taxed) = rule.document(total, inclusive) else {
                        continue;
                    };
                    assert_eq!(taxed.net + taxed.tax, taxed.gross, "{rule:?} {total}");
                }
            }
        }
    }
    #[test]
    fn the_tax_contained_in_an_exclusive_gross_is_exactly_the_tax_that_was_added() {
        for rule in rules() {
            for net in amounts() {
                let Ok((tax, gross)) = rule.exclusive(net) else {
                    continue;
                };
                assert_eq!(rule.contained(gross).unwrap(), tax, "{rule:?} {net}");
                assert_eq!(rule.inclusive(gross).unwrap(), (net, tax), "{rule:?} {net}");
            }
        }
    }
    #[test]
    fn corrections_split_so_that_the_parts_add_up_and_a_full_reversal_is_zero() {
        let eleven = rule((11, 100), (1, 1));
        // Exclusive 30,000 + 3,300: a correction of -11,100 is -10,000 net and
        // -1,100 tax; reversing the rest leaves nothing.
        assert_eq!(
            eleven.correction(33_300, 3_300, -11_100).unwrap(),
            (-10_000, -1_100)
        );
        assert_eq!(
            eleven.correction(22_200, 2_200, -22_200).unwrap(),
            (-20_000, -2_200)
        );
        // An untaxed-looking unit still splits by the cumulative total: ten
        // corrections of one unit move the tax by one unit exactly once.
        let (mut total, mut tax) = (33_300_i64, 3_300_i64);
        let mut tax_parts = Vec::new();
        for _ in 0..10 {
            let (net_part, tax_part) = eleven.correction(total, tax, -1).unwrap();
            assert_eq!(net_part + tax_part, -1);
            total -= 1;
            tax += tax_part;
            tax_parts.push(tax_part);
            assert_eq!(tax, eleven.contained(total).unwrap());
        }
        assert_eq!(tax_parts.iter().sum::<i64>(), -1);
        assert_eq!((total, tax), (33_290, 3_299));
        for rule in rules() {
            for inclusive in [false, true] {
                for total in [0, 1, 9, 999, 30_000, 1_234_567_891] {
                    let Ok(document) = rule.document(total, inclusive) else {
                        continue;
                    };
                    let (mut gross, mut tax) = (document.gross, document.tax);
                    // Any path of corrections, then whatever is left.
                    for step in [-1, -7, 3, -(gross / 3), 11, -2] {
                        let (net_part, tax_part) = rule.correction(gross, tax, step).unwrap();
                        assert_eq!(net_part + tax_part, step, "{rule:?} {total}");
                        gross += step;
                        tax += tax_part;
                        assert_eq!(tax, rule.contained(gross).unwrap(), "{rule:?} {total}");
                    }
                    let (net_part, tax_part) = rule.correction(gross, tax, -gross).unwrap();
                    assert_eq!(gross + net_part + tax_part, 0);
                    assert_eq!(tax + tax_part, 0, "{rule:?} {total}");
                    assert_eq!((gross - tax) + net_part, 0, "{rule:?} {total}");
                }
            }
        }
    }
    #[test]
    fn a_document_names_its_tax_code_with_an_exact_rate_and_base() {
        assert_eq!(describe("VAT", fraction(11, 100), Fraction::ONE), "VAT 11%");
        assert_eq!(
            describe("VAT", fraction(43, 400), fraction(5, 5)),
            "VAT 10.75%"
        );
        assert_eq!(
            describe("PPN", fraction(12, 100), fraction(11, 12)),
            "PPN 12% of 11/12 of the price"
        );
        assert!(DOCUMENT_NOTICE.contains("entered by the company"));
        assert!(DOCUMENT_NOTICE.contains("Archava does not verify"));
    }
    #[test]
    fn a_zero_rate_adds_and_extracts_nothing() {
        let zero = rule((0, 100), (11, 12));
        for value in amounts() {
            assert_eq!(zero.exclusive(value).unwrap(), (0, value));
            assert_eq!(zero.inclusive(value).unwrap(), (value, 0));
        }
    }
    #[test]
    fn amounts_near_the_ledger_limit_are_exact_or_refused_never_wrapped() {
        let eleven = rule((11, 100), (1, 1));
        // The largest gross the ledger holds splits exactly.
        let (net, tax) = eleven.inclusive(MAX_MINOR).unwrap();
        assert_eq!(net + tax, MAX_MINOR);
        assert_eq!(net, 8_108_108_108_108_108);
        assert_eq!(eleven.inclusive(-MAX_MINOR).unwrap(), (-net, -tax));
        // The largest net whose gross still fits, and the first that does not.
        assert_eq!(eleven.exclusive(net).unwrap(), (tax, MAX_MINOR));
        assert!(eleven.exclusive(net + 5).is_err());
        assert!(eleven.exclusive(MAX_MINOR).is_err());
        assert!(eleven.exclusive(-MAX_MINOR).is_err());
        let zero = rule((0, 1), (1, 1));
        assert_eq!(zero.exclusive(MAX_MINOR).unwrap(), (0, MAX_MINOR));
        for outside in [MAX_MINOR + 1, -MAX_MINOR - 1, i64::MAX, i64::MIN] {
            assert!(eleven.exclusive(outside).is_err(), "{outside}");
            assert!(eleven.inclusive(outside).is_err(), "{outside}");
        }
        // The widest fractions with the widest amount stay inside i128.
        let widest = rule((999_999, 1_000_000), (999_999, 1_000_000));
        let (net, tax) = widest.inclusive(MAX_MINOR).unwrap();
        assert_eq!(net + tax, MAX_MINOR);
    }
    #[test]
    fn rules_refuse_fractions_outside_their_bounds() {
        for (rate, base) in [
            ((-1, 100), (1, 1)),
            ((101, 100), (1, 1)),
            ((1, 0), (1, 1)),
            ((1, -100), (1, 1)),
            ((1, MAX_DENOMINATOR + 1), (1, 1)),
            ((11, 100), (0, 1)),
            ((11, 100), (13, 12)),
            ((11, 100), (1, 0)),
            ((11, 100), (-1, -1)),
            ((11, 100), (1, MAX_DENOMINATOR + 1)),
        ] {
            assert!(
                TaxRule::new(fraction(rate.0, rate.1), fraction(base.0, base.1)).is_err(),
                "{rate:?} {base:?}"
            );
        }
        let accepted = rule((1, 1), (1, 1));
        assert_eq!(accepted.rate(), Fraction::ONE);
        assert!(accepted.base().is_one());
        assert!(TaxRule::new(fraction(0, MAX_DENOMINATOR), fraction(1, MAX_DENOMINATOR)).is_ok());
    }
    #[test]
    fn percentages_are_exact_decimal_digits_or_the_fraction_itself() {
        for (numerator, denominator, shown) in [
            (11, 100, "11%"),
            (12, 100, "12%"),
            (0, 1, "0%"),
            (1, 1, "100%"),
            (43, 400, "10.75%"),
            (1, 8, "12.5%"),
            (1, 1000, "0.1%"),
            (22, 200, "11%"),
            (1, 3, "1/3"),
            (11, 12, "11/12"),
        ] {
            assert_eq!(fraction(numerator, denominator).percent(), shown);
        }
    }
    #[test]
    fn fractions_are_integers_on_the_wire() {
        assert_eq!(
            serde_json::to_value(fraction(11, 100)).unwrap(),
            json!({"numerator":11,"denominator":100})
        );
        for bad in [
            json!({"numerator":11.5,"denominator":100}),
            json!({"numerator":"11","denominator":100}),
            json!({"numerator":11}),
            json!({"numerator":11,"denominator":100,"percent":11}),
            json!(0.11),
        ] {
            assert!(
                serde_json::from_value::<Fraction>(bad.clone()).is_err(),
                "{bad}"
            );
        }
        assert_eq!(Kind::Output.as_str(), "output");
        assert_eq!(Kind::Input.as_str(), "input");
    }
}
