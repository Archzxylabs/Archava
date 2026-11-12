//! Exact native purchase-source account roles and balanced receipt/claim/void effects.
use super::sources::{valid_refs, AccountRef};
use super::{AccountClass, CashRole, ValidationError, MAX_MINOR};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PurchaseRole {
    Inventory,
    GoodsReceivedNotInvoiced,
    Payable,
    LandedFeeAccrual,
}
impl PurchaseRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inventory => "inventory",
            Self::GoodsReceivedNotInvoiced => "goods_received_not_invoiced",
            Self::Payable => "payable",
            Self::LandedFeeAccrual => "landed_fee_accrual",
        }
    }
    pub fn classification(self) -> (AccountClass, CashRole) {
        (
            if self == Self::Inventory {
                AccountClass::Asset
            } else {
                AccountClass::Liability
            },
            CashRole::Operating,
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PurchaseMapping {
    pub role: PurchaseRole,
    pub account_id: String,
    pub expected_account_revision: i32,
}
pub fn valid_mappings(mappings: &[PurchaseMapping]) -> bool {
    mappings.len() == 4
        && mappings
            .iter()
            .map(|m| m.role)
            .collect::<BTreeSet<_>>()
            .len()
            == 4
        && valid_refs(
            &mappings
                .iter()
                .map(|m| AccountRef {
                    account_id: m.account_id.clone(),
                    expected_account_revision: m.expected_account_revision,
                })
                .collect::<Vec<_>>(),
            4,
        )
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PurchaseEffect {
    Receipt,
    Bill,
    Void,
}
impl PurchaseEffect {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Receipt => "receipt",
            Self::Bill => "bill",
            Self::Void => "void",
        }
    }
}
/// Decimal strings cross JSON boundaries without truncating financial integers.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseLeg {
    pub role: PurchaseRole,
    pub debit_minor: String,
    pub credit_minor: String,
}
/// Amounts are already book minor units. Unit conversion belongs at source capture.
pub fn legs(
    effect: PurchaseEffect,
    goods: i64,
    landed_fee: i64,
) -> Result<Vec<PurchaseLeg>, ValidationError> {
    if !(-MAX_MINOR..=MAX_MINOR).contains(&goods)
        || !(0..=MAX_MINOR).contains(&landed_fee)
        || (effect != PurchaseEffect::Receipt && landed_fee != 0)
        || (effect == PurchaseEffect::Void && goods > 0)
        || (effect != PurchaseEffect::Void && goods < 0)
    {
        return Err(ValidationError("purchaseSourceAmount"));
    }
    let mut result = vec![];
    let mut push = |role, amount: i64, debit| {
        if amount == 0 {
            return;
        }
        result.push(PurchaseLeg {
            role,
            debit_minor: if debit {
                amount.to_string()
            } else {
                "0".into()
            },
            credit_minor: if debit {
                "0".into()
            } else {
                amount.to_string()
            },
        });
    };
    match effect {
        PurchaseEffect::Receipt => {
            let total = goods
                .checked_add(landed_fee)
                .filter(|n| *n <= MAX_MINOR)
                .ok_or(ValidationError("purchaseSourceAmount"))?;
            push(PurchaseRole::Inventory, total, true);
            push(PurchaseRole::GoodsReceivedNotInvoiced, goods, false);
            push(PurchaseRole::LandedFeeAccrual, landed_fee, false);
        }
        PurchaseEffect::Bill => {
            push(PurchaseRole::GoodsReceivedNotInvoiced, goods, true);
            push(PurchaseRole::Payable, goods, false);
        }
        PurchaseEffect::Void => {
            push(PurchaseRole::Payable, -goods, true);
            push(PurchaseRole::GoodsReceivedNotInvoiced, -goods, false);
        }
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn net(rows: &[PurchaseLeg], role: PurchaseRole) -> i128 {
        rows.iter()
            .filter(|r| r.role == role)
            .map(|r| {
                r.debit_minor.parse::<i128>().unwrap() - r.credit_minor.parse::<i128>().unwrap()
            })
            .sum()
    }
    #[test]
    fn receipt_fee_and_matched_claim_clear_grni_without_inventing_supplier_fees() {
        // Synthetic goods 1,000,000 whole IDR plus 10,000 unclaimed landed fee.
        let receipt = legs(PurchaseEffect::Receipt, 100_000_000, 1_000_000).unwrap();
        let bill = legs(PurchaseEffect::Bill, 100_000_000, 0).unwrap();
        assert_eq!(net(&receipt, PurchaseRole::Inventory), 101_000_000);
        assert_eq!(net(&receipt, PurchaseRole::LandedFeeAccrual), -1_000_000);
        assert_eq!(net(&receipt, PurchaseRole::Payable), 0);
        assert_eq!(
            net(&receipt, PurchaseRole::GoodsReceivedNotInvoiced)
                + net(&bill, PurchaseRole::GoodsReceivedNotInvoiced),
            0
        );
        assert_eq!(net(&bill, PurchaseRole::Payable), -100_000_000);
    }
    #[test]
    fn void_offsets_the_claim_without_reversing_physical_receipt_cost() {
        let receipt = legs(PurchaseEffect::Receipt, 10_000, 100).unwrap();
        let mut claims = legs(PurchaseEffect::Bill, 10_000, 0).unwrap();
        claims.extend(legs(PurchaseEffect::Void, -10_000, 0).unwrap());
        assert_eq!(net(&claims, PurchaseRole::Payable), 0);
        assert_eq!(net(&claims, PurchaseRole::GoodsReceivedNotInvoiced), 0);
        assert_eq!(net(&claims, PurchaseRole::Inventory), 0);
        assert_eq!(net(&receipt, PurchaseRole::Inventory), 10_100);
    }
    #[test]
    fn all_component_combinations_balance_and_do_not_create_zero_lines() {
        for goods in 0..=50 {
            for fee in 0..=50 {
                let rows = legs(PurchaseEffect::Receipt, goods, fee).unwrap();
                let debit: i128 = rows
                    .iter()
                    .map(|r| r.debit_minor.parse::<i128>().unwrap())
                    .sum();
                let credit: i128 = rows
                    .iter()
                    .map(|r| r.credit_minor.parse::<i128>().unwrap())
                    .sum();
                assert_eq!(debit, credit);
                assert_eq!(debit, i128::from(goods + fee));
                for r in rows {
                    assert_ne!(r.debit_minor == "0", r.credit_minor == "0");
                }
            }
        }
        for effect in [
            PurchaseEffect::Receipt,
            PurchaseEffect::Bill,
            PurchaseEffect::Void,
        ] {
            assert!(legs(effect, 0, 0).unwrap().is_empty());
        }
    }
    #[test]
    fn signed_component_and_combined_book_bounds_refuse_invalid_sources() {
        for (effect, goods, fee) in [
            (PurchaseEffect::Receipt, -1, 0),
            (PurchaseEffect::Receipt, 1, -1),
            (PurchaseEffect::Bill, -1, 0),
            (PurchaseEffect::Bill, 1, 1),
            (PurchaseEffect::Void, 1, 0),
            (PurchaseEffect::Void, -1, 1),
            (PurchaseEffect::Receipt, MAX_MINOR, 1),
            (PurchaseEffect::Receipt, i64::MAX, 0),
            (PurchaseEffect::Void, i64::MIN, 0),
        ] {
            assert!(legs(effect, goods, fee).is_err());
        }
        assert_eq!(
            net(
                &legs(PurchaseEffect::Receipt, MAX_MINOR, 0).unwrap(),
                PurchaseRole::Inventory
            ),
            i128::from(MAX_MINOR)
        );
        assert_eq!(
            net(
                &legs(PurchaseEffect::Void, -MAX_MINOR, 0).unwrap(),
                PurchaseRole::Payable
            ),
            i128::from(MAX_MINOR)
        );
    }
}
