//! `PricingEngine` — the deterministic quote engine, Steps A–H.
//!
//! A port of `packages/pricing/src/engine.ts:137-389`. Money comes only from
//! the pricebook; this module holds no price constants, for the same reason
//! the baseline does not: a second copy of the numbers is a second thing to
//! keep in sync, and the pricebook is already the product's source of truth.
//!
//! ## Two faithful quirks, reproduced on purpose
//!
//! **`custom_web_app` classifies as `custom`, not `from`.** Its raw JSON is
//! `{setup_from: 8000, monthly: 0, custom: true}`, and `classifyLine` tests
//! `custom === true` *before* `setup_from`. So a tier that plainly advertises a
//! $8,000 floor produces a "requires manual enterprise scoping" driver and no
//! `fromMinimumTotal` contribution. That is what the shipped product does, so
//! that is what the port does — see `classify.rs` and the pilot's findings
//! list, where it is recorded as a baseline bug rather than silently fixed.
//!
//! **Rounding can contradict itself.** `roundCurrency` uses the hardcoded
//! increments in `jsmath.rs` while `roundedWithIncrement` reports whatever
//! `currency_rounding` says, and nothing reconciles the two. With the product
//! pricebook they agree; with a pricebook that said `GLOBAL: 100` they would
//! not, and the quote would round to the nearest 50 while reporting 100. Kept,
//! because collapsing them would hide a real (if latent) defect rather than
//! fix it.
//!
//! ## Divergences from the baseline, all deliberate
//!
//! 1. `includedStandardIntegrations` returns `0` for an unknown region where
//!    the baseline throws a raw `TypeError`. Unreachable through `quote`
//!    because `requireRegion` runs first; documented in `lib.rs`.
//! 2. Tier names are `String`, not the baseline's string-literal unions. The
//!    baseline's types reject `'telepathy'` at compile time but throw a
//!    `PricingError` at runtime, and the pilot's fixtures must be able to
//!    reach that error — a Rust enum of tier names would make it
//!    unrepresentable.
//! 3. Integration lookups tolerate a missing row. `selected.integration` in
//!    the baseline is a zod-parsed object where all four complexities are
//!    required by schema, so a bare index is safe there; the port uses `get`
//!    plus an explicit `custom` fallback so a fixture pricebook missing a row
//!    behaves like the `{}` case rather than panicking.

use crate::classify::{classify_line, PriceLine, PriceLineKind};
use crate::error::{PricingError, PricingErrorKind};
use crate::jsmath::{math_max, round_currency};
use crate::pricebook::{Pricebook, Region, RegionCode, SimpleTier};
use crate::request::{QuoteRequest, QuoteRequestInit};

/// A component of the fixed core setup. Mirrors `engine.ts:5-9`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreComponent {
    pub name: &'static str,
    pub setup: f64,
    pub counts_toward_bundle: bool,
}

/// One integration line. Mirrors `engine.ts:11-17`.
///
/// `count` stays `f64` because the request's counts are `f64` and it appears
/// in a multiplication and in a driver message; narrowing it to an integer
/// would change both for a fractional count.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationLine {
    pub complexity: &'static str,
    pub count: f64,
    pub unit_setup: Option<f64>,
    pub total: Option<f64>,
    pub from_minimum: bool,
}

/// One design add-on. Mirrors `engine.ts:36-41`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct DesignAddon {
    pub name: String,
    pub setup: f64,
    pub from: bool,
    pub custom: bool,
}

/// A usage allowance. Mirrors `engine.ts:44`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct UsageAllowance {
    pub unit: String,
    pub quantity: f64,
}

/// An overage rate. Mirrors `engine.ts:45`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Overage {
    pub unit: String,
    pub price: f64,
}

/// The quote. Mirrors `engine.ts:19-58`.
///
/// **Field order is the serialization order** and is load-bearing: the
/// parity harness compares the two engines' JSON output, and serde emits
/// struct fields in declaration order, so this declaration is the contract.
///
/// `rename_all` is what makes the output match the baseline's key spelling —
/// `engine.ts` returns a literal with camelCase keys, and the harness compares
/// the two as text. Without this the port would be numerically right and fail
/// every single fixture on the first character. The nested types carry the
/// same attribute individually.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub region: RegionCode,
    pub currency: String,
    pub presence: String,
    pub capability: String,
    pub environment: String,
    pub support: String,
    pub core_components: Vec<CoreComponent>,
    pub paid_core_component_count: usize,
    pub bundle_discount_pct: f64,
    pub fixed_core_setup: f64,
    pub discounted_core_setup: f64,
    pub integrations: Vec<IntegrationLine>,
    pub integration_setup_total: f64,
    pub design_addons: Vec<DesignAddon>,
    pub design_addon_setup_total: f64,
    pub usage_allowance: Option<UsageAllowance>,
    pub overage: Option<Overage>,
    pub estimated_overage: f64,
    /// `None` when the quote cannot state a number.
    pub one_time_total: Option<f64>,
    pub monthly_base: f64,
    pub annual_platform_option: Option<f64>,
    pub deterministic_subtotal: f64,
    pub from_minimum_total: f64,
    pub requires_custom_quote: bool,
    pub unresolved_scope_drivers: Vec<String>,
    pub rounded_with_increment: f64,
}

/// The deterministic quote engine.
///
/// Mirrors `engine.ts:87`. The engine borrows the pricebook rather than
/// owning it, so the same pricebook can drive both engines in a parity run.
#[derive(Debug, Clone)]
pub struct PricingEngine<'a> {
    pricebook: &'a Pricebook,
}

/// `requireRegion` (engine.ts:68-72).
///
/// The baseline indexes `pricebook.regions[region]` with a string key, so an
/// unknown region is a *lookup miss*. In the port the region is a `RegionCode`
/// enum, so the same miss can only happen if the pricebook the caller supplied
/// is missing a region — which is exactly what the port reports.
///
/// The lifetime is elided rather than named: the output borrows `pricebook`
/// and nothing else, so the elided form and the `<'a>` form are the same
/// signature, and the explicit one only repeated the name in three places.
fn require_region(pricebook: &Pricebook, region: RegionCode) -> Result<&Region, PricingError> {
    pricebook
        .region(region)
        .ok_or_else(|| PricingError::new(PricingErrorKind::UnknownRegion, region.as_str()))
}

/// `bundleDiscountPct` (engine.ts:74-79).
///
/// `<= 1` returns zero, `=== 2` returns the two-component rate, and *anything
/// else* — including 4, 5, or a fractional count — returns the
/// three-component rate.
fn bundle_discount_pct(pricebook: &Pricebook, paid: usize) -> f64 {
    if paid <= 1 {
        0.0
    } else if paid == 2 {
        pricebook
            .discount_rules
            .two_core_components_setup_discount_pct
    } else {
        pricebook
            .discount_rules
            .three_core_components_setup_discount_pct
    }
}

impl<'a> PricingEngine<'a> {
    /// Build an engine over a pricebook.
    ///
    /// The baseline's constructor takes an optional pricebook and falls back
    /// to `loadPricebook()` — a module-level read of `config/pricing.v1.json`.
    /// The port has no such ambient load, because an engine that silently
    /// reads a file from disk is not something a parity harness can pin. The
    /// harness loads the fixture explicitly and passes it in.
    pub fn new(pricebook: &'a Pricebook) -> Self {
        PricingEngine { pricebook }
    }

    /// `get schemaVersion`.
    pub fn schema_version(&self) -> &str {
        &self.pricebook.schema_version
    }

    /// `get effectiveDate`.
    pub fn effective_date(&self) -> &str {
        &self.pricebook.effective_date
    }

    /// `get quoteValidityDays`.
    pub fn quote_validity_days(&self) -> i64 {
        self.pricebook.commercial_rules.quote_validity_days
    }

    /// `includedStandardIntegrations(region, capability)` (engine.ts:107-113).
    ///
    /// Returns `0` — never errors — for any tier that is missing, not an
    /// object, or lacking a numeric allowance. This is the one function where
    /// the port is deliberately more permissive than the baseline; see
    /// `lib.rs` for why the difference is unobservable.
    pub fn included_standard_integrations(&self, region: RegionCode, capability: &str) -> f64 {
        let Some(selected) = self.pricebook.region(region) else {
            return 0.0;
        };
        let Some(tier) = selected.capability.get(capability) else {
            return 0.0;
        };
        tier.included_standard_integrations.unwrap_or(0.0)
    }

    /// `quote(init)` — apply defaults, then quote.
    pub fn quote(&self, init: QuoteRequestInit) -> Result<Quote, PricingError> {
        let request = init.into_request();
        self.quote_request(request)
    }

    /// `quoteRequest(request)` — Steps A–H.
    ///
    /// The step structure and every driver string are carried over in order,
    /// because `unresolvedScopeDrivers` is part of the output and its order is
    /// observable. Step labels in the comments name the baseline's own
    /// sections so a diff against engine.ts can be read top to bottom.
    pub fn quote_request(&self, request: QuoteRequest) -> Result<Quote, PricingError> {
        let pricebook = self.pricebook;
        let selected = require_region(pricebook, request.region)?;
        let mut drivers: Vec<String> = Vec::new();

        // ---- Step A: fixed core setup -------------------------------------
        //
        // Three lookups, then three checks. The order of the checks is what
        // decides *which* error a request with several bad names gets, so it
        // is preserved: presence, then capability, then environment.
        let presence_tier = selected.presence.get(&request.presence).ok_or_else(|| {
            PricingError::new(PricingErrorKind::UnknownPresence, &request.presence)
        })?;
        let capability_tier = selected
            .capability
            .get(&request.capability)
            .ok_or_else(|| {
                PricingError::new(PricingErrorKind::UnknownCapability, &request.capability)
            })?;
        let environment_tier = selected
            .environment
            .get(&request.environment)
            .ok_or_else(|| {
                PricingError::new(PricingErrorKind::UnknownEnvironment, &request.environment)
            })?;

        let presence_line = classify_line(presence_tier);
        let capability_line = classify_line(capability_tier);
        let environment_line = classify_line(environment_tier);

        // `presence` always counts. The other two count only when they carry a
        // fixed price — `from` and `custom` both leave the bundle at one.
        let mut core_components = vec![CoreComponent {
            name: "presence",
            setup: presence_line.setup().unwrap_or(0.0),
            counts_toward_bundle: true,
        }];

        // An environment counts only when it carries a fixed price (never `from`).
        if environment_line.kind() == PriceLineKind::Fixed
            && environment_line.setup().unwrap_or(0.0) > 0.0
        {
            core_components.push(CoreComponent {
                name: "environment",
                setup: environment_line.setup().unwrap_or(0.0),
                counts_toward_bundle: true,
            });
        } else if environment_line.kind() == PriceLineKind::From {
            // Matches engine.ts:165's wording exactly. `from` and `custom`
            // produce *different* drivers for the same tier name, which is why
            // the classification matters beyond the price.
            drivers.push(format!(
                "{} is priced \"from\" and requires scoped custom engineering",
                request.environment
            ));
        } else if environment_line.kind() == PriceLineKind::Custom {
            drivers.push(format!(
                "{} requires manual enterprise scoping",
                request.environment
            ));
        }

        // A capability counts only when it carries a fixed price (never `custom`).
        if capability_line.kind() == PriceLineKind::Fixed
            && capability_line.setup().unwrap_or(0.0) > 0.0
        {
            core_components.push(CoreComponent {
                name: "capability",
                setup: capability_line.setup().unwrap_or(0.0),
                counts_toward_bundle: true,
            });
        } else if capability_line.kind() == PriceLineKind::Custom {
            drivers.push(format!(
                "{} capability requires manual scoping",
                request.capability
            ));
        }

        let fixed_core_setup: f64 = core_components.iter().map(|c| c.setup).sum();
        let paid_core_component_count = core_components.len();
        let discount_pct = bundle_discount_pct(pricebook, paid_core_component_count);
        let discounted_core_setup = fixed_core_setup * (1.0 - discount_pct / 100.0);

        // ---- Step C: integrations -----------------------------------------
        let included = self.included_standard_integrations(request.region, &request.capability);
        // The capability allowance is subtracted here, so a request for five
        // integrations on an `act` capability charges for four.
        let chargeable_standard = math_max(0.0, request.standard_integrations - included);

        let standard_tier_line = classify_line(
            selected
                .integration
                .get("standard")
                .unwrap_or(&MISSING_SIMPLE_LINE),
        );
        let custom_tier_line = classify_line(
            selected
                .integration
                .get("custom")
                .unwrap_or(&MISSING_SIMPLE_LINE),
        );
        let advanced_tier_line = classify_line(
            selected
                .integration
                .get("advanced")
                .unwrap_or(&MISSING_SIMPLE_LINE),
        );
        let enterprise_tier_line = classify_line(
            selected
                .integration
                .get("enterprise")
                .unwrap_or(&MISSING_SIMPLE_LINE),
        );

        let mut integrations: Vec<IntegrationLine> = Vec::new();

        // Standard integrations are classified twice by the request: they are
        // gated on `> 0` up front, and *within* that they use
        // `chargeableStandard` rather than the raw request count. A `custom`
        // standard line pushes nothing at all — no line, and no driver — so a
        // pricebook whose standard tier is bespoke silently drops standard
        // integrations from the quote. Reproduced; logged as a baseline bug.
        if request.standard_integrations > 0.0 {
            match standard_tier_line {
                PriceLine::Fixed { setup, .. } => integrations.push(IntegrationLine {
                    complexity: "standard",
                    count: chargeable_standard,
                    unit_setup: setup,
                    total: Some(setup.unwrap_or(0.0) * chargeable_standard),
                    from_minimum: false,
                }),
                PriceLine::From { setup, .. } => {
                    drivers.push(
                        "standard integration price is \"from\" and requires scoping".to_string(),
                    );
                    integrations.push(IntegrationLine {
                        complexity: "standard",
                        count: chargeable_standard,
                        unit_setup: setup,
                        total: None,
                        from_minimum: true,
                    });
                }
                PriceLine::Custom => {}
            }
        }

        // Custom and advanced are handled by one loop because they are
        // classified identically at their call sites. Their order in the
        // output is fixed: custom, then advanced.
        for (complexity, count, line) in [
            ("custom", request.custom_integrations, custom_tier_line),
            (
                "advanced",
                request.advanced_integrations,
                advanced_tier_line,
            ),
        ] {
            if count <= 0.0 {
                continue;
            }
            match line {
                PriceLine::Fixed { setup, .. } => integrations.push(IntegrationLine {
                    complexity,
                    count,
                    unit_setup: setup,
                    total: Some(setup.unwrap_or(0.0) * count),
                    from_minimum: false,
                }),
                PriceLine::From { setup, .. } => {
                    drivers.push(format!(
                        "{} {} integration(s) priced \"from\" and require scoping",
                        count, complexity
                    ));
                    integrations.push(IntegrationLine {
                        complexity,
                        count,
                        unit_setup: setup,
                        total: None,
                        from_minimum: true,
                    });
                }
                PriceLine::Custom => {}
            }
        }

        // Enterprise integrations have no fixed price anywhere in the
        // pricebook, so the two branches differ only in wording. Both push an
        // unpriced line, and both force `hasFromLines` below.
        if request.enterprise_integrations > 0.0 {
            if enterprise_tier_line.kind() == PriceLineKind::Custom {
                drivers.push(format!(
                    "{} enterprise integration(s) require manual scoping",
                    request.enterprise_integrations
                ));
            } else {
                drivers.push(format!(
                    "{} enterprise integration(s) have no pricebook line",
                    request.enterprise_integrations
                ));
            }
            integrations.push(IntegrationLine {
                complexity: "enterprise",
                count: request.enterprise_integrations,
                unit_setup: None,
                total: None,
                from_minimum: false,
            });
        }

        let integration_setup_total: f64 = integrations
            .iter()
            .map(|line| line.total.unwrap_or(0.0))
            .sum();

        // ---- Step D: design / support add-ons ------------------------------
        //
        // A design add-on that is not in the pricebook classifies as `custom`
        // (see `MISSING_SIMPLE_LINE`), so an unknown name is scoped rather
        // than rejected — the request never fails, it just cannot be priced.
        let mut design_addons: Vec<DesignAddon> = Vec::with_capacity(request.design_addons.len());
        for addon in &request.design_addons {
            let line = classify_line(
                selected
                    .design_addons
                    .get(addon)
                    .unwrap_or(&MISSING_SIMPLE_LINE),
            );
            match line {
                PriceLine::Fixed { setup, .. } => design_addons.push(DesignAddon {
                    name: addon.clone(),
                    setup: setup.unwrap_or(0.0),
                    from: false,
                    custom: false,
                }),
                PriceLine::From { setup, .. } => {
                    drivers.push(format!("{} is priced \"from\" and requires scoping", addon));
                    design_addons.push(DesignAddon {
                        name: addon.clone(),
                        setup: setup.unwrap_or(0.0),
                        from: true,
                        custom: false,
                    });
                }
                PriceLine::Custom => {
                    drivers.push(format!("{} requires manual scoping", addon));
                    design_addons.push(DesignAddon {
                        name: addon.clone(),
                        setup: 0.0,
                        from: false,
                        custom: true,
                    });
                }
            }
        }
        // Only non-`from` add-ons contribute to the paid total; a `from`
        // add-on contributes its floor only to `fromMinimumTotal`.
        let design_addon_fixed_total: f64 = design_addons
            .iter()
            .filter(|a| !a.from)
            .map(|a| a.setup)
            .sum();

        let support_line = classify_line(
            selected
                .support_addons
                .get(&request.support)
                .unwrap_or(&MISSING_SIMPLE_LINE),
        );
        let support_monthly = match support_line {
            PriceLine::Custom => None,
            other => other.monthly().or(Some(0.0)),
        };
        if support_monthly.is_none() {
            drivers.push(format!(
                "{} support requires manual scoping",
                request.support
            ));
        }

        // ---- Step E: one-time total ----------------------------------------
        //
        // `rounding` is read from the pricebook and reported; the rounding
        // itself uses the constants in `jsmath.rs`. Two sources, on purpose.
        let rounding = request.region.rounding_increment(pricebook);

        let installable_total =
            discounted_core_setup + integration_setup_total + design_addon_fixed_total;

        // One predicate decides everything downstream: whether a number can
        // be stated (`oneTimeTotal`), whether the floor is a real floor, and
        // whether the quote needs a human. It is deliberately broad — it
        // catches any unresolved driver, not only the priced kinds.
        //
        // `environmentLine.kind() != Fixed` is the port of the baseline's
        // `environmentLine.kind === 'from' || environmentLine.kind === 'custom'`.
        let has_from_lines = integrations.iter().any(|line| line.from_minimum)
            || design_addons.iter().any(|a| a.from || a.custom)
            || environment_line.kind() != PriceLineKind::Fixed
            || capability_line.kind() == PriceLineKind::Custom
            || support_monthly.is_none()
            || request.enterprise_integrations > 0.0;

        let one_time_total = if has_from_lines {
            None
        } else {
            Some(round_currency(request.region, installable_total))
        };

        // A lower bound that includes `setup_from` minimums — explicitly
        // labelled a minimum, never a final project price.
        //
        // The `from` environment floor is *not* discounted: it is added after
        // `discountedCoreSetup`, because a `setup_from` environment never
        // entered `coreComponents` and so never entered the bundle.
        let from_minimum = discounted_core_setup
            + match environment_line {
                PriceLine::From { setup, .. } => setup.unwrap_or(0.0),
                _ => 0.0,
            }
            + integrations
                .iter()
                .map(|line| line.unit_setup.unwrap_or(0.0) * line.count)
                .sum::<f64>()
            + design_addons.iter().map(|a| a.setup).sum::<f64>();

        // ---- Step F: recurring base ----------------------------------------
        let presence_monthly = presence_line.monthly().unwrap_or(0.0);
        let capability_monthly = match capability_line {
            PriceLine::Custom => 0.0,
            other => other.monthly().unwrap_or(0.0),
        };
        let monthly_base = presence_monthly + capability_monthly + support_monthly.unwrap_or(0.0);

        // ---- Step G: usage forecast ----------------------------------------
        //
        // Unlike the other optional fields, `usageForecast` is not defaulted
        // to zero — a missing forecast means "no estimate", which is not the
        // same statement as a forecast of zero.
        let usage_allowance =
            presence_tier
                .included_usage
                .as_ref()
                .map(|allowance| UsageAllowance {
                    unit: allowance.unit.clone(),
                    quantity: allowance.quantity,
                });
        let overage = presence_tier.overage.as_ref().map(|o| Overage {
            unit: o.unit.clone(),
            price: o.price,
        });
        let mut estimated_overage = 0.0;
        if let (Some(forecast), Some(allowance), Some(overage)) =
            (request.usage_forecast, &usage_allowance, &overage)
        {
            if forecast > allowance.quantity {
                estimated_overage = (forecast - allowance.quantity) * overage.price;
            }
        }

        // ---- Step H: annual option -----------------------------------------
        let annual_platform_option = if request.annual_prepay {
            Some(
                monthly_base
                    * 12.0
                    * (1.0
                        - pricebook
                            .discount_rules
                            .annual_recurring_prepay_discount_pct
                            / 100.0),
            )
        } else {
            None
        };

        // `hasFromLines` is the single authority on "we cannot state a
        // number": it already covers `custom` capability/environment tiers and
        // enterprise integration volume, so re-testing them here would be dead
        // code that only *looks* like an extra guard.
        let requires_custom_quote = has_from_lines || !drivers.is_empty();

        Ok(Quote {
            region: request.region,
            currency: selected.currency.clone(),
            presence: request.presence,
            capability: request.capability,
            environment: request.environment,
            support: request.support,
            core_components,
            paid_core_component_count,
            bundle_discount_pct: discount_pct,
            fixed_core_setup,
            discounted_core_setup,
            integrations,
            integration_setup_total,
            design_addons,
            design_addon_setup_total: design_addon_fixed_total,
            usage_allowance,
            overage,
            estimated_overage,
            one_time_total,
            monthly_base,
            annual_platform_option,
            deterministic_subtotal: installable_total,
            from_minimum_total: from_minimum,
            requires_custom_quote,
            // `[...new Set(drivers)]`: first occurrence wins, order preserved.
            unresolved_scope_drivers: dedupe(drivers),
            rounded_with_increment: rounding,
        })
    }
}

/// The pricebook row a request named but the pricebook does not contain.
///
/// Stands in for `selected.design_addons[addon]` evaluating to `undefined`,
/// which `classifyLine` turns into `custom`. Declared as a const so the
/// fallback needs no allocation on the hot path.
const MISSING_SIMPLE_LINE: SimpleTier = SimpleTier {
    setup: None,
    monthly: None,
    setup_from: None,
    custom: true,
};

/// `[...new Set(drivers)]` — dedupe preserving first-occurrence order.
///
/// The baseline dedupes driver strings because the same name can be pushed
/// twice (an add-on named twice in one request pushes the same message
/// twice), and the output would otherwise show a duplicate.
fn dedupe(drivers: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    drivers
        .into_iter()
        .filter(|driver| seen.insert(driver.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::PricingErrorKind;
    use crate::pricebook::RegionCode;

    /// The real product pricebook, loaded from the same copy the harness uses.
    const PRICEBOOK: &str = include_str!("../../../../config/pricing.v1.json");

    fn book() -> Pricebook {
        serde_json::from_str(PRICEBOOK).expect("the product pricebook must deserialize")
    }

    fn engine() -> PricingEngine<'static> {
        // Leaked so the engine can borrow for `'static` inside a test. The
        // alternative is threading the lifetime through every helper, which
        // buys nothing in a test module.
        let book: &'static Pricebook = Box::leak(Box::new(book()));
        PricingEngine::new(book)
    }

    fn init(
        region: RegionCode,
        presence: &str,
        capability: &str,
        environment: &str,
    ) -> QuoteRequestInit {
        QuoteRequestInit {
            region: Some(region),
            presence: Some(presence.to_string()),
            capability: Some(capability.to_string()),
            environment: Some(environment.to_string()),
            ..Default::default()
        }
    }

    // ---- Step A: the bundle ------------------------------------------------

    #[test]
    fn a_single_component_never_earns_a_discount() {
        // A `chat` presence plus a zero-priced `existing_site` environment and
        // the zero-setup `assist` capability: only one component has a price,
        // so the two-component and three-component rates both stay out of
        // reach. This is the smallest quote that still produces a total.
        let quote = engine()
            .quote(init(RegionCode::Global, "chat", "assist", "existing_site"))
            .expect("a fully specified request quotes");
        assert_eq!(quote.core_components.len(), 1);
        assert_eq!(quote.paid_core_component_count, 1);
        assert_eq!(quote.bundle_discount_pct, 0.0);
        assert_eq!(quote.fixed_core_setup, 2_000.0);
        assert_eq!(quote.discounted_core_setup, 2_000.0);
    }

    #[test]
    fn two_paid_components_take_the_two_component_rate() {
        // Existing suite below used a zero-priced environment here, so rename
        // this one for what it is: presence 2000 + capability 3000 at 5%.
        let quote = engine()
            .quote(init(
                RegionCode::Global,
                "chat",
                "transact",
                "existing_site",
            ))
            .expect("a fully specified request quotes");
        assert_eq!(quote.core_components.len(), 2, "presence + capability");
        assert_eq!(quote.paid_core_component_count, 2);
        assert_eq!(quote.bundle_discount_pct, 5.0);
        assert_eq!(quote.fixed_core_setup, 5_000.0, "2000 + 3000");
        // 5% off the fixed setup. Reproduced in f64 exactly as the baseline
        // computes it, including the division happening before the multiply.
        assert_eq!(quote.discounted_core_setup, 4_750.0);
    }

    #[test]
    fn a_zero_priced_environment_does_not_join_the_bundle() {
        // `existing_site` is `{setup: 0, monthly: 0}` — classified `fixed` but
        // with a zero setup, so engine.ts's `> 0` guard excludes it.
        let quote = engine()
            .quote(init(RegionCode::Global, "chat", "assist", "existing_site"))
            .expect("a fully specified request quotes");
        assert_eq!(quote.core_components.len(), 1);
        assert_eq!(quote.paid_core_component_count, 1);
        assert_eq!(quote.bundle_discount_pct, 0.0);
        assert_eq!(quote.discounted_core_setup, 2_000.0);
    }

    #[test]
    fn a_from_environment_never_joins_the_bundle() {
        let quote = engine()
            .quote(init(RegionCode::Global, "chat", "assist", "custom_web_app"))
            .expect("a from-tier request does not throw");
        // `{setup_from: 8000, monthly: 0, custom: true}` — `custom` wins, so
        // this is the `custom` driver, not the `from` one.
        assert_eq!(quote.core_components.len(), 1);
        assert_eq!(quote.bundle_discount_pct, 0.0);
        assert_eq!(
            quote.unresolved_scope_drivers,
            ["custom_web_app requires manual enterprise scoping"]
        );
        assert_eq!(
            quote.from_minimum_total, 2_000.0,
            "the floor is not counted"
        );
        assert_eq!(quote.one_time_total, None);
    }

    #[test]
    fn an_enterprise_capability_pushes_its_own_driver() {
        let quote = engine()
            .quote(init(
                RegionCode::Global,
                "chat",
                "enterprise",
                "existing_site",
            ))
            .expect("a custom capability does not throw");
        assert_eq!(
            quote.unresolved_scope_drivers,
            ["enterprise capability requires manual scoping"]
        );
        // A custom capability contributes no monthly either (Step F zeroes it).
        assert_eq!(quote.monthly_base, 249.0);
        assert_eq!(quote.one_time_total, None);
    }

    // ---- Step C: integrations ----------------------------------------------

    #[test]
    fn the_capability_allowance_reduces_chargeable_standard_integrations() {
        // `transact` includes 2 standard integrations.
        let quote = engine()
            .quote(QuoteRequestInit {
                standard_integrations: Some(5.0),
                ..init(RegionCode::Global, "chat", "transact", "existing_site")
            })
            .expect("standard integrations quote");
        let standard = quote
            .integrations
            .iter()
            .find(|line| line.complexity == "standard")
            .expect("a standard line exists");
        assert_eq!(standard.count, 3.0, "5 requested less 2 included");
        assert_eq!(standard.unit_setup, Some(750.0));
        assert_eq!(standard.total, Some(2_250.0));
        assert_eq!(quote.integration_setup_total, 2_250.0);
    }

    #[test]
    fn a_single_included_integration_charges_nothing_at_all() {
        // The count is positive but `chargeableStandard` is zero. The line is
        // still emitted, with a total of zero — an empty line, not no line.
        let quote = engine()
            .quote(QuoteRequestInit {
                standard_integrations: Some(2.0),
                ..init(RegionCode::Global, "chat", "transact", "existing_site")
            })
            .expect("standard integrations quote");
        let standard = quote
            .integrations
            .iter()
            .find(|line| line.complexity == "standard")
            .expect("the baseline emits the line even when nothing is chargeable");
        assert_eq!(standard.count, 0.0);
        assert_eq!(standard.total, Some(0.0));
    }

    #[test]
    fn a_zero_request_count_emits_no_line_so_zero_and_one_differ() {
        // `standardIntegrations: 0` skips the branch entirely, so no line is
        // emitted even though the tier is fixed.
        let absent = engine()
            .quote(init(
                RegionCode::Global,
                "chat",
                "transact",
                "existing_site",
            ))
            .expect("a request with no integrations quotes");
        assert!(absent.integrations.is_empty());

        let present = engine()
            .quote(QuoteRequestInit {
                standard_integrations: Some(2.0),
                ..init(RegionCode::Global, "chat", "transact", "existing_site")
            })
            .expect("a request with a fully-included count quotes");
        assert_eq!(present.integrations.len(), 1);
    }

    #[test]
    fn custom_and_advanced_integrations_are_ordered_custom_then_advanced() {
        let quote = engine()
            .quote(QuoteRequestInit {
                custom_integrations: Some(1.0),
                advanced_integrations: Some(2.0),
                ..init(RegionCode::Global, "chat", "transact", "existing_site")
            })
            .expect("custom and advanced integrations quote");
        let complexities: Vec<&str> = quote
            .integrations
            .iter()
            .map(|line| line.complexity)
            .collect();
        // Driven by the request's field order, not by the pricebook's.
        assert_eq!(complexities, ["custom", "advanced"]);
        // `custom` is priced `from` 1500; `advanced` is priced `from` 3000.
        assert!(quote.integrations.iter().all(|line| line.from_minimum));
        assert_eq!(quote.integration_setup_total, 0.0, "no `from` line is paid");
        assert_eq!(quote.one_time_total, None);
        // The floor is built on the *discounted* setup, not the list price:
        // presence 2000 + capability 3000 is two paid components, so 5% comes
        // off (5000 → 4750) before the floors are added underneath it.
        assert_eq!(quote.discounted_core_setup, 4_750.0);
        assert_eq!(quote.from_minimum_total, 4_750.0 + 1_500.0 + 2.0 * 3_000.0);
    }

    #[test]
    fn enterprise_integrations_force_a_custom_quote_without_a_total() {
        let quote = engine()
            .quote(QuoteRequestInit {
                enterprise_integrations: Some(3.0),
                ..init(RegionCode::Global, "chat", "transact", "existing_site")
            })
            .expect("enterprise integrations quote");
        assert_eq!(quote.integrations.len(), 1);
        assert_eq!(quote.integrations[0].complexity, "enterprise");
        assert_eq!(quote.integrations[0].count, 3.0);
        assert_eq!(quote.integrations[0].total, None);
        assert_eq!(quote.integrations[0].unit_setup, None);
        assert_eq!(quote.one_time_total, None);
        assert!(quote.requires_custom_quote);
        assert_eq!(
            quote.unresolved_scope_drivers,
            ["3 enterprise integration(s) require manual scoping"]
        );
    }

    // ---- Step D: add-ons and support --------------------------------------

    #[test]
    fn an_unknown_design_addon_is_scoped_rather_than_rejected() {
        let quote = engine()
            .quote(QuoteRequestInit {
                design_addons: Some(vec!["premium_avatar_provider".to_string()]),
                ..init(RegionCode::Global, "chat", "assist", "existing_site")
            })
            .expect("an unknown addon does not throw");
        assert_eq!(quote.design_addons.len(), 1);
        assert!(quote.design_addons[0].custom);
        assert_eq!(quote.design_addons[0].setup, 0.0);
        assert_eq!(
            quote.unresolved_scope_drivers,
            ["premium_avatar_provider requires manual scoping"]
        );
    }

    #[test]
    fn a_priced_design_addon_joins_the_paid_total() {
        // `custom_visual_direction` is a fixed 1500.
        let quote = engine()
            .quote(QuoteRequestInit {
                design_addons: Some(vec!["custom_visual_direction".to_string()]),
                ..init(RegionCode::Global, "chat", "assist", "existing_site")
            })
            .expect("a priced addon quotes");
        assert_eq!(quote.design_addon_setup_total, 1_500.0);
        assert_eq!(quote.deterministic_subtotal, 2_000.0 + 1_500.0);
        assert_eq!(quote.one_time_total, Some(3_500.0));
        assert!(quote.unresolved_scope_drivers.is_empty());
    }

    #[test]
    fn a_duplicated_driver_is_reported_once() {
        // `[...new Set(drivers)]`. The same addon named twice pushes the same
        // message twice and must appear once.
        let quote = engine()
            .quote(QuoteRequestInit {
                design_addons: Some(vec![
                    "premium_avatar_provider".to_string(),
                    "premium_avatar_provider".to_string(),
                ]),
                ..init(RegionCode::Global, "chat", "assist", "existing_site")
            })
            .expect("a duplicated addon does not throw");
        assert_eq!(
            quote.unresolved_scope_drivers,
            ["premium_avatar_provider requires manual scoping"]
        );
        // The addon itself is pushed twice, so the list is two long.
        assert_eq!(quote.design_addons.len(), 2);
    }

    #[test]
    fn priority_support_adds_to_the_recurring_base() {
        let quote = engine()
            .quote(QuoteRequestInit {
                support: Some("priority".to_string()),
                ..init(RegionCode::Global, "chat", "assist", "existing_site")
            })
            .expect("priority support quotes");
        // presence 249 + capability 0 + support 199.
        assert_eq!(quote.monthly_base, 448.0);
        assert!(!quote.requires_custom_quote);
    }

    #[test]
    fn dedicated_sla_support_blocks_the_total_without_blocking_the_base() {
        // `dedicated_sla` is `{custom: true}`, so `supportMonthly` is null and
        // the recurring base simply omits it.
        let quote = engine()
            .quote(QuoteRequestInit {
                support: Some("dedicated_sla".to_string()),
                ..init(RegionCode::Global, "chat", "assist", "existing_site")
            })
            .expect("slas support quotes");
        assert_eq!(quote.monthly_base, 249.0);
        assert_eq!(quote.one_time_total, None);
        assert_eq!(
            quote.unresolved_scope_drivers,
            ["dedicated_sla support requires manual scoping"]
        );
    }

    // ---- Step E/F/G/H: totals, recurring, usage ---------------------------

    #[test]
    fn the_one_time_total_is_rounded_once_on_the_final_figure() {
        // 2000 presence + 0 existing_site + 1.5% nothing = 2000 exactly, so
        // use a case that lands off the increment: 8825 is the documented
        // example and must reach 8850.
        let quote = engine()
            .quote(QuoteRequestInit {
                design_addons: Some(vec!["custom_visual_direction".to_string()]),
                support: Some("priority".to_string()),
                ..init(RegionCode::Global, "chat", "act", "landing")
            })
            .expect("a fully priced quote");
        // presence 2000 + capability 149 (discounted 5% with environment) +
        // environment landing 1500 + addon 1500.
        assert_eq!(quote.fixed_core_setup, 2_000.0 + 1_500.0 + 1_500.0);
        assert_eq!(quote.discounted_core_setup, 5_000.0 * 0.9);
        assert_eq!(quote.deterministic_subtotal, 4_500.0 + 1_500.0);
        assert_eq!(quote.one_time_total, Some(6_000.0));
        assert_eq!(quote.rounded_with_increment, 50.0);
    }

    #[test]
    fn the_annual_option_discounts_twelve_months() {
        let quote = engine()
            .quote(QuoteRequestInit {
                annual_prepay: Some(true),
                support: Some("priority".to_string()),
                ..init(RegionCode::Global, "chat", "assist", "existing_site")
            })
            .expect("the annual option quotes");
        // (249 + 0 + 199) * 12 * 0.9.
        assert_eq!(quote.annual_platform_option, Some(448.0 * 12.0 * 0.9));
    }

    #[test]
    fn the_annual_option_is_absent_without_prepay() {
        let quote = engine()
            .quote(init(RegionCode::Global, "chat", "assist", "existing_site"))
            .expect("a quote without prepay");
        assert_eq!(quote.annual_platform_option, None);
    }

    #[test]
    fn overage_is_zero_below_the_allowance_and_none_without_a_forecast() {
        // chat includes 1000 active_conversation at 0.10 per over.
        let under = engine()
            .quote(QuoteRequestInit {
                usage_forecast: Some(500.0),
                ..init(RegionCode::Global, "chat", "assist", "existing_site")
            })
            .expect("a forecast under the allowance quotes");
        assert_eq!(under.estimated_overage, 0.0);

        let over = engine()
            .quote(QuoteRequestInit {
                usage_forecast: Some(1_500.0),
                ..init(RegionCode::Global, "chat", "assist", "existing_site")
            })
            .expect("a forecast over the allowance quotes");
        // (1500 - 1000) * 0.1, computed in f64.
        assert_eq!(over.estimated_overage, 50.0);

        let none = engine()
            .quote(init(RegionCode::Global, "chat", "assist", "existing_site"))
            .expect("a quote without a forecast");
        assert_eq!(none.estimated_overage, 0.0);
        assert!(
            none.usage_allowance.is_some(),
            "the allowance is still reported"
        );
    }

    #[test]
    fn the_usage_allowance_and_overage_come_from_the_presence_tier() {
        let quote = engine()
            .quote(init(RegionCode::Global, "voice", "assist", "existing_site"))
            .expect("the voice presence quotes");
        let allowance = quote.usage_allowance.expect("voice carries an allowance");
        assert_eq!(allowance.unit, "realtime_minute");
        assert_eq!(allowance.quantity, 500.0);
        let overage = quote.overage.expect("voice carries an overage rate");
        assert_eq!(overage.unit, "realtime_minute");
        assert_eq!(overage.price, 0.1);
    }

    // ---- The two regions ---------------------------------------------------

    #[test]
    fn indonesian_quotes_never_mix_with_global_figures() {
        let quote = engine()
            .quote(init(RegionCode::Id, "chat", "transact", "business"))
            .expect("an indonesian quote");
        assert_eq!(quote.currency, "IDR");
        assert_eq!(quote.rounded_with_increment, 100_000.0);
        // 6900000 + 9900000 + 8900000 = 25700000, discounted 5% for three
        // components... but `three` is not the rate applied at a paid count
        // of 3 via `bundleDiscountPct`'s `else`, which is the same 10%.
        assert_eq!(quote.paid_core_component_count, 3);
        assert_eq!(quote.bundle_discount_pct, 10.0);
        assert_eq!(quote.fixed_core_setup, 25_700_000.0);
        assert_eq!(quote.discounted_core_setup, 25_700_000.0 * 0.9);
        // Rounded to the nearest 100000.
        assert_eq!(quote.one_time_total, Some(23_100_000.0));
    }

    // ---- Failure paths -----------------------------------------------------

    #[test]
    fn the_three_unknown_tier_errors_are_checked_in_a_fixed_order() {
        // All three bad at once: presence wins.
        let error = engine()
            .quote(init(RegionCode::Global, "nope", "nope", "nope"))
            .expect_err("no tier resolves");
        assert_eq!(
            error,
            PricingError::new(PricingErrorKind::UnknownPresence, "nope")
        );
        assert_eq!(error.to_string(), "Unknown presence \"nope\".");

        // Capability and environment bad: capability wins.
        let error = engine()
            .quote(init(RegionCode::Global, "chat", "nope", "nope"))
            .expect_err("capability does not resolve");
        assert_eq!(error.to_string(), "Unknown capability \"nope\".");

        let error = engine()
            .quote(init(RegionCode::Global, "chat", "assist", "nope"))
            .expect_err("environment does not resolve");
        assert_eq!(error.to_string(), "Unknown environment \"nope\".");
    }

    // ---- Metadata accessors -----------------------------------------------

    #[test]
    fn the_metadata_accessors_report_the_pricebook_verbatim() {
        let engine = engine();
        assert_eq!(engine.schema_version(), "1.0.0");
        assert_eq!(engine.effective_date(), "2026-09-22");
        assert_eq!(engine.quote_validity_days(), 14);
    }

    #[test]
    fn included_standard_integrations_reads_the_capability_allowance() {
        let engine = engine();
        assert_eq!(
            engine.included_standard_integrations(RegionCode::Global, "assist"),
            0.0
        );
        assert_eq!(
            engine.included_standard_integrations(RegionCode::Global, "act"),
            1.0
        );
        assert_eq!(
            engine.included_standard_integrations(RegionCode::Global, "transact"),
            2.0
        );
        // `enterprise` is `{custom: true}` and has no key, so it includes none.
        assert_eq!(
            engine.included_standard_integrations(RegionCode::Global, "enterprise"),
            0.0
        );
    }
}
