//! Unposted supplier claims and exact goods-document reconciliation.
use super::{
    purchasing::{ValidationError, COST_MAX, REVISION_MAX, TOTAL_MAX},
    valid_uuid,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimLine {
    pub line_id: String,
    pub quantity: i64,
    pub unit_cost: i64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Capture {
    pub order_id: String,
    pub expected_order_revision: i64,
    pub supplier_id: String,
    pub vendor_reference: String,
    pub document_date: String,
    pub currency: String,
    pub declared_total: i64,
    pub lines: Vec<ClaimLine>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Revise {
    pub id: String,
    pub expected_revision: i64,
    pub expected_order_revision: i64,
    pub document_date: String,
    pub currency: String,
    pub declared_total: i64,
    pub lines: Vec<ClaimLine>,
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Match {
    pub id: String,
    pub expected_revision: i64,
    pub expected_order_revision: i64,
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Void {
    pub id: String,
    pub expected_revision: i64,
    pub reason: String,
}
fn uuid(s: &mut str) -> Result<(), ValidationError> {
    if !valid_uuid(s) {
        return Err(ValidationError("id"));
    }
    s.make_ascii_lowercase();
    Ok(())
}
fn revision(n: i64) -> Result<(), ValidationError> {
    if !(1..REVISION_MAX).contains(&n) {
        return Err(ValidationError("revision"));
    }
    Ok(())
}
fn reason(s: &mut String) -> Result<(), ValidationError> {
    *s = s.trim().to_owned();
    if s.is_empty() || s.chars().count() > 2000 {
        return Err(ValidationError("reason"));
    }
    Ok(())
}
fn date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || !b
            .iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return false;
    }
    let y = s[..4].parse::<u32>().unwrap_or(0);
    let m = s[5..7].parse::<usize>().unwrap_or(0);
    let d = s[8..].parse::<u32>().unwrap_or(0);
    let days = [
        0,
        31,
        if y % 400 == 0 || (y % 4 == 0 && y % 100 != 0) {
            29
        } else {
            28
        },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    (1900..=9999).contains(&y) && (1..=12).contains(&m) && d > 0 && d <= days[m]
}
fn claim(
    day: &str,
    currency: &str,
    total: i64,
    lines: &mut [ClaimLine],
) -> Result<(), ValidationError> {
    if !date(day) {
        return Err(ValidationError("documentDate"));
    }
    if currency != "IDR" {
        return Err(ValidationError("currency"));
    }
    if !(0..=TOTAL_MAX).contains(&total) {
        return Err(ValidationError("declaredTotal"));
    }
    if !(1..=50).contains(&lines.len()) {
        return Err(ValidationError("lines"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for line in lines {
        uuid(&mut line.line_id)?;
        if !seen.insert(line.line_id.clone())
            || !(1..=1_000_000).contains(&line.quantity)
            || !(0..=COST_MAX).contains(&line.unit_cost)
        {
            return Err(ValidationError("lines"));
        }
    }
    Ok(())
}
fn decode<T: serde::de::DeserializeOwned>(raw: &Value) -> Result<T, ValidationError> {
    let mut raw = raw.clone();
    raw.as_object_mut()
        .ok_or(ValidationError("operation"))?
        .remove("operation");
    serde_json::from_value(raw).map_err(|_| ValidationError("document"))
}
impl Capture {
    pub fn parse(raw: &Value) -> Result<Self, ValidationError> {
        let mut v: Self = decode(raw)?;
        uuid(&mut v.order_id)?;
        uuid(&mut v.supplier_id)?;
        revision(v.expected_order_revision)?;
        v.vendor_reference = v
            .vendor_reference
            .split_ascii_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_uppercase();
        if v.vendor_reference.is_empty()
            || v.vendor_reference.len() > 120
            || !v.vendor_reference.as_bytes()[0].is_ascii_alphanumeric()
            || !v
                .vendor_reference
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b" ./_-".contains(&b))
        {
            return Err(ValidationError("vendorReference"));
        }
        claim(
            &v.document_date,
            &v.currency,
            v.declared_total,
            &mut v.lines,
        )?;
        Ok(v)
    }
}
impl Revise {
    pub fn parse(raw: &Value) -> Result<Self, ValidationError> {
        let mut v: Self = decode(raw)?;
        uuid(&mut v.id)?;
        revision(v.expected_revision)?;
        revision(v.expected_order_revision)?;
        reason(&mut v.reason)?;
        claim(
            &v.document_date,
            &v.currency,
            v.declared_total,
            &mut v.lines,
        )?;
        Ok(v)
    }
}
impl Match {
    pub fn parse(raw: &Value) -> Result<Self, ValidationError> {
        let mut v: Self = decode(raw)?;
        uuid(&mut v.id)?;
        revision(v.expected_revision)?;
        revision(v.expected_order_revision)?;
        reason(&mut v.reason)?;
        Ok(v)
    }
}
impl Void {
    pub fn parse(raw: &Value) -> Result<Self, ValidationError> {
        let mut v: Self = decode(raw)?;
        uuid(&mut v.id)?;
        revision(v.expected_revision)?;
        reason(&mut v.reason)?;
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn supplier_claims_reject_impossible_dates_duplicate_ids_and_authority_fields() {
        let mut raw = json!({"operation":"document.capture","orderId":"00000000-0000-4000-8000-000000000001","expectedOrderRevision":2,"supplierId":"00000000-0000-4000-8000-000000000002","vendorReference":" inv / 007 ","documentDate":"2000-02-29","currency":"IDR","declaredTotal":23,"lines":[{"lineId":"00000000-0000-4000-8000-00000000000a","quantity":2,"unitCost":10}]});
        let parsed = Capture::parse(&raw).unwrap();
        assert_eq!(parsed.vendor_reference, "INV / 007");
        assert_eq!(parsed.declared_total, 23); // A total discrepancy is evidence for a held match, not a discarded vendor claim.
        for day in [
            "1900-02-29",
            "2026-02-29",
            "2026-04-31",
            "2026-1-01",
            "2026-01-00",
        ] {
            raw["documentDate"] = json!(day);
            assert!(Capture::parse(&raw).is_err())
        }
        raw["documentDate"] = json!("2024-02-29");
        raw["ownerId"] = json!("other");
        assert!(Capture::parse(&raw).is_err());
        raw.as_object_mut().unwrap().remove("ownerId");
        let mut second = raw["lines"][0].clone();
        second["lineId"] = json!("00000000-0000-4000-8000-00000000000A");
        raw["lines"].as_array_mut().unwrap().push(second);
        assert!(Capture::parse(&raw).is_err());
    }
}
