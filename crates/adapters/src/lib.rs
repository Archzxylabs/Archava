//! Non-owner RLS transaction cohort and raw SQL adapters.
//!
//! This crate is where G03 lives. The security boundary does not move because
//! the implementation language did (ADR-R07): authorization is still enforced by
//! native PostgreSQL row-level security, and this crate's job is to make the
//! cohort that RLS depends on impossible to forget.
//!
//! The pattern, transcribed from the baseline rather than invented:
//!
//! ```text
//! BEGIN;
//!   SELECT set_config('archava.tenant_id', $1, true);   -- transaction-local
//!   lock_membership(...);                               -- caller-owned identity
//!   SELECT ... FOR UPDATE;                              -- explicit row locks
//! COMMIT;
//! ```
//!
//! Verified against live sources:
//! - `packages/laundry-pg/src/repository.ts:175` —
//!   `client.query("SELECT set_config('archava.tenant_id', $1, true)", [tenant])`
//!   (`formal-education-pg/src/repository.ts:188`,
//!   `field-services-pg/src/repository.ts:158`,
//!   `projects-pg/src/repository.ts:158` are the same statement)
//! - `packages/scheduling-pg/migrations/001_resource_reservations.sql:53-57` —
//!   `CREATE POLICY ... USING(tenant_id=current_setting('archava.tenant_id',true))
//!    WITH CHECK(tenant_id=current_setting('archava.tenant_id',true))`
//! - the same file, lines 44-52 — `ALTER TABLE ... FORCE ROW LEVEL SECURITY`, so
//!   the table owner is subject to the policy too, not exempt from it
//! - `packages/commerce-pg/src/kernel.ts:82-85` reads the setting back and throws
//!   `Commerce kernel requires caller-owned tenant transaction` on mismatch
//!
//! The `true` third argument to `set_config` is what makes the setting
//! transaction-local: it disappears at COMMIT, so a pooled connection cannot
//! leak one tenant's context into the next transaction. That is the property the
//! baseline relies on and the one a port must not drop.

pub mod rls;
// The ERP namespace (Archxy 2, PACKET-000): purchasing repository + the
// commerce kernel primitives the stock cohort reconciles against.
/// The CRM transactional persistence adapter (ADR-014).
pub mod crm;
pub mod erp;

pub use rls::{assert_startup_readiness, TenantContextError, TenantTransaction};

/// Process-fixed serving writer lease and readiness.
pub mod writer;
