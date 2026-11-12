//! The provider attempt ledger — the port of `packages/provider-runtime`.
//!
//! Port lineage:
//! - `packages/provider-runtime/src/index.ts` (the schemas, `runProvider`) →
//!   [`contracts`] + [`runtime`]
//! - `packages/provider-runtime/src/postgres.ts`
//!   (`PostgresProviderAttemptStore`, `migrateProviderRuntime`,
//!   `holdProviderAfterRestore`, `readProviderStatus`) → [`store`]
//! - `packages/provider-runtime/migrations/001_provider_attempts.sql` — the
//!   same SQL bytes, applied through the fixture's single-writer ledger (and
//!   applyable standalone through [`store::migrate_provider_runtime`]).
//!
//! ## The budget rule this module exists to enforce (AI-02)
//!
//! Reserve the budget lease BEFORE invoking; finalize the authoritative
//! outcome AFTER the invoke. A crash, cancel or lost HTTP response after the
//! request was sent can leave an unrecorded usage — those attempts record
//! `interrupted` and stay unknown until reconciliation, and any reserve that
//! could not be *durable* fails closed (`outcome_unknown`): a caller that
//! cannot prove the provider was NOT invoked must not call it again for the
//! same request, and must not assume the budget counted.
//!
//! ## What is deliberately absent
//!
//! No prompts, results, record identities or credentials reach the ledger.
//! The store sees only the policy's ids/limits and the completion's status
//! code enum — the privacy boundary `runtime.test.ts` pins with a serialized
//! mock inspection (`never passes its contents to the ledger`), pinned in
//! Rust by the same witness over a scripted store.
//!
//! Legacy Gemini/Postmark seams remain injected and synthetic-only. Native
//! bounded HTTP and Midtrans transports live in [`http`] and [`midtrans`].
//! Their authenticated responses still require host-side tenant authority,
//! durable invoice correlation and provider budget admission before financial
//! effects; transport success alone is not a settlement receipt.

pub mod contracts;
pub mod gemini;
pub mod http;
pub mod midtrans;
pub mod pilot;
pub mod postmark;
pub mod resend;
pub mod runtime;
pub mod store;
pub mod voice;

pub use contracts::{
    Admission, AdmissionRefusal, Completion, FailureCode, InvokeError, PolicyError, ProviderPolicy,
    DEFAULT_POLICY_LIMITS,
};
pub use runtime::{run_provider, ProviderAttemptStore, ProviderRunFailure, StoreError};
pub use store::{
    hold_provider_after_restore, migrate_provider_runtime, read_provider_status,
    PostgresProviderAttemptStore, ProviderAttemptView, ProviderStatus, SetupError,
};

/// Structured business planning transport.
pub mod agent;
