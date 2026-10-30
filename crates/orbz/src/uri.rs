//! Virtual Filesystem URI parser for Ava Orbz (`orbz://`).
//! Port of the OpenViking filesystem metaphor into structured Rust types.

use crate::security::OrbzScope;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrbzUri {
    pub tenant_id: String,
    pub scope: OrbzScope,
    pub category: String,
    pub resource_name: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UriParseError {
    #[error("URI must start with 'orbz://'")]
    InvalidScheme,
    #[error(
        "URI path is incomplete: expected 'orbz://{{tenant}}/{{scope}}/{{category}}/{{resource}}'"
    )]
    IncompletePath,
    #[error("Unrecognized scope in URI: '{0}'")]
    UnrecognizedScope(String),
}

impl OrbzUri {
    /// Parses an `orbz://` URI string into a strongly-typed structure.
    pub fn parse(input: &str) -> Result<Self, UriParseError> {
        let stripped = input
            .strip_prefix("orbz://")
            .ok_or(UriParseError::InvalidScheme)?;
        let segments: Vec<&str> = stripped.split('/').filter(|s| !s.is_empty()).collect();

        if segments.len() < 4 {
            return Err(UriParseError::IncompletePath);
        }

        let tenant_id = segments[0].to_string();
        let scope_str = segments[1];

        let (scope, category_idx) = match scope_str {
            "public" => (OrbzScope::Public, 2),
            "sovereign" => (OrbzScope::Sovereign, 2),
            "department" => {
                if segments.len() < 5 {
                    return Err(UriParseError::IncompletePath);
                }
                (OrbzScope::Department(segments[2].to_string()), 3)
            }
            "entities" => {
                if segments.len() < 6 {
                    return Err(UriParseError::IncompletePath);
                }
                (
                    OrbzScope::Entity {
                        entity_type: segments[2].to_string(),
                        entity_id: segments[3].to_string(),
                    },
                    4,
                )
            }
            other => return Err(UriParseError::UnrecognizedScope(other.to_string())),
        };

        let category = segments[category_idx].to_string();
        let resource_name = segments[category_idx + 1..].join("/");

        Ok(Self {
            tenant_id,
            scope,
            category,
            resource_name,
        })
    }

    /// Renders the canonical `orbz://` string representation.
    pub fn to_uri_string(&self) -> String {
        match &self.scope {
            OrbzScope::Public => {
                format!(
                    "orbz://{}/public/{}/{}",
                    self.tenant_id, self.category, self.resource_name
                )
            }
            OrbzScope::Sovereign => {
                format!(
                    "orbz://{}/sovereign/{}/{}",
                    self.tenant_id, self.category, self.resource_name
                )
            }
            OrbzScope::Department(dept) => {
                format!(
                    "orbz://{}/department/{}/{}/{}",
                    self.tenant_id, dept, self.category, self.resource_name
                )
            }
            OrbzScope::Entity {
                entity_type,
                entity_id,
            } => {
                format!(
                    "orbz://{}/entities/{}/{}/{}/{}",
                    self.tenant_id, entity_type, entity_id, self.category, self.resource_name
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_public_uri() {
        let uri = OrbzUri::parse("orbz://ten_01/public/sop/return_policy.md").expect("valid uri");
        assert_eq!(uri.tenant_id, "ten_01");
        assert_eq!(uri.scope, OrbzScope::Public);
        assert_eq!(uri.category, "sop");
        assert_eq!(uri.resource_name, "return_policy.md");
        assert_eq!(
            uri.to_uri_string(),
            "orbz://ten_01/public/sop/return_policy.md"
        );
    }

    #[test]
    fn test_parse_department_uri() {
        let uri = OrbzUri::parse("orbz://ten_01/department/warehouse/vendors/barokah.json")
            .expect("valid uri");
        assert_eq!(uri.tenant_id, "ten_01");
        assert_eq!(uri.scope, OrbzScope::Department("warehouse".into()));
        assert_eq!(uri.category, "vendors");
        assert_eq!(uri.resource_name, "barokah.json");
    }

    #[test]
    fn test_parse_entity_uri() {
        let uri = OrbzUri::parse("orbz://ten_01/entities/customers/cust_99/profile/habits.md")
            .expect("valid uri");
        assert_eq!(uri.tenant_id, "ten_01");
        assert_eq!(
            uri.scope,
            OrbzScope::Entity {
                entity_type: "customers".into(),
                entity_id: "cust_99".into()
            }
        );
        assert_eq!(uri.category, "profile");
        assert_eq!(uri.resource_name, "habits.md");
    }

    #[test]
    fn test_parse_invalid_scheme() {
        let err = OrbzUri::parse("http://ten_01/public/sop/return_policy.md").unwrap_err();
        assert_eq!(err, UriParseError::InvalidScheme);
    }
}
