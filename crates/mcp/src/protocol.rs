//! Model Context Protocol (MCP) JSON-RPC 2.0 Types and Primitives.
//! Conforms to the official Linux Foundation AAIF MCP specification.

use serde::{Deserialize, Serialize};

/// Standard JSON-RPC 2.0 Request envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    pub fn new(
        id: impl Into<serde_json::Value>,
        method: impl Into<String>,
        params: Option<serde_json::Value>,
    ) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

/// Standard JSON-RPC 2.0 Response envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: serde_json::Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

/// JSON-RPC 2.0 Error details.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// MCP Tool definition exposed to agents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// Result of an MCP tool execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallToolResult {
    pub content: Vec<McpContent>,
    #[serde(default, rename = "isError")]
    pub is_error: bool,
}

impl CallToolResult {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![McpContent {
                r#type: "text".to_string(),
                text: text.into(),
            }],
            is_error: false,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: vec![McpContent {
                r#type: "text".to_string(),
                text: message.into(),
            }],
            is_error: true,
        }
    }
}

/// Content item inside an MCP response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpContent {
    pub r#type: String,
    pub text: String,
}

/// MCP Resource metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

/// MCP Prompt template definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpPrompt {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub arguments: Vec<McpPromptArgument>,
}

/// Argument definition for an MCP Prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpPromptArgument {
    pub name: String,
    pub description: String,
    pub required: bool,
}
