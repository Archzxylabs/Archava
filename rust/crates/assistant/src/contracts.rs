//! The conversation wire shapes and their validation — the port of
//! `packages/crm-assistant/src/contracts.ts`.
//!
//! Every type here mirrors its zod counterpart field-for-field, including the
//! two rules the baseline encodes as `.strict()`: unknown keys refuse, and
//! stored vocabulary is closed. Parsing happens through [`ConversationInput::parse`]
//! and [`ConversationPlan::parse`] (the baseline's `z.parse`/`safeParse`),
//! which is what makes planner output a *request*: a plan that was never
//! produced by these contracts is exactly the "unverified plan" refusal the
//! flow is required to answer with [`ErrorCode::Unavailable`].

use serde::{Deserialize, Serialize};

/// The refusal vocabulary of the CRM surface, ported from TS `CrmError`'s
/// `code` union. The host route maps each to its HTTP status and masks the
/// message the way `apps/crm/src/host.ts` does (the detailed `message` is for
/// logs and tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Unauthenticated,
    Forbidden,
    NotFound,
    Conflict,
    Unavailable,
    /// A caller mistake caught before any authority question — the port of
    /// what zod rejects at parse time (unknown key, out-of-range length, a
    /// non-uuid identifier, a non-instant timestamp).
    Invalid,
}

#[derive(Debug, Clone)]
pub struct AssistantError {
    pub code: ErrorCode,
    pub message: String,
}

impl AssistantError {
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Forbidden,
            message: message.into(),
        }
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Conflict,
            message: message.into(),
        }
    }
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Unavailable,
            message: message.into(),
        }
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::NotFound,
            message: message.into(),
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Invalid,
            message: message.into(),
        }
    }
}

impl core::fmt::Display for AssistantError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for AssistantError {}

/// A canonical UUID string, checked shape-first (8-4-4-4-12 lowercase-insensitive
/// hex). The baseline validates with `z.uuid()` at every id boundary; the Rust
/// type keeps the same check without pulling the uuid crate into this pure
/// dependency leaf.
fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36
        || bytes[8] != b'-'
        || bytes[13] != b'-'
        || bytes[18] != b'-'
        || bytes[23] != b'-'
    {
        return false;
    }
    let hex = |group: &[u8]| group.iter().all(|&b| b.is_ascii_hexdigit());
    (0..36).all(|i| {
        if matches!(i, 8 | 13 | 18 | 23) {
            true
        } else {
            let group = match i {
                0..=7 => &bytes[0..8],
                9..=12 => &bytes[9..13],
                14..=17 => &bytes[14..18],
                19..=22 => &bytes[19..23],
                _ => &bytes[24..36],
            };
            hex(group)
        }
    })
}

/// An ISO-8601 instant, the port of `z.iso.datetime()`: calendar date, `T`,
/// clock time and either `Z` or an explicit offset. The shape check mirrors
/// what the schema refuses at parse time (a malformed instant is a caller
/// mistake, answered 400 before any database work); the DATABASE stays the
/// calendar authority beyond that, which is the same convention the CRM
/// commands ride (`crm.rs`'s boundary comment).
fn is_instant(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20 {
        return false;
    }
    let digits = |slice: &[u8]| slice.iter().all(|b| b.is_ascii_digit());
    if !digits(&bytes[0..4])
        || bytes[4] != b'-'
        || !digits(&bytes[5..7])
        || bytes[7] != b'-'
        || !digits(&bytes[8..10])
    {
        return false;
    }
    if bytes[10] != b'T'
        || !digits(&bytes[11..13])
        || bytes[13] != b':'
        || !digits(&bytes[14..16])
        || bytes[16] != b':'
    {
        return false;
    }
    if !digits(&bytes[17..19]) {
        return false;
    }
    // Optional fractional seconds, then the zone. zod's `.datetime()` accepts
    // `Z` or `±HH:MM`, with fractions of at least one digit.
    let mut rest = &bytes[19..];
    if !rest.is_empty() && rest[0] == b'.' {
        let digits_only = &rest[1..];
        let count = digits_only
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if count == 0 {
            return false;
        }
        rest = &digits_only[count..];
    }
    match rest.first() {
        Some(b'Z') => rest.len() == 1,
        Some(b'+') | Some(b'-') => {
            rest.len() == 6 && digits(&rest[1..3]) && rest[3] == b':' && digits(&rest[4..6])
        }
        _ => false,
    }
}

/// The trimmed length in CHARACTERS, the way `.max(n)` counts JS strings.
fn trimmed_len(value: &str) -> usize {
    value.trim().chars().count()
}

/// The worklist cursor — due-time/UUID keyset. Both halves are required; the
/// PostgreSQL side keeps microseconds across page boundaries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Cursor {
    pub due_at: String,
    pub id: String,
}

impl Cursor {
    pub fn parse(value: serde_json::Value) -> Result<Self, AssistantError> {
        let cursor: Cursor = serde_json::from_value(value).map_err(|_| {
            AssistantError::invalid("the follow-up cursor is not the recorded shape")
        })?;
        if !is_instant(&cursor.due_at) {
            return Err(AssistantError::invalid(
                "the cursor's due time is not an instant",
            ));
        }
        if !is_uuid(&cursor.id) {
            return Err(AssistantError::invalid("the cursor's id is not a uuid"));
        }
        Ok(cursor)
    }
}

pub fn parse_uuid(value: &serde_json::Value, what: &str) -> Result<String, AssistantError> {
    let id = value
        .as_str()
        .ok_or_else(|| AssistantError::invalid(format!("the {what} is not a uuid")))?;
    if !is_uuid(id) {
        return Err(AssistantError::invalid(format!("the {what} is not a uuid")));
    }
    Ok(id.to_string())
}

/// The optional follow-up draft a planner flow may carry. Strict like the
/// baseline's `.strict()` draft object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConversationDraft {
    pub summary: String,
    pub due_at: String,
}

/// `conversationInputSchema`. The utterance is trimmed and bounded, and only
/// the documented optional keys exist.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConversationInput {
    pub utterance: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opportunity_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub followup_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub followup_after: Option<Cursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub followup_as_of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<ConversationDraft>,
}

impl ConversationInput {
    /// The port of `conversationInputSchema.parse`: deserialize strictly, then
    /// run the boundary checks zod encodes (trim/lengths/vocabulary) in the
    /// same order. Every failure is [`ErrorCode::Invalid`] — the host answers
    /// 400 `Invalid request.` without database work.
    pub fn parse(value: serde_json::Value) -> Result<Self, AssistantError> {
        let input: ConversationInput = serde_json::from_value(value).map_err(|_| {
            AssistantError::invalid("the conversation input is not the recorded shape")
        })?;
        if !(1..=1000).contains(&trimmed_len(&input.utterance)) {
            return Err(AssistantError::invalid(
                "the utterance is empty or over its bound",
            ));
        }
        for (what, id) in [
            ("contact id", input.contact_id.as_deref()),
            ("opportunity id", input.opportunity_id.as_deref()),
            ("follow-up id", input.followup_id.as_deref()),
            ("page cursor", input.after.as_deref()),
        ] {
            // An absent optional id carries no check; a present one must be a
            // canonical uuid before it is ever *resolved* against a tenant's
            // visible rows.
            if let Some(id) = id {
                if !is_uuid(id) {
                    return Err(AssistantError::invalid(format!("the {what} is not a uuid")));
                }
            }
        }
        if let Some(as_of) = &input.followup_as_of {
            if !is_instant(as_of) {
                return Err(AssistantError::invalid(
                    "the follow-up instant is not an instant",
                ));
            }
        }
        if let Some(draft) = &input.draft {
            if !(1..=2000).contains(&trimmed_len(&draft.summary)) {
                return Err(AssistantError::invalid(
                    "the draft summary is empty or over its bound",
                ));
            }
            if !is_instant(&draft.due_at) {
                return Err(AssistantError::invalid(
                    "the draft due time is not an instant",
                ));
            }
        }
        Ok(input)
    }
}

/// `followupBucketSchema`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Bucket {
    Open,
    Overdue,
    Upcoming,
}

/// `pipelineStageSchema`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PipelineStage {
    New,
    Qualified,
    Proposal,
    Won,
    Lost,
}

impl PipelineStage {
    fn parse(value: &serde_json::Value) -> Result<Self, AssistantError> {
        serde_json::from_value(value.clone())
            .map_err(|_| AssistantError::invalid("the stage is outside its vocabulary"))
    }
}

/// A contact as the conversation reports it — the identifier, one name and
/// the revision the follow-up command pins against. Never an email or owner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub full_name: String,
    pub revision: i64,
}

/// An opportunity as the conversation reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OpportunityItem {
    pub id: String,
    pub contact_id: String,
    pub title: String,
    pub stage: PipelineStage,
    pub revision: i64,
}

/// `conversationPlanSchema` — the discriminated union a planner may emit,
/// keyed on `operation`. Unknown operations refuse at parse time; a plan that
/// parses is still only a REQUEST.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "camelCase")]
pub enum ConversationPlan {
    #[serde(rename = "crm.overview")]
    CrmOverview,
    #[serde(rename = "followup.complete.prepare")]
    FollowupCompletePrepare,
    #[serde(rename = "followups.list")]
    FollowupsList { bucket: Bucket },
    #[serde(rename = "followup.prepare")]
    FollowupPrepare {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
    },
    #[serde(rename = "contacts.list")]
    ContactsList {
        #[serde(default)]
        search: String,
    },
    #[serde(rename = "contact.summary")]
    ContactSummary {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
    },
    #[serde(rename = "unsupported")]
    Unsupported,
    #[serde(rename = "opportunities.list")]
    OpportunitiesList {
        #[serde(default)]
        search: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stage: Option<PipelineStage>,
    },
    #[serde(rename = "opportunity.summary")]
    OpportunitySummary {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
    },
}

fn plan_query(
    value: Option<&serde_json::Value>,
    what: &str,
) -> Result<Option<String>, AssistantError> {
    let Some(value) = value else { return Ok(None) };
    let query = value
        .as_str()
        .ok_or_else(|| AssistantError::invalid(format!("the {what} is not a text value")))?;
    let trimmed = query.trim();
    if !(1..=120).contains(&trimmed.chars().count()) {
        return Err(AssistantError::invalid(format!(
            "the {what} is empty or over its bound"
        )));
    }
    Ok(Some(trimmed.to_string()))
}

fn plan_search(value: &serde_json::Value, what: &str) -> Result<String, AssistantError> {
    let search = value
        .as_str()
        .ok_or_else(|| AssistantError::invalid(format!("the {what} is not a text value")))?;
    let trimmed = search.trim();
    if trimmed.chars().count() > 120 {
        return Err(AssistantError::invalid(format!(
            "the {what} is over its bound"
        )));
    }
    Ok(trimmed.to_string())
}

impl ConversationPlan {
    /// The port of `conversationPlanSchema.safeParse`. This is the step that
    /// makes a foreign planner's output *verifiable*: anything outside this
    /// union — however plausible it reads — is refused, and the flow turns the
    /// refusal into the `unavailable` answer. Each member object is
    /// `.strict()` exactly where the baseline is: unknown keys refuse, and the
    /// documented fields carry their recorded bounds.
    pub fn parse(value: serde_json::Value) -> Result<Self, AssistantError> {
        let object = value
            .as_object()
            .ok_or_else(|| {
                AssistantError::invalid("the conversation plan is not the recorded shape")
            })?
            .clone();
        let operation = object
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AssistantError::invalid("the conversation plan has no operation"))?
            .to_string();
        let mut rest = object;
        rest.remove("operation");
        // The member objects the baseline writes as `{...fields}.strict()`:
        // only their documented keys may appear.
        let only = |rest: &serde_json::Map<String, serde_json::Value>, allowed: &[&str]| {
            rest.keys().all(|key| allowed.contains(&key.as_str()))
        };
        match operation.as_str() {
            "crm.overview" | "followup.complete.prepare" | "unsupported" => {
                if !rest.is_empty() {
                    return Err(AssistantError::invalid(
                        "the conversation plan carries unknown keys",
                    ));
                }
                Ok(match operation.as_str() {
                    "crm.overview" => ConversationPlan::CrmOverview,
                    "followup.complete.prepare" => ConversationPlan::FollowupCompletePrepare,
                    _ => ConversationPlan::Unsupported,
                })
            }
            "followups.list" => {
                if !only(&rest, &["bucket"]) {
                    return Err(AssistantError::invalid(
                        "the conversation plan carries unknown keys",
                    ));
                }
                let bucket = rest
                    .get("bucket")
                    .ok_or_else(|| AssistantError::invalid("the follow-up bucket is required"))?;
                let bucket = serde_json::from_value::<Bucket>(bucket.clone()).map_err(|_| {
                    AssistantError::invalid("the follow-up bucket is outside its vocabulary")
                })?;
                Ok(ConversationPlan::FollowupsList { bucket })
            }
            "followup.prepare" => {
                if !only(&rest, &["query"]) {
                    return Err(AssistantError::invalid(
                        "the conversation plan carries unknown keys",
                    ));
                }
                let query = plan_query(rest.get("query"), "follow-up name")?;
                Ok(ConversationPlan::FollowupPrepare { query })
            }
            "contacts.list" => {
                if !only(&rest, &["search"]) {
                    return Err(AssistantError::invalid(
                        "the conversation plan carries unknown keys",
                    ));
                }
                let search = plan_search(
                    rest.get("search").unwrap_or(&serde_json::Value::Null),
                    "contact search",
                )?;
                Ok(ConversationPlan::ContactsList { search })
            }
            "contact.summary" => {
                if !only(&rest, &["query"]) {
                    return Err(AssistantError::invalid(
                        "the conversation plan carries unknown keys",
                    ));
                }
                let query = plan_query(rest.get("query"), "contact name")?;
                Ok(ConversationPlan::ContactSummary { query })
            }
            "opportunities.list" => {
                if !only(&rest, &["search", "stage"]) {
                    return Err(AssistantError::invalid(
                        "the conversation plan carries unknown keys",
                    ));
                }
                let search = plan_search(
                    rest.get("search").unwrap_or(&serde_json::Value::Null),
                    "opportunity search",
                )?;
                let stage = match rest.get("stage") {
                    Some(value) if !value.is_null() => Some(PipelineStage::parse(value)?),
                    _ => None,
                };
                Ok(ConversationPlan::OpportunitiesList { search, stage })
            }
            "opportunity.summary" => {
                if !only(&rest, &["query"]) {
                    return Err(AssistantError::invalid(
                        "the conversation plan carries unknown keys",
                    ));
                }
                let query = plan_query(rest.get("query"), "opportunity name")?;
                Ok(ConversationPlan::OpportunitySummary { query })
            }
            other => Err(AssistantError::invalid(format!(
                "the conversation plan names an operation outside its vocabulary: {other}"
            ))),
        }
    }
}

/// The overview snapshot (`crmOverviewSchema`), carrying its two invariants
/// the same way the baseline does — as REFUSAL conditions at construction,
/// not as silent counts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Overview {
    pub tenant_id: String,
    pub as_of: String,
    pub scope: Scope,
    pub contacts: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub followups: Option<FollowupCounts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<PipelineCounts>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    All,
    Assigned,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FollowupCounts {
    pub open: i64,
    pub overdue: i64,
    pub upcoming: i64,
    pub completed: i64,
}

impl FollowupCounts {
    /// `open === overdue + upcoming` is the baseline's own refusal condition.
    pub fn verified(
        open: i64,
        overdue: i64,
        upcoming: i64,
        completed: i64,
    ) -> Result<Self, AssistantError> {
        if open < 0 || overdue < 0 || upcoming < 0 || completed < 0 || open != overdue + upcoming {
            return Err(AssistantError::unavailable(
                "CRM overview could not be verified.",
            ));
        }
        Ok(Self {
            open,
            overdue,
            upcoming,
            completed,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PipelineCounts {
    pub total: i64,
    pub active: i64,
    pub new: i64,
    pub qualified: i64,
    pub proposal: i64,
    pub won: i64,
    pub lost: i64,
}

impl PipelineCounts {
    /// All seven counts arrive from the database independently; the
    /// baseline's refusal conditions (`active === new + qualified + proposal`
    /// and `total === active + won + lost`) decide they are SERVED only when
    /// they add up. A mismatch is an untrustworthy overview, answered
    /// `unavailable` — never a silently corrected snapshot.
    #[allow(clippy::too_many_arguments)]
    pub fn verified(
        total: i64,
        active: i64,
        new: i64,
        qualified: i64,
        proposal: i64,
        won: i64,
        lost: i64,
    ) -> Result<Self, AssistantError> {
        if total < 0
            || active < 0
            || new < 0
            || qualified < 0
            || proposal < 0
            || won < 0
            || lost < 0
            || active != new + qualified + proposal
            || total != active + won + lost
        {
            return Err(AssistantError::unavailable(
                "CRM overview could not be verified.",
            ));
        }
        Ok(Self {
            total,
            active,
            new,
            qualified,
            proposal,
            won,
            lost,
        })
    }
}

/// `followupQueueItemSchema` — the open worklist's page item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FollowupQueueItem {
    pub id: String,
    pub contact_id: String,
    pub full_name: String,
    pub summary: String,
    pub due_at: String,
}

/// `followupQueueSchema` — one page of the bucket, the query's `asOf` echoed,
/// and a cursor only when a further row existed after the page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FollowupQueue {
    pub as_of: String,
    pub items: Vec<FollowupQueueItem>,
    pub next: Option<Cursor>,
}

/// `followupSchema`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FollowUp {
    pub id: String,
    pub tenant_id: String,
    pub contact_id: String,
    pub summary: String,
    pub due_at: String,
    pub status: FollowUpStatus,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FollowUpStatus {
    Open,
    Completed,
}

/// `followupDetailSchema` — the follow-up plus the contact reference it
/// hangs from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FollowupDetail {
    pub followup: FollowUp,
    pub contact: ContactRef,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ContactRef {
    pub id: String,
    pub full_name: String,
    pub revision: i64,
}

/// What `crm.contact.summary` returns for one contact — the record plus its
/// visible activities and follow-ups (each listed at most 20 by the repo).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SummaryContact {
    pub id: String,
    pub full_name: String,
    pub revision: i64,
    pub activities: Vec<ActivityView>,
    pub followups: Vec<FollowupView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActivityKind {
    Note,
    Call,
    Meeting,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ActivityView {
    pub kind: ActivityKind,
    pub summary: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FollowupView {
    pub id: String,
    pub summary: String,
    pub due_at: String,
    pub status: FollowUpStatus,
}

/// The prepared follow-up draft — the envelope `crm.followup.prepare` returns.
/// It is a REQUEST ENVELOPE, not a stored row: `status` is always `draft` and
/// it carries the contact revision the eventual command must pin against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PreparedDraft {
    pub contact_id: String,
    pub summary: String,
    pub due_at: String,
    pub tenant_id: String,
    pub contact_revision: i64,
    /// Always `"draft"` in this slice — the only value the baseline admits.
    pub status: DraftStatus,
    /// Always `true`: a draft leaves the conversation for a separate
    /// confirmation, it does not become a task here.
    #[serde(rename = "requiresApproval")]
    pub requires_approval: bool,
}

/// The single-value status vocabulary a prepared draft carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DraftStatus {
    Draft,
}

/// One stage transition in an opportunity's recorded history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Transition {
    pub from: Option<PipelineStage>,
    pub to: PipelineStage,
    pub reason: Option<String>,
    pub revision: i64,
    pub occurred_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OpportunityDetail {
    pub id: String,
    pub title: String,
    pub stage: PipelineStage,
    pub revision: i64,
    pub transitions: Vec<Transition>,
}

/// `conversationDataSchema` — the discriminated union the flow resolves to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[allow(clippy::large_enum_variant)]
pub enum ConversationData {
    #[serde(rename = "overview")]
    Overview { snapshot: Overview },
    #[serde(rename = "completion_draft")]
    CompletionDraft {
        task: FollowupDetail,
        #[serde(rename = "requiresApproval")]
        requires_approval: bool,
    },
    #[serde(rename = "needs_followup")]
    NeedsFollowup,
    #[serde(rename = "followups")]
    Followups {
        bucket: Bucket,
        #[serde(rename = "asOf")]
        as_of: String,
        items: Vec<FollowupQueueItem>,
        next: Option<Cursor>,
    },
    #[serde(rename = "help")]
    Help,
    #[serde(rename = "needs_contact")]
    NeedsContact,
    #[serde(rename = "needs_draft")]
    NeedsDraft,
    #[serde(rename = "needs_opportunity")]
    NeedsOpportunity,
    #[serde(rename = "opportunity_not_found")]
    OpportunityNotFound,
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "contacts")]
    Contacts {
        items: Vec<Item>,
        next: Option<String>,
    },
    #[serde(rename = "opportunities")]
    Opportunities {
        items: Vec<OpportunityItem>,
        next: Option<String>,
    },
    #[serde(rename = "choose_opportunity")]
    ChooseOpportunity {
        items: Vec<OpportunityItem>,
        next: Option<String>,
    },
    #[serde(rename = "opportunity")]
    Opportunity { opportunity: OpportunityDetail },
    #[serde(rename = "choose")]
    Choose {
        items: Vec<Item>,
        next: Option<String>,
    },
    #[serde(rename = "summary")]
    Summary { contact: SummaryContact },
    #[serde(rename = "draft")]
    Draft {
        #[serde(rename = "fullName")]
        full_name: String,
        draft: PreparedDraft,
    },
}

impl ConversationData {
    /// The kinds the baseline counts as `source: 'crm'` — a real answer served
    /// from the CRM — versus everything else (`help`, the `needs_*` prompts),
    /// which is answered but carries no CRM data.
    pub fn served_from_crm(&self) -> bool {
        matches!(
            self,
            ConversationData::Overview { .. }
                | ConversationData::CompletionDraft { .. }
                | ConversationData::Followups { .. }
                | ConversationData::NotFound
                | ConversationData::Contacts { .. }
                | ConversationData::Opportunities { .. }
                | ConversationData::ChooseOpportunity { .. }
                | ConversationData::Opportunity { .. }
                | ConversationData::OpportunityNotFound
                | ConversationData::Choose { .. }
                | ConversationData::Summary { .. }
                | ConversationData::Draft { .. }
        )
    }

    /// The data schema's own refusals (`conversationDataSchema`): the bounded
    /// collection ceiling (`max(20)`) applies to ARRAYS only, and a prepared
    /// completion must be an OPEN task with `requiresApproval: true`. A tool
    /// that handed back a longer page, a completed task or an unapproved
    /// envelope could not have produced a verifiable reply.
    pub fn verified(self) -> Result<Self, AssistantError> {
        let over = |n: usize| n > 20;
        let refused = match &self {
            ConversationData::Overview { snapshot } => {
                snapshot.contacts < 0
                    || snapshot.tenant_id.is_empty()
                    || snapshot.tenant_id.chars().count() > 100
            }
            ConversationData::Followups { items, .. } => over(items.len()),
            ConversationData::Contacts { items, .. } | ConversationData::Choose { items, .. } => {
                over(items.len())
            }
            ConversationData::Opportunities { items, .. }
            | ConversationData::ChooseOpportunity { items, .. } => over(items.len()),
            ConversationData::Summary { contact } => {
                over(contact.activities.len()) || over(contact.followups.len())
            }
            ConversationData::Opportunity { opportunity } => over(opportunity.transitions.len()),
            // `followupDetailSchema.refine(status === 'open')` plus the
            // literal `requiresApproval: true` — NOT a length ceiling: a
            // contact's name may legitimately exceed twenty characters, and
            // refusing the reply over it made every completion for such a
            // contact an unexplained 503.
            ConversationData::CompletionDraft {
                task,
                requires_approval,
            } => task.followup.status != FollowUpStatus::Open || !*requires_approval,
            ConversationData::Draft { .. } => false,
            _ => false,
        };
        if refused {
            return Err(AssistantError::unavailable(
                "Conversation reply could not be verified.",
            ));
        }
        Ok(self)
    }
}

/// The reply the turn serves. `source` says whether the content came from the
/// CRM (`crm`) or is a prompt/meta answer (`none`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConversationReply {
    pub text: String,
    pub source: Source,
    pub data: Option<ConversationData>,
    pub planner: PlannerIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    #[serde(rename = "crm")]
    Crm,
    #[serde(rename = "none")]
    Absent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PlannerIdentity {
    pub provider_id: String,
    pub model: String,
}

// --- The host seam -----------------------------------------------------------

/// How one turn sees the caller's grants. The flow rechecks it once at entry
/// (`contacts.read` decides whether the CRM may be read at all), and the
/// narrower gates (`pipeline.read`, `followups.write`) are re-read from this
/// view per operation — the same recheck-every-turn rule the baseline writes
/// above its `access()` call.
#[derive(Debug, Clone)]
pub struct AccessView {
    pub scope: Scope,
    pub owner_id: String,
    pub permissions: Vec<String>,
}

impl AccessView {
    pub fn allows(&self, permission: &str) -> bool {
        self.permissions.iter().any(|held| held == permission)
    }
}

/// A contact page as `crm.contacts.list` returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct ContactPage {
    pub items: Vec<Item>,
    pub next: Option<String>,
}

/// An opportunity page as `crm.opportunities.list` returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct OpportunityPage {
    pub items: Vec<OpportunityItem>,
    pub next: Option<String>,
}

/// What `crm.followup.complete.prepare` returns — the open task and the
/// separate-confirmation flag.
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionPrepare {
    pub task: FollowupDetail,
}

/// The host seam: the database-backed tools the conversation flow speaks to,
/// in the same names the baseline's `createCrmTools`/`createPipelineTools`
/// register. Each method opens ITS OWN transaction and re-authorizes inside
/// it, exactly like the baseline's `repository.transaction(tenant, principal,
/// permission, ...)`; none of them trusts values the flow derived from a
/// plan.
///
/// Refusals surface as [`AssistantError`] — `Forbidden` when the caller's
/// grants do not cover the tool, `NotFound` when a visible-scope record is
/// missing, `Conflict` for lifecycle races (an already-completed follow-up),
/// `Unavailable` for infrastructure conditions.
///
/// `async fn` in a public trait normally warns because automatic `Send`
/// bounds cannot be stated; this trait is deliberately used only inside this
/// workspace — the host implements it, and the flow is awaited on the serving
/// runtime that owns the host. Any future `tokio::spawn` of a flow (rather
/// than of plain request tasks) must re-open this decision, not silence it.
#[allow(async_fn_in_trait)]
pub trait HostCrm {
    async fn authorize(&self) -> Result<AccessView, AssistantError>;
    async fn overview(&self) -> Result<Overview, AssistantError>;
    async fn contacts_list(
        &self,
        search: String,
        after: Option<String>,
    ) -> Result<ContactPage, AssistantError>;
    async fn followups_list(
        &self,
        bucket: Bucket,
        as_of: String,
        after: Option<Cursor>,
    ) -> Result<FollowupQueue, AssistantError>;
    async fn complete_prepare(&self, id: String) -> Result<CompletionPrepare, AssistantError>;
    async fn contact_summary(&self, id: String) -> Result<SummaryContact, AssistantError>;
    async fn followup_prepare(
        &self,
        contact_id: String,
        summary: String,
        due_at: String,
    ) -> Result<PreparedDraft, AssistantError>;
    async fn opportunities_list(
        &self,
        search: String,
        after: Option<String>,
        stage: Option<PipelineStage>,
    ) -> Result<OpportunityPage, AssistantError>;
    async fn opportunity_summary(&self, id: String) -> Result<OpportunityDetail, AssistantError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(name: &str, status: FollowUpStatus) -> FollowupDetail {
        FollowupDetail {
            followup: FollowUp {
                id: "11111111-1111-4111-8111-111111111111".to_string(),
                tenant_id: "acme".to_string(),
                contact_id: "22222222-2222-4222-8222-222222222222".to_string(),
                summary: "Send the catalog".to_string(),
                due_at: "2027-01-01T00:00:00Z".to_string(),
                status,
                revision: 1,
                created_at: "2026-10-20T00:00:00Z".to_string(),
                updated_at: "2026-10-20T00:00:00Z".to_string(),
            },
            contact: ContactRef {
                id: "22222222-2222-4222-8222-222222222222".to_string(),
                full_name: name.to_string(),
                revision: 2,
            },
        }
    }

    /// The regression the assistant desk exposed: the `max(20)` ceiling
    /// belongs to ARRAYS, not to a contact's name. A completion for a
    /// 21-character name was refused as an unverifiable reply (503).
    #[test]
    fn a_long_contact_name_does_not_refuse_a_prepared_completion() {
        let data = ConversationData::CompletionDraft {
            task: task("Ada Lovelace (edited)", FollowUpStatus::Open),
            requires_approval: true,
        };
        assert!(
            data.verified().is_ok(),
            "a 21-character name is a valid contact name, not a page overflow"
        );
    }

    /// The schema's real completion conditions: an open task and a literal
    /// `requiresApproval: true`.
    #[test]
    fn a_completed_task_or_an_unapproved_envelope_refuses() {
        let completed = ConversationData::CompletionDraft {
            task: task("Ada Lovelace", FollowUpStatus::Completed),
            requires_approval: true,
        };
        assert!(completed.verified().is_err());
        let unapproved = ConversationData::CompletionDraft {
            task: task("Ada Lovelace", FollowUpStatus::Open),
            requires_approval: false,
        };
        assert!(unapproved.verified().is_err());
    }

    /// A draft's summary is a string bounded by its own schema (`max(2000)`),
    /// not a page: a 200-character summary is a normal draft.
    #[test]
    fn a_long_draft_summary_is_not_a_page_overflow() {
        let data = ConversationData::Draft {
            full_name: "Ada Lovelace".to_string(),
            draft: PreparedDraft {
                contact_id: "22222222-2222-4222-8222-222222222222".to_string(),
                summary: "x".repeat(200),
                due_at: "2027-01-01T00:00:00Z".to_string(),
                tenant_id: "acme".to_string(),
                contact_revision: 2,
                status: DraftStatus::Draft,
                requires_approval: true,
            },
        };
        assert!(data.verified().is_ok());
    }

    /// The ceiling still refuses a page the tool could not have produced.
    #[test]
    fn a_page_over_twenty_items_still_refuses() {
        let items = (0..21)
            .map(|n| Item {
                id: format!("item-{n}"),
                full_name: "Ada Lovelace".to_string(),
                revision: 1,
            })
            .collect();
        let data = ConversationData::Contacts { items, next: None };
        assert!(data.verified().is_err());
    }
}
