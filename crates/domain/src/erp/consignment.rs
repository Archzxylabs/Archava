//! Consignment Retail & Vendor Custody Module (`archava_consignment`).
//!
//! Models consignment retail operations: third-party merchandise held in custody,
//! commission split computation, settlement lifecycle, and unsold item return chains.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::erp::{bounded_title, trimmed, valid_uuid};

pub const CONSIGNMENT_PERMISSIONS: &[&str] = &[
    "consignment.read",
    "consignment.intake",
    "consignment.display",
    "consignment.settle",
    "consignment.return",
    "audit.read",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgreementStatus {
    Draft,
    Active,
    Suspended,
    Terminated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    ReceivedInCustody,
    OnDisplay,
    SoldPendingSettlement,
    Settled,
    ReturnedToConsignor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettlementStatus {
    Calculated,
    Approved,
    Disbursed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsignmentAgreement {
    pub agreement_id: String,
    pub consignor_id: String,
    pub commission_rate_bps: u32, // Basis points (e.g. 2000 = 20.00%)
    pub settlement_period_days: u32,
    pub status: AgreementStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsignmentItem {
    pub item_id: String,
    pub agreement_id: String,
    pub consignor_id: String,
    pub sku: String,
    pub title: String,
    pub agreed_retail_price_minor: u64,
    pub status: ItemStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsignmentSettlement {
    pub settlement_id: String,
    pub consignor_id: String,
    pub total_sales_minor: u64,
    pub platform_commission_minor: u64,
    pub consignor_share_minor: u64,
    pub status: SettlementStatus,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConsignmentError {
    #[error("validation error: {0}")]
    Validation(&'static str),
    #[error("invalid status transition from {from:?} to {to:?}")]
    InvalidTransition { from: ItemStatus, to: ItemStatus },
    #[error("commission rate cannot exceed 10000 basis points (100%)")]
    ExcessiveCommission,
}

impl ConsignmentAgreement {
    pub fn new(
        agreement_id: String,
        consignor_id: String,
        commission_rate_bps: u32,
        settlement_period_days: u32,
    ) -> Result<Self, ConsignmentError> {
        if !valid_uuid(&agreement_id) || !valid_uuid(&consignor_id) {
            return Err(ConsignmentError::Validation("invalid uuid format"));
        }
        if commission_rate_bps > 10_000 {
            return Err(ConsignmentError::ExcessiveCommission);
        }
        if settlement_period_days == 0 || settlement_period_days > 365 {
            return Err(ConsignmentError::Validation(
                "settlement period must be between 1 and 365 days",
            ));
        }

        Ok(Self {
            agreement_id,
            consignor_id,
            commission_rate_bps,
            settlement_period_days,
            status: AgreementStatus::Draft,
        })
    }

    pub fn activate(&mut self) -> Result<(), ConsignmentError> {
        if self.status != AgreementStatus::Draft && self.status != AgreementStatus::Suspended {
            return Err(ConsignmentError::Validation(
                "can only activate draft or suspended agreement",
            ));
        }
        self.status = AgreementStatus::Active;
        Ok(())
    }
}

impl ConsignmentItem {
    pub fn new(
        item_id: String,
        agreement_id: String,
        consignor_id: String,
        sku: String,
        title: String,
        agreed_retail_price_minor: u64,
    ) -> Result<Self, ConsignmentError> {
        if !valid_uuid(&item_id) || !valid_uuid(&agreement_id) || !valid_uuid(&consignor_id) {
            return Err(ConsignmentError::Validation("invalid uuid format"));
        }
        if trimmed(&sku).is_empty() {
            return Err(ConsignmentError::Validation("sku cannot be empty"));
        }
        if bounded_title(&title, 200).is_none() {
            return Err(ConsignmentError::Validation(
                "title must be between 1 and 200 characters",
            ));
        }
        if agreed_retail_price_minor == 0 {
            return Err(ConsignmentError::Validation(
                "retail price must be greater than zero",
            ));
        }

        Ok(Self {
            item_id,
            agreement_id,
            consignor_id,
            sku,
            title,
            agreed_retail_price_minor,
            status: ItemStatus::ReceivedInCustody,
        })
    }

    pub fn place_on_display(&mut self) -> Result<(), ConsignmentError> {
        if self.status != ItemStatus::ReceivedInCustody {
            return Err(ConsignmentError::InvalidTransition {
                from: self.status,
                to: ItemStatus::OnDisplay,
            });
        }
        self.status = ItemStatus::OnDisplay;
        Ok(())
    }

    pub fn mark_sold(&mut self) -> Result<(), ConsignmentError> {
        if self.status != ItemStatus::OnDisplay {
            return Err(ConsignmentError::InvalidTransition {
                from: self.status,
                to: ItemStatus::SoldPendingSettlement,
            });
        }
        self.status = ItemStatus::SoldPendingSettlement;
        Ok(())
    }

    pub fn return_to_consignor(&mut self) -> Result<(), ConsignmentError> {
        match self.status {
            ItemStatus::ReceivedInCustody | ItemStatus::OnDisplay => {
                self.status = ItemStatus::ReturnedToConsignor;
                Ok(())
            }
            _ => Err(ConsignmentError::InvalidTransition {
                from: self.status,
                to: ItemStatus::ReturnedToConsignor,
            }),
        }
    }
}

impl ConsignmentSettlement {
    pub fn calculate(
        settlement_id: String,
        consignor_id: String,
        sold_items: &[ConsignmentItem],
        commission_rate_bps: u32,
    ) -> Result<Self, ConsignmentError> {
        if !valid_uuid(&settlement_id) || !valid_uuid(&consignor_id) {
            return Err(ConsignmentError::Validation("invalid uuid format"));
        }
        if commission_rate_bps > 10_000 {
            return Err(ConsignmentError::ExcessiveCommission);
        }
        if sold_items.is_empty() {
            return Err(ConsignmentError::Validation(
                "settlement requires at least one sold item",
            ));
        }

        let mut total_sales: u64 = 0;
        for item in sold_items {
            if item.status != ItemStatus::SoldPendingSettlement {
                return Err(ConsignmentError::Validation(
                    "all items in settlement must be sold and pending settlement",
                ));
            }
            if item.consignor_id != consignor_id {
                return Err(ConsignmentError::Validation(
                    "items must belong to the specified consignor",
                ));
            }
            total_sales = total_sales
                .checked_add(item.agreed_retail_price_minor)
                .ok_or(ConsignmentError::Validation("total sales overflow"))?;
        }

        let platform_commission =
            (total_sales as u128 * commission_rate_bps as u128 / 10_000) as u64;
        let consignor_share = total_sales - platform_commission;

        Ok(Self {
            settlement_id,
            consignor_id,
            total_sales_minor: total_sales,
            platform_commission_minor: platform_commission,
            consignor_share_minor: consignor_share,
            status: SettlementStatus::Calculated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consignment_agreement_lifecycle() {
        let mut agree = ConsignmentAgreement::new(
            "00000000-0000-0000-0000-000000000001".into(),
            "00000000-0000-0000-0000-000000000002".into(),
            2000, // 20%
            30,
        )
        .expect("agreement valid");

        assert_eq!(agree.status, AgreementStatus::Draft);
        agree.activate().expect("activated");
        assert_eq!(agree.status, AgreementStatus::Active);
    }

    #[test]
    fn consignment_item_and_settlement_split() {
        let mut item = ConsignmentItem::new(
            "00000000-0000-0000-0000-000000000010".into(),
            "00000000-0000-0000-0000-000000000001".into(),
            "00000000-0000-0000-0000-000000000002".into(),
            "SKU-WATCH-01".into(),
            "Vintage Mechanical Watch".into(),
            100_000_000, // 1,000,000 IDR (in cents)
        )
        .expect("item valid");

        assert_eq!(item.status, ItemStatus::ReceivedInCustody);
        item.place_on_display().expect("on display");
        item.mark_sold().expect("sold");

        let settlement = ConsignmentSettlement::calculate(
            "00000000-0000-0000-0000-000000000099".into(),
            "00000000-0000-0000-0000-000000000002".into(),
            &[item],
            2500, // 25% take rate
        )
        .expect("settlement calculated");

        assert_eq!(settlement.total_sales_minor, 100_000_000);
        assert_eq!(settlement.platform_commission_minor, 25_000_000); // 25%
        assert_eq!(settlement.consignor_share_minor, 75_000_000); // 75%
    }
}
