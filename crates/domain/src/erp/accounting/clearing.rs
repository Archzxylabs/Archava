//! Exact gateway-clearing to bank arithmetic. Source sign and authority come
//! from immutable verified invoice events, never a user-supplied payment status.
use super::{bank, minor, ValidationError, MAX_MINOR};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Collection,
    Refund,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Clearing,
    Cash,
    Fee,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Leg {
    pub role: Role,
    pub debit_minor: String,
    pub credit_minor: String,
}

/// Gross applies to the original verified collection/refund source. Fees are
/// explicit reviewed expenses, not an inferred balancing difference or revenue.
/// A collection deposits gross minus fee; a refund withdraws gross plus fee.
pub fn legs(effect: Effect, gross: &str, fee: &str) -> Result<Vec<Leg>, ValidationError> {
    let gross = minor(gross)?;
    let fee = minor(fee)?;
    if gross == 0 {
        return Err(ValidationError("clearingGross"));
    }
    let cash = match effect {
        Effect::Collection => gross.checked_sub(fee).filter(|n| *n > 0),
        Effect::Refund => gross.checked_add(fee).filter(|n| *n <= MAX_MINOR),
    }
    .ok_or(ValidationError("clearingCash"))?;
    let collection = effect == Effect::Collection;
    let mut lines = vec![
        Leg {
            role: Role::Clearing,
            debit_minor: if collection {
                "0".into()
            } else {
                gross.to_string()
            },
            credit_minor: if collection {
                gross.to_string()
            } else {
                "0".into()
            },
        },
        Leg {
            role: Role::Cash,
            debit_minor: if collection {
                cash.to_string()
            } else {
                "0".into()
            },
            credit_minor: if collection {
                "0".into()
            } else {
                cash.to_string()
            },
        },
    ];
    if fee > 0 {
        lines.push(Leg {
            role: Role::Fee,
            debit_minor: fee.to_string(),
            credit_minor: "0".into(),
        })
    }
    Ok(lines)
}
pub fn claim(
    source_total: &str,
    posted: &str,
    reserved: &str,
    gross: &str,
) -> Result<(), ValidationError> {
    let total = minor(source_total)?;
    let posted = minor(posted)?;
    let reserved = minor(reserved)?;
    let gross = minor(gross)?;
    if gross == 0
        || posted > total
        || reserved > total - posted
        || gross > total - posted - reserved
    {
        return Err(ValidationError("clearingCapacity"));
    }
    Ok(())
}
pub fn bank_claim(
    effect: Effect,
    row_remaining: &str,
    gross: &str,
    fee: &str,
) -> Result<(), ValidationError> {
    let lines = legs(effect, gross, fee)?;
    let line = &lines[1];
    let amount = if effect == Effect::Collection {
        &line.debit_minor
    } else {
        &line.credit_minor
    };
    let signed = if effect == Effect::Collection {
        amount.clone()
    } else {
        format!("-{amount}")
    };
    bank::allocate(row_remaining, &signed, amount)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settlement_command_requires_explicit_fee_account_and_never_accepts_claimed_source_status_or_date(
    ) {
        use serde_json::{json, Value};
        let input = json!({"requestId":"A0000000-0000-4000-8000-000000000001","command":{"type":"clearing_settlement.draft","sourceId":"A0000000-0000-4000-8000-000000000002","bankRowId":"A0000000-0000-4000-8000-000000000003","code":"ACTUAL-SETTLEMENT","clearingAccount":{"accountId":"A0000000-0000-4000-8000-000000000004","expectedAccountRevision":1},"cashAccount":{"accountId":"A0000000-0000-4000-8000-000000000005","expectedAccountRevision":2},"grossMinor":"60000000","feeMinor":"0","reason":"Review exact actual gateway-to-bank evidence"}});
        let parsed = super::super::parse(&input).unwrap();
        assert_eq!(parsed.command.permission(), "clearing.write");
        assert_eq!(
            serde_json::to_value(parsed).unwrap()["command"]["sourceId"],
            "a0000000-0000-4000-8000-000000000002"
        );
        for (field, value) in [
            ("date", json!("2026-11-11")),
            ("providerStatus", json!("settled")),
            ("actorId", json!("owner")),
            ("availableMinor", json!("60000000")),
        ] {
            let mut forged = input.clone();
            forged["command"][field] = value;
            assert!(super::super::parse(&forged).is_err());
        }
        let mut fee = input.clone();
        fee["command"]["feeMinor"] = json!("100000");
        assert!(super::super::parse(&fee).is_err());
        fee["command"]["feeAccount"] =
            json!({"accountId":"A0000000-0000-4000-8000-000000000006","expectedAccountRevision":1});
        assert!(super::super::parse(&fee).is_ok());
        fee["command"]["feeMinor"] = json!("0");
        assert!(super::super::parse(&fee).is_err());
        fee["command"]["feeMinor"] = json!("100000");
        fee["command"]["feeAccount"] = fee["command"]["cashAccount"].clone();
        assert!(super::super::parse(&fee).is_err());
        fee["command"]["feeAccount"] = Value::Null;
        assert!(super::super::parse(&fee).is_err());
    }
    #[test]
    fn collection_transfers_clearing_to_actual_net_bank_without_reposting_ar_or_revenue() {
        let rows = legs(Effect::Collection, "60000000", "100000").unwrap();
        assert_eq!(
            rows,
            vec![
                Leg {
                    role: Role::Clearing,
                    debit_minor: "0".into(),
                    credit_minor: "60000000".into()
                },
                Leg {
                    role: Role::Cash,
                    debit_minor: "59900000".into(),
                    credit_minor: "0".into()
                },
                Leg {
                    role: Role::Fee,
                    debit_minor: "100000".into(),
                    credit_minor: "0".into()
                }
            ]
        );
        assert!(bank_claim(Effect::Collection, "59900000", "60000000", "100000").is_ok());
        assert!(bank_claim(Effect::Collection, "59899999", "60000000", "100000").is_err());
        assert!(bank_claim(Effect::Collection, "-59900000", "60000000", "100000").is_err());
    }
    #[test]
    fn refund_clears_only_the_posted_provider_refund_and_explicit_additional_bank_fee() {
        let rows = legs(Effect::Refund, "15000000", "50000").unwrap();
        assert_eq!(rows[0].debit_minor, "15000000");
        assert_eq!(rows[1].credit_minor, "15050000");
        assert_eq!(rows[2].debit_minor, "50000");
        assert!(bank_claim(Effect::Refund, "-15050000", "15000000", "50000").is_ok());
        assert!(bank_claim(Effect::Refund, "-15000000", "15000000", "50000").is_err());
    }
    #[test]
    fn partial_settlements_reserve_the_same_verified_source_and_never_infer_a_netting_event() {
        assert!(claim("60000000", "30000000", "20000000", "10000000").is_ok());
        assert!(claim("60000000", "30000000", "20000000", "10000001").is_err());
        assert!(claim("60000000", "60000001", "0", "1").is_err());
        assert!(claim("60000000", "1", "60000000", "1").is_err());
        assert_eq!(legs(Effect::Collection, "60000000", "0").unwrap().len(), 2);
    }
    #[test]
    fn the_unchanged_no_fee_b01_money_oracle_has_final_bank450000_whole_idr() {
        let collection = legs(Effect::Collection, "60000000", "0").unwrap();
        let refund = legs(Effect::Refund, "15000000", "0").unwrap();
        let opening = 100000000_i128;
        let supplier = 100000000_i128;
        let bank = opening + collection[1].debit_minor.parse::<i128>().unwrap()
            - supplier
            - refund[1].credit_minor.parse::<i128>().unwrap();
        assert_eq!(bank, 45000000);
        let remaining_clearing = 60000000_i128 - 60000000 - 15000000 + 15000000;
        assert_eq!(remaining_clearing, 0);
    }
    #[test]
    fn large_exact_values_and_explicit_fees_never_round_or_hide_invalid_direction() {
        assert!(legs(Effect::Collection, "9000000000000000", "1").is_ok());
        assert!(legs(Effect::Refund, "9000000000000000", "1").is_err());
        assert!(legs(Effect::Collection, "100", "100").is_err());
        assert!(legs(Effect::Collection, "100", "101").is_err());
        for bad in ["-1", "+1", "01", "1.0", "1e3", " 1", "9000000000000001"] {
            assert!(legs(Effect::Collection, bad, "0").is_err());
            assert!(legs(Effect::Refund, "1", bad).is_err());
        }
        for effect in [Effect::Collection, Effect::Refund] {
            for (gross, fee) in [("1", "0"), ("9000000000000000", "0"), ("1001", "100")] {
                let rows = legs(effect, gross, fee).unwrap();
                let total = rows.iter().fold((0_i128, 0_i128), |a, l| {
                    (
                        a.0 + l.debit_minor.parse::<i128>().unwrap(),
                        a.1 + l.credit_minor.parse::<i128>().unwrap(),
                    )
                });
                assert_eq!(total.0, total.1);
            }
        }
    }
}
