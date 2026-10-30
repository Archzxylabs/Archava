//! Discrete Manufacturing & Assembly (MRP) domain — Bill of Materials (BOM),
//! component allocation calculations, production work orders, and finished goods reconciliation.
//!
//! ## Invariants
//!
//! - Closed vocabularies: BOM status, order status, component unit of measure.
//! - Accurate material allocation: `required_quantity = (qty_per_unit * target_batch) * (1 + scrap_percent / 100)`.
//! - Strict monotonic lifecycle: `planned` → `released` → `in_production` → `completed`.
//! - Pure domain logic: zero I/O, zero sqlx, wasm32-clean.

use serde::{Deserialize, Serialize};

use super::purchasing::ValidationError;
use super::valid_uuid;

pub const MRP_PERMISSIONS: &[&str] = &["mrp.read", "mrp.plan", "mrp.produce", "mrp.audit"];

// BOM Statuses
pub const BOM_DRAFT: &str = "draft";
pub const BOM_ACTIVE: &str = "active";
pub const BOM_DEPRECATED: &str = "deprecated";

// Production Order Statuses
pub const ORDER_PLANNED: &str = "planned";
pub const ORDER_RELEASED: &str = "released";
pub const ORDER_IN_PRODUCTION: &str = "in_production";
pub const ORDER_COMPLETED: &str = "completed";
pub const ORDER_CANCELED: &str = "canceled";

// Units
pub const UNIT_PIECE: &str = "piece";
pub const UNIT_METER: &str = "meter";
pub const UNIT_KILOGRAM: &str = "kilogram";
pub const UNIT_LITER: &str = "liter";

// Bounds
pub const SKU_MAX: usize = 60;
pub const NAME_MAX: usize = 120;
pub const MAX_COMPONENTS_PER_BOM: usize = 100;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BomComponent {
    pub component_sku: String,
    pub name: String,
    pub quantity_per_unit: f64,
    pub unit: String,
    pub scrap_factor_percent: f64, // e.g. 5.0 for 5% expected waste
}

impl BomComponent {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.component_sku.trim().is_empty() || self.component_sku.len() > SKU_MAX {
            return Err(ValidationError("invalid component SKU length"));
        }
        if self.name.trim().is_empty() || self.name.len() > NAME_MAX {
            return Err(ValidationError("invalid component name length"));
        }
        if self.quantity_per_unit <= 0.0 {
            return Err(ValidationError(
                "quantity_per_unit must be strictly positive",
            ));
        }
        match self.unit.as_str() {
            UNIT_PIECE | UNIT_METER | UNIT_KILOGRAM | UNIT_LITER => {}
            _ => return Err(ValidationError("unsupported unit of measure")),
        }
        if self.scrap_factor_percent < 0.0 || self.scrap_factor_percent > 100.0 {
            return Err(ValidationError(
                "scrap_factor_percent must be between 0.0 and 100.0",
            ));
        }
        Ok(())
    }

    /// Calculate total required raw materials including estimated scrap.
    pub fn calculate_allocation(&self, target_batch: u32) -> f64 {
        let gross = self.quantity_per_unit * (target_batch as f64);
        let scrap = gross * (self.scrap_factor_percent / 100.0);
        gross + scrap
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateBomCommand {
    pub finished_sku: String,
    pub name: String,
    pub revision: u32,
    pub components: Vec<BomComponent>,
}

impl CreateBomCommand {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.finished_sku.trim().is_empty() || self.finished_sku.len() > SKU_MAX {
            return Err(ValidationError("invalid finished SKU length"));
        }
        if self.name.trim().is_empty() || self.name.len() > NAME_MAX {
            return Err(ValidationError("invalid BOM name length"));
        }
        if self.revision == 0 {
            return Err(ValidationError("revision must start at 1 or higher"));
        }
        if self.components.is_empty() || self.components.len() > MAX_COMPONENTS_PER_BOM {
            return Err(ValidationError(
                "BOM must contain between 1 and 100 components",
            ));
        }
        for comp in &self.components {
            comp.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvanceProductionCommand {
    pub order_id: String,
    pub from_status: String,
    pub to_status: String,
    pub units_produced: u32,
    pub units_scrapped: u32,
}

impl AdvanceProductionCommand {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if !valid_uuid(&self.order_id) {
            return Err(ValidationError("invalid order_id UUID"));
        }
        match (self.from_status.as_str(), self.to_status.as_str()) {
            (ORDER_PLANNED, ORDER_RELEASED) => {}
            (ORDER_RELEASED, ORDER_IN_PRODUCTION) => {}
            (ORDER_IN_PRODUCTION, ORDER_COMPLETED) => {}
            (ORDER_PLANNED, ORDER_CANCELED) | (ORDER_RELEASED, ORDER_CANCELED) => {}
            _ => {
                return Err(ValidationError(
                    "illegal production order status transition",
                ))
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bom_component_allocation_calculation() {
        let fabric = BomComponent {
            component_sku: "RAW-FABRIC-01".to_string(),
            name: "Katun Combed 30s".to_string(),
            quantity_per_unit: 1.5, // 1.5 meters per shirt
            unit: UNIT_METER.to_string(),
            scrap_factor_percent: 10.0, // 10% cutting scrap
        };
        assert!(fabric.validate().is_ok());

        // Batch of 100 shirts: (1.5 * 100) + 10% = 150 + 15 = 165 meters
        let required = fabric.calculate_allocation(100);
        assert!((required - 165.0).abs() < 1e-6);
    }

    #[test]
    fn production_order_valid_transitions() {
        let advance = AdvanceProductionCommand {
            order_id: "00000000-0000-0000-0000-000000000001".to_string(),
            from_status: ORDER_PLANNED.to_string(),
            to_status: ORDER_RELEASED.to_string(),
            units_produced: 0,
            units_scrapped: 0,
        };
        assert!(advance.validate().is_ok());

        // Illegal jump from planned to completed directly
        let mut illegal = advance.clone();
        illegal.to_status = ORDER_COMPLETED.to_string();
        assert!(illegal.validate().is_err());
    }
}
