//! Download subsystem (PRD §7.3.3 steps 2–3, MKT-3): resolve the matching upstream
//! asset, stream it to disk with resume support, verify SHA-256, and unpack.
//!
//! Progress is reported through a callback so the caller can forward it to the UI
//! as a friendly state ("Getting your model ready — about 2 minutes", §5.4.1).

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The upstream repo for the llama.cpp engine assets.
pub const LLAMA_REPO: &str = "ggml-org/llama.cpp";
const USER_AGENT: &str = concat!("ProjectPoiesis/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("no asset in release {tag} matched '{prefix}…{keywords}…'")]
    NoAsset {
        tag: String,
        prefix: String,
        keywords: String,
    },
    // Constructed by verify_sha256, used from Phase 3 (MKT-3).
    #[allow(dead_code)]
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    Checksum { expected: String, actual: String },
    #[error("archive error: {0}")]
    Archive(String),
    #[error("download truncated: got {received} of {expected} bytes (connection dropped) — retry")]
    Truncated { received: u64, expected: u64 },
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub received: u64,
    pub total: Option<u64>,
    pub label: String,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    assets: Vec<GhAsset>,
}

/// A resolved asset ready to download.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

/// Does `name` start with `prefix`, end in an archive we can unpack, and
/// contain every keyword?
///
/// The prefix is what separates an engine zip from the CUDA DLL package that
/// sits beside it in the same release: since upstream dropped the `cu`
/// discriminator, `llama-b11418-bin-win-cuda-12.4-x64.zip` and
/// `cudart-llama-bin-win-cuda-12.4-x64.zip` share every substring that matters.
fn name_matches(name: &str, prefix: &str, keywords: &[&str]) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with(prefix)
        && (lower.ends_with(".zip") || lower.ends_with(".tar.gz"))
        && keywords.iter().all(|k| lower.contains(k))
}

/// Query the pinned llama.cpp release and find the asset matching all keywords.
pub async fn resolve_asset(
    client: &reqwest::Client,
    build_tag: &str,
    prefix: &str,
    keywords: &[&str],
) -> Result<ResolvedAsset, DownloadError> {
    resolve_asset_from(client, LLAMA_REPO, build_tag, prefix, keywords).await
}

/// Query a specific GitHub repo's release (by tag) and find the asset whose name
/// starts with `prefix` and matches all keywords. Used for both the llama.cpp
/// engine and the stable-diffusion.cpp image engine (9F).
pub async fn resolve_asset_from(
    client: &reqwest::Client,
    repo: &str,
    build_tag: &str,
    prefix: &str,
    keywords: &[&str],
) -> Result<ResolvedAsset, DownloadError> {
    let url = format!("https://api.github.com/repos/{repo}/releases/tags/{build_tag}");
    let release: GhRelease = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    release
        .assets
        .into_iter()
        .find(|a| name_matches(&a.name, prefix, keywords))
        .map(|a| ResolvedAsset {
            name: a.name,
            url: a.browser_download_url,
            size: a.size,
        })
        .ok_or_else(|| DownloadError::NoAsset {
            tag: build_tag.to_string(),
            prefix: prefix.to_string(),
            keywords: keywords.join("…"),
        })
}

/// Stream `url` to `dest`, resuming from any existing partial file via a Range
/// request. Calls `on_progress` periodically. Returns the number of bytes now
/// on disk.
pub async fn download_with_resume<F>(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    label: &str,
    mut on_progress: F,
) -> Result<u64, DownloadError>
where
    F: FnMut(DownloadProgress),
{
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    let existing = fs::metadata(dest).map(|m| m.len()).unwrap_or(0);

    let mut req = client.get(url).header("User-Agent", USER_AGENT);
    if existing > 0 {
        req = req.header("Range", format!("bytes={existing}-"));
    }
    let resp = req.send().await?.error_for_status()?;

    // If the server ignored the Range request, restart from scratch.
    let resuming = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let start = if resuming { existing } else { 0 };
    let content_len = resp.content_length();
    let total = content_len.map(|c| c + start);

    let mut file = if resuming {
        let mut f = fs::OpenOptions::new().write(true).open(dest)?;
        f.seek(SeekFrom::Start(existing))?;
        f
    } else {
        File::create(dest)?
    };

    let mut received = start;
    let mut stream = resp.bytes_stream();
    let mut since_emit = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)?;
        received += chunk.len() as u64;
        since_emit += chunk.len() as u64;
        // Throttle UI updates to ~once per MB.
        if since_emit >= 1024 * 1024 {
            since_emit = 0;
            on_progress(DownloadProgress {
                received,
                total,
                label: label.to_string(),
            });
        }
    }
    file.flush()?;
    // Guard against a silently-dropped connection: if the server told us how
    // many bytes to expect, a short read means the file is incomplete. Return
    // an error so the partial bytes on disk can be resumed on the next attempt
    // instead of being mistaken for a finished download.
    if let Some(total) = total {
        if received < total {
            return Err(DownloadError::Truncated { received, expected: total });
        }
    }
    on_progress(DownloadProgress {
        received,
        total,
        label: label.to_string(),
    });
    Ok(received)
}

/// Compute the SHA-256 of a file as a lowercase hex string (§7.3.3 step 3).
// Wired into model-download verification in Phase 3 (MKT-3).
#[allow(dead_code)]
pub fn sha256_file(path: &Path) -> Result<String, DownloadError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Verify a file against an expected lowercase-hex SHA-256, if one is known.
// Wired into model-download verification in Phase 3 (MKT-3).
#[allow(dead_code)]
pub fn verify_sha256(path: &Path, expected: Option<&str>) -> Result<(), DownloadError> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let actual = sha256_file(path)?;
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(DownloadError::Checksum {
            expected: expected.to_string(),
            actual,
        })
    }
}

/// Extract a downloaded `.zip` or `.tar.gz` (picked by extension) into
/// `dest_dir`. Upstream ships Windows engines as zips and Linux/macOS llama.cpp
/// engines as tarballs; sd.cpp ships zips everywhere.
pub fn unpack_archive(archive: &Path, dest_dir: &Path) -> Result<Vec<PathBuf>, DownloadError> {
    let name = archive
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name.ends_with(".tar.gz") {
        unpack_tar_gz(archive, dest_dir)
    } else {
        unpack_zip(archive, dest_dir)
    }
}

/// Extract a downloaded `.zip` into `dest_dir`, flattening nothing. Returns the
/// list of extracted file paths.
pub fn unpack_zip(archive: &Path, dest_dir: &Path) -> Result<Vec<PathBuf>, DownloadError> {
    let file = File::open(archive)?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| DownloadError::Archive(e.to_string()))?;
    fs::create_dir_all(dest_dir)?;
    let mut written = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| DownloadError::Archive(e.to_string()))?;
        let Some(rel) = entry.enclosed_name() else {
            continue; // skip unsafe / path-traversal entries
        };
        let out_path = dest_dir.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        // A symlink entry's content is its target (the `.so`/`.dylib` version
        // links). Written as a plain file it would be a few bytes of text
        // where the loader expects a library.
        #[cfg(unix)]
        if entry.is_symlink() {
            let mut target = String::new();
            entry.read_to_string(&mut target)?;
            let _ = fs::remove_file(&out_path);
            std::os::unix::fs::symlink(target, &out_path)?;
            written.push(out_path);
            continue;
        }
        let mut out = File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
        // File::create drops the zip's exec bit; without it the engine binary
        // can't be started off Windows.
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&out_path, fs::Permissions::from_mode(mode & 0o777))?;
        }
        written.push(out_path);
    }
    Ok(written)
}

/// Extract a downloaded `.tar.gz` into `dest_dir`. Returns the list of
/// extracted file paths.
pub fn unpack_tar_gz(archive: &Path, dest_dir: &Path) -> Result<Vec<PathBuf>, DownloadError> {
    let file = File::open(archive)?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    fs::create_dir_all(dest_dir)?;
    let mut written = Vec::new();
    for entry in tar.entries()? {
        let mut entry = entry?;
        let rel = entry.path()?.into_owned();
        // `unpack_in` skips entries that would land outside `dest_dir`, and
        // keeps the header's mode and symlinks, so the binary stays executable.
        if entry.unpack_in(dest_dir)? && !entry.header().entry_type().is_dir() {
            written.push(dest_dir.join(rel));
        }
    }
    Ok(written)
}

/// Find the single asset matching `prefix` + `keywords` (test helper / mirror of
/// the selection done inside [`resolve_asset`]).
#[cfg(test)]
fn select_asset_name<'a>(names: &[&'a str], prefix: &str, keywords: &[&str]) -> Option<&'a str> {
    names
        .iter()
        .copied()
        .find(|n| name_matches(n, prefix, keywords))
}

/// Locate `llama-server` (`.exe` on Windows) within an extracted runtime directory.
pub fn find_server_binary(dir: &Path) -> Option<PathBuf> {
    find_binary(dir, &[&format!("llama-server{}", std::env::consts::EXE_SUFFIX)])
}

/// Recursively find the first file matching any of `names` (case-insensitive).
pub fn find_binary(dir: &Path, names: &[&str]) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_binary(&path, names) {
                return Some(found);
            }
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| names.iter().any(|want| n.eq_ignore_ascii_case(want)))
            .unwrap_or(false)
        {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::manifest::Backend;

    /// The real assets published on the pinned release (`ggml-org/llama.cpp`
    /// `b11418`), all platforms, in the API's (alphabetical) order. The release
    /// endpoint returns every one of them on every OS, so each OS's keywords
    /// are checked against the whole list — this guards against the
    /// asset-naming drift that broke the CUDA and CPU paths once already, and
    /// against one OS's keywords matching another OS's archive. Keep this list
    /// in sync with `PINNED_BUILD_TAG` whenever the pin moves.
    const PINNED_ASSETS: &[&str] = &[
        "cudart-llama-b11418-bin-ubuntu-cuda-12.8-x64.tar.gz",
        "cudart-llama-b11418-bin-ubuntu-cuda-13.4-arm64.tar.gz",
        "cudart-llama-b11418-bin-ubuntu-cuda-13.4-x64.tar.gz",
        "cudart-llama-bin-win-cuda-12.4-x64.zip",
        "cudart-llama-bin-win-cuda-13.4-arm64.zip",
        "cudart-llama-bin-win-cuda-13.4-x64.zip",
        "llama-b11418-bin-android-arm64-snapdragon.tar.gz",
        "llama-b11418-bin-android-arm64.tar.gz",
        "llama-b11418-bin-linux-arm64-snapdragon.tar.gz",
        "llama-b11418-bin-macos-arm64.tar.gz",
        "llama-b11418-bin-macos-x64.tar.gz",
        "llama-b11418-bin-ubuntu-arm64.tar.gz",
        "llama-b11418-bin-ubuntu-cuda-12.8-x64.tar.gz",
        "llama-b11418-bin-ubuntu-cuda-13.4-arm64.tar.gz",
        "llama-b11418-bin-ubuntu-cuda-13.4-x64.tar.gz",
        "llama-b11418-bin-ubuntu-openvino-2026.4.1-x64.tar.gz",
        "llama-b11418-bin-ubuntu-rocm-10.0-x64.tar.gz",
        "llama-b11418-bin-ubuntu-s390x.tar.gz",
        "llama-b11418-bin-ubuntu-sycl-fp16-x64.tar.gz",
        "llama-b11418-bin-ubuntu-sycl-fp32-x64.tar.gz",
        "llama-b11418-bin-ubuntu-vulkan-arm64.tar.gz",
        "llama-b11418-bin-ubuntu-vulkan-x64.tar.gz",
        "llama-b11418-bin-ubuntu-x64.tar.gz",
        "llama-b11418-bin-win-cpu-arm64.zip",
        "llama-b11418-bin-win-cpu-x64.zip",
        "llama-b11418-bin-win-cuda-12.4-x64.zip",
        "llama-b11418-bin-win-cuda-13.4-arm64.zip",
        "llama-b11418-bin-win-cuda-13.4-x64.zip",
        "llama-b11418-bin-win-opencl-adreno-arm64.zip",
        "llama-b11418-bin-win-openvino-2026.4.1-x64.zip",
        "llama-b11418-bin-win-rocm-10.0-x64.zip",
        "llama-b11418-bin-win-sycl-x64.zip",
        "llama-b11418-bin-win-vulkan-arm64.zip",
        "llama-b11418-bin-win-vulkan-x64.zip",
        "llama-b11418-ui.tar.gz",
        "llama-b11418-xcframework.zip",
    ];

    fn engine_asset(backend: Backend, os: &str) -> Option<&'static str> {
        select_asset_name(
            PINNED_ASSETS,
            Backend::ENGINE_PREFIX,
            &backend.asset_keywords_for(os),
        )
    }

    #[test]
    fn engine_keywords_resolve_real_pinned_assets() {
        let cases = [
            (Backend::Cuda12, "llama-b11418-bin-win-cuda-12.4-x64.zip"),
            (Backend::Cuda13, "llama-b11418-bin-win-cuda-13.4-x64.zip"),
            (Backend::Vulkan, "llama-b11418-bin-win-vulkan-x64.zip"),
            (Backend::Sycl, "llama-b11418-bin-win-sycl-x64.zip"),
            (Backend::Hip, "llama-b11418-bin-win-rocm-10.0-x64.zip"),
            (Backend::Cpu, "llama-b11418-bin-win-cpu-x64.zip"),
        ];
        for (backend, expected) in cases {
            assert_eq!(
                engine_asset(backend, "windows"),
                Some(expected),
                "wrong engine asset for {backend:?}"
            );
        }
    }

    #[test]
    fn linux_engine_keywords_resolve_real_pinned_assets() {
        let cases = [
            (Backend::Cuda12, "llama-b11418-bin-ubuntu-cuda-12.8-x64.tar.gz"),
            (Backend::Cuda13, "llama-b11418-bin-ubuntu-cuda-13.4-x64.tar.gz"),
            (Backend::Vulkan, "llama-b11418-bin-ubuntu-vulkan-x64.tar.gz"),
            (Backend::Sycl, "llama-b11418-bin-ubuntu-sycl-fp16-x64.tar.gz"),
            (Backend::Hip, "llama-b11418-bin-ubuntu-rocm-10.0-x64.tar.gz"),
            (Backend::Cpu, "llama-b11418-bin-ubuntu-x64.tar.gz"),
        ];
        for (backend, expected) in cases {
            assert_eq!(
                engine_asset(backend, "linux"),
                Some(expected),
                "wrong Linux engine asset for {backend:?}"
            );
        }
    }

    #[test]
    fn macos_resolves_the_one_apple_silicon_build() {
        assert_eq!(
            engine_asset(Backend::Metal, "macos"),
            Some("llama-b11418-bin-macos-arm64.tar.gz")
        );
        assert_eq!(Backend::Metal.cudart_keywords_for("macos"), None);
    }

    /// The regression that motivated the prefix: upstream renamed the CUDA
    /// builds from `cuda-cu12.4` to `cuda-12.4`, at which point the engine zip
    /// and the CUDA DLL package became substring-identical.
    #[test]
    fn cuda_engine_does_not_match_the_cudart_package() {
        for os in ["windows", "linux"] {
            for backend in [Backend::Cuda12, Backend::Cuda13] {
                let got = engine_asset(backend, os).unwrap();
                assert!(
                    !got.starts_with("cudart"),
                    "matched the cudart package for {backend:?} on {os}: {got}"
                );
            }
        }
    }

    /// The Ubuntu CUDA 12 package sorts first and shares every word but the
    /// OS with the Windows one, so the OS word must be part of the keywords.
    #[test]
    fn cudart_keywords_resolve_the_matching_package_per_os() {
        let cases = [
            ("windows", Backend::Cuda12, "cudart-llama-bin-win-cuda-12.4-x64.zip"),
            ("windows", Backend::Cuda13, "cudart-llama-bin-win-cuda-13.4-x64.zip"),
            ("linux", Backend::Cuda12, "cudart-llama-b11418-bin-ubuntu-cuda-12.8-x64.tar.gz"),
            ("linux", Backend::Cuda13, "cudart-llama-b11418-bin-ubuntu-cuda-13.4-x64.tar.gz"),
        ];
        for (os, backend, expected) in cases {
            let kw = backend.cudart_keywords_for(os).unwrap();
            let got = select_asset_name(PINNED_ASSETS, Backend::CUDART_PREFIX, &kw);
            assert_eq!(got, Some(expected), "wrong cudart package for {backend:?} on {os}");
        }
    }

    /// Every backend ships an arm64 sibling under a near-identical name; `x64`
    /// is the only thing keeping us off a binary that cannot run here.
    #[test]
    fn never_resolves_an_arm64_asset_off_macos() {
        for os in ["windows", "linux"] {
            for backend in Backend::ALL {
                if let Some(got) = engine_asset(backend, os) {
                    assert!(!got.contains("arm64"), "picked an arm64 build on {os}: {got}");
                }
            }
        }
    }

    /// Non-CUDA backends must not drag in a CUDA DLL download.
    #[test]
    fn only_cuda_backends_want_the_cudart_package() {
        for os in ["windows", "linux"] {
            for backend in Backend::ALL {
                let wants = backend.cudart_keywords_for(os).is_some();
                assert_eq!(
                    wants,
                    matches!(backend, Backend::Cuda12 | Backend::Cuda13),
                    "unexpected cudart requirement for {backend:?} on {os}"
                );
            }
        }
    }

    #[test]
    fn unpack_archive_handles_tar_gz() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("llama-test-bin-ubuntu-x64.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(
                File::create(&archive).unwrap(),
                flate2::Compression::fast(),
            );
            let mut tar = tar::Builder::new(gz);
            let body = b"#!/bin/sh\n";
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append_data(&mut header, "build/bin/llama-server", &body[..])
                .unwrap();
            tar.into_inner().unwrap().finish().unwrap();
        }
        let out = dir.path().join("out");
        let written = unpack_archive(&archive, &out).unwrap();
        assert_eq!(written, vec![out.join("build/bin/llama-server")]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&written[0]).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "exec bit lost: {mode:o}");
        }
    }
}
