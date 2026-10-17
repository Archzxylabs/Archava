//! The release catalogue, as this crate reads it.
//!
//! A subset of `releaseCatalogSchema` from
//! `packages/config/src/release-contracts.ts`. Only the fields `planAssembly`
//! itself touches are modelled, and that is deliberate: the schema validates
//! more than these — duplicate detection, the registry-name shapes, the
//! migration-ownership direction D08 names as the reverse one — and none of
//! those change what a deployment plan decides.
//!
//! The fields that *are* here are the ones whose absence would change a
//! decision, and each carries the constraint it must satisfy in a doc comment,
//! so a caller assembling a catalogue by hand can see what the schema would
//! have enforced. Anyone needing the full validation — including D08's reverse
//! direction — needs the configuration layer, not this crate.

use serde::{Deserialize, Serialize};

/// A semver-shaped version string.
///
/// The schema's shape is `^\d+\.\d+\.\d+$`. Rust cannot express that in the type
/// system, so the constraint lives here as documentation and in the caller's
/// own boundary; a `ReleaseCatalog` is expected to arrive already validated.
pub type Version = String;

/// A `name`/`version` pair. Used for `installation` entries and `externals`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedVersion {
    /// The schema's shape is `^@archava/[a-z][a-z0-9-]*$` for packages and
    /// `^(?:@[a-z0-9-]+\/)?[a-z0-9-]+$` for externals.
    pub name: String,
    /// `^\d+\.\d+\.\d+$`.
    pub version: Version,
}

/// A catalogue package: the artifact, plus the migrations it owns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPackage {
    /// `^@archava/[a-z][a-z0-9-]*$`, unique across the catalogue.
    pub name: String,
    /// `^\d+\.\d+\.\d+$`.
    pub version: Version,
    /// Migrations this package owns. The schema requires unique `file` values,
    /// and D08's reverse direction additionally requires that every package
    /// owning a migration is also named by some `databaseLedgers` entry.
    #[serde(default)]
    pub migrations: Vec<Migration>,
}

/// A single migration: its release filename and its recorded checksum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Migration {
    /// `^\d{3}_[a-z0-9_]+\.sql$`.
    pub file: String,
    /// `^[a-f0-9]{64}$` — a full sha256, not a prefix.
    pub checksum: String,
}

/// A database ledger: the schema, its owning package, and its migrations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseLedger {
    /// `^archava_[a-z_]+$`, unique across the catalogue.
    pub schema: String,
    /// `^@archava/[a-z][a-z0-9-]*$` — must name a package in this catalogue.
    pub package: String,
    /// `^\d{3}_[a-z0-9_]+$`, at least one, each owned by `package`.
    pub migrations: Vec<String>,
}

/// The host this catalogue's artifacts are hosted by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogHost {
    /// The schema pins this to the literal `@archava/crm-host`.
    pub name: String,
    /// `^\d+\.\d+\.\d+$`.
    pub version: Version,
}

/// The contract versions this catalogue implements.
///
/// `assembly` is `Option` because the schema allows it absent — and an absent
/// value is the state in which **every** gated request is refused, which is
/// what the version gate in [`crate::plan`] implements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogContracts {
    /// The schema pins this to the literal `1.0.0`.
    #[serde(rename = "clientConfig")]
    pub client_config: String,
    /// The schema pins this to the literal `2.0.0`.
    #[serde(rename = "deploymentPlan")]
    pub deployment_plan: String,
    /// The schema pins this to the literal `1`. **A number, not a string** —
    /// the one field in the contracts object whose type differs from its
    /// neighbours. A port that made it a string would parse the real catalogue
    /// and then reject it.
    #[serde(rename = "hostSettings")]
    pub host_settings: u32,
    /// `1.0.0` through `17.0.0`, or absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly: Option<String>,
    /// `1.0.0`, or absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence: Option<String>,
    /// `1.0.0`, or absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "kitPlan")]
    pub kit_plan: Option<String>,
    /// `1.0.0`, or absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "kitCatalogue")]
    pub kit_catalogue: Option<String>,
}

/// The trusted release catalogue a deployment plan is resolved against.
///
/// "Host-owned metadata, never a permission, price or tenant-authentication
/// token" — the schema's own comment, and one reason this crate is a leaf. A
/// catalogue decides what can be installed; it does not decide who may act, and
/// that separation is why nothing here needs a database, an HTTP client, or a
/// signature check.
///
/// This is a **read** shape, not a validated one. It carries the fields
/// `planAssembly` reads and none of the schema's cross-field rules, so a
/// `ReleaseCatalog` built by hand or parsed from an untrusted source is the
/// caller's responsibility to validate. See D08 for the check this crate did
/// not inherit and the reason it is named rather than silently dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseCatalog {
    /// The schema pins this to the literal `1.0.0`.
    #[serde(rename = "schemaVersion")]
    pub schema_version: String,
    /// The schema pins this to the literal `archava-local-library`.
    pub id: String,
    /// `^\d+\.\d+\.\d+$`. This is the **profile**, and `request.profile` must
    /// equal it or nothing else is evaluated.
    pub version: Version,
    pub host: CatalogHost,
    pub contracts: CatalogContracts,
    /// 1 to 64 entries, names unique.
    pub packages: Vec<CatalogPackage>,
    /// 1 to 128 entries, names unique. Optional, because the schema allows it
    /// absent — and `plan` never reads it, so an absent list is a difference
    /// this port cannot observe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub externals: Option<Vec<NamedVersion>>,
    /// 1 to 32 entries, schemas unique. Read for shape only; a plan's decisions
    /// never depend on it, because the migration-ownership rule is validation's
    /// job, not the planner's.
    #[serde(rename = "databaseLedgers")]
    #[serde(default)]
    pub database_ledgers: Vec<DatabaseLedger>,
    /// At least one, names unique, each must exist in `packages`. This is the
    /// set the schema refuses to let drift — a catalogue listing an adapter it
    /// does not ship.
    #[serde(rename = "requiredAdapters")]
    pub required_adapters: Vec<String>,
    /// 1 to 120-character operator check descriptions, at least one.
    #[serde(rename = "operatorChecks")]
    pub operator_checks: Vec<String>,
}
