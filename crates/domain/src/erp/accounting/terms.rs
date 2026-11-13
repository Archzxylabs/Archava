//! Explicit reviewed due terms and their effective-date selection. Persistence
//! supplies one document's complete immutable history and enforces authority,
//! original-source identity, document versions and open-period restrictions.
use super::{parse_calendar_date, ValidationError};
use serde::{Deserialize, Serialize};

/// Unknown is an explicit state; a missing condition cannot silently clear terms.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    Unknown {},
    DueOn { date: String },
}
impl Condition {
    pub fn due_date(&self) -> Option<&str> {
        match self {
            Self::Unknown {} => None,
            Self::DueOn { date } => Some(date),
        }
    }
    fn validate(&self) -> Result<(), ValidationError> {
        if self
            .due_date()
            .is_some_and(|date| parse_calendar_date(date).is_none())
        {
            return Err(ValidationError("documentDueDate"));
        }
        Ok(())
    }
}
pub(super) fn valid_proposal(
    source: &str,
    document_revision: i32,
    revision: i32,
    effective: &str,
    condition: &Condition,
) -> bool {
    super::valid_uuid(source)
        && document_revision > 0
        && (0..i32::MAX).contains(&revision)
        && parse_calendar_date(effective).is_some()
        && condition.validate().is_ok()
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Terms {
    pub revision: u32,
    pub effective_from: String,
    pub condition: Condition,
}
/// Validate a compare-and-set replacement without choosing terms for the user.
/// An explicit due date may precede the accounting source date (for example an
/// advance-payment agreement); its truth remains a reviewed business fact.
pub fn next(
    source_date: &str,
    previous: Option<&Terms>,
    expected_revision: u32,
    effective_from: &str,
    condition: Condition,
) -> Result<Terms, ValidationError> {
    let source = parse_calendar_date(source_date).ok_or(ValidationError("termsSourceDate"))?;
    let effective =
        parse_calendar_date(effective_from).ok_or(ValidationError("termsEffectiveDate"))?;
    if source.epoch_days > effective.epoch_days {
        return Err(ValidationError("termsEffectiveDate"));
    }
    condition.validate()?;
    let revision = match previous {
        Some(previous) => {
            previous.condition.validate()?;
            if previous.revision == 0
                || previous.revision != expected_revision
                || parse_calendar_date(&previous.effective_from).is_none()
                || previous.effective_from.as_str() > effective_from
                || previous.effective_from.as_str() < source_date
            {
                return Err(ValidationError("termsRevision"));
            }
            previous.revision.checked_add(1)
        }
        None if expected_revision == 0 => Some(1),
        None => None,
    }
    .ok_or(ValidationError("termsRevision"))?;
    Ok(Terms {
        revision,
        effective_from: effective_from.into(),
        condition,
    })
}
/// Return the last known condition effective at the requested business cutoff.
/// No history and explicitly unknown history stay distinguishable to the caller.
/// The original input is never reordered or amended by this read.
pub fn at<'a>(
    source_date: &str,
    as_of: &str,
    history: &'a [Terms],
) -> Result<Option<&'a Terms>, ValidationError> {
    let source = parse_calendar_date(source_date).ok_or(ValidationError("termsSourceDate"))?;
    let cutoff = parse_calendar_date(as_of).ok_or(ValidationError("termsAsOf"))?;
    if source.epoch_days > cutoff.epoch_days {
        return Err(ValidationError("termsFutureSource"));
    }
    let mut versions = history.iter().collect::<Vec<_>>();
    versions.sort_by_key(|version| version.revision);
    let mut previous = None;
    let mut selected = None;
    for term in versions {
        let checked = next(
            source_date,
            previous,
            previous.map_or(0, |p: &Terms| p.revision),
            &term.effective_from,
            term.condition.clone(),
        )?;
        if &checked != term {
            return Err(ValidationError("termsHistory"));
        }
        if term.effective_from.as_str() <= as_of {
            selected = Some(term);
        }
        previous = Some(term);
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn registered_term_commands_require_explicit_source_versions_and_condition_without_claimed_authority(
    ) {
        for operation in ["invoice_terms.set", "supplier_terms.set"] {
            let body = json!({"requestId":"A0000000-0000-4000-8000-000000000001","command":{"type":operation,"sourceId":"A0000000-0000-4000-8000-000000000002","expectedDocumentRevision":3,"expectedRevision":0,"effectiveFrom":"2026-11-12","condition":{"kind":"due_on","date":"2026-11-30"},"reason":"Review actual original document and explicit dates"}});
            let decoded = super::super::parse(&body).unwrap();
            assert_eq!(decoded.command.operation(), operation);
            assert_eq!(decoded.command.permission(), "terms.write");
            assert_eq!(
                serde_json::to_value(decoded).unwrap()["command"]["sourceId"],
                "a0000000-0000-4000-8000-000000000002"
            );
            for (key, value) in [
                ("expectedDocumentRevision", json!(0)),
                ("expectedRevision", json!(-1)),
                ("expectedRevision", json!(i32::MAX)),
                ("effectiveFrom", json!("2026-02-29")),
                ("condition", json!({})),
                ("condition", json!({"kind":"unknown","date":"2026-11-30"})),
                ("actorId", json!("owner")),
                ("documentKind", json!("payable")),
                ("balanceMinor", json!("0")),
            ] {
                let mut bad = body.clone();
                bad["command"][key] = value;
                assert!(super::super::parse(&bad).is_err(), "{key}");
            }
            let mut unknown = body;
            unknown["command"]["condition"] = json!({"kind":"unknown"});
            assert!(super::super::parse(&unknown).is_ok());
        }
    }
    fn due(date: &str) -> Condition {
        Condition::DueOn { date: date.into() }
    }
    #[test]
    fn condition_requires_an_explicit_known_date_or_unknown_state() {
        for value in [
            json!({}),
            json!({"kind":"due_on"}),
            json!({"kind":"unknown","date":"2026-11-30"}),
            json!({"kind":"net30"}),
        ] {
            assert!(serde_json::from_value::<Condition>(value).is_err());
        }
        assert_eq!(
            serde_json::from_value::<Condition>(json!({"kind":"unknown"})).unwrap(),
            Condition::Unknown {}
        );
        assert_eq!(
            serde_json::from_value::<Condition>(json!({"kind":"due_on","date":"2026-11-30"}))
                .unwrap(),
            due("2026-11-30")
        );
    }
    #[test]
    fn stale_or_missing_expected_versions_cannot_replace_terms_and_dates_are_not_inferred() {
        let first = next("2026-11-01", None, 0, "2026-11-01", due("2026-11-30")).unwrap();
        assert_eq!(first.revision, 1);
        assert!(next("2026-11-01", None, 1, "2026-11-01", due("2026-11-30")).is_err());
        assert!(next(
            "2026-11-01",
            Some(&first),
            0,
            "2026-11-12",
            Condition::Unknown {}
        )
        .is_err());
        assert!(next(
            "2026-11-01",
            Some(&first),
            1,
            "2026-10-31",
            Condition::Unknown {}
        )
        .is_err());
        let unknown = next(
            "2026-11-01",
            Some(&first),
            1,
            "2026-11-12",
            Condition::Unknown {},
        )
        .unwrap();
        assert_eq!(unknown.revision, 2);
        assert_eq!(unknown.condition.due_date(), None);
        assert_eq!(first.condition.due_date(), Some("2026-11-30"));
    }
    #[test]
    fn future_terms_and_later_unknown_terms_do_not_rewrite_earlier_cutoffs() {
        let first = next("2026-11-01", None, 0, "2026-11-05", due("2026-11-30")).unwrap();
        let second = next(
            "2026-11-01",
            Some(&first),
            1,
            "2026-11-15",
            due("2026-12-15"),
        )
        .unwrap();
        let third = next(
            "2026-11-01",
            Some(&second),
            2,
            "2026-11-20",
            Condition::Unknown {},
        )
        .unwrap();
        let history = [third.clone(), first.clone(), second.clone()];
        assert_eq!(at("2026-11-01", "2026-11-04", &history).unwrap(), None);
        assert_eq!(
            at("2026-11-01", "2026-11-05", &history).unwrap(),
            Some(&first)
        );
        assert_eq!(
            at("2026-11-01", "2026-11-14", &history).unwrap(),
            Some(&first)
        );
        assert_eq!(
            at("2026-11-01", "2026-11-15", &history).unwrap(),
            Some(&second)
        );
        assert_eq!(
            at("2026-11-01", "2026-11-20", &history).unwrap(),
            Some(&third)
        );
        assert_eq!(history[0], third);
    }
    #[test]
    fn duplicate_missing_or_backdated_versions_refuse_instead_of_choosing_a_plausible_history() {
        let first = next("2026-11-01", None, 0, "2026-11-05", due("2026-11-30")).unwrap();
        assert!(at("2026-11-01", "2026-11-12", &[first.clone(), first.clone()]).is_err());
        let mut gap = first.clone();
        gap.revision = 3;
        assert!(at("2026-11-01", "2026-11-12", &[gap]).is_err());
        let mut backdated = first.clone();
        backdated.revision = 2;
        backdated.effective_from = "2026-11-04".into();
        assert!(at("2026-11-01", "2026-11-12", &[first, backdated]).is_err());
        assert!(at("2026-11-13", "2026-11-12", &[]).is_err());
    }
    #[test]
    fn invalid_calendar_dates_and_revision_overflow_are_refused() {
        for date in ["2026-02-29", "2100-02-29", "2026-1-01"] {
            assert!(next("2026-01-01", None, 0, "2026-01-01", due(date)).is_err());
            assert!(next("2026-01-01", None, 0, date, Condition::Unknown {}).is_err());
        }
        let max = Terms {
            revision: u32::MAX,
            effective_from: "2026-11-01".into(),
            condition: Condition::Unknown {},
        };
        assert!(next(
            "2026-11-01",
            Some(&max),
            u32::MAX,
            "2026-11-12",
            Condition::Unknown {}
        )
        .is_err());
        assert!(at("2026-11-01", "2026-02-29", &[]).is_err());
    }
}
