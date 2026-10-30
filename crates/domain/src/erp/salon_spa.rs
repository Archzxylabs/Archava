//! Salon, Spa & Beauty Wellness Module (`archava_salon_spa`).
//!
//! Models wellness salon operations: therapist & stylist scheduling, multi-treatment
//! service chains, consumable product (lotions, serums, oils) batch deductions,
//! and therapist commission ledger.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::erp::{bounded_title, valid_uuid};

pub const SALON_SPA_PERMISSIONS: &[&str] = &[
    "salon.read",
    "salon.book",
    "salon.start",
    "salon.complete",
    "salon.commission",
    "audit.read",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreatmentStatus {
    Booked,
    InSession,
    Completed,
    Paid,
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumableDeduction {
    pub product_batch_id: String,
    pub quantity_milliliters: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreatmentSession {
    pub session_id: String,
    pub customer_id: String,
    pub therapist_id: String,
    pub station_or_room_id: String,
    pub service_name: String,
    pub scheduled_start_sec: i64,
    pub scheduled_end_sec: i64,
    pub price_minor: u64,
    pub commission_bps: u32, // e.g. 1500 = 15%
    pub consumables: Vec<ConsumableDeduction>,
    pub status: TreatmentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommissionPayout {
    pub therapist_id: String,
    pub session_id: String,
    pub gross_service_minor: u64,
    pub earned_commission_minor: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SalonSpaError {
    #[error("validation error: {0}")]
    Validation(&'static str),
    #[error("invalid status transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: TreatmentStatus,
        to: TreatmentStatus,
    },
    #[error("commission exceeds 10000 bps")]
    ExcessiveCommission,
}

impl TreatmentSession {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_id: String,
        customer_id: String,
        therapist_id: String,
        station_or_room_id: String,
        service_name: String,
        scheduled_start_sec: i64,
        scheduled_end_sec: i64,
        price_minor: u64,
        commission_bps: u32,
    ) -> Result<Self, SalonSpaError> {
        if !valid_uuid(&session_id)
            || !valid_uuid(&customer_id)
            || !valid_uuid(&therapist_id)
            || !valid_uuid(&station_or_room_id)
        {
            return Err(SalonSpaError::Validation("invalid uuid format"));
        }
        if bounded_title(&service_name, 100).is_none() {
            return Err(SalonSpaError::Validation(
                "service name must be between 1 and 100 characters",
            ));
        }
        if scheduled_end_sec <= scheduled_start_sec {
            return Err(SalonSpaError::Validation(
                "end time must be strictly after start time",
            ));
        }
        if price_minor == 0 {
            return Err(SalonSpaError::Validation(
                "service price must be greater than zero",
            ));
        }
        if commission_bps > 10_000 {
            return Err(SalonSpaError::ExcessiveCommission);
        }

        Ok(Self {
            session_id,
            customer_id,
            therapist_id,
            station_or_room_id,
            service_name,
            scheduled_start_sec,
            scheduled_end_sec,
            price_minor,
            commission_bps,
            consumables: Vec::new(),
            status: TreatmentStatus::Booked,
        })
    }

    pub fn start_session(&mut self) -> Result<(), SalonSpaError> {
        if self.status != TreatmentStatus::Booked {
            return Err(SalonSpaError::InvalidTransition {
                from: self.status,
                to: TreatmentStatus::InSession,
            });
        }
        self.status = TreatmentStatus::InSession;
        Ok(())
    }

    pub fn record_consumable(
        &mut self,
        product_batch_id: String,
        quantity_milliliters: u32,
    ) -> Result<(), SalonSpaError> {
        if self.status != TreatmentStatus::InSession {
            return Err(SalonSpaError::Validation(
                "consumables can only be recorded while session is in progress",
            ));
        }
        if !valid_uuid(&product_batch_id) {
            return Err(SalonSpaError::Validation("invalid product batch uuid"));
        }
        if quantity_milliliters == 0 {
            return Err(SalonSpaError::Validation(
                "quantity must be greater than zero",
            ));
        }

        self.consumables.push(ConsumableDeduction {
            product_batch_id,
            quantity_milliliters,
        });
        Ok(())
    }

    pub fn complete_session(&mut self) -> Result<(), SalonSpaError> {
        if self.status != TreatmentStatus::InSession {
            return Err(SalonSpaError::InvalidTransition {
                from: self.status,
                to: TreatmentStatus::Completed,
            });
        }
        self.status = TreatmentStatus::Completed;
        Ok(())
    }

    pub fn calculate_commission(&self) -> Result<CommissionPayout, SalonSpaError> {
        if self.status != TreatmentStatus::Completed && self.status != TreatmentStatus::Paid {
            return Err(SalonSpaError::Validation(
                "commission can only be calculated on completed or paid sessions",
            ));
        }

        let earned = (self.price_minor as u128 * self.commission_bps as u128 / 10_000) as u64;

        Ok(CommissionPayout {
            therapist_id: self.therapist_id.clone(),
            session_id: self.session_id.clone(),
            gross_service_minor: self.price_minor,
            earned_commission_minor: earned,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn salon_spa_session_workflow_and_commission() {
        let mut session = TreatmentSession::new(
            "00000000-0000-0000-0000-000000000001".into(),
            "00000000-0000-0000-0000-000000000002".into(),
            "00000000-0000-0000-0000-000000000003".into(),
            "00000000-0000-0000-0000-000000000004".into(),
            "Deep Tissue Balinese Massage 90m".into(),
            10_000,
            15_400,
            35_000_000, // 350,000 IDR (cents)
            2000,       // 20% commission
        )
        .expect("session created");

        assert_eq!(session.status, TreatmentStatus::Booked);
        session.start_session().expect("started");

        session
            .record_consumable(
                "00000000-0000-0000-0000-000000000055".into(),
                50, // 50ml essential massage oil
            )
            .expect("recorded consumable");

        session.complete_session().expect("completed");

        let payout = session
            .calculate_commission()
            .expect("commission calculated");
        assert_eq!(payout.gross_service_minor, 35_000_000);
        assert_eq!(payout.earned_commission_minor, 7_000_000); // 20% of 35M
    }
}
