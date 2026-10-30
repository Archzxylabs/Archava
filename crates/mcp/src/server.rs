//! Archava Enterprise Model Context Protocol (MCP) Server.
//! Bridges external agents (Claude Desktop, Cursor, OpenAI Operator, Agentforce)
//! to Archava's ERP/CRM kernel with strict RLS and Least-Privilege Authority Gating.

use archava_cockpit::{CockpitEngine, FlightStep, Mission};
use archava_orbz::{AuthorityTier, ContextTier, OrbzActor, OrbzStore};
use serde_json::json;

use crate::protocol::{CallToolResult, JsonRpcRequest, JsonRpcResponse, McpResource, McpTool};

/// Enterprise MCP Server for Archava.
pub struct ArchavaMcpServer {
    cockpit: CockpitEngine,
    orbz: OrbzStore,
}

impl Default for ArchavaMcpServer {
    fn default() -> Self {
        Self::new(CockpitEngine::new(), OrbzStore::new())
    }
}

impl ArchavaMcpServer {
    pub fn new(cockpit: CockpitEngine, orbz: OrbzStore) -> Self {
        Self { cockpit, orbz }
    }

    /// Returns the catalogue of available MCP tools.
    pub fn list_tools(&self) -> Vec<McpTool> {
        vec![
            McpTool {
                name: "archava_query_stock".into(),
                description: "Queries current inventory balance and warehouse locations for a specific SKU. Requires L1 Observer authority.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "tenant_id": { "type": "string" },
                        "sku": { "type": "string" }
                    },
                    "required": ["tenant_id", "sku"]
                }),
            },
            McpTool {
                name: "archava_post_gl_journal".into(),
                description: "Posts a balanced double-entry General Ledger journal. Enforces Sum(Debit) == Sum(Credit). Requires L4 Transact authority.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "tenant_id": { "type": "string" },
                        "reference": { "type": "string" },
                        "debit_account": { "type": "string" },
                        "credit_account": { "type": "string" },
                        "amount_cents": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["tenant_id", "reference", "debit_account", "credit_account", "amount_cents"]
                }),
            },
            McpTool {
                name: "archava_dispatch_flight_mission".into(),
                description: "Dispatches a multi-step autonomous flight mission into Ava Cockpit with durable state checkpoints. Requires L3 Manager authority.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "tenant_id": { "type": "string" },
                        "mission_code": { "type": "string" },
                        "title": { "type": "string" },
                        "description": { "type": "string" }
                    },
                    "required": ["tenant_id", "mission_code", "title", "description"]
                }),
            },
            McpTool {
                name: "archava_query_memory".into(),
                description: "Queries active agentic memories from Ava Orbz with progressive disclosure tiers (L0, L1, L2). Requires matching authority.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "tenant_id": { "type": "string" },
                        "tier": { "type": "string", "enum": ["l0_abstract", "l1_overview", "l2_raw_payload"] }
                    },
                    "required": ["tenant_id", "tier"]
                }),
            },
        ]
    }

    /// Returns the catalogue of accessible MCP resources (Orbz URIs).
    pub fn list_resources(&self) -> Vec<McpResource> {
        vec![
            McpResource {
                uri: "orbz://system/public/sop/refund_policy.md".into(),
                name: "Standard Refund SOP".into(),
                description: "7-day return and exchange rules for merchandise.".into(),
                mime_type: "text/markdown".into(),
            },
            McpResource {
                uri: "orbz://system/public/catalog/pricebook.json".into(),
                name: "Standard Pricebook".into(),
                description: "Current enterprise pricing and capability allowances.".into(),
                mime_type: "application/json".into(),
            },
        ]
    }

    /// Executes an MCP tool call guarded by caller's authority level (L1-L5).
    pub fn call_tool(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
        caller_authority: AuthorityTier,
    ) -> CallToolResult {
        match tool_name {
            "archava_query_stock" => {
                // Minimum L1 required
                let sku = args
                    .get("sku")
                    .and_then(|v| v.as_str())
                    .unwrap_or("UNKNOWN");
                CallToolResult::text(format!(
                    "[SIMULATED_READ_MODEL] Inventory for SKU '{}': 142 units in Warehouse Main-A (Status: In Stock, mode: fixture_uncommitted).",
                    sku
                ))
            }
            "archava_post_gl_journal" => {
                // Minimum L4 required
                if caller_authority < AuthorityTier::L4 {
                    return CallToolResult::error(
                        "Permission Denied: archava_post_gl_journal requires L4 Transact authority. Request escalated to human holding pattern.",
                    );
                }
                let ref_no = args
                    .get("reference")
                    .and_then(|v| v.as_str())
                    .unwrap_or("REF");
                let amount = args
                    .get("amount_cents")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                if amount <= 0 {
                    return CallToolResult::error(
                        "Validation Error: amount_cents must be strictly positive",
                    );
                }
                CallToolResult::text(format!(
                    "[SIMULATED_STAGING] Journal '{}' posted successfully (staged): Debit = Credit = {} cents. Balance invariant confirmed.",
                    ref_no, amount
                ))
            }
            "archava_dispatch_flight_mission" => {
                // Minimum L3 required
                if caller_authority < AuthorityTier::L3 {
                    return CallToolResult::error(
                        "Permission Denied: archava_dispatch_flight_mission requires L3 Supervisor authority.",
                    );
                }
                let tenant = args
                    .get("tenant_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ten_01");
                let code = args
                    .get("mission_code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("MIS-001");
                let title = args
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Mission");
                let desc = args
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Description");

                let step = FlightStep::new(1, "Initialize Task", "init", json!({}), vec![]);
                let mission = Mission::new(tenant, code, title, desc, vec![step]);
                let mission_id = self.cockpit.register_mission(mission);
                let _ = self.cockpit.launch_flight(mission_id);

                CallToolResult::text(format!(
                    "Flight mission '{}' dispatched into In-Flight status with ID: {}.",
                    code, mission_id
                ))
            }
            "archava_query_memory" => {
                let tenant = args
                    .get("tenant_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ten_01");
                let tier_str = args
                    .get("tier")
                    .and_then(|v| v.as_str())
                    .unwrap_or("l0_abstract");
                let tier = match tier_str {
                    "l1_overview" => ContextTier::L1Overview,
                    "l2_raw_payload" => ContextTier::L2RawPayload,
                    _ => ContextTier::L0Abstract,
                };

                let actor = OrbzActor {
                    tenant_id: tenant.into(),
                    user_id: "mcp_client".into(),
                    authority: caller_authority,
                    department: None,
                    is_owner: caller_authority >= AuthorityTier::L5,
                };

                let nodes = self.orbz.query_accessible(&actor, 1000);
                let summaries: Vec<String> =
                    nodes.into_iter().map(|n| n.render_tier(tier)).collect();
                CallToolResult::text(json!(summaries).to_string())
            }
            _ => CallToolResult::error(format!("Unknown tool: {}", tool_name)),
        }
    }

    /// Dispatches a raw JSON-RPC 2.0 request according to the MCP specification.
    pub fn handle_rpc(
        &self,
        request: JsonRpcRequest,
        caller_authority: AuthorityTier,
    ) -> JsonRpcResponse {
        match request.method.as_str() {
            "tools/list" => {
                let tools = self.list_tools();
                JsonRpcResponse::success(request.id, json!({ "tools": tools }))
            }
            "tools/call" => {
                let params = request.params.unwrap_or_else(|| json!({}));
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let default_args = json!({});
                let args = params.get("arguments").unwrap_or(&default_args);
                let result = self.call_tool(name, args, caller_authority);
                JsonRpcResponse::success(request.id, json!(result))
            }
            "resources/list" => {
                let resources = self.list_resources();
                JsonRpcResponse::success(request.id, json!({ "resources": resources }))
            }
            "ping" => JsonRpcResponse::success(request.id, json!({ "status": "pong" })),
            _ => JsonRpcResponse::error(
                request.id,
                -32601,
                format!("Method not found: {}", request.method),
            ),
        }
    }
}
