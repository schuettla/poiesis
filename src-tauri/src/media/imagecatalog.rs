//! The diffusion-model catalog and the per-model generation settings that make
//! each one actually produce good images.
//!
//! Two things live here because they are the same fact seen from two sides:
//! what a model *is* (its architecture and which files it needs) and how it
//! must be *driven* (cfg-scale, step count, native resolution, sampler). The
//! engine has no idea what it has been handed — `stable-diffusion.cpp` applies
//! one built-in default cfg of 7.0 to everything — so a distilled Turbo model
//! run at cfg 7 comes out scorched, and an SDXL model run at 512px (its native
//! size is 1024) comes out with mangled composition. Keeping the settings
//! beside the download URL is what stops the two drifting apart.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::runtime::imageengine::{DEFAULT_MODEL_NAME, DEFAULT_MODEL_URL};

/// Written into a multi-file model's directory so the generator can rediscover
/// which file plays which role without consulting the catalog (the entry may
/// have changed, or the model may have been sideloaded).
pub const MANIFEST_NAME: &str = "poiesis-model.json";

/// How the engine has to be invoked for a given model family. Single-file
/// checkpoints take `-m`; everything newer ships the diffusion transformer,
/// the VAE and the text encoder(s) as separate files with their own flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Architecture {
    /// Stable Diffusion 1.x — one file, native 512px.
    Sd1,
    /// SDXL and its finetunes — one file, native 1024px.
    Sdxl,
    /// FLUX — diffusion + vae + clip_l + t5xxl.
    Flux,
    /// Z-Image — diffusion + vae + a Qwen3 LLM as the text encoder.
    ZImage,
    /// Qwen-Image — diffusion + vae + a Qwen2.5-VL LLM as the text encoder.
    QwenImage,
    /// Ideogram 4 — as above, plus a second *unconditional* transformer of the
    /// same size that the engine takes on its own flag.
    Ideogram4,
}

impl Architecture {
    /// Whether this family ships as several files in a directory rather than a
    /// single checkpoint.
    pub fn is_bundle(&self) -> bool {
        !matches!(self, Architecture::Sd1 | Architecture::Sdxl)
    }
}

/// The settings a model needs to produce its intended output. These are
/// defaults: an explicit width/height/steps from the caller still wins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProfile {
    pub arch: Architecture,
    /// Classifier-free guidance. Distilled ("Turbo"/"schnell") models are
    /// trained for 1.0 and burn out at the engine's default of 7.0.
    pub cfg_scale: f32,
    pub steps: i64,
    /// Native training resolution; generating far from it degrades composition.
    pub size: i64,
    pub sampling: Option<String>,
    pub flow_shift: Option<f32>,
}

impl ModelProfile {
    fn new(arch: Architecture, cfg_scale: f32, steps: i64, size: i64) -> Self {
        Self { arch, cfg_scale, steps, size, sampling: None, flow_shift: None }
    }
    fn sampler(mut self, s: &str) -> Self {
        self.sampling = Some(s.to_string());
        self
    }
    fn shift(mut self, f: f32) -> Self {
        self.flow_shift = Some(f);
        self
    }
}

/// One file of a model, tagged with the engine flag it belongs to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageComponent {
    /// "model" (single-file), "diffusion", "vae", "llm", "clip_l", "t5xxl".
    pub role: String,
    pub url: String,
    pub filename: String,
    pub size_bytes: u64,
}

/// A downloadable model offering.
#[derive(Debug, Clone, Serialize)]
pub struct ImageCatalogEntry {
    /// Stable slug; doubles as the directory name for multi-file models.
    pub id: String,
    pub name: String,
    pub note: String,
    pub size_label: String,
    pub arch: Architecture,
    pub components: Vec<ImageComponent>,
    /// Total download in bytes, for progress and the size label.
    pub total_bytes: u64,
    /// Surfaced so the UI can show what the model will actually be run at —
    /// the settings are the difference between a good image and a ruined one,
    /// so they should not be a hidden implementation detail.
    pub profile: ModelProfile,
}

impl ImageCatalogEntry {
    /// Bytes that have to live on the GPU: the diffusion transformer alone.
    /// The text encoders and VAE are pushed to CPU RAM by the flags the engine
    /// is given, which is why Z-Image runs on a 4 GB card despite being a 6 GB
    /// download — judging it on the download size would wrongly rule it out.
    pub fn vram_bytes(&self) -> u64 {
        self.components
            .iter()
            .find(|c| c.role == "model" || c.role == "diffusion")
            .map(|c| c.size_bytes)
            .unwrap_or(self.total_bytes)
    }

    /// Bytes that stay in system RAM: everything that isn't the transformer.
    pub fn host_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.vram_bytes())
    }
}

/// What a downloaded bundle leaves on disk beside its files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleManifest {
    pub name: String,
    pub profile: ModelProfile,
    /// role -> filename, relative to the bundle directory.
    pub files: BTreeMap<String, String>,
}

fn hf(repo: &str, file: &str) -> String {
    format!("https://huggingface.co/{repo}/resolve/main/{file}?download=true")
}

fn component(role: &str, repo: &str, path: &str, size_bytes: u64) -> ImageComponent {
    ImageComponent {
        role: role.into(),
        url: hf(repo, path),
        // Bundles are stored flat, so a nested source path keeps only its leaf.
        filename: path.rsplit('/').next().unwrap_or(path).to_string(),
        size_bytes,
    }
}

const GB: u64 = 1024 * 1024 * 1024;
const MB: u64 = 1024 * 1024;

fn single(
    id: &str,
    name: &str,
    note: &str,
    url: String,
    filename: &str,
    size_bytes: u64,
    profile: ModelProfile,
) -> ImageCatalogEntry {
    entry(
        id,
        name,
        note,
        profile,
        vec![ImageComponent { role: "model".into(), url, filename: filename.into(), size_bytes }],
    )
}

fn entry(
    id: &str,
    name: &str,
    note: &str,
    profile: ModelProfile,
    components: Vec<ImageComponent>,
) -> ImageCatalogEntry {
    let total_bytes: u64 = components.iter().map(|c| c.size_bytes).sum();
    ImageCatalogEntry {
        id: id.into(),
        name: name.into(),
        note: note.into(),
        size_label: format!("~{:.1} GB", total_bytes as f64 / GB as f64),
        arch: profile.arch,
        components,
        total_bytes,
        profile,
    }
}

/// The curated diffusion catalog. Every URL is an un-gated Hugging Face
/// `resolve` link, so downloads work without an account or token — that rules
/// out FLUX.1-dev and Ideogram (both licence-gated) but not FLUX.1-schnell,
/// which is Apache-2.0 and mirrored un-gated.
pub fn image_catalog() -> Vec<ImageCatalogEntry> {
    use Architecture::*;
    vec![
        single(
            "sd-1-5",
            "Stable Diffusion 1.5",
            "Fast and light. Runs on almost anything — great default.",
            DEFAULT_MODEL_URL.into(),
            DEFAULT_MODEL_NAME,
            4 * GB,
            ModelProfile::new(Sd1, 7.0, 20, 512),
        ),
        single(
            "sd-turbo",
            "SD-Turbo",
            "Single-step 512px generation — near-instant. Best at 1–4 steps.",
            hf("stabilityai/sd-turbo", "sd_turbo.safetensors"),
            "sd_turbo.safetensors",
            4900 * MB,
            ModelProfile::new(Sd1, 1.0, 4, 512),
        ),
        single(
            "sdxl-turbo",
            "SDXL-Turbo",
            "SDXL quality in 1–4 steps. Fast and sharp; the go-to for most users.",
            hf("stabilityai/sdxl-turbo", "sd_xl_turbo_1.0_fp16.safetensors"),
            "sd_xl_turbo_1.0_fp16.safetensors",
            6500 * MB,
            ModelProfile::new(Sdxl, 1.0, 4, 1024),
        ),
        single(
            "sdxl-base",
            "Stable Diffusion XL (base 1.0)",
            "The full SDXL base — top all-round quality. Best at 25–40 steps.",
            hf("stabilityai/stable-diffusion-xl-base-1.0", "sd_xl_base_1.0.safetensors"),
            "sd_xl_base_1.0.safetensors",
            6900 * MB,
            ModelProfile::new(Sdxl, 7.0, 30, 1024),
        ),
        single(
            "dreamshaper-xl-turbo",
            "DreamShaper XL (Turbo v2)",
            "Versatile SDXL finetune — art + photoreal, fast at ~6 steps.",
            hf("Lykon/dreamshaper-xl-v2-turbo", "DreamShaperXL_Turbo_v2_1.safetensors"),
            "DreamShaperXL_Turbo_v2_1.safetensors",
            6500 * MB,
            ModelProfile::new(Sdxl, 2.0, 8, 1024),
        ),
        single(
            "juggernaut-xl-v9",
            "Juggernaut XL (v9)",
            "State-of-the-art SDXL photorealism. Best at 30–40 steps.",
            hf("RunDiffusion/Juggernaut-XL-v9", "Juggernaut-XL_v9_RunDiffusionPhoto_v2.safetensors"),
            "Juggernaut-XL_v9_RunDiffusionPhoto_v2.safetensors",
            6600 * MB,
            ModelProfile::new(Sdxl, 6.0, 35, 1024),
        ),
        single(
            "realvisxl-v4",
            "RealVisXL (V4.0)",
            "Photorealistic portraits & scenes. Best at 25–35 steps.",
            hf("SG161222/RealVisXL_V4.0", "RealVisXL_V4.0.safetensors"),
            "RealVisXL_V4.0.safetensors",
            6500 * MB,
            ModelProfile::new(Sdxl, 6.0, 30, 1024),
        ),
        single(
            "playground-v2-5",
            "Playground v2.5",
            "High-aesthetic 1024px generations — vivid colour and contrast.",
            hf(
                "playgroundai/playground-v2.5-1024px-aesthetic",
                "playground-v2.5-1024px-aesthetic.fp16.safetensors",
            ),
            "playground-v2.5-1024px-aesthetic.fp16.safetensors",
            6500 * MB,
            ModelProfile::new(Sdxl, 3.0, 30, 1024),
        ),
        // ---- Multi-file families -------------------------------------------
        // Each ships the transformer, VAE and text encoder(s) separately. The
        // VAE and encoders are pulled from un-gated mirrors: the upstream docs
        // point at black-forest-labs/FLUX.1-schnell for Z-Image's VAE, which is
        // gated and would fail without a token.
        entry(
            "z-image-turbo",
            "Z-Image-Turbo",
            "Excellent quality at 8 steps, and light enough for 4 GB of VRAM. The best all-round local model here.",
            ModelProfile::new(ZImage, 1.0, 8, 1024),
            vec![
                component("diffusion", "leejet/Z-Image-Turbo-GGUF", "z_image_turbo-Q4_K.gguf", 3865470566),
                component("vae", "Comfy-Org/z_image_turbo", "split_files/vae/ae.safetensors", 335304388),
                component(
                    "llm",
                    "unsloth/Qwen3-4B-Instruct-2507-GGUF",
                    "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
                    2497281696,
                ),
            ],
        ),
        entry(
            "flux-1-schnell",
            "FLUX.1-schnell",
            "Flagship-class prompt following and text rendering, in 4 steps. Apache-2.0 (unlike FLUX.1-dev).",
            ModelProfile::new(Flux, 1.0, 4, 1024).sampler("euler"),
            vec![
                component("diffusion", "city96/FLUX.1-schnell-gguf", "flux1-schnell-Q4_K_S.gguf", 6786321440),
                component("vae", "unsloth/FLUX.1-schnell", "ae.safetensors", 335304388),
                component("clip_l", "comfyanonymous/flux_text_encoders", "clip_l.safetensors", 246144152),
                component(
                    "t5xxl",
                    "comfyanonymous/flux_text_encoders",
                    "t5xxl_fp8_e4m3fn.safetensors",
                    4893934904,
                ),
            ],
        ),
        entry(
            "qwen-image",
            "Qwen-Image",
            "Outstanding text rendering, including Chinese. The heaviest model here — wants a lot of memory.",
            ModelProfile::new(QwenImage, 2.5, 20, 1024).sampler("euler").shift(3.0),
            vec![
                component("diffusion", "QuantStack/Qwen-Image-GGUF", "Qwen_Image-Q4_K_M.gguf", 13067089920),
                component(
                    "vae",
                    "Comfy-Org/Qwen-Image_ComfyUI",
                    "split_files/vae/qwen_image_vae.safetensors",
                    257725640,
                ),
                component(
                    "llm",
                    "mradermacher/Qwen2.5-VL-7B-Instruct-GGUF",
                    "Qwen2.5-VL-7B-Instruct.Q4_K_M.gguf",
                    4683073152,
                ),
            ],
        ),
        // The diffusion weights are the sd.cpp author's own conversion; the
        // upstream fp8 release would otherwise need a Python conversion pass
        // before the engine could read it. The VAE comes from Comfy-Org rather
        // than the documented FLUX.2-dev, which is gated.
        entry(
            "ideogram-4",
            "Ideogram 4",
            "The strongest text rendering and layout control available locally. Non-commercial licence; downloads two transformers, so it is the largest here.",
            ModelProfile::new(Ideogram4, 7.0, 20, 1024),
            vec![
                component("diffusion", "leejet/ideogram-4-GGUF", "ideogram4-Q4_0.gguf", 5648121856),
                component(
                    "uncond_diffusion",
                    "leejet/ideogram-4-GGUF",
                    "ideogram4_uncond-Q4_0.gguf",
                    5648121856,
                ),
                component("vae", "Comfy-Org/Ideogram-4", "vae/flux2-vae.safetensors", 332859392),
                component(
                    "llm",
                    "unsloth/Qwen3-VL-8B-Instruct-GGUF",
                    "Qwen3-VL-8B-Instruct-Q4_K_M.gguf",
                    5025112739,
                ),
            ],
        ),
    ]
}

/// The settings to drive `path` with. `path` is either a bundle directory (its
/// manifest is authoritative) or a single checkpoint file, in which case the
/// catalog is consulted by filename and, failing that, the name is read for the
/// usual conventions. The fallback is a guess and is deliberately conservative:
/// getting the resolution right matters far more than the exact cfg.
pub fn profile_for(path: &Path) -> ModelProfile {
    if let Some(m) = read_manifest(path) {
        return m.profile;
    }

    let file = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if let Some(hit) = image_catalog().into_iter().find(|e| {
        e.components
            .iter()
            .any(|c| c.filename.to_ascii_lowercase() == file)
    }) {
        return hit.profile;
    }

    // Sideloaded file: infer from the name.
    let xl = file.contains("xl") || file.contains("1024");
    let distilled = ["turbo", "schnell", "lightning", "lcm", "hyper"]
        .iter()
        .any(|k| file.contains(k));
    let size = if xl { 1024 } else { 512 };
    let arch = if xl { Architecture::Sdxl } else { Architecture::Sd1 };
    if distilled {
        ModelProfile::new(arch, 1.0, 6, size)
    } else {
        ModelProfile::new(arch, 7.0, 25, size)
    }
}

/// Read a bundle manifest from a model directory, if `path` is one.
pub fn read_manifest(path: &Path) -> Option<BundleManifest> {
    if !path.is_dir() {
        return None;
    }
    let raw = std::fs::read_to_string(path.join(MANIFEST_NAME)).ok()?;
    serde_json::from_str(&raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_ids_and_filenames_are_unique() {
        let cat = image_catalog();
        let mut ids: Vec<_> = cat.iter().map(|e| e.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), cat.len(), "catalog ids must be unique");
        for e in &cat {
            assert!(!e.components.is_empty(), "{} has no components", e.id);
            assert_eq!(e.arch.is_bundle(), e.components.len() > 1, "{} arch/shape mismatch", e.id);
        }
    }

    #[test]
    fn every_catalog_model_resolves_to_its_own_profile() {
        // The whole point of the table: a Turbo model must not inherit cfg 7.
        for e in image_catalog().into_iter().filter(|e| !e.arch.is_bundle()) {
            let p = profile_for(Path::new(&e.components[0].filename));
            assert_eq!(p.cfg_scale, e.profile.cfg_scale, "{} cfg", e.id);
            assert_eq!(p.size, e.profile.size, "{} size", e.id);
            assert_eq!(p.steps, e.profile.steps, "{} steps", e.id);
        }
    }

    #[test]
    fn sdxl_models_are_never_driven_at_512() {
        for e in image_catalog() {
            if matches!(e.arch, Architecture::Sdxl) {
                assert_eq!(e.profile.size, 1024, "{} must render at its native size", e.id);
            }
        }
    }

    #[test]
    fn a_bundle_is_judged_on_its_transformer_not_its_download_size() {
        // Z-Image is a 6 GB download but upstream documents it running on 4 GB
        // of VRAM, because only the transformer is GPU-resident. Sizing it by
        // the download would wrongly rule it out on exactly the mid-range cards
        // it was built for.
        let z = image_catalog().into_iter().find(|e| e.id == "z-image-turbo").unwrap();
        assert!(z.vram_bytes() < z.total_bytes, "the encoders are not GPU-resident");
        assert!(z.vram_bytes() < 4 * GB, "should fit a 4 GB card");
        assert_eq!(z.vram_bytes() + z.host_bytes(), z.total_bytes);
    }

    #[test]
    fn every_model_has_a_gpu_resident_component_to_size_against() {
        for e in image_catalog() {
            assert!(e.vram_bytes() > 0, "{} has no diffusion component", e.id);
            assert!(e.vram_bytes() <= e.total_bytes, "{} vram exceeds total", e.id);
        }
    }

    #[test]
    fn unknown_files_are_inferred_from_their_name() {
        let xl = profile_for(Path::new("someRandom_XL_finetune.safetensors"));
        assert_eq!(xl.size, 1024);
        let turbo = profile_for(Path::new("mystery-turbo.safetensors"));
        assert_eq!(turbo.cfg_scale, 1.0);
        let plain = profile_for(Path::new("whatever.safetensors"));
        assert_eq!(plain.size, 512);
        assert_eq!(plain.cfg_scale, 7.0);
    }
}
