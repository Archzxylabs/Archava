//! The Postgres provider attempt store — the port of
//! `packages/provider-runtime/src/postgres.ts`.
//!
//! ## The ledger's invariants, transcribed in the baseline's order
//!
//! 1. **One bucket row per budget, policy-exact.** [`seed`] inserts
//!    `ON CONFLICT DO NOTHING` and then re-reads the row `FOR UPDATE`,
//!    refusing a policy mismatch (`postgres.ts` `seed`). The SQL CHECKs
//!    bound the same limits the contract validator bounds — a second parser
//!    behind the first, so a policy the Rust validator accepts but the SQL
//!    refuses fails closed at INSERT time.
//! 2. **Attempt transitions are append-only after the first finalize.**
//!    `001_provider_attempts.sql`'s `preserve_attempt` trigger refuses any
//!    UPDATE that moves a finished attempt (or re-pends it) — a later
//!    `finish` cannot rewrite history, and the `unknown` reap of an expired
//!    lease is itself durable history.
//! 3. **Row-level security is forced on both tables**, scoped by
//!    `archava.provider_bucket` set `local` per transaction — the same
//!    non-owner cohort the domain ledgers use, but with the BUDGET as the
//!    scope: a caller working bucket A cannot touch bucket B's counters,
//!    even pooled from the same connection set.
//! 4. **The clock is the server's.** Every comparison, margin, rollover and
//!    elapsed computation runs inside PostgreSQL against ONE instant per
//!    operation — `clock_timestamp()` fetched once as text and passed back
//!    through an explicit `$now::timestamptz` cast, exactly the role of
//!    `postgres.ts`'s `time()` (`clock_timestamp()` fetched once, reused).
//!    The text round trip is timezone-consistent because the same session
//!    formats and parses it, and every DAY decision anchors to
//!    `AT TIME ZONE 'UTC'` explicitly.
//!
//! ## Non-owner cohort
//!
//! [`PostgresProviderAttemptStore::create`] refuses superusers, BYPASSRLS
//! roles and namespace owners
//! (`SELECT (rolsuper OR rolbypassrls OR EXISTS ... relowner ...)`) and
//! requires the forced-RLS relation count to be exactly 2 — the same checks
//! TS's `create` runs. Without them the grants below are decoration.

use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnection, PgRow};
use sqlx::types::Uuid;
use sqlx::PgPool;
use sqlx::Row;

use crate::contracts::{
    validate_budget_id, Admission, AdmissionRefusal, Completion, ProviderPolicy,
};
use crate::runtime::{ProviderAttemptStore, StoreError};

/// The namespace the provider ledger lives in.
const NAMESPACE: &str = "archava_provider";
/// The migration's ledger id, as recorded in `schema_migrations`.
const MIGRATION_ID: &str = "001_provider_attempts";
/// The migration's advisory lock label — `postgres.ts` locks
/// `hashtext('archava_provider_migrations')`.
const MIGRATION_LOCK: &str = "archava_provider_migrations";

/// TS `toISOString()`'s exact shape for timestamps: `YYYY-MM-DDTHH:MM:SS.mmmZ`
/// over the UTC wall time.
const ISO_FORMAT: &str = "YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"";

/// Where the migration SQL lives — the same file the TS store reads, so the
/// checksum the ledger records is the checksum of the one source of truth.
fn migration_sql() -> Result<Vec<u8>, SetupError> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../packages/provider-runtime/migrations/001_provider_attempts.sql");
    std::fs::read(&path).map_err(|error| SetupError::MigrationBytes {
        path: path.display().to_string(),
        error: error.to_string(),
    })
}

/// The sha256 the ledger records — the same digest TS's
/// `createHash('sha256')` produces over the same bytes (`postgres.ts`
/// `migration()`). The adapters' readiness checker uses the same digest
/// function shape, but the provider ledger is graded on its own pair.
fn migration_checksum(sql: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(sql);
    digest
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use core::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// A validated runtime role name for grants (`grantProviderRuntime`'s
/// `/^[a-z][a-z0-9_]{0,62}$/`).
fn validate_grant_role(role: &str) -> Result<(), SetupError> {
    let bytes = role.as_bytes();
    let head = bytes.first().copied();
    let ok = head.is_some_and(|b| b.is_ascii_lowercase())
        && bytes.len() <= 63
        && bytes[1..]
            .iter()
            .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(SetupError::InvalidRole)
    }
}

/// Setup-time refusals — every one means the store will not open, and none
/// leak values (the SQL state each error names IS the finding).
#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    /// The connecting role is an owner, superuser or BYPASSRLS role, or the
    /// forced-RLS count is not exactly the two tables.
    #[error("Provider runtime requires a non-owner role")]
    NonOwnerRole,
    /// The recorded schema-migrations ledger is not exactly the store's
    /// frozen pair.
    #[error("Unsupported provider schema")]
    UnsupportedSchema,
    /// The scope policies exist but are not forced; the non-owner cohort is
    /// decoration without FORCE.
    #[error("Provider scope policies must be forced")]
    ScopePoliciesNotForced,
    /// The runtime role name refused for interpolation into a GRANT.
    #[error("Invalid provider runtime role")]
    InvalidRole,
    /// Migration bytes unreadable at their owned path.
    #[error("Provider migration unreadable at {path}: {error}")]
    MigrationBytes { path: String, error: String },
    /// A store-call failure while migrating or granting — same opaque
    /// boundary.
    #[error("Provider attempt store could not answer durably")]
    StoreFailed,
}

/// Map any store-call SQL failure to the opaque [`StoreError`]: a caller
/// cannot read the failure's SQL state, only the fact that the ledger could
/// not answer durably — TS establishes the same boundary by only ever
/// catching the exception into `outcome_unknown`.
fn ledger_error(_error: sqlx::Error) -> StoreError {
    StoreError
}

/// The same mapping for the SETUP paths, whose result type is
/// [`SetupError`]: the SQL detail is still not the caller's to read.
fn setup_ledger_error(_error: sqlx::Error) -> SetupError {
    SetupError::StoreFailed
}

/// Open one transaction with the bucket scope set `local`.
///
/// TS: `transaction(pool, bucketId, run)` sets
/// `set_config('archava.provider_bucket', parsed, true)` before the run —
/// the `true` third argument makes the scope disappear at COMMIT, exactly
/// like the domain ledgers' tenant setting. The caller owns the whole body
/// (the reserve ladder, the finalize, the status read) and commits exactly
/// on its success; any error leaves the transaction to drop — rollback —
/// which is its own durability boundary, matching the TS wrapper's
/// rollback-on-throw.
async fn open_bucket(
    pool: &PgPool,
    bucket_id: &str,
) -> Result<sqlx::Transaction<'static, sqlx::Postgres>, StoreError> {
    let mut transaction = pool.begin().await.map_err(ledger_error)?;
    sqlx::query("SELECT set_config('archava.provider_bucket',$1,true)")
        .bind(bucket_id)
        .execute(&mut *transaction)
        .await
        .map_err(ledger_error)?;
    Ok(transaction)
}

/// Insert the bucket if absent and re-read it locked, refusing a policy
/// mismatch.
///
/// TS: `seed` (`postgres.ts` lines 111–145) — same INSERT shape, same
/// `FOR UPDATE`, same refusal condition. A mismatch is a store ERROR, not an
/// admission refusal: the caller's policy does not describe this ledger's
/// budget, and the orchestration converts any reserve error to
/// `outcome_unknown` (fail closed).
async fn seed(connection: &mut PgConnection, policy: &ProviderPolicy) -> Result<(), StoreError> {
    // TS inserts `usage_day = (clock_timestamp() AT TIME ZONE 'UTC')::date` —
    // the UTC day of the server's own clock, never the session timezone.
    sqlx::query(
        "INSERT INTO archava_provider.buckets
    (bucket_id,provider_id,model,daily_limit,minimum_interval_ms,lease_ms,usage_day)
    VALUES($1,$2,$3,$4,$5,$6,(clock_timestamp() AT TIME ZONE 'UTC')::date) ON CONFLICT DO NOTHING",
    )
    .bind(policy.bucket_id())
    .bind(policy.provider_id())
    .bind(policy.model())
    .bind(policy.daily_limit())
    .bind(policy.minimum_interval_ms())
    .bind(policy.lease_ms())
    .execute(&mut *connection)
    .await
    .map_err(ledger_error)?;
    let row = sqlx::query(
        "SELECT provider_id::text, model::text, daily_limit, minimum_interval_ms, lease_ms
    FROM archava_provider.buckets WHERE bucket_id=$1::text FOR UPDATE",
    )
    .bind(policy.bucket_id())
    .fetch_one(&mut *connection)
    .await
    .map_err(ledger_error)?;
    let matches_policy = row.try_get::<String, _>(0).map_err(|_| StoreError)?
        == policy.provider_id()
        && row.try_get::<String, _>(1).map_err(|_| StoreError)? == policy.model()
        && row.try_get::<i32, _>(2).map_err(|_| StoreError)? == policy.daily_limit()
        && row.try_get::<i32, _>(3).map_err(|_| StoreError)? == policy.minimum_interval_ms()
        && row.try_get::<i32, _>(4).map_err(|_| StoreError)? == policy.lease_ms();
    if !matches_policy {
        return Err(StoreError);
    }
    Ok(())
}

/// Reap an expired lease: the pending attempt becomes
/// `unknown`/`interrupted` with the server-computed elapsed time, and the
/// bucket's lease columns clear.
///
/// TS: `reap` (`postgres.ts` lines 147–166) — gated on
/// `lease_until > now` RETURNING early; here the same guard is each
/// statement's WHERE, so a live lease reaps nothing in both languages. The
/// reap is also the *only* writer that may move a pending attempt to
/// `unknown` outside a `finish` — and the trigger keeps every write
/// append-only.
async fn reap(
    connection: &mut PgConnection,
    bucket_id: &str,
    now_text: &str,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE archava_provider.attempts a
        SET state='unknown',failure_code='interrupted',
        finished_at=$2::timestamptz,
        elapsed_ms=LEAST(2147483647,GREATEST(0,EXTRACT(EPOCH FROM ($2::timestamptz-started_at))*1000)::bigint)::integer
        WHERE a.bucket_id=$1::text AND a.state='pending'
        AND a.attempt_id=(SELECT b.active_attempt FROM archava_provider.buckets b WHERE b.bucket_id=$1::text AND b.active_attempt IS NOT NULL AND b.lease_until <= $2::timestamptz)",
    )
    .bind(bucket_id)
    .bind(now_text)
    .execute(&mut *connection)
    .await
    .map_err(ledger_error)?;
    sqlx::query(
        "UPDATE archava_provider.buckets SET active_attempt=NULL,lease_until=NULL
        WHERE bucket_id=$1::text AND active_attempt IS NOT NULL AND lease_until <= $2::timestamptz",
    )
    .bind(bucket_id)
    .bind(now_text)
    .execute(&mut *connection)
    .await
    .map_err(ledger_error)?;
    Ok(())
}

/// The Postgres provider attempt store. Open through
/// [`PostgresProviderAttemptStore::create`]; the runtime role must already
/// hold the provider grants (`grant_provider_runtime`, or the fixture's
/// provision, which applies the same statements).
#[derive(Debug, Clone)]
pub struct PostgresProviderAttemptStore {
    pool: PgPool,
}

impl PostgresProviderAttemptStore {
    /// TS: `static create(pool)`'s three checks — role safety, the frozen
    /// ledger pair, and the forced-RLS count, in that order. Every failure
    /// fails closed.
    pub async fn create(pool: &PgPool) -> Result<Self, SetupError> {
        // 1. The connecting role may not be an owner of the namespace, nor
        //    bypass RLS. TS verbatim:
        //    `SELECT (rolsuper OR rolbypassrls OR EXISTS (SELECT 1 FROM
        //     pg_class WHERE relnamespace='archava_provider'::regnamespace
        //     AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE
        //     rolname=current_user`
        let rows: Vec<bool> = sqlx::query(
            "SELECT (rolsuper OR rolbypassrls OR EXISTS (SELECT 1 FROM pg_class WHERE relnamespace='archava_provider'::regnamespace AND relowner=pg_roles.oid)) AS unsafe
             FROM pg_roles WHERE rolname=current_user",
        )
        .fetch_all(pool)
        .await
        .map_err(setup_ledger_error)?
        .into_iter()
        .map(|row| row.try_get::<bool, _>(0).unwrap_or(true))
        .collect();
        if rows.as_slice() != [false] {
            return Err(SetupError::NonOwnerRole);
        }
        // 2. The recorded ledger must be exactly the frozen pair.
        let sql_bytes = migration_sql()?;
        let expected = (MIGRATION_ID.to_string(), migration_checksum(&sql_bytes));
        let rows: Vec<(String, String)> =
            sqlx::query("SELECT id::text, checksum::text FROM archava_provider.schema_migrations")
                .fetch_all(pool)
                .await
                .map_err(setup_ledger_error)?
                .into_iter()
                .map(|row| (row.get::<String, _>(0), row.get::<String, _>(1)))
                .collect();
        if rows.as_slice() != [expected] {
            return Err(SetupError::UnsupportedSchema);
        }
        // 3. Both tables must FORCE row-level security, not merely enable
        //    it — without FORCE the owner silence-holes the policies.
        let forced = sqlx::query_scalar::<_, i64>(
            "SELECT count(*)::bigint AS n FROM pg_class WHERE relnamespace='archava_provider'::regnamespace AND relrowsecurity AND relforcerowsecurity",
        )
        .fetch_one(pool)
        .await
        .map_err(setup_ledger_error)?;
        if forced != 2 {
            return Err(SetupError::ScopePoliciesNotForced);
        }
        Ok(Self { pool: pool.clone() })
    }

    /// One instant per operation, from the server's own clock.
    ///
    /// TS: `time()` fetches `clock_timestamp()` ONCE and every comparison in
    /// the transaction uses that instant. The text round trip keeps the
    /// single-instant contract client-side-timezone-free: the same session
    /// formats and parses it, and every DAY decision anchors to
    /// `AT TIME ZONE 'UTC'` explicitly.
    async fn now_instant(connection: &mut PgConnection) -> Result<String, StoreError> {
        sqlx::query_scalar("SELECT clock_timestamp()::text")
            .fetch_one(&mut *connection)
            .await
            .map_err(ledger_error)
    }

    /// Open the bucket transaction, run the ladder, commit exactly at the
    /// success path — refusals commit (their reap/rollover effects are real
    /// durability), store errors drop the transaction, which rolls back.
    async fn reserve_within(
        &self,
        bucket_id: &str,
        policy: &ProviderPolicy,
        attempt_id: &Uuid,
    ) -> Result<Admission, StoreError> {
        let mut transaction = open_bucket(&self.pool, bucket_id).await?;
        let admission = reserve_ladder(&mut transaction, policy, bucket_id, attempt_id).await?;
        transaction.commit().await.map_err(ledger_error)?;
        Ok(admission)
    }

    /// Finalize the attempt.
    ///
    /// TS `finish` (`postgres.ts` lines 320–362): lock the bucket, reap the
    /// expired lease, refuse when the attempt is not this bucket's active
    /// lease, update the pending row exactly once, clear the lease. `false`
    /// — not an error — covers every unknown outcome: a missing bucket, an
    /// expired lease, an already-finished attempt.
    async fn finish_within(
        &self,
        bucket_id: &str,
        attempt_id: &Uuid,
        completion: Completion,
    ) -> Result<bool, StoreError> {
        let mut transaction = open_bucket(&self.pool, bucket_id).await?;
        let result = finish_body(&mut transaction, bucket_id, attempt_id, completion).await?;
        transaction.commit().await.map_err(ledger_error)?;
        Ok(result)
    }
}

/// The reserve ladder, `postgres.ts` `reserve`, statement for statement:
/// duplicate attempt first, then the restore hold, then the live lease,
/// then the usage-day rollover, then the daily limit, then the cooldown,
/// and only then the attempt insert and the lease columns. Named as its own
/// function so the transaction lifecycle (open in [`open_bucket`], commit on
/// success, drop-on-error rollback) stays the caller's — the TS wrapper
/// committed the closure's success return, and these are the same commit
/// points.
async fn reserve_ladder(
    transaction: &mut PgConnection,
    policy: &ProviderPolicy,
    bucket_id: &str,
    attempt_id: &Uuid,
) -> Result<Admission, StoreError> {
    let now_text = PostgresProviderAttemptStore::now_instant(&mut *transaction).await?;
    seed(&mut *transaction, policy).await?;
    reap(&mut *transaction, bucket_id, &now_text).await?;
    // --- duplicate_attempt: an attempt id is a replay shield. TS: the
    // attempt row existing at all refuses with `retryAfterMs: 0`.
    let duplicated = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM archava_provider.attempts WHERE bucket_id=$1::text AND attempt_id=$2) AS dup",
    )
    .bind(bucket_id)
    .bind(attempt_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(ledger_error)?;
    if duplicated {
        return Ok(Admission::Refused {
            code: AdmissionRefusal::DuplicateAttempt,
            retry_after_ms: 0,
        });
    }
    // --- restore_hold: the operator hold freezes the day. TS computes
    // `retryAfterMs` from the same instant it compares against.
    let row = sqlx::query(
        "SELECT (hold_until IS NOT NULL AND hold_until > $2::timestamptz) AS held,
        COALESCE(FLOOR(EXTRACT(EPOCH FROM (hold_until - $2::timestamptz))*1000)::bigint, 0) AS hold_margin
        FROM archava_provider.buckets WHERE bucket_id=$1::text",
    )
    .bind(bucket_id)
    .bind(&now_text)
    .fetch_one(&mut *transaction)
    .await
    .map_err(ledger_error)?;
    if row.try_get::<bool, _>(0).map_err(|_| StoreError)? {
        return Ok(Admission::Refused {
            code: AdmissionRefusal::RestoreHold,
            retry_after_ms: row.try_get::<i64, _>(1).map_err(|_| StoreError)?,
        });
    }
    // --- busy: a live (post-reap) lease is exactly one active attempt; its
    // margin is the lease's remaining time.
    let lease_margin: Option<i64> = sqlx::query_scalar(
        "SELECT CASE WHEN active_attempt IS NOT NULL THEN GREATEST(0, FLOOR(EXTRACT(EPOCH FROM (lease_until - $2::timestamptz))*1000)::bigint) ELSE NULL END
        FROM archava_provider.buckets WHERE bucket_id=$1::text",
    )
    .bind(bucket_id)
    .bind(&now_text)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(ledger_error)?
    .flatten();
    if let Some(retry_after_ms) = lease_margin {
        return Ok(Admission::Refused {
            code: AdmissionRefusal::Busy,
            retry_after_ms,
        });
    }
    // --- rollover: the usage day is a UTC date COMPARED INSIDE PostgreSQL
    // (TS records the reason: `pg's date parser follows the process
    // timezone`). A new day resets the counters and clears a stale hold.
    // None of the checks below run before this, matching the TS order.
    let usage_day_matches = sqlx::query_scalar::<_, bool>(
        "SELECT usage_day=(($2::timestamptz AT TIME ZONE 'UTC')::date) AS same FROM archava_provider.buckets WHERE bucket_id=$1::text",
    )
    .bind(bucket_id)
    .bind(&now_text)
    .fetch_one(&mut *transaction)
    .await
    .map_err(ledger_error)?;
    if !usage_day_matches {
        sqlx::query(
            "UPDATE archava_provider.buckets
            SET usage_day=(($2::timestamptz AT TIME ZONE 'UTC')::date), used=0, hold_until=NULL
            WHERE bucket_id=$1::text",
        )
        .bind(bucket_id)
        .bind(&now_text)
        .execute(&mut *transaction)
        .await
        .map_err(ledger_error)?;
    }
    // --- daily_limit: after the rollover the (possibly reset) counter is
    // the one compared, expressed in one row:
    // `usage_day == today ? used : 0`.
    let row = sqlx::query(
        "SELECT used, daily_limit, (usage_day=(($2::timestamptz AT TIME ZONE 'UTC')::date)) AS same
        FROM archava_provider.buckets WHERE bucket_id=$1::text",
    )
    .bind(bucket_id)
    .bind(&now_text)
    .fetch_one(&mut *transaction)
    .await
    .map_err(ledger_error)?;
    let same_day = row.try_get::<bool, _>(2).map_err(|_| StoreError)?;
    let used: i32 = if same_day {
        row.try_get(0).map_err(|_| StoreError)?
    } else {
        0
    };
    let daily_limit = row.try_get::<i32, _>(1).map_err(|_| StoreError)?;
    if used >= daily_limit {
        // TS: `retryAfterMs = nextDay - now` — next UTC midnight, computed
        // against the same instant.
        let retry_after_ms = sqlx::query_scalar::<_, i64>(
            "SELECT GREATEST(0, FLOOR(EXTRACT(EPOCH FROM (((($2::timestamptz AT TIME ZONE 'UTC')::date + 1)::timestamp) AT TIME ZONE 'UTC' - $2::timestamptz))*1000)::bigint) AS wait_ms",
        )
        .bind(bucket_id)
        .bind(&now_text)
        .fetch_one(&mut *transaction)
        .await
        .map_err(ledger_error)?;
        return Ok(Admission::Refused {
            code: AdmissionRefusal::DailyLimit,
            retry_after_ms,
        });
    }
    // --- cooldown: the minimum interval between two attempt starts, against
    // the row's own `last_started_at`. TS computes
    // `remaining = minimumIntervalMs - (now - lastStarted)` and only refuses
    // when it is positive; the same single expression with
    // NULL-for-no-start mirrors that exactly.
    let remaining: Option<i64> = sqlx::query_scalar(
        "SELECT CASE WHEN last_started_at IS NULL THEN NULL
         ELSE GREATEST(0, ($3::bigint) - FLOOR(EXTRACT(EPOCH FROM ($2::timestamptz - last_started_at))*1000)::bigint) END
        FROM archava_provider.buckets WHERE bucket_id=$1::text",
    )
    .bind(bucket_id)
    .bind(&now_text)
    .bind(i64::from(policy.minimum_interval_ms()))
    .fetch_one(&mut *transaction)
    .await
    .map_err(ledger_error)?;
    if let Some(retry_after_ms) = remaining.filter(|ms| *ms > 0) {
        return Ok(Admission::Refused {
            code: AdmissionRefusal::Cooldown,
            retry_after_ms,
        });
    }
    // --- admit: the attempt row is the durable record of the reservation;
    // the lease columns reference the SAME instant.
    let inserted = sqlx::query(
        "INSERT INTO archava_provider.attempts(bucket_id,attempt_id,started_at,state)
        VALUES($1::text,$2,$3::timestamptz,'pending')",
    )
    .bind(bucket_id)
    .bind(attempt_id)
    .bind(&now_text)
    .execute(&mut *transaction)
    .await
    .map_err(ledger_error)?;
    if inserted.rows_affected() != 1 {
        return Err(StoreError);
    }
    let leased = sqlx::query(
        "UPDATE archava_provider.buckets
        SET used=used+1,last_started_at=$2::timestamptz,active_attempt=$3,
            lease_until=($2::timestamptz + ($4::integer * interval '1 millisecond'))
        WHERE bucket_id=$1::text",
    )
    .bind(bucket_id)
    .bind(&now_text)
    .bind(attempt_id)
    .bind(policy.lease_ms())
    .execute(&mut *transaction)
    .await
    .map_err(ledger_error)?;
    if leased.rows_affected() != 1 {
        return Err(StoreError);
    }
    Ok(Admission::Allowed)
}

/// The finalize body, on an already-open bucket transaction.
async fn finish_body(
    transaction: &mut PgConnection,
    bucket_id: &str,
    attempt_id: &Uuid,
    completion: Completion,
) -> Result<bool, StoreError> {
    let now_text = PostgresProviderAttemptStore::now_instant(&mut *transaction).await?;
    // The bucket must exist. TS does NOT re-seed here: a finish for an
    // unknown budget stays unknown. (The bucket id validation happens in TS
    // via `budgetIdSchema.parse` before the transaction; a validation
    // failure is an exception → `outcome_unknown`, the same place this maps
    // to.)
    let active_attempt: Option<String> = sqlx::query(
        "SELECT active_attempt::text FROM archava_provider.buckets WHERE bucket_id=$1::text FOR UPDATE",
    )
    .bind(bucket_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(ledger_error)?
    .and_then(|row| row.try_get::<String, _>(0).ok());
    // Reap the expired lease BEFORE the ownership check — TS's call order
    // (`reap(client, bucket, now)` then `active_attempt !== id` on the
    // mutated bucket object).
    reap(&mut *transaction, bucket_id, &now_text).await?;
    // After the reap, a still-set active attempt is THIS one only if the
    // lease was live (the reap cleared nothing); a cleared one means the
    // lease expired — its attempt is `unknown` now, and nothing may finalize
    // it. (The pending-only UPDATE below double-covers the reaped case.)
    let owned = active_attempt
        .as_deref()
        .is_some_and(|active| active == attempt_id.to_string());
    if !owned {
        return Ok(false);
    }
    let (state, failure_code, http_status) = completion.as_sql_args();
    let finalized = sqlx::query(
        "UPDATE archava_provider.attempts
        SET state=$2::text,http_status=$3,failure_code=$4,finished_at=$5::timestamptz,
        elapsed_ms=LEAST(2147483647,GREATEST(0,EXTRACT(EPOCH FROM ($5::timestamptz-started_at))*1000)::bigint)::integer
        WHERE bucket_id=$1::text AND attempt_id=$6 AND state='pending'",
    )
    .bind(bucket_id)
    .bind(state)
    .bind(http_status)
    .bind(failure_code)
    .bind(&now_text)
    .bind(attempt_id)
    .execute(&mut *transaction)
    .await
    .map_err(ledger_error)?;
    // A second finish of the same attempt (already recorded under a live
    // lease, lease cleared) or a reaped attempt leaves zero rows — an
    // unknown outcome.
    if finalized.rows_affected() != 1 {
        return Ok(false);
    }
    // TS clears the lease columns ONLY after a successful finalize.
    sqlx::query(
        "UPDATE archava_provider.buckets SET active_attempt=NULL,lease_until=NULL WHERE bucket_id=$1::text",
    )
    .bind(bucket_id)
    .execute(&mut *transaction)
    .await
    .map_err(ledger_error)?;
    Ok(true)
}

impl ProviderAttemptStore for PostgresProviderAttemptStore {
    async fn reserve(
        &self,
        policy: &ProviderPolicy,
        attempt_id: &Uuid,
    ) -> Result<Admission, StoreError> {
        let bucket_id = policy.bucket_id().to_string();
        self.reserve_within(&bucket_id, policy, attempt_id).await
    }

    async fn finish(
        &self,
        bucket_id: &str,
        attempt_id: &Uuid,
        completion: Completion,
    ) -> Result<bool, StoreError> {
        // TS parses the bucket id before opening the transaction; a
        // validation failure maps to the same unknown-outcome corner.
        let bucket_id = validate_budget_id(bucket_id).map_err(|_| StoreError)?.value;
        self.finish_within(&bucket_id, attempt_id, completion).await
    }
}

/// The operator recovery hold: freeze THIS UTC day's allowance rather than
/// trusting stale backup counters.
///
/// TS: `holdProviderAfterRestore` (`postgres.ts` lines 402–425) — every
/// pending attempt is reaped as `unknown`/`interrupted`, every lease clears,
/// and the bucket holds until tomorrow's UTC midnight. The clock stays the
/// server's: `seed` and both updates use `clock_timestamp()` directly.
pub async fn hold_provider_after_restore(
    pool: &PgPool,
    policy: &ProviderPolicy,
) -> Result<(), StoreError> {
    let bucket_id = policy.bucket_id().to_string();
    let mut transaction = open_bucket(pool, &bucket_id).await?;
    seed(&mut transaction, policy).await?;
    sqlx::query(
        "UPDATE archava_provider.attempts
        SET state='unknown',failure_code='interrupted',finished_at=clock_timestamp(),
        elapsed_ms=LEAST(2147483647,GREATEST(0,EXTRACT(EPOCH FROM (clock_timestamp()-started_at))*1000)::bigint)::integer
        WHERE bucket_id=$1::text AND state='pending'",
    )
    .bind(&bucket_id)
    .execute(&mut *transaction)
    .await
    .map_err(ledger_error)?;
    sqlx::query(
        "UPDATE archava_provider.buckets SET active_attempt=NULL,lease_until=NULL,
        hold_until=((clock_timestamp() AT TIME ZONE 'UTC')::date+1)::timestamp AT TIME ZONE 'UTC'
        WHERE bucket_id=$1::text",
    )
    .bind(&bucket_id)
    .execute(&mut *transaction)
    .await
    .map_err(ledger_error)?;
    transaction.commit().await.map_err(ledger_error)?;
    Ok(())
}

/// One recent attempt under the status read's projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAttemptView {
    pub request_id: String,
    pub state: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub elapsed_ms: Option<i32>,
    pub failure_code: Option<String>,
    pub http_status: Option<i32>,
}

/// The status read the operator surface consumes.
///
/// TS: `readProviderStatus` (`postgres.ts` lines 427–...) — the seeded,
/// policy-checked, reaped bucket's state of this UTC day plus the twenty
/// most recent attempts. Like the TS function, this path checks nothing
/// about the connecting role (`create` did, in the paths that open a
/// store); the RLS scope still applies to whatever the pool's role can see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStatus {
    pub bucket_id: String,
    pub provider_id: String,
    pub model: String,
    pub utc_day: String,
    pub daily_limit: i32,
    pub attempts_today: i32,
    pub remaining: i32,
    pub restore_hold_until: Option<String>,
    pub active_request_id: Option<String>,
    pub recent: Vec<ProviderAttemptView>,
}

/// TS `readProviderStatus(pool, policy)` — the bucket's status read.
pub async fn read_provider_status(
    pool: &PgPool,
    policy: &ProviderPolicy,
) -> Result<ProviderStatus, StoreError> {
    let bucket_id = policy.bucket_id().to_string();
    let mut transaction = open_bucket(pool, &bucket_id).await?;
    let now_text = PostgresProviderAttemptStore::now_instant(&mut transaction).await?;
    // Seed FIRST: a status read of an unseen budget seeds then refuses a
    // policy mismatch — the status of a budget the caller misdescribes is
    // unknown, not zero.
    seed(&mut transaction, policy).await?;
    reap(&mut transaction, &bucket_id, &now_text).await?;
    let bucket = sqlx::query(
        "SELECT
          to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD') AS utc_day,
          CASE WHEN usage_day=(($2::timestamptz AT TIME ZONE 'UTC')::date) THEN used ELSE 0 END AS used,
          (hold_until IS NOT NULL AND hold_until > $2::timestamptz) AS held,
          to_char(CASE WHEN hold_until IS NOT NULL AND hold_until > $2::timestamptz THEN hold_until END, $3::text) AS restore_hold_until,
          active_attempt::text AS active_attempt
        FROM archava_provider.buckets WHERE bucket_id=$1::text",
    )
    .bind(&bucket_id)
    .bind(&now_text)
    .bind(ISO_FORMAT)
    .fetch_one(&mut *transaction)
    .await
    .map_err(ledger_error)?;
    let attempts_today: i32 = bucket.try_get("used").map_err(|_| StoreError)?;
    let held: bool = bucket.try_get("held").map_err(|_| StoreError)?;
    let restore_hold_until: Option<String> = bucket
        .try_get("restore_hold_until")
        .map_err(|_| StoreError)?;
    let active_request_id: Option<String> =
        bucket.try_get("active_attempt").map_err(|_| StoreError)?;
    let utc_day: String = bucket.try_get("utc_day").map_err(|_| StoreError)?;
    let recent = sqlx::query(
        "SELECT
          attempt_id::text,
          state::text,
          to_char(started_at, $2::text) AS started_at,
          to_char(finished_at, $2::text) AS finished_at,
          elapsed_ms, failure_code, http_status
        FROM archava_provider.attempts WHERE bucket_id=$1::text
        ORDER BY started_at DESC, attempt_id LIMIT 20",
    )
    .bind(&bucket_id)
    .bind(ISO_FORMAT)
    .fetch_all(&mut *transaction)
    .await
    .map_err(ledger_error)?
    .into_iter()
    .map(attempt_view)
    .collect::<Vec<_>>();
    transaction.commit().await.map_err(ledger_error)?;
    Ok(ProviderStatus {
        bucket_id: policy.bucket_id().to_string(),
        provider_id: policy.provider_id().to_string(),
        model: policy.model().to_string(),
        utc_day,
        daily_limit: policy.daily_limit(),
        attempts_today,
        // TS: `remaining = held ? 0 : max(0, dailyLimit - today)`.
        remaining: if held {
            0
        } else {
            (policy.daily_limit() - attempts_today).max(0)
        },
        restore_hold_until,
        active_request_id,
        recent,
    })
}

/// The per-row projection of the recent-attempt list. Every timestamp is
/// already ISO-formatted IN SQL (`to_char` over the same ISO_FORMAT text),
/// which is what keeps the operational time format byte-identical to TS's
/// `toISOString()` without a client-side timezone.
fn attempt_view(row: PgRow) -> ProviderAttemptView {
    ProviderAttemptView {
        request_id: row.get(0),
        state: row.get(1),
        started_at: row.get(2),
        finished_at: row.try_get(3).ok(),
        elapsed_ms: row.try_get(4).ok(),
        failure_code: row.try_get(5).ok(),
        http_status: row.try_get(6).ok(),
    }
}

/// Apply the provider migration standalone, exactly like TS's
/// `migrateProviderRuntime` (`postgres.ts` lines 19–52): advisory-locked,
/// verified-first, applied only when the ledger is empty. A ledger that
/// carries anything else is unsupported and refuses.
///
/// The fixture's provision applies the same migration through its
/// single-writer ledger (`archava_provider` arm), so both paths converge on
/// one recorded `(id, checksum)`.
pub async fn migrate_provider_runtime(pool: &PgPool) -> Result<(), SetupError> {
    let sql_bytes = migration_sql()?;
    let expected = (MIGRATION_ID.to_string(), migration_checksum(&sql_bytes));
    // `raw_sql` demands a 'static str: the migration text is frozen content
    // read once, so leaking the verified bytes is the honest shape (the
    // fixture asserts the same pairing through its ledger).
    let utf8: &'static str = Box::leak(
        String::from_utf8(sql_bytes)
            .map_err(|error| SetupError::MigrationBytes {
                path: MIGRATION_ID.to_string(),
                error: error.to_string(),
            })?
            .into_boxed_str(),
    );
    let mut transaction = pool.begin().await.map_err(setup_ledger_error)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(MIGRATION_LOCK)
        .execute(&mut *transaction)
        .await
        .map_err(setup_ledger_error)?;
    sqlx::query("CREATE SCHEMA IF NOT EXISTS archava_provider")
        .execute(&mut *transaction)
        .await
        .map_err(setup_ledger_error)?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS archava_provider.schema_migrations(id text PRIMARY KEY,checksum text NOT NULL)",
    )
    .execute(&mut *transaction)
    .await
    .map_err(setup_ledger_error)?;
    let rows: Vec<(String, String)> =
        sqlx::query("SELECT id::text, checksum::text FROM archava_provider.schema_migrations")
            .fetch_all(&mut *transaction)
            .await
            .map_err(setup_ledger_error)?
            .into_iter()
            .map(|row| (row.get::<String, _>(0), row.get::<String, _>(1)))
            .collect();
    if rows.len() == 1 && rows[0] == expected {
        transaction.commit().await.map_err(setup_ledger_error)?;
        return Ok(());
    }
    if !rows.is_empty() {
        return Err(SetupError::UnsupportedSchema);
    }
    sqlx::raw_sql(utf8)
        .execute(&mut *transaction)
        .await
        .map_err(setup_ledger_error)?;
    sqlx::query("INSERT INTO archava_provider.schema_migrations VALUES($1,$2)")
        .bind(&expected.0)
        .bind(&expected.1)
        .execute(&mut *transaction)
        .await
        .map_err(setup_ledger_error)?;
    transaction.commit().await.map_err(setup_ledger_error)?;
    Ok(())
}

/// Port the runtime grants (`grantProviderRuntime`, `postgres.ts` lines
/// 54–65): usage on the schema, read-only history, and draft writing on
/// buckets/attempts. The role name is validated, then interpolated — `sqlx`
/// does not bind identifiers, and the fixture's provision applies the same
/// statements so the two paths provably match.
pub async fn grant_provider_runtime(pool: &PgPool, role: &str) -> Result<(), SetupError> {
    validate_grant_role(role)?;
    // Interpolation is audited: `role` passed `validate_grant_role` — the
    // identifier charset is closed (`[a-z][a-z0-9_]{0,62}`), the fixed
    // namespace comes from this file's constant, and `sqlx::AssertSqlSafe`
    // carries the audit the adapters' GRANT cohort uses for the same shape.
    for statement in [
        format!("GRANT USAGE ON SCHEMA {NAMESPACE} TO \"{role}\""),
        format!("GRANT SELECT ON {NAMESPACE}.schema_migrations TO \"{role}\""),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON {NAMESPACE}.buckets,{NAMESPACE}.attempts TO \"{role}\""
        ),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .execute(pool)
            .await
            .map_err(setup_ledger_error)?;
    }
    Ok(())
}
