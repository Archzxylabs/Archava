//! Tenant scoping, mirroring `packages/core/src/tenant.ts`.
//!
//! The TypeScript original states the rule directly (PRD.md §15): cross-tenant
//! access is a *bug*, not a recoverable error. Any read or write that crosses a
//! tenant boundary throws rather than returning empty or partial data.
//!
//! Rust makes the same decision structurally, which is the one place the port is
//! stricter than the baseline rather than equal to it: a [`TenantId`] cannot be
//! constructed from an unvalidated string at all, so an invalid tenant is
//! unrepresentable instead of merely rejected. The baseline's
//! `tenantIdSchema` regex is carried over verbatim.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A validated tenant identifier.
///
/// Constructed only through [`TenantId::new`] or `Deserialize`, both of which
/// enforce the slug rule. There is deliberately no `From<String>`, because that
/// would let an unvalidated value into a type whose entire purpose is that it
/// cannot be invalid.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct TenantId(String);

/// Raised when a tenant identifier does not match the slug rule, or when a
/// value fails to carry the tenant the caller is operating in.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum TenantScopeError {
    #[error("tenantId must be a lowercase kebab-case slug, got {0:?}")]
    InvalidTenantId(String),
    #[error("missing tenant-scoped value; expected tenant \"{expected}\"")]
    Missing { expected: String },
    #[error(
        "tenant scope violation: encountered tenant \"{encountered}\" while operating in tenant \"{expected}\""
    )]
    Violation {
        expected: String,
        encountered: String,
    },
}

impl TenantId {
    /// Validate and brand a tenant identifier.
    ///
    /// Mirrors `tenantIdSchema.parse`. The regex is implemented by hand rather
    /// than pulled in from a `regex` dependency: the rule is small, the crate
    /// cost is not, and a hand-written check cannot drift into accepting
    /// something the contract does not.
    pub fn new(value: impl Into<String>) -> Result<Self, TenantScopeError> {
        let value = value.into();
        if is_valid_slug(&value) {
            Ok(Self(value))
        } else {
            Err(TenantScopeError::InvalidTenantId(value))
        }
    }

    /// The underlying slug.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for TenantId {
    type Error = TenantScopeError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl fmt::Display for TenantId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A value that belongs to exactly one tenant.
///
/// Mirrors `TenantScope`. Carrying the tenant in the type rather than in a
/// side-channel is what lets [`assert_tenant`] be checked at call sites that
/// receive one.
///
/// `Copy` is deliberately absent: `TenantId` owns a `String`, and deriving it
/// here would not compile. Leaving it off also keeps the type honest — cloning a
/// tenant scope allocates, so it cannot be hidden in an unnoticed copy of a hot
/// loop.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TenantScope {
    pub tenant_id: TenantId,
}

/// Implemented by records that are owned by a tenant.
pub trait TenantScoped {
    fn tenant_id(&self) -> &TenantId;
}

impl TenantScoped for TenantScope {
    fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }
}

/// Assert that a value carries the required tenant.
///
/// Mirrors `assertTenant`. The comment in the TypeScript is the contract and is
/// reproduced because it is the reason this returns an error at all: a
/// cross-tenant read is a bug, so the caller must not be able to treat the
/// result as empty data and carry on.
pub fn assert_tenant<T>(value: Option<T>, expected: &TenantId) -> Result<T, TenantScopeError>
where
    T: TenantScoped,
{
    match value {
        None => Err(TenantScopeError::Missing {
            expected: expected.to_string(),
        }),
        Some(value) => {
            let encountered = value.tenant_id();
            if encountered == expected {
                Ok(value)
            } else {
                Err(TenantScopeError::Violation {
                    expected: expected.to_string(),
                    encountered: encountered.to_string(),
                })
            }
        }
    }
}

/// Brand a record as belonging to a tenant. Mirrors `scoped`.
///
/// The baseline spreads `{ ...value, tenantId }`, which produces a plain object
/// whose tenant field is indistinguishable from any other. Here the returned
/// value is a [`TenantScope`], so a record that has been through `scoped` keeps
/// its brand.
pub fn scoped(tenant_id: &TenantId) -> TenantScope {
    TenantScope {
        tenant_id: tenant_id.clone(),
    }
}

/// The slug rule, expressed directly.
///
/// Equivalent to `^[a-z0-9]+(?:-[a-z0-9]+)*$`: one or more lowercase
/// alphanumeric segments joined by single hyphens.
fn is_valid_slug(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|segment| {
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(value: &str) -> TenantId {
        TenantId::new(value).expect("fixture tenant must be valid")
    }

    #[test]
    fn accepts_lowercase_kebab_slugs() {
        assert_eq!(tenant("acme").as_str(), "acme");
        assert_eq!(tenant("acme-hotels").as_str(), "acme-hotels");
        assert_eq!(tenant("acme-2-west").as_str(), "acme-2-west");
    }

    /// Each rejection below is a case the baseline's `tenantIdSchema` regex also
    /// rejects. They are asserted individually because a single "rejects
    /// everything invalid" test would pass even if one form slipped through.
    #[test]
    fn rejects_every_form_the_baseline_regex_rejects() {
        for invalid in [
            "",
            "Acme",
            "ACME",
            "-acme",
            "acme-",
            "acme--hotels",
            "acme hotels",
            "acme.hotels",
            "acme_hotels",
            "café",
            "acme/../other",
        ] {
            assert_eq!(
                TenantId::new(invalid),
                Err(TenantScopeError::InvalidTenantId(invalid.to_string())),
                "expected {invalid:?} to be rejected"
            );
        }
    }

    #[test]
    fn deserialization_enforces_the_same_rule() {
        let parsed: TenantId = serde_json::from_str("\"acme-hotels\"").expect("valid slug");
        assert_eq!(parsed.as_str(), "acme-hotels");

        let rejected: Result<TenantId, _> = serde_json::from_str("\"Acme\"");
        assert!(
            rejected.is_err(),
            "deserialization must not bypass the slug rule"
        );
    }

    #[test]
    fn matching_tenant_passes() {
        let expected = tenant("acme");
        let value = scoped(&expected);
        assert_eq!(
            assert_tenant(Some(value), &expected).map(|scope| scope.tenant_id),
            Ok(expected)
        );
    }

    #[test]
    fn a_missing_value_is_a_bug_not_empty_data() {
        let expected = tenant("acme");
        assert_eq!(
            assert_tenant(None::<TenantScope>, &expected),
            Err(TenantScopeError::Missing {
                expected: "acme".to_string()
            })
        );
    }

    #[test]
    fn a_cross_tenant_value_is_a_violation_not_a_miss() {
        let expected = tenant("acme");
        let encountered = scoped(&tenant("other"));
        assert_eq!(
            assert_tenant(Some(encountered), &expected),
            Err(TenantScopeError::Violation {
                expected: "acme".to_string(),
                encountered: "other".to_string()
            })
        );
    }
}
