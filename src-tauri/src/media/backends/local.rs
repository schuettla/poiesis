//! The local, on-device backend — `stable-diffusion.cpp` run as a confined
//! subprocess. This is today's `imagegen::generate()`, moved verbatim behind
//! the `MediaBackend` trait; the CLI-driving logic is unchanged.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;

use crate::agent::imagegen::{BINARY_KEY, MODEL_KEY};
use crate::db::Db;
use crate::media::{BackendDescriptor, Credential, MediaBackend, MediaModel, MediaRequest, MediaResult, Modality};

/// Diffusion can take a while; allow a generous wall-clock budget.
const TIMEOUT: Duration = Duration::from_secs(300);
const DEFAULT_STEPS: i64 = 20;
const DEFAULT_SIZE: i64 = 512;

static DESCRIPTOR: BackendDescriptor = BackendDescriptor {
    id: "local",
    label: "On this device",
    modalities: &[Modality::Image],
    credential: Credential::Local,
    supports_references: false,
    supports_edit: false,
    is_async: false,
    console_url: None,
};

pub struct LocalBackend;

#[async_trait]
impl MediaBackend for LocalBackend {
    fn descriptor(&self) -> &'static BackendDescriptor {
        &DESCRIPTOR
    }

    /// Unlike a cloud backend, holding "no credential" is not the same as
    /// being usable: without both the engine binary and a checkpoint on disk
    /// this backend can only ever return an error, and reporting itself ready
    /// would let it shadow a perfectly good cloud backend in `resolve_backend`.
    fn is_ready(&self, db: &Db) -> bool {
        let set = |key: &str| {
            db.get_setting(key)
                .ok()
                .flatten()
                .filter(|s| !s.trim().is_empty() && Path::new(s).exists())
                .is_some()
        };
        set(BINARY_KEY) && set(MODEL_KEY)
    }

    async fn list_models(&self, db: &Db) -> Result<Vec<MediaModel>, String> {
        let model_path = db
            .get_setting(MODEL_KEY)
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty() && Path::new(s).exists());
        Ok(match model_path {
            Some(path) => {
                let name = Path::new(&path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("local model")
                    .to_string();
                vec![MediaModel {
                    id: format!("media:local/{path}"),
                    name,
                    backend_id: DESCRIPTOR.id.to_string(),
                    backend_label: DESCRIPTOR.label.to_string(),
                    modality: Modality::Image,
                    price_label: None,
                    supports_edit: false,
                    supports_streaming: false,
                    supported_aspect_ratios: vec!["1:1".to_string()],
                    supported_resolutions: vec![],
                    max_duration_secs: None,
                }]
            }
            None => vec![],
        })
    }

    /// `cancel` is honoured before the subprocess starts. Once
    /// `stable-diffusion.cpp` is sampling there is no interruption point short
    /// of killing the child, and the job layer discards the result anyway — so
    /// a late cancel costs some GPU time, not a wrong picture in the transcript.
    async fn generate(
        &self,
        db: &Db,
        req: &MediaRequest,
        out_dir: &Path,
        cancel: &crate::runtime::proxy::CancelFlag,
    ) -> Result<MediaResult, String> {
        if cancel.is_cancelled() {
            return Err(crate::media::CANCELLED.to_string());
        }
        if !req.references.is_empty() {
            return Err(
                "This model can't edit images — pick a cloud image model to refine this one.".into(),
            );
        }

        let binary = db
            .get_setting(BINARY_KEY)
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .ok_or("Local image generation isn't set up. Add a stable-diffusion.cpp binary and a diffusion model in Settings → Local image generation.")?;
        let model = db
            .get_setting(MODEL_KEY)
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .ok_or("No diffusion model is configured. Add one in Settings → Local image generation.")?;

        std::fs::create_dir_all(out_dir).map_err(|e| format!("couldn't create the output directory: {e}"))?;
        let out_path = out_dir.join(format!("img-{}.png", uuid::Uuid::new_v4().simple()));

        let width = req.width.unwrap_or(DEFAULT_SIZE);
        let height = req.height.unwrap_or(DEFAULT_SIZE);
        let steps = req.steps.unwrap_or(DEFAULT_STEPS);

        run_cli(&binary, &model, &out_path, &req.prompt, req.negative.as_deref(), width, height, steps).await?;

        let model_name = Path::new(&model)
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("local")
            .to_string();

        let mut ignored = Vec::new();
        if req.aspect_ratio.is_some() {
            ignored.push("aspect_ratio".to_string());
        }
        if req.resolution.is_some() {
            ignored.push("resolution".to_string());
        }

        Ok(MediaResult {
            path: out_path,
            mime: "image/png".to_string(),
            width: Some(width as u32),
            height: Some(height as u32),
            duration_secs: None,
            model_id: format!("local:{model_name}"),
            provider_label: model_name,
            seed: req.seed,
            cost_usd: None,
            ignored,
        })
    }
}

/// Run the diffusion CLI to produce `out_path`.
#[allow(clippy::too_many_arguments)]
async fn run_cli(
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
