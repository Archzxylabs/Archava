//! Model Context Protocol (MCP) Client Gateway.
//! Allows Ava to connect dynamically to external MCP servers (Shopify, Google Drive, Slack, Stripe).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::protocol::{CallToolResult, McpTool};

/// Registry of external MCP servers connected to Ava.
#[derive(Debug, Default, Clone)]
pub struct McpClientGateway {
    connected_tools: Arc<RwLock<HashMap<String, McpTool>>>,
}

impl McpClientGateway {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers tools discovered from a connected external MCP server.
    pub fn register_external_tools(&self, server_name: &str, tools: Vec<McpTool>) {
        let mut guard = self.connected_tools.write().expect("lock");
        for mut tool in tools {
            let prefixed_name = format!("{}__{}", server_name, tool.name);
            tool.name = prefixed_name.clone();
            guard.insert(prefixed_name, tool);
        }
    }

    /// Lists all aggregated external tools available for Ava's reasoning loops.
    pub fn list_aggregated_tools(&self) -> Vec<McpTool> {
        self.connected_tools
            .read()
            .expect("lock")
            .values()
            .cloned()
            .collect()
    }

    /// Dispatches an invocation to an external MCP tool.
    pub fn invoke_external(
        &self,
        namespaced_tool: &str,
        arguments: serde_json::Value,
    ) -> Result<CallToolResult, String> {
        let guard = self.connected_tools.read().expect("lock");
        if !guard.contains_key(namespaced_tool) {
            return Err(format!(
                "External tool '{}' not registered",
                namespaced_tool
            ));
        }

        // Simulates outbound JSON-RPC dispatch to the remote MCP agent transport
        Ok(CallToolResult::text(format!(
            "Executed external tool '{}' with payload: {}",
            namespaced_tool, arguments
        )))
    }
}
