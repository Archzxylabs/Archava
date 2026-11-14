//! Exact supplier-payment arithmetic; persistence derives authority and all source balances.
//! These rules neither execute a transfer nor infer that a payment/refund has posted.
use super::{bank, minor, ValidationError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Capacity {
    pub total_minor: String,
    pub posted_minor: String,
    pub reserved_minor: String,
    pub outstanding_minor: String,
    pub available_minor: String,
}

/// The database supplies actual posted amounts and live draft reservations.
/// A draft refund must not reduce `posted`; only its committed posting can do so.
/// For refund capacity, pass the original paid amount and actual posted/reserved refunds.
pub fn capacity(total: &str, posted: &str, reserved: &str) -> Result<Capacity, ValidationError> {
    let total = minor(total)?;
    let posted = minor(posted)?;
    let reserved = minor(reserved)?;
    if posted > total || reserved > total - posted {
        return Err(ValidationError("payableCapacity"));
    }
    Ok(Capacity {
        total_minor: total.to_string(),
        posted_minor: posted.to_string(),
        reserved_minor: reserved.to_string(),
        outstanding_minor: (total - posted).to_string(),
        available_minor: (total - posted - reserved).to_string(),
    })
}

pub fn claim(capacity: &Capacity, amount: &str) -> Result<(), ValidationError> {
    let amount = minor(amount)?;
    if amount == 0 || amount > minor(&capacity.available_minor)? {
        return Err(ValidationError("payableAllocation"));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Payment,
    Return,
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Payable,
    Cash,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Leg {
    pub role: Role,
    pub debit_minor: String,
    pub credit_minor: String,
}

/// Source-specific postings, with no fee, FX or balancing amount inferred.
pub fn legs(effect: Effect, amount: &str) -> Result<[Leg; 2], ValidationError> {
    let amount = minor(amount)?;
    if amount == 0 {
        return Err(ValidationError("payableAmount"));
    }
    let (debit, credit) = if effect == Effect::Payment {
        (amount.to_string(), "0".to_string())
    } else {
        ("0".to_string(), amount.to_string())
    };
    Ok([
        Leg {
            role: Role::Payable,
            debit_minor: debit.clone(),
            credit_minor: credit.clone(),
        },
        Leg {
            role: Role::Cash,
            debit_minor: credit,
            credit_minor: debit,
        },
    ])
}

/// The bank statement direction must agree with the actual payment or returned payment.
/// The caller derives the row remainder after all active reservations under the book lock.
pub fn bank_claim(
    effect: Effect,
    row_remaining: &str,
    amount: &str,
) -> Result<(), ValidationError> {
    let value = minor(amount)?;
    if value == 0 {
        return Err(ValidationError("payableAmount"));
    }
    let cash = if effect == Effect::Payment {
        format!("-{value}")
    } else {
        value.to_string()
    };
    bank::allocate(row_remaining, &cash, amount)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn payment_commands_use_observed_source_refs_and_refuse_claimed_actor_balances_or_dates() {
        let raw = json!({"requestId":"A0000000-0000-4000-8000-000000000001","command":{
            "type":"payable_payment.draft","sourceId":"A0000000-0000-4000-8000-000000000002",
            "expectedDocumentRevision":2,"bankRowId":"A0000000-0000-4000-8000-000000000003","code":"AP-PAYMENT",
            "payableAccount":{"accountId":"A0000000-0000-4000-8000-000000000004","expectedAccountRevision":2},
            "cashAccount":{"accountId":"A0000000-0000-4000-8000-000000000005","expectedAccountRevision":1},
            "amountMinor":"10000","reason":"Review actual synthetic supplier withdrawal"}});
        let decoded = super::super::parse(&raw).unwrap();
        assert_eq!(decoded.command.operation(), "payable_payment.draft");
        assert_eq!(decoded.command.permission(), "payables.write");
        assert_eq!(
            serde_json::to_value(decoded).unwrap()["command"]["sourceId"],
            "a0000000-0000-4000-8000-000000000002"
        );
        for (field, value) in [
            ("actorId", json!("owner")),
            ("paidMinor", json!("0")),
            ("availableMinor", json!("999999")),
            ("date", json!("2026-11-07")),
        ] {
            let mut forged = raw.clone();
            forged["command"][field] = value;
            assert!(super::super::parse(&forged).is_err());
        }
        for invalid in ["0", "-1", "01", "1.0"] {
            let mut forged = raw.clone();
            forged["command"]["amountMinor"] = json!(invalid);
            assert!(super::super::parse(&forged).is_err());
        }
        let mut reused_account = raw;
        reused_account["command"]["cashAccount"] =
            reused_account["command"]["payableAccount"].clone();
        assert!(super::super::parse(&reused_account).is_err());
    }
    #[test]
    fn partial_payment_and_live_reservations_cannot_spend_the_same_bill_capacity() {
        let remaining = capacity("10000", "2500", "4000").unwrap();
        assert_eq!(remaining.outstanding_minor, "7500");
        assert_eq!(remaining.available_minor, "3500");
        assert!(claim(&remaining, "3500").is_ok());
        assert!(claim(&remaining, "3501").is_err());
        assert!(capacity("10000", "2500", "7501").is_err());
        assert!(capacity("10000", "10001", "0").is_err());
    }
    #[test]
    fn a_draft_refund_does_not_make_an_already_reserved_bill_payable_again() {
        let before_post = capacity("10000", "5000", "5000").unwrap();
        assert_eq!(before_post.available_minor, "0");
        assert!(claim(&before_post, "1").is_err());
        let after_actual_post = capacity("10000", "3000", "5000").unwrap();
        assert_eq!(after_actual_post.available_minor, "2000");
    }
    #[test]
    fn returns_are_bounded_by_the_original_payment_and_other_return_reservations() {
        let remaining = capacity("7000", "2000", "3000").unwrap();
        assert_eq!(remaining.available_minor, "2000");
        assert!(claim(&remaining, "2001").is_err());
        assert!(capacity("7000", "2000", "5001").is_err());
    }
    #[test]
    fn payment_and_return_legs_keep_the_same_amount_and_explicit_opposite_cash_direction() {
        let payment = legs(Effect::Payment, "15000").unwrap();
        let returned = legs(Effect::Return, "15000").unwrap();
        assert_eq!(
            (
                payment[0].role,
                payment[0].debit_minor.as_str(),
                payment[1].credit_minor.as_str()
            ),
            (Role::Payable, "15000", "15000")
        );
        for (paid, returned) in payment.iter().zip(returned.iter()) {
            assert_eq!(paid.role, returned.role);
            assert_eq!(paid.debit_minor, returned.credit_minor);
            assert_eq!(paid.credit_minor, returned.debit_minor);
        }
        assert!(bank_claim(Effect::Payment, "-20000", "15000").is_ok());
        assert!(bank_claim(Effect::Payment, "20000", "15000").is_err());
        assert!(bank_claim(Effect::Return, "20000", "15000").is_ok());
        assert!(bank_claim(Effect::Return, "-20000", "15000").is_err());
        assert!(bank_claim(Effect::Payment, "-14999", "15000").is_err());
    }
    #[test]
    fn large_exact_capacities_and_ambiguous_amounts_never_round_or_saturate() {
        let remaining = capacity("9000000000000000", "8999999999999998", "1").unwrap();
        assert_eq!(remaining.outstanding_minor, "2");
        assert_eq!(remaining.available_minor, "1");
        for bad in ["-1", "+1", "01", "1.0", "1e3", " 1", "9000000000000001"] {
            assert!(capacity(bad, "0", "0").is_err());
            assert!(legs(Effect::Payment, bad).is_err());
        }
        assert!(legs(Effect::Payment, "0").is_err());
    }
}
