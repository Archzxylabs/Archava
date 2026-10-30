//! The SQLx adapter for `archava_edu` — formal education.
//!
//! Enforces:
//! - Non-owner runtime with Row Level Security on all tables.
//! - Member ladder & scope checks (`all`, `guardian`, `self`, `assigned_class`).
//! - SQL-backed visibility predicate `archava_edu.visible_enrollment`.
//! - Monotonic lifecycle transitions and score bounds.
//! - Replay-safe advisory locking and immutable audit trail.

use archava_domain::erp::formal_education::{
    AssessmentResult, EducationAccess, EducationAuditEntry, EducationCommand,
    EducationCommandStatus, EducationReceipt, EducationScope, Enrollment, EnrollmentDetail,
    EnrollmentPageQuery, EnrollmentStatus, ReviewRecord, SessionOutcome, SessionRecord,
};
use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, Row};

pub const EDU_NAMESPACE: &str = "archava_edu";
pub const EDU_LEDGER: &str = "Education";

pub const EDU_TABLES: &[&str] = &[
    "memberships",
    "enrollments",
    "sessions",
    "results",
    "reviews",
    "commands",
    "audit",
];

pub const EDU_PERMISSIONS: &[&str] = &[
    "education.read",
    "education.enroll",
    "education.record_session",
    "education.record_result",
    "education.review",
    "education.retain",
    "audit.read",
];

#[derive(Debug)]
pub enum FormalEducationError {
    Forbidden,
    NotFound,
    Conflict(String),
    IllegalTransition(String),
    ReviewRequired(String),
    UnknownEffect,
    ValidationFailed(String),
    Database {
        ledger: &'static str,
        source: sqlx::Error,
    },
}

impl std::fmt::Display for FormalEducationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Forbidden => write!(f, "Forbidden: insufficient permissions or scope"),
            Self::NotFound => write!(f, "Resource not found"),
            Self::Conflict(msg) => write!(f, "Conflict: {}", msg),
            Self::IllegalTransition(msg) => write!(f, "Illegal transition: {}", msg),
            Self::ReviewRequired(msg) => write!(f, "Review required: {}", msg),
            Self::UnknownEffect => write!(f, "Command effect is not yet known"),
            Self::ValidationFailed(msg) => write!(f, "Validation failed: {}", msg),
            Self::Database { ledger, source } => {
                write!(f, "Database error on {}: {}", ledger, source)
            }
        }
    }
}

impl std::error::Error for FormalEducationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn database(source: sqlx::Error) -> FormalEducationError {
    FormalEducationError::Database {
        ledger: EDU_LEDGER,
        source,
    }
}

fn forbidden() -> FormalEducationError {
    FormalEducationError::Forbidden
}

fn not_found() -> FormalEducationError {
    FormalEducationError::NotFound
}

fn require_permission(
    access: &EducationAccess,
    permission: &str,
) -> Result<(), FormalEducationError> {
    if access.permissions.iter().any(|p| p == permission) {
        Ok(())
    } else {
        Err(forbidden())
    }
}

pub async fn resolve_education_context(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<EducationAccess, FormalEducationError> {
    let row = sqlx::query(
        "SELECT principal_id, kind, permissions, scope, active \
         FROM archava_edu.lock_membership($1, $2)",
    )
    .bind(tenant)
    .bind(&principal.id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;

    let row = row.ok_or_else(forbidden)?;
    let active: bool = row.try_get("active").map_err(database)?;
    if !active {
        return Err(forbidden());
    }

    let kind_str: String = row.try_get("kind").map_err(database)?;
    let principal_kind_str = match principal.kind {
        PrincipalKind::Human => "human",
        PrincipalKind::Service => "service",
    };
    if kind_str != principal_kind_str {
        return Err(forbidden());
    }

    let permissions: Vec<String> = row.try_get("permissions").map_err(database)?;
    let scope_str: String = row.try_get("scope").map_err(database)?;
    let scope = EducationScope::parse(&scope_str).ok_or_else(forbidden)?;

    Ok(EducationAccess {
        permissions,
        owner_id: principal.id.clone(),
        scope,
    })
}

fn map_enrollment_row(r: &sqlx::postgres::PgRow) -> Result<Enrollment, FormalEducationError> {
    let status_str: String = r.try_get("status").map_err(database)?;
    let status = EnrollmentStatus::parse(&status_str)
        .ok_or(FormalEducationError::ValidationFailed("status".to_string()))?;

    Ok(Enrollment {
        enrollment_id: r.try_get("enrollment_id").map_err(database)?,
        student_ref: r.try_get("student_ref").map_err(database)?,
        class_ref: r.try_get("class_ref").map_err(database)?,
        term_ref: r.try_get("term_ref").map_err(database)?,
        status,
        revision: r.try_get("revision").map_err(database)?,
        invoice_ref: r.try_get("invoice_ref").map_err(database)?,
        guardian_ref: r.try_get("guardian_ref").map_err(database)?,
        created_by: r.try_get("created_by").map_err(database)?,
        created_at: r.try_get("created_at").map_err(database)?,
        updated_at: r.try_get("updated_at").map_err(database)?,
    })
}

pub async fn execute_education_command(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    command: EducationCommand,
    fingerprint: &str,
) -> Result<EducationReceipt, FormalEducationError> {
    if principal.kind == PrincipalKind::Service {
        return Err(forbidden());
    }

    let access = resolve_education_context(connection, tenant, principal).await?;

    let required_permission = match &command {
        EducationCommand::AdmitApplicant { .. }
        | EducationCommand::ConfirmEnrollment { .. }
        | EducationCommand::EnrollStudent { .. }
        | EducationCommand::WithdrawStudent { .. } => "education.enroll",
        EducationCommand::RecordSession { .. } => "education.record_session",
        EducationCommand::RecordResult { .. } => "education.record_result",
        EducationCommand::ReviewRecord { .. } | EducationCommand::GraduateEnrollment { .. } => {
            "education.review"
        }
        EducationCommand::RecordRetention { .. } => "education.retain",
    };
    require_permission(&access, required_permission)?;

    // Advisory transaction lock on command key
    let lock_key = format!("{}\0{}", tenant, command.command_id());
    let lock_digest = format!("{:x}", Sha256::digest(lock_key.as_bytes()));
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(&lock_digest)
        .execute(&mut *connection)
        .await
        .map_err(database)?;

    // Check existing command
    let existing = sqlx::query(
        "SELECT command_id, action, enrollment_id, revision, fingerprint, receipt \
         FROM archava_edu.commands \
         WHERE tenant_id = $1 AND command_id = $2 AND created_by = $3",
    )
    .bind(tenant)
    .bind(command.command_id())
    .bind(&access.owner_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;

    if let Some(row) = existing {
        let stored_fp: String = row.try_get("fingerprint").map_err(database)?;
        if stored_fp != fingerprint {
            return Err(FormalEducationError::Conflict(
                "Command payload fingerprint mismatch on replay".to_string(),
            ));
        }
        let receipt_val: Option<serde_json::Value> = row.try_get("receipt").map_err(database)?;
        let receipt_val = receipt_val.ok_or(FormalEducationError::UnknownEffect)?;
        let receipt: EducationReceipt =
            serde_json::from_value(receipt_val).map_err(|_| FormalEducationError::UnknownEffect)?;
        return Ok(receipt);
    }

    let foreign = sqlx::query(
        "SELECT command_id FROM archava_edu.commands \
         WHERE tenant_id = $1 AND command_id = $2",
    )
    .bind(tenant)
    .bind(command.command_id())
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;

    if foreign.is_some() {
        return Err(FormalEducationError::Conflict(
            "Command payload fingerprint mismatch on replay".to_string(),
        ));
    }

    let enrollment_id = command.enrollment_id().to_string();

    let (status, revision) = match command {
        EducationCommand::AdmitApplicant {
            command_id: _,
            enrollment_id: ref enr_id,
            ref student_ref,
            ref class_ref,
            ref term_ref,
            ref guardian_ref,
        } => {
            sqlx::query(
                "INSERT INTO archava_edu.enrollments ( \
                 enrollment_id, tenant_id, student_ref, class_ref, term_ref, status, revision, guardian_ref, created_by \
                 ) VALUES ($1, $2, $3, $4, $5, 'applied', 1, $6, $7)",
            )
            .bind(enr_id)
            .bind(tenant)
            .bind(student_ref)
            .bind(class_ref)
            .bind(term_ref)
            .bind(guardian_ref)
            .bind(&access.owner_id)
            .execute(&mut *connection)
            .await
            .map_err(database)?;

            (EnrollmentStatus::Applied, 1)
        }

        EducationCommand::ConfirmEnrollment {
            command_id: _,
            ref enrollment_id,
        } => {
            let row = sqlx::query(
                "SELECT status, revision FROM archava_edu.enrollments \
                 WHERE tenant_id = $1 AND enrollment_id = $2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(enrollment_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
            .ok_or_else(not_found)?;

            let cur_status_str: String = row.try_get("status").map_err(database)?;
            let cur_status =
                EnrollmentStatus::parse(&cur_status_str).ok_or(FormalEducationError::NotFound)?;
            if !cur_status.can_transition_to(EnrollmentStatus::Admitted) {
                return Err(FormalEducationError::IllegalTransition(format!(
                    "Enrollment cannot move from {} to admitted",
                    cur_status_str
                )));
            }

            let updated = sqlx::query(
                "UPDATE archava_edu.enrollments SET status = 'admitted', revision = revision + 1, updated_at = now() \
                 WHERE tenant_id = $1 AND enrollment_id = $2 RETURNING revision, status",
            )
            .bind(tenant)
            .bind(enrollment_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            let rev: i32 = updated.try_get("revision").map_err(database)?;
            (EnrollmentStatus::Admitted, rev)
        }

        EducationCommand::EnrollStudent {
            command_id: _,
            ref enrollment_id,
        } => {
            let row = sqlx::query(
                "SELECT status, revision FROM archava_edu.enrollments \
                 WHERE tenant_id = $1 AND enrollment_id = $2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(enrollment_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
            .ok_or_else(not_found)?;

            let cur_status_str: String = row.try_get("status").map_err(database)?;
            let cur_status =
                EnrollmentStatus::parse(&cur_status_str).ok_or(FormalEducationError::NotFound)?;
            if !cur_status.can_transition_to(EnrollmentStatus::Enrolled) {
                return Err(FormalEducationError::IllegalTransition(format!(
                    "Enrollment cannot move from {} to enrolled",
                    cur_status_str
                )));
            }

            let updated = sqlx::query(
                "UPDATE archava_edu.enrollments SET status = 'enrolled', revision = revision + 1, updated_at = now() \
                 WHERE tenant_id = $1 AND enrollment_id = $2 RETURNING revision, status",
            )
            .bind(tenant)
            .bind(enrollment_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            let rev: i32 = updated.try_get("revision").map_err(database)?;
            (EnrollmentStatus::Enrolled, rev)
        }

        EducationCommand::RecordSession {
            command_id: _,
            ref enrollment_id,
            ref session_id,
            outcome,
            ref note,
            ref appointment_ref,
        } => {
            let row = sqlx::query(
                "SELECT status, revision FROM archava_edu.enrollments \
                 WHERE tenant_id = $1 AND enrollment_id = $2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(enrollment_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
            .ok_or_else(not_found)?;

            let cur_status_str: String = row.try_get("status").map_err(database)?;
            if !["enrolled", "attending", "reported"].contains(&cur_status_str.as_str()) {
                return Err(FormalEducationError::IllegalTransition(
                    "Attendance requires an enrolled or attending student".to_string(),
                ));
            }

            sqlx::query(
                "INSERT INTO archava_edu.sessions ( \
                 session_id, enrollment_id, tenant_id, outcome, note, appointment_ref, recorded_by \
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(session_id)
            .bind(enrollment_id)
            .bind(tenant)
            .bind(outcome.as_str())
            .bind(note)
            .bind(appointment_ref)
            .bind(&access.owner_id)
            .execute(&mut *connection)
            .await
            .map_err(database)?;

            if cur_status_str == "enrolled" {
                let updated = sqlx::query(
                    "UPDATE archava_edu.enrollments SET status = 'attending', revision = revision + 1, updated_at = now() \
                     WHERE tenant_id = $1 AND enrollment_id = $2 RETURNING revision, status",
                )
                .bind(tenant)
                .bind(enrollment_id)
                .fetch_one(&mut *connection)
                .await
                .map_err(database)?;

                let rev: i32 = updated.try_get("revision").map_err(database)?;
                (EnrollmentStatus::Attending, rev)
            } else {
                let cur_status = EnrollmentStatus::parse(&cur_status_str).unwrap();
                let rev: i32 = row.try_get("revision").map_err(database)?;
                (cur_status, rev)
            }
        }

        EducationCommand::RecordResult {
            command_id: _,
            ref enrollment_id,
            ref result_id,
            ref assessment_ref,
            score,
            out_of,
            ref letter_ref,
            ref note,
        } => {
            let row = sqlx::query(
                "SELECT status, revision FROM archava_edu.enrollments \
                 WHERE tenant_id = $1 AND enrollment_id = $2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(enrollment_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
            .ok_or_else(not_found)?;

            let cur_status_str: String = row.try_get("status").map_err(database)?;
            if !["attending", "reported"].contains(&cur_status_str.as_str()) {
                return Err(FormalEducationError::IllegalTransition(
                    "Results require a student who is attending".to_string(),
                ));
            }

            sqlx::query(
                "INSERT INTO archava_edu.results ( \
                 result_id, enrollment_id, tenant_id, assessment_ref, score, out_of, letter_ref, note, recorded_by \
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(result_id)
            .bind(enrollment_id)
            .bind(tenant)
            .bind(assessment_ref)
            .bind(score)
            .bind(out_of)
            .bind(letter_ref)
            .bind(note)
            .bind(&access.owner_id)
            .execute(&mut *connection)
            .await
            .map_err(database)?;

            if cur_status_str == "attending" {
                let updated = sqlx::query(
                    "UPDATE archava_edu.enrollments SET status = 'reported', revision = revision + 1, updated_at = now() \
                     WHERE tenant_id = $1 AND enrollment_id = $2 RETURNING revision, status",
                )
                .bind(tenant)
                .bind(enrollment_id)
                .fetch_one(&mut *connection)
                .await
                .map_err(database)?;

                let rev: i32 = updated.try_get("revision").map_err(database)?;
                (EnrollmentStatus::Reported, rev)
            } else {
                let cur_status = EnrollmentStatus::parse(&cur_status_str).unwrap();
                let rev: i32 = row.try_get("revision").map_err(database)?;
                (cur_status, rev)
            }
        }

        EducationCommand::ReviewRecord {
            command_id: _,
            ref enrollment_id,
            ref review_id,
            ref decision,
            ref note,
        } => {
            let row = sqlx::query(
                "SELECT status, revision FROM archava_edu.enrollments \
                 WHERE tenant_id = $1 AND enrollment_id = $2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(enrollment_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
            .ok_or_else(not_found)?;

            let cur_status_str: String = row.try_get("status").map_err(database)?;
            if cur_status_str != "reported" {
                return Err(FormalEducationError::ReviewRequired(
                    "Only a reported enrollment can be reviewed".to_string(),
                ));
            }

            sqlx::query(
                "INSERT INTO archava_edu.reviews ( \
                 review_id, enrollment_id, tenant_id, decision, note, reviewer_ref \
                 ) VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(review_id)
            .bind(enrollment_id)
            .bind(tenant)
            .bind(decision)
            .bind(note)
            .bind(&access.owner_id)
            .execute(&mut *connection)
            .await
            .map_err(database)?;

            let next_status = if decision == "approved" {
                EnrollmentStatus::Reviewed
            } else {
                EnrollmentStatus::Attending
            };

            let updated = sqlx::query(
                "UPDATE archava_edu.enrollments SET status = $1, revision = revision + 1, updated_at = now() \
                 WHERE tenant_id = $2 AND enrollment_id = $3 RETURNING revision, status",
            )
            .bind(next_status.as_str())
            .bind(tenant)
            .bind(enrollment_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            let rev: i32 = updated.try_get("revision").map_err(database)?;
            (next_status, rev)
        }

        EducationCommand::RecordRetention {
            command_id: _,
            ref enrollment_id,
        } => {
            let row = sqlx::query(
                "SELECT status, revision FROM archava_edu.enrollments \
                 WHERE tenant_id = $1 AND enrollment_id = $2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(enrollment_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
            .ok_or_else(not_found)?;

            let cur_status_str: String = row.try_get("status").map_err(database)?;
            let cur_status =
                EnrollmentStatus::parse(&cur_status_str).ok_or(FormalEducationError::NotFound)?;
            if !cur_status.can_transition_to(EnrollmentStatus::Retained) {
                return Err(FormalEducationError::IllegalTransition(format!(
                    "Enrollment cannot move from {} to retained",
                    cur_status_str
                )));
            }

            let updated = sqlx::query(
                "UPDATE archava_edu.enrollments SET status = 'retained', revision = revision + 1, updated_at = now() \
                 WHERE tenant_id = $1 AND enrollment_id = $2 RETURNING revision, status",
            )
            .bind(tenant)
            .bind(enrollment_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            let rev: i32 = updated.try_get("revision").map_err(database)?;
            (EnrollmentStatus::Retained, rev)
        }

        EducationCommand::GraduateEnrollment {
            command_id: _,
            ref enrollment_id,
        } => {
            let row = sqlx::query(
                "SELECT status, revision FROM archava_edu.enrollments \
                 WHERE tenant_id = $1 AND enrollment_id = $2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(enrollment_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
            .ok_or_else(not_found)?;

            let cur_status_str: String = row.try_get("status").map_err(database)?;
            let cur_status =
                EnrollmentStatus::parse(&cur_status_str).ok_or(FormalEducationError::NotFound)?;
            if !cur_status.can_transition_to(EnrollmentStatus::Graduated) {
                return Err(FormalEducationError::IllegalTransition(format!(
                    "Enrollment cannot move from {} to graduated",
                    cur_status_str
                )));
            }

            let updated = sqlx::query(
                "UPDATE archava_edu.enrollments SET status = 'graduated', revision = revision + 1, updated_at = now() \
                 WHERE tenant_id = $1 AND enrollment_id = $2 RETURNING revision, status",
            )
            .bind(tenant)
            .bind(enrollment_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            let rev: i32 = updated.try_get("revision").map_err(database)?;
            (EnrollmentStatus::Graduated, rev)
        }

        EducationCommand::WithdrawStudent {
            command_id: _,
            ref enrollment_id,
            reason: _,
        } => {
            let row = sqlx::query(
                "SELECT status, revision FROM archava_edu.enrollments \
                 WHERE tenant_id = $1 AND enrollment_id = $2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(enrollment_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?
            .ok_or_else(not_found)?;

            let cur_status_str: String = row.try_get("status").map_err(database)?;
            let cur_status =
                EnrollmentStatus::parse(&cur_status_str).ok_or(FormalEducationError::NotFound)?;
            if !cur_status.can_transition_to(EnrollmentStatus::Withdrawn) {
                return Err(FormalEducationError::IllegalTransition(format!(
                    "Enrollment cannot move from {} to withdrawn",
                    cur_status_str
                )));
            }

            let updated = sqlx::query(
                "UPDATE archava_edu.enrollments SET status = 'withdrawn', revision = revision + 1, updated_at = now() \
                 WHERE tenant_id = $1 AND enrollment_id = $2 RETURNING revision, status",
            )
            .bind(tenant)
            .bind(enrollment_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;

            let rev: i32 = updated.try_get("revision").map_err(database)?;
            (EnrollmentStatus::Withdrawn, rev)
        }
    };

    let action_str = command.action();

    // Insert audit
    let audit_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO archava_edu.audit ( \
         entry_id, enrollment_id, tenant_id, action, actor_ref, command_id \
         ) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&audit_id)
    .bind(&enrollment_id)
    .bind(tenant)
    .bind(action_str)
    .bind(&access.owner_id)
    .bind(command.command_id())
    .execute(&mut *connection)
    .await
    .map_err(database)?;

    let now_row = sqlx::query(
        "SELECT to_char(clock_timestamp() at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS now",
    )
    .fetch_one(&mut *connection)
    .await
    .map_err(database)?;
    let now_iso: String = now_row.try_get("now").map_err(database)?;

    let receipt = EducationReceipt {
        command_id: command.command_id().to_string(),
        action: action_str.to_string(),
        enrollment_id: enrollment_id.clone(),
        status,
        revision,
        recorded_at: now_iso,
    };

    let receipt_json = serde_json::to_value(&receipt).unwrap();
    let payload_json = serde_json::json!({
        "commandId": command.command_id(),
        "action": action_str,
        "enrollmentId": enrollment_id,
    });

    sqlx::query(
        "INSERT INTO archava_edu.commands ( \
         command_id, tenant_id, enrollment_id, action, fingerprint, payload, receipt, status, revision, created_by \
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, 'accepted', $8, $9)",
    )
    .bind(command.command_id())
    .bind(tenant)
    .bind(&enrollment_id)
    .bind(action_str)
    .bind(fingerprint)
    .bind(payload_json)
    .bind(receipt_json)
    .bind(revision)
    .bind(&access.owner_id)
    .execute(&mut *connection)
    .await
    .map_err(database)?;

    Ok(receipt)
}

pub async fn get_education_command_status(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    command_id: &str,
) -> Result<EducationCommandStatus, FormalEducationError> {
    let access = resolve_education_context(connection, tenant, principal).await?;
    require_permission(&access, "education.read")?;

    let row = sqlx::query(
        "SELECT command_id, action, enrollment_id, receipt, status \
         FROM archava_edu.commands \
         WHERE tenant_id = $1 AND command_id = $2 AND created_by = $3",
    )
    .bind(tenant)
    .bind(command_id)
    .bind(&access.owner_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;

    match row {
        Some(r) => {
            let cmd_id: String = r.try_get("command_id").map_err(database)?;
            let action: String = r.try_get("action").map_err(database)?;
            let enrollment_id: String = r.try_get("enrollment_id").map_err(database)?;
            let status: String = r.try_get("status").map_err(database)?;
            let receipt_val: Option<serde_json::Value> = r.try_get("receipt").map_err(database)?;
            let (revision, recorded_at) = if let Some(rc) = receipt_val {
                (
                    rc.get("revision")
                        .and_then(|v| v.as_i64())
                        .map(|v| v as i32),
                    rc.get("recordedAt")
                        .and_then(|v| v.as_str())
                        .map(|v| v.to_string()),
                )
            } else {
                (None, None)
            };

            Ok(EducationCommandStatus {
                command_id: cmd_id,
                action,
                enrollment_id,
                status,
                revision,
                recorded_at,
            })
        }
        None => Ok(EducationCommandStatus {
            command_id: command_id.to_string(),
            action: "admit_applicant".to_string(),
            enrollment_id: "unknown".to_string(),
            status: "unknown".to_string(),
            revision: None,
            recorded_at: None,
        }),
    }
}

pub async fn list_education_enrollments(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    query: EnrollmentPageQuery,
) -> Result<Vec<Enrollment>, FormalEducationError> {
    let access = resolve_education_context(connection, tenant, principal).await?;
    require_permission(&access, "education.read")?;

    let mut sql = String::from(
        "SELECT e.enrollment_id, e.student_ref, e.class_ref, e.term_ref, e.status, e.revision, \
                e.invoice_ref, e.guardian_ref, e.created_by, \
                to_char(e.created_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, \
                to_char(e.updated_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at \
         FROM archava_edu.enrollments e \
         WHERE e.tenant_id = $1 \
           AND archava_edu.visible_enrollment($1, $2, $3, e) ",
    );

    let mut param_index = 4;
    if query.status.is_some() {
        sql.push_str(&format!("AND e.status = ${param_index} "));
        param_index += 1;
    }
    if query.cursor.is_some() {
        sql.push_str(&format!("AND e.enrollment_id > ${param_index} "));
        param_index += 1;
    }
    sql.push_str(&format!(
        "ORDER BY e.enrollment_id ASC LIMIT ${param_index}"
    ));

    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(tenant)
        .bind(&principal.id)
        .bind(access.scope.as_str());

    if let Some(status) = query.status {
        q = q.bind(status.as_str());
    }
    if let Some(cursor) = query.cursor {
        q = q.bind(cursor);
    }
    q = q.bind(query.limit);

    let rows = q.fetch_all(&mut *connection).await.map_err(database)?;
    let mut enrollments = Vec::with_capacity(rows.len());
    for r in rows {
        enrollments.push(map_enrollment_row(&r)?);
    }
    Ok(enrollments)
}

pub async fn get_education_enrollment_detail(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    enrollment_id: &str,
) -> Result<Option<EnrollmentDetail>, FormalEducationError> {
    let access = resolve_education_context(connection, tenant, principal).await?;
    require_permission(&access, "education.read")?;

    let row = sqlx::query(
        "SELECT e.enrollment_id, e.student_ref, e.class_ref, e.term_ref, e.status, e.revision, \
                e.invoice_ref, e.guardian_ref, e.created_by, \
                to_char(e.created_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, \
                to_char(e.updated_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at \
         FROM archava_edu.enrollments e \
         WHERE e.tenant_id = $1 AND e.enrollment_id = $2 \
           AND archava_edu.visible_enrollment($1, $3, $4, e)",
    )
    .bind(tenant)
    .bind(enrollment_id)
    .bind(&principal.id)
    .bind(access.scope.as_str())
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;

    let enrollment_row = match row {
        Some(r) => r,
        None => return Ok(None),
    };

    let status_str: String = enrollment_row.try_get("status").map_err(database)?;
    let status = EnrollmentStatus::parse(&status_str).unwrap();
    let enrollment = map_enrollment_row(&enrollment_row)?;

    // Sessions
    let session_rows = sqlx::query(
        "SELECT session_id, outcome, note, appointment_ref, recorded_by \
         FROM archava_edu.sessions \
         WHERE tenant_id = $1 AND enrollment_id = $2 \
         ORDER BY recorded_at ASC, session_id ASC",
    )
    .bind(tenant)
    .bind(enrollment_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(database)?;

    let mut sessions = Vec::with_capacity(session_rows.len());
    for s in session_rows {
        let outcome_str: String = s.try_get("outcome").map_err(database)?;
        let outcome = SessionOutcome::parse(&outcome_str).unwrap();
        sessions.push(SessionRecord {
            session_id: s.try_get("session_id").map_err(database)?,
            outcome,
            note: s.try_get("note").map_err(database)?,
            appointment_ref: s.try_get("appointment_ref").map_err(database)?,
            recorded_by: s.try_get("recorded_by").map_err(database)?,
        });
    }

    // Published check
    let published_row = sqlx::query("SELECT archava_edu.results_published($1) AS published")
        .bind(status.as_str())
        .fetch_one(&mut *connection)
        .await
        .map_err(database)?;
    let published: bool = published_row.try_get("published").map_err(database)?;

    // Results
    let results = if published {
        let result_rows = sqlx::query(
            "SELECT result_id, assessment_ref, score::double precision AS score, out_of::double precision AS out_of, \
                    letter_ref, note, recorded_by \
             FROM archava_edu.results \
             WHERE tenant_id = $1 AND enrollment_id = $2 \
             ORDER BY recorded_at ASC, result_id ASC",
        )
        .bind(tenant)
        .bind(enrollment_id)
        .fetch_all(&mut *connection)
        .await
        .map_err(database)?;

        let mut res = Vec::with_capacity(result_rows.len());
        for r in result_rows {
            res.push(AssessmentResult {
                result_id: r.try_get("result_id").map_err(database)?,
                assessment_ref: r.try_get("assessment_ref").map_err(database)?,
                score: r.try_get("score").map_err(database)?,
                out_of: r.try_get("out_of").map_err(database)?,
                letter_ref: r.try_get("letter_ref").map_err(database)?,
                note: r.try_get("note").map_err(database)?,
                recorded_by: r.try_get("recorded_by").map_err(database)?,
            });
        }
        res
    } else {
        Vec::new()
    };

    // Review
    let review_row = sqlx::query(
        "SELECT review_id, decision, note, reviewer_ref \
         FROM archava_edu.reviews \
         WHERE tenant_id = $1 AND enrollment_id = $2 \
         ORDER BY reviewed_at ASC, review_id ASC",
    )
    .bind(tenant)
    .bind(enrollment_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(database)?;

    let review = review_row.last().map(|rev| ReviewRecord {
        review_id: rev.try_get("review_id").unwrap(),
        decision: rev.try_get("decision").unwrap(),
        note: rev.try_get("note").unwrap(),
        reviewer_ref: rev.try_get("reviewer_ref").unwrap(),
    });

    Ok(Some(EnrollmentDetail {
        enrollment,
        sessions,
        results,
        review,
        results_published: published,
    }))
}

pub async fn list_education_audit(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    enrollment_id: &str,
) -> Result<Vec<EducationAuditEntry>, FormalEducationError> {
    let access = resolve_education_context(connection, tenant, principal).await?;
    require_permission(&access, "audit.read")?;

    let visible_row = sqlx::query(
        "SELECT archava_edu.visible_enrollment($1, $3, $4, e) AS ok \
         FROM archava_edu.enrollments e \
         WHERE e.tenant_id = $1 AND e.enrollment_id = $2",
    )
    .bind(tenant)
    .bind(enrollment_id)
    .bind(&principal.id)
    .bind(access.scope.as_str())
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;

    let visible = visible_row
        .and_then(|r| r.try_get::<bool, _>("ok").ok())
        .unwrap_or(false);
    if !visible {
        return Ok(Vec::new());
    }

    let rows = sqlx::query(
        "SELECT entry_id, enrollment_id, action, actor_ref, command_id, \
                to_char(at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS at \
         FROM archava_edu.audit \
         WHERE tenant_id = $1 AND enrollment_id = $2 \
         ORDER BY at ASC, entry_id ASC",
    )
    .bind(tenant)
    .bind(enrollment_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(database)?;

    let mut audit = Vec::with_capacity(rows.len());
    for r in rows {
        audit.push(EducationAuditEntry {
            entry_id: r.try_get("entry_id").map_err(database)?,
            enrollment_id: r.try_get("enrollment_id").map_err(database)?,
            action: r.try_get("action").map_err(database)?,
            actor_ref: r.try_get("actor_ref").map_err(database)?,
            command_id: r.try_get("command_id").map_err(database)?,
            at: r.try_get("at").map_err(database)?,
        });
    }

    Ok(audit)
}
