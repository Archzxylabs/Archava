//! Advanced Inventory Valuation & Landed Cost Engine.
//!
//! Provides deterministic inventory costing:
//! 1. FIFO (First-In, First-Out) Cost Layer Queue.
//! 2. Moving Average Cost (MAC) continuous recalculation.
//! 3. Landed Cost Allocation Engine (freight, customs, tariffs, handling fees).
//! 4. FEFO (First-Expired, First-Out) batch prioritization.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostLayer {
    pub layer_id: String,
    pub lot_number: Option<String>,
    pub received_at: String,
    pub expiry_date: Option<String>, // "YYYY-MM-DD"
    pub original_qty: i64,
    pub remaining_qty: i64,
    pub unit_cost_cents: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepletionResult {
    pub consumed_qty: i64,
    pub total_cogs_cents: i64,
    pub depleted_layers: Vec<CostLayerDepletion>,
    pub remaining_stock_qty: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostLayerDepletion {
    pub layer_id: String,
    pub lot_number: Option<String>,
    pub qty_taken: i64,
    pub unit_cost_cents: i64,
    pub subtotal_cents: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AllocationMethod {
    ByValue,
    ByQuantity,
    ByWeightGrams,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandedCostLineInput {
    pub item_id: String,
    pub quantity: i64,
    pub base_cost_cents: i64,
    pub weight_grams: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandedCostAllocatedItem {
    pub item_id: String,
    pub quantity: i64,
    pub original_unit_cost_cents: i64,
    pub allocated_landed_fee_cents: i64,
    pub effective_unit_cost_cents: i64,
}

/// FIFO Inventory Layer Manager.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FifoInventoryQueue {
    pub layers: Vec<CostLayer>,
}

impl FifoInventoryQueue {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    /// Receives new goods into inventory as a new FIFO cost layer.
    pub fn receive_goods(
        &mut self,
        layer_id: &str,
        lot_number: Option<&str>,
        received_at: &str,
        expiry_date: Option<&str>,
        qty: i64,
        unit_cost_cents: i64,
    ) -> Result<(), &'static str> {
        if qty <= 0 {
            return Err("Received quantity must be positive");
        }
        if unit_cost_cents < 0 {
            return Err("Unit cost cannot be negative");
        }
        self.layers.push(CostLayer {
            layer_id: layer_id.to_string(),
            lot_number: lot_number.map(str::to_string),
            received_at: received_at.to_string(),
            expiry_date: expiry_date.map(str::to_string),
            original_qty: qty,
            remaining_qty: qty,
            unit_cost_cents,
        });
        Ok(())
    }

    /// Depletes inventory according to First-In, First-Out (FIFO) queue order.
    pub fn deplete_fifo(&mut self, request_qty: i64) -> Result<DepletionResult, &'static str> {
        if request_qty <= 0 {
            return Err("Requested quantity must be positive");
        }

        let total_available: i64 = self.layers.iter().map(|l| l.remaining_qty).sum();
        if request_qty > total_available {
            return Err("Insufficient inventory stock to fulfill depletion");
        }

        let mut to_deplete = request_qty;
        let mut total_cogs: i64 = 0;
        let mut depleted_records = Vec::new();

        for layer in &mut self.layers {
            if to_deplete == 0 {
                break;
            }
            if layer.remaining_qty == 0 {
                continue;
            }

            let take = to_deplete.min(layer.remaining_qty);
            layer.remaining_qty -= take;
            to_deplete -= take;

            let subtotal = take * layer.unit_cost_cents;
            total_cogs += subtotal;

            depleted_records.push(CostLayerDepletion {
                layer_id: layer.layer_id.clone(),
                lot_number: layer.lot_number.clone(),
                qty_taken: take,
                unit_cost_cents: layer.unit_cost_cents,
                subtotal_cents: subtotal,
            });
        }

        // Clean up completely exhausted layers
        self.layers.retain(|l| l.remaining_qty > 0);

        let remaining_stock = self.layers.iter().map(|l| l.remaining_qty).sum();

        Ok(DepletionResult {
            consumed_qty: request_qty,
            total_cogs_cents: total_cogs,
            depleted_layers: depleted_records,
            remaining_stock_qty: remaining_stock,
        })
    }

    /// Sorts remaining layers by expiration date (FEFO: First Expired, First Out).
    pub fn sort_fefo(&mut self) {
        self.layers
            .sort_by(|a, b| match (&a.expiry_date, &b.expiry_date) {
                (Some(exp_a), Some(exp_b)) => exp_a.cmp(exp_b),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.received_at.cmp(&b.received_at),
            });
    }
}

/// Continuous Moving Average Cost (MAC) calculator.
pub fn update_moving_average_cost(
    current_qty: i64,
    current_mac_cents: i64,
    incoming_qty: i64,
    incoming_cost_cents: i64,
) -> i64 {
    let total_qty = current_qty + incoming_qty;
    if total_qty <= 0 {
        return incoming_cost_cents;
    }

    let current_total_value = current_qty * current_mac_cents;
    let incoming_total_value = incoming_qty * incoming_cost_cents;
    (current_total_value + incoming_total_value) / total_qty
}

/// Allocates total landed expenses (shipping, customs, duties) across inbound purchase items.
pub fn allocate_landed_cost(
    items: &[LandedCostLineInput],
    total_landed_fee_cents: i64,
    method: AllocationMethod,
) -> Result<Vec<LandedCostAllocatedItem>, &'static str> {
    if items.is_empty() {
        return Err("Items list cannot be empty");
    }
    if total_landed_fee_cents < 0 {
        return Err("Landed cost fee cannot be negative");
    }
    for it in items {
        if it.quantity <= 0 {
            return Err("Item quantity must be strictly positive");
        }
        if it.base_cost_cents < 0 {
            return Err("Item base cost cannot be negative");
        }
        if it.weight_grams < 0 {
            return Err("Item weight cannot be negative");
        }
    }

    let divisor: f64 = match method {
        AllocationMethod::ByValue => items
            .iter()
            .map(|it| (it.quantity * it.base_cost_cents) as f64)
            .sum(),
        AllocationMethod::ByQuantity => items.iter().map(|it| it.quantity as f64).sum(),
        AllocationMethod::ByWeightGrams => items.iter().map(|it| it.weight_grams as f64).sum(),
    };

    if divisor <= 0.0 {
        return Err("Total allocation basis cannot be zero");
    }

    // C05 FIX: Largest Remainder Method (Hare-Niemeyer / Hamilton Quota Allocation)
    // Prevents negative fee distribution and guarantees exact cent conservation: sum(allocated) == total_landed_fee_cents
    let mut integer_allocations = Vec::with_capacity(items.len());
    let mut fractional_remainders = Vec::with_capacity(items.len());
    let mut allocated_sum: i64 = 0;

    for (idx, it) in items.iter().enumerate() {
        let weight = match method {
            AllocationMethod::ByValue => (it.quantity * it.base_cost_cents) as f64 / divisor,
            AllocationMethod::ByQuantity => it.quantity as f64 / divisor,
            AllocationMethod::ByWeightGrams => it.weight_grams as f64 / divisor,
        };

        let continuous_quota = (total_landed_fee_cents as f64) * weight;
        let floor_quota = continuous_quota.floor() as i64;
        let remainder = continuous_quota - (floor_quota as f64);

        integer_allocations.push(floor_quota);
        fractional_remainders.push((idx, remainder));
        allocated_sum += floor_quota;
    }

    let remaining_cents = (total_landed_fee_cents - allocated_sum) as usize;
    fractional_remainders
        .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (idx, _) in fractional_remainders.iter().take(remaining_cents) {
        integer_allocations[*idx] += 1;
    }

    let mut allocated_items = Vec::with_capacity(items.len());
    for (idx, it) in items.iter().enumerate() {
        let allocated_fee = integer_allocations[idx];
        let unit_allocated_fee = if it.quantity > 0 {
            allocated_fee / it.quantity
        } else {
            0
        };

        let effective_unit_cost = it.base_cost_cents + unit_allocated_fee;

        allocated_items.push(LandedCostAllocatedItem {
            item_id: it.item_id.clone(),
            quantity: it.quantity,
            original_unit_cost_cents: it.base_cost_cents,
            allocated_landed_fee_cents: allocated_fee,
            effective_unit_cost_cents: effective_unit_cost,
        });
    }

    Ok(allocated_items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fifo_inventory_depletion_and_cogs() {
        let mut queue = FifoInventoryQueue::new();

        // Batch 1: 10 units @ Rp 10.000
        queue
            .receive_goods("layer_1", Some("LOT-A"), "2026-10-01", None, 10, 10_000_00)
            .expect("receive layer_1");
        // Batch 2: 15 units @ Rp 12.000
        queue
            .receive_goods("layer_2", Some("LOT-B"), "2026-10-05", None, 15, 12_000_00)
            .expect("receive layer_2");

        // Deplete 12 units (should take all 10 from layer_1 @ 10k, and 2 from layer_2 @ 12k)
        let res = queue.deplete_fifo(12).expect("Depletion succeeds");

        assert_eq!(res.consumed_qty, 12);
        // COGS = (10 * 10k) + (2 * 12k) = 100k + 24k = Rp 124.000
        assert_eq!(res.total_cogs_cents, 124_000_00);
        assert_eq!(res.remaining_stock_qty, 13);
        assert_eq!(res.depleted_layers.len(), 2);
    }

    #[test]
    fn test_moving_average_cost_update() {
        // Have 100 units @ Rp 5.000 (Val: 500.000)
        // Receive 50 units @ Rp 8.000 (Val: 400.000)
        // Total = 150 units, Total Val = 900.000 -> MAC = Rp 6.000
        let mac = update_moving_average_cost(100, 5_000_00, 50, 8_000_00);
        assert_eq!(mac, 6_000_00);
    }

    #[test]
    fn test_landed_cost_allocation_by_value() {
        let items = vec![
            LandedCostLineInput {
                item_id: "SKU_A".into(),
                quantity: 10,
                base_cost_cents: 100_000_00,
                weight_grams: 1000,
            }, // Val: 1.000.000 (25%)
            LandedCostLineInput {
                item_id: "SKU_B".into(),
                quantity: 10,
                base_cost_cents: 300_000_00,
                weight_grams: 1000,
            }, // Val: 3.000.000 (75%)
        ];

        // Total import duty & freight = Rp 400.000
        let allocated = allocate_landed_cost(&items, 400_000_00, AllocationMethod::ByValue)
            .expect("Allocation succeeds");

        assert_eq!(allocated.len(), 2);
        // SKU_A gets 25% of 400k = 100k -> 10k per unit -> Effective unit cost = 110.000
        assert_eq!(allocated[0].allocated_landed_fee_cents, 100_000_00);
        assert_eq!(allocated[0].effective_unit_cost_cents, 110_000_00);

        // SKU_B gets 75% of 400k = 300k -> 30k per unit -> Effective unit cost = 330.000
        assert_eq!(allocated[1].allocated_landed_fee_cents, 300_000_00);
        assert_eq!(allocated[1].effective_unit_cost_cents, 330_000_00);
    }

    #[test]
    fn test_fefo_expiry_sorting() {
        let mut queue = FifoInventoryQueue::new();
        queue
            .receive_goods(
                "layer_far",
                None,
                "2026-10-01",
                Some("2027-12-31"),
                10,
                1000,
            )
            .expect("receive layer_far");
        queue
            .receive_goods(
                "layer_soon",
                None,
                "2026-10-02",
                Some("2026-11-15"),
                10,
                1000,
            )
            .expect("receive layer_soon");

        queue.sort_fefo();
        assert_eq!(queue.layers[0].layer_id, "layer_soon");
    }

    #[test]
    fn test_c05_reproduction_no_negative_landed_cost() {
        // Codex C05 finding: 4 items quantity 1, fee 2 -> previously produced [1, 1, 1, -1]
        let items = vec![
            LandedCostLineInput {
                item_id: "i1".into(),
                quantity: 1,
                base_cost_cents: 10,
                weight_grams: 1,
            },
            LandedCostLineInput {
                item_id: "i2".into(),
                quantity: 1,
                base_cost_cents: 10,
                weight_grams: 1,
            },
            LandedCostLineInput {
                item_id: "i3".into(),
                quantity: 1,
                base_cost_cents: 10,
                weight_grams: 1,
            },
            LandedCostLineInput {
                item_id: "i4".into(),
                quantity: 1,
                base_cost_cents: 10,
                weight_grams: 1,
            },
        ];

        let allocated =
            allocate_landed_cost(&items, 2, AllocationMethod::ByQuantity).expect("allocated");
        assert_eq!(allocated.len(), 4);
        let total_allocated: i64 = allocated
            .iter()
            .map(|it| it.allocated_landed_fee_cents)
            .sum();
        assert_eq!(total_allocated, 2);
        for item in &allocated {
            assert!(
                item.allocated_landed_fee_cents >= 0,
                "no allocation may be negative: got {}",
                item.allocated_landed_fee_cents
            );
        }
    }

    #[test]
    fn test_negative_quantity_in_allocation_is_rejected() {
        let items = vec![
            LandedCostLineInput {
                item_id: "i1".into(),
                quantity: -1,
                base_cost_cents: 10,
                weight_grams: 1,
            },
            LandedCostLineInput {
                item_id: "i2".into(),
                quantity: 2,
                base_cost_cents: 10,
                weight_grams: 1,
            },
        ];

        let result = allocate_landed_cost(&items, 10, AllocationMethod::ByQuantity);
        assert!(result.is_err(), "Negative quantity item must be rejected");
    }

    #[test]
    fn test_invalid_goods_receipt_is_rejected() {
        let mut queue = FifoInventoryQueue::new();
        assert!(queue
            .receive_goods("l1", None, "2026-10-28", None, -5, 100)
            .is_err());
        assert!(queue
            .receive_goods("l2", None, "2026-10-28", None, 5, -100)
            .is_err());
    }
}
