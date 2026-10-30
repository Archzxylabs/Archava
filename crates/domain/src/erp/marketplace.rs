//! Multi-Vendor Marketplace & Escrow Split Ledger Module (`archava_marketplace`).
//!
//! Models multi-vendor e-commerce marketplaces: merchant onboarding, escrow holds,
//! platform commission take-rate deductions, and vendor payout disbursements.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::erp::{bounded_title, trimmed, valid_uuid};

pub const MARKETPLACE_PERMISSIONS: &[&str] = &[
    "market.read",
    "market.onboard",
    "market.order",
    "market.release",
    "market.refund",
    "market.payout",
    "audit.read",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VendorStatus {
    PendingKyc,
    VerifiedActive,
    Suspended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscrowState {
    HeldInEscrow,
    ReleasedToMerchant,
    RefundedToCustomer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MerchantStore {
    pub store_id: String,
    pub merchant_user_id: String,
    pub store_name: String,
    pub commission_take_rate_bps: u32, // e.g. 500 = 5.00%
    pub payout_bank_code: String,
    pub payout_account_number: String,
    pub status: VendorStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketplaceOrder {
    pub marketplace_order_id: String,
    pub store_id: String,
    pub customer_id: String,
    pub items_subtotal_minor: u64,
    pub shipping_fee_minor: u64,
    pub platform_commission_minor: u64,
    pub merchant_net_payout_minor: u64,
    pub escrow_state: EscrowState,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MarketplaceError {
    #[error("validation error: {0}")]
    Validation(&'static str),
    #[error("commission cannot exceed 10000 bps")]
    ExcessiveCommission,
    #[error("invalid escrow transition from {from:?} to {to:?}")]
    InvalidTransition { from: EscrowState, to: EscrowState },
}

impl MerchantStore {
    pub fn new(
        store_id: String,
        merchant_user_id: String,
        store_name: String,
        commission_take_rate_bps: u32,
        payout_bank_code: String,
        payout_account_number: String,
    ) -> Result<Self, MarketplaceError> {
        if !valid_uuid(&store_id) || !valid_uuid(&merchant_user_id) {
            return Err(MarketplaceError::Validation("invalid uuid format"));
        }
        if bounded_title(&store_name, 100).is_none() {
            return Err(MarketplaceError::Validation(
                "store name must be between 1 and 100 characters",
            ));
        }
        if commission_take_rate_bps > 10_000 {
            return Err(MarketplaceError::ExcessiveCommission);
        }
        if trimmed(&payout_bank_code).is_empty() || trimmed(&payout_account_number).is_empty() {
            return Err(MarketplaceError::Validation(
                "bank code and account number are required",
            ));
        }

        Ok(Self {
            store_id,
            merchant_user_id,
            store_name,
            commission_take_rate_bps,
            payout_bank_code,
            payout_account_number,
            status: VendorStatus::PendingKyc,
        })
    }

    pub fn verify_kyc(&mut self) -> Result<(), MarketplaceError> {
        if self.status != VendorStatus::PendingKyc {
            return Err(MarketplaceError::Validation(
                "can only verify vendor in pending KYC state",
            ));
        }
        self.status = VendorStatus::VerifiedActive;
        Ok(())
    }
}

impl MarketplaceOrder {
    pub fn create_held_order(
        marketplace_order_id: String,
        store_id: String,
        customer_id: String,
        items_subtotal_minor: u64,
        shipping_fee_minor: u64,
        commission_take_rate_bps: u32,
    ) -> Result<Self, MarketplaceError> {
        if !valid_uuid(&marketplace_order_id) || !valid_uuid(&store_id) || !valid_uuid(&customer_id)
        {
            return Err(MarketplaceError::Validation("invalid uuid format"));
        }
        if items_subtotal_minor == 0 {
            return Err(MarketplaceError::Validation(
                "items subtotal must be greater than zero",
            ));
        }
        if commission_take_rate_bps > 10_000 {
            return Err(MarketplaceError::ExcessiveCommission);
        }

        let platform_commission =
            (items_subtotal_minor as u128 * commission_take_rate_bps as u128 / 10_000) as u64;

        // Shipping fee is passed directly to the merchant without platform cut
        let merchant_net =
            (items_subtotal_minor - platform_commission).saturating_add(shipping_fee_minor);

        Ok(Self {
            marketplace_order_id,
            store_id,
            customer_id,
            items_subtotal_minor,
            shipping_fee_minor,
            platform_commission_minor: platform_commission,
            merchant_net_payout_minor: merchant_net,
            escrow_state: EscrowState::HeldInEscrow,
        })
    }

    pub fn release_escrow_to_merchant(&mut self) -> Result<(), MarketplaceError> {
        if self.escrow_state != EscrowState::HeldInEscrow {
            return Err(MarketplaceError::InvalidTransition {
                from: self.escrow_state,
                to: EscrowState::ReleasedToMerchant,
            });
        }
        self.escrow_state = EscrowState::ReleasedToMerchant;
        Ok(())
    }

    pub fn refund_to_customer(&mut self) -> Result<(), MarketplaceError> {
        if self.escrow_state != EscrowState::HeldInEscrow {
            return Err(MarketplaceError::InvalidTransition {
                from: self.escrow_state,
                to: EscrowState::RefundedToCustomer,
            });
        }
        self.escrow_state = EscrowState::RefundedToCustomer;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marketplace_vendor_and_escrow_split() {
        let mut store = MerchantStore::new(
            "00000000-0000-0000-0000-000000000001".into(),
            "00000000-0000-0000-0000-000000000002".into(),
            "Batik Artisan Jogja".into(),
            500, // 5% take rate
            "BCA".into(),
            "8820192811".into(),
        )
        .expect("store created");

        assert_eq!(store.status, VendorStatus::PendingKyc);
        store.verify_kyc().expect("verified");
        assert_eq!(store.status, VendorStatus::VerifiedActive);

        let mut order = MarketplaceOrder::create_held_order(
            "00000000-0000-0000-0000-000000000010".into(),
            store.store_id.clone(),
            "00000000-0000-0000-0000-000000000099".into(),
            1_000_000, // 10,000 IDR items
            200_000,   // 2,000 IDR shipping
            store.commission_take_rate_bps,
        )
        .expect("order created in escrow");

        assert_eq!(order.escrow_state, EscrowState::HeldInEscrow);
        assert_eq!(order.platform_commission_minor, 50_000); // 5% of 1,000,000
        assert_eq!(order.merchant_net_payout_minor, 1_150_000); // 950,000 + 200,000

        order.release_escrow_to_merchant().expect("released");
        assert_eq!(order.escrow_state, EscrowState::ReleasedToMerchant);
    }
}
