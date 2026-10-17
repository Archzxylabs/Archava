//! The kit consumer's own witnesses.
//!
//! This file is identical in both business-model consumers — one template is
//! materialized twice, and the only generated difference between the two
//! consumers is `kit.config.json`, which carries the kit's identity, its
//! workspace slug and the action configuration its modules run under. A fork
//! between the consumers is a change to test CODE (the producer's tree-hash
//! comparison catches that); every behavioral difference is a change to the
//! configuration the same code runs against.
//!
//! What the witnesses claim, and what they do not claim:
//!
//! - They claim that, outside the product workspace and with no product code
//!   path reachable, the shipped authority surface — tenant slug discipline
//!   ([`archava_core::tenant`]), the role/permission tables
//!   ([`archava_core::roles`]) and the command policy gate
//!   ([`archava_domain::CommandAuthorizer`]) — decides exactly what the
//!   configuration records, offline.
//! - They do NOT claim anything about PostgreSQL (RLS lives above this seam in
//!   the host and its adapters), HTTP (the host owns that), or the kits' own
//!   TypeScript modules (verified by the module gates on that lane). "Core"
//!   here is `archava-core`; "CRM" is `archava-domain`, the CRM command
//!   policy's chokepoint — the two crates every business model consumes before
//!   any module-specific code runs.

use archava_core::actions::ActionRegistry;
use archava_core::roles::{
    get_role, role_name_of, AdminPermission, AdminResource, CapabilityTier, RoleName,
};
use archava_core::tenant::{assert_tenant, scoped, TenantId, TenantScope, TenantScopeError};
use archava_domain::authority::{ActionRequest, PolicyDecision};
use archava_domain::CommandAuthorizer;
use serde_json::Value;

const KIT: &str = include_str!("../kit.config.json");

fn config() -> Value {
    serde_json::from_str(KIT).expect("kit.config.json must parse")
}

fn workspace_tenant(config: &Value) -> TenantId {
    let slug = config["workspace"]["tenant"].as_str().expect("workspace.tenant");
    TenantId::new(slug).expect("the kit's workspace tenant is a valid slug")
}

fn role_of(value: &Value, key: &str) -> RoleName {
    role_name_of(value[key].as_str().expect(key))
        .unwrap_or_else(|| panic!("role {key:?} is not in the shipped baseline"))
}

fn capability_of(value: &Value, key: &str) -> CapabilityTier {
    match value[key].as_str().expect(key) {
        "assist" => CapabilityTier::Assist,
        "act" => CapabilityTier::Act,
        "transact" => CapabilityTier::Transact,
        "enterprise" => CapabilityTier::Enterprise,
        other => panic!("capability tier {other:?} is not in the shipped baseline"),
    }
}

fn permission_of(value: &Value) -> AdminPermission {
    match value["permission"].as_str().expect("permission") {
        "read" => AdminPermission::Read,
        "write" => AdminPermission::Write,
        "configure" => AdminPermission::Configure,
        other => panic!("permission {other:?} is not in the shipped vocabulary"),
    }
}

fn resource_of(value: &Value) -> AdminResource {
    match value["resource"].as_str().expect("resource") {
        "content" => AdminResource::Content,
        "products" => AdminResource::Products,
        "media" => AdminResource::Media,
        "knowledge" => AdminResource::Knowledge,
        "leads" => AdminResource::Leads,
        "bookings" => AdminResource::Bookings,
        "conversations" => AdminResource::Conversations,
        "handoff_queue" => AdminResource::HandoffQueue,
        "analytics" => AdminResource::Analytics,
        "members" => AdminResource::Members,
        "settings" => AdminResource::Settings,
        other => panic!("resource {other:?} is not in the shipped vocabulary"),
    }
}

/// A record owned by one tenant, for the isolation witnesses below.
struct Packet(TenantId);

impl archava_core::tenant::TenantScoped for Packet {
    fn tenant_id(&self) -> &TenantId {
        &self.0
    }
}

/// The kit's identity block names the business model this consumer runs as,
/// and its workspace slug is the tenant every decision below operates in.
///
/// The kit catalogue is the source of truth for these values; the producer
/// checks each field against `packages/kits/src/catalogue.ts` before this test
/// runs, so drift between the two is caught there rather than asserted here.
#[test]
fn the_kit_identity_and_workspace_bounds_are_the_configured_ones() {
    let config = config();
    assert_eq!(config["kit"]["id"].as_str().expect("kit.id").len() == 3, true, "the kit id is a three-character K-number");
    for field in ["slug", "label", "domainRequirement", "recordOwner", "apiContract"] {
        let value = config["kit"][field].as_str().expect(field);
        assert!(!value.is_empty(), "kit.{field} is recorded, not empty");
    }

    let workspace = workspace_tenant(&config);
    for bad in config["invalidTenants"].as_array().expect("invalidTenants") {
        let attempt = TenantId::new(bad.as_str().expect("an invalid tenant string"));
        assert!(
            matches!(attempt, Err(TenantScopeError::InvalidTenantId(_))),
            "the slug rule refuses {bad:?} — an invalid tenant is not representable at all"
        );
    }

    // The workspace markdown carries the kit's decision identity: every action
    // request in this file is issued for THIS tenant.
    assert_eq!(workspace.as_str(), config["workspace"]["tenant"].as_str().expect("workspace.tenant"));
}

/// The platform's role/permission table decides the workspace grants. The
/// expectations are identical for both business models on purpose: the five
/// roles are the platform's, not a kit's, and the SAME table ships to every
/// business model. A kit-level expectation that contradicted the shipped table
/// is refused here rather than scoped around.
#[test]
fn the_workspace_grants_match_the_shipped_role_table() {
    let config = config();
    for entry in config["roleExpectations"].as_array().expect("roleExpectations") {
        let role = role_of(entry, "role");
        let granted = get_role(role).may(permission_of(entry), resource_of(entry));
        assert_eq!(
            granted,
            entry["granted"].as_bool().expect("granted"),
            "{} may({:?}, {:?}) agrees with the configuration",
            role,
            entry["permission"],
            entry["resource"]
        );
    }
}

/// The command policy gate decides the kit's action configuration. Every row
/// runs one [`ActionRequest`] and pins the decision shape — allowed,
/// confirmation-needed (with its verbatim prompt), or denied (with the denial
/// the API contract names).
///
/// The business-model boundary is the `enabledActions` configuration: the
/// registry CARRIES both models' actions (the anti-fork witness below), and
/// this workspace's configuration is what scopes them for this business model.
#[test]
fn the_kit_action_configuration_decides_as_recorded() {
    let config = config();
    let authorizer = CommandAuthorizer::new();
    assert_eq!(
        authorizer.version(),
        config["registryVersion"].as_str().expect("registryVersion"),
        "the consumer pins the registry version it was verified against"
    );

    let tenant = workspace_tenant(&config);
    let enabled: Vec<String> = config["enabledActions"]
        .as_array()
        .expect("enabledActions")
        .iter()
        .map(|id| id.as_str().expect("an action id").to_string())
        .collect();
    assert!(
        enabled.iter().any(|id| id == config["disabledProbe"].as_str().expect("disabledProbe")) == false,
        "the action the configuration scopes off is not in this kit's enabled set"
    );

    for row in config["actions"].as_array().expect("actions") {
        let request = ActionRequest {
            tenant: tenant.clone(),
            role: role_of(row, "role"),
            capability: capability_of(row, "capability"),
            action_id: row["actionId"].as_str().expect("actionId").to_string(),
            available_on_page: row.get("availableOnPage").and_then(Value::as_bool),
            confirmed: row.get("confirmed").and_then(Value::as_bool),
            declined: row.get("declined").and_then(Value::as_bool),
            human_approved: row.get("humanApproved").and_then(Value::as_bool),
            enabled_action_ids: Some(enabled.clone()),
        };
        let decision = authorizer.evaluate(&request);
        expect_decision(&decision, &row["expect"], row["actionId"].as_str().expect("actionId"));
    }
}

/// The decision assertion. The gate's answer is matched, not stringified: the
/// assertion reads the fields that carry meaning (the confirmation mode and
/// its recorded prompt; the denial reason), and refuses a decision that
/// merely resembles the recorded one.
fn expect_decision(decision: &PolicyDecision, expected: &Value, action_id: &str) {
    match decision {
        PolicyDecision::Allowed { action } => {
            assert_eq!(expected["decision"].as_str().expect("expect.decision"), "Allowed");
            assert_eq!(action.id, action_id, "the authorized action is the requested one");
        }
        PolicyDecision::ConfirmationRequired { action, mode, prompt } => {
            assert_eq!(expected["decision"].as_str().expect("expect.decision"), "ConfirmationRequired");
            assert_eq!(mode.as_str(), expected["mode"].as_str().expect("expect.mode"));
            assert_eq!(*prompt, expected["prompt"].as_str().expect("expect.prompt"));
            assert_eq!(action.id, action_id);
        }
        PolicyDecision::Denied { action: _, reason, message: _ } => {
            assert_eq!(expected["decision"].as_str().expect("expect.decision"), "Denied");
            assert_eq!(reason.as_str(), expected["reason"].as_str().expect("expect.reason"));
        }
    }
}

/// The shipped registry is one artifact: both business models' action spines
/// resolve against the SAME vocabulary. A kit gets its scope from the
/// configuration, not from a registry that was filtered for it — filtering the
/// vocabulary itself would be the fork this whole consumer pair refuses.
#[test]
fn the_shipped_registry_carries_both_business_models_without_a_preference() {
    for action_id in ["order.status.read", "cart.item.add", "crm.followup.create"] {
        assert!(
            ActionRegistry::has(action_id),
            "shipped registry carries {action_id} for every business model"
        );
    }
    assert!(!ActionRegistry::has("kit.wholesale.special"), "an invented action id is not registered — ids are shipped, not invented");
}

/// Cross-workspace isolation is a bug-shaped refusal, not empty data: a value
/// owned by another tenant is a violation, and an absent value is a distinct
/// missing-tenant error.
#[test]
fn a_record_from_another_workspace_is_refused_through_the_seam() {
    let config = config();
    let workspace = workspace_tenant(&config);
    let foreign_slug = config["workspace"]["foreignTenant"].as_str().expect("workspace.foreignTenant");
    let foreign = TenantId::new(foreign_slug)
        .expect("the foreign tenant is also a valid slug (isolation is semantic, not representational)");

    let read = assert_tenant(Some(Packet(foreign.clone())), &workspace);
    assert!(
        matches!(read, Err(TenantScopeError::Violation { .. })),
        "a cross-workspace read is a violation, not empty data"
    );
    let absent = assert_tenant::<TenantScope>(None, &workspace);
    assert!(
        matches!(absent, Err(TenantScopeError::Missing { .. })),
        "an absent record is missing, not an empty success"
    );
}
