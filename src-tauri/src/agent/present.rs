//! Built-in Present toolset (Generative UI). The model calls `present` to render a
//! typed, interactive workspace block — a comparison table, checklist, form,
//! progress meter, collection, or document reference — inline in the assistant
//! turn instead of describing data in prose. It calls `remember` to update the
//! conversation's durable session state (entities/constraints/decisions).
//!
//! Both tools return only a short receipt to the model so a large payload doesn't
//! bloat the context window; the block/state itself streams to the UI via the
//! event sink.

use super::toolsets::ToolContext;
use crate::db::Db;

/// The six block kinds the renderer understands.
const KINDS: [&str; 6] = ["comparison", "collection", "plan", "form", "progress", "document"];

/// Cap on the serialized session state, so it can't grow unbounded in context.
const STATE_CAP_BYTES: usize = 4096;

/// The conversation's live workspace surface is stored as one blocks-table row
/// with this reserved kind/title, so the existing persistence, events, and
/// state plumbing carry it without a schema change.
const SURFACE_KIND: &str = "surface";
const SURFACE_TITLE: &str = "Workspace";

/// Cap on the serialized surface tree.
const SURFACE_CAP_BYTES: usize = 65536;

/// The OpenAI tool schemas advertised to the model for this toolset.
pub fn tool_specs() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "present",
                "description": "Show a structured, interactive block inline in the chat instead of describing data in prose. Kinds and their data shapes: comparison {columns:[{id,label}],options:[{id,label,values:{colId:val},pros?,cons?}],recommended_id?} for ranked options; collection {items:[{id,title,subtitle?,tags?,url?,meta?}],facets?:[{id,label}]} for browsable lists; plan {steps:[{id,label,detail?,status:todo|doing|done|blocked}]} for checklists; form {fields:[{id,label,type:text|number|select|multiselect|toggle,options?,required?}],submit_label?} to request structured input from the user; progress {label,current,total,unit?,status:running|done|error,note?} for long tasks; document {artifact_id} or {markdown} for a full document. Pass block_id to UPDATE an existing block in place (e.g. mark plan steps done, refresh progress). Prefer a block whenever the user is deciding, choosing, filling in, or tracking something.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "enum": ["comparison", "collection", "plan", "form", "progress", "document"] },
                        "title": { "type": "string", "description": "Short heading shown above the block" },
                        "data": { "type": "object", "description": "Block payload per the kind's shape" },
                        "block_id": { "type": "string", "description": "Omit to create; pass an existing block id to update it in place" }
                    },
                    "required": ["kind", "title", "data"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "render_ui",
                "description": "Compose the live Workspace interface for this conversation. Do not pick from prebuilt widgets — build ANY interface (dashboard, board, wizard, picker, table, tracker) as a tree of primitive nodes. Every node is {type, id?, children?} plus type-specific fields. LAYOUT: stack {direction?:'column'|'row', children}; grid {columns:N, children}; section {title?, children}; divider {}. CONTENT: text {value, variant?:'title'|'heading'|'body'|'caption'|'code'}; metric {label, value, unit?, delta?, intent?:'ok'|'warn'|'danger'}; badge {value, intent?}; progress {label?, value, max?}; link {value, url}. INTERACTIVE: item {title, subtitle?, meta?, selected?, action?, payload?} — a selectable row; choice {bind, options:[{id,label,detail?}], multi?}; input {bind, label?, type?:'text'|'number', placeholder?}; toggle {bind, label}; button {label, action, payload?, style?:'primary'}; form {fields:[{id,label,type:'text'|'number'|'select'|'multiselect'|'toggle',options?,required?,placeholder?}], submit_label?, action?} — a group of labeled fields, each bound by its own id; add a button (or submit_label) to submit them. Fields named 'bind' store the user's edits locally in the surface state (no turn is spent); a node with 'action' sends you a message carrying that action name, its payload, and all bound state. Give an id to any node you may want to revise, then call render_ui with node_id to replace only that subtree (keep the same id on the replacement so it stays addressable); omit node_id to replace the whole surface. The surface is persistent and current — keep it updated as the task evolves instead of narrating in prose.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "ui": { "type": "object", "description": "The root node of the interface tree" },
                        "node_id": { "type": "string", "description": "Omit to replace the whole surface; pass a node id to replace just that subtree" }
                    },
                    "required": ["ui"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "remember",
                "description": "Update the durable session state for this conversation. Pass a merge patch: keys overwrite, null deletes a key. Use top-level keys entities (people/places/things), constraints (user requirements), decisions (settled choices). Keep values short.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "patch": { "type": "object", "description": "JSON merge patch over the session state" }
                    },
                    "required": ["patch"]
                }
            }
        }
    ])
}

/// Is this a Present tool name?
pub fn handles(name: &str) -> bool {
    matches!(name, "present" | "remember" | "render_ui")
}

/// Human-readable (verb, target) for the timeline (§5.6 plain past-tense).
pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    match name {
        "present" => {
            let title = args.get("title").and_then(|t| t.as_str()).unwrap_or("block");
            let verb = if args.get("block_id").and_then(|b| b.as_str()).is_some() {
                "updated"
            } else {
                "presented"
            };
            (verb.into(), title.to_string())
        }
        "render_ui" => {
            let verb = if args.get("node_id").and_then(|n| n.as_str()).is_some() {
                "revised"
            } else {
                "composed"
            };
            (verb.into(), "the workspace".to_string())
        }
        "remember" => {
            let key = args
                .get("patch")
                .and_then(|p| p.as_object())
                .and_then(|o| o.keys().next())
                .map(String::as_str)
                .unwrap_or("session state");
            ("remembered".into(), key.to_string())
        }
        other => (other.into(), String::new()),
    }
}

/// Execute a Present call. Returns the text fed back to the model.
pub async fn execute(
    ctx: &ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    match name {
        "present" => present(ctx, args),
        "remember" => remember(ctx, args),
        "render_ui" => render_ui(ctx, args),
        other => Err(format!("Present toolset can't handle '{other}'.")),
    }
}

/// Compose or revise the conversation's live workspace surface: an arbitrary
/// interface expressed as a tree of primitive nodes, rendered by one recursive
/// renderer in the Workspace view. Stored as a single reserved blocks row.
fn render_ui(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let ui = args
        .get("ui")
        .filter(|u| u.is_object())
        .cloned()
        .ok_or("missing 'ui' object argument (the root node of the interface tree)")?;
    if ui.get("type").and_then(|t| t.as_str()).is_none() {
        return Err("the root 'ui' node needs a string 'type' (e.g. stack, grid, section)".into());
    }

    let existing = ctx
        .db
        .find_block_by_title(ctx.conversation_id, SURFACE_KIND, SURFACE_TITLE)
        .ok()
        .flatten();

    // node_id patches just that subtree of the current surface in place.
    let tree = match (args.get("node_id").and_then(|n| n.as_str()), &existing) {
        (Some(node_id), Some(block)) => {
            let mut current: serde_json::Value =
                serde_json::from_str(&block.data_json).unwrap_or(serde_json::json!({}));
            if !replace_node(&mut current, node_id, &ui) {
                return Err(format!(
                    "no node with id '{node_id}' on the current surface — re-render the whole surface (omit node_id) or use an existing id"
                ));
            }
            current
        }
        _ => ui,
    };

    let data_json = serde_json::to_string(&tree).map_err(|e| e.to_string())?;
    if data_json.len() > SURFACE_CAP_BYTES {
        return Err(format!(
            "surface tree is too large ({} bytes; limit {SURFACE_CAP_BYTES}). Simplify or split it into sections you update separately.",
            data_json.len()
        ));
    }
    let nodes = count_nodes(&tree);

    if let Some(block) = existing {
        ctx.db
            .update_block_data(&block.id, SURFACE_TITLE, &data_json)
            .map_err(|e| format!("couldn't update the surface: {e}"))?;
        ctx.sink.block_update(&block.id, SURFACE_TITLE, &tree);
        let _ = ctx
            .db
            .log_activity(Some(ctx.conversation_id), "surface", "revised the workspace");
        Ok(format!(
            "Updated the workspace surface ({nodes} nodes). The user sees it now — do not restate it in prose; reply in at most one sentence."
        ))
    } else {
        let block = ctx
            .db
            .add_block(
                ctx.conversation_id,
                ctx.assistant_message_id,
                SURFACE_KIND,
                SURFACE_TITLE,
                &data_json,
            )
            .map_err(|e| format!("couldn't save the surface: {e}"))?;
        ctx.sink
            .block(&block.id, ctx.assistant_message_id, SURFACE_KIND, SURFACE_TITLE, &tree);
        let _ = ctx
            .db
            .log_activity(Some(ctx.conversation_id), "surface", "composed the workspace");
        Ok(format!(
            "Rendered the workspace surface ({nodes} nodes). The user sees it in the Workspace view — do not describe it in prose; conclude in one sentence. Revise a region later with render_ui + node_id, or re-render the whole surface."
        ))
    }
}

/// Write a conversation's surface directly, outside any agent run (RCP-UI-2:
/// starting a workspace from a recipe template). Same reserved row, same
/// create-or-update rule as `render_ui` — a seeded surface is an ordinary
/// surface the model can revise from its first turn. Returns the block id.
pub fn write_surface(db: &Db, conversation_id: &str, data_json: &str) -> Result<String, String> {
    if data_json.len() > SURFACE_CAP_BYTES {
        return Err("that surface template is too large".into());
    }
    if let Some(block) = db
        .find_block_by_title(conversation_id, SURFACE_KIND, SURFACE_TITLE)
        .ok()
        .flatten()
    {
        db.update_block_data(&block.id, SURFACE_TITLE, data_json)
            .map_err(|e| e.to_string())?;
        return Ok(block.id);
    }
    let block = db
        .add_block(conversation_id, None, SURFACE_KIND, SURFACE_TITLE, data_json)
        .map_err(|e| e.to_string())?;
    Ok(block.id)
}

/// Depth-first replace of the node whose `id` matches; returns whether found.
fn replace_node(tree: &mut serde_json::Value, id: &str, new_node: &serde_json::Value) -> bool {
    if tree.get("id").and_then(|i| i.as_str()) == Some(id) {
        *tree = new_node.clone();
        return true;
    }
    if let Some(children) = tree.get_mut("children").and_then(|c| c.as_array_mut()) {
        for child in children {
            if replace_node(child, id, new_node) {
                return true;
            }
        }
    }
    false
}

/// Count nodes in a surface tree (for the tool receipt).
fn count_nodes(tree: &serde_json::Value) -> usize {
    1 + tree
        .get("children")
        .and_then(|c| c.as_array())
        .map(|kids| kids.iter().map(count_nodes).sum())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{count_nodes, replace_node};

    #[test]
    fn replaces_a_nested_node_by_id_and_counts() {
        let mut tree = serde_json::json!({
            "type": "stack",
            "children": [
                { "type": "text", "value": "hello" },
                { "type": "section", "id": "results", "children": [
                    { "type": "item", "title": "old" }
                ]}
            ]
        });
        assert_eq!(count_nodes(&tree), 4);

        let new_node = serde_json::json!({
            "type": "section", "id": "results", "children": [
                { "type": "item", "title": "new" },
                { "type": "item", "title": "newer" }
            ]
        });
        assert!(replace_node(&mut tree, "results", &new_node));
        assert_eq!(count_nodes(&tree), 5);
        assert_eq!(tree["children"][1]["children"][0]["title"], "new");

        // Unknown id leaves the tree untouched.
        assert!(!replace_node(&mut tree, "nope", &new_node));
    }
}

/// Create or update a workspace block.
fn present(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let kind = args
        .get("kind")
        .and_then(|k| k.as_str())
        .filter(|k| KINDS.contains(k))
        .ok_or_else(|| {
            format!("missing or unknown 'kind'; use one of: {}", KINDS.join(", "))
        })?;
    let title = args
        .get("title")
        .and_then(|t| t.as_str())
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or("Untitled");
    let mut data = args
        .get("data")
        .filter(|d| d.is_object())
        .cloned()
        .ok_or("missing 'data' object argument")?;

    // A document given as inline markdown becomes an artifact; the block just
    // references it so the full text never bloats the block payload.
    if kind == "document" {
        if let Some(md) = data.get("markdown").and_then(|m| m.as_str()) {
            let artifact = ctx
                .db
                .add_artifact(Some(ctx.conversation_id), title, "markdown", md, ctx.assistant_message_id)
                .map_err(|e| format!("couldn't save the document: {e}"))?;
            ctx.sink.artifact(&artifact.id, title, "markdown", md);
            data = serde_json::json!({ "artifact_id": artifact.id });
        }
    }

    let data_json = serde_json::to_string(&data).map_err(|e| e.to_string())?;

    // Update in place when a valid block_id for this conversation is given.
    if let Some(block_id) = args.get("block_id").and_then(|b| b.as_str()) {
        if let Ok(Some(existing)) = ctx.db.get_block(block_id) {
            if existing.conversation_id == ctx.conversation_id {
                return update_existing(ctx, &existing.id, title, &data, &data_json);
            }
        }
        // Fall through to create if the id was unknown or from another chat.
    }

    // W3 safety net: a model that ignores the block registry and re-presents the
    // same kind+title (common with weak local models) should update in place
    // rather than spawn a duplicate on the user's workspace.
    if let Ok(Some(existing)) = ctx.db.find_block_by_title(ctx.conversation_id, kind, title) {
        return update_existing(ctx, &existing.id, title, &data, &data_json);
    }

    let block = ctx
        .db
        .add_block(ctx.conversation_id, ctx.assistant_message_id, kind, title, &data_json)
        .map_err(|e| format!("couldn't save the block: {e}"))?;
    ctx.sink.block(&block.id, ctx.assistant_message_id, kind, title, &data);
    let _ = ctx
        .db
        .log_activity(Some(ctx.conversation_id), "block", &format!("presented {title}"));

    // W4: the receipt keeps the model from narrating the block back in prose.
    Ok(format!(
        "Presented the {kind} block \"{title}\" (block_id: {}). The user can now see it in full — do not repeat its contents in prose; conclude briefly. Pass this block_id to `present` to update it later.",
        block.id
    ))
}

/// Update an existing block's data and emit the in-place update event.
fn update_existing(
    ctx: &ToolContext<'_>,
    block_id: &str,
    title: &str,
    data: &serde_json::Value,
    data_json: &str,
) -> Result<String, String> {
    ctx.db
        .update_block_data(block_id, title, data_json)
        .map_err(|e| format!("couldn't update the block: {e}"))?;
    ctx.sink.block_update(block_id, title, data);
    let _ = ctx
        .db
        .log_activity(Some(ctx.conversation_id), "block", &format!("updated {title}"));
    Ok(format!(
        "Updated block \"{title}\" ({block_id}) in place. The user sees the change — do not restate its contents."
    ))
}

/// Apply a JSON merge patch to the conversation's durable session state.
fn remember(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let patch = args
        .get("patch")
        .filter(|p| p.is_object())
        .ok_or("missing 'patch' object argument")?;

    let mut state: serde_json::Value = ctx
        .db
        .get_session_state(ctx.conversation_id)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    merge_patch(&mut state, patch);

    let serialized = serde_json::to_string(&state).map_err(|e| e.to_string())?;
    if serialized.len() > STATE_CAP_BYTES {
        return Err(format!(
            "session state is too large ({} bytes; limit {STATE_CAP_BYTES}). Remove keys you no longer need with a null patch.",
            serialized.len()
        ));
    }

    ctx.db
        .set_session_state(ctx.conversation_id, &serialized)
        .map_err(|e| format!("couldn't save session state: {e}"))?;
    ctx.sink.state_update(&state);

    let keys: Vec<&str> = state
        .as_object()
        .map(|o| o.keys().map(String::as_str).collect())
        .unwrap_or_default();
    Ok(format!("Remembered. Current state keys: {}.", keys.join(", ")))
}

/// RFC 7386-style JSON merge patch: object keys are merged recursively, an
/// explicit null deletes a key, and any non-object patch replaces the target.
fn merge_patch(target: &mut serde_json::Value, patch: &serde_json::Value) {
    match patch {
        serde_json::Value::Object(patch_map) => {
            if !target.is_object() {
                *target = serde_json::json!({});
            }
            let target_map = target.as_object_mut().unwrap();
            for (k, v) in patch_map {
                if v.is_null() {
                    target_map.remove(k);
                } else {
                    merge_patch(target_map.entry(k.clone()).or_insert(serde_json::Value::Null), v);
                }
            }
        }
        other => *target = other.clone(),
    }
}
