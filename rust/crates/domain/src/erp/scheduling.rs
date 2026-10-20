//! The scheduling domain contracts — the Archxy 2 port of
//! `packages/scheduling/src/contracts.ts` (R05 first cohort: resource
//! reservations with the overlap guard, hold/confirm/move/cancel lifecycle).
//!
//! ## The wire grammar
//!
//! `schedulingMutationSchema` is a strict two-key document
//! `{requestId, command}` — no `type`/`data` split like the R04 cohorts —
//! and `command` itself is a discriminated union on `operation` whose
//! members are all `.strict()`. Two members carry SOFT grammar the parse
//! bakes in, exactly the zod transform does:
//!
//! - `schedulingInstantSchema` REJECTS wall-clock ambiguity (offset
//!   required), confines the range by regex to `<year> 2000–2100`,
//!   checks instant validity through `Date.parse`, then TRANSFORMS every
//!   value to `new Date(value).toISOString()` — the canonical
//!   `YYYY-MM-DDTHH:MM:SS.mmmZ` render. This port stores that exact
//!   canonical string, so an input's offset form never survives the parse
//!   (`+07:00` becomes its UTC render) and two spellings of the same
//!   instant are indistinguishable downstream.
//! - `holdSeconds` is `.default(900)` — an ABSENT member parses as 900,
//!   and the fingerprint sees the filled document, same as the TS side.
//!
//! `title`/`reason` are `.trim()`-min-1-max-bound: the trimmed value is
//! what travels to SQL and what the fingerprint covers.
//!
//! ## Timezone acceptance (recorded divergence, same class as the R04 texts)
//!
//! The TS boundary validates timezones through a real `Intl.DateTimeFormat`
//! probe — a live IANA check the pure domain cannot reproduce without a tz
//! database, and this crate must stay wasm-clean. The port therefore
//! validates the SHAPE (1–100 chars, IANA-ish characters) and records that
//! the width is wider than `Intl`'s at the 400 boundary: e.g. `Region/Fake`
//! parses here and `Intl` would refuse. The SQL CHECK on the column is
//! length-only in the baseline too, so no persisted row can drift beyond
//! the column bounds; the recorded flattening lives in the ERP packet's
//! KNOWN_DIFFERENCES ledger, never silently.
//!
//! ## The error vocabulary
//!
//! The TS `SchedulingError('forbidden'|'not_found'|'conflict'|'unavailable')`
//! rides this crate's shared [`ErpError`] carrier — the host maps them to
//! 403/404/409/503 exactly as the R04 cohorts do.

use serde::Serialize;
use serde_json::Value;

pub use super::purchasing::{
    PrincipalKind, PurchasePrincipal as SchedulingPrincipal, ValidationError,
};
pub use super::{ErpError, ErpScope};

/// The scheduling permission vocabulary — `SCHEDULING_PERMISSIONS`.
/// Module constants, not an enum: the membership rows store strings and the
/// grant gates test membership of the list, like the R04 cohorts do.
pub const SCHEDULING_PERMISSIONS: &[&str] = &[
    "resources.read",
    "resources.write",
    "reservations.read",
    "reservations.write",
    "audit.read",
];

/// The seven operations — `schedulingCommandSchema`'s discriminator values.
pub const SCHEDULING_OPERATIONS: &[&str] = &[
    "resource.create",
    "resource.set-active",
    "reservation.hold",
    "reservation.block",
    "reservation.confirm",
    "reservation.move",
    "reservation.cancel",
];

/// `capacity` is `z.literal(1)` — the schema pins it, the column CHECK
/// re-pins it, and the row grammar refuses anything else.
pub const SCHEDULING_CAPACITY: i64 = 1;

/// The interval bound — ends after starts by at most 31 whole days
/// (`validInterval`: `length > 0 && length <= 31 * 86_400_000`), measured in
/// instant milliseconds, never wall-clock.
pub const INTERVAL_MS_MAX: i64 = 31 * 86_400_000;

/// `revision` is `.int().positive().max(2_147_483_647)` — shared bound with
/// the other ERLedger's revision cells; a row AT the ceiling refuses
/// further bumps (the SQL CHECK rejects, mapped to conflict).
pub const REVISION_MAX: i64 = 2_147_483_647;

/// `holdSeconds` bounds (`min 1 max 1800`) — the DEFAULT lives in the parse
/// (900 when absent) because the fingerprint must cover the filled value.
pub const HOLD_SECONDS_DEFAULT: i64 = 900;
pub const HOLD_SECONDS_MAX: i64 = 1800;

/// `title` and `reason` bounds (`title` ≤ 160, `reason` ≤ 2000 after trim).
pub const SCHEDULING_TITLE_MAX: usize = 160;
pub const SCHEDULING_REASON_MAX: usize = 2000;

/// The timezone column's length-only bound — the recorded divergence note
/// above describes what the boundary adds on top.
pub const SCHEDULING_TIMEZONE_MAX: usize = 100;
/// The scheduling receipt kinds.
pub const SCHEDULING_RECEIPT_KINDS: &[&str] = &["resource", "reservation"];

/// A scheduling membership's resolved grant — `schedulingAccessSchema`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulingAccess {
    pub permissions: Vec<String>,
    pub owner_id: String,
    pub scope: ErpScope,
}

impl SchedulingAccess {
    /// Whether the grant holds every named permission — the typed form of
    /// the per-mutation `requireGrant` chain.
    pub fn holds(&self, wanted: &[&str]) -> bool {
        wanted
            .iter()
            .all(|needed| self.permissions.iter().any(|held| held == needed))
    }

    /// Whether a bit of the ledger is inside the actor's scope or owned by
    /// them — the `(scope === 'all' OR owner_id = $owner)` predicate the
    /// owned-row SQL embeds everywhere.
    pub fn sees(&self, owner_id: &str) -> bool {
        self.scope == ErpScope::All || self.owner_id == owner_id
    }
}

/// The resource kinds — `z.enum(['staff','asset','room','space'])`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulingResourceKind {
    Staff,
    Asset,
    Room,
    Space,
}

impl SchedulingResourceKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "staff" => Some(Self::Staff),
            "asset" => Some(Self::Asset),
            "room" => Some(Self::Room),
            "space" => Some(Self::Space),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Staff => "staff",
            Self::Asset => "asset",
            Self::Room => "room",
            Self::Space => "space",
        }
    }
}

impl Serialize for SchedulingResourceKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// The reservation kinds and states — `booking`/`block` on insert
/// (`reservation.hold` → `booking` + `held`, `reservation.block` →
/// `block` + `confirmed`), state is `held`/`confirmed`/`cancelled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulingReservationKind {
    Booking,
    Block,
}

impl SchedulingReservationKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "booking" => Some(Self::Booking),
            "block" => Some(Self::Block),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Booking => "booking",
            Self::Block => "block",
        }
    }
}

impl Serialize for SchedulingReservationKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl Serialize for SchedulingReservationState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl Serialize for SchedulingEffectiveState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// The stored state — the effective state adds `expired` DERIVED, never
/// stored (a stale `held` row with `expires_at` in the past reads as
/// `expired` through the repository's row grammar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulingReservationState {
    Held,
    Confirmed,
    Cancelled,
}

impl SchedulingReservationState {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "held" => Some(Self::Held),
            "confirmed" => Some(Self::Confirmed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::Confirmed => "confirmed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// The effective state the reads report — `reservationSchema`'s
/// `effectiveState` member: a held row whose hold expired is `expired`,
/// otherwise the stored state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulingEffectiveState {
    Held,
    Confirmed,
    Cancelled,
    Expired,
}

impl SchedulingEffectiveState {
    /// The row-side refinement rule, driven by the instant millis the row
    /// grammar already parsed (`held && expires_at <= observed` → expired).
    pub fn of(state: SchedulingReservationState, expires_at: Option<i64>, observed: i64) -> Self {
        match state {
            SchedulingReservationState::Held => match expires_at {
                Some(expires_at) if expires_at <= observed => Self::Expired,
                _ => Self::Held,
            },
            SchedulingReservationState::Confirmed => Self::Confirmed,
            SchedulingReservationState::Cancelled => Self::Cancelled,
        }
    }

    /// The wire spelling (`'held'|'confirmed'|'cancelled'|'expired'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::Confirmed => "confirmed",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }
}

// --- Instants ----------------------------------------------------------------

/// A validated scheduling instant: the canonical
/// `YYYY-MM-DDTHH:MM:SS.mmmZ` render plus its instant milliseconds. Built
/// only by [`parse_instant`], so comparisons and the hold-expiry math
/// operate on numbers the calendar already accepted — never on wire text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulingInstant {
    canonical: String,
    epoch_millis: i64,
}

impl SchedulingInstant {
    /// The canonical render — what the adapters bind and the fingerprint
    /// covers (`toISOString()`'s exact shape: always milliseconds, always Z).
    pub fn canonical(&self) -> &str {
        &self.canonical
    }

    /// The instant, comparable with every other instant of this type.
    pub fn epoch_millis(&self) -> i64 {
        self.epoch_millis
    }
}

impl Serialize for SchedulingInstant {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.canonical)
    }
}

/// Days from the civil-based epoch algorithm (`days_from_civil`, Hinnant) —
/// the harness that turns accepted caLendar parts into epoch days. Leap
/// years follow the Gregorian rule 2000–2100 makes trivial: divisible by
/// 4 except 2100 itself is NOT (`(year % 4 == 0 && year % 100 != 0) || year
/// % 400 == 0` — 2100 fails).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_shifted = (month + 9) % 12;
    let day_of_year = (153 * month_shifted + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The leap rule inside 2000–2100.
fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// The month length table — February's entry takes the leap year argument.
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// One digit run of an exact length.
fn digits(bytes: &[u8], start: usize, count: usize) -> Option<i64> {
    let mut value: i64 = 0;
    for &byte in &bytes[start..start + count] {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value * 10 + i64::from(byte - b'0');
    }
    Some(value)
}

/// The parse of `schedulingInstantSchema` minus the live `Date.parse` and
/// `Intl` surprises: the regex the schema refines with (`^(20\\d\\d|2100)-.+T
/// \\d\\d:\\d\\d:\\d\\d(?:\\.\\d{1,3})?(?:Z|[+-]\\d\\d:\\d\\d)$`), then the
/// exact calendar facts `Date.parse` enforces — two-digit month/day, real
/// month lengths incl. the leap rule (note 2100 is NOT a leap year, though
/// `\d\d`-century logic alone would call it one), 0–23h, 0–59m, 0–59s,
/// 1–3 fraction digits, and an offset in 00–23:00–59 (`Date.parse` refuses
/// `+24:00`, so larger offsets mark the document invalid too).
///
/// The accepted range starts 2000-01-01 ANYWHERE — the offset may push the
/// represented instant before the epoch-line; only the LOCAL spelling the
/// regex checks is confine by `20\\d\\d|2100`, and this parse reproduces
/// that (the TS refine tests the STRING, not the instant).
///
/// Every accepted value canonicalizes to the UTC render
/// `YYYY-MM-DDTHH:MM:SS.mmmZ` — `toISOString()`'s shape — so the SQL side
/// (timestamptz) and the fingerprint see one spelling.
pub fn parse_instant(value: &str) -> Option<SchedulingInstant> {
    let bytes = value.as_bytes();
    // Year: `20\d\d` or 2100 exactly — four bytes only, and the shortest
    // accepted tail is `T HH:MM:SS` + `Z` at 20 bytes of document.
    if bytes.len() < 20 || !value.starts_with("20") {
        return None;
    }
    let year = digits(bytes, 0, 4)?;
    if !(2000..=2100).contains(&year) {
        return None;
    }
    if bytes[4] != b'-' {
        return None;
    }
    // `.+T`: a nonempty date run to the FIRST 'T'. The registry `Date.parse`
    // only reads a conforming calendar date, so the parse demands the exact
    // ISO shape `MM-DD` here — anything nonconformant is NaN's refusal.
    let t_index = bytes
        .iter()
        .skip(5)
        .position(|&b| b == b'T')
        .map(|index| index + 5)?;
    if t_index != 10 || bytes[7] != b'-' {
        return None;
    }
    let month = digits(bytes, 5, 2)?;
    let day = digits(bytes, 8, 2)?;
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    // `T HH:MM:SS`
    let hour = digits(bytes, 11, 2)?;
    let minute = digits(bytes, 14, 2)?;
    let second = digits(bytes, 17, 2)?;
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    if bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    // Fraction optional: `. 1-3 digits`, then Z or ±HH:MM. The regex's
    // `\.d{1,3}` caps the run's width; a FOURTH digit leaves the tail unable to
    // match `(Z|\u00b1HH:MM)`, so the scan stops at 3 and the
    // offset must start right there.
    let (offset_start, frac_millis): (usize, i64) = if bytes.get(19) == Some(&b'.') {
        let mut count = 0;
        let mut value = 0;
        while count < 3 && matches!(bytes.get(20 + count), Some(&b) if b.is_ascii_digit()) {
            value = value * 10 + i64::from(bytes[20 + count] - b'0');
            count += 1;
        }
        if count == 0 {
            return None;
        }
        // `.1` = `.100` — the accepted prefix reads at its toISOString
        // weight, never truncated.
        let aligned = match count {
            1 => value * 100,
            2 => value * 10,
            _ => value,
        };
        (20 + count, aligned)
    } else {
        (19, 0)
    };
    // Offset: Z or ±HH:MM with hour ≤ 23 (`Date.parse` refuses +24:00).
    let offset: i64 = match bytes.get(offset_start) {
        Some(b'Z') => 0,
        Some(sign @ (b'+' | b'-')) => {
            if bytes.len() != offset_start + 6 {
                return None;
            }
            if bytes[offset_start + 3] != b':' {
                return None;
            }
            let offset_hour = digits(bytes, offset_start + 1, 2)?;
            let offset_minute = digits(bytes, offset_start + 4, 2)?;
            if offset_hour > 23 || offset_minute > 59 {
                return None;
            }
            let magnitude = offset_hour * 3_600_000 + offset_minute * 60_000;
            if *sign == b'+' {
                magnitude
            } else {
                -magnitude
            }
        }
        _ => return None,
    };
    // The instant: local wall-clock at the given offset → UTC millis.
    let day = days_from_civil(year, month, day);
    let local = day * 86_400_000 + hour * 3_600_000 + minute * 60_000 + second * 1_000;
    let utc = local - offset + frac_millis;
    // Render through the civil inverse — the same walk `toISOString`
    // does, so the canonical text is byte-stable across the years.
    let (rendered, _) = civil_from_millis(utc);
    Some(SchedulingInstant {
        canonical: rendered,
        epoch_millis: utc,
    })
}

/// The civil inverse — epoch millis → `(YYYY-MM-DDTHH:MM:SS.mmmZ, _)`.
/// Mirrors `instant_string`'s civil math in `proposals` so both renders
/// stay table-driven by the same constants.
fn civil_from_millis(mut millis: i64) -> (String, (i64, i64, i64)) {
    let days = millis.div_euclid(86_400_000);
    millis -= days * 86_400_000;
    // Howard Hinnant's civil_from_days on the count of whole days.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    let hour = millis / 3_600_000;
    let minute = (millis / 60_000) % 60;
    let second = (millis / 1_000) % 60;
    let fraction = millis % 1_000;
    (
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{fraction:03}Z"),
        (year, month, day),
    )
}

/// The canonical instant render for an epoch-millis value the ADAPTER
/// computed (the hold expiry: observed + holdSeconds * 1000, rendered the
/// same `toISOString` way before the INSERT binds it).
pub fn instant_render(epoch_millis: i64) -> String {
    civil_from_millis(epoch_millis).0
}

/// The shape-level timezone acceptance recorded in the module header: 1–100
/// characters from the IANA render alphabet — letters, digits, `/`, `_`,
/// `-` and `+` (`Etc/GMT-14` carries a sign), no leading/trailing junk. The
/// live-`Intl` probe the TS version probes with would refuse fabricated
/// prefixes this shape accepts; that width is the recorded divergence.
pub fn valid_timezone(value: &str) -> Option<&str> {
    if value.is_empty() || value.len() > SCHEDULING_TIMEZONE_MAX {
        return None;
    }
    if value
        .bytes()
        .any(|b| !(b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'+')))
    {
        return None;
    }
    Some(value)
}

// --- Commands ----------------------------------------------------------------

/// One command arm of `schedulingCommandSchema` — the discriminated union.
/// Field spellings match the wire exactly (#[serde(rename)]s): a parsed
/// command serializes back to the SHAPE the fingerprint covered, and the
/// absent `holdSeconds` is already filled by the parse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "operation")]
pub enum SchedulingCommand {
    #[serde(rename = "resource.create")]
    ResourceCreate {
        /// Trimmed, 1–160 chars.
        title: String,
        kind: SchedulingResourceKind,
        /// Shape-checked (1–100, IANA-ish) — the divergence note in the
        /// module header applies.
        timezone: String,
    },
    #[serde(rename = "resource.set-active")]
    ResourceSetActive {
        #[serde(rename = "resourceId")]
        resource_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        active: bool,
        /// Required, trimmed, 1–2000.
        reason: String,
    },
    #[serde(rename = "reservation.hold")]
    ReservationHold {
        #[serde(rename = "resourceId")]
        resource_id: String,
        title: String,
        #[serde(rename = "startsAt")]
        starts_at: String,
        #[serde(rename = "endsAt")]
        ends_at: String,
        /// 1–1800; the parse fills 900 when the document omits it.
        #[serde(rename = "holdSeconds")]
        hold_seconds: i64,
    },
    #[serde(rename = "reservation.block")]
    ReservationBlock {
        #[serde(rename = "resourceId")]
        resource_id: String,
        title: String,
        #[serde(rename = "startsAt")]
        starts_at: String,
        #[serde(rename = "endsAt")]
        ends_at: String,
        reason: String,
    },
    #[serde(rename = "reservation.confirm")]
    ReservationConfirm {
        #[serde(rename = "resourceId")]
        resource_id: String,
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
    },
    #[serde(rename = "reservation.move")]
    ReservationMove {
        #[serde(rename = "resourceId")]
        resource_id: String,
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "startsAt")]
        starts_at: String,
        #[serde(rename = "endsAt")]
        ends_at: String,
        reason: String,
    },
    #[serde(rename = "reservation.cancel")]
    ReservationCancel {
        #[serde(rename = "resourceId")]
        resource_id: String,
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        reason: String,
    },
}

impl SchedulingCommand {
    /// The discriminator — audit rows record it verbatim.
    pub fn operation(&self) -> &'static str {
        match self {
            Self::ResourceCreate { .. } => "resource.create",
            Self::ResourceSetActive { .. } => "resource.set-active",
            Self::ReservationHold { .. } => "reservation.hold",
            Self::ReservationBlock { .. } => "reservation.block",
            Self::ReservationConfirm { .. } => "reservation.confirm",
            Self::ReservationMove { .. } => "reservation.move",
            Self::ReservationCancel { .. } => "reservation.cancel",
        }
    }

    /// Which write permission the mutation needs
    /// (`resource.*` → `resources.write`, `reservation.*` →
    /// `reservations.write`; the read-floor and the block's extra
    /// `resources.write` belong to the repository ladder).
    pub fn required_permission(&self) -> &'static str {
        if self.operation().starts_with("resource.") {
            "resources.write"
        } else {
            "reservations.write"
        }
    }

    /// The audit reason — on the arms that carry one (`'reason' in
    /// command ? command.reason : null`).
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::ResourceCreate { .. } => None,
            Self::ResourceSetActive { reason, .. } => Some(reason),
            Self::ReservationHold { .. } => None,
            Self::ReservationBlock { reason, .. }
            | Self::ReservationMove { reason, .. }
            | Self::ReservationCancel { reason, .. } => Some(reason),
            Self::ReservationConfirm { .. } => None,
        }
    }
}

/// The strict mutation document — `{requestId, command}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulingMutation {
    pub request_id: String,
    pub command: SchedulingCommand,
}

// --- parse helpers (module-local, the purchasing grammar's style) ------------

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

/// The interval refine (`validInterval` on parsed instants).
fn valid_interval(starts_at: &SchedulingInstant, ends_at: &SchedulingInstant) -> bool {
    let length = ends_at.epoch_millis() - starts_at.epoch_millis();
    length > 0 && length <= INTERVAL_MS_MAX
}

/// `schedulingInstantSchema.parse` as a member — the error names the member
/// the schema would have flagged.
fn instant_member(
    object: &serde_json::Map<String, Value>,
    name: &'static str,
) -> Result<SchedulingInstant, ValidationError> {
    let raw = object
        .get(name)
        .and_then(Value::as_str)
        .ok_or(ValidationError(name))?;
    parse_instant(raw).ok_or(ValidationError(name))
}

/// `schedulingMutationSchema.parse` — strict keys, uuid requestId, strict
/// command by operation. Errors name the member the schema would have
/// flagged, as the R04 parses do.
#[allow(clippy::result_large_err)]
pub fn parse_mutation(raw: &Value) -> Result<SchedulingMutation, ValidationError> {
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
    Ok(SchedulingMutation {
        request_id,
        command,
    })
}

/// The strict command document parse.
#[allow(clippy::result_large_err)]
pub fn parse_command(
    command: &serde_json::Map<String, Value>,
) -> Result<SchedulingCommand, ValidationError> {
    let operation = command
        .get("operation")
        .and_then(Value::as_str)
        .ok_or(ValidationError("operation"))?;
    match operation {
        "resource.create" => {
            if !strict_members(command, &["operation", "title", "kind", "timezone"]) {
                return Err(ValidationError("operation"));
            }
            let title = bounded_member(command, "title", SCHEDULING_TITLE_MAX)
                .ok_or(ValidationError("title"))?;
            let kind = command
                .get("kind")
                .and_then(Value::as_str)
                .and_then(SchedulingResourceKind::parse)
                .ok_or(ValidationError("kind"))?;
            let timezone = valid_timezone(
                command
                    .get("timezone")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )
            .ok_or(ValidationError("timezone"))?;
            Ok(SchedulingCommand::ResourceCreate {
                title,
                kind,
                timezone: timezone.to_string(),
            })
        }
        "resource.set-active" => {
            if !strict_members(
                command,
                &[
                    "operation",
                    "resourceId",
                    "expectedRevision",
                    "active",
                    "reason",
                ],
            ) {
                return Err(ValidationError("operation"));
            }
            let resource_id =
                required_uuid(command, "resourceId").ok_or(ValidationError("resourceId"))?;
            let expected_revision = bounded_integer(command, "expectedRevision", 1, REVISION_MAX)
                .ok_or(ValidationError("expectedRevision"))?;
            let active = command
                .get("active")
                .and_then(Value::as_bool)
                .ok_or(ValidationError("active"))?;
            let reason = bounded_member(command, "reason", SCHEDULING_REASON_MAX)
                .ok_or(ValidationError("reason"))?;
            Ok(SchedulingCommand::ResourceSetActive {
                resource_id,
                expected_revision,
                active,
                reason,
            })
        }
        "reservation.hold" => {
            if !strict_members(
                command,
                &[
                    "operation",
                    "resourceId",
                    "title",
                    "startsAt",
                    "endsAt",
                    "holdSeconds",
                ],
            ) && !strict_members(
                command,
                &["operation", "resourceId", "title", "startsAt", "endsAt"],
            ) {
                return Err(ValidationError("operation"));
            }
            let resource_id =
                required_uuid(command, "resourceId").ok_or(ValidationError("resourceId"))?;
            let title = bounded_member(command, "title", SCHEDULING_TITLE_MAX)
                .ok_or(ValidationError("title"))?;
            let starts_at = instant_member(command, "startsAt")?;
            let ends_at = instant_member(command, "endsAt")?;
            if !valid_interval(&starts_at, &ends_at) {
                return Err(ValidationError("startsAt"));
            }
            let hold_seconds = match command.get("holdSeconds") {
                Some(Value::Number(_)) => {
                    bounded_integer(command, "holdSeconds", 1, HOLD_SECONDS_MAX)
                        .ok_or(ValidationError("holdSeconds"))?
                }
                // The zod default applies to ABSENT only — an explicit null
                // is invalid_type, same as any other non-number member.
                None => HOLD_SECONDS_DEFAULT,
                Some(_) => return Err(ValidationError("holdSeconds")),
            };
            Ok(SchedulingCommand::ReservationHold {
                resource_id,
                title,
                starts_at: starts_at.canonical().to_string(),
                ends_at: ends_at.canonical().to_string(),
                hold_seconds,
            })
        }
        "reservation.block" => {
            if !strict_members(
                command,
                &[
                    "operation",
                    "resourceId",
                    "title",
                    "startsAt",
                    "endsAt",
                    "reason",
                ],
            ) {
                return Err(ValidationError("operation"));
            }
            let resource_id =
                required_uuid(command, "resourceId").ok_or(ValidationError("resourceId"))?;
            let title = bounded_member(command, "title", SCHEDULING_TITLE_MAX)
                .ok_or(ValidationError("title"))?;
            let starts_at = instant_member(command, "startsAt")?;
            let ends_at = instant_member(command, "endsAt")?;
            if !valid_interval(&starts_at, &ends_at) {
                return Err(ValidationError("startsAt"));
            }
            let reason = bounded_member(command, "reason", SCHEDULING_REASON_MAX)
                .ok_or(ValidationError("reason"))?;
            Ok(SchedulingCommand::ReservationBlock {
                resource_id,
                title,
                starts_at: starts_at.canonical().to_string(),
                ends_at: ends_at.canonical().to_string(),
                reason,
            })
        }
        "reservation.confirm" | "reservation.move" | "reservation.cancel" => {
            let keys = match operation {
                "reservation.confirm" => {
                    vec!["operation", "resourceId", "id", "expectedRevision"]
                }
                "reservation.move" => {
                    vec![
                        "operation",
                        "resourceId",
                        "id",
                        "expectedRevision",
                        "startsAt",
                        "endsAt",
                        "reason",
                    ]
                }
                _ => vec![
                    "operation",
                    "resourceId",
                    "id",
                    "expectedRevision",
                    "reason",
                ],
            };
            if !strict_members(command, &keys) {
                return Err(ValidationError("operation"));
            }
            let resource_id =
                required_uuid(command, "resourceId").ok_or(ValidationError("resourceId"))?;
            let id = required_uuid(command, "id").ok_or(ValidationError("id"))?;
            let expected_revision = bounded_integer(command, "expectedRevision", 1, REVISION_MAX)
                .ok_or(ValidationError("expectedRevision"))?;
            match operation {
                "reservation.confirm" => Ok(SchedulingCommand::ReservationConfirm {
                    resource_id,
                    id,
                    expected_revision,
                }),
                "reservation.cancel" => {
                    let reason = bounded_member(command, "reason", SCHEDULING_REASON_MAX)
                        .ok_or(ValidationError("reason"))?;
                    Ok(SchedulingCommand::ReservationCancel {
                        resource_id,
                        id,
                        expected_revision,
                        reason,
                    })
                }
                _ => {
                    let starts_at = instant_member(command, "startsAt")?;
                    let ends_at = instant_member(command, "endsAt")?;
                    if !valid_interval(&starts_at, &ends_at) {
                        return Err(ValidationError("startsAt"));
                    }
                    let reason = bounded_member(command, "reason", SCHEDULING_REASON_MAX)
                        .ok_or(ValidationError("reason"))?;
                    Ok(SchedulingCommand::ReservationMove {
                        resource_id,
                        id,
                        expected_revision,
                        starts_at: starts_at.canonical().to_string(),
                        ends_at: ends_at.canonical().to_string(),
                        reason,
                    })
                }
            }
        }
        _ => Err(ValidationError("operation")),
    }
}

/// The availability-side row DTO the status/recovery re-reads ride — the
/// full reservation document `reservationSchema` produces, WITH the
/// effective-state derivation the mutas path needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchedulingReservation {
    pub id: String,
    #[serde(rename = "tenantId")]
    pub tenant_id: String,
    #[serde(rename = "resourceId")]
    pub resource_id: String,
    #[serde(rename = "ownerId")]
    pub owner_id: String,
    pub title: String,
    pub kind: SchedulingReservationKind,
    pub state: SchedulingReservationState,
    #[serde(rename = "effectiveState")]
    pub effective_state: SchedulingEffectiveState,
    #[serde(rename = "startsAt")]
    pub starts_at: String,
    #[serde(rename = "endsAt")]
    pub ends_at: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: Option<String>,
    pub revision: i64,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

/// The resource row DTO — `schedulingResourceSchema`. `capacity` is pinned
/// to literal 1 by the schema and it is where the row-side check bites.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchedulingResource {
    pub id: String,
    #[serde(rename = "tenantId")]
    pub tenant_id: String,
    #[serde(rename = "ownerId")]
    pub owner_id: String,
    pub title: String,
    pub kind: SchedulingResourceKind,
    pub timezone: String,
    pub active: bool,
    pub capacity: i64,
    pub revision: i64,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

/// The receipt — `schedulingReceiptSchema`. The replayed marker follows the
/// R04 receipt pattern: never serialized, a host-side decision field only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchedulingReceipt {
    pub kind: String,
    pub id: String,
    #[serde(rename = "resourceId")]
    pub resource_id: String,
    pub revision: i64,
    /// Replay knowledge for the HOST decision — never serialized, per the
    /// `.strict()` receipt schema.
    #[serde(skip_serializing)]
    pub replayed: bool,
}

impl SchedulingReceipt {
    /// The stored receipt's strict parse — the recovery path re-derives the
    /// document through this surface.
    #[allow(clippy::result_large_err)]
    pub fn parse(raw: &Value) -> Result<Self, ValidationError> {
        let object = raw.as_object().ok_or(ValidationError("kind"))?;
        if !strict_members(object, &["kind", "id", "resourceId", "revision"]) {
            return Err(ValidationError("kind"));
        }
        let kind = object
            .get("kind")
            .and_then(Value::as_str)
            .filter(|kind| SCHEDULING_RECEIPT_KINDS.contains(kind))
            .ok_or(ValidationError("kind"))?;
        let id = required_uuid(object, "id").ok_or(ValidationError("id"))?;
        let resource_id =
            required_uuid(object, "resourceId").ok_or(ValidationError("resourceId"))?;
        let revision = bounded_integer(object, "revision", 1, REVISION_MAX)
            .ok_or(ValidationError("revision"))?;
        Ok(Self {
            kind: kind.to_string(),
            id,
            resource_id,
            revision,
            replayed: false,
        })
    }
}

/// Builds the replayed receipt — same class as the other ledgers' replay
/// flips.
pub fn replayed_receipt(receipt: &SchedulingReceipt) -> SchedulingReceipt {
    let mut receipt = receipt.clone();
    receipt.replayed = true;
    receipt
}

/// The command-status union — `schedulingCommandStatusSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status")]
pub enum SchedulingCommandStatus {
    #[serde(rename = "committed")]
    Committed {
        #[serde(rename = "requestId")]
        request_id: String,
        receipt: SchedulingReceipt,
    },
    #[serde(rename = "not_committed")]
    NotCommitted {
        #[serde(rename = "requestId")]
        request_id: String,
    },
}

// --- calendar (reserved, accommodation cohort) -------------------------------

// The calendar helpers of `packages/scheduling/src/calendar.ts` —
// `calendarDateSchema`, `calendarTimeSchema`, `calendarNights` and the
// DST-exacting `resolveCalendarInstant` — port with the ACCOMMODATION
// cohort, the only consumer (`scheduling` itself never resolves wall
// clock). Kept as a listed reserved surface here so the port ledger shows
// the shape was not silently dropped; the module carrying them lands with
// the stay grammar.

// --- portable invariant tests ------------------------------------------------

#[allow(clippy::result_large_err)] // (test-side parse errors are truthy carriers)
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn instant_accepts_canonical_z_and_normalizes_offsets() {
        let instant = parse_instant("2026-01-05T08:00:00Z").expect("canonical");
        assert_eq!(instant.canonical(), "2026-01-05T08:00:00.000Z");
        // +07:00 renders as UTC before it.
        let offset = parse_instant("2026-01-05T15:00:00+07:00").expect("offset");
        assert_eq!(offset.canonical(), "2026-01-05T08:00:00.000Z");
        assert_eq!(offset.epoch_millis(), instant.epoch_millis());
    }

    #[test]
    fn instant_refuses_ambiguous_and_out_of_band_input() {
        // Wall-clock without an offset — the refine refuses.
        assert!(parse_instant("2026-01-05T08:00:00").is_none());
        assert!(parse_instant("2026-01-05T08:00:00.000").is_none()); // no Z
                                                                     // Out of the range the regex pins (2000–2100).
        assert!(parse_instant("2110-01-05T08:00:00Z").is_none());
        assert!(parse_instant("1999-01-05T08:00:00Z").is_none());
        // Impossible calendar dates — `Date.parse` returns NaN.
        assert!(parse_instant("2026-02-30T08:00:00Z").is_none());
        assert!(parse_instant("2100-02-29T08:00:00Z").is_none()); // 2100 not leap
        assert!(parse_instant("2024-02-29T08:00:00Z").is_some()); // 2024 leap
        assert!(parse_instant("2026-13-01T08:00:00Z").is_none());
        // Fraction beyond 3 digits — the regex's `\.d{1,3}` cap.
        assert!(parse_instant("2026-01-05T08:00:00.0001Z").is_none());
        // Offset beyond 23:59 — `Date.parse` refuses.
        assert!(parse_instant("2026-01-05T08:00:00+24:00").is_none());
        assert!(parse_instant("2026-01-05T08:00:00+23:59").is_some());
    }

    #[test]
    fn hold_seconds_defaults_to_900_and_interval_bounds_hold() {
        // (The parse fill is exercised via parse_mutation below.)
        let mutation = serde_json::json!({
            "requestId": "2b1c1a58-1a90-4a20-9a35-1a59499fcb7f",
            "command": {
                "operation": "reservation.hold",
                "resourceId": "2b1c1a58-1a90-4a20-9a35-1a59499fcb70",
                "title": "  Resus bay  ",
                "startsAt": "2026-01-05T08:00:00Z",
                "endsAt": "2026-01-05T09:00:00Z"
            }
        });
        let parsed = parse_mutation(&mutation).expect("hold with default");
        assert_eq!(
            parsed.command,
            SchedulingCommand::ReservationHold {
                resource_id: "2b1c1a58-1a90-4a20-9a35-1a59499fcb70".to_string(),
                title: "Resus bay".to_string(),
                starts_at: "2026-01-05T08:00:00.000Z".to_string(),
                ends_at: "2026-01-05T09:00:00.000Z".to_string(),
                hold_seconds: HOLD_SECONDS_DEFAULT,
            }
        );
    }

    #[test]
    fn interval_must_be_positive_and_at_most_31_days() {
        let mutation = serde_json::json!({
            "requestId": "2b1c1a58-1a90-4a20-9a35-1a59499fcb7f",
            "command": {
                "operation": "reservation.block",
                "resourceId": "2b1c1a58-1a90-4a20-9a35-1a59499fcb70",
                "title": "Maintenance window",
                "startsAt": "2026-01-05T08:00:00Z",
                "endsAt": "2026-02-06T08:00:00Z",
                "reason": "long shutdown"
            }
        });
        // 32 days — a day too long.
        assert!(parse_mutation(&mutation).is_err());
        let mutation = serde_json::json!({
            "requestId": "2b1c1a58-1a90-4a20-9a35-1a59499fcb7f",
            "command": {
                "operation": "reservation.block",
                "resourceId": "2b1c1a58-1a90-4a20-9a35-1a59499fcb70",
                "title": "Maintenance window",
                "startsAt": "2026-01-05T08:00:00Z",
                "endsAt": "2026-02-05T08:00:00Z",
                "reason": "exactly 31 days"
            }
        });
        assert!(parse_mutation(&mutation).is_ok());
    }

    #[test]
    fn strict_members_refuse_unknown_and_missing_keys() {
        // `holdSeconds` of the wrong SHAPE (a string) — not the default
        // branch, a hard refusal.
        let mutation = serde_json::json!({
            "requestId": "2b1c1a58-1a90-4a20-9a35-1a59499fcb7f",
            "command": {
                "operation": "reservation.hold",
                "resourceId": "2b1c1a58-1a90-4a20-9a35-1a59499fcb70",
                "title": "Resus bay",
                "startsAt": "2026-01-05T08:00:00Z",
                "endsAt": "2026-01-05T09:00:00Z",
                "holdSeconds": "ten"
            }
        });
        assert!(parse_mutation(&mutation).is_err());
        // Unknown member on set-active — strict refusal.
        let mutation = serde_json::json!({
            "requestId": "2b1c1a58-1a90-4a20-9a35-1a59499fcb7f",
            "command": {
                "operation": "resource.set-active",
                "resourceId": "2b1c1a58-1a90-4a20-9a35-1a59499fcb70",
                "expectedRevision": 1,
                "active": false,
                "reason": "decommissioning",
                "extra": true
            }
        });
        assert!(parse_mutation(&mutation).is_err());
    }
}
