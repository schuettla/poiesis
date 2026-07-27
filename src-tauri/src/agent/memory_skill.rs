//! The Memory skill (MEM-2, SOUL-2): how the agent writes to its own durable
//! self.
//!
//! Two verbs with very different weight. `memory` edits *facts about the user* —
//! narrow ops, visible in the timeline, undoable from a toast. `propose_soul_edit`
//! touches *standing instructions*, which change the agent's own behaviour
//! everywhere; it can only ever propose. Nothing here applies a soul change,
//! by construction — that requires the user saying yes.

use super::skills::SkillContext;
use super::AgentEvent;
use crate::autonomy::{autonomy_gate, Rung};
use crate::memory::{Fact, FACTS};

pub fn tool_specs() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "memory",
                "description": "Manage durable memory about the user that persists across ALL conversations. op:save stores a NEW fact (name, description, type, text required). op:update rewrites one fact's text. op:forget deletes a fact. op:read returns a fact's full text — do this before relying on details of an indexed fact. Save ONLY durable, user-relevant facts (preferences, standing decisions, stable personal/project facts). NEVER save task state, opinions, or anything the user hasn't confirmed. When in doubt, don't save.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "op": { "type": "string", "enum": ["save", "update", "forget", "read"] },
                        "name": { "type": "string", "description": "short-kebab-case-slug" },
                        "description": { "type": "string", "description": "one line for the index" },
                        "type": { "type": "string", "enum": ["preference", "fact", "decision", "project"] },
                        "text": { "type": "string", "description": "the fact body, under 1500 chars" }
                    },
                    "required": ["op"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "propose_soul_edit",
                "description": "Propose adding/changing a STANDING instruction (how the assistant should always behave) after the user has confirmed a lasting preference more than once. The user must approve; do not assume it is active.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "proposed_text": { "type": "string", "description": "the COMPLETE new SOUL.md text (existing text with your change applied)" },
                        "rationale": { "type": "string", "description": "one sentence: why" }
                    },
                    "required": ["proposed_text", "rationale"]
                }
            }
        }
    ])
}

pub fn handles(name: &str) -> bool {
    matches!(name, "memory" | "propose_soul_edit")
}

/// Human-readable (verb, target) for the timeline (§5.6 plain past-tense).
pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    let entry = args.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
    match name {
        "propose_soul_edit" => ("proposed".into(), "a standing instruction".to_string()),
        "memory" => match args.get("op").and_then(|o| o.as_str()).unwrap_or("") {
            "save" => ("remembered".into(), entry),
            "update" => ("updated memory".into(), entry),
            "forget" => ("forgot".into(), entry),
            "read" => ("recalled memory".into(), entry),
            other => (other.into(), entry),
        },
        other => (other.into(), entry),
    }
}

fn required<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("missing '{key}' argument"))
}

pub async fn execute(
    ctx: &SkillContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    match name {
        "memory" => memory_op(ctx, args),
        "propose_soul_edit" => propose_soul_edit(ctx, args),
        other => Err(format!("Memory doesn't handle '{other}'.")),
    }
}

fn memory_op(ctx: &SkillContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let op = required(args, "op")?;
    let mem = ctx.memory;

    // AUT-1: `read` changes nothing and is always allowed. Anything that writes
    // obeys the `facts` rung.
    //
    // v1 implements `auto` and `off` fully. `ask` falls back to refusing the
    // write: a fact-as-proposal has no review UI, so accepting one would be
    // impossible and the proposal would sit pending forever. The Autonomy card
    // therefore offers facts only Auto-with-undo or Off.
    if op != "read" && autonomy_gate(ctx.db, "facts") != Rung::Auto {
        return Err(
            "saving memories is turned off — carry on and mention what you'd have remembered".into(),
        );
    }

    match op {
        "save" => {
            let name = required(args, "name")?;
            let description = required(args, "description")?;
            let text = required(args, "text")?;
            let kind = args
                .get("type")
                .and_then(|t| t.as_str())
                .filter(|t| !t.trim().is_empty())
                .unwrap_or("fact");

            let saved = mem.save(
                ctx.db,
                &Fact {
                    name: name.to_string(),
                    description: description.to_string(),
                    kind: kind.to_string(),
                    created: String::new(),
                    source_conversation: Some(ctx.conversation_id.to_string()),
                    body: text.to_string(),
                },
            )?;
            announce(ctx, "save", &saved, description, "");
            Ok(format!(
                "Saved memory \"{saved}\". It is now in your index in every conversation."
            ))
        }
        "update" => {
            let name = required(args, "name")?;
            let text = required(args, "text")?;
            let description = args.get("description").and_then(|d| d.as_str());
            mem.update(ctx.db, name, description, text)?;
            let entry = mem.read(name);
            let description = description
                .map(str::to_string)
                .or_else(|| entry.map(|f| f.description))
                .unwrap_or_default();
            announce(ctx, "update", name, &description, "");
            Ok(format!("Updated \"{name}\"."))
        }
        "forget" => {
            let name = required(args, "name")?;
            let description = mem.read(name).map(|f| f.description).unwrap_or_default();
            // The returned trash filename is the token that undoes this forget.
            let token = mem.forget(ctx.db, name)?;
            announce(ctx, "forget", name, &description, &token);
            Ok(format!("Forgot \"{name}\"."))
        }
        "read" => {
            let name = required(args, "name")?;
            let fact = mem.read(name).ok_or_else(|| format!("no memory named {name}"))?;
            // Reading changes nothing, so no toast — but it is still part of the
            // record the user can audit (MEM-6).
            let _ = ctx
                .db
                .log_activity(Some(ctx.conversation_id), "memory", &format!("read {name}"));
            Ok(fact.body)
        }
        other => Err(format!(
            "unknown op '{other}' — use save, update, forget, or read"
        )),
    }
}

/// Every self-write is visible: a timeline event the UI turns into an undoable
/// toast, plus a line in the activity log the user can audit later. `undo_token`
/// carries the trash filename for a `forget`, so its Undo restores rather than
/// re-deletes; it is empty for other ops.
fn announce(ctx: &SkillContext<'_>, op: &str, name: &str, description: &str, undo_token: &str) {
    ctx.sink.emit(AgentEvent::MemoryWrite {
        op: op.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        collection: FACTS.to_string(),
        undo_token: undo_token.to_string(),
    });
    let _ = ctx
        .db
        .log_activity(Some(ctx.conversation_id), "memory", &format!("{op} {name}"));
}

fn propose_soul_edit(ctx: &SkillContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let proposed_text = required(args, "proposed_text")?;
    let rationale = required(args, "rationale")?;

    // Reject an over-long proposal now: storing one that `set_soul` will later
    // refuse leaves a card the user can never accept and a badge that never clears.
    if proposed_text.chars().count() > crate::memory::SOUL_CAP {
        return Err(format!(
            "that would make the standing instructions too long (max {} characters) — tighten it first",
            crate::memory::SOUL_CAP
        ));
    }

    let proposal = ctx
        .db
        .add_change_proposal("soul", None, proposed_text, rationale)
        .map_err(|e| e.to_string())?;

    ctx.sink.emit(AgentEvent::Proposal {
        id: proposal.id.clone(),
        target: "soul".to_string(),
        rationale: rationale.to_string(),
    });
    let _ = ctx.db.log_activity(
        Some(ctx.conversation_id),
        "memory",
        &format!("proposed a standing instruction: {rationale}"),
    );

    Ok("Proposed. The user will review it; continue without assuming it's active.".into())
}
