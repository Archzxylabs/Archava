//! Exact manual bank evidence and cash-leg matching. This module executes no transfers.
use super::{
    minor, minor_units, parse_calendar_date, sources, text, AccountClass, CashRole,
    SourceAuthority, ValidationError,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_STATEMENT_ROWS: usize = 1000;

/// The existing nonnegative minor-unit grammar, plus one canonical negative sign.
pub fn signed_minor(value: &str) -> Result<i64, ValidationError> {
    if let Some(digits) = value.strip_prefix('-') {
        let amount = minor(digits)?;
        if amount == 0 {
            return Err(ValidationError("signedMinorAmount"));
        }
        Ok(-amount)
    } else {
        minor(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatementRow {
    pub external_ref: String,
    pub date: String,
    pub amount_minor: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Statement {
    pub reference: String,
    pub from_date: String,
    pub to_date: String,
    pub currency: String,
    pub opening_minor: String,
    pub closing_minor: String,
    pub rows: Vec<StatementRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StatementTotals {
    pub row_count: usize,
    pub inflow_minor: String,
    pub outflow_minor: String,
    pub net_minor: String,
    pub closing_minor: String,
}

/// Validate every row and the full control total; never omit a duplicate or round.
pub fn statement_totals(
    statement: &Statement,
    book_currency: &str,
) -> Result<StatementTotals, ValidationError> {
    if statement.currency != book_currency || minor_units(book_currency).is_none() {
        return Err(ValidationError("bankCurrency"));
    }
    if !text(&statement.reference, 160)
        || parse_calendar_date(&statement.from_date).is_none()
        || parse_calendar_date(&statement.to_date).is_none()
        || statement.from_date > statement.to_date
        || statement.rows.len() > MAX_STATEMENT_ROWS
    {
        return Err(ValidationError("bankStatement"));
    }
    let opening = i128::from(signed_minor(&statement.opening_minor)?);
    let closing = i128::from(signed_minor(&statement.closing_minor)?);
    let (mut inflow, mut outflow) = (0_i128, 0_i128);
    let mut references = BTreeSet::new();
    for row in &statement.rows {
        if !text(&row.external_ref, 160)
            || !references.insert(&row.external_ref)
            || parse_calendar_date(&row.date).is_none()
            || row.date < statement.from_date
            || row.date > statement.to_date
            || !(row.description.is_empty() || text(&row.description, 500))
        {
            return Err(ValidationError("bankStatementRow"));
        }
        let amount = i128::from(signed_minor(&row.amount_minor)?);
        if amount == 0 {
            return Err(ValidationError("bankStatementRowAmount"));
        }
        if amount > 0 {
            inflow += amount;
        } else {
            outflow -= amount;
        }
    }
    let net = inflow - outflow;
    if opening + net != closing {
        return Err(ValidationError("bankControlTotal"));
    }
    Ok(StatementTotals {
        row_count: statement.rows.len(),
        inflow_minor: inflow.to_string(),
        outflow_minor: outflow.to_string(),
        net_minor: net.to_string(),
        closing_minor: closing.to_string(),
    })
}

pub fn valid_cash_binding(
    account: &sources::AccountRef,
    source: SourceAuthority,
    class: AccountClass,
    cash_role: CashRole,
    active: bool,
    currency: &str,
) -> bool {
    source == SourceAuthority::Native
        && class == AccountClass::Asset
        && cash_role == CashRole::Cash
        && active
        && minor_units(currency).is_some()
        && sources::valid_refs(std::slice::from_ref(account), 1)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AllocationRemainders {
    pub statement_remaining_minor: String,
    pub cash_leg_remaining_minor: String,
}

/// Only already scoped, posted cash legs may reach this arithmetic boundary.
/// Amount is a positive magnitude; the statement and ledger determine direction.
pub fn allocate(
    statement_remaining: &str,
    cash_leg_remaining: &str,
    amount: &str,
) -> Result<AllocationRemainders, ValidationError> {
    let statement = signed_minor(statement_remaining)?;
    let cash = signed_minor(cash_leg_remaining)?;
    let amount = minor(amount)?;
    if amount == 0
        || statement == 0
        || cash == 0
        || statement.signum() != cash.signum()
        || amount > statement.abs()
        || amount > cash.abs()
    {
        return Err(ValidationError("bankAllocation"));
    }
    let signed_amount = amount * statement.signum();
    Ok(AllocationRemainders {
        statement_remaining_minor: (statement - signed_amount).to_string(),
        cash_leg_remaining_minor: (cash - signed_amount).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn statement() -> Statement {
        Statement {
            reference: "SYNTHETIC-NOV-01".into(),
            from_date: "2026-11-01".into(),
            to_date: "2026-11-30".into(),
            currency: "IDR".into(),
            opening_minor: "100000000".into(),
            closing_minor: "45000000".into(),
            rows: ["60000000", "-100000000", "-15000000"]
                .iter()
                .enumerate()
                .map(|(i, amount)| StatementRow {
                    external_ref: format!("B01-{i}"),
                    date: "2026-11-09".into(),
                    amount_minor: (*amount).into(),
                    description: "Explicit synthetic control; not a live transfer".into(),
                })
                .collect(),
        }
    }

    #[test]
    fn synthetic_b01_bank_control_preserves_exact_book_units() {
        let totals = statement_totals(&statement(), "IDR").unwrap();
        assert_eq!(totals.row_count, 3);
        assert_eq!(totals.inflow_minor, "60000000");
        assert_eq!(totals.outflow_minor, "115000000");
        assert_eq!(totals.net_minor, "-55000000");
        assert_eq!(totals.closing_minor, "45000000");
    }

    #[test]
    fn signed_units_reject_ambiguous_or_out_of_range_wire_values() {
        for raw in [
            "-0",
            "+1",
            "00",
            "-01",
            " 1",
            "1 ",
            "--1",
            "1.0",
            "1e2",
            "9000000000000001",
            "-9000000000000001",
            "9223372036854775808",
            "−1",
        ] {
            assert!(signed_minor(raw).is_err(), "{raw}");
        }
        assert_eq!(signed_minor("0").unwrap(), 0);
        assert_eq!(
            signed_minor("-9000000000000000").unwrap(),
            -super::super::MAX_MINOR
        );
    }

    #[test]
    fn control_totals_remain_strings_above_javascript_safe_integer() {
        let mut input = statement();
        input.opening_minor = "0".into();
        input.closing_minor = "0".into();
        input.rows = [
            "9000000000000000",
            "9000000000000000",
            "-9000000000000000",
            "-9000000000000000",
        ]
        .iter()
        .enumerate()
        .map(|(i, amount)| StatementRow {
            external_ref: format!("LARGE-{i}"),
            date: "2026-11-10".into(),
            amount_minor: (*amount).into(),
            description: String::new(),
        })
        .collect();
        let totals = statement_totals(&input, "IDR").unwrap();
        assert_eq!(totals.inflow_minor, "18000000000000000");
        assert_eq!(totals.outflow_minor, "18000000000000000");
        assert_eq!(serde_json::to_value(totals).unwrap()["netMinor"], "0");
    }

    #[test]
    fn duplicate_rows_wrong_currency_and_incomplete_totals_never_preview_as_valid() {
        let input = statement();
        assert!(statement_totals(&input, "USD").is_err());
        assert!(statement_totals(&input, "UNKNOWN").is_err());
        let mut duplicate = input.clone();
        duplicate.rows[1].external_ref = duplicate.rows[0].external_ref.clone();
        assert!(statement_totals(&duplicate, "IDR").is_err());
        let mut wrong = input;
        wrong.closing_minor = "45000001".into();
        assert!(statement_totals(&wrong, "IDR").is_err());
    }

    #[test]
    fn real_dates_complete_windows_and_bounded_rows_are_required() {
        for date in ["2026-02-29", "2026-10-31", "2026-12-01"] {
            let mut input = statement();
            input.rows[0].date = date.into();
            assert!(statement_totals(&input, "IDR").is_err());
        }
        let mut input = statement();
        input.rows = vec![input.rows[0].clone(); MAX_STATEMENT_ROWS + 1];
        assert!(statement_totals(&input, "IDR").is_err());
        input.rows.clear();
        input.closing_minor = input.opening_minor.clone();
        assert_eq!(statement_totals(&input, "IDR").unwrap().row_count, 0);
    }

    #[test]
    fn opaque_refs_and_descriptions_never_silently_normalize() {
        for invalid in [" reference", "reference\n", "", "\t"] {
            let mut input = statement();
            input.rows[0].external_ref = invalid.into();
            assert!(statement_totals(&input, "IDR").is_err());
        }
        let mut input = statement();
        input.rows[0].description = "bad\nline".into();
        assert!(statement_totals(&input, "IDR").is_err());
    }

    #[test]
    fn matching_supports_exact_partial_allocations_in_both_directions() {
        let partial = allocate("-100000000", "-70000000", "60000000").unwrap();
        assert_eq!(partial.statement_remaining_minor, "-40000000");
        assert_eq!(partial.cash_leg_remaining_minor, "-10000000");
        assert_eq!(
            allocate("100", "100", "100")
                .unwrap()
                .cash_leg_remaining_minor,
            "0"
        );
        for (row, cash, amount) in [
            ("100", "-100", "1"),
            ("100", "100", "0"),
            ("100", "90", "91"),
            ("90", "100", "91"),
            ("0", "100", "1"),
            ("100", "0", "1"),
            ("100", "100", "-1"),
        ] {
            assert!(allocate(row, cash, amount).is_err());
        }
    }

    #[test]
    fn bank_binding_needs_explicit_native_active_cash_asset_review() {
        let account = sources::AccountRef {
            account_id: "00000000-0000-4000-8000-000000000001".into(),
            expected_account_revision: 1,
        };
        assert!(valid_cash_binding(
            &account,
            SourceAuthority::Native,
            AccountClass::Asset,
            CashRole::Cash,
            true,
            "USD"
        ));
        for (source, class, role, active) in [
            (
                SourceAuthority::External,
                AccountClass::Asset,
                CashRole::Cash,
                true,
            ),
            (
                SourceAuthority::Native,
                AccountClass::Liability,
                CashRole::Cash,
                true,
            ),
            (
                SourceAuthority::Native,
                AccountClass::Asset,
                CashRole::None,
                true,
            ),
            (
                SourceAuthority::Native,
                AccountClass::Asset,
                CashRole::Cash,
                false,
            ),
        ] {
            assert!(!valid_cash_binding(
                &account, source, class, role, active, "USD"
            ));
        }
    }
}
