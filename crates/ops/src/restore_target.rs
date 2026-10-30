//! Hold an isolated target closed to ordinary clients throughout restore.
//! The CLI requires a cluster administrator; application runtime roles are never superusers.
use crate::backup::{ConnectionSpec, RestoreError};
use sqlx::{Connection, PgConnection, Row};
/// An owned session holds the restore lock until the final metadata reset.
/// If the operator process crashes, connection limit zero remains fail-closed.
pub struct RestoreTargetGuard {
    connection: PgConnection,
    database: String,
    previous_limit: i32,
}
fn alter_limit(database: &str, limit: i32) -> String {
    format!(
        "ALTER DATABASE \"{}\" CONNECTION LIMIT {limit}",
        database.replace('"', "\"\"")
    )
}
impl RestoreTargetGuard {
    pub async fn acquire(target: &ConnectionSpec) -> Result<Self, RestoreError> {
        let options = sqlx::postgres::PgConnectOptions::new()
            .host(&target.host)
            .port(target.port)
            .username(&target.user)
            .database(&target.database);
        let mut connection = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            PgConnection::connect_with(&options),
        )
        .await
        .map_err(|_| RestoreError::Tool("restore target connection timed out".into()))?
        .map_err(|_| RestoreError::Tool("restore target connection unavailable".into()))?;
        let superuser: bool =
            sqlx::query_scalar("SELECT rolsuper FROM pg_roles WHERE rolname=current_user")
                .fetch_one(&mut connection)
                .await
                .map_err(db)?;
        if !superuser {
            return Err(RestoreError::Tool(
                "isolated restore requires an explicit cluster administrator".into(),
            ));
        }
        let locked:bool=sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtextextended('archava:restore:'||current_database(),0))").fetch_one(&mut connection).await.map_err(db)?;
        if !locked {
            return Err(RestoreError::Tool("another restore owns the target".into()));
        }
        let row = sqlx::query(
            "SELECT datname,datconnlimit FROM pg_database WHERE datname=current_database()",
        )
        .fetch_one(&mut connection)
        .await
        .map_err(db)?;
        let mut guard = Self {
            database: row.get("datname"),
            previous_limit: row.get("datconnlimit"),
            connection,
        };
        let check = guard.close_and_check().await;
        if let Err(error) = check {
            guard.release().await?;
            return Err(error);
        }
        Ok(guard)
    }
    async fn close_and_check(&mut self) -> Result<(), RestoreError> {
        // A committed connection limit blocks new ordinary runtime connections.
        // Never terminate existing sessions: an active target is a refusal.
        let statement = alter_limit(&self.database, 0);
        sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .execute(&mut self.connection)
            .await
            .map_err(db)?;
        let active:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND pid<>pg_backend_pid() AND backend_type='client backend'").fetch_one(&mut self.connection).await.map_err(db)?;
        if active > 0 {
            return Err(RestoreError::Tool("target still has client connections; stop/drain all target services before restore".into()));
        }
        let lease: Result<Option<String>, sqlx::Error> = sqlx::query_scalar(
            "SELECT lease_name FROM archava_control.writer_lease WHERE NOT drained LIMIT 1",
        )
        .fetch_optional(&mut self.connection)
        .await;
        match lease {
            Ok(Some(lease)) => Err(RestoreError::UndrainedWriter { lease }),
            Ok(None) => Ok(()),
            Err(sqlx::Error::Database(ref error)) if error.code().as_deref() == Some("42P01") => {
                Ok(())
            }
            Err(error) => Err(db(error)),
        }
    }
    pub async fn release(mut self) -> Result<(), RestoreError> {
        let statement = alter_limit(&self.database, self.previous_limit);
        sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .execute(&mut self.connection)
            .await
            .map_err(db)?;
        let unlocked: bool = sqlx::query_scalar(
            "SELECT pg_advisory_unlock(hashtextextended('archava:restore:'||current_database(),0))",
        )
        .fetch_one(&mut self.connection)
        .await
        .map_err(db)?;
        if !unlocked {
            return Err(RestoreError::Tool("restore target lock was lost".into()));
        }
        self.connection.close().await.map_err(db)?;
        Ok(())
    }
}
fn db(_error: sqlx::Error) -> RestoreError {
    RestoreError::Tool("restore target isolation operation failed; inspect target connection limit before restarting services".into())
}
