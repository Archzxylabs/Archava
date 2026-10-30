//! Explicit owner-role installation. The serving runtime never applies DDL or provisions authority.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::path::{Component, Path};
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("invalid installation input: {0}")]
    Invalid(&'static str),
    #[error("installation files are unavailable or do not match their recorded checksums")]
    Files,
    #[error("installation database operation failed")]
    Database(#[from] sqlx::Error),
    #[error("password derivation failed")]
    Password,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    ledgers: Vec<Ledger>,
    runtime_grants: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    namespace: String,
    lock: String,
    migrations: Vec<Migration>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Migration {
    id: String,
    path: String,
    sha256: String,
}
#[derive(Debug, Serialize)]
pub struct InstallReport {
    pub ledgers: usize,
    pub applied: usize,
    pub existing: usize,
    pub runtime_role: String,
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}
fn slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}
fn manifest(root: &Path) -> Result<Manifest, InstallError> {
    let bytes =
        std::fs::read(root.join("migrations/manifest.v1.json")).map_err(|_| InstallError::Files)?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| InstallError::Files)?;
    if manifest.schema_version != 1 || manifest.ledgers.is_empty() {
        return Err(InstallError::Files);
    }
    let mut namespaces = std::collections::BTreeSet::new();
    for ledger in &manifest.ledgers {
        if !identifier(&ledger.namespace)
            || !ledger.namespace.starts_with("archava_")
            || !namespaces.insert(&ledger.namespace)
            || ledger.migrations.is_empty()
        {
            return Err(InstallError::Files);
        }
        let mut previous = "";
        for migration in &ledger.migrations {
            if migration.id.is_empty()
                || !migration
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
                || migration.id.as_str() <= previous
                || !migration.path.starts_with("migrations/")
                || Path::new(&migration.path)
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_)))
            {
                return Err(InstallError::Files);
            }
            let path = root
                .join(&migration.path)
                .canonicalize()
                .map_err(|_| InstallError::Files)?;
            if !path.starts_with(root.canonicalize().map_err(|_| InstallError::Files)?) {
                return Err(InstallError::Files);
            }
            let sql = std::fs::read(path).map_err(|_| InstallError::Files)?;
            if format!("{:x}", Sha256::digest(&sql)) != migration.sha256 {
                return Err(InstallError::Files);
            }
            previous = &migration.id;
        }
    }
    // These are reviewed privilege statements shipped in the hashed distribution,
    // never SQL received over the application API. Only one identifier placeholder is accepted.
    if manifest.runtime_grants.is_empty()
        || manifest.runtime_grants.iter().any(|q| {
            !q.starts_with("GRANT ") || q.matches("{runtime_role}").count() != 1 || q.contains(';')
        })
    {
        return Err(InstallError::Files);
    }
    Ok(manifest)
}
pub async fn migrate(
    pool: &PgPool,
    root: &Path,
    runtime_role: &str,
) -> Result<InstallReport, InstallError> {
    if !identifier(runtime_role) {
        return Err(InstallError::Invalid("runtime role"));
    }
    let manifest = manifest(root)?;
    let mut tx = pool.begin().await?;
    let runtime=sqlx::query("SELECT r.rolsuper,r.rolbypassrls,r.rolcreaterole,r.rolcreatedb,pg_has_role(r.oid,current_user,'MEMBER') AS owner_member,pg_has_role(r.oid,d.datdba,'MEMBER') AS database_owner FROM pg_roles r CROSS JOIN pg_database d WHERE r.rolname=$1 AND d.datname=current_database()").bind(runtime_role).fetch_optional(&mut *tx).await?.ok_or(InstallError::Invalid("runtime role must already exist"))?;
    if [
        "rolsuper",
        "rolbypassrls",
        "rolcreaterole",
        "rolcreatedb",
        "owner_member",
        "database_owner",
    ]
    .iter()
    .any(|key| runtime.get::<bool, _>(*key))
    {
        return Err(InstallError::Invalid(
            "runtime role must be separate and unprivileged",
        ));
    }
    let mut report = InstallReport {
        ledgers: manifest.ledgers.len(),
        applied: 0,
        existing: 0,
        runtime_role: runtime_role.into(),
    };
    for ledger in &manifest.ledgers {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(&ledger.lock)
            .execute(&mut *tx)
            .await?;
        let schema = format!("CREATE SCHEMA IF NOT EXISTS {}", ledger.namespace);
        sqlx::query(sqlx::AssertSqlSafe(schema.as_str()))
            .execute(&mut *tx)
            .await?;
        let owner: bool = sqlx::query_scalar(
            "SELECT pg_get_userbyid(nspowner)=current_user FROM pg_namespace WHERE nspname=$1",
        )
        .bind(&ledger.namespace)
        .fetch_one(&mut *tx)
        .await?;
        if !owner {
            return Err(InstallError::Invalid(
                "migration connection must own installed schemas",
            ));
        }
        let history=format!("CREATE TABLE IF NOT EXISTS {}.schema_migrations(id text PRIMARY KEY,checksum text NOT NULL,applied_at timestamptz NOT NULL DEFAULT now())",ledger.namespace);
        sqlx::query(sqlx::AssertSqlSafe(history.as_str()))
            .execute(&mut *tx)
            .await?;
        let read = format!(
            "SELECT id,checksum FROM {}.schema_migrations ORDER BY id",
            ledger.namespace
        );
        let old: Vec<(String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(read.as_str()))
            .fetch_all(&mut *tx)
            .await?;
        if old.len() > ledger.migrations.len()
            || old
                .iter()
                .zip(&ledger.migrations)
                .any(|((id, checksum), m)| id != &m.id || checksum != &m.sha256)
        {
            return Err(InstallError::Invalid(
                "unsupported or modified migration history",
            ));
        }
        report.existing += old.len();
        for migration in ledger.migrations.iter().skip(old.len()) {
            let sql = std::fs::read_to_string(root.join(&migration.path))
                .map_err(|_| InstallError::Files)?;
            // Recheck the bytes actually executed, not a previous read of a mutable file.
            if format!("{:x}", Sha256::digest(sql.as_bytes())) != migration.sha256 {
                return Err(InstallError::Files);
            }
            sqlx::raw_sql(sqlx::AssertSqlSafe(sql.as_str()))
                .execute(&mut *tx)
                .await?;
            let record = format!(
                "INSERT INTO {}.schema_migrations(id,checksum) VALUES($1,$2)",
                ledger.namespace
            );
            sqlx::query(sqlx::AssertSqlSafe(record.as_str()))
                .bind(&migration.id)
                .bind(&migration.sha256)
                .execute(&mut *tx)
                .await?;
            report.applied += 1;
        }
    }
    sqlx::query("SELECT archava_control.install_runtime_write_guards()")
        .execute(&mut *tx)
        .await?;
    for grant in manifest.runtime_grants {
        let statement = grant.replace("{runtime_role}", runtime_role);
        sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(report)
}
/// Provision only the first human owner of a tenant. Existing users/grants are never overwritten.
pub async fn bootstrap_owner(
    pool: &PgPool,
    tenant: &str,
    principal: &str,
    username: &str,
    password: &str,
) -> Result<(), InstallError> {
    if !slug(tenant)
        || !slug(principal)
        || username.is_empty()
        || username.len() > 254
        || username != username.trim().to_lowercase()
        || !username.contains('@')
        || !(15..=128).contains(&password.len())
    {
        return Err(InstallError::Invalid(
            "tenant, principal, normalized email or password length",
        ));
    }
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|_| InstallError::Password)?;
    let mut key = [0u8; 32];
    let params = scrypt::Params::new(15, 8, 3, 32).map_err(|_| InstallError::Password)?;
    scrypt::scrypt(password.as_bytes(), &salt, &params, &mut key)
        .map_err(|_| InstallError::Password)?;
    let hash = format!(
        "scrypt$32768$8$3${}${}",
        URL_SAFE_NO_PAD.encode(salt),
        URL_SAFE_NO_PAD.encode(key)
    );
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('archava.tenant_id',$1,true)")
        .bind(tenant)
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("bootstrap-owner:{tenant}"))
        .execute(&mut *tx)
        .await?;
    let held: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM archava_owner.owner_grants WHERE tenant_id=$1)",
    )
    .bind(tenant)
    .fetch_one(&mut *tx)
    .await?;
    if held {
        return Err(InstallError::Invalid(
            "this tenant already has owner provisioning; use an explicit administration procedure",
        ));
    }
    sqlx::query("INSERT INTO archava_identity.users(id,username,password_hash,active,version) VALUES($1,$2,$3,true,1)").bind(principal).bind(username).bind(hash).execute(&mut *tx).await?;
    for (namespace, permissions) in [
        (
            "archava_commerce",
            vec![
                "catalog.read",
                "catalog.write",
                "stock.read",
                "stock.write",
                "orders.read",
                "orders.write",
                "returns.write",
                "valuation.read",
                "valuation.write",
                "audit.read",
            ],
        ),
        (
            "archava_crm",
            vec![
                "contacts.read",
                "contacts.write",
                "activities.read",
                "activities.write",
                "followups.read",
                "followups.write",
                "audit.read",
            ],
        ),
        (
            "archava_purchasing",
            vec![
                "suppliers.read",
                "suppliers.write",
                "offers.read",
                "offers.write",
                "orders.read",
                "orders.write",
                "receipts.read",
                "receipts.write",
                "audit.read",
            ],
        ),
        (
            "archava_billing",
            vec![
                "billing.read",
                "billing.write",
                "billing.settle",
                "audit.read",
            ],
        ),
        (
            "archava_support",
            vec![
                "cases.read",
                "cases.write",
                "cases.assign",
                "notes.read",
                "notes.write",
                "audit.read",
            ],
        ),
    ] {
        let query=format!("INSERT INTO {namespace}.memberships(tenant_id,principal_id,kind,permissions,scope,active) VALUES($1,$2,'human',$3,'all',true)");
        sqlx::query(sqlx::AssertSqlSafe(query.as_str()))
            .bind(tenant)
            .bind(principal)
            .bind(permissions)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE archava_commerce.memberships SET role='owner',capability='enterprise' WHERE tenant_id=$1 AND principal_id=$2").bind(tenant).bind(principal).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO archava_owner.owner_grants(tenant_id,principal_id,permissions,active) VALUES($1,$2,ARRAY['config.read','config.apply'],true)").bind(tenant).bind(principal).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// Explicit provisioning of a gateway-only service; never convert/overwrite an
/// existing human or service. The runtime is not permitted to run this command.
pub async fn bootstrap_payment_service(
    pool: &PgPool,
    tenant: &str,
    principal: &str,
) -> Result<(), InstallError> {
    if !slug(tenant) || !slug(principal) {
        return Err(InstallError::Invalid("tenant or service principal"));
    }
    let mut tx = pool.begin().await?;
    let owner:bool=sqlx::query_scalar("SELECT r.rolsuper OR pg_has_role(r.oid,n.nspowner,'MEMBER') FROM pg_roles r CROSS JOIN pg_namespace n WHERE r.rolname=current_user AND n.nspname='archava_billing'")
        .fetch_optional(&mut *tx).await?.unwrap_or(false);
    if !owner {
        return Err(InstallError::Invalid(
            "payment service provisioning requires an explicit Billing owner or administrator",
        ));
    }
    sqlx::query("SELECT set_config('archava.tenant_id',$1,true)")
        .bind(tenant)
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("bootstrap-payment:{tenant}:{principal}"))
        .execute(&mut *tx)
        .await?;
    let existing:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM archava_billing.memberships WHERE tenant_id=$1 AND principal_id=$2)").bind(tenant).bind(principal).fetch_one(&mut *tx).await?;
    if existing {
        return Err(InstallError::Invalid(
            "payment principal already exists; use an explicit administration procedure",
        ));
    }
    sqlx::query("INSERT INTO archava_billing.memberships(tenant_id,principal_id,display_name,kind,permissions,scope,active) VALUES($1,$2,'Payment gateway','service',ARRAY['billing.read','billing.write'],'all',true)")
        .bind(tenant).bind(principal).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// Administrative drain, serialized with in-flight valid writes. No token is returned.
pub async fn resume_payment_creation(pool: &PgPool) -> Result<(), InstallError> {
    let mut tx = pool.begin().await?;
    let owner:bool=sqlx::query_scalar("SELECT r.rolsuper OR pg_has_role(r.oid,n.nspowner,'MEMBER') FROM pg_roles r CROSS JOIN pg_namespace n WHERE r.rolname=current_user AND n.nspname='archava_billing'")
        .fetch_optional(&mut *tx).await?.unwrap_or(false);
    if !owner {
        return Err(InstallError::Invalid(
            "releasing the payment hold requires an explicit Billing owner or administrator",
        ));
    }
    sqlx::query("UPDATE archava_billing.gateway_runtime SET held=false WHERE singleton")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Administrative drain, serialized with in-flight valid writes. No token is returned.
pub async fn drain_writer(pool: &PgPool) -> Result<Option<i64>, InstallError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET LOCAL lock_timeout='3s'")
        .execute(&mut *tx)
        .await?;
    let owner:bool=sqlx::query_scalar("SELECT r.rolsuper OR pg_has_role(r.oid,n.nspowner,'MEMBER') FROM pg_roles r CROSS JOIN pg_namespace n WHERE r.rolname=current_user AND n.nspname='archava_control'")
        .fetch_optional(&mut *tx).await?.unwrap_or(false);
    if !owner {
        return Err(InstallError::Invalid(
            "drain-writer requires an explicit control owner or administrator",
        ));
    }
    let epoch=sqlx::query_scalar("UPDATE archava_control.writer_lease SET drained=true,updated_at=clock_timestamp() WHERE lease_name='archava-serving' RETURNING epoch")
        .fetch_optional(&mut *tx).await?;
    tx.commit().await?;
    Ok(epoch)
}
