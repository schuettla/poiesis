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
pub const PINNED_BUILD_TAG: &str = "b4585";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    Cuda12,
    Cuda13,
    Vulkan,
    Hip,
    Sycl,
    CpuAvx512,
    CpuAvx2,
    Cpu,
}

impl Backend {
    /// Every backend, in display order (for the engine view's picker).
    pub const ALL: [Backend; 8] = [
        Backend::Cuda12,
        Backend::Cuda13,
        Backend::Vulkan,
        Backend::Hip,
        Backend::Sycl,
        Backend::CpuAvx512,
        Backend::CpuAvx2,
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
            Backend::CpuAvx512 => "cpu-avx512",
            Backend::CpuAvx2 => "cpu-avx2",
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
            Backend::CpuAvx512 => "CPU (AVX-512)",
            Backend::CpuAvx2 => "CPU (AVX2)",
            Backend::Cpu => "CPU (baseline)",
        }
    }

    /// Whether this backend additionally needs the separate CUDA runtime DLL
    /// package shipped alongside the engine (§7.3.2 NVIDIA note).
    // Provisioning uses cudart_keywords directly; this predicate is for the UI.
    #[allow(dead_code)]
    pub fn needs_cudart(&self) -> bool {
        matches!(self, Backend::Cuda12 | Backend::Cuda13)
    }

    /// Lowercase substrings that must all appear in an upstream asset's file
    /// name for it to match this backend (Windows x64 only in v1).
    ///
    /// Matched against the real upstream naming, e.g.
    /// `llama-b4585-bin-win-cuda-cu12.4-x64.zip`,
    /// `llama-b4585-bin-win-avx2-x64.zip`. Note the `cuda-cu` discriminator on
    /// the GPU builds: it keeps the engine match from also matching the separate
    /// `cudart-llama-bin-win-cu12.4-x64.zip` DLL package (which contains the
    /// substring "cuda" inside "cudart").
    pub fn asset_keywords(&self) -> Vec<&'static str> {
        match self {
            Backend::Cuda12 => vec!["win", "cuda-cu12", "x64"],
            Backend::Cuda13 => vec!["win", "cuda-cu13", "x64"],
            Backend::Vulkan => vec!["win", "vulkan", "x64"],
            Backend::Hip => vec!["win", "hip", "x64"],
            Backend::Sycl => vec!["win", "sycl", "x64"],
            // Upstream ships per-ISA CPU Windows builds (avx512 / avx2 / avx /
            // noavx); pick the one matching detected CPU features (§7.3.2).
            Backend::CpuAvx512 => vec!["win", "avx512", "x64"],
            Backend::CpuAvx2 => vec!["win", "avx2", "x64"],
            Backend::Cpu => vec!["win", "noavx", "x64"],
        }
    }

    /// Keywords identifying the matching CUDA runtime DLL package, if any.
    /// Real package names look like `cudart-llama-bin-win-cu12.4-x64.zip`.
    pub fn cudart_keywords(&self) -> Option<Vec<&'static str>> {
        match self {
            Backend::Cuda12 => Some(vec!["cudart", "cu12"]),
            Backend::Cuda13 => Some(vec!["cudart", "cu13"]),
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

    // CPU fallback; choose the ISA variant from detection (§7.3.2).
    let backend = if profile.cpu.avx512 {
        Backend::CpuAvx512
    } else if profile.cpu.avx2 {
        Backend::CpuAvx2
    } else {
        Backend::Cpu
    };
    RuntimeSelection {
        backend,
        alternates: vec![Backend::Cpu],
        rationale: "No supported GPU detected — running on the CPU. Larger models will be slow."
            .to_string(),
        build_tag,
    }
}
