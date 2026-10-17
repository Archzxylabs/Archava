//! What can refuse a plan, and what each refusal says.
//!
//! Two groups, because the baseline has two groups and they arrive at different
//! times.
//!
//! **Parse-time** — [`AssemblyError::MissingCore`] and
//! [`AssemblyError::ModuleDependency`] are the schema's own `superRefine` and
//! `literal(true)` checks. In the baseline these fire inside
//! `assemblyRequestSchema.parse(raw)` at `assembly.ts:620`, before any
//! catalogue is consulted.
//!
//! **Plan-time** — the other four are `throw new Error(...)` inside
//! `planAssembly` itself, in this order: profile (`:621`), contract version
//! (`:674`), prerequisite (`:678`), package availability (`:680`).
//!
//! The port keeps the order because a caller that handles one specific refusal
//! must be able to tell them apart, and because the order is observable: a
//! request that is both profile-mismatched and missing a prerequisite reports
//! the profile, exactly as the baseline does.
//!
//! ## Message parity
//!
//! [`Display`](std::fmt::Display) renders the baseline's four strings
//! **verbatim** — same words, same casing, no trailing period — because they
//! are asserted against by `packages/config/test/assembly.test.ts` and anything
//! that switches on a message must keep matching. Variants that carry a fact
//! the baseline had to throw away (`Disabled module prerequisite` names which
//! module and dependency; `Unsupported assembly contract` names both versions)
//! still *start* with that string, so a `starts_with` check on either side
//! holds and the extra detail is a superset, not a rewrite.
//!
//! The parse-time variants have no baseline string to match: Zod's
//! `superRefine` issues carry a path and a code, not a message, and this port
//! is the only place they surface as text.

use crate::request::ModuleId;

/// Why a plan was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssemblyError {
    /// `contacts` was false, or absent in a shape that deserialized to false.
    ///
    /// The baseline refuses this at parse time with `z.literal(true)`, so it
    /// never reaches the catalogue. See [`crate::request`] for why this port
    /// enforces it in `validate` instead.
    MissingCore,

    /// A module was enabled whose prerequisite module was not.
    ///
    /// Parse-time: the schema's `superRefine` on `moduleSelectionSchema`. The
    /// five pairs it can name are exactly the ones
    /// [`ModuleSelection::validate`](crate::request::ModuleSelection::validate)
    /// checks — `nativeActions`→`assistant`, `purchasing`→`commerce`,
    /// `foodservice`→`commerce`, `accommodation`→`scheduling`, and the core case
    /// above.
    ModuleDependency {
        /// The module that was enabled.
        module: ModuleId,
        /// The module that was required and was not enabled.
        dependency: ModuleId,
    },

    /// A module was named at a request version that predates it.
    ///
    /// Parse-time, and the check most easily lost in a port. Each of the 17
    /// `assemblyRequestSchema` variants wraps `moduleSelectionSchema` in a
    /// `.refine` that refuses every module introduced *after* that version, so
    /// the gate is keyed on the **request's own version**, not on the
    /// catalogue's. A request at `3.0.0` naming `commerce` is refused by the
    /// schema whether the catalogue declares `3.0.0` or `17.0.0` — this is what
    /// stops a newer module reaching a host that predates its migration, and it
    /// is separate from [`AssemblyError::UnsupportedContract`], which is about
    /// what the catalogue can serve.
    ///
    /// ## Message parity
    ///
    /// The baseline's 17 refine messages cannot be rendered byte-for-byte from
    /// a per-module error, because four of them name a *group*
    /// (`'Organizations, proposals and capacity require assembly contract
    /// 9.0.0'`) and six use a display name that differs from the module id
    /// (`'Food service requires assembly contract 6.0.0'` for `foodservice`,
    /// `'Asset register requires assembly contract 10.0.0'` for `assets`). This
    /// renders `"{module} requires assembly contract {required}"`, which
    /// reproduces the shape and the version but reads the module id. A caller
    /// that needs the baseline's exact string needs the baseline; a caller that
    /// switches on the *refusal* is unaffected.
    ModuleContractUnavailable {
        /// The module that was named.
        module: ModuleId,
        /// The earliest assembly contract that carries it, e.g. `"9.0.0"`.
        required: String,
    },

    /// The request's `profile` did not equal the catalogue's `version`.
    ///
    /// The one field-to-field comparison `planAssembly` makes before anything
    /// else: a plan is always for one catalogue, and a request naming a
    /// different profile is refused rather than silently re-targeted.
    UnsupportedProfile {
        /// What the request asked for.
        requested: String,
        /// What the catalogue is.
        available: String,
    },

    /// The request's assembly contract version is newer than the catalogue's
    /// `contracts.assembly`, or the catalogue does not declare one.
    ///
    /// An absent (`None`) catalogue contract refuses **every** request except
    /// `1.0.0`, which is what the baseline's
    /// `!validVersions.slice(N).includes(catalog.contracts.assembly ?? '')`
    /// does with the empty string. The baseline cannot say which version it
    /// needed; this variant carries both so the caller can.
    UnsupportedContract {
        /// The assembly contract the request targets, e.g. `"14.0.0"`.
        requested: String,
        /// The catalogue's `contracts.assembly`, or `None` if absent.
        available: Option<String>,
    },

    /// A selected module's prerequisite module was not selected.
    ///
    /// Plan-time, and a different check from
    /// [`AssemblyError::ModuleDependency`]: the schema's `superRefine` already
    /// covers the four pairs above, but the contract table in
    /// [`crate::contracts`] carries `requires` for all 22 modules, so this is
    /// the check that would catch a contract whose prerequisite pair the
    /// schema's `superRefine` does not already name.
    DisabledModulePrerequisite {
        /// The module that was selected.
        module: ModuleId,
        /// The prerequisite that was not selected.
        dependency: ModuleId,
    },

    /// A selected module names a package the catalogue does not ship.
    ///
    /// The availability half of `planAssembly`'s per-module loop: a module with
    /// every prerequisite enabled still cannot be installed if its artifact is
    /// not in the catalogue, and an unavailable package refuses the whole plan
    /// rather than producing one that cannot be executed.
    ModuleImplementationUnavailable {
        /// The module whose package is missing.
        module: ModuleId,
        /// The package name the module requires and the catalogue lacks.
        package: String,
    },
}

impl std::fmt::Display for AssemblyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssemblyError::MissingCore => {
                write!(f, "contacts is required and cannot be disabled")
            }
            AssemblyError::ModuleDependency { module, dependency } => {
                write!(f, "{module} requires {dependency} to be enabled")
            }
            AssemblyError::ModuleContractUnavailable { module, required } => {
                write!(f, "{module} requires assembly contract {required}")
            }
            AssemblyError::UnsupportedProfile {
                requested,
                available,
            } => {
                write!(
                    f,
                    "Unsupported assembly profile: requested {requested}, catalogue provides {available}"
                )
            }
            AssemblyError::UnsupportedContract {
                requested,
                available,
            } => match available {
                Some(available) => write!(
                    f,
                    "Unsupported assembly contract: requested {requested}, catalogue provides {available}"
                ),
                None => write!(
                    f,
                    "Unsupported assembly contract: requested {requested}, catalogue declares none"
                ),
            },
            AssemblyError::DisabledModulePrerequisite { module, dependency } => {
                write!(f, "Disabled module prerequisite: {module} requires {dependency}")
            }
            AssemblyError::ModuleImplementationUnavailable { module, package } => {
                write!(
                    f,
                    "Module implementation unavailable: {module} requires {package}"
                )
            }
        }
    }
}

impl std::error::Error for AssemblyError {}
