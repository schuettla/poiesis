//! The Recipes skill (RCP-2): procedures the agent developed with the user and
//! may reuse.
//!
//! A recipe is *identity*, not a note — it changes how the agent will approach
//! a whole class of future task — so creation is ask-first by construction:
//! `propose_recipe` can only ever write a proposal. `use_recipe` is the cheap
//! half: reading back a procedure the user already approved, and counting the
//! use so an unused recipe is visibly unused.

use super::skills::SkillContext;
use super::AgentEvent;
use crate::autonomy::{autonomy_gate, Rung};
use crate::memory::{render_recipe, Recipe};

pub fn tool_specs() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "propose_recipe",
                "description": "Propose saving a reusable PROCEDURE after completing a multi-step task the user is likely to repeat. The user must approve it; continue without assuming it exists. Include a surface template only if a workspace surface was central to the task.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "short-kebab-case-slug" },
                        "description": { "type": "string", "description": "one line" },
                        "trigger": { "type": "string", "description": "one line: when to use this recipe" },
                        "steps": { "type": "string", "description": "numbered steps, imperative, under 2000 chars" },
                        "surface_json": { "type": "string", "description": "OPTIONAL: the render_ui tree JSON to start the workspace from" }
                    },
                    "required": ["name", "description", "trigger", "steps"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "use_recipe",
                "description": "Read a saved recipe's full steps before executing a task it covers. Increments its usage count.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    },
                    "required": ["name"]
                }
            }
        }
    ])
}

pub fn handles(name: &str) -> bool {
    matches!(name, "propose_recipe" | "use_recipe")
}

/// Human-readable (verb, target) for the timeline (§5.6 plain past-tense).
pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    let entry = args.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
    match name {
        "propose_recipe" => ("proposed a procedure".into(), entry),
        "use_recipe" => ("followed the recipe".into(), entry),
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
        "propose_recipe" => propose_recipe(ctx, args),
        "use_recipe" => use_recipe(ctx, args),
        other => Err(format!("Recipes doesn't handle '{other}'.")),
    }
}

fn propose_recipe(ctx: &SkillContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    if autonomy_gate(ctx.db, "recipes") == Rung::Off {
        return Err("keeping procedures is turned off — carry on without saving one".into());
    }
    let name = crate::memory::slugify(required(args, "name")?)?;
    let description = required(args, "description")?;
    let trigger = required(args, "trigger")?;
    let steps = required(args, "steps")?;
    let surface_json = args
        .get("surface_json")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty());

    // Validate now, not at accept time: a proposal the user can never apply is
    // worse than a tool error the model can still fix this turn (GRM-3 retry).
    if steps.chars().count() > crate::memory::RECIPE_STEPS_CAP {
        return Err(format!(
            "keep the steps under {} characters",
            crate::memory::RECIPE_STEPS_CAP
        ));
    }
    if let Some(json) = surface_json {
        serde_json::from_str::<serde_json::Value>(json)
            .map_err(|e| format!("the surface template isn't valid JSON: {e}"))?;
    }

    // Store the complete future file, so accepting is a parse and a write and
    // the user reviews exactly what will land on disk.
    let file = render_recipe(&Recipe {
        name: name.clone(),
        description: description.to_string(),
        trigger: trigger.to_string(),
        created: String::new(),
        used: 0,
        last_used: None,
        steps: steps.to_string(),
        surface_json: surface_json.map(str::to_string),
    });

    let proposal = ctx
        .db
        .add_change_proposal("recipe", Some(&name), &file, description)
        .map_err(|e| e.to_string())?;

    ctx.sink.emit(AgentEvent::Proposal {
        id: proposal.id,
        target: "recipe".to_string(),
        rationale: description.to_string(),
    });
    let _ = ctx.db.log_activity(
        Some(ctx.conversation_id),
        "memory",
        &format!("proposed the recipe {name}"),
    );

    Ok(format!(
        "Proposed recipe \"{name}\". The user will review it; continue normally."
    ))
}

fn use_recipe(ctx: &SkillContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let name = required(args, "name")?;
    let Some(recipe) = ctx.memory.read_recipe(name) else {
        let available = ctx
            .memory
            .list_recipes()
            .into_iter()
            .map(|r| r.name)
            .collect::<Vec<_>>();
        return Err(if available.is_empty() {
            "there are no saved recipes yet".to_string()
        } else {
            format!("no recipe named {name} — available: {}", available.join(", "))
        });
    };
    let _ = ctx.memory.touch_recipe(ctx.db, &recipe.name);
    let _ = ctx.db.log_activity(
        Some(ctx.conversation_id),
        "memory",
        &format!("used the recipe {}", recipe.name),
    );
    // The surface template is deliberately withheld: it belongs to starting a
    // workspace from the recipe (RCP-UI-2), not to reading the procedure.
    Ok(format!(
        "Recipe \"{}\" — use when: {}\n{}",
        recipe.name, recipe.trigger, recipe.steps
    ))
}
