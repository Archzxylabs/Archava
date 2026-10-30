//! What each module needs and what it ships.
//!
//! A port of `MODULE_CONTRACTS` from
//! `packages/config/src/assembly.ts` — 22 entries, transcribed in source order.
//!
//! ## What each entry carries, and why all five parts
//!
//! Stripped to what `plan` reads, an entry is an id, a version, a set of module
//! prerequisites, a set of package names that must be installed, an entry
//! point, and a permission list. The first four shape plan output and its
//! refusals; the last two are carried through so a consumer can render the
//! module's authority surface without a second lookup back into the
//! TypeScript.
//!
//! ## Why the table is written out rather than derived
//!
//! Every entry could in principle be generated — the `packages` lists are one
//! shape (`domain`, `domain-pg`, `identity-pg`), the `requires` lists are two
//! shapes (`[]` and `["contacts"]`), and the version is `1.0.0` on all 22. That
//! regularity is exactly why deriving it would be wrong: the next entry that
//! breaks the pattern would either fail to fit the generator or, worse, be
//! silently coerced into the shape. Four entries already differ from the
//! biggest group — `accommodation`, `nativeActions`, `foodservice` and
//! `purchasing` each need two prerequisites, and `accommodation` and
//! `purchasing` share a `-pg` package with another module — so the exceptions
//! are already the interesting part.
//!
//! This is the same reason the contract gate in [`crate::plan`] is a table and
//! not arithmetic.

use crate::request::ModuleId as M;
use crate::request::ModuleId;

/// One module's contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleContract {
    /// The module's identifier.
    pub id: ModuleId,
    /// The module's own contract version. `1.0.0` on all 22 today.
    pub version: &'static str,
    /// Modules that must also be selected. `plan` refuses a selection whose
    /// prerequisite is disabled.
    pub requires: &'static [ModuleId],
    /// Package names that must be installed. These are looked up against the
    /// catalogue, not the module list — an unavailable package refuses the
    /// module even when every prerequisite is enabled.
    pub packages: &'static [&'static str],
    /// The module's entry point, as a package specifier.
    pub entry: &'static str,
    /// The permissions the module contributes.
    pub permissions: &'static [&'static str],
}

/// The 22 module contracts, in the order the baseline declares them.
///
/// This order is what `plan`'s `entries` output follows, so it is part of the
/// contract: a plan's shape is stable for a given selection.
pub const MODULE_CONTRACTS: [ModuleContract; 22] = [
    ModuleContract {
        id: M::Contacts,
        version: "1.0.0",
        requires: &[],
        packages: &["@archava/crm", "@archava/crm-pg", "@archava/identity-pg"],
        entry: "@archava/crm",
        permissions: &["contacts.read", "contacts.write"],
    },
    ModuleContract {
        id: M::Pipeline,
        version: "1.0.0",
        requires: &[M::Contacts],
        packages: &["@archava/crm"],
        entry: "@archava/crm/pipeline",
        permissions: &["pipeline.read", "pipeline.write"],
    },
    ModuleContract {
        id: M::Assistant,
        version: "1.0.0",
        requires: &[M::Contacts],
        packages: &["@archava/crm-assistant"],
        entry: "@archava/crm-assistant",
        permissions: &["contacts.read"],
    },
    ModuleContract {
        id: M::Projects,
        version: "1.0.0",
        requires: &[M::Contacts],
        packages: &[
            "@archava/projects",
            "@archava/projects-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/projects",
        permissions: &["projects.read", "projects.write"],
    },
    ModuleContract {
        id: M::Scheduling,
        version: "1.0.0",
        requires: &[M::Contacts],
        packages: &[
            "@archava/scheduling",
            "@archava/scheduling-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/scheduling",
        permissions: &[
            "resources.read",
            "resources.write",
            "reservations.read",
            "reservations.write",
        ],
    },
    ModuleContract {
        id: M::Accommodation,
        version: "1.0.0",
        requires: &[M::Contacts, M::Scheduling],
        packages: &[
            "@archava/accommodation",
            "@archava/accommodation-pg",
            "@archava/scheduling-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/accommodation",
        permissions: &["properties.read", "rooms.read", "stays.read", "stays.write"],
    },
    ModuleContract {
        id: M::NativeActions,
        version: "1.0.0",
        requires: &[M::Contacts, M::Assistant],
        packages: &["@archava/crm-act", "@archava/act-host-pg"],
        entry: "@archava/crm-act",
        permissions: &["followups.write"],
    },
    ModuleContract {
        id: M::Commerce,
        version: "1.0.0",
        requires: &[M::Contacts],
        packages: &[
            "@archava/commerce",
            "@archava/commerce-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/commerce",
        permissions: &["catalog.read", "stock.read", "orders.read", "orders.write"],
    },
    ModuleContract {
        id: M::Foodservice,
        version: "1.0.0",
        requires: &[M::Contacts, M::Commerce],
        packages: &[
            "@archava/foodservice",
            "@archava/foodservice-pg",
            "@archava/commerce-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/foodservice",
        permissions: &[
            "tables.read",
            "menu.read",
            "tickets.read",
            "tickets.write",
            "kitchen.write",
        ],
    },
    ModuleContract {
        id: M::Support,
        version: "1.0.0",
        requires: &[M::Contacts],
        packages: &[
            "@archava/support",
            "@archava/support-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/support",
        permissions: &[
            "cases.read",
            "cases.write",
            "cases.assign",
            "notes.read",
            "notes.write",
        ],
    },
    ModuleContract {
        id: M::Purchasing,
        version: "1.0.0",
        requires: &[M::Contacts, M::Commerce],
        packages: &[
            "@archava/purchasing",
            "@archava/purchasing-pg",
            "@archava/commerce-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/purchasing",
        permissions: &[
            "suppliers.read",
            "offers.read",
            "orders.read",
            "orders.write",
            "receipts.read",
            "receipts.write",
        ],
    },
    ModuleContract {
        id: M::Organizations,
        version: "1.0.0",
        requires: &[M::Contacts],
        packages: &[
            "@archava/organizations",
            "@archava/organizations-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/organizations",
        permissions: &[
            "organizations.read",
            "organizations.write",
            "affiliations.read",
            "affiliations.write",
        ],
    },
    ModuleContract {
        id: M::Proposals,
        version: "1.0.0",
        requires: &[M::Contacts],
        packages: &[
            "@archava/proposals",
            "@archava/proposals-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/proposals",
        permissions: &[
            "proposals.read",
            "proposals.write",
            "proposals.decide",
            "changerequests.read",
            "changerequests.write",
            "changerequests.decide",
        ],
    },
    ModuleContract {
        id: M::Capacity,
        version: "1.0.0",
        requires: &[M::Contacts],
        packages: &[
            "@archava/capacity",
            "@archava/capacity-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/capacity",
        permissions: &["capacity.read", "capacity.write", "capacity.allocate"],
    },
    ModuleContract {
        id: M::Assets,
        version: "1.0.0",
        requires: &[M::Contacts],
        packages: &[
            "@archava/assets",
            "@archava/assets-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/assets",
        permissions: &[
            "assets.read",
            "assets.write",
            "assets.handover",
            "assets.maintenance",
        ],
    },
    ModuleContract {
        id: M::Subscriptions,
        version: "1.0.0",
        requires: &[M::Contacts],
        packages: &[
            "@archava/subscriptions",
            "@archava/subscriptions-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/subscriptions",
        permissions: &[
            "subscriptions.read",
            "subscriptions.write",
            "subscriptions.payment",
            "subscriptions.usage",
        ],
    },
    ModuleContract {
        id: M::Attendance,
        version: "1.0.0",
        requires: &[M::Contacts],
        packages: &[
            "@archava/attendance",
            "@archava/attendance-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/attendance",
        permissions: &["attendance.read", "attendance.write", "attendance.checkin"],
    },
    ModuleContract {
        id: M::Fulfillment,
        version: "1.0.0",
        requires: &[M::Contacts],
        packages: &[
            "@archava/fulfillment",
            "@archava/fulfillment-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/fulfillment",
        permissions: &[
            "fulfillment.read",
            "fulfillment.write",
            "fulfillment.dispatch",
            "fulfillment.produce",
        ],
    },
    ModuleContract {
        id: M::Billing,
        version: "1.0.0",
        requires: &[M::Contacts],
        packages: &[
            "@archava/billing",
            "@archava/billing-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/billing",
        permissions: &["billing.read", "billing.write", "billing.settle"],
    },
    ModuleContract {
        id: M::Laundry,
        version: "1.0.0",
        requires: &[M::Contacts],
        packages: &[
            "@archava/laundry",
            "@archava/laundry-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/laundry",
        permissions: &["laundry.read", "laundry.write", "laundry.process"],
    },
    ModuleContract {
        id: M::FieldServices,
        version: "1.0.0",
        requires: &[M::Contacts],
        packages: &[
            "@archava/field-services",
            "@archava/field-services-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/field-services",
        permissions: &[
            "field.read",
            "field.write",
            "field.dispatch",
            "field.signoff",
            "audit.read",
        ],
    },
    ModuleContract {
        id: M::FormalEducation,
        version: "1.0.0",
        requires: &[M::Contacts],
        packages: &[
            "@archava/formal-education",
            "@archava/formal-education-pg",
            "@archava/identity-pg",
        ],
        entry: "@archava/formal-education",
        // Keep this vocabulary identical to EDUCATION_PERMISSIONS in the
        // TypeScript module; the assembly contract feeds role/config tooling.
        permissions: &[
            "education.read",
            "education.enroll",
            "education.record_session",
            "education.record_result",
            "education.review",
            "education.retain",
            "audit.read",
        ],
    },
];

impl ModuleContract {
    /// Look up a contract by id. `plan` iterates [`MODULE_CONTRACTS`] rather
    /// than calling this, because it needs the order anyway.
    pub fn find(id: ModuleId) -> Option<&'static ModuleContract> {
        MODULE_CONTRACTS.iter().find(|contract| contract.id == id)
    }
}

/// The assembly contract version each module was introduced in.
///
/// This is the other half of the gate and it is **not** the same table as
/// [`CONTRACT_VERSIONS`](crate::plan) — that one is indexed comparison, this one
/// is a per-module introduction version. Both are needed, and they answer
/// different questions:
///
/// * **this table** — *may a request at version `V` name module `M` at all?*
///   Keyed on the **request's** version. A request at `3.0.0` naming `commerce`
///   is refused whether the catalogue declares `3.0.0` or `17.0.0`.
/// * `CONTRACT_VERSIONS` — *can this catalogue serve a request at version `V`?*
///   Keyed on the **catalogue's** contract.
///
/// The baseline expresses the first inside the schema:
/// `assemblyRequestSchema` is a `z.discriminatedUnion('version', [...])` of 17
/// variants, and every variant wraps `moduleSelectionSchema` in a `.refine`
/// that refuses each module introduced after its own version.
///
/// ## Why the values are not `index + 1`
///
/// They are not, because modules did not arrive in id order. `nativeActions` is
/// a base module (`1.0.0`) but sits at index 6 — after `scheduling` (`3.0.0`),
/// `accommodation` (`4.0.0`) and `commerce` (`5.0.0`). `fulfillment` is
/// `13.0.0` and `billing` is `14.0.0`, but `laundry` is `12.0.0`, so the three
/// modules the baseline's own refine message groups together
/// (`'Fulfillment requires assembly contract 13.0.0'` refuses `fulfillment`,
/// `billing` and `laundry` at `12.0.0`) do not share an introduction version.
/// Deriving one from the other, in either direction, is wrong for at least four
/// modules.
///
/// ## Transcription
///
/// Each value is the version named in the refine message that first refuses the
/// module — which is `introduced - 1` in every case, since a module introduced
/// at `N.0.0` is first refused by the `N-1.0.0` variant's refine. Read
/// `MINIMUM_CONTRACT` as "the first version that admits the module".
/// `MINIMUM_CONTRACT` as "the first version that admits the module".
///
/// All 22 modules have a row — the four base modules included — so that the
/// `1.0.0` case is answered by the table rather than by an absence. A table
/// listing only the gated modules would leave `minimum_contract(Contacts)` as
/// `None`, and the gate would have to special-case the base four back in, which
/// is the same off-by-one-shaped hole in a different place.
pub const MINIMUM_CONTRACT: [(ModuleId, &str); 22] = [
    // The four base modules: `legacyModules` (`assembly.ts:308`) refuses every
    // optional module at `1.0.0`, so these are the ones it leaves in place.
    (M::Contacts, "1.0.0"),
    (M::Pipeline, "1.0.0"),
    (M::Assistant, "1.0.0"),
    (M::NativeActions, "1.0.0"),
    // Introduced one per contract, in contract order.
    (M::Projects, "2.0.0"),
    (M::Scheduling, "3.0.0"),
    (M::Accommodation, "4.0.0"),
    (M::Commerce, "5.0.0"),
    (M::Foodservice, "6.0.0"),
    (M::Support, "7.0.0"),
    (M::Purchasing, "8.0.0"),
    // The baseline's one three-module group: `'Organizations, proposals and
    // capacity require assembly contract 9.0.0'`. All three share a version,
    // which is why the row is three entries long and not one.
    (M::Organizations, "9.0.0"),
    (M::Proposals, "9.0.0"),
    (M::Capacity, "9.0.0"),
    (M::Assets, "10.0.0"),
    (M::Subscriptions, "11.0.0"),
    (M::Attendance, "12.0.0"),
    (M::Fulfillment, "13.0.0"),
    (M::Billing, "14.0.0"),
    // `laundry` is the odd one: refused by the `12.0.0` refine (which groups it
    // with `fulfillment` and `billing`) *and* again by the `14.0.0` refine
    // (`'Laundry requires assembly contract 15.0.0'`, which also refuses
    // `fieldServices` and `formalEducation`). The later of the two is what
    // decides, so `laundry` is `15.0.0`, not `13.0.0`. Getting this row wrong
    // silently admits the module one contract early.
    (M::Laundry, "15.0.0"),
    (M::FieldServices, "16.0.0"),
    (M::FormalEducation, "17.0.0"),
];

/// The first assembly contract version that admits `id`.
///
/// `None` only for an id this crate does not model, which cannot happen for
/// [`ModuleId::ALL`]; see [`MINIMUM_CONTRACT`] for why its values are written
/// out rather than derived.
pub fn minimum_contract(id: ModuleId) -> Option<&'static str> {
    MINIMUM_CONTRACT
        .iter()
        .find(|(candidate, _)| *candidate == id)
        .map(|(_, version)| *version)
}
