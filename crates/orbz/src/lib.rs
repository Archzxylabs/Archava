//! Ava Orbz: The Sovereign Multi-Layer Agentic Memory Engine for Archava.
//! Combines OpenViking (URI + Progressive L0/L1/L2), Graphiti (Bi-temporal invalidation),
//! Letta (Context paging & reflection), and CoALA (4-quadrant cognitive taxonomy).

pub mod episodes;
pub mod progressive;
pub mod reflection;
pub mod security;
pub mod temporal;
pub mod uri;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub use episodes::Episode;
pub use progressive::{ContextTier, ProgressiveNode};
pub use reflection::{ReflectionSynthesizer, SynthesizedFact};
pub use security::{AuthorityTier, OrbzActor, OrbzScope};
pub use temporal::TemporalCoordinate;
pub use uri::{OrbzUri, UriParseError};

/// A memory node stored in the Orbz engine.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MemoryOrb {
    pub node: ProgressiveNode,
    pub temporal: TemporalCoordinate,
    pub created_at_millis: i64,
}

/// In-memory thread-safe store for Ava Orbz with strict RLS and bi-temporal query filtering.
#[derive(Debug, Default, Clone)]
pub struct OrbzStore {
    orbs: Arc<RwLock<HashMap<Uuid, MemoryOrb>>>,
    episodes: Arc<RwLock<Vec<Episode>>>,
}

impl OrbzStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores a new memory orb into the engine.
    pub fn crystallize(&self, node: ProgressiveNode, now_millis: i64) -> Uuid {
        let id = node.id;
        let orb = MemoryOrb {
            node,
            temporal: TemporalCoordinate::new_active(now_millis),
            created_at_millis: now_millis,
        };
        self.orbs.write().expect("lock").insert(id, orb);
        id
    }

    /// Appends a new episodic conversation turn.
    pub fn record_episode(&self, episode: Episode) {
        self.episodes.write().expect("lock").push(episode);
    }

    /// Queries active memory orbs filtered by actor permissions (Row-Level Security)
    /// and bi-temporal validity at `point_in_time_millis`.
    pub fn query_accessible(
        &self,
        actor: &OrbzActor,
        point_in_time_millis: i64,
    ) -> Vec<ProgressiveNode> {
        let guard = self.orbs.read().expect("lock");
        guard
            .values()
            .filter(|orb| {
                // Tenant isolation guard
                if orb.node.uri.tenant_id != actor.tenant_id {
                    return false;
                }
                // Temporal validity guard (Graphiti)
                if !orb.temporal.is_active_at(point_in_time_millis) {
                    return false;
                }
                // Security access guard (RLS)
                actor.can_access(&orb.node.uri.scope, orb.node.min_authority)
            })
            .map(|orb| orb.node.clone())
            .collect()
    }

    /// Supersedes an existing memory orb with a replacement node.
    pub fn supersede_node(
        &self,
        old_node_id: Uuid,
        new_node: ProgressiveNode,
        now_millis: i64,
    ) -> Result<Uuid, &'static str> {
        let mut guard = self.orbs.write().expect("lock");
        let old_orb = guard.get_mut(&old_node_id).ok_or("Node not found")?;

        let new_id = new_node.id;
        old_orb.temporal.supersede(new_id, now_millis);

        let new_orb = MemoryOrb {
            node: new_node,
            temporal: TemporalCoordinate::new_active(now_millis),
            created_at_millis: now_millis,
        };
        guard.insert(new_id, new_orb);
        Ok(new_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_rls_and_temporal_filtering() {
        let store = OrbzStore::new();
        let t0 = 1_700_000_000_000;

        // 1. Crystallize public SOP
        let sop_uri = OrbzUri::parse("orbz://ten_01/public/sop/refund.md").unwrap();
        let sop_node = ProgressiveNode::new(
            Uuid::new_v4(),
            sop_uri,
            AuthorityTier::L1,
            "Refund policy 7 days",
            "Full refund within 7 days",
            None,
        );
        store.crystallize(sop_node, t0);

        // 2. Crystallize sovereign financial target
        let target_uri = OrbzUri::parse("orbz://ten_01/sovereign/owner/q4_target.md").unwrap();
        let target_node = ProgressiveNode::new(
            Uuid::new_v4(),
            target_uri,
            AuthorityTier::L5,
            "Q4 profit target Rp 500jt",
            "Strategic profit and dividend targets",
            None,
        );
        store.crystallize(target_node, t0);

        let cashier = OrbzActor {
            tenant_id: "ten_01".into(),
            user_id: "usr_cashier".into(),
            authority: AuthorityTier::L2,
            department: Some("pos".into()),
            is_owner: false,
        };

        let owner = OrbzActor {
            tenant_id: "ten_01".into(),
            user_id: "usr_owner".into(),
            authority: AuthorityTier::L5,
            department: None,
            is_owner: true,
        };

        // Cashier only sees public SOP
        let cashier_memories = store.query_accessible(&cashier, t0);
        assert_eq!(cashier_memories.len(), 1);
        assert_eq!(cashier_memories[0].summary_l0, "Refund policy 7 days");

        // Owner sees both SOP and sovereign financial target
        let owner_memories = store.query_accessible(&owner, t0);
        assert_eq!(owner_memories.len(), 2);
    }
}
