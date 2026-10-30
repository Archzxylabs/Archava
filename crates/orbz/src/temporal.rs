//! Bi-Temporal Knowledge Invalidation engine for Ava Orbz.
//! Implements Graphiti/Zep's breakthrough:
//! In business, facts change over time (prices rise, addresses move, tax laws update).
//! Bi-temporal timestamps guarantee Ava never hallucinates outdated facts while
//! maintaining an auditable historical continuum.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Temporal coordinates tracking both transaction time and real-world validity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporalCoordinate {
    /// When this fact was recorded into the database (Unix millis).
    pub tx_timestamp: i64,
    /// When this fact became true in the real world (Unix millis).
    pub valid_from: i64,
    /// When this fact ceased to be true (None = currently active).
    pub valid_to: Option<i64>,
    /// Pointer to the newer memory node that replaced this fact.
    pub superseded_by: Option<Uuid>,
}

impl TemporalCoordinate {
    pub fn new_active(now_millis: i64) -> Self {
        Self {
            tx_timestamp: now_millis,
            valid_from: now_millis,
            valid_to: None,
            superseded_by: None,
        }
    }

    /// Evaluates if this fact is currently active at a given moment in time.
    pub fn is_active_at(&self, point_in_time_millis: i64) -> bool {
        if point_in_time_millis < self.valid_from {
            return false;
        }
        match self.valid_to {
            Some(end) => point_in_time_millis < end,
            None => true,
        }
    }

    /// Supersedes this fact with a newer version.
    pub fn supersede(&mut self, replacement_id: Uuid, effective_at_millis: i64) {
        self.valid_to = Some(effective_at_millis);
        self.superseded_by = Some(replacement_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bi_temporal_invalidation_lifecycle() {
        let t0 = 1_700_000_000_000; // Jan 1
        let mut old_fact = TemporalCoordinate::new_active(t0);

        assert!(old_fact.is_active_at(t0));
        assert!(old_fact.is_active_at(t0 + 10_000));

        let t1 = t0 + 50_000; // Feb 1
        let new_fact_id = Uuid::new_v4();
        old_fact.supersede(new_fact_id, t1);

        // Before Feb 1, the old fact was true
        assert!(old_fact.is_active_at(t0 + 20_000));
        // After Feb 1, the old fact is invalid / superseded
        assert!(!old_fact.is_active_at(t1));
        assert!(!old_fact.is_active_at(t1 + 10_000));
        assert_eq!(old_fact.superseded_by, Some(new_fact_id));
    }
}
