//! Built-in Image Generation skill (9F). Local, diffusion-based image synthesis
//! via a `stable-diffusion.cpp` CLI binary the user has installed — the diffusion
//! world's twin of `llama-server`. The model calls `generate_image`; we run the
//! CLI as a confined subprocess, then persist the PNG under app-data and surface
//! it as an image artifact in the Canvas panel (CHT-6).
//!
//! The binary + model live outside the app (user-configured in Settings), so this
//! is BYO-engine for now; a hardware-matched auto-download (mirroring the Phase-1
//! runtime manifest) is the remaining follow-up once release assets are pinned.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use super::skills::SkillContext;

/// Settings keys for the user-configured diffusion binary + model.
pub const BINARY_KEY: &str = "imagegen.binary_path";
pub const MODEL_KEY: &str = "imagegen.model_path";

/// Diffusion can take a while; allow a generous wall-clock budget.
const TIMEOUT: Duration = Duration::from_secs(300);
const DEFAULT_STEPS: i64 = 20;
const DEFAULT_SIZE: i64 = 512;

/// The OpenAI tool schema advertised to the model for this skill.
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
                        "steps": { "type": "integer", "description": "Sampling steps (optional)" }
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
    let short = if prompt.len() > 40 { &prompt[..40] } else { prompt };
    match name {
        "generate_image" => ("generated".into(), short.to_string()),
        other => (other.into(), short.to_string()),
    }
}

/// Run the diffusion CLI and surface the result as an image artifact.
pub async fn execute(
    ctx: &SkillContext<'_>,
    _name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let prompt = args
        .get("prompt")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .ok_or("missing 'prompt' argument")?;

    // Resolve the user-configured binary + model (Settings → 9F).
    let binary = ctx
        .db
        .get_setting(BINARY_KEY)
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .ok_or("Local image generation isn't set up. Add a stable-diffusion.cpp binary and a diffusion model in Settings → Local image generation.")?;
    let model = ctx
        .db
        .get_setting(MODEL_KEY)
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .ok_or("No diffusion model is configured. Add one in Settings → Local image generation.")?;

    std::fs::create_dir_all(ctx.data_dir)
        .map_err(|e| format!("couldn't create the output directory: {e}"))?;
    let out_path = ctx.data_dir.join(format!("img-{}.png", uuid::Uuid::new_v4().simple()));

    let steps = args.get("steps").and_then(|s| s.as_i64()).unwrap_or(DEFAULT_STEPS);
    let width = args.get("width").and_then(|w| w.as_i64()).unwrap_or(DEFAULT_SIZE);
    let height = args.get("height").and_then(|h| h.as_i64()).unwrap_or(DEFAULT_SIZE);
    let negative = args.get("negative_prompt").and_then(|n| n.as_str());

    generate(&binary, &model, &out_path, prompt, negative, width, height, steps).await?;

    let path_str = out_path.to_string_lossy().into_owned();
    let title = if prompt.len() > 48 { format!("{}…", &prompt[..48]) } else { prompt.to_string() };
    let artifact = ctx
        .db
        .add_artifact(Some(ctx.conversation_id), &title, "image", &path_str)
        .map_err(|e| format!("couldn't save the image artifact: {e}"))?;

    ctx.sink.artifact(&artifact.id, &title, "image", &path_str);
    let _ = ctx
        .db
        .log_activity(Some(ctx.conversation_id), "image", &format!("generated: {title}"));

    Ok(format!("Generated an image for \"{title}\" and opened it in the Canvas panel."))
}

/// Run the diffusion CLI to produce `out_path`. Shared by the chat skill and the
/// direct "Create image" command (the primary consumer path).
#[allow(clippy::too_many_arguments)]
pub async fn generate(
    binary: &str,
    model: &str,
    out_path: &Path,
    prompt: &str,
    negative: Option<&str>,
    width: i64,
    height: i64,
    steps: i64,
) -> Result<(), String> {
    if !Path::new(binary).exists() {
        return Err("The image engine isn't installed. Install it under Engine → Image.".into());
    }
    if !Path::new(model).exists() {
        return Err("No diffusion model found. Get one under Models → Image.".into());
    }
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let mut cli_args: Vec<String> = vec![
        "-m".into(), model.to_string(),
        "-p".into(), prompt.to_string(),
        "-o".into(), out_path.to_string_lossy().into_owned(),
        "--steps".into(), steps.to_string(),
        "-W".into(), width.to_string(),
        "-H".into(), height.to_string(),
        // Fit consumer GPUs (e.g. a 6 GB GTX 1060) without capping larger ones:
        // keep only the UNet on the GPU by running the text encoder and VAE on
        // CPU RAM, and decode the VAE in tiles. Without this, sd.cpp loads the
        // whole fp32 model onto the card and OOMs on <8 GB VRAM. These two are
        // backend-agnostic (a no-op on a CPU-only build, valid on Vulkan/ROCm).
        "--backend".into(), "te=cpu,vae=cpu".into(),
        "--vae-tiling".into(),
    ];
    // Flash attention further cuts UNet memory but is reliable on CUDA and only
    // sometimes supported on Vulkan/ROCm drivers — enable it only for the CUDA
    // engine build (the backend is encoded in the installed binary's path).
    if binary.to_ascii_lowercase().contains("cuda") {
        cli_args.push("--diffusion-fa".into());
    }
    if let Some(neg) = negative.filter(|n| !n.trim().is_empty()) {
        cli_args.push("-n".into());
        cli_args.push(neg.to_string());
    }

    let child = Command::new(binary)
        .args(&cli_args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("couldn't start the image generator: {e}"))?;

    let done = match tokio::time::timeout(TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return Err(format!("the image generator failed: {e}")),
        Err(_) => return Err("Image generation ran past the time limit and was stopped.".into()),
    };

    if !done.status.success() || !out_path.exists() {
        let stderr = String::from_utf8_lossy(&done.stderr);
        let tail: String = stderr
            .lines()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" ");
        return Err(format!("Image generation didn't produce an image. {tail}"));
    }
    Ok(())
}
