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
    /// What the server says about the tool's effects (`PLF-1`). A tool cached
    /// before this field existed has none, so plan-first leaves it out until the
    /// connector is tested again, rather than guessing it is harmless.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<McpToolAnnotations>,
}

/// The slice of MCP's tool annotations this app acts on.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpToolAnnotations {
    #[serde(default, rename = "readOnlyHint", skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
}

impl McpTool {
    /// `PLF-1`: only a tool whose server said it changes nothing may run while
    /// the agent is planning first.
    pub fn is_read_only(&self) -> bool {
        self.annotations.as_ref().and_then(|a| a.read_only_hint) == Some(true)
    }

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

/// A prompt a server offers (`prompts/list`). In Poiesis it is a command: typing
/// `/name` sends the text the server builds for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPrompt {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub arguments: Vec<McpPromptArgument>,
}

/// One named argument of a prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptArgument {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub required: bool,
}

/// The text of a `prompts/get` result: every message's words, in order, with
/// anything that is not text named rather than dropped silently. This is sent as
/// the user's message, so the roles are not kept.
pub fn prompt_text(result: &serde_json::Value) -> String {
    let Some(messages) = result.get("messages").and_then(|m| m.as_array()) else {
        return String::new();
    };
    let mut parts: Vec<String> = Vec::new();
    for message in messages {
        // The spec gives one content block; a few servers send a list.
        let blocks: Vec<&serde_json::Value> = match message.get("content") {
            Some(serde_json::Value::Array(list)) => list.iter().collect(),
            Some(one) => vec![one],
            None => Vec::new(),
        };
        for block in blocks {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        if !text.trim().is_empty() {
                            parts.push(text.trim().to_string());
                        }
                    }
                }
                Some("resource") => {
                    // An embedded resource carries its own text.
                    let text = block.pointer("/resource/text").and_then(|t| t.as_str());
                    parts.push(text.map(|t| t.trim().to_string()).unwrap_or_else(|| "[resource]".into()));
                }
                Some(other) => parts.push(format!("[{other} content]")),
                None => {}
            }
        }
    }
    parts.join("

")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prompt_is_every_text_block_in_order() {
        let result = serde_json::json!({
            "messages": [
                { "role": "user", "content": { "type": "text", "text": "Review this diff." } },
                { "role": "user", "content": [
                    { "type": "text", "text": "Be brief." },
                    { "type": "image", "data": "x", "mimeType": "image/png" },
                    { "type": "resource", "resource": { "uri": "file:///a", "text": "the file" } }
                ] },
            ]
        });
        assert_eq!(prompt_text(&result), "Review this diff.

Be brief.

[image content]

the file");
    }

    #[test]
    fn no_messages_is_no_text() {
        assert_eq!(prompt_text(&serde_json::json!({})), "");
    }

    #[test]
    fn a_prompt_listing_reads_with_or_without_arguments() {
        let list: Vec<McpPrompt> = serde_json::from_value(serde_json::json!([
            { "name": "summarise", "description": "Summarise a thing",
              "arguments": [{ "name": "topic", "required": true }] },
            { "name": "hello" }
        ]))
        .unwrap();
        assert!(list[0].arguments[0].required);
        assert!(list[1].arguments.is_empty());
    }
}
