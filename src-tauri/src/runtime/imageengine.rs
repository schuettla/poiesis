//! Image engine provisioning (9F): the `stable-diffusion.cpp` prebuilt binary,
//! selected for the machine's GPU backend exactly like the llama.cpp engine.
//! Upstream ships per-backend Windows zips (`…-bin-win-cuda12-x64.zip`,
//! `…-vulkan-x64.zip`, `…-cpu-x64.zip`) plus a `cudart-sd-…` DLL package — the
//! same shape the Phase-1 downloader already handles, so this reuses it wholesale.
//! Linux (`…-bin-Linux-Ubuntu-24.04-x86_64[-vulkan|-rocm-…].zip`) and Apple
//! Silicon (`…-bin-Darwin-macOS-…-arm64.zip`) are zips too, without CUDA.

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

/// Candidate names for the sd.cpp CLI inside the extracted zip, without the
/// platform's exe suffix (the binary was renamed `sd` → `sd-cli` across
/// releases; accept either).
pub const SD_BINARIES: [&str; 2] = ["sd-cli", "sd"];

/// Filename prefix of sd.cpp's own engine assets (`sd-master-…-bin-win-…`),
/// which is what keeps them apart from the `cudart-sd-…` DLL package.
pub const SD_ENGINE_PREFIX: &str = "sd-";
/// Filename prefix of the CUDA runtime DLL package shipped beside them.
pub const SD_CUDART_PREFIX: &str = "cudart-";

/// Asset keywords per backend on this OS. sd.cpp ships a single CPU build
/// (not per-ISA) and only a CUDA 12 GPU build, so several llama backends collapse.
pub fn sd_asset_keywords(backend: Backend) -> Vec<&'static str> {
    sd_asset_keywords_for(backend, std::env::consts::OS)
}

/// [`sd_asset_keywords`] for a given `std::env::consts::OS`.
pub fn sd_asset_keywords_for(backend: Backend, os: &str) -> Vec<&'static str> {
    match os {
        "linux" => match backend {
            Backend::Hip => vec!["linux", "x86_64", "rocm"],
            // The CPU build is the one with no backend word after the arch.
            Backend::Cpu | Backend::Metal => vec!["linux", "x86_64.zip"],
            // No CUDA or SYCL Linux build upstream — Vulkan drives those GPUs.
            _ => vec!["linux", "x86_64", "vulkan"],
        },
        // One Apple Silicon build (no Intel Mac build exists upstream).
        "macos" => vec!["darwin", "arm64"],
        _ => match backend {
            // Only a cuda12 GPU build exists upstream; a cuda13 machine uses it too.
            Backend::Cuda12 | Backend::Cuda13 => vec!["win", "cuda12", "x64"],
            Backend::Vulkan => vec!["win", "vulkan", "x64"],
            Backend::Hip => vec!["win", "rocm", "x64"],
            // No SYCL Windows build upstream — Vulkan covers Intel GPUs.
            Backend::Sycl => vec!["win", "vulkan", "x64"],
            Backend::Cpu | Backend::Metal => vec!["win", "cpu", "x64"],
        },
    }
}

/// Keywords for the matching CUDA runtime DLL package (`cudart-sd-…-cu12-…`),
/// applied on top of [`SD_CUDART_PREFIX`]. Windows only: upstream's only
/// CUDA build is the Windows one.
pub fn sd_cudart_keywords(backend: Backend) -> Option<Vec<&'static str>> {
    (cfg!(windows) && matches!(backend, Backend::Cuda12 | Backend::Cuda13))
        .then(|| vec!["cu12", "x64"])
}

/// Locate the sd.cpp CLI inside an extracted directory.
pub fn find_sd_binary(dir: &Path) -> Option<PathBuf> {
    let names: Vec<String> = SD_BINARIES
        .iter()
        .map(|n| format!("{n}{}", std::env::consts::EXE_SUFFIX))
        .collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    find_binary(dir, &names)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real assets from the pinned sd.cpp release, all platforms, in the API's
    /// order. The keyword matcher must resolve each backend to the right one —
    /// and the CUDA engine match must not be fooled by the `cudart-sd-…`
    /// package (guards the same asset-naming trap that bit the llama.cpp path).
    const SD_ASSETS: &[&str] = &[
        "cudart-sd-bin-win-cu12-x64.zip",
        "sd-master-484baa4-bin-Darwin-macOS-15.7.7-arm64.zip",
        "sd-master-484baa4-bin-Linux-Ubuntu-24.04-x86_64-rocm-7.13.0.zip",
        "sd-master-484baa4-bin-Linux-Ubuntu-24.04-x86_64-rocm-7.2.1.zip",
        "sd-master-484baa4-bin-Linux-Ubuntu-24.04-x86_64-vulkan.zip",
        "sd-master-484baa4-bin-Linux-Ubuntu-24.04-x86_64.zip",
        "sd-master-484baa4-bin-win-cpu-x64.zip",
        "sd-master-484baa4-bin-win-cuda12-x64.zip",
        "sd-master-484baa4-bin-win-rocm-7.1.1-x64.zip",
        "sd-master-484baa4-bin-win-rocm-7.13.0-x64.zip",
        "sd-master-484baa4-bin-win-vulkan-x64.zip",
    ];

    fn matches(name: &str, prefix: &str, kw: &[&str]) -> bool {
        let l = name.to_ascii_lowercase();
        l.starts_with(prefix) && l.ends_with(".zip") && kw.iter().all(|k| l.contains(k))
    }

    fn pick<'a>(prefix: &str, kw: &[&str]) -> Option<&'a str> {
        SD_ASSETS.iter().copied().find(|n| matches(n, prefix, kw))
    }

    fn engine<'a>(backend: Backend, os: &str) -> Option<&'a str> {
        pick(SD_ENGINE_PREFIX, &sd_asset_keywords_for(backend, os))
    }

    #[test]
    fn resolves_backends_to_real_sd_assets() {
        let w = "windows";
        assert_eq!(engine(Backend::Cuda12, w), Some("sd-master-484baa4-bin-win-cuda12-x64.zip"));
        assert_eq!(engine(Backend::Vulkan, w), Some("sd-master-484baa4-bin-win-vulkan-x64.zip"));
        assert_eq!(engine(Backend::Cpu, w), Some("sd-master-484baa4-bin-win-cpu-x64.zip"));
        assert_eq!(engine(Backend::Hip, w), Some("sd-master-484baa4-bin-win-rocm-7.1.1-x64.zip"));
    }

    #[test]
    fn resolves_linux_and_macos_sd_assets() {
        let l = "linux";
        assert_eq!(engine(Backend::Cpu, l), Some("sd-master-484baa4-bin-Linux-Ubuntu-24.04-x86_64.zip"));
        assert_eq!(
            engine(Backend::Vulkan, l),
            Some("sd-master-484baa4-bin-Linux-Ubuntu-24.04-x86_64-vulkan.zip")
        );
        // No Linux CUDA build: NVIDIA runs the Vulkan one.
        assert_eq!(engine(Backend::Cuda12, l), engine(Backend::Vulkan, l));
        assert!(engine(Backend::Hip, l).unwrap().contains("rocm"));
        assert_eq!(
            engine(Backend::Metal, "macos"),
            Some("sd-master-484baa4-bin-Darwin-macOS-15.7.7-arm64.zip")
        );
    }

    #[test]
    fn cuda_engine_does_not_match_the_cudart_package() {
        let got = engine(Backend::Cuda12, "windows").unwrap();
        assert!(!got.starts_with("cudart"), "matched the cudart package: {got}");
        assert_eq!(pick(SD_CUDART_PREFIX, &["cu12", "x64"]), Some("cudart-sd-bin-win-cu12-x64.zip"));
    }
}
