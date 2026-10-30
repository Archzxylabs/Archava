//! Model Context Protocol (MCP) Server and Client for Archava.
//! Conforms to Linux Foundation AAIF open standard.

pub mod client;
pub mod protocol;
pub mod server;

pub use client::McpClientGateway;
pub use protocol::{
    CallToolResult, JsonRpcError, JsonRpcRequest, JsonRpcResponse, McpResource, McpTool,
};
pub use server::ArchavaMcpServer;

#[cfg(test)]
mod tests {
    use super::*;
    use archava_cockpit::CockpitEngine;
    use archava_orbz::{AuthorityTier, OrbzStore};
    use serde_json::json;

    #[test]
    fn test_mcp_server_tools_and_rpc() {
        let server = ArchavaMcpServer::new(CockpitEngine::new(), OrbzStore::new());

        // 1. List tools via JSON-RPC
        let list_req = JsonRpcRequest::new(1, "tools/list", None);
        let resp = server.handle_rpc(list_req, AuthorityTier::L1);
        assert!(resp.error.is_none());
        assert!(resp.result.is_some());

        // 2. Query stock via tool call (L1 allowed)
        let call_res = server.call_tool(
            "archava_query_stock",
            &json!({ "tenant_id": "ten_01", "sku": "DETERGENT-5L" }),
            AuthorityTier::L1,
        );
        assert!(!call_res.is_error);
        assert!(call_res.content[0].text.contains("142 units"));

        // 3. Post GL journal refused for L1 (L4 required)
        let unauth_res = server.call_tool(
            "archava_post_gl_journal",
            &json!({
                "tenant_id": "ten_01",
                "reference": "JV-01",
                "debit_account": "10100",
                "credit_account": "40100",
                "amount_cents": 5000000
            }),
            AuthorityTier::L1,
        );
        assert!(unauth_res.is_error);
        assert!(unauth_res.content[0].text.contains("Permission Denied"));

        // 4. Post GL journal allowed for L4
        let auth_res = server.call_tool(
            "archava_post_gl_journal",
            &json!({
                "tenant_id": "ten_01",
                "reference": "JV-01",
                "debit_account": "10100",
                "credit_account": "40100",
                "amount_cents": 5000000
            }),
            AuthorityTier::L4,
        );
        assert!(!auth_res.is_error);
        assert!(auth_res.content[0].text.contains("posted successfully"));
    }

    #[test]
    fn test_mcp_client_gateway() {
        let gateway = McpClientGateway::new();
        let shopify_tool = McpTool {
            name: "fetch_orders".into(),
            description: "Fetches orders from Shopify store".into(),
            input_schema: json!({}),
        };

        gateway.register_external_tools("shopify", vec![shopify_tool]);
        let tools = gateway.list_aggregated_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "shopify__fetch_orders");

        let result = gateway.invoke_external("shopify__fetch_orders", json!({"limit": 10}));
        assert!(result.is_ok());
    }
}
