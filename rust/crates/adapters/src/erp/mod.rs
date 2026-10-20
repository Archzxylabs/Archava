//! The ERP transactional adapters — the Archxy 2 namespace for the bounded
//! ERP cohorts (R04–R08).
//!
//! ## Placement
//!
//! These modules are the machine room the domain contracts talk to: raw SQL
//! against the frozen `.sql` migrations, executed strictly inside the shared
//! RLS transaction cohort (`archava_adapters::rls::TenantTransaction`). No
//! module here may issue a statement outside a tenant transaction — the same
//! rule the crate header pins for the whole crate — and every ERP function
//! re-reads the tenant setting through `checked_executor` before its first
//! statement, the port of the baseline's per-function `context(client…)`.
//!
//! ## Source of truth
//!
//! - `rust/crates/domain/src/erp/` — the contract types and grant rules these
//!   functions consume (the pure half).
//! - `packages/purchasing-pg/src/repository.ts` +
//!   `packages/commerce-pg/src/{kernel,repository,migrate}.ts` — the TS
//!   implementations these SQL paths are ported from. A deviation is a record
//!   in the module header, never a silent rewrite.
//!
//! ## Status (Archxy 2, PACKET-000)
//!
//! This increment ports the R06 stock-cohort core: the purchasing repository
//! (suppliers, offers, purchase orders, partial receipts), the commerce
//! kernel primitives the receipts reconcile against (product locks, stock
//! read, the cross-ledger inbound write), and the real `product.create`
//! path so the required flow never seeds catalog rows by hand. The rest of
//! the commerce surface (orders, returns, reservations) ports in the next
//! packet and is deliberately NOT half-faked here.

pub mod commerce_kernel;
pub mod foodservice;
pub mod organizations;
pub mod projects;
pub mod proposals;
pub mod purchasing;

use archava_domain::erp::ErpError;
use thiserror::Error;

/// The permission vocabulary the `archava_commerce.memberships` CHECK
/// constrains (`packages/commerce/src/contracts.ts` `COMMERCE_PERMISSIONS`,
/// mirrored by the SQL `<@ ARRAY[...]`). An unknown permission in a loaded
/// row refuses closed.
pub const COMMERCE_PERMISSION_VOCABULARY: &[&str] = &[
    "catalog.read",
    "catalog.write",
    "stock.read",
    "stock.write",
    "orders.read",
    "orders.write",
    "returns.write",
    "audit.read",
];

/// The ledger labels carried on database errors. Lowercase, human-greppable,
/// equal to the TS repository names so an operator reading a refusal sees the
/// same word the baseline names.
pub const COMMERCE_LEDGER: &str = "commerce";
pub const PURCHASING_LEDGER: &str = "purchasing";

/// The foodservice ledger label — the TS repository name the refusals carry.
pub const FOODSERVICE_LEDGER: &str = "foodservice";

/// The SQL namespaces both ledgers serve — fixed constants, because the
/// `lock_membership` statements interpolate them after a plain-identifier
/// check and no caller supplies anything else.
pub const COMMERCE_NAMESPACE: &str = "archava_commerce";
pub const PURCHASING_NAMESPACE: &str = "archava_purchasing";

/// The foodservice SQL namespace — the third of the stock cohort's ledgers.
pub const FOODSERVICE_NAMESPACE: &str = "archava_foodservice";

/// A purchasing-cohort refusal, exactly one of two classes:
///
/// - [`PurchaseError::Domain`] — an API-code refusal (`forbidden`,
///   `not_found`, `conflict`) the caller can act on. Validation refusals map
///   through the host's own invalid-submission shape before the repository
///   ever runs.
/// - [`PurchaseError::Database`] — a driver error the SQL.state whitelist did
///   NOT reclassify; an infrastructure fault, never a caller decision.
#[derive(Debug, Error)]
pub enum PurchaseError {
    #[error("{0}")]
    Domain(#[from] ErpError),
    #[error("{ledger} database failure")]
    Database {
        ledger: &'static str,
        source: sqlx::Error,
    },
}

/// Classifies a driver error into the repository's refusal vocabulary.
///
/// The SQL states that mean CONFLICT are exactly the ones the TS catch maps
/// (`packages/purchasing-pg/src/repository.ts`, `transaction()`'s catch):
/// `23514` check/trigger refusal, `23503` foreign key, `23505` unique,
/// `22003` numeric out of range, `40P01` deadlock, `55P03` lock not
/// available. Everything else stays a database failure — swallowing a serial
/// error as a conflict would misreport history corruptions as caller races.
pub fn purchase_database_error(ledger: &'static str, error: sqlx::Error) -> PurchaseError {
    if database_error_reclassifies_conflict(&error) {
        return PurchaseError::Domain(ErpError::Conflict);
    }
    PurchaseError::Database {
        ledger,
        source: error,
    }
}

/// The commerce kernel's errors use the same shape as the purchasing ones —
/// the baseline shares the error class too (`CommerceError` cases the
/// purchasing catch re-throws).
pub type CommerceError = PurchaseError;

pub fn commerce_database_error(ledger: &'static str, error: sqlx::Error) -> PurchaseError {
    purchase_database_error(ledger, error)
}

fn database_error_reclassifies_conflict(error: &sqlx::Error) -> bool {
    // `SqlxError::Database(boxed)` — the boxed trait object answers with its
    // SQLSTATE through the `DatabaseError::code()` method (the same call
    // `crm.rs:767` makes for its timestamp-shape conflict). Only the
    // whitelist reclassifies; no postgres-module downcast is needed.
    if let sqlx::Error::Database(database) = error {
        return matches!(
            database.code().as_deref(),
            Some("23514")
                | Some("23503")
                | Some("23505")
                | Some("22003")
                | Some("40P01")
                | Some("55P03")
        );
    }
    false
}
