//! The fenced single-writer lease — MIG-05 at the persistence boundary.
//!
//! MIG-05: "Single writer across HTTP/CLI/workers/webhooks/jobs/recovery; the
//! fencing is applied at the persistence boundary, refused a stale process."
//! A file lock cannot deliver that: it guards the process's seat, not the
//! write itself. This fence does the only thing that can — every write goes
//! through a transaction that FIRST locks the lease row, verifies the caller
//! still holds the current epoch, and only then executes the statement. A
//! process that lost its lease between starting and writing is refused where
//! its write lands, no matter how long ago it acquired the lease.
//!
//! The table is `archava_control.writer_lease`, created by an explicit
//! ownership statement from the ops runner (fixture/lab scope), not by a
//! migration: the migration set is frozen in the release identity, and
//! operational tooling must not edit it.

use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

/// The control schema + lease table, installed idempotently.
///
/// The DDL is operator-owned tooling identical to the migrations'
/// `AssertSqlSafe` pattern: every interpolated identifier is a constant
/// declared in this file.
pub async fn install(pool: &PgPool) -> Result<(), LeaseError> {
    sqlx::raw_sql("CREATE SCHEMA IF NOT EXISTS archava_control")
        .execute(pool)
        .await
        .map_err(LeaseError::Database)?;
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS archava_control.writer_lease(\
         lease_name text PRIMARY KEY, \
         epoch bigint NOT NULL DEFAULT 0, \
         owner_token text NOT NULL, \
         drained boolean NOT NULL DEFAULT false, \
         compat_window_seconds integer NOT NULL DEFAULT 300, \
         taken_at timestamptz NOT NULL DEFAULT now(), \
         updated_at timestamptz NOT NULL DEFAULT now())",
    )
    .execute(pool)
    .await
    .map_err(LeaseError::Database)?;
    Ok(())
}

/// A live writer's seat. `epoch` is the fencing token the write path checks;
/// `token` identifies the holder for operators (never a secret — it names a
/// local process seat, not a credential).
#[derive(Debug, Clone, Serialize)]
pub struct WriterLease {
    pub lease: String,
    pub epoch: i64,
    pub token: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LeaseError {
    /// The write's `SELECT … FOR UPDATE` saw an epoch HIGHER than the one the
    /// caller proved at acquisition. The stale writer is the REFUSAL CLASS —
    /// its own seat, not a database error.
    #[error("stale writer on lease '{lease}': held epoch {held}, lease is at {current}")]
    StaleWriter {
        lease: String,
        held: i64,
        current: i64,
    },
    /// The lease was drained; new-state writes must be coordinated by the
    /// handover, not squeezed past a drained fence.
    #[error("lease '{lease}' is drained — a writer must take over by epoch, not push through")]
    Drained { lease: String },
    #[error("lease database failure: {0}")]
    Database(#[from] sqlx::Error),
}

/// Take over (or acquire) the named lease, advancing the epoch.
///
/// The advisory-lock + retry-through-serialization shape mirrors the
/// migration runner's advisory lock: two takers race into a serialized
/// `upsert`, each bumps the epoch, and the EARLIER one's writes die at the
/// fence with [`LeaseError::StaleWriter`]. The winner is whoever holds the
/// epoch when the write happens — exactly the MIG-05 property.
pub async fn take(pool: &PgPool, lease: &str) -> Result<WriterLease, LeaseError> {
    let token = Uuid::new_v4().to_string();
    let row: (i64, String) = sqlx::query_as(
        "INSERT INTO archava_control.writer_lease(lease_name, epoch, owner_token) \
         VALUES ($1, 1, $2) \
         ON CONFLICT (lease_name) DO UPDATE SET \
             epoch = writer_lease.epoch + 1, \
             owner_token = EXCLUDED.owner_token, \
             drained = false, \
             taken_at = now(), \
             updated_at = now() \
         RETURNING epoch, owner_token",
    )
    .bind(lease)
    .bind(&token)
    .fetch_one(pool)
    .await?;
    Ok(WriterLease {
        lease: lease.to_string(),
        epoch: row.0,
        token: row.1,
    })
}

/// Mark the lease drained (MIG-08's handover step: drain before cutover).
pub async fn drain(pool: &PgPool, lease: &str) -> Result<(), LeaseError> {
    sqlx::query(
        "UPDATE archava_control.writer_lease SET drained = true, updated_at = now() \
         WHERE lease_name = $1",
    )
    .bind(lease)
    .execute(pool)
    .await?;
    Ok(())
}

impl WriterLease {
    /// Prove THIS lease still owns the seat, ON the caller's transaction.
    ///
    /// `SELECT … FOR UPDATE` on the lease row does two jobs at once while the
    /// transaction stays open:
    ///
    /// 1. the epoch the caller holds is compared against the lease's — a
    ///    stale writer sees the mismatch and is refused exactly here; and
    /// 2. the row lock holds any concurrent `take()` until this transaction
    ///    ends, so a write proven here cannot have its seat taken away while
    ///    it writes. (Staleness is checked per-CONSEQUENT transaction: a
    ///    writer that lost its seat mid-flight discovers it on its next
    ///    transaction's `SELECT`, which is the refusal the rollback/handover
    ///    rehearsals require — see MIG-05.)
    ///
    /// Call it IMMEDIATELY before the statements that depend on it, inside
    /// the same transaction: the lock is released only when that transaction
    /// commits, and the takeover that refuses the next attempt is serialized
    /// behind this one.
    pub async fn assert_fresh(&self, tx: &mut sqlx::PgConnection) -> Result<(), LeaseError> {
        let row: (i64, bool) = sqlx::query_as(
            "SELECT epoch, drained FROM archava_control.writer_lease \
             WHERE lease_name = $1 FOR UPDATE",
        )
        .bind(&self.lease)
        .fetch_one(&mut *tx)
        .await?;
        if row.0 != self.epoch {
            return Err(LeaseError::StaleWriter {
                lease: self.lease.clone(),
                held: self.epoch,
                current: row.0,
            });
        }
        if row.1 {
            return Err(LeaseError::Drained {
                lease: self.lease.clone(),
            });
        }
        Ok(())
    }
}
