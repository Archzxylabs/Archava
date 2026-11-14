//! The unit of a trade amount, stated instead of left to inference.
//!
//! A trade record's integer amounts are whole Rupiah when its currency is IDR
//! and the currency's ISO minor unit otherwise
//! (`docs/architecture/MULTI_CURRENCY.md`). The planner is told which applies;
//! a proposal is still reviewed by a person, so this is a hint, not a guard.
use archava_domain::erp::trade_money;
use serde_json::{json, Value};

/// `whole IDR` or `minor units of <CUR>`: the words every amount is labelled with.
pub(super) fn label(currency: &str) -> String {
    if currency == trade_money::IDR {
        "whole IDR".into()
    } else {
        format!("minor units of {currency}")
    }
}
/// The stored unit name of the trade rule: `whole_idr` or `minor`.
pub(super) fn unit(currency: &str) -> &'static str {
    if currency == trade_money::IDR {
        "whole_idr"
    } else {
        "minor"
    }
}
/// What the planner is told about the amounts of one currency.
pub(super) fn describe(currency: &str) -> Value {
    let sample = if currency == trade_money::IDR {
        150_000
    } else {
        1_999
    };
    json!({
        "currency": currency,
        "unit": unit(currency),
        "unitLabel": label(currency),
        "fractionDigits": trade_money::trade_decimals(currency),
        "example": format!("the integer {sample} is {}", trade_money::format_trade(currency, sample)),
        "rule": "Every integer money field of this company's trade records (price, unitCost, unitCostMinor, landedFeeMinor, declaredTotal, taxMinor, unitMinor, amountMinor, totals) is in this unit. Never convert between currencies and never scale an amount by 100 unless this unit requires it.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idr_is_whole_rupiah_and_every_other_currency_is_its_minor_unit() {
        let idr = describe("IDR");
        assert_eq!(idr["unit"], "whole_idr");
        assert_eq!(idr["unitLabel"], "whole IDR");
        assert_eq!(idr["fractionDigits"], 0);
        assert_eq!(idr["example"], "the integer 150000 is Rp 150.000");
        let usd = describe("USD");
        assert_eq!(usd["unit"], "minor");
        assert_eq!(usd["unitLabel"], "minor units of USD");
        assert_eq!(usd["fractionDigits"], 2);
        assert_eq!(usd["example"], "the integer 1999 is USD 19.99");
        assert_eq!(describe("JPY")["example"], "the integer 1999 is JPY 1999");
        assert_eq!(describe("KWD")["example"], "the integer 1999 is KWD 1.999");
    }
}
