//! The assembly request and its module selection.
//!
//! A port of `moduleSelectionSchema` from
//! `packages/config/src/assembly.ts:4-38`.
//!
//! ## Why `contacts` is a `bool` here and a `literal(true)` in the schema
//!
//! The baseline types it `z.literal(true)` — the only field whose type differs
//! from every other's. That is not a modelling accident: a deployment without
//! contacts has no tenant surface at all, so the schema refuses it at parse
//! time rather than letting `plan` refuse it later.
//!
//! This port cannot express the literal in the type system and still accept a
//! deserialized request, so `contacts: bool` carries the same rule enforced in
//! [`ModuleSelection::validate`] instead. The difference is in *where* the
//! refusal happens — at parse for Zod, at the first check for this port — and
//! not in whether it happens. The parity harness compares the outcome, which is
//! the refusal; the baseline's own test fixture for a missing core
//! (`{ ...FULL_MODULES, contacts: false }`) expects a throw, and this port
//! returns [`AssemblyError::MissingCore`].
//!
//! The `nativeActions` / `purchasing` / `foodservice` / `accommodation`
//! dependencies are the schema's `superRefine`, moved into the same
//! `validate` for the same reason.

use serde::{Deserialize, Serialize};

use crate::error::AssemblyError;

/// A module identifier, in the order [`MODULE_CONTRACTS`] declares them.
///
/// The order matters and is not incidental: `plan` emits `entries` in contract
/// order, so `contacts` is always first and a plan's shape is stable for a
/// given selection regardless of how the request was serialized.
///
/// [`MODULE_CONTRACTS`]: crate::contracts::MODULE_CONTRACTS
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ModuleId {
    Contacts,
    Pipeline,
    Assistant,
    Projects,
    Scheduling,
    Accommodation,
    NativeActions,
    Commerce,
    Foodservice,
    Support,
    Purchasing,
    Organizations,
    Proposals,
    Capacity,
    Assets,
    Subscriptions,
    Attendance,
    Fulfillment,
    Billing,
    Laundry,
    FieldServices,
    FormalEducation,
}

impl ModuleId {
    /// Every module, in contract order. Used by the default `ModuleSelection`
    /// and by parity case generation.
    pub const ALL: [ModuleId; 22] = [
        ModuleId::Contacts,
        ModuleId::Pipeline,
        ModuleId::Assistant,
        ModuleId::Projects,
        ModuleId::Scheduling,
        ModuleId::Accommodation,
        ModuleId::NativeActions,
        ModuleId::Commerce,
        ModuleId::Foodservice,
        ModuleId::Support,
        ModuleId::Purchasing,
        ModuleId::Organizations,
        ModuleId::Proposals,
        ModuleId::Capacity,
        ModuleId::Assets,
        ModuleId::Subscriptions,
        ModuleId::Attendance,
        ModuleId::Fulfillment,
        ModuleId::Billing,
        ModuleId::Laundry,
        ModuleId::FieldServices,
        ModuleId::FormalEducation,
    ];

    /// The identifier as the schema spells it. `fieldServices` and
    /// `formalEducation` are camelCase, which is what the JSON says.
    pub fn as_str(self) -> &'static str {
        match self {
            ModuleId::Contacts => "contacts",
            ModuleId::Pipeline => "pipeline",
            ModuleId::Assistant => "assistant",
            ModuleId::Projects => "projects",
            ModuleId::Scheduling => "scheduling",
            ModuleId::Accommodation => "accommodation",
            ModuleId::NativeActions => "nativeActions",
            ModuleId::Commerce => "commerce",
            ModuleId::Foodservice => "foodservice",
            ModuleId::Support => "support",
            ModuleId::Purchasing => "purchasing",
            ModuleId::Organizations => "organizations",
            ModuleId::Proposals => "proposals",
            ModuleId::Capacity => "capacity",
            ModuleId::Assets => "assets",
            ModuleId::Subscriptions => "subscriptions",
            ModuleId::Attendance => "attendance",
            ModuleId::Fulfillment => "fulfillment",
            ModuleId::Billing => "billing",
            ModuleId::Laundry => "laundry",
            ModuleId::FieldServices => "fieldServices",
            ModuleId::FormalEducation => "formalEducation",
        }
    }

    /// Read one back from the spelling [`ModuleId::as_str`] produces, for a
    /// selection that arrived as JSON rather than as Rust values.
    ///
    /// `Option`, not `Result`: an id that names no module is a caller asking
    /// about something this crate does not model, which is not an error worth a
    /// variant. This is why it is named `parse` and not `from_str` — the latter
    /// would read as [`std::str::FromStr`], whose `Err` type this function has
    /// nothing to put in it.
    pub fn parse(name: &str) -> Option<ModuleId> {
        ModuleId::ALL.into_iter().find(|id| id.as_str() == name)
    }
}

impl std::fmt::Display for ModuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which modules a request asks for.
///
/// The type distinguishes **absent** from **false**, and that is not pedantry —
/// it is what makes the version gate work at all. The baseline's gate is
/// `modules.foodservice === undefined`, so a selection that says
/// `foodservice: false` at contract `4.0.0` is *refused*, while an absent one
/// plans fine. A `HashMap<ModuleId, bool>` would collapse those two and
/// silently accept a request the baseline rejects.
///
/// Serde therefore gives every optional field
/// `#[serde(skip_serializing_if = "Option::is_none")]`, so a round-trip
/// preserves the distinction.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleSelection {
    /// `z.literal(true)` in the schema; enforced by [`ModuleSelection::validate`].
    pub contacts: bool,
    pub pipeline: bool,
    pub assistant: bool,
    #[serde(rename = "nativeActions")]
    pub native_actions: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projects: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduling: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accommodation: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commerce: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foodservice: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purchasing: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organizations: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposals: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscriptions: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attendance: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fulfillment: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub billing: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub laundry: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "fieldServices")]
    pub field_services: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "formalEducation")]
    pub formal_education: Option<bool>,
}

impl ModuleSelection {
    /// A selection with only the four base modules, all disabled except
    /// contacts.
    pub fn minimal() -> Self {
        Self {
            contacts: true,
            pipeline: false,
            assistant: false,
            native_actions: false,
            ..Self::default()
        }
    }

    /// A selection with the four base modules enabled and nothing else.
    ///
    /// The baseline's `FULL_MODULES` is `{contacts, pipeline, assistant,
    /// nativeActions}` — the shared name is `FULL` but the value is the base
    /// four, because every optional module is absent rather than false.
    pub fn full() -> Self {
        Self {
            contacts: true,
            pipeline: true,
            assistant: true,
            native_actions: true,
            ..Self::default()
        }
    }

    /// [`ModuleSelection::full`] with every optional module enabled.
    pub fn composable() -> Self {
        Self {
            contacts: true,
            pipeline: true,
            assistant: true,
            native_actions: true,
            projects: Some(true),
            scheduling: Some(true),
            accommodation: Some(true),
            commerce: Some(true),
            foodservice: Some(true),
            support: Some(true),
            purchasing: Some(true),
            organizations: Some(true),
            proposals: Some(true),
            capacity: Some(true),
            assets: Some(true),
            subscriptions: Some(true),
            attendance: Some(true),
            fulfillment: Some(true),
            billing: Some(true),
            laundry: Some(true),
            field_services: Some(true),
            formal_education: Some(true),
        }
    }

    /// Whether a module is selected. `None` is not selected, exactly as the
    /// schema's `=== undefined` reads.
    pub fn selected(&self, id: ModuleId) -> bool {
        self.get(id).unwrap_or(false)
    }

    /// The module's raw slot: `None` for absent, `Some(false)` for explicit
    /// false. This is the accessor the version gate needs — it must tell
    /// "absent" from "false" — so it is public rather than collapsed into
    /// [`ModuleSelection::selected`].
    pub fn get(&self, id: ModuleId) -> Option<bool> {
        match id {
            ModuleId::Contacts => Some(self.contacts),
            ModuleId::Pipeline => Some(self.pipeline),
            ModuleId::Assistant => Some(self.assistant),
            ModuleId::Projects => self.projects,
            ModuleId::Scheduling => self.scheduling,
            ModuleId::Accommodation => self.accommodation,
            ModuleId::NativeActions => Some(self.native_actions),
            ModuleId::Commerce => self.commerce,
            ModuleId::Foodservice => self.foodservice,
            ModuleId::Support => self.support,
            ModuleId::Purchasing => self.purchasing,
            ModuleId::Organizations => self.organizations,
            ModuleId::Proposals => self.proposals,
            ModuleId::Capacity => self.capacity,
            ModuleId::Assets => self.assets,
            ModuleId::Subscriptions => self.subscriptions,
            ModuleId::Attendance => self.attendance,
            ModuleId::Fulfillment => self.fulfillment,
            ModuleId::Billing => self.billing,
            ModuleId::Laundry => self.laundry,
            ModuleId::FieldServices => self.field_services,
            ModuleId::FormalEducation => self.formal_education,
        }
    }

    /// The schema's own selection rules, ported one for one.
    ///
    /// Each of the four is a `superRefine` issue in the baseline, so each is
    /// reported separately rather than as one combined message: Zod collects all
    /// of them, and a caller handling one specific case needs to tell them
    /// apart.
    pub fn validate(&self) -> Result<(), AssemblyError> {
        if !self.contacts {
            return Err(AssemblyError::MissingCore);
        }
        if self.native_actions && !self.assistant {
            return Err(AssemblyError::ModuleDependency {
                module: ModuleId::NativeActions,
                dependency: ModuleId::Assistant,
            });
        }
        if self.purchasing == Some(true) && self.commerce != Some(true) {
            return Err(AssemblyError::ModuleDependency {
                module: ModuleId::Purchasing,
                dependency: ModuleId::Commerce,
            });
        }
        if self.foodservice == Some(true) && self.commerce != Some(true) {
            return Err(AssemblyError::ModuleDependency {
                module: ModuleId::Foodservice,
                dependency: ModuleId::Commerce,
            });
        }
        if self.accommodation == Some(true) && self.scheduling != Some(true) {
            return Err(AssemblyError::ModuleDependency {
                module: ModuleId::Accommodation,
                dependency: ModuleId::Scheduling,
            });
        }
        Ok(())
    }
}

/// An assembly request.
///
/// A port of one `assemblyRequestSchema` variant's shape: `version` is the
/// assembly contract the request targets, `profile` is the release profile it
/// must match, and `modules` is what to install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssemblyRequest {
    /// The assembly contract version, e.g. `"14.0.0"`. The baseline's
    /// discriminated union accepts exactly `1.0.0` through `17.0.0`.
    pub version: String,
    /// The release profile this plan targets. Must equal the catalogue's
    /// `version` verbatim, or the plan is refused.
    pub profile: String,
    pub modules: ModuleSelection,
}
