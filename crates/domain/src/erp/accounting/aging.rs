//! Exact as-of aging of explicitly supplied posted positions. No default terms,
//! authority, database reads, currency conversion or current operational balances.
use super::{bank::signed_minor, parse_calendar_date, valid_uuid, ValidationError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Position {
    pub document_id: String,
    pub source_date: String,
    pub due_date: Option<String>,
    pub balance_minor: String,
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Bucket {
    Settled,
    Credit,
    UnknownTerms,
    Current,
    Overdue1To30,
    Overdue31To60,
    Overdue61To90,
    OverdueOver90,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgedPosition {
    #[serde(flatten)]
    pub position: Position,
    pub bucket: Bucket,
    pub days_past_due: Option<i64>,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub unknown_terms_minor: String,
    pub current_minor: String,
    pub overdue_1_to_30_minor: String,
    pub overdue_31_to_60_minor: String,
    pub overdue_61_to_90_minor: String,
    pub overdue_over_90_minor: String,
    pub credit_minor: String,
    pub positive_outstanding_minor: String,
    pub net_outstanding_minor: String,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Aging {
    pub as_of: String,
    pub positions: Vec<AgedPosition>,
    pub totals: Totals,
}
/// Each input must already be the complete position for one original document
/// through the same business-date cutoff. This pure function makes no claim that
/// a page, caller-selected slice or operational invoice state is a full subledger.
pub fn report(as_of: &str, positions: &[Position]) -> Result<Aging, ValidationError> {
    let cutoff = parse_calendar_date(as_of).ok_or(ValidationError("agingAsOf"))?;
    let mut observed = BTreeSet::new();
    let mut totals = [0_i128; 7];
    let mut rows = Vec::with_capacity(positions.len());
    for position in positions {
        if !valid_uuid(&position.document_id)
            || !observed.insert(position.document_id.to_ascii_lowercase())
        {
            return Err(ValidationError("agingDocument"));
        }
        let source =
            parse_calendar_date(&position.source_date).ok_or(ValidationError("agingSourceDate"))?;
        if source.epoch_days > cutoff.epoch_days {
            return Err(ValidationError("agingFutureSource"));
        }
        let due = position
            .due_date
            .as_ref()
            .map(|d| parse_calendar_date(d).ok_or(ValidationError("agingDueDate")))
            .transpose()?;
        let value = i128::from(signed_minor(&position.balance_minor)?);
        let (bucket, days, index) = if value == 0 {
            (Bucket::Settled, None, None)
        } else if value < 0 {
            (Bucket::Credit, None, Some(6))
        } else if let Some(due) = due {
            let days = cutoff.epoch_days - due.epoch_days;
            let (bucket, index) = match days {
                ..=0 => (Bucket::Current, 1),
                1..=30 => (Bucket::Overdue1To30, 2),
                31..=60 => (Bucket::Overdue31To60, 3),
                61..=90 => (Bucket::Overdue61To90, 4),
                _ => (Bucket::OverdueOver90, 5),
            };
            (bucket, Some(days.max(0)), Some(index))
        } else {
            (Bucket::UnknownTerms, None, Some(0))
        };
        if let Some(index) = index {
            totals[index] = totals[index]
                .checked_add(value)
                .ok_or(ValidationError("agingTotal"))?;
        }
        rows.push(AgedPosition {
            position: position.clone(),
            bucket,
            days_past_due: days,
        });
    }
    let positive = totals[..6]
        .iter()
        .try_fold(0_i128, |sum, value| sum.checked_add(*value))
        .ok_or(ValidationError("agingTotal"))?;
    let net = positive
        .checked_add(totals[6])
        .ok_or(ValidationError("agingTotal"))?;
    Ok(Aging {
        as_of: as_of.into(),
        positions: rows,
        totals: Totals {
            unknown_terms_minor: totals[0].to_string(),
            current_minor: totals[1].to_string(),
            overdue_1_to_30_minor: totals[2].to_string(),
            overdue_31_to_60_minor: totals[3].to_string(),
            overdue_61_to_90_minor: totals[4].to_string(),
            overdue_over_90_minor: totals[5].to_string(),
            credit_minor: totals[6].to_string(),
            positive_outstanding_minor: positive.to_string(),
            net_outstanding_minor: net.to_string(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(n: u32, due: Option<&str>, amount: &str) -> Position {
        Position {
            document_id: format!("a0000000-0000-4000-8000-{n:012}"),
            source_date: "2026-01-01".into(),
            due_date: due.map(Into::into),
            balance_minor: amount.into(),
        }
    }
    #[test]
    fn unknown_due_terms_are_not_aged_as_immediate_or_net_thirty() {
        let r = report("2026-11-12", &[row(1, None, "100")]).unwrap();
        assert_eq!(r.positions[0].bucket, Bucket::UnknownTerms);
        assert_eq!(r.positions[0].days_past_due, None);
        assert_eq!(r.totals.unknown_terms_minor, "100");
        assert_eq!(r.totals.current_minor, "0");
    }
    #[test]
    fn all_overdue_boundaries_and_due_today_use_calendar_days() {
        let dates = [
            ("2026-11-13", Bucket::Current, 0),
            ("2026-11-12", Bucket::Current, 0),
            ("2026-11-11", Bucket::Overdue1To30, 1),
            ("2026-10-13", Bucket::Overdue1To30, 30),
            ("2026-10-12", Bucket::Overdue31To60, 31),
            ("2026-09-13", Bucket::Overdue31To60, 60),
            ("2026-09-12", Bucket::Overdue61To90, 61),
            ("2026-08-14", Bucket::Overdue61To90, 90),
            ("2026-08-13", Bucket::OverdueOver90, 91),
        ];
        for (i, (date, bucket, days)) in dates.into_iter().enumerate() {
            let r = report("2026-11-12", &[row(i as u32 + 1, Some(date), "1")]).unwrap();
            assert_eq!(r.positions[0].bucket, bucket, "{date}");
            assert_eq!(r.positions[0].days_past_due, Some(days));
        }
    }
    #[test]
    fn credits_and_settled_positions_do_not_disappear_into_overdue_amounts() {
        let r = report(
            "2026-11-12",
            &[
                row(1, Some("2026-01-01"), "-200"),
                row(2, None, "0"),
                row(3, None, "100"),
            ],
        )
        .unwrap();
        assert_eq!(r.positions[0].bucket, Bucket::Credit);
        assert_eq!(r.positions[0].days_past_due, None);
        assert_eq!(r.positions[1].bucket, Bucket::Settled);
        assert_eq!(r.totals.credit_minor, "-200");
        assert_eq!(r.totals.positive_outstanding_minor, "100");
        assert_eq!(r.totals.net_outstanding_minor, "-100");
        assert_eq!(r.totals.overdue_over_90_minor, "0");
    }
    #[test]
    fn totals_above_the_per_document_and_javascript_integer_limits_remain_exact() {
        let r = report(
            "2026-11-12",
            &[
                row(1, None, "9000000000000000"),
                row(2, Some("2026-11-11"), "9000000000000000"),
                row(3, None, "-1"),
            ],
        )
        .unwrap();
        assert_eq!(r.totals.positive_outstanding_minor, "18000000000000000");
        assert_eq!(r.totals.net_outstanding_minor, "17999999999999999");
    }
    #[test]
    fn invalid_future_duplicate_and_noncanonical_positions_fail_without_partial_totals() {
        let good = row(1, Some("2026-02-28"), "100");
        for as_of in ["2026-02-29", "2026-1-01", "2101-01-01"] {
            assert!(report(as_of, std::slice::from_ref(&good)).is_err());
        }
        assert!(report("2026-11-12", &[good.clone(), good.clone()]).is_err());
        let mut alias = good.clone();
        alias.document_id = alias.document_id.to_ascii_uppercase();
        assert!(report("2026-11-12", &[good.clone(), alias]).is_err());
        for invalid in ["-0", "01", "-01", "1.5", "9000000000000001"] {
            let mut bad = good.clone();
            bad.balance_minor = invalid.into();
            assert!(report("2026-11-12", &[bad]).is_err());
        }
        let mut bad = good.clone();
        bad.source_date = "2026-11-13".into();
        assert!(report("2026-11-12", &[bad]).is_err());
        let mut bad = good;
        bad.due_date = Some("2026-02-29".into());
        assert!(report("2026-11-12", &[bad]).is_err());
    }
    #[test]
    fn leap_day_and_century_rules_use_the_shared_business_calendar() {
        let mut p = row(1, Some("2028-02-28"), "1");
        p.source_date = "2028-02-01".into();
        assert_eq!(
            report("2028-03-01", &[p]).unwrap().positions[0].days_past_due,
            Some(2)
        );
        let mut p = row(1, Some("2100-02-28"), "1");
        p.source_date = "2100-02-01".into();
        assert_eq!(
            report("2100-03-01", &[p]).unwrap().positions[0].days_past_due,
            Some(1)
        );
    }
}
