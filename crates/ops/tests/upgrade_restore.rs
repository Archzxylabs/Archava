//! R10's upgrade/restore/rollback/fencing rehearsal suite — the five MIG
//! rules the `P-UPGRADE-RESTORE-RUST` proof requires, each as ONE named
//! witness the producer (`tools/rust/release-ops.mjs`) can grade.
//!
//! Every test provisions its own disposable fixture through
//! [`archava_pg_fixture`], so no rehearsal can see, depend on, or corrupt
//! another's state — the same fixture discipline the adapters' G03 suite
//! carries. The facilities under test — digests, refusals, fences — live in
//! [`archava_ops`]; the suite is the SCENARIO, not the mechanism, and prints
//! nothing beyond libtest's own `test <name> ... ok|FAILED` lines.
//!
//! The MIG rule each witness carries:
//!
//! - `fresh_install_upgrade_passes` — MIG-03's fresh side: every frozen
//!   ledger applies cleanly and its recorded history is exactly what the
//!   frozen migration files derive.
//! - `prior_profile_upgrade_passes` — MIG-03's prior side + MIG-08's data
//!   preservation: a database created one profile earlier upgrades by REAL
//!   continuation (history prefix accepted, newest migration applied), the
//!   prior rows intact and unchanged apart from 004's two least-privileged
//!   default columns being EXISTING-migration addenda the projection
//!   normalizes, and the renewed history exactly re-derivable from the same
//!   files — including the second call being a no-op (idempotence).
//! - `backup_restore_rehearsal_passes` — OBS-04 + MIG-04: capture publishes
//!   digests and counts (never values), the restore REFUSES its own capture
//!   source and an undrained writer lease, and the replay lands in a fresh,
//!   disposable instance carrying identical preserved rows.
//! - `rollback_rehearsal_passes` — MIG-08: a handover drains the old seat,
//!   the new holder owns the epoch, and the old seat's write is refused
//!   where it executes while the protected rows stay intact.
//! - `fenced_single_writer_handover_passes` — MIG-05: takeover fencing — a
//!   write whose epoch is behind the lease's is refused on the write path,
//!   from any stale process, without exception.

use archava_ops::backup::{
    capture, capture_with_key, encode_base64, read_manifest, refuse_undrained_target, restore,
    restore_with_key, BackupManifest, ConnectionSpec, ReleaseIdentity, RestoreError,
    BACKUP_MAGIC_HEADER,
};
use archava_ops::lease;
use archava_pg_fixture::{
    apply_pending_commerce_migrations, ledger_expectations, provision,
    provision_pre_costing_commerce_profile, provision_pre_private_cost_writers_profile,
    provision_prior_commerce_profile,
};
use sqlx::Row;

/// The membership seed the prior-profile side plants. Mirrors the column
/// list the baseline's own verifier seeds (`verify-foodservice.mjs`), so the
/// upgraded table's history rules see a REAL prior row, not a schema-only
/// empty shell.
const SEED_MEMBERSHIP: &str =
    "INSERT INTO archava_commerce.memberships(tenant_id, principal_id, kind, permissions, scope) \
     VALUES ('prior', 'owner', 'human', '{\"catalog.read\",\"catalog.write\"}', 'all')";

/// The membership row, read back in the projection the comparators use.
async fn membership_projection(pool: &sqlx::PgPool) -> Result<String, sqlx::Error> {
    let row =
        sqlx::query("SELECT row_to_json(m)::text AS j FROM (SELECT * FROM archava_commerce.memberships ORDER BY tenant_id, principal_id LIMIT 1) m")
            .fetch_one(pool)
            .await?;
    Ok(row.get::<String, _>("j"))
}

async fn ledger_history(
    pool: &sqlx::PgPool,
    namespace: &str,
) -> Result<Vec<(String, String)>, sqlx::Error> {
    let text = format!("SELECT id, checksum FROM {namespace}.schema_migrations ORDER BY id");
    sqlx::query_as(sqlx::AssertSqlSafe(text.as_str()))
        .fetch_all(pool)
        .await
}

async fn ledger_history_count(pool: &sqlx::PgPool, namespace: &str) -> Result<i64, sqlx::Error> {
    let text = format!("SELECT count(*) FROM {}.schema_migrations", namespace);
    let row = sqlx::query(sqlx::AssertSqlSafe(text.as_str()))
        .fetch_one(pool)
        .await?;
    Ok(row.get::<i64, _>(0))
}

/// The recorded commerce history pairs, read straight from the ledger.
async fn recorded_history(pool: &sqlx::PgPool) -> Result<Vec<(String, String)>, sqlx::Error> {
    ledger_history(pool, "archava_commerce").await
}

/// MIG-03 fresh side.
#[tokio::test]
async fn fresh_install_upgrade_passes() {
    let fixture = provision().await;
    let expected = ledger_expectations("archava_commerce");
    let history = recorded_history(&fixture.owner_pool)
        .await
        .expect("read commerce history");
    assert_eq!(
        history, expected,
        "fresh commerce history must be the frozen ledger verbatim"
    );
    for namespace in [
        "archava_identity",
        "archava_crm",
        "archava_purchasing",
        "archava_foodservice",
        "archava_provider",
        "archava_control",
    ] {
        let count = ledger_history_count(&fixture.owner_pool, namespace)
            .await
            .unwrap_or_else(|error| panic!("read {namespace} history: {error}"));
        assert!(
            count >= 1,
            "{namespace} must have recorded history on a fresh provision"
        );
        let expected = ledger_expectations(namespace);
        let history = ledger_history(&fixture.owner_pool, namespace)
            .await
            .unwrap_or_else(|error| panic!("read {namespace} pairs: {error}"));
        assert_eq!(
            history, expected,
            "{namespace} history must be the frozen ledger"
        );
    }
}

/// MIG-03 prior side + MIG-08 data preservation.
#[tokio::test]
async fn prior_profile_upgrade_passes() {
    let fixture = provision_prior_commerce_profile().await;

    // The prior profile's history is the frozen ledger MINUS its newest
    // migration — recorded as a real prefix, not a flag.
    let prior = recorded_history(&fixture.owner_pool)
        .await
        .expect("read prior history");
    let expected_full = ledger_expectations("archava_commerce");
    assert_eq!(
        prior,
        expected_full[..3].to_vec(),
        "prior profile history must stay pinned before 004_bound_approvals"
    );

    // Seed a real membership row the upgrade must preserve.
    sqlx::query(SEED_MEMBERSHIP)
        .execute(&fixture.admin_pool)
        .await
        .expect("seed prior membership (administration: superuser bypasses FORCE RLS)");
    let before = membership_projection(&fixture.admin_pool)
        .await
        .expect("read membership before upgrade");
    assert!(
        before.contains("prior"),
        "the seeded row must exist before the upgrade"
    );

    // The upgrade: apply the pending migrations as the owner role. First
    // call applies 004 and subsequent files; the second is the idempotence proof a retried
    // upgrade run must hold.
    let _ = apply_pending_commerce_migrations(&fixture.owner_pool).await;
    let _ = apply_pending_commerce_migrations(&fixture.owner_pool).await;

    // History now equals the full frozen ledger, pair for pair.
    let history = recorded_history(&fixture.owner_pool)
        .await
        .expect("read upgraded history");
    assert_eq!(
        history, expected_full,
        "upgraded history must be the whole frozen ledger"
    );

    // MIG-08's preservation rule: the seeded row is byte-for-byte the SAME
    // membership content it was; 004 adds the role/capability columns with
    // their least-privileged defaults, which is the migration's documented
    // addenda (the projection would otherwise differ in exactly those two
    // fields) — assert that fact directly so the difference is the
    // migration's default, not some other rewrite.
    let after = membership_projection(&fixture.admin_pool)
        .await
        .expect("read after upgrade");
    let before_value: serde_json::Value = serde_json::from_str(&before).expect("before is json");
    let after_value: serde_json::Value = serde_json::from_str(&after).expect("after is json");
    let mut normalized = after_value.clone();
    if let Some(object) = normalized.as_object_mut() {
        object.remove("role");
        object.remove("capability");
    }
    assert_eq!(
        normalized, before_value,
        "the preserved row must keep every pre-004 field identical"
    );
    assert_eq!(
        after_value["role"], "viewer",
        "004's least-privileged role default must hold on a preserved row"
    );
    assert_eq!(
        after_value["capability"], "assist",
        "004's least-privileged capability default must hold on a preserved row"
    );
}

#[tokio::test]
async fn existing_stock_upgrades_to_unknown_cost_without_losing_tenants() {
    let fixture = provision_pre_costing_commerce_profile().await;
    for (tenant, quantity) in [("old-a", 7_i32), ("old-b", 11_i32)] {
        let mut tx = fixture.admin_pool.begin().await.expect("seed transaction");
        sqlx::query("SELECT set_config('archava.tenant_id',$1,true)")
            .bind(tenant)
            .execute(&mut *tx)
            .await
            .expect("trigger tenant context");
        sqlx::query("INSERT INTO archava_commerce.products(tenant_id,id,sku,title,unit,track_stock,price) VALUES($1,'11111111-1111-4111-8111-111111111111','OLD','Existing stock','unit',true,99)")
            .bind(tenant).execute(&mut *tx).await.expect("old product");
        sqlx::query("INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,'old-owner','human','{stock.write}','all')")
            .bind(tenant).execute(&mut *tx).await.expect("old membership");
        sqlx::query("INSERT INTO archava_commerce.commands(tenant_id,request_id,actor_id,fingerprint) VALUES($1,'22222222-2222-4222-8222-222222222222','old-owner','historical-adjustment')")
            .bind(tenant).execute(&mut *tx).await.expect("old command");
        sqlx::query("INSERT INTO archava_commerce.movements(tenant_id,request_id,product_id,kind,quantity,reason) VALUES($1,'22222222-2222-4222-8222-222222222222','11111111-1111-4111-8111-111111111111','adjust',$2,'Pre-costing opening stock')")
            .bind(tenant).bind(quantity).execute(&mut *tx).await.expect("old stock movement");
        tx.commit().await.expect("old stock committed");
    }
    apply_pending_commerce_migrations(&fixture.owner_pool).await;
    apply_pending_commerce_migrations(&fixture.owner_pool).await;
    let layers: Vec<(String, i64, i64, Option<i64>)> = sqlx::query_as(
        "SELECT tenant_id,original_quantity,remaining_quantity,total_cost_minor FROM archava_commerce.inventory_layers ORDER BY tenant_id"
    ).fetch_all(&fixture.admin_pool).await.expect("backfilled cost layers");
    assert_eq!(
        layers,
        vec![("old-a".into(), 7, 7, None), ("old-b".into(), 11, 11, None)]
    );
    let protected: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='archava_commerce' AND c.relname IN ('stock_balances','inventory_layers','inventory_uses','inventory_return_costs') AND c.relrowsecurity AND c.relforcerowsecurity")
        .fetch_one(&fixture.admin_pool).await.expect("forced tenant RLS restored");
    assert_eq!(protected, 4);
}

/// OBS-04 + MIG-04.
#[tokio::test]
async fn backup_restore_rehearsal_passes() {
    let source_fixture = provision().await;
    sqlx::query(SEED_MEMBERSHIP)
        .execute(&source_fixture.admin_pool)
        .await
        .expect("seed membership (administration: superuser bypasses FORCE RLS)");
    let before = membership_projection(&source_fixture.admin_pool)
        .await
        .expect("read source row");

    let out_dir = source_fixture.dir().join("ops-backup");
    let release = ReleaseIdentity {
        id: "archava-local-library".to_string(),
        version: "rehearsal".to_string(),
    };
    // The capture/restore authority: ledger contents are FORCE-RLS protected
    // (every table's policy binds the owner too), so a COPY-based capture or
    // replay runs with the cluster ADMIN role — the same split the TS
    // verifier used (its capture went through the local-database
    // administration path). This is administration of the instance, not an
    // authority assertion: no tenant context is consulted, and the manifest
    // carries digests and counts, never values.
    //
    // The password note on `ConnectionSpec` still applies. Here the fixture
    // cluster is trust-auth on loopback, so `--no-password` is safe.
    let source_spec = ConnectionSpec {
        host: "127.0.0.1".to_string(),
        port: source_fixture.port,
        database: "archava_test".to_string(),
        user: "postgres".to_string(),
    };

    // S06: Refusal if ARCHAVA_BACKUP_KEY is missing
    std::env::remove_var("ARCHAVA_BACKUP_KEY");
    let missing_key_res = capture(&source_spec, &out_dir, release.clone());
    assert!(
        matches!(missing_key_res, Err(RestoreError::MissingBackupKey)),
        "capture without ARCHAVA_BACKUP_KEY must refuse with MissingBackupKey"
    );

    let test_key = [11u8; 32];
    let b64_key = encode_base64(&test_key);
    std::env::set_var("ARCHAVA_BACKUP_KEY", &b64_key);

    let manifest: BackupManifest = capture(&source_spec, &out_dir, release).expect("capture");
    assert!(
        !manifest.credential_values_printed,
        "capture must report that no credential values were printed"
    );
    assert!(
        !manifest.restore_rehearsal_verified,
        "a capture starts unverified — the restore does the verifying"
    );

    // Verify file on disk is encrypted with AES-256-GCM and starts with magic header
    let archive_path = out_dir.join(&manifest.archive.file_name);
    let raw_archive = std::fs::read(&archive_path).expect("read raw archive");
    assert!(
        raw_archive.starts_with(BACKUP_MAGIC_HEADER),
        "archive on disk must carry the AES-256-GCM header"
    );

    // Digest self-check: the manifest is persisted (the CLI does this after
    // capture; the rehearsal replicates it exactly), then read back and
    // verified against its archive.
    let manifest_path = out_dir.join("archava-backup.manifest.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).expect("serialize manifest"),
    )
    .expect("write manifest");
    let read_back = read_manifest(&manifest_path).expect("read the manifest");
    assert_eq!(read_back.archive.sha256, manifest.archive.sha256);

    // MIG-04's refusal: restoring into the capture source is refused BEFORE
    // any data moves.
    let refused_same = restore(&manifest, &out_dir, &source_spec);
    assert!(
        matches!(refused_same, Err(RestoreError::ActiveSource { .. })),
        "the restore must refuse its own capture source with the named refusal"
    );

    // MIG-04's live-form refusal: an undrained writer lease refuses the
    // restore until drained.
    let fresh_target = provision().await;
    fresh_target
        .writer_lease
        .drain(&fresh_target.runtime_pool)
        .await
        .expect("drain target serving writer before restore rehearsal");
    lease::install(&fresh_target.owner_pool)
        .await
        .expect("install lease table");
    lease::take(&fresh_target.owner_pool, "restore-guard")
        .await
        .expect("take lease");
    let undrained = refuse_undrained_target(&fresh_target.owner_pool).await;
    assert!(
        matches!(undrained, Err(RestoreError::UndrainedWriter { ref lease, .. }) if lease == "restore-guard"),
        "an undrained lease must refuse the restore by name"
    );
    lease::drain(&fresh_target.owner_pool, "restore-guard")
        .await
        .expect("drain");
    refuse_undrained_target(&fresh_target.owner_pool)
        .await
        .expect("drained target must accept");

    // The restore into the disposable target.
    let target_spec = ConnectionSpec {
        host: "127.0.0.1".to_string(),
        port: fresh_target.port,
        database: "archava_test".to_string(),
        user: "postgres".to_string(),
    };

    // S06: Restore with wrong key must fail with DecryptionFailed
    let wrong_key = [99u8; 32];
    let bad_restore = restore_with_key(&manifest, &out_dir, &target_spec, Some(&wrong_key));
    assert!(
        matches!(bad_restore, Err(RestoreError::DecryptionFailed(_))),
        "restore with invalid key must fail authentication"
    );

    restore(&manifest, &out_dir, &target_spec).expect("restore into the isolated target");
    let payments_held: bool =
        sqlx::query_scalar("SELECT held FROM archava_billing.gateway_runtime WHERE singleton")
            .fetch_one(&fresh_target.admin_pool)
            .await
            .unwrap();
    assert!(
        payments_held,
        "restored snapshots must not create new external payment requests before reconciliation"
    );
    let restored_lease: (i64, bool) = sqlx::query_as(
        "SELECT epoch,drained FROM archava_control.writer_lease WHERE lease_name='archava-serving'",
    )
    .fetch_one(&fresh_target.admin_pool)
    .await
    .unwrap();
    assert!(restored_lease.0 > source_fixture.writer_lease.epoch);
    assert!(
        restored_lease.1,
        "restore must invalidate and drain the source's process identity"
    );
    let copied_source_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with(
            source_fixture
                .writer_lease
                .connection_options(fresh_target.runtime_url().parse().unwrap()),
        )
        .await
        .unwrap();
    let stale = sqlx::query("DELETE FROM archava_identity.login_buckets WHERE false")
        .execute(&copied_source_pool)
        .await
        .unwrap_err();
    assert_eq!(
        stale.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("57P03")
    );
    copied_source_pool.close().await;
    let (new_runtime, _) =
        archava_pg_fixture::connect_writer(fresh_target.runtime_url().parse().unwrap(), 2, false)
            .await
            .unwrap();
    sqlx::query("SELECT archava_control.require_serving_writer()")
        .execute(&new_runtime)
        .await
        .unwrap();
    new_runtime.close().await;

    let after = membership_projection(&fresh_target.admin_pool)
        .await
        .expect("read restored row");
    assert_eq!(
        before, after,
        "the restored row must be byte-identical to the captured row"
    );
}

/// MIG-08: drained rollback within a compat window.
#[tokio::test]
async fn rollback_rehearsal_passes() {
    let fixture = provision().await;
    lease::install(&fixture.owner_pool)
        .await
        .expect("install lease table");

    // The current writer holds epoch 1 and writes a protected row. The
    // write runs on an ADMIN transaction (fixture administration — the
    // superuser path bypasses FORCE RLS the way every seeded row in this
    // repository's verifiers does); the FENCE is what's under test here,
    // and it lives on the lease row, not on the tenant table.
    let writer = lease::take(&fixture.owner_pool, "rollback")
        .await
        .expect("take lease");
    assert_eq!(writer.epoch, 1);
    let mut tx = fixture
        .admin_pool
        .begin()
        .await
        .expect("begin protected write");
    writer
        .assert_fresh(&mut tx)
        .await
        .expect("the current writer proves freshness on its own transaction");
    sqlx::query(SEED_MEMBERSHIP)
        .execute(&mut *tx)
        .await
        .expect("protected write under the fence");
    tx.commit().await.expect("commit protected write");

    // The handover: drain, then the NEW writer takes over — a fresh epoch,
    // with the drain flag cleared by the takeover (a live handover is a
    // takeover, not a rescue of the dead seat).
    lease::drain(&fixture.owner_pool, "rollback")
        .await
        .expect("drain");
    let new_writer = lease::take(&fixture.owner_pool, "rollback")
        .await
        .expect("take over");
    assert_eq!(new_writer.epoch, 2, "the takeover advances the epoch");
    assert!(
        new_writer.token != writer.token,
        "a new holder is a new token"
    );

    // MIG-08's refusal: the OLD seat cannot write after the drain/handover —
    // refused by the fence on the lease row (an admin transaction still
    // carries the fence check; the refusal is the fence's own class).
    let mut old_tx = fixture
        .admin_pool
        .begin()
        .await
        .expect("begin old writer tx");
    let refused = writer.assert_fresh(&mut old_tx).await;
    assert!(
        matches!(refused, Err(lease::LeaseError::StaleWriter { .. })),
        "the rolled-back writer must be refused with the stale-writer class, got {refused:?}"
    );
    drop(old_tx);

    // The protected rows are intact across the whole rehearsal (read as
    // administration, like every seeded-row count in the verifiers).
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_commerce.memberships")
        .fetch_one(&fixture.admin_pool)
        .await
        .expect("read preserved rows");
    assert_eq!(
        count, 1,
        "exactly the protected row survives the rollback rehearsal"
    );

    // The compat window is recorded on the lease and still finite.
    let window: i32 = sqlx::query_scalar(
        "SELECT compat_window_seconds FROM archava_control.writer_lease WHERE lease_name = 'rollback'",
    )
    .fetch_one(&fixture.owner_pool)
    .await
    .expect("read compat window");
    assert!(
        window > 0,
        "the recorded compat window must be a positive bound, got {window}"
    );

    // The new writer can still write (service continued under the new seat).
    let mut new_tx = fixture
        .admin_pool
        .begin()
        .await
        .expect("begin new writer tx");
    new_writer
        .assert_fresh(&mut new_tx)
        .await
        .expect("the new writer proves freshness on its own transaction");
    sqlx::query("SELECT 1")
        .execute(&mut *new_tx)
        .await
        .expect("the new writer writes under the fence");
    new_tx.commit().await.expect("commit new writer tx");
}

/// MIG-05: takeover fencing, every stale write refused.
#[tokio::test]
async fn fenced_single_writer_handover_passes() {
    let fixture = provision().await;
    lease::install(&fixture.owner_pool)
        .await
        .expect("install lease table");

    let first = lease::take(&fixture.owner_pool, "handover")
        .await
        .expect("first writer");
    assert_eq!(first.epoch, 1);
    let mut tx = fixture.owner_pool.begin().await.expect("begin");
    first
        .assert_fresh(&mut tx)
        .await
        .expect("seat one proves freshness on its own transaction");
    sqlx::query("SELECT 1")
        .execute(&mut *tx)
        .await
        .expect("seat one writes");
    tx.commit().await.expect("commit");

    let second = lease::take(&fixture.owner_pool, "handover")
        .await
        .expect("second writer");
    assert_eq!(second.epoch, 2);
    let mut stale_tx = fixture.owner_pool.begin().await.expect("begin stale tx");
    let refused = first.assert_fresh(&mut stale_tx).await;
    assert!(
        matches!(
            refused,
            Err(lease::LeaseError::StaleWriter {
                held: 1,
                current: 2,
                ..
            })
        ),
        "seat one must be refused stale at the fence, got {refused:?}"
    );
    drop(stale_tx);

    let mut tx = fixture.owner_pool.begin().await.expect("begin");
    second
        .assert_fresh(&mut tx)
        .await
        .expect("seat two proves freshness on its own transaction");
    sqlx::query("SELECT 1")
        .execute(&mut *tx)
        .await
        .expect("seat two writes");
    tx.commit().await.expect("commit");
}

#[tokio::test]
async fn private_cost_writer_upgrade_revokes_preexisting_runtime_capabilities() {
    let f = provision_pre_private_cost_writers_profile().await;
    sqlx::raw_sql("GRANT INSERT,UPDATE ON archava_commerce.inventory_layers,archava_commerce.inventory_uses TO archava_runtime;GRANT INSERT ON archava_commerce.inventory_return_costs TO archava_runtime;GRANT USAGE,SELECT ON ALL SEQUENCES IN SCHEMA archava_commerce TO archava_runtime;GRANT EXECUTE ON FUNCTION archava_commerce.cost_movement(),archava_commerce.cost_return() TO archava_runtime")
        .execute(&f.admin_pool).await.unwrap();
    apply_pending_commerce_migrations(&f.owner_pool).await;
    apply_pending_commerce_migrations(&f.owner_pool).await;
    let capabilities:(bool,bool,bool,bool,bool,bool,bool)=sqlx::query_as("SELECT has_table_privilege('archava_runtime','archava_commerce.inventory_layers','SELECT'),has_table_privilege('archava_runtime','archava_commerce.inventory_layers','INSERT'),has_table_privilege('archava_runtime','archava_commerce.inventory_uses','UPDATE'),has_table_privilege('archava_runtime','archava_commerce.inventory_return_costs','INSERT'),has_function_privilege('archava_runtime','archava_commerce.cost_movement()','EXECUTE'),has_function_privilege('archava_runtime','archava_commerce.cost_return()','EXECUTE'),has_sequence_privilege('archava_runtime','archava_commerce.inventory_layers_id_seq','USAGE')")
        .fetch_one(&f.admin_pool).await.unwrap();
    assert_eq!(
        capabilities,
        (true, false, false, false, false, false, false)
    );
}

#[tokio::test]
async fn failed_restore_keeps_the_existing_target_transactionally_intact() {
    let source = provision().await;
    sqlx::raw_sql("CREATE ROLE absent_archive_owner;CREATE SCHEMA restore_fault;CREATE TABLE restore_fault.owner_will_be_missing(value integer);ALTER TABLE restore_fault.owner_will_be_missing OWNER TO absent_archive_owner")
        .execute(&source.admin_pool).await.unwrap();
    let key = [39_u8; 32];
    let directory = source.dir().join("failing-restore");
    let source_spec = ConnectionSpec {
        host: "127.0.0.1".into(),
        port: source.port,
        database: "archava_test".into(),
        user: "postgres".into(),
    };
    let manifest = capture_with_key(
        &source_spec,
        &directory,
        ReleaseIdentity {
            id: "archava-local-library".into(),
            version: "atomic-restore-witness".into(),
        },
        Some(&key),
    )
    .unwrap();
    let target = provision().await;
    target
        .writer_lease
        .drain(&target.runtime_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO archava_identity.login_buckets(key,attempts) VALUES($1,9)")
        .bind("e".repeat(64))
        .execute(&target.admin_pool)
        .await
        .unwrap();
    let target_spec = ConnectionSpec {
        host: "127.0.0.1".into(),
        port: target.port,
        database: "archava_test".into(),
        user: "postgres".into(),
    };
    // The valid archive names a role absent on the target. pg_restore fails
    // during replay, after clean/drop statements have already begun.
    assert!(matches!(
        restore_with_key(&manifest, &directory, &target_spec, Some(&key)),
        Err(RestoreError::ReplayUnconfirmed { .. })
    ));
    let attempts: i32 =
        sqlx::query_scalar("SELECT attempts FROM archava_identity.login_buckets WHERE key=$1")
            .bind("e".repeat(64))
            .fetch_one(&target.admin_pool)
            .await
            .unwrap();
    assert_eq!(
        attempts, 9,
        "failed restore must roll back all preceding schema/data changes"
    );
}
