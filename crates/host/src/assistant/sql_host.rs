//! The [`HostCrm`] implementation against the CRM schema, with the same
//! vocabulary `crm.rs` established for scope, grants and timestamps.

use archava_adapters::rls::TenantTransaction;
use archava_assistant::{
    AccessView, ActivityView, AssistantError, Bucket, CompletionPrepare, ContactPage, ContactRef,
    Cursor, DraftStatus, FollowUp, FollowUpStatus, FollowupCounts, FollowupDetail, FollowupQueue,
    FollowupQueueItem, FollowupView, HostCrm, Item, OpportunityDetail, OpportunityItem,
    OpportunityPage, Overview, PipelineCounts, PipelineStage, PreparedDraft, SummaryContact,
    Transition,
};
use archava_core::tenant::TenantId;
use uuid::Uuid;

use super::bridge::{
    as_uuid, bridged_access, bridged_activity_kind, bridged_followup_status, bridged_scope,
    bridged_stage, context_refusal, db_refusal, escaped_search, refusal, stage_text,
};
use crate::crm::{authorize, begin, CrmAccess, CrmActor, CrmScope, Permission};
use crate::router::AppState;

/// The [`HostCrm`] implementation against the CRM schema, with the same
/// vocabulary `crm.rs` established for scope, grants and timestamps.
pub(crate) struct SqlHost<'c> {
    pub(crate) state: &'c AppState,
    pub(crate) tenant: TenantId,
    pub(crate) principal_id: String,
}

impl SqlHost<'_> {
    fn actor(&self) -> CrmActor {
        CrmActor::Human(self.principal_id.clone())
    }

    /// One transaction with the authorize step already served — the entry the
    /// baseline opens with `repository.transaction(...)`. The transaction is
    /// OPEN on return; the caller owns rollback-on-refusal and may drop it
    /// after a read.
    async fn open_authorized(&self) -> Result<(CrmAccess, TenantTransaction<'_>), AssistantError> {
        let mut transaction = begin(self.state, self.tenant.clone())
            .await
            .map_err(refusal)?;
        let resolved = {
            let connection = transaction
                .checked_executor()
                .await
                .map_err(context_refusal)?;
            authorize(connection, &self.tenant, &self.actor()).await
        };
        match resolved {
            Ok(access) => Ok((access, transaction)),
            Err(error) => {
                let _ = transaction.rollback().await;
                Err(refusal(error))
            }
        }
    }

    /// `checked_executor`, bridged — the executor for the next query in the
    /// transaction, with the context re-read the RLS wrapper requires.
    async fn executor<'c>(
        transaction: &'c mut TenantTransaction<'_>,
    ) -> Result<&'c mut sqlx::PgConnection, AssistantError> {
        transaction
            .checked_executor()
            .await
            .map_err(context_refusal)
    }

    /// The contact detail read the baseline's `detail()` runs: the owned
    /// record plus its activity and follow-up sections, each conditioned on
    /// the caller's grants. Runs its own transaction (the baseline's tool
    /// composes two service calls, so this is called through one).
    async fn detail(
        &self,
        id: String,
    ) -> Result<(ContactRef, Vec<ActivityView>, Vec<FollowupView>), AssistantError> {
        let (access, mut transaction) = self.open_authorized().await?;
        if !access.allows(Permission::CONTACTS_READ) {
            let _ = transaction.rollback().await;
            return Err(AssistantError::forbidden("CRM permission denied."));
        }
        let contact = {
            let connection = Self::executor(&mut transaction).await?;
            let row = sqlx::query_as::<_, (Uuid, String, i32)>(
                "SELECT id, full_name, revision FROM archava_crm.contacts \
                 WHERE tenant_id=$1 AND id=$2 AND ($3::boolean OR owner_id=$4)",
            )
            .bind(self.tenant.as_str())
            .bind(as_uuid(&id, "the id")?)
            .bind(access.scope == CrmScope::All)
            .bind(&access.owner_id)
            .fetch_one(connection)
            .await;
            match row {
                Ok((id, full_name, revision)) => ContactRef {
                    id: id.to_string(),
                    full_name,
                    revision: i64::from(revision),
                },
                Err(sqlx::Error::RowNotFound) => {
                    // Invisible and missing are one answer, the containment
                    // rule `ownedContact` rides.
                    let _ = transaction.rollback().await;
                    return Err(AssistantError::not_found("CRM contact not found."));
                }
                Err(error) => return self.fail(transaction, error).await,
            }
        };
        let activities = if access.allows(Permission::ACTIVITIES_READ) {
            let rows = {
                let connection = Self::executor(&mut transaction).await?;
                sqlx::query_as::<_, (String, String, String)>(
                    "SELECT kind, summary, \
                        to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
                     FROM archava_crm.activities \
                     WHERE tenant_id=$1 AND contact_id=$2 \
                     ORDER BY created_at DESC,id DESC LIMIT 20",
                )
                .bind(self.tenant.as_str())
                .bind(as_uuid(contact.id.as_str(), "the section contact id")?)
                .fetch_all(connection)
                .await
            };
            match rows {
                Ok(rows) => {
                    let mut activities = Vec::with_capacity(rows.len());
                    for (kind, summary, created_at) in rows {
                        let kind = match bridged_activity_kind(&kind) {
                            Ok(kind) => kind,
                            Err(error) => {
                                let _ = transaction.rollback().await;
                                return Err(error);
                            }
                        };
                        activities.push(ActivityView {
                            kind,
                            summary,
                            created_at,
                        });
                    }
                    activities
                }
                Err(error) => return self.fail(transaction, error).await,
            }
        } else {
            Vec::new()
        };
        let followups = if access.allows(Permission::FOLLOWUPS_READ) {
            let rows = {
                let connection = Self::executor(&mut transaction).await?;
                sqlx::query_as::<_, (Uuid, String, String, String)>(
                    "SELECT id, summary, \
                        to_char(due_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), \
                        status \
                     FROM archava_crm.followups \
                     WHERE tenant_id=$1 AND contact_id=$2 \
                     ORDER BY due_at,id LIMIT 20",
                )
                .bind(self.tenant.as_str())
                .bind(as_uuid(contact.id.as_str(), "the section contact id")?)
                .fetch_all(connection)
                .await
            };
            match rows {
                Ok(rows) => {
                    let mut followups = Vec::with_capacity(rows.len());
                    for (id, summary, due_at, status) in rows {
                        let status = match bridged_followup_status(&status) {
                            Ok(status) => status,
                            Err(error) => {
                                let _ = transaction.rollback().await;
                                return Err(error);
                            }
                        };
                        followups.push(FollowupView {
                            id: id.to_string(),
                            summary,
                            due_at,
                            status,
                        });
                    }
                    followups
                }
                Err(error) => return self.fail(transaction, error).await,
            }
        } else {
            Vec::new()
        };
        Ok((contact, activities, followups))
    }

    /// Rollback + ledger-labelled unavailability for a mid-tool database
    /// failure; the shape the house's read paths settle refusals with.
    async fn fail<T>(
        &self,
        transaction: TenantTransaction<'_>,
        error: sqlx::Error,
    ) -> Result<T, AssistantError> {
        let _ = transaction.rollback().await;
        Err(db_refusal(error))
    }
}

impl HostCrm for SqlHost<'_> {
    /// The entry recheck — the baseline's `service.access`: one transaction,
    /// authorize, the view the flow's early gates read. No permission label:
    /// the flow itself decides what the turn may ask.
    async fn authorize(&self) -> Result<AccessView, AssistantError> {
        match self.open_authorized().await {
            Ok((access, _transaction)) => {
                // A read transaction that dropped rolled back; nothing to
                // commit on an access read.
                Ok(bridged_access(&access))
            }
            Err(error) => Err(error),
        }
    }

    async fn overview(&self) -> Result<Overview, AssistantError> {
        let (access, mut transaction) = self.open_authorized().await?;
        // The overview's transaction label is `contacts.read` in the
        // baseline; the flow's own contacts.read gate already held by the
        // time a tool runs, so this tool need not re-ask — the baseline's
        // label check is belt-and-braces behind its own flow gate, and the
        // read is scope-contained regardless.
        let can_followups = access.allows(Permission::FOLLOWUPS_READ);
        let can_pipeline = if access.allows(Permission::PIPELINE_READ) {
            let connection = Self::executor(&mut transaction).await?;
            crate::own::module_enabled(connection, &self.tenant, "crm-pipeline")
                .await
                .unwrap_or(false)
        } else {
            false
        };
        // `now()` is transaction-fixed, so the overview and its `asOf` share
        // one snapshot stamp; the counts CTE reads the same statement.
        let row = {
            let connection = Self::executor(&mut transaction).await?;
            sqlx::query_as::<_, (String,)>(
                "SELECT to_char(now() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS as_of",
            )
            .fetch_one(connection)
            .await
        };
        let as_of = match row {
            Ok((as_of,)) => as_of,
            Err(error) => return self.fail(transaction, error).await,
        };
        let counts = {
            let connection = Self::executor(&mut transaction).await?;
            sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64)>(
                "WITH visible_contacts AS (
                    SELECT id FROM archava_crm.contacts
                    WHERE tenant_id=$1 AND ($2::boolean OR owner_id=$3)
                ), tasks AS (
                    SELECT f.status, f.due_at FROM archava_crm.followups f
                    JOIN visible_contacts c ON c.id=f.contact_id
                    WHERE f.tenant_id=$1 AND $5::boolean
                ), opportunities AS (
                    SELECT o.stage FROM archava_crm.opportunities o
                    JOIN visible_contacts c ON c.id=o.contact_id
                    WHERE o.tenant_id=$1 AND $6::boolean AND ($2::boolean OR o.owner_id=$3)
                ) SELECT
                    (SELECT count(*) FROM visible_contacts) AS contacts,
                    (SELECT count(*) FROM tasks WHERE status='open') AS opened,
                    (SELECT count(*) FROM tasks WHERE status='open' AND due_at<$4::timestamptz) AS overdue,
                    (SELECT count(*) FROM tasks WHERE status='open' AND due_at>=$4::timestamptz) AS upcoming,
                    (SELECT count(*) FROM tasks WHERE status='completed') AS completed,
                    (SELECT count(*) FROM opportunities) AS total_opportunities,
                    (SELECT count(*) FROM opportunities WHERE stage='new') AS stage_new,
                    (SELECT count(*) FROM opportunities WHERE stage='qualified') AS stage_qualified,
                    (SELECT count(*) FROM opportunities WHERE stage='proposal') AS stage_proposal,
                    (SELECT count(*) FROM opportunities WHERE stage='won') AS stage_won,
                    (SELECT count(*) FROM opportunities WHERE stage='lost') AS stage_lost",
            )
            .bind(self.tenant.as_str())
            .bind(access.scope == CrmScope::All)
            .bind(&access.owner_id)
            .bind(&as_of)
            .bind(can_followups)
            .bind(can_pipeline)
            .fetch_one(connection)
            .await
        };
        let (
            contacts,
            opened,
            overdue,
            upcoming,
            completed,
            total,
            stage_new,
            stage_qualified,
            stage_proposal,
            stage_won,
            stage_lost,
        ) = match counts {
            Ok(counts) => counts,
            Err(error) => return self.fail(transaction, error).await,
        };
        let followups = if can_followups {
            match FollowupCounts::verified(opened, overdue, upcoming, completed) {
                Ok(counts) => Some(counts),
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        let pipeline = if can_pipeline {
            match PipelineCounts::verified(
                total,
                stage_new + stage_qualified + stage_proposal,
                stage_new,
                stage_qualified,
                stage_proposal,
                stage_won,
                stage_lost,
            ) {
                Ok(counts) => Some(counts),
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        Ok(Overview {
            tenant_id: self.tenant.as_str().to_string(),
            as_of,
            scope: bridged_scope(access.scope),
            contacts,
            followups,
            pipeline,
        })
    }

    async fn contacts_list(
        &self,
        search: String,
        after: Option<String>,
    ) -> Result<ContactPage, AssistantError> {
        let (access, mut transaction) = self.open_authorized().await?;
        if !access.allows(Permission::CONTACTS_READ) {
            let _ = transaction.rollback().await;
            return Err(AssistantError::forbidden("CRM permission denied."));
        }
        let after_id = match after.as_deref() {
            None => None,
            Some(cursor) => Some(as_uuid(cursor, "the page cursor")?),
        };
        let pattern = format!("%{}%", escaped_search(&search));
        let rows = {
            let connection = Self::executor(&mut transaction).await?;
            sqlx::query_as::<_, (Uuid, String, i32)>(
                "SELECT id, full_name, revision FROM archava_crm.contacts \
                 WHERE tenant_id=$1 AND ($2::boolean OR owner_id=$3) \
                   AND ($4::uuid IS NULL OR id>$4) \
                   AND full_name ILIKE $5 ESCAPE '\\' \
                 ORDER BY id LIMIT $6",
            )
            .bind(self.tenant.as_str())
            .bind(access.scope == CrmScope::All)
            .bind(&access.owner_id)
            .bind(after_id)
            .bind(&pattern)
            .bind(20i64 + 1)
            .fetch_all(connection)
            .await
        };
        let rows = match rows {
            Ok(rows) => rows,
            Err(error) => return self.fail(transaction, error).await,
        };
        let overflow = rows.len() > 20;
        let items: Vec<Item> = rows
            .into_iter()
            .take(20)
            .map(|(id, full_name, revision)| Item {
                id: id.to_string(),
                full_name,
                revision: i64::from(revision),
            })
            .collect();
        // `next` is the last emitted id only when a further row existed.
        let next = if overflow {
            items.last().map(|item| item.id.clone())
        } else {
            None
        };
        Ok(ContactPage { items, next })
    }

    async fn followups_list(
        &self,
        bucket: Bucket,
        as_of: String,
        after: Option<Cursor>,
    ) -> Result<FollowupQueue, AssistantError> {
        let (access, mut transaction) = self.open_authorized().await?;
        if !access.allows(Permission::FOLLOWUPS_READ) {
            let _ = transaction.rollback().await;
            return Err(AssistantError::forbidden("CRM permission denied."));
        }
        if !access.allows(Permission::CONTACTS_READ) {
            let _ = transaction.rollback().await;
            return Err(AssistantError::forbidden("Contact access is required."));
        }
        // The flow forwards no search: the baseline's query carries the
        // default '' and the clause stays in place for parity with the
        // recorded shape (both ILIKEs see the same empty pattern).
        let pattern = format!("%{}%", escaped_search(""));
        let after_id = match after.as_ref() {
            None => None,
            Some(cursor) => Some(as_uuid(&cursor.id, "the follow-up cursor")?),
        };
        let bucket_text = match bucket {
            Bucket::Open => "open",
            Bucket::Overdue => "overdue",
            Bucket::Upcoming => "upcoming",
        };
        let rows = {
            let connection = Self::executor(&mut transaction).await?;
            sqlx::query_as::<_, (Uuid, Uuid, String, String, String)>(
                "SELECT f.id, f.contact_id, c.full_name, f.summary, \
                    to_char(f.due_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
                 FROM archava_crm.followups f JOIN archava_crm.contacts c \
                   ON c.tenant_id=f.tenant_id AND c.id=f.contact_id \
                 WHERE f.tenant_id=$1 AND c.tenant_id=$1 AND f.status='open' \
                   AND ($2::boolean OR c.owner_id=$3) \
                   AND ($4::text='open' OR ($4='overdue' AND f.due_at<$5::timestamptz) \
                     OR ($4='upcoming' AND f.due_at>=$5::timestamptz)) \
                   AND ($6::timestamptz IS NULL OR (f.due_at,f.id)>($6::timestamptz,$7::uuid)) \
                   AND (c.full_name ILIKE $8 ESCAPE '\\' OR f.summary ILIKE $8 ESCAPE '\\') \
                 ORDER BY f.due_at,f.id LIMIT $9",
            )
            .bind(self.tenant.as_str())
            .bind(access.scope == CrmScope::All)
            .bind(&access.owner_id)
            .bind(bucket_text)
            .bind(&as_of)
            .bind(after.as_ref().map(|cursor| cursor.due_at.as_str()))
            .bind(after_id)
            .bind(&pattern)
            .bind(20i64 + 1)
            .fetch_all(connection)
            .await
        };
        let rows = match rows {
            Ok(rows) => rows,
            Err(error) => return self.fail(transaction, error).await,
        };
        let overflow = rows.len() > 20;
        let items: Vec<FollowupQueueItem> = rows
            .into_iter()
            .take(20)
            .map(
                |(id, contact_id, full_name, summary, due_at)| FollowupQueueItem {
                    id: id.to_string(),
                    contact_id: contact_id.to_string(),
                    full_name,
                    summary,
                    due_at,
                },
            )
            .collect();
        let next = if overflow {
            items.last().map(|item| Cursor {
                due_at: item.due_at.clone(),
                id: item.id.clone(),
            })
        } else {
            None
        };
        Ok(FollowupQueue { as_of, items, next })
    }

    async fn complete_prepare(&self, id: String) -> Result<CompletionPrepare, AssistantError> {
        // The tool's own write gate first — the baseline composes
        // `service.access()` (one transaction) and then `service.followup`
        // (its own transaction). Two transactions here for the same reasons:
        // the grant cannot be trusted across a wait.
        let (access, transaction) = self.open_authorized().await?;
        if !access.allows(Permission::FOLLOWUPS_WRITE) {
            let _ = transaction.rollback().await;
            return Err(AssistantError::forbidden(
                "Follow-up completion preparation is not permitted.",
            ));
        }
        match transaction.rollback().await {
            Ok(()) => {}
            Err(error) => return Err(context_refusal(error)),
        }
        let (access, mut transaction) = self.open_authorized().await?;
        if !access.allows(Permission::FOLLOWUPS_READ) {
            let _ = transaction.rollback().await;
            return Err(AssistantError::forbidden("CRM permission denied."));
        }
        if !access.allows(Permission::CONTACTS_READ) {
            let _ = transaction.rollback().await;
            return Err(AssistantError::forbidden("Contact access is required."));
        }
        let row = {
            let connection = Self::executor(&mut transaction).await?;
            sqlx::query_as::<
                _,
                (
                    Uuid,
                    String,
                    String,
                    String,
                    String,
                    i32,
                    String,
                    String,
                    Uuid,
                    String,
                    i32,
                ),
            >(
                "SELECT f.id, f.tenant_id, f.summary,
                    to_char(f.due_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
                    f.status, f.revision,
                    to_char(f.created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
                    to_char(f.updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
                    c.id, c.full_name, c.revision
                 FROM archava_crm.followups f
                 JOIN archava_crm.contacts c ON c.tenant_id=f.tenant_id AND c.id=f.contact_id
                 WHERE f.tenant_id=$1 AND c.tenant_id=$1 AND f.id=$2 AND ($3::boolean OR c.owner_id=$4)",
            )
            .bind(self.tenant.as_str())
            .bind(as_uuid(&id, "the id")?)
            .bind(access.scope == CrmScope::All)
            .bind(&access.owner_id)
            .fetch_one(connection)
            .await
        };
        let (
            followup_id,
            tenant_id,
            summary,
            due_at,
            status,
            revision,
            created_at,
            updated_at,
            contact_id,
            full_name,
            contact_revision,
        ) = match row {
            Ok(row) => row,
            Err(sqlx::Error::RowNotFound) => {
                let _ = transaction.rollback().await;
                // Invisible and missing are one answer, the containment
                // rule the follow-up detail rides.
                return Err(AssistantError::not_found("CRM follow-up not found."));
            }
            Err(error) => return self.fail(transaction, error).await,
        };
        let status = match bridged_followup_status(&status) {
            Ok(status) => status,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(error);
            }
        };
        // The lifecycle: a completed task is a conflict, not 404 — the
        // caller DID name a task they could see.
        if status != FollowUpStatus::Open {
            let _ = transaction.rollback().await;
            return Err(AssistantError::conflict(
                "This follow-up is already completed.",
            ));
        }
        Ok(CompletionPrepare {
            task: FollowupDetail {
                followup: FollowUp {
                    id: followup_id.to_string(),
                    tenant_id,
                    contact_id: contact_id.to_string(),
                    summary,
                    due_at,
                    status,
                    revision: i64::from(revision),
                    created_at,
                    updated_at,
                },
                contact: ContactRef {
                    id: contact_id.to_string(),
                    full_name,
                    revision: i64::from(contact_revision),
                },
            },
        })
    }

    async fn contact_summary(&self, id: String) -> Result<SummaryContact, AssistantError> {
        let (contact, activities, followups) = self.detail(id).await?;
        Ok(SummaryContact {
            id: contact.id,
            full_name: contact.full_name,
            revision: contact.revision,
            activities,
            followups,
        })
    }

    async fn followup_prepare(
        &self,
        contact_id: String,
        summary: String,
        due_at: String,
    ) -> Result<PreparedDraft, AssistantError> {
        // The tool's own write gate first (the baseline's `service.access` +
        // the `followups.write` check), then the detail read for the revision
        // pin — two transactions, like the baseline's two service calls.
        let (access, transaction) = self.open_authorized().await?;
        if !access.allows(Permission::FOLLOWUPS_WRITE) {
            let _ = transaction.rollback().await;
            return Err(AssistantError::forbidden(
                "Follow-up preparation is not permitted.",
            ));
        }
        match transaction.rollback().await {
            Ok(()) => {}
            Err(error) => return Err(context_refusal(error)),
        }
        let (contact, _activities, _followups) = self.detail(contact_id.clone()).await?;
        Ok(PreparedDraft {
            contact_id: contact.id,
            summary,
            due_at,
            tenant_id: self.tenant.as_str().to_string(),
            contact_revision: contact.revision,
            status: DraftStatus::Draft,
            requires_approval: true,
        })
    }

    async fn opportunities_list(
        &self,
        search: String,
        after: Option<String>,
        stage: Option<PipelineStage>,
    ) -> Result<OpportunityPage, AssistantError> {
        let (access, mut transaction) = self.open_authorized().await?;
        if !access.allows(Permission::PIPELINE_READ) {
            let _ = transaction.rollback().await;
            return Err(AssistantError::forbidden("CRM permission denied."));
        }
        if !access.allows(Permission::CONTACTS_READ) {
            let _ = transaction.rollback().await;
            return Err(AssistantError::forbidden("Contact access is required."));
        }
        let enabled = {
            let connection = Self::executor(&mut transaction).await?;
            crate::own::module_enabled(connection, &self.tenant, "crm-pipeline")
                .await
                .map_err(|_| AssistantError::unavailable("Service temporarily unavailable."))?
        };
        if !enabled {
            let _ = transaction.rollback().await;
            return Err(AssistantError::not_found("Pipeline module is disabled."));
        }
        let after_id = match after.as_deref() {
            None => None,
            Some(cursor) => Some(as_uuid(cursor, "the page cursor")?),
        };
        let pattern = format!("%{}%", escaped_search(&search));
        let rows = {
            let connection = Self::executor(&mut transaction).await?;
            sqlx::query_as::<_, (Uuid, Uuid, String, String, i32)>(
                "SELECT id, contact_id, title, stage, revision \
                 FROM archava_crm.opportunities \
                 WHERE tenant_id=$1 AND ($2::boolean OR owner_id=$3) \
                   AND ($4::uuid IS NULL OR id>$4) \
                   AND ($5::text IS NULL OR stage=$5) \
                   AND title ILIKE $6 ESCAPE '\\' \
                 ORDER BY id LIMIT $7",
            )
            .bind(self.tenant.as_str())
            .bind(access.scope == CrmScope::All)
            .bind(&access.owner_id)
            .bind(after_id)
            .bind(stage.map(stage_text))
            .bind(&pattern)
            .bind(20i64 + 1)
            .fetch_all(connection)
            .await
        };
        let rows = match rows {
            Ok(rows) => rows,
            Err(error) => return self.fail(transaction, error).await,
        };
        let overflow = rows.len() > 20;
        let mut items: Vec<OpportunityItem> = Vec::new();
        for (id, contact_id, title, raw_stage, revision) in rows.into_iter().take(20) {
            let stage = match bridged_stage(&raw_stage) {
                Ok(stage) => stage,
                Err(error) => {
                    let _ = transaction.rollback().await;
                    return Err(error);
                }
            };
            items.push(OpportunityItem {
                id: id.to_string(),
                contact_id: contact_id.to_string(),
                title,
                stage,
                revision: i64::from(revision),
            });
        }
        let next = if overflow {
            items.last().map(|item| item.id.clone())
        } else {
            None
        };
        Ok(OpportunityPage { items, next })
    }

    async fn opportunity_summary(&self, id: String) -> Result<OpportunityDetail, AssistantError> {
        let (access, mut transaction) = self.open_authorized().await?;
        if !access.allows(Permission::PIPELINE_READ) {
            let _ = transaction.rollback().await;
            return Err(AssistantError::forbidden("CRM permission denied."));
        }
        if !access.allows(Permission::CONTACTS_READ) {
            let _ = transaction.rollback().await;
            return Err(AssistantError::forbidden("Contact access is required."));
        }
        // The record under FOR SHARE, so a concurrent move cannot slip
        // between the record and its history reads (the baseline's
        // `lock: 'share'`).
        let (row_id, title, raw_stage, revision) = {
            let connection = Self::executor(&mut transaction).await?;
            let row = sqlx::query_as::<_, (Uuid, String, String, i32)>(
                "SELECT id, title, stage, revision \
                 FROM archava_crm.opportunities \
                 WHERE tenant_id=$1 AND id=$2 AND ($3::boolean OR owner_id=$4) \
                 FOR SHARE",
            )
            .bind(self.tenant.as_str())
            .bind(as_uuid(&id, "the id")?)
            .bind(access.scope == CrmScope::All)
            .bind(&access.owner_id)
            .fetch_one(connection)
            .await;
            match row {
                Ok(row) => row,
                Err(sqlx::Error::RowNotFound) => {
                    let _ = transaction.rollback().await;
                    return Err(AssistantError::not_found("Opportunity not found."));
                }
                Err(error) => {
                    let _ = transaction.rollback().await;
                    return Err(db_refusal(error));
                }
            }
        };
        let stage = match bridged_stage(&raw_stage) {
            Ok(stage) => stage,
            // The transaction drops here (rollback both ways).
            Err(error) => return Err(error),
        };
        let history = {
            let connection = Self::executor(&mut transaction).await?;
            sqlx::query_as::<_, (Option<String>, String, Option<String>, i32, String)>(
                "SELECT from_stage, to_stage, reason, revision, \
                    to_char(occurred_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
                 FROM archava_crm.opportunity_transitions \
                 WHERE tenant_id=$1 AND opportunity_id=$2 \
                 ORDER BY revision DESC LIMIT 20",
            )
            .bind(self.tenant.as_str())
            .bind(as_uuid(&id, "the id")?)
            .fetch_all(connection)
            .await
        };
        let history = match history {
            Ok(rows) => rows,
            Err(error) => return self.fail(transaction, error).await,
        };
        let mut transitions: Vec<Transition> = Vec::new();
        for (from_stage, to_stage, reason, revision, occurred_at) in history {
            let from = match from_stage.as_deref().map(bridged_stage) {
                Some(result) => Some(result?),
                None => None,
            };
            let to = match bridged_stage(&to_stage) {
                Ok(to) => to,
                Err(error) => return Err(error),
            };
            transitions.push(Transition {
                from,
                to,
                reason,
                revision: i64::from(revision),
                occurred_at,
            });
        }
        Ok(OpportunityDetail {
            id: row_id.to_string(),
            title,
            stage,
            revision: i64::from(revision),
            transitions,
        })
    }
}
