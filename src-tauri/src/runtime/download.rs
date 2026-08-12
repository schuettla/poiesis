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
    #[error("no asset in release {tag} matched '{prefix}…{keywords}…zip'")]
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

/// Does `name` start with `prefix` and contain every keyword?
///
/// The prefix is what separates an engine zip from the CUDA DLL package that
/// sits beside it in the same release: since upstream dropped the `cu`
/// discriminator, `llama-b10333-bin-win-cuda-12.4-x64.zip` and
/// `cudart-llama-bin-win-cuda-12.4-x64.zip` share every substring that matters.
fn name_matches(name: &str, prefix: &str, keywords: &[&str]) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with(prefix)
        && lower.ends_with(".zip")
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
        let mut out = File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
        written.push(out_path);
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

/// Locate `llama-server.exe` within an extracted runtime directory.
pub fn find_server_binary(dir: &Path) -> Option<PathBuf> {
    find_binary(dir, &["llama-server.exe"])
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

    /// The real Windows `.zip` assets published on the pinned release
    /// (`ggml-org/llama.cpp`). Our matcher must resolve each backend to exactly
    /// the right one of these — this guards against the asset-naming drift that
    /// broke the CUDA and CPU paths once already. Keep this list in sync with
    /// `PINNED_BUILD_TAG` whenever the pin moves.
    const PINNED_WIN_ASSETS: &[&str] = &[
        "cudart-llama-bin-win-cuda-12.4-x64.zip",
        "cudart-llama-bin-win-cuda-13.3-x64.zip",
        "llama-b10333-bin-win-cpu-arm64.zip",
        "llama-b10333-bin-win-cpu-x64.zip",
        "llama-b10333-bin-win-cuda-12.4-x64.zip",
        "llama-b10333-bin-win-cuda-13.3-x64.zip",
        "llama-b10333-bin-win-hip-radeon-x64.zip",
        "llama-b10333-bin-win-opencl-adreno-arm64.zip",
        "llama-b10333-bin-win-openvino-2026.2.1-x64.zip",
        "llama-b10333-bin-win-sycl-x64.zip",
        "llama-b10333-bin-win-vulkan-x64.zip",
    ];

    fn engine_asset(backend: Backend) -> Option<&'static str> {
        select_asset_name(
            PINNED_WIN_ASSETS,
            Backend::ENGINE_PREFIX,
            &backend.asset_keywords(),
        )
    }

    #[test]
    fn engine_keywords_resolve_real_pinned_assets() {
        let cases = [
            (Backend::Cuda12, "llama-b10333-bin-win-cuda-12.4-x64.zip"),
            (Backend::Cuda13, "llama-b10333-bin-win-cuda-13.3-x64.zip"),
            (Backend::Vulkan, "llama-b10333-bin-win-vulkan-x64.zip"),
            (Backend::Sycl, "llama-b10333-bin-win-sycl-x64.zip"),
            (Backend::Hip, "llama-b10333-bin-win-hip-radeon-x64.zip"),
            (Backend::Cpu, "llama-b10333-bin-win-cpu-x64.zip"),
        ];
        for (backend, expected) in cases {
            assert_eq!(
                engine_asset(backend),
                Some(expected),
                "wrong engine asset for {backend:?}"
            );
        }
    }

    /// The regression that motivated the prefix: upstream renamed the CUDA
    /// builds from `cuda-cu12.4` to `cuda-12.4`, at which point the engine zip
    /// and the CUDA DLL package became substring-identical.
    #[test]
    fn cuda_engine_does_not_match_the_cudart_package() {
        for backend in [Backend::Cuda12, Backend::Cuda13] {
            let got = engine_asset(backend).unwrap();
            assert!(
                !got.starts_with("cudart"),
                "matched the cudart package for {backend:?}: {got}"
            );
        }
    }

    #[test]
    fn cudart_keywords_resolve_the_matching_dll_package() {
        let cases = [
            (Backend::Cuda12, "cudart-llama-bin-win-cuda-12.4-x64.zip"),
            (Backend::Cuda13, "cudart-llama-bin-win-cuda-13.3-x64.zip"),
        ];
        for (backend, expected) in cases {
            let kw = backend.cudart_keywords().unwrap();
            let got = select_asset_name(PINNED_WIN_ASSETS, Backend::CUDART_PREFIX, &kw);
            assert_eq!(got, Some(expected), "wrong cudart package for {backend:?}");
        }
    }

    /// Every backend ships an arm64 sibling under a near-identical name; `x64`
    /// is the only thing keeping us off a binary that cannot run here.
    #[test]
    fn never_resolves_an_arm64_asset() {
        for backend in Backend::ALL {
            if let Some(got) = engine_asset(backend) {
                assert!(!got.contains("arm64"), "picked an arm64 build: {got}");
            }
        }
    }

    /// Non-CUDA backends must not drag in a CUDA DLL download.
    #[test]
    fn only_cuda_backends_want_the_cudart_package() {
        for backend in Backend::ALL {
            let wants = backend.cudart_keywords().is_some();
            assert_eq!(
                wants,
                matches!(backend, Backend::Cuda12 | Backend::Cuda13),
                "unexpected cudart requirement for {backend:?}"
            );
        }
    }
}
