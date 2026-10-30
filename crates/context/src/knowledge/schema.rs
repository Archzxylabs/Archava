//! Knowledge schemas (§17, §23) — port of `packages/knowledge/src/schema.ts`.
//!
//! The unit of storage is a *chunk*, not a document: retrieval returns chunks,
//! provenance is recorded per chunk, and tenant scope lives on the chunk so it
//! can never be separated from the bytes it governs. A chunk without its tenant
//! is not a knowledge artefact — it is an isolation bug waiting to happen.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::Value::Object;

pub use super::structured_truth::StructuredTruthSubject;

/// Mirrors `knowledgeSourceSchema.kind` from the client config.
pub const KNOWLEDGE_KINDS: &[KnowledgeKind] = &[
    KnowledgeKind::Faq,
    KnowledgeKind::Documentation,
    KnowledgeKind::Policy,
    KnowledgeKind::Product,
    KnowledgeKind::Service,
    KnowledgeKind::Marketing,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KnowledgeKind {
    Faq,
    Documentation,
    Policy,
    Product,
    Service,
    Marketing,
}

impl KnowledgeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            KnowledgeKind::Faq => "faq",
            KnowledgeKind::Documentation => "documentation",
            KnowledgeKind::Policy => "policy",
            KnowledgeKind::Product => "product",
            KnowledgeKind::Service => "service",
            KnowledgeKind::Marketing => "marketing",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        KNOWLEDGE_KINDS
            .iter()
            .find(|kind| kind.as_str() == value)
            .copied()
    }
}

/// Where a chunk came from. §17: "source provenance stored per chunk/document"
/// and "every answer should be able to preserve source provenance internally
/// for debugging/evals."
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    #[serde(rename = "sourceKind")]
    pub source_kind: KnowledgeKind,
    #[serde(rename = "sourceId")]
    pub source_id: String,
    #[serde(rename = "sourceTitle")]
    pub source_title: String,
    #[serde(rename = "sourceUrl", default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    /// ISO date of the source's own last update, not of ingestion.
    #[serde(rename = "sourceUpdatedAt")]
    pub source_updated_at: String,
    #[serde(rename = "ingestedAt")]
    pub ingested_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    #[serde(rename = "tenantId")]
    pub tenant_id: String,
    pub kind: KnowledgeKind,
    pub title: String,
    pub content: String,
    #[serde(rename = "sourceUrl", default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    #[serde(rename = "tenantId")]
    pub tenant_id: String,
    #[serde(rename = "documentId")]
    pub document_id: String,
    /// Zero-based position inside the document.
    pub ordinal: u32,
    pub text: String,
    pub provenance: Provenance,
}

/// A validated retrieval query: tenant-scoped, bounded, kind-filtered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalQuery {
    #[serde(rename = "tenantId")]
    pub tenant_id: String,
    pub query: String,
    pub limit: u32,
    /// Restrict retrieval to a subset of source kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<KnowledgeKind>>,
}

/// The retrieval limit's default when a caller leaves it out.
pub const DEFAULT_RETRIEVAL_LIMIT: u32 = 8;

/// Relevance in `[0, 1]`. `1.0` — exact lexical match, not a guarantee.
/// Exported from `store`, which owns the scoring function.
pub use super::store::ScoredChunk;

/// Every validation refusal this crate raises with the subject it refused.
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeSchemaError {
    pub subject: String,
    pub issues: Vec<String>,
}

impl KnowledgeSchemaError {
    pub fn issues_line(&self) -> String {
        self.issues.join("; ")
    }
}

impl std::fmt::Display for KnowledgeSchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Invalid {}: {}", self.subject, self.issues.join("; "))
    }
}

impl std::error::Error for KnowledgeSchemaError {}

/// `^[a-z0-9]+(?:-[a-z0-9]+)*$` — the slug shape tenant ids, document ids and
/// chunk ids share.
pub fn is_slug(value: &str) -> bool {
    let mut segments = 0;
    let mut in_segment = false;
    let mut previous_dash = true; // a leading dash is invalid
    for character in value.chars() {
        match character {
            'a'..='z' | '0'..='9' => {
                in_segment = true;
                previous_dash = false;
            }
            '-' => {
                if !in_segment || previous_dash {
                    return false;
                }
                segments += 1;
                previous_dash = true;
                in_segment = false;
            }
            _ => return false,
        }
    }
    in_segment && !(segments > 0 && previous_dash)
}

/// `^\d{4}-\d{2}-\d{2}$` — an ISO date. The digits are not checked against a
/// calendar (matching the baseline's regex, which is a shape contract, not a
/// date parser).
fn is_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        let dash = index == 4 || index == 7;
        if dash {
            if *byte != b'-' {
                return false;
            }
        } else if !byte.is_ascii_digit() {
            return false;
        }
    }
    true
}

/// `^\d{4}-\d{2}-\d{2}T.+` — an ISO instant, printed or offset, prefix-shaped.
fn is_iso_instant(value: &str) -> bool {
    value.len() > 11 && is_iso_date(&value[..10]) && value.as_bytes()[10] == b'T'
}

/// One validation issue, in the baseline's issue format: `path: message`,
/// with `(root)` when the field name is empty.
fn issue(field: &str, _value: &Value, problem: &'static str) -> String {
    let path = if field.is_empty() {
        "(root)".to_string()
    } else {
        field.to_string()
    };
    format!("{path}: {problem}")
}

/// Validate and build a document record from untrusted JSON.
pub fn parse_knowledge_document(raw: &Value) -> Result<Document, KnowledgeSchemaError> {
    let Object(map) = raw else {
        return Err(KnowledgeSchemaError {
            subject: "document".to_string(),
            issues: vec!["(root): must be an object".to_string()],
        });
    };
    let mut issues = Vec::new();
    let id = string_field(map.get("id"));
    let tenant_id = string_field(map.get("tenantId"));
    let title = string_field(map.get("title"));
    let content = string_field(map.get("content"));
    let updated_at = string_field(map.get("updatedAt"));
    // `z.string().optional()` accepts any string when present, including "".
    let source_url = match map.get("sourceUrl") {
        None | Some(Value::Null) => None,
        Some(Value::String(url)) => Some(url.clone()),
        Some(_) => None,
    };
    for (name, value) in [
        ("id", &id),
        ("tenantId", &tenant_id),
        ("title", &title),
        ("content", &content),
        ("updatedAt", &updated_at),
    ] {
        if value.is_empty() {
            issues.push(issue(
                name,
                map.get(name).unwrap_or(&Value::Null),
                "must be a non-empty string",
            ));
        }
    }
    if !is_slug(&id) {
        issues.push(issue(
            "id",
            &Value::String(id.clone()),
            "must be a lowercase kebab-case slug",
        ));
    }
    if !is_slug(&tenant_id) {
        issues.push(issue(
            "tenantId",
            &Value::String(tenant_id.clone()),
            "must be a lowercase kebab-case slug",
        ));
    }
    if !is_iso_date(&updated_at) {
        issues.push(issue(
            "updatedAt",
            &Value::String(updated_at.clone()),
            "must be an ISO YYYY-MM-DD date",
        ));
    }
    let kind = match KnowledgeKind::parse(kind_field(map.get("kind"))) {
        Some(kind) => kind,
        None => {
            issues.push(issue(
                "kind",
                map.get("kind").unwrap_or(&Value::Null),
                "must be one of faq, documentation, policy, product, service, marketing",
            ));
            KnowledgeKind::Faq
        }
    };
    if !issues.is_empty() {
        return Err(KnowledgeSchemaError {
            subject: "document".to_string(),
            issues,
        });
    }
    Ok(Document {
        id,
        tenant_id,
        kind,
        title,
        content,
        source_url,
        updated_at,
    })
}

fn kind_field(value: Option<&Value>) -> &str {
    match value {
        Some(Value::String(kind)) => kind,
        _ => "",
    }
}

fn string_field(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        _ => String::new(),
    }
}

/// Validate and build a chunk record from untrusted JSON.
pub fn parse_knowledge_chunk(raw: &Value) -> Result<Chunk, KnowledgeSchemaError> {
    let Value::Object(map) = raw else {
        return Err(KnowledgeSchemaError {
            subject: "chunk".to_string(),
            issues: vec!["(root): must be an object".to_string()],
        });
    };
    let mut issues = Vec::new();
    let id = string_field(map.get("id"));
    let tenant_id = string_field(map.get("tenantId"));
    let document_id = string_field(map.get("documentId"));
    let text = string_field(map.get("text"));
    for (name, value) in [
        ("id", &id),
        ("tenantId", &tenant_id),
        ("documentId", &document_id),
    ] {
        if value.is_empty() {
            issues.push(issue(
                name,
                map.get(name).unwrap_or(&Value::Null),
                "must be a non-empty string",
            ));
        }
        if !is_slug(value) {
            issues.push(issue(
                name,
                &Value::String(value.clone()),
                "must be a lowercase kebab-case slug",
            ));
        }
    }
    if text.is_empty() {
        issues.push(issue(
            "text",
            map.get("text").unwrap_or(&Value::Null),
            "must be a non-empty string",
        ));
    }
    let ordinal = match map.get("ordinal") {
        Some(Value::Number(number)) if number.as_f64().unwrap_or_default().fract() == 0.0 => {
            let ordinal = number.as_f64().unwrap_or_default();
            if ordinal < 0.0 {
                issues.push(issue(
                    "ordinal",
                    map.get("ordinal").unwrap_or(&Value::Null),
                    "must be a non-negative integer",
                ));
            }
            ordinal as u32
        }
        Some(_) => {
            issues.push(issue(
                "ordinal",
                map.get("ordinal").unwrap_or(&Value::Null),
                "must be a non-negative integer",
            ));
            0
        }
        None => {
            issues.push(issue(
                "ordinal",
                map.get("ordinal").unwrap_or(&Value::Null),
                "must be a non-negative integer",
            ));
            0
        }
    };

    // Provenance is validated by the same rules the fields above use.
    let mut provenance_raw = None;
    if let Some(Object(provenance_map)) = map.get("provenance") {
        let mut provenance_issues = Vec::new();
        let kind = KnowledgeKind::parse(kind_field(provenance_map.get("sourceKind")));
        if kind.is_none() {
            provenance_issues.push(issue(
                "provenance.sourceKind",
                provenance_map.get("sourceKind").unwrap_or(&Value::Null),
                "must be one of faq, documentation, policy, product, service, marketing",
            ));
        }
        let source_id = string_field(provenance_map.get("sourceId"));
        if !is_slug(&source_id) {
            provenance_issues.push(issue(
                "provenance.sourceId",
                &Value::String(source_id.clone()),
                "must be a lowercase kebab-case slug",
            ));
        }
        let source_title = string_field(provenance_map.get("sourceTitle"));
        if source_title.is_empty() {
            provenance_issues.push(issue(
                "provenance.sourceTitle",
                provenance_map.get("sourceTitle").unwrap_or(&Value::Null),
                "must be a non-empty string",
            ));
        }
        let source_updated_at = string_field(provenance_map.get("sourceUpdatedAt"));
        if !is_iso_date(&source_updated_at) {
            provenance_issues.push(issue(
                "provenance.sourceUpdatedAt",
                &Value::String(source_updated_at.clone()),
                "must be an ISO YYYY-MM-DD date",
            ));
        }
        let ingested_at = string_field(provenance_map.get("ingestedAt"));
        if !is_iso_instant(&ingested_at) {
            provenance_issues.push(issue(
                "provenance.ingestedAt",
                &Value::String(ingested_at.clone()),
                "must be an ISO timestamp",
            ));
        }
        let source_url = match provenance_map.get("sourceUrl") {
            None | Some(Value::Null) => None,
            Some(Value::String(url)) => Some(url.clone()),
            Some(_) => None,
        };
        if !provenance_issues.is_empty() {
            issues.extend(provenance_issues);
        }
        provenance_raw = Some((
            kind,
            source_id,
            source_title,
            source_url,
            source_updated_at,
            ingested_at,
        ));
    } else {
        issues.push(issue(
            "provenance",
            map.get("provenance").unwrap_or(&Value::Null),
            "must be an object with sourceKind, sourceId, sourceTitle, sourceUpdatedAt and ingestedAt",
        ));
    }

    if !issues.is_empty() {
        return Err(KnowledgeSchemaError {
            subject: "chunk".to_string(),
            issues,
        });
    }

    let provenance = provenance_raw.unwrap();
    Ok(Chunk {
        id,
        tenant_id,
        document_id,
        ordinal,
        text,
        provenance: Provenance {
            source_kind: (provenance.0).unwrap_or(KnowledgeKind::Policy),
            source_id: provenance.1,
            source_title: provenance.2,
            source_url: provenance.3,
            source_updated_at: provenance.4,
            ingested_at: provenance.5,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_accept_their_shape_and_refuse_others() {
        assert!(is_slug("deluxe-suite"));
        assert!(is_slug("a"));
        assert!(!is_slug("Deluxe"));
        assert!(!is_slug("deluxe--suite"));
        assert!(!is_slug("-deluxe"));
        assert!(!is_slug("deluxe-"));
        assert!(!is_slug("deluxe suite"));
    }

    #[test]
    fn dates_check_the_shape_not_the_calendar() {
        assert!(is_iso_date("2026-02-30"));
        assert!(!is_iso_date("2026-2-30"));
        assert!(is_iso_instant("2026-02-13T00:00:00.000Z"));
        assert!(!is_iso_instant("2026-02-13 00:00:00"));
    }
}
