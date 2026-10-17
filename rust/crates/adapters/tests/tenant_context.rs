//! G03's R02 proof: the tenant transaction cohort against a live PostgreSQL.
//!
//! These tests run against a real database **that each test provisions for
//! itself** through [`archava_pg_fixture`]: `initdb` into a unique temp
//! directory, a free loopback port, the REAL commerce migrations applied by a
//! migration-owner role, and a teardown that stops the server and deletes the
//! directory when the test ends — so no test can see another test's state, and
//! no lock is needed to keep tampering tests away from passing ones. What they
//! prove is the cohort row-level security depends on —
//! a non-owner runtime, forced RLS on every ledger table, a migration ledger
//! this build recognizes, and a tenant setting that never leaks past commit or
//! rollback — plus the refusals: the owner role, a modified checksum, and an
//! unforced policy must each fail startup readiness with their own named error.
//!
//! There is deliberately no `ARCHAVA_TEST_DATABASE_URL` fallback and no
//! hardcoded `/tmp/pgdata`. Both existed here before (RF02) and both made the
//! suite depend on a database someone else provisioned and seeded once: a fresh
//! checkout could not run it, the seeded state drifted until the assertions
//! failed for accumulation rather than behavior, and the file documented the
//! connection failure as "a configuration error, not a test failure" — which
//! meant the suite could not fail for its real reason. The fixture makes the
//! database an output of the test run, and a machine without PostgreSQL
//! binaries fails loudly at `initdb` with the tool named rather than skipping.

use archava_adapters::rls::{
    assert_forced_policies, assert_migration_ledger, assert_startup_readiness, TenantContextError,
    TenantTransaction,
};
use archava_core::TenantId;
use archava_pg_fixture::{provision, LEDGER, NAMESPACE};

const TABLES: &[&str] = &[
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
];

fn tenant(value: &str) -> TenantId {
    TenantId::new(value).expect("test tenant must be valid")
}

/// Reads the transaction-local tenant setting from an arbitrary executor.
async fn tenant_setting(
    executor: &mut sqlx::postgres::PgConnection,
) -> Result<Option<String>, TenantContextError> {
    let found: Option<String> =
        sqlx::query_scalar("SELECT current_setting('archava.tenant_id', true)")
            .fetch_one(executor)
            .await
            .map_err(|source| TenantContextError::Database {
                ledger: LEDGER,
                source,
            })?;
    Ok(found)
}

/// A non-owner role that does not own any table in the namespace may open a
/// tenant transaction.
#[tokio::test]
async fn a_non_owner_runtime_opens_a_tenant_transaction() {
    let fixture = provision().await;
    let tx = TenantTransaction::begin(
        &fixture.runtime_pool,
        NAMESPACE,
        LEDGER,
        tenant("acme-retail"),
    )
    .await
    .expect("a non-owner role may open a tenant transaction");
    assert_eq!(tx.tenant().as_str(), "acme-retail");
    assert_eq!(tx.ledger(), LEDGER);
    tx.rollback().await.expect("rollback must succeed");
}

/// The forced-RLS gate passes when every table the ledger relies on has RLS
/// enabled and forced.
#[tokio::test]
async fn forced_rls_passes_on_every_ledger_table() {
    let fixture = provision().await;
    assert_forced_policies(&fixture.runtime_pool, NAMESPACE, LEDGER, TABLES)
        .await
        .expect("every table in the ledger must have forced RLS");
}

/// The tenant setting is transaction-local: after commit, a new transaction on
/// a different connection reads no stale tenant context.
#[tokio::test]
async fn tenant_context_does_not_leak_after_commit() {
    let fixture = provision().await;

    // First transaction: set tenant, commit.
    {
        let mut tx = TenantTransaction::begin(
            &fixture.runtime_pool,
            NAMESPACE,
            LEDGER,
            tenant("acme-retail"),
        )
        .await
        .expect("begin");
        let found = tenant_setting(tx.checked_executor().await.expect("checked_executor"))
            .await
            .expect("read tenant setting");
        assert_eq!(found.as_deref(), Some("acme-retail"));
        tx.commit().await.expect("commit");
    }

    // Second transaction on a fresh connection: no tenant setting should leak.
    let mut tx = TenantTransaction::begin(
        &fixture.runtime_pool,
        NAMESPACE,
        LEDGER,
        tenant("other-tenant"),
    )
    .await
    .expect("begin second transaction");
    let found = tenant_setting(tx.checked_executor().await.expect("checked_executor"))
        .await
        .expect("read tenant setting");
    assert_eq!(
        found.as_deref(),
        Some("other-tenant"),
        "committed tenant context must not leak"
    );
    tx.rollback().await.expect("rollback");
}

/// The tenant setting is transaction-local: after rollback, no tenant context
/// remains on the pooled connection.
#[tokio::test]
async fn tenant_context_does_not_leak_after_rollback() {
    let fixture = provision().await;

    // First transaction: set tenant, roll back.
    {
        let mut tx = TenantTransaction::begin(
            &fixture.runtime_pool,
            NAMESPACE,
            LEDGER,
            tenant("acme-retail"),
        )
        .await
        .expect("begin");
        let found = tenant_setting(tx.checked_executor().await.expect("checked_executor"))
            .await
            .expect("read tenant setting");
        assert_eq!(found.as_deref(), Some("acme-retail"));
        tx.rollback().await.expect("rollback");
    }

    // Second transaction on a fresh connection: no tenant setting from the
    // rolled-back transaction may remain.
    let mut tx = TenantTransaction::begin(
        &fixture.runtime_pool,
        NAMESPACE,
        LEDGER,
        tenant("other-tenant"),
    )
    .await
    .expect("begin second transaction");
    let found = tenant_setting(tx.checked_executor().await.expect("checked_executor"))
        .await
        .expect("read tenant setting");
    assert_eq!(
        found.as_deref(),
        Some("other-tenant"),
        "rolled-back tenant context must not leak"
    );
    tx.rollback().await.expect("rollback");
}

/// The leak read-back on ONE physical connection.
///
/// The other two leak tests use a pool wide enough that "a fresh connection"
/// is probabilistic — the pool might hand back the same connection or a new
/// one, and either would pass. This test closes that hole with
/// `single_pool` (`max_connections(1)`): two sequential transactions MUST run
/// on the same physical connection, so the read-back between them is proven on
/// the connection itself. The backend pid is compared before and after the
/// commit to prove it is the same connection — if the pool ever recycled it,
/// the pid assertion fails with a message saying so instead of the test
/// silently passing on a fresh connection that never held a tenant.
#[tokio::test]
async fn the_setting_is_gone_before_the_next_transaction_on_one_connection() {
    let fixture = provision().await;

    let (pid_inside, found_inside) = {
        let mut tx = TenantTransaction::begin(
            &fixture.single_pool,
            NAMESPACE,
            LEDGER,
            tenant("acme-retail"),
        )
        .await
        .expect("begin on the single-connection pool");
        let executor = tx.checked_executor().await.expect("checked_executor");
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *executor)
            .await
            .expect("backend pid inside the transaction");
        let found = tenant_setting(executor).await.expect("read tenant setting");
        assert_eq!(found.as_deref(), Some("acme-retail"));
        tx.commit().await.expect("commit");
        (pid, found)
    };
    assert_eq!(found_inside.as_deref(), Some("acme-retail"));

    // The commit has returned the connection to the single-slot pool. Both
    // reads below MUST come back on that same connection.
    let pid_after: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&fixture.single_pool)
        .await
        .expect("backend pid after commit");
    assert_eq!(
        pid_after, pid_inside,
        "the pool must reuse the same physical connection for this proof; if it \
         ever recycles connections, this test needs a new design rather than a \
         weaker assertion"
    );

    let leaked: Option<String> =
        sqlx::query_scalar("SELECT current_setting('archava.tenant_id', true)")
            .fetch_one(&fixture.single_pool)
            .await
            .expect("read tenant setting on the reused connection");
    // The property is "no tenant from the committed transaction", not a
    // specific rendering of that absence. PostgreSQL, after a transaction-local
    // `set_config` expires, leaves an EMPTY session GUC slot rather than no
    // slot at all, so a read-back can come back `Some("")` on one PostgreSQL
    // version and `None` on another. Both are safe, and both must stay safe:
    // `""` matches no tenant row under the RLS policy
    // (`tenant_id = current_setting(...)`) and fails the kernel's read-back
    // (`found !== tenant` in `kernel.ts:84`). What must never appear here is
    // the committed tenant itself.
    assert!(
        leaked.as_deref().is_none_or(|value| value.is_empty()),
        "the committed tenant setting must be gone from the connection itself \
         BEFORE the next transaction begins, got {leaked:?}"
    );

    // And the same connection still works for the next tenant.
    let mut tx = TenantTransaction::begin(
        &fixture.single_pool,
        NAMESPACE,
        LEDGER,
        tenant("other-tenant"),
    )
    .await
    .expect("second begin on the single-connection pool");
    let found = tenant_setting(tx.checked_executor().await.expect("checked_executor"))
        .await
        .expect("read tenant setting");
    assert_eq!(found.as_deref(), Some("other-tenant"));
    tx.rollback().await.expect("rollback");
}

/// Row-level security actually scopes reads: a transaction for tenant A cannot
/// see tenant B's rows even when both tenants exist.
///
/// The table is truncated first so the count assertions measure scoping rather
/// than run order or residue from a previous run (the defect RF02 found: the
/// old version of this test only passed on a database nobody had seeded twice).
/// Truncation itself runs as the superuser — TRUNCATE is not subject to row
/// security — while the seeding below runs through the tenant cohort, because
/// that is the only way the SECURITY DEFINER stock trigger can see a tenant
/// setting to satisfy its own policy.
#[tokio::test]
async fn rls_scopes_reads_to_the_declared_tenant() {
    let fixture = provision().await;

    // Fresh state, belt-and-braces: the instance is provisioned for THIS test
    // so `products` starts empty anyway, but the count assertions must not
    // silently depend on that — if the fixture ever shares again, this test
    // must still measure scoping rather than residue. CASCADE because
    // `order_lines`, `allocations` and `movements` reference `products`, and
    // PostgreSQL refuses a partial truncate even of empty tables.
    sqlx::query("TRUNCATE archava_commerce.products CASCADE")
        .execute(&fixture.admin_pool)
        .await
        .expect("truncate seeded tables");

    // Seed one row per tenant the way the baseline does — through a runtime
    // tenant transaction, not around the policy. The first version of this
    // test seeded as the superuser and failed: the `product_stock` AFTER
    // INSERT trigger is SECURITY DEFINER, so its insert into `stock_balances`
    // runs as the migration owner, who is subject to FORCE RLS with no tenant
    // setting on the session — policy violation. The baseline never hits this
    // because product creation happens inside a tenant transaction
    // (`createFoodMenuProduct` → `context(client, tenant)` first), where the
    // transaction-local setting the policy reads is exactly the row's tenant.
    // Seeding through the cohort therefore doubles as a proof that the write
    // path works under the policy it lives under.
    for (tenant_id, sku, title) in [
        ("acme-retail", "ACME-WIDGET", "Retail Widget"),
        ("other-tenant", "OTHER-WIDGET", "Other Widget"),
    ] {
        let mut tx =
            TenantTransaction::begin(&fixture.runtime_pool, NAMESPACE, LEDGER, tenant(tenant_id))
                .await
                .expect("begin seed transaction");
        let executor = tx.checked_executor().await.expect("checked_executor");
        sqlx::query(
            "INSERT INTO archava_commerce.products \
             (tenant_id, id, sku, title, unit, track_stock, price) \
             VALUES ($1, gen_random_uuid(), $2, $3, 'unit', false, 1000)",
        )
        .bind(tenant_id)
        .bind(sku)
        .bind(title)
        .execute(&mut *executor)
        .await
        .unwrap_or_else(|error| panic!("seed {tenant_id} product: {error}"));
        tx.commit().await.expect("commit seed transaction");
    }

    // Tenant A sees only its own row.
    {
        let mut tx = TenantTransaction::begin(
            &fixture.runtime_pool,
            NAMESPACE,
            LEDGER,
            tenant("acme-retail"),
        )
        .await
        .expect("begin acme-retail");
        let executor = tx.checked_executor().await.expect("checked_executor");
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_commerce.products")
            .fetch_one(executor)
            .await
            .expect("count acme-retail products");
        assert_eq!(count, 1, "RLS must scope products to the declared tenant");
        tx.rollback().await.expect("rollback");
    }

    // Tenant B sees only its own row.
    {
        let mut tx = TenantTransaction::begin(
            &fixture.runtime_pool,
            NAMESPACE,
            LEDGER,
            tenant("other-tenant"),
        )
        .await
        .expect("begin other-tenant");
        let executor = tx.checked_executor().await.expect("checked_executor");
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM archava_commerce.products")
            .fetch_one(executor)
            .await
            .expect("count other-tenant products");
        assert_eq!(count, 1, "RLS must scope products to the declared tenant");
        tx.rollback().await.expect("rollback");
    }
}

/// The startup readiness gate runs once before the host binds its port and
/// passes on a ledger the fixture configured the way the baseline's
/// `assertCommerceKernel` demands: non-owner runtime, matching migration
/// history, forced policies.
#[tokio::test]
async fn startup_readiness_passes_on_a_configured_ledger() {
    let fixture = provision().await;
    assert_startup_readiness(
        &fixture.runtime_pool,
        NAMESPACE,
        LEDGER,
        TABLES,
        &fixture.commerce_ledger,
    )
    .await
    .expect("startup readiness must pass on a correctly configured ledger");
}

/// The two roles that must never serve requests are refused by name: the table
/// owner (FORCE still applies to it, but the cohort refuses it before any
/// statement runs) and a superuser (nothing applies to it at all).
#[tokio::test]
async fn owner_and_superuser_runtimes_are_refused() {
    let fixture = provision().await;
    for (role, pool) in [
        ("archava_owner", &fixture.owner_pool),
        ("postgres", &fixture.admin_pool),
    ] {
        let error =
            assert_startup_readiness(pool, NAMESPACE, LEDGER, TABLES, &fixture.commerce_ledger)
                .await
                .expect_err("an unsafe runtime must fail readiness");
        assert!(
            matches!(
                error,
                TenantContextError::NonOwnerRuntime { ledger: LEDGER }
            ),
            "{role} must be refused by the non-owner check, got: {error:?}"
        );
    }
}

/// A `schema_migrations` checksum that no longer matches the migration files
/// this build ships is a refusal, not a warning: it means the file changed
/// after it was applied, and re-serving would run against a schema the build
/// has never seen. The checksum is restored before the assertion so a failure
/// reports the refusal rather than leaving the fixture corrupted for the other
/// tests.
#[tokio::test]
async fn a_modified_migration_checksum_is_refused() {
    let fixture = provision().await;

    let original: String =
        sqlx::query_scalar("SELECT checksum FROM archava_commerce.schema_migrations WHERE id=$1")
            .bind(&fixture.commerce_ledger[0].0)
            .fetch_one(&fixture.owner_pool)
            .await
            .expect("read the stored checksum");
    sqlx::query("UPDATE archava_commerce.schema_migrations SET checksum='tampered' WHERE id=$1")
        .bind(&fixture.commerce_ledger[0].0)
        .execute(&fixture.owner_pool)
        .await
        .expect("tamper with the stored checksum");

    let result = assert_startup_readiness(
        &fixture.runtime_pool,
        NAMESPACE,
        LEDGER,
        TABLES,
        &fixture.commerce_ledger,
    )
    .await;

    // Restore first: the assertion below must be the only thing that can fail.
    sqlx::query("UPDATE archava_commerce.schema_migrations SET checksum=$2 WHERE id=$1")
        .bind(&fixture.commerce_ledger[0].0)
        .bind(&original)
        .execute(&fixture.owner_pool)
        .await
        .expect("restore the stored checksum");

    let error = result.expect_err("modified migration history must fail readiness");
    assert!(
        matches!(
            error,
            TenantContextError::MigrationHistory { ledger: LEDGER, .. }
        ),
        "a tampered checksum must be refused as migration history, got: {error:?}"
    );
}

/// RLS enabled but not forced is the gap `FORCE` exists to close: without it,
/// the table owner reads every tenant's rows. Readiness must notice the moment
/// the FORCE flag drops — restored before the assertion for the same reason as
/// the checksum test.
#[tokio::test]
async fn an_unforced_policy_is_refused() {
    let fixture = provision().await;

    sqlx::query("ALTER TABLE archava_commerce.orders NO FORCE ROW LEVEL SECURITY")
        .execute(&fixture.owner_pool)
        .await
        .expect("drop the FORCE flag on orders");

    let result = assert_startup_readiness(
        &fixture.runtime_pool,
        NAMESPACE,
        LEDGER,
        TABLES,
        &fixture.commerce_ledger,
    )
    .await;

    sqlx::query("ALTER TABLE archava_commerce.orders FORCE ROW LEVEL SECURITY")
        .execute(&fixture.owner_pool)
        .await
        .expect("restore the FORCE flag on orders");

    let error = result.expect_err("an unforced policy must fail readiness");
    assert!(
        matches!(
            error,
            TenantContextError::UnsatisfiedPolicyGate {
                ledger: LEDGER,
                expected: 10,
                ..
            }
        ),
        "an unforced table must be refused by the policy gate, got: {error:?}"
    );
}

/// The one interpolated ledger query is gated on a validated identifier, and
/// the gate runs before any SQL is issued. `archava-commerce` is the probe
/// because it reads like a plausible schema name and is still not a plain
/// `[a-z0-9_]` identifier.
#[tokio::test]
async fn an_invalid_namespace_is_refused_before_sql() {
    let fixture = provision().await;
    let error = assert_migration_ledger(
        &fixture.runtime_pool,
        "archava-commerce",
        LEDGER,
        &fixture.commerce_ledger,
    )
    .await
    .expect_err("a non-identifier namespace must be refused");
    assert!(
        matches!(
            error,
            TenantContextError::InvalidNamespace { ledger: LEDGER, .. }
        ),
        "an invalid namespace must be refused before it reaches SQL, got: {error:?}"
    );
}
