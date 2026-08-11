//! Built-in Artifacts toolset (CHT-6). The model calls `create_artifact` to emit a
//! titled, self-contained piece of content — HTML, SVG, markdown, or code — which
//! Poiesis persists and renders in the Canvas side panel (HTML/SVG in a sandboxed
//! iframe). The tool result fed back to the model is only a short confirmation, so
//! a large artifact doesn't bloat the context window.

use super::toolsets::ToolContext;

const KINDS: [&str; 4] = ["html", "svg", "markdown", "code"];

/// The OpenAI tool schema advertised to the model for this toolset.
pub fn tool_specs() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "create_artifact",
                "description": "Render a self-contained artifact in the side panel: a web page (html), vector graphic (svg), rich document (markdown), or code file. Use for anything the user should see rendered rather than described.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "description": "Short title for the artifact" },
                        "kind": {
                            "type": "string",
                            "enum": ["html", "svg", "markdown", "code"],
                            "description": "How to render the content"
                        },
                        "content": { "type": "string", "description": "The full artifact content" }
                    },
                    "required": ["title", "kind", "content"]
                }
            }
        }
    ])
}

/// Is this an Artifacts tool name?
pub fn handles(name: &str) -> bool {
    name == "create_artifact"
}

/// Human-readable (verb, target) for the timeline (§5.6 plain past-tense).
pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    let title = args.get("title").and_then(|t| t.as_str()).unwrap_or("artifact");
    match name {
        "create_artifact" => ("created".into(), title.to_string()),
        other => (other.into(), title.to_string()),
    }
}

/// Persist the artifact, emit it to the Canvas panel, and return a short receipt.
pub async fn execute(
    ctx: &ToolContext<'_>,
    _name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let title = args
        .get("title")
        .and_then(|t| t.as_str())
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or("Untitled");
    let kind = args
        .get("kind")
        .and_then(|k| k.as_str())
        .filter(|k| KINDS.contains(k))
        .unwrap_or("markdown");
    let content = args
        .get("content")
        .and_then(|c| c.as_str())
        .ok_or("missing 'content' argument")?;

    let artifact = ctx
        .db
        .add_artifact(Some(ctx.conversation_id), title, kind, content)
        .map_err(|e| format!("couldn't save the artifact: {e}"))?;

    ctx.sink.artifact(&artifact.id, title, kind, content);
    let _ = ctx
        .db
        .log_activity(Some(ctx.conversation_id), "artifact", &format!("created {title}"));

    Ok(format!(
        "Created the {kind} artifact \"{title}\" and opened it in the Canvas panel."
    ))
}
