//! The accommodation domain contracts — the Archxy 2 port of
//! `packages/accommodation/src/contracts.ts` (R05 second cohort: room-night
//! stays composed on the scheduling occupancy kernel).
//!
//! ## The wire grammar
//!
//! Same family as the scheduling port: a strict `{requestId, command}`
//! mutation whose `command` is an `.strict()` discriminated union on
//! `operation` — eleven arms (property.create, room-type.create,
//! room-type.set-rate, room.create, room.set-active, room.housekeeping,
//! stay.hold, stay.confirm, stay.check-in, stay.check-out, stay.cancel).
//! The parse bakes in every zod transform the TS document carries:
//!
//! - `checkInTime`/`checkOutTime` are `.default('15:00')/('11:00')` — an
//!   absent member parses as the default;
//! - `holdSeconds` is `.default(900)`, capped 1–1800;
//! - `title`/`reason`/`guestName` are `.trim()`-min-1-max-bound (the
//!   trimmed value travels);
//! - `property.create` carries the refine `checkOutTime <= checkInTime`
//!   ("checkout must not be after the next arrival time");
//! - `stay.hold` carries the refine `validNights` — 1–31 calendar nights
//!   between the arrival and departure dates.
//!
//! ## The calendar helpers (`packages/scheduling/src/calendar.ts`, riding
//! this cohort per the ERP_SCOPE reservation)
//!
//! `calendarDateSchema` is `z.iso.date()` with the `2000–2099` + `2100`
//! century refine — and zod's `iso.date()` regex is calendar-precise
//! (leap-year rule, per-month day ranges), so a parsed calendar date IS a
//! real Gregorian date and this port validates it with pure civil
//! arithmetic (`days_from_civil`), wasm-clean, no `Date.parse`.
//! `calendarNights` is the same pure civil span (1–31 nights).
//!
//! The timezone-dependent helpers (`calendarToday`, the DST-exacting
//! `resolveCalendarInstant`) canNOT live in the pure domain: they enumerate
//! a zone's offsets, which needs a tz database. The TS side resolves them
//! through live `Intl`; this port resolves them through PostgreSQL's
//! bundled IANA tzdata — `to_char(instant AT TIME ZONE zone, …)` is the
//! same authoritative family — inside the adapters, using the TS probe
//! algorithm verbatim (enumerate −48..+48 hour offsets, require exactly
//! one round-tripping instant, refuse ambiguity as
//! `CalendarTimeError`). That split is a recorded divergence (module
//! header, `KNOWN_DIFFERENCES` ledger), never silent.
//!
//! ## The error vocabulary
//!
//! `AccommodationError('forbidden'|'not_found'|'conflict'|'unavailable')`
//! rides the shared [`ErpError`] carrier (403/404/409/503), and
//! [`CalendarTimeError`] — the "the requested local time is missing or
//! ambiguous in its timezone" verdict — rises as a conflict, exactly as
//! the TS `transaction()` catch whitelist does.

use serde::Serialize;
use serde_json::Value;

pub use super::purchasing::{
    PrincipalKind, PurchasePrincipal as AccommodationPrincipal, ValidationError,
};
pub use super::{ErpError, ErpScope};

/// The accommodation permission vocabulary — `ACCOMMODATION_PERMISSIONS`.
/// Strings, not an enum: membership rows store them and the grant gates
/// test membership of the list, like every other ERP cohort.
pub const ACCOMMODATION_PERMISSIONS: &[&str] = &[
    "properties.read",
    "properties.write",
    "rooms.read",
    "rooms.write",
    "housekeeping.write",
    "rooms.inspect",
    "stays.read",
    "stays.write",
    "audit.read",
];

// --- Vocabulary constants (contracts.ts's constants, mirrored) --------------

/// Bounded text (`title`/`guestName`): trimmed 1–160.
pub const ACCOMMODATION_TITLE_MAX: usize = 160;
/// Bounded `reason`: trimmed 1–2000.
pub const ACCOMMODATION_REASON_MAX: usize = 2000;
/// Bounded room `code`: trimmed 1–40.
pub const ACCOMMODATION_CODE_MAX: usize = 40;
/// The revision ceiling every mutation's optimistic check guards.
pub const ACCOMMODATION_REVISION_MAX: i64 = 2_147_483_647;
/// Rate ceiling (`nightlyRate`: 0–1,000,000,000 IDR).
pub const ACCOMMODATION_RATE_MAX: i64 = 1_000_000_000;
/// Total-quote ceiling (`totalEstimate`: ≤ 31 billion IDR — 31 nights
/// × a billion IDR nightly).
pub const ACCOMMODATION_TOTAL_MAX: i64 = 31_000_000_000;
/// Guest bounds (stays, room types, availability queries).
pub const ACCOMMODATION_GUESTS_MAX: i64 = 16;
/// Wait: 1–1000 (the rate's floor is 0 — the ceiling is the meaningful one).
pub const ACCOMMODATION_NIGHTS_MAX: i64 = 31;
/// `holdSeconds`: `.default(900)`, 1–1800.
pub const HOLD_SECONDS_DEFAULT: i64 = 900;
pub const HOLD_SECONDS_MAX: i64 = 1800;
/// Calendar/timezone vocabulary (`calendar.ts` + the property's timezone).
pub const CALENDAR_TIMEZONE_MAX: usize = 100;

// --- The calendar vocabulary (calendar.ts, pure-civil half) -------------------

/// The `CalendarTimeError` TS class — "the requested local time is missing
/// or ambiguous in its timezone", raised for an unresolvable span too
/// (`calendarNights`' 0/32-night refusals). Adapters map it to conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarTimeError;

/// A renewable calendar date — parsed through the exact zod `iso.date()`
/// grammar (`YYYY-MM-DD`, the Gregorian leap rule, per-month day ranges) +
/// the century refine `2000–2099 | 2100`. `epoch_days` are civil days
/// (the same ordinal `Date.parse(+ 'T00:00:00Z')` yields, in days).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CivilDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub epoch_days: i64,
}

impl CivilDate {
    /// The canonical `YYYY-MM-DD` render (`toISOString().slice(0, 10)`).
    pub fn render(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Howard Hinnant's `days_from_civil` — the civil-day ordinal of a
/// Gregorian date. Integral, month-accurate, leap-rule-exact.
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if month > 2 {
        month as i64 - 3
    } else {
        month as i64 + 9
    }; // [0, 11]
    let doy = (153 * mp + 2) / 5 + day as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// The Gregorian leap rule — the exact predicate zod's `iso.date()`
/// regex's leap alternation encodes (`\d\d[2468][048]`, `\d\d0[48]`,
/// `[02468][048]00`, `[13579][26]00`).
fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days in a month, the regex's per-month alternation.
fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// `calendarDateSchema.parse` — the zod grammar + century refine. Weights:
/// exactly `YYYY-MM-DD` digits, real Gregorian dates, years 2000–2099 or
/// 2100 (the `2[01]`-century band the TS refine pins).
pub fn parse_calendar_date(value: &str) -> Option<CivilDate> {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let digits = |slice: &[u8]| -> Option<u32> {
        let mut value = 0u32;
        for &b in slice {
            if !b.is_ascii_digit() {
                return None;
            }
            value = value * 10 + u32::from(b - b'0');
        }
        Some(value)
    };
    let year_signed = digits(&bytes[..4])?;
    let month = digits(&bytes[5..7])?;
    let day = digits(&bytes[8..10])?;
    if month == 0 || month > 12 || day == 0 {
        return None;
    }
    if day > days_in_month(year_signed as i32, month) {
        return None;
    }
    // The century refine `/^20\d\d-|^2100-/`.
    if !(2000..=2100).contains(&year_signed) {
        return None;
    }
    Some(CivilDate {
        year: year_signed as i32,
        month,
        day,
        epoch_days: days_from_civil(year_signed as i32, month, day),
    })
}

/// `calendarTimeSchema.parse` — `^(?:[01]\d|2[0-3]):[0-5]\d$`, hour + minute.
/// (Not the instant grammar: this is a DAILY time-of-day, which the
/// property's check-in/check-out and the stay's resolved edges carry.)
pub fn parse_calendar_time(value: &str) -> Option<(u32, u32)> {
    let bytes = value.as_bytes();
    if bytes.len() != 5 || bytes[2] != b':' {
        return None;
    }
    let digit = |b: u8| b.is_ascii_digit();
    if !(digit(bytes[0]) && digit(bytes[1]) && digit(bytes[3]) && digit(bytes[4])) {
        return None;
    }
    let hour = (u32::from(bytes[0] - b'0')) * 10 + u32::from(bytes[1] - b'0');
    let minute = (u32::from(bytes[3] - b'0')) * 10 + u32::from(bytes[4] - b'0');
    if hour > 23 || minute > 59 {
        return None;
    }
    Some((hour, minute))
}

/// `calendarNights(arrival, departure)` — the pure-civil span: both dates
/// parse, and the distance is 1–31 days. Returns each night's date.
pub fn calendar_nights(arrival: &str, departure: &str) -> Result<Vec<String>, CalendarTimeError> {
    let first = parse_calendar_date(arrival).ok_or(CalendarTimeError)?;
    let last = parse_calendar_date(departure).ok_or(CalendarTimeError)?;
    let count = last.epoch_days - first.epoch_days;
    if !(1..=ACCOMMODATION_NIGHTS_MAX).contains(&count) {
        return Err(CalendarTimeError);
    }
    Ok((0..count)
        .map(|offset| {
            // The TS renders each night through `toISOString()` — the same
            // civil arithmetic forward.
            let ymd = civil_from_days(first.epoch_days + offset);
            format!("{:04}-{:02}-{:02}", ymd.0, ymd.1, ymd.2)
        })
        .collect())
}

/// The inverse of `days_from_civil` — `(year, month, day)` of a civil
/// ordinal (Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year as i32, month, day)
}

/// The canonical `YYYY-MM-DD` render of a civil-day ordinal — the inverse
/// of [`parse_calendar_date`]'s ordinal walk, used wherever a suite or
/// route needs a RELATIVE calendar date (no calendar dependency, no
/// `Date.parse`).
pub fn render_civil_date(epoch_days: i64) -> String {
    let (year, month, day) = civil_from_days(epoch_days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The daily housekeeping vocabulary — `housekeepingSchema`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Housekeeping {
    Dirty,
    Clean,
    Inspected,
}

impl Housekeeping {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "dirty" => Some(Self::Dirty),
            "clean" => Some(Self::Clean),
            "inspected" => Some(Self::Inspected),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dirty => "dirty",
            Self::Clean => "clean",
            Self::Inspected => "inspected",
        }
    }
}

/// The transition table `mutate()`'s housekeeping arm checks — and
/// migration 001's `guard_room` trigger enforces with the same clauses
/// (`dirty→clean`, `clean→dirty|inspected`, `inspected→dirty`).
pub fn housekeeping_transition_allowed(from: Housekeeping, to: Housekeeping) -> bool {
    matches!(
        (from, to),
        (Housekeeping::Dirty, Housekeeping::Clean)
            | (Housekeeping::Clean, Housekeeping::Dirty)
            | (Housekeeping::Clean, Housekeeping::Inspected)
            | (Housekeeping::Inspected, Housekeeping::Dirty)
    )
}

// --- Stay state vocabulary -----------------------------------------------------

/// The stored stay states (`staySchema.state` + migration 001's CHECK).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StayState {
    Held,
    Confirmed,
    InHouse,
    CheckedOut,
    Cancelled,
}

impl StayState {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "held" => Some(Self::Held),
            "confirmed" => Some(Self::Confirmed),
            "in_house" => Some(Self::InHouse),
            "checked_out" => Some(Self::CheckedOut),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::Confirmed => "confirmed",
            Self::InHouse => "in_house",
            Self::CheckedOut => "checked_out",
            Self::Cancelled => "cancelled",
        }
    }
}

/// The derived stay state — `staySchema.effectiveState`: a held row whose
/// hold has already expired reads as `expired`; everything else rides the
/// stored word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StayEffectiveState {
    Held,
    Expired,
    Confirmed,
    InHouse,
    CheckedOut,
    Cancelled,
}

impl StayEffectiveState {
    /// `state === 'held' && expires_at <= observed ? 'expired' : state`.
    pub fn of(state: StayState, expires_at_millis: i64, observed_millis: i64) -> Self {
        if state == StayState::Held && expires_at_millis <= observed_millis {
            Self::Expired
        } else {
            match state {
                StayState::Held => Self::Held,
                StayState::Confirmed => Self::Confirmed,
                StayState::InHouse => Self::InHouse,
                StayState::CheckedOut => Self::CheckedOut,
                StayState::Cancelled => Self::Cancelled,
            }
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::Expired => "expired",
            Self::Confirmed => "confirmed",
            Self::InHouse => "in_house",
            Self::CheckedOut => "checked_out",
            Self::Cancelled => "cancelled",
        }
    }
}

// --- Commands ----------------------------------------------------------------

/// One command arm of `accommodationCommandSchema` — the discriminated
/// union. Field spellings match the wire exactly: a parsed command
/// serializes back into the shape the fingerprint covered, and the absent
/// `checkInTime`/`checkOutTime`/`holdSeconds` are already filled by the
/// parse, exactly as the zod defaults fill them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "operation")]
pub enum AccommodationCommand {
    #[serde(rename = "property.create")]
    PropertyCreate {
        /// Trimmed, 1–160 chars.
        title: String,
        /// Shape-checked — the scheduling module's recorded divergence
        /// (pure-domain width) applies.
        timezone: String,
        #[serde(rename = "checkInTime")]
        check_in_time: String,
        #[serde(rename = "checkOutTime")]
        check_out_time: String,
    },
    #[serde(rename = "room-type.create")]
    RoomTypeCreate {
        #[serde(rename = "propertyId")]
        property_id: String,
        title: String,
        #[serde(rename = "maxGuests")]
        max_guests: i64,
        #[serde(rename = "nightlyRate")]
        nightly_rate: i64,
    },
    #[serde(rename = "room-type.set-rate")]
    RoomTypeSetRate {
        #[serde(rename = "propertyId")]
        property_id: String,
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "nightlyRate")]
        nightly_rate: i64,
        reason: String,
    },
    #[serde(rename = "room.create")]
    RoomCreate {
        #[serde(rename = "propertyId")]
        property_id: String,
        #[serde(rename = "roomId")]
        room_type_id: String,
        /// Trimmed, 1–40 chars.
        code: String,
    },
    #[serde(rename = "room.set-active")]
    RoomSetActive {
        #[serde(rename = "propertyId")]
        property_id: String,
        #[serde(rename = "roomId")]
        room_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        active: bool,
        reason: String,
    },
    #[serde(rename = "room.housekeeping")]
    RoomHousekeeping {
        #[serde(rename = "propertyId")]
        property_id: String,
        #[serde(rename = "roomId")]
        room_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        state: Housekeeping,
        reason: String,
    },
    #[serde(rename = "stay.hold")]
    StayHold {
        #[serde(rename = "propertyId")]
        property_id: String,
        #[serde(rename = "roomId")]
        room_id: String,
        /// Calendar dates — shape-checked at the parse, resolved to
        /// instants inside the transaction (the property's timezone).
        arrival: String,
        departure: String,
        #[serde(rename = "guestName")]
        guest_name: String,
        guests: i64,
        #[serde(rename = "holdSeconds")]
        hold_seconds: i64,
    },
    #[serde(rename = "stay.confirm")]
    StayConfirm {
        #[serde(rename = "propertyId")]
        property_id: String,
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
    },
    #[serde(rename = "stay.check-in")]
    StayCheckIn {
        #[serde(rename = "propertyId")]
        property_id: String,
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
    },
    #[serde(rename = "stay.check-out")]
    StayCheckOut {
        #[serde(rename = "propertyId")]
        property_id: String,
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
    },
    #[serde(rename = "stay.cancel")]
    StayCancel {
        #[serde(rename = "propertyId")]
        property_id: String,
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        reason: String,
    },
}

impl AccommodationCommand {
    /// The discriminator — audit rows record it verbatim.
    pub fn operation(&self) -> &'static str {
        match self {
            Self::PropertyCreate { .. } => "property.create",
            Self::RoomTypeCreate { .. } => "room-type.create",
            Self::RoomTypeSetRate { .. } => "room-type.set-rate",
            Self::RoomCreate { .. } => "room.create",
            Self::RoomSetActive { .. } => "room.set-active",
            Self::RoomHousekeeping { .. } => "room.housekeeping",
            Self::StayHold { .. } => "stay.hold",
            Self::StayConfirm { .. } => "stay.confirm",
            Self::StayCheckIn { .. } => "stay.check-in",
            Self::StayCheckOut { .. } => "stay.check-out",
            Self::StayCancel { .. } => "stay.cancel",
        }
    }

    /// The mutation's own write permission — the TS ladder:
    /// `property.create` → `properties.write`; `room.housekeeping` ->
    /// `rooms.inspect` when the target state is `inspected` (the finer
    /// gate), otherwise `housekeeping.write`; any other `room.*` ->
    /// `rooms.write`; every `stay.*` -> `stays.write`.
    pub fn required_permission(&self) -> &'static str {
        match self {
            Self::PropertyCreate { .. } => "properties.write",
            Self::RoomHousekeeping { state, .. } => match state {
                Housekeeping::Inspected => "rooms.inspect",
                _ => "housekeeping.write",
            },
            Self::RoomTypeCreate { .. }
            | Self::RoomTypeSetRate { .. }
            | Self::RoomCreate { .. }
            | Self::RoomSetActive { .. } => "rooms.write",
            Self::StayHold { .. }
            | Self::StayConfirm { .. }
            | Self::StayCheckIn { .. }
            | Self::StayCheckOut { .. }
            | Self::StayCancel { .. } => "stays.write",
        }
    }

    /// The audit reason — on the arms that carry one (`'reason' in
    /// command ? command.reason : null`). The stay lifecycle's
    /// hold/confirm/check-in/check-out carry NO reason member; only the
    /// set-rate/set-active/housekeeping and the cancel do.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::RoomTypeSetRate { reason, .. }
            | Self::RoomSetActive { reason, .. }
            | Self::RoomHousekeeping { reason, .. }
            | Self::StayCancel { reason, .. } => Some(reason),
            _ => None,
        }
    }
}

/// The strict mutation document — `{requestId, command}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccommodationMutation {
    pub request_id: String,
    pub command: AccommodationCommand,
}

/// The access envelope — `accommodationAccessSchema` resolved inside the
/// transaction: the membership's permissions, the OWNING human's id (the
/// assigned scope's screen), and the scope. A service principal narrows
/// to the human it names, exactly as the other cohorts' ladders do.
#[derive(Debug, Clone)]
pub struct AccommodationAccess {
    pub permissions: Vec<String>,
    pub owner_id: String,
    pub scope: ErpScope,
}

impl AccommodationAccess {
    /// Whether the access holds ANY of the wanted permissions.
    pub fn holds(&self, wanted: &[&str]) -> bool {
        self.permissions
            .iter()
            .any(|held| wanted.contains(&held.as_str()))
    }
}

// --- The receipts + status ------------------------------------------------------

/// `accommodationReceiptSchema` — the discriminated union on `kind`; every
/// arm carries `(id, propertyId, revision)`. The TS schema has NO
/// `replayed` member — a replay is invisible on the wire, the same receipt
/// bytes of the original
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind")]
pub enum AccommodationReceipt {
    #[serde(rename = "property")]
    Property {
        id: String,
        #[serde(rename = "propertyId")]
        property_id: String,
        revision: i64,
    },
    #[serde(rename = "room-type")]
    RoomType {
        id: String,
        #[serde(rename = "propertyId")]
        property_id: String,
        revision: i64,
    },
    #[serde(rename = "room")]
    Room {
        id: String,
        #[serde(rename = "propertyId")]
        property_id: String,
        revision: i64,
    },
    #[serde(rename = "stay")]
    Stay {
        id: String,
        #[serde(rename = "propertyId")]
        property_id: String,
        revision: i64,
    },
}

impl AccommodationReceipt {
    /// The parse of a REPLAYED receipt from a stored row — the TS
    /// `accommodationReceiptSchema.parse(row.receipt)`; every arm carries
    /// exactly the three members.
    pub fn parse(raw: &Value) -> Result<Self, ValidationError> {
        let object = raw.as_object().ok_or(ValidationError("kind"))?;
        let kind = object
            .get("kind")
            .and_then(Value::as_str)
            .ok_or(ValidationError("kind"))?;
        if !super::valid_uuid(object.get("id").and_then(Value::as_str).unwrap_or_default())
            || !super::valid_uuid(
                object
                    .get("propertyId")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
        {
            return Err(ValidationError("id"));
        }
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let property_id = object
            .get("propertyId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let revision = object
            .get("revision")
            .and_then(Value::as_i64)
            .filter(|value| (1..=ACCOMMODATION_REVISION_MAX).contains(value))
            .ok_or(ValidationError("revision"))?;
        let strict = object.len() == 4
            && object.contains_key("kind")
            && object.contains_key("id")
            && object.contains_key("propertyId")
            && object.contains_key("revision");
        if !strict {
            return Err(ValidationError("propertyId"));
        }
        match kind {
            "property" => Ok(Self::Property {
                id,
                property_id,
                revision,
            }),
            "room-type" => Ok(Self::RoomType {
                id,
                property_id,
                revision,
            }),
            "room" => Ok(Self::Room {
                id,
                property_id,
                revision,
            }),
            "stay" => Ok(Self::Stay {
                id,
                property_id,
                revision,
            }),
            _ => Err(ValidationError("kind")),
        }
    }
}

/// `accommodationCommandStatusSchema` — the committed/not_committed pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status")]
pub enum AccommodationCommandStatus {
    #[serde(rename = "committed")]
    Committed {
        #[serde(rename = "requestId")]
        request_id: String,
        receipt: AccommodationReceipt,
    },
    #[serde(rename = "not_committed")]
    NotCommitted {
        #[serde(rename = "requestId")]
        request_id: String,
    },
}

// --- parse helpers (the scheduling grammar's style) ----------------------------

/// A required UUID member.
fn required_uuid(object: &serde_json::Map<String, Value>, name: &'static str) -> Option<String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| super::valid_uuid(value))
        .map(str::to_string)
}

/// A trimmed bounded text member (`.trim().min(1).max(max)`).
fn bounded_member(
    object: &serde_json::Map<String, Value>,
    name: &'static str,
    max: usize,
) -> Option<String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .map(|value| super::trimmed(value).to_string())
        .filter(|value| !value.is_empty() && value.len() <= max)
}

/// An integer member within `min..=max`.
fn bounded_integer(
    object: &serde_json::Map<String, Value>,
    name: &'static str,
    min: i64,
    max: i64,
) -> Option<i64> {
    let value = object.get(name)?;
    let number = match value {
        Value::Number(number) => number,
        _ => return None,
    };
    let as_f64 = number.as_f64()?;
    if as_f64.fract() != 0.0 || as_f64 < min as f64 || as_f64 > max as f64 {
        return None;
    }
    number.as_i64()
}

/// Whether the document's member set is EXACTLY these keys (`.strict()`).
fn strict_members(object: &serde_json::Map<String, Value>, keys: &[&str]) -> bool {
    object.keys().all(|key| keys.contains(&key.as_str()))
        && keys.iter().all(|key| object.contains_key(*key))
}

/// The strict member check where some members are `.default(...)`-filled:
/// the keys in `defaulted` MAY be absent (the parse fills them) and every
/// other named key is required; nothing beyond the named set survives.
fn strict_members_with_defaults(
    object: &serde_json::Map<String, Value>,
    keys: &[&str],
    defaulted: &[&str],
) -> bool {
    object
        .keys()
        .all(|key| keys.contains(&key.as_str()) || defaulted.contains(&key.as_str()))
        && keys.iter().all(|key| object.contains_key(*key))
}

/// A calendar-date member — `calendarDateSchema.parse` (zod's
/// calendar-precise grammar + the century refine).
fn calendar_date_member(
    object: &serde_json::Map<String, Value>,
    name: &'static str,
) -> Option<CivilDate> {
    object
        .get(name)
        .and_then(Value::as_str)
        .and_then(parse_calendar_date)
}

/// A calendar-time member — `calendarTimeSchema.parse`.
fn calendar_time_member(
    object: &serde_json::Map<String, Value>,
    name: &'static str,
) -> Option<String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| parse_calendar_time(value).is_some())
        .map(str::to_string)
}

/// The timezone member — the scheduling module's `schedulingTimezoneSchema`
/// with its recorded shape-only divergence (`crate::scheduling` carries it).
fn timezone_member(object: &serde_json::Map<String, Value>, name: &'static str) -> Option<String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .and_then(|value| super::scheduling::valid_timezone(value).map(|_| value.to_string()))
}

/// Whether two calendar times are in daily order (`checkOutTime <=
/// checkInTime`): same grammar family, so minute-of-day ordering.
fn calendar_time_le(earlier: &str, later: &str) -> bool {
    match (parse_calendar_time(earlier), parse_calendar_time(later)) {
        (Some((a_h, a_m)), Some((b_h, b_m))) => (a_h, a_m) <= (b_h, b_m),
        _ => false,
    }
}

/// `parse_mutation` — the schema's full walk, in the mode the TS one runs
/// (`mutate()`'s boundary): strict keys, uuid requestId, the command by
/// `operation`, and BOTH refines (`property.create`'s checkout order and
/// `stay.hold`'s `validNights`) folded in exactly where the schema puts
/// them.
#[allow(clippy::result_large_err)]
pub fn parse_mutation(raw: &Value) -> Result<AccommodationMutation, ValidationError> {
    let object = raw.as_object().ok_or(ValidationError("requestId"))?;
    let request_id = required_uuid(object, "requestId").ok_or(ValidationError("requestId"))?;
    if !strict_members(object, &["requestId", "command"]) {
        return Err(ValidationError("command"));
    }
    let command = object
        .get("command")
        .and_then(Value::as_object)
        .ok_or(ValidationError("command"))?;
    let command = parse_command(command)?;
    Ok(AccommodationMutation {
        request_id,
        command,
    })
}

/// `accommodationCommandSchema.parse` — the strict discriminated union.
/// Every arm is strict; the defaults fill at the parse so the fingerprint
/// covers the SAME document the effect reads.
#[allow(clippy::result_large_err)]
pub fn parse_command(
    command: &serde_json::Map<String, Value>,
) -> Result<AccommodationCommand, ValidationError> {
    let operation = command
        .get("operation")
        .and_then(Value::as_str)
        .ok_or(ValidationError("operation"))?;
    match operation {
        "property.create" => {
            if !strict_members_with_defaults(
                command,
                &["operation", "title", "timezone"],
                &["checkInTime", "checkOutTime"],
            ) {
                return Err(ValidationError("operation"));
            }
            let title = bounded_member(command, "title", ACCOMMODATION_TITLE_MAX)
                .ok_or(ValidationError("title"))?;
            let timezone =
                timezone_member(command, "timezone").ok_or(ValidationError("timezone"))?;
            // `.default('15:00')` / `.default('11:00')`.
            let check_in_time = match command.get("checkInTime") {
                None => "15:00".to_string(),
                Some(_) => calendar_time_member(command, "checkInTime")
                    .ok_or(ValidationError("checkInTime"))?,
            };
            let check_out_time = match command.get("checkOutTime") {
                None => "11:00".to_string(),
                Some(_) => calendar_time_member(command, "checkOutTime")
                    .ok_or(ValidationError("checkOutTime"))?,
            };
            if !calendar_time_le(&check_out_time, &check_in_time) {
                // "Checkout must not be after next arrival time".
                return Err(ValidationError("checkOutTime"));
            }
            Ok(AccommodationCommand::PropertyCreate {
                title,
                timezone,
                check_in_time,
                check_out_time,
            })
        }
        "room-type.create" => {
            if !strict_members(
                command,
                &[
                    "operation",
                    "propertyId",
                    "title",
                    "maxGuests",
                    "nightlyRate",
                ],
            ) {
                return Err(ValidationError("operation"));
            }
            let property_id =
                required_uuid(command, "propertyId").ok_or(ValidationError("propertyId"))?;
            let title = bounded_member(command, "title", ACCOMMODATION_TITLE_MAX)
                .ok_or(ValidationError("title"))?;
            let max_guests = bounded_integer(command, "maxGuests", 1, ACCOMMODATION_GUESTS_MAX)
                .ok_or(ValidationError("maxGuests"))?;
            let nightly_rate = bounded_integer(command, "nightlyRate", 0, ACCOMMODATION_RATE_MAX)
                .ok_or(ValidationError("nightlyRate"))?;
            Ok(AccommodationCommand::RoomTypeCreate {
                property_id,
                title,
                max_guests,
                nightly_rate,
            })
        }
        "room-type.set-rate" => {
            if !strict_members(
                command,
                &[
                    "operation",
                    "propertyId",
                    "id",
                    "expectedRevision",
                    "nightlyRate",
                    "reason",
                ],
            ) {
                return Err(ValidationError("operation"));
            }
            let property_id =
                required_uuid(command, "propertyId").ok_or(ValidationError("propertyId"))?;
            let id = required_uuid(command, "id").ok_or(ValidationError("id"))?;
            let expected_revision =
                bounded_integer(command, "expectedRevision", 1, ACCOMMODATION_REVISION_MAX)
                    .ok_or(ValidationError("expectedRevision"))?;
            let nightly_rate = bounded_integer(command, "nightlyRate", 0, ACCOMMODATION_RATE_MAX)
                .ok_or(ValidationError("nightlyRate"))?;
            let reason = bounded_member(command, "reason", ACCOMMODATION_REASON_MAX)
                .ok_or(ValidationError("reason"))?;
            Ok(AccommodationCommand::RoomTypeSetRate {
                property_id,
                id,
                expected_revision,
                nightly_rate,
                reason,
            })
        }
        "room.create" => {
            if !strict_members(command, &["operation", "propertyId", "roomTypeId", "code"]) {
                return Err(ValidationError("operation"));
            }
            let property_id =
                required_uuid(command, "propertyId").ok_or(ValidationError("propertyId"))?;
            let room_type_id =
                required_uuid(command, "roomTypeId").ok_or(ValidationError("roomTypeId"))?;
            let code = bounded_member(command, "code", ACCOMMODATION_CODE_MAX)
                .ok_or(ValidationError("code"))?;
            Ok(AccommodationCommand::RoomCreate {
                property_id,
                room_type_id,
                code,
            })
        }
        "room.set-active" => {
            if !strict_members(
                command,
                &[
                    "operation",
                    "propertyId",
                    "roomId",
                    "expectedRevision",
                    "active",
                    "reason",
                ],
            ) {
                return Err(ValidationError("operation"));
            }
            let property_id =
                required_uuid(command, "propertyId").ok_or(ValidationError("propertyId"))?;
            let room_id = required_uuid(command, "roomId").ok_or(ValidationError("roomId"))?;
            let expected_revision =
                bounded_integer(command, "expectedRevision", 1, ACCOMMODATION_REVISION_MAX)
                    .ok_or(ValidationError("expectedRevision"))?;
            let active = command
                .get("active")
                .and_then(Value::as_bool)
                .ok_or(ValidationError("active"))?;
            let reason = bounded_member(command, "reason", ACCOMMODATION_REASON_MAX)
                .ok_or(ValidationError("reason"))?;
            Ok(AccommodationCommand::RoomSetActive {
                property_id,
                room_id,
                expected_revision,
                active,
                reason,
            })
        }
        "room.housekeeping" => {
            if !strict_members(
                command,
                &[
                    "operation",
                    "propertyId",
                    "roomId",
                    "expectedRevision",
                    "state",
                    "reason",
                ],
            ) {
                return Err(ValidationError("operation"));
            }
            let property_id =
                required_uuid(command, "propertyId").ok_or(ValidationError("propertyId"))?;
            let room_id = required_uuid(command, "roomId").ok_or(ValidationError("roomId"))?;
            let expected_revision =
                bounded_integer(command, "expectedRevision", 1, ACCOMMODATION_REVISION_MAX)
                    .ok_or(ValidationError("expectedRevision"))?;
            let state = command
                .get("state")
                .and_then(Value::as_str)
                .and_then(Housekeeping::parse)
                .ok_or(ValidationError("state"))?;
            let reason = bounded_member(command, "reason", ACCOMMODATION_REASON_MAX)
                .ok_or(ValidationError("reason"))?;
            Ok(AccommodationCommand::RoomHousekeeping {
                property_id,
                room_id,
                expected_revision,
                state,
                reason,
            })
        }
        "stay.hold" => {
            if !strict_members_with_defaults(
                command,
                &[
                    "operation",
                    "propertyId",
                    "roomId",
                    "arrival",
                    "departure",
                    "guestName",
                    "guests",
                ],
                &["holdSeconds"],
            ) {
                return Err(ValidationError("operation"));
            }
            let property_id =
                required_uuid(command, "propertyId").ok_or(ValidationError("propertyId"))?;
            let room_id = required_uuid(command, "roomId").ok_or(ValidationError("roomId"))?;
            let arrival = calendar_date_member(command, "arrival")
                .ok_or(ValidationError("arrival"))?
                .render();
            let departure = calendar_date_member(command, "departure")
                .ok_or(ValidationError("departure"))?
                .render();
            let guest_name = bounded_member(command, "guestName", ACCOMMODATION_TITLE_MAX)
                .ok_or(ValidationError("guestName"))?;
            let guests = bounded_integer(command, "guests", 1, ACCOMMODATION_GUESTS_MAX)
                .ok_or(ValidationError("guests"))?;
            let hold_seconds = match command.get("holdSeconds") {
                None => HOLD_SECONDS_DEFAULT,
                Some(_) => bounded_integer(command, "holdSeconds", 1, HOLD_SECONDS_MAX)
                    .ok_or(ValidationError("holdSeconds"))?,
            };
            if calendar_nights(&arrival, &departure).is_err() {
                // "Stay must have 1–31 calendar nights".
                return Err(ValidationError("departure"));
            }
            Ok(AccommodationCommand::StayHold {
                property_id,
                room_id,
                arrival,
                departure,
                guest_name,
                guests,
                hold_seconds,
            })
        }
        "stay.confirm" | "stay.check-in" | "stay.check-out" | "stay.cancel" => {
            let keys: &[&str] = if operation == "stay.cancel" {
                &[
                    "operation",
                    "propertyId",
                    "id",
                    "expectedRevision",
                    "reason",
                ]
            } else {
                &["operation", "propertyId", "id", "expectedRevision"]
            };
            if !strict_members(command, keys) {
                return Err(ValidationError("operation"));
            }
            let property_id =
                required_uuid(command, "propertyId").ok_or(ValidationError("propertyId"))?;
            let id = required_uuid(command, "id").ok_or(ValidationError("id"))?;
            let expected_revision =
                bounded_integer(command, "expectedRevision", 1, ACCOMMODATION_REVISION_MAX)
                    .ok_or(ValidationError("expectedRevision"))?;
            match operation {
                "stay.confirm" => Ok(AccommodationCommand::StayConfirm {
                    property_id,
                    id,
                    expected_revision,
                }),
                "stay.check-in" => Ok(AccommodationCommand::StayCheckIn {
                    property_id,
                    id,
                    expected_revision,
                }),
                "stay.check-out" => Ok(AccommodationCommand::StayCheckOut {
                    property_id,
                    id,
                    expected_revision,
                }),
                _ => {
                    let reason = bounded_member(command, "reason", ACCOMMODATION_REASON_MAX)
                        .ok_or(ValidationError("reason"))?;
                    Ok(AccommodationCommand::StayCancel {
                        property_id,
                        id,
                        expected_revision,
                        reason,
                    })
                }
            }
        }
        _ => Err(ValidationError("operation")),
    }
}

// --- The row shapes the adapters read back ---------------------------------------

/// The property row shape — `propertySchema` narrowed to the members the
/// mutation surface touches (the reads ride their serving cohorts the same
/// way the scheduling module's keep).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccommodationProperty {
    pub id: String,
    pub tenant_id: String,
    pub owner_id: String,
    pub title: String,
    pub timezone: String,
    pub check_in_time: String,
    pub check_out_time: String,
    pub revision: i64,
}

/// The room row shape — the arm's active/housekeeping/occupied/revision
/// cells (with the `occupied` cell the TS read computes via the one
/// in-house-stay EXISTS).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccommodationRoom {
    pub id: String,
    pub tenant_id: String,
    pub property_id: String,
    pub room_type_id: String,
    pub active: bool,
    pub housekeeping: Housekeeping,
    pub occupied: bool,
    pub revision: i64,
}

/// The room-type row shape, as the arms read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccommodationRoomType {
    pub id: String,
    pub tenant_id: String,
    pub property_id: String,
    pub title: String,
    pub max_guests: i64,
    pub nightly_rate: i64,
    pub revision: i64,
}

/// The stay row shape, as the arms read it — with the resolution
/// instants the occupying slot binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccommodationStay {
    pub id: String,
    pub tenant_id: String,
    pub property_id: String,
    pub room_id: String,
    pub state: StayState,
    pub effective_state: StayEffectiveState,
    pub starts_at: String,
    pub ends_at: String,
    pub expires_at: String,
    pub revision: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const REQUEST: &str = "aa5ded77-1c33-40f6-8a5f-000000000005";

    // --- the calendar vocabulary -------------------------------------------------

    #[test]
    fn calendar_dates_patrol_the_grammar_the_zod_regex_encodes() {
        // Real dates inside the century band.
        assert_eq!(
            parse_calendar_date("2026-01-15")
                .expect("a real date")
                .epoch_days,
            20_468,
            "the civil ordinal of 2026-01-15 (20468 days = 10957 to 2000 + 9497 + 14)"
        );
        assert!(parse_calendar_date("2024-02-29").is_some()); // 2024 leap
        assert!(parse_calendar_date("2100-12-31").is_some()); // the 2100 arm
                                                              // The regex-encoded impossibilities: never-parse days.
        assert!(parse_calendar_date("2026-02-29").is_none()); // 2026 not leap
        assert!(parse_calendar_date("2026-02-30").is_none()); // February caps
        assert!(parse_calendar_date("2026-04-31").is_none()); // 30-day month
        assert!(parse_calendar_date("2100-02-29").is_none()); // 2100 NOT leap
        assert!(parse_calendar_date("2026-13-01").is_none()); // month bound
        assert!(parse_calendar_date("2026-00-10").is_none()); // month bound
        assert!(parse_calendar_date("2026-06-31").is_none());
        // Out of the century band — the resolve refines.
        assert!(parse_calendar_date("1999-01-15").is_none());
        assert!(parse_calendar_date("2101-01-15").is_none());
        assert!(parse_calendar_date("2400-02-29").is_none()); // leap, but foreign century
                                                              // Shapes.
        assert!(parse_calendar_date("2026-1-15").is_none()); // short shape
        assert!(parse_calendar_date("2026/01/15").is_none());
        assert!(parse_calendar_date("2026-01-1f").is_none());
    }

    #[test]
    fn calendar_nights_spans_count_1_to_31() {
        let nights = calendar_nights("2026-01-15", "2026-01-17").expect("two nights");
        assert_eq!(
            nights,
            vec!["2026-01-15".to_string(), "2026-01-16".to_string()]
        );
        // A month hop — civil arithmetic runs past the month edge.
        let nights = calendar_nights("2026-01-31", "2026-02-01").expect("one night");
        assert_eq!(nights, vec!["2026-01-31".to_string()]);
        // A leap day inside the stay.
        let nights = calendar_nights("2028-02-28", "2028-03-01").expect("two nights");
        assert_eq!(
            nights,
            vec!["2028-02-28".to_string(), "2028-02-29".to_string()]
        );
        // Zero nights (same day) and 32 nights refuse.
        assert!(calendar_nights("2026-01-15", "2026-01-15").is_err());
        assert!(calendar_nights("2026-01-15", "2026-02-16").is_err()); // 32
        assert!(calendar_nights("2026-01-15", "2026-02-15").is_ok()); // 31
                                                                      // Backwards and impossible dates refuse.
        assert!(calendar_nights("2026-01-15", "2026-01-14").is_err());
        assert!(calendar_nights("2026-02-30", "2026-03-01").is_err());
    }

    #[test]
    fn calendar_times_are_daily_clocks_only() {
        assert_eq!(parse_calendar_time("15:00"), Some((15, 0)));
        assert_eq!(parse_calendar_time("00:00"), Some((0, 0)));
        assert_eq!(parse_calendar_time("07:30"), Some((7, 30))); // leading zero
        assert_eq!(parse_calendar_time("23:59"), Some((23, 59)));
        assert_eq!(parse_calendar_time("24:00"), None);
        assert_eq!(parse_calendar_time("7:30"), None); // single-digit hour
        assert_eq!(parse_calendar_time("15:60"), None);
        assert_eq!(parse_calendar_time("15:0"), None);
        assert_eq!(parse_calendar_time("15:00:00"), None);
    }

    // --- the command grammar -----------------------------------------------------

    fn hold_command() -> serde_json::Value {
        json!({
            "operation": "stay.hold",
            "propertyId": "9a5ded77-1c33-40f6-8a5f-0000000000a5",
            "roomId": "9a5ded77-1c33-40f6-8a5f-0000000000b5",
            "arrival": "2026-01-15",
            "departure": "2026-01-17",
            "guestName": "Dewi Anggraini",
            "guests": 2
        })
    }

    #[test]
    fn property_create_fills_the_daily_defaults_and_cares_about_checkout_order() {
        let command = json!({
            "operation": "property.create",
            "title": "  Villa Nusantara Ubud  ",
            "timezone": "Asia/Makassar"
        });
        let parsed = parse_command(command.as_object().expect("an object")).expect("defaults fill");
        assert_eq!(
            parsed,
            AccommodationCommand::PropertyCreate {
                title: "Villa Nusantara Ubud".to_string(),
                timezone: "Asia/Makassar".to_string(),
                check_in_time: "15:00".to_string(),
                check_out_time: "11:00".to_string(),
            }
        );
        // Checkout AFTER check-in — the daily cycle inverts, the refine refuses.
        let late_checkout = json!({
            "operation": "property.create",
            "title": "Late sheets",
            "timezone": "Asia/Makassar",
            "checkInTime": "14:00",
            "checkOutTime": "16:00"
        });
        assert!(parse_command(late_checkout.as_object().expect("o")).is_err());
        // The legal kind: checkout at/before arrival.
        let early_checkout = json!({
            "operation": "property.create",
            "title": "Hostel stacks",
            "timezone": "Asia/Makassar",
            "checkInTime": "14:00",
            "checkOutTime": "14:00"
        });
        assert!(parse_command(early_checkout.as_object().expect("o")).is_ok());
        // Malformed daily times refuse.
        let broken = json!({
            "operation": "property.create",
            "title": "Broken clock",
            "timezone": "Asia/Makassar",
            "checkInTime": "15pm"
        });
        assert!(parse_command(broken.as_object().expect("o")).is_err());
    }

    #[test]
    fn stay_hold_defaults_hold_seconds_and_demands_real_night_spans() {
        let parsed = parse_command(hold_command().as_object().expect("o")).expect("990s default");
        match parsed {
            AccommodationCommand::StayHold {
                guest_name,
                guests,
                hold_seconds,
                ..
            } => {
                assert_eq!(guest_name, "Dewi Anggraini");
                assert_eq!(guests, 2);
                assert_eq!(hold_seconds, HOLD_SECONDS_DEFAULT);
            }
            _ => panic!("the hold parses as the stay.hold arm"),
        }
        // Zero nights — the validNights refine refuses at the parse boundary.
        let mut zero = hold_command();
        zero["departure"] = json!("2026-01-15");
        assert!(parse_command(zero.as_object().expect("o")).is_err());
        // Forty days in the future — also the refine's window.
        let mut long = hold_command();
        long["departure"] = json!("2026-05-01");
        assert!(parse_command(long.as_object().expect("o")).is_err());
        // holdSeconds of the wrong shape and beyond the cap refuse.
        let mut wrong_shape = hold_command();
        wrong_shape["holdSeconds"] = json!("tall");
        assert!(parse_command(wrong_shape.as_object().expect("o")).is_err());
        let mut over = hold_command();
        over["holdSeconds"] = json!(1801);
        assert!(parse_command(over.as_object().expect("o")).is_err());
        let mut at_cap = hold_command();
        at_cap["holdSeconds"] = json!(1800);
        assert!(parse_command(at_cap.as_object().expect("o")).is_ok());
    }

    #[test]
    fn strict_refusals_honor_the_union() {
        // An unknown operation.
        let stray = json!({
            "operation": "stay.move",
            "propertyId": "9a5ded77-1c33-40f6-8a5f-0000000000a5",
            "id": "9a5ded77-1c33-40f6-8a5f-0000000000c5",
            "expectedRevision": 1
        });
        assert!(parse_command(stray.as_object().expect("o")).is_err());
        // An extra member on a strict arm.
        let mut extras = hold_command();
        extras["currency"] = json!("IDR");
        assert!(parse_command(extras.as_object().expect("o")).is_err());
        // A stay arm with an unexpected reason (only the cancel carries one).
        let mut sneaky = json!({
            "operation": "stay.confirm",
            "propertyId": "9a5ded77-1c33-40f6-8a5f-0000000000a5",
            "id": "9a5ded77-1c33-40f6-8a5f-0000000000c5",
            "expectedRevision": 1
        });
        sneaky["reason"] = json!("why not");
        assert!(parse_command(sneaky.as_object().expect("o")).is_err());
    }

    #[test]
    fn permission_ladder_matches_the_ts_word_per_operation() {
        let cases: Vec<(&str, AccommodationCommand)> = vec![
            ("properties.write", property_create()),
            ("rooms.write", room_type_create()),
            ("rooms.write", room_type_set_rate()),
            ("rooms.write", room_create()),
            ("rooms.write", room_set_active()),
            ("housekeeping.write", housekeeping_clean()),
            ("rooms.inspect", housekeeping_inspect()),
            ("stays.write", hold_parsed()),
            ("stays.write", stay_confirm()),
            ("stays.write", stay_cancel()),
        ];
        for (want, command) in cases {
            assert_eq!(
                command.required_permission(),
                want,
                "{}",
                command.operation()
            );
            // Audit reasons ride only the arms that carry one.
            let wants_reason = matches!(
                command.operation(),
                "room-type.set-rate" | "room.set-active" | "room.housekeeping" | "stay.cancel"
            );
            assert_eq!(
                command.reason().is_some(),
                wants_reason,
                "{}: reason presence",
                command.operation()
            );
        }
    }

    fn property_create() -> AccommodationCommand {
        parse_command(
            json!({
                "operation": "property.create",
                "title": "Villa",
                "timezone": "Asia/Makassar"
            })
            .as_object()
            .expect("o"),
        )
        .expect("parses")
    }

    fn room_type_create() -> AccommodationCommand {
        parse_command(
            json!({
                "operation": "room-type.create",
                "propertyId": "9a5ded77-1c33-40f6-8a5f-0000000000a5",
                "title": "Deluxe garden",
                "maxGuests": 3,
                "nightlyRate": 450_001
            })
            .as_object()
            .expect("o"),
        )
        .expect("parses")
    }

    fn room_type_set_rate() -> AccommodationCommand {
        parse_command(
            json!({
                "operation": "room-type.set-rate",
                "propertyId": "9a5ded77-1c33-40f6-8a5f-0000000000a5",
                "id": "9a5ded77-1c33-40f6-8a5f-0000000000d5",
                "expectedRevision": 2,
                "nightlyRate": 460_000,
                "reason": "high season"
            })
            .as_object()
            .expect("o"),
        )
        .expect("parses")
    }

    fn room_create() -> AccommodationCommand {
        parse_command(
            json!({
                "operation": "room.create",
                "propertyId": "9a5ded77-1c33-40f6-8a5f-0000000000a5",
                "roomTypeId": "9a5ded77-1c33-40f6-8a5f-0000000000d5",
                "code": "W-101"
            })
            .as_object()
            .expect("o"),
        )
        .expect("parses")
    }

    fn room_set_active() -> AccommodationCommand {
        parse_command(
            json!({
                "operation": "room.set-active",
                "propertyId": "9a5ded77-1c33-40f6-8a5f-0000000000a5",
                "roomId": "9a5ded77-1c33-40f6-8a5f-0000000000b5",
                "expectedRevision": 2,
                "active": false,
                "reason": "deep clean"
            })
            .as_object()
            .expect("o"),
        )
        .expect("parses")
    }

    fn housekeeping_clean() -> AccommodationCommand {
        parse_command(
            json!({
                "operation": "room.housekeeping",
                "propertyId": "9a5ded77-1c33-40f6-8a5f-0000000000a5",
                "roomId": "9a5ded77-1c33-40f6-8a5f-0000000000b5",
                "expectedRevision": 2,
                "state": "clean",
                "reason": "morning round"
            })
            .as_object()
            .expect("o"),
        )
        .expect("parses")
    }

    fn housekeeping_inspect() -> AccommodationCommand {
        parse_command(
            json!({
                "operation": "room.housekeeping",
                "propertyId": "9a5ded77-1c33-40f6-8a5f-0000000000a5",
                "roomId": "9a5ded77-1c33-40f6-8a5f-0000000000b5",
                "expectedRevision": 2,
                "state": "inspected",
                "reason": "passed the checklist"
            })
            .as_object()
            .expect("o"),
        )
        .expect("parses")
    }

    fn hold_parsed() -> AccommodationCommand {
        parse_command(hold_command().as_object().expect("o")).expect("parses")
    }

    fn stay_confirm() -> AccommodationCommand {
        parse_command(
            json!({
                "operation": "stay.confirm",
                "propertyId": "9a5ded77-1c33-40f6-8a5f-0000000000a5",
                "id": "9a5ded77-1c33-40f6-8a5f-0000000000c5",
                "expectedRevision": 1
            })
            .as_object()
            .expect("o"),
        )
        .expect("parses")
    }

    fn stay_cancel() -> AccommodationCommand {
        parse_command(
            json!({
                "operation": "stay.cancel",
                "propertyId": "9a5ded77-1c33-40f6-8a5f-0000000000a5",
                "id": "9a5ded77-1c33-40f6-8a5f-0000000000c5",
                "expectedRevision": 1,
                "reason": "the guest changed plans"
            })
            .as_object()
            .expect("o"),
        )
        .expect("parses")
    }

    #[test]
    fn mutation_parse_shapes_the_document_strictly() {
        let mutation = json!({
            "requestId": REQUEST,
            "command": hold_command()
        });
        let parsed = parse_mutation(&mutation).expect("parses");
        assert_eq!(parsed.request_id, REQUEST);
        // A stray third key refuses.
        let extras = json!({
            "requestId": REQUEST,
            "command": hold_command(),
            "tenant": "nope"
        });
        assert!(parse_mutation(&extras).is_err());
        // A non-uuid request id refuses.
        let bad_keys = json!({
            "requestId": "not a uuid",
            "command": hold_command()
        });
        assert!(parse_mutation(&bad_keys).is_err());
    }

    // --- receipts + derivations ---------------------------------------------------

    #[test]
    fn receipts_round_trip_the_three_kinds_and_refuse_strangers() {
        let receipt = json!({
            "kind": "stay",
            "id": "9a5ded77-1c33-40f6-8a5f-0000000000c5",
            "propertyId": "9a5ded77-1c33-40f6-8a5f-0000000000a5",
            "revision": 4
        });
        let parsed = AccommodationReceipt::parse(&receipt).expect("the stored receipt parses");
        assert_eq!(
            parsed,
            AccommodationReceipt::Stay {
                id: "9a5ded77-1c33-40f6-8a5f-0000000000c5".to_string(),
                property_id: "9a5ded77-1c33-40f6-8a5f-0000000000a5".to_string(),
                revision: 4,
            }
        );
        assert_eq!(
            serde_json::to_value(&parsed).expect("serializes"),
            receipt,
            "the canonical serialization is byte-shape identical"
        );
        // An unknown kind refuses.
        let stranger = json!({
            "kind": "block",
            "id": "9a5ded77-1c33-40f6-8a5f-0000000000c5",
            "propertyId": "9a5ded77-1c33-40f6-8a5f-0000000000a5",
            "revision": 4
        });
        assert!(AccommodationReceipt::parse(&stranger).is_err());
        // Extra members refuse.
        let padded = json!({
            "kind": "room",
            "id": "9a5ded77-1c33-40f6-8a5f-0000000000b5",
            "propertyId": "9a5ded77-1c33-40f6-8a5f-0000000000a5",
            "revision": 2,
            "extra": true
        });
        assert!(AccommodationReceipt::parse(&padded).is_err());
    }

    #[test]
    fn housekeeping_walks_only_the_legal_transitions() {
        use Housekeeping::{Clean, Dirty, Inspected};
        assert!(housekeeping_transition_allowed(Dirty, Clean));
        assert!(housekeeping_transition_allowed(Clean, Dirty));
        assert!(housekeeping_transition_allowed(Clean, Inspected));
        assert!(housekeeping_transition_allowed(Inspected, Dirty));
        assert!(!housekeeping_transition_allowed(Dirty, Inspected));
        assert!(!housekeeping_transition_allowed(Dirty, Dirty));
        assert!(!housekeeping_transition_allowed(Inspected, Clean));
        assert!(!housekeeping_transition_allowed(Inspected, Inspected));
    }

    #[test]
    fn effective_state_derives_expiry_from_the_observed_clock() {
        use StayState::Held;
        assert_eq!(
            StayEffectiveState::of(Held, 1_000, 999),
            StayEffectiveState::Held,
            "the hold lives"
        );
        assert_eq!(
            StayEffectiveState::of(Held, 999, 999),
            StayEffectiveState::Expired,
            "expires AT the boundary — <= observed"
        );
        assert_eq!(
            StayEffectiveState::of(StayState::Confirmed, 1, 5000),
            StayEffectiveState::Confirmed,
            "non-held states ignore the expiry"
        );
    }
}
