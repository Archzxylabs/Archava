//! Core contracts, error taxonomy, and the policy seam.
//!
//! Every type in this crate mirrors a TypeScript contract that already exists
//! and is already enforced, so the Rust host inherits real authority rules
//! rather than inventing its own:
//!
//! | Rust | TypeScript source |
//! | --- | --- |
//! | [`graph`] | `packages/core/src/context-graph.ts` |
//! | [`tenant`] | `packages/core/src/tenant.ts` |
//! | [`roles`] | `packages/acl/src/roles.ts` |
//! | [`actions`] | `packages/acl/src/actions.ts` |
//!
//! ADR-R07: the security boundary does not move because the implementation
//! language did. Authorization is enforced by native PostgreSQL row-level
//! security and by domain permissions, so this crate's job is to make the
//! fail-closed decision *explicitly* and to make a denial observable. Rust's
//! memory safety does not satisfy G03; a denial test at a real entrypoint does.

pub mod actions;
pub mod graph;
pub mod roles;
pub mod tenant;

pub use actions::{ActionDefinition, ActionLevel, ConfirmationMode};
pub use graph::{
    fold_context_events, parse_context_graph, reduce_context_graph, seed_context_graph,
    ContextActionAvailability, ContextCart, ContextCheckout, ContextComparison, ContextEntityRef,
    ContextError, ContextEvent, ContextForm, ContextGraph, ContextGraphError, ContextPage,
    ContextSession,
};
pub use roles::{AdminPermission, AdminResource, RoleName};
pub use tenant::{assert_tenant, scoped, TenantId, TenantScope, TenantScopeError};
