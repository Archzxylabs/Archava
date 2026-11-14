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

pub mod accounting;
mod billing_invoice;
pub mod commerce_kernel;
pub mod companies;
pub mod company_trade;
/// The CRM transactional persistence adapter (ADR-018: normalized under ERP adapters).
pub mod crm;
mod customer;
pub mod foodservice;
pub mod operator;
pub mod organizations;
pub mod parties;
pub mod projections;
pub mod projects;
pub mod proposals;
// R05 cohorts (PACKET-004..006).
pub mod accommodation;
pub mod assets;
pub mod commerce;
pub mod purchasing;
pub mod scheduling;
mod supplier_documents;

// R07 cohorts (PACKET-007..010).
pub mod attendance;
pub mod billing;
pub mod mail;
pub mod payments;
pub mod subscriptions;
pub mod support;

// R08 cohorts.
pub mod field_services;
pub mod formal_education;
pub mod fulfillment;
pub mod laundry;

use archava_domain::erp::refusals::{self, Refusal};
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
    "valuation.read",
    "valuation.write",
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

/// A purchasing-cohort refusal, exactly one of three classes:
///
/// - [`PurchaseError::Domain`] — an API-code refusal (`forbidden`,
///   `not_found`, `conflict`) the caller can act on. Validation refusals map
///   through the host's own invalid-submission shape before the repository
///   ever runs.
/// - [`PurchaseError::Refused`] — a conflict whose reason is known: the
///   database guard that refused is in the refusal reason catalogue
///   (`config/refusal-reasons.v1.json`). It is a conflict in every respect
///   (same HTTP class, same `reason`); it only adds the catalogued code and
///   sentence. A handler that has no use for the reason treats it exactly as
///   `Domain(ErpError::Conflict)`.
/// - [`PurchaseError::Database`] — a driver error the SQL.state whitelist did
///   NOT reclassify; an infrastructure fault, never a caller decision.
#[derive(Debug, Error)]
pub enum PurchaseError {
    #[error("{0}")]
    Domain(#[from] ErpError),
    #[error("refused: {}", .0.code)]
    Refused(&'static Refusal),
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
    if let Some(refusal) = catalogued_refusal(&error) {
        return PurchaseError::Refused(refusal);
    }
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

/// The catalogued reason behind a guard refusal, if there is one.
///
/// Only `23514` is read: that is the state every business guard raises with,
/// and the other conflict states (`23503`, `23505`, …) carry driver-composed
/// messages that name constraints, never a guard sentence. The guard's whole
/// message must equal a catalogue key.
pub fn catalogued_refusal(error: &sqlx::Error) -> Option<&'static Refusal> {
    let database = error.as_database_error()?;
    refusal_for(database.code().as_deref(), database.message())
}

/// A conflict an adapter decides itself, before the database guard would,
/// carrying the catalogued reason of that guard. An unknown code is a plain
/// conflict, never a different class of answer.
pub fn refused_by_code(code: &str) -> PurchaseError {
    refusals::for_code(code).map_or(
        PurchaseError::Domain(ErpError::Conflict),
        PurchaseError::Refused,
    )
}

/// [`catalogued_refusal`] on the two facts it reads, so the rule is testable
/// without a database.
fn refusal_for(sqlstate: Option<&str>, message: &str) -> Option<&'static Refusal> {
    if sqlstate != Some("23514") {
        return None;
    }
    refusals::for_guard(message)
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

pub mod inventory_valuation;

#[cfg(test)]
mod refusal_tests {
    use super::*;
    use sqlx::error::{DatabaseError, ErrorKind};
    use std::borrow::Cow;

    /// A driver error with a chosen SQLSTATE and message.
    #[derive(Debug)]
    struct Raised(&'static str, String);
    impl std::fmt::Display for Raised {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.1)
        }
    }
    impl std::error::Error for Raised {}
    impl DatabaseError for Raised {
        fn message(&self) -> &str {
            &self.1
        }
        fn code(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed(self.0))
        }
        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }
        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }
        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }
        fn kind(&self) -> ErrorKind {
            ErrorKind::Other
        }
    }
    fn raised(sqlstate: &'static str, message: &str) -> sqlx::Error {
        sqlx::Error::Database(Box::new(Raised(sqlstate, message.to_string())))
    }

    const GUARD: &str =
        "A company that already trades in IDR cannot open a book in another currency";

    #[test]
    fn a_catalogued_guard_message_carries_its_code() {
        match purchase_database_error("accounting", raised("23514", GUARD)) {
            PurchaseError::Refused(refusal) => {
                assert_eq!(refusal.code, "book_currency_conflicts_with_existing_trade");
                assert_eq!(refusal.guard, GUARD);
            }
            other => panic!("{other:?}"),
        }
        // The commerce alias classifies the same way.
        assert!(matches!(
            commerce_database_error(COMMERCE_LEDGER, raised("23514", GUARD)),
            PurchaseError::Refused(_)
        ));
        assert_eq!(
            PurchaseError::Refused(refusals::for_guard(GUARD).unwrap()).to_string(),
            "refused: book_currency_conflicts_with_existing_trade"
        );
    }

    #[test]
    fn an_uncatalogued_guard_message_stays_a_plain_conflict() {
        for message in [
            "Financial history is immutable",
            "",
            // Containing a catalogued sentence is not being it.
            "ERROR: A company that already trades in IDR cannot open a book in another currency",
            "A company that already trades in IDR cannot open a book in another currency.",
            "a company that already trades in idr cannot open a book in another currency",
            " A company that already trades in IDR cannot open a book in another currency",
        ] {
            assert!(
                matches!(
                    purchase_database_error("accounting", raised("23514", message)),
                    PurchaseError::Domain(ErpError::Conflict)
                ),
                "{message}"
            );
        }
    }

    /// Billing keeps its own whitelist, but a deadlock and a lock timeout are
    /// a race the caller can retry, as in the shared classifier: a conflict,
    /// not a database failure. Every other state billing did not whitelist
    /// stays a database failure there.
    #[test]
    fn billing_classifies_lock_failures_as_conflicts_and_nothing_else_new() {
        for state in ["40P01", "55P03"] {
            assert!(
                matches!(
                    billing::refuses(raised(state, "deadlock detected")),
                    PurchaseError::Domain(ErpError::Conflict)
                ),
                "{state}"
            );
        }
        for state in ["23503", "23505", "22003", "P0001", "40001", "42501"] {
            assert!(
                matches!(
                    billing::refuses(raised(state, GUARD)),
                    PurchaseError::Database { .. }
                ),
                "{state}"
            );
        }
        // An uncatalogued or unlisted guard sentence is still a database failure.
        assert!(matches!(
            billing::refuses(raised("23514", GUARD)),
            PurchaseError::Database { .. }
        ));
    }

    #[test]
    fn other_sql_states_classify_as_before_whatever_their_message() {
        for state in ["23503", "23505", "22003", "40P01", "55P03"] {
            assert!(
                matches!(
                    purchase_database_error("accounting", raised(state, GUARD)),
                    PurchaseError::Domain(ErpError::Conflict)
                ),
                "{state}"
            );
            assert!(catalogued_refusal(&raised(state, GUARD)).is_none());
        }
        for state in ["P0001", "40001", "42501", "23502", "23P01"] {
            assert!(
                matches!(
                    purchase_database_error("accounting", raised(state, GUARD)),
                    PurchaseError::Database {
                        ledger: "accounting",
                        ..
                    }
                ),
                "{state}"
            );
        }
        assert!(matches!(
            purchase_database_error("accounting", sqlx::Error::RowNotFound),
            PurchaseError::Database { .. }
        ));
        assert!(catalogued_refusal(&sqlx::Error::RowNotFound).is_none());
        assert!(refusal_for(None, GUARD).is_none());
    }

    #[test]
    fn an_adapter_decided_refusal_names_a_catalogued_code() {
        // The codes the adapters name themselves (billing_invoice.rs,
        // purchasing.rs).
        for code in [
            "invoice_order_not_fulfilled",
            "invoice_order_has_returns",
            "purchase_line_currency_mismatch",
            "offer_currency_differs_from_product",
        ] {
            assert!(
                matches!(refused_by_code(code), PurchaseError::Refused(r) if r.code == code),
                "{code}"
            );
        }
        assert!(matches!(
            refused_by_code("no_such_code"),
            PurchaseError::Domain(ErpError::Conflict)
        ));
    }
}
