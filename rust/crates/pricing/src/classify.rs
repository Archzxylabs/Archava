//! `classifyLine` — the pricebook→engine translation layer.
//!
//! Every money-bearing tier in the pricebook passes through here before the
//! engine reads it, and the classification decides *how* a tier is charged:
//! `fixed` states a number, `from` states a floor plus a scope driver, and
//! `custom` refuses to state anything. The baseline's version takes `unknown`
//! and narrows with `typeof`, so the Rust port needs a way to express
//! "this key held a JSON number" rather than "this field is a non-null float".
//!
//! That is what [`TierFields`] is for. Implementing it for the four tier
//! shapes in `pricebook.rs` lets `classify_line` stay a total function on
//! typed data while preserving the exact four decisions the TypeScript makes.
//!
//! ## The one place this port is deliberately looser than the baseline
//!
//! `typeof record.setup === 'number'` is false for the string `"2000"`, so the
//! baseline falls through to the next check and eventually reports `custom`.
//! A Rust `Option<f64>` cannot represent a key that holds a string at all —
//! deserialization of the whole pricebook fails instead. That is a
//! difference, but it lives in pricebook parsing, not here: the zod schema
//! (`packages/config/src/pricing/schema.ts`) already requires a number in
//! every one of those positions, so such a pricebook is rejected by the
//! baseline too — just at a different boundary, with a different message.
//! No pricebook the product can actually load behaves differently.

/// The engine's internal view of one pricebook line.
///
/// `setup` and `monthly` are optional because the zod unions make them
/// optional independently: after parsing, a line really can be `{monthly: 199}`
/// with no setup, and the engine's `?? 0` / `?? null` at the use sites is what
/// turns that into money. Inventing a zero here would move that decision to
/// the wrong place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PriceLine {
    /// A stated price. `setup` may still be absent (`{monthly: 199}`), and
    /// `classify_line` materializes `setup: Some(0.0)` for that shape.
    Fixed {
        setup: Option<f64>,
        monthly: Option<f64>,
    },
    /// A stated floor plus scope work. Always carries a driver message.
    From {
        setup: Option<f64>,
        monthly: Option<f64>,
    },
    /// No price is knowable; the quote must say so.
    Custom,
}

/// The three classifications, named for matching.
///
/// Exposed as an accessor rather than as `enum` matching at every call site
/// because the engine's branches are not exhaustive over the variants — it
/// frequently tests one kind and treats the other two together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceLineKind {
    Fixed,
    From,
    Custom,
}

impl PriceLine {
    /// Which classification this is.
    pub fn kind(&self) -> PriceLineKind {
        match self {
            PriceLine::Fixed { .. } => PriceLineKind::Fixed,
            PriceLine::From { .. } => PriceLineKind::From,
            PriceLine::Custom => PriceLineKind::Custom,
        }
    }

    /// The stated setup, or the floor for a `from` line, or `None` for `custom`.
    ///
    /// Falls back to the engine's `?? 0` when the line carries no setup,
    /// which is what a `{monthly: 199}` line needs.
    pub fn setup(&self) -> Option<f64> {
        match self {
            PriceLine::Fixed { setup, .. } | PriceLine::From { setup, .. } => *setup,
            PriceLine::Custom => None,
        }
    }

    /// The monthly amount, or `None` when the line carries none.
    pub fn monthly(&self) -> Option<f64> {
        match self {
            PriceLine::Fixed { monthly, .. } | PriceLine::From { monthly, .. } => *monthly,
            PriceLine::Custom => None,
        }
    }
}

/// The four keys `classifyLine` narrows on.
///
/// Implementors return `Some` exactly when the JSON held a number in that
/// position, which is what `typeof x === 'number'` tests.
pub trait TierFields {
    /// `typeof record.custom === true` — the only value that forces `custom`.
    fn custom(&self) -> bool;
    fn setup_from(&self) -> Option<f64>;
    fn setup(&self) -> Option<f64>;
    /// A monthly amount that is genuinely absent, as opposed to `0`.
    ///
    /// The zod unions make `monthly` optional, and the support tiers use
    /// `{monthly: 0}` rather than omitting it, so the distinction between
    /// "no monthly" and "monthly zero" is observable in the output: the
    /// first yields `custom`, the second yields `fixed`.
    fn monthly(&self) -> Option<f64>;
}

/// `classifyLine` — resolve a pricebook line into the engine's representation.
///
/// Order of checks is the load-bearing part and is copied verbatim from
/// `packages/config/src/pricing/types.ts:47`:
/// 1. non-object → `custom`
/// 2. `custom === true` → `custom` (wins over every other key)
/// 3. `setup_from` is a number → `from`
/// 4. `setup` is a number → `fixed`
/// 5. `monthly` is a number → `fixed` with `setup` forced to 0
/// 6. otherwise → `custom`
///
/// Step 2 beating step 3 is what makes `custom_web_app` — whose raw JSON is
/// `{setup_from: 8000, monthly: 0, custom: true}` — classify as `custom`
/// rather than `from`. Verified by executing the baseline.
pub fn classify_line<T: TierFields + ?Sized>(tier: &T) -> PriceLine {
    if tier.custom() {
        return PriceLine::Custom;
    }
    if let Some(setup_from) = tier.setup_from() {
        return PriceLine::From {
            setup: Some(setup_from),
            monthly: tier.monthly(),
        };
    }
    if let Some(setup) = tier.setup() {
        return PriceLine::Fixed {
            setup: Some(setup),
            monthly: tier.monthly(),
        };
    }
    if let Some(monthly) = tier.monthly() {
        return PriceLine::Fixed {
            setup: Some(0.0),
            monthly: Some(monthly),
        };
    }
    PriceLine::Custom
}

/// A line the pricebook does not contain at all.
///
/// `classifyLine(undefined)` hits the baseline's first branch — `undefined` is
/// not an object — so a missing key behaves identically to `{custom: true}`.
/// This is what a request naming an unknown add-on or support tier gets.
pub const MISSING_LINE: PriceLine = PriceLine::Custom;

impl TierFields for crate::pricebook::PresenceTier {
    fn custom(&self) -> bool {
        // Presence tiers have no `custom` key; `{custom: true}` never appears.
        false
    }
    fn setup_from(&self) -> Option<f64> {
        // `presenceTierSchema` has no `setup_from` member, so this shape can
        // never reach the `from` branch.
        None
    }
    fn setup(&self) -> Option<f64> {
        self.setup
    }
    fn monthly(&self) -> Option<f64> {
        self.monthly
    }
}

impl TierFields for crate::pricebook::CapabilityTier {
    fn custom(&self) -> bool {
        self.custom
    }
    fn setup_from(&self) -> Option<f64> {
        None
    }
    fn setup(&self) -> Option<f64> {
        self.setup
    }
    fn monthly(&self) -> Option<f64> {
        self.monthly
    }
}

impl TierFields for crate::pricebook::EnvironmentTier {
    fn custom(&self) -> bool {
        self.custom
    }
    fn setup_from(&self) -> Option<f64> {
        self.setup_from
    }
    fn setup(&self) -> Option<f64> {
        self.setup
    }
    fn monthly(&self) -> Option<f64> {
        self.monthly
    }
}

impl TierFields for crate::pricebook::SimpleTier {
    fn custom(&self) -> bool {
        self.custom
    }
    fn setup_from(&self) -> Option<f64> {
        self.setup_from
    }
    fn setup(&self) -> Option<f64> {
        self.setup
    }
    fn monthly(&self) -> Option<f64> {
        self.monthly
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricebook::{CapabilityTier, EnvironmentTier, PresenceTier, SimpleTier};

    fn env(
        setup: Option<f64>,
        monthly: Option<f64>,
        from: Option<f64>,
        custom: bool,
    ) -> EnvironmentTier {
        EnvironmentTier {
            setup,
            monthly,
            setup_from: from,
            custom,
            note: None,
        }
    }

    #[test]
    fn custom_true_beats_every_other_key() {
        // The exact shape of `custom_web_app` in the product pricebook.
        assert_eq!(
            classify_line(&env(Some(8_000.0), Some(0.0), Some(8_000.0), true)),
            PriceLine::Custom
        );
    }

    #[test]
    fn setup_from_wins_over_setup_and_monthly() {
        assert_eq!(
            classify_line(&env(Some(1.0), Some(2.0), Some(3.0), false)),
            PriceLine::From {
                setup: Some(3.0),
                monthly: Some(2.0)
            }
        );
    }

    #[test]
    fn setup_makes_a_fixed_line_and_keeps_its_monthly() {
        assert_eq!(
            classify_line(&env(Some(2_000.0), Some(249.0), None, false)),
            PriceLine::Fixed {
                setup: Some(2_000.0),
                monthly: Some(249.0)
            }
        );
    }

    #[test]
    fn monthly_only_is_fixed_with_a_materialized_zero_setup() {
        // `{monthly: 0}` — the shape of every GLOBAL support tier.
        assert_eq!(
            classify_line(&SimpleTier {
                setup: None,
                monthly: Some(0.0),
                setup_from: None,
                custom: false,
            }),
            PriceLine::Fixed {
                setup: Some(0.0),
                monthly: Some(0.0)
            }
        );
    }

    #[test]
    fn an_empty_line_is_custom() {
        assert_eq!(
            classify_line(&env(None, None, None, false)),
            PriceLine::Custom
        );
    }

    #[test]
    fn monthly_zero_and_monthly_absent_are_different_lines() {
        // This is the distinction that decides whether support is priced.
        let zero = classify_line(&SimpleTier {
            setup: None,
            monthly: Some(0.0),
            setup_from: None,
            custom: false,
        });
        let absent = classify_line(&SimpleTier {
            setup: None,
            monthly: None,
            setup_from: None,
            custom: false,
        });
        assert_eq!(
            zero,
            PriceLine::Fixed {
                setup: Some(0.0),
                monthly: Some(0.0)
            }
        );
        assert_eq!(absent, PriceLine::Custom);
    }

    #[test]
    fn presence_tiers_are_always_a_stated_price() {
        // `presenceTierSchema` requires all four keys, so a presence line can
        // never be `custom` — but its `setup` is still optional in the model.
        let tier = PresenceTier {
            setup: Some(2_000.0),
            monthly: Some(249.0),
            included_usage: None,
            overage: None,
        };
        assert_eq!(
            classify_line(&tier),
            PriceLine::Fixed {
                setup: Some(2_000.0),
                monthly: Some(249.0)
            }
        );
    }

    #[test]
    fn capability_enterprise_is_custom() {
        // `enterprise` is `{custom: true}` — the flag is what does the work.
        let tier = CapabilityTier {
            setup: None,
            monthly: None,
            included_standard_integrations: None,
            custom: true,
        };
        assert_eq!(classify_line(&tier), PriceLine::Custom);
    }

    #[test]
    fn capability_paid_is_fixed_even_with_no_monthly() {
        let tier = CapabilityTier {
            setup: Some(1_500.0),
            monthly: Some(149.0),
            included_standard_integrations: Some(1.0),
            custom: false,
        };
        assert_eq!(
            classify_line(&tier),
            PriceLine::Fixed {
                setup: Some(1_500.0),
                monthly: Some(149.0)
            }
        );
    }

    #[test]
    fn a_missing_line_behaves_like_an_explicit_custom_one() {
        // `classifyLine(undefined)` in the baseline.
        assert_eq!(MISSING_LINE, PriceLine::Custom);
    }
}
