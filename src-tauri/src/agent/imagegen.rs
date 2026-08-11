//! Built-in Image Generation toolset (9F). The model calls `generate_image`;
//! this toolset turns that into a `media::MediaRequest`, resolves a backend
//! through the `media::Registry` (today: local `stable-diffusion.cpp` only),
//! and records the result as a real artifact via `media::record` — the same
//! path the composer's direct "Create image" command uses (`ART-2`).

use super::toolsets::{set_step_note, ToolContext};
use crate::media::{self, MediaRef, MediaRequest, Modality};

/// Settings keys for the user-configured diffusion binary + model. Read by
/// the local backend (`media::backends::local`) and by the Settings UI.
pub const BINARY_KEY: &str = "imagegen.binary_path";
pub const MODEL_KEY: &str = "imagegen.model_path";

/// The OpenAI tool schema advertised to the model for this toolset.
pub fn tool_specs() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "generate_image",
                "description": "Generate an image locally from a text prompt using the on-device diffusion model. Returns an image shown in the Canvas panel.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string", "description": "What to depict" },
                        "negative_prompt": { "type": "string", "description": "What to avoid (optional)" },
                        "width": { "type": "integer", "description": "Image width in px (optional)" },
                        "height": { "type": "integer", "description": "Image height in px (optional)" },
                        "steps": { "type": "integer", "description": "Sampling steps (optional)" },
                        "reference_images": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Paths or artifact ids to edit from or take style from (optional, up to 8). Only images already visible in this conversation may be used."
                        },
                        "reference_role": {
                            "type": "string",
                            "enum": ["source", "style"],
                            "description": "Whether the reference images are the thing being edited ('source', the default) or only a look to borrow ('style')."
                        }
                    },
                    "required": ["prompt"]
                }
            }
        }
    ])
}

/// Is this an Image Generation tool name?
pub fn handles(name: &str) -> bool {
    name == "generate_image"
}

/// Human-readable (verb, target) for the timeline (§5.6 plain past-tense).
pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    let prompt = args.get("prompt").and_then(|p| p.as_str()).unwrap_or("image");
    let short = media::ellipsize(prompt, 40);
    match name {
        "generate_image" => ("generated".into(), short),
        other => (other.into(), short),
    }
}

/// Resolve a backend, generate, and surface the result as an image artifact.
pub async fn execute(
    ctx: &ToolContext<'_>,
    _name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let prompt = args
        .get("prompt")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .ok_or("missing 'prompt' argument")?;

    // `EDT-1`: a reference has to already be something this conversation
    // showed the user — an artifact it made or a file the user attached —
    // never an arbitrary path the model names, which a tool argument is not
    // trusted enough to authorize on its own.
    let raw_refs: Vec<String> = args
        .get("reference_images")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    if raw_refs.len() > 8 {
        return Err("Up to 8 reference images are supported.".into());
    }
    let role = match args.get("reference_role").and_then(|r| r.as_str()) {
        Some("style") => media::RefRole::Style,
        _ => media::RefRole::Source,
    };
    let mut references = Vec::with_capacity(raw_refs.len());
    for path in raw_refs {
        let known = ctx.db.is_known_attachment(&path).unwrap_or(false)
            || ctx.db.is_known_artifact_content(&path).unwrap_or(false);
        if !known {
            return Err(format!("\"{path}\" isn't an image from this conversation."));
        }
        references.push(MediaRef { path: std::path::PathBuf::from(path), role });
    }

    let req = MediaRequest {
        prompt: prompt.to_string(),
        negative: args.get("negative_prompt").and_then(|n| n.as_str()).map(str::to_string),
        width: args.get("width").and_then(|w| w.as_i64()),
        height: args.get("height").and_then(|h| h.as_i64()),
        steps: args.get("steps").and_then(|s| s.as_i64()),
        references,
        ..Default::default()
    };

    // `JOB-1`: submit and return. This used to block the whole agent loop for
    // up to 300s, which meant the user could not say anything else — not even
    // "no, not like that" — until the picture landed. The job records the
    // conversation and message it belongs to, so its result reaches this turn
    // whether or not the run is still going when it arrives.
    let job = media::jobs::submit(
        ctx.db,
        media::jobs::SubmitArgs {
            conversation_id: Some(ctx.conversation_id.to_string()),
            message_id: ctx.assistant_message_id.map(str::to_string),
            modality: Modality::Image,
            model_id: None,
            request: req,
            parent_artifact_id: None,
        },
    )?;

    let _ = ctx
        .db
        .log_activity(Some(ctx.conversation_id), "image", &format!("generating: {prompt}"));
    set_step_note(ctx, "— generating");

    Ok(format!(
        "Image generation started (job {}). It will appear in this conversation on its own when it finishes — \
         don't wait for it, and don't call this tool again for the same picture.",
        job.id
    ))
}
