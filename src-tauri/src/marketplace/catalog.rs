//! Unified catalog model + hardware-fit classifier.

use serde::{Deserialize, Serialize};

use crate::runtime::hardware::HardwareProfile;

/// A model offering normalized across sources (curated, Hugging Face, GitHub).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogModel {
    /// Stable id, e.g. "hf:bartowski/Llama-3.2-3B-Instruct-GGUF:Q4_K_M".
    pub id: String,
    pub name: String,
    /// One-line plain description (§5.4.2).
    pub description: String,
    /// Quantization label, e.g. "Q4_K_M".
    pub quant: String,
    /// Approximate on-disk size in MB (drives the fit estimate + display).
    pub size_mb: u64,
    pub vision: bool,
    /// Direct download URL for the GGUF file.
    pub url: String,
    /// "huggingface" | "github" | "curated".
    pub source: String,
    pub license: Option<String>,
}

/// Hardware-fit verdict for a model on this machine (MKT-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fit {
    /// Fits comfortably in VRAM — fast.
    Great,
    /// Fits in RAM but not VRAM — runs on CPU, slower.
    Slow,
    /// Too large for available memory.
    WontFit,
}

impl Fit {
    // The frontend renders its own localized labels; kept for tests/logging.
    #[allow(dead_code)]
    pub fn label(&self) -> &'static str {
        match self {
            Fit::Great => "Runs great on your PC",
            Fit::Slow => "Runs slowly",
            Fit::WontFit => "Won't fit",
        }
    }
}

/// A rough, clearly-approximate tokens/sec band for a model on this machine
/// (MKT-2). Generation speed is dominated by memory bandwidth, which we don't
/// know precisely, so this is a coarse band by fit + size — labelled as an
/// estimate in the UI, never a promise.
pub fn estimate_speed(size_mb: u64, fit: Fit) -> String {
    let gb = size_mb as f64 / 1024.0;
    match fit {
        Fit::Great => {
            if gb < 2.0 {
                "≈ 40–60 tok/s"
            } else if gb < 5.0 {
                "≈ 20–40 tok/s"
            } else if gb < 9.0 {
                "≈ 12–22 tok/s"
            } else {
                "≈ 6–12 tok/s"
            }
        }
        Fit::Slow => "≈ 3–8 tok/s (CPU)",
        Fit::WontFit => "—",
    }
    .to_string()
}

/// Classify how a model of `size_mb` will run on `hw`. Heuristic: a GGUF needs
/// roughly its file size plus ~20% headroom for context/KV-cache. If that fits
/// in GPU VRAM it runs great; else if it fits in system RAM it runs (slowly) on
/// CPU; otherwise it won't fit.
pub fn classify_fit(size_mb: u64, hw: &HardwareProfile) -> Fit {
    let needed = (size_mb as f64 * 1.2) as u64;
    let vram = hw
        .primary_gpu()
        .and_then(|g| g.vram_mb)
        .unwrap_or(0);
    if vram >= needed {
        Fit::Great
    } else if hw.ram_mb >= needed + 2048 {
        // Leave ~2 GB for the OS + app shell.
        Fit::Slow
    } else {
        Fit::WontFit
    }
}

/// The curated "recommended" overlay for the consumer persona (D-5). Small,
/// broadly-capable instruct models with widely-mirrored GGUF builds.
pub fn recommended_catalog() -> Vec<CatalogModel> {
    fn hf_url(repo: &str, file: &str) -> String {
        format!("https://huggingface.co/{repo}/resolve/main/{file}?download=true")
    }
    vec![
        CatalogModel {
            id: "curated:llama-3.2-3b-q4".into(),
            name: "Llama 3.2 3B Instruct".into(),
            description: "Fast, capable, and small — a great first model for most PCs.".into(),
            quant: "Q4_K_M".into(),
            size_mb: 2020,
            vision: false,
            url: hf_url(
                "bartowski/Llama-3.2-3B-Instruct-GGUF",
                "Llama-3.2-3B-Instruct-Q4_K_M.gguf",
            ),
            source: "curated".into(),
            license: Some("Llama 3.2 Community".into()),
        },
        CatalogModel {
            id: "curated:qwen2.5-7b-q4".into(),
            name: "Qwen 2.5 7B Instruct".into(),
            description: "Stronger reasoning and coding; needs a bit more memory.".into(),
            quant: "Q4_K_M".into(),
            size_mb: 4680,
            vision: false,
            url: hf_url(
                "bartowski/Qwen2.5-7B-Instruct-GGUF",
                "Qwen2.5-7B-Instruct-Q4_K_M.gguf",
            ),
            source: "curated".into(),
            license: Some("Apache-2.0".into()),
        },
        CatalogModel {
            id: "curated:llama-3.1-8b-q4".into(),
            name: "Llama 3.1 8B Instruct".into(),
            description: "A well-rounded general assistant for capable machines.".into(),
            quant: "Q4_K_M".into(),
            size_mb: 4920,
            vision: false,
            url: hf_url(
                "bartowski/Meta-Llama-3.1-8B-Instruct-GGUF",
                "Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf",
            ),
            source: "curated".into(),
            license: Some("Llama 3.1 Community".into()),
        },
    ]
}
