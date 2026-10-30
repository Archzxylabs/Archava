//! Retrieval orchestration — port of `packages/knowledge/src/retrieval.ts`.
//!
//! This is where two rules from the PRD meet for one query:
//!
//! - §17 — the structured-truth / retrieval split, decided *before* retrieval;
//! - §23 — tenant scope, non-negotiable on every read.
//!
//! On masking (§16) — the part a reader will reasonably wonder about here: this
//! module does NOT chunk-text-mask, and that is deliberate. Chunks are
//! tenant-authored, deliberately published content; a FAQ that says "email
//! reservations@acme.test" has to survive retrieval verbatim or the assistant
//! cannot tell anyone how to contact the business. The masker in the [`mask`]
//! module works on *field names*, which a chunk does not have — applying it to
//! the text would be a no-op pretending to be a control.
//!
//! What §16 actually protects is customer and session data, and that is masked
//! at the boundary where it enters the turn — the same seam as everywhere else.
//! The split is: retrieval supplies *tenant business* content; the turn context
//! supplies *customer* context, and only the latter carries PII. Merging the
//! two would both weaken the real control and corrupt the published content.
//!
//! [`mask`]: crate::mask

use serde::Serialize;

use super::schema::{is_slug, KnowledgeKind, RetrievalQuery, DEFAULT_RETRIEVAL_LIMIT};
use super::store::{KnowledgeStore, ScoredChunk};
use super::structured_truth::{classify_knowledge_need, KnowledgeNeed};

/// A chunk prepared for a provider: content plus provenance, nothing else.
///
/// Score and ids-without-content do not cross this boundary: a provider gets
/// what it needs to quote a source, and nothing about how the corpus ranked it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KnowledgeContextChunk {
    #[serde(rename = "sourceId")]
    pub source_id: String,
    #[serde(rename = "sourceTitle")]
    pub source_title: String,
    #[serde(rename = "sourceKind")]
    pub source_kind: KnowledgeKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RetrievalContextRequest {
    pub question: String,
    pub tenant_id: String,
    pub limit: Option<u32>,
    pub kinds: Option<Vec<KnowledgeKind>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RetrievalPlan {
    pub need: KnowledgeNeed,
    pub subjects: Vec<super::structured_truth::StructuredTruthSubject>,
    pub query: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RetrievalOutcome {
    pub plan: RetrievalPlan,
    /// Chunks to ground the answer in — tenant-scoped, provenance intact.
    pub context: Vec<KnowledgeContextChunk>,
    /// True when the answer must come from a live system. Retrieval still
    /// returns supporting copy, but the caller must not quote it as the answer.
    pub defer_to_structured_truth: bool,
}

/// The retrieval refusal: an invalid query is refused before it reaches the
/// store, because an unparsed request is how an empty tenantId gets there.
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeRetrievalError(String);

impl std::fmt::Display for KnowledgeRetrievalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Invalid retrieval query: {}", self.0)
    }
}

impl std::error::Error for KnowledgeRetrievalError {}

/// Normalise and validate a retrieval query.
///
/// Parsing here rather than at each call site matters for §23: an unparsed
/// request is how an empty or malformed tenantId reaches the store.
pub fn parse_retrieval_query(
    request: &RetrievalContextRequest,
) -> Result<RetrievalQuery, KnowledgeRetrievalError> {
    let mut issues: Vec<String> = Vec::new();
    if !is_slug(&request.tenant_id) {
        issues.push("tenantId: must be a lowercase kebab-case slug".to_string());
    }
    if request.question.is_empty() {
        issues.push("query: must be a non-empty string".to_string());
    }
    if let Some(limit) = request.limit {
        if limit == 0 || limit > 100 {
            issues.push("limit: must be an integer between 1 and 100".to_string());
        }
    }
    if let Some(kinds) = &request.kinds {
        if kinds.is_empty() {
            issues.push("kinds: must contain at least one kind".to_string());
        }
    }
    if !issues.is_empty() {
        return Err(KnowledgeRetrievalError(issues.join("; ")));
    }
    Ok(RetrievalQuery {
        tenant_id: request.tenant_id.clone(),
        query: request.question.clone(),
        limit: request.limit.unwrap_or(DEFAULT_RETRIEVAL_LIMIT),
        kinds: request.kinds.clone(),
    })
}

/// Decide what a question needs before touching the store.
pub fn plan_retrieval(
    request: &RetrievalContextRequest,
) -> Result<RetrievalPlan, KnowledgeRetrievalError> {
    let query = parse_retrieval_query(request)?;
    let classification = classify_knowledge_need(&query.query);
    Ok(RetrievalPlan {
        need: classification.need,
        subjects: classification.subjects,
        query: query.query,
    })
}

/// Shape stored chunks into provider-facing context. Content is not rewritten.
pub fn to_context_chunks(chunks: &[ScoredChunk]) -> Vec<KnowledgeContextChunk> {
    chunks
        .iter()
        .map(|chunk| KnowledgeContextChunk {
            source_id: chunk.provenance.source_id.clone(),
            source_title: chunk.provenance.source_title.clone(),
            source_kind: chunk.provenance.source_kind,
            text: chunk.text.clone(),
        })
        .collect()
}

/// Retrieve context for a question. The store enforces tenant scope; this layer
/// decides what the answer needs and prepares what crosses the boundary.
pub fn retrieve_context(
    request: &RetrievalContextRequest,
    store: &KnowledgeStore,
) -> Result<RetrievalOutcome, KnowledgeRetrievalError> {
    let query = parse_retrieval_query(request)?;
    let plan = plan_retrieval(request)?;

    let hits = store
        .search(
            &query.query,
            &query.tenant_id,
            query.limit,
            &super::store::SearchOptions {
                min_score: None,
                kinds: query.kinds.clone(),
            },
        )
        .map_err(|error| KnowledgeRetrievalError(error.to_string()))?;

    Ok(RetrievalOutcome {
        context: to_context_chunks(&hits),
        defer_to_structured_truth: plan.need == KnowledgeNeed::StructuredTruth,
        plan,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_limit_defaults_to_eight() {
        let request = RetrievalContextRequest {
            question: "what is your cancellation policy".to_string(),
            tenant_id: "acme".to_string(),
            limit: None,
            kinds: None,
        };
        let parsed = parse_retrieval_query(&request).expect("a valid query");
        assert_eq!(parsed.limit, 8);
    }

    #[test]
    fn an_unknown_tenant_shape_refuses_before_the_store() {
        let request = RetrievalContextRequest {
            question: "anything".to_string(),
            tenant_id: "".to_string(),
            limit: None,
            kinds: None,
        };
        assert!(parse_retrieval_query(&request).is_err());
    }
}
