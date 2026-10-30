//! Manufacturing Quality Control & Inspection Checkpoints Module (`archava_quality_control`).
//!
//! Models manufacturing and incoming goods quality assurance: AQL sampling plans,
//! critical/major/minor defect classifications, inspection order execution,
//! and quarantine disposition chains.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::erp::{bounded_title, valid_uuid};

pub const QC_PERMISSIONS: &[&str] = &[
    "qc.read",
    "qc.plan",
    "qc.inspect",
    "qc.decide",
    "qc.quarantine",
    "audit.read",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionStage {
    IncomingGoods,
    InProcessRouting,
    FinalFinishedGoods,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QcVerdict {
    PendingInspection,
    Accepted,
    Rejected,
    Quarantined,
    ReworkAuthorized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefectCounts {
    pub critical: u32,
    pub major: u32,
    pub minor: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QcInspectionOrder {
    pub order_id: String,
    pub production_order_or_po_id: String,
    pub item_sku: String,
    pub lot_number: String,
    pub stage: InspectionStage,
    pub batch_size: u32,
    pub sample_size: u32,
    pub max_acceptable_defects: u32, // Ac limit
    pub defects_found: Option<DefectCounts>,
    pub verdict: QcVerdict,
    pub inspector_notes: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum QcError {
    #[error("validation error: {0}")]
    Validation(&'static str),
    #[error("sample size {sample} cannot exceed batch size {batch}")]
    SampleExceedsBatch { sample: u32, batch: u32 },
    #[error("order has already been concluded with verdict {0:?}")]
    AlreadyConcluded(QcVerdict),
}

impl QcInspectionOrder {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        order_id: String,
        production_order_or_po_id: String,
        item_sku: String,
        lot_number: String,
        stage: InspectionStage,
        batch_size: u32,
        sample_size: u32,
        max_acceptable_defects: u32,
    ) -> Result<Self, QcError> {
        if !valid_uuid(&order_id) || !valid_uuid(&production_order_or_po_id) {
            return Err(QcError::Validation("invalid uuid format"));
        }
        if bounded_title(&item_sku, 50).is_none() || bounded_title(&lot_number, 50).is_none() {
            return Err(QcError::Validation("sku and lot number must be 1-50 chars"));
        }
        if batch_size == 0 || sample_size == 0 {
            return Err(QcError::Validation(
                "batch and sample sizes must be greater than zero",
            ));
        }
        if sample_size > batch_size {
            return Err(QcError::SampleExceedsBatch {
                sample: sample_size,
                batch: batch_size,
            });
        }

        Ok(Self {
            order_id,
            production_order_or_po_id,
            item_sku,
            lot_number,
            stage,
            batch_size,
            sample_size,
            max_acceptable_defects,
            defects_found: None,
            verdict: QcVerdict::PendingInspection,
            inspector_notes: None,
        })
    }

    pub fn record_findings(
        &mut self,
        defects: DefectCounts,
        notes: String,
    ) -> Result<QcVerdict, QcError> {
        if self.verdict != QcVerdict::PendingInspection {
            return Err(QcError::AlreadyConcluded(self.verdict));
        }

        // Strict Invariants:
        // 1. Any critical defect results in immediate rejection/quarantine
        // 2. Total non-critical defects exceeding max_acceptable_defects results in Rejection
        let verdict = if defects.critical > 0
            || defects.major.saturating_add(defects.minor) > self.max_acceptable_defects
        {
            QcVerdict::Rejected
        } else {
            QcVerdict::Accepted
        };

        self.defects_found = Some(defects);
        self.verdict = verdict;
        self.inspector_notes = Some(notes);

        Ok(verdict)
    }

    pub fn send_to_quarantine(&mut self) -> Result<(), QcError> {
        if self.verdict != QcVerdict::Rejected {
            return Err(QcError::Validation(
                "only rejected orders can be quarantined",
            ));
        }
        self.verdict = QcVerdict::Quarantined;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qc_sample_order_pass() {
        let mut qc = QcInspectionOrder::new(
            "00000000-0000-0000-0000-000000000001".into(),
            "00000000-0000-0000-0000-000000000002".into(),
            "PCB-MAIN-001".into(),
            "LOT-2026-X1".into(),
            InspectionStage::FinalFinishedGoods,
            1000,
            80, // AQL sample
            2,  // Ac = 2
        )
        .expect("qc order valid");

        let verdict = qc
            .record_findings(
                DefectCounts {
                    critical: 0,
                    major: 1,
                    minor: 1,
                },
                "Inspection passed within Ac limit".into(),
            )
            .expect("recorded findings");

        assert_eq!(verdict, QcVerdict::Accepted);
    }

    #[test]
    fn qc_critical_defect_rejects_and_quarantines() {
        let mut qc = QcInspectionOrder::new(
            "00000000-0000-0000-0000-000000000001".into(),
            "00000000-0000-0000-0000-000000000002".into(),
            "SERUM-VIT-C".into(),
            "LOT-COSM-99".into(),
            InspectionStage::IncomingGoods,
            500,
            32,
            1,
        )
        .expect("qc valid");

        // 1 critical defect (foreign particulate)
        let verdict = qc
            .record_findings(
                DefectCounts {
                    critical: 1,
                    major: 0,
                    minor: 0,
                },
                "Critical foreign matter discovered".into(),
            )
            .expect("findings");

        assert_eq!(verdict, QcVerdict::Rejected);
        qc.send_to_quarantine().expect("quarantined");
        assert_eq!(qc.verdict, QcVerdict::Quarantined);
    }
}
