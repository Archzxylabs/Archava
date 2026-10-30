//! Subagent Context Isolation & Data Compression (Claude Code Paradigm).
//! Spawns isolated worker turns with fresh context windows to prevent
//! context rot and prompt pollution, condensing raw outputs into high-signal summaries.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentWorkerContext {
    pub role: String,
    pub read_only: bool,
    pub allowed_tools: Vec<String>,
}

impl SubagentWorkerContext {
    pub fn read_only_inspector() -> Self {
        Self {
            role: "inspector".into(),
            read_only: true,
            allowed_tools: vec![
                "read_catalog".into(),
                "query_inventory".into(),
                "inspect_ledger".into(),
            ],
        }
    }

    pub fn domain_worker(domain: &str) -> Self {
        Self {
            role: format!("{}_worker", domain),
            read_only: false,
            allowed_tools: vec![format!("{}.execute", domain)],
        }
    }
}

/// The condensed result returned by a subagent to Ava Cockpit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerReport {
    pub worker_role: String,
    pub success: bool,
    /// 50:1 compressed high-signal summary that enters Ava's main context
    pub high_signal_summary: String,
    /// Complete structured outcome data
    pub output_payload: serde_json::Value,
    pub tokens_consumed: usize,
}

impl WorkerReport {
    pub fn success(
        worker_role: impl Into<String>,
        summary: impl Into<String>,
        output_payload: serde_json::Value,
        tokens_consumed: usize,
    ) -> Self {
        Self {
            worker_role: worker_role.into(),
            success: true,
            high_signal_summary: summary.into(),
            output_payload,
            tokens_consumed,
        }
    }

    pub fn failure(
        worker_role: impl Into<String>,
        error_summary: impl Into<String>,
        tokens_consumed: usize,
    ) -> Self {
        Self {
            worker_role: worker_role.into(),
            success: false,
            high_signal_summary: error_summary.into(),
            output_payload: serde_json::json!({ "error": true }),
            tokens_consumed,
        }
    }
}
