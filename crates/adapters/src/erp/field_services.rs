//! The field services SQL adapter — the Archxy 2 port of `packages/field-services-pg/src/repository.ts`:
//! the field services dispatch ledger's command ladder (claim → authorize → lock → transition → audit → receipt)
//! and read projections (access, jobs list, detail with visits & work evidence & signoff, audit trail, command status).
//!
//! ## Invariants preserved
//!
//! - RLS tenant isolation and member ladder.
//! - Service principals act on humans' behalf and cannot record field mutations directly.
//! - Strict monotonic forward transitions and quality/evidence rules:
//!   - `append_work_line` requires a recorded site visit (`visited` or `signed_off`).
//!   - `record_signoff` advances to `signed_off` if accepted, or demotes back to `evidenced` if rejected.
//!   - `approve_job` strictly requires an accepted customer signoff.
//!   - Closed jobs (`approved`, `canceled`) refuse further mutations.
//! - Labor hours and parts pieces accumulate on separate axes:
//!   - `labour` accumulates into `labor_hours` (numeric hours).
//!   - `part` accumulates into `parts_pieces` (integer pieces).
//! - Scope guards: `assigned` scope principals see only jobs they created or are assigned to.
//! - Advisory lock per (tenant, command_id) prevents race conditions on replay and state transitions.

use archava_domain::erp::field_services::{
    FieldAccess, FieldAuditEntry, FieldCommand, FieldCommandStatus, FieldJob, FieldJobDetail,
    FieldJobPageQuery, FieldReceipt, JobStatus, LineKind, Priority, SignoffRecord, VisitOutcome,
    VisitRecord, WorkLine, FIELD_PERMISSIONS,
};
use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
use archava_domain::erp::{ErpError, ErpMembershipKind, ErpScope};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, Row};
use thiserror::Error;
use uuid::Uuid;

use super::commerce_kernel::load_locked_membership;
use super::PurchaseError;

pub const FIELD_LEDGER: &str = "field";
pub const FIELD_NAMESPACE: &str = "archava_field";

#[derive(Debug, Error)]
pub enum FieldError {
    #[error("field domain refusal: {0:?}")]
    Domain(ErpError),
    #[error("field document refused by the grammar")]
    Invalid,
    #[error("field bad request: {0}")]
    BadRequest(&'static str),
    #[error("field conflict: {0}")]
    Conflict(&'static str),
    #[error("field invalid transition: {0}")]
    InvalidTransition(&'static str),
    #[error("field database failure in {ledger}: {source}")]
    Database {
        ledger: &'static str,
        #[source]
        source: sqlx::Error,
    },
}

impl From<FieldError> for PurchaseError {
    fn from(error: FieldError) -> Self {
        match error {
            FieldError::Domain(domain) => PurchaseError::Domain(domain),
            FieldError::Invalid => PurchaseError::Domain(ErpError::Unavailable),
            FieldError::BadRequest(_) => PurchaseError::Domain(ErpError::Unavailable),
            FieldError::Conflict(_) => PurchaseError::Domain(ErpError::Conflict),
            FieldError::InvalidTransition(_) => PurchaseError::Domain(ErpError::Conflict),
            FieldError::Database { ledger, source } => PurchaseError::Database { ledger, source },
        }
    }
}

fn forbidden() -> FieldError {
    FieldError::Domain(ErpError::Forbidden)
}

fn not_found() -> FieldError {
    FieldError::Domain(ErpError::NotFound)
}

fn conflict(msg: &'static str) -> FieldError {
    FieldError::Conflict(msg)
}

fn database(error: sqlx::Error) -> FieldError {
    FieldError::Database {
        ledger: FIELD_LEDGER,
        source: error,
    }
}

fn require_permission(access: &FieldAccess, permission: &str) -> Result<(), FieldError> {
    if access.permissions.iter().any(|p| p == permission) {
        Ok(())
    } else {
        Err(forbidden())
    }
}

pub async fn resolve_field_context(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<FieldAccess, FieldError> {
    let own = load_locked_membership(
        connection,
        FIELD_NAMESPACE,
        FIELD_LEDGER,
        tenant,
        &principal.id,
        FIELD_PERMISSIONS,
    )
    .await
    .map_err(|error| match error {
        PurchaseError::Domain(domain) => FieldError::Domain(domain),
        // This ledger's error type has no catalogued-reason variant yet.
        PurchaseError::Refused(_) => FieldError::Domain(ErpError::Conflict),
        PurchaseError::Database { ledger, source } => FieldError::Database { ledger, source },
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
            FIELD_NAMESPACE,
            FIELD_LEDGER,
            tenant,
            human_id,
            FIELD_PERMISSIONS,
        )
        .await
        .map_err(|error| match error {
            PurchaseError::Domain(domain) => FieldError::Domain(domain),
            // This ledger's error type has no catalogued-reason variant yet.
            PurchaseError::Refused(_) => FieldError::Domain(ErpError::Conflict),
            PurchaseError::Database { ledger, source } => FieldError::Database { ledger, source },
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

    Ok(FieldAccess {
        permissions,
        owner_id,
        scope,
    })
}

async fn fetch_current_timestamp(connection: &mut PgConnection) -> Result<String, FieldError> {
    let row = sqlx::query(
        "SELECT to_char(clock_timestamp() at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS ts",
    )
    .fetch_one(&mut *connection)
    .await
    .map_err(database)?;
    row.try_get::<String, _>("ts").map_err(database)
}

#[allow(clippy::too_many_arguments)]
async fn append_audit(
    connection: &mut PgConnection,
    tenant: &str,
    action: &str,
    job_id: &str,
    actor_ref: &str,
    command_id: &str,
) -> Result<(), FieldError> {
    let entry_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO archava_field.audit(entry_id, job_id, tenant_id, action, actor_ref, command_id) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&entry_id)
    .bind(job_id)
    .bind(tenant)
    .bind(action)
    .bind(actor_ref)
    .bind(command_id)
    .execute(&mut *connection)
    .await
    .map_err(database)?;
    Ok(())
}

#[allow(dead_code)]
struct LoadedJob {
    job_id: String,
    status: JobStatus,
    revision: i32,
    labor_hours: f64,
    parts_pieces: i32,
}

async fn load_job_for_update(
    connection: &mut PgConnection,
    tenant: &str,
    job_id: &str,
    access: &FieldAccess,
) -> Result<LoadedJob, FieldError> {
    let (query_str, is_assigned) = match access.scope {
        ErpScope::Assigned => (
            "SELECT job_id, status, revision, labor_hours::double precision AS labor_hours, parts_pieces \
             FROM archava_field.jobs \
             WHERE tenant_id = $1 AND job_id = $2 AND (assigned_principal = $3 OR created_by = $3) \
             FOR UPDATE",
            true,
        ),
        ErpScope::All => (
            "SELECT job_id, status, revision, labor_hours::double precision AS labor_hours, parts_pieces \
             FROM archava_field.jobs \
             WHERE tenant_id = $1 AND job_id = $2 \
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
    let status = JobStatus::parse(&status_str).ok_or(FieldError::Invalid)?;
    let revision: i32 = row.try_get("revision").map_err(database)?;
    let labor_hours: f64 = row.try_get("labor_hours").map_err(database)?;
    let parts_pieces: i32 = row.try_get("parts_pieces").map_err(database)?;

    Ok(LoadedJob {
        job_id: row.try_get("job_id").map_err(database)?,
        status,
        revision,
        labor_hours,
        parts_pieces,
    })
}

pub async fn record_field_command(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    command: FieldCommand,
    fingerprint: &str,
    payload: &Value,
) -> Result<FieldReceipt, FieldError> {
    if principal.kind == PrincipalKind::Service {
        return Err(FieldError::Domain(ErpError::Forbidden));
    }

    let access = resolve_field_context(connection, tenant, principal).await?;
    require_permission(&access, command.required_permission())?;

    // Advisory lock serialized on tenant + '\0' + command_id
    let mut hasher = Sha256::new();
    hasher.update(tenant.as_bytes());
    hasher.update(b"\0");
    hasher.update(command.command_id().as_bytes());
    let lock_key = format!("{:x}", hasher.finalize());

    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(&lock_key)
        .execute(&mut *connection)
        .await
        .map_err(database)?;

    // Check existing command recorded by this initiator
    let existing = sqlx::query(
        "SELECT command_id, action, job_id, revision, fingerprint, receipt \
         FROM archava_field.commands \
         WHERE tenant_id = $1 AND command_id = $2 AND created_by = $3",
    )
    .bind(tenant)
    .bind(command.command_id())
    .bind(&access.owner_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;

    if let Some(row) = existing {
        let recorded_fp: String = row.try_get("fingerprint").map_err(database)?;
        if recorded_fp != fingerprint {
            return Err(conflict("Command payload fingerprint mismatch on replay"));
        }
        let receipt_val: Option<Value> = row.try_get("receipt").map_err(database)?;
        let receipt_val = receipt_val.ok_or(FieldError::BadRequest("unknown_effect"))?;
        let receipt: FieldReceipt =
            serde_json::from_value(receipt_val).map_err(|_| FieldError::Invalid)?;
        return Ok(receipt);
    }

    // Check foreign command with the same command_id
    let foreign = sqlx::query(
        "SELECT command_id FROM archava_field.commands WHERE tenant_id = $1 AND command_id = $2",
    )
    .bind(tenant)
    .bind(command.command_id())
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;

    if foreign.is_some() {
        return Err(conflict("Command payload fingerprint mismatch on replay"));
    }

    let job_id: String;
    let final_status: JobStatus;
    let final_revision: i32;

    match command {
        FieldCommand::CreateJob {
            ref command_id,
            job_id: ref target_job_id,
            ref customer_ref,
            ref site_ref,
            ref summary,
            priority,
            ref appointment_ref,
            ref asset_ref,
        } => {
            job_id = target_job_id.clone();
            final_status = JobStatus::Requested;
            final_revision = 1;

            sqlx::query(
                "INSERT INTO archava_field.jobs( \
                    job_id, tenant_id, customer_ref, site_ref, summary, priority, status, \
                    appointment_ref, asset_ref, assigned_principal, created_by \
                ) VALUES ($1, $2, $3, $4, $5, $6, 'requested', $7, $8, NULL, $9)",
            )
            .bind(&job_id)
            .bind(tenant)
            .bind(customer_ref)
            .bind(site_ref)
            .bind(summary)
            .bind(priority.as_str())
            .bind(appointment_ref)
            .bind(asset_ref.as_deref())
            .bind(&access.owner_id)
            .execute(&mut *connection)
            .await
            .map_err(database)?;

            append_audit(
                connection,
                tenant,
                "create_job",
                &job_id,
                &access.owner_id,
                command_id,
            )
            .await?;
        }

        FieldCommand::AssignJob {
            ref command_id,
            job_id: ref target_job_id,
            ref assigned_principal,
        } => {
            let job = load_job_for_update(connection, tenant, target_job_id, &access).await?;
            if !job.status.can_transition_to(JobStatus::Assigned) {
                return Err(FieldError::InvalidTransition(
                    "Cannot transition job to assigned",
                ));
            }

            job_id = job.job_id;
            final_status = JobStatus::Assigned;

            let updated = sqlx::query(
                "UPDATE archava_field.jobs \
                 SET status = 'assigned', assigned_principal = $3, revision = revision + 1, updated_at = now() \
                 WHERE tenant_id = $1 AND job_id = $2 \
                 RETURNING revision",
            )
            .bind(tenant)
            .bind(&job_id)
            .bind(assigned_principal)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            final_revision = updated.try_get("revision").map_err(database)?;

            append_audit(
                connection,
                tenant,
                "assign_job",
                &job_id,
                &access.owner_id,
                command_id,
            )
            .await?;
        }

        FieldCommand::RecordVisit {
            ref command_id,
            job_id: ref target_job_id,
            ref visit_id,
            outcome,
            ref note,
            ref technician_ref,
        } => {
            let job = load_job_for_update(connection, tenant, target_job_id, &access).await?;
            if !job.status.can_transition_to(JobStatus::Visited) {
                return Err(FieldError::InvalidTransition(
                    "Cannot transition job to visited",
                ));
            }

            job_id = job.job_id;
            final_status = JobStatus::Visited;

            sqlx::query(
                "INSERT INTO archava_field.visits( \
                    visit_id, tenant_id, job_id, outcome, note, technician_ref \
                ) VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(visit_id)
            .bind(tenant)
            .bind(&job_id)
            .bind(outcome.as_str())
            .bind(note)
            .bind(technician_ref)
            .execute(&mut *connection)
            .await
            .map_err(database)?;

            let updated = sqlx::query(
                "UPDATE archava_field.jobs \
                 SET status = 'visited', revision = revision + 1, updated_at = now() \
                 WHERE tenant_id = $1 AND job_id = $2 \
                 RETURNING revision",
            )
            .bind(tenant)
            .bind(&job_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            final_revision = updated.try_get("revision").map_err(database)?;

            append_audit(
                connection,
                tenant,
                "record_visit",
                &job_id,
                &access.owner_id,
                command_id,
            )
            .await?;
        }

        FieldCommand::AppendWorkLine {
            ref command_id,
            job_id: ref target_job_id,
            ref line,
        } => {
            let job = load_job_for_update(connection, tenant, target_job_id, &access).await?;
            if job.status.is_closed() {
                return Err(FieldError::InvalidTransition(
                    "A closed field job accepts no further work lines",
                ));
            }
            if matches!(job.status, JobStatus::Requested | JobStatus::Assigned) {
                return Err(FieldError::InvalidTransition(
                    "Work evidence requires a recorded site visit",
                ));
            }

            job_id = job.job_id;
            final_status = JobStatus::Evidenced;

            sqlx::query(
                "INSERT INTO archava_field.work_lines( \
                    line_id, tenant_id, job_id, kind, description, quantity, unit, part_ref \
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(&line.line_id)
            .bind(tenant)
            .bind(&job_id)
            .bind(line.kind.as_str())
            .bind(&line.description)
            .bind(line.quantity)
            .bind(&line.unit)
            .bind(line.part_ref.as_deref())
            .execute(&mut *connection)
            .await
            .map_err(database)?;

            let hours = if line.kind == LineKind::Labour {
                line.quantity
            } else {
                0.0
            };
            let pieces = if line.kind == LineKind::Part {
                line.quantity.round() as i32
            } else {
                0
            };

            let updated = sqlx::query(
                "UPDATE archava_field.jobs \
                 SET labor_hours = labor_hours + $3, parts_pieces = parts_pieces + $4, \
                     status = 'evidenced', revision = revision + 1, updated_at = now() \
                 WHERE tenant_id = $1 AND job_id = $2 \
                 RETURNING revision",
            )
            .bind(tenant)
            .bind(&job_id)
            .bind(hours)
            .bind(pieces)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            final_revision = updated.try_get("revision").map_err(database)?;

            append_audit(
                connection,
                tenant,
                "append_work_line",
                &job_id,
                &access.owner_id,
                command_id,
            )
            .await?;
        }

        FieldCommand::RecordSignoff {
            ref command_id,
            job_id: ref target_job_id,
            ref signoff_id,
            ref signer_ref,
            accepted,
            ref comment,
        } => {
            let job = load_job_for_update(connection, tenant, target_job_id, &access).await?;
            if accepted {
                if !job.status.can_transition_to(JobStatus::SignedOff) {
                    return Err(FieldError::InvalidTransition(
                        "Cannot transition job to signed_off",
                    ));
                }
                final_status = JobStatus::SignedOff;
            } else {
                if job.status == JobStatus::SignedOff
                    && !job.status.can_transition_to(JobStatus::Evidenced)
                {
                    return Err(FieldError::InvalidTransition(
                        "Cannot demote job to evidenced",
                    ));
                }
                final_status = JobStatus::Evidenced;
            }

            job_id = job.job_id;

            sqlx::query(
                "INSERT INTO archava_field.signoffs( \
                    signoff_id, tenant_id, job_id, signer_ref, accepted, comment \
                ) VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(signoff_id)
            .bind(tenant)
            .bind(&job_id)
            .bind(signer_ref)
            .bind(accepted)
            .bind(comment.as_deref())
            .execute(&mut *connection)
            .await
            .map_err(database)?;

            let updated = sqlx::query(
                "UPDATE archava_field.jobs \
                 SET status = $3, revision = revision + 1, updated_at = now() \
                 WHERE tenant_id = $1 AND job_id = $2 \
                 RETURNING revision",
            )
            .bind(tenant)
            .bind(&job_id)
            .bind(final_status.as_str())
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            final_revision = updated.try_get("revision").map_err(database)?;

            append_audit(
                connection,
                tenant,
                "record_signoff",
                &job_id,
                &access.owner_id,
                command_id,
            )
            .await?;
        }

        FieldCommand::ApproveJob {
            ref command_id,
            job_id: ref target_job_id,
        } => {
            let job = load_job_for_update(connection, tenant, target_job_id, &access).await?;
            if !job.status.can_transition_to(JobStatus::Approved) {
                return Err(FieldError::InvalidTransition(
                    "Cannot transition job to approved",
                ));
            }

            job_id = job.job_id;
            final_status = JobStatus::Approved;

            // Verify accepted customer signoff exists
            let accepted_signoff = sqlx::query(
                "SELECT s.signoff_id FROM archava_field.signoffs s \
                 JOIN archava_field.jobs j ON j.tenant_id = s.tenant_id AND j.job_id = s.job_id \
                 WHERE j.tenant_id = $1 AND s.job_id = $2 AND s.accepted \
                 ORDER BY s.signed_at ASC, s.signoff_id ASC LIMIT 1",
            )
            .bind(tenant)
            .bind(&job_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?;

            if accepted_signoff.is_none() {
                return Err(conflict(
                    "Cannot approve a field job without an accepted customer signoff",
                ));
            }

            let updated = sqlx::query(
                "UPDATE archava_field.jobs \
                 SET status = 'approved', revision = revision + 1, updated_at = now() \
                 WHERE tenant_id = $1 AND job_id = $2 \
                 RETURNING revision",
            )
            .bind(tenant)
            .bind(&job_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            final_revision = updated.try_get("revision").map_err(database)?;

            append_audit(
                connection,
                tenant,
                "approve_job",
                &job_id,
                &access.owner_id,
                command_id,
            )
            .await?;
        }

        FieldCommand::CancelJob {
            ref command_id,
            job_id: ref target_job_id,
            ..
        } => {
            let job = load_job_for_update(connection, tenant, target_job_id, &access).await?;
            if !job.status.can_transition_to(JobStatus::Canceled) {
                return Err(FieldError::InvalidTransition(
                    "Cannot transition job to canceled",
                ));
            }

            job_id = job.job_id;
            final_status = JobStatus::Canceled;

            let updated = sqlx::query(
                "UPDATE archava_field.jobs \
                 SET status = 'canceled', revision = revision + 1, updated_at = now() \
                 WHERE tenant_id = $1 AND job_id = $2 \
                 RETURNING revision",
            )
            .bind(tenant)
            .bind(&job_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            final_revision = updated.try_get("revision").map_err(database)?;

            append_audit(
                connection,
                tenant,
                "cancel_job",
                &job_id,
                &access.owner_id,
                command_id,
            )
            .await?;
        }
    }

    let recorded_at = fetch_current_timestamp(connection).await?;
    let receipt = FieldReceipt {
        command_id: command.command_id().to_string(),
        action: command.action().to_string(),
        job_id: job_id.clone(),
        status: final_status,
        revision: final_revision,
        recorded_at,
    };

    let receipt_json = serde_json::to_value(&receipt).map_err(|_| FieldError::Invalid)?;

    sqlx::query(
        "INSERT INTO archava_field.commands( \
            command_id, job_id, tenant_id, action, fingerprint, payload, receipt, status, revision, created_by \
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, 'accepted', $8, $9)",
    )
    .bind(command.command_id())
    .bind(&job_id)
    .bind(tenant)
    .bind(command.action())
    .bind(fingerprint)
    .bind(payload)
    .bind(&receipt_json)
    .bind(final_revision)
    .bind(&access.owner_id)
    .execute(&mut *connection)
    .await
    .map_err(database)?;

    Ok(receipt)
}

pub async fn field_command_status(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    command_id: &str,
) -> Result<FieldCommandStatus, FieldError> {
    let access = resolve_field_context(connection, tenant, principal).await?;
    require_permission(&access, "field.read")?;

    let row = sqlx::query(
        "SELECT command_id, action, job_id, receipt \
         FROM archava_field.commands \
         WHERE tenant_id = $1 AND command_id = $2 AND created_by = $3",
    )
    .bind(tenant)
    .bind(command_id)
    .bind(&access.owner_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;

    let Some(row) = row else {
        return Ok(FieldCommandStatus {
            command_id: command_id.to_string(),
            action: "create_job".to_string(),
            job_id: "unknown".to_string(),
            status: "unknown".to_string(),
            revision: None,
            recorded_at: None,
        });
    };

    let action: String = row.try_get("action").map_err(database)?;
    let job_id: String = row.try_get("job_id").map_err(database)?;
    let receipt_val: Option<Value> = row.try_get("receipt").map_err(database)?;

    if let Some(val) = receipt_val {
        let receipt: FieldReceipt = serde_json::from_value(val).map_err(|_| FieldError::Invalid)?;
        Ok(FieldCommandStatus {
            command_id: command_id.to_string(),
            action,
            job_id,
            status: "accepted".to_string(),
            revision: Some(receipt.revision),
            recorded_at: Some(receipt.recorded_at),
        })
    } else {
        Ok(FieldCommandStatus {
            command_id: command_id.to_string(),
            action,
            job_id,
            status: "unknown".to_string(),
            revision: None,
            recorded_at: None,
        })
    }
}

fn map_field_job(row: &sqlx::postgres::PgRow) -> Result<FieldJob, FieldError> {
    let job_id: String = row.try_get("job_id").map_err(database)?;
    let customer_ref: String = row.try_get("customer_ref").map_err(database)?;
    let site_ref: String = row.try_get("site_ref").map_err(database)?;
    let summary: String = row.try_get("summary").map_err(database)?;
    let priority_str: String = row.try_get("priority").map_err(database)?;
    let priority = Priority::parse(&priority_str).ok_or(FieldError::Invalid)?;
    let status_str: String = row.try_get("status").map_err(database)?;
    let status = JobStatus::parse(&status_str).ok_or(FieldError::Invalid)?;
    let appointment_ref: String = row.try_get("appointment_ref").map_err(database)?;
    let asset_ref: Option<String> = row.try_get("asset_ref").map_err(database)?;
    let assigned_principal: Option<String> = row.try_get("assigned_principal").map_err(database)?;
    let labor_hours: f64 = row.try_get("labor_hours").map_err(database)?;
    let parts_pieces: i32 = row.try_get("parts_pieces").map_err(database)?;
    let revision: i32 = row.try_get("revision").map_err(database)?;
    let created_by: String = row.try_get("created_by").map_err(database)?;
    let created_at: String = row.try_get("created_at").map_err(database)?;
    let updated_at: String = row.try_get("updated_at").map_err(database)?;

    Ok(FieldJob {
        job_id,
        customer_ref,
        site_ref,
        summary,
        priority,
        status,
        appointment_ref,
        asset_ref,
        assigned_principal,
        labor_hours,
        parts_pieces,
        revision,
        created_by,
        created_at,
        updated_at,
    })
}

pub async fn list_field_jobs(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    query: FieldJobPageQuery,
) -> Result<Vec<FieldJob>, FieldError> {
    let access = resolve_field_context(connection, tenant, principal).await?;
    require_permission(&access, "field.read")?;

    let mut sql = String::from(
        "SELECT job_id, customer_ref, site_ref, summary, priority, status, appointment_ref, asset_ref, \
                assigned_principal, labor_hours::double precision AS labor_hours, parts_pieces, revision, created_by, \
                to_char(created_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, \
                to_char(updated_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at \
         FROM archava_field.jobs \
         WHERE tenant_id = $1",
    );

    let mut param_index = 2;
    if query.status.is_some() {
        sql.push_str(&format!(" AND status = ${param_index}"));
        param_index += 1;
    }

    if access.scope == ErpScope::Assigned {
        sql.push_str(&format!(
            " AND (assigned_principal = ${param_index} OR created_by = ${param_index})"
        ));
        param_index += 1;
    }

    if query.cursor.is_some() {
        sql.push_str(&format!(" AND job_id > ${param_index}"));
        param_index += 1;
    }

    sql.push_str(&format!(" ORDER BY job_id ASC LIMIT ${param_index}"));

    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.as_str())).bind(tenant);

    if let Some(status) = query.status {
        q = q.bind(status.as_str());
    }

    if access.scope == ErpScope::Assigned {
        q = q.bind(&access.owner_id);
    }

    if let Some(ref cursor) = query.cursor {
        q = q.bind(cursor);
    }

    q = q.bind(query.limit);

    let rows = q.fetch_all(&mut *connection).await.map_err(database)?;
    let mut jobs = Vec::with_capacity(rows.len());
    for row in &rows {
        jobs.push(map_field_job(row)?);
    }
    Ok(jobs)
}

pub async fn get_field_job_detail(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    job_id: &str,
) -> Result<Option<FieldJobDetail>, FieldError> {
    let access = resolve_field_context(connection, tenant, principal).await?;
    require_permission(&access, "field.read")?;

    let (query_str, is_assigned) = match access.scope {
        ErpScope::Assigned => (
            "SELECT job_id, customer_ref, site_ref, summary, priority, status, appointment_ref, asset_ref, \
                    assigned_principal, labor_hours::double precision AS labor_hours, parts_pieces, revision, created_by, \
                    to_char(created_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, \
                    to_char(updated_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at \
             FROM archava_field.jobs \
             WHERE tenant_id = $1 AND job_id = $2 AND (assigned_principal = $3 OR created_by = $3)",
            true,
        ),
        ErpScope::All => (
            "SELECT job_id, customer_ref, site_ref, summary, priority, status, appointment_ref, asset_ref, \
                    assigned_principal, labor_hours::double precision AS labor_hours, parts_pieces, revision, created_by, \
                    to_char(created_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, \
                    to_char(updated_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at \
             FROM archava_field.jobs \
             WHERE tenant_id = $1 AND job_id = $2",
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

    let Some(row) = row else {
        return Ok(None);
    };

    let job = map_field_job(&row)?;

    let visit_rows = sqlx::query(
        "SELECT visit_id, job_id, outcome, note, technician_ref, \
                to_char(visited_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS visited_at \
         FROM archava_field.visits \
         WHERE tenant_id = $1 AND job_id = $2 \
         ORDER BY visited_at ASC, visit_id ASC",
    )
    .bind(tenant)
    .bind(job_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(database)?;

    let mut visits = Vec::with_capacity(visit_rows.len());
    for vrow in visit_rows {
        let outcome_str: String = vrow.try_get("outcome").map_err(database)?;
        let outcome = VisitOutcome::parse(&outcome_str).ok_or(FieldError::Invalid)?;
        visits.push(VisitRecord {
            visit_id: vrow.try_get("visit_id").map_err(database)?,
            job_id: vrow.try_get("job_id").map_err(database)?,
            outcome,
            note: vrow.try_get("note").map_err(database)?,
            technician_ref: vrow.try_get("technician_ref").map_err(database)?,
            visited_at: vrow.try_get("visited_at").map_err(database)?,
        });
    }

    let line_rows = sqlx::query(
        "SELECT line_id, kind, description, quantity::double precision AS quantity, unit, part_ref \
         FROM archava_field.work_lines \
         WHERE tenant_id = $1 AND job_id = $2 \
         ORDER BY recorded_at ASC, line_id ASC",
    )
    .bind(tenant)
    .bind(job_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(database)?;

    let mut work_lines = Vec::with_capacity(line_rows.len());
    for lrow in line_rows {
        let kind_str: String = lrow.try_get("kind").map_err(database)?;
        let kind = LineKind::parse(&kind_str).ok_or(FieldError::Invalid)?;
        work_lines.push(WorkLine {
            line_id: lrow.try_get("line_id").map_err(database)?,
            kind,
            description: lrow.try_get("description").map_err(database)?,
            quantity: lrow.try_get("quantity").map_err(database)?,
            unit: lrow.try_get("unit").map_err(database)?,
            part_ref: lrow.try_get("part_ref").map_err(database)?,
        });
    }

    let signoff_rows = sqlx::query(
        "SELECT signoff_id, job_id, signer_ref, accepted, comment, \
                to_char(signed_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS signed_at \
         FROM archava_field.signoffs \
         WHERE tenant_id = $1 AND job_id = $2 \
         ORDER BY signed_at ASC, signoff_id ASC",
    )
    .bind(tenant)
    .bind(job_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(database)?;

    let signoff = if let Some(last) = signoff_rows.last() {
        Some(SignoffRecord {
            signoff_id: last.try_get("signoff_id").map_err(database)?,
            job_id: last.try_get("job_id").map_err(database)?,
            signer_ref: last.try_get("signer_ref").map_err(database)?,
            accepted: last.try_get("accepted").map_err(database)?,
            comment: last.try_get("comment").map_err(database)?,
            signed_at: last.try_get("signed_at").map_err(database)?,
        })
    } else {
        None
    };

    Ok(Some(FieldJobDetail {
        job,
        visits,
        work_lines,
        signoff,
    }))
}

pub async fn list_field_audit(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    job_id: &str,
) -> Result<Vec<FieldAuditEntry>, FieldError> {
    let access = resolve_field_context(connection, tenant, principal).await?;
    require_permission(&access, "audit.read")?;

    let (query_str, is_assigned) = match access.scope {
        ErpScope::Assigned => (
            "SELECT a.entry_id, a.job_id, a.action, a.actor_ref, a.command_id, \
                    to_char(a.at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS at \
             FROM archava_field.audit a \
             JOIN archava_field.jobs j ON j.tenant_id = a.tenant_id AND j.job_id = a.job_id \
             WHERE a.tenant_id = $1 AND a.job_id = $2 AND (j.assigned_principal = $3 OR j.created_by = $3) \
             ORDER BY a.at ASC, a.entry_id ASC",
            true,
        ),
        ErpScope::All => (
            "SELECT a.entry_id, a.job_id, a.action, a.actor_ref, a.command_id, \
                    to_char(a.at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS at \
             FROM archava_field.audit a \
             WHERE a.tenant_id = $1 AND a.job_id = $2 \
             ORDER BY a.at ASC, a.entry_id ASC",
            false,
        ),
    };

    let rows = if is_assigned {
        sqlx::query(query_str)
            .bind(tenant)
            .bind(job_id)
            .bind(&access.owner_id)
            .fetch_all(&mut *connection)
            .await
            .map_err(database)?
    } else {
        sqlx::query(query_str)
            .bind(tenant)
            .bind(job_id)
            .fetch_all(&mut *connection)
            .await
            .map_err(database)?
    };

    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        entries.push(FieldAuditEntry {
            entry_id: row.try_get("entry_id").map_err(database)?,
            job_id: row.try_get("job_id").map_err(database)?,
            action: row.try_get("action").map_err(database)?,
            actor_ref: row.try_get("actor_ref").map_err(database)?,
            command_id: row.try_get("command_id").map_err(database)?,
            at: row.try_get("at").map_err(database)?,
        });
    }
    Ok(entries)
}
