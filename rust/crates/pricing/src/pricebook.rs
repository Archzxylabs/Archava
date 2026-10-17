//! The pricebook data model — a direct mirror of `config/pricing.v1.json`
//! as validated by `packages/config/src/pricing/schema.ts`.
//!
//! Three shapes in this file are chosen for parity rather than for taste.
//!
//! First, a tier is `Option`-free on `custom` only. The zod schema makes
//! several money fields `optional()` across the unions, so after parsing a
//! pricebook really can hold `{monthly: 199}` with no `setup`; `SimpleTier`
//! therefore keeps every money field optional instead of inventing a zero, and
//! the engine applies `?? 0` at exactly the spots the TypeScript does.
//!
//! Second, tier tables key on `String`, not on an enum of tier names. `quote()`
//! must be able to *miss* on an unknown name — the baseline throws a specific
//! error for it, and it never validates caller-supplied tier names — so an
//! exhaustive enum would make that lookup unrepresentable.
//!
//! Third, the pricebook is deserialized, never hand-constructed. The harness
//! loads the same `config/pricing.v1.json` the product loads, so the
//! comparison is over identical data rather than a Rust transcription that
//! could drift from the file.

use serde::Deserialize;

/// The two independent pricebooks. Never FX-converted between them.
///
/// `Ord` is derived because the regions table is a `BTreeMap`; the ordering it
/// implies (GLOBAL before ID) is not used for anything observable — the engine
/// looks regions up by key.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, serde::Serialize,
)]
#[serde(rename_all = "UPPERCASE")]
pub enum RegionCode {
    Global,
    Id,
    /// A region absent from the request.
    ///
    /// Not a third region and not a pricebook key. It exists because the
    /// baseline's `toRequest` copies `init.region` across untouched, so a
    /// request with no `region` reaches `quoteRequest` with `undefined`, and
    /// `pricebook.regions[undefined]` coerces to the literal string
    /// `"undefined"`, misses, and throws `Unknown region "undefined"`.
    ///
    /// Without this variant the port had to either panic on an absent field —
    /// which on a napi boundary aborts the host process rather than throwing,
    /// see `request.rs` — or widen `QuoteRequest::region` to a `String`, which
    /// would move the *typing* of a closed set into the engine and lose the
    /// exhaustiveness the enum buys everywhere else.
    ///
    /// It is deliberately invisible to the wire format: no JSON string maps to
    /// it, and it never serializes back out. An unknown *string* region is
    /// rejected at deserialization instead (`unknown variant \`EMEA\``), which
    /// is a separate divergence from the baseline documented in `engine.rs`.
    /// The only way to construct one is [`RegionCode::undefined_key`], called
    /// from `into_request` when the field is absent.
    #[serde(skip)]
    Absent,
}

impl RegionCode {
    /// The currency each region quotes in.
    pub fn currency(self) -> &'static str {
        match self {
            RegionCode::Global => "USD",
            RegionCode::Id => "IDR",
            // Unreachable in practice: `require_region` rejects `Absent` before
            // anything asks for a currency. Kept so the match stays exhaustive
            // and a future caller cannot forget the case.
            RegionCode::Absent => "USD",
        }
    }

    /// The region's key as the pricebook spells it.
    ///
    /// This is the string that appears in `PricingError` messages when a
    /// region lookup fails, so it must match the pricebook key exactly.
    pub fn as_str(self) -> &'static str {
        match self {
            RegionCode::Global => "GLOBAL",
            RegionCode::Id => "ID",
            // The literal key the baseline's `regions[undefined]` lookup
            // produces, so the error message is byte-identical.
            RegionCode::Absent => "undefined",
        }
    }

    /// The region standing for a request field the caller did not supply.
    ///
    /// A constructor rather than a direct `RegionCode::Absent` construction so
    /// the one place that models "absent" is nameable in a grep.
    pub fn undefined_key() -> Self {
        RegionCode::Absent
    }

    /// The increment reported as `roundedWithIncrement`.
    ///
    /// This is the *pricebook's* `currency_rounding`, selected exactly as
    /// `engine.ts` selects it (`region === 'ID' ? ID : GLOBAL`), so any region
    /// outside the two known ones reports the GLOBAL increment. It is a
    /// separate source from the hardcoded constants in `jsmath.rs`, and the
    /// two are deliberately allowed to disagree because in the baseline they
    /// already can: `rounding.ts` would keep rounding by 50 even if the
    /// pricebook said otherwise, while `engine.ts` would report the file's
    /// value. Unifying them would hide that.
    pub fn rounding_increment(self, pricebook: &Pricebook) -> f64 {
        match self {
            RegionCode::Global => pricebook.currency_rounding.global,
            RegionCode::Id => pricebook.currency_rounding.id,
            // `engine.ts` selects with `region === 'ID' ? ID : GLOBAL`, so an
            // unknown region — and therefore an absent one — reports GLOBAL.
            // Unreachable for the same reason `currency`'s arm is.
            RegionCode::Absent => pricebook.currency_rounding.global,
        }
    }
}

/// A usage allowance attached to a presence tier.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UsageAllowance {
    pub unit: String,
    #[serde(default)]
    pub quantity: f64,
}

/// An overage rate attached to a presence tier.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Overage {
    pub unit: String,
    #[serde(default)]
    pub price: f64,
}

/// A presence tier. `presenceTierSchema` requires all four keys, so the
/// `Option`s are for schema shape rather than for this pricebook.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PresenceTier {
    #[serde(default)]
    pub setup: Option<f64>,
    #[serde(default)]
    pub monthly: Option<f64>,
    #[serde(default)]
    pub included_usage: Option<UsageAllowance>,
    #[serde(default)]
    pub overage: Option<Overage>,
}

/// A capability tier.
///
/// `included_standard_integrations` is only present on the fixed variant; the
/// `enterprise` tier is `{custom: true}`, has no such key, and therefore
/// reports `0` inclusions rather than an error.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CapabilityTier {
    #[serde(default)]
    pub setup: Option<f64>,
    #[serde(default)]
    pub monthly: Option<f64>,
    #[serde(default)]
    pub included_standard_integrations: Option<f64>,
    #[serde(default)]
    pub custom: bool,
}

/// An environment tier. `note` is carried but never read by the engine.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EnvironmentTier {
    #[serde(default)]
    pub setup: Option<f64>,
    #[serde(default)]
    pub monthly: Option<f64>,
    #[serde(default)]
    pub setup_from: Option<f64>,
    #[serde(default)]
    pub custom: bool,
    #[serde(default)]
    pub note: Option<String>,
}

/// An integration tier, a design add-on, and a support add-on all carry the
/// same three shapes and are classified identically at their call sites, so
/// they share one deserialized form.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SimpleTier {
    #[serde(default)]
    pub setup: Option<f64>,
    #[serde(default)]
    pub monthly: Option<f64>,
    #[serde(default)]
    pub setup_from: Option<f64>,
    #[serde(default)]
    pub custom: bool,
}

/// A region's tier tables.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Region {
    pub currency: String,
    #[serde(default)]
    pub label: Option<String>,
    pub presence: std::collections::BTreeMap<String, PresenceTier>,
    pub capability: std::collections::BTreeMap<String, CapabilityTier>,
    pub environment: std::collections::BTreeMap<String, EnvironmentTier>,
    pub integration: std::collections::BTreeMap<String, SimpleTier>,
    pub design_addons: std::collections::BTreeMap<String, SimpleTier>,
    pub support_addons: std::collections::BTreeMap<String, SimpleTier>,
}

/// `currency_rounding` from the pricebook. Only the reporting path reads it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CurrencyRounding {
    #[serde(rename = "GLOBAL")]
    pub global: f64,
    #[serde(rename = "ID")]
    pub id: f64,
}

/// `discount_rules` from the pricebook.
///
/// `core_components` is deserialized but never used: the engine hard-codes
/// which components count, and the pricebook's array disagrees with that
/// hard-coding (it omits `capability_paid`). The field is kept so the model
/// stays faithful to the file; the engine ignores it for the same reason
/// `engine.ts` does.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DiscountRules {
    pub two_core_components_setup_discount_pct: f64,
    pub three_core_components_setup_discount_pct: f64,
    #[serde(default)]
    pub core_components: Vec<String>,
    pub annual_recurring_prepay_discount_pct: f64,
}

/// `commercial_rules` — the one field the engine reads.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CommercialRules {
    pub quote_validity_days: i64,
}

/// The whole pricebook.
///
/// Regions are a map keyed by `RegionCode`, which is what the file holds and
/// what `pricebook.regions[region]` in the baseline indexes: an absent region
/// is a lookup miss, which is exactly how `requireRegion` reaches its
/// `PricingError`. (An earlier draft of this struct used a `Vec` so that an
/// unknown region "reported as missing" — but a two-variant enum key fails
/// deserialization on a foreign key either way, so the `Vec` bought nothing
/// and cost a mismatch with the file.)
///
/// Unread material (`tax_policy`, `usage_definitions`, most of
/// `commercial_rules`) is intentionally not modeled. Dropping it is a
/// faithfulness loss only if some later step starts reading it, and modeling
/// it would let an unrelated edit to `pricing.v1.json` break an experiment
/// that never looks at it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Pricebook {
    pub schema_version: String,
    pub effective_date: String,
    pub currency_rounding: CurrencyRounding,
    pub regions: std::collections::BTreeMap<RegionCode, Region>,
    pub discount_rules: DiscountRules,
    pub commercial_rules: CommercialRules,
}

impl Pricebook {
    /// Look up a region, `None` for an absent one.
    ///
    /// Mirrors `requireRegion`'s falsy check. Note the baseline's
    /// `includedStandardIntegrations` reaches `pricebook.regions[region]`
    /// *unguarded* and throws a raw `TypeError` for an unknown region; that
    /// asymmetry is reproduced in `engine.rs`, not here.
    pub fn region(&self, code: RegionCode) -> Option<&Region> {
        self.regions.get(&code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsmath::{GLOBAL_ROUNDING_INCREMENT, ID_ROUNDING_INCREMENT};

    /// The real product pricebook, loaded from a read-only copy.
    ///
    /// Deliberately a real fixture rather than an inline literal: the pilot's
    /// claim depends on Rust seeing exactly the file the TS engine sees,
    /// including the `custom: true` keys the zod union strips and the `note`
    /// field the engine never reads.
    const PRICEBOOK: &str = include_str!("../../../../config/pricing.v1.json");

    fn load() -> Pricebook {
        serde_json::from_str(PRICEBOOK).expect("the product pricebook must deserialize")
    }

    #[test]
    fn both_regions_load_and_stay_independent() {
        let book = load();
        let global = book.region(RegionCode::Global).expect("GLOBAL");
        let id = book.region(RegionCode::Id).expect("ID");
        assert_eq!(global.currency, "USD");
        assert_eq!(id.currency, "IDR");
        assert_eq!(global.presence["chat"].setup, Some(2_000.0));
        assert_eq!(id.presence["chat"].setup, Some(6_900_000.0));
    }

    #[test]
    fn the_two_rounding_sources_stay_modelled_separately() {
        let book = load();
        assert_eq!(book.currency_rounding.global, GLOBAL_ROUNDING_INCREMENT);
        assert_eq!(book.currency_rounding.id, ID_ROUNDING_INCREMENT);
        assert_eq!(RegionCode::Global.rounding_increment(&book), 50.0);
        assert_eq!(RegionCode::Id.rounding_increment(&book), 100_000.0);
    }

    #[test]
    fn capability_tiers_carry_their_inclusion_allowance() {
        let book = load();
        for code in [RegionCode::Global, RegionCode::Id] {
            let region = book.region(code).unwrap();
            assert_eq!(
                region.capability["assist"].included_standard_integrations,
                Some(0.0)
            );
            assert_eq!(
                region.capability["act"].included_standard_integrations,
                Some(1.0)
            );
            assert_eq!(
                region.capability["transact"].included_standard_integrations,
                Some(2.0)
            );
            // The enterprise tier is `{custom: true}` and has no key at all,
            // so it reports no inclusions rather than an error.
            assert_eq!(
                region.capability["enterprise"].included_standard_integrations,
                None
            );
            assert!(region.capability["enterprise"].custom);
        }
    }

    #[test]
    fn tier_tables_are_keyed_by_string_and_can_miss() {
        let book = load();
        let region = book.region(RegionCode::Global).unwrap();
        // `contains_key`, not `get(..).is_none()`, per clippy: the same claim,
        // and the shorter spelling cannot accidentally assert on a value.
        assert!(!region.environment.contains_key("no_such_environment"));
        // `note` survives deserialization even though the engine ignores it.
        assert!(region.environment["existing_site"].note.is_some());
    }

    #[test]
    fn the_custom_web_app_line_keeps_every_key_the_file_has() {
        let book = load();
        let global = book.region(RegionCode::Global).unwrap();
        let tier = &global.environment["custom_web_app"];
        // The raw JSON carries all three keys; whether that classifies as
        // `from` or `custom` is decided in classify.rs, not here.
        assert_eq!(tier.setup_from, Some(8_000.0));
        assert_eq!(tier.monthly, Some(0.0));
        assert!(tier.custom);
    }
}
