//! Progressive Disclosure Tiers (L0 / L1 / L2) for Ava Orbz.
//! Implements OpenViking's token-conservation breakthrough:
//! Agents inspect L0 abstracts and L1 overviews first; L2 raw payloads
//! are retrieved strictly on demand, saving up to 63% in prompt tokens.

use crate::security::AuthorityTier;
use crate::uri::OrbzUri;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextTier {
    /// L0: High-level abstract (~50 tokens). Used for fast route/intent matching.
    L0Abstract,
    /// L1: Synthesized overview (~500 tokens). Injected into active conversational reasoning.
    L1Overview,
    /// L2: Complete raw payload (JSON/document). Pulled strictly when executing mutations.
    L2RawPayload,
}

/// A self-contained memory node with multi-tiered progressive disclosure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressiveNode {
    pub id: Uuid,
    pub uri: OrbzUri,
    pub min_authority: AuthorityTier,

    /// ~50 token abstract
    pub summary_l0: String,
    /// ~500 token contextual knowledge map
    pub overview_l1: String,
    /// Full on-demand detail document / raw record
    pub detail_l2: Option<serde_json::Value>,
}

impl ProgressiveNode {
    pub fn new(
        id: Uuid,
        uri: OrbzUri,
        min_authority: AuthorityTier,
        summary_l0: impl Into<String>,
        overview_l1: impl Into<String>,
        detail_l2: Option<serde_json::Value>,
    ) -> Self {
        Self {
            id,
            uri,
            min_authority,
            summary_l0: summary_l0.into(),
            overview_l1: overview_l1.into(),
            detail_l2,
        }
    }

    /// Renders text content formatted specifically for the requested context tier.
    pub fn render_tier(&self, tier: ContextTier) -> String {
        match tier {
            ContextTier::L0Abstract => {
                format!("[{}] {}", self.uri.to_uri_string(), self.summary_l0.trim())
            }
            ContextTier::L1Overview => {
                format!(
                    "### URI: {}\n**Summary:** {}\n\n**Overview:**\n{}",
                    self.uri.to_uri_string(),
                    self.summary_l0.trim(),
                    self.overview_l1.trim()
                )
            }
            ContextTier::L2RawPayload => {
                let detail_str = match &self.detail_l2 {
                    Some(val) => serde_json::to_string_pretty(val).unwrap_or_else(|_| "{}".into()),
                    None => "null".into(),
                };
                format!(
                    "### URI: {}\n**Summary:** {}\n**Detail Payload:**\n```json\n{}\n```",
                    self.uri.to_uri_string(),
                    self.summary_l0.trim(),
                    detail_str
                )
            }
        }
    }

    /// Estimated token count (approximated at 4 characters per token).
    pub fn estimated_tokens(&self, tier: ContextTier) -> usize {
        let text = self.render_tier(tier);
        text.len().div_ceil(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::OrbzScope;

    #[test]
    fn test_progressive_rendering_and_token_economy() {
        let uri = OrbzUri {
            tenant_id: "ten_01".into(),
            scope: OrbzScope::Public,
            category: "sop".into(),
            resource_name: "refund.md".into(),
        };

        let node = ProgressiveNode::new(
            Uuid::new_v4(),
            uri,
            AuthorityTier::L1,
            "Customer refund policy allowing 7-day returns with receipt.",
            "Items can be returned within 7 calendar days if unopened and in original packaging. Cash refunds require manager approval.",
            Some(serde_json::json!({
                "eligible_days": 7,
                "requires_receipt": true,
                "exceptions": ["perishables", "clearance"]
            })),
        );

        let l0 = node.render_tier(ContextTier::L0Abstract);
        let l1 = node.render_tier(ContextTier::L1Overview);
        let l2 = node.render_tier(ContextTier::L2RawPayload);

        assert!(
            node.estimated_tokens(ContextTier::L0Abstract)
                < node.estimated_tokens(ContextTier::L1Overview)
        );
        assert!(
            node.estimated_tokens(ContextTier::L1Overview)
                < node.estimated_tokens(ContextTier::L2RawPayload)
        );

        assert!(l0.contains("Customer refund policy"));
        assert!(l1.contains("original packaging"));
        assert!(l2.contains("eligible_days"));
    }
}
