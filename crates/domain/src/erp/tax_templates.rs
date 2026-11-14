//! Tax code templates: starting values for the `tax_code.create` form.
//!
//! `config/tax-templates.v1.json` lists, per market, a suggested code text, a
//! kind, a rate and a base fraction. A template is data, not a rule: nothing
//! here says what any law requires, every template carries `verified: false`,
//! and choosing one only pre-fills the form. The company still chooses the
//! control account and the first day, and a reviewer confirms the code
//! (`docs/architecture/TAX.md`, rule 1). No template creates or confirms a
//! tax code, and nothing installs a market's templates in bulk.
//!
//! `embedded/` is what this crate compiles in (`tools/rust/domain-assets.mjs
//! --write` refreshes it). Another market is added as data in the catalogue.
//! See `docs/modules/TAX_INDONESIA_PACK.md`.
use super::tax::{Fraction, Kind, TaxRule};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// One template of one market.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxTemplate {
    /// The market the template is listed under (`ID`).
    pub market: String,
    /// Stable identifier, unique in the catalogue.
    pub id: String,
    /// The suggested tax code text; the company may change it.
    pub code: String,
    pub kind: Kind,
    pub rate: Fraction,
    pub base: Fraction,
    /// What the template's own numbers are, in neutral words.
    pub label: String,
    /// Always `false`: Archava has not verified any template.
    pub verified: bool,
    /// The sentence shown with the template.
    pub note: String,
}
impl TaxTemplate {
    /// The members of `tax_code.create` a template fills. The book, the
    /// control account, the first day and the reason are not among them.
    pub fn form_values(&self) -> Value {
        json!({
            "code": self.code,
            "kind": self.kind.as_str(),
            "rate": {"numerator": self.rate.numerator, "denominator": self.rate.denominator},
            "base": {"numerator": self.base.numerator, "denominator": self.base.denominator},
        })
    }
    /// The template's fractions as a validated rule.
    pub fn rule(&self) -> Option<TaxRule> {
        TaxRule::new(self.rate, self.base).ok()
    }
}

/// The templates of one market.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Market {
    /// The catalogue key (`ID`).
    pub code: String,
    pub name: String,
    /// The book currencies the Accounting screen offers this market's
    /// templates on. A display rule only: it decides where the picker is
    /// shown, not what a company may enter.
    pub currencies: Vec<String>,
    pub templates: Vec<TaxTemplate>,
}

/// The whole catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalogue {
    /// The standing sentence shown with every template.
    pub notice: String,
    pub markets: Vec<Market>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateEntry {
    id: String,
    code: String,
    kind: Kind,
    rate: Fraction,
    base: Fraction,
    label: String,
    verified: bool,
    note: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MarketEntry {
    name: String,
    currencies: Vec<String>,
    templates: Vec<TemplateEntry>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    #[allow(dead_code)]
    schema_version: u32,
    notice: String,
    markets: BTreeMap<String, MarketEntry>,
}

/// Every market and its templates, in catalogue order.
pub fn catalogue() -> &'static Catalogue {
    static DATA: OnceLock<Catalogue> = OnceLock::new();
    DATA.get_or_init(|| {
        let document: Document =
            serde_json::from_str(include_str!("../../embedded/tax-templates.v1.json"))
                .expect("the embedded tax template catalogue is valid");
        Catalogue {
            notice: document.notice,
            markets: document
                .markets
                .into_iter()
                .map(|(code, market)| Market {
                    templates: market
                        .templates
                        .into_iter()
                        .map(|entry| TaxTemplate {
                            market: code.clone(),
                            id: entry.id,
                            code: entry.code,
                            kind: entry.kind,
                            rate: entry.rate,
                            base: entry.base,
                            label: entry.label,
                            verified: entry.verified,
                            note: entry.note,
                        })
                        .collect(),
                    code,
                    name: market.name,
                    currencies: market.currencies,
                })
                .collect(),
        }
    })
}

/// The templates of one market, or none for a market without any.
pub fn for_market(code: &str) -> &'static [TaxTemplate] {
    catalogue()
        .markets
        .iter()
        .find(|market| market.code == code)
        .map_or(&[], |market| market.templates.as_slice())
}

/// The markets whose templates are offered on a book kept in `currency`.
pub fn markets_for_currency(currency: &str) -> Vec<&'static Market> {
    catalogue()
        .markets
        .iter()
        .filter(|market| market.currencies.iter().any(|c| c == currency))
        .collect()
}

/// The template an identifier names.
pub fn by_id(id: &str) -> Option<&'static TaxTemplate> {
    catalogue()
        .markets
        .iter()
        .flat_map(|market| market.templates.iter())
        .find(|template| template.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn valid_code(code: &str) -> bool {
        !code.is_empty()
            && code.len() <= 40
            && code
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
            && code.starts_with(|c: char| c.is_ascii_uppercase() || c.is_ascii_digit())
    }

    #[test]
    fn the_catalogue_loads_and_no_template_is_verified() {
        let catalogue = catalogue();
        assert!(catalogue
            .notice
            .contains("does not verify tax rates or rules"));
        assert!(catalogue.notice.contains("tax adviser"));
        assert!(!catalogue.markets.is_empty());
        let mut ids = BTreeSet::new();
        for market in &catalogue.markets {
            assert!(market.code.len() == 2 && market.code.bytes().all(|c| c.is_ascii_uppercase()));
            assert!(!market.name.is_empty() && !market.templates.is_empty());
            for currency in &market.currencies {
                assert!(currency.len() == 3 && currency.bytes().all(|c| c.is_ascii_uppercase()));
            }
            let mut codes = BTreeSet::new();
            for template in &market.templates {
                assert!(!template.verified, "{}", template.id);
                assert!(template.note.contains("not verified"), "{}", template.id);
                assert!(template.note.contains("tax adviser"), "{}", template.id);
                assert_eq!(template.market, market.code);
                assert!(valid_code(&template.code), "{}", template.code);
                assert!(template.rule().is_some(), "{}", template.id);
                assert!(!template.label.is_empty());
                assert!(codes.insert(&template.code), "duplicate {}", template.code);
                assert!(ids.insert(&template.id), "duplicate {}", template.id);
            }
        }
    }

    #[test]
    fn no_template_names_an_authority_or_claims_to_be_official() {
        let raw = include_str!("../../embedded/tax-templates.v1.json").to_lowercase();
        for word in [
            "compliant",
            "official",
            "e-faktur",
            "efaktur",
            "coretax",
            "djp",
            "required by",
            "\"verified\": true",
        ] {
            assert!(!raw.contains(word), "{word}");
        }
    }

    #[test]
    fn the_indonesia_market_has_both_kinds_at_both_rules() {
        let templates = for_market("ID");
        assert_eq!(templates.len(), 4);
        for (kind, rate, base) in [
            (Kind::Output, (11, 100), (1, 1)),
            (Kind::Input, (11, 100), (1, 1)),
            (Kind::Output, (12, 100), (11, 12)),
            (Kind::Input, (12, 100), (11, 12)),
        ] {
            assert_eq!(
                templates
                    .iter()
                    .filter(|t| t.kind == kind
                        && (t.rate.numerator, t.rate.denominator) == rate
                        && (t.base.numerator, t.base.denominator) == base)
                    .count(),
                1
            );
        }
        assert!(for_market("XX").is_empty());
        assert_eq!(markets_for_currency("IDR").len(), 1);
        assert!(markets_for_currency("USD").is_empty());
        assert_eq!(by_id("id-output-11").unwrap().code, "VAT-OUT-11");
        assert!(by_id("missing").is_none());
    }

    /// 12/100 on 11/12 of a price is 11/100 of it: the two templates give the
    /// same tax through the integer rule, on either price basis.
    #[test]
    fn twelve_percent_on_eleven_twelfths_equals_eleven_percent_on_the_whole_price() {
        let whole = by_id("id-output-11").unwrap().rule().unwrap();
        let part = by_id("id-output-12-base-11-12").unwrap().rule().unwrap();
        let mut amounts: Vec<i64> = vec![
            0,
            1,
            2,
            3,
            4,
            5,
            9,
            10,
            11,
            12,
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
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..2_000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            amounts.push((state >> 20) as i64 % 10_000_000_000);
        }
        for amount in amounts {
            for signed in [amount, -amount] {
                assert_eq!(whole.exclusive(signed), part.exclusive(signed), "{signed}");
                assert_eq!(whole.inclusive(signed), part.inclusive(signed), "{signed}");
                assert_eq!(whole.contained(signed), part.contained(signed), "{signed}");
            }
        }
    }

    #[test]
    fn a_template_fills_only_the_code_kind_rate_and_base_of_the_form() {
        let values = by_id("id-input-12-base-11-12").unwrap().form_values();
        assert_eq!(
            values,
            json!({"code":"VAT-IN-12-B11-12","kind":"input","rate":{"numerator":12,"denominator":100},"base":{"numerator":11,"denominator":12}})
        );
        let object = values.as_object().unwrap();
        for absent in [
            "bookId",
            "controlAccount",
            "effectiveFrom",
            "reason",
            "state",
        ] {
            assert!(!object.contains_key(absent));
        }
    }
}
