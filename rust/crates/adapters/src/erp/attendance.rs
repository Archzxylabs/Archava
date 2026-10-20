//! The attendance SQL adapter — the Archxy 2 port of
//! `packages/attendance-pg/src/repository.ts` (`execute()` +
//! `getCommandStatus()` + the read projections), the R07 attendance cohort
//! (PACKET-009).
//!
//! ## Statement order (the TS port)
//!
//! The `execute()` ladder with the attendance vocabulary: the membership
//! ladder (`load_locked_membership`, the actor's own row only — the TS
//! repository has no on-behalf-of branch for a human: a service principal is
//! refused by the service layer before SQL, and the gate keeps that rule
//! here as well), the operation's own permission (`record_check_in` →
//! `attendance.checkin`; the other four arms → `attendance.write`), then the
//! arm's own row locks and the `commands` claim: SELECT the command row
//! first, refuse a fingerprint mismatch, replay a stored receipt with
//! `idempotencyReplay: true`, claim only when absent, and fall THROUGH on a
//! receipt-less row (the TS partial-command case).
//!
//! ## The corrected rule (audit A04 — P1)
//!
//! The check-in arm reads its roster by `(tenant, id, session_id)` — the
//! supplied session is part of the ROW SELECT, and the domain policy
//! ([`check_in_refusal`]) names the binding explicitly. A roster UUID from
//! another session is `not_found`; it can neither be checked in nor leak its
//! contact. The event's `session_id`/`contact_id` are the ROSTER's own,
//! never a caller-supplied pair.
//!
//! ## What the port tightens (recorded in the acceptance matrix)
//!
//! - A duplicate `(tenant, session, contact)` registration or duplicate
//!   ticket code is a `conflict`, not the baseline's database failure: the
//!   TS lets the UNIQUE constraint raise after the session counter moved and
//!   the whole transaction rolls back into a 500-class answer. The port
//!   answers the conflict class and records nothing.
//! - A duplicate check-in event key is `already_checked_in`, never a raw
//!   SQLSTATE.
//! - The status read grants nothing beyond the transaction's enrollment,
//!   exactly like the TS `getCommandStatus()`.
//!
//! ## The sampled clock
//!
//! One `clock_timestamp()` sample per command, bound into every stamping
//! position of the arm's statements (the R04/R05/R07 adapters' family).

use archava_domain::erp::attendance::{
    check_in_refusal, registration_refusal, AttendanceAccess, AttendanceCommand,
    AttendanceCommandStatus, AttendanceMutation, AttendanceReceipt, CheckInRecord, CheckInRefusal,
    RegistrationRefusal, RosterEntry, RosterStatus, Session, SessionDetail, SessionStatus,
    ATTENDANCE_PERMISSIONS,
};
use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
use archava_domain::erp::{ErpError, ErpMembershipKind, ErpScope};
use serde_json::Value;
use sqlx::PgConnection;
use sqlx::Row;
use thiserror::Error;

use super::commerce_kernel::load_locked_membership;
use super::{purchase_database_error, PurchaseError};

/// The ledger this adapter owns.
pub const ATTENDANCE_LEDGER: &str = "attendance";
/// The schema the statements ride.
pub const ATTENDANCE_NAMESPACE: &str = "archava_attendance";

/// The attendance refusal vocabulary — the TS `AttendanceError` codes,
/// carried one-for-one so the host can answer the TS api's own `codeMap`
/// (`capacity_exceeded` and `already_checked_in` are 409 classes of their
/// own, `bad_request` is 400).
#[derive(Debug, Error)]
pub enum AttendanceError {
    /// `forbidden` / `not_found` / `conflict` — the shared domain classes.
    #[error("attendance domain refusal: {0:?}")]
    Domain(ErpError),
    /// `capacity_exceeded` — the seat allocation is spent (409).
    #[error("attendance session capacity reached")]
    CapacityExceeded,
    /// `already_checked_in` — the roster is already attended or the
    /// event key is already used for this roster (409).
    #[error("attendance participant is already checked in")]
    AlreadyCheckedIn,
    /// `bad_request` — a closed session, a canceled registration, a stale
    /// shape the TS refuses with 400.
    #[error("attendance request refused by the lifecycle")]
    BadRequest,
    /// A database failure the SQL-state whitelist did not clear.
    #[error("attendance database failure in {ledger}: {source}")]
    Database {
        ledger: &'static str,
        #[source]
        source: sqlx::Error,
    },
}

impl From<AttendanceError> for PurchaseError {
    fn from(error: AttendanceError) -> Self {
        match error {
            AttendanceError::Domain(domain) => PurchaseError::Domain(domain),
            AttendanceError::CapacityExceeded
            | AttendanceError::AlreadyCheckedIn
            | AttendanceError::BadRequest => PurchaseError::Domain(ErpError::Conflict),
            AttendanceError::Database { ledger, source } => {
                PurchaseError::Database { ledger, source }
            }
        }
    }
}

fn forbidden() -> AttendanceError {
    AttendanceError::Domain(ErpError::Forbidden)
}

fn not_found() -> AttendanceError {
    AttendanceError::Domain(ErpError::NotFound)
}

fn conflict() -> AttendanceError {
    AttendanceError::Domain(ErpError::Conflict)
}

fn database(error: sqlx::Error) -> AttendanceError {
    AttendanceError::Database {
        ledger: ATTENDANCE_LEDGER,
        source: error,
    }
}

// --- The context ---------------------------------------------------------------

/// The member ladder — the TS `transaction()`'s membership resolution.
///
/// Attendance's TS repository only resolves the actor's OWN membership (the
/// service layer refuses a service principal before SQL), so the port keeps
/// the human-only cast at the host and resolves one row here.
pub async fn resolve_attendance_context(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<AttendanceAccess, AttendanceError> {
    let own = load_locked_membership(
        connection,
        ATTENDANCE_NAMESPACE,
        ATTENDANCE_LEDGER,
        tenant,
        &principal.id,
        ATTENDANCE_PERMISSIONS,
    )
    .await
    .map_err(|error| match error {
        PurchaseError::Domain(domain) => AttendanceError::Domain(domain),
        PurchaseError::Database { ledger, source } => AttendanceError::Database { ledger, source },
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
    Ok(AttendanceAccess {
        permissions: own.permissions.clone(),
        owner_id: principal.id.clone(),
        scope: match own.scope {
            ErpScope::All => "all".to_string(),
            ErpScope::Assigned => "assigned".to_string(),
        },
    })
}

// --- The claim -----------------------------------------------------------------

/// The command claim — SELECT first, refuse a fingerprint mismatch, replay a
/// stored receipt, claim only when absent, fall through on a receipt-less
/// row (the TS partial-command case).
enum Claim {
    Fresh,
    Replay(Box<AttendanceReceipt>),
}

/// The `commands` row read + claim.
async fn claim_command(
    connection: &mut PgConnection,
    tenant: &str,
    command_id: &str,
    actor_id: &str,
    fingerprint: &str,
) -> Result<Claim, AttendanceError> {
    let row = sqlx::query(
        "SELECT receipt, fingerprint FROM archava_attendance.commands \
WHERE tenant_id=$1 AND request_id=$2 FOR UPDATE",
    )
    .bind(tenant)
    .bind(uuid_of(command_id))
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;
    if let Some(row) = row {
        let stored_fingerprint: String = row.try_get("fingerprint").map_err(database)?;
        if stored_fingerprint != fingerprint {
            return Err(conflict());
        }
        let receipt: Option<Value> = row.try_get("receipt").map_err(database)?;
        if let Some(receipt) = receipt {
            let parsed = AttendanceReceipt::parse(&receipt, true)
                .map_err(|_| AttendanceError::Domain(ErpError::Unavailable))?;
            return Ok(Claim::Replay(Box::new(parsed)));
        }
        return Ok(Claim::Fresh);
    }
    sqlx::query(
        "INSERT INTO archava_attendance.commands(tenant_id, request_id, actor_id, fingerprint) \
VALUES($1, $2, $3, $4)",
    )
    .bind(tenant)
    .bind(uuid_of(command_id))
    .bind(actor_id)
    .bind(fingerprint)
    .execute(&mut *connection)
    .await
    .map_err(database)?;
    Ok(Claim::Fresh)
}

/// The receipt binding — the one UPDATE the `command_guard` trigger allows.
async fn bind_receipt(
    connection: &mut PgConnection,
    tenant: &str,
    command_id: &str,
    receipt: &AttendanceReceipt,
) -> Result<(), AttendanceError> {
    let document = serde_json::to_value(receipt)
        .map_err(|_| AttendanceError::Domain(ErpError::Unavailable))?;
    sqlx::query(
        "UPDATE archava_attendance.commands SET receipt=$1 WHERE tenant_id=$2 AND request_id=$3",
    )
    .bind(document)
    .bind(tenant)
    .bind(uuid_of(command_id))
    .execute(&mut *connection)
    .await
    .map_err(database)?;
    Ok(())
}

/// The audit row — the TS `INSERT INTO …audit(...)` with its own revision.
#[allow(clippy::too_many_arguments)]
async fn audit(
    connection: &mut PgConnection,
    tenant: &str,
    command_id: &str,
    actor_id: &str,
    operation: &str,
    session_id: &str,
    revision: i64,
    reason: Option<&str>,
    occurred_at: &str,
) -> Result<(), AttendanceError> {
    sqlx::query(
        "INSERT INTO archava_attendance.audit(\
tenant_id, request_id, actor_id, operation, session_id, revision, reason, occurred_at) \
VALUES($1, $2, $3, $4, $5, $6, $7, $8::timestamptz)",
    )
    .bind(tenant)
    .bind(uuid_of(command_id))
    .bind(actor_id)
    .bind(operation)
    .bind(uuid_of(session_id))
    .bind(revision)
    .bind(reason)
    .bind(occurred_at)
    .execute(&mut *connection)
    .await
    .map_err(database)?;
    Ok(())
}

fn require(access: &AttendanceAccess, wanted: &str) -> Result<(), AttendanceError> {
    if access.holds(wanted) {
        Ok(())
    } else {
        Err(forbidden())
    }
}

/// The refusals the pure policy names, carried to the wire classes.
fn policy_refusal(refusal: CheckInRefusal) -> AttendanceError {
    match refusal {
        CheckInRefusal::RosterNotInSession => not_found(),
        CheckInRefusal::SessionClosed(_) => AttendanceError::BadRequest,
        CheckInRefusal::AlreadyCheckedIn => AttendanceError::AlreadyCheckedIn,
        CheckInRefusal::RosterCanceled => AttendanceError::BadRequest,
    }
}

fn registration_error(refusal: RegistrationRefusal) -> AttendanceError {
    match refusal {
        RegistrationRefusal::SessionClosed(_) => AttendanceError::BadRequest,
        RegistrationRefusal::CapacityExceeded => AttendanceError::CapacityExceeded,
    }
}

// --- The arms ------------------------------------------------------------------

/// The whole command ladder — claim, arm, audit, receipt binding.
///
/// The commit is the HOST's (one transaction per request); this function
/// only speaks SQL.
pub async fn record_attendance_command(
    connection: &mut PgConnection,
    tenant: &str,
    access: &AttendanceAccess,
    fingerprint: &str,
    mutation: &AttendanceMutation,
) -> Result<AttendanceReceipt, AttendanceError> {
    let command_id = mutation.command.command_id();
    match claim_command(
        connection,
        tenant,
        command_id,
        &access.owner_id,
        fingerprint,
    )
    .await?
    {
        Claim::Replay(receipt) => return Ok(*receipt),
        Claim::Fresh => {}
    }

    // The sampled clock — one instant per command, bound everywhere.
    let (sampled,): (String,) = sqlx::query_as(
        "SELECT to_char(clock_timestamp() at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .fetch_one(&mut *connection)
    .await
    .map_err(database)?;

    let receipt = match &mutation.command {
        AttendanceCommand::CreateSession {
            command_id,
            title,
            session_code,
            capacity,
            start_time,
            end_time,
            location,
        } => {
            require(access, "attendance.write")?;
            let session_id = uuid_v4();
            let row = sqlx::query(
                "INSERT INTO archava_attendance.sessions(\
tenant_id, id, title, session_code, capacity, start_time, end_time, status, location, \
owner_id, created_at, updated_at) \
VALUES($1,$2,$3,$4,$5,$6::timestamptz,$7::timestamptz,'scheduled',$8,$9,$10::timestamptz,$10::timestamptz) \
RETURNING to_char(created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at",
            )
            .bind(tenant)
            .bind(session_id)
            .bind(title)
            .bind(session_code)
            .bind(capacity)
            .bind(start_time)
            .bind(end_time)
            .bind(location)
            .bind(&access.owner_id)
            .bind(&sampled)
            .fetch_one(&mut *connection)
            .await
            .map_err(|error| {
                if is_unique_violation(&error) {
                    conflict()
                } else {
                    database(error)
                }
            })?;
            let created_at: String = row.try_get("created_at").map_err(database)?;
            audit(
                connection,
                tenant,
                command_id,
                &access.owner_id,
                "create_session",
                &session_id.to_string(),
                1,
                Some(&format!("Created session {session_code}")),
                &sampled,
            )
            .await?;
            AttendanceReceipt {
                command_id: command_id.clone(),
                tenant_id: tenant.to_string(),
                action: "create_session".to_string(),
                session_id: Some(Some(session_id.to_string())),
                roster_id: None,
                check_in_id: None,
                status: "scheduled".to_string(),
                revision: Some(1),
                idempotency_replay: false,
                executed_at: created_at,
            }
        }

        AttendanceCommand::RegisterParticipant {
            command_id,
            session_id,
            contact_id,
            role,
            ticket_code,
        } => {
            require(access, "attendance.write")?;
            let row = sqlx::query(
                "SELECT status, capacity::bigint AS capacity, enrolled_count::bigint AS enrolled_count, \
revision::bigint AS revision FROM archava_attendance.sessions \
WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(uuid_of(session_id))
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?;
            let Some(row) = row else {
                return Err(not_found());
            };
            let status: String = row.try_get("status").map_err(database)?;
            let capacity: Option<i64> = row.try_get("capacity").map_err(database)?;
            let enrolled: i64 = row.try_get("enrolled_count").map_err(database)?;
            let revision: i64 = row.try_get("revision").map_err(database)?;
            let session_status = SessionStatus::parse(&status).ok_or_else(forbidden)?;
            if let Some(refusal) = registration_refusal(session_status, capacity, enrolled) {
                return Err(registration_error(refusal));
            }

            let roster_id = uuid_v4();
            let next_enrolled = enrolled + 1;
            let next_revision = revision + 1;
            sqlx::query(
                "UPDATE archava_attendance.sessions SET enrolled_count=$1, revision=$2, \
updated_at=$3::timestamptz WHERE tenant_id=$4 AND id=$5",
            )
            .bind(next_enrolled)
            .bind(next_revision)
            .bind(&sampled)
            .bind(tenant)
            .bind(uuid_of(session_id))
            .execute(&mut *connection)
            .await
            .map_err(database)?;

            let roster_row = sqlx::query(
                "INSERT INTO archava_attendance.rosters(\
tenant_id, id, session_id, contact_id, role, status, ticket_code, created_at, updated_at) \
VALUES($1,$2,$3,$4,$5,'registered',$6,$7::timestamptz,$7::timestamptz) \
RETURNING to_char(created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at",
            )
            .bind(tenant)
            .bind(roster_id)
            .bind(uuid_of(session_id))
            .bind(contact_id)
            .bind(role)
            .bind(ticket_code)
            .bind(&sampled)
            .fetch_one(&mut *connection)
            .await
            .map_err(|error| {
                if is_unique_violation(&error) {
                    conflict()
                } else {
                    database(error)
                }
            })?;
            let created_at: String = roster_row.try_get("created_at").map_err(database)?;

            audit(
                connection,
                tenant,
                command_id,
                &access.owner_id,
                "register_participant",
                session_id,
                1,
                Some(&format!(
                    "Registered contact {contact_id} with ticket {ticket_code}"
                )),
                &sampled,
            )
            .await?;
            AttendanceReceipt {
                command_id: command_id.clone(),
                tenant_id: tenant.to_string(),
                action: "register_participant".to_string(),
                session_id: Some(Some(session_id.clone())),
                roster_id: Some(Some(roster_id.to_string())),
                check_in_id: None,
                status: "registered".to_string(),
                revision: Some(1),
                idempotency_replay: false,
                executed_at: created_at,
            }
        }

        AttendanceCommand::RecordCheckIn {
            command_id,
            session_id,
            roster_id,
            method,
            evidence_ref,
            idempotency_key,
        } => {
            require(access, "attendance.checkin")?;
            let session_row = sqlx::query(
                "SELECT status FROM archava_attendance.sessions \
WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(uuid_of(session_id))
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?;
            let Some(session_row) = session_row else {
                return Err(not_found());
            };
            let session_status_text: String = session_row.try_get("status").map_err(database)?;
            let session_status =
                SessionStatus::parse(&session_status_text).ok_or_else(forbidden)?;

            // A04: the supplied session is part of the roster's identity. A
            // roster from another session is invisible here.
            let roster_row = sqlx::query(
                "SELECT session_id::text AS session_id, contact_id, ticket_code, status, \
revision::bigint AS revision FROM archava_attendance.rosters \
WHERE tenant_id=$1 AND id=$2 AND session_id=$3 FOR UPDATE",
            )
            .bind(tenant)
            .bind(uuid_of(roster_id))
            .bind(uuid_of(session_id))
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?;
            let Some(roster_row) = roster_row else {
                return Err(not_found());
            };
            let roster_session: String = roster_row.try_get("session_id").map_err(database)?;
            let contact_id: String = roster_row.try_get("contact_id").map_err(database)?;
            let ticket_code: String = roster_row.try_get("ticket_code").map_err(database)?;
            let roster_status_text: String = roster_row.try_get("status").map_err(database)?;
            let roster_revision: i64 = roster_row.try_get("revision").map_err(database)?;
            let roster_status = RosterStatus::parse(&roster_status_text).ok_or_else(forbidden)?;
            // The binding is enforced by the SELECT; the policy names it so
            // a looser SELECT can never silently pass.
            let bound = roster_session == session_id.as_str();
            if let Some(refusal) = check_in_refusal(session_status, roster_status, bound) {
                return Err(policy_refusal(refusal));
            }

            let existing = sqlx::query(
                "SELECT id FROM archava_attendance.check_in_events \
WHERE tenant_id=$1 AND roster_id=$2 AND idempotency_key=$3",
            )
            .bind(tenant)
            .bind(uuid_of(roster_id))
            .bind(idempotency_key)
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?;
            if existing.is_some() {
                return Err(AttendanceError::AlreadyCheckedIn);
            }

            let check_in_id = uuid_v4();
            let next_revision = roster_revision + 1;
            let event_row = sqlx::query(
                "INSERT INTO archava_attendance.check_in_events(\
tenant_id, id, roster_id, session_id, contact_id, method, evidence_ref, \
idempotency_key, recorded_by, recorded_at) \
VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10::timestamptz) \
RETURNING to_char(recorded_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at",
            )
            .bind(tenant)
            .bind(check_in_id)
            .bind(uuid_of(roster_id))
            .bind(uuid_of(session_id))
            .bind(&contact_id)
            .bind(method)
            .bind(evidence_ref)
            .bind(idempotency_key)
            .bind(&access.owner_id)
            .bind(&sampled)
            .fetch_one(&mut *connection)
            .await
            .map_err(|error| {
                if is_unique_violation(&error) {
                    AttendanceError::AlreadyCheckedIn
                } else {
                    database(error)
                }
            })?;
            let recorded_at: String = event_row.try_get("recorded_at").map_err(database)?;

            sqlx::query(
                "UPDATE archava_attendance.rosters SET status='attended', \
checked_in_at=$1::timestamptz, check_in_method=$2, revision=$3, updated_at=$1::timestamptz \
WHERE tenant_id=$4 AND id=$5",
            )
            .bind(&sampled)
            .bind(method)
            .bind(next_revision)
            .bind(tenant)
            .bind(uuid_of(roster_id))
            .execute(&mut *connection)
            .await
            .map_err(database)?;
            sqlx::query(
                "UPDATE archava_attendance.sessions SET attended_count=attended_count+1, \
updated_at=$1::timestamptz WHERE tenant_id=$2 AND id=$3",
            )
            .bind(&sampled)
            .bind(tenant)
            .bind(uuid_of(session_id))
            .execute(&mut *connection)
            .await
            .map_err(database)?;
            audit(
                connection,
                tenant,
                command_id,
                &access.owner_id,
                "record_check_in",
                session_id,
                1,
                Some(&format!(
                    "Check-in recorded for ticket {ticket_code} via {method}"
                )),
                &sampled,
            )
            .await?;
            AttendanceReceipt {
                command_id: command_id.clone(),
                tenant_id: tenant.to_string(),
                action: "record_check_in".to_string(),
                session_id: Some(Some(session_id.clone())),
                roster_id: Some(Some(roster_id.clone())),
                check_in_id: Some(Some(check_in_id.to_string())),
                status: "attended".to_string(),
                revision: Some(next_revision),
                idempotency_replay: false,
                executed_at: recorded_at,
            }
        }

        AttendanceCommand::UpdateRosterStatus {
            command_id,
            roster_id,
            expected_revision,
            target_status,
            reason,
        } => {
            require(access, "attendance.write")?;
            let row = sqlx::query(
                "SELECT session_id::text AS session_id, status, revision::bigint AS revision \
FROM archava_attendance.rosters WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(uuid_of(roster_id))
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?;
            let Some(row) = row else {
                return Err(not_found());
            };
            let session_id: String = row.try_get("session_id").map_err(database)?;
            let current_status: String = row.try_get("status").map_err(database)?;
            let revision: i64 = row.try_get("revision").map_err(database)?;
            if revision != *expected_revision {
                return Err(conflict());
            }
            let next_revision = revision + 1;
            sqlx::query(
                "UPDATE archava_attendance.rosters SET status=$1, revision=$2, \
updated_at=$3::timestamptz WHERE tenant_id=$4 AND id=$5",
            )
            .bind(target_status)
            .bind(next_revision)
            .bind(&sampled)
            .bind(tenant)
            .bind(uuid_of(roster_id))
            .execute(&mut *connection)
            .await
            .map_err(database)?;
            if target_status == "canceled" && current_status != "canceled" {
                sqlx::query(
                    "UPDATE archava_attendance.sessions SET enrolled_count=GREATEST(0, enrolled_count-1), \
updated_at=$1::timestamptz WHERE tenant_id=$2 AND id=$3",
                )
                .bind(&sampled)
                .bind(tenant)
                .bind(uuid_of(&session_id))
                .execute(&mut *connection)
                .await
                .map_err(database)?;
            }
            audit(
                connection,
                tenant,
                command_id,
                &access.owner_id,
                "update_roster_status",
                &session_id,
                next_revision,
                Some(reason),
                &sampled,
            )
            .await?;
            AttendanceReceipt {
                command_id: command_id.clone(),
                tenant_id: tenant.to_string(),
                action: "update_roster_status".to_string(),
                session_id: Some(Some(session_id)),
                roster_id: Some(Some(roster_id.clone())),
                check_in_id: None,
                status: target_status.clone(),
                revision: Some(next_revision),
                idempotency_replay: false,
                executed_at: sampled.clone(),
            }
        }

        AttendanceCommand::ChangeSessionStatus {
            command_id,
            session_id,
            expected_revision,
            target_status,
            reason,
        } => {
            require(access, "attendance.write")?;
            let row = sqlx::query(
                "SELECT revision::bigint AS revision FROM archava_attendance.sessions \
WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(uuid_of(session_id))
            .fetch_optional(&mut *connection)
            .await
            .map_err(database)?;
            let Some(row) = row else {
                return Err(not_found());
            };
            let revision: i64 = row.try_get("revision").map_err(database)?;
            if revision != *expected_revision {
                return Err(conflict());
            }
            let next_revision = revision + 1;
            sqlx::query(
                "UPDATE archava_attendance.sessions SET status=$1, revision=$2, \
updated_at=$3::timestamptz WHERE tenant_id=$4 AND id=$5",
            )
            .bind(target_status)
            .bind(next_revision)
            .bind(&sampled)
            .bind(tenant)
            .bind(uuid_of(session_id))
            .execute(&mut *connection)
            .await
            .map_err(database)?;
            if target_status == "canceled" {
                sqlx::query(
                    "UPDATE archava_attendance.rosters SET status='canceled', \
updated_at=$1::timestamptz WHERE tenant_id=$2 AND session_id=$3 AND status != 'attended'",
                )
                .bind(&sampled)
                .bind(tenant)
                .bind(uuid_of(session_id))
                .execute(&mut *connection)
                .await
                .map_err(database)?;
            }
            audit(
                connection,
                tenant,
                command_id,
                &access.owner_id,
                "change_session_status",
                session_id,
                next_revision,
                Some(reason),
                &sampled,
            )
            .await?;
            AttendanceReceipt {
                command_id: command_id.clone(),
                tenant_id: tenant.to_string(),
                action: "change_session_status".to_string(),
                session_id: Some(Some(session_id.clone())),
                roster_id: None,
                check_in_id: None,
                status: target_status.clone(),
                revision: Some(next_revision),
                idempotency_replay: false,
                executed_at: sampled.clone(),
            }
        }
    };

    bind_receipt(connection, tenant, command_id, &receipt).await?;
    Ok(receipt)
}

// --- The status read -----------------------------------------------------------

/// The status read — the TS `getCommandStatus()`.
pub async fn attendance_command_status(
    connection: &mut PgConnection,
    tenant: &str,
    command_id: &str,
) -> Result<AttendanceCommandStatus, AttendanceError> {
    let row = sqlx::query(
        "SELECT receipt FROM archava_attendance.commands WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(uuid_of(command_id))
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;
    let Some(row) = row else {
        return Ok(AttendanceCommandStatus::Unknown);
    };
    let receipt: Option<Value> = row.try_get("receipt").map_err(database)?;
    let Some(receipt) = receipt else {
        return Ok(AttendanceCommandStatus::Unknown);
    };
    let parsed = AttendanceReceipt::parse(&receipt, false)
        .map_err(|_| AttendanceError::Domain(ErpError::Unavailable))?;
    Ok(AttendanceCommandStatus::Recorded {
        receipt: Box::new(parsed),
    })
}

// --- The read projections ------------------------------------------------------

/// The page query — the TS `sessionPageQuerySchema`.
#[derive(Debug, Clone)]
pub struct AttendancePageQuery {
    pub limit: i64,
    pub offset: i64,
    pub status: Option<String>,
}

/// The session list — the TS `listSessions()`, bounded and ordered.
pub async fn list_attendance_sessions(
    connection: &mut PgConnection,
    tenant: &str,
    access: &AttendanceAccess,
    query: &AttendancePageQuery,
) -> Result<Vec<Session>, AttendanceError> {
    require(access, "attendance.read")?;
    enum Param {
        Text(String),
        Count(i64),
    }
    let mut conditions = vec!["tenant_id = $1".to_string()];
    let mut params: Vec<Param> = vec![Param::Text(tenant.to_string())];
    if let Some(status) = query.status.as_deref() {
        params.push(Param::Text(status.to_string()));
        conditions.push(format!("status = ${}", params.len()));
    }
    // The assigned scope narrows to the owner's own sessions — the same
    // grant narrowing the subscriptions reads carry.
    if access.scope == "assigned" {
        params.push(Param::Text(access.owner_id.clone()));
        conditions.push(format!("owner_id = ${}", params.len()));
    }
    params.push(Param::Count(query.limit));
    let limit_index = params.len();
    params.push(Param::Count(query.offset));
    let offset_index = params.len();
    let statement = format!(
        "SELECT id::text AS id, tenant_id, title, session_code, capacity::bigint AS capacity, \
to_char(start_time at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS start_time, \
to_char(end_time at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS end_time, \
status, location, enrolled_count::bigint AS enrolled_count, \
attended_count::bigint AS attended_count, revision::bigint AS revision, \
to_char(created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, \
to_char(updated_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at \
FROM archava_attendance.sessions WHERE {} ORDER BY start_time ASC, id ASC \
LIMIT ${} OFFSET ${}",
        conditions.join(" AND "),
        limit_index,
        offset_index
    );
    let mut builder = sqlx::query(sqlx::AssertSqlSafe(statement.as_str()));
    for param in &params {
        builder = match param {
            Param::Text(value) => builder.bind(value),
            Param::Count(value) => builder.bind(*value),
        };
    }
    let rows = builder
        .fetch_all(&mut *connection)
        .await
        .map_err(database)?;
    rows.iter().map(map_session).collect()
}

const SESSION_COLUMNS: &str = "id::text AS id, tenant_id, title, session_code, \
capacity::bigint AS capacity, owner_id, \
to_char(start_time at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS start_time, \
to_char(end_time at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS end_time, \
status, location, enrolled_count::bigint AS enrolled_count, \
attended_count::bigint AS attended_count, revision::bigint AS revision, \
to_char(created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, \
to_char(updated_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at";

const ROSTER_COLUMNS: &str = "id::text AS id, tenant_id, session_id::text AS session_id, \
contact_id, role, status, ticket_code, \
to_char(checked_in_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS checked_in_at, \
check_in_method, revision::bigint AS revision, \
to_char(created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, \
to_char(updated_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at";

const CHECK_IN_COLUMNS: &str = "id::text AS id, tenant_id, roster_id::text AS roster_id, \
session_id::text AS session_id, contact_id, method, evidence_ref, idempotency_key, \
recorded_by, \
to_char(recorded_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS recorded_at";

/// Read one session row and apply the assigned-scope narrowing. `None` is
/// the TS's `null` — the caller cannot tell a hidden row from a missing one.
async fn read_session_row(
    connection: &mut PgConnection,
    tenant: &str,
    access: &AttendanceAccess,
    session_id: &str,
) -> Result<Option<Session>, AttendanceError> {
    let statement = format!(
        "SELECT {SESSION_COLUMNS} FROM archava_attendance.sessions WHERE tenant_id=$1 AND id=$2"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
        .bind(tenant)
        .bind(uuid_of(session_id))
        .fetch_optional(&mut *connection)
        .await
        .map_err(database)?;
    let Some(row) = row else {
        return Ok(None);
    };
    if access.scope == "assigned" {
        let owner: String = row.try_get("owner_id").map_err(database)?;
        if owner != access.owner_id {
            return Ok(None);
        }
    }
    map_session(&row).map(Some)
}

/// The detail read — the TS `getSessionDetail()`.
pub async fn read_attendance_detail(
    connection: &mut PgConnection,
    tenant: &str,
    access: &AttendanceAccess,
    session_id: &str,
) -> Result<Option<SessionDetail>, AttendanceError> {
    require(access, "attendance.read")?;
    let Some(session) = read_session_row(connection, tenant, access, session_id).await? else {
        return Ok(None);
    };
    let roster_statement = format!(
        "SELECT {ROSTER_COLUMNS} FROM archava_attendance.rosters \
WHERE tenant_id=$1 AND session_id=$2 ORDER BY created_at ASC, id ASC"
    );
    let roster_rows = sqlx::query(sqlx::AssertSqlSafe(roster_statement.as_str()))
        .bind(tenant)
        .bind(uuid_of(session_id))
        .fetch_all(&mut *connection)
        .await
        .map_err(database)?;
    let roster = roster_rows
        .iter()
        .map(map_roster)
        .collect::<Result<Vec<RosterEntry>, AttendanceError>>()?;
    let check_in_statement = format!(
        "SELECT {CHECK_IN_COLUMNS} FROM archava_attendance.check_in_events \
WHERE tenant_id=$1 AND session_id=$2 ORDER BY recorded_at DESC, id DESC"
    );
    let check_in_rows = sqlx::query(sqlx::AssertSqlSafe(check_in_statement.as_str()))
        .bind(tenant)
        .bind(uuid_of(session_id))
        .fetch_all(&mut *connection)
        .await
        .map_err(database)?;
    let check_ins = check_in_rows
        .iter()
        .map(map_check_in)
        .collect::<Result<Vec<CheckInRecord>, AttendanceError>>()?;
    Ok(Some(SessionDetail {
        session,
        roster,
        check_ins,
    }))
}

/// The roster read — the TS `listRoster()`.
pub async fn read_attendance_roster(
    connection: &mut PgConnection,
    tenant: &str,
    access: &AttendanceAccess,
    session_id: &str,
) -> Result<Vec<RosterEntry>, AttendanceError> {
    require(access, "attendance.read")?;
    // The session is read first so the assigned scope can hide the whole
    // page (the TS lists the rows of whatever session it is handed; the
    // scope narrowing is this port's own).
    let Some(_session) = read_session_row(connection, tenant, access, session_id).await? else {
        return Ok(Vec::new());
    };
    let statement = format!(
        "SELECT {ROSTER_COLUMNS} FROM archava_attendance.rosters \
WHERE tenant_id=$1 AND session_id=$2 ORDER BY created_at ASC, id ASC"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
        .bind(tenant)
        .bind(uuid_of(session_id))
        .fetch_all(&mut *connection)
        .await
        .map_err(database)?;
    rows.iter().map(map_roster).collect()
}

/// The check-in read — the TS `listCheckIns()`.
pub async fn read_attendance_check_ins(
    connection: &mut PgConnection,
    tenant: &str,
    access: &AttendanceAccess,
    session_id: &str,
) -> Result<Vec<CheckInRecord>, AttendanceError> {
    require(access, "attendance.read")?;
    let Some(_session) = read_session_row(connection, tenant, access, session_id).await? else {
        return Ok(Vec::new());
    };
    let statement = format!(
        "SELECT {CHECK_IN_COLUMNS} FROM archava_attendance.check_in_events \
WHERE tenant_id=$1 AND session_id=$2 ORDER BY recorded_at DESC, id DESC"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
        .bind(tenant)
        .bind(uuid_of(session_id))
        .fetch_all(&mut *connection)
        .await
        .map_err(database)?;
    rows.iter().map(map_check_in).collect()
}

// --- The mappers ---------------------------------------------------------------

fn map_session(row: &sqlx::postgres::PgRow) -> Result<Session, AttendanceError> {
    Ok(Session {
        id: row.try_get("id").map_err(database)?,
        tenant_id: row.try_get("tenant_id").map_err(database)?,
        title: row.try_get("title").map_err(database)?,
        session_code: row.try_get("session_code").map_err(database)?,
        capacity: row.try_get("capacity").map_err(database)?,
        start_time: row.try_get("start_time").map_err(database)?,
        end_time: row.try_get("end_time").map_err(database)?,
        status: row.try_get("status").map_err(database)?,
        location: row.try_get("location").map_err(database)?,
        enrolled_count: row.try_get("enrolled_count").map_err(database)?,
        attended_count: row.try_get("attended_count").map_err(database)?,
        revision: row.try_get("revision").map_err(database)?,
        created_at: row.try_get("created_at").map_err(database)?,
        updated_at: row.try_get("updated_at").map_err(database)?,
    })
}

fn map_roster(row: &sqlx::postgres::PgRow) -> Result<RosterEntry, AttendanceError> {
    Ok(RosterEntry {
        id: row.try_get("id").map_err(database)?,
        tenant_id: row.try_get("tenant_id").map_err(database)?,
        session_id: row.try_get("session_id").map_err(database)?,
        contact_id: row.try_get("contact_id").map_err(database)?,
        role: row.try_get("role").map_err(database)?,
        status: row.try_get("status").map_err(database)?,
        ticket_code: row.try_get("ticket_code").map_err(database)?,
        checked_in_at: row.try_get("checked_in_at").map_err(database)?,
        check_in_method: row.try_get("check_in_method").map_err(database)?,
        revision: row.try_get("revision").map_err(database)?,
        created_at: row.try_get("created_at").map_err(database)?,
        updated_at: row.try_get("updated_at").map_err(database)?,
    })
}

fn map_check_in(row: &sqlx::postgres::PgRow) -> Result<CheckInRecord, AttendanceError> {
    Ok(CheckInRecord {
        id: row.try_get("id").map_err(database)?,
        tenant_id: row.try_get("tenant_id").map_err(database)?,
        roster_id: row.try_get("roster_id").map_err(database)?,
        session_id: row.try_get("session_id").map_err(database)?,
        contact_id: row.try_get("contact_id").map_err(database)?,
        method: row.try_get("method").map_err(database)?,
        evidence_ref: row.try_get("evidence_ref").map_err(database)?,
        idempotency_key: row.try_get("idempotency_key").map_err(database)?,
        recorded_by: row.try_get("recorded_by").map_err(database)?,
        recorded_at: row.try_get("recorded_at").map_err(database)?,
    })
}

// --- Helpers -------------------------------------------------------------------

/// A fresh identifier — the TS `randomUUID()`.
fn uuid_v4() -> uuid::Uuid {
    uuid::Uuid::new_v4()
}

/// The uuid a grammar-admitted id string carries. The columns are `uuid`
/// columns: a text bind raises `operator does not exist: uuid = text`.
fn uuid_of(value: &str) -> uuid::Uuid {
    uuid::Uuid::parse_str(value).expect("the grammar admits only uuid ids")
}

/// The unique-violation test — SQLSTATE 23505.
fn is_unique_violation(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(inner) => inner.code().as_deref() == Some("23505"),
        _ => false,
    }
}

// --- The kernel -----------------------------------------------------------------

/// The ledger's own readiness facts — the port of the TS repository's
/// `create()` guards: a NON-OWNER runtime role, the exact migration ledger,
/// and six FORCED tenant policies.
pub async fn assert_attendance_kernel(
    pool: &sqlx::PgPool,
    attendance_ledger: &[(String, String)],
) -> Result<(), String> {
    let role: Option<(bool,)> = sqlx::query_as(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class \
WHERE relnamespace='archava_attendance'::regnamespace AND relowner=pg_roles.oid)) AS unsafe \
FROM pg_roles WHERE rolname=current_user",
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("attendance runtime role check failed: {error}"))?;
    if role.map(|(unsafe_,)| unsafe_).unwrap_or(true) {
        return Err("Attendance requires a non-owner runtime".to_string());
    }
    let actual: Vec<(String, String)> =
        sqlx::query_as("SELECT id,checksum FROM archava_attendance.schema_migrations ORDER BY id")
            .fetch_all(pool)
            .await
            .map_err(|error| format!("unsupported attendance ledger read: {error}"))?;
    if actual.len() != attendance_ledger.len()
        || actual.iter().zip(attendance_ledger).any(|(a, b)| a != b)
    {
        return Err("Unsupported attendance migration checksum".to_string());
    }
    let names: &[&str] = &[
        "memberships",
        "sessions",
        "rosters",
        "check_in_events",
        "commands",
        "audit",
    ][..];
    let statement = "SELECT count(*)::integer AS count FROM pg_class WHERE \
relnamespace='archava_attendance'::regnamespace AND relname=ANY($1::text[]) \
AND relrowsecurity AND relforcerowsecurity";
    let (count,): (i32,) = sqlx::query_as(statement)
        .bind(names)
        .fetch_one(pool)
        .await
        .map_err(|error| format!("attendance policy check failed: {error}"))?;
    if count as usize != names.len() {
        return Err("Attendance tenant policies must be forced".to_string());
    }
    Ok(())
}

/// The database-error helper the shared crate exposes — re-exported so the
/// host can map a failure the same way the other ERP ledgers do.
pub fn attendance_database_error(error: sqlx::Error) -> PurchaseError {
    purchase_database_error(ATTENDANCE_LEDGER, error)
}
