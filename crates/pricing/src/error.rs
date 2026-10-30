//! `PricingError` — the engine's only failure mode.
//!
//! The baseline's version is a plain `Error` subclass that sets `name` and
//! carries no structured code, so every failure the engine reports is
//! distinguishable only by its message string. The port keeps that shape
//! exactly, because the pilot's parity check compares thrown errors between
//! the two implementations, and a Rust `enum` of failure variants cannot be
//! compared against anything the TypeScript produces.
//!
//! Consequence, worth stating plainly: a Rust caller can only distinguish
//! failures by matching the message, which is not a good API. It is good
//! *evidence*, which is what this crate is for. If the port were promoted to
//! production the fix would be adding a code field in `engine.ts` first and
//! only then splitting the Rust variant — a product change, outside this
//! pilot's mandate.

use std::fmt;

/// The four messages the baseline can throw, in the order the engine checks.
///
/// Kept as a small enum rather than four free functions so a call site cannot
/// mistype a message, while [`PricingErrorKind::message`] stays the single
/// place the exact strings live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PricingErrorKind {
    /// `requireRegion` — the very first check in `quoteRequest`.
    UnknownRegion,
    /// Step A, presence.
    UnknownPresence,
    /// Step A, capability.
    UnknownCapability,
    /// Step A, environment.
    UnknownEnvironment,
}

impl PricingErrorKind {
    /// The message, with the offending value interpolated.
    ///
    /// The baseline builds these with template literals and no escaping, so
    /// an empty name yields `""` between the quotes rather than a fallback.
    /// The port does the same.
    pub fn message(self, detail: &str) -> String {
        match self {
            PricingErrorKind::UnknownRegion => {
                format!("Unknown region \"{detail}\". No quote produced.")
            }
            PricingErrorKind::UnknownPresence => format!("Unknown presence \"{detail}\"."),
            PricingErrorKind::UnknownCapability => format!("Unknown capability \"{detail}\"."),
            PricingErrorKind::UnknownEnvironment => {
                format!("Unknown environment \"{detail}\".")
            }
        }
    }
}

/// The error thrown by every failure path in the engine.
///
/// Mirrors `engine.ts:61-66`: `name` is `'PricingError'`, the message is
/// positional and human-readable, and there is no code field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricingError {
    kind: PricingErrorKind,
    /// The region, presence, capability, or environment name that failed.
    detail: String,
}

impl PricingError {
    /// Build the error for one of the four known failure kinds.
    pub fn new(kind: PricingErrorKind, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        PricingError { kind, detail }
    }

    /// Which of the four failures this is.
    pub fn kind(&self) -> PricingErrorKind {
        self.kind
    }
}

impl fmt::Display for PricingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.kind.message(&self.detail))
    }
}

impl std::error::Error for PricingError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_renders_the_baseline_message_verbatim() {
        // Copied from engine.ts, character for character.
        assert_eq!(
            PricingError::new(PricingErrorKind::UnknownRegion, "EMEA").to_string(),
            "Unknown region \"EMEA\". No quote produced."
        );
        assert_eq!(
            PricingError::new(PricingErrorKind::UnknownPresence, "telepathy").to_string(),
            "Unknown presence \"telepathy\"."
        );
        assert_eq!(
            PricingError::new(PricingErrorKind::UnknownCapability, "teleport").to_string(),
            "Unknown capability \"teleport\"."
        );
        assert_eq!(
            PricingError::new(PricingErrorKind::UnknownEnvironment, "moon").to_string(),
            "Unknown environment \"moon\"."
        );
    }

    #[test]
    fn an_empty_detail_still_produces_a_readable_message() {
        assert_eq!(
            PricingError::new(PricingErrorKind::UnknownPresence, "").to_string(),
            "Unknown presence \"\"."
        );
    }

    #[test]
    fn the_kind_survives_round_tripping_for_callers() {
        let error = PricingError::new(PricingErrorKind::UnknownEnvironment, "orbital");
        assert_eq!(error.kind(), PricingErrorKind::UnknownEnvironment);
    }
}
