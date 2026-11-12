//! Business-module domain rules for the Rust host.
//!
//! This crate holds rules that must be true regardless of how a request arrived
//! — HTTP, a native client, a replay. It deliberately depends on no HTTP
//! framework: a domain rule that can open a socket is a domain rule that can be
//! reached without the host's authority checks.
//!
//! R01 scope is the authority seam only. The domain modules themselves
//! (`docs/archive/rust/MIGRATION_INVENTORY.md` lists them) are ported at R02–R05, and
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

#![allow(clippy::inconsistent_digit_grouping)]

pub mod authority;
pub mod erp;
pub mod feed;
pub mod hardware;
pub mod ingestion;
pub mod messaging;
pub mod modality;
pub mod operator;
pub mod presence;
pub mod presets;
pub mod preview;
pub mod telematics;

pub use authority::{ActionDenial, AuthorizedCommand, CommandAuthorizer};
pub use feed::{generate_action_feed, ActionCard, CardAction, CardCategory, CardPriority};
pub use ingestion::{
    execute_batch_commit, match_column, reconcile_schema, BatchCommitResult, ColumnMapping,
    DiffPreview, IngestionError, RawTableInput, TargetEntityKind,
};
pub use modality::AvaModality;
pub use presence::{
    generate_livekit_room_token, PresenceError, PresenceFallbackEvent, PresenceGateway,
    PresenceMode, PresenceSession, VoiceTranscriptFrame, DEFAULT_SESSION_TTL_MS,
    MAX_PRESENCE_SESSIONS_PER_TENANT,
};
pub use presets::{IndustryProfile, ProfileMask};
pub use preview::{generate_preview, PreviewBrand, PreviewProduct, PreviewProjection, Sector};
