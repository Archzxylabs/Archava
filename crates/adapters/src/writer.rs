//! Fixed process lease on every connection, including reconnects.
use crate::rls::{assert_startup_readiness, migration_checksum, TenantContextError};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::time::Duration;
use uuid::Uuid;
pub const NAMESPACE: &str = "archava_control";
pub const LEDGER: &str = "Serving writer";
pub const MIGRATION_ID: &str = "001_serving_writer";
// This include path is relative to crates/adapters/src/writer.rs after integration.
pub const MIGRATION_SQL: &[u8] =
    include_bytes!("../../../migrations/control-pg/001_serving_writer.sql");
#[derive(Debug, thiserror::Error)]
pub enum WriterError {
    #[error("serving writer schema is not ready")]
    Readiness(#[from] TenantContextError),
    #[error(
        "serving writer operation failed; check database readiness, drain or explicit takeover"
    )]
    Database(#[from] sqlx::Error),
}
#[derive(Clone)]
pub struct WriterLease {
    pub epoch: i64,
    token: String,
}
impl std::fmt::Debug for WriterLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriterLease")
            .field("epoch", &self.epoch)
            .finish_non_exhaustive()
    }
}
impl WriterLease {
    pub fn connection_options(&self, options: PgConnectOptions) -> PgConnectOptions {
        options.options([
            ("archava.writer_epoch", self.epoch.to_string()),
            ("archava.writer_token", self.token.clone()),
        ])
    }
    pub async fn drain(&self, pool: &PgPool) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar("SELECT archava_control.drain_serving_writer($1,$2)")
            .bind(self.epoch)
            .bind(&self.token)
            .fetch_one(pool)
            .await
    }
}
pub async fn assert_schema(pool: &PgPool) -> Result<(), TenantContextError> {
    assert_startup_readiness(
        pool,
        NAMESPACE,
        LEDGER,
        &["writer_lease"],
        &[(MIGRATION_ID.into(), migration_checksum(MIGRATION_SQL))],
    )
    .await
}
pub async fn is_current(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT archava_control.require_serving_writer()")
        .execute(pool)
        .await?;
    Ok(())
}
pub async fn assert_ready(pool: &PgPool) -> Result<(), TenantContextError> {
    assert_schema(pool).await?;
    is_current(pool)
        .await
        .map_err(|source| TenantContextError::Database {
            ledger: LEDGER,
            source,
        })?;
    let complete: bool = sqlx::query_scalar("SELECT archava_control.write_guards_complete()")
        .fetch_one(pool)
        .await
        .map_err(|source| TenantContextError::Database {
            ledger: LEDGER,
            source,
        })?;
    if !complete {
        return Err(TenantContextError::Database {
            ledger: LEDGER,
            source: sqlx::Error::Protocol("Runtime write fence coverage is incomplete".into()),
        });
    }
    Ok(())
}
/// Connect without adopting any other process's epoch later. Takeover is an
/// explicit operator decision, never an automatic response to a query error.
pub async fn connect(
    options: PgConnectOptions,
    maximum: u32,
    takeover: bool,
) -> Result<(PgPool, WriterLease), WriterError> {
    let bootstrap = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options.clone())
        .await?;
    assert_schema(&bootstrap).await?;
    let token = Uuid::new_v4().to_string();
    let epoch = sqlx::query_scalar("SELECT archava_control.take_serving_writer($1,$2)")
        .bind(&token)
        .bind(takeover)
        .fetch_one(&bootstrap)
        .await?;
    let lease = WriterLease { epoch, token };
    let pool_result = PgPoolOptions::new()
        .max_connections(maximum.max(1))
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(lease.connection_options(options))
        .await;
    let pool = match pool_result {
        Ok(pool) => pool,
        Err(error) => {
            let _ = lease.drain(&bootstrap).await;
            bootstrap.close().await;
            return Err(error.into());
        }
    };
    if let Err(error) = assert_ready(&pool).await {
        let _ = lease.drain(&bootstrap).await;
        pool.close().await;
        bootstrap.close().await;
        return Err(error.into());
    }
    bootstrap.close().await;
    Ok((pool, lease))
}
