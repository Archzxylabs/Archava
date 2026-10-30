//! Flight status state machine for Ava Cockpit.
//! Maps autonomous agent execution to aviation flight phases:
//! Pre-Flight -> In-Flight -> Holding Pattern -> Touchdown / Aborted.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlightStatus {
    /// In planning / DAG formulation. Breaking down the user's objective into work items.
    PreFlight,
    /// Actively executing steps across background workers.
    InFlight,
    /// Paused at L4 Gate. Awaiting human confirmation via WhatsApp or Cockpit UI. Zero compute cost.
    HoldingPattern,
    /// All steps completed, double-entry financial invariants verified, state committed to database.
    Touchdown,
    /// Terminated, cancelled, or transactionally rolled back due to failure or rejection.
    Aborted,
}

impl FlightStatus {
    /// Enforces strict, legal state machine transitions.
    pub fn can_transition_to(&self, next: FlightStatus) -> bool {
        match (self, next) {
            // Pre-Flight can transition to In-Flight (dispatched) or Aborted (invalid/declined)
            (FlightStatus::PreFlight, FlightStatus::InFlight) => true,
            (FlightStatus::PreFlight, FlightStatus::Aborted) => true,

            // In-Flight can pause into Holding Pattern, land successfully (Touchdown), or abort
            (FlightStatus::InFlight, FlightStatus::HoldingPattern) => true,
            (FlightStatus::InFlight, FlightStatus::Touchdown) => true,
            (FlightStatus::InFlight, FlightStatus::Aborted) => true,

            // Holding Pattern can resume back to In-Flight (approved) or Abort (rejected)
            (FlightStatus::HoldingPattern, FlightStatus::InFlight) => true,
            (FlightStatus::HoldingPattern, FlightStatus::Aborted) => true,

            // Terminal states cannot transition further
            (FlightStatus::Touchdown, _) => false,
            (FlightStatus::Aborted, _) => false,

            _ => false,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, FlightStatus::Touchdown | FlightStatus::Aborted)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PreFlight => "pre_flight",
            Self::InFlight => "in_flight",
            Self::HoldingPattern => "holding_pattern",
            Self::Touchdown => "touchdown",
            Self::Aborted => "aborted",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_legal_flight_transitions() {
        assert!(FlightStatus::PreFlight.can_transition_to(FlightStatus::InFlight));
        assert!(FlightStatus::InFlight.can_transition_to(FlightStatus::HoldingPattern));
        assert!(FlightStatus::HoldingPattern.can_transition_to(FlightStatus::InFlight));
        assert!(FlightStatus::InFlight.can_transition_to(FlightStatus::Touchdown));

        // Terminal checks
        assert!(!FlightStatus::Touchdown.can_transition_to(FlightStatus::InFlight));
        assert!(!FlightStatus::Aborted.can_transition_to(FlightStatus::PreFlight));
    }
}
