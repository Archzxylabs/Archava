//! Type conversions and error bridges between CRM host data and `archava_assistant` contracts.

use archava_assistant::{
    AccessView, ActivityKind, AssistantError, FollowUpStatus, PipelineStage, Scope,
};
use uuid::Uuid;

use crate::authority::AuthorityError;
use crate::crm::{CrmAccess, CrmScope, CRM_LEDGER};

/// `AuthorityError` → [`AssistantError`] for the tools. Missing/foreign
/// membership keeps the baseline authorizer's own refusal; stored data
/// outside its vocabulary stays an unavailability (`UnusableGrant` is an
/// operator problem, never a caller answer).
pub(crate) fn refusal(error: AuthorityError) -> AssistantError {
    match error {
        AuthorityError::MissingMembership | AuthorityError::MissingScope => {
            AssistantError::forbidden("CRM membership required.")
        }
        AuthorityError::InvalidSubmission => {
            AssistantError::invalid("The submission could not be processed.")
        }
        other => {
            tracing::error!(error = %other, "conversation refused: the CRM could not be reached");
            AssistantError::unavailable("Service temporarily unavailable.")
        }
    }
}

/// A database failure inside a tool — logged with the ledger label, answered
/// as an unavailability.
pub(crate) fn db_refusal(error: sqlx::Error) -> AssistantError {
    refusal(AuthorityError::database(CRM_LEDGER.ledger, error))
}

/// A `TenantContextError` inside a tool — the same unavailability shape.
pub(crate) fn context_refusal(error: archava_adapters::rls::TenantContextError) -> AssistantError {
    refusal(AuthorityError::from(error))
}

/// The scope enum bridged to the contracts'.
pub(crate) fn bridged_scope(scope: CrmScope) -> Scope {
    match scope {
        CrmScope::All => Scope::All,
        CrmScope::Assigned => Scope::Assigned,
    }
}

/// The access view the flow sees.
pub(crate) fn bridged_access(access: &CrmAccess) -> AccessView {
    AccessView {
        scope: bridged_scope(access.scope),
        owner_id: access.owner_id.clone(),
        permissions: access.permissions.clone(),
    }
}

/// A stored pipeline stage bound to its vocabulary; anything else is stored
/// corruption, refused loudly — the rule `crm.rs` established for scope.
pub(crate) fn bridged_stage(raw: &str) -> Result<PipelineStage, AssistantError> {
    match raw {
        "new" => Ok(PipelineStage::New),
        "qualified" => Ok(PipelineStage::Qualified),
        "proposal" => Ok(PipelineStage::Proposal),
        "won" => Ok(PipelineStage::Won),
        "lost" => Ok(PipelineStage::Lost),
        other => {
            tracing::error!(value = %other, "refusing: stored pipeline stage outside its vocabulary");
            Err(AssistantError::unavailable(
                "A stored pipeline stage could not be trusted.",
            ))
        }
    }
}

/// A stored activity kind bound to its vocabulary.
pub(crate) fn bridged_activity_kind(raw: &str) -> Result<ActivityKind, AssistantError> {
    match raw {
        "note" => Ok(ActivityKind::Note),
        "call" => Ok(ActivityKind::Call),
        "meeting" => Ok(ActivityKind::Meeting),
        other => {
            tracing::error!(value = %other, "refusing: stored activity kind outside its vocabulary");
            Err(AssistantError::unavailable(
                "A stored activity kind could not be trusted.",
            ))
        }
    }
}

/// A stored follow-up status bound to its vocabulary.
pub(crate) fn bridged_followup_status(raw: &str) -> Result<FollowUpStatus, AssistantError> {
    match raw {
        "open" => Ok(FollowUpStatus::Open),
        "completed" => Ok(FollowUpStatus::Completed),
        other => {
            tracing::error!(value = %other, "refusing: stored follow-up status outside its vocabulary");
            Err(AssistantError::unavailable(
                "A stored follow-up status could not be trusted.",
            ))
        }
    }
}

/// The caller's `\`, `%` and `_` stay literal — the same three lines
/// `crm.rs`'s list uses, so a search value can never widen into a wildcard.
pub(crate) fn escaped_search(search: &str) -> String {
    search
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// A canonical id bound AS a uuid — the served ids are uuid strings, but a
/// text bind would make `id>$4`-style comparisons an operator error (the
/// parameter carries a text OID). `crm.rs` binds parsed uuids; so does this.
pub(crate) fn as_uuid(id: &str, what: &'static str) -> Result<Uuid, AssistantError> {
    Uuid::parse_str(id).map_err(|_| AssistantError::invalid(format!("the {what} is not a uuid")))
}

/// A stage's storage name, for binds.
pub(crate) fn stage_text(stage: PipelineStage) -> &'static str {
    match stage {
        PipelineStage::New => "new",
        PipelineStage::Qualified => "qualified",
        PipelineStage::Proposal => "proposal",
        PipelineStage::Won => "won",
        PipelineStage::Lost => "lost",
    }
}
