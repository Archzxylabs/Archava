//! `plan` — the entry point, and the shape it returns.
//!
//! A port of `planAssembly` from `packages/config/src/assembly.ts:618-697`.
//!
//! ## The order is the contract
//!
//! Five checks, in this exact order, because the baseline performs them in this
//! order and a caller that handles one refusal must be able to tell which one
//! it got. A request that is both profile-mismatched and dependency-broken
//! reports the profile, exactly as the baseline does.
//!
//! 1. `modules.validate()` — the schema's own `superRefine` rules. In the
//!    baseline this runs at parse time (`assembly.ts:620`), before
//!    `planAssembly` proceeds; here it runs first inside `plan` because `plan`
//!    takes deserialized values.
//! 2. the per-module version gate — every named module must be one the
//!    request's own version already carries.
//! 3. `request.profile === catalog.version` — `assembly.ts:621`, the first
//!    thing `planAssembly` itself checks.
//! 4. the request's `version` is one the discriminated union accepts.
//! 5. the catalogue's `contracts.assembly` covers the request's version, then
//!    every selected module's prerequisites are selected and its packages exist
//!    — one loop, per module, in the baseline's order.
//!
//! Steps 2 and 3 are ordered to match *where the baseline does them*, not for
//! convenience: the schema (and therefore 2) runs at `assembly.ts:620`, and
//! `planAssembly`'s first statement is the profile check. A port that ran the
//! profile check first would report `UnsupportedProfile` for a request that
//! also names a module it predates, and the two refusals are not
//! interchangeable — one is a deployment targeting the wrong catalogue, the
//! other is a request naming something that does not exist yet.
//!
//! Checks 4 and 5 are what `z.discriminatedUnion('version', [...])` does: an
//! unknown `version` literal never gets past Zod at all, so `planAssembly` is
//! never called. This port cannot put the check in a type, so it runs as a
//! plain string comparison. The observable difference is the error's *shape* —
//! Zod reports an `invalid_union_discriminator` naming the discriminator and
//! the allowed values; this port reports [`AssemblyError::UnsupportedContract`]
//! carrying the requested version and whatever the catalogue provides. Same
//! refusal, different text, recorded here rather than smoothed over.
//!
//! ## Why the version gate is a table
//!
//! The gate has to know, for each of the 17 assembly contract versions, which
//! modules are allowed. That is not arithmetic, in two independent ways:
//!
//! * the **request-side** gate is per-module — see [`MINIMUM_CONTRACT`] for why
//!   `laundry` is `15.0.0` when three modules share a refine at `12.0.0`;
//! * the **catalogue-side** gate is an index comparison against
//!   [`CONTRACT_VERSIONS`], which reproduces the baseline's
//!   `validVersions.slice(N).includes(...)` — the *slice* is what makes the
//!   comparison what it is.
//!
//! `12.0.0` allows only `fulfillment` and `billing` beyond `11.0.0`; `14.0.0`
//! allows `laundry`, `fieldServices` and `formalEducation`. So a request for
//! `fieldServices` at contract `13.0.0` must be refused, and an `accommodation`
//! request at `1.0.0` must be refused. Deriving the allowed set from an index
//! would accept them, silently, on every machine, forever.

use crate::catalog::ReleaseCatalog;
use crate::contracts::minimum_contract;
use crate::contracts::ModuleContract;
use crate::contracts::MODULE_CONTRACTS;
use crate::error::AssemblyError;
use crate::request::{AssemblyRequest, ModuleId, ModuleSelection};
use serde::{Deserialize, Serialize};

/// The 16 assembly contract versions `planAssembly` gates on, in the order
/// `validVersions` declares them — **`2.0.0` through `17.0.0`, not `1.0.0`**.
///
/// This is the list the slices are taken over, so its first entry is `2.0.0`
/// and `slice(N)` for a request at `N + 1` has that request's version at its
/// head. `1.0.0` is deliberately absent: the baseline's gate has no
/// `request.version === '1.0.0'` clause at all, so a `1.0.0` request is
/// admitted unconditionally — which is *not* the same as being gated at the
/// floor, and modelling it as the floor would refuse or admit the wrong
/// catalogues depending on which side of the off-by-one it landed.
///
/// Both the request's `version` and the catalogue's `contracts.assembly` are
/// looked up here, and the *index* is what the gate compares: a request at
/// index `N` is admitted when the catalogue's contract is at index `N` or
/// later. A catalogue declaring a version not on this list — or none at all —
/// has nothing it can offer except to a `1.0.0` request.
const CONTRACT_VERSIONS: [&str; 16] = [
    "2.0.0", "3.0.0", "4.0.0", "5.0.0", "6.0.0", "7.0.0", "8.0.0", "9.0.0", "10.0.0", "11.0.0",
    "12.0.0", "13.0.0", "14.0.0", "15.0.0", "16.0.0", "17.0.0",
];

/// The index of `version` in [`CONTRACT_VERSIONS`], or `None`.
fn contract_index(version: &str) -> Option<usize> {
    CONTRACT_VERSIONS.iter().position(|v| *v == version)
}

/// The 17 accepted request versions — `1.0.0` plus the gated 16.
///
/// What the discriminated union accepts. Kept separate from
/// [`CONTRACT_VERSIONS`] because the two lists differ, and collapsing them is
/// exactly the bug the off-by-one above describes.
const REQUEST_VERSIONS: [&str; 17] = [
    "1.0.0", "2.0.0", "3.0.0", "4.0.0", "5.0.0", "6.0.0", "7.0.0", "8.0.0", "9.0.0", "10.0.0",
    "11.0.0", "12.0.0", "13.0.0", "14.0.0", "15.0.0", "16.0.0", "17.0.0",
];

/// Whether `version` is one the discriminated union accepts.
fn is_request_version(version: &str) -> bool {
    REQUEST_VERSIONS.contains(&version)
}

/// One module in the resolved plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanEntry {
    /// The module's identifier, as the schema spells it.
    pub id: String,
    /// The module's entry point, from its contract.
    pub entry: String,
    /// The module's own contract version, `1.0.0` on all 22 today.
    pub version: String,
}

/// One artifact the plan installs, in catalogue order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
}

/// What a plan resolves to.
///
/// The field set mirrors `planAssembly`'s return object (`assembly.ts:682-696`)
/// one for one, including the two string literals — they are part of the shape
/// and not this port's invention, so a consumer rendering a plan does not need
/// to know which implementation produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssemblyPlan {
    /// The request's own version, echoed back.
    pub version: String,
    /// The catalogue's version — the profile this plan is for.
    pub profile: String,
    /// The module selection the request asked for, echoed back.
    pub modules: ModuleSelection,
    /// The resolved modules, in [`MODULE_CONTRACTS`] order.
    pub entries: Vec<PlanEntry>,
    /// Every artifact the catalogue ships, in catalogue order. This is what the
    /// plan installs, and it is *not* filtered by the selection: a deployment
    /// takes the catalogue it is given.
    pub installation: Vec<InstalledPackage>,
    /// Names the baseline knows are gone and refuses to resolve. Carried
    /// through verbatim — an operator reading a plan sees exactly which legacy
    /// identifiers to stop looking for.
    #[serde(rename = "unresolvedLegacyNames")]
    pub unresolved_legacy_names: Vec<String>,
    /// `configuration-only`, always. A plan configures what is installed; it
    /// does not decide who may act.
    pub authority: String,
    /// `retain-data-audit-receipts; refuse disable with unresolved native
    /// effects`, always.
    #[serde(rename = "dataPolicy")]
    pub data_policy: String,
}

/// Resolve a request against a catalogue, or say why it cannot be resolved.
///
/// Both arguments are expected to have been validated at their boundary — the
/// request by whatever produced it, the catalogue by the configuration layer's
/// `releaseCatalogSchema`. See [`crate`] on the schema-validation boundary this
/// crate does not reproduce.
pub fn plan(
    request: &AssemblyRequest,
    catalog: &ReleaseCatalog,
) -> Result<AssemblyPlan, AssemblyError> {
    // 1. The schema's own selection rules, first.
    request.modules.validate()?;

    // 2. The per-module version gate, before the profile check for the reason
    //    the module doc gives: the schema runs at `assembly.ts:620`, so this is
    //    the first thing a request is judged on.
    //
    //    `moduleSelectionSchema`'s own `superRefine` (four prerequisite pairs,
    //    handled above) is shared by all 17 variants. This is the check that is
    //    *not* shared: each variant adds the modules that were available by
    //    then and refuses every later one, so the gate is keyed on
    //    `request.version` and reads the selection's raw slots — `Some(false)`
    //    is refused exactly as `Some(true)` is, because the baseline's refine
    //    reads `=== undefined`.
    //
    //    Iterating MODULE_CONTRACTS rather than the request's fields is what
    //    keeps the reported module deterministic: the first offending module is
    //    the earliest in contract order, so two requests with the same set of
    //    offending modules always produce the same error.
    for contract in &MODULE_CONTRACTS {
        if request.modules.get(contract.id).is_none() {
            continue;
        }
        let minimum = minimum_contract(contract.id).unwrap_or(contract.version);
        if contract_index(minimum).is_some_and(|minimum| {
            contract_index(&request.version).is_none_or(|requested| requested < minimum)
        }) {
            return Err(AssemblyError::ModuleContractUnavailable {
                module: contract.id,
                required: minimum.to_string(),
            });
        }
    }

    // 3. Profile match — the first thing `planAssembly` itself checks
    //    (`assembly.ts:621`), one field-to-field equality and nothing else.
    if request.profile != catalog.version {
        return Err(AssemblyError::UnsupportedProfile {
            requested: request.profile.clone(),
            available: catalog.version.clone(),
        });
    }

    // 4. The request's `version` must be one the baseline's discriminated union
    //    accepts. An unknown literal never reaches `planAssembly` there, so this
    //    port refuses it here rather than reporting a contract mismatch that
    //    would name a version that was never valid in the first place.
    if !is_request_version(&request.version) {
        return Err(AssemblyError::UnsupportedContract {
            requested: request.version.clone(),
            available: catalog.contracts.assembly.clone(),
        });
    }

    // 5. The catalogue's contract must cover the request. This reproduces the
    //    baseline's `validVersions.slice(N).includes(assembly ?? '')`: a request
    //    at index `N` is admitted when the catalogue's contract is at index `N`
    //    or later in the same list.
    //
    //    `1.0.0` has no clause in the baseline's gate at all — `validVersions`
    //    starts at `2.0.0`, so there is no `request.version === '1.0.0'` arm to
    //    reach — and it is admitted unconditionally. It is therefore checked
    //    *before* the index lookup: `contract_index("1.0.0")` is `None` here,
    //    which is the same `None` an absent catalogue contract produces, and
    //    folding the two together would refuse a request the baseline admits.
    //
    //    An absent (`None`) catalogue contract is the `?? ''` case — the empty
    //    string is on no list, so every gated request is refused, while `1.0.0`
    //    still passes.
    let allowed = if !is_request_version(&request.version) {
        false
    } else if contract_index(&request.version).is_none() {
        true
    } else {
        match catalog.contracts.assembly.as_deref() {
            Some(provided) => match (contract_index(&request.version), contract_index(provided)) {
                (Some(requested_index), Some(provided_index)) => provided_index >= requested_index,
                // A version the list does not carry is on no slice, so it is
                // never in one.
                _ => false,
            },
            None => false,
        }
    };
    if !allowed {
        return Err(AssemblyError::UnsupportedContract {
            requested: request.version.clone(),
            available: catalog.contracts.assembly.clone(),
        });
    }

    // The selected modules, in contract order. The order is part of the plan's
    // shape, so this is a filter over the table rather than a fold over the
    // request.
    let selected: Vec<&'static ModuleContract> = MODULE_CONTRACTS
        .iter()
        .filter(|contract| request.modules.selected(contract.id))
        .collect();

    // 6. Prerequisites and package availability, in the one loop the baseline
    //    uses — both checks run per module, prerequisite first.
    //    `assembly.ts:676-681` iterates `selected` once and does both inside the
    //    body, so a module that breaks both reports its prerequisite. Two
    //    separate passes over the same list would report the *other* module's
    //    package failure when that module came first, and the difference is
    //    invisible until a caller switches on it.
    for contract in &selected {
        for dependency in contract.requires {
            if !request.modules.selected(*dependency) {
                return Err(AssemblyError::DisabledModulePrerequisite {
                    module: contract.id,
                    dependency: *dependency,
                });
            }
        }
        for package in contract.packages {
            if !catalog
                .packages
                .iter()
                .any(|candidate| candidate.name == *package)
            {
                return Err(AssemblyError::ModuleImplementationUnavailable {
                    module: contract.id,
                    package: (*package).to_string(),
                });
            }
        }
    }

    Ok(AssemblyPlan {
        version: request.version.clone(),
        profile: catalog.version.clone(),
        modules: request.modules.clone(),
        entries: selected
            .iter()
            .map(|contract| PlanEntry {
                id: contract.id.as_str().to_string(),
                entry: contract.entry.to_string(),
                version: contract.version.to_string(),
            })
            .collect(),
        installation: catalog
            .packages
            .iter()
            .map(|pkg| InstalledPackage {
                name: pkg.name.clone(),
                version: pkg.version.clone(),
            })
            .collect(),
        unresolved_legacy_names: ["crm_core", "payment_gateway", "inventory", "booking", "erp"]
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
        authority: "configuration-only".to_string(),
        data_policy: "retain-data-audit-receipts; refuse disable with unresolved native effects"
            .to_string(),
    })
}

impl AssemblyPlan {
    /// The module ids this plan resolved, in contract order.
    ///
    /// The plan carries ids as strings because that is what a serialized plan
    /// looks like on the wire; this is the typed view for a caller that already
    /// works in [`ModuleId`]. An id that no longer names a module is dropped
    /// rather than guessed at.
    pub fn selected_ids(&self) -> Vec<ModuleId> {
        self.entries
            .iter()
            .filter_map(|entry| ModuleId::parse(&entry.id))
            .collect()
    }
}
