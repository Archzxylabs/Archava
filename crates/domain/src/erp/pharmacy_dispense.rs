//! Clinical Pharmacy & Prescription Dispensing Module (`archava_pharmacy_dispense`).
//!
//! Models outpatient and clinical pharmacy operations: prescription validation,
//! drug batch and expiration verification, allergy screen checks, and licensed pharmacist signoff.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::erp::{bounded_title, trimmed, valid_uuid};

pub const PHARMACY_PERMISSIONS: &[&str] = &[
    "pharmacy.read",
    "pharmacy.verify",
    "pharmacy.dispense",
    "pharmacy.refuse",
    "audit.read",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispenseStatus {
    PendingValidation,
    ScreenedAndApproved,
    Dispensed,
    Refused,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispenseLineItem {
    pub medication_id: String,
    pub medication_name: String,
    pub batch_number: String,
    pub expiry_epoch_sec: i64,
    pub quantity_prescribed: u32,
    pub quantity_dispensed: u32,
    pub dosage_instructions: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrescriptionDispenseOrder {
    pub order_id: String,
    pub prescription_id: String,
    pub patient_id: String,
    pub doctor_id: String,
    pub pharmacist_license_no: Option<String>,
    pub allergy_check_cleared: bool,
    pub contraindication_cleared: bool,
    pub items: Vec<DispenseLineItem>,
    pub status: DispenseStatus,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PharmacyError {
    #[error("validation error: {0}")]
    Validation(&'static str),
    #[error("cannot dispense expired drug batch: {batch} expired at {expiry}")]
    ExpiredBatch { batch: String, expiry: i64 },
    #[error("invalid transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: DispenseStatus,
        to: DispenseStatus,
    },
    #[error("safety screen not cleared: allergy={allergy}, contraindication={contra}")]
    SafetyScreenFailed { allergy: bool, contra: bool },
}

impl PrescriptionDispenseOrder {
    pub fn new(
        order_id: String,
        prescription_id: String,
        patient_id: String,
        doctor_id: String,
    ) -> Result<Self, PharmacyError> {
        if !valid_uuid(&order_id)
            || !valid_uuid(&prescription_id)
            || !valid_uuid(&patient_id)
            || !valid_uuid(&doctor_id)
        {
            return Err(PharmacyError::Validation("invalid uuid format"));
        }

        Ok(Self {
            order_id,
            prescription_id,
            patient_id,
            doctor_id,
            pharmacist_license_no: None,
            allergy_check_cleared: false,
            contraindication_cleared: false,
            items: Vec::new(),
            status: DispenseStatus::PendingValidation,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_item(
        &mut self,
        medication_id: String,
        medication_name: String,
        batch_number: String,
        expiry_epoch_sec: i64,
        quantity_prescribed: u32,
        dosage_instructions: String,
        current_epoch_sec: i64,
    ) -> Result<(), PharmacyError> {
        if self.status != DispenseStatus::PendingValidation {
            return Err(PharmacyError::Validation(
                "items can only be added to pending orders",
            ));
        }
        if !valid_uuid(&medication_id) {
            return Err(PharmacyError::Validation("invalid medication uuid"));
        }
        if bounded_title(&medication_name, 100).is_none() || trimmed(&batch_number).is_empty() {
            return Err(PharmacyError::Validation(
                "medication name and batch number are required",
            ));
        }
        if quantity_prescribed == 0 {
            return Err(PharmacyError::Validation(
                "prescribed quantity must be greater than zero",
            ));
        }
        if expiry_epoch_sec <= current_epoch_sec {
            return Err(PharmacyError::ExpiredBatch {
                batch: batch_number,
                expiry: expiry_epoch_sec,
            });
        }

        self.items.push(DispenseLineItem {
            medication_id,
            medication_name,
            batch_number,
            expiry_epoch_sec,
            quantity_prescribed,
            quantity_dispensed: 0,
            dosage_instructions,
        });
        Ok(())
    }

    pub fn approve_by_pharmacist(
        &mut self,
        license_no: String,
        allergy_cleared: bool,
        contraindication_cleared: bool,
    ) -> Result<(), PharmacyError> {
        if self.status != DispenseStatus::PendingValidation {
            return Err(PharmacyError::InvalidTransition {
                from: self.status,
                to: DispenseStatus::ScreenedAndApproved,
            });
        }
        if trimmed(&license_no).is_empty() {
            return Err(PharmacyError::Validation(
                "pharmacist license number is mandatory",
            ));
        }
        if !allergy_cleared || !contraindication_cleared {
            return Err(PharmacyError::SafetyScreenFailed {
                allergy: allergy_cleared,
                contra: contraindication_cleared,
            });
        }
        if self.items.is_empty() {
            return Err(PharmacyError::Validation(
                "cannot approve order with no line items",
            ));
        }

        self.pharmacist_license_no = Some(license_no);
        self.allergy_check_cleared = true;
        self.contraindication_cleared = true;
        self.status = DispenseStatus::ScreenedAndApproved;
        Ok(())
    }

    pub fn dispense(&mut self) -> Result<(), PharmacyError> {
        if self.status != DispenseStatus::ScreenedAndApproved {
            return Err(PharmacyError::InvalidTransition {
                from: self.status,
                to: DispenseStatus::Dispensed,
            });
        }

        for item in &mut self.items {
            item.quantity_dispensed = item.quantity_prescribed;
        }
        self.status = DispenseStatus::Dispensed;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pharmacy_dispense_full_workflow() {
        let mut order = PrescriptionDispenseOrder::new(
            "00000000-0000-0000-0000-000000000001".into(),
            "00000000-0000-0000-0000-000000000002".into(),
            "00000000-0000-0000-0000-000000000003".into(),
            "00000000-0000-0000-0000-000000000004".into(),
        )
        .expect("order created");

        order
            .add_item(
                "00000000-0000-0000-0000-000000000010".into(),
                "Amoxicillin 500mg".into(),
                "BATCH-AMX-2027".into(),
                1_800_000_000,
                30,
                "1 tablet 3x daily after meals".into(),
                1_700_000_000,
            )
            .expect("item added");

        order
            .approve_by_pharmacist("SIPA-31.71.01-2024".into(), true, true)
            .expect("screened and approved");

        assert_eq!(order.status, DispenseStatus::ScreenedAndApproved);
        order.dispense().expect("dispensed");
        assert_eq!(order.status, DispenseStatus::Dispensed);
        assert_eq!(order.items[0].quantity_dispensed, 30);
    }
}
