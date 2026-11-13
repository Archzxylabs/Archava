//! A declared opening balance on a control account: one reviewed lump amount
//! against equity. No documents are imported, so nothing here is aged.
use super::{minor, ValidationError};
use serde::{Deserialize, Serialize};

/// The control account an opening is declared on.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Control {
    Receivable,
    Payable,
    Inventory,
}
impl Control {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Receivable => "receivable",
            Self::Payable => "payable",
            Self::Inventory => "inventory",
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Control,
    Offset,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Leg {
    pub role: Role,
    pub debit_minor: String,
    pub credit_minor: String,
}

/// The control account on its natural side (receivable and inventory are
/// debits, payable is a credit) and the equity offset on the other. A zero or
/// opposite-side opening is not representable.
pub fn legs(control: Control, amount: &str) -> Result<[Leg; 2], ValidationError> {
    let amount = minor(amount)?;
    if amount == 0 {
        return Err(ValidationError("openingAmount"));
    }
    let (debit, credit) = if control == Control::Payable {
        ("0".to_string(), amount.to_string())
    } else {
        (amount.to_string(), "0".to_string())
    };
    Ok([
        Leg {
            role: Role::Control,
            debit_minor: debit.clone(),
            credit_minor: credit.clone(),
        },
        Leg {
            role: Role::Offset,
            debit_minor: credit,
            credit_minor: debit,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn opening_commands_name_one_control_and_refuse_claimed_actors_sides_and_amounts() {
        let raw = json!({"requestId":"A0000000-0000-4000-8000-000000000001","command":{
            "type":"opening_balance.draft","bookId":"A0000000-0000-4000-8000-000000000002",
            "control":"receivable","code":"OPEN-AR","date":"2026-11-01",
            "controlAccount":{"accountId":"A0000000-0000-4000-8000-000000000004","expectedAccountRevision":1},
            "offsetAccount":{"accountId":"A0000000-0000-4000-8000-000000000005","expectedAccountRevision":1},
            "amountMinor":"250000","reason":"Reviewed opening receivable"}});
        let decoded = super::super::parse(&raw).unwrap();
        assert_eq!(decoded.command.operation(), "opening_balance.draft");
        assert_eq!(decoded.command.permission(), "journals.write");
        let canonical = serde_json::to_value(decoded).unwrap();
        assert_eq!(
            canonical["command"]["bookId"],
            "a0000000-0000-4000-8000-000000000002"
        );
        assert_eq!(
            canonical["command"]["controlAccount"]["accountId"],
            "a0000000-0000-4000-8000-000000000004"
        );
        assert_eq!(
            canonical["command"]["offsetAccount"]["accountId"],
            "a0000000-0000-4000-8000-000000000005"
        );
        for (field, value) in [
            ("actorId", json!("owner")),
            ("sourceRef", json!("opening:receivable:OPEN-AR")),
            ("side", json!("credit")),
            ("lines", json!([])),
        ] {
            let mut forged = raw.clone();
            forged["command"][field] = value;
            assert!(super::super::parse(&forged).is_err(), "{field}");
        }
        for (field, value) in [
            ("control", json!("cash")),
            ("date", json!("2026-02-30")),
            ("code", json!("lower case")),
            ("amountMinor", json!("0")),
            ("amountMinor", json!("-1")),
            ("amountMinor", json!("1.0")),
        ] {
            let mut invalid = raw.clone();
            invalid["command"][field] = value;
            assert!(super::super::parse(&invalid).is_err(), "{field}");
        }
        let mut reused = raw;
        reused["command"]["offsetAccount"] = reused["command"]["controlAccount"].clone();
        assert!(super::super::parse(&reused).is_err());
    }
    #[test]
    fn each_control_opens_on_its_natural_side_against_the_offset() {
        for (control, debit) in [
            (Control::Receivable, true),
            (Control::Inventory, true),
            (Control::Payable, false),
        ] {
            let [first, second] = legs(control, "15000").unwrap();
            assert_eq!(first.role, Role::Control);
            assert_eq!(second.role, Role::Offset);
            let (d, c) = if debit {
                ("15000", "0")
            } else {
                ("0", "15000")
            };
            assert_eq!(
                (first.debit_minor.as_str(), first.credit_minor.as_str()),
                (d, c)
            );
            assert_eq!(
                (second.debit_minor.as_str(), second.credit_minor.as_str()),
                (c, d)
            );
        }
        for bad in ["0", "-1", "+1", "01", "1.0", " 1", "9000000000000001"] {
            assert!(legs(Control::Receivable, bad).is_err(), "{bad}");
        }
    }
}
