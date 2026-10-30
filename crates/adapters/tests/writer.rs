//! Serving fencing at the real SQL boundary, including raw Identity writes.
use archava_adapters::{
    rls::{assert_non_owner_runtime, TenantContextError},
    writer::{assert_ready, connect, is_current, WriterError},
};
use archava_pg_fixture::provision;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::time::Duration;

async fn no_op_write(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM archava_identity.login_buckets WHERE false")
        .execute(pool)
        .await?;
    Ok(())
}
fn stale(error: sqlx::Error) {
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("57P03"),
        "{error}"
    );
}

#[tokio::test]
async fn missing_context_takeover_and_reconnect_never_adopt_another_process_identity() {
    let f = provision().await;
    let base: PgConnectOptions = f.runtime_url().parse().unwrap();
    let absent = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(base.clone())
        .await
        .unwrap();
    stale(no_op_write(&absent).await.unwrap_err());
    assert!(assert_ready(&absent).await.is_err());
    absent.close().await;
    match connect(base.clone(), 2, false).await {
        Err(WriterError::Database(error)) => assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("55006")
        ),
        other => panic!("active writer must require explicit takeover: {other:?}"),
    }
    let old_options = f.runtime_options();
    let (current, lease) = connect(base.clone(), 2, true).await.unwrap();
    assert!(lease.epoch > f.writer_lease.epoch);
    stale(no_op_write(&f.runtime_pool).await.unwrap_err());
    let reconnected = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(old_options)
        .await
        .unwrap();
    stale(no_op_write(&reconnected).await.unwrap_err());
    assert!(
        !f.writer_lease.drain(&reconnected).await.unwrap(),
        "stale shutdown cannot drain its successor"
    );
    no_op_write(&current).await.unwrap();
    assert!(lease.drain(&current).await.unwrap());
    stale(no_op_write(&current).await.unwrap_err());
    let (next, next_lease) = connect(base, 2, false).await.unwrap();
    assert!(next_lease.epoch > lease.epoch);
    is_current(&next).await.unwrap();
    next.close().await;
    reconnected.close().await;
    current.close().await;
}

#[tokio::test]
async fn valid_writes_run_in_parallel_and_takeover_waits_until_the_old_commit() {
    let f = provision().await;
    for key in ["a".repeat(64), "b".repeat(64)] {
        sqlx::query("INSERT INTO archava_identity.login_buckets(key,attempts) VALUES($1,1)")
            .bind(key)
            .execute(&f.admin_pool)
            .await
            .unwrap();
    }
    let mut first = f.runtime_pool.begin().await.unwrap();
    sqlx::query("UPDATE archava_identity.login_buckets SET attempts=2 WHERE key=$1")
        .bind("a".repeat(64))
        .execute(&mut *first)
        .await
        .unwrap();
    let mut parallel = f.runtime_pool.begin().await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        sqlx::query("UPDATE archava_identity.login_buckets SET attempts=2 WHERE key=$1")
            .bind("b".repeat(64))
            .execute(&mut *parallel),
    )
    .await
    .expect("valid writers must share the lease lock")
    .unwrap();
    parallel.commit().await.unwrap();
    let options = f.runtime_url().parse().unwrap();
    let mut takeover = tokio::spawn(async move { connect(options, 2, true).await });
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut takeover)
            .await
            .is_err(),
        "takeover cannot cross an uncommitted valid write"
    );
    first.commit().await.unwrap();
    let (current, _) = tokio::time::timeout(Duration::from_secs(4), takeover)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let amounts: Vec<i32> =
        sqlx::query_scalar("SELECT attempts FROM archava_identity.login_buckets ORDER BY key")
            .fetch_all(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(amounts, vec![2, 2]);
    stale(no_op_write(&f.runtime_pool).await.unwrap_err());
    no_op_write(&current).await.unwrap();
    current.close().await;
}

#[tokio::test]
async fn disabled_guard_and_inherited_schema_ownership_refuse_readiness() {
    let f = provision().await;
    sqlx::query(
        "ALTER TABLE archava_identity.login_buckets DISABLE TRIGGER archava_runtime_write_fence",
    )
    .execute(&f.admin_pool)
    .await
    .unwrap();
    assert!(assert_ready(&f.runtime_pool).await.is_err());
    sqlx::query(
        "ALTER TABLE archava_identity.login_buckets ENABLE TRIGGER archava_runtime_write_fence",
    )
    .execute(&f.admin_pool)
    .await
    .unwrap();
    assert_ready(&f.runtime_pool).await.unwrap();
    sqlx::raw_sql("DROP TRIGGER archava_runtime_write_fence ON archava_identity.login_buckets;CREATE TRIGGER archava_runtime_write_fence AFTER DELETE ON archava_identity.login_buckets FOR EACH STATEMENT EXECUTE FUNCTION archava_control.guard_runtime_write()")
        .execute(&f.admin_pool).await.unwrap();
    assert!(
        assert_ready(&f.runtime_pool).await.is_err(),
        "a partial/late trigger is not a complete write fence"
    );
    sqlx::query("DROP TRIGGER archava_runtime_write_fence ON archava_identity.login_buckets")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    sqlx::query("SELECT archava_control.install_runtime_write_guards()")
        .execute(&f.owner_pool)
        .await
        .unwrap();
    assert_ready(&f.runtime_pool).await.unwrap();
    sqlx::query("GRANT archava_owner TO archava_runtime")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    assert!(matches!(
        assert_non_owner_runtime(&f.runtime_pool, "archava_commerce", "Commerce").await,
        Err(TenantContextError::NonOwnerRuntime { .. })
    ));
    assert!(assert_ready(&f.runtime_pool).await.is_err());
    sqlx::query("REVOKE archava_owner FROM archava_runtime")
        .execute(&f.admin_pool)
        .await
        .unwrap();
    assert_ready(&f.runtime_pool).await.unwrap();
}
