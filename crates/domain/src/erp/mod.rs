//! The ERP namespace — Archxy 2's domain contracts for the bounded ERP
//! cohorts (R04–R08).
//!
//! ## Placement (the R02 wasm boundary, recorded on this crate's header)
//!
//! Everything under this module is pure business grammar: commands, DTOs,
//! validation and the permission rules the gate evaluates. No HTTP, no sqlx,
//! no socket reach. The transaction-taking halves live in
//! `archava-adapters/src/erp/` (SQL adapters) and `archava-host/src/erp/`
//! (typed routes), which is the split `crates/domain/Cargo.toml` records as
//! the boundary: a wasm32 build of this crate must stay valid, because the
//! client-side policy preview evaluates exactly these rules.
//!
//! ## Source of truth
//!
//! Each cohort module is ported from the TypeScript package that owns the
//! frozen contract (module header cites it). Known corrections ride along in
//! `KNOWN_DIFFERENCES.md`; a divergence is recorded there, never silently
//! absorbed.

// Core CRM Customer Relationship Management domain contracts (ADR-018).
pub mod accounting;
pub mod crm;

pub mod foodservice;
// R04 cohorts (PACKET-003).
pub mod companies;
pub mod organizations;
pub mod parties;
pub mod projects;
pub mod proposals;
// R05 cohorts (PACKET-004..006).
pub mod accommodation;
pub mod assets;
pub mod commerce;
pub mod purchasing;
pub mod purchasing_documents;
pub mod scheduling;
pub mod stock;
// Showing and entering trade amounts by currency (MULTI_CURRENCY.md rule 6).
pub mod trade_money;

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

// Domain rules only: no adapter, schema, route or screen mounts these.
pub mod auto_workshop;
pub mod clinical_records;
pub mod consignment;
pub mod equipment_rental;
pub mod expenses;
pub mod fleet_management;
pub mod lms_digital;
pub mod marketplace;
pub mod mrp_discrete;
pub mod patient_queue;
pub mod payroll;
pub mod pharmacy_dispense;
pub mod pos_terminal;
pub mod property_lease;
pub mod quality_control;
pub mod salon_spa;
pub mod studio_canvas;
pub mod venue_booking;

// Unmounted accounting, costing and tax helpers; the mounted ledger is
// `accounting`.
pub mod general_ledger;
pub mod inventory_costing;
pub mod tax_indonesia;

// The portable invariant tests live with the rules they pin (no database
// needed: these run on every `cargo test`, wasm-clean).
#[cfg(test)]
mod tests;

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The refusal vocabulary every ERP domain shares.
///
/// The baseline's per-domain error classes are the same four strings
/// (`PurchaseError`, `CommerceError`, …): these are API contracts, so the
/// wire spelling is kept exactly. `Unavailable` surfaces a runtime gate the
/// caller cannot fix by retrying with different input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ErpError {
    #[error("forbidden")]
    Forbidden,
    #[error("not_found")]
    NotFound,
    #[error("conflict")]
    Conflict,
    #[error("unavailable")]
    Unavailable,
}

/// A tenant-wide or personally-assigned grant — the schema's
/// `scope CHECK(scope IN ('all','assigned'))` on every ledger's memberships.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErpScope {
    All,
    Assigned,
}

impl ErpScope {
    /// Parses the column value, failing closed on anything outside the
    /// vocabulary. An unreadable grant is a refusal, never a default upward.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "assigned" => Some(Self::Assigned),
            _ => None,
        }
    }

    /// Whether a resource owned by someone may be seen under this scope.
    /// `all` sees every row; `assigned` (the intersection-narrowed case)
    /// only the owner's — the narrowing direction of the grant rule.
    pub fn covers_all(self) -> bool {
        matches!(self, Self::All)
    }
}

/// The membership kind every ERP schema CHECK names (`('human','service')`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErpMembershipKind {
    Human,
    Service,
}

impl ErpMembershipKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "human" => Some(Self::Human),
            "service" => Some(Self::Service),
            _ => None,
        }
    }

    /// The wire spelling stored in the column.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Service => "service",
        }
    }
}

/// One ledger's resolved grant: the permissions the row holds and the scope
/// it carries. Built only from parsed, vocabulary-checked rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerGrant {
    pub kind: ErpMembershipKind,
    pub permissions: Vec<String>,
    pub scope: ErpScope,
}

impl LedgerGrant {
    /// Whether the grant holds every named permission.
    pub fn holds(&self, permissions: &[&str]) -> bool {
        permissions
            .iter()
            .all(|needed| self.permissions.iter().any(|held| held == needed))
    }

    /// A service principal's authority is the INTERSECTION with the human it
    /// represents — permissions both hold, kind stays the actor's own, and
    /// the scope narrows to `assigned` if either side carries it. The same
    /// rule the baseline derives per domain (`scope === 'assigned'` when
    /// either side is), and the reason a service principal can never widen.
    pub fn intersect_on_behalf_of(&self, human: &LedgerGrant) -> Self {
        let mut permissions = Vec::new();
        for needed in &self.permissions {
            if human.permissions.iter().any(|other| other == needed)
                && !permissions.contains(needed)
            {
                permissions.push(needed.clone());
            }
        }
        // Order follows the ACTOR's list (the TS filter keeps `b.filter(...)`
        // order); a different order here would be a visible divergence in
        // every access payload that carries permissions.
        Self {
            kind: self.kind,
            permissions,
            scope: match (self.scope, human.scope) {
                (ErpScope::Assigned, _) | (_, ErpScope::Assigned) => ErpScope::Assigned,
                (ErpScope::All, ErpScope::All) => ErpScope::All,
            },
        }
        // Deliberately not `human.kind`: the ACTOR keeps their own kind; the
        // check that the named human is real lives where the row loads.
    }
}

/// The lowercase kebab-case slug the per-domain id schemas enforce
/// (`purchaseIdSchema` etc.: `^[a-z0-9]+(?:-[a-z0-9]+)*$`, ≤100 chars).
/// Hand-written like the tenant rule — the regex cost does not earn its crate
/// weight, and `archava_core::tenant::TenantId` already carries the shape.
pub fn valid_principal_id(value: &str) -> bool {
    if value.is_empty() || value.len() > 100 {
        return false;
    }
    let mut after_dash = true;
    for byte in value.bytes() {
        if byte == b'-' {
            // Consecutive dashes (`--`) are not part of the rule: a segment
            // must be non-empty. (Checked FIRST — a dash is a legal segment
            // separator even though it is not itself a segment character.)
            if after_dash {
                return false;
            }
            after_dash = true;
        } else if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
            after_dash = false;
        } else {
            return false;
        }
    }
    // A trailing dash would end a segment empty.
    !after_dash
}

/// The canonical JSON element id schemas accept (8-4-4-4-12 hex ids).
///
/// Mirrors `z.uuid()` acceptably for the ids the ERP schemas carry: lowercase
/// or uppercase hex accepted by column type, and the checker is what makes a
/// non-uuid string unable to reach SQL as a bind.
pub fn valid_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    let ok_hex = |b: u8| b.is_ascii_hexdigit();
    let dashes = [8usize, 13, 18, 23];
    for (index, byte) in bytes.iter().enumerate() {
        if dashes.contains(&index) {
            if *byte != b'-' {
                return false;
            }
        } else if !ok_hex(*byte) {
            return false;
        }
    }
    true
}

/// The supplier/menu code regex every frozen schema shares:
/// `^[A-Z0-9][A-Z0-9_-]{0,39}$`.
pub fn valid_code(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 40 {
        return false;
    }
    if !(bytes[0].is_ascii_uppercase() || bytes[0].is_ascii_digit()) {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_' || *b == b'-')
}

/// The `sku-shaped` menu code field names vary; the supplier code and the
/// menu SKU share the pattern above. Documented so a caller does not assume
/// lowercase SKUs are accepted anywhere in the frozen ERP contracts.
pub const SKU_PATTERN_DOC: &str = "^[A-Z0-9][A-Z0-9_-]{0,39}$";

/// Canonical JSON otherwise: objects key-sorted, arrays in order, compact
/// separators. Port of the per-repository `canonical` helper the baseline uses
/// for idempotency fingerprints — the serialized form must not depend on the
/// object key order a caller happened to use.
pub fn canonical_json(value: &serde_json::Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::Object(map) => {
            out.push('{');
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).expect("json string round-trip"));
                out.push(':');
                write_canonical(&map[key.as_str()], out);
            }
            out.push('}');
        }
        serde_json::Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        serde_json::Value::Number(number) => {
            // Integers rule: the TS side JSON.stringify's Number values and
            // every ERP quantity/cost/price/revision is an integer there.
            if let Some(as_int) = number.as_i64() {
                out.push_str(&as_int.to_string());
            } else if let Some(as_u64) = number.as_u64() {
                out.push_str(&as_u64.to_string());
            } else {
                out.push_str(&number.to_string());
            }
        }
        other => {
            // String / bool / null print like JSON.stringify's own.
            out.push_str(
                &serde_json::to_string(other).expect("json value round-trips to its own JSON"),
            );
        }
    }
}

/// Trims per the HTML-ish trimming the schemas use: the TS side's `.trim()`
/// removes the whitespace class the `UnicodeWhiteSpaces` trimmed — ASCII
/// whitespace and the usual unicode spaces here. A value that ends up empty
/// after trimming cannot pass a `.min(1)` schema.
pub fn trimmed(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_whitespace())
}

/// A string field with the schema's `.trim().min(1).max(max)` rule baked in.
pub fn bounded_title(value: &str, max: usize) -> Option<String> {
    let trimmed = trimmed(value);
    if trimmed.is_empty() || trimmed.len() > max {
        return None;
    }
    Some(trimmed.to_string())
}

/// An owned, unchanged first record of an eviction-safe id for error paths.
///
/// Wrapper is internal-only convenience so callers cannot forget the
/// vocabulary class they chose; nothing renders raw values to a browser from
/// here.
#[derive(Debug)]
pub struct ErpValue<'a>(pub &'a str);

impl fmt::Display for ErpValue<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Errors never gown raw caller bytes into operator logs; keep the
        // length and a non-secret-shaped prefix.
        if self.0.len() <= 8 {
            write!(formatter, "{:?}", self.0)
        } else {
            write!(formatter, "{:?}…", &self.0[..8])
        }
    }
}

pub mod inventory_valuation;
pub mod mail;
pub mod payments;
