//! Employee Expenses & Reimbursement Ledger Module (`archava_expenses`).
//!
//! Models corporate employee expenses: receipt image hashing (SHA-256), multi-tier
//! manager authorization ladders, currency conversions, and finance disbursement.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::erp::{bounded_title, valid_uuid};

pub const EXPENSE_PERMISSIONS: &[&str] = &[
    "expense.read",
    "expense.submit",
    "expense.approve",
    "expense.disburse",
    "expense.reject",
    "audit.read",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpenseCategory {
    TravelTransport,
    MealsEntertainment,
    OfficeSupplies,
    SoftwareTools,
    TrainingEducation,
    MedicalReimbursement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Draft,
    Submitted,
    ManagerApproved,
    FinanceDisbursed,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpenseClaim {
    pub claim_id: String,
    pub employee_id: String,
    pub category: ExpenseCategory,
    pub description: String,
    pub amount_minor: u64,
    pub currency: String,
    pub receipt_sha256: String,
    pub approved_by_manager_id: Option<String>,
    pub disbursed_by_finance_id: Option<String>,
    pub status: ClaimStatus,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ExpenseError {
    #[error("validation error: {0}")]
    Validation(&'static str),
    #[error("invalid transition from {from:?} to {to:?}")]
    InvalidTransition { from: ClaimStatus, to: ClaimStatus },
}

impl ExpenseClaim {
    pub fn new(
        claim_id: String,
        employee_id: String,
        category: ExpenseCategory,
        description: String,
        amount_minor: u64,
        currency: String,
        receipt_sha256: String,
    ) -> Result<Self, ExpenseError> {
        if !valid_uuid(&claim_id) || !valid_uuid(&employee_id) {
            return Err(ExpenseError::Validation("invalid uuid format"));
        }
        if bounded_title(&description, 200).is_none() {
            return Err(ExpenseError::Validation(
                "description must be between 1 and 200 characters",
            ));
        }
        if amount_minor == 0 {
            return Err(ExpenseError::Validation("amount must be greater than zero"));
        }
        if currency.len() != 3 {
            return Err(ExpenseError::Validation(
                "currency must be 3-letter ISO code",
            ));
        }
        if receipt_sha256.len() != 64 {
            return Err(ExpenseError::Validation(
                "receipt sha256 must be a 64-char hex string",
            ));
        }

        Ok(Self {
            claim_id,
            employee_id,
            category,
            description,
            amount_minor,
            currency: currency.to_uppercase(),
            receipt_sha256,
            approved_by_manager_id: None,
            disbursed_by_finance_id: None,
            status: ClaimStatus::Draft,
        })
    }

    pub fn submit(&mut self) -> Result<(), ExpenseError> {
        if self.status != ClaimStatus::Draft {
            return Err(ExpenseError::InvalidTransition {
                from: self.status,
                to: ClaimStatus::Submitted,
            });
        }
        self.status = ClaimStatus::Submitted;
        Ok(())
    }

    pub fn approve_by_manager(&mut self, manager_id: String) -> Result<(), ExpenseError> {
        if self.status != ClaimStatus::Submitted {
            return Err(ExpenseError::InvalidTransition {
                from: self.status,
                to: ClaimStatus::ManagerApproved,
            });
        }
        if !valid_uuid(&manager_id) {
            return Err(ExpenseError::Validation("invalid manager uuid"));
        }
        if manager_id == self.employee_id {
            return Err(ExpenseError::Validation(
                "employee cannot approve their own expense claim",
            ));
        }

        self.approved_by_manager_id = Some(manager_id);
        self.status = ClaimStatus::ManagerApproved;
        Ok(())
    }

    pub fn disburse_by_finance(&mut self, finance_id: String) -> Result<(), ExpenseError> {
        if self.status != ClaimStatus::ManagerApproved {
            return Err(ExpenseError::InvalidTransition {
                from: self.status,
                to: ClaimStatus::FinanceDisbursed,
            });
        }
        if !valid_uuid(&finance_id) {
            return Err(ExpenseError::Validation("invalid finance uuid"));
        }

        self.disbursed_by_finance_id = Some(finance_id);
        self.status = ClaimStatus::FinanceDisbursed;
        Ok(())
    }

    pub fn reject(&mut self) -> Result<(), ExpenseError> {
        match self.status {
            ClaimStatus::Submitted | ClaimStatus::ManagerApproved => {
                self.status = ClaimStatus::Rejected;
                Ok(())
            }
            _ => Err(ExpenseError::InvalidTransition {
                from: self.status,
                to: ClaimStatus::Rejected,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expense_claim_workflow_and_segregation_of_duties() {
        let mut claim = ExpenseClaim::new(
            "00000000-0000-0000-0000-000000000001".into(),
            "00000000-0000-0000-0000-000000000002".into(),
            ExpenseCategory::TravelTransport,
            "Client site visit taxi fare".into(),
            150_000,
            "IDR".into(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
        )
        .expect("claim created");

        claim.submit().expect("submitted");

        // Self-approval refused
        let err = claim.approve_by_manager("00000000-0000-0000-0000-000000000002".into());
        assert!(err.is_err());

        // Approved by separate manager
        claim
            .approve_by_manager("00000000-0000-0000-0000-000000000099".into())
            .expect("approved");
        assert_eq!(claim.status, ClaimStatus::ManagerApproved);

        // Disbursed by finance
        claim
            .disburse_by_finance("00000000-0000-0000-0000-000000000077".into())
            .expect("disbursed");
        assert_eq!(claim.status, ClaimStatus::FinanceDisbursed);
    }
}
