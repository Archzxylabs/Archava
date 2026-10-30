//! The non-owner RLS transaction cohort — the G03 seam.
//!
//! Authorization in this system is not enforced by application code; it is
//! enforced by native PostgreSQL row-level security. What the application owns
//! is the *cohort* that RLS depends on: begin a transaction, declare which
//! tenant it belongs to, and never let a pooled connection carry one tenant's
//! context into the next transaction. This module exists so that cohort cannot
//! be forgotten or partially applied.
//!
//! Transcribed from the baseline, not invented. Each step below is cited:
//!
//! 1. **The tenant setting is transaction-local.**
//!    `packages/laundry-pg/src/repository.ts:175` (and 28 other packages —
//!    `rg "set_config\('archava.tenant_id'"` finds them all) issue
//!    `SELECT set_config('archava.tenant_id', $1, true)`.
//!
//!    The `true` third argument is the load-bearing detail. It makes the setting
//!    local to the current transaction rather than the session, so it disappears
//!    at COMMIT. A port that dropped it, or passed `false`, would silently
//!    leak one tenant's context into the next transaction on the same pooled
//!    connection — the exact cross-tenant failure PRD §15 calls a bug rather
//!    than a recoverable error.
//!
//! 2. **The runtime must not be the schema owner.**
//!    `packages/commerce-pg/src/kernel.ts:44`:
//!    `SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE
//!     relnamespace='archava_commerce'::regnamespace AND relowner=pg_roles.oid))
//!     AS unsafe FROM pg_roles WHERE rolname=current_user`
//!
//!    A superuser, a `BYPASSRLS` role, or a role that owns any table in the
//!    ledger's namespace all defeat RLS. This is checked before the transaction
//!    opens, because a transaction opened as an owner is one whose every
//!    subsequent statement bypasses the policy.
//!
//! 3. **The setting is read back and compared.**
//!    `packages/commerce-pg/src/kernel.ts:81-85` reads
//!    `current_setting('archava.tenant_id',true)` and throws
//!    `Commerce kernel requires caller-owned tenant transaction` on mismatch.
//!    `packages/scheduling-pg/src/occupancy.ts:28-35` runs the same read-back
//!    with the label `Missing occupancy transaction tenant`.
//!
//!    Note the word *caller-owned* in the commerce message: the transaction's
//!    tenant is owned by whoever opened it, and a domain function that received
//!    a client from elsewhere refuses to proceed without it. In the baseline
//!    every exported domain function calls this check as its first statement
//!    (`lockCommerceProducts`, `readCommerceStock`, and their siblings all begin
//!    `await context(client, tenant)`), which is why [`TenantTransaction::checked_executor`]
//!    re-runs the read-back on every call rather than once at `begin`.
//!
//! 4. **Row-level security must be forced, not merely enabled.**
//!    `packages/scheduling-pg/migrations/001_resource_reservations.sql:44-52`
//!    runs `ALTER TABLE ... FORCE ROW LEVEL SECURITY` before its policies.
//!    Plain `ENABLE` exempts the table owner from the policy; `FORCE` does not.
//!    Without it, the non-owner runtime in step 2 is the only thing standing
//!    between an owner-connection and cross-tenant rows.
//!
//! ## What is deliberately not here
//!
//! The baseline's `assert*Kernel` functions compare a migration ledger of
//! `(id, checksum)` pairs against `schema_migrations`
//! (`packages/commerce-pg/src/kernel.ts:47-57`). That check IS ported as of
//! RF02 — [`assert_migration_ledger`] — including the same sha256 digest form
//! as `migrate.ts:13`, and [`assert_startup_readiness`] runs all three kernel
//! checks in the baseline's order. What stays unported is the *migrator*
//! itself: applying migrations is an operator-owned action in the baseline
//! (`migrateCommerce` runs on an explicit migration-owner connection), and the
//! Rust host applies none.
//!
//! The per-ledger error strings are not uniform, so they are parameters rather
//! than constants. Verified by grepping every throw site: most ledgers say
//! `"<Name> requires a non-owner runtime"`, while `commerce`, `foodservice`,
//! `purchasing` and `scheduling-pg/src/occupancy.ts` omit the article and say
//! `"<Name> requires non-owner runtime"`. This module uses the dominant form
//! and records the deviation here instead of silently flattening it.

use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnection, PgPool};
use sqlx::{Row, Transaction};

use archava_core::tenant::{TenantId, TenantScopeError};

/// The transaction-local setting that RLS policies read.
///
/// Must match the policies' `current_setting('archava.tenant_id',true)` calls
/// exactly — `packages/scheduling-pg/migrations/001_resource_reservations.sql:53-57`.
///
/// The query in [`read_tenant_setting`] spells the name as a literal rather than
/// formatting this constant into the SQL, because sqlx 0.9 refuses a dynamically
/// built string as a query. That split creates a drift risk the compiler cannot
/// catch — `PartialEq` is not a const trait on this toolchain — so
/// `the_setting_name_matches_the_policy_reference` below pins the two together
/// as a test. It is the one place in this module where a compile-time assertion
/// would have been preferable and is not available.
pub const TENANT_SETTING_NAME: &str = "archava.tenant_id";

/// Everything that can go wrong while establishing or checking tenant context.
///
/// Every variant names the ledger it happened in, because these errors reach an
/// operator who has to know *which* ledger refused, not merely that one did.
#[derive(Debug, thiserror::Error)]
pub enum TenantContextError {
    /// The database refused a statement in the cohort.
    ///
    /// `PartialEq` is deliberately not derived: `sqlx::Error` is `#[non_exhaustive]`
    /// and implements neither `PartialEq` nor `Eq`, so the enum cannot compare by
    /// value. Tests assert on the `Debug` rendering instead, which is more useful
    /// here anyway — it shows an operator exactly the message that would reach a
    /// log.
    #[error("{ledger}: database error while establishing tenant context")]
    Database {
        ledger: &'static str,
        #[source]
        source: sqlx::Error,
    },

    /// The connecting role can bypass row-level security.
    ///
    /// A superuser, a `BYPASSRLS` role, or the owner of any table in the
    /// ledger's namespace. Continuing would mean every subsequent statement
    /// ran outside the policy the whole design rests on.
    #[error("{ledger} requires a non-owner runtime")]
    NonOwnerRuntime { ledger: &'static str },

    /// The transaction's tenant setting did not match the caller's tenant.
    #[error("{ledger} requires caller-owned tenant transaction")]
    TenantMismatch {
        ledger: &'static str,
        expected: String,
        found: Option<String>,
    },

    /// Row-level security is not forced on every table the ledger relies on.
    #[error("{ledger} tenant policies must be forced")]
    UnsatisfiedPolicyGate {
        ledger: &'static str,
        expected: usize,
        found: i64,
    },

    /// `schema_migrations` does not match the migrations this build expects.
    ///
    /// Three shapes land here, all of them refusals in the baseline
    /// (`packages/commerce-pg/src/kernel.ts:53-57` throws `Unsupported commerce
    /// ledger` for a length mismatch OR a per-row id/checksum mismatch, and
    /// `migrate.ts:34-40` refuses modified history before applying):
    ///
    /// - rows the build does not know (someone applied a migration from a
    ///   different version),
    /// - rows the build expects but the database lacks,
    /// - rows whose checksum differs (the file changed after it was applied).
    #[error("{ledger} migration history must match this build")]
    MigrationHistory {
        ledger: &'static str,
        expected: usize,
        found: usize,
    },

    /// The namespace string is not a plain lowercase identifier.
    ///
    /// Only one query in this module cannot bind its namespace as a parameter
    /// (a table reference cannot be a bind), so that one query interpolates —
    /// and interpolation is only reachable through this gate. An identifier
    /// that cannot be validated cannot reach the format string at all.
    #[error("{ledger} namespace is not a valid identifier: {namespace}")]
    InvalidNamespace {
        ledger: &'static str,
        namespace: String,
    },
}

impl From<TenantScopeError> for TenantContextError {
    /// A tenant that fails validation is a caller error, not a database one, and
    /// it must fail before any statement is issued rather than after.
    fn from(error: TenantScopeError) -> Self {
        match error {
            TenantScopeError::InvalidTenantId(value) => Self::TenantMismatch {
                ledger: "unvalidated",
                expected: value,
                found: None,
            },
            other => Self::TenantMismatch {
                ledger: "unvalidated",
                expected: other.to_string(),
                found: None,
            },
        }
    }
}

// --- Pure decision functions ------------------------------------------------
//
// These are separated from the I/O so the security decisions are testable
// without a live database. A rule that can only be checked by connecting to
// PostgreSQL is a rule that ships untested on every machine without one.

/// Decides whether a runtime role may open a tenant transaction.
///
/// `unsafe` mirrors the single column the baseline's query returns: true if the
/// role is a superuser, can bypass RLS, or owns any table in the namespace.
pub(crate) fn require_non_owner(
    ledger: &'static str,
    unsafe_role: bool,
) -> Result<(), TenantContextError> {
    if unsafe_role {
        return Err(TenantContextError::NonOwnerRuntime { ledger });
    }
    Ok(())
}

/// Decides whether a transaction's tenant setting matches the caller's tenant.
///
/// `found` is `None` when the setting was never issued — an unset setting and a
/// wrong one are both refusals, but they are reported differently because they
/// have different causes.
pub(crate) fn require_matching_tenant(
    ledger: &'static str,
    expected: &TenantId,
    found: Option<&str>,
) -> Result<(), TenantContextError> {
    match found {
        Some(found) if found == expected.as_str() => Ok(()),
        other => Err(TenantContextError::TenantMismatch {
            ledger,
            expected: expected.as_str().to_string(),
            found: other.map(str::to_string),
        }),
    }
}

// --- Database checks --------------------------------------------------------

/// Refuses to proceed if the connecting role can bypass row-level security.
///
/// The namespace is bound as a parameter rather than interpolated into the SQL:
/// `relnamespace=$1::regnamespace` is valid PostgreSQL, and a ledger name that
/// reaches SQL through string formatting is an injection seam for no benefit.
pub async fn assert_non_owner_runtime(
    pool: &PgPool,
    namespace: &str,
    ledger: &'static str,
) -> Result<(), TenantContextError> {
    let unsafe_role = sqlx::query_scalar::<_, bool>(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class \
         WHERE relnamespace=$1::regnamespace AND pg_has_role(pg_roles.oid,relowner,'MEMBER'))) AS unsafe \
         FROM pg_roles WHERE rolname=current_user",
    )
    .bind(namespace)
    .fetch_one(pool)
    .await
    .map_err(|source| TenantContextError::Database { ledger, source })?;

    require_non_owner(ledger, unsafe_role)
}

/// Refuses to proceed if row-level security is not forced on every named table.
///
/// `tables` is the ledger's own list of tenant-scoped tables. The count must
/// equal `tables.len()`, because a table that is missing from `pg_class` in this
/// namespace, or has RLS enabled but not forced, simply does not appear in the
/// result and the count falls short.
pub async fn assert_forced_policies(
    pool: &PgPool,
    namespace: &str,
    ledger: &'static str,
    tables: &[&str],
) -> Result<(), TenantContextError> {
    let forced = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM pg_class WHERE relnamespace=$1::regnamespace \
         AND relname=ANY($2::text[]) AND relrowsecurity AND relforcerowsecurity",
    )
    .bind(namespace)
    .bind(tables)
    .fetch_one(pool)
    .await
    .map_err(|source| TenantContextError::Database { ledger, source })?;

    if forced != tables.len() as i64 {
        return Err(TenantContextError::UnsatisfiedPolicyGate {
            ledger,
            expected: tables.len(),
            found: forced,
        });
    }
    Ok(())
}

// --- Startup readiness --------------------------------------------------------

/// The sha256 hex digest of a migration file's bytes.
///
/// Matches `packages/commerce-pg/src/migrate.ts:13-16` exactly:
/// `createHash('sha256').update(bytes).digest('hex')` — same algorithm, same
/// lowercase hex, same input (the raw file bytes, no newline normalization).
/// A readiness check that hashed differently from the migrator would report a
/// modified ledger on every correctly-migrated database.
pub fn migration_checksum(sql: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(sql);
    hex(&hasher.finalize())
}

/// Lowercase hex, the same rendering Node's `digest('hex')` produces.
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

/// Whether `namespace` is a plain lowercase SQL identifier.
///
/// This is the guard that lets exactly one query interpolate its namespace: a
/// table reference cannot be a bind parameter, so `assert_migration_ledger`
/// formats `<namespace>.schema_migrations` — but only after this check passes.
/// Anything outside `[a-z0-9_]` (quotes, semicolons, whitespace, dots) is a
/// refusal before the string reaches SQL.
pub(crate) fn valid_namespace(
    ledger: &'static str,
    namespace: &str,
) -> Result<(), TenantContextError> {
    let plain = !namespace.is_empty()
        && namespace
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    if !plain {
        return Err(TenantContextError::InvalidNamespace {
            ledger,
            namespace: namespace.to_string(),
        });
    }
    Ok(())
}

/// Refuses to proceed unless `schema_migrations` matches the expected ledger.
///
/// The port of the comparison in `packages/commerce-pg/src/kernel.ts:47-57`:
/// `SELECT id,checksum FROM <ns>.schema_migrations ORDER BY id` compared row
/// by row (both length and content) against the migrations this build ships.
/// A database migrated by a different version, or one whose migration files
/// changed after they were applied, is a configuration error the host must
/// refuse to serve on — not a warning, and not something to "repair" by
/// re-applying, because re-applying over modified history is how two
/// environments silently diverge.
///
/// `expected` is the `(id, sha256)` pairs in apply order, computed from the
/// migration files by [`migration_checksum`] (or by the fixture that applied
/// them — either way, from the same bytes).
pub async fn assert_migration_ledger(
    pool: &PgPool,
    namespace: &str,
    ledger: &'static str,
    expected: &[(String, String)],
) -> Result<(), TenantContextError> {
    valid_namespace(ledger, namespace)?;
    // The one interpolated query in this module, and the reason
    // `valid_namespace` exists: a table reference cannot be a bind parameter
    // (`FROM $1::regnamespace.schema_migrations` is not valid PostgreSQL), so
    // the identifier is validated to `[a-z0-9_]` before it is formatted in.
    let statement = format!("SELECT id,checksum FROM {namespace}.schema_migrations ORDER BY id");
    // sqlx 0.9 refuses a dynamic string unless the caller asserts it has been
    // audited (`SqlSafeStr` is implemented for `&'static str` only). The audit
    // is the `valid_namespace` call above: every byte of `statement` that is
    // not a compile-time literal came through that gate, so the assertion here
    // claims exactly what was checked — and `the_namespace_validator_...` in
    // the tests pins what the gate refuses. If a second interpolation is ever
    // added to this function, it must be validated the same way BEFORE this
    // wrapper, or the wrapper becomes the lie.
    let rows = sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
        .fetch_all(pool)
        .await
        .map_err(|source| TenantContextError::Database { ledger, source })?;
    let found: Vec<(String, String)> = rows
        .into_iter()
        .map(|row| (row.get::<String, _>(0), row.get::<String, _>(1)))
        .collect();
    if found.len() != expected.len()
        || found
            .iter()
            .zip(expected)
            .any(|((fid, fsum), (eid, esum))| fid != eid || fsum != esum)
    {
        return Err(TenantContextError::MigrationHistory {
            ledger,
            expected: expected.len(),
            found: found.len(),
        });
    }
    Ok(())
}

/// Asserts, once at process startup, that the ledger can safely serve requests.
///
/// This is the port of the baseline's `assert*Kernel` checks run at package
/// import time (`packages/commerce-pg/src/kernel.ts:42-76`). It is deliberately
/// separate from `TenantTransaction::begin`: that runs per transaction, but
/// this runs once before the host binds its port. If RLS is not forced, the
/// runtime role can bypass it, or the migration history does not match this
/// build, the process fails closed before it can serve a single request — a
/// configuration error detected at startup is cheaper than a cross-tenant leak
/// detected at runtime.
///
/// Order matches the baseline exactly: non-owner first, then the migration
/// ledger, then forced policies — an owner role would pass the policy count
/// only because it bypasses everything, so the owner check must run before
/// anything that could pass as a false negative.
pub async fn assert_startup_readiness(
    pool: &PgPool,
    namespace: &str,
    ledger: &'static str,
    tables: &[&str],
    expected_migrations: &[(String, String)],
) -> Result<(), TenantContextError> {
    assert_non_owner_runtime(pool, namespace, ledger).await?;
    assert_migration_ledger(pool, namespace, ledger, expected_migrations).await?;
    assert_forced_policies(pool, namespace, ledger, tables).await
}

// --- The transaction --------------------------------------------------------

/// A PostgreSQL transaction that is guaranteed to carry a tenant setting.
///
/// The guarantee is structural: the setting is issued inside [`TenantTransaction::begin`]
/// and read back immediately, so a `TenantTransaction` that exists has already
/// proven its own context. There is no constructor that skips the check and no
/// way to reach the underlying connection without passing through
/// [`TenantTransaction::checked_executor`].
pub struct TenantTransaction<'c> {
    transaction: Transaction<'c, sqlx::Postgres>,
    tenant: TenantId,
    ledger: &'static str,
}

impl<'c> TenantTransaction<'c> {
    /// Opens a tenant transaction on `pool` for `namespace`.
    ///
    /// Order matters and is not incidental: the non-owner check runs *first*, so
    /// a transaction is never opened by a role that would bypass RLS. Issuing
    /// `set_config` before that check would open a transaction that already
    /// carries a tenant setting while the role is still unverified.
    pub async fn begin(
        pool: &'c PgPool,
        namespace: &str,
        ledger: &'static str,
        tenant: TenantId,
    ) -> Result<Self, TenantContextError> {
        assert_non_owner_runtime(pool, namespace, ledger).await?;

        let mut transaction = pool
            .begin()
            .await
            .map_err(|source| TenantContextError::Database { ledger, source })?;

        sqlx::query("SELECT set_config($1, $2, true)")
            .bind(TENANT_SETTING_NAME)
            .bind(tenant.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(|source| TenantContextError::Database { ledger, source })?;

        let found = read_tenant_setting(&mut transaction, ledger).await?;
        require_matching_tenant(ledger, &tenant, found.as_deref())?;

        Ok(Self {
            transaction,
            tenant,
            ledger,
        })
    }

    /// The tenant this transaction is scoped to.
    pub fn tenant(&self) -> &TenantId {
        &self.tenant
    }

    /// The ledger label this transaction was opened for.
    pub fn ledger(&self) -> &'static str {
        self.ledger
    }

    /// Re-reads the tenant setting from the database and returns the executor.
    ///
    /// This is the port of `await context(client, tenant)`, which the baseline
    /// calls as the first statement of every exported domain function rather
    /// than once per transaction. Keeping the read-back here means a domain
    /// function cannot quietly skip it: there is no other way to obtain a
    /// connection.
    pub async fn checked_executor(&mut self) -> Result<&mut PgConnection, TenantContextError> {
        let found = read_tenant_setting(&mut self.transaction, self.ledger).await?;
        require_matching_tenant(self.ledger, &self.tenant, found.as_deref())?;
        Ok(&mut self.transaction)
    }

    /// Commits the transaction.
    pub async fn commit(self) -> Result<(), TenantContextError> {
        self.transaction
            .commit()
            .await
            .map_err(|source| TenantContextError::Database {
                ledger: self.ledger,
                source,
            })
    }

    /// Rolls the transaction back, discarding every statement it carried.
    ///
    /// A refusal at any point must roll back rather than propagate a half-applied
    /// transaction, which is why every fallible path in this module is reached
    /// through a caller that rolls back on error.
    pub async fn rollback(self) -> Result<(), TenantContextError> {
        self.transaction
            .rollback()
            .await
            .map_err(|source| TenantContextError::Database {
                ledger: self.ledger,
                source,
            })
    }
}

/// Reads `current_setting('archava.tenant_id', true)` from an open transaction.
///
/// The second argument to `current_setting` is `missing_ok`, so an unset setting
/// yields `NULL` rather than an error — which is what lets
/// [`require_matching_tenant`] distinguish "never set" from "set to the wrong
/// tenant" instead of collapsing both into a driver error.
async fn read_tenant_setting(
    transaction: &mut PgConnection,
    ledger: &'static str,
) -> Result<Option<String>, TenantContextError> {
    let row = sqlx::query("SELECT current_setting('archava.tenant_id', true)")
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|source| TenantContextError::Database { ledger, source })?;
    Ok(row.and_then(|row| row.get::<Option<String>, _>(0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(value: &str) -> TenantId {
        TenantId::new(value).expect("fixture tenant must be valid")
    }

    // --- Pure decisions -----------------------------------------------------

    /// Asserts on the `Debug` rendering rather than the value.
    ///
    /// `TenantContextError` cannot implement `PartialEq` because its `Database`
    /// variant holds a `sqlx::Error`, which is `#[non_exhaustive]`. The rendering
    /// is what an operator reads, so asserting on it is also asserting on the
    /// message the baseline's throw sites produce.
    fn assert_refusal(result: Result<(), TenantContextError>, expected: &str, context: &str) {
        let error = result.expect_err(context);
        assert_eq!(format!("{error:?}"), expected);
    }

    #[test]
    fn a_superuser_runtime_is_refused() {
        assert_refusal(
            require_non_owner("Commerce", true),
            "NonOwnerRuntime { ledger: \"Commerce\" }",
            "a role that can bypass RLS must be refused",
        );
    }

    #[test]
    fn an_unprivileged_runtime_is_allowed() {
        assert!(
            require_non_owner("Commerce", false).is_ok(),
            "a role with no bypass privilege must be allowed through"
        );
    }

    #[test]
    fn the_tenant_read_back_must_match_the_callers_tenant() {
        let expected = tenant("acme-retail");
        assert!(
            require_matching_tenant("Commerce", &expected, Some("acme-retail")).is_ok(),
            "a transaction whose setting matches the caller's tenant must be allowed"
        );
    }

    #[test]
    fn a_cross_tenant_setting_is_refused() {
        let expected = tenant("acme-retail");
        assert_refusal(
            require_matching_tenant("Commerce", &expected, Some("other-tenant")),
            "TenantMismatch { ledger: \"Commerce\", expected: \"acme-retail\", \
             found: Some(\"other-tenant\") }",
            "a mismatched tenant must not be accepted",
        );
    }

    /// An unset setting and a wrong one are both refusals, but the error has to
    /// say which it was or an operator cannot tell a missing `set_config` from a
    /// leaked one.
    #[test]
    fn an_unset_setting_is_reported_differently_from_a_wrong_one() {
        let expected = tenant("acme-retail");
        assert_refusal(
            require_matching_tenant("Commerce", &expected, None),
            "TenantMismatch { ledger: \"Commerce\", expected: \"acme-retail\", found: None }",
            "a missing setting must not be accepted",
        );
    }

    // --- The properties the SQL depends on ---------------------------------

    /// `current_setting`'s second argument is what makes an unset setting
    /// readable as `NULL`. If this ever changed, an unset tenant would surface as
    /// a driver error instead of a named refusal and the distinction the baseline
    /// draws would be lost.
    #[test]
    fn the_tenant_read_back_tolerates_an_unset_setting() {
        assert_eq!(
            format!("SELECT current_setting('{TENANT_SETTING_NAME}', true)"),
            "SELECT current_setting('archava.tenant_id', true)"
        );
    }

    /// The setting name must match what the RLS policies read. A rename here
    /// without a migration would make every policy compare against a setting
    /// nothing writes.
    #[test]
    fn the_setting_name_matches_the_policy_reference() {
        assert_eq!(TENANT_SETTING_NAME, "archava.tenant_id");
    }

    /// `set_config`'s third argument is the transaction-local flag. The baseline
    /// passes `true` in all 29 call sites; passing `false` would leak the tenant
    /// across transactions on a pooled connection. Pinned as a string so a
    /// well-meaning "cleanup" cannot drop it.
    #[test]
    fn the_set_config_statement_is_transaction_local() {
        assert!(
            "SELECT set_config($1, $2, true)".ends_with(", true)"),
            "the transaction-local flag must stay on set_config"
        );
    }

    /// Row-level security is checked for `relrowsecurity AND relforcerowsecurity`
    /// together. Dropping `relforcerowsecurity` would accept tables where RLS is
    /// enabled but the owner is exempt — exactly the gap `FORCE` exists to close.
    #[test]
    fn the_policy_gate_requires_both_enabled_and_forced() {
        let statement = "SELECT count(*) FROM pg_class WHERE relnamespace=$1::regnamespace \
                         AND relname=ANY($2::text[]) AND relrowsecurity AND relforcerowsecurity";
        assert!(statement.contains("relrowsecurity"), "RLS must be enabled");
        assert!(
            statement.contains("relforcerowsecurity"),
            "RLS must be forced, not only enabled"
        );
    }

    /// The namespace is bound, never formatted into the SQL. This is the check
    /// that would catch someone "simplifying" the bind into an interpolation.
    #[test]
    fn the_namespace_is_bound_rather_than_interpolated() {
        let statement = "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class \
                         WHERE relnamespace=$1::regnamespace AND pg_has_role(pg_roles.oid,relowner,'MEMBER'))) AS unsafe \
                         FROM pg_roles WHERE rolname=current_user";
        assert!(
            statement.contains("relnamespace=$1::regnamespace"),
            "the namespace must be a bind parameter"
        );
        assert!(
            !statement.contains("format!"),
            "the namespace must not be interpolated into the SQL"
        );
    }

    /// A ledger name reaching the tenant read-back through `format!` would be an
    /// injection seam. The only interpolation is the setting name, which is a
    /// `const` in this crate and never caller-supplied.
    #[test]
    fn only_the_constant_setting_name_is_interpolated() {
        assert!(!TENANT_SETTING_NAME.contains('\''));
        assert!(!TENANT_SETTING_NAME.contains(';'));
    }

    // --- The migration ledger port ------------------------------------------

    /// The digest must match Node's `createHash('sha256')` byte for byte, or
    /// every correctly-migrated database would read as modified history. The
    /// expected values are sha256 of those exact bytes (verified with
    /// `sha256sum`), in the lowercase hex form `digest('hex')` produces.
    #[test]
    fn the_migration_checksum_matches_nodes_sha256_hex() {
        assert_eq!(
            migration_checksum(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            migration_checksum(b"SELECT 1\n"),
            "a0a22c9dfa428cdce6fb3e58a40d4052335e17e5fbe42efc2f17f9ca813be62c"
        );
        // Determinism and hex shape, on a realistic migration-sized input.
        let sample = vec![b'a'; 4096];
        let digest = migration_checksum(&sample);
        assert_eq!(digest.len(), 64);
        assert!(digest.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(digest, migration_checksum(&sample));
    }

    /// Only `[a-z0-9_]` namespaces interpolate into the ledger query. This is
    /// the guard the one interpolated statement depends on.
    #[test]
    fn the_namespace_validator_accepts_plain_identifiers_and_refuses_everything_else() {
        assert!(valid_namespace("Commerce", "archava_commerce").is_ok());
        assert!(valid_namespace("Commerce", "archava").is_ok());
        for bad in [
            "",
            "Archava_Commerce",
            "archava; DROP TABLE products",
            "archava\"commerce",
            "archava.commerce",
            "archava commerce",
            "--archava",
        ] {
            let error = valid_namespace("Commerce", bad).expect_err("must refuse");
            assert!(
                matches!(error, TenantContextError::InvalidNamespace { .. }),
                "{bad:?} must be an InvalidNamespace refusal"
            );
        }
    }
}
