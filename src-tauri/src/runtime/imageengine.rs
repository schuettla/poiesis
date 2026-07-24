//! Image engine provisioning (9F): the `stable-diffusion.cpp` prebuilt binary,
//! selected for the machine's GPU backend exactly like the llama.cpp engine.
//! Upstream ships per-backend Windows zips (`…-bin-win-cuda12-x64.zip`,
//! `…-vulkan-x64.zip`, `…-cpu-x64.zip`) plus a `cudart-sd-…` DLL package — the
//! same shape the Phase-1 downloader already handles, so this reuses it wholesale.

use std::path::{Path, PathBuf};

use super::download::find_binary;
use super::manifest::Backend;

/// Upstream repo for the image engine.
pub const SD_REPO: &str = "leejet/stable-diffusion.cpp";
/// Pinned, tested build (D-4: pin rather than track master).
pub const SD_PINNED_TAG: &str = "master-741-484baa4";

/// Curated default consumer model: Stable Diffusion 1.5 (the model the sd.cpp
/// docs recommend to start), from the maintained mirror of the original repo.
/// ~4 GB; runs on modest GPUs. Users can swap it in Settings.
pub const DEFAULT_MODEL_URL: &str =
    "https://huggingface.co/stable-diffusion-v1-5/stable-diffusion-v1-5/resolve/main/v1-5-pruned-emaonly.safetensors";
pub const DEFAULT_MODEL_NAME: &str = "v1-5-pruned-emaonly.safetensors";

/// Candidate names for the sd.cpp CLI inside the extracted zip (the binary was
/// renamed `sd` → `sd-cli` across releases; accept either).
pub const SD_BINARIES: [&str; 2] = ["sd-cli.exe", "sd.exe"];

/// Windows asset keywords per backend. sd.cpp ships a single CPU build (not
/// per-ISA) and only a CUDA 12 GPU build, so several llama backends collapse.
pub fn sd_asset_keywords(backend: Backend) -> Vec<&'static str> {
    match backend {
        // Only a cuda12 GPU build exists upstream; a cuda13 machine uses it too.
        Backend::Cuda12 | Backend::Cuda13 => vec!["win", "cuda12", "x64"],
        Backend::Vulkan => vec!["win", "vulkan", "x64"],
        Backend::Hip => vec!["win", "rocm", "x64"],
        // No SYCL Windows build upstream — Vulkan covers Intel GPUs.
        Backend::Sycl => vec!["win", "vulkan", "x64"],
        Backend::CpuAvx512 | Backend::CpuAvx2 | Backend::Cpu => vec!["win", "cpu", "x64"],
    }
}

/// Keywords for the matching CUDA runtime DLL package (`cudart-sd-…-cu12-…`).
/// Uses `cu12` (not `cuda12`) so it never collides with the engine zip.
pub fn sd_cudart_keywords(backend: Backend) -> Option<Vec<&'static str>> {
    matches!(backend, Backend::Cuda12 | Backend::Cuda13).then(|| vec!["cudart", "cu12"])
}

/// Locate the sd.cpp CLI inside an extracted directory.
pub fn find_sd_binary(dir: &Path) -> Option<PathBuf> {
    find_binary(dir, &SD_BINARIES)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real Windows assets from the pinned sd.cpp release. The keyword matcher
    /// must resolve each backend to the right one — and the CUDA engine match
    /// must not be fooled by the `cudart-sd-…` package (guards the same
    /// asset-naming trap that bit the llama.cpp path).
    const SD_WIN_ASSETS: &[&str] = &[
        "cudart-sd-bin-win-cu12-x64.zip",
        "sd-master-484baa4-bin-win-cpu-x64.zip",
        "sd-master-484baa4-bin-win-cuda12-x64.zip",
        "sd-master-484baa4-bin-win-rocm-7.1.1-x64.zip",
        "sd-master-484baa4-bin-win-vulkan-x64.zip",
    ];

    fn matches(name: &str, kw: &[&str]) -> bool {
        let l = name.to_ascii_lowercase();
        kw.iter().all(|k| l.contains(k)) && l.ends_with(".zip")
    }

    fn pick<'a>(kw: &[&str]) -> Option<&'a str> {
        SD_WIN_ASSETS.iter().copied().find(|n| matches(n, kw))
    }

    #[test]
    fn resolves_backends_to_real_sd_assets() {
        assert_eq!(pick(&sd_asset_keywords(Backend::Cuda12)), Some("sd-master-484baa4-bin-win-cuda12-x64.zip"));
        assert_eq!(pick(&sd_asset_keywords(Backend::Vulkan)), Some("sd-master-484baa4-bin-win-vulkan-x64.zip"));
        assert_eq!(pick(&sd_asset_keywords(Backend::Cpu)), Some("sd-master-484baa4-bin-win-cpu-x64.zip"));
        assert_eq!(pick(&sd_asset_keywords(Backend::Hip)), Some("sd-master-484baa4-bin-win-rocm-7.1.1-x64.zip"));
    }

    #[test]
    fn cuda_engine_does_not_match_the_cudart_package() {
        let got = pick(&sd_asset_keywords(Backend::Cuda12)).unwrap();
        assert!(!got.starts_with("cudart"), "matched the cudart package: {got}");
        assert_eq!(pick(&sd_cudart_keywords(Backend::Cuda12).unwrap()), Some("cudart-sd-bin-win-cu12-x64.zip"));
    }
}
