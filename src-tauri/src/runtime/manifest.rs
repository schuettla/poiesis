//! Runtime manifest (PRD §7.3.1–7.3.2): maps a [`HardwareProfile`] to the correct
//! prebuilt `llama-server` asset from upstream `ggml-org/llama.cpp` releases.
//!
//! Poiesis does not build llama.cpp; it consumes upstream's Windows binaries. This
//! module encodes only *which* asset to pick. Resolving the concrete download URL
//! (querying the GitHub release for a matching asset name) lives in `download`.

use serde::{Deserialize, Serialize};

use super::hardware::{GpuVendor, HardwareProfile};

/// The pinned, tested upstream build (Decision D-4: pin rather than track dailies).
/// Surfaced to the user as an "update available" flow rather than auto-updated.
///
/// **A pin is a ceiling on which models can run.** The previous pin, `b4585`
/// (Jan 2025), predates llama.cpp's Gemma 3 support, so every attempt to load a
/// `gemma3` GGUF died with "unknown model architecture" before the server ever
/// bound its port. When bumping this, bump it far enough that the architectures
/// in the model catalogue actually load.
pub const PINNED_BUILD_TAG: &str = "b10333";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    Cuda12,
    Cuda13,
    Vulkan,
    Hip,
    Sycl,
    Cpu,
}

impl Backend {
    /// Every backend, in display order (for the engine view's picker).
    pub const ALL: [Backend; 6] = [
        Backend::Cuda12,
        Backend::Cuda13,
        Backend::Vulkan,
        Backend::Hip,
        Backend::Sycl,
        Backend::Cpu,
    ];

    /// Stable kebab id, matching the serde representation. Used for settings
    /// values and per-backend install directories.
    pub fn kebab(&self) -> &'static str {
        match self {
            Backend::Cuda12 => "cuda12",
            Backend::Cuda13 => "cuda13",
            Backend::Vulkan => "vulkan",
            Backend::Hip => "hip",
            Backend::Sycl => "sycl",
            Backend::Cpu => "cpu",
        }
    }

    /// Parse a kebab id back into a [`Backend`].
    pub fn from_kebab(s: &str) -> Option<Backend> {
        Backend::ALL.into_iter().find(|b| b.kebab() == s)
    }

    /// Short human label for the engine view.
    pub fn label(&self) -> &'static str {
        match self {
            Backend::Cuda12 => "NVIDIA CUDA 12",
            Backend::Cuda13 => "NVIDIA CUDA 13",
            Backend::Vulkan => "Vulkan (cross-vendor GPU)",
            Backend::Hip => "AMD HIP/ROCm",
            Backend::Sycl => "Intel SYCL",
            Backend::Cpu => "CPU",
        }
    }

    /// Whether this backend additionally needs the separate CUDA runtime DLL
    /// package shipped alongside the engine (§7.3.2 NVIDIA note).
    // Provisioning uses cudart_keywords directly; this predicate is for the UI.
    #[allow(dead_code)]
    pub fn needs_cudart(&self) -> bool {
        matches!(self, Backend::Cuda12 | Backend::Cuda13)
    }

    /// Filename prefix every llama.cpp *engine* asset carries.
    ///
    /// This is load-bearing, not decoration. Upstream renamed the CUDA builds
    /// from `…-cuda-cu12.4-…` to `…-cuda-12.4-…`, which left the engine zip
    /// (`llama-b10333-bin-win-cuda-12.4-x64.zip`) and the CUDA DLL package
    /// (`cudart-llama-bin-win-cuda-12.4-x64.zip`) differing *only* in their
    /// prefix — no set of substrings can tell them apart any more.
    pub const ENGINE_PREFIX: &'static str = "llama-";
    /// Filename prefix of the separate CUDA runtime DLL packages.
    pub const CUDART_PREFIX: &'static str = "cudart-";

    /// Lowercase substrings that must all appear in an upstream asset's file
    /// name for it to match this backend (Windows x64 only in v1). Applied on
    /// top of [`Backend::ENGINE_PREFIX`].
    ///
    /// Matched against the real upstream naming, e.g.
    /// `llama-b10333-bin-win-cuda-12.4-x64.zip`,
    /// `llama-b10333-bin-win-cpu-x64.zip`. The `x64` term also excludes the
    /// arm64 builds published under otherwise identical names.
    pub fn asset_keywords(&self) -> Vec<&'static str> {
        match self {
            // Minor version deliberately unpinned: upstream moves the CUDA
            // toolkit point release (12.4 → 12.x) without warning, and any
            // 12.x engine runs against the 12.x DLL package we fetch beside it.
            Backend::Cuda12 => vec!["win", "cuda-12", "x64"],
            Backend::Cuda13 => vec!["win", "cuda-13", "x64"],
            Backend::Vulkan => vec!["win", "vulkan", "x64"],
            Backend::Hip => vec!["win", "hip", "x64"],
            Backend::Sycl => vec!["win", "sycl", "x64"],
            // Upstream no longer ships per-ISA CPU builds. The single CPU zip
            // carries a `ggml-cpu-<microarch>.dll` per ISA (sandybridge …
            // zen4, sapphirerapids) and picks one at runtime, which is both
            // better than our detection and one less thing to get wrong.
            Backend::Cpu => vec!["win", "cpu", "x64"],
        }
    }

    /// Keywords identifying the matching CUDA runtime DLL package, if any.
    /// Applied on top of [`Backend::CUDART_PREFIX`]; real package names look
    /// like `cudart-llama-bin-win-cuda-12.4-x64.zip`.
    pub fn cudart_keywords(&self) -> Option<Vec<&'static str>> {
        match self {
            Backend::Cuda12 => Some(vec!["cuda-12", "x64"]),
            Backend::Cuda13 => Some(vec!["cuda-13", "x64"]),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSelection {
    pub backend: Backend,
    /// All viable alternates, best-first, so Advanced users can force one
    /// (§7.3.3 step 5) — e.g. Vulkan on an NVIDIA card for debugging.
    pub alternates: Vec<Backend>,
    /// Plain-language reason shown to the user.
    pub rationale: String,
    pub build_tag: String,
}

/// Heuristic: does this NVIDIA card name look like a newest-generation
/// (Blackwell, RTX 50-series) part that wants the CUDA 13 toolkit?
fn is_blackwell(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("rtx 50") || n.contains("blackwell") || n.contains("b100") || n.contains("b200")
}

/// Select the backend for this machine per the v1 Windows matrix (§7.3.2).
pub fn select_runtime(profile: &HardwareProfile) -> RuntimeSelection {
    let build_tag = PINNED_BUILD_TAG.to_string();

    if let Some(gpu) = profile.primary_gpu() {
        match gpu.vendor {
            GpuVendor::Nvidia => {
                let (backend, alt) = if is_blackwell(&gpu.name) {
                    (Backend::Cuda13, Backend::Cuda12)
                } else {
                    (Backend::Cuda12, Backend::Cuda13)
                };
                return RuntimeSelection {
                    backend,
                    alternates: vec![alt, Backend::Vulkan, Backend::Cpu],
                    rationale: format!("Using CUDA acceleration on your {}.", gpu.name),
                    build_tag,
                };
            }
            GpuVendor::Amd => {
                // D-6: Vulkan default for reliability, HIP opt-in.
                return RuntimeSelection {
                    backend: Backend::Vulkan,
                    alternates: vec![Backend::Hip, Backend::Cpu],
                    rationale: format!("Using Vulkan acceleration on your {}.", gpu.name),
                    build_tag,
                };
            }
            GpuVendor::Intel => {
                return RuntimeSelection {
                    backend: Backend::Vulkan,
                    alternates: vec![Backend::Sycl, Backend::Cpu],
                    rationale: format!("Using Vulkan acceleration on your {}.", gpu.name),
                    build_tag,
                };
            }
            GpuVendor::Unknown => {}
        }
    }

    // CPU fallback. There is one CPU build to choose; it dispatches to the
    // right ISA itself, so the detected AVX level is display-only now (§7.3.2).
    RuntimeSelection {
        backend: Backend::Cpu,
        // Vulkan is the one worth offering: it drives GPUs our vendor probe
        // failed to identify, which is exactly how a machine lands here.
        alternates: vec![Backend::Vulkan],
        rationale: "No supported GPU detected — running on the CPU. Larger models will be slow."
            .to_string(),
        build_tag,
    }
}
