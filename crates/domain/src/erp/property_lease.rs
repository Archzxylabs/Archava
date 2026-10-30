//! Commercial & Residential Property Leasing domain — tenancy contracts, security deposits,
//! recurring service charge (IPL) billing, and handover inspection status.
//!
//! ## Invariants
//!
//! - Closed vocabularies: lease status, billing cycle, property unit kind.
//! - Chronological integrity: `end_date` must strictly follow `start_date`.
//! - Escrow security deposit: deposit must be held in full throughout active tenancy.
//! - Pure domain logic: zero I/O, zero sqlx, wasm32-clean.

use serde::{Deserialize, Serialize};

use super::purchasing::ValidationError;
use super::valid_uuid;

pub const LEASE_PERMISSIONS: &[&str] =
    &["lease.read", "lease.write", "lease.inspect", "lease.audit"];

// Lease Statuses
pub const LEASE_DRAFT: &str = "draft";
pub const LEASE_ACTIVE: &str = "active";
pub const LEASE_NOTICE_GIVEN: &str = "notice_given";
pub const LEASE_EXPIRED: &str = "expired";
pub const LEASE_TERMINATED: &str = "terminated";

// Billing Cycles
pub const CYCLE_MONTHLY: &str = "monthly";
pub const CYCLE_QUARTERLY: &str = "quarterly";
pub const CYCLE_SEMI_ANNUAL: &str = "semi_annual";
pub const CYCLE_ANNUAL: &str = "annual";

// Property Kinds
pub const PROPERTY_SHOPHOUSE: &str = "shophouse_ruko";
pub const PROPERTY_OFFICE: &str = "commercial_office";
pub const PROPERTY_APARTMENT: &str = "apartment";
pub const PROPERTY_WAREHOUSE: &str = "warehouse_gudang";
pub const PROPERTY_LAND: &str = "land";

// Bounds
pub const REF_MAX: usize = 80;
pub const UNIT_MAX: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateLeaseContractCommand {
    pub property_ref: String,
    pub unit_number: String,
    pub tenant_id: String,
    pub property_kind: String,
    pub start_timestamp: u64,
    pub end_timestamp: u64,
    pub rent_cents_per_cycle: u64,
    pub security_deposit_cents: u64,
    pub monthly_service_charge_cents: u64,
    pub billing_cycle: String,
}

impl CreateLeaseContractCommand {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if !valid_uuid(&self.tenant_id) {
            return Err(ValidationError("invalid tenant_id UUID"));
        }
        if self.property_ref.trim().is_empty() || self.property_ref.len() > REF_MAX {
            return Err(ValidationError("invalid property_ref length"));
        }
        if self.unit_number.trim().is_empty() || self.unit_number.len() > UNIT_MAX {
            return Err(ValidationError("invalid unit_number length"));
        }
        match self.property_kind.as_str() {
            PROPERTY_SHOPHOUSE | PROPERTY_OFFICE | PROPERTY_APARTMENT | PROPERTY_WAREHOUSE
            | PROPERTY_LAND => {}
            _ => return Err(ValidationError("unknown property kind")),
        }
        if self.end_timestamp <= self.start_timestamp {
            return Err(ValidationError(
                "end_timestamp must be strictly after start_timestamp",
            ));
        }
        if self.rent_cents_per_cycle == 0 {
            return Err(ValidationError("rent amount must be positive"));
        }
        if self.security_deposit_cents == 0 {
            return Err(ValidationError("security deposit cannot be zero"));
        }
        match self.billing_cycle.as_str() {
            CYCLE_MONTHLY | CYCLE_QUARTERLY | CYCLE_SEMI_ANNUAL | CYCLE_ANNUAL => {}
            _ => return Err(ValidationError("unknown billing cycle")),
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettleDepositReturn {
    pub gross_deposit_cents: u64,
    pub unpaid_utility_charge_cents: u64,
    pub damage_repair_cost_cents: u64,
    pub net_refund_cents: u64,
}

/// Calculate deposit return upon tenancy checkout and final inspection.
pub fn calculate_deposit_settlement(
    gross_deposit: u64,
    unpaid_utilities: u64,
    damage_costs: u64,
) -> SettleDepositReturn {
    let total_deductions = unpaid_utilities.saturating_add(damage_costs);
    let net_refund = gross_deposit.saturating_sub(total_deductions);
    SettleDepositReturn {
        gross_deposit_cents: gross_deposit,
        unpaid_utility_charge_cents: unpaid_utilities,
        damage_repair_cost_cents: damage_costs,
        net_refund_cents: net_refund,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_contract_validation_rules() {
        let cmd = CreateLeaseContractCommand {
            property_ref: "Ruko Sudirman Grand".to_string(),
            unit_number: "No. 12B".to_string(),
            tenant_id: "00000000-0000-0000-0000-000000000001".to_string(),
            property_kind: PROPERTY_SHOPHOUSE.to_string(),
            start_timestamp: 1735689600,           // Jan 1 2025
            end_timestamp: 1767225600,             // Jan 1 2026
            rent_cents_per_cycle: 12_000_000_000,  // Rp 120jt / tahun
            security_deposit_cents: 2_000_000_000, // Rp 20jt deposit
            monthly_service_charge_cents: 150_000_000,
            billing_cycle: CYCLE_ANNUAL.to_string(),
        };
        assert!(cmd.validate().is_ok());

        // Inverted dates
        let mut inverted = cmd.clone();
        inverted.end_timestamp = cmd.start_timestamp;
        assert!(inverted.validate().is_err());
    }

    #[test]
    fn deposit_settlement_calculation() {
        let deposit = 20_000_000;
        let utilities = 1_500_000;
        let wall_repairs = 2_000_000;

        let settlement = calculate_deposit_settlement(deposit, utilities, wall_repairs);
        assert_eq!(settlement.gross_deposit_cents, 20_000_000);
        assert_eq!(settlement.net_refund_cents, 16_500_000);
    }
}
