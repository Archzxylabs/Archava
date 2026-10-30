//! Automotive Fleet Management domain — vehicle registers, monotonic odometer logs,
//! fuel efficiency tracking, and periodic maintenance dispatch.
//!
//! ## Invariants
//!
//! - Closed vocabularies: vehicle status, fuel type, service kind.
//! - Monotonic odometer invariant: new odometer reading cannot be lower than existing vehicle odometer.
//! - Fuel efficiency calculation: tracks kilometers traveled per liter between fill-ups.
//! - Pure domain logic: zero I/O, zero sqlx, wasm32-clean.

use serde::{Deserialize, Serialize};

use super::purchasing::ValidationError;
use super::valid_uuid;

pub const FLEET_PERMISSIONS: &[&str] = &[
    "fleet.read",
    "fleet.log",
    "fleet.maintenance",
    "fleet.audit",
];

// Vehicle Status
pub const VEHICLE_ACTIVE: &str = "active";
pub const VEHICLE_IN_SERVICE: &str = "in_service";
pub const VEHICLE_DECOMMISSIONED: &str = "decommissioned";

// Fuel Types
pub const FUEL_GASOLINE: &str = "gasoline";
pub const FUEL_DIESEL: &str = "diesel";
pub const FUEL_ELECTRIC: &str = "electric";
pub const FUEL_HYBRID: &str = "hybrid";

// Service Kinds
pub const SERVICE_PERIODIC_OIL: &str = "periodic_oil";
pub const SERVICE_TIRE_REPLACE: &str = "tire_replacement";
pub const SERVICE_BRAKE_OVERHAUL: &str = "brake_overhaul";
pub const SERVICE_BATTERY_CHECK: &str = "battery_check";
pub const SERVICE_EMERGENCY: &str = "emergency_repair";

// Bounds
pub const PLATE_MAX: usize = 20;
pub const VIN_MAX: usize = 30;
pub const MODEL_MAX: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordFuelLogCommand {
    pub vehicle_id: String,
    pub driver_id: String,
    pub liters: u32,
    pub cost_cents: u64,
    pub current_odometer_km: u64,
}

impl RecordFuelLogCommand {
    pub fn validate(&self, existing_vehicle_odometer_km: u64) -> Result<(), ValidationError> {
        if !valid_uuid(&self.vehicle_id) {
            return Err(ValidationError("invalid vehicle_id UUID"));
        }
        if !valid_uuid(&self.driver_id) {
            return Err(ValidationError("invalid driver_id UUID"));
        }
        if self.liters == 0 {
            return Err(ValidationError("liters must be greater than zero"));
        }
        if self.cost_cents == 0 {
            return Err(ValidationError("cost_cents must be greater than zero"));
        }
        if self.current_odometer_km < existing_vehicle_odometer_km {
            return Err(ValidationError("odometer reading cannot roll backward"));
        }
        Ok(())
    }
}

/// Calculate fuel efficiency in km/liter.
pub fn calculate_fuel_efficiency(
    distance_traveled_km: u64,
    liters_consumed: f64,
) -> Result<f64, ValidationError> {
    if liters_consumed <= 0.0 {
        return Err(ValidationError("liters consumed must be strictly positive"));
    }
    Ok((distance_traveled_km as f64) / liters_consumed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuel_log_odometer_monotonic_check() {
        let cmd = RecordFuelLogCommand {
            vehicle_id: "00000000-0000-0000-0000-000000000001".to_string(),
            driver_id: "00000000-0000-0000-0000-000000000002".to_string(),
            liters: 45,
            cost_cents: 67500000,
            current_odometer_km: 12500,
        };

        // Current odometer 12000 -> Valid
        assert!(cmd.validate(12000).is_ok());

        // Current odometer 13000 -> Attempt to roll back to 12500 -> Error
        assert!(cmd.validate(13000).is_err());
    }

    #[test]
    fn fuel_efficiency_computation() {
        let efficiency = calculate_fuel_efficiency(450, 30.0).unwrap();
        assert!((efficiency - 15.0).abs() < 1e-6); // 15 km/liter
    }
}
