//! Venue Booking & Event Spaces Module (`archava_venue_booking`).
//!
//! Models banquet hall, convention space, and meeting room reservations:
//! layout arrangements (theater, classroom, banquet), audiovisual/catering package
//! bundles, and multi-day calendar locking.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::erp::valid_uuid;

pub const VENUE_PERMISSIONS: &[&str] = &[
    "venue.read",
    "venue.reserve",
    "venue.deposit",
    "venue.settle",
    "venue.cancel",
    "audit.read",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutType {
    Theater,
    Classroom,
    BanquetRoundTables,
    UShape,
    CocktailStanding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReservationStatus {
    Inquiry,
    HoldPendingDeposit,
    DepositPaid,
    Settled,
    Completed,
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventPackage {
    pub catering_package_id: Option<String>,
    pub av_sound_system: bool,
    pub stage_lighting: bool,
    pub live_streaming_rig: bool,
    pub package_addon_minor: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenueReservation {
    pub reservation_id: String,
    pub space_id: String,
    pub organizer_id: String,
    pub start_epoch_sec: i64,
    pub end_epoch_sec: i64,
    pub guest_count: u32,
    pub layout: LayoutType,
    pub package: EventPackage,
    pub venue_rental_minor: u64,
    pub deposit_required_minor: u64,
    pub status: ReservationStatus,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VenueError {
    #[error("validation error: {0}")]
    Validation(&'static str),
    #[error("invalid transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: ReservationStatus,
        to: ReservationStatus,
    },
    #[error("guest count exceeds space capacity limit")]
    CapacityExceeded,
}

impl VenueReservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        reservation_id: String,
        space_id: String,
        organizer_id: String,
        start_epoch_sec: i64,
        end_epoch_sec: i64,
        guest_count: u32,
        space_max_capacity: u32,
        layout: LayoutType,
        package: EventPackage,
        venue_rental_minor: u64,
        deposit_required_minor: u64,
    ) -> Result<Self, VenueError> {
        if !valid_uuid(&reservation_id) || !valid_uuid(&space_id) || !valid_uuid(&organizer_id) {
            return Err(VenueError::Validation("invalid uuid format"));
        }
        if end_epoch_sec <= start_epoch_sec {
            return Err(VenueError::Validation(
                "end timestamp must be strictly after start timestamp",
            ));
        }
        if guest_count == 0 {
            return Err(VenueError::Validation(
                "guest count must be greater than zero",
            ));
        }
        if guest_count > space_max_capacity {
            return Err(VenueError::CapacityExceeded);
        }
        if deposit_required_minor > venue_rental_minor.saturating_add(package.package_addon_minor) {
            return Err(VenueError::Validation(
                "deposit cannot exceed total reservation cost",
            ));
        }

        Ok(Self {
            reservation_id,
            space_id,
            organizer_id,
            start_epoch_sec,
            end_epoch_sec,
            guest_count,
            layout,
            package,
            venue_rental_minor,
            deposit_required_minor,
            status: ReservationStatus::Inquiry,
        })
    }

    pub fn place_hold(&mut self) -> Result<(), VenueError> {
        if self.status != ReservationStatus::Inquiry {
            return Err(VenueError::InvalidTransition {
                from: self.status,
                to: ReservationStatus::HoldPendingDeposit,
            });
        }
        self.status = ReservationStatus::HoldPendingDeposit;
        Ok(())
    }

    pub fn record_deposit(&mut self) -> Result<(), VenueError> {
        if self.status != ReservationStatus::HoldPendingDeposit {
            return Err(VenueError::InvalidTransition {
                from: self.status,
                to: ReservationStatus::DepositPaid,
            });
        }
        self.status = ReservationStatus::DepositPaid;
        Ok(())
    }

    pub fn total_cost_minor(&self) -> u64 {
        self.venue_rental_minor
            .saturating_add(self.package.package_addon_minor)
    }

    pub fn remaining_balance_minor(&self) -> u64 {
        self.total_cost_minor()
            .saturating_sub(self.deposit_required_minor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn venue_booking_capacity_and_pricing() {
        let pkg = EventPackage {
            catering_package_id: Some("00000000-0000-0000-0000-000000000099".into()),
            av_sound_system: true,
            stage_lighting: true,
            live_streaming_rig: false,
            package_addon_minor: 15_000_000, // 150k
        };

        let mut res = VenueReservation::new(
            "00000000-0000-0000-0000-000000000001".into(),
            "00000000-0000-0000-0000-000000000002".into(),
            "00000000-0000-0000-0000-000000000003".into(),
            1_700_000_000,
            1_700_028_800, // 8 hours
            250,
            500, // max capacity 500
            LayoutType::BanquetRoundTables,
            pkg,
            50_000_000, // 500k venue
            20_000_000, // 200k deposit
        )
        .expect("reservation valid");

        assert_eq!(res.total_cost_minor(), 65_000_000);
        assert_eq!(res.remaining_balance_minor(), 45_000_000);

        res.place_hold().expect("hold placed");
        res.record_deposit().expect("deposit recorded");
        assert_eq!(res.status, ReservationStatus::DepositPaid);
    }
}
