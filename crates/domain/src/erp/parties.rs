//! Reviewed business-party identities. Source identity and business roles are distinct.
use super::{purchasing::ValidationError, valid_uuid};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum PartyCommand {
    #[serde(rename = "party.create_from_contact")]
    FromContact {
        #[serde(rename = "contactId")]
        contact_id: String,
        #[serde(rename = "expectedSourceRevision")]
        expected_source_revision: i32,
        customer: bool,
        supplier: bool,
    },
    #[serde(rename = "party.create_from_organization")]
    FromOrganization {
        #[serde(rename = "organizationId")]
        organization_id: String,
        #[serde(rename = "expectedSourceRevision")]
        expected_source_revision: i32,
        customer: bool,
        supplier: bool,
    },
    #[serde(rename = "party.set_roles")]
    SetRoles {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i32,
        customer: bool,
        supplier: bool,
        reason: String,
    },
    #[serde(rename = "party.refresh_source")]
    RefreshSource {
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i32,
        #[serde(rename = "expectedSourceRevision")]
        expected_source_revision: i32,
        reason: String,
    },
}
impl PartyCommand {
    pub fn operation(&self) -> &'static str {
        match self {
            Self::FromContact { .. } => "party.create_from_contact",
            Self::FromOrganization { .. } => "party.create_from_organization",
            Self::SetRoles { .. } => "party.set_roles",
            Self::RefreshSource { .. } => "party.refresh_source",
        }
    }
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::SetRoles { id, .. } | Self::RefreshSource { id, .. } => Some(id),
            _ => None,
        }
    }
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::SetRoles { reason, .. } | Self::RefreshSource { reason, .. } => Some(reason),
            _ => None,
        }
    }
    pub fn expected_source(&self) -> Option<i32> {
        match self {
            Self::FromContact {
                expected_source_revision,
                ..
            }
            | Self::FromOrganization {
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
pub struct PartyMutation {
    pub request_id: String,
    pub command: PartyCommand,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Wire {
    request_id: String,
    #[serde(rename = "type")]
    operation: String,
    data: serde_json::Map<String, Value>,
}
pub fn parse(raw: &Value) -> Result<PartyMutation, ValidationError> {
    let mut wire: Wire =
        serde_json::from_value(raw.clone()).map_err(|_| ValidationError("mutation"))?;
    if !valid_uuid(&wire.request_id) || wire.data.contains_key("type") {
        return Err(ValidationError("requestId"));
    }
    wire.request_id.make_ascii_lowercase();
    for key in ["id", "contactId", "organizationId"] {
        if let Some(value) = wire.data.get_mut(key) {
            if !value.as_str().is_some_and(valid_uuid) {
                return Err(ValidationError("id"));
            }
            if let Some(id) = value.as_str() {
                *value = Value::String(id.to_ascii_lowercase());
            }
        }
    }
    wire.data
        .insert("type".into(), Value::String(wire.operation));
    let mut command: PartyCommand =
        serde_json::from_value(Value::Object(wire.data)).map_err(|_| ValidationError("data"))?;
    if command.expected_source().is_some_and(|r| r < 1) {
        return Err(ValidationError("expectedSourceRevision"));
    }
    match &mut command {
        PartyCommand::FromContact {
            customer, supplier, ..
        }
        | PartyCommand::FromOrganization {
            customer, supplier, ..
        } if !*customer && !*supplier => return Err(ValidationError("roles")),
        PartyCommand::SetRoles {
            expected_revision,
            reason,
            ..
        }
        | PartyCommand::RefreshSource {
            expected_revision,
            reason,
            ..
        } => {
            if *expected_revision < 1 || *expected_revision == i32::MAX {
                return Err(ValidationError("expectedRevision"));
            }
            *reason = reason.trim().into();
            if reason.is_empty() || reason.chars().count() > 2000 {
                return Err(ValidationError("reason"));
            }
        }
        _ => {}
    }
    Ok(PartyMutation {
        request_id: wire.request_id,
        command,
    })
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PartyReceipt {
    pub request_id: String,
    #[serde(rename = "type")]
    pub operation: String,
    pub party_id: String,
    pub revision: i32,
    pub replayed: bool,
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn reviewed_party_grammar_refuses_forged_sources_roles_and_revisions() {
        let id = "01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d";
        for (kind, data) in [
            (
                "party.create_from_contact",
                json!({"contactId":id,"expectedSourceRevision":1,"customer":true,"supplier":true}),
            ),
            (
                "party.create_from_organization",
                json!({"organizationId":id,"expectedSourceRevision":1,"customer":true,"supplier":false}),
            ),
            (
                "party.set_roles",
                json!({"id":id,"expectedRevision":1,"customer":false,"supplier":false,"reason":"Stop both roles"}),
            ),
            (
                "party.refresh_source",
                json!({"id":id,"expectedRevision":1,"expectedSourceRevision":2,"reason":"Reviewed name"}),
            ),
        ] {
            let input = json!({"requestId":id,"type":kind,"data":data});
            assert!(parse(&input).is_ok());
            for key in ["name", "ownerId", "permissions", "sourceKind", "type"] {
                let mut forged = input.clone();
                forged["data"][key] = json!("forged");
                assert!(parse(&forged).is_err());
            }
            for value in [
                json!(0),
                json!(-1),
                json!(1.5),
                json!(2147483648i64),
                json!("1"),
            ] {
                let mut invalid = input.clone();
                let key = if data.get("expectedSourceRevision").is_some() {
                    "expectedSourceRevision"
                } else {
                    "expectedRevision"
                };
                invalid["data"][key] = value;
                assert!(parse(&invalid).is_err());
            }
        }
    }
    #[test]
    fn new_party_requires_a_business_role() {
        assert!(parse(&json!({"requestId":"01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d","type":"party.create_from_contact","data":{"contactId":"01933df1-8d5e-783f-9b1e-1d3f9a2b3c4d","expectedSourceRevision":1,"customer":false,"supplier":false}})).is_err());
    }
}
