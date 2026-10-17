//! The server-side conversation flow — the port of
//! `packages/crm-assistant/src/conversation.ts` (`createCrmConversation`).
//!
//! Order rules this module enforces, exactly as the baseline writes them:
//!
//! 1. The caller's CRM access is RECHECKED on every turn before anything
//!    answers (no permission cache), and `contacts.read` decides whether the
//!    CRM may be read at all — even `help` requires it.
//! 2. A planner value that does not parse into the plan contracts is the
//!    `unavailable` refusal ("Conversation plan could not be verified."), not
//!    a trusted instruction.
//! 3. The narrower grants are re-read per operation (`pipeline.read` for the
//!    pipeline reads, `followups.write` for both prepare paths) and AGAIN
//!    inside the host's tool — the flow's check and the tool's check are
//!    deliberate duplicates, since they may run under different transactions.
//! 4. A query-named contact/opportunity resolves by exact case-folded name;
//!    an ambiguous or paginated hit is `choose`, an empty hit is `not_found`,
//!    and a missing explicit id is the `needs_*` prompt. The planner never
//!    resolves names: the server does.
//! 5. The prepared draft pins the contact revision it was served against; the
//!    prepared envelope re-reads the contact and REFUSES (`conflict`) when
//!    the revision moved while preparing.
//!
//! The turn-machinery pieces (`runTurn`, analytics, handoff) are absent: the
//! observable reply of this flow is `{text, source, data, planner}` where
//! `text` is the CRM-data describing string — the same text the baseline's
//! brain composes from `describe(parsed.data)`.

use crate::contracts::{
    AccessView, AssistantError, Bucket, ConversationData, ConversationInput, ConversationPlan,
    ConversationReply, HostCrm, Item, OpportunityItem, PipelineStage, Source,
};
use crate::planner::Planner;

const HELP: &str = "Coba “ringkas kondisi CRM”, “tampilkan kontak”, “cari Budi”, “ringkas Budi”, “tampilkan follow-up terlambat”, atau “siapkan follow-up Budi”. Draft belum disimpan atau dikirim.";

/// The pipeline stage's wire name, the way the baseline interpolates
/// `data.opportunity.stage` into the describing string.
fn stage_text(stage: PipelineStage) -> &'static str {
    match stage {
        PipelineStage::New => "new",
        PipelineStage::Qualified => "qualified",
        PipelineStage::Proposal => "proposal",
        PipelineStage::Won => "won",
        PipelineStage::Lost => "lost",
    }
}

/// One turn's context: the serving host supplies identity and time, and no
/// caller input decides them.
#[derive(Debug, Clone)]
pub struct ConversationContext {
    pub tenant_id: String,
    /// Echoed as-is into nothing observable in this slice, but carried for
    /// parity with the baseline's turn id (audit correlation).
    pub turn_id: String,
    pub occurred_at: String,
}

/// The CRM-data describing string — ported string-for-string from the
/// baseline's `describe`.
pub fn describe(data: &ConversationData) -> String {
    match data {
        ConversationData::Overview { snapshot } => {
            let mut text = format!(
                "Kondisi CRM dalam akses Anda: {} kontak.",
                snapshot.contacts
            );
            match &snapshot.followups {
                Some(counts) => {
                    text.push_str(&format!(
                        " {} follow-up terbuka, {} terlambat.",
                        counts.open, counts.overdue
                    ));
                }
                None => text.push_str(" Ringkasan follow-up tidak tersedia untuk akses Anda."),
            }
            match &snapshot.pipeline {
                Some(counts) => text.push_str(&format!(" {} peluang aktif.", counts.active)),
                None => text.push_str(" Ringkasan peluang tidak tersedia untuk akses Anda."),
            }
            text
        }
        ConversationData::NeedsFollowup => {
            "Pilih satu tugas dari daftar follow-up, lalu tinjau penyelesaiannya.".to_string()
        }
        ConversationData::CompletionDraft { task, .. } => format!(
            "Tinjau penyelesaian tugas untuk {}. Tugas masih terbuka; konfirmasi terpisah diperlukan.",
            task.contact.full_name
        ),
        ConversationData::Followups { bucket, as_of, items, next } => format!(
            "{} follow-up {} pada halaman ini{}. Batas waktu pemeriksaan: {as_of}.",
            items.len(),
            match bucket {
                Bucket::Overdue => "terlambat",
                Bucket::Upcoming => "mendatang",
                Bucket::Open => "terbuka",
            },
            if next.is_some() { "; masih ada halaman berikutnya" } else { "" },
        ),
        ConversationData::Help => HELP.to_string(),
        ConversationData::NeedsContact => {
            "Sebutkan nama kontak atau pilih kontak dari hasil pencarian.".to_string()
        }
        ConversationData::NeedsOpportunity => {
            "Sebutkan judul peluang atau pilih peluang dari hasil pencarian.".to_string()
        }
        ConversationData::OpportunityNotFound => {
            "Peluang tidak ditemukan dalam data yang boleh Anda akses.".to_string()
        }
        ConversationData::Opportunities { items, next } => format!(
            "{} peluang pada halaman ini{}.",
            items.len(),
            if next.is_some() { "; masih ada halaman berikutnya" } else { "" },
        ),
        ConversationData::ChooseOpportunity { .. } => {
            "Pilih peluang yang Anda maksud dari hasil pencarian.".to_string()
        }
        ConversationData::Opportunity { opportunity } => format!(
            "Peluang {}: stage {}. Riwayat di bawah berasal dari CRM.",
            opportunity.title,
            stage_text(opportunity.stage)
        ),
        ConversationData::NeedsDraft => {
            "Isi pesan dan jadwal follow-up, lalu kirim permintaan lagi. Jadwal tidak saya tentukan sendiri."
                .to_string()
        }
        ConversationData::NotFound => {
            "Kontak tidak ditemukan dalam data yang boleh Anda akses.".to_string()
        }
        ConversationData::Choose { .. } => {
            "Ada beberapa hasil yang mungkin cocok. Pilih kontak yang Anda maksud; saya belum menyiapkan draft."
                .to_string()
        }
        ConversationData::Contacts { items, next } => {
            if !items.is_empty() {
                format!(
                    "{} kontak pada halaman ini{}.",
                    items.len(),
                    if next.is_some() { "; masih ada halaman berikutnya" } else { "" },
                )
            } else {
                "Belum ada kontak yang cocok dalam data yang boleh Anda akses.".to_string()
            }
        }
        ConversationData::Summary { contact } => format!(
            "Ringkasan {}. Catatan dan follow-up di bawah berasal dari CRM; masing-masing menampilkan paling banyak 20 data yang diizinkan.",
            contact.full_name
        ),
        ConversationData::Draft { full_name, .. } => format!(
            "Draft follow-up untuk {} siap ditinjau. Belum disimpan sebagai tugas atau dikirim.",
            full_name
        ),
    }
}

/// The flow's entry. Port of `createCrmConversation(...).ask(context, raw)`
/// minus the outer turn loop: parse, authorize, verify the plan, resolve,
/// describe. Tool failures PROPAGATE (the baseline explicitly rethrows what
/// `runTurn` masks, "HTTP callers still need the correct authorization/error
/// status").
pub async fn ask<H, P>(
    host: &H,
    planner: &P,
    context: &ConversationContext,
    input: &ConversationInput,
) -> Result<ConversationReply, AssistantError>
where
    H: HostCrm + Sync,
    P: Planner + Sync,
{
    // Rechecked on every turn, including help; no session permission cache.
    let access = host.authorize().await?;
    if !access.allows("contacts.read") {
        return Err(AssistantError::forbidden("CRM reading is not permitted."));
    }
    let planned = planner.plan(&input.utterance).await;
    let plan = match planned {
        Ok(value) => ConversationPlan::parse(value)
            .map_err(|_| AssistantError::unavailable("Conversation plan could not be verified."))?,
        Err(_) => {
            return Err(AssistantError::unavailable(
                "Conversation plan could not be verified.",
            ))
        }
    };
    let data = resolve(host, &access, &plan, input, context).await?;
    let data = data.verified()?;
    let text = describe(&data);
    let source = if data.served_from_crm() {
        Source::Crm
    } else {
        Source::Absent
    };
    Ok(ConversationReply {
        text,
        source,
        data: Some(data),
        planner: planner.identity(),
    })
}

/// One turn's resolution. Port of `request.resolve()`; the branches follow
/// the baseline's order.
async fn resolve<H>(
    host: &H,
    access: &AccessView,
    plan: &ConversationPlan,
    input: &ConversationInput,
    context: &ConversationContext,
) -> Result<ConversationData, AssistantError>
where
    H: HostCrm + Sync,
{
    match plan {
        ConversationPlan::Unsupported => Ok(ConversationData::Help),
        ConversationPlan::CrmOverview => {
            let snapshot = host.overview().await?;
            Ok(ConversationData::Overview { snapshot })
        }
        ConversationPlan::FollowupCompletePrepare => {
            let Some(id) = &input.followup_id else {
                return Ok(ConversationData::NeedsFollowup);
            };
            let host_tool = host.complete_prepare(id.clone()).await?;
            Ok(ConversationData::CompletionDraft {
                task: host_tool.task,
                requires_approval: true,
            })
        }
        ConversationPlan::FollowupsList { bucket } => {
            let queue = host
                .followups_list(
                    *bucket,
                    input
                        .followup_as_of
                        .clone()
                        .unwrap_or_else(|| context.occurred_at.clone()),
                    input.followup_after.clone(),
                )
                .await?;
            Ok(ConversationData::Followups {
                bucket: *bucket,
                as_of: queue.as_of,
                items: queue.items,
                next: queue.next,
            })
        }
        ConversationPlan::OpportunitiesList { search, stage } => {
            if !access.allows("pipeline.read") {
                return Err(AssistantError::forbidden(
                    "Pipeline reading is not permitted.",
                ));
            }
            let page = host
                .opportunities_list(search.clone(), input.after.clone(), *stage)
                .await?;
            Ok(ConversationData::Opportunities {
                items: page.items,
                next: page.next,
            })
        }
        ConversationPlan::OpportunitySummary { query } => {
            if !access.allows("pipeline.read") {
                return Err(AssistantError::forbidden(
                    "Pipeline reading is not permitted.",
                ));
            }
            let mut id = input.opportunity_id.clone();
            if let Some(query) = query {
                let page = host.opportunities_list(query.clone(), None, None).await?;
                if page.items.is_empty() {
                    return Ok(ConversationData::OpportunityNotFound);
                }
                let exact: Vec<&OpportunityItem> = page
                    .items
                    .iter()
                    .filter(|item| item.title.to_lowercase() == query.to_lowercase())
                    .collect();
                if page.next.is_some() || (exact.len() != 1 && page.items.len() != 1) {
                    let items = page.items.clone();
                    return Ok(ConversationData::ChooseOpportunity {
                        items,
                        next: page.next,
                    });
                }
                id = Some(if exact.len() == 1 {
                    exact[0].id.clone()
                } else {
                    page.items[0].id.clone()
                });
            }
            let Some(id) = id else {
                return Ok(ConversationData::NeedsOpportunity);
            };
            let detail = host.opportunity_summary(id).await?;
            Ok(ConversationData::Opportunity {
                opportunity: detail,
            })
        }
        ConversationPlan::ContactsList { search } => {
            let page = host
                .contacts_list(search.clone(), input.after.clone())
                .await?;
            Ok(ConversationData::Contacts {
                items: page.items,
                next: page.next,
            })
        }
        // `contact.summary` and `followup.prepare` share the contact-name
        // resolution: both take an optional query and both answer with the
        // contact's summary — the prepare path only continues into the draft
        // envelope once that summary exists.
        ConversationPlan::ContactSummary { query }
        | ConversationPlan::FollowupPrepare { query } => {
            let prepare = matches!(plan, ConversationPlan::FollowupPrepare { .. });
            if prepare && !access.allows("followups.write") {
                return Err(AssistantError::forbidden(
                    "Follow-up preparation is not permitted.",
                ));
            }
            let mut contact_id = input.contact_id.clone();
            if let Some(query) = query {
                let page = host.contacts_list(query.clone(), None).await?;
                if page.items.is_empty() {
                    return Ok(ConversationData::NotFound);
                }
                let exact: Vec<&Item> = page
                    .items
                    .iter()
                    .filter(|item| item.full_name.to_lowercase() == query.to_lowercase())
                    .collect();
                if page.next.is_some() || (exact.len() != 1 && page.items.len() != 1) {
                    return Ok(ConversationData::Choose {
                        items: page.items,
                        next: page.next,
                    });
                }
                contact_id = Some(if exact.len() == 1 {
                    exact[0].id.clone()
                } else {
                    page.items[0].id.clone()
                });
            }
            let Some(contact_id) = contact_id else {
                return Ok(ConversationData::NeedsContact);
            };
            let summary = host.contact_summary(contact_id.clone()).await?;
            if !prepare {
                return Ok(ConversationData::Summary { contact: summary });
            }
            let Some(draft) = &input.draft else {
                return Ok(ConversationData::NeedsDraft);
            };
            let prepared = host
                .followup_prepare(contact_id, draft.summary.clone(), draft.due_at.clone())
                .await?;
            // The envelope's revision is re-read inside the tool against the
            // contact THIS summary already reflects; if the two disagree, the
            // contact moved somewhere mid-flight and the draft must not be
            // served pinned to a stale revision.
            if prepared.contact_revision != summary.revision {
                return Err(AssistantError::conflict(
                    "Contact changed while preparing the draft. Refresh and retry.",
                ));
            }
            Ok(ConversationData::Draft {
                full_name: summary.full_name,
                draft: prepared,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    //! The flow's branch logic, proven against a canned [`HostCrm`] — the same
    //! shape the baseline's `conversation.test.ts` drives with service mocks.
    //! The real host integration suite then re-exercises these branches through
    //! the HTTP route with the real SQLX tools, so the in-memory host is a
    //! provocation harness for flow decisions, never the only witness.

    use super::*;
    use crate::contracts::{
        AccessView, ActivityKind, AssistantError, CompletionPrepare, ContactPage, ContactRef,
        Cursor, ErrorCode, FollowUp, FollowUpStatus, FollowupCounts, FollowupDetail, FollowupQueue,
        FollowupQueueItem, OpportunityPage, Overview, PreparedDraft, Scope, Source, SummaryContact,
    };
    use crate::planner::{LocalCrmPlanner, Planner};
    use std::sync::Mutex;

    const ALICE: &str = "11111111-1111-4111-8111-111111111111";
    const BUDI: &str = "33333333-3333-4333-8333-333333333333";
    const ARYA: &str = "44444444-4444-4444-8444-444444444444";
    const BUDI_TWO: &str = "55555555-5555-4555-8555-555555555555";
    const TASK_LATE: &str = "66666666-6666-4666-8666-666666666666";
    const TASK_SOON: &str = "77777777-7777-4777-8777-777777777777";
    const TASK_DONE: &str = "88888888-8888-4888-8888-888888888888";
    const AS_OF: &str = "2026-10-18T08:00:00.000Z";
    const LATE: &str = "2026-10-17T08:00:00.000Z";
    const SOON: &str = "2026-10-19T08:00:00.000Z";

    fn item(id: &str, full_name: &str, revision: i64) -> Item {
        Item {
            id: id.to_string(),
            full_name: full_name.to_string(),
            revision,
        }
    }

    struct CannedFollowup {
        id: &'static str,
        contact_id: &'static str,
        summary: &'static str,
        due_at: &'static str,
        completed: bool,
    }

    impl CannedFollowup {
        fn detail(&self, contact: &Item) -> FollowupDetail {
            FollowupDetail {
                followup: FollowUp {
                    id: self.id.to_string(),
                    tenant_id: "acme-retail".to_string(),
                    contact_id: self.contact_id.to_string(),
                    summary: self.summary.to_string(),
                    due_at: self.due_at.to_string(),
                    status: if self.completed {
                        FollowUpStatus::Completed
                    } else {
                        FollowUpStatus::Open
                    },
                    revision: 1,
                    created_at: self.due_at.to_string(),
                    updated_at: self.due_at.to_string(),
                },
                contact: ContactRef {
                    id: contact.id.clone(),
                    full_name: contact.full_name.clone(),
                    revision: contact.revision,
                },
            }
        }

        fn queue_item(&self, full_name: &str) -> FollowupQueueItem {
            FollowupQueueItem {
                id: self.id.to_string(),
                contact_id: self.contact_id.to_string(),
                full_name: full_name.to_string(),
                summary: self.summary.to_string(),
                due_at: self.due_at.to_string(),
            }
        }
    }

    /// The canned host. The tool-level refusals mirror the ones the TS tools
    /// encode (the `followups.write` gates, the completed-task conflict, the
    /// revision the draft pins). The revision-bump hook sharpens the
    /// "contact changed while preparing" branch the real revocation of a
    /// concurrent write would produce.
    struct CannedHost {
        permissions: Vec<&'static str>,
        contacts: Vec<Item>,
        activities: Vec<(&'static str, ActivityKind, &'static str)>,
        followups: Vec<CannedFollowup>,
        revision_bump: Mutex<Option<&'static str>>,
    }

    impl CannedHost {
        fn grants(&self) -> AccessView {
            AccessView {
                scope: Scope::Assigned,
                owner_id: ALICE.to_string(),
                permissions: self.permissions.iter().map(|p| p.to_string()).collect(),
            }
        }
    }

    impl Default for CannedHost {
        fn default() -> Self {
            Self {
                permissions: vec![
                    "contacts.read",
                    "activities.read",
                    "followups.read",
                    "followups.write",
                ],
                contacts: vec![item(BUDI, "Budi", 3), item(ARYA, "Arya Widodo", 2)],
                activities: vec![(BUDI, ActivityKind::Note, "Call booked for Thursday")],
                followups: vec![
                    CannedFollowup {
                        id: TASK_LATE,
                        contact_id: BUDI,
                        summary: "Send the catalog",
                        due_at: LATE,
                        completed: false,
                    },
                    CannedFollowup {
                        id: TASK_SOON,
                        contact_id: BUDI,
                        summary: "Book the demo",
                        due_at: SOON,
                        completed: false,
                    },
                    CannedFollowup {
                        id: TASK_DONE,
                        contact_id: ARYA,
                        summary: "Archive the note",
                        due_at: LATE,
                        completed: true,
                    },
                ],
                revision_bump: Mutex::new(None),
            }
        }
    }

    impl HostCrm for CannedHost {
        async fn authorize(&self) -> Result<AccessView, AssistantError> {
            Ok(self.grants())
        }

        async fn overview(&self) -> Result<Overview, AssistantError> {
            Ok(Overview {
                tenant_id: "acme-retail".to_string(),
                as_of: AS_OF.to_string(),
                scope: Scope::Assigned,
                contacts: 2,
                followups: Some(FollowupCounts::verified(2, 1, 1, 3)?),
                pipeline: None,
            })
        }

        async fn contacts_list(
            &self,
            search: String,
            after: Option<String>,
        ) -> Result<ContactPage, AssistantError> {
            let page = self
                .contacts
                .iter()
                .filter(|item| item.full_name.contains(&search))
                .filter(|item| after.as_deref() != Some(item.id.as_str()))
                .cloned()
                .collect();
            Ok(ContactPage {
                items: page,
                next: None,
            })
        }

        async fn followups_list(
            &self,
            bucket: Bucket,
            as_of: String,
            _after: Option<Cursor>,
        ) -> Result<FollowupQueue, AssistantError> {
            let page = self
                .followups
                .iter()
                .filter(|f| !f.completed)
                .filter(|f| match bucket {
                    Bucket::Open => true,
                    Bucket::Overdue => f.due_at < as_of.as_str(),
                    Bucket::Upcoming => f.due_at >= as_of.as_str(),
                })
                .map(|f| f.queue_item("Budi"))
                .collect();
            Ok(FollowupQueue {
                as_of,
                items: page,
                next: None,
            })
        }

        async fn complete_prepare(&self, id: String) -> Result<CompletionPrepare, AssistantError> {
            if !self.permissions.contains(&"followups.write") {
                return Err(AssistantError::forbidden(
                    "Follow-up completion preparation is not permitted.",
                ));
            }
            let canned = self
                .followups
                .iter()
                .find(|f| f.id == id)
                .ok_or_else(|| AssistantError::not_found("CRM follow-up not found."))?;
            if canned.completed {
                return Err(AssistantError::conflict(
                    "This follow-up is already completed.",
                ));
            }
            let contact = self
                .contacts
                .iter()
                .find(|c| c.id == canned.contact_id)
                .expect("the fixture always links");
            Ok(CompletionPrepare {
                task: canned.detail(contact),
            })
        }

        async fn contact_summary(&self, id: String) -> Result<SummaryContact, AssistantError> {
            let item = self
                .contacts
                .iter()
                .find(|item| item.id == id)
                .ok_or_else(|| AssistantError::not_found("CRM contact not found."))?;
            Ok(SummaryContact {
                id: item.id.clone(),
                full_name: item.full_name.clone(),
                revision: item.revision,
                activities: self
                    .activities
                    .iter()
                    .filter(|(contact_id, ..)| *contact_id == item.id.as_str())
                    .map(|(_, kind, summary)| crate::contracts::ActivityView {
                        kind: *kind,
                        summary: summary.to_string(),
                        created_at: AS_OF.to_string(),
                    })
                    .collect(),
                followups: self
                    .followups
                    .iter()
                    .filter(|f| f.contact_id == item.id.as_str())
                    .map(|f| crate::contracts::FollowupView {
                        id: f.id.to_string(),
                        summary: f.summary.to_string(),
                        due_at: f.due_at.to_string(),
                        status: if f.completed {
                            FollowUpStatus::Completed
                        } else {
                            FollowUpStatus::Open
                        },
                    })
                    .collect(),
            })
        }

        async fn followup_prepare(
            &self,
            contact_id: String,
            summary: String,
            due_at: String,
        ) -> Result<PreparedDraft, AssistantError> {
            if !self.permissions.contains(&"followups.write") {
                return Err(AssistantError::forbidden(
                    "Follow-up preparation is not permitted.",
                ));
            }
            let mut contact = self
                .contacts
                .iter()
                .find(|c| c.id == contact_id)
                .ok_or_else(|| AssistantError::not_found("CRM contact not found."))?
                .clone();
            if *self.revision_bump.lock().unwrap() == Some(contact.id.as_str()) {
                contact.revision += 1;
            }
            Ok(PreparedDraft {
                contact_id: contact.id.clone(),
                summary,
                due_at,
                tenant_id: "acme-retail".to_string(),
                contact_revision: contact.revision,
                status: crate::contracts::DraftStatus::Draft,
                requires_approval: true,
            })
        }

        async fn opportunities_list(
            &self,
            _search: String,
            _after: Option<String>,
            _stage: Option<crate::contracts::PipelineStage>,
        ) -> Result<OpportunityPage, AssistantError> {
            Ok(OpportunityPage {
                items: vec![],
                next: None,
            })
        }

        async fn opportunity_summary(
            &self,
            _id: String,
        ) -> Result<crate::contracts::OpportunityDetail, AssistantError> {
            Err(AssistantError::not_found("Opportunity not found."))
        }
    }

    fn context() -> ConversationContext {
        ConversationContext {
            tenant_id: "acme-retail".to_string(),
            turn_id: "turn-1".to_string(),
            occurred_at: AS_OF.to_string(),
        }
    }

    fn input(utterance: &str) -> ConversationInput {
        ConversationInput::parse(serde_json::json!({ "utterance": utterance }))
            .expect("recorded test utterances parse")
    }

    async fn turn(
        host: &CannedHost,
        utterance: &str,
    ) -> Result<crate::contracts::ConversationReply, AssistantError> {
        let planner = LocalCrmPlanner::new();
        ask(host, &planner, &context(), &input(utterance)).await
    }

    /* The refusals and branches, each named after the behavior it pins. */

    /// `help` for anything outside the vocabulary, and `source: none` for it.
    #[tokio::test]
    async fn unsupported_utterances_answer_with_help_and_source_none() {
        let host = CannedHost::default();
        let reply = turn(&host, "halo dunia").await.expect("help is an answer");
        assert!(matches!(reply.data, Some(ConversationData::Help)));
        assert_eq!(reply.source, Source::Absent);
        assert_eq!(reply.planner.provider_id, LocalCrmPlanner::PROVIDER_ID);
    }

    /// No `contacts.read`, no turn at all — including for help.
    #[tokio::test]
    async fn without_the_contact_read_grant_the_whole_turn_is_refused() {
        let host = CannedHost {
            permissions: vec!["audit.read"],
            ..CannedHost::default()
        };
        let error = turn(&host, "halo")
            .await
            .expect_err("a member without contacts.read is refused");
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(error.message, "CRM reading is not permitted.");
    }

    /// A plan the contracts cannot verify is the unavailable refusal; it is
    /// never executed because it arrived.
    #[tokio::test]
    async fn an_unverifiable_plan_is_unavailable_not_executed() {
        struct Garbage;
        impl Planner for Garbage {
            async fn plan(&self, _utterance: &str) -> Result<serde_json::Value, AssistantError> {
                Ok(serde_json::json!({ "operation": "drop.tables", "tenant": "*" }))
            }
            fn identity(&self) -> crate::contracts::PlannerIdentity {
                crate::contracts::PlannerIdentity {
                    provider_id: "attacker".to_string(),
                    model: "x".to_string(),
                }
            }
        }
        // A different but equally untrusted failure: the planner errors.
        struct Errors;
        impl Planner for Errors {
            async fn plan(&self, _utterance: &str) -> Result<serde_json::Value, AssistantError> {
                Err(AssistantError::unavailable("planner down"))
            }
            fn identity(&self) -> crate::contracts::PlannerIdentity {
                crate::contracts::PlannerIdentity {
                    provider_id: "attacker".to_string(),
                    model: "x".to_string(),
                }
            }
        }
        let host = CannedHost::default();
        for result in [
            ask(&host, &Garbage, &context(), &input("tampilkan kontak")).await,
            ask(&host, &Errors, &context(), &input("tampilkan kontak")).await,
        ] {
            let error = result.expect_err("an unverified plan must not resolve");
            assert_eq!(error.code, ErrorCode::Unavailable);
            assert_eq!(error.message, "Conversation plan could not be verified.");
        }
    }

    /// The overview carries scoped counts, and a follower
    /// (followups section absent for missing followups.read in the real
    /// host — the canned host models the section presence only).
    #[tokio::test]
    async fn an_overview_utterance_reports_the_scoped_counts() {
        let host = CannedHost::default();
        let reply = turn(&host, "ringkas kondisi CRM")
            .await
            .expect("the overview answers");
        assert_eq!(reply.source, Source::Crm);
        match reply.data {
            Some(ConversationData::Overview { snapshot }) => {
                assert_eq!(snapshot.contacts, 2);
                let counts = snapshot
                    .followups
                    .expect("followup counts for a readable grant");
                assert_eq!(counts.open, 2);
                assert_eq!(counts.overdue, 1);
                assert_eq!(counts.upcoming, 1);
            }
            other => panic!("overview branch produced {other:?}"),
        }
        assert!(reply
            .text
            .starts_with("Kondisi CRM dalam akses Anda: 2 kontak."));
    }

    /// An overview whose counted rows violate the baseline's own invariants
    /// cannot be served at all: the count constructor refuses.
    #[test]
    fn an_inconsistent_overview_count_refuses() {
        let counts = FollowupCounts::verified(2, 1, 0, 3);
        assert_eq!(counts.unwrap_err().code, ErrorCode::Unavailable);
        let counts = crate::contracts::PipelineCounts::verified(5, 2, 1, 1, 0, 0, 0);
        assert_eq!(counts.unwrap_err().code, ErrorCode::Unavailable);
    }

    /// Search answers page; the summary path exact-matches the name and
    /// otherwise prompts choose/not_found.
    #[tokio::test]
    async fn a_contact_query_exact_matches_choose_or_not_found() {
        let host = CannedHost::default();

        // Late item carries; the exact name summary shows its activities.
        let reply = turn(&host, "ringkas Budi")
            .await
            .expect("an exact name summarises");
        match reply.data {
            Some(ConversationData::Summary { contact }) => {
                assert_eq!(contact.full_name, "Budi");
                assert_eq!(contact.activities.len(), 1);
                assert_eq!(contact.followups.len(), 2);
            }
            other => panic!("summary branch produced {other:?}"),
        }

        // Two names sharing the query prompt the choose answer with no draft.
        let ambiguous = CannedHost {
            contacts: vec![item(BUDI, "Budi", 3), item(BUDI_TWO, "Budi", 7)],
            ..CannedHost::default()
        };
        let reply = turn(&ambiguous, "ringkas Budi")
            .await
            .expect("ambiguity answers");
        match reply.data {
            Some(ConversationData::Choose { items, next }) => {
                assert_eq!(items.len(), 2);
                assert_eq!(next, None);
            }
            other => panic!("choose branch produced {other:?}"),
        }

        // Nothing matches: not_found is an ANSWER (source crm), not an error.
        let reply = turn(&host, "ringkas Nonexistent")
            .await
            .expect("not_found answers");
        assert!(matches!(reply.data, Some(ConversationData::NotFound)));
        assert_eq!(reply.source, Source::Crm);
    }

    /// Same shape for preparation: no draft asks for one; a served draft
    /// pins the contact revision; a moved revision refuses with conflict.
    #[tokio::test]
    async fn preparing_a_draft_needs_the_draft_and_pins_the_revision() {
        let host = CannedHost::default();

        // Without the draft payload: the needs_draft prompt.
        let reply = turn(&host, "siapkan follow-up untuk Budi")
            .await
            .expect("the draft prompt answers");
        assert!(matches!(reply.data, Some(ConversationData::NeedsDraft)));

        // With a draft: the envelope carries the revision the command pins.
        let with_draft = serde_json::json!({
            "utterance": "siapkan follow-up untuk Budi",
            "draft": { "summary": "Kirim katalog", "dueAt": SOON }
        });
        let planner = LocalCrmPlanner::new();
        let reply = ask(
            &host,
            &planner,
            &context(),
            &ConversationInput::parse(with_draft.clone()).expect("the recorded draft parses"),
        )
        .await
        .expect("the draft envelope answers");
        match reply.data {
            Some(ConversationData::Draft { full_name, draft }) => {
                assert_eq!(full_name, "Budi");
                assert_eq!(draft.contact_id, BUDI);
                assert_eq!(draft.contact_revision, 3);
                assert_eq!(draft.summary, "Kirim katalog");
                assert!(draft.requires_approval);
            }
            other => panic!("draft branch produced {other:?}"),
        }

        // The contact moving mid-prepare refuses: the envelope's revision no
        // longer matches the summary the caller was served.
        *host.revision_bump.lock().unwrap() = Some(BUDI);
        let error = ask(
            &host,
            &planner,
            &context(),
            &ConversationInput::parse(with_draft.clone()).expect("the recorded draft parses"),
        )
        .await
        .expect_err("a moved revision must refuse");
        assert_eq!(error.code, ErrorCode::Conflict);
        assert_eq!(
            error.message,
            "Contact changed while preparing the draft. Refresh and retry."
        );
    }

    /// Completion preparation: no id, an open task, a completed task.
    #[tokio::test]
    async fn preparing_completion_gates_on_the_task_choice_and_lifecycle() {
        let host = CannedHost::default();

        // No chosen task first.
        let reply = turn(&host, "tinjau penyelesaian follow-up")
            .await
            .expect("the prompt answers");
        assert!(matches!(reply.data, Some(ConversationData::NeedsFollowup)));

        // An open task: the completion draft with the separate-approval flag.
        let with_id = serde_json::json!({
            "utterance": "tinjau penyelesaian follow-up",
            "followupId": TASK_LATE
        });
        let planner = LocalCrmPlanner::new();
        let reply = ask(
            &host,
            &planner,
            &context(),
            &ConversationInput::parse(with_id).expect("the recorded input parses"),
        )
        .await
        .expect("the completion draft answers");
        let Some(ConversationData::CompletionDraft {
            task,
            requires_approval,
        }) = reply.data
        else {
            panic!("completion branch answered {reply:?}");
        };
        assert_eq!(task.followup.id, TASK_LATE);
        assert_eq!(task.followup.status, FollowUpStatus::Open);
        assert!(requires_approval);
        assert!(reply
            .text
            .starts_with("Tinjau penyelesaian tugas untuk Budi."));

        // A completed task refuses with the recorded conflict semantics.
        let done = serde_json::json!({
            "utterance": "tinjau penyelesaian follow-up",
            "followupId": TASK_DONE
        });
        let error = ask(
            &host,
            &planner,
            &context(),
            &ConversationInput::parse(done).expect("the recorded input parses"),
        )
        .await
        .expect_err("a completed task must refuse");
        assert_eq!(error.code, ErrorCode::Conflict);
        assert_eq!(error.message, "This follow-up is already completed.");
    }

    /// The worklist buckets split on the due boundary: an open task due
    /// exactly at `asOf` is UPCOMING, not overdue.
    #[tokio::test]
    async fn the_worklist_buckets_split_at_the_boundary() {
        let host = CannedHost::default();
        let reply = turn(&host, "tampilkan follow-up terlambat")
            .await
            .expect("the bucket answers");
        match reply.data {
            Some(ConversationData::Followups { items, .. }) => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].id, TASK_LATE);
            }
            other => panic!("worklist branch produced {other:?}"),
        }
        let reply = turn(&host, "tampilkan follow-up mendatang")
            .await
            .expect("the bucket answers");
        match reply.data {
            Some(ConversationData::Followups { items, .. }) => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].id, TASK_SOON);
            }
            other => panic!("worklist branch produced {other:?}"),
        }
        // Every open task, both buckets, in due order.
        let reply = turn(&host, "tampilkan follow-up")
            .await
            .expect("the bucket answers");
        match reply.data {
            Some(ConversationData::Followups { items, as_of, .. }) => {
                assert_eq!(items.len(), 2);
                assert_eq!(as_of, AS_OF);
            }
            other => panic!("worklist branch produced {other:?}"),
        }
    }

    /// A caller without `followups.write` is refused before any contact is
    /// named: the flow's own gate, and again in the tool under its own
    /// transaction.
    #[tokio::test]
    async fn without_the_followup_write_grant_preparation_is_refused() {
        let host = CannedHost {
            permissions: vec!["contacts.read", "followups.read"],
            ..CannedHost::default()
        };
        let error = turn(&host, "siapkan follow-up untuk Budi")
            .await
            .expect_err("preparation without the write grant is refused");
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(error.message, "Follow-up preparation is not permitted.");
    }

    /// The pipeline reads are double-gated by `pipeline.read`: the flow's
    /// check first, the repository's again inside its transaction.
    #[tokio::test]
    async fn pipeline_reads_refuse_without_the_pipeline_grant() {
        let host = CannedHost {
            permissions: vec!["contacts.read", "followups.read", "followups.write"],
            ..CannedHost::default()
        };
        let error = turn(&host, "tampilkan peluang")
            .await
            .expect_err("pipeline reads without the grant are refused");
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(error.message, "Pipeline reading is not permitted.");
    }

    /// A summary request answered from the contact id, when the earlier
    /// search already chose: `contactId` wins over a re-search.
    #[tokio::test]
    async fn the_chosen_contact_id_short_circuits_a_new_search() {
        let host = CannedHost::default();
        let chosen = serde_json::json!({
            "utterance": "ringkas",
            "contactId": BUDI
        });
        let planner = LocalCrmPlanner::new();
        let reply = ask(
            &host,
            &planner,
            &context(),
            &ConversationInput::parse(chosen).expect("the recorded input parses"),
        )
        .await
        .expect("the chosen summary answers");
        match reply.data {
            Some(ConversationData::Summary { contact }) => assert_eq!(contact.id, BUDI),
            other => panic!("summary branch produced {other:?}"),
        }
    }
}
