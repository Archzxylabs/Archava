//! Real CLI installation into an empty database, with encrypted backup/restore.
use archava_pg_fixture::provision;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};
struct Sandbox(PathBuf);
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn fixture_root() -> Sandbox {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::temp_dir().join(format!("archava-native-install-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("config")).unwrap();
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(repository.join("migrations/manifest.v1.json")).unwrap(),
    )
    .unwrap();
    let mut paths = vec!["migrations/manifest.v1.json".to_string()];
    for ledger in manifest["ledgers"].as_array().unwrap() {
        for m in ledger["migrations"].as_array().unwrap() {
            paths.push(m["path"].as_str().unwrap().into());
        }
    }
    for path in paths {
        let target = dir.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let from = repository.join(target.strip_prefix(&dir).unwrap());
        std::fs::copy(from, target).unwrap();
    }
    std::fs::write(
        dir.join("config/library-release.v1.json"),
        r#"{"id":"native-test","version":"test-1"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("fixture.bin"),
        b"synthetic native profile artifact",
    )
    .unwrap();
    let digest = format!("{:x}", Sha256::digest(b"synthetic native profile artifact"));
    std::fs::write(dir.join("release-profile.json"),json!({"catalog":{"id":"native-test","version":"test-1"},"artifacts":[{"name":"fixture","version":"test-1","tarball":"fixture.bin","sha256":digest}]}).to_string()).unwrap();
    Sandbox(dir)
}
fn cli(root: &Path, owner_url: &str, args: &[&str]) -> (i32, Value) {
    let output: Output = Command::new(env!("CARGO_BIN_EXE_archava-ops"))
        .args(args)
        .env("ARCHAVA_OPS_ROOT", root)
        .env("ARCHAVA_MIGRATION_DATABASE_URL", owner_url)
        .env(
            "ARCHAVA_OWNER_PASSWORD",
            "synthetic owner password for tests",
        )
        .env("ARCHAVA_BACKUP_KEY", "07".repeat(32))
        .env("PGPASSWORD", "archava")
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("synthetic owner password"));
    let body: Value = serde_json::from_str(text.trim()).unwrap_or_else(|_| {
        panic!(
            "invalid CLI output; status {:?}, stderr {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().unwrap(), body)
}
#[tokio::test]
async fn empty_install_owner_bootstrap_replay_refusal_and_cli_disaster_recovery() {
    let f = provision().await;
    let root = fixture_root();
    for name in ["native_install", "native_restore"] {
        let sql = format!("CREATE DATABASE {name} OWNER archava_owner TEMPLATE template0");
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .execute(&f.admin_pool)
            .await
            .unwrap();
    }
    let owner = f.owner_url().replace("/archava_test", "/native_install");
    let (code, result) = cli(
        &root.0,
        &owner,
        &["migrate", "--runtime-role", "archava_runtime"],
    );
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["installation"]["migration"]["ledgers"], 24);
    assert_eq!(result["installation"]["migration"]["applied"], 36);
    let (code, result) = cli(
        &root.0,
        &owner,
        &["migrate", "--runtime-role", "archava_runtime"],
    );
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["installation"]["migration"]["applied"], 0);
    let args = [
        "bootstrap-owner",
        "--tenant",
        "acme",
        "--principal",
        "owner",
        "--username",
        "owner@archava.test",
    ];
    let (code, result) = cli(&root.0, &owner, &args);
    assert_eq!(code, 0, "{result}");
    assert_eq!(
        cli(&root.0, &owner, &args).0,
        2,
        "bootstrap must not overwrite authority"
    );
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&owner)
        .await
        .unwrap();
    let hash: String =
        sqlx::query_scalar("SELECT password_hash FROM archava_identity.users WHERE id='owner'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(hash.starts_with("scrypt$32768$8$3$"));
    assert_serving_runtime(&f, "native_install", true).await;
    let runtime = f.runtime_url().replace("/archava_test", "/native_install");
    let runtime = PgPoolOptions::new()
        .max_connections(1)
        .connect(&runtime)
        .await
        .unwrap();
    let mut tx = runtime.begin().await.unwrap();
    sqlx::query("SELECT set_config('archava.tenant_id','acme',true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    let n:i64=sqlx::query_scalar("SELECT count(*) FROM archava_commerce.memberships WHERE principal_id='owner' AND 'orders.write'=ANY(permissions)").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(n, 1);
    assert!(
        sqlx::query("UPDATE archava_identity.users SET active=false WHERE id='owner'")
            .execute(&mut *tx)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    runtime.close().await;
    let (code, drained) = cli(&root.0, &owner, &["drain-writer"]);
    assert_eq!(code, 0, "{drained}");
    assert_eq!(drained["installation"]["writer"]["drained"], true);
    let (code, refused) = cli(
        &root.0,
        &f.runtime_url().replace("/archava_test", "/native_install"),
        &["drain-writer"],
    );
    assert_eq!(
        code, 2,
        "runtime credentials must not act as the administrative drain operator"
    );
    assert_eq!(refused["ok"], false);
    let backup = root.0.join("backup");
    let port = f.port.to_string();
    let started = std::time::Instant::now();
    let (code, result) = cli(
        &root.0,
        &owner,
        &[
            "backup",
            "--port",
            &port,
            "--db",
            "native_install",
            "--user",
            "postgres",
            "--out",
            backup.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{result}");
    let encrypted = std::fs::read(backup.join("archava-backup.dump")).unwrap();
    assert!(encrypted.starts_with(b"ARCHAVA-BACKUP-AESGCM-1"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(backup.join("archava-backup.dump"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    assert!(
        !std::fs::read_dir(&backup).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".staging-")),
        "capture removes plaintext staging"
    );

    let manifest = backup.join("archava-backup.manifest.json");
    let target = archava_ops::backup::ConnectionSpec {
        host: "127.0.0.1".into(),
        port: f.port,
        database: "native_restore".into(),
        user: "postgres".into(),
    };
    let guard = archava_ops::restore_target::RestoreTargetGuard::acquire(&target)
        .await
        .unwrap();
    assert!(
        archava_ops::restore_target::RestoreTargetGuard::acquire(&target)
            .await
            .is_err(),
        "a second restore cannot own this target"
    );
    assert!(
        PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_secs(1))
            .connect(&f.runtime_url().replace("/archava_test", "/native_restore"))
            .await
            .is_err(),
        "target blocks new ordinary runtime clients during restore"
    );
    guard.release().await.unwrap();
    let busy = PgPoolOptions::new()
        .max_connections(1)
        .connect(&f.owner_url().replace("/archava_test", "/native_restore"))
        .await
        .unwrap();
    let (code, result) = cli(
        &root.0,
        &owner,
        &[
            "restore",
            "--manifest",
            manifest.to_str().unwrap(),
            "--port",
            &port,
            "--db",
            "native_restore",
            "--user",
            "postgres",
        ],
    );
    assert_eq!(code, 2, "active target must refuse: {result}");
    let limit: i32 =
        sqlx::query_scalar("SELECT datconnlimit FROM pg_database WHERE datname='native_restore'")
            .fetch_one(&f.admin_pool)
            .await
            .unwrap();
    assert_eq!(limit, -1, "refusal restores connection policy");
    busy.close().await;

    let (code, result) = cli(
        &root.0,
        &owner,
        &[
            "restore",
            "--manifest",
            manifest.to_str().unwrap(),
            "--port",
            &port,
            "--db",
            "native_restore",
            "--user",
            "postgres",
        ],
    );
    assert_eq!(code, 0, "{result}");
    let restore = PgPoolOptions::new()
        .max_connections(1)
        .connect(&f.owner_url().replace("/archava_test", "/native_restore"))
        .await
        .unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM archava_identity.users WHERE id='owner'")
            .fetch_one(&restore)
            .await
            .unwrap();
    assert_eq!(count, 1);
    assert_serving_runtime(&f, "native_restore", false).await;
    assert!(
        started.elapsed().as_secs() < 300,
        "local small-fixture rehearsal target"
    );
    // Refuse altered installation SQL before any DDL; existing release guard is separate.
    let migration = root
        .0
        .join("migrations/commerce-pg/001_catalog_orders_stock.sql");
    std::fs::write(migration, b"SELECT 1;").unwrap();
    assert_eq!(
        cli(
            &root.0,
            &owner,
            &["migrate", "--runtime-role", "archava_runtime"]
        )
        .0,
        2
    );
    pool.close().await;
    restore.close().await;
}

async fn assert_serving_runtime(f: &archava_pg_fixture::Fixture, database: &str, seed: bool) {
    use archava_host::{router, AllReadiness, AppState, Ledger};
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let options = f
        .runtime_url()
        .replace("/archava_test", &format!("/{database}"))
        .parse()
        .unwrap();
    let (pool, _writer) = archava_pg_fixture::connect_writer(options, 2, false)
        .await
        .unwrap();
    let additional = f.erp_readiness();
    let state = AppState::try_new_all(
        pool.clone(),
        Ledger {
            namespace: "archava_commerce",
            ledger: "Commerce",
            tables: &[
                "memberships",
                "products",
                "stock_balances",
                "orders",
                "order_lines",
                "allocations",
                "commands",
                "movements",
                "returns",
                "audit",
                "approvals",
                "inventory_layers",
                "inventory_uses",
                "inventory_return_costs",
            ],
        },
        AllReadiness {
            commerce: &f.commerce_ledger,
            identity: &f.identity_ledger,
            crm: &f.crm_ledger,
            owner: &f.owner_ledger,
            purchasing: &f.purchasing_ledger,
            additional_erp: &additional,
        },
        &[7u8; 32],
    )
    .await
    .unwrap();
    let app = router(state);
    let response=app.clone().oneshot(Request::builder().method("POST").uri("/login").header("content-type","application/json").header("x-archava-client","cli").body(Body::from(json!({"username":"owner@archava.test","password":"synthetic owner password for tests"}).to_string())).unwrap()).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        status,
        StatusCode::OK,
        "fresh/restored runtime login: {body}"
    );
    assert!(body["token"].is_string());
    async fn request(
        app: &axum::Router,
        session: &Value,
        method: &str,
        path: &str,
        document: Value,
    ) -> Value {
        let result = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("content-type", "application/json")
                    .header("x-archava-client", "cli")
                    .header(
                        "authorization",
                        format!("Bearer {}", session["token"].as_str().unwrap()),
                    )
                    .header("x-archava-csrf", session["csrf"].as_str().unwrap())
                    .body(Body::from(document.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = result.status();
        let bytes = to_bytes(result.into_body(), 1024 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }
    if seed {
        let product=request(&app,&body,"POST","/erp/commerce/commands",json!({"tenant":"acme","requestId":uuid::Uuid::new_v4(),"command":{"operation":"product.create","sku":"RESTORE-COST","title":"Restored inventory basis","unit":"unit","trackStock":true,"price":10}})).await;
        request(&app,&body,"POST","/erp/commerce/commands",json!({"tenant":"acme","requestId":uuid::Uuid::new_v4(),"command":{"operation":"stock.adjust","productId":product["receipt"]["id"],"quantity":2,"unitCostMinor":3,"reason":"Known opening basis"}})).await;
    }
    let view = request(
        &app,
        &body,
        "GET",
        "/erp/commerce/workspace?tenant=acme",
        Value::Null,
    )
    .await;
    assert_eq!(view["products"][0]["quantity"], 2);
    assert_eq!(
        view["products"][0]["inventoryValueMinor"], "6",
        "Acquisition layers and their runtime grants survive restore"
    );
    pool.close().await;
}

#[tokio::test]
async fn unconfirmed_cli_replay_keeps_the_target_closed_and_redacts_replay_details() {
    use archava_ops::backup::{capture_with_key, ConnectionSpec, ReleaseIdentity};
    let source = provision().await;
    sqlx::raw_sql("CREATE ROLE private_archive_owner_value;CREATE SCHEMA restore_fault;CREATE TABLE restore_fault.extra(value integer);ALTER TABLE restore_fault.extra OWNER TO private_archive_owner_value")
        .execute(&source.admin_pool).await.unwrap();
    let root = fixture_root();
    let directory = root.0.join("failed-replay");
    let spec = ConnectionSpec {
        host: "127.0.0.1".into(),
        port: source.port,
        database: "archava_test".into(),
        user: "postgres".into(),
    };
    let manifest = capture_with_key(
        &spec,
        &directory,
        ReleaseIdentity {
            id: "native-test".into(),
            version: "test-1".into(),
        },
        Some(&[7_u8; 32]),
    )
    .unwrap();
    let manifest_path = directory.join("archava-backup.manifest.json");
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let target = provision().await;
    target
        .writer_lease
        .drain(&target.runtime_pool)
        .await
        .unwrap();
    target.single_pool.close().await;
    target.runtime_pool.close().await;
    target.owner_pool.close().await;
    target.admin_pool.close().await;
    let port = target.port.to_string();
    let (code, result) = cli(
        &root.0,
        &target.owner_url(),
        &[
            "restore",
            "--manifest",
            manifest_path.to_str().unwrap(),
            "--port",
            &port,
            "--db",
            "archava_test",
            "--user",
            "postgres",
        ],
    );
    assert_eq!(code, 2, "{result}");
    assert!(result["refused"].as_str().unwrap().contains("isolated"));
    assert!(
        !result.to_string().contains("private_archive_owner_value"),
        "replay stderr is private"
    );
    let maintenance = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::postgres::PgConnectOptions::new()
                .host("127.0.0.1")
                .port(target.port)
                .username("postgres")
                .database("postgres"),
        )
        .await
        .unwrap();
    let limit: i32 =
        sqlx::query_scalar("SELECT datconnlimit FROM pg_database WHERE datname='archava_test'")
            .fetch_one(&maintenance)
            .await
            .unwrap();
    assert_eq!(limit, 0, "an unconfirmed replay must not reopen the target");
    assert!(PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(2))
        .connect(&target.runtime_url())
        .await
        .is_err());
    sqlx::query("ALTER DATABASE archava_test CONNECTION LIMIT -1")
        .execute(&maintenance)
        .await
        .unwrap();
    maintenance.close().await;
}
