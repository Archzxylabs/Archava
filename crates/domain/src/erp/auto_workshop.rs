//! Automotive Repair & Mechanical Workshop Module (`archava_auto_workshop`).
//!
//! Models vehicle repair workshops: Surat Perintah Kerja (SPK), diagnostic complaint intake,
//! spare parts inventory deduction, mechanic labor tracking, and post-repair test drive signoff.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::erp::{bounded_title, trimmed, valid_uuid};

pub const WORKSHOP_PERMISSIONS: &[&str] = &[
    "workshop.read",
    "workshop.spk",
    "workshop.diagnose",
    "workshop.repair",
    "workshop.testdrive",
    "workshop.bill",
    "audit.read",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpkStatus {
    IntakeDraft,
    Diagnosed,
    CustomerApproved,
    InProgress,
    QualityInspection,
    TestDrivePassed,
    BilledCompleted,
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartConsumption {
    pub part_sku: String,
    pub part_name: String,
    pub quantity: u32,
    pub unit_price_minor: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaborLine {
    pub mechanic_id: String,
    pub service_operation: String,
    pub labor_hours_tenths: u32, // e.g. 25 = 2.5 hours
    pub hourly_rate_minor: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkOrderSpk {
    pub spk_id: String,
    pub spk_number: String, // e.g. "SPK-2026-0881"
    pub vehicle_plate: String,
    pub odometer_km: u32,
    pub customer_id: String,
    pub lead_mechanic_id: Option<String>,
    pub complaints: Vec<String>,
    pub parts: Vec<PartConsumption>,
    pub labor: Vec<LaborLine>,
    pub test_driver_id: Option<String>,
    pub status: SpkStatus,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkshopError {
    #[error("validation error: {0}")]
    Validation(&'static str),
    #[error("invalid SPK transition from {from:?} to {to:?}")]
    InvalidTransition { from: SpkStatus, to: SpkStatus },
}

impl WorkOrderSpk {
    pub fn new(
        spk_id: String,
        spk_number: String,
        vehicle_plate: String,
        odometer_km: u32,
        customer_id: String,
        initial_complaint: String,
    ) -> Result<Self, WorkshopError> {
        if !valid_uuid(&spk_id) || !valid_uuid(&customer_id) {
            return Err(WorkshopError::Validation("invalid uuid format"));
        }
        if bounded_title(&spk_number, 32).is_none() || bounded_title(&vehicle_plate, 20).is_none() {
            return Err(WorkshopError::Validation(
                "invalid spk number or vehicle plate length",
            ));
        }
        if odometer_km == 0 {
            return Err(WorkshopError::Validation(
                "odometer must be greater than zero",
            ));
        }
        if trimmed(&initial_complaint).is_empty() {
            return Err(WorkshopError::Validation("initial complaint is required"));
        }

        Ok(Self {
            spk_id,
            spk_number,
            vehicle_plate: vehicle_plate.to_uppercase(),
            odometer_km,
            customer_id,
            lead_mechanic_id: None,
            complaints: vec![initial_complaint],
            parts: Vec::new(),
            labor: Vec::new(),
            test_driver_id: None,
            status: SpkStatus::IntakeDraft,
        })
    }

    pub fn assign_mechanic_and_diagnose(
        &mut self,
        mechanic_id: String,
    ) -> Result<(), WorkshopError> {
        if self.status != SpkStatus::IntakeDraft {
            return Err(WorkshopError::InvalidTransition {
                from: self.status,
                to: SpkStatus::Diagnosed,
            });
        }
        if !valid_uuid(&mechanic_id) {
            return Err(WorkshopError::Validation("invalid mechanic uuid"));
        }

        self.lead_mechanic_id = Some(mechanic_id);
        self.status = SpkStatus::Diagnosed;
        Ok(())
    }

    pub fn approve_by_customer(&mut self) -> Result<(), WorkshopError> {
        if self.status != SpkStatus::Diagnosed {
            return Err(WorkshopError::InvalidTransition {
                from: self.status,
                to: SpkStatus::CustomerApproved,
            });
        }
        self.status = SpkStatus::CustomerApproved;
        Ok(())
    }

    pub fn start_repair(&mut self) -> Result<(), WorkshopError> {
        if self.status != SpkStatus::CustomerApproved {
            return Err(WorkshopError::InvalidTransition {
                from: self.status,
                to: SpkStatus::InProgress,
            });
        }
        self.status = SpkStatus::InProgress;
        Ok(())
    }

    pub fn add_part(&mut self, part: PartConsumption) -> Result<(), WorkshopError> {
        if self.status != SpkStatus::InProgress && self.status != SpkStatus::Diagnosed {
            return Err(WorkshopError::Validation(
                "parts can only be added during diagnosis or repair",
            ));
        }
        if part.quantity == 0 {
            return Err(WorkshopError::Validation("part quantity must be > 0"));
        }
        self.parts.push(part);
        Ok(())
    }

    pub fn add_labor(&mut self, labor: LaborLine) -> Result<(), WorkshopError> {
        if self.status != SpkStatus::InProgress && self.status != SpkStatus::Diagnosed {
            return Err(WorkshopError::Validation(
                "labor can only be added during diagnosis or repair",
            ));
        }
        if labor.labor_hours_tenths == 0 {
            return Err(WorkshopError::Validation("labor hours must be > 0"));
        }
        self.labor.push(labor);
        Ok(())
    }

    pub fn pass_quality_inspection(&mut self) -> Result<(), WorkshopError> {
        if self.status != SpkStatus::InProgress {
            return Err(WorkshopError::InvalidTransition {
                from: self.status,
                to: SpkStatus::QualityInspection,
            });
        }
        self.status = SpkStatus::QualityInspection;
        Ok(())
    }

    pub fn pass_test_drive(&mut self, tester_id: String) -> Result<(), WorkshopError> {
        if self.status != SpkStatus::QualityInspection {
            return Err(WorkshopError::InvalidTransition {
                from: self.status,
                to: SpkStatus::TestDrivePassed,
            });
        }
        if !valid_uuid(&tester_id) {
            return Err(WorkshopError::Validation("invalid tester uuid"));
        }

        self.test_driver_id = Some(tester_id);
        self.status = SpkStatus::TestDrivePassed;
        Ok(())
    }

    pub fn calculate_total_bill_minor(&self) -> u64 {
        let parts_total: u64 = self
            .parts
            .iter()
            .map(|p| (p.quantity as u64).saturating_mul(p.unit_price_minor))
            .sum();

        let labor_total: u64 = self
            .labor
            .iter()
            .map(|l| (l.labor_hours_tenths as u64 * l.hourly_rate_minor) / 10)
            .sum();

        parts_total.saturating_add(labor_total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workshop_spk_full_repair_lifecycle() {
        let mut spk = WorkOrderSpk::new(
            "00000000-0000-0000-0000-000000000001".into(),
            "SPK-2026-0881".into(),
            "B 1234 XYZ".into(),
            45_200,
            "00000000-0000-0000-0000-000000000002".into(),
            "Brake squeaking and periodic 40k service".into(),
        )
        .expect("spk created");

        assert_eq!(spk.status, SpkStatus::IntakeDraft);

        spk.assign_mechanic_and_diagnose("00000000-0000-0000-0000-000000000003".into())
            .expect("diagnosed");
        spk.approve_by_customer().expect("customer approved");
        spk.start_repair().expect("started repair");

        // Add brake pads & engine oil
        spk.add_part(PartConsumption {
            part_sku: "BRAKE-PAD-FRONT".into(),
            part_name: "Front Ceramic Brake Pad Set".into(),
            quantity: 1,
            unit_price_minor: 450_000,
        })
        .expect("part added");

        // Add 2.0 hours labor at 100k/hr
        spk.add_labor(LaborLine {
            mechanic_id: "00000000-0000-0000-0000-000000000003".into(),
            service_operation: "Brake pad replacement and brake bleed".into(),
            labor_hours_tenths: 20, // 2.0 hrs
            hourly_rate_minor: 100_000,
        })
        .expect("labor added");

        assert_eq!(spk.calculate_total_bill_minor(), 650_000); // 450k parts + 200k labor

        spk.pass_quality_inspection().expect("qc passed");
        spk.pass_test_drive("00000000-0000-0000-0000-000000000005".into())
            .expect("test drive passed");

        assert_eq!(spk.status, SpkStatus::TestDrivePassed);
    }
}
