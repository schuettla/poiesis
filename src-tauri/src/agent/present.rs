//! Built-in Present toolset (Generative UI). The model calls `render_ui` to compose
//! the live interface of a Workspace chat as a tree of primitive nodes, and
//! `remember` to update the conversation's durable session state
//! (entities/constraints/decisions).
//!
//! There used to be a second tool, `present`, that drew one of seven fixed block
//! kinds into the message stream. It overlapped `render_ui` and is gone: the
//! blocks other tools still leave (a build's diagnostics) and the ones already
//! stored are drawn by the frontend as before, but the model can no longer make
//! new ones.
//!
//! Both tools return only a short receipt to the model so a large payload doesn't
//! bloat the context window; the surface/state itself streams to the UI via the
//! event sink.

use super::toolsets::ToolContext;
use crate::db::Db;

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
                "name": "render_ui",
                "description": "Compose the live Workspace interface for this conversation. Do not pick from prebuilt widgets — build ANY interface (dashboard, board, wizard, picker, table, tracker) as a tree of primitive nodes. Every node is {type, id?, children?} plus type-specific fields. LAYOUT: stack {direction?:'column'|'row', children}; grid {columns:N, children}; section {title?, children}; divider {}. CONTENT: text {value, variant?:'title'|'heading'|'body'|'caption'|'code'}; metric {label, value, unit?, delta?, intent?:'ok'|'warn'|'danger'}; badge {value, intent?}; progress {label?, value, max?}; link {value, url}; table {columns:[string], rows:[[string|number]]} for anything compared or listed in columns. INTERACTIVE: item {title, subtitle?, meta?, selected?, action?, payload?} — a selectable row; choice {bind, options:[{id,label,detail?}], multi?}; input {bind, label?, type?:'text'|'number', placeholder?}; toggle {bind, label}; button {label, action, payload?, style?:'primary'}; form {fields:[{id,label,type:'text'|'number'|'select'|'multiselect'|'toggle',options?,required?,placeholder?}], submit_label?, action?} — a group of labeled fields, each bound by its own id; add a button (or submit_label) to submit them. Fields named 'bind' store the user's edits locally in the surface state (no turn is spent); a node with 'action' sends you a message carrying that action name, its payload, and all bound state. A checklist is a stack of toggle nodes, a comparison is a table, a list to choose from is a choice or a set of item rows. Give an id to any node you may want to revise, then call render_ui with node_id to replace only that subtree (keep the same id on the replacement so it stays addressable); omit node_id to replace the whole surface. The surface is persistent and current — keep it updated as the task evolves instead of narrating in prose.",
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

/// A chat outside Workspace mode has nowhere to show an interface, so it is not
/// offered the tool that composes one. `ToolRegistry::build` calls this with the
/// chat's own flag; the prompt makes the same cut (`PromptInputs::workspace`).
pub fn withdraw_for_mode(specs: &mut Vec<serde_json::Value>, workspace: bool) {
    if workspace {
        return;
    }
    specs.retain(|s| s.pointer("/function/name").and_then(|n| n.as_str()) != Some("render_ui"));
}

/// Is this a Present tool name?
pub fn handles(name: &str) -> bool {
    matches!(name, "remember" | "render_ui")
}

/// Human-readable (verb, target) for the timeline (§5.6 plain past-tense).
pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    match name {
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
        "remember" => remember(ctx, args),
        "render_ui" => render_ui(ctx, args),
        other => Err(format!("Present toolset can't handle '{other}'.")),
    }
}

/// Compose or revise the conversation's live workspace surface: an arbitrary
/// interface expressed as a tree of primitive nodes, rendered by one recursive
/// renderer in the Workspace view. Stored as a single reserved blocks row.
fn render_ui(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    // `withdraw_for_mode` keeps the tool off the menu, but a model copying an
    // older call from history can still name it. Nothing would show what it
    // wrote, so the call is refused here, where every dispatch passes.
    let in_mode = ctx
        .db
        .get_conversation(ctx.conversation_id)
        .ok()
        .flatten()
        .is_some_and(|c| crate::experiments::in_workspace(ctx.db, &c));
    if !in_mode {
        return Err("This chat is not in Workspace mode, so there is no interface to compose. Answer in the chat instead.".into());
    }

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
        // A patch needs something to patch. Taking the fragment as the whole
        // surface would wipe out the page the model believes it is revising.
        (Some(node_id), None) => {
            return Err(format!(
                "there is no surface yet, so '{node_id}' cannot be replaced. Call render_ui without node_id to compose the whole interface first."
            ));
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
    use super::{count_nodes, handles, replace_node, tool_specs, withdraw_for_mode};

    #[test]
    fn render_ui_is_offered_only_to_a_chat_in_workspace_mode() {
        let names = |specs: &[serde_json::Value]| -> Vec<String> {
            specs
                .iter()
                .filter_map(|s| s.pointer("/function/name").and_then(|n| n.as_str()))
                .map(str::to_string)
                .collect()
        };
        let mut specs = tool_specs().as_array().unwrap().clone();
        withdraw_for_mode(&mut specs, true);
        assert_eq!(names(&specs), ["render_ui", "remember"]);
        withdraw_for_mode(&mut specs, false);
        assert_eq!(names(&specs), ["remember"]);
    }

    #[test]
    fn the_model_can_no_longer_make_the_old_block_kinds() {
        assert!(!handles("present"));
        let specs = tool_specs();
        assert!(specs.as_array().unwrap().iter().all(|s| s["function"]["name"] != "present"));
    }

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
