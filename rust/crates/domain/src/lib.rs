//! Business-module domain rules for the Rust host.
//!
//! This crate holds rules that must be true regardless of how a request arrived
//! — HTTP, a native client, a replay. It deliberately depends on no HTTP
//! framework: a domain rule that can open a socket is a domain rule that can be
//! reached without the host's authority checks.
//!
//! R01 scope is the authority seam only. The domain modules themselves
//! (`docs/rust/MIGRATION_INVENTORY.md` lists them) are ported at R02–R05, and
//! this crate grows a module per ledger as each is verified against its
//! TypeScript original.
//!
//! ## The one rule this crate already enforces
//!
//! Every domain operation takes a checked `TenantTransaction` and never a
//! `PgPool`. That is not stylistic: a repository reachable with a pool can run a
//! statement outside any tenant transaction, and RLS would then evaluate
//! `current_setting('archava.tenant_id', true)` against an unset value and
//! return nothing — a silent empty result rather than an error, which is the
//! failure PRD §15 calls a bug rather than a recoverable error. Taking the
//! transaction makes the cohort the only path that exists.
//!
//! The baseline makes the same choice at every entry point: `lockCommerceProducts`
//! and `readCommerceStock` (`packages/commerce-pg/src/kernel.ts:87,114`) both
//! receive a `PoolClient` and begin `await context(client, tenant)`.

pub mod authority;
pub mod erp;

pub use authority::{ActionDenial, AuthorizedCommand, CommandAuthorizer};
