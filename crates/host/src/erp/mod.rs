//! The ERP host surface — the Archxy 2 namespace for the bounded ERP cohorts.
//!
//! Placement mirrors `host/src/crm.rs`: this module owns the purchasing
//! ledger descriptor, the transaction opener with the baseline's `SET LOCAL`
//! guards, the submission boundary, and the routes that call the adapters'
//! step functions from ONE transaction. The authorization ladder is a
//! database property here (membership rows + RLS + the domain authorize
//! rule the adapters reapply), exactly as it is for the CRM slice.
//!
//! The commerce-ledger routes (R06's commercial half) arrive with their own
//! cohort packet; the purchasing receipts already drive cross-ledger writes
//! through the kernel port, so this module's scope is narrower than its
//! fathers' and deliberately so.

pub mod foodservice;
// R04 cohorts (PACKET-003).
pub mod organizations;
pub mod projects;
pub mod proposals;
// R05 cohorts (PACKET-004..006).
pub mod accommodation;
pub mod assets;
pub mod commerce;
pub mod purchasing;
pub mod scheduling;

// R07 cohorts (PACKET-007..010).
pub mod attendance;
pub mod billing;
pub mod subscriptions;
pub mod support;

// R08 cohorts.
pub mod field_services;
pub mod formal_education;
pub mod fulfillment;
pub mod laundry;

use archava_adapters::rls::{TenantContextError, TenantTransaction};

use crate::router::Ledger;

/// The purchasing ledger descriptor — the nine tables migration 001 creates
/// and every one of the readiness gates counts.
pub const PURCHASING_LEDGER: Ledger = Ledger {
    namespace: "archava_purchasing",
    ledger: "purchasing",
    tables: &[
        "memberships",
        "suppliers",
        "offers",
        "orders",
        "lines",
        "receipts",
        "receipt_lines",
        "commands",
        "audit",
    ],
};

/// The ways an ERP request fails as INFRASTRUCTURE — everything else is a
/// caller-facing answer the submit functions return as data.
#[derive(Debug)]
pub enum ErpGateError {
    /// Authority-resolution (>session, tenant context, checked executor) —
    /// mapped with the SAME fixed texts the CRM routes use.
    Authority(crate::authority::AuthorityError),
    /// A database failure the SQL-state whitelist did not clear.
    Database(&'static str, sqlx::Error),
}

impl From<crate::authority::AuthorityError> for ErpGateError {
    fn from(error: crate::authority::AuthorityError) -> Self {
        Self::Authority(error)
    }
}

impl From<TenantContextError> for ErpGateError {
    fn from(error: TenantContextError) -> Self {
        Self::Authority(crate::authority::AuthorityError::from(error))
    }
}

/// The transaction opener — [`crate::crm`]'s opener with the purchasing
/// ledger's namespace: non-owner runtime first (inside
/// `TenantTransaction::begin`), then the `SET LOCAL` timeouts
/// `packages/purchasing-pg/src/repository.ts` sets on every transaction
/// (statement 10s, lock 3s).
pub(crate) async fn begin(
    state: &crate::router::AppState,
    tenant: archava_core::tenant::TenantId,
) -> Result<TenantTransaction<'_>, ErpGateError> {
    let mut transaction = TenantTransaction::begin(
        state.pool(),
        PURCHASING_LEDGER.namespace,
        PURCHASING_LEDGER.ledger,
        tenant,
    )
    .await
    .map_err(ErpGateError::from)?;
    {
        let connection = transaction.checked_executor().await?;
        for statement in [
            "SET LOCAL statement_timeout='10s'",
            "SET LOCAL lock_timeout='3s'",
        ] {
            sqlx::query(statement)
                .execute(&mut *connection)
                .await
                .map_err(|source| ErpGateError::Database(PURCHASING_LEDGER.ledger, source))?;
        }
    }
    Ok(transaction)
}

/// The foodservice ledger descriptor — the nine tables migration 001
/// creates and the readiness gate counts.
pub const FOODSERVICE_LEDGER: Ledger = Ledger {
    namespace: "archava_foodservice",
    ledger: "foodservice",
    tables: &[
        "memberships",
        "tables",
        "menu_items",
        "recipe_versions",
        "recipe_components",
        "tickets",
        "ticket_lines",
        "commands",
        "audit",
    ],
};

/// The foodservice transaction opener — the same `SET LOCAL` guards the
/// purchasing opener sets, against the foodservice namespace.
pub(crate) async fn begin_foodservice(
    state: &crate::router::AppState,
    tenant: archava_core::tenant::TenantId,
) -> Result<TenantTransaction<'_>, ErpGateError> {
    let mut transaction = TenantTransaction::begin(
        state.pool(),
        FOODSERVICE_LEDGER.namespace,
        FOODSERVICE_LEDGER.ledger,
        tenant,
    )
    .await
    .map_err(ErpGateError::from)?;
    {
        let connection = transaction.checked_executor().await?;
        for statement in [
            "SET LOCAL statement_timeout='10s'",
            "SET LOCAL lock_timeout='3s'",
        ] {
            sqlx::query(statement)
                .execute(&mut *connection)
                .await
                .map_err(|source| ErpGateError::Database(FOODSERVICE_LEDGER.ledger, source))?;
        }
    }
    Ok(transaction)
}

// ===========================================================================
// The R04 ledgers (PACKET-003) — organizations, proposals and projects. The
// three cohorts share the kernel ledgers' gate shape: one descriptor, one
// opener with the same `SET LOCAL` guards, and routes that drive the
// adapters' step functions from ONE transaction.
// ===========================================================================

/// The organizations ledger descriptor — `001_organizations_affiliations`
/// creates the five data tables plus `schema_migrations`.
pub const ORGANIZATIONS_LEDGER: Ledger = Ledger {
    namespace: "archava_organizations",
    ledger: "organizations",
    tables: &[
        "memberships",
        "organizations",
        "affiliations",
        "commands",
        "audit",
    ],
};

/// The proposals ledger descriptor — `001_proposals_scope_amendments`
/// creates the seven data tables plus `schema_migrations`.
pub const PROPOSALS_LEDGER: Ledger = Ledger {
    namespace: "archava_proposals",
    ledger: "proposals",
    tables: &[
        "memberships",
        "proposals",
        "scope_items",
        "scope_versions",
        "change_requests",
        "commands",
        "audit",
    ],
};

/// The projects ledger descriptor — `001_project_delivery` creates the six
/// data tables plus `schema_migrations`.
pub const PROJECTS_LEDGER: Ledger = Ledger {
    namespace: "archava_projects",
    ledger: "projects",
    tables: &[
        "memberships",
        "projects",
        "milestones",
        "tasks",
        "commands",
        "audit",
    ],
};

macro_rules! r04_opener {
    ($name:ident, $ledger:ident) => {
        pub(crate) async fn $name(
            state: &crate::router::AppState,
            tenant: archava_core::tenant::TenantId,
        ) -> Result<TenantTransaction<'_>, ErpGateError> {
            let mut transaction =
                TenantTransaction::begin(state.pool(), $ledger.namespace, $ledger.ledger, tenant)
                    .await
                    .map_err(ErpGateError::from)?;
            {
                let connection = transaction.checked_executor().await?;
                for statement in [
                    "SET LOCAL statement_timeout='10s'",
                    "SET LOCAL lock_timeout='3s'",
                ] {
                    sqlx::query(statement)
                        .execute(&mut *connection)
                        .await
                        .map_err(|source| ErpGateError::Database($ledger.ledger, source))?;
                }
            }
            Ok(transaction)
        }
    };
}

r04_opener!(begin_organizations, ORGANIZATIONS_LEDGER);
r04_opener!(begin_proposals, PROPOSALS_LEDGER);
r04_opener!(begin_projects, PROJECTS_LEDGER);

// ===========================================================================
// R05 cohorts (PACKET-004..006): scheduling, accommodation and assets. All
// three use the same opener shape; the scheduling ledger carries the shared
// occupancy kernel the accommodation cohort reserves through.
// ===========================================================================

/// The scheduling ledger descriptor — `001_resource_reservations` creates the
/// five data tables plus `schema_migrations`, and `002_shared_occupancy`
/// adds the `occupancy` kernel table the accommodation cohort reserves
/// through (PACKET-004).
pub const SCHEDULING_LEDGER: Ledger = Ledger {
    namespace: "archava_scheduling",
    ledger: "scheduling",
    tables: &[
        "memberships",
        "resources",
        "reservations",
        "commands",
        "audit",
    ],
};

/// The accommodation ledger descriptor — `001_room_night_stays` creates the
/// seven data tables plus `schema_migrations`; the rooms live inside the
/// scheduling kernel's resources (PACKET-005).
pub const ACCOMMODATION_LEDGER: Ledger = Ledger {
    namespace: "archava_accommodation",
    ledger: "accommodation",
    tables: &[
        "memberships",
        "properties",
        "room_types",
        "rooms",
        "stays",
        "stay_nights",
        "commands",
        "audit",
    ],
};

/// The assets ledger descriptor — `001_assets_custody_maintenance` creates
/// the six data tables plus `schema_migrations` (PACKET-006).
pub const ASSETS_LEDGER: Ledger = Ledger {
    namespace: "archava_assets",
    ledger: "assets",
    tables: &[
        "memberships",
        "assets",
        "custody_events",
        "maintenance_records",
        "commands",
        "audit",
    ],
};

r04_opener!(begin_scheduling, SCHEDULING_LEDGER);
r04_opener!(begin_accommodation, ACCOMMODATION_LEDGER);
r04_opener!(begin_assets, ASSETS_LEDGER);
/// The R07 first cohort (PACKET-007): the billing ledger — the eight
/// data tables migration `001_billing_invoices_settlement` creates
/// (memberships, invoices, invoice_lines, invoice_corrections,
/// payment_attempts, payment_events, commands, audit) plus
/// `schema_migrations`. The settlement ledger is its own kernel: the
/// balance CHECK and the guard triggers are the tables' own defenses,
/// no sibling ledger composes in.
pub const BILLING_LEDGER: Ledger = Ledger {
    namespace: "archava_billing",
    ledger: "billing",
    tables: &[
        "memberships",
        "invoices",
        "invoice_lines",
        "invoice_corrections",
        "payment_attempts",
        "payment_events",
        "gateway_links",
        "gateway_runtime",
        "gateway_observations",
        "commands",
        "audit",
    ],
};

r04_opener!(begin_billing, BILLING_LEDGER);

/// The R07 second cohort (PACKET-008): the subscriptions ledger — the six
/// data tables migration `001_subscriptions_entitlements_usage` creates
/// (memberships, subscriptions, entitlements, usage_events, commands,
/// audit) plus `schema_migrations`. The entitlement ledger is its own
/// kernel: the quota CHECK and the usage-idempotency UNIQUE constraint are
/// the tables' own defenses, no sibling ledger composes in.
pub const SUBSCRIPTIONS_LEDGER: Ledger = Ledger {
    namespace: "archava_subscriptions",
    ledger: "subscriptions",
    tables: &[
        "memberships",
        "subscriptions",
        "entitlements",
        "usage_events",
        "commands",
        "audit",
    ],
};

r04_opener!(begin_subscriptions, SUBSCRIPTIONS_LEDGER);

/// The R07 third cohort (PACKET-009): the attendance ledger — the six
/// data tables migration `001_attendance_sessions_rosters` creates
/// (memberships, sessions, rosters, check_in_events, commands, audit)
/// plus `schema_migrations`. The roster binding audit A04 names is a
/// SQL property here (the check-in row SELECT carries the session) and a
/// domain property (`check_in_refusal`).
pub const ATTENDANCE_LEDGER: Ledger = Ledger {
    namespace: "archava_attendance",
    ledger: "attendance",
    tables: &[
        "memberships",
        "sessions",
        "rosters",
        "check_in_events",
        "commands",
        "audit",
    ],
};

r04_opener!(begin_attendance, ATTENDANCE_LEDGER);

/// The R07 fourth cohort (PACKET-010): the support case ledger — the six
/// data tables migration `001_cases_notes_resolution` creates
/// (memberships, cases, commands, notes, transitions, audit)
/// plus `schema_migrations`. The deferred `case_complete` trigger is the
/// history invariant; the adapter writes the whole chain in one transaction.
pub const SUPPORT_LEDGER: Ledger = Ledger {
    namespace: "archava_support",
    ledger: "support",
    tables: &[
        "memberships",
        "cases",
        "commands",
        "notes",
        "transitions",
        "audit",
    ],
};

r04_opener!(begin_support, SUPPORT_LEDGER);

/// The field services ledger descriptor — migrations 001 and 002 create.
pub const FIELD_LEDGER: Ledger = Ledger {
    namespace: "archava_field",
    ledger: "field",
    tables: &[
        "memberships",
        "jobs",
        "visits",
        "work_lines",
        "signoffs",
        "commands",
        "audit",
    ],
};

r04_opener!(begin_field, FIELD_LEDGER);

/// The laundry ledger descriptor — migration 001 creates.
pub const LAUNDRY_LEDGER: Ledger = Ledger {
    namespace: "archava_laundry",
    ledger: "laundry",
    tables: &[
        "memberships",
        "orders",
        "order_lines",
        "process_events",
        "findings",
        "commands",
        "audit",
    ],
};

r04_opener!(begin_laundry, LAUNDRY_LEDGER);

/// The fulfillment ledger descriptor — migration 001 creates.
pub const FULFILLMENT_LEDGER: Ledger = Ledger {
    namespace: "archava_fulfillment",
    ledger: "fulfillment",
    tables: &[
        "memberships",
        "jobs",
        "job_lines",
        "findings",
        "custody_events",
        "commands",
        "audit",
    ],
};

r04_opener!(begin_fulfillment, FULFILLMENT_LEDGER);

/// The formal education ledger descriptor — migrations 001 and 002 create.
pub const EDU_LEDGER: Ledger = Ledger {
    namespace: "archava_edu",
    ledger: "formal_education",
    tables: &[
        "memberships",
        "enrollments",
        "sessions",
        "results",
        "reviews",
        "commands",
        "audit",
    ],
};

r04_opener!(begin_education, EDU_LEDGER);

pub mod workspace;
