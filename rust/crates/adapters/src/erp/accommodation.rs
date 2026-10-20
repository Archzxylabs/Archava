//! The accommodation SQL adapter — the Archxy 2 port of
//! `packages/accommodation-pg/src/repository.ts` (`mutate()` +
//! `commandStatus()`, the R05 second-cohort surface) plus the occupancy
//! runtime helpers it composes (`packages/scheduling-pg/src/occupancy.ts`).
//!
//! ## Statement order (the TS port)
//!
//! The `mutate()` ladder is the ERP cohorts' ladder with accommodation's
//! vocabulary: the membership row (FOR SHARE through
//! `archava_accommodation.lock_membership`, resolved by the host's
//! context step), the human/service on-behalf intersection, the
//! operation's own write permission (with `room.housekeeping`'s inspected
//! arm needing the finer `rooms.inspect` grant), then `mutate`'s
//! read-floor grants (`properties.read` always, `rooms.read` beside every
//! non-create arm, `stays.read` beside the stay arms), the `commands`
//! claim, then the arm's effect. Effect + audit row + receipt binding all
//! share ONE transaction the host commits exactly once.
//!
//! ## The occupancy composition (the whole point of this cohort)
//!
//! Rooms are scheduling resources (`managed_by='accommodation'`, created
//! through scheduling's `create_accommodation_resource` RPC) and stays are
//! occupancy slots (`archava_scheduling.occupancy`,
//! `owner_module='accommodation'`). Every stay-unit write serializes on
//! the RESOURCE row first (`lock_accommodation_resource` — FOR UPDATE)
//! and samples `clock_timestamp()` only after that lock; the stay's
//! INSERT and every room UPDATE then re-refuse through migration
//! `001_room_night_stays`' triggers (`guard_room`, `guard_stay`,
//! `dirty_after_stay`, `night_guard`) and the occupancy kernel's own
//! `guard_occupancy` — the 23P01 overlap verdict IS the shared room's
//! single-tenant-across-modules authority. Those SQLSTATE words surface
//! as conflict, exactly the TS catch whitelist does.
//!
//! ## The calendar resolution (recorded divergence, never silent)
//!
//! `period()` resolves `arrival`/`departure` calendar dates against the
//! property's check-in/check-out daily time IN THE PROPERTY'S TIMEZONE.
//! The TS side enumerates zone offsets through live `Intl`
//! (`packages/scheduling/src/calendar.ts`); the pure domain has no tz
//! database, so this port runs the TS algorithm — probes at −48..+48
//! hours in 4-hour steps, requiring EXACTLY ONE round-tripping instant,
//! ambiguity refused as conflict (the `CalendarTimeError` verdict) —
//! against PostgreSQL's bundled IANA tzdata via
//! `to_char(instant AT TIME ZONE zone, …)`. Both sides refuse the same
//! ambiguity classes; the zone authority differs (Node's ICU vs the
//! database's tzdata), recorded in the packet's KNOWN_DIFFERENCES ledger.
//!
//! ## The refusal whitelist
//!
//! `conflict` maps from `23P01` (the shared-occupancy overlap verdict),
//! then through the crate classifier (`23514`, `23503`, `23505`,
//! `22003`, plus deadlocks) — the TS accommodation catch names `23P01`,
//! `23514`, `23503`, `23505`, `22003` and gets them ALL through those two
//! layers.
//!
//! ## Deliberately not here
//!
//! The repository's read paths (`access`, `properties`, `roomTypes`,
//! `rooms`, `room`, `stays`, `detail`, `availability`) and the tools
//! surface ride their serving cohorts: no mounted read route exists yet,
//! so porting them would ship unused SQL with no witness. The mutation +
//! commandStatus surface this module ports is exactly what the host
//! routes and the suite drive, plus `assert_accommodation_kernel` — the
//! port of repository `create()`'s provision checks plus
//! `assertOccupancyKernel` (the startup support check).

use sqlx::PgConnection;
use uuid::Uuid;

use archava_domain::erp::accommodation::{
    calendar_nights, parse_calendar_date, parse_calendar_time, AccommodationAccess,
    AccommodationCommand, AccommodationCommandStatus, AccommodationMutation, AccommodationProperty,
    AccommodationReceipt, AccommodationRoom, AccommodationRoomType, AccommodationStay, CivilDate,
    Housekeeping, StayEffectiveState, StayState, ACCOMMODATION_PERMISSIONS,
    ACCOMMODATION_REVISION_MAX, ACCOMMODATION_TITLE_MAX,
};
use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
use archava_domain::erp::scheduling::valid_timezone;
use archava_domain::erp::{ErpError, ErpMembershipKind, ErpScope};

use super::commerce_kernel::{load_locked_membership, MembershipRow};
use super::PurchaseError;

/// The ledger label on database errors.
pub const ACCOMMODATION_LEDGER: &str = "accommodation";

/// The `archava_accommodation` namespace — checked constants only.
pub const ACCOMMODATION_NAMESPACE: &str = "archava_accommodation";

// --- Refusal helpers ---------------------------------------------------------

fn conflict() -> PurchaseError {
    PurchaseError::Domain(ErpError::Conflict)
}

fn forbidden() -> PurchaseError {
    PurchaseError::Domain(ErpError::Forbidden)
}

fn not_found() -> PurchaseError {
    PurchaseError::Domain(ErpError::NotFound)
}

/// The accommodation catch whitelist: 23P01 is the occupancy kernel's
/// overlap verdict (`guard_occupancy`'s 'Shared occupancy overlaps');
/// `23514`/`23503`/`23505`/`22003` reclassify through the crate-level
/// rule exactly like the other ERP ledgers.
fn refuses(error: sqlx::Error) -> PurchaseError {
    if let sqlx::Error::Database(database) = &error {
        if database.code().as_deref() == Some("23P01") {
            return conflict();
        }
    }
    super::purchase_database_error(ACCOMMODATION_LEDGER, error)
}

fn parse_uuid_value(value: &str) -> Result<Uuid, PurchaseError> {
    Uuid::parse_str(value).map_err(|_| conflict())
}

fn int4(value: i64) -> Result<i32, PurchaseError> {
    if !(i32::MIN as i64..=i32::MAX as i64).contains(&value) {
        return Err(conflict());
    }
    Ok(value as i32)
}

/// The `clock_timestamp()` sample every write takes AFTER its resource
/// lock — the canonical millisecond `Z` render, same family as the
/// scheduling adapter's clock.
async fn observed_now(connection: &mut PgConnection) -> Result<String, PurchaseError> {
    sqlx::query_as::<_, (String,)>(
        "SELECT to_char(clock_timestamp() at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .fetch_one(&mut *connection)
    .await
    .map(|(text,)| text)
    .map_err(refuses)
}

/// The point-in-time of a canonical render.
fn instant_millis(raw: &str) -> Result<i64, PurchaseError> {
    archava_domain::erp::scheduling::parse_instant(raw)
        .map(|instant| instant.epoch_millis())
        .ok_or_else(conflict)
}

/// An instant bind parameter (the canonical render the SQL casts bound in).
fn instant_bind(raw: &str) -> Result<String, PurchaseError> {
    archava_domain::erp::scheduling::parse_instant(raw)
        .map(|instant| instant.canonical().to_string())
        .ok_or_else(conflict)
}

// --- The calendar resolution (Postgres tzdata authority) -----------------------

/// `calendarToday(observed, timezone)` — the observed instant's wall clock
/// date in the zone. The TS slices the Intl wall render; this SQL render
/// is the same authority family (bundled tzdata).
async fn calendar_today(
    connection: &mut PgConnection,
    observed_millis: i64,
    timezone: &str,
) -> Result<CivilDate, PurchaseError> {
    let (date,) = sqlx::query_as::<_, (String,)>(
        "SELECT to_char(to_timestamp($1::double precision / 1000.0) AT TIME ZONE $2, 'YYYY-MM-DD')",
    )
    .bind(observed_millis as f64)
    .bind(timezone)
    .fetch_one(&mut *connection)
    .await
    .map_err(refuses)?;
    parse_calendar_date(&date).ok_or_else(conflict)
}

const RESOLVE_PROBE_STATEMENT: &str = "SELECT DISTINCT \
  (EXTRACT(EPOCH FROM probes.wall::timestamp) * 1000)::bigint - probes.probe_ms \
  FROM (SELECT ($1::bigint + h::bigint * 3_600_000) AS probe_ms, \
               to_char(to_timestamp((($1::bigint + h::bigint * 3_600_000)::double precision) \
                                    / 1000.0) AT TIME ZONE $2, 'YYYY-MM-DD\"T\"HH24:MI:SS') AS wall \
        FROM generate_series(-48, 48, 4) AS h) probes";

const RESOLVE_VERIFY_STATEMENT: &str =
    "SELECT to_char(to_timestamp($1::double precision / 1000.0) AT TIME ZONE $2, \
     'YYYY-MM-DD\"T\"HH24:MI:SS') = $3 AS hits";

/// `resolveCalendarInstant(date, time, timezone)` — the TS algorithm
/// verbatim with Postgres as the wall-clock authority:
///
/// 1. probe instants at −48..+48 hours (4-hour steps) around the naive
///    render, collecting each probe's wall clock and the offset the
///    round-trip implies;
/// 2. one candidate offset set (distinct) → candidate instants
///    `naive - offset`;
/// 3. exactly ONE candidate must re-render as the target wall clock —
///    anything else is the CalendarTimeError verdict, i.e. conflict.
///
/// No silent DST guessing on either side.
async fn resolve_calendar_instant(
    connection: &mut PgConnection,
    date: &CivilDate,
    time: (u32, u32),
    timezone: &str,
) -> Result<String, PurchaseError> {
    let naive_millis =
        date.epoch_days * 86_400_000 + i64::from(time.0) * 3_600_000 + i64::from(time.1) * 60_000;
    let offsets: Vec<i64> = sqlx::query_scalar(RESOLVE_PROBE_STATEMENT)
        .bind(naive_millis)
        .bind(timezone)
        .fetch_all(&mut *connection)
        .await
        .map_err(refuses)?;
    let target = format!(
        // The parsed calendar date IS the canonical render (the parse
        // refused every non-canonical shape).
        "{}T{:02}:{:02}:00",
        date.render(),
        time.0,
        time.1
    );
    let mut hits: Vec<i64> = Vec::new();
    for offset in offsets {
        let candidate = naive_millis - offset;
        let (hit,): (bool,) = sqlx::query_as(RESOLVE_VERIFY_STATEMENT)
            .bind(candidate as f64)
            .bind(timezone)
            .bind(&target)
            .fetch_one(&mut *connection)
            .await
            .map_err(refuses)?;
        if hit {
            hits.push(candidate);
        }
    }
    // The TS verdict: `candidates.length !== 1` → CalendarTimeError.
    if hits.len() == 1 {
        Ok(archava_domain::erp::scheduling::instant_render(hits[0]))
    } else {
        Err(conflict())
    }
}

/// `period(property, arrival, departure)` — the stay's nights plus the
/// resolved check-in/check-out instants, in the property's timezone.
struct StayPeriod {
    nights: Vec<String>,
    starts_at: String,
    ends_at: String,
}

async fn stay_period(
    connection: &mut PgConnection,
    property: &AccommodationProperty,
    arrival: &str,
    departure: &str,
) -> Result<StayPeriod, PurchaseError> {
    let nights = calendar_nights(arrival, departure).map_err(|_| conflict())?;
    let arrival_date = parse_calendar_date(arrival).ok_or_else(conflict)?;
    let departure_date = parse_calendar_date(departure).ok_or_else(conflict)?;
    let check_in = parse_calendar_time(&property.check_in_time).ok_or_else(conflict)?;
    let check_out = parse_calendar_time(&property.check_out_time).ok_or_else(conflict)?;
    let starts_at =
        resolve_calendar_instant(connection, &arrival_date, check_in, &property.timezone).await?;
    let ends_at =
        resolve_calendar_instant(connection, &departure_date, check_out, &property.timezone)
            .await?;
    Ok(StayPeriod {
        nights,
        starts_at,
        ends_at,
    })
}

// --- Row grammar -------------------------------------------------------------

/// The property row's column reader — `property = (row) =>
/// propertySchema.parse({...})`. Grammar failures are ledger drift and
/// answer conflict.
fn property_row(
    row: &sqlx::postgres::PgRow,
    tenant: &str,
) -> Result<AccommodationProperty, PurchaseError> {
    use sqlx::Row;
    let tenant_id: String = row.try_get("tenant_id").map_err(refuses)?;
    if tenant_id != tenant {
        // RLS makes a cross-tenant screen impossible; a mismatch is drift.
        return Err(conflict());
    }
    let title_raw: String = row.try_get("title").map_err(refuses)?;
    let title = archava_domain::erp::trimmed(&title_raw);
    if title.is_empty() || title.len() > ACCOMMODATION_TITLE_MAX {
        return Err(conflict());
    }
    let timezone: String = row.try_get("timezone").map_err(refuses)?;
    let check_in_time: String = row.try_get("check_in_time").map_err(refuses)?;
    let check_out_time: String = row.try_get("check_out_time").map_err(refuses)?;
    if valid_timezone(&timezone).is_none()
        || parse_calendar_time(&check_in_time).is_none()
        || parse_calendar_time(&check_out_time).is_none()
        || !calendar_time_order(&check_out_time, &check_in_time)
    {
        return Err(conflict());
    }
    let revision: i32 = row.try_get("revision").map_err(refuses)?;
    let revision = i64::from(revision);
    if !(1..=ACCOMMODATION_REVISION_MAX).contains(&revision) {
        return Err(conflict());
    }
    Ok(AccommodationProperty {
        id: row.try_get::<Uuid, _>("id").map_err(refuses)?.to_string(),
        tenant_id,
        owner_id: row.try_get("owner_id").map_err(refuses)?,
        title: title.to_string(),
        timezone,
        check_in_time,
        check_out_time,
        revision,
    })
}

/// The daily-time daily-cycle check (`check_out <= check_in`, the shared
/// grammar family — same comparison the create refine makes).
fn calendar_time_order(earlier: &str, later: &str) -> bool {
    match (parse_calendar_time(earlier), parse_calendar_time(later)) {
        (Some((a_h, a_m)), Some((b_h, b_m))) => (a_h, a_m) <= (b_h, b_m),
        _ => false,
    }
}

/// The room-type row's column reader — `roomType = (row | undefined) =>`.
fn room_type_row(row: &sqlx::postgres::PgRow) -> Result<AccommodationRoomType, PurchaseError> {
    use sqlx::Row;
    let id: Uuid = row.try_get("id").map_err(refuses)?;
    let tenant_id: String = row.try_get("tenant_id").map_err(refuses)?;
    let property_id: Uuid = row.try_get("property_id").map_err(refuses)?;
    let title_raw: String = row.try_get("title").map_err(refuses)?;
    let title = archava_domain::erp::trimmed(&title_raw);
    if title.is_empty() || title.len() > ACCOMMODATION_TITLE_MAX {
        return Err(conflict());
    }
    let max_guests: i32 = row.try_get("max_guests").map_err(refuses)?;
    let nightly_rate: i32 = row.try_get("nightly_rate").map_err(refuses)?;
    let revision: i32 = row.try_get("revision").map_err(refuses)?;
    if !(1..=16).contains(&i64::from(max_guests))
        || !(0..=archava_domain::erp::accommodation::ACCOMMODATION_RATE_MAX)
            .contains(&i64::from(nightly_rate))
    {
        return Err(conflict());
    }
    let revision = i64::from(revision);
    if !(1..=ACCOMMODATION_REVISION_MAX).contains(&revision) {
        return Err(conflict());
    }
    Ok(AccommodationRoomType {
        id: id.to_string(),
        tenant_id,
        property_id: property_id.to_string(),
        title: title.to_string(),
        max_guests: i64::from(max_guests),
        nightly_rate: i64::from(nightly_rate),
        revision,
    })
}

/// The room row's column reader — the arms' cells, with the `occupied`
/// EXISTS cell the TS read computes beside the row.
fn room_row(row: &sqlx::postgres::PgRow, tenant: &str) -> Result<AccommodationRoom, PurchaseError> {
    use sqlx::Row;
    let tenant_id: String = row.try_get("tenant_id").map_err(refuses)?;
    if tenant_id != tenant {
        return Err(conflict());
    }
    let id: Uuid = row.try_get("id").map_err(refuses)?;
    let property_id: Uuid = row.try_get("property_id").map_err(refuses)?;
    let room_type_id: Uuid = row.try_get("room_type_id").map_err(refuses)?;
    let active: bool = row.try_get("active").map_err(refuses)?;
    let occupied: bool = row.try_get("occupied").map_err(refuses)?;
    let housekeeping_raw: String = row.try_get("housekeeping").map_err(refuses)?;
    let housekeeping = Housekeeping::parse(&housekeeping_raw).ok_or_else(conflict)?;
    let revision: i32 = row.try_get("revision").map_err(refuses)?;
    let revision = i64::from(revision);
    if !(1..=ACCOMMODATION_REVISION_MAX).contains(&revision) {
        return Err(conflict());
    }
    Ok(AccommodationRoom {
        id: id.to_string(),
        tenant_id,
        property_id: property_id.to_string(),
        room_type_id: room_type_id.to_string(),
        active,
        housekeeping,
        occupied,
        revision,
    })
}

/// The stay row's column reader — `stay = (row, observed) =>` with the
/// effective-state rule (a stale hold reads `expired`).
fn stay_row(
    row: &sqlx::postgres::PgRow,
    tenant: &str,
    observed_millis: i64,
) -> Result<AccommodationStay, PurchaseError> {
    use sqlx::Row;
    let tenant_id: String = row.try_get("tenant_id").map_err(refuses)?;
    if tenant_id != tenant {
        return Err(conflict());
    }
    let property_id: Uuid = row.try_get("property_id").map_err(refuses)?;
    let room_id: Uuid = row.try_get("room_id").map_err(refuses)?;
    let state_raw: String = row.try_get("state").map_err(refuses)?;
    let starts_at: String = row.try_get("starts_at").map_err(refuses)?;
    let ends_at: String = row.try_get("ends_at").map_err(refuses)?;
    let expires_at: String = row.try_get("expires_at").map_err(refuses)?;
    let revision: i32 = row.try_get("revision").map_err(refuses)?;
    let state = StayState::parse(&state_raw).ok_or_else(conflict)?;
    let revision = i64::from(revision);
    if !(1..=ACCOMMODATION_REVISION_MAX).contains(&revision) {
        return Err(conflict());
    }
    let effective_state =
        StayEffectiveState::of(state, instant_millis(&expires_at)?, observed_millis);
    Ok(AccommodationStay {
        id: row.try_get::<Uuid, _>("id").map_err(refuses)?.to_string(),
        tenant_id,
        property_id: property_id.to_string(),
        room_id: room_id.to_string(),
        state,
        effective_state,
        starts_at,
        ends_at,
        expires_at,
        revision,
    })
}

// --- The context ladder ------------------------------------------------------

/// The membership ladder — ONE ledger. Loads the actor's own grant,
/// refuses inactive/unknown/shape-foreign rows, intersects with the human
/// principal a service names, and hands the access envelope back. The TS
/// `member(...)` + service branch of `transaction()`, statement for
/// statement.
pub async fn resolve_accommodation_context(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<AccommodationAccess, PurchaseError> {
    let own: MembershipRow = load_locked_membership(
        connection,
        ACCOMMODATION_NAMESPACE,
        ACCOMMODATION_LEDGER,
        tenant,
        &principal.id,
        ACCOMMODATION_PERMISSIONS,
    )
    .await?
    .ok_or_else(forbidden)?;
    // The TS ladder compares the ROW's kind against the PRINCIPAL's kind
    // directly (`actor.kind !== principal.kind`).
    if !own.active || own.kind.as_str() != principal.kind.as_str() {
        return Err(forbidden());
    }
    Ok(match principal.kind {
        PrincipalKind::Human => AccommodationAccess {
            permissions: own.permissions.clone(),
            owner_id: principal.id.clone(),
            scope: own.scope,
        },
        PrincipalKind::Service => {
            let on_behalf_of = principal.on_behalf_of.as_deref().ok_or_else(forbidden)?;
            let human: MembershipRow = load_locked_membership(
                connection,
                ACCOMMODATION_NAMESPACE,
                ACCOMMODATION_LEDGER,
                tenant,
                on_behalf_of,
                ACCOMMODATION_PERMISSIONS,
            )
            .await?
            .ok_or_else(forbidden)?;
            if human.kind != ErpMembershipKind::Human || !human.active {
                return Err(forbidden());
            }
            AccommodationAccess {
                permissions: own
                    .permissions
                    .iter()
                    .filter(|held| human.permissions.contains(*held))
                    .cloned()
                    .collect(),
                owner_id: on_behalf_of.to_string(),
                scope: match (own.scope, human.scope) {
                    (ErpScope::Assigned, _) | (_, ErpScope::Assigned) => ErpScope::Assigned,
                    (ErpScope::All, ErpScope::All) => ErpScope::All,
                },
            }
        }
    })
}

// --- The owned-row reads -----------------------------------------------------

/// `ownedProperty` — the `lock_property` SECURITY DEFINER lock with the
/// scope-or-owner predicate on top (a scoped access cannot see a
/// property it does not own; the SQL returns zero rows and the TS
/// `not_found` verdict follows).
const PROPERTY_SELECT: &str =
    "SELECT tenant_id,id,owner_id,title,timezone,check_in_time,check_out_time,revision \
     FROM archava_accommodation.lock_property($1,$2) WHERE ($3::boolean OR owner_id=$4)";

async fn owned_property(
    connection: &mut PgConnection,
    tenant: &str,
    access: &AccommodationAccess,
    id: Uuid,
) -> Result<AccommodationProperty, PurchaseError> {
    let row = sqlx::query(PROPERTY_SELECT)
        .bind(tenant)
        .bind(id)
        .bind(access.scope == ErpScope::All)
        .bind(&access.owner_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(refuses)?
        .ok_or_else(not_found)?;
    property_row(&row, tenant)
}

/// `roomRecord` — the room row WITH the one-in-house-stay `occupied`
/// EXISTS cell, no lock (the arms serialize on the occupancy resource).
const ROOM_SELECT: &str =
    "SELECT r.tenant_id,r.id,r.property_id,r.room_type_id,r.active,r.housekeeping,r.revision,\
 EXISTS(SELECT 1 FROM archava_accommodation.stays s WHERE s.tenant_id=r.tenant_id AND \
 s.room_id=r.id AND s.state='in_house') AS occupied \
 FROM archava_accommodation.rooms r WHERE r.tenant_id=$1 AND r.property_id=$2 AND r.id=$3";

async fn room_record(
    connection: &mut PgConnection,
    tenant: &str,
    property_id: Uuid,
    id: Uuid,
) -> Result<AccommodationRoom, PurchaseError> {
    let row = sqlx::query(ROOM_SELECT)
        .bind(tenant)
        .bind(property_id)
        .bind(id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(refuses)?
        .ok_or_else(not_found)?;
    room_row(&row, tenant)
}

/// The room-type row FOR SHARE (the quote the hold/set-rate reads) — the
/// TS reads it with `FOR SHARE` so a concurrent set-rate blocks until
/// this read passes without racing.
const ROOM_TYPE_SELECT_SHARE: &str =
    "SELECT tenant_id,id,property_id,title,max_guests,nightly_rate,revision \
     FROM archava_accommodation.room_types WHERE tenant_id=$1 AND property_id=$2 AND id=$3 \
     FOR SHARE";

/// The same row FOR UPDATE — `room-type.set-rate`'s optimistic gate.
const ROOM_TYPE_SELECT_UPDATE: &str =
    "SELECT tenant_id,id,property_id,title,max_guests,nightly_rate,revision \
     FROM archava_accommodation.room_types WHERE tenant_id=$1 AND property_id=$2 AND id=$3 \
     FOR UPDATE";

async fn room_type_record(
    connection: &mut PgConnection,
    tenant: &str,
    property_id: Uuid,
    id: Uuid,
    lock_update: bool,
) -> Result<AccommodationRoomType, PurchaseError> {
    let statement = if lock_update {
        ROOM_TYPE_SELECT_UPDATE
    } else {
        ROOM_TYPE_SELECT_SHARE
    };
    let row = sqlx::query(statement)
        .bind(tenant)
        .bind(property_id)
        .bind(id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(refuses)?
        .ok_or_else(not_found)?;
    room_type_row(&row)
}

/// `stayRecord` — the scope-or-owner screen, raw (the TS returns the
/// unpacked row; the callers either check location cells or re-read the
/// parsed record).
const STAY_SELECT: &str = "SELECT s.tenant_id,s.id,s.property_id,s.room_id,s.state,s.revision,\
 to_char(s.starts_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS starts_at,\
 to_char(s.ends_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS ends_at,\
 to_char(s.expires_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS expires_at \
 FROM archava_accommodation.stays s WHERE s.tenant_id=$1 AND s.id=$2 \
 AND ($3::boolean OR s.owner_id=$4)";

async fn stay_record(
    connection: &mut PgConnection,
    tenant: &str,
    access: &AccommodationAccess,
    id: Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, PurchaseError> {
    sqlx::query(STAY_SELECT)
        .bind(tenant)
        .bind(id)
        .bind(access.scope == ErpScope::All)
        .bind(&access.owner_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(refuses)
}

// --- The occupancy runtime (occupancy.ts, accommodation's helpers) -------------

/// `lockAccommodationResource` — the managed room's scheduling resource
/// FOR UPDATE, THE serialization point every stay-unit arm walks. A miss
/// is ledger drift (the room row's FK guarantees the resource), which per
/// the module header answers conflict, never silently passes.
const SCHEDULING_RESOURCE_LOCK: &str =
    "SELECT id,active FROM archava_scheduling.resources WHERE tenant_id=$1 AND id=$2 \
     AND managed_by='accommodation' FOR UPDATE";

pub struct SharedResource {
    pub id: Uuid,
    pub active: bool,
}

async fn lock_accommodation_resource(
    connection: &mut PgConnection,
    tenant: &str,
    resource_id: Uuid,
) -> Result<SharedResource, PurchaseError> {
    let row: Option<(Uuid, bool)> = sqlx::query_as(SCHEDULING_RESOURCE_LOCK)
        .bind(tenant)
        .bind(resource_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(refuses)?;
    match row {
        Some((id, active)) => Ok(SharedResource { id, active }),
        None => Err(conflict()),
    }
}

/// `createAccommodationResource` — the scheduling kernel's bounded RPC:
/// the room BECOMES a shared resource owned by this module with a fixed
/// internal service identity, which the 002 migration freezes.
pub(crate) async fn create_accommodation_resource(
    connection: &mut PgConnection,
    tenant: &str,
    resource_id: Uuid,
    title: &str,
    timezone: &str,
) -> Result<(), PurchaseError> {
    sqlx::query("SELECT archava_scheduling.create_accommodation_resource($1,$2,$3,$4)")
        .bind(tenant)
        .bind(resource_id)
        .bind(title)
        .bind(timezone)
        .execute(&mut *connection)
        .await
        .map_err(refuses)?;
    Ok(())
}

/// `holdAccommodationOccupancy` — the slot INSERT; the kernel's
/// `guard_occupancy` raises the 23P01 overlap verdict against ANY live
/// slot on the room, which is THE single-occupancy authority.
const OCCUPANCY_HOLD: &str = "INSERT INTO archava_scheduling.occupancy(\
 tenant_id,id,resource_id,owner_module,state,starts_at,ends_at,expires_at) \
 VALUES($1,$2,$3,'accommodation','held',$4::timestamptz,$5::timestamptz,$6::timestamptz)";

async fn hold_accommodation_occupancy(
    connection: &mut PgConnection,
    tenant: &str,
    id: Uuid,
    resource_id: Uuid,
    starts_at: &str,
    ends_at: &str,
    expires_at: &str,
) -> Result<(), PurchaseError> {
    sqlx::query(OCCUPANCY_HOLD)
        .bind(tenant)
        .bind(id)
        .bind(resource_id)
        .bind(instant_bind(starts_at)?)
        .bind(instant_bind(ends_at)?)
        .bind(instant_bind(expires_at)?)
        .execute(&mut *connection)
        .await
        .map_err(refuses)?;
    Ok(())
}

/// `transitionAccommodationOccupancy` — held→confirmed on the confirm
/// arm, →cancelled on the cancel/check-out arms. The kernel trigger
/// refuses every other transition; a zero-row update is drift (conflict).
const OCCUPANCY_TRANSITION: &str =
    "UPDATE archava_scheduling.occupancy SET state=$4,updated_at=clock_timestamp() \
     WHERE tenant_id=$1 AND resource_id=$2 AND id=$3 AND owner_module='accommodation'";

async fn transition_accommodation_occupancy(
    connection: &mut PgConnection,
    tenant: &str,
    resource_id: Uuid,
    id: Uuid,
    state: &str,
) -> Result<(), PurchaseError> {
    let result = sqlx::query(OCCUPANCY_TRANSITION)
        .bind(tenant)
        .bind(resource_id)
        .bind(id)
        .bind(state)
        .execute(&mut *connection)
        .await
        .map_err(refuses)?;
    if result.rows_affected() != 1 {
        return Err(conflict());
    }
    Ok(())
}

/// `setAccommodationResourceActive` — the shared resource's active flag
/// moves WITH the room's (`guard_room` refuses a divergence), and the
/// kernel's `guard_resource` refuses deactivating under live occupancy.
const RESOURCE_ACTIVE_MOVE: &str =
    "UPDATE archava_scheduling.resources SET active=$3,revision=revision+1,\
 updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2 AND managed_by='accommodation'";

async fn set_accommodation_resource_active(
    connection: &mut PgConnection,
    tenant: &str,
    resource_id: Uuid,
    active: bool,
) -> Result<(), PurchaseError> {
    let result = sqlx::query(RESOURCE_ACTIVE_MOVE)
        .bind(tenant)
        .bind(resource_id)
        .bind(active)
        .execute(&mut *connection)
        .await
        .map_err(refuses)?;
    if result.rows_affected() != 1 {
        return Err(conflict());
    }
    Ok(())
}

// --- The claim + replay plumbing ---------------------------------------------

async fn claim_accommodation_command(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    actor_id: &str,
    fingerprint: &str,
) -> Result<bool, PurchaseError> {
    let claimed = sqlx::query(
        "INSERT INTO archava_accommodation.commands(\
 tenant_id,request_id,actor_id,fingerprint) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(actor_id)
    .bind(fingerprint)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(claimed.rows_affected() == 1)
}

async fn fetch_command_row(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
) -> Result<Option<(String, String, Option<serde_json::Value>)>, PurchaseError> {
    sqlx::query_as(
        "SELECT actor_id,fingerprint,receipt FROM archava_accommodation.commands \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)
}

/// The accommodation audit row — `(tenant, request, actor, operation,
/// property_id, record_id, revision, reason?)`, one per command.
#[allow(clippy::too_many_arguments)]
async fn insert_accommodation_audit(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    actor_id: &str,
    operation: &str,
    property_id: Uuid,
    record_id: Uuid,
    revision: i64,
    reason: Option<&str>,
) -> Result<(), PurchaseError> {
    sqlx::query(
        "INSERT INTO archava_accommodation.audit(\
 tenant_id,request_id,actor_id,operation,property_id,record_id,revision,reason) \
 VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(actor_id)
    .bind(operation)
    .bind(property_id)
    .bind(record_id)
    .bind(int4(revision)?)
    .bind(reason)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(())
}

// --- The receipt access -------------------------------------------------------

/// `receiptAccess` — the replay/status re-authorization: the property
/// MUST still belong to the caller's access; room and room-type receipts
/// need the rooms read; stay receipts need BOTH stay and room reads plus
/// the stay row still inside the scope (zero rows = the TS `not_found`).
async fn receipt_access(
    connection: &mut PgConnection,
    tenant: &str,
    access: &AccommodationAccess,
    receipt: &AccommodationReceipt,
) -> Result<(), PurchaseError> {
    let property = parse_uuid_value(receipt_property_id(receipt))?;
    owned_property(connection, tenant, access, property).await?;
    match receipt {
        AccommodationReceipt::Room { .. } | AccommodationReceipt::RoomType { .. } => {
            if !access.holds(&["rooms.read"]) {
                return Err(forbidden());
            }
        }
        AccommodationReceipt::Property { .. } => {}
        AccommodationReceipt::Stay { .. } => {
            if !access.holds(&["stays.read"]) {
                return Err(forbidden());
            }
            if !access.holds(&["rooms.read"]) {
                return Err(forbidden());
            }
            let stay_id = parse_uuid_value(receipt_id(receipt))?;
            if stay_record(connection, tenant, access, stay_id)
                .await?
                .is_none()
            {
                return Err(not_found());
            }
        }
    }
    Ok(())
}

fn receipt_property_id(receipt: &AccommodationReceipt) -> &str {
    match receipt {
        AccommodationReceipt::Property { property_id, .. }
        | AccommodationReceipt::RoomType { property_id, .. }
        | AccommodationReceipt::Room { property_id, .. }
        | AccommodationReceipt::Stay { property_id, .. } => property_id,
    }
}

fn receipt_id(receipt: &AccommodationReceipt) -> &str {
    match receipt {
        AccommodationReceipt::Property { id, .. }
        | AccommodationReceipt::RoomType { id, .. }
        | AccommodationReceipt::Room { id, .. }
        | AccommodationReceipt::Stay { id, .. } => id,
    }
}

// --- The mutation pipeline ----------------------------------------------------

/// The mutation pipeline — the port of `mutate()`. The human-only kind
/// gate is the route-boundary rule; every refusal rolls the WHOLE
/// transaction back. The host hands over the ALREADY STRICT-PARSED
/// mutation (its boundary parse answers 400), and the fingerprint that
/// keyed the claim.
pub async fn record_accommodation_command(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &AccommodationAccess,
    fingerprint: &str,
    mutation: &AccommodationMutation,
) -> Result<AccommodationReceipt, PurchaseError> {
    if principal.kind.as_str() != "human" {
        return Err(forbidden());
    }
    let command = &mutation.command;
    // The permission ladder in the TS order: the operation's OWN write
    // grant (the transaction() call), then the read floors
    // (properties.read always; rooms.read beside every non-create arm;
    // stays.read beside the stay arms).
    if !access.holds(&[command.required_permission()]) {
        return Err(forbidden());
    }
    if !access.holds(&["properties.read"]) {
        return Err(forbidden());
    }
    if command.operation() != "property.create" && !access.holds(&["rooms.read"]) {
        return Err(forbidden());
    }
    if command.operation().starts_with("stay.") && !access.holds(&["stays.read"]) {
        return Err(forbidden());
    }
    let request_id = parse_uuid_value(&mutation.request_id)?;
    let actor_id = principal.id.as_str();

    let claimed =
        claim_accommodation_command(connection, tenant, request_id, actor_id, fingerprint).await?;
    if !claimed {
        let (row_actor, row_fingerprint, row_receipt) =
            fetch_command_row(connection, tenant, request_id)
                .await?
                .ok_or_else(conflict)?;
        if row_fingerprint != fingerprint || row_actor != actor_id {
            return Err(conflict());
        }
        let receipt_value = row_receipt.ok_or_else(conflict)?;
        let stored = AccommodationReceipt::parse(&receipt_value).map_err(|_| conflict())?;
        // Recovery re-authorization — `receiptAccess`.
        receipt_access(connection, tenant, access, &stored).await?;
        return Ok(stored);
    }

    let receipt = execute_accommodation_arm(connection, tenant, access, command).await?;
    insert_accommodation_audit(
        connection,
        tenant,
        request_id,
        actor_id,
        command.operation(),
        parse_uuid_value(receipt_property_id(&receipt))?,
        parse_uuid_value(receipt_id(&receipt))?,
        receipt_revision(&receipt),
        command.reason(),
    )
    .await?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|_| conflict())?;
    sqlx::query(
        "UPDATE archava_accommodation.commands SET receipt=$3 \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(&receipt_value)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(receipt)
}

fn receipt_revision(receipt: &AccommodationReceipt) -> i64 {
    match receipt {
        AccommodationReceipt::Property { revision, .. }
        | AccommodationReceipt::RoomType { revision, .. }
        | AccommodationReceipt::Room { revision, .. }
        | AccommodationReceipt::Stay { revision, .. } => *revision,
    }
}

// --- execute(): the eleven arms ------------------------------------------------

/// The eleven executed arms, in the TS branch order. The property.create
/// write IS its own row; every other arm locks the property first, and
/// the room/stay arms additionally serialize on the SHARED occupancy
/// resource (clock sampled after that lock). All statements are static
/// `.bind`ed forms — the TS parameterized queries one-for-one.
async fn execute_accommodation_arm(
    connection: &mut PgConnection,
    tenant: &str,
    access: &AccommodationAccess,
    command: &AccommodationCommand,
) -> Result<AccommodationReceipt, PurchaseError> {
    if let AccommodationCommand::PropertyCreate {
        title,
        timezone,
        check_in_time,
        check_out_time,
    } = command
    {
        // property.create — the unguarded create (the property's OWN row
        // is the effect; the audit + command rows are the bookkeeping).
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO archava_accommodation.properties(\
 tenant_id,id,owner_id,title,timezone,check_in_time,check_out_time) \
 VALUES($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(tenant)
        .bind(id)
        .bind(&access.owner_id)
        .bind(title)
        .bind(timezone)
        .bind(check_in_time)
        .bind(check_out_time)
        .execute(&mut *connection)
        .await
        .map_err(refuses)?;
        return Ok(AccommodationReceipt::Property {
            id: id.to_string(),
            property_id: id.to_string(),
            revision: 1,
        });
    }

    // Every non-create arm names its property (`propertyRef`). The typed
    // extraction makes the absent case a hard conflict, never an assumed
    // identifier.
    let property_ref = match command {
        AccommodationCommand::RoomTypeCreate { property_id, .. }
        | AccommodationCommand::RoomTypeSetRate { property_id, .. }
        | AccommodationCommand::RoomCreate { property_id, .. }
        | AccommodationCommand::RoomSetActive { property_id, .. }
        | AccommodationCommand::RoomHousekeeping { property_id, .. }
        | AccommodationCommand::StayHold { property_id, .. }
        | AccommodationCommand::StayConfirm { property_id, .. }
        | AccommodationCommand::StayCheckIn { property_id, .. }
        | AccommodationCommand::StayCheckOut { property_id, .. }
        | AccommodationCommand::StayCancel { property_id, .. } => property_id,
        AccommodationCommand::PropertyCreate { .. } => return Err(conflict()),
    };
    let property_id = parse_uuid_value(property_ref)?;
    let property = owned_property(connection, tenant, access, property_id).await?;

    match command {
        AccommodationCommand::RoomTypeCreate {
            title,
            max_guests,
            nightly_rate,
            ..
        } => {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO archava_accommodation.room_types(\
 tenant_id,id,property_id,title,max_guests,nightly_rate) VALUES($1,$2,$3,$4,$5,$6)",
            )
            .bind(tenant)
            .bind(id)
            .bind(property_id)
            .bind(title.trim())
            .bind(int4(*max_guests)?)
            .bind(nightly_rate)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok(AccommodationReceipt::RoomType {
                id: id.to_string(),
                property_id: property.id.clone(),
                revision: 1,
            })
        }
        AccommodationCommand::RoomTypeSetRate {
            id,
            expected_revision,
            nightly_rate,
            ..
        } => {
            let type_id = parse_uuid_value(id)?;
            let row = room_type_record(connection, tenant, property_id, type_id, true).await?;
            if row.revision != *expected_revision
                || row.revision >= ACCOMMODATION_REVISION_MAX
                || row.nightly_rate == *nightly_rate
            {
                return Err(conflict());
            }
            sqlx::query(
                "UPDATE archava_accommodation.room_types \
 SET nightly_rate=$3,revision=revision+1,updated_at=clock_timestamp() \
 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(type_id)
            .bind(int4(*nightly_rate)?)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok(AccommodationReceipt::RoomType {
                id: row.id.clone(),
                property_id: property.id.clone(),
                revision: row.revision + 1,
            })
        }
        AccommodationCommand::RoomCreate {
            room_type_id: room_type_ref,
            code,
            ..
        } => {
            let room_type_uuid = parse_uuid_value(room_type_ref)?;
            // The FOR SHARE read is the existence check AND the lock — the
            // record itself rides nowhere, exactly as the TS arm binds
            // only the command's identifiers into the INSERT.
            let _room_type =
                room_type_record(connection, tenant, property_id, room_type_uuid, false).await?;
            let id = Uuid::new_v4();
            // The room BECOMES a shared, accommodation-managed resource —
            // the RPC the 002 migration froze, with the FIXED internal
            // service identity its guard pins.
            create_accommodation_resource(connection, tenant, id, code, &property.timezone).await?;
            sqlx::query(
                "INSERT INTO archava_accommodation.rooms(\
 tenant_id,id,property_id,room_type_id,code) VALUES($1,$2,$3,$4,$5)",
            )
            .bind(tenant)
            .bind(id)
            .bind(property_id)
            .bind(room_type_uuid)
            .bind(code)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok(AccommodationReceipt::Room {
                id: id.to_string(),
                property_id: property.id.clone(),
                revision: 1,
            })
        }
        AccommodationCommand::RoomSetActive { .. }
        | AccommodationCommand::RoomHousekeeping { .. }
        | AccommodationCommand::StayHold { .. } => {
            // The shared-resource arms: own-room existence, then the
            // occupancy resource LOCK, then the clock, then the fresh
            // row. Two room reads (the TS `roomRecord` twice) bracket the
            // lock exactly.
            let room_ref = match command {
                AccommodationCommand::RoomSetActive { room_id, .. }
                | AccommodationCommand::RoomHousekeeping { room_id, .. }
                | AccommodationCommand::StayHold { room_id, .. } => room_id,
                _ => return Err(conflict()),
            };
            let room_uuid = parse_uuid_value(room_ref)?;
            let _locating = room_record(connection, tenant, property_id, room_uuid).await?;
            let shared = lock_accommodation_resource(connection, tenant, room_uuid).await?;
            let observed_raw = observed_now(connection).await?;
            let observed = instant_millis(&observed_raw)?;
            let room = room_record(connection, tenant, property_id, room_uuid).await?;

            match command {
                AccommodationCommand::RoomSetActive {
                    expected_revision,
                    active,
                    ..
                } => {
                    if room.revision != *expected_revision
                        || room.revision >= ACCOMMODATION_REVISION_MAX
                    {
                        return Err(conflict());
                    }
                    if room.active == *active || room.occupied {
                        return Err(conflict());
                    }
                    set_accommodation_resource_active(connection, tenant, shared.id, *active)
                        .await?;
                    sqlx::query(
                        "UPDATE archava_accommodation.rooms \
 SET active=$3,revision=revision+1,updated_at=clock_timestamp() \
 WHERE tenant_id=$1 AND id=$2",
                    )
                    .bind(tenant)
                    .bind(parse_uuid_value(&room.id)?)
                    .bind(active)
                    .execute(&mut *connection)
                    .await
                    .map_err(refuses)?;
                    Ok(AccommodationReceipt::Room {
                        id: room.id.clone(),
                        property_id: property.id.clone(),
                        revision: room.revision + 1,
                    })
                }
                AccommodationCommand::RoomHousekeeping {
                    expected_revision,
                    state,
                    ..
                } => {
                    if room.revision != *expected_revision
                        || room.revision >= ACCOMMODATION_REVISION_MAX
                    {
                        return Err(conflict());
                    }
                    if room.occupied
                        || !archava_domain::erp::accommodation::housekeeping_transition_allowed(
                            room.housekeeping,
                            *state,
                        )
                    {
                        return Err(conflict());
                    }
                    sqlx::query(
                        "UPDATE archava_accommodation.rooms \
 SET housekeeping=$3,revision=revision+1,updated_at=clock_timestamp() \
 WHERE tenant_id=$1 AND id=$2",
                    )
                    .bind(tenant)
                    .bind(parse_uuid_value(&room.id)?)
                    .bind(state.as_str())
                    .execute(&mut *connection)
                    .await
                    .map_err(refuses)?;
                    Ok(AccommodationReceipt::Room {
                        id: room.id.clone(),
                        property_id: property.id.clone(),
                        revision: room.revision + 1,
                    })
                }
                AccommodationCommand::StayHold {
                    arrival,
                    departure,
                    guest_name,
                    guests,
                    hold_seconds,
                    ..
                } => {
                    let dates = stay_period(connection, &property, arrival, departure).await?;
                    let today = calendar_today(connection, observed, &property.timezone).await?;
                    let arrival_date = parse_calendar_date(arrival).ok_or_else(conflict)?;
                    let stay_id = Uuid::new_v4();
                    let room_type = room_type_record(
                        connection,
                        tenant,
                        property_id,
                        parse_uuid_value(&room.room_type_id)?,
                        false,
                    )
                    .await?;
                    // The TS hold gate — resource liveness, capacity, the
                    // stay window inside the property's own future, and
                    // same-arrival physical occupancy.
                    if !room.active
                        || !shared.active
                        || *guests > room_type.max_guests
                        || arrival_date.epoch_days < today.epoch_days
                        || instant_millis(&dates.ends_at)? <= observed
                        || (arrival_date.epoch_days == today.epoch_days && room.occupied)
                    {
                        return Err(conflict());
                    }
                    let expires_at = archava_domain::erp::scheduling::instant_render(
                        observed + hold_seconds * 1_000,
                    );
                    hold_accommodation_occupancy(
                        connection,
                        tenant,
                        stay_id,
                        shared.id,
                        &dates.starts_at,
                        &dates.ends_at,
                        &expires_at,
                    )
                    .await?;
                    sqlx::query(
                        "INSERT INTO archava_accommodation.stays(\
 tenant_id,id,property_id,room_id,owner_id,guest_name,guests,arrival,departure,\
 nights,nightly_rate,rate_revision,total_estimate,starts_at,ends_at,expires_at) \
 VALUES($1,$2,$3,$4,$5,$6,$7,$8::date,$9::date,$10,$11,$12,$13,$14::timestamptz,$15::timestamptz,$16::timestamptz)",
                    )
                    .bind(tenant)
                    .bind(stay_id)
                    .bind(property_id)
                    .bind(parse_uuid_value(&room.id)?)
                    .bind(&access.owner_id)
                    .bind(guest_name)
                    .bind(int4(*guests)?)
                    .bind(arrival)
                    .bind(departure)
                    .bind(int4(dates.nights.len() as i64)?)
                    .bind(int4(room_type.nightly_rate)?)
                    .bind(int4(room_type.revision)?)
                    .bind(room_type.nightly_rate * dates.nights.len() as i64)
                    .bind(instant_bind(&dates.starts_at)?)
                    .bind(instant_bind(&dates.ends_at)?)
                    .bind(instant_bind(&expires_at)?)
                    .execute(&mut *connection)
                    .await
                    .map_err(refuses)?;
                    // One night-quote row per night, each through the
                    // `night_guard` trigger's match to the stay header.
                    for night in &dates.nights {
                        sqlx::query(
                            "INSERT INTO archava_accommodation.stay_nights(\
 tenant_id,stay_id,night_date,rate,rate_revision) \
 VALUES($1,$2,$3::date,$4,$5)",
                        )
                        .bind(tenant)
                        .bind(stay_id)
                        .bind(night)
                        .bind(int4(room_type.nightly_rate)?)
                        .bind(int4(room_type.revision)?)
                        .execute(&mut *connection)
                        .await
                        .map_err(refuses)?;
                    }
                    Ok(AccommodationReceipt::Stay {
                        id: stay_id.to_string(),
                        property_id: property.id.clone(),
                        revision: 1,
                    })
                }
                _ => Err(conflict()),
            }
        }
        AccommodationCommand::StayConfirm { .. }
        | AccommodationCommand::StayCancel { .. }
        | AccommodationCommand::StayCheckIn { .. }
        | AccommodationCommand::StayCheckOut { .. } => {
            // The stay lifecycle arms: locate through the scope screen,
            // then the resource lock, then the clock, then the REFRESH
            // read (the TS `stayRecord` runs twice around the lock).
            let (stay_ref_raw, expected_revision) = match command {
                AccommodationCommand::StayConfirm {
                    id,
                    expected_revision,
                    ..
                }
                | AccommodationCommand::StayCancel {
                    id,
                    expected_revision,
                    ..
                }
                | AccommodationCommand::StayCheckIn {
                    id,
                    expected_revision,
                    ..
                }
                | AccommodationCommand::StayCheckOut {
                    id,
                    expected_revision,
                    ..
                } => (id, *expected_revision),
                _ => return Err(conflict()),
            };
            let stay_ref = parse_uuid_value(stay_ref_raw)?;
            let located = stay_record(connection, tenant, access, stay_ref)
                .await?
                .ok_or_else(not_found)?;
            use sqlx::Row as _;
            let located_room: Uuid = located.try_get("room_id").map_err(refuses)?;
            let located_property: Uuid = located.try_get("property_id").map_err(refuses)?;
            if located_property != property_id {
                return Err(not_found());
            }
            let shared = lock_accommodation_resource(connection, tenant, located_room).await?;
            let observed_raw = observed_now(connection).await?;
            let observed = instant_millis(&observed_raw)?;
            let refreshed = stay_record(connection, tenant, access, stay_ref)
                .await?
                .ok_or_else(not_found)?;
            let item = stay_row(&refreshed, tenant, observed)?;

            if item.revision != expected_revision
                || item.revision >= ACCOMMODATION_REVISION_MAX
                || matches!(
                    item.effective_state,
                    StayEffectiveState::Expired
                        | StayEffectiveState::CheckedOut
                        | StayEffectiveState::Cancelled
                )
            {
                return Err(conflict());
            }
            match command {
                AccommodationCommand::StayConfirm { .. } => {
                    if item.effective_state != StayEffectiveState::Held
                        || instant_millis(&item.ends_at)? <= observed
                    {
                        return Err(conflict());
                    }
                    transition_accommodation_occupancy(
                        connection,
                        tenant,
                        shared.id,
                        stay_ref,
                        "confirmed",
                    )
                    .await?;
                    sqlx::query(
                        "UPDATE archava_accommodation.stays \
 SET state='confirmed',revision=revision+1,updated_at=clock_timestamp() \
 WHERE tenant_id=$1 AND id=$2",
                    )
                    .bind(tenant)
                    .bind(stay_ref)
                    .execute(&mut *connection)
                    .await
                    .map_err(refuses)?;
                    Ok(())
                }
                AccommodationCommand::StayCancel { .. } => {
                    if !matches!(item.state, StayState::Held | StayState::Confirmed) {
                        return Err(conflict());
                    }
                    transition_accommodation_occupancy(
                        connection,
                        tenant,
                        shared.id,
                        stay_ref,
                        "cancelled",
                    )
                    .await?;
                    sqlx::query(
                        "UPDATE archava_accommodation.stays \
 SET state='cancelled',revision=revision+1,updated_at=clock_timestamp() \
 WHERE tenant_id=$1 AND id=$2",
                    )
                    .bind(tenant)
                    .bind(stay_ref)
                    .execute(&mut *connection)
                    .await
                    .map_err(refuses)?;
                    Ok(())
                }
                AccommodationCommand::StayCheckIn { .. } => {
                    let room = room_record(connection, tenant, property_id, located_room).await?;
                    if item.state != StayState::Confirmed
                        || !room.active
                        || room.occupied
                        || room.housekeeping != Housekeeping::Inspected
                        || instant_millis(&item.starts_at)? > observed
                        || instant_millis(&item.ends_at)? <= observed
                    {
                        return Err(conflict());
                    }
                    sqlx::query(
                        "UPDATE archava_accommodation.stays \
 SET state='in_house',checked_in_at=clock_timestamp(),revision=revision+1,\
 updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2",
                    )
                    .bind(tenant)
                    .bind(stay_ref)
                    .execute(&mut *connection)
                    .await
                    .map_err(refuses)?;
                    Ok(())
                }
                AccommodationCommand::StayCheckOut { .. } => {
                    if item.state != StayState::InHouse {
                        return Err(conflict());
                    }
                    transition_accommodation_occupancy(
                        connection,
                        tenant,
                        shared.id,
                        stay_ref,
                        "cancelled",
                    )
                    .await?;
                    sqlx::query(
                        "UPDATE archava_accommodation.stays \
 SET state='checked_out',checked_out_at=clock_timestamp(),revision=revision+1,\
 updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2",
                    )
                    .bind(tenant)
                    .bind(stay_ref)
                    .execute(&mut *connection)
                    .await
                    .map_err(refuses)?;
                    Ok(())
                }
                _ => Err(conflict()),
            }
            .map(|()| AccommodationReceipt::Stay {
                id: stay_ref.to_string(),
                property_id: property.id.clone(),
                revision: item.revision + 1,
            })
        }
        AccommodationCommand::PropertyCreate { .. } => Err(conflict()),
    }
}

// --- commandStatus -------------------------------------------------------------

/// `commandStatus()` — the discovered receipt replays only while the
/// CURRENT access still authorizes the recorded operation: the read
/// refuses a caller whose actor id is not the recorded one (the TS
/// `row.actor_id !== principal.id` gate; the human-only gate rides the
/// same boundary role), and `receiptAccess` re-walks the recorded
/// property (and stay) against today's envelope.
pub async fn accommodation_command_status(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &AccommodationAccess,
    request_id: Uuid,
) -> Result<AccommodationCommandStatus, PurchaseError> {
    if principal.kind.as_str() != "human" {
        return Err(forbidden());
    }
    // The status transaction's own grant ('properties.read').
    if !access.holds(&["properties.read"]) {
        return Err(forbidden());
    }
    let Some((row_actor, _, row_receipt)) =
        fetch_command_row(connection, tenant, request_id).await?
    else {
        return Ok(AccommodationCommandStatus::NotCommitted {
            request_id: request_id.to_string(),
        });
    };
    if row_actor != principal.id {
        return Err(forbidden());
    }
    let receipt_value = row_receipt.ok_or_else(conflict)?;
    let stored = AccommodationReceipt::parse(&receipt_value).map_err(|_| conflict())?;
    receipt_access(connection, tenant, access, &stored).await?;
    Ok(AccommodationCommandStatus::Committed {
        request_id: request_id.to_string(),
        receipt: stored,
    })
}

// --- The startup support check ---------------------------------------------------

/// The port of `PostgresAccommodationRepository.create()`'s checks plus
/// `assertOccupancyKernel`: the runtime role must be a NON-owner (no
/// super, no bypassrls, no schema ownership of either ledger), BOTH
/// migration ledgers must match the frozen pair lists, and BOTH ledgers'
/// tenant policies must be forced (8 accommodation tables + the kernel's
/// 6). Startup support, not an authorization token — the posting ladder
/// belongs to the request path.
///
/// The TS derives the expected pair lists from its OWN module's file
/// bytes (`migrate.ts` recomputes at import); this crate consumes the
/// frozen pair list its fixture recorded — the same derivation, the
/// caller supplies it (recorded divergence, ledger-internal).
///
/// Errors are `&'static str` refusals — the TS generic `Error`s. These
/// paths never answer callers; the serving state refuses to come up.
pub async fn assert_accommodation_kernel(
    pool: &sqlx::PgPool,
    scheduling_ledger: &[(String, String)],
    accommodation_ledger: &[(String, String)],
) -> Result<(), String> {
    let role: Option<(bool,)> = sqlx::query_as(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class \
 WHERE relnamespace='archava_accommodation'::regnamespace AND relowner=pg_roles.oid)) AS unsafe \
 FROM pg_roles WHERE rolname=current_user",
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("accommodation runtime role check failed: {error}"))?;
    if role.map(|(unsafe_,)| unsafe_).unwrap_or(true) {
        return Err("Accommodation requires a non-owner runtime".to_string());
    }
    for (namespace, expected) in [
        ("archava_scheduling", scheduling_ledger),
        ("archava_accommodation", accommodation_ledger),
    ] {
        let statement =
            format!("SELECT id,checksum FROM {namespace}.schema_migrations ORDER BY id");
        let actual: Vec<(String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(statement.as_str()))
            .fetch_all(pool)
            .await
            .map_err(|error| format!("unsupported {namespace} ledger read: {error}"))?;
        if actual.len() != expected.len() || actual.iter().zip(expected).any(|(a, b)| a != b) {
            return Err("Unsupported accommodation migration checksum".to_string());
        }
    }
    for (namespace, names) in [
        (
            "archava_accommodation",
            &[
                "memberships",
                "properties",
                "room_types",
                "rooms",
                "stays",
                "stay_nights",
                "commands",
                "audit",
            ][..],
        ),
        (
            "archava_scheduling",
            &[
                "memberships",
                "resources",
                "reservations",
                "commands",
                "audit",
                "occupancy",
            ][..],
        ),
    ] {
        let statement = format!(
            "SELECT count(*)::integer FROM pg_class WHERE \
 relnamespace='{namespace}'::regnamespace AND relname=ANY($1::text[]) \
 AND relrowsecurity AND relforcerowsecurity"
        );
        let (count,): (i32,) = sqlx::query_as(sqlx::AssertSqlSafe(statement.as_str()))
            .bind(names)
            .fetch_one(pool)
            .await
            .map_err(|error| format!("accommodation policy check failed: {error}"))?;
        if count as usize != names.len() {
            return Err("Accommodation tenant policies must be forced".to_string());
        }
    }
    Ok(())
}
