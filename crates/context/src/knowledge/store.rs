//! The tenant-scoped knowledge store — port of `packages/knowledge/src/store.ts`.
//!
//! §23 is categorical: "No cross-tenant retrieval is permitted." That is
//! enforced by construction rather than by callers remembering to pass a
//! filter:
//!
//! 1. Every chunk carries its `tenantId`, and every read opens with an explicit
//!    `tenantId`. A cross-tenant read throws — it is a bug, not an empty result,
//!    because silently returning `[]` hides the bug that caused it.
//! 2. The index is partitioned per tenant, so a query can only ever see its own
//!    partition. There is no cross-partition code path to get wrong.
//! 3. Reads refuse a tenant with no documents, so a typo'd tenant id cannot be
//!    confused with "no results".

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::schema::{parse_knowledge_document, Chunk, Document, KnowledgeKind};
use super::text::{chunk_text, stem, tokenise_for_scoring};
use archava_core::TenantScopeError;

/// The per-tenant isolation refusal ("not found" and "not yours" are different
/// facts and must not collapse).
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeIsolationError(String);

impl std::fmt::Display for KnowledgeIsolationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for KnowledgeIsolationError {}

/// The store's own tenant-scope refusal.
///
/// The baseline raises these as `TenantScopeError` (the free-form message class
/// of `packages/core/src/tenant.ts`); the Rust core's `TenantScopeError` is an
/// enum whose variants pin *its* messages, and this store's refusals carry a
/// different one (the list of registered tenants). It stays the same kind of
/// refusal — a bug, not empty data — under a name that can carry that text.
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeTenantError(String);

impl std::fmt::Display for KnowledgeTenantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for KnowledgeTenantError {}

/// The per-document record: the document plus the chunk ids it owns, so an
/// ingest that replaces it can remove exactly its chunks.
#[derive(Debug, Clone)]
struct DocumentRecord {
    tenant_id: String,
    document: Document,
    chunk_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeStoreStats {
    pub documents: usize,
    pub chunks: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    /// Minimum relevance to return. Defaults to 0.01 — nothing in common.
    pub min_score: Option<f64>,
    /// Restrict retrieval to a subset of source kinds.
    pub kinds: Option<Vec<KnowledgeKind>>,
}

/// The default relevance floor when a caller leaves it out.
pub const DEFAULT_MIN_SCORE: f64 = 0.01;

/// A chunk with its relevance attached. Retrieved chunks are the store's
/// purpose: a chunk is provenance on a text, and its score says how relevant
/// that text was to one query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoredChunk {
    pub id: String,
    #[serde(rename = "tenantId")]
    pub tenant_id: String,
    #[serde(rename = "documentId")]
    pub document_id: String,
    pub ordinal: u32,
    pub text: String,
    pub provenance: super::schema::Provenance,
    pub score: f64,
}

impl ScoredChunk {
    fn from_chunk(chunk: Chunk, score: f64) -> Self {
        ScoredChunk {
            id: chunk.id,
            tenant_id: chunk.tenant_id,
            document_id: chunk.document_id,
            ordinal: chunk.ordinal,
            text: chunk.text,
            provenance: chunk.provenance,
            score,
        }
    }
}

/// The tenant knowledge store.
///
/// `partitions` are per-tenant: a tenant appears only once it has a document.
/// `documents` are keyed `tenant/document`, which is what makes an id whose
/// tenant changed a *replacement of the same key*, not two documents.
#[derive(Default)]
pub struct KnowledgeStore {
    partitions: HashMap<String, HashMap<String, Chunk>>,
    documents: HashMap<String, DocumentRecord>,
}

impl std::fmt::Debug for KnowledgeStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KnowledgeStore")
            .field("tenants", &self.partitions.len())
            .field("documents", &self.documents.len())
            .finish()
    }
}

impl KnowledgeStore {
    /// Ingest a document, replacing any earlier document with the same id.
    pub fn ingest(
        &mut self,
        raw: &serde_json::Value,
    ) -> Result<Vec<Chunk>, super::KnowledgeSchemaError> {
        let document = parse_knowledge_document(raw)?;
        let document_key = format!("{}/{}", document.tenant_id, document.id);

        if let Some(previous) = self.documents.get(&document_key) {
            if let Some(partition) = self.partitions.get_mut(&document.tenant_id) {
                for chunk_id in &previous.chunk_ids {
                    partition.remove(chunk_id);
                }
            }
        }

        let chunks = chunks_of_document(&document);
        for chunk in &chunks {
            self.partitions
                .entry(document.tenant_id.clone())
                .or_default()
                .insert(chunk.id.clone(), chunk.clone());
        }
        self.documents.insert(
            document_key,
            DocumentRecord {
                tenant_id: document.tenant_id.clone(),
                chunk_ids: chunks.iter().map(|chunk| chunk.id.clone()).collect(),
                document: document.clone(),
            },
        );

        Ok(chunks)
    }

    /// Every chunk of one document, in document order.
    pub fn chunks_of(
        &self,
        tenant_id: &str,
        document_id: &str,
    ) -> Result<Vec<Chunk>, KnowledgeTenantError> {
        self.assert_known_tenant(tenant_id)?;
        let key = format!("{tenant_id}/{document_id}");
        let Some(record) = self.documents.get(&key) else {
            return Ok(Vec::new());
        };
        let partition = self
            .partitions
            .get(tenant_id)
            .expect("known tenants always have a partition");
        Ok(record
            .chunk_ids
            .iter()
            .filter_map(|chunk_id| partition.get(chunk_id))
            .cloned()
            .collect())
    }

    /// Fetch one chunk. Throws on a cross-tenant id rather than returning a
    /// miss: "not found" and "not yours" are different facts (§23).
    pub fn chunk(&self, tenant_id: &str, chunk_id: &str) -> Result<Chunk, KnowledgeIsolationError> {
        for (candidate_tenant, partition) in &self.partitions {
            let Some(chunk) = partition.get(chunk_id) else {
                continue;
            };
            if candidate_tenant != tenant_id {
                return Err(KnowledgeIsolationError(format!(
                    "Tenant scope violation: chunk \"{}\" belongs to tenant \"{}\", not \"{}\".",
                    chunk_id, candidate_tenant, tenant_id
                )));
            }
            return Ok(chunk.clone());
        }
        Err(KnowledgeIsolationError(format!(
            "Chunk \"{}\" does not exist in tenant \"{}\".",
            chunk_id, tenant_id
        )))
    }

    /// Relevance-ranked retrieval. Only ever reads the caller's partition.
    pub fn search(
        &self,
        query: &str,
        tenant_id: &str,
        limit: u32,
        options: &SearchOptions,
    ) -> Result<Vec<ScoredChunk>, KnowledgeTenantError> {
        self.assert_known_tenant(tenant_id)?;
        let min_score = options.min_score.unwrap_or(DEFAULT_MIN_SCORE);

        let mut scored: Vec<ScoredChunk> = Vec::new();
        if let Some(partition) = self.partitions.get(tenant_id) {
            for chunk in partition.values() {
                let allowed = options
                    .kinds
                    .as_ref()
                    .is_none_or(|allowed| allowed.contains(&chunk.provenance.source_kind));
                if !allowed {
                    continue;
                }
                let score = score_chunk(query, &chunk.text);
                if score >= min_score {
                    scored.push(ScoredChunk::from_chunk(chunk.clone(), score));
                }
            }
        }
        scored.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.id.cmp(&right.id))
        });
        scored.truncate(limit as usize);
        Ok(scored)
    }

    /// The document record itself, after `assertTenant` ran — the same two
    /// refusals the core's contract raises: "missing" for an unknown key, and
    /// a violation if the record under this key carried another tenant.
    pub fn document(
        &self,
        tenant_id: &str,
        document_id: &str,
    ) -> Result<&Document, TenantScopeError> {
        let key = format!("{tenant_id}/{document_id}");
        match self.documents.get(&key) {
            Some(record) => {
                if record.tenant_id != tenant_id {
                    return Err(TenantScopeError::Violation {
                        expected: tenant_id.to_string(),
                        encountered: record.tenant_id.clone(),
                    });
                }
                Ok(&record.document)
            }
            None => Err(TenantScopeError::Missing {
                expected: tenant_id.to_string(),
            }),
        }
    }

    /// The tenant's documents, in key order.
    pub fn list_documents(&self, tenant_id: &str) -> Result<Vec<Document>, KnowledgeTenantError> {
        self.assert_known_tenant(tenant_id)?;
        let mut documents: Vec<&Document> = self
            .documents
            .values()
            .filter(|record| record.tenant_id == tenant_id)
            .map(|record| &record.document)
            .collect();
        documents.sort_by(|left, right| {
            left.tenant_id
                .cmp(&right.tenant_id)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(documents.into_iter().cloned().collect())
    }

    /// How much of the tenant's corpus is here, for a status read.
    pub fn stats(&self, tenant_id: &str) -> Result<KnowledgeStoreStats, KnowledgeTenantError> {
        self.assert_known_tenant(tenant_id)?;
        let chunks = self.partitions.get(tenant_id).map_or(0, |p| p.len());
        let documents = self
            .documents
            .values()
            .filter(|record| record.tenant_id == tenant_id)
            .count();
        Ok(KnowledgeStoreStats { documents, chunks })
    }

    /// The tenants that have ingested anything, in order.
    pub fn tenants(&self) -> Vec<String> {
        let mut tenants: Vec<String> = self.partitions.keys().cloned().collect();
        tenants.sort();
        tenants
    }

    /// A tenant with no documents is almost certainly a typo or a missing
    /// ingest, and reporting it as "no results" lets that ship.
    fn assert_known_tenant(&self, tenant_id: &str) -> Result<(), KnowledgeTenantError> {
        if self.partitions.contains_key(tenant_id) {
            return Ok(());
        }
        if self.documents.is_empty() {
            return Err(KnowledgeTenantError(format!(
                "Knowledge store is empty; no tenant \"{}\" exists.",
                tenant_id
            )));
        }
        Err(KnowledgeTenantError(format!(
            "No knowledge documents for tenant \"{}\". Registered tenants: {}.",
            tenant_id,
            self.tenants().join(", ")
        )))
    }
}

/// Chunk a document deterministically and stamp provenance at ingest.
///
/// A document carries only a date, so the instant is the start of that day in
/// UTC rather than a fabrication.
fn chunks_of_document(document: &Document) -> Vec<Chunk> {
    chunk_text(&document.content)
        .into_iter()
        .enumerate()
        .map(|(ordinal, text)| Chunk {
            id: format!("{}.c{}", document.id, ordinal),
            tenant_id: document.tenant_id.clone(),
            document_id: document.id.clone(),
            ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
            text,
            provenance: super::schema::Provenance {
                source_kind: document.kind,
                source_id: document.id.clone(),
                source_title: document.title.clone(),
                source_url: document.source_url.clone(),
                source_updated_at: document.updated_at.clone(),
                ingested_at: format!("{}T00:00:00.000Z", document.updated_at),
            },
        })
        .collect()
}

/// Lexical relevance, normalised to `[0, 1]`.
///
/// Two things are deliberate here. (1) Matching is on stems, not raw tokens:
/// "cancellation" must match "cancellations", and without stemming the single
/// most natural query against the single most natural document scores zero.
/// (2) The score mixes term coverage with a length-normalised density, so a
/// match inside a very long chunk does not outrank a match inside a focused
/// one.
///
/// This is the hermetic stand-in for the pgvector/embedding path the PRD
/// selects for production: same seam, no network, deterministic under test. A
/// production store swaps in the vector scorer behind this function.
pub fn score_chunk(query: &str, chunk_text_value: &str) -> f64 {
    let query_tokens: std::collections::BTreeSet<String> =
        tokenise_for_scoring(query).into_iter().collect();
    if query_tokens.is_empty() {
        return 0.0;
    }

    let chunk_tokens = tokenise_for_scoring(chunk_text_value)
        .iter()
        .map(|token| stem(token))
        .collect::<Vec<String>>();
    if chunk_tokens.is_empty() {
        return 0.0;
    }

    let mut frequencies: HashMap<String, usize> = HashMap::new();
    for token in &chunk_tokens {
        *frequencies.entry(token.clone()).or_insert(0) += 1;
    }

    let mut hits = 0usize;
    let mut weighted = 0.0f64;
    for token in &query_tokens {
        let stemmed = stem(token);
        let frequency = frequencies.get(&stemmed).copied().unwrap_or(0);
        if frequency > 0 {
            hits += 1;
        }
        weighted += frequency as f64;
    }

    let coverage = hits as f64 / query_tokens.len() as f64;
    let density = weighted / (chunk_tokens.len() as f64).sqrt();
    (coverage * 0.75 + density * 0.25).min(1.0)
}
