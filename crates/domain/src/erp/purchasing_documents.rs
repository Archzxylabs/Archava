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
    /// The code text of an input tax code of the company's book
    /// (`docs/architecture/TAX.md`, stage D). Stated together with `tax_minor`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tax_code: Option<String>,
    /// The tax amount printed on the supplier's document, in the document's
    /// unit (whole Rupiah for IDR, the ISO minor unit otherwise). Beside the
    /// goods total, never part of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tax_minor: Option<i64>,
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
    /// The code text of an input tax code of the company's book
    /// (`docs/architecture/TAX.md`, stage D). Stated together with `tax_minor`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tax_code: Option<String>,
    /// The tax amount printed on the supplier's document, in the document's
    /// unit (whole Rupiah for IDR, the ISO minor unit otherwise). Beside the
    /// goods total, never part of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tax_minor: Option<i64>,
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
    // The grammar only: which code a claim may carry is its purchase order's
    // currency, and the adapter refuses any other against the locked order.
    if currency.len() != 3 || !currency.bytes().all(|byte| byte.is_ascii_uppercase()) {
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
/// The grammar of the two tax members only. Whether both are present, whether
/// the code is a confirmed input code that applies on the document date and
/// whether the amount is within the goods total are decided against the stored
/// rows, with their own reasons.
fn tax(code: &Option<String>, amount: Option<i64>) -> Result<(), ValidationError> {
    if code.as_deref().is_some_and(|code| {
        let bytes = code.as_bytes();
        !(1..=40).contains(&bytes.len())
            || !(bytes[0].is_ascii_uppercase() || bytes[0].is_ascii_digit())
            || !bytes
                .iter()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b"_-".contains(b))
    }) {
        return Err(ValidationError("taxCode"));
    }
    if amount.is_some_and(|amount| !(0..=TOTAL_MAX).contains(&amount)) {
        return Err(ValidationError("taxMinor"));
    }
    Ok(())
}
/// What a claim says about tax: nothing, or a code with the supplier's amount.
/// One member without the other is neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimTax<'a> {
    None,
    Entered { code: &'a str, amount: i64 },
    Incomplete,
}
fn claim_tax(code: &Option<String>, amount: Option<i64>) -> ClaimTax<'_> {
    match (code.as_deref(), amount) {
        (None, None) => ClaimTax::None,
        (Some(code), Some(amount)) => ClaimTax::Entered { code, amount },
        _ => ClaimTax::Incomplete,
    }
}
impl Capture {
    pub fn tax(&self) -> ClaimTax<'_> {
        claim_tax(&self.tax_code, self.tax_minor)
    }
}
impl Revise {
    pub fn tax(&self) -> ClaimTax<'_> {
        claim_tax(&self.tax_code, self.tax_minor)
    }
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
        tax(&v.tax_code, v.tax_minor)?;
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
        tax(&v.tax_code, v.tax_minor)?;
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
        for (code, accepted) in [
            ("USD", true),
            ("idr", false),
            ("RUPIAH", false),
            ("", false),
        ] {
            raw["currency"] = json!(code);
            assert_eq!(Capture::parse(&raw).is_ok(), accepted, "{code}");
        }
        raw["currency"] = json!("IDR");
        raw["ownerId"] = json!("other");
        assert!(Capture::parse(&raw).is_err());
        raw.as_object_mut().unwrap().remove("ownerId");
        // Without tax members a claim parses and serialises as before.
        let plain = Capture::parse(&raw).unwrap();
        assert_eq!(plain.tax(), ClaimTax::None);
        let written = serde_json::to_value(&plain).unwrap();
        assert!(written.get("taxCode").is_none() && written.get("taxMinor").is_none());
        let mut second = raw["lines"][0].clone();
        second["lineId"] = json!("00000000-0000-4000-8000-00000000000A");
        raw["lines"].as_array_mut().unwrap().push(second);
        assert!(Capture::parse(&raw).is_err());
    }
    #[test]
    fn a_claim_may_carry_a_tax_code_with_the_suppliers_tax_amount() {
        let base = json!({"operation":"document.capture","orderId":"00000000-0000-4000-8000-000000000001","expectedOrderRevision":2,"supplierId":"00000000-0000-4000-8000-000000000002","vendorReference":"INV-1","documentDate":"2026-11-14","currency":"IDR","declaredTotal":140000,"lines":[{"lineId":"00000000-0000-4000-8000-00000000000a","quantity":20,"unitCost":7000}]});
        let mut taxed = base.clone();
        taxed["taxCode"] = json!("VAT-IN");
        taxed["taxMinor"] = json!(15_400);
        let parsed = Capture::parse(&taxed).unwrap();
        assert_eq!(
            parsed.tax(),
            ClaimTax::Entered {
                code: "VAT-IN",
                amount: 15_400
            }
        );
        // The goods total stays the net of goods; the tax is beside it.
        assert_eq!(parsed.declared_total, 140_000);
        let mut written = serde_json::to_value(&parsed).unwrap();
        written["operation"] = json!("document.capture");
        assert_eq!(written, taxed);
        // A zero amount is an amount.
        taxed["taxMinor"] = json!(0);
        assert_eq!(
            Capture::parse(&taxed).unwrap().tax(),
            ClaimTax::Entered {
                code: "VAT-IN",
                amount: 0
            }
        );
        // One member without the other parses: the refusal has its own reason.
        for key in ["taxCode", "taxMinor"] {
            let mut half = taxed.clone();
            half.as_object_mut().unwrap().remove(key);
            assert_eq!(Capture::parse(&half).unwrap().tax(), ClaimTax::Incomplete);
        }
        for (code, accepted) in [
            ("VAT-IN", true),
            ("V", true),
            ("11_IN", true),
            ("vat", false),
            ("", false),
            ("-VAT", false),
            ("VAT IN", false),
            ("VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV", false),
        ] {
            let mut body = taxed.clone();
            body["taxCode"] = json!(code);
            assert_eq!(Capture::parse(&body).is_ok(), accepted, "{code}");
        }
        for (amount, accepted) in [
            (json!(-1), false),
            (json!(TOTAL_MAX), true),
            (json!(TOTAL_MAX + 1), false),
            (json!(15.4), false),
            (json!("15400"), false),
        ] {
            let mut body = taxed.clone();
            body["taxMinor"] = amount.clone();
            assert_eq!(Capture::parse(&body).is_ok(), accepted, "{amount}");
        }
        // A rate, a computed amount or an account is never a member.
        for key in [
            "taxRate",
            "taxComputedMinor",
            "taxAccountId",
            "priceIncludesTax",
        ] {
            let mut forged = taxed.clone();
            forged[key] = json!(1);
            assert!(Capture::parse(&forged).is_err(), "{key}");
        }
        // A correction takes the same two members.
        let mut revise = json!({"operation":"document.revise","id":"00000000-0000-4000-8000-000000000003","expectedRevision":1,"expectedOrderRevision":2,"documentDate":"2026-11-14","currency":"IDR","declaredTotal":140000,"lines":base["lines"],"reason":"Supplier reissued the document"});
        assert_eq!(Revise::parse(&revise).unwrap().tax(), ClaimTax::None);
        revise["taxCode"] = json!("VAT-IN");
        assert_eq!(Revise::parse(&revise).unwrap().tax(), ClaimTax::Incomplete);
        revise["taxMinor"] = json!(15_399);
        assert_eq!(
            Revise::parse(&revise).unwrap().tax(),
            ClaimTax::Entered {
                code: "VAT-IN",
                amount: 15_399
            }
        );
        revise["taxMinor"] = json!(-1);
        assert!(Revise::parse(&revise).is_err());
    }
}
