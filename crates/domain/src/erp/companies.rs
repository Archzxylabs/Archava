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
}
impl CompanyCommand {
    pub fn operation(&self) -> &'static str {
        match self {
            Self::FromOrganization { .. } => "company.create_from_organization",
            Self::RefreshSource { .. } => "company.refresh_source",
            Self::SetActive { .. } => "company.set_active",
        }
    }
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::RefreshSource { id, .. } | Self::SetActive { id, .. } => Some(id),
            _ => None,
        }
    }
    pub fn reason(&self) -> &str {
        match self {
            Self::FromOrganization { reason, .. }
            | Self::RefreshSource { reason, .. }
            | Self::SetActive { reason, .. } => reason,
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
}
