//! Model Context Protocol client (MCP-1, MCP-4). Connects to a remote MCP server
//! over the Streamable HTTP transport and exposes its tools to the agent loop.
//!
//! Scope for v1: remote HTTP servers (the modern Streamable HTTP transport, which
//! supersedes the older HTTP+SSE transport). stdio servers (MCP-2) are deferred.

pub mod client;

use serde::{Deserialize, Serialize};

pub use client::McpClient;

/// A tool advertised by an MCP server (`tools/list`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// JSON Schema for the tool's arguments (advertised to the model verbatim).
    #[serde(default, rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

impl McpTool {
    /// Convert to the OpenAI-compatible function-tool schema the engine expects.
    pub fn to_openai_spec(&self) -> serde_json::Value {
        let params = if self.input_schema.is_object() {
            self.input_schema.clone()
        } else {
            serde_json::json!({ "type": "object", "properties": {} })
        };
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": params,
            }
        })
    }
}
