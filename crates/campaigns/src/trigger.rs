//! Operational Triggers for Proactive Autonomous Campaigns.
//! Monitors business telemetry across AR invoices, inventory levels, and customer lifecycle.

use serde::{Deserialize, Serialize};

/// The operational condition that triggers an autonomous campaign.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerKind {
    /// Overdue invoice requiring accounts receivable follow-up.
    OverdueInvoice {
        invoice_id: String,
        customer_name: String,
        customer_phone: String,
        amount_cents: i64,
        days_overdue: u32,
    },
    /// Inventory stock fell below calculated safety buffer.
    LowStockAlert {
        sku: String,
        item_name: String,
        current_qty: i32,
        reorder_point_qty: i32,
        recommended_order_qty: i32,
        supplier_id: String,
    },
    /// High-value customer dormant for extended duration.
    DormantCustomer {
        customer_id: String,
        customer_name: String,
        customer_phone: String,
        days_inactive: u32,
        lifetime_spend_cents: i64,
    },
}
