//! P-READINESS-PG host-level proof (RA02).
//!
//! Produces the `P-READINESS-PG` manifest entries that concern serving:
//! - `missing_forced_policy_readiness_check` / `modified_checksum_readiness_check` /
//!   `owner_runtime_readiness_check`: each broken-fixture condition must make the
//!   ONLY public constructor, `AppState::try_new`, return an `Err` — so an unready
//!   host cannot be constructed, let alone served.
//! - `host_refuses_to_serve_before_readiness_check` (readiness variant): with a
//!   ready fixture, `try_new` succeeds and the constructed router answers over
//!   real HTTP; with a broken fixture the same construction path fails, so no
//!   listener exists to answer.
//!
//! Recorded by `tools/rust/verify.mjs` step `host-readiness` from the
//! `archava-host` test stdout (`test readiness::…` lines are the witnesses).

#![cfg(unix)]

use archava_adapters::rls::TenantContextError;
use archava_host::{AppState, AuthorityError, Ledger};
use archava_pg_fixture::{provision, LEDGER, NAMESPACE};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

/// The commerce ledger's tenant-scoped tables — the count `assert_startup_readiness`
/// expects to find forced, so a table dropped from the gate is a shortfall not a
/// pass (mirrors `adapters/tests/tenant_context.rs`).
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
    "approvals",
];

/// A test identity key — 32 bytes, the minimum the identity gate accepts.
const IDENTITY_KEY: &[u8] = &[7u8; 32];

fn commerce_ledger() -> Ledger {
    Ledger {
        namespace: NAMESPACE,
        ledger: LEDGER,
        tables: TABLES,
    }
}

/// Assert `try_new` refuses the fixture — the readiness gate fired before any
/// state existed, so there is structurally no router to serve.
async fn assert_wont_serve(
    pool: sqlx::PgPool,
    expected: &[(String, String)],
    identity_ledger: &[(String, String)],
    condition: &str,
) {
    let error = AppState::try_new(
        pool,
        commerce_ledger(),
        expected,
        identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect_err(condition);
    let message = error.to_string();
    assert!(
        message.contains("non-owner")
            || message.contains("migration history")
            || message.contains("forced"),
        "expected a readiness refusal for {condition}, got: {message}"
    );
    // No AppState => no Router => nothing binds. Construction never completed.
    println!("readiness refusal [{condition}]: {message}");
}

#[tokio::test]
async fn ready_fixture_constructs_state_and_router_serves() {
    let fixture = provision().await;
    let state = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect("a ready fixture must construct serving state");
    let app = archava_host::router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    println!("ready host serves /healthz: {}", response.status());
}

#[tokio::test]
async fn missing_forced_policy_readiness_check_blocks_construction() {
    let fixture = provision().await;
    sqlx::query("ALTER TABLE archava_commerce.orders NO FORCE ROW LEVEL SECURITY")
        .execute(&fixture.owner_pool)
        .await
        .expect("drop the FORCE flag on orders");

    assert_wont_serve(
        fixture.runtime_pool.clone(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        "missing forced policy",
    )
    .await;

    // Restore so the fixture's teardown report is about this test only.
    sqlx::query("ALTER TABLE archava_commerce.orders FORCE ROW LEVEL SECURITY")
        .execute(&fixture.owner_pool)
        .await
        .expect("restore the FORCE flag on orders");
}

#[tokio::test]
async fn modified_checksum_readiness_check_blocks_construction() {
    let fixture = provision().await;
    sqlx::query("UPDATE archava_commerce.schema_migrations SET checksum='tampered' WHERE id=$1")
        .bind(&fixture.commerce_ledger[0].0)
        .execute(&fixture.owner_pool)
        .await
        .expect("tamper with the stored checksum");

    assert_wont_serve(
        fixture.runtime_pool.clone(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        "modified migration checksum",
    )
    .await;
}

#[tokio::test]
async fn owner_runtime_readiness_check_blocks_construction() {
    let fixture = provision().await;
    // The table owner itself connecting: readiness must refuse the runtime
    // before any request is served — same refusal the baseline's
    // `assertCommerceKernel` raises for an owner-role connection.
    assert_wont_serve(
        fixture.owner_pool.clone(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        "owner runtime",
    )
    .await;
}

/// The negative tests above each returned `Err`; this one proves the error they
/// hit is the readiness gate itself and not an incidental failure (e.g. a typo'd
/// table name would also make construction fail).
#[tokio::test]
async fn readiness_refusal_is_the_named_gate() {
    let fixture = provision().await;
    sqlx::query("UPDATE archava_commerce.schema_migrations SET checksum='tampered' WHERE id=$1")
        .bind(&fixture.commerce_ledger[0].0)
        .execute(&fixture.owner_pool)
        .await
        .expect("tamper with the stored checksum");

    let error = AppState::try_new(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
    )
    .await
    .expect_err("tampered history must refuse construction");
    assert!(
        matches!(
            error,
            AuthorityError::Context(TenantContextError::MigrationHistory { ledger: LEDGER, .. })
        ),
        "the refusal must be the migration-history gate, got: {error:?}"
    );
}

#[tokio::test]
async fn supplier_document_tables_are_covered_by_the_serving_readiness_gate() {
    let fixture = provision().await;
    for table in [
        "supplier_documents",
        "supplier_document_versions",
        "supplier_document_allocations",
    ] {
        let sql = format!("ALTER TABLE archava_purchasing.{table} NO FORCE ROW LEVEL SECURITY");
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .execute(&fixture.owner_pool)
            .await
            .unwrap();
        let error = AppState::try_new_erp(
            fixture.runtime_pool.clone(),
            commerce_ledger(),
            &fixture.commerce_ledger,
            &fixture.identity_ledger,
            IDENTITY_KEY,
            &fixture.purchasing_ledger,
            &fixture.erp_readiness(),
        )
        .await
        .expect_err("every mounted supplier document table must be forced before serving");
        assert!(error.to_string().contains("forced"), "{table}: {error}");
        let sql = format!("ALTER TABLE archava_purchasing.{table} FORCE ROW LEVEL SECURITY");
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .execute(&fixture.owner_pool)
            .await
            .unwrap();
    }
    AppState::try_new_erp(
        fixture.runtime_pool.clone(),
        commerce_ledger(),
        &fixture.commerce_ledger,
        &fixture.identity_ledger,
        IDENTITY_KEY,
        &fixture.purchasing_ledger,
        &fixture.erp_readiness(),
    )
    .await
    .expect("restored FORCE RLS permits the same profile");
}
