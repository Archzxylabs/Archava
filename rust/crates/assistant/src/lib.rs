//! CRM assistant conversation — contracts, planner and server-side validation.
//!
//! Port lineage:
//! - `packages/crm-assistant/src/contracts.ts` → [`contracts`]
//! - `packages/crm-assistant/src/planner.ts` (`LocalCrmPlanner`) → [`planner`]
//! - `packages/crm-assistant/src/conversation.ts` (`createCrmConversation`) →
//!   [`conversation`]
//!
//! The design rule this crate exists to keep: **planner output is a request**.
//! No permission, tenant, identifier scope or write EVER travels with the
//! plan; the flow rechecks the caller's CRM grants on every turn and refuses
//! what the grants do not cover (`contacts.read` gates the turn, per
//! operation `pipeline.read` and `followups.write` are re-read, and every
//! tool re-checks again inside its own transaction). The turn loop, analytics
//! and handoff machinery of `packages/assistant` are intentionally absent —
//! they are the delivery vehicle of a real model drive, and this R03 slice is
//! the planning/validation half whose required behavior is "assistant
//! proposes, server authorizes and validates".
//!
//! [`HostCrm`], the trait in [`contracts`], is the same seam
//! `createCrmConversation(service, planner, pipeline?)` already had: the flow
//! speaks in tool invocations and the host supplies the database, opening one
//! transaction and re-authorizing per tool call — the shape of the baseline's
//! `repository.transaction(tenant, principal, permission, ...)`.

pub mod contracts;
pub mod conversation;
pub mod planner;

pub use contracts::{
    AccessView, ActivityKind, ActivityView, AssistantError, Bucket, CompletionPrepare, ContactPage,
    ContactRef, ConversationData, ConversationDraft, ConversationInput, ConversationPlan,
    ConversationReply, Cursor, DraftStatus, ErrorCode, FollowUp, FollowUpStatus, FollowupCounts,
    FollowupDetail, FollowupQueue, FollowupQueueItem, FollowupView, HostCrm, Item,
    OpportunityDetail, OpportunityItem, OpportunityPage, Overview, PipelineCounts, PipelineStage,
    PlannerIdentity, PreparedDraft, Scope, Source, SummaryContact, Transition,
};
pub use conversation::{ask, describe, ConversationContext};
pub use planner::{LocalCrmPlanner, Planner};
