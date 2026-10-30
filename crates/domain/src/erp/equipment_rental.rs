//! Equipment & Machinery Rental Module (`archava_equipment_rental`).
//!
//! Models serialized equipment leasing: security deposit management, condition check-out
//! and check-in inspection reports, hourly/daily duration computation, and overdue penalties.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::erp::valid_uuid;

pub const EQUIPMENT_RENTAL_PERMISSIONS: &[&str] = &[
    "rental.read",
    "rental.dispatch",
    "rental.inspect",
    "rental.settle",
    "rental.claim",
    "audit.read",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RentalState {
    Reserved,
    Dispatched,
    InUse,
    ReturnedInspected,
    DamageClaimed,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConditionInspection {
    pub checklist_notes: String,
    pub cosmetic_rating: u8, // 1 (poor) to 5 (pristine)
    pub functional_tested: bool,
    pub inspection_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RentalContract {
    pub contract_id: String,
    pub renter_id: String,
    pub equipment_id: String,
    pub start_epoch_sec: i64,
    pub agreed_end_epoch_sec: i64,
    pub actual_returned_epoch_sec: Option<i64>,
    pub daily_rate_minor: u64,
    pub security_deposit_minor: u64,
    pub dispatch_inspection: ConditionInspection,
    pub return_inspection: Option<ConditionInspection>,
    pub state: RentalState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RentalSettlement {
    pub base_cost_minor: u64,
    pub late_penalty_minor: u64,
    pub damage_deduction_minor: u64,
    pub net_deposit_refund_minor: u64,
    pub outstanding_liability_minor: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RentalError {
    #[error("validation error: {0}")]
    Validation(&'static str),
    #[error("invalid state transition from {from:?} to {to:?}")]
    InvalidTransition { from: RentalState, to: RentalState },
}

impl RentalContract {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        contract_id: String,
        renter_id: String,
        equipment_id: String,
        start_epoch_sec: i64,
        agreed_end_epoch_sec: i64,
        daily_rate_minor: u64,
        security_deposit_minor: u64,
        dispatch_inspection: ConditionInspection,
    ) -> Result<Self, RentalError> {
        if !valid_uuid(&contract_id) || !valid_uuid(&renter_id) || !valid_uuid(&equipment_id) {
            return Err(RentalError::Validation("invalid uuid format"));
        }
        if agreed_end_epoch_sec <= start_epoch_sec {
            return Err(RentalError::Validation(
                "end timestamp must be strictly after start timestamp",
            ));
        }
        if daily_rate_minor == 0 {
            return Err(RentalError::Validation(
                "daily rate must be greater than zero",
            ));
        }
        if dispatch_inspection.cosmetic_rating == 0 || dispatch_inspection.cosmetic_rating > 5 {
            return Err(RentalError::Validation(
                "cosmetic rating must be between 1 and 5",
            ));
        }
        if !dispatch_inspection.functional_tested {
            return Err(RentalError::Validation(
                "equipment must pass functional testing before dispatch",
            ));
        }

        Ok(Self {
            contract_id,
            renter_id,
            equipment_id,
            start_epoch_sec,
            agreed_end_epoch_sec,
            actual_returned_epoch_sec: None,
            daily_rate_minor,
            security_deposit_minor,
            dispatch_inspection,
            return_inspection: None,
            state: RentalState::Reserved,
        })
    }

    pub fn dispatch(&mut self) -> Result<(), RentalError> {
        if self.state != RentalState::Reserved {
            return Err(RentalError::InvalidTransition {
                from: self.state,
                to: RentalState::Dispatched,
            });
        }
        self.state = RentalState::Dispatched;
        Ok(())
    }

    pub fn record_return(
        &mut self,
        returned_epoch_sec: i64,
        inspection: ConditionInspection,
    ) -> Result<(), RentalError> {
        if self.state != RentalState::Dispatched && self.state != RentalState::InUse {
            return Err(RentalError::InvalidTransition {
                from: self.state,
                to: RentalState::ReturnedInspected,
            });
        }
        if returned_epoch_sec < self.start_epoch_sec {
            return Err(RentalError::Validation(
                "return timestamp cannot precede rental start",
            ));
        }
        if inspection.cosmetic_rating == 0 || inspection.cosmetic_rating > 5 {
            return Err(RentalError::Validation(
                "return cosmetic rating must be between 1 and 5",
            ));
        }

        self.actual_returned_epoch_sec = Some(returned_epoch_sec);
        self.return_inspection = Some(inspection);
        self.state = RentalState::ReturnedInspected;
        Ok(())
    }

    pub fn calculate_settlement(
        &self,
        damage_deduction_minor: u64,
    ) -> Result<RentalSettlement, RentalError> {
        if self.state != RentalState::ReturnedInspected && self.state != RentalState::DamageClaimed
        {
            return Err(RentalError::Validation(
                "cannot settle contract before return inspection",
            ));
        }

        let returned_at = self
            .actual_returned_epoch_sec
            .ok_or(RentalError::Validation("missing actual returned timestamp"))?;

        let duration_seconds = (returned_at - self.start_epoch_sec).max(0);
        let days = ((duration_seconds + 86399) / 86400).max(1) as u64;
        let base_cost = days.saturating_mul(self.daily_rate_minor);

        let late_penalty = if returned_at > self.agreed_end_epoch_sec {
            let overdue_sec = returned_at - self.agreed_end_epoch_sec;
            let overdue_days = ((overdue_sec + 86399) / 86400).max(1) as u64;
            // 1.5x daily rate penalty for late returns
            (overdue_days * self.daily_rate_minor * 3) / 2
        } else {
            0
        };

        let total_deductions = damage_deduction_minor.saturating_add(late_penalty);

        let (net_deposit_refund, outstanding_liability) =
            if self.security_deposit_minor >= total_deductions {
                (self.security_deposit_minor - total_deductions, 0)
            } else {
                (0, total_deductions - self.security_deposit_minor)
            };

        Ok(RentalSettlement {
            base_cost_minor: base_cost,
            late_penalty_minor: late_penalty,
            damage_deduction_minor,
            net_deposit_refund_minor: net_deposit_refund,
            outstanding_liability_minor: outstanding_liability,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rental_lifecycle_and_settlement() {
        let dispatch_insp = ConditionInspection {
            checklist_notes: "Clean and tested".into(),
            cosmetic_rating: 5,
            functional_tested: true,
            inspection_sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                .into(),
        };

        let mut contract = RentalContract::new(
            "00000000-0000-0000-0000-000000000001".into(),
            "00000000-0000-0000-0000-000000000002".into(),
            "00000000-0000-0000-0000-000000000003".into(),
            1_000_000,
            1_000_000 + (3 * 86400), // 3 days
            500_000,                 // 5,000 IDR / day
            2_000_000,               // 20,000 IDR deposit
            dispatch_insp,
        )
        .expect("contract valid");

        contract.dispatch().expect("dispatched");

        // Returned 1 day late with minor damage
        let return_insp = ConditionInspection {
            checklist_notes: "Minor scuff on tripod leg".into(),
            cosmetic_rating: 4,
            functional_tested: true,
            inspection_sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                .into(),
        };

        let returned_at = 1_000_000 + (4 * 86400); // 4 days total
        contract
            .record_return(returned_at, return_insp)
            .expect("recorded return");

        let settlement = contract.calculate_settlement(300_000).expect("settled");

        // 1 day late penalty: 1 * 500,000 * 1.5 = 750,000
        assert_eq!(settlement.late_penalty_minor, 750_000);
        assert_eq!(settlement.damage_deduction_minor, 300_000);
        // Total deductions = 1,050,000; deposit = 2,000,000 => refund = 950,000
        assert_eq!(settlement.net_deposit_refund_minor, 950_000);
        assert_eq!(settlement.outstanding_liability_minor, 0);
    }
}
