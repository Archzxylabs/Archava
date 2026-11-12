//! Reviewed source units and account roles for the native IDR invoice bridge.
use super::{revision, AccountClass, CashRole, ValidationError, MAX_MINOR};
use crate::erp::valid_uuid;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const INVOICE_SOURCE_UNIT: &str = "whole_idr";
pub const CURRENCY_CATALOGUE: &str = "SIX-2026-09-17";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountRef {
    pub account_id: String,
    pub expected_account_revision: i32,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum InvoiceRole {
    Receivable,
    Revenue,
    PaymentClearing,
}
impl InvoiceRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Receivable => "receivable",
            Self::Revenue => "revenue",
            Self::PaymentClearing => "payment_clearing",
        }
    }
    pub fn classification(self) -> (AccountClass, CashRole) {
        match self {
            Self::Receivable => (AccountClass::Asset, CashRole::Operating),
            Self::Revenue => (AccountClass::Revenue, CashRole::Operating),
            // Verified provider collection is not proof of a bank settlement.
            Self::PaymentClearing => (AccountClass::Asset, CashRole::None),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvoiceMapping {
    pub role: InvoiceRole,
    pub account_id: String,
    pub expected_account_revision: i32,
}
pub fn valid_refs(refs: &[AccountRef], count: usize) -> bool {
    refs.len() == count
        && refs
            .iter()
            .all(|r| valid_uuid(&r.account_id) && revision(r.expected_account_revision))
        && refs
            .iter()
            .map(|r| &r.account_id)
            .collect::<BTreeSet<_>>()
            .len()
            == count
}
pub fn valid_mappings(mappings: &[InvoiceMapping]) -> bool {
    mappings.len() == 3
        && mappings
            .iter()
            .all(|r| valid_uuid(&r.account_id) && revision(r.expected_account_revision))
        && mappings
            .iter()
            .map(|r| r.role)
            .collect::<BTreeSet<_>>()
            .len()
            == 3
        && mappings
            .iter()
            .map(|r| &r.account_id)
            .collect::<BTreeSet<_>>()
            .len()
            == 3
}
/// This bridge explicitly accepts whole Rupiah and performs no FX or rounding.
pub fn invoice_minor(whole_idr: i64) -> Result<i64, ValidationError> {
    whole_idr
        .checked_mul(100)
        .filter(|n| (-MAX_MINOR..=MAX_MINOR).contains(n))
        .ok_or(ValidationError("sourceAmount"))
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoiceLeg {
    pub role: InvoiceRole,
    pub debit_minor: String,
    pub credit_minor: String,
}
/// Same original-source rules for persistence and human review. Zero-effect
/// sources require no journal; amounts are already exact book minor units.
pub fn legs(effect: &str, amount: i64) -> Result<Vec<InvoiceLeg>, ValidationError> {
    if !matches!(
        effect,
        "issue" | "correction" | "void" | "collection" | "refund"
    ) || !(-MAX_MINOR..=MAX_MINOR).contains(&amount)
    {
        return Err(ValidationError("invoiceSource"));
    }
    if amount == 0 {
        return Ok(Vec::new());
    }
    let collection = matches!(effect, "collection" | "refund");
    let (mut debit, mut credit) = if collection {
        (InvoiceRole::PaymentClearing, InvoiceRole::Receivable)
    } else {
        (InvoiceRole::Receivable, InvoiceRole::Revenue)
    };
    if amount < 0 {
        std::mem::swap(&mut debit, &mut credit);
    }
    Ok(vec![
        InvoiceLeg {
            role: debit,
            debit_minor: amount.abs().to_string(),
            credit_minor: "0".into(),
        },
        InvoiceLeg {
            role: credit,
            debit_minor: "0".into(),
            credit_minor: amount.abs().to_string(),
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captured_invoice_effects_reverse_original_roles_and_keep_clearing_distinct_from_cash() {
        let issue = legs("issue", 705100).unwrap();
        assert_eq!(issue[0].role, InvoiceRole::Receivable);
        assert_eq!(issue[1].role, InvoiceRole::Revenue);
        assert_eq!(issue[0].debit_minor, "705100");
        let credit = legs("correction", -705100).unwrap();
        assert_eq!(credit[0].role, InvoiceRole::Revenue);
        assert_eq!(credit[1].role, InvoiceRole::Receivable);
        let receipt = legs("collection", 705100).unwrap();
        assert_eq!(receipt[0].role, InvoiceRole::PaymentClearing);
        assert_eq!(receipt[1].role, InvoiceRole::Receivable);
        let refund = legs("refund", -705100).unwrap();
        assert_eq!(refund[0].role, InvoiceRole::Receivable);
        assert_eq!(refund[1].role, InvoiceRole::PaymentClearing);
        assert!(legs("void", 0).unwrap().is_empty());
        assert!(legs("bank_settlement", 1).is_err());
        assert!(legs("issue", MAX_MINOR + 1).is_err());
    }
    #[test]
    fn invoice_source_units_are_signed_exact_and_bounded() {
        assert_eq!(invoice_minor(15000).unwrap(), 1500000);
        assert_eq!(invoice_minor(-125).unwrap(), -12500);
        assert_eq!(invoice_minor(0).unwrap(), 0);
        assert_eq!(invoice_minor(MAX_MINOR / 100).unwrap(), MAX_MINOR);
        for n in [
            MAX_MINOR / 100 + 1,
            -MAX_MINOR / 100 - 1,
            i64::MAX,
            i64::MIN,
        ] {
            assert!(invoice_minor(n).is_err());
        }
        assert_eq!(
            InvoiceRole::PaymentClearing.classification(),
            (AccountClass::Asset, CashRole::None)
        );
    }
}
