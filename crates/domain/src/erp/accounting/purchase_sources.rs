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
/// The account a leg of a purchase source posts to: an account of the
/// purchase posting policy, or the input tax control account captured with a
/// taxed supplier bill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurchaseLegAccount {
    Policy(PurchaseRole),
    TaxControl,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxedPurchaseLeg {
    pub account: PurchaseLegAccount,
    pub debit_minor: String,
    pub credit_minor: String,
}
/// The legs of a purchase source that carries `tax` beside its goods (both in
/// exact book minor units, the tax with the sign of the goods and no larger).
/// Without tax these are the legs of [`legs`]. A taxed bill debits goods
/// received not invoiced (the net goods) and the tax control account (the
/// tax) and credits the payable (goods plus tax); its void is the reverse. A
/// receipt carries no tax.
pub fn taxed_legs(
    effect: PurchaseEffect,
    goods: i64,
    landed_fee: i64,
    tax: i64,
) -> Result<Vec<TaxedPurchaseLeg>, ValidationError> {
    let plain = legs(effect, goods, landed_fee)?;
    let leg = |account, value: i64, debit: bool| TaxedPurchaseLeg {
        account,
        debit_minor: if debit { value } else { 0 }.to_string(),
        credit_minor: if debit { 0 } else { value }.to_string(),
    };
    if tax == 0 {
        return Ok(plain
            .into_iter()
            .map(|leg| TaxedPurchaseLeg {
                account: PurchaseLegAccount::Policy(leg.role),
                debit_minor: leg.debit_minor,
                credit_minor: leg.credit_minor,
            })
            .collect());
    }
    let within = match effect {
        PurchaseEffect::Bill => (1..=goods).contains(&tax),
        PurchaseEffect::Void => (goods..0).contains(&tax),
        PurchaseEffect::Receipt => false,
    };
    let total = goods
        .checked_add(tax)
        .filter(|n| (-MAX_MINOR..=MAX_MINOR).contains(n));
    let (true, Some(total)) = (within, total) else {
        return Err(ValidationError("purchaseSourceAmount"));
    };
    let grni = PurchaseLegAccount::Policy(PurchaseRole::GoodsReceivedNotInvoiced);
    let payable = PurchaseLegAccount::Policy(PurchaseRole::Payable);
    Ok(if effect == PurchaseEffect::Bill {
        vec![
            leg(grni, goods, true),
            leg(PurchaseLegAccount::TaxControl, tax, true),
            leg(payable, total, false),
        ]
    } else {
        vec![
            leg(payable, -total, true),
            leg(grni, -goods, false),
            leg(PurchaseLegAccount::TaxControl, -tax, false),
        ]
    })
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
    #[test]
    fn a_taxed_bill_posts_goods_and_input_tax_against_a_gross_payable_and_its_void_reverses_all_three(
    ) {
        use PurchaseLegAccount::{Policy, TaxControl};
        let line = |leg: &TaxedPurchaseLeg| {
            (
                leg.account,
                leg.debit_minor.parse::<i64>().unwrap(),
                leg.credit_minor.parse::<i64>().unwrap(),
            )
        };
        let grni = Policy(PurchaseRole::GoodsReceivedNotInvoiced);
        let payable = Policy(PurchaseRole::Payable);
        // Goods 140,000 whole IDR with 15,400 of tax, as book minor units.
        let bill = taxed_legs(PurchaseEffect::Bill, 14_000_000, 0, 1_540_000).unwrap();
        assert_eq!(
            bill.iter().map(line).collect::<Vec<_>>(),
            vec![
                (grni, 14_000_000, 0),
                (TaxControl, 1_540_000, 0),
                (payable, 0, 15_540_000)
            ]
        );
        let void = taxed_legs(PurchaseEffect::Void, -14_000_000, 0, -1_540_000).unwrap();
        assert_eq!(
            void.iter().map(line).collect::<Vec<_>>(),
            vec![
                (payable, 15_540_000, 0),
                (grni, 0, 14_000_000),
                (TaxControl, 0, 1_540_000)
            ]
        );
        // Bill and void together leave nothing on any account.
        for account in [grni, payable, TaxControl] {
            let net: i64 = bill
                .iter()
                .chain(&void)
                .map(line)
                .filter(|(a, _, _)| *a == account)
                .map(|(_, debit, credit)| debit - credit)
                .sum();
            assert_eq!(net, 0, "{account:?}");
        }
        // Without tax the legs are the two-leg shape of `legs`, in its order.
        for (effect, goods, fee) in [
            (PurchaseEffect::Bill, 1_400_000, 0),
            (PurchaseEffect::Void, -1_400_000, 0),
            (PurchaseEffect::Receipt, 1_400_000, 10_100),
        ] {
            let plain = legs(effect, goods, fee).unwrap();
            let taxed = taxed_legs(effect, goods, fee, 0).unwrap();
            assert_eq!(taxed.len(), plain.len());
            for (taxed, plain) in taxed.iter().zip(&plain) {
                assert_eq!(taxed.account, Policy(plain.role));
                assert_eq!(taxed.debit_minor, plain.debit_minor);
                assert_eq!(taxed.credit_minor, plain.credit_minor);
            }
        }
        // All of the goods as tax is the bound; every taxed journal balances.
        for (goods, tax) in [
            (100, 100),
            (100, 1),
            (1_005, 111),
            (MAX_MINOR / 2, MAX_MINOR / 2),
        ] {
            let rows = taxed_legs(PurchaseEffect::Bill, goods, 0, tax).unwrap();
            let debit: i64 = rows.iter().map(|r| line(r).1).sum();
            let credit: i64 = rows.iter().map(|r| line(r).2).sum();
            assert_eq!((debit, credit), (goods + tax, goods + tax));
            assert!(rows.iter().all(|r| (line(r).1 == 0) != (line(r).2 == 0)));
        }
        for (effect, goods, fee, tax) in [
            (PurchaseEffect::Bill, 100, 0, 101),
            (PurchaseEffect::Bill, 100, 0, -1),
            (PurchaseEffect::Void, -100, 0, 1),
            (PurchaseEffect::Void, -100, 0, -101),
            (PurchaseEffect::Receipt, 100, 0, 1),
            (PurchaseEffect::Bill, 0, 0, 1),
            (PurchaseEffect::Bill, MAX_MINOR, 0, 1),
        ] {
            assert!(
                taxed_legs(effect, goods, fee, tax).is_err(),
                "{effect:?} {goods} {tax}"
            );
        }
    }
}
