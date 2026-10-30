//! Baseline role model and capability tiers.
//!
//! Mirrors `packages/acl/src/roles.ts` and the tier helpers in
//! `packages/acl/src/policy.ts`.
//!
//! PRD §15, quoted from the TypeScript because it is the reason the shape is
//! what it is: the client-facing Client Admin is separate from Archava Studio
//! and must be role-scoped. Client users must never reach provider secrets,
//! global pricing configuration, cross-tenant data, internal margin/COGS data,
//! unsupported action permissions, or raw system prompts.
//!
//! A per-client override can narrow these further but can never widen a lower
//! role past `owner`.

use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Admin resources a role may act on. Mirrors `ADMIN_RESOURCES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdminResource {
    Content,
    Products,
    Media,
    Knowledge,
    Leads,
    Bookings,
    Conversations,
    HandoffQueue,
    Analytics,
    Members,
    Settings,
}

impl AdminResource {
    pub fn as_str(self) -> &'static str {
        match self {
            AdminResource::Content => "content",
            AdminResource::Products => "products",
            AdminResource::Media => "media",
            AdminResource::Knowledge => "knowledge",
            AdminResource::Leads => "leads",
            AdminResource::Bookings => "bookings",
            AdminResource::Conversations => "conversations",
            AdminResource::HandoffQueue => "handoff_queue",
            AdminResource::Analytics => "analytics",
            AdminResource::Members => "members",
            AdminResource::Settings => "settings",
        }
    }
}

/// What a role may do with a resource. Mirrors `AdminPermission`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdminPermission {
    Read,
    Write,
    Configure,
}

/// Role names. Mirrors `ROLE_NAMES`.
///
/// Deserialization goes through [`RoleName::try_from`], so a role that is not in
/// the baseline cannot be represented as a `RoleName` at all — the same decision
/// [`TenantId`](crate::tenant::TenantId) makes for tenants. An unregistered role
/// therefore fails at the HTTP boundary, before any handler runs, rather than
/// being carried into a policy evaluation that has to decide what an unknown
/// actor means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub enum RoleName {
    ArchavaAssistant,
    Owner,
    Admin,
    Editor,
    Viewer,
}

impl RoleName {
    pub fn as_str(self) -> &'static str {
        match self {
            RoleName::ArchavaAssistant => "archava_assistant",
            RoleName::Owner => "owner",
            RoleName::Admin => "admin",
            RoleName::Editor => "editor",
            RoleName::Viewer => "viewer",
        }
    }

    fn index(self) -> usize {
        match self {
            RoleName::ArchavaAssistant => 0,
            RoleName::Owner => 1,
            RoleName::Admin => 2,
            RoleName::Editor => 3,
            RoleName::Viewer => 4,
        }
    }
}

impl fmt::Display for RoleName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
/// Raised when a role or capability tier is not in the baseline vocabulary.
///
/// These are actor and grant boundaries rather than tenant ones, but they are
/// refusals for the same reason: an identity the system has no definition for
/// cannot be authorized, so it must not be able to reach the policy gate.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RoleParseError {
    #[error("role must be one of the baseline roles, got {0:?}")]
    UnknownRole(String),
    #[error("capability tier must be one of assist, act, transact, enterprise, got {0:?}")]
    UnknownTier(String),
}
impl TryFrom<String> for RoleName {
    type Error = RoleParseError;
    /// The only way a `RoleName` comes into existence from a string.
    ///
    /// Mirrors `isRoleName`: the vocabulary is closed, so a value outside it is
    /// refused here rather than defaulted to a weaker or stronger role.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        role_name_of(&value).ok_or(RoleParseError::UnknownRole(value))
    }
}

/// A role's effective permissions.
///
/// Same shape as `RoleDefinition`. Read/write/configure are held as slices
/// rather than a set so the definition is a compile-time constant, exactly as
/// the baseline's arrays are.
#[derive(Debug, Clone, Copy)]
pub struct RoleDefinition {
    pub name: RoleName,
    pub read: &'static [AdminResource],
    pub write: &'static [AdminResource],
    pub configure: &'static [AdminResource],
}

impl RoleDefinition {
    /// Whether this role may perform `permission` on `resource`.
    pub fn may(&self, permission: AdminPermission, resource: AdminResource) -> bool {
        let table = match permission {
            AdminPermission::Read => self.read,
            AdminPermission::Write => self.write,
            AdminPermission::Configure => self.configure,
        };
        table.contains(&resource)
    }
}

/// Capability tiers, ordered weakest to strongest. Mirrors `CAPABILITY_TIERS`.
///
/// The declaration order *is* the ranking, which is the property
/// [`satisfies_capability`] depends on. Reordering these variants would silently
/// change authorization; `capability_ranking_matches_the_baseline` guards
/// against exactly that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub enum CapabilityTier {
    Assist,
    Act,
    Transact,
    Enterprise,
}

impl CapabilityTier {
    /// The numeric weight of a tier, matching `CAPABILITY_RANK`.
    pub fn rank(self) -> u8 {
        match self {
            CapabilityTier::Assist => 0,
            CapabilityTier::Act => 1,
            CapabilityTier::Transact => 2,
            CapabilityTier::Enterprise => 3,
        }
    }
}

impl fmt::Display for CapabilityTier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            CapabilityTier::Assist => "assist",
            CapabilityTier::Act => "act",
            CapabilityTier::Transact => "transact",
            CapabilityTier::Enterprise => "enterprise",
        };
        formatter.write_str(name)
    }
}
impl TryFrom<String> for CapabilityTier {
    type Error = RoleParseError;
    /// The only way a `CapabilityTier` comes into existence from a string.
    ///
    /// A tier outside the vocabulary is refused rather than clamped. Clamping
    /// upward would grant; clamping downward would be a silent privilege change;
    /// the third option, refusing, is the only one that cannot be mistaken for
    /// either.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "assist" => Ok(CapabilityTier::Assist),
            "act" => Ok(CapabilityTier::Act),
            "transact" => Ok(CapabilityTier::Transact),
            "enterprise" => Ok(CapabilityTier::Enterprise),
            _ => Err(RoleParseError::UnknownTier(value)),
        }
    }
}

/// Mirrors `satisfiesCapability`.
///
/// Returns `true` when `granted` is at least the `required` tier.
pub fn satisfies_capability(granted: CapabilityTier, required: CapabilityTier) -> bool {
    granted.rank() >= required.rank()
}

/// The five baseline roles, in the order the baseline lists them.
pub const BASELINE_ROLES: &[RoleDefinition] = &[ASSISTANT, OWNER, ADMIN, EDITOR, VIEWER];

/// Look up a role by name. Mirrors `getRole`.
pub fn get_role(name: RoleName) -> &'static RoleDefinition {
    &BASELINE_ROLES[name.index()]
}

/// Type guard for role names. Mirrors `isRoleName`.
pub fn is_role_name(value: &str) -> bool {
    role_name_of(value).is_some()
}

/// Parse a role name from its string form.
pub fn role_name_of(value: &str) -> Option<RoleName> {
    BASELINE_ROLES
        .iter()
        .find(|role| role.name.as_str() == value)
        .map(|role| role.name)
}

// --- Role permission tables, mirroring BASELINE_ROLES in roles.ts ---

const ASSISTANT: RoleDefinition = RoleDefinition {
    name: RoleName::ArchavaAssistant,
    read: &[AdminResource::Content],
    write: &[],
    configure: &[],
};

const OWNER: RoleDefinition = RoleDefinition {
    name: RoleName::Owner,
    read: &[
        AdminResource::Content,
        AdminResource::Products,
        AdminResource::Media,
        AdminResource::Knowledge,
        AdminResource::Leads,
        AdminResource::Bookings,
        AdminResource::Conversations,
        AdminResource::HandoffQueue,
        AdminResource::Analytics,
        AdminResource::Members,
        AdminResource::Settings,
    ],
    write: &[
        AdminResource::Content,
        AdminResource::Products,
        AdminResource::Media,
        AdminResource::Knowledge,
        AdminResource::Leads,
        AdminResource::Bookings,
        AdminResource::Conversations,
        AdminResource::HandoffQueue,
        AdminResource::Analytics,
        AdminResource::Members,
        AdminResource::Settings,
    ],
    configure: &[AdminResource::Members, AdminResource::Settings],
};

const ADMIN: RoleDefinition = RoleDefinition {
    name: RoleName::Admin,
    read: &[
        AdminResource::Content,
        AdminResource::Products,
        AdminResource::Media,
        AdminResource::Knowledge,
        AdminResource::Leads,
        AdminResource::Bookings,
        AdminResource::Conversations,
        AdminResource::HandoffQueue,
        AdminResource::Analytics,
        AdminResource::Members,
    ],
    write: &[
        AdminResource::Content,
        AdminResource::Products,
        AdminResource::Media,
        AdminResource::Knowledge,
        AdminResource::Leads,
        AdminResource::Bookings,
        AdminResource::Conversations,
        AdminResource::HandoffQueue,
        AdminResource::Analytics,
    ],
    configure: &[],
};

const EDITOR: RoleDefinition = RoleDefinition {
    name: RoleName::Editor,
    read: &[
        AdminResource::Content,
        AdminResource::Products,
        AdminResource::Media,
        AdminResource::Knowledge,
        AdminResource::Leads,
        AdminResource::Bookings,
        AdminResource::Conversations,
        AdminResource::HandoffQueue,
        AdminResource::Analytics,
    ],
    write: &[
        AdminResource::Content,
        AdminResource::Products,
        AdminResource::Media,
        AdminResource::Knowledge,
    ],
    configure: &[],
};

const VIEWER: RoleDefinition = RoleDefinition {
    name: RoleName::Viewer,
    read: &[
        AdminResource::Content,
        AdminResource::Products,
        AdminResource::Media,
        AdminResource::Knowledge,
        AdminResource::Leads,
        AdminResource::Bookings,
        AdminResource::Conversations,
        AdminResource::HandoffQueue,
        AdminResource::Analytics,
    ],
    write: &[],
    configure: &[],
};

#[cfg(test)]
mod tests {
    use super::*;

    /// The tier ranking is load-bearing for authorization. If the variant order
    /// ever changes, `satisfies_capability` would silently invert, so the ranks
    /// are pinned to the baseline's `CAPABILITY_RANK` explicitly.
    #[test]
    fn capability_ranking_matches_the_baseline() {
        assert_eq!(CapabilityTier::Assist.rank(), 0);
        assert_eq!(CapabilityTier::Act.rank(), 1);
        assert_eq!(CapabilityTier::Transact.rank(), 2);
        assert_eq!(CapabilityTier::Enterprise.rank(), 3);
    }

    #[test]
    fn a_granted_tier_satisfies_itself_and_anything_weaker() {
        assert!(satisfies_capability(
            CapabilityTier::Act,
            CapabilityTier::Assist
        ));
        assert!(satisfies_capability(
            CapabilityTier::Act,
            CapabilityTier::Act
        ));
        assert!(!satisfies_capability(
            CapabilityTier::Assist,
            CapabilityTier::Act
        ));
        assert!(!satisfies_capability(
            CapabilityTier::Transact,
            CapabilityTier::Enterprise
        ));
    }

    #[test]
    fn all_five_baseline_roles_are_lookupable() {
        assert_eq!(BASELINE_ROLES.len(), 5);
        for role in BASELINE_ROLES {
            assert_eq!(get_role(role.name).name, role.name);
        }
    }

    #[test]
    fn owner_is_the_widest_role() {
        // PRD §15: no override may widen a lower role past owner, so the table
        // itself must be ordered with owner on top.
        assert!(get_role(RoleName::Owner).may(AdminPermission::Write, AdminResource::Settings));
        assert!(!get_role(RoleName::Admin).may(AdminPermission::Write, AdminResource::Settings));
    }

    #[test]
    fn editor_cannot_reach_members_or_settings() {
        let editor = get_role(RoleName::Editor);
        assert!(!editor.may(AdminPermission::Read, AdminResource::Members));
        assert!(!editor.may(AdminPermission::Read, AdminResource::Settings));
        assert!(!editor.may(AdminPermission::Configure, AdminResource::Members));
    }

    #[test]
    fn viewer_is_read_only() {
        let viewer = get_role(RoleName::Viewer);
        assert!(viewer.may(AdminPermission::Read, AdminResource::Analytics));
        assert!(viewer.write.is_empty());
        assert!(viewer.configure.is_empty());
    }

    #[test]
    fn assistant_only_reads_content() {
        let assistant = get_role(RoleName::ArchavaAssistant);
        assert!(assistant.may(AdminPermission::Read, AdminResource::Content));
        assert!(assistant.write.is_empty());
        assert!(assistant.configure.is_empty());
    }

    #[test]
    fn role_name_round_trips_through_its_string_form() {
        for role in BASELINE_ROLES {
            assert_eq!(role_name_of(role.name.as_str()), Some(role.name));
        }
    }

    #[test]
    fn an_unknown_role_name_is_not_a_role() {
        assert!(!is_role_name("superadmin"));
        assert!(!is_role_name(""));
        assert_eq!(
            role_name_of("Owner"),
            None,
            "case-sensitive, like the baseline"
        );
    }
}
