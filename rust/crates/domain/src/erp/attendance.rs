//! The attendance domain — the Archxy 2 port of
//! `packages/attendance/src/{contracts,service}.ts`: session setup and status,
//! participant registration on a session roster, and verified check-in events
//! with per-roster idempotency keys.
//!
//! ## What this port keeps from the baseline
//!
//! The five command arms (`create_session`, `register_participant`,
//! `record_check_in`, `update_roster_status`, `change_session_status`), the
//! strict object grammar (the TS `.strict()` objects refuse extra members),
//! the four vocabularies (session statuses, roster roles and statuses,
//! check-in methods), the receipt shape (including the
//! `idempotencyReplay` member every ERP ledger carries) and the
//! `unknown`/`recorded` command-status union. The service layer's
//! HUMAN-OPERATOR rule lives in the host, exactly where the TS service
//! applies it — a service principal refuses every arm.
//!
//! ## The corrected rule (audit A04 — P1)
//!
//! The baseline's `record_check_in` loads the roster by
//! `(tenant_id, id)` alone. A valid roster UUID from ANOTHER session checks
//! in against the supplied session: the roster flips to `attended` while the
//! SUPPLIED session's `attended_count` advances, and the event row carries
//! the supplied `session_id` over the roster's own. A UUID is not authority.
//!
//! This port makes the binding explicit in one pure function
//! ([`check_in_refusal`]): the roster must belong to the supplied session
//! (which the adapter enforces in the row SELECT as well), the session must
//! be open (`scheduled`/`in_progress`), and only then do the roster's own
//! statuses matter. A cross-session roster is `not_found` at the ladder —
//! the wrong session can neither check in nor expose the row.
//!
//! ## The other tight spots
//!
//! - `capacity` is a positive seat count or `null`; registration at the
//!   boundary is `capacity_exceeded`, refused atomically.
//! - `(tenant, session, contact)` and `(tenant, ticket_code)` stay unique;
//!   a duplicate registration or ticket is a `conflict`, never a second row.
//! - A session window must ADVANCE (`endTime > startTime`); a window that
//!   does not is a 400-class refusal, not a row a scanner can never claim.
//! - The status targets are closed vocabularies (a roster cannot be moved
//!   to `attended` by the status arm — only a check-in does that).
//!
//! The corrected deltas are recorded in `docs/rust/KNOWN_DIFFERENCES.md`
//! (D22/D23) and in `docs/rust/archxy2/ERP_ACCEPTANCE_MATRIX.md`.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::purchasing::ValidationError;
use super::{bounded_title, trimmed, valid_uuid};

/// The permission vocabulary migration 001's CHECK allows.
pub const ATTENDANCE_PERMISSIONS: &[&str] = &[
    "attendance.read",
    "attendance.write",
    "attendance.checkin",
    "audit.read",
];

/// The session statuses (the TS `SESSION_STATUSES`).
pub const SESSION_DRAFT: &str = "draft";
pub const SESSION_SCHEDULED: &str = "scheduled";
pub const SESSION_IN_PROGRESS: &str = "in_progress";
pub const SESSION_COMPLETED: &str = "completed";
pub const SESSION_CANCELED: &str = "canceled";

/// The roster roles (the TS `ROSTER_ROLES`).
pub const ROLE_ATTENDEE: &str = "attendee";
pub const ROLE_INSTRUCTOR: &str = "instructor";
pub const ROLE_SPEAKER: &str = "speaker";
pub const ROLE_STAFF: &str = "staff";

/// The roster statuses (the TS `ROSTER_STATUSES`).
pub const ROSTER_REGISTERED: &str = "registered";
pub const ROSTER_WAITLISTED: &str = "waitlisted";
pub const ROSTER_CONFIRMED: &str = "confirmed";
pub const ROSTER_ATTENDED: &str = "attended";
pub const ROSTER_CANCELED: &str = "canceled";
pub const ROSTER_NO_SHOW: &str = "no_show";

/// The check-in methods (the TS `CHECK_IN_METHODS`).
pub const METHOD_QR_SCAN: &str = "qr_scan";
pub const METHOD_MANUAL_OPERATOR: &str = "manual_operator";
pub const METHOD_KIOSK: &str = "kiosk";

/// Field bounds (the TS schemas' own numbers).
pub const TITLE_MAX: usize = 200;
pub const SESSION_CODE_MAX: usize = 50;
pub const LOCATION_MAX: usize = 300;
pub const TICKET_CODE_MAX: usize = 100;
pub const EVIDENCE_REF_MAX: usize = 300;
pub const IDEMPOTENCY_KEY_MAX: usize = 100;
pub const REASON_MAX: usize = 500;
pub const TENANT_MAX: usize = 100;

// --- Vocabularies -------------------------------------------------------------

/// The session status — the TS enum, parsed without invention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Draft,
    Scheduled,
    InProgress,
    Completed,
    Canceled,
}

impl SessionStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            SESSION_DRAFT => Some(Self::Draft),
            SESSION_SCHEDULED => Some(Self::Scheduled),
            SESSION_IN_PROGRESS => Some(Self::InProgress),
            SESSION_COMPLETED => Some(Self::Completed),
            SESSION_CANCELED => Some(Self::Canceled),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => SESSION_DRAFT,
            Self::Scheduled => SESSION_SCHEDULED,
            Self::InProgress => SESSION_IN_PROGRESS,
            Self::Completed => SESSION_COMPLETED,
            Self::Canceled => SESSION_CANCELED,
        }
    }

    /// Whether a check-in is admissible while the session is in this state —
    /// the TS's `scheduled`/`in_progress` pair.
    pub fn is_open(self) -> bool {
        matches!(self, Self::Scheduled | Self::InProgress)
    }
}

/// The roster status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RosterStatus {
    Registered,
    Waitlisted,
    Confirmed,
    Attended,
    Canceled,
    NoShow,
}

impl RosterStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            ROSTER_REGISTERED => Some(Self::Registered),
            ROSTER_WAITLISTED => Some(Self::Waitlisted),
            ROSTER_CONFIRMED => Some(Self::Confirmed),
            ROSTER_ATTENDED => Some(Self::Attended),
            ROSTER_CANCELED => Some(Self::Canceled),
            ROSTER_NO_SHOW => Some(Self::NoShow),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Registered => ROSTER_REGISTERED,
            Self::Waitlisted => ROSTER_WAITLISTED,
            Self::Confirmed => ROSTER_CONFIRMED,
            Self::Attended => ROSTER_ATTENDED,
            Self::Canceled => ROSTER_CANCELED,
            Self::NoShow => ROSTER_NO_SHOW,
        }
    }
}

// --- The corrected policy (A04) -----------------------------------------------

/// Why a registration is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationRefusal {
    /// The session is `canceled` or `completed` — registration is closed.
    SessionClosed(&'static str),
    /// The session's seat allocation is spent (the TS `capacity_exceeded`).
    CapacityExceeded,
}

/// The registration gate: the session must be open and the seat allocation
/// must have room. The caller holds the session row's lock, so the check and
/// the counter update are one atomic step.
pub fn registration_refusal(
    session: SessionStatus,
    capacity: Option<i64>,
    enrolled: i64,
) -> Option<RegistrationRefusal> {
    if matches!(session, SessionStatus::Canceled | SessionStatus::Completed) {
        return Some(RegistrationRefusal::SessionClosed(session.as_str()));
    }
    match capacity {
        Some(capacity) if enrolled >= capacity => Some(RegistrationRefusal::CapacityExceeded),
        _ => None,
    }
}

/// Why a check-in is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckInRefusal {
    /// The roster does not belong to the supplied session — the A04
    /// correction. A valid UUID from another session proves nothing.
    RosterNotInSession,
    /// The session is not open (`draft`/`completed`/`canceled`).
    SessionClosed(&'static str),
    /// The participant is already checked in (the TS `already_checked_in`).
    AlreadyCheckedIn,
    /// The registration was canceled (the TS's `bad_request`).
    RosterCanceled,
}

/// The check-in gate. The BINDING is checked first: a roster that does not
/// belong to the supplied session is refused before its own status can be
/// read as authority. Then the session's state, then the roster's.
pub fn check_in_refusal(
    session: SessionStatus,
    roster: RosterStatus,
    roster_belongs_to_session: bool,
) -> Option<CheckInRefusal> {
    if !roster_belongs_to_session {
        return Some(CheckInRefusal::RosterNotInSession);
    }
    if !session.is_open() {
        return Some(CheckInRefusal::SessionClosed(session.as_str()));
    }
    match roster {
        RosterStatus::Attended => Some(CheckInRefusal::AlreadyCheckedIn),
        RosterStatus::Canceled => Some(CheckInRefusal::RosterCanceled),
        RosterStatus::Registered
        | RosterStatus::Waitlisted
        | RosterStatus::Confirmed
        | RosterStatus::NoShow => None,
    }
}

/// The session window must advance. Both sides ride the canonical UTC
/// grammar ([`valid_datetime_utc`]): the fixed-width seconds compare
/// lexicographically, and the fractional digits compare as decimals (padded
/// right with zeros), so `…:00.5Z` sits after `…:00.25Z`.
pub fn session_window_advances(start: &str, end: &str) -> bool {
    let (start_seconds, start_fraction) = instant_parts(start);
    let (end_seconds, end_fraction) = instant_parts(end);
    match start_seconds.cmp(end_seconds) {
        Ordering::Less => true,
        Ordering::Greater => false,
        Ordering::Equal => {
            let width = start_fraction.len().max(end_fraction.len());
            let pad = |fraction: &str| {
                let mut padded = String::with_capacity(width);
                padded.push_str(fraction);
                while padded.len() < width {
                    padded.push('0');
                }
                padded
            };
            pad(start_fraction) < pad(end_fraction)
        }
    }
}

fn instant_parts(value: &str) -> (&str, &str) {
    let body = value.strip_suffix('Z').unwrap_or(value);
    match body.split_once('.') {
        Some((seconds, fraction)) => (seconds, fraction),
        None => (body, ""),
    }
}

// --- The commands -------------------------------------------------------------

/// The five command arms — the TS `attendanceMutationSchema`.
///
/// The derive is the fingerprint's: the host digests the typed arm document
/// the parse produced (`serde_json::to_value(&mutation.command)`), exactly
/// like the sibling R07 cohorts.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AttendanceCommand {
    CreateSession {
        command_id: String,
        title: String,
        session_code: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        capacity: Option<i64>,
        start_time: String,
        end_time: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        location: Option<String>,
    },
    RegisterParticipant {
        command_id: String,
        session_id: String,
        contact_id: String,
        role: String,
        ticket_code: String,
    },
    RecordCheckIn {
        command_id: String,
        session_id: String,
        roster_id: String,
        method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        evidence_ref: Option<String>,
        idempotency_key: String,
    },
    UpdateRosterStatus {
        command_id: String,
        roster_id: String,
        expected_revision: i64,
        target_status: String,
        reason: String,
    },
    ChangeSessionStatus {
        command_id: String,
        session_id: String,
        expected_revision: i64,
        target_status: String,
        reason: String,
    },
}

impl AttendanceCommand {
    pub fn command_id(&self) -> &str {
        match self {
            Self::CreateSession { command_id, .. }
            | Self::RegisterParticipant { command_id, .. }
            | Self::RecordCheckIn { command_id, .. }
            | Self::UpdateRosterStatus { command_id, .. }
            | Self::ChangeSessionStatus { command_id, .. } => command_id,
        }
    }

    /// The permission the TS repository's `grant()` demands for this arm.
    pub fn required_permission(&self) -> &'static str {
        match self {
            Self::RecordCheckIn { .. } => "attendance.checkin",
            Self::CreateSession { .. }
            | Self::RegisterParticipant { .. }
            | Self::UpdateRosterStatus { .. }
            | Self::ChangeSessionStatus { .. } => "attendance.write",
        }
    }
}

/// The parsed mutation — the arm plus the metadata the arms carry.
#[derive(Debug, Clone, PartialEq)]
pub struct AttendanceMutation {
    pub command: AttendanceCommand,
}

// --- The access envelope ------------------------------------------------------

/// The caller's effective access — the TS `AttendanceAccess`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttendanceAccess {
    pub permissions: Vec<String>,
    pub owner_id: String,
    pub scope: String,
}

impl AttendanceAccess {
    pub fn holds(&self, wanted: &str) -> bool {
        self.permissions.iter().any(|held| held == wanted)
    }
}

// --- The receipt --------------------------------------------------------------

/// The receipt document — the TS `attendanceReceiptSchema`.
///
/// The optional ids are `Option<Option<…>>` because the TS spells them
/// `.nullable().optional()`: absent, explicit null, and a value are three
/// different documents and the port keeps the difference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttendanceReceipt {
    pub command_id: String,
    pub tenant_id: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roster_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_in_id: Option<Option<String>>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    pub idempotency_replay: bool,
    pub executed_at: String,
}

impl AttendanceReceipt {
    /// Parse a stored receipt document (the command-status read's job).
    pub fn parse(raw: &Value, replay: bool) -> Result<Self, ValidationError> {
        let object = raw.as_object().ok_or(ValidationError("receipt"))?;
        let command_id = object
            .get("commandId")
            .and_then(Value::as_str)
            .filter(|value| valid_uuid(value))
            .ok_or(ValidationError("commandId"))?;
        let tenant_id = object
            .get("tenantId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && value.len() <= TENANT_MAX)
            .ok_or(ValidationError("tenantId"))?;
        let action = object
            .get("action")
            .and_then(Value::as_str)
            .ok_or(ValidationError("action"))?;
        let status = object
            .get("status")
            .and_then(Value::as_str)
            .ok_or(ValidationError("status"))?;
        let executed_at = object
            .get("executedAt")
            .and_then(Value::as_str)
            .filter(|value| valid_datetime_utc(value))
            .ok_or(ValidationError("executedAt"))?;
        let optional_id = |key: &'static str| -> Result<Option<Option<String>>, ValidationError> {
            match object.get(key) {
                None | Some(Value::Null) => Ok(Some(None)),
                Some(Value::String(value)) if valid_uuid(value) => Ok(Some(Some(value.clone()))),
                _ => Err(ValidationError(key)),
            }
        };
        let revision = match object.get("revision") {
            None => None,
            Some(Value::Number(number)) => {
                let value = number.as_i64().ok_or(ValidationError("revision"))?;
                if value < 0 {
                    return Err(ValidationError("revision"));
                }
                Some(value)
            }
            _ => return Err(ValidationError("revision")),
        };
        Ok(Self {
            command_id: command_id.to_string(),
            tenant_id: tenant_id.to_string(),
            action: action.to_string(),
            session_id: optional_id("sessionId")?,
            roster_id: optional_id("rosterId")?,
            check_in_id: optional_id("checkInId")?,
            status: status.to_string(),
            revision,
            idempotency_replay: replay,
            executed_at: executed_at.to_string(),
        })
    }
}

// --- The command status -------------------------------------------------------

/// The command-status document — the TS `attendanceCommandStatusSchema`.
/// The receipt rides a Box for the same reason the billing/subscription
/// statuses do: the `unknown` arm names none of the recorded arm's cells,
/// and the wire shape is unchanged by the indirection.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum AttendanceCommandStatus {
    Unknown,
    Recorded { receipt: Box<AttendanceReceipt> },
}

// --- The read projections -----------------------------------------------------

/// A session row — the TS `sessionSchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub tenant_id: String,
    pub title: String,
    pub session_code: String,
    pub capacity: Option<i64>,
    pub start_time: String,
    pub end_time: String,
    pub status: String,
    pub location: Option<String>,
    pub enrolled_count: i64,
    pub attended_count: i64,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// A roster row — the TS `rosterEntrySchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterEntry {
    pub id: String,
    pub tenant_id: String,
    pub session_id: String,
    pub contact_id: String,
    pub role: String,
    pub status: String,
    pub ticket_code: String,
    pub checked_in_at: Option<String>,
    pub check_in_method: Option<String>,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// A check-in event — the TS `checkInRecordSchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckInRecord {
    pub id: String,
    pub tenant_id: String,
    pub roster_id: String,
    pub session_id: String,
    pub contact_id: String,
    pub method: String,
    pub evidence_ref: Option<String>,
    pub idempotency_key: String,
    pub recorded_by: String,
    pub recorded_at: String,
}

/// One session with its roster and its check-in ledger — the TS
/// `sessionDetailSchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub session: Session,
    pub roster: Vec<RosterEntry>,
    pub check_ins: Vec<CheckInRecord>,
}

/// The visit list query — the TS `sessionPageQuerySchema`.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionPageQuery {
    pub limit: i64,
    pub offset: i64,
    pub status: Option<String>,
}

// --- The grammar --------------------------------------------------------------

/// Parse one mutation document. The strict walk refuses an arm member the
/// TS `.strict()` object would refuse, and the closed vocabularies refuse
/// any target the schemas do not name.
pub fn parse_mutation(raw: &Value) -> Result<AttendanceMutation, ValidationError> {
    let command = raw.as_object().ok_or(ValidationError("type"))?;
    let arm = command.get("type").and_then(Value::as_str);
    let Some(arm) = arm else {
        return Err(ValidationError("type"));
    };
    let parsed = match arm {
        "create_session" => {
            let start_time = datetime_member(command, "startTime")?;
            let end_time = datetime_member(command, "endTime")?;
            if !session_window_advances(&start_time, &end_time) {
                return Err(ValidationError("endTime"));
            }
            AttendanceCommand::CreateSession {
                command_id: uuid_member(command, "commandId")?,
                title: title_member(command, "title", TITLE_MAX)?,
                session_code: title_member(command, "sessionCode", SESSION_CODE_MAX)?,
                capacity: optional_positive_member(command, "capacity")?,
                start_time,
                end_time,
                location: optional_text_member(command, "location", LOCATION_MAX)?,
            }
        }
        "register_participant" => AttendanceCommand::RegisterParticipant {
            command_id: uuid_member(command, "commandId")?,
            session_id: uuid_member(command, "sessionId")?,
            contact_id: uuid_member(command, "contactId")?,
            role: match command.get("role") {
                None => ROLE_ATTENDEE.to_string(),
                Some(Value::String(value)) if ROSTER_ROLES.contains(&value.as_str()) => {
                    value.clone()
                }
                _ => return Err(ValidationError("role")),
            },
            ticket_code: title_member(command, "ticketCode", TICKET_CODE_MAX)?,
        },
        "record_check_in" => AttendanceCommand::RecordCheckIn {
            command_id: uuid_member(command, "commandId")?,
            session_id: uuid_member(command, "sessionId")?,
            roster_id: uuid_member(command, "rosterId")?,
            method: vocabulary_member(command, "method", CHECK_IN_METHODS)?,
            evidence_ref: optional_text_member(command, "evidenceRef", EVIDENCE_REF_MAX)?,
            idempotency_key: title_member(command, "idempotencyKey", IDEMPOTENCY_KEY_MAX)?,
        },
        "update_roster_status" => AttendanceCommand::UpdateRosterStatus {
            command_id: uuid_member(command, "commandId")?,
            roster_id: uuid_member(command, "rosterId")?,
            expected_revision: revision_member(command, "expectedRevision")?,
            target_status: vocabulary_member(
                command,
                "targetStatus",
                &[
                    ROSTER_CONFIRMED,
                    ROSTER_CANCELED,
                    ROSTER_NO_SHOW,
                    ROSTER_WAITLISTED,
                ],
            )?,
            reason: title_member(command, "reason", REASON_MAX)?,
        },
        "change_session_status" => AttendanceCommand::ChangeSessionStatus {
            command_id: uuid_member(command, "commandId")?,
            session_id: uuid_member(command, "sessionId")?,
            expected_revision: revision_member(command, "expectedRevision")?,
            target_status: vocabulary_member(
                command,
                "targetStatus",
                &[
                    SESSION_SCHEDULED,
                    SESSION_IN_PROGRESS,
                    SESSION_COMPLETED,
                    SESSION_CANCELED,
                ],
            )?,
            reason: title_member(command, "reason", REASON_MAX)?,
        },
        _ => return Err(ValidationError("type")),
    };
    // The strict walk: every member of the arm must be one this grammar
    // knows. The TS `.strict()` objects refuse the same extra members.
    let allowed: &[&str] = match &parsed {
        AttendanceCommand::CreateSession { .. } => &[
            "type",
            "commandId",
            "title",
            "sessionCode",
            "capacity",
            "startTime",
            "endTime",
            "location",
        ],
        AttendanceCommand::RegisterParticipant { .. } => &[
            "type",
            "commandId",
            "sessionId",
            "contactId",
            "role",
            "ticketCode",
        ],
        AttendanceCommand::RecordCheckIn { .. } => &[
            "type",
            "commandId",
            "sessionId",
            "rosterId",
            "method",
            "evidenceRef",
            "idempotencyKey",
        ],
        AttendanceCommand::UpdateRosterStatus { .. } => &[
            "type",
            "commandId",
            "rosterId",
            "expectedRevision",
            "targetStatus",
            "reason",
        ],
        AttendanceCommand::ChangeSessionStatus { .. } => &[
            "type",
            "commandId",
            "sessionId",
            "expectedRevision",
            "targetStatus",
            "reason",
        ],
    };
    for key in command.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(ValidationError("member"));
        }
    }
    Ok(AttendanceMutation { command: parsed })
}

const ROSTER_ROLES: &[&str] = &[ROLE_ATTENDEE, ROLE_INSTRUCTOR, ROLE_SPEAKER, ROLE_STAFF];
const CHECK_IN_METHODS: &[&str] = &[METHOD_QR_SCAN, METHOD_MANUAL_OPERATOR, METHOD_KIOSK];

fn uuid_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<String, ValidationError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| valid_uuid(value))
        .map(str::to_string)
        .ok_or(ValidationError(key))
}

fn title_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
    max: usize,
) -> Result<String, ValidationError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| bounded_title(value, max))
        .ok_or(ValidationError(key))
}

fn optional_text_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
    max: usize,
) -> Result<Option<String>, ValidationError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            let trimmed = trimmed(value);
            if trimmed.len() > max {
                return Err(ValidationError(key));
            }
            Ok(Some(trimmed.to_string()))
        }
        _ => Err(ValidationError(key)),
    }
}

/// `capacity` — `.int().positive().nullable().optional()`.
fn optional_positive_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<Option<i64>, ValidationError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            let value = number.as_i64().ok_or(ValidationError(key))?;
            if value <= 0 {
                return Err(ValidationError(key));
            }
            Ok(Some(value))
        }
        _ => Err(ValidationError(key)),
    }
}

fn revision_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<i64, ValidationError> {
    match object.get(key) {
        Some(Value::Number(number)) => {
            let value = number.as_i64().ok_or(ValidationError(key))?;
            if value < 0 {
                return Err(ValidationError(key));
            }
            Ok(value)
        }
        _ => Err(ValidationError(key)),
    }
}

fn vocabulary_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
    allowed: &[&str],
) -> Result<String, ValidationError> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(ValidationError(key))?;
    if allowed.contains(&value) {
        Ok(value.to_string())
    } else {
        Err(ValidationError(key))
    }
}

fn datetime_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<String, ValidationError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| valid_datetime_utc(value))
        .map(str::to_string)
        .ok_or(ValidationError(key))
}

/// The ISO-8601 UTC instant the TS `.datetime()` accepts:
/// `YYYY-MM-DDTHH:MM:SS[.fff]Z` — a `Z` suffix, no offset, real calendar
/// fields. Module-local like the subscriptions cohort's copy: the two
/// grammars are frozen independently.
pub fn valid_datetime_utc(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20 || bytes.len() > 30 {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    if !digits(0..4) || bytes[4] != b'-' || !digits(5..7) || bytes[7] != b'-' || !digits(8..10) {
        return false;
    }
    if bytes[10] != b'T'
        || !digits(11..13)
        || bytes[13] != b':'
        || !digits(14..16)
        || bytes[16] != b':'
    {
        return false;
    }
    if !digits(17..19) {
        return false;
    }
    let mut cursor = 19;
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        let start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        if cursor == start {
            return false;
        }
    }
    if bytes.get(cursor) != Some(&b'Z') {
        return false;
    }
    if cursor + 1 != bytes.len() {
        return false;
    }
    let month: u32 = value[5..7].parse().unwrap_or(0);
    let day: u32 = value[8..10].parse().unwrap_or(0);
    let hour: u32 = value[11..13].parse().unwrap_or(0);
    let minute: u32 = value[14..16].parse().unwrap_or(0);
    let second: u32 = value[17..19].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day) && hour < 24 && minute < 60 && second < 60
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mutation(value: Value) -> Result<AttendanceMutation, ValidationError> {
        parse_mutation(&value)
    }

    fn create(command_id: &str) -> Value {
        json!({
            "type": "create_session",
            "commandId": command_id,
            "title": "Fullstack Bootcamp Week 1",
            "sessionCode": "FS-BC-W1",
            "startTime": "2026-10-15T09:00:00.000Z",
            "endTime": "2026-10-15T17:00:00.000Z"
        })
    }

    #[test]
    fn create_session_defaults_follow_the_baseline() {
        let parsed = mutation(create("6b240400-9999-4999-9999-000000000001"))
            .expect("the create arm parses");
        match parsed.command {
            AttendanceCommand::CreateSession {
                capacity, location, ..
            } => {
                assert_eq!(capacity, None);
                assert_eq!(location, None);
            }
            other => panic!("unexpected arm: {other:?}"),
        }
    }

    #[test]
    fn the_strict_grammar_refuses_an_extra_member() {
        let mut document = create("6b240400-9999-4999-9999-000000000001");
        document["extra"] = json!(true);
        assert!(mutation(document).is_err(), "an extra member is a refusal");
    }

    #[test]
    fn capacity_must_be_positive_when_present() {
        for capacity in [json!(0), json!(-5), json!(1.5), json!("ten")] {
            let mut document = create("6b240400-9999-4999-9999-000000000001");
            document["capacity"] = capacity;
            assert!(
                mutation(document).is_err(),
                "capacity must be a positive int"
            );
        }
        let mut document = create("6b240400-9999-4999-9999-000000000001");
        document["capacity"] = json!(35);
        let parsed = mutation(document).expect("a positive capacity parses");
        match parsed.command {
            AttendanceCommand::CreateSession { capacity, .. } => assert_eq!(capacity, Some(35)),
            other => panic!("unexpected arm: {other:?}"),
        }
    }

    #[test]
    fn the_session_window_must_advance() {
        // Same instant, reversed window, and a fractional reversal: every
        // one refuses at the parse.
        for (start, end) in [
            ("2026-10-15T09:00:00.000Z", "2026-10-15T09:00:00.000Z"),
            ("2026-10-15T17:00:00.000Z", "2026-10-15T09:00:00.000Z"),
            ("2026-10-15T09:00:00.500Z", "2026-10-15T09:00:00.250Z"),
        ] {
            let mut document = create("6b240400-9999-4999-9999-000000000001");
            document["startTime"] = json!(start);
            document["endTime"] = json!(end);
            assert!(mutation(document).is_err(), "{start} -> {end} must refuse");
        }
        // And the fractional order itself: .25 is before .5.
        assert!(session_window_advances(
            "2026-10-15T09:00:00.25Z",
            "2026-10-15T09:00:00.5Z"
        ));
        assert!(!session_window_advances(
            "2026-10-15T09:00:00.5Z",
            "2026-10-15T09:00:00.25Z"
        ));
    }

    #[test]
    fn register_defaults_the_attendee_role() {
        let parsed = mutation(json!({
            "type": "register_participant",
            "commandId": "6b240400-9999-4999-9999-000000000002",
            "sessionId": "6b240400-9999-4999-9999-0000000000a1",
            "contactId": "6b240400-9999-4999-9999-0000000000a2",
            "ticketCode": "TCK-99201"
        }))
        .expect("the register arm parses");
        match parsed.command {
            AttendanceCommand::RegisterParticipant { role, .. } => {
                assert_eq!(role, ROLE_ATTENDEE)
            }
            other => panic!("unexpected arm: {other:?}"),
        }
    }

    #[test]
    fn the_a04_policy_binds_the_roster_to_the_supplied_session() {
        // A cross-session roster is refused BEFORE its status is consulted:
        // even an `attended` row from another session proves nothing here.
        for roster in [
            RosterStatus::Registered,
            RosterStatus::Attended,
            RosterStatus::Canceled,
        ] {
            assert_eq!(
                check_in_refusal(SessionStatus::InProgress, roster, false),
                Some(CheckInRefusal::RosterNotInSession)
            );
        }
        // A bound roster on an open session walks; on a closed session it
        // refuses with the session's own class.
        assert_eq!(
            check_in_refusal(SessionStatus::Scheduled, RosterStatus::Registered, true),
            None
        );
        assert_eq!(
            check_in_refusal(SessionStatus::Completed, RosterStatus::Registered, true),
            Some(CheckInRefusal::SessionClosed(SESSION_COMPLETED))
        );
        assert_eq!(
            check_in_refusal(SessionStatus::InProgress, RosterStatus::Attended, true),
            Some(CheckInRefusal::AlreadyCheckedIn)
        );
    }

    #[test]
    fn registration_gates_on_the_session_and_its_seats() {
        assert_eq!(
            registration_refusal(SessionStatus::Canceled, Some(10), 0),
            Some(RegistrationRefusal::SessionClosed(SESSION_CANCELED))
        );
        assert_eq!(
            registration_refusal(SessionStatus::Scheduled, Some(2), 2),
            Some(RegistrationRefusal::CapacityExceeded)
        );
        assert_eq!(
            registration_refusal(SessionStatus::Scheduled, Some(2), 1),
            None
        );
        assert_eq!(
            registration_refusal(SessionStatus::Scheduled, None, 99),
            None
        );
    }

    #[test]
    fn the_closed_target_vocabularies_refuse_invention() {
        let mut roster = json!({
            "type": "update_roster_status",
            "commandId": "6b240400-9999-4999-9999-000000000003",
            "rosterId": "6b240400-9999-4999-9999-0000000000b1",
            "expectedRevision": 1,
            "targetStatus": "attended",
            "reason": "operator correction"
        });
        assert!(
            mutation(roster.clone()).is_err(),
            "attended is check-in only"
        );
        roster["targetStatus"] = json!("confirmed");
        assert!(mutation(roster).is_ok());
        let session = json!({
            "type": "change_session_status",
            "commandId": "6b240400-9999-4999-9999-000000000004",
            "sessionId": "6b240400-9999-4999-9999-0000000000c1",
            "expectedRevision": 1,
            "targetStatus": "draft",
            "reason": "rewind"
        });
        assert!(mutation(session).is_err(), "draft is not a status target");
    }

    #[test]
    fn the_receipt_and_status_documents_keep_their_wire_shape() {
        let receipt = AttendanceReceipt {
            command_id: "6b240400-9999-4999-9999-000000000001".to_string(),
            tenant_id: "acme-retail".to_string(),
            action: "create_session".to_string(),
            session_id: Some(Some("6b240400-9999-4999-9999-0000000000a1".to_string())),
            roster_id: None,
            check_in_id: Some(None),
            status: SESSION_SCHEDULED.to_string(),
            revision: Some(1),
            idempotency_replay: false,
            executed_at: "2026-10-15T09:00:00.000Z".to_string(),
        };
        let document = serde_json::to_value(&receipt).expect("the receipt serializes");
        assert_eq!(
            document["sessionId"],
            json!("6b240400-9999-4999-9999-0000000000a1")
        );
        assert!(document.get("rosterId").is_none(), "absent stays absent");
        assert_eq!(document["checkInId"], Value::Null, "explicit null rides");
        assert_eq!(document["idempotencyReplay"], json!(false));
        let status = AttendanceCommandStatus::Recorded {
            receipt: Box::new(receipt),
        };
        let document = serde_json::to_value(&status).expect("the status serializes");
        assert_eq!(document["status"], json!("recorded"));
        let unknown = serde_json::to_value(AttendanceCommandStatus::Unknown)
            .expect("the unknown status serializes");
        assert_eq!(unknown, json!({ "status": "unknown" }));
    }
}
