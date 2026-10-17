//! `QuoteRequest` / `QuoteRequestInit` — the engine's input.
//!
//! The baseline keeps two types (request.ts:14-44) and converts between them
//! in `toRequest`, applying the defaults in exactly one place. The port keeps
//! the same split and the same single conversion site, because those defaults
//! are observable: a request that omits `support` and one that passes
//! `'standard'` produce identical quotes, so a port that applied `?? 'standard'`
//! in two places — or in one place too eagerly — could produce a different
//! result for a `usageForecast` of `0`.
//!
//! ## The default that is not a default
//!
//! `usageForecast` is the one field where `QuoteRequest` deliberately
//! preserves *absent* as a distinct state. It is declared optional and
//! `toRequest` copies it through untouched — no `?? 0`, unlike every numeric
//! field around it. The engine then tests `request.usageForecast !== undefined`
//! before computing overage. Modelling it as `Option<f64>` keeps that
//! distinction visible instead of flattening it.

use crate::pricebook::RegionCode;
use serde::Deserialize;

/// A fully-specified quote request. Every field is filled, so the engine can
/// never silently fall back to a default tier or price.
///
/// This is the baseline's `QuoteRequest`, and the field names here are the
/// Rust spellings — snake_case. The JSON names matter only at the boundary, and
/// those are on `QuoteRequestInit` below, where `rename_all` makes them match
/// the baseline exactly. Keeping the two apart means a reader is never left
/// guessing which spelling a given name refers to.
#[derive(Debug, Clone, PartialEq)]
pub struct QuoteRequest {
    pub region: RegionCode,
    pub presence: String,
    pub capability: String,
    pub environment: String,
    pub standard_integrations: f64,
    pub custom_integrations: f64,
    pub advanced_integrations: f64,
    pub enterprise_integrations: f64,
    pub design_addons: Vec<String>,
    pub support: String,
    /// `None` means "no forecast was supplied", which the engine treats as
    /// "no overage estimate" rather than "zero usage".
    pub usage_forecast: Option<f64>,
    pub annual_prepay: bool,
}

/// The caller-facing shape. Every field except the four tier names is
/// optional, and each optional field has exactly one default.
///
/// This is the baseline's `QuoteRequestInit`.
///
/// `Deserialize` is derived so the napi boundary can accept the request as
/// JSON text rather than as a JS object converted through napi's own type
/// mapping. That is the whole reason the boundary is a string one: a struct
/// mapping would let a wrong default be repaired silently during conversion.
///
/// `rename_all = "camelCase"` is not cosmetic. Without it the struct accepts
/// `standard_integrations` while the boundary is handed `standardIntegrations`,
/// and serde's default of ignoring unknown keys means **every optional field is
/// silently dropped** — the request parses, the quote is computed, and the two
/// engines disagree on 164 of 346 fixtures because each optional field looked
/// absent. The Rust unit tests could not have caught this: they construct the
/// struct directly, so no serde renaming is ever exercised. Only the parity
/// run, which feeds real JSON through the boundary, exposed it.
///
/// The port is also stricter than serde's default, and deliberately so.
/// `deny_unknown_fields` turns the failure mode that hid this bug from
/// "quietly ignored" into "loudly rejected", so a future field-name drift
/// fails at the boundary instead of producing a plausible wrong quote.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuoteRequestInit {
    pub region: Option<RegionCode>,
    pub presence: Option<String>,
    pub capability: Option<String>,
    pub environment: Option<String>,
    pub standard_integrations: Option<f64>,
    pub custom_integrations: Option<f64>,
    pub advanced_integrations: Option<f64>,
    pub enterprise_integrations: Option<f64>,
    pub design_addons: Option<Vec<String>>,
    pub support: Option<String>,
    pub usage_forecast: Option<f64>,
    pub annual_prepay: Option<bool>,
}

impl QuoteRequestInit {
    /// Fill every unset field with the baseline's default.
    ///
    /// A port of `toRequest` (engine.ts:115-131). The baseline copies the four
    /// tier names across untouched — `region: init.region`, no `??`, no check —
    /// so an *absent* tier name does not fail here. It reaches `quoteRequest`
    /// as `undefined`, and the object lookup `selected.presence[undefined]`
    /// coerces the key to the literal string `"undefined"`, misses the table,
    /// and throws `Unknown presence "undefined"`.
    ///
    /// ## Why this is not `expect`
    ///
    /// The first version of this port made the four names required fields and
    /// called `.expect()` on each. That turned a caller mistake into a **panic**,
    /// and a panic on a napi boundary does not surface as a JavaScript
    /// exception: it aborts the host process. Proven against the addon —
    /// `quote_json` with a request whose `region` was missing printed
    ///
    ///   thread '<unnamed>' panicked at crates/pricing/src/request.rs:105:18:
    ///   region is a required tier name on QuoteRequestInit
    ///   fatal runtime error: failed to initiate panic, error 5, aborting
    ///
    /// and exited 134 with a core dump, after which the calling script's own
    /// `try/catch` never ran. The baseline, handed the identical request, threw
    /// a catchable `PricingError: Unknown region "undefined".`
    ///
    /// So the port cannot use the types' opinions as a substitute for the
    /// baseline's behaviour. The absent tier name is carried as the literal
    /// string the baseline's property access produces, and the error comes out
    /// of `quote_request` where the baseline throws it — a value the caller can
    /// catch, with the message the baseline writes.
    ///
    /// p1's 346 pricing fixtures never reached this path: not one of them omits a
    /// tier name, so the parity run passed over it. It was found by probing the
    /// addon directly, which is why the p1 case set is being extended to cover
    /// it rather than relying on this comment.
    pub fn into_request(self) -> QuoteRequest {
        QuoteRequest {
            region: self.region.unwrap_or(RegionCode::undefined_key()),
            presence: self.presence.unwrap_or_else(|| "undefined".to_string()),
            capability: self.capability.unwrap_or_else(|| "undefined".to_string()),
            environment: self.environment.unwrap_or_else(|| "undefined".to_string()),
            // Clamped at zero, per Step B. `Math.max(0, -3)` is `0`, and
            // `Math.max(0, NaN)` is `NaN` — jsmath.rs documents both.
            standard_integrations: crate::jsmath::math_max(
                0.0,
                self.standard_integrations.unwrap_or(0.0),
            ),
            custom_integrations: crate::jsmath::math_max(
                0.0,
                self.custom_integrations.unwrap_or(0.0),
            ),
            advanced_integrations: crate::jsmath::math_max(
                0.0,
                self.advanced_integrations.unwrap_or(0.0),
            ),
            enterprise_integrations: crate::jsmath::math_max(
                0.0,
                self.enterprise_integrations.unwrap_or(0.0),
            ),
            design_addons: self.design_addons.unwrap_or_default(),
            support: self.support.unwrap_or_else(|| "standard".to_string()),
            // Deliberately no `?? 0` here. See the module comment.
            usage_forecast: self.usage_forecast,
            annual_prepay: self.annual_prepay.unwrap_or(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricebook::RegionCode;

    fn init() -> QuoteRequestInit {
        QuoteRequestInit {
            region: Some(RegionCode::Global),
            presence: Some("chat".to_string()),
            capability: Some("assist".to_string()),
            environment: Some("existing_site".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn omitted_numerics_default_to_zero_not_to_absent() {
        let request = init().into_request();
        assert_eq!(request.standard_integrations, 0.0);
        assert_eq!(request.custom_integrations, 0.0);
        assert_eq!(request.advanced_integrations, 0.0);
        assert_eq!(request.enterprise_integrations, 0.0);
    }

    #[test]
    fn omitted_addons_support_and_prepay_default_to_their_baseline_values() {
        let request = init().into_request();
        assert!(request.design_addons.is_empty());
        assert_eq!(request.support, "standard");
        assert!(!request.annual_prepay);
    }

    #[test]
    fn usage_forecast_stays_distinguishable_from_zero() {
        // The port keeps `None` rather than collapsing it to `0.0`. Both
        // compute the same overage, but they are not the same request.
        let absent = init().into_request();
        let zero = QuoteRequestInit {
            usage_forecast: Some(0.0),
            ..init()
        }
        .into_request();
        assert_eq!(absent.usage_forecast, None);
        assert_eq!(zero.usage_forecast, Some(0.0));
    }

    #[test]
    fn negative_integration_counts_are_clamped_at_zero() {
        // `Math.max(0, -3)` in the baseline.
        let request = QuoteRequestInit {
            standard_integrations: Some(-3.0),
            custom_integrations: Some(-1.5),
            ..init()
        }
        .into_request();
        assert_eq!(request.standard_integrations, 0.0);
        assert_eq!(request.custom_integrations, 0.0);
    }

    #[test]
    fn explicit_values_pass_through_untouched() {
        let request = QuoteRequestInit {
            standard_integrations: Some(4.0),
            design_addons: Some(vec!["advanced_motion_3d".to_string()]),
            support: Some("priority".to_string()),
            annual_prepay: Some(true),
            ..init()
        }
        .into_request();
        assert_eq!(request.standard_integrations, 4.0);
        assert_eq!(request.design_addons, ["advanced_motion_3d"]);
        assert_eq!(request.support, "priority");
        assert!(request.annual_prepay);
    }

    /// The regression test for the bug the unit tests cannot see.
    ///
    /// Every test above builds the struct directly, so none of them exercises
    /// serde's key names. The first parity run therefore disagreed on 164 of
    /// 346 fixtures: `standardIntegrations` was handed to a struct that knew
    /// only `standard_integrations`, serde's default of ignoring unknown keys
    /// dropped it, and the quote was computed as though the field were absent.
    ///
    /// This asserts the *wire* names, from JSON text, which is the only way a
    /// rename that breaks the boundary gets caught here.
    #[test]
    fn json_field_names_are_the_baselines_camel_case() {
        let request: QuoteRequestInit = serde_json::from_str(
            r#"{
              "region": "GLOBAL",
              "presence": "chat",
              "capability": "assist",
              "environment": "existing_site",
              "standardIntegrations": 3,
              "customIntegrations": 1,
              "advancedIntegrations": 2,
              "enterpriseIntegrations": 0,
              "designAddons": ["premium_avatar_provider"],
              "support": "priority",
              "usageForecast": 1500,
              "annualPrepay": true
            }"#,
        )
        .expect("a JSON request the TypeScript engine accepts parses too");

        assert_eq!(request.region, Some(RegionCode::Global));
        assert_eq!(request.presence.as_deref(), Some("chat"));
        assert_eq!(request.standard_integrations, Some(3.0));
        assert_eq!(request.custom_integrations, Some(1.0));
        assert_eq!(request.advanced_integrations, Some(2.0));
        assert_eq!(request.enterprise_integrations, Some(0.0));
        assert_eq!(
            request.design_addons.as_deref(),
            Some(&["premium_avatar_provider".to_string()][..]),
            "designAddons must not be silently dropped — that is the exact bug"
        );
        assert_eq!(request.support.as_deref(), Some("priority"));
        assert_eq!(request.usage_forecast, Some(1500.0));
        assert_eq!(request.annual_prepay, Some(true));
    }

    /// The snake_case spelling must now *fail*, not quietly parse.
    ///
    /// `deny_unknown_fields` inverts the failure mode that hid the bug above:
    /// rather than ignoring a name it does not recognize, the boundary rejects
    /// it, so field-name drift cannot produce a plausible wrong quote.
    #[test]
    fn a_snake_case_spelling_is_rejected_rather_than_ignored() {
        let error = serde_json::from_str::<QuoteRequestInit>(
            r#"{
              "region": "GLOBAL",
              "presence": "chat",
              "capability": "assist",
              "environment": "existing_site",
              "standard_integrations": 3
            }"#,
        )
        .expect_err("snake_case is the Rust spelling, not the wire spelling");
        assert!(
            error.to_string().contains("standard_integrations"),
            "the error should name the offending field, got: {error}"
        );
    }
}
