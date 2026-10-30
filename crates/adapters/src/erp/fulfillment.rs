//! The fulfillment SQL adapter — the Archxy 2 port of `packages/fulfillment-pg/src/repository.ts`:
//! the fulfillment ledger's command ladder (claim → authorize → lifecycle → custody → findings → receipt)
//! and read projections (access, jobs list, detail, lines, findings, custody events, command status).
//!
//! ## Invariants preserved
//!
//! - RLS tenant isolation and member ladder.
//! - Sequential custody events: begins with `received` (stage 0), strictly advances stage index;
//!   `returned` strictly requires a prior `dispatched` event.
//! - Quality gate on completion: moving to `completed` strictly requires `unreconciled_count == 0`.
//! - Exception findings (`partial`, `rejected`, `lost`, `damaged`) increment `unreconciled_count`.
//!   Accepted finding with `reconciles_id` decrements `unreconciled_count`.
//! - Optimistic locking on `change_job_status`: requires matching `expectedRevision` and advances revision by 1.
//! - Idempotency replay with fingerprint verification and actor isolation.

use archava_domain::erp::fulfillment::{
    CustodyEvent, CustodyRecord, Finding, FindingCategory, FindingDisposition, FulfillmentAccess,
    FulfillmentCommandStatus, FulfillmentMutation, FulfillmentReceipt, Job, JobDetail, JobKind,
    JobLine, JobPageQuery, JobStatus, LineDirection, LineDisposition, FULFILLMENT_PERMISSIONS,
};
use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
use archava_domain::erp::{ErpError, ErpMembershipKind, ErpScope};
use serde_json::Value;
use sqlx::{PgConnection, Row};
use thiserror::Error;
use uuid::Uuid;

use super::commerce_kernel::load_locked_membership;
use super::PurchaseError;

pub const FULFILLMENT_LEDGER: &str = "fulfillment";
pub const FULFILLMENT_NAMESPACE: &str = "archava_fulfillment";

#[derive(Debug, Error)]
pub enum FulfillmentError {
    #[error("fulfillment domain refusal: {0:?}")]
    Domain(ErpError),
    #[error("fulfillment document refused by the grammar")]
    Invalid,
    #[error("fulfillment bad request: {0}")]
    BadRequest(&'static str),
    #[error("fulfillment conflict: {0}")]
    Conflict(&'static str),
    #[error("fulfillment invalid transition: {0}")]
    InvalidTransition(&'static str),
    #[error("fulfillment custody out of order: {0}")]
    CustodyOutOfOrder(&'static str),
    #[error("fulfillment unreconciled exceptions: {0}")]
    UnreconciledExceptions(&'static str),
    #[error("fulfillment database failure in {ledger}: {source}")]
    Database {
        ledger: &'static str,
        #[source]
        source: sqlx::Error,
    },
}

impl From<FulfillmentError> for PurchaseError {
    fn from(error: FulfillmentError) -> Self {
        match error {
            FulfillmentError::Domain(domain) => PurchaseError::Domain(domain),
            FulfillmentError::Invalid => PurchaseError::Domain(ErpError::Unavailable),
            FulfillmentError::BadRequest(_) => PurchaseError::Domain(ErpError::Unavailable),
            FulfillmentError::Conflict(_) => PurchaseError::Domain(ErpError::Conflict),
            FulfillmentError::InvalidTransition(_) => PurchaseError::Domain(ErpError::Conflict),
            FulfillmentError::CustodyOutOfOrder(_) => PurchaseError::Domain(ErpError::Conflict),
            FulfillmentError::UnreconciledExceptions(_) => {
                PurchaseError::Domain(ErpError::Conflict)
            }
            FulfillmentError::Database { ledger, source } => {
                PurchaseError::Database { ledger, source }
            }
        }
    }
}

fn forbidden() -> FulfillmentError {
    FulfillmentError::Domain(ErpError::Forbidden)
}

fn not_found() -> FulfillmentError {
    FulfillmentError::Domain(ErpError::NotFound)
}

fn conflict(msg: &'static str) -> FulfillmentError {
    FulfillmentError::Conflict(msg)
}

fn database(error: sqlx::Error) -> FulfillmentError {
    FulfillmentError::Database {
        ledger: FULFILLMENT_LEDGER,
        source: error,
    }
}

fn uuid_of(value: &str) -> Result<Uuid, FulfillmentError> {
    Uuid::parse_str(value).map_err(|_| FulfillmentError::Invalid)
}

fn require_permission(
    access: &FulfillmentAccess,
    permission: &str,
) -> Result<(), FulfillmentError> {
    if access.permissions.iter().any(|p| p == permission) {
        Ok(())
    } else {
        Err(forbidden())
    }
}

pub async fn resolve_fulfillment_context(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<FulfillmentAccess, FulfillmentError> {
    let own = load_locked_membership(
        connection,
        FULFILLMENT_NAMESPACE,
        FULFILLMENT_LEDGER,
        tenant,
        &principal.id,
        FULFILLMENT_PERMISSIONS,
    )
    .await
    .map_err(|error| match error {
        PurchaseError::Domain(domain) => FulfillmentError::Domain(domain),
        PurchaseError::Database { ledger, source } => FulfillmentError::Database { ledger, source },
    })?
    .ok_or_else(forbidden)?;

    if !own.active {
        return Err(forbidden());
    }

    let principal_kind = match principal.kind {
        PrincipalKind::Human => ErpMembershipKind::Human,
        PrincipalKind::Service => ErpMembershipKind::Service,
    };
    if own.kind != principal_kind {
        return Err(forbidden());
    }

    let mut permissions = own.permissions.clone();
    let mut scope = own.scope;
    let owner_id = principal.id.clone();

    if principal.kind == PrincipalKind::Service {
        let human_id = principal.on_behalf_of.as_deref().ok_or_else(forbidden)?;
        let human = load_locked_membership(
            connection,
            FULFILLMENT_NAMESPACE,
            FULFILLMENT_LEDGER,
            tenant,
            human_id,
            FULFILLMENT_PERMISSIONS,
        )
        .await
        .map_err(|error| match error {
            PurchaseError::Domain(domain) => FulfillmentError::Domain(domain),
            PurchaseError::Database { ledger, source } => {
                FulfillmentError::Database { ledger, source }
            }
        })?
        .ok_or_else(forbidden)?;

        if !human.active || human.kind != ErpMembershipKind::Human {
            return Err(forbidden());
        }

        permissions.retain(|p| human.permissions.iter().any(|hp| hp == p));
        if human.scope == ErpScope::Assigned {
            scope = ErpScope::Assigned;
        }
    }

    Ok(FulfillmentAccess {
        permissions,
        owner_id,
        scope,
    })
}

#[allow(clippy::too_many_arguments)]
async fn append_audit(
    connection: &mut PgConnection,
    tenant: &str,
    command_id: Uuid,
    actor_id: &str,
    operation: &str,
    job_id: Uuid,
    revision: i32,
    reason: &str,
) -> Result<(), FulfillmentError> {
    sqlx::query(
        "INSERT INTO archava_fulfillment.audit \
         (tenant_id, request_id, actor_id, operation, job_id, revision, reason) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(tenant)
    .bind(command_id)
    .bind(actor_id)
    .bind(operation)
    .bind(job_id)
    .bind(revision)
    .bind(reason)
    .execute(&mut *connection)
    .await
    .map_err(database)?;
    Ok(())
}

#[allow(dead_code)]
struct LoadedJob {
    id: Uuid,
    status: JobStatus,
    consumed_count: i32,
    produced_count: i32,
    scrap_count: i32,
    unreconciled_count: i32,
    revision: i32,
}

async fn load_job_for_update(
    connection: &mut PgConnection,
    tenant: &str,
    job_id: Uuid,
    access: &FulfillmentAccess,
) -> Result<LoadedJob, FulfillmentError> {
    let (query_str, is_assigned) = match access.scope {
        ErpScope::Assigned => (
            "SELECT id, status, consumed_count, produced_count, scrap_count, unreconciled_count, revision \
             FROM archava_fulfillment.jobs \
             WHERE tenant_id = $1 AND id = $2 AND owner_id = $3 \
             FOR UPDATE",
            true,
        ),
        ErpScope::All => (
            "SELECT id, status, consumed_count, produced_count, scrap_count, unreconciled_count, revision \
             FROM archava_fulfillment.jobs \
             WHERE tenant_id = $1 AND id = $2 \
             FOR UPDATE",
            false,
        ),
    };

    let row = if is_assigned {
        sqlx::query(query_str)
            .bind(tenant)
            .bind(job_id)
            .bind(&access.owner_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
    } else {
        sqlx::query(query_str)
            .bind(tenant)
            .bind(job_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
    };

    let row = row.ok_or_else(not_found)?;
    let status_str: String = row.try_get("status").map_err(database)?;
    let status = JobStatus::parse(&status_str).ok_or(FulfillmentError::Invalid)?;

    Ok(LoadedJob {
        id: row.try_get("id").map_err(database)?,
        status,
        consumed_count: row.try_get("consumed_count").map_err(database)?,
        produced_count: row.try_get("produced_count").map_err(database)?,
        scrap_count: row.try_get("scrap_count").map_err(database)?,
        unreconciled_count: row.try_get("unreconciled_count").map_err(database)?,
        revision: row.try_get("revision").map_err(database)?,
    })
}

fn assert_open(job: &LoadedJob) -> Result<(), FulfillmentError> {
    if job.status.is_closed() {
        Err(FulfillmentError::InvalidTransition(
            "Evidence cannot be recorded on a completed or canceled job",
        ))
    } else {
        Ok(())
    }
}

pub async fn record_fulfillment_mutation(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    mutation: FulfillmentMutation,
    fingerprint: &str,
) -> Result<FulfillmentReceipt, FulfillmentError> {
    let access = resolve_fulfillment_context(connection, tenant, principal).await?;

    // Authorization checks
    require_permission(&access, "fulfillment.write")?;

    match &mutation {
        FulfillmentMutation::RecordFinding { category, .. } => {
            if *category == FindingCategory::Delivery {
                require_permission(&access, "fulfillment.dispatch")?;
            }
        }
        FulfillmentMutation::RecordCustody { event, .. } => {
            if event.is_dispatch_stage() {
                require_permission(&access, "fulfillment.dispatch")?;
            }
        }
        FulfillmentMutation::RecordLine { job_id, .. } => {
            let job_uuid = uuid_of(job_id)?;
            let kind_row = sqlx::query(
                "SELECT kind FROM archava_fulfillment.jobs \
                 WHERE tenant_id = $1 AND id = $2",
            )
            .bind(tenant)
            .bind(job_uuid)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?;

            if let Some(row) = kind_row {
                let kind_str: String = row.try_get("kind").map_err(database)?;
                if kind_str == "production" {
                    require_permission(&access, "fulfillment.produce")?;
                }
            }
        }
        _ => {}
    }

    let command_uuid = uuid_of(mutation.command_id())?;

    // Command claim / idempotency replay
    let claimed = sqlx::query(
        "INSERT INTO archava_fulfillment.commands(tenant_id, request_id, actor_id, fingerprint) \
         VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING RETURNING request_id",
    )
    .bind(tenant)
    .bind(command_uuid)
    .bind(&access.owner_id)
    .bind(fingerprint)
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;

    if claimed.is_none() {
        let existing = sqlx::query(
            "SELECT actor_id, fingerprint, receipt \
             FROM archava_fulfillment.commands \
             WHERE tenant_id = $1 AND request_id = $2 AND actor_id = $3 FOR UPDATE",
        )
        .bind(tenant)
        .bind(command_uuid)
        .bind(&access.owner_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(database)?;

        let Some(row) = existing else {
            return Err(conflict("Foreign command key conflict"));
        };

        let actor_id: String = row.try_get("actor_id").map_err(database)?;
        let recorded_fp: String = row.try_get("fingerprint").map_err(database)?;
        if actor_id != access.owner_id || recorded_fp != fingerprint {
            return Err(conflict("Command payload fingerprint mismatch on replay"));
        }

        let receipt_val: Option<Value> = row.try_get("receipt").map_err(database)?;
        let receipt_val = receipt_val.ok_or_else(|| conflict("Command effect not yet recorded"))?;
        let mut receipt: FulfillmentReceipt =
            serde_json::from_value(receipt_val).map_err(|_| FulfillmentError::Invalid)?;
        receipt.idempotency_replay = true;
        return Ok(receipt);
    }

    let receipt: FulfillmentReceipt;

    match mutation {
        FulfillmentMutation::CreateJob {
            command_id,
            job_code,
            kind,
            title,
            owner_ref,
        } => {
            let job_id = Uuid::new_v4();
            let row = sqlx::query(
                "INSERT INTO archava_fulfillment.jobs( \
                    tenant_id, id, job_code, kind, title, owner_ref, status, owner_id \
                ) VALUES ($1, $2, $3, $4, $5, $6, 'draft', $7) \
                RETURNING to_char(created_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS executed_at",
            )
            .bind(tenant)
            .bind(job_id)
            .bind(&job_code)
            .bind(kind.as_str())
            .bind(&title)
            .bind(owner_ref.as_deref())
            .bind(&access.owner_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            let executed_at: String = row.try_get("executed_at").map_err(database)?;

            append_audit(
                connection,
                tenant,
                command_uuid,
                &access.owner_id,
                "create_job",
                job_id,
                1,
                &format!("Created {} job {}", kind.as_str(), job_code),
            )
            .await?;

            receipt = FulfillmentReceipt {
                command_id,
                tenant_id: tenant.to_string(),
                action: "create_job".to_string(),
                job_id: Some(job_id.to_string()),
                line_id: None,
                finding_id: None,
                custody_id: None,
                status: "draft".to_string(),
                revision: Some(1),
                idempotency_replay: false,
                executed_at,
            };
        }

        FulfillmentMutation::RecordLine {
            command_id,
            job_id: target_job_id,
            direction,
            item_ref,
            quantity,
            disposition,
            evidence_ref,
        } => {
            let job_uuid = uuid_of(&target_job_id)?;
            let job = load_job_for_update(connection, tenant, job_uuid, &access).await?;
            assert_open(&job)?;

            let next_consumed = job.consumed_count
                + if disposition == LineDisposition::Normal && direction == LineDirection::Consume {
                    quantity
                } else {
                    0
                };
            let next_produced = job.produced_count
                + if disposition == LineDisposition::Normal && direction == LineDirection::Produce {
                    quantity
                } else {
                    0
                };
            let next_scrap = job.scrap_count
                + if disposition == LineDisposition::Scrap {
                    quantity
                } else {
                    0
                };

            sqlx::query(
                "UPDATE archava_fulfillment.jobs \
                 SET consumed_count = $1, produced_count = $2, scrap_count = $3, updated_at = clock_timestamp() \
                 WHERE tenant_id = $4 AND id = $5",
            )
            .bind(next_consumed)
            .bind(next_produced)
            .bind(next_scrap)
            .bind(tenant)
            .bind(job_uuid)
            .execute(&mut *connection)
            .await
            .map_err(database)?;

            let line_id = Uuid::new_v4();
            let row = sqlx::query(
                "INSERT INTO archava_fulfillment.job_lines( \
                    tenant_id, id, job_id, direction, item_ref, quantity, disposition, evidence_ref, recorded_by \
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
                RETURNING to_char(recorded_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS executed_at",
            )
            .bind(tenant)
            .bind(line_id)
            .bind(job_uuid)
            .bind(direction.as_str())
            .bind(&item_ref)
            .bind(quantity)
            .bind(disposition.as_str())
            .bind(evidence_ref.as_deref())
            .bind(&access.owner_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            let executed_at: String = row.try_get("executed_at").map_err(database)?;

            append_audit(
                connection,
                tenant,
                command_uuid,
                &access.owner_id,
                "record_line",
                job_uuid,
                job.revision,
                &format!(
                    "Recorded {} line {} x{} ({})",
                    direction.as_str(),
                    item_ref,
                    quantity,
                    disposition.as_str()
                ),
            )
            .await?;

            receipt = FulfillmentReceipt {
                command_id,
                tenant_id: tenant.to_string(),
                action: "record_line".to_string(),
                job_id: Some(job_uuid.to_string()),
                line_id: Some(line_id.to_string()),
                finding_id: None,
                custody_id: None,
                status: format!("{}/{}", direction.as_str(), disposition.as_str()),
                revision: Some(job.revision),
                idempotency_replay: false,
                executed_at,
            };
        }

        FulfillmentMutation::RecordFinding {
            command_id,
            job_id: target_job_id,
            category,
            disposition,
            quantity,
            reason,
            evidence_ref,
            reconciles_id,
        } => {
            let job_uuid = uuid_of(&target_job_id)?;
            let job = load_job_for_update(connection, tenant, job_uuid, &access).await?;
            assert_open(&job)?;

            let reconciles_uuid = match &reconciles_id {
                Some(s) => Some(uuid_of(s)?),
                None => None,
            };

            let mut next_unreconciled = job.unreconciled_count;

            if disposition.is_exception() {
                next_unreconciled += 1;
            } else if disposition == FindingDisposition::Accepted {
                if let Some(target_uuid) = reconciles_uuid {
                    let target_row = sqlx::query(
                        "SELECT job_id, disposition FROM archava_fulfillment.findings \
                         WHERE tenant_id = $1 AND id = $2",
                    )
                    .bind(tenant)
                    .bind(target_uuid)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(database)?;

                    let Some(target) = target_row else {
                        return Err(not_found());
                    };

                    let target_job: Uuid = target.try_get("job_id").map_err(database)?;
                    if target_job != job_uuid {
                        return Err(conflict("Reconciliation target belongs to a different job"));
                    }

                    let target_disp_str: String =
                        target.try_get("disposition").map_err(database)?;
                    let target_disp = FindingDisposition::parse(&target_disp_str)
                        .ok_or(FulfillmentError::Invalid)?;
                    if !target_disp.is_exception() {
                        return Err(conflict(
                            "Only partial, rejected, lost or damaged findings require reconciliation",
                        ));
                    }

                    let already_row = sqlx::query(
                        "SELECT 1 FROM archava_fulfillment.findings \
                         WHERE tenant_id = $1 AND reconciles_id = $2",
                    )
                    .bind(tenant)
                    .bind(target_uuid)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(database)?;

                    if already_row.is_some() {
                        return Err(conflict("Finding has already been reconciled"));
                    }

                    next_unreconciled = (next_unreconciled - 1).max(0);
                }
            }

            sqlx::query(
                "UPDATE archava_fulfillment.jobs \
                 SET unreconciled_count = $1, updated_at = clock_timestamp() \
                 WHERE tenant_id = $2 AND id = $3",
            )
            .bind(next_unreconciled)
            .bind(tenant)
            .bind(job_uuid)
            .execute(&mut *connection)
            .await
            .map_err(database)?;

            let finding_id = Uuid::new_v4();
            let row = sqlx::query(
                "INSERT INTO archava_fulfillment.findings( \
                    tenant_id, id, job_id, category, disposition, quantity, reason, evidence_ref, reconciles_id, recorded_by \
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
                RETURNING to_char(recorded_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS executed_at",
            )
            .bind(tenant)
            .bind(finding_id)
            .bind(job_uuid)
            .bind(category.as_str())
            .bind(disposition.as_str())
            .bind(quantity)
            .bind(&reason)
            .bind(evidence_ref.as_deref())
            .bind(reconciles_uuid)
            .bind(&access.owner_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            let executed_at: String = row.try_get("executed_at").map_err(database)?;

            append_audit(
                connection,
                tenant,
                command_uuid,
                &access.owner_id,
                "record_finding",
                job_uuid,
                job.revision,
                &format!(
                    "Recorded {} finding ({}): {}",
                    category.as_str(),
                    disposition.as_str(),
                    reason
                ),
            )
            .await?;

            receipt = FulfillmentReceipt {
                command_id,
                tenant_id: tenant.to_string(),
                action: "record_finding".to_string(),
                job_id: Some(job_uuid.to_string()),
                line_id: None,
                finding_id: Some(finding_id.to_string()),
                custody_id: None,
                status: disposition.as_str().to_string(),
                revision: Some(job.revision),
                idempotency_replay: false,
                executed_at,
            };
        }

        FulfillmentMutation::RecordCustody {
            command_id,
            job_id: target_job_id,
            event,
            from_ref,
            to_ref,
            evidence_ref,
            note,
        } => {
            let job_uuid = uuid_of(&target_job_id)?;
            let job = load_job_for_update(connection, tenant, job_uuid, &access).await?;
            assert_open(&job)?;

            let history_rows = sqlx::query(
                "SELECT event FROM archava_fulfillment.custody_events \
                 WHERE tenant_id = $1 AND job_id = $2 \
                 ORDER BY recorded_at ASC, id ASC",
            )
            .bind(tenant)
            .bind(job_uuid)
            .fetch_all(&mut *connection)
            .await
            .map_err(database)?;

            if history_rows.is_empty() {
                if event != CustodyEvent::Received {
                    return Err(FulfillmentError::CustodyOutOfOrder(
                        "Custody chain must begin with received",
                    ));
                }
            } else {
                let mut max_stage = -1;
                for row in &history_rows {
                    let ev_str: String = row.try_get("event").map_err(database)?;
                    if let Some(ev) = CustodyEvent::parse(&ev_str) {
                        if let Some(st) = ev.stage() {
                            if st > max_stage {
                                max_stage = st;
                            }
                        }
                    }
                }

                if event == CustodyEvent::Returned {
                    if max_stage < 4 {
                        return Err(FulfillmentError::CustodyOutOfOrder(
                            "Return requires a prior dispatched custody event",
                        ));
                    }
                } else {
                    let stage = event.stage().unwrap_or(-1);
                    if stage <= max_stage {
                        return Err(FulfillmentError::CustodyOutOfOrder(
                            "Custody event must strictly follow previous stage",
                        ));
                    }
                }
            }

            let custody_id = Uuid::new_v4();
            let row = sqlx::query(
                "INSERT INTO archava_fulfillment.custody_events( \
                    tenant_id, id, job_id, event, from_ref, to_ref, evidence_ref, note, recorded_by \
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
                RETURNING to_char(recorded_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS executed_at",
            )
            .bind(tenant)
            .bind(custody_id)
            .bind(job_uuid)
            .bind(event.as_str())
            .bind(from_ref.as_deref())
            .bind(to_ref.as_deref())
            .bind(evidence_ref.as_deref())
            .bind(note.as_deref())
            .bind(&access.owner_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            let executed_at: String = row.try_get("executed_at").map_err(database)?;

            append_audit(
                connection,
                tenant,
                command_uuid,
                &access.owner_id,
                "record_custody",
                job_uuid,
                job.revision,
                &format!("Recorded custody event {}", event.as_str()),
            )
            .await?;

            receipt = FulfillmentReceipt {
                command_id,
                tenant_id: tenant.to_string(),
                action: "record_custody".to_string(),
                job_id: Some(job_uuid.to_string()),
                line_id: None,
                finding_id: None,
                custody_id: Some(custody_id.to_string()),
                status: event.as_str().to_string(),
                revision: Some(job.revision),
                idempotency_replay: false,
                executed_at,
            };
        }

        FulfillmentMutation::ChangeJobStatus {
            command_id,
            job_id: target_job_id,
            expected_revision,
            target_status,
            reason,
        } => {
            let job_uuid = uuid_of(&target_job_id)?;
            let job = load_job_for_update(connection, tenant, job_uuid, &access).await?;

            if job.revision != expected_revision {
                return Err(conflict("Optimistic revision conflict on job"));
            }

            if !job.status.can_transition_to(target_status) {
                return Err(FulfillmentError::InvalidTransition(
                    "Job cannot move to target status",
                ));
            }

            if target_status == JobStatus::Completed && job.unreconciled_count > 0 {
                return Err(FulfillmentError::UnreconciledExceptions(
                    "Delivery or inspection findings must be reconciled before completion",
                ));
            }

            let next_rev = job.revision + 1;
            let row = sqlx::query(
                "UPDATE archava_fulfillment.jobs \
                 SET status = $1, revision = $2, updated_at = clock_timestamp() \
                 WHERE tenant_id = $3 AND id = $4 \
                 RETURNING to_char(updated_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS executed_at",
            )
            .bind(target_status.as_str())
            .bind(next_rev)
            .bind(tenant)
            .bind(job_uuid)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            let executed_at: String = row.try_get("executed_at").map_err(database)?;

            append_audit(
                connection,
                tenant,
                command_uuid,
                &access.owner_id,
                "change_job_status",
                job_uuid,
                next_rev,
                &reason,
            )
            .await?;

            receipt = FulfillmentReceipt {
                command_id,
                tenant_id: tenant.to_string(),
                action: "change_job_status".to_string(),
                job_id: Some(job_uuid.to_string()),
                line_id: None,
                finding_id: None,
                custody_id: None,
                status: target_status.as_str().to_string(),
                revision: Some(next_rev),
                idempotency_replay: false,
                executed_at,
            };
        }
    }

    let receipt_json = serde_json::to_value(&receipt).map_err(|_| FulfillmentError::Invalid)?;

    sqlx::query(
        "UPDATE archava_fulfillment.commands \
         SET receipt = $1 \
         WHERE tenant_id = $2 AND request_id = $3",
    )
    .bind(&receipt_json)
    .bind(tenant)
    .bind(command_uuid)
    .execute(&mut *connection)
    .await
    .map_err(database)?;

    Ok(receipt)
}

pub async fn fulfillment_command_status(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    command_id: &str,
) -> Result<FulfillmentCommandStatus, FulfillmentError> {
    let access = resolve_fulfillment_context(connection, tenant, principal).await?;
    require_permission(&access, "fulfillment.read")?;

    let command_uuid = uuid_of(command_id)?;

    let row = sqlx::query(
        "SELECT receipt FROM archava_fulfillment.commands \
         WHERE tenant_id = $1 AND request_id = $2 AND actor_id = $3",
    )
    .bind(tenant)
    .bind(command_uuid)
    .bind(&access.owner_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;

    let Some(row) = row else {
        return Ok(FulfillmentCommandStatus::Unknown);
    };

    let receipt_val: Option<Value> = row.try_get("receipt").map_err(database)?;
    let Some(receipt_val) = receipt_val else {
        return Ok(FulfillmentCommandStatus::Unknown);
    };

    let receipt: FulfillmentReceipt =
        serde_json::from_value(receipt_val).map_err(|_| FulfillmentError::Invalid)?;

    Ok(FulfillmentCommandStatus::Recorded {
        receipt: Box::new(receipt),
    })
}

fn map_job(row: &sqlx::postgres::PgRow, tenant: &str) -> Result<Job, FulfillmentError> {
    let id: Uuid = row.try_get("id").map_err(database)?;
    let kind_str: String = row.try_get("kind").map_err(database)?;
    let kind = JobKind::parse(&kind_str).ok_or(FulfillmentError::Invalid)?;
    let status_str: String = row.try_get("status").map_err(database)?;
    let status = JobStatus::parse(&status_str).ok_or(FulfillmentError::Invalid)?;

    Ok(Job {
        id: id.to_string(),
        tenant_id: tenant.to_string(),
        job_code: row.try_get("job_code").map_err(database)?,
        kind,
        title: row.try_get("title").map_err(database)?,
        owner_ref: row.try_get("owner_ref").map_err(database)?,
        status,
        consumed_count: row.try_get("consumed_count").map_err(database)?,
        produced_count: row.try_get("produced_count").map_err(database)?,
        scrap_count: row.try_get("scrap_count").map_err(database)?,
        unreconciled_count: row.try_get("unreconciled_count").map_err(database)?,
        revision: row.try_get("revision").map_err(database)?,
        created_at: row.try_get("created_at").map_err(database)?,
        updated_at: row.try_get("updated_at").map_err(database)?,
    })
}

pub async fn list_fulfillment_jobs(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    query: JobPageQuery,
) -> Result<Vec<Job>, FulfillmentError> {
    let access = resolve_fulfillment_context(connection, tenant, principal).await?;
    require_permission(&access, "fulfillment.read")?;

    let mut sql = String::from(
        "SELECT id, job_code, kind, title, owner_ref, status, consumed_count, produced_count, \
                scrap_count, unreconciled_count, revision, \
                to_char(created_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, \
                to_char(updated_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at \
         FROM archava_fulfillment.jobs \
         WHERE tenant_id = $1",
    );

    let mut param_index = 2;
    if query.status.is_some() {
        sql.push_str(&format!(" AND status = ${param_index}"));
        param_index += 1;
    }

    if query.kind.is_some() {
        sql.push_str(&format!(" AND kind = ${param_index}"));
        param_index += 1;
    }

    if access.scope == ErpScope::Assigned {
        sql.push_str(&format!(" AND owner_id = ${param_index}"));
        param_index += 1;
    }

    sql.push_str(&format!(
        " ORDER BY created_at ASC, id ASC LIMIT ${param_index} OFFSET ${}",
        param_index + 1
    ));

    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.as_str())).bind(tenant);

    if let Some(status) = query.status {
        q = q.bind(status.as_str());
    }

    if let Some(kind) = query.kind {
        q = q.bind(kind.as_str());
    }

    if access.scope == ErpScope::Assigned {
        q = q.bind(&access.owner_id);
    }

    q = q.bind(query.limit).bind(query.offset);

    let rows = q.fetch_all(&mut *connection).await.map_err(database)?;
    let mut jobs = Vec::with_capacity(rows.len());
    for row in &rows {
        jobs.push(map_job(row, tenant)?);
    }
    Ok(jobs)
}

pub async fn get_fulfillment_job_detail(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    job_id: &str,
) -> Result<Option<JobDetail>, FulfillmentError> {
    let access = resolve_fulfillment_context(connection, tenant, principal).await?;
    require_permission(&access, "fulfillment.read")?;

    let job_uuid = uuid_of(job_id)?;

    let (query_str, is_assigned) = match access.scope {
        ErpScope::Assigned => (
            "SELECT id, job_code, kind, title, owner_ref, status, consumed_count, produced_count, \
                    scrap_count, unreconciled_count, revision, \
                    to_char(created_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, \
                    to_char(updated_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at \
             FROM archava_fulfillment.jobs \
             WHERE tenant_id = $1 AND id = $2 AND owner_id = $3",
            true,
        ),
        ErpScope::All => (
            "SELECT id, job_code, kind, title, owner_ref, status, consumed_count, produced_count, \
                    scrap_count, unreconciled_count, revision, \
                    to_char(created_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, \
                    to_char(updated_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at \
             FROM archava_fulfillment.jobs \
             WHERE tenant_id = $1 AND id = $2",
            false,
        ),
    };

    let row = if is_assigned {
        sqlx::query(query_str)
            .bind(tenant)
            .bind(job_uuid)
            .bind(&access.owner_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
    } else {
        sqlx::query(query_str)
            .bind(tenant)
            .bind(job_uuid)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
    };

    let Some(row) = row else {
        return Ok(None);
    };

    let job = map_job(&row, tenant)?;

    let line_rows = sqlx::query(
        "SELECT c.id, c.job_id, c.direction, c.item_ref, c.quantity, c.disposition, c.evidence_ref, c.recorded_by, \
                to_char(c.recorded_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at \
         FROM archava_fulfillment.job_lines c \
         JOIN archava_fulfillment.jobs p ON p.tenant_id = c.tenant_id AND p.id = c.job_id \
         WHERE c.tenant_id = $1 AND c.job_id = $2 \
         ORDER BY c.recorded_at ASC, c.id ASC",
    )
    .bind(tenant)
    .bind(job_uuid)
    .fetch_all(&mut *connection)
    .await
    .map_err(database)?;

    let mut lines = Vec::with_capacity(line_rows.len());
    for lrow in line_rows {
        let lid: Uuid = lrow.try_get("id").map_err(database)?;
        let ljid: Uuid = lrow.try_get("job_id").map_err(database)?;
        let dir_str: String = lrow.try_get("direction").map_err(database)?;
        let disp_str: String = lrow.try_get("disposition").map_err(database)?;
        lines.push(JobLine {
            id: lid.to_string(),
            tenant_id: tenant.to_string(),
            job_id: ljid.to_string(),
            direction: LineDirection::parse(&dir_str).ok_or(FulfillmentError::Invalid)?,
            item_ref: lrow.try_get("item_ref").map_err(database)?,
            quantity: lrow.try_get("quantity").map_err(database)?,
            disposition: LineDisposition::parse(&disp_str).ok_or(FulfillmentError::Invalid)?,
            evidence_ref: lrow.try_get("evidence_ref").map_err(database)?,
            recorded_by: lrow.try_get("recorded_by").map_err(database)?,
            recorded_at: lrow.try_get("recorded_at").map_err(database)?,
        });
    }

    let finding_rows = sqlx::query(
        "SELECT c.id, c.job_id, c.category, c.disposition, c.quantity, c.reason, c.evidence_ref, c.reconciles_id, c.recorded_by, \
                to_char(c.recorded_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at \
         FROM archava_fulfillment.findings c \
         JOIN archava_fulfillment.jobs p ON p.tenant_id = c.tenant_id AND p.id = c.job_id \
         WHERE c.tenant_id = $1 AND c.job_id = $2 \
         ORDER BY c.recorded_at ASC, c.id ASC",
    )
    .bind(tenant)
    .bind(job_uuid)
    .fetch_all(&mut *connection)
    .await
    .map_err(database)?;

    let mut findings = Vec::with_capacity(finding_rows.len());
    for frow in finding_rows {
        let fid: Uuid = frow.try_get("id").map_err(database)?;
        let fjid: Uuid = frow.try_get("job_id").map_err(database)?;
        let cat_str: String = frow.try_get("category").map_err(database)?;
        let disp_str: String = frow.try_get("disposition").map_err(database)?;
        let reconciles: Option<Uuid> = frow.try_get("reconciles_id").map_err(database)?;
        findings.push(Finding {
            id: fid.to_string(),
            tenant_id: tenant.to_string(),
            job_id: fjid.to_string(),
            category: FindingCategory::parse(&cat_str).ok_or(FulfillmentError::Invalid)?,
            disposition: FindingDisposition::parse(&disp_str).ok_or(FulfillmentError::Invalid)?,
            quantity: frow.try_get("quantity").map_err(database)?,
            reason: frow.try_get("reason").map_err(database)?,
            evidence_ref: frow.try_get("evidence_ref").map_err(database)?,
            reconciles_id: reconciles.map(|u| u.to_string()),
            recorded_by: frow.try_get("recorded_by").map_err(database)?,
            recorded_at: frow.try_get("recorded_at").map_err(database)?,
        });
    }

    let custody_rows = sqlx::query(
        "SELECT c.id, c.job_id, c.event, c.from_ref, c.to_ref, c.evidence_ref, c.note, c.recorded_by, \
                to_char(c.recorded_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at \
         FROM archava_fulfillment.custody_events c \
         JOIN archava_fulfillment.jobs p ON p.tenant_id = c.tenant_id AND p.id = c.job_id \
         WHERE c.tenant_id = $1 AND c.job_id = $2 \
         ORDER BY c.recorded_at ASC, c.id ASC",
    )
    .bind(tenant)
    .bind(job_uuid)
    .fetch_all(&mut *connection)
    .await
    .map_err(database)?;

    let mut custody = Vec::with_capacity(custody_rows.len());
    for crow in custody_rows {
        let cid: Uuid = crow.try_get("id").map_err(database)?;
        let cjid: Uuid = crow.try_get("job_id").map_err(database)?;
        let ev_str: String = crow.try_get("event").map_err(database)?;
        custody.push(CustodyRecord {
            id: cid.to_string(),
            tenant_id: tenant.to_string(),
            job_id: cjid.to_string(),
            event: CustodyEvent::parse(&ev_str).ok_or(FulfillmentError::Invalid)?,
            from_ref: crow.try_get("from_ref").map_err(database)?,
            to_ref: crow.try_get("to_ref").map_err(database)?,
            evidence_ref: crow.try_get("evidence_ref").map_err(database)?,
            note: crow.try_get("note").map_err(database)?,
            recorded_by: crow.try_get("recorded_by").map_err(database)?,
            recorded_at: crow.try_get("recorded_at").map_err(database)?,
        });
    }

    Ok(Some(JobDetail {
        job,
        lines,
        findings,
        custody,
    }))
}

pub async fn list_fulfillment_lines(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    job_id: &str,
) -> Result<Vec<JobLine>, FulfillmentError> {
    let access = resolve_fulfillment_context(connection, tenant, principal).await?;
    require_permission(&access, "fulfillment.read")?;
    let job_uuid = uuid_of(job_id)?;

    let (query_str, is_assigned) = match access.scope {
        ErpScope::Assigned => (
            "SELECT c.id, c.job_id, c.direction, c.item_ref, c.quantity, c.disposition, c.evidence_ref, c.recorded_by, \
                    to_char(c.recorded_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at \
             FROM archava_fulfillment.job_lines c \
             JOIN archava_fulfillment.jobs p ON p.tenant_id = c.tenant_id AND p.id = c.job_id \
             WHERE c.tenant_id = $1 AND c.job_id = $2 AND p.owner_id = $3 \
             ORDER BY c.recorded_at ASC, c.id ASC",
            true,
        ),
        ErpScope::All => (
            "SELECT c.id, c.job_id, c.direction, c.item_ref, c.quantity, c.disposition, c.evidence_ref, c.recorded_by, \
                    to_char(c.recorded_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at \
             FROM archava_fulfillment.job_lines c \
             JOIN archava_fulfillment.jobs p ON p.tenant_id = c.tenant_id AND p.id = c.job_id \
             WHERE c.tenant_id = $1 AND c.job_id = $2 \
             ORDER BY c.recorded_at ASC, c.id ASC",
            false,
        ),
    };

    let rows = if is_assigned {
        sqlx::query(query_str)
            .bind(tenant)
            .bind(job_uuid)
            .bind(&access.owner_id)
            .fetch_all(&mut *connection)
            .await
            .map_err(database)?
    } else {
        sqlx::query(query_str)
            .bind(tenant)
            .bind(job_uuid)
            .fetch_all(&mut *connection)
            .await
            .map_err(database)?
    };

    let mut lines = Vec::with_capacity(rows.len());
    for lrow in rows {
        let lid: Uuid = lrow.try_get("id").map_err(database)?;
        let ljid: Uuid = lrow.try_get("job_id").map_err(database)?;
        let dir_str: String = lrow.try_get("direction").map_err(database)?;
        let disp_str: String = lrow.try_get("disposition").map_err(database)?;
        lines.push(JobLine {
            id: lid.to_string(),
            tenant_id: tenant.to_string(),
            job_id: ljid.to_string(),
            direction: LineDirection::parse(&dir_str).ok_or(FulfillmentError::Invalid)?,
            item_ref: lrow.try_get("item_ref").map_err(database)?,
            quantity: lrow.try_get("quantity").map_err(database)?,
            disposition: LineDisposition::parse(&disp_str).ok_or(FulfillmentError::Invalid)?,
            evidence_ref: lrow.try_get("evidence_ref").map_err(database)?,
            recorded_by: lrow.try_get("recorded_by").map_err(database)?,
            recorded_at: lrow.try_get("recorded_at").map_err(database)?,
        });
    }
    Ok(lines)
}

pub async fn list_fulfillment_findings(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    job_id: &str,
) -> Result<Vec<Finding>, FulfillmentError> {
    let access = resolve_fulfillment_context(connection, tenant, principal).await?;
    require_permission(&access, "fulfillment.read")?;
    let job_uuid = uuid_of(job_id)?;

    let (query_str, is_assigned) = match access.scope {
        ErpScope::Assigned => (
            "SELECT c.id, c.job_id, c.category, c.disposition, c.quantity, c.reason, c.evidence_ref, c.reconciles_id, c.recorded_by, \
                    to_char(c.recorded_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at \
             FROM archava_fulfillment.findings c \
             JOIN archava_fulfillment.jobs p ON p.tenant_id = c.tenant_id AND p.id = c.job_id \
             WHERE c.tenant_id = $1 AND c.job_id = $2 AND p.owner_id = $3 \
             ORDER BY c.recorded_at ASC, c.id ASC",
            true,
        ),
        ErpScope::All => (
            "SELECT c.id, c.job_id, c.category, c.disposition, c.quantity, c.reason, c.evidence_ref, c.reconciles_id, c.recorded_by, \
                    to_char(c.recorded_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at \
             FROM archava_fulfillment.findings c \
             JOIN archava_fulfillment.jobs p ON p.tenant_id = c.tenant_id AND p.id = c.job_id \
             WHERE c.tenant_id = $1 AND c.job_id = $2 \
             ORDER BY c.recorded_at ASC, c.id ASC",
            false,
        ),
    };

    let rows = if is_assigned {
        sqlx::query(query_str)
            .bind(tenant)
            .bind(job_uuid)
            .bind(&access.owner_id)
            .fetch_all(&mut *connection)
            .await
            .map_err(database)?
    } else {
        sqlx::query(query_str)
            .bind(tenant)
            .bind(job_uuid)
            .fetch_all(&mut *connection)
            .await
            .map_err(database)?
    };

    let mut findings = Vec::with_capacity(rows.len());
    for frow in rows {
        let fid: Uuid = frow.try_get("id").map_err(database)?;
        let fjid: Uuid = frow.try_get("job_id").map_err(database)?;
        let cat_str: String = frow.try_get("category").map_err(database)?;
        let disp_str: String = frow.try_get("disposition").map_err(database)?;
        let reconciles: Option<Uuid> = frow.try_get("reconciles_id").map_err(database)?;
        findings.push(Finding {
            id: fid.to_string(),
            tenant_id: tenant.to_string(),
            job_id: fjid.to_string(),
            category: FindingCategory::parse(&cat_str).ok_or(FulfillmentError::Invalid)?,
            disposition: FindingDisposition::parse(&disp_str).ok_or(FulfillmentError::Invalid)?,
            quantity: frow.try_get("quantity").map_err(database)?,
            reason: frow.try_get("reason").map_err(database)?,
            evidence_ref: frow.try_get("evidence_ref").map_err(database)?,
            reconciles_id: reconciles.map(|u| u.to_string()),
            recorded_by: frow.try_get("recorded_by").map_err(database)?,
            recorded_at: frow.try_get("recorded_at").map_err(database)?,
        });
    }
    Ok(findings)
}

pub async fn list_fulfillment_custody(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    job_id: &str,
) -> Result<Vec<CustodyRecord>, FulfillmentError> {
    let access = resolve_fulfillment_context(connection, tenant, principal).await?;
    require_permission(&access, "fulfillment.read")?;
    let job_uuid = uuid_of(job_id)?;

    let (query_str, is_assigned) = match access.scope {
        ErpScope::Assigned => (
            "SELECT c.id, c.job_id, c.event, c.from_ref, c.to_ref, c.evidence_ref, c.note, c.recorded_by, \
                    to_char(c.recorded_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at \
             FROM archava_fulfillment.custody_events c \
             JOIN archava_fulfillment.jobs p ON p.tenant_id = c.tenant_id AND p.id = c.job_id \
             WHERE c.tenant_id = $1 AND c.job_id = $2 AND p.owner_id = $3 \
             ORDER BY c.recorded_at ASC, c.id ASC",
            true,
        ),
        ErpScope::All => (
            "SELECT c.id, c.job_id, c.event, c.from_ref, c.to_ref, c.evidence_ref, c.note, c.recorded_by, \
                    to_char(c.recorded_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at \
             FROM archava_fulfillment.custody_events c \
             JOIN archava_fulfillment.jobs p ON p.tenant_id = c.tenant_id AND p.id = c.job_id \
             WHERE c.tenant_id = $1 AND c.job_id = $2 \
             ORDER BY c.recorded_at ASC, c.id ASC",
            false,
        ),
    };

    let rows = if is_assigned {
        sqlx::query(query_str)
            .bind(tenant)
            .bind(job_uuid)
            .bind(&access.owner_id)
            .fetch_all(&mut *connection)
            .await
            .map_err(database)?
    } else {
        sqlx::query(query_str)
            .bind(tenant)
            .bind(job_uuid)
            .fetch_all(&mut *connection)
            .await
            .map_err(database)?
    };

    let mut custody = Vec::with_capacity(rows.len());
    for crow in rows {
        let cid: Uuid = crow.try_get("id").map_err(database)?;
        let cjid: Uuid = crow.try_get("job_id").map_err(database)?;
        let ev_str: String = crow.try_get("event").map_err(database)?;
        custody.push(CustodyRecord {
            id: cid.to_string(),
            tenant_id: tenant.to_string(),
            job_id: cjid.to_string(),
            event: CustodyEvent::parse(&ev_str).ok_or(FulfillmentError::Invalid)?,
            from_ref: crow.try_get("from_ref").map_err(database)?,
            to_ref: crow.try_get("to_ref").map_err(database)?,
            evidence_ref: crow.try_get("evidence_ref").map_err(database)?,
            note: crow.try_get("note").map_err(database)?,
            recorded_by: crow.try_get("recorded_by").map_err(database)?,
            recorded_at: crow.try_get("recorded_at").map_err(database)?,
        });
    }
    Ok(custody)
}
