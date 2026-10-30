//! Security boundaries and Row-Level Security (RLS) enforcement for Ava Orbz.
//! Guarantees that sensitive business memories (finances, margins, owner notes)
//! can never leak to frontline staff or external visitors.

use serde::{Deserialize, Serialize};

/// The sensitivity scope of an Orbz memory node.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrbzScope {
    /// Universally accessible within the tenant (SOPs, product catalog, opening hours).
    Public,
    /// Scoped to a specific department or operational domain (e.g. "warehouse", "finance", "kitchen").
    Department(String),
    /// Scoped to an individual entity (e.g. customer preferences, specific supplier contracts).
    Entity {
        entity_type: String,
        entity_id: String,
    },
    /// Sovereign executive memory (owner-only: profit margins, tax strategy, cash reserves).
    Sovereign,
}

impl OrbzScope {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Public => "public",
            Self::Department(_) => "department",
            Self::Entity { .. } => "entity",
            Self::Sovereign => "sovereign",
        }
    }
}

/// The required authority level to read or mutate this memory node (L1 to L5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AuthorityTier {
    /// L1: Read-only public knowledge (e.g. FAQ, product list).
    L1,
    /// L2: Frontline staff access (e.g. POS cashier guidelines, shift checklists).
    L2,
    /// L3: Managerial access (e.g. reorder thresholds, vendor reliability records).
    L3,
    /// L4: High-impact executive access (requires explicit confirmation).
    L4,
    /// L5: Sovereign owner-only access (uncompromised financial & strategic truth).
    L5,
}

impl AuthorityTier {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "L1" => Some(Self::L1),
            "L2" => Some(Self::L2),
            "L3" => Some(Self::L3),
            "L4" => Some(Self::L4),
            "L5" => Some(Self::L5),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::L1 => "L1",
            Self::L2 => "L2",
            Self::L3 => "L3",
            Self::L4 => "L4",
            Self::L5 => "L5",
        }
    }
}

/// The verified actor identity querying the Orbz memory store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrbzActor {
    pub tenant_id: String,
    pub user_id: String,
    pub authority: AuthorityTier,
    pub department: Option<String>,
    pub is_owner: bool,
}

impl OrbzActor {
    /// Evaluates whether this actor is mathematically permitted to view the memory node.
    /// A failure here completely prevents the memory from reaching the LLM's prompt context.
    pub fn can_access(&self, scope: &OrbzScope, min_authority: AuthorityTier) -> bool {
        // Rule 1: Authority level must be equal or greater
        if self.authority < min_authority {
            return false;
        }

        // Rule 2: Sovereign scope is accessible ONLY to the verified owner
        if matches!(scope, OrbzScope::Sovereign) && !self.is_owner {
            return false;
        }

        // Rule 3: Department scope matches actor's department or owner bypass
        match scope {
            OrbzScope::Public => true,
            OrbzScope::Sovereign => self.is_owner,
            OrbzScope::Department(dept) => {
                self.is_owner || self.department.as_deref() == Some(dept.as_str())
            }
            OrbzScope::Entity { .. } => {
                // Entity profile requires at least L2 staff or owner
                self.authority >= AuthorityTier::L2
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cashier_cannot_access_sovereign_memory() {
        let cashier = OrbzActor {
            tenant_id: "ten_01".into(),
            user_id: "usr_cashier".into(),
            authority: AuthorityTier::L2,
            department: Some("pos".into()),
            is_owner: false,
        };

        let sovereign_scope = OrbzScope::Sovereign;
        assert!(!cashier.can_access(&sovereign_scope, AuthorityTier::L5));
        assert!(!cashier.can_access(&sovereign_scope, AuthorityTier::L2)); // Even if tier artificially lowered
    }

    #[test]
    fn test_department_isolation() {
        let kitchen_staff = OrbzActor {
            tenant_id: "ten_01".into(),
            user_id: "usr_chef".into(),
            authority: AuthorityTier::L2,
            department: Some("kitchen".into()),
            is_owner: false,
        };

        let kitchen_scope = OrbzScope::Department("kitchen".into());
        let warehouse_scope = OrbzScope::Department("warehouse".into());

        assert!(kitchen_staff.can_access(&kitchen_scope, AuthorityTier::L2));
        assert!(!kitchen_staff.can_access(&warehouse_scope, AuthorityTier::L2));
    }

    #[test]
    fn test_owner_super_access() {
        let owner = OrbzActor {
            tenant_id: "ten_01".into(),
            user_id: "usr_owner".into(),
            authority: AuthorityTier::L5,
            department: None,
            is_owner: true,
        };

        assert!(owner.can_access(&OrbzScope::Public, AuthorityTier::L1));
        assert!(owner.can_access(&OrbzScope::Department("finance".into()), AuthorityTier::L3));
        assert!(owner.can_access(&OrbzScope::Sovereign, AuthorityTier::L5));
    }
}
