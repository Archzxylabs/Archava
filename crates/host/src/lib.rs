//! The Axum HTTP surface for the Rust release candidate.
//!
//! The host's job is to be the only place where an unauthenticated request
//! becomes a tenant-scoped database transaction. Everything past this boundary
//! is authority-checked; everything before it is not, and must not touch the
//! database at all.
//!
//! ## Server-side authority (audit RA01, closed)
//!
//! Authority is never read from a request body. A request arrives, presents a
//! *session token this server issued*, and the host resolves everything else
//! from the database:
//!
//! - `identity` — the port of `packages/identity-pg`: scrypt passwords, HMAC
//!   session digests, CSRF derived from the token, login rate buckets. Only
//!   [`encode_password`] and [`IdentityError`] are public; the service itself
//!   is owned by [`AppState`].
//! - `authority` — the membership grant (role/capability/scope columns from
//!   migration 004), the scope rule, and bound single-use human approvals.
//! - [`router`] — extraction → session → grant → gate → write, in that order,
//!   with the negative tests that pin each refusal.
//!
//! Startup readiness — the baseline's `assert*Kernel`, including both
//! `schema_migrations` `(id, sha256)` ledgers — runs in [`AppState::try_new`]
//! and refuses to build serving state that has not passed; its fixture-backed
//! suite is `tests/readiness.rs`, and the authority suite is
//! `tests/authority.rs`.
//!
//! The persistent Leptos launcher lives in `crates/slice/src/bin` and joins
//! this router to the SSR/WASM document only after this crate's complete
//! readiness constructor succeeds. The fixture-backed `slice_server` remains
//! a separate preview/proof entrypoint; it is not the production runtime.

pub mod assistant;
mod authority;
pub mod cockpit;
pub mod crm;
// The ERP host surface — Archxy 2's bounded cohorts (R06 onward), one module
// per ledger family, transactions opened through the same TenantTransaction
// gate as `crm`. PACKED-000 records this lib.rs diff for the integration
// writer along with the other shared-file patches.
pub mod erp;
pub mod feed;
mod identity;
pub mod ingestion;
mod metrics;
pub mod orbz;
// C03: the persisted owner-configuration lifecycle (archava_owner ledger).
pub mod mail;
pub mod own;
pub mod payments;
pub mod presence;
pub mod preview;
pub mod router;
mod security;
pub mod webhooks;

#[cfg(test)]
mod extractors;

pub use authority::AuthorityError;
pub use identity::{encode_password, max_active_derivations, IdentityError};
pub use router::{router, AllReadiness, AppState, Ledger};
// The same header policy the JSON router carries, exported so the SSR document
// router cannot be assembled without it (audit H10: `/app` bypassed the headers
// because the layer lived only on the host router).
pub use security::{document_security_headers, security_headers};
