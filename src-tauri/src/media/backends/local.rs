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
use crate::media::imagecatalog::{self, ModelProfile};
use crate::media::{BackendDescriptor, Credential, MediaBackend, MediaModel, MediaRequest, MediaResult, Modality};

/// Diffusion can take a while, and the multi-file families are slower to load
/// than a single SD checkpoint; allow a generous wall-clock budget.
const TIMEOUT: Duration = Duration::from_secs(900);

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

        // What the model needs in order to be any good: its native resolution,
        // step count and guidance scale. An explicit request still wins — this
        // only replaces the old one-size-fits-all 512px/20-step/cfg-7 default,
        // which quietly ruined every SDXL and every distilled model.
        let profile = imagecatalog::profile_for(Path::new(&model));
        let width = req.width.unwrap_or(profile.size);
        let height = req.height.unwrap_or(profile.size);
        let steps = req.steps.unwrap_or(profile.steps);

        run_cli(
            &binary,
            &model,
            &out_path,
            &req.prompt,
            req.negative.as_deref(),
            width,
            height,
            steps,
            &profile,
        )
        .await?;

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

/// Build the model-loading half of the command line. A single-file checkpoint
/// is just `-m`; the newer families are assembled from a directory of parts,
/// each on its own flag, so the roles recorded in the bundle manifest are what
/// decide the arguments.
fn model_args(model: &str, profile: &ModelProfile) -> Result<Vec<String>, String> {
    let path = Path::new(model);
    if !profile.arch.is_bundle() {
        return Ok(vec!["-m".into(), model.to_string()]);
    }

    let manifest = imagecatalog::read_manifest(path).ok_or_else(|| {
        format!(
            "This model's files are incomplete — {} is missing. Re-download it under Models → Image.",
            imagecatalog::MANIFEST_NAME
        )
    })?;

    let mut args = Vec::new();
    for (role, flag) in [
        ("diffusion", "--diffusion-model"),
        ("uncond_diffusion", "--uncond-diffusion-model"),
        ("vae", "--vae"),
        ("llm", "--llm"),
        ("clip_l", "--clip_l"),
        ("t5xxl", "--t5xxl"),
    ] {
        if let Some(name) = manifest.files.get(role) {
            let part = path.join(name);
            if !part.exists() {
                return Err(format!(
                    "This model is missing its {role} file ({name}). Re-download it under Models → Image."
                ));
            }
            args.push(flag.to_string());
            args.push(part.to_string_lossy().into_owned());
        }
    }
    Ok(args)
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
    profile: &ModelProfile,
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

    let bundle = profile.arch.is_bundle();
    let mut cli_args = model_args(model, profile)?;
    cli_args.extend([
        "-p".into(), prompt.to_string(),
        "-o".into(), out_path.to_string_lossy().into_owned(),
        "--steps".into(), steps.to_string(),
        "-W".into(), width.to_string(),
        "-H".into(), height.to_string(),
        // The engine's built-in default is 7.0 regardless of model. Distilled
        // models ("Turbo", "schnell") are trained for 1.0 and come out burnt
        // and oversaturated at 7, so this is never left to the default.
        "--cfg-scale".into(), format!("{}", profile.cfg_scale),
        "--vae-tiling".into(),
    ]);
    if let Some(sampler) = &profile.sampling {
        cli_args.push("--sampling-method".into());
        cli_args.push(sampler.clone());
    }
    if let Some(shift) = profile.flow_shift {
        cli_args.push("--flow-shift".into());
        cli_args.push(format!("{shift}"));
    }
    if bundle {
        // The multi-file families are far larger than SDXL; the documented way
        // to fit them on a consumer card is to stage weights through CPU RAM.
        cli_args.push("--offload-to-cpu".into());
    } else {
        // Fit consumer GPUs (e.g. a 6 GB GTX 1060) without capping larger ones:
        // keep only the UNet on the GPU by running the text encoder and VAE on
        // CPU RAM. Not used for bundles, whose encoders are separate files the
        // engine places itself.
        cli_args.push("--backend".into());
        cli_args.push("te=cpu,vae=cpu".into());
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::imagecatalog::{Architecture, BundleManifest, MANIFEST_NAME};
    use std::collections::BTreeMap;

    /// Lay down a bundle directory: the named parts plus a manifest.
    fn bundle(arch: Architecture, roles: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let mut files = BTreeMap::new();
        for (role, name) in roles {
            std::fs::write(dir.path().join(name), b"x").unwrap();
            files.insert(role.to_string(), name.to_string());
        }
        let manifest = BundleManifest {
            name: "Test Model".into(),
            profile: ModelProfile {
                arch,
                cfg_scale: 1.0,
                steps: 8,
                size: 1024,
                sampling: None,
                flow_shift: None,
            },
            files,
        };
        std::fs::write(
            dir.path().join(MANIFEST_NAME),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();
        dir
    }

    fn profile(arch: Architecture) -> ModelProfile {
        ModelProfile { arch, cfg_scale: 1.0, steps: 8, size: 1024, sampling: None, flow_shift: None }
    }

    #[test]
    fn single_file_models_still_load_with_dash_m() {
        let args = model_args("C:/models/sd15.safetensors", &profile(Architecture::Sd1)).unwrap();
        assert_eq!(args, vec!["-m".to_string(), "C:/models/sd15.safetensors".to_string()]);
    }

    #[test]
    fn z_image_gets_its_three_component_flags() {
        let dir = bundle(
            Architecture::ZImage,
            &[("diffusion", "d.gguf"), ("vae", "ae.safetensors"), ("llm", "q3.gguf")],
        );
        let args =
            model_args(&dir.path().to_string_lossy(), &profile(Architecture::ZImage)).unwrap();
        let flags: Vec<&String> = args.iter().step_by(2).collect();
        assert_eq!(flags, vec!["--diffusion-model", "--vae", "--llm"]);
        // Every flag must be followed by a real path, not a bare filename.
        for v in args.iter().skip(1).step_by(2) {
            assert!(Path::new(v).exists(), "{v} should exist");
        }
    }

    #[test]
    fn flux_uses_clip_and_t5_rather_than_llm() {
        let dir = bundle(
            Architecture::Flux,
            &[
                ("diffusion", "flux.gguf"),
                ("vae", "ae.safetensors"),
                ("clip_l", "clip_l.safetensors"),
                ("t5xxl", "t5.safetensors"),
            ],
        );
        let args = model_args(&dir.path().to_string_lossy(), &profile(Architecture::Flux)).unwrap();
        let flags: Vec<&String> = args.iter().step_by(2).collect();
        assert_eq!(flags, vec!["--diffusion-model", "--vae", "--clip_l", "--t5xxl"]);
    }

    #[test]
    fn ideogram_passes_its_second_unconditional_transformer() {
        let dir = bundle(
            Architecture::Ideogram4,
            &[
                ("diffusion", "i4.gguf"),
                ("uncond_diffusion", "i4_uncond.gguf"),
                ("vae", "ae.safetensors"),
                ("llm", "qwen3vl.gguf"),
            ],
        );
        let args =
            model_args(&dir.path().to_string_lossy(), &profile(Architecture::Ideogram4)).unwrap();
        let flags: Vec<&String> = args.iter().step_by(2).collect();
        assert_eq!(
            flags,
            vec!["--diffusion-model", "--uncond-diffusion-model", "--vae", "--llm"]
        );
    }

    #[test]
    fn a_missing_part_is_reported_rather_than_handed_to_the_engine() {
        let dir = bundle(Architecture::ZImage, &[("diffusion", "d.gguf"), ("vae", "ae.safetensors")]);
        std::fs::remove_file(dir.path().join("ae.safetensors")).unwrap();
        let err = model_args(&dir.path().to_string_lossy(), &profile(Architecture::ZImage))
            .unwrap_err();
        assert!(err.contains("vae"), "error should name the missing role: {err}");
    }

    #[test]
    fn a_bundle_without_a_manifest_is_not_silently_run_as_a_single_file() {
        let dir = tempfile::tempdir().unwrap();
        let err =
            model_args(&dir.path().to_string_lossy(), &profile(Architecture::ZImage)).unwrap_err();
        assert!(err.contains(MANIFEST_NAME), "{err}");
    }
}
