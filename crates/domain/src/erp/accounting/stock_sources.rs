//! Exact original-cost journal effects. Persistence must resolve the real use,
//! book and original recognized source; these values never confer authority.
use super::{minor, valid_uuid, ValidationError, MAX_MINOR};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_PORTIONS: usize = 10_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StockEffect {
    Fulfillment,
    RestockReturn,
    DiscardReturn,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StockRole {
    Inventory,
    CostOfGoodsSold,
}

/// Cost is already exact book minor units. Unknown cost must remain unknown.
/// A return's COGS account is its original recognized fulfillment account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CostPortion {
    pub inventory_account_id: String,
    pub cogs_account_id: String,
    pub cost_minor: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StockLeg {
    pub account_id: String,
    pub role: StockRole,
    pub debit_minor: String,
    pub credit_minor: String,
}

/// Never net across an unknown basis or substitute a current policy account.
/// The operating adapter must distinguish a service-only source from missing
/// tracked-stock portions and verify original recognition before return posting.
pub fn legs(
    effect: StockEffect,
    portions: &[CostPortion],
) -> Result<Vec<StockLeg>, ValidationError> {
    if portions.len() > MAX_PORTIONS {
        return Err(ValidationError("stockSourceSize"));
    }
    if effect == StockEffect::DiscardReturn {
        // Discard adds no inventory and does not reverse the original COGS.
        // Its original cost evidence belongs in the immutable source snapshot.
        return Ok(vec![]);
    }
    let mut total = 0_i64;
    let mut grouped = BTreeMap::<String, (StockRole, i64)>::new();
    for portion in portions {
        let inventory = portion.inventory_account_id.to_ascii_lowercase();
        let cogs = portion.cogs_account_id.to_ascii_lowercase();
        if !valid_uuid(&portion.inventory_account_id)
            || !valid_uuid(&portion.cogs_account_id)
            || inventory == cogs
        {
            return Err(ValidationError("stockSourceAccount"));
        }
        let cost = minor(
            portion
                .cost_minor
                .as_deref()
                .ok_or(ValidationError("unknownStockCost"))?,
        )?;
        total = total
            .checked_add(cost)
            .filter(|v| *v <= MAX_MINOR)
            .ok_or(ValidationError("stockSourceAmount"))?;
        for (id, role) in [
            (&inventory, StockRole::Inventory),
            (&cogs, StockRole::CostOfGoodsSold),
        ] {
            let entry = grouped.entry(id.clone()).or_insert((role, 0));
            if entry.0 != role {
                return Err(ValidationError("stockSourceAccountRole"));
            }
            entry.1 = entry
                .1
                .checked_add(cost)
                .filter(|v| *v <= MAX_MINOR)
                .ok_or(ValidationError("stockSourceAmount"))?;
        }
    }
    let result = grouped
        .into_iter()
        .filter(|(_, (_, amount))| *amount != 0)
        .map(|(account_id, (role, amount))| {
            let debit =
                (effect == StockEffect::Fulfillment) == (role == StockRole::CostOfGoodsSold);
            StockLeg {
                account_id,
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
            }
        })
        .collect::<Vec<_>>();
    if result.len() > 100 {
        return Err(ValidationError("stockSourceAccounts"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const OLD: &str = "00000000-0000-4000-8000-000000000001";
    const NEW: &str = "00000000-0000-4000-8000-000000000002";
    const COGS: &str = "00000000-0000-4000-8000-000000000003";
    const OTHER_COGS: &str = "00000000-0000-4000-8000-000000000004";
    #[test]
    fn stock_commands_bind_only_reviewed_references_and_reject_amount_or_authority_claims() {
        let policy = json!({"requestId":OLD,"command":{"type":"stock_policy.set","bookId":OLD,"expectedRevision":0,"currencyCatalogue":"SIX-2026-09-17","sourceUnit":"whole_idr","cogsAccount":{"accountId":"AAAAAAAA-0000-4000-8000-000000000001","expectedAccountRevision":1},"reason":"Reviewed future stock policy"}});
        let parsed = super::super::parse(&policy).unwrap();
        match parsed.command {
            super::super::Command::StockPolicySet { cogs_account, .. } => assert_eq!(
                cogs_account.account_id,
                "aaaaaaaa-0000-4000-8000-000000000001"
            ),
            _ => panic!("wrong typed stock policy"),
        }
        for (key, value) in [
            ("sourceUnit", json!("book_minor")),
            ("currencyCatalogue", json!("unreviewed")),
            ("expectedRevision", json!(-1)),
            ("actorId", json!("owner")),
            ("amountMinor", json!("100")),
        ] {
            let mut bad = policy.clone();
            bad["command"][key] = value;
            assert!(super::super::parse(&bad).is_err());
        }
        let draft = json!({"requestId":OLD,"command":{"type":"stock_source.draft","sourceId":NEW,"code":"STOCK","accounts":[{"accountId":OLD,"expectedAccountRevision":1},{"accountId":COGS,"expectedAccountRevision":1},{"accountId":NEW,"expectedAccountRevision":2}],"reason":"Review actual original stock source"}});
        assert!(super::super::parse(&draft).is_ok());
        for (key, value) in [
            ("amountMinor", json!("999")),
            ("effect", json!("fulfillment")),
            ("approved", json!(true)),
            ("expectedRevision", json!(1)),
        ] {
            let mut bad = draft.clone();
            bad["command"][key] = value;
            assert!(super::super::parse(&bad).is_err());
        }
        let mut repeated = draft.clone();
        repeated["command"]["accounts"][2] = repeated["command"]["accounts"][0].clone();
        assert!(super::super::parse(&repeated).is_err());
        let mut stale = draft;
        stale["command"]["accounts"][0]["expectedAccountRevision"] = json!(0);
        assert!(super::super::parse(&stale).is_err());
    }
    fn portion(inventory: &str, cogs: &str, cost: Option<&str>) -> CostPortion {
        CostPortion {
            inventory_account_id: inventory.into(),
            cogs_account_id: cogs.into(),
            cost_minor: cost.map(str::to_owned),
        }
    }
    #[test]
    fn split_original_inventory_and_cogs_accounts_balance_without_using_a_later_mapping() {
        let portions = [
            portion(OLD, COGS, Some("700000")),
            portion(NEW, OTHER_COGS, Some("700000")),
            portion(OLD, COGS, Some("10100")),
        ];
        let sale = legs(StockEffect::Fulfillment, &portions).unwrap();
        assert_eq!(sale.len(), 4);
        assert_eq!(sale[0].credit_minor, "710100");
        assert_eq!(sale[1].credit_minor, "700000");
        assert_eq!(sale[2].debit_minor, "710100");
        assert_eq!(sale[3].debit_minor, "700000");
        let restock = legs(StockEffect::RestockReturn, &portions).unwrap();
        for (sale, returned) in sale.iter().zip(restock) {
            assert_eq!(sale.account_id, returned.account_id);
            assert_eq!(sale.debit_minor, returned.credit_minor);
            assert_eq!(sale.credit_minor, returned.debit_minor);
        }
    }
    #[test]
    fn unknown_cost_is_not_zero_and_discard_never_invents_restock_recognition() {
        let unknown = [portion(OLD, COGS, None)];
        assert!(legs(StockEffect::Fulfillment, &unknown).is_err());
        assert!(legs(StockEffect::RestockReturn, &unknown).is_err());
        assert!(legs(StockEffect::DiscardReturn, &unknown)
            .unwrap()
            .is_empty());
        assert!(
            legs(StockEffect::Fulfillment, &[portion(OLD, COGS, Some("0"))])
                .unwrap()
                .is_empty()
        );
        assert!(serde_json::from_value::<CostPortion>(
            serde_json::json!({"inventoryAccountId":OLD,"cogsAccountId":COGS,"costMinor":700000})
        )
        .is_err());
    }
    #[test]
    fn source_total_cannot_overflow_even_when_each_individual_portion_fits() {
        let maximum = MAX_MINOR.to_string();
        assert!(legs(
            StockEffect::Fulfillment,
            &[portion(OLD, COGS, Some(&maximum))]
        )
        .is_ok());
        assert!(legs(
            StockEffect::Fulfillment,
            &[
                portion(OLD, COGS, Some(&maximum)),
                portion(NEW, COGS, Some("1"))
            ]
        )
        .is_err());
        for bad in ["-1", "00", "1.5", "1e2", "+1"] {
            assert!(legs(StockEffect::RestockReturn, &[portion(OLD, COGS, Some(bad))]).is_err());
        }
    }
    #[test]
    fn one_account_cannot_change_role_and_all_small_split_reversals_are_exact() {
        let alpha = "aaaaaaaa-0000-4000-8000-000000000001";
        assert!(legs(
            StockEffect::Fulfillment,
            &[portion(alpha, &alpha.to_ascii_uppercase(), Some("1"))]
        )
        .is_err());
        let aliased = legs(
            StockEffect::Fulfillment,
            &[
                portion(alpha, COGS, Some("1")),
                portion(&alpha.to_ascii_uppercase(), COGS, Some("2")),
            ],
        )
        .unwrap();
        assert_eq!(aliased.len(), 2);
        assert!(aliased
            .iter()
            .any(|l| l.account_id == alpha && l.credit_minor == "3"));
        assert!(legs(
            StockEffect::Fulfillment,
            &[portion(OLD, COGS, Some("1")), portion(COGS, NEW, Some("1"))]
        )
        .is_err());
        for left in 0..=40 {
            for right in 0..=40 {
                let portions = [
                    portion(OLD, COGS, Some(&left.to_string())),
                    portion(NEW, COGS, Some(&right.to_string())),
                ];
                for effect in [StockEffect::Fulfillment, StockEffect::RestockReturn] {
                    let result = legs(effect, &portions).unwrap();
                    let debit: i64 = result
                        .iter()
                        .map(|l| l.debit_minor.parse::<i64>().unwrap())
                        .sum();
                    let credit: i64 = result
                        .iter()
                        .map(|l| l.credit_minor.parse::<i64>().unwrap())
                        .sum();
                    assert_eq!(debit, left + right);
                    assert_eq!(debit, credit);
                }
            }
        }
    }
}
