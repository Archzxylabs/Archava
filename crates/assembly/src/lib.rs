//! `archava-assembly` — deployment and assembly planning, ported.
//!
//! A translation of `packages/config/src/assembly.ts`, which is the only place
//! this logic has ever existed. The port answers the same questions the
//! TypeScript does, in the same order, with the same error strings:
//!
//! 1. **Is this request itself well-formed?** (`modules` validates its own
//!    internal dependencies, and the version's contract gate rejects a module
//!    that did not exist yet at that assembly contract version.)
//! 2. **Does the release catalogue support it?** (`profile` matching, contract
//!    version gate, then per-module prerequisite and package-availability
//!    checks.)
//!
//! What is deliberately NOT reproduced:
//!
//! - **Zod.** The baseline parses `raw: unknown` and `trustedCatalog:
//!   ReleaseCatalog` through `assemblyRequestSchema` and `releaseCatalogSchema`.
//!   This port takes **already-validated** values and leaves schema validation
//!   at the configuration layer's boundary — the same choice
//!   `archava-pricing` documents in its `lib.rs`. Porting a schema library the
//!   baseline did not write would add a dependency and a second vocabulary of
//!   error strings for no behaviour change.
//!
//!   The distinction this leaves behind is recorded, not papered over: a caller
//!   that passes an unvalidated request gets *Rust's* rejection rather than
//!   Zod's path-shaped messages. Each type's documentation names exactly which
//!   fields the schema would have constrained. The parity harness is what
//!   decides whether that difference shows, and a caller needing Zod-identical
//!   messages is a compatibility bridge, not a port.
//!
//! ## The one inherited shape worth naming
//!
//! The contract gate is the part most likely to be "simplified" wrong. It is
//! spelled out as a table against the baseline's own list rather than as an
//! arithmetic derivation, because the baseline's gate is not arithmetic in two
//! independent ways, and a formula would silently accept a request it should
//! refuse in both.
//!
//! **Per-module, keyed on the request's version.** Each
//! `assemblyRequestSchema` variant refuses the modules that did not exist yet
//! at its own version, so a request at `3.0.0` naming `commerce` is refused
//! whether the catalogue declares `3.0.0` or `17.0.0`. See
//! [`contracts::MINIMUM_CONTRACT`] for the table.
//!
//! **Index comparison, keyed on the catalogue's contract.** A request at `N` is
//! admitted when the catalogue declares `N` or later. See
//! [`plan`]'s `CONTRACT_VERSIONS`.
//!
//! The two are not derivable from each other. `laundry` is refused by the
//! `12.0.0` refine *and again* by the `14.0.0` one, so it needs `15.0.0`; and
//! `nativeActions` is a base module sitting at contract index 6, after three
//! modules that arrived after it.
//!
//! | module | first admitted at assembly contract |
//! |---|---|
//! | `contacts`, `pipeline`, `assistant`, `nativeActions` | `1.0.0` |
//! | `projects` | `2.0.0` |
//! | `scheduling` | `3.0.0` |
//! | `accommodation` | `4.0.0` |
//! | `commerce` | `5.0.0` |
//! | `foodservice` | `6.0.0` |
//! | `support` | `7.0.0` |
//! | `purchasing` | `8.0.0` |
//! | `organizations`, `proposals`, `capacity` | `9.0.0` |
//! | `assets` | `10.0.0` |
//! | `subscriptions` | `11.0.0` |
//! | `attendance` | `12.0.0` |
//! | `fulfillment` | `13.0.0` |
//! | `billing` | `14.0.0` |
//! | `laundry` | `15.0.0` |
//! | `fieldServices` | `16.0.0` |
//! | `formalEducation` | `17.0.0` |
//!
//! The three-module row is the baseline's own grouping, quoted from its refine
//! message, not this port's tidying. `laundry` is the row to get right: it is
//! the one module the baseline refuses twice, and the later refusal decides.
//!
//! ## Layout
//!
//! | module | role |
//! |---|---|
//! | [`catalog`] | `ReleaseCatalog` and friends — the shapes this port reads |
//! | [`request`] | `AssemblyRequest` and the module selection |
//! | [`contracts`] | `MODULE_CONTRACTS` — what each module needs and ships |
//! | [`error`] | `AssemblyError` and its message parity |
//! | [`plan`] | `plan()` — the entry point |

pub mod catalog;
pub mod contracts;
pub mod error;
pub mod plan;
pub mod request;

pub use catalog::ReleaseCatalog;
pub use contracts::{minimum_contract, ModuleContract, MINIMUM_CONTRACT, MODULE_CONTRACTS};
pub use error::AssemblyError;
pub use plan::{plan, AssemblyPlan};
pub use request::{AssemblyRequest, ModuleId};
