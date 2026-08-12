//! Unified catalog model + the language-model speed estimate. The fit
//! classifier itself now lives in `runtime::hardware`, since the image catalog
//! asks the same question of the same hardware.

use serde::{Deserialize, Serialize};

use crate::runtime::hardware::Fit;

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
        CatalogModel {
            id: "curated:gemma-3-4b-q4".into(),
            name: "Gemma 3 4B Instruct".into(),
            description: "Google's latest small model — quick and efficient for everyday chat.".into(),
            quant: "Q4_K_M".into(),
            size_mb: 2549,
            vision: false,
            url: hf_url(
                "bartowski/google_gemma-3-4b-it-GGUF",
                "google_gemma-3-4b-it-Q4_K_M.gguf",
            ),
            source: "curated".into(),
            license: Some("Gemma".into()),
        },
        CatalogModel {
            id: "curated:qwen3-8b-q4".into(),
            name: "Qwen3 8B".into(),
            description: "Alibaba's newest generation; sharper reasoning than Qwen 2.5 at a similar size.".into(),
            quant: "Q4_K_M".into(),
            size_mb: 5151,
            vision: false,
            url: hf_url(
                "bartowski/Qwen_Qwen3-8B-GGUF",
                "Qwen_Qwen3-8B-Q4_K_M.gguf",
            ),
            source: "curated".into(),
            license: Some("Apache-2.0".into()),
        },
        CatalogModel {
            id: "curated:gemma-3-12b-q4".into(),
            name: "Gemma 3 12B Instruct".into(),
            description: "A step up in quality from the 4B — still comfortable on mid-range GPUs.".into(),
            quant: "Q4_K_M".into(),
            size_mb: 7475,
            vision: false,
            url: hf_url(
                "bartowski/google_gemma-3-12b-it-GGUF",
                "google_gemma-3-12b-it-Q4_K_M.gguf",
            ),
            source: "curated".into(),
            license: Some("Gemma".into()),
        },
        CatalogModel {
            id: "curated:qwen3-14b-q4".into(),
            name: "Qwen3 14B".into(),
            description: "Strong reasoning and long-context handling for capable machines.".into(),
            quant: "Q4_K_M".into(),
            size_mb: 9216,
            vision: false,
            url: hf_url(
                "bartowski/Qwen_Qwen3-14B-GGUF",
                "Qwen_Qwen3-14B-Q4_K_M.gguf",
            ),
            source: "curated".into(),
            license: Some("Apache-2.0".into()),
        },
        CatalogModel {
            id: "curated:gpt-oss-20b-q4".into(),
            name: "GPT-OSS 20B".into(),
            description: "OpenAI's first open-weight model — a mixture-of-experts tuned for reasoning and agentic tasks.".into(),
            quant: "Q4_K_M".into(),
            size_mb: 11776,
            vision: false,
            url: hf_url(
                "unsloth/gpt-oss-20b-GGUF",
                "gpt-oss-20b-Q4_K_M.gguf",
            ),
            source: "curated".into(),
            license: Some("Apache-2.0".into()),
        },
        CatalogModel {
            id: "curated:muse-glimmer-30b-q4".into(),
            name: "Muse Glimmer 30B".into(),
            description: "Meta Superintelligence Lab's newest agentic model — reasoning, tool use, and multimodal understanding in one 30B checkpoint. Released days ago; still largely unproven.".into(),
            quant: "UD-Q4_K_XL".into(),
            size_mb: 16282,
            vision: false,
            url: hf_url(
                "unsloth/Muse-Glimmer-30B-GGUF",
                "Muse-Glimmer-30B-UD-Q4_K_XL.gguf",
            ),
            source: "curated".into(),
            license: Some("Apache-2.0".into()),
        },
        CatalogModel {
            id: "curated:gemma-3-27b-q4".into(),
            name: "Gemma 3 27B Instruct".into(),
            description: "Google's largest widely-run Gemma — near-flagship quality; needs a high-VRAM GPU.".into(),
            quant: "Q4_K_M".into(),
            size_mb: 16947,
            vision: false,
            url: hf_url(
                "bartowski/google_gemma-3-27b-it-GGUF",
                "google_gemma-3-27b-it-Q4_K_M.gguf",
            ),
            source: "curated".into(),
            license: Some("Gemma".into()),
        },
        CatalogModel {
            id: "curated:gemma-4-26b-a4b-q4".into(),
            name: "Gemma 4 26B-A4B Instruct".into(),
            description: "Google's newest generation, mixture-of-experts — flagship quality with lighter compute per token.".into(),
            quant: "Q4_K_M".into(),
            size_mb: 17408,
            vision: false,
            url: hf_url(
                "bartowski/google_gemma-4-26B-A4B-it-GGUF",
                "google_gemma-4-26B-A4B-it-Q4_K_M.gguf",
            ),
            source: "curated".into(),
            license: Some("Gemma".into()),
        },
        CatalogModel {
            id: "curated:qwen3.6-27b-q4".into(),
            name: "Qwen3.6 27B".into(),
            description: "Alibaba's latest release; their strongest mid-size model to date.".into(),
            quant: "Q4_K_M".into(),
            size_mb: 18432,
            vision: false,
            url: hf_url(
                "bartowski/Qwen_Qwen3.6-27B-GGUF",
                "Qwen_Qwen3.6-27B-Q4_K_M.gguf",
            ),
            source: "curated".into(),
            license: Some("Apache-2.0".into()),
        },
        CatalogModel {
            id: "curated:qwen3-32b-q4".into(),
            name: "Qwen3 32B".into(),
            description: "The largest Qwen3 in this catalog — top-tier reasoning for high-VRAM machines.".into(),
            quant: "Q4_K_M".into(),
            size_mb: 20234,
            vision: false,
            url: hf_url(
                "bartowski/Qwen_Qwen3-32B-GGUF",
                "Qwen_Qwen3-32B-Q4_K_M.gguf",
            ),
            source: "curated".into(),
            license: Some("Apache-2.0".into()),
        },
    ]
}
