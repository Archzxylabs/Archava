//! Reviewed source units and account roles for the native invoice bridge.
//!
//! A posting policy names the unit its source amounts are in. The database
//! decides which unit fits a book: `whole_idr` only on an IDR book, `minor`
//! only on a book in any other currency.
use super::{revision, AccountClass, CashRole, ValidationError, MAX_MINOR};
use crate::erp::valid_uuid;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Whole Rupiah; the ledger holds IDR with two decimals, so the factor is 100.
pub const INVOICE_SOURCE_UNIT: &str = "whole_idr";
/// The ISO minor unit of a non-IDR book currency; posted unchanged.
pub const MINOR_SOURCE_UNIT: &str = "minor";

/// The exact factor from a source amount to ledger minor units, for a unit a
/// policy command may name.
pub fn source_unit_factor(unit: &str) -> Option<i64> {
    match unit {
        INVOICE_SOURCE_UNIT => Some(100),
        MINOR_SOURCE_UNIT => Some(1),
        _ => None,
    }
}
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
/// A source amount in ledger minor units under the named source unit: whole
/// Rupiah times 100, or an ISO minor amount unchanged. No FX and no rounding.
pub fn source_minor(unit: &str, amount: i64) -> Result<i64, ValidationError> {
    source_unit_factor(unit)
        .and_then(|factor| amount.checked_mul(factor))
        .filter(|n| (-MAX_MINOR..=MAX_MINOR).contains(n))
        .ok_or(ValidationError("sourceAmount"))
}
/// The `whole_idr` case of [`source_minor`].
pub fn invoice_minor(whole_idr: i64) -> Result<i64, ValidationError> {
    source_minor(INVOICE_SOURCE_UNIT, whole_idr)
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

/// The account a leg of an invoice source posts to: an account of the posting
/// policy, or the tax control account captured with a taxed source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvoiceLegAccount {
    Policy(InvoiceRole),
    TaxControl,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxedInvoiceLeg {
    pub account: InvoiceLegAccount,
    pub debit_minor: String,
    pub credit_minor: String,
}
/// The legs of an invoice source whose captured amount contains `tax` (both
/// in exact book minor units, the tax with the sign of the amount and no
/// larger). Without tax these are the two legs of [`legs`]. With tax an
/// invoice, credit or void posts receivable (the amount) against revenue (the
/// amount less the tax) and the tax control account (the tax); a negative
/// amount reverses the sides. A journal line cannot be zero, so a net of zero
/// leaves the revenue leg out. A payment carries no tax.
pub fn taxed_legs(
    effect: &str,
    amount: i64,
    tax: i64,
) -> Result<Vec<TaxedInvoiceLeg>, ValidationError> {
    if tax == 0 {
        return Ok(legs(effect, amount)?
            .into_iter()
            .map(|leg| TaxedInvoiceLeg {
                account: InvoiceLegAccount::Policy(leg.role),
                debit_minor: leg.debit_minor,
                credit_minor: leg.credit_minor,
            })
            .collect());
    }
    let within = if amount > 0 {
        (1..=amount).contains(&tax)
    } else {
        (amount..0).contains(&tax)
    };
    if !matches!(effect, "issue" | "correction" | "void")
        || !(-MAX_MINOR..=MAX_MINOR).contains(&amount)
        || !within
    {
        return Err(ValidationError("invoiceSource"));
    }
    let forward = amount > 0;
    let leg = |account, value: i64, debit: bool| TaxedInvoiceLeg {
        account,
        debit_minor: if debit { value.abs() } else { 0 }.to_string(),
        credit_minor: if debit { 0 } else { value.abs() }.to_string(),
    };
    let receivable = leg(
        InvoiceLegAccount::Policy(InvoiceRole::Receivable),
        amount,
        forward,
    );
    let mut others = Vec::new();
    if amount != tax {
        others.push(leg(
            InvoiceLegAccount::Policy(InvoiceRole::Revenue),
            amount - tax,
            !forward,
        ));
    }
    others.push(leg(InvoiceLegAccount::TaxControl, tax, !forward));
    // Debit legs first, as the two-leg journal has them.
    Ok(if forward {
        std::iter::once(receivable).chain(others).collect()
    } else {
        others
            .into_iter()
            .chain(std::iter::once(receivable))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn taxed_invoice_sources_post_receivable_against_revenue_and_the_tax_control_account() {
        use InvoiceLegAccount::{Policy, TaxControl};
        let line = |leg: &TaxedInvoiceLeg| {
            (
                leg.account,
                leg.debit_minor.clone(),
                leg.credit_minor.clone(),
            )
        };
        let own = |v: &str| v.to_owned();
        // Rp33.300 gross with Rp3.300 of tax, in book minor units.
        let issue = taxed_legs("issue", 3_330_000, 330_000).unwrap();
        assert_eq!(
            issue.iter().map(line).collect::<Vec<_>>(),
            vec![
                (Policy(InvoiceRole::Receivable), own("3330000"), own("0")),
                (Policy(InvoiceRole::Revenue), own("0"), own("3000000")),
                (TaxControl, own("0"), own("330000")),
            ]
        );
        // A credit and a void reverse the three sides.
        for effect in ["correction", "void"] {
            let credit = taxed_legs(effect, -1_110_000, -110_000).unwrap();
            assert_eq!(
                credit.iter().map(line).collect::<Vec<_>>(),
                vec![
                    (Policy(InvoiceRole::Revenue), own("1000000"), own("0")),
                    (TaxControl, own("110000"), own("0")),
                    (Policy(InvoiceRole::Receivable), own("0"), own("1110000")),
                ],
                "{effect}"
            );
        }
        // Without tax the legs are the two legs of `legs`, in its order.
        for (effect, amount) in [
            ("issue", 705_100),
            ("correction", -705_100),
            ("collection", 705_100),
            ("refund", -705_100),
        ] {
            let plain = legs(effect, amount).unwrap();
            let taxed = taxed_legs(effect, amount, 0).unwrap();
            assert_eq!(taxed.len(), 2);
            for (a, b) in plain.iter().zip(&taxed) {
                assert_eq!(
                    (Policy(a.role), &a.debit_minor, &a.credit_minor),
                    (b.account, &b.debit_minor, &b.credit_minor)
                );
            }
        }
        assert!(taxed_legs("void", 0, 0).unwrap().is_empty());
        // A correction of one unit that is all tax has no revenue leg.
        let unit = taxed_legs("correction", -100, -100).unwrap();
        assert_eq!(
            unit.iter().map(line).collect::<Vec<_>>(),
            vec![
                (TaxControl, own("100"), own("0")),
                (Policy(InvoiceRole::Receivable), own("0"), own("100")),
            ]
        );
        // Debits equal credits in every taxed shape.
        for (amount, tax) in [(3_330_000, 330_000), (-999, -99), (1, 1), (-1, -1)] {
            let sum = |pick: fn(&TaxedInvoiceLeg) -> &String| -> i64 {
                taxed_legs("issue", amount, tax)
                    .unwrap()
                    .iter()
                    .map(|l| pick(l).parse::<i64>().unwrap())
                    .sum()
            };
            assert_eq!(sum(|l| &l.debit_minor), sum(|l| &l.credit_minor));
            assert_eq!(sum(|l| &l.debit_minor), i64::abs(amount));
        }
        // Tax has the sign of the amount, is no larger, and a payment has none.
        for (effect, amount, tax) in [
            ("issue", 100, 101),
            ("issue", 100, -1),
            ("correction", -100, 1),
            ("correction", -100, -101),
            ("issue", 0, 1),
            ("collection", 100, 10),
            ("refund", -100, -10),
            ("bank_settlement", 100, 10),
            ("issue", MAX_MINOR + 1, 1),
        ] {
            assert!(
                taxed_legs(effect, amount, tax).is_err(),
                "{effect} {amount} {tax}"
            );
        }
    }
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
        // The `minor` unit posts the amount unchanged, whatever the exponent.
        assert_eq!(source_minor(MINOR_SOURCE_UNIT, 3998).unwrap(), 3998);
        assert_eq!(source_minor(MINOR_SOURCE_UNIT, -1500).unwrap(), -1500);
        assert_eq!(
            source_minor(MINOR_SOURCE_UNIT, MAX_MINOR).unwrap(),
            MAX_MINOR
        );
        assert!(source_minor(MINOR_SOURCE_UNIT, MAX_MINOR + 1).is_err());
        assert!(source_minor(MINOR_SOURCE_UNIT, -MAX_MINOR - 1).is_err());
        assert!(source_minor("book_minor", 1).is_err());
        assert_eq!(source_unit_factor("iso_minor"), None);
        assert_eq!(
            InvoiceRole::PaymentClearing.classification(),
            (AccountClass::Asset, CashRole::None)
        );
    }
}
