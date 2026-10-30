//! Point of Sale (POS) domain — offline-first retail & F&B cash register operations,
//! shift management, multi-payment reconciliation, and receipt auditing.
//!
//! ## Invariants
//!
//! - Closed vocabularies: register status, shift status, transaction kind, payment methods.
//! - Strict balance accounting: `expected_cash = opening_cash + cash_sales + cash_in - cash_out - cash_refunds`.
//! - Variance derivation: `variance_cents = declared_closing_cash - expected_cash` (over/short tracking).
//! - Payment reconciliation: sum of payments must equal `total_cents`.
//! - Offline idempotency: `offline_id` guarantees that offline-queued sales are deduplicated on sync.
//! - Pure domain logic: zero I/O, zero sqlx, wasm32-clean.

use serde::{Deserialize, Serialize};

use super::purchasing::ValidationError;
use super::valid_uuid;

pub const POS_PERMISSIONS: &[&str] = &["pos.read", "pos.operate", "pos.close_shift", "pos.audit"];

// Register Statuses
pub const REGISTER_ACTIVE: &str = "active";
pub const REGISTER_SUSPENDED: &str = "suspended";
pub const REGISTER_RETIRED: &str = "retired";

// Shift Statuses
pub const SHIFT_OPEN: &str = "open";
pub const SHIFT_CLOSING: &str = "closing";
pub const SHIFT_RECONCILED: &str = "reconciled";

// Transaction Kinds
pub const TX_SALE: &str = "sale";
pub const TX_REFUND: &str = "refund";
pub const TX_CASH_IN: &str = "cash_in";
pub const TX_CASH_OUT: &str = "cash_out";

// Payment Methods
pub const PAY_CASH: &str = "cash";
pub const PAY_QRIS: &str = "qris";
pub const PAY_CARD: &str = "card";
pub const PAY_SPLIT: &str = "split";

// Limits
pub const MAX_LINES_PER_TX: usize = 100;
pub const MAX_PAYMENTS_PER_TX: usize = 10;
pub const NOTE_MAX: usize = 500;
pub const NAME_MAX: usize = 120;
pub const REF_MAX: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PosLine {
    pub line_no: u32,
    pub item_ref: String,
    pub name: String,
    pub unit_price_cents: u64,
    pub quantity: u32,
    pub discount_cents: u64,
    pub total_cents: u64,
}

impl PosLine {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.line_no == 0 {
            return Err(ValidationError("line_no must be non-zero"));
        }
        if self.item_ref.trim().is_empty() || self.item_ref.len() > REF_MAX {
            return Err(ValidationError("invalid item_ref length"));
        }
        if self.name.trim().is_empty() || self.name.len() > NAME_MAX {
            return Err(ValidationError("invalid item name length"));
        }
        if self.quantity == 0 {
            return Err(ValidationError("quantity must be greater than zero"));
        }
        let gross = self.unit_price_cents.saturating_mul(self.quantity as u64);
        if self.discount_cents > gross {
            return Err(ValidationError("discount cannot exceed gross line price"));
        }
        let expected = gross.saturating_sub(self.discount_cents);
        if self.total_cents != expected {
            return Err(ValidationError(
                "line total_cents mismatch with calculated price",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PosPayment {
    pub method: String,
    pub amount_cents: u64,
    pub auth_code: Option<String>,
    pub reference_id: Option<String>,
}

impl PosPayment {
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self.method.as_str() {
            PAY_CASH | PAY_QRIS | PAY_CARD => {}
            _ => return Err(ValidationError("unknown payment method")),
        }
        if self.amount_cents == 0 {
            return Err(ValidationError("payment amount must be positive"));
        }
        if let Some(ref auth) = self.auth_code {
            if auth.len() > REF_MAX {
                return Err(ValidationError("auth_code too long"));
            }
        }
        if let Some(ref r) = self.reference_id {
            if r.len() > REF_MAX {
                return Err(ValidationError("reference_id too long"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenShiftCommand {
    pub register_id: String,
    pub cashier_id: String,
    pub opening_cash_cents: u64,
}

impl OpenShiftCommand {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if !valid_uuid(&self.register_id) {
            return Err(ValidationError("invalid register_id UUID"));
        }
        if !valid_uuid(&self.cashier_id) {
            return Err(ValidationError("invalid cashier_id UUID"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordTransactionCommand {
    pub shift_id: String,
    pub sequence_no: u64,
    pub kind: String,
    pub lines: Vec<PosLine>,
    pub payment_method: String,
    pub payments: Vec<PosPayment>,
    pub offline_id: Option<String>,
}

impl RecordTransactionCommand {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if !valid_uuid(&self.shift_id) {
            return Err(ValidationError("invalid shift_id UUID"));
        }
        if self.sequence_no == 0 {
            return Err(ValidationError("sequence_no must be greater than zero"));
        }
        match self.kind.as_str() {
            TX_SALE | TX_REFUND => {}
            _ => return Err(ValidationError("invalid transaction kind for record")),
        }
        if self.lines.is_empty() || self.lines.len() > MAX_LINES_PER_TX {
            return Err(ValidationError("lines must be between 1 and 100"));
        }
        let mut subtotal: u64 = 0;
        for line in &self.lines {
            line.validate()?;
            subtotal = subtotal.saturating_add(line.total_cents);
        }
        if self.payments.is_empty() || self.payments.len() > MAX_PAYMENTS_PER_TX {
            return Err(ValidationError("payments must be between 1 and 10"));
        }
        let mut paid_total: u64 = 0;
        for payment in &self.payments {
            payment.validate()?;
            paid_total = paid_total.saturating_add(payment.amount_cents);
        }
        if paid_total != subtotal {
            return Err(ValidationError("sum of payments must equal subtotal"));
        }
        if let Some(ref off) = self.offline_id {
            if off.trim().is_empty() || off.len() > REF_MAX {
                return Err(ValidationError("invalid offline_id length"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseShiftCommand {
    pub shift_id: String,
    pub declared_closing_cash_cents: u64,
    pub notes: Option<String>,
}

impl CloseShiftCommand {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if !valid_uuid(&self.shift_id) {
            return Err(ValidationError("invalid shift_id UUID"));
        }
        if let Some(ref n) = self.notes {
            if n.len() > NOTE_MAX {
                return Err(ValidationError("notes exceed max length"));
            }
        }
        Ok(())
    }
}

/// Computes variance when reconciling shift closing.
pub fn calculate_shift_variance(
    opening_cash: u64,
    cash_sales: u64,
    cash_in: u64,
    cash_out: u64,
    cash_refunds: u64,
    declared_closing: u64,
) -> (u64, i64) {
    let expected = opening_cash
        .saturating_add(cash_sales)
        .saturating_add(cash_in)
        .saturating_sub(cash_out)
        .saturating_sub(cash_refunds);

    let variance = (declared_closing as i64) - (expected as i64);
    (expected, variance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pos_line_validation_rules() {
        let line = PosLine {
            line_no: 1,
            item_ref: "sku-coffee-01".to_string(),
            name: "Americano 250ml".to_string(),
            unit_price_cents: 25000,
            quantity: 2,
            discount_cents: 5000,
            total_cents: 45000,
        };
        assert!(line.validate().is_ok());

        // Discount exceeds gross
        let mut bad_line = line.clone();
        bad_line.discount_cents = 60000;
        assert!(bad_line.validate().is_err());

        // Mismatched total
        let mut mismatch_line = line.clone();
        mismatch_line.total_cents = 50000;
        assert!(mismatch_line.validate().is_err());
    }

    #[test]
    fn pos_transaction_payment_equality() {
        let tx = RecordTransactionCommand {
            shift_id: "00000000-0000-0000-0000-000000000001".to_string(),
            sequence_no: 1,
            kind: TX_SALE.to_string(),
            lines: vec![PosLine {
                line_no: 1,
                item_ref: "sku-01".to_string(),
                name: "Pastry".to_string(),
                unit_price_cents: 30000,
                quantity: 1,
                discount_cents: 0,
                total_cents: 30000,
            }],
            payment_method: PAY_SPLIT.to_string(),
            payments: vec![
                PosPayment {
                    method: PAY_CASH.to_string(),
                    amount_cents: 20000,
                    auth_code: None,
                    reference_id: None,
                },
                PosPayment {
                    method: PAY_QRIS.to_string(),
                    amount_cents: 10000,
                    auth_code: Some("QRIS-9923".to_string()),
                    reference_id: Some("MIDTRANS-001".to_string()),
                },
            ],
            offline_id: Some("offline-pos-1-001".to_string()),
        };
        assert!(tx.validate().is_ok());

        // Underpaid
        let mut underpaid = tx.clone();
        underpaid.payments[1].amount_cents = 5000;
        assert!(underpaid.validate().is_err());
    }

    #[test]
    fn shift_variance_calculation() {
        let opening = 500_000; // Rp 500.000 kas awal
        let sales = 1_200_000; // Rp 1.200.000 penjualan tunai
        let cash_in = 100_000; // Rp 100.000 tukar uang receh
        let cash_out = 50_000; // Rp 50.000 beli es batu
        let refunds = 25_000; // Rp 25.000 retur

        let declared_exact = 1_725_000;
        let (expected, var) =
            calculate_shift_variance(opening, sales, cash_in, cash_out, refunds, declared_exact);
        assert_eq!(expected, 1_725_000);
        assert_eq!(var, 0);

        // Shortage (Kas tekor Rp 15.000)
        let declared_short = 1_710_000;
        let (_, var_short) =
            calculate_shift_variance(opening, sales, cash_in, cash_out, refunds, declared_short);
        assert_eq!(var_short, -15_000);

        // Overage (Kas lebih Rp 10.000)
        let declared_over = 1_735_000;
        let (_, var_over) =
            calculate_shift_variance(opening, sales, cash_in, cash_out, refunds, declared_over);
        assert_eq!(var_over, 10_000);
    }
}
