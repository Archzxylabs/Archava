//! Explicit managed-company identities; registration does not verify legal status.
use super::{purchasing::ValidationError, valid_code, valid_uuid};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum CompanyCommand {
    #[serde(rename = "company.create_from_organization")]
    FromOrganization {
        code: String,
        #[serde(rename = "organizationId")]
        organization_id: String,
        #[serde(rename = "expectedSourceRevision")]
        expected_source_revision: i32,
        reason: String,
    },
    #[serde(rename = "company.refresh_source")]
    RefreshSource {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i32,
        #[serde(rename = "expectedSourceRevision")]
        expected_source_revision: i32,
        reason: String,
    },
    #[serde(rename = "company.set_active")]
    SetActive {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i32,
        active: bool,
        reason: String,
    },
    /// The company's own statement that it is registered for value-added
    /// tax. The tax id itself is the reviewed snapshot of the source
    /// organization and is not a member here. Nothing is verified.
    #[serde(rename = "company.set_tax_registration")]
    SetTaxRegistration {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i32,
        #[serde(rename = "vatRegistered")]
        vat_registered: bool,
        reason: String,
    },
}

/// Whether a tax id is made of digits and the common separators (space, dot,
/// slash, hyphen), starting and ending with a digit, within the stored bound
/// of 50 characters. This is the only check on a tax id anywhere: no length,
/// checksum or format of any tax authority is known or claimed.
pub fn tax_id_is_digits_and_separators(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 50
        && bytes[0].is_ascii_digit()
        && bytes[bytes.len() - 1].is_ascii_digit()
        && bytes
            .iter()
            .all(|b| b.is_ascii_digit() || matches!(b, b' ' | b'.' | b'/' | b'-'))
}
impl CompanyCommand {
    pub fn operation(&self) -> &'static str {
        match self {
            Self::FromOrganization { .. } => "company.create_from_organization",
            Self::RefreshSource { .. } => "company.refresh_source",
            Self::SetActive { .. } => "company.set_active",
            Self::SetTaxRegistration { .. } => "company.set_tax_registration",
        }
    }
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::RefreshSource { id, .. }
            | Self::SetActive { id, .. }
            | Self::SetTaxRegistration { id, .. } => Some(id),
            _ => None,
        }
    }
    pub fn reason(&self) -> &str {
        match self {
            Self::FromOrganization { reason, .. }
            | Self::RefreshSource { reason, .. }
            | Self::SetActive { reason, .. }
            | Self::SetTaxRegistration { reason, .. } => reason,
        }
    }
    pub fn expected_source(&self) -> Option<i32> {
        match self {
            Self::FromOrganization {
                expected_source_revision,
                ..
            }
            | Self::RefreshSource {
                expected_source_revision,
                ..
            } => Some(*expected_source_revision),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct CompanyMutation {
    pub request_id: String,
    pub command: CompanyCommand,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Wire {
    request_id: String,
    #[serde(rename = "type")]
    operation: String,
    data: serde_json::Map<String, Value>,
}
pub fn parse(raw: &Value) -> Result<CompanyMutation, ValidationError> {
    let mut wire: Wire =
        serde_json::from_value(raw.clone()).map_err(|_| ValidationError("mutation"))?;
    if !valid_uuid(&wire.request_id) || wire.data.contains_key("type") {
        return Err(ValidationError("requestId"));
    }
    wire.request_id.make_ascii_lowercase();
    for key in ["id", "organizationId"] {
        if let Some(value) = wire.data.get_mut(key) {
            let id = value
                .as_str()
                .filter(|id| valid_uuid(id))
                .ok_or(ValidationError("id"))?;
            *value = Value::String(id.to_ascii_lowercase());
        }
    }
    wire.data
        .insert("type".into(), Value::String(wire.operation));
    let mut command: CompanyCommand =
        serde_json::from_value(Value::Object(wire.data)).map_err(|_| ValidationError("data"))?;
    if command
        .expected_source()
        .is_some_and(|revision| revision < 1)
    {
        return Err(ValidationError("expectedSourceRevision"));
    }
    match &mut command {
        CompanyCommand::FromOrganization { code, reason, .. } => {
            if !valid_code(code) {
                return Err(ValidationError("code"));
            }
            *reason = reason.trim().into();
        }
        CompanyCommand::RefreshSource {
            expected_revision,
            reason,
            ..
        }
        | CompanyCommand::SetActive {
            expected_revision,
            reason,
            ..
        }
        | CompanyCommand::SetTaxRegistration {
            expected_revision,
            reason,
            ..
        } => {
            if !(1..i32::MAX).contains(expected_revision) {
                return Err(ValidationError("expectedRevision"));
            }
            *reason = reason.trim().into();
        }
    }
    if command.reason().is_empty() || command.reason().chars().count() > 2000 {
        return Err(ValidationError("reason"));
    }
    Ok(CompanyMutation {
        request_id: wire.request_id,
        command,
    })
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompanyReceipt {
    pub request_id: String,
    #[serde(rename = "type")]
    pub operation: String,
    pub company_id: String,
    pub revision: i32,
    pub replayed: bool,
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn managed_company_grammar_requires_explicit_review_and_refuses_legal_or_accounting_claims() {
        let id = "01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d";
        for (kind, data) in [
            (
                "company.create_from_organization",
                json!({"code":"ACME","organizationId":id,"expectedSourceRevision":1,"reason":"Register our managed company"}),
            ),
            (
                "company.refresh_source",
                json!({"id":id,"expectedRevision":1,"expectedSourceRevision":2,"reason":"Review changed profile"}),
            ),
            (
                "company.set_active",
                json!({"id":id,"expectedRevision":1,"active":false,"reason":"Stop using this profile"}),
            ),
            (
                "company.set_tax_registration",
                json!({"id":id,"expectedRevision":1,"vatRegistered":true,"reason":"The company states it is registered"}),
            ),
        ] {
            let raw = json!({"requestId":id,"type":kind,"data":data});
            assert!(parse(&raw).is_ok());
            for key in [
                "name",
                "legalName",
                "verified",
                "accountingMode",
                "currency",
                "ownerId",
                "permissions",
                "partyId",
                "type",
                "taxId",
                "taxRegistrationNumber",
                "taxVerified",
            ] {
                let mut b = raw.clone();
                b["data"][key] = json!("forged");
                assert!(parse(&b).is_err());
            }
            for reason in [json!(""), json!("  "), json!(null)] {
                let mut b = raw.clone();
                b["data"]["reason"] = reason;
                assert!(parse(&b).is_err());
            }
            let key = if data.get("expectedSourceRevision").is_some() {
                "expectedSourceRevision"
            } else {
                "expectedRevision"
            };
            for revision in [
                json!(0),
                json!(-1),
                json!(1.5),
                json!(2147483648i64),
                json!("1"),
            ] {
                let mut b = raw.clone();
                b["data"][key] = revision;
                assert!(parse(&b).is_err());
            }
        }
    }
    #[test]
    fn managed_company_identity_and_code_are_strict_and_canonical() {
        let upper = "01933DF1-8D5E-783F-9B1E-1D3F9A2B3C4D";
        let raw = json!({"requestId":upper,"type":"company.create_from_organization","data":{"code":"ACME","organizationId":upper,"expectedSourceRevision":1,"reason":" Review "}});
        let parsed = parse(&raw).unwrap();
        assert_eq!(parsed.request_id, upper.to_ascii_lowercase());
        assert_eq!(parsed.command.reason(), "Review");
        for code in ["", "acme", "AC ME", "-ACME"] {
            let mut b = raw.clone();
            b["data"]["code"] = json!(code);
            assert!(parse(&b).is_err());
        }
        let mut b = raw;
        b["data"]["organizationId"] = json!("missing");
        assert!(parse(&b).is_err());
    }
    #[test]
    fn a_tax_registration_statement_is_a_boolean_and_a_tax_id_is_only_digits_and_separators() {
        let id = "01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d";
        let raw = json!({"requestId":id,"type":"company.set_tax_registration","data":{"id":id,"expectedRevision":3,"vatRegistered":false,"reason":" No longer registered "}});
        let parsed = parse(&raw).unwrap();
        assert_eq!(parsed.command.operation(), "company.set_tax_registration");
        assert_eq!(parsed.command.reason(), "No longer registered");
        assert_eq!(parsed.command.expected_source(), None);
        for invalid in [json!("true"), json!(1), json!(null)] {
            let mut b = raw.clone();
            b["data"]["vatRegistered"] = invalid;
            assert!(parse(&b).is_err());
        }
        let mut b = raw;
        b["data"].as_object_mut().unwrap().remove("vatRegistered");
        assert!(parse(&b).is_err());
        for valid in [
            "7",
            "0123456789012345",
            "01.234.567.8-999.000",
            "12 345 678/0001-90",
        ] {
            assert!(tax_id_is_digits_and_separators(valid), "{valid}");
        }
        let long = "1".repeat(51);
        for invalid in [
            "",
            " ",
            "DECLARED-A",
            "ID123",
            "-123",
            "123-",
            " 123",
            "12_3",
            "12,3",
            "１２３",
            long.as_str(),
        ] {
            assert!(!tax_id_is_digits_and_separators(invalid), "{invalid}");
        }
    }
}
