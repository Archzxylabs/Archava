//! Knowledge: the tenant-scoped corpus and the retrieval decision.
//!
//! Port lineage (each submodule names its source):
//!
//! | Rust | TypeScript source |
//! | --- | --- |
//! | [`schema`] | `packages/knowledge/src/schema.ts` |
//! | [`structured_truth`] | `packages/knowledge/src/structured-truth.ts` |
//! | [`text`] | `packages/knowledge/src/text.ts` |
//! | [`store`] | `packages/knowledge/src/store.ts` |
//! | [`retrieval`] | `packages/knowledge/src/retrieval.ts` |
//!
//! Public error types re-exported here so a caller does not need to know which
//! submodule refused.

pub mod retrieval;
pub mod schema;
pub mod store;
pub mod structured_truth;
pub mod text;

pub use retrieval::KnowledgeRetrievalError;
pub use schema::KnowledgeSchemaError;
pub use store::{KnowledgeIsolationError, KnowledgeTenantError};
