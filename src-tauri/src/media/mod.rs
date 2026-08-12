//! Phase 13 — the media backend seam. One provider-agnostic request/response
//! pair, a trait every backend implements, and a registry that makes adding a
//! new provider a matter of one new file and one line (`BKD-2`).
//!
//! Three backends live under `backends/` today — local (`stable-diffusion.cpp`),
//! OpenRouter (image + video) and OpenAI. Adding a fourth is one more file plus
//! one line in `Registry::new()`, per the checklist in `backends/mod.rs`.
//!
//! The shared helpers near the bottom of this file — `normalize_aspect_ratio`,
//! `nearest_supported`, `poll_until_done`, `materialize`, `probe_dimensions`,
//! `data_uri_for` — exist so that stays true: a backend that had to hand-roll
//! polling and dimension probing would not be a small file.

pub mod backends;
pub mod imagecatalog;
pub mod jobs;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine as _;

use crate::db::{Artifact, Db, DbError};
use crate::runtime::proxy::CancelFlag;
use crate::secrets::{self, SERVICE_CLOUD, SERVICE_MEDIA};

/// What a generation request produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modality {
    Image,
    Video,
}

impl Modality {
    pub fn as_kind(&self) -> &'static str {
        match self {
            Modality::Image => "image",
            Modality::Video => "video",
        }
    }
}

/// Whole-image guidance a backend that supports it can take into account.
#[derive(Debug, Clone)]
pub struct MediaRef {
    pub path: PathBuf,
    pub role: RefRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefRole {
    Source,
    Style,
}

/// A provider-agnostic generation request. Hosted backends work from
/// `aspect_ratio`/`resolution` (normalised tiers); the local backend wants
/// literal pixels, so `width`/`height` pass through for it directly and are
/// ignored by backends that only understand ratios.
#[derive(Debug, Clone, Default)]
pub struct MediaRequest {
    /// The backend-specific slug from a `media:<backend_id>/<slug>` id — for
    /// the local backend, the model file path; for a hosted backend, its
    /// model slug. `None` lets the backend pick its own default (the inferred
    /// route, `PIK-3`, never names one).
    pub model: Option<String>,
    /// Which endpoint a multi-modality backend (OpenRouter serves both) should
    /// hit. `None` defaults to `Modality::Image` — every caller that can
    /// possibly mean video sets this explicitly rather than relying on that.
    pub modality: Option<Modality>,
    pub prompt: String,
    pub negative: Option<String>,
    /// Normalised: "1:1" | "16:9" | "9:16" | "4:3" | "3:4" | "21:9".
    pub aspect_ratio: Option<String>,
    /// Normalised tier: "512" | "1K" | "2K" | "4K" (image) / "480p".."4K" (video).
    pub resolution: Option<String>,
    /// Local-only knobs; ignored (and reported) by hosted backends.
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub steps: Option<i64>,
    pub seed: Option<i64>,
    /// Whole-image guidance. The local backend rejects a non-empty vec today.
    pub references: Vec<MediaRef>,
    /// Video only.
    pub duration_secs: Option<u32>,
    /// The background job this belongs to (`JOB-1`), when there is one, so a
    /// backend can report progress against it (`STR-4`). `None` means nobody
    /// is listening and a backend should not bother.
    pub job_id: Option<String>,
}

impl MediaRequest {
    /// The prompt as the provider should see it. A `Style` reference is a look
    /// to borrow, not a picture to edit — and no hosted image API has a field
    /// for that distinction, so saying it in words is what actually carries it
    /// across every provider (`EDT-1`).
    pub fn effective_prompt(&self) -> String {
        if !self.references.is_empty() && self.references.iter().all(|r| r.role == RefRole::Style) {
            format!(
                "{}\n\nMatch the visual style of the reference image(s); do not reproduce their subject.",
                self.prompt
            )
        } else {
            self.prompt.clone()
        }
    }
}

/// What a generation call produced.
pub struct MediaResult {
    /// Always a real file under `generated_media_dir()`. Hosted providers must
    /// materialise their (expiring) provider URL here before returning.
    pub path: PathBuf,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_secs: Option<f32>,
    /// "local:<file stem>" | "openrouter:<slug>" | …
    pub model_id: String,
    /// "SDXL-Turbo" | "Nano Banana Pro" | …
    pub provider_label: String,
    pub seed: Option<i64>,
    pub cost_usd: Option<f64>,
    /// Hints the backend could not honour, echoed for honest UI (OpenClaw's
    /// `ignoredOverrides` — normalise, report, never fail).
    pub ignored: Vec<String>,
}

/// The one shape the picker/UI will eventually see for a selectable model
/// (`PIK-1`, out of scope this pass — kept here so `list_models` has a real
/// return type from the start).
#[derive(Debug, Clone, serde::Serialize)]
pub struct MediaModel {
    pub id: String,
    pub name: String,
    pub backend_id: String,
    pub backend_label: String,
    pub modality: Modality,
    pub price_label: Option<String>,
    pub supports_edit: bool,
    /// `STR-4`: this model emits partial images while it works, so the
    /// placeholder can fill in rather than just shimmer. Off unless the
    /// provider's own catalog said so — guessing would break generation.
    pub supports_streaming: bool,
    pub supported_aspect_ratios: Vec<String>,
    pub supported_resolutions: Vec<String>,
    pub max_duration_secs: Option<u32>,
}

/// Static, per-backend metadata the rest of the app can reason about without
/// knowing which backend it's looking at.
pub struct BackendDescriptor {
    pub id: &'static str,
    pub label: &'static str,
    pub modalities: &'static [Modality],
    pub credential: Credential,
    pub supports_references: bool,
    pub supports_edit: bool,
    /// Submit + poll, vs one blocking call.
    pub is_async: bool,
    pub console_url: Option<&'static str>,
}

pub enum Credential {
    /// Reuses an existing chat-provider key (OpenRouter, OpenAI).
    #[allow(dead_code)]
    Cloud(crate::cloud::Provider),
    /// Media-only provider with its own key: `SERVICE_MEDIA` / backend id.
    #[allow(dead_code)]
    Media { key_hint: &'static str },
    /// No key — a local binary or a self-hosted endpoint.
    Local,
}

#[async_trait]
pub trait MediaBackend: Send + Sync {
    fn descriptor(&self) -> &'static BackendDescriptor;
    /// Models this backend can offer right now.
    async fn list_models(&self, db: &Db) -> Result<Vec<MediaModel>, String>;
    /// `cancel` is checked at whatever boundaries this backend can actually be
    /// interrupted at — before starting, and between polls for an async
    /// provider. A backend that has no such boundary simply ignores it.
    async fn generate(
        &self,
        db: &Db,
        req: &MediaRequest,
        out_dir: &Path,
        cancel: &CancelFlag,
    ) -> Result<MediaResult, String>;

    /// Whether this backend can actually run right now, beyond holding a
    /// credential. A cloud backend with a key is ready; the local one needs an
    /// installed binary *and* a model file on disk, and saying otherwise is
    /// what used to make it shadow every cloud backend in `resolve_backend`.
    fn is_ready(&self, _db: &Db) -> bool {
        true
    }
}

/// Owns construction and credential checks. Adding a provider is: write a
/// `backends/<name>.rs`, add one line in `new()` below — see the checklist in
/// `backends/mod.rs`.
pub struct Registry {
    backends: Vec<Box<dyn MediaBackend>>,
}

impl Registry {
    pub fn new() -> Self {
        Registry {
            backends: vec![
                Box::new(backends::local::LocalBackend),
                Box::new(backends::openrouter::OpenRouterBackend::new()),
                Box::new(backends::openai::OpenAiBackend::new()),
            ],
        }
    }

    pub fn get(&self, backend_id: &str) -> Option<&dyn MediaBackend> {
        self.backends
            .iter()
            .find(|b| b.descriptor().id == backend_id)
            .map(|b| b.as_ref())
    }

    /// Backends that hold their credential *and* can actually run — a key with
    /// no engine behind it is not a usable backend.
    pub fn available(&self, db: &Db) -> Vec<&dyn MediaBackend> {
        self.backends
            .iter()
            .filter(|b| credential_present(b.descriptor()) && b.is_ready(db))
            .map(|b| b.as_ref())
            .collect()
    }

    /// Union of every available backend's models.
    ///
    /// Only the *hosted* backends are cached, and only they need to be: their
    /// catalog is an HTTP round trip, and the picker should not pay for one
    /// every time it opens. A local backend's list is a file-exists check, so
    /// it is always read live — which also means installing an engine or
    /// picking a checkpoint shows up immediately instead of waiting out a TTL.
    pub async fn all_models(&self, db: &Db, modality: Option<Modality>) -> Vec<MediaModel> {
        let mut out = Vec::new();
        let cached = cached_models();
        let mut fetched: Option<Vec<MediaModel>> = cached.is_none().then(Vec::new);

        for backend in self.available(db) {
            let hosted = !matches!(backend.descriptor().credential, Credential::Local);
            if hosted && cached.is_some() {
                continue; // served from the cache below
            }
            let Ok(models) = backend.list_models(db).await else { continue };
            if hosted {
                if let Some(f) = fetched.as_mut() {
                    f.extend(models.clone());
                }
            }
            out.extend(models);
        }

        match (cached, fetched) {
            (Some(hit), _) => out.extend(hit),
            (None, Some(f)) => store_models(&f),
            (None, None) => {}
        }
        filter_modality(out, modality)
    }
}

/// How long a fetched catalog stays good. Provider catalogs move on the order
/// of weeks; the cost of being six hours stale is nil next to an HTTP round
/// trip every time the picker opens.
const MODEL_CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);

/// The cached catalog and when it was fetched.
type ModelCache = Mutex<Option<(Instant, Vec<MediaModel>)>>;

fn model_cache() -> &'static ModelCache {
    static CACHE: OnceLock<ModelCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn cached_models() -> Option<Vec<MediaModel>> {
    let guard = model_cache().lock().ok()?;
    let (at, models) = guard.as_ref()?;
    (at.elapsed() < MODEL_CACHE_TTL).then(|| models.clone())
}

fn store_models(models: &[MediaModel]) {
    if let Ok(mut guard) = model_cache().lock() {
        *guard = Some((Instant::now(), models.to_vec()));
    }
}

/// A model from the cached catalog, by its `media:<backend>/<slug>` id.
/// `None` when the cache is cold — callers must treat that as "assume
/// nothing", never as "assume the default".
pub fn cached_model(model_id: &str) -> Option<MediaModel> {
    let guard = model_cache().lock().ok()?;
    let (_, models) = guard.as_ref()?;
    models.iter().find(|m| m.id == model_id).cloned()
}

/// Drop the cached hosted catalog so the next `all_models` refetches — for
/// when a key is added or removed and the previous answer is now wrong.
pub fn invalidate_model_cache() {
    if let Ok(mut guard) = model_cache().lock() {
        *guard = None;
    }
}

fn filter_modality(models: Vec<MediaModel>, modality: Option<Modality>) -> Vec<MediaModel> {
    match modality {
        None => models,
        Some(want) => models.into_iter().filter(|m| m.modality == want).collect(),
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

// `descriptor` alone decides this today (`secrets::has_secret` reads straight
// from the OS keychain), but `available()` still threads `db` through in case
// a later credential kind needs a DB-backed check.
fn credential_present(descriptor: &BackendDescriptor) -> bool {
    match &descriptor.credential {
        Credential::Local => true,
        Credential::Cloud(provider) => secrets::has_secret(SERVICE_CLOUD, provider.id()),
        Credential::Media { .. } => secrets::has_secret(SERVICE_MEDIA, descriptor.id),
    }
}

/// Parse a `media:<backend_id>/<slug>` id into its parts.
pub fn parse_model_id(model_id: &str) -> Option<(&str, &str)> {
    model_id.strip_prefix("media:")?.split_once('/')
}

/// Settings keys for the inferred route's preferences. Free-text backend ids,
/// so adding a backend never needs a migration.
pub const PRIMARY_IMAGE_KEY: &str = "media.primary_image";
pub const PRIMARY_VIDEO_KEY: &str = "media.primary_video";
pub const FALLBACKS_KEY: &str = "media.fallbacks";

/// Resolution for the declared route (`PIK-2`): the user picked an exact
/// model in the chooser, so its id names the backend directly rather than
/// going through availability precedence.
pub fn resolve_backend_for_model<'a>(registry: &'a Registry, model_id: &str) -> Result<(&'a dyn MediaBackend, String), String> {
    let (backend_id, slug) = parse_model_id(model_id)
        .ok_or_else(|| format!("\"{model_id}\" isn't a media model id."))?;
    let backend = registry
        .get(backend_id)
        .filter(|b| credential_present(b.descriptor()))
        .ok_or_else(|| format!("The {backend_id} backend isn't set up anymore."))?;
    Ok((backend, slug.to_string()))
}

/// Resolution for the inferred route (`PIK-3`) and the agent's tool: nothing
/// was declared, so walk the precedence chain — the `media.primary_*` setting,
/// then `media.fallbacks` in order, then whatever is available.
///
/// The last step matters more than it looks. It used to be the *only* step,
/// and because the local backend reported itself available unconditionally it
/// always won — so a user with an OpenRouter key and no diffusion engine got
/// "the image engine isn't installed" from a machine that could plainly make
/// the picture. `is_ready` is what fixed that; this chain is what lets someone
/// with both choose between them.
pub fn resolve_backend<'a>(registry: &'a Registry, db: &Db, modality: Modality) -> Result<&'a dyn MediaBackend, String> {
    let usable: Vec<&dyn MediaBackend> = registry
        .available(db)
        .into_iter()
        .filter(|b| b.descriptor().modalities.contains(&modality))
        .collect();

    let setting = |key: &str| db.get_setting(key).ok().flatten().filter(|s| !s.trim().is_empty());

    let primary_key = match modality {
        Modality::Image => PRIMARY_IMAGE_KEY,
        Modality::Video => PRIMARY_VIDEO_KEY,
    };
    let mut preferred: Vec<String> = Vec::new();
    if let Some(p) = setting(primary_key) {
        preferred.push(p.trim().to_string());
    }
    if let Some(f) = setting(FALLBACKS_KEY) {
        preferred.extend(f.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    }

    for want in &preferred {
        // A preference naming a backend that is gone or unusable is skipped,
        // not an error — the point of a fallback list is to survive that.
        if let Some(b) = usable.iter().find(|b| b.descriptor().id == want) {
            return Ok(*b);
        }
    }

    usable.into_iter().next().ok_or_else(|| match modality {
        Modality::Image => {
            "No image backend is set up yet. Install the local image engine under Engine → Image, or add a cloud key in Settings → Cloud."
                .to_string()
        }
        Modality::Video => "No video backend is set up yet. Add an OpenRouter key in Settings → Cloud.".to_string(),
    })
}

/// Records a generation result as a real artifact — the single place both the
/// composer's direct command and the agent's tool call converge (`ART-2`).
pub fn record(
    db: &Db,
    conversation_id: Option<&str>,
    req: &MediaRequest,
    res: &MediaResult,
    parent_id: Option<&str>,
    modality: Modality,
    message_id: Option<&str>,
) -> Result<Artifact, DbError> {
    let title = ellipsize(&req.prompt, 60);
    let meta = serde_json::json!({
        "model_id": res.model_id,
        "provider_label": res.provider_label,
        "prompt": req.prompt,
        "negative": req.negative,
        "seed": res.seed,
        "cost_usd": res.cost_usd,
        "width": res.width,
        "height": res.height,
        "duration_secs": res.duration_secs,
        "mime": res.mime,
        "aspect_ratio": req.aspect_ratio,
        "ignored": res.ignored,
    })
    .to_string();
    db.add_artifact_with(
        conversation_id,
        &title,
        modality.as_kind(),
        &res.path.to_string_lossy(),
        Some(&meta),
        parent_id,
        message_id,
    )
}

// ---------------------------------------------------------------------------
// Shared backend helpers (`BKD-2`). These live here, not in each backend, or
// "adding a provider is one file" stops being true.
// ---------------------------------------------------------------------------

/// The ratios the whole app speaks. Backends map these onto whatever their own
/// API wants (pixels, size strings, their own ratio list).
pub const ASPECT_RATIOS: &[&str] = &["1:1", "16:9", "9:16", "4:3", "3:4", "21:9"];

/// Fold a free-text ratio ("16x9", " 16:9 ", "1.78") onto a known tier.
pub fn normalize_aspect_ratio(raw: &str) -> Option<&'static str> {
    let cleaned = raw.trim().replace(['x', 'X', '/'], ":");
    if let Some(hit) = ASPECT_RATIOS.iter().find(|r| **r == cleaned) {
        return Some(hit);
    }
    // Numeric ("1.78") or an unlisted pair ("1920:1080") — match on the value.
    let value = match cleaned.split_once(':') {
        Some((w, h)) => w.trim().parse::<f64>().ok()? / h.trim().parse::<f64>().ok()?,
        None => cleaned.parse::<f64>().ok()?,
    };
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    ASPECT_RATIOS.iter().copied().min_by(|a, b| {
        let d = |r: &str| {
            let (w, h) = r.split_once(':').unwrap();
            (w.parse::<f64>().unwrap() / h.parse::<f64>().unwrap() - value).abs()
        };
        d(a).partial_cmp(&d(b)).unwrap_or(std::cmp::Ordering::Equal)
    })
}

/// Pick the entry closest to `want` from what a model actually supports, and
/// say so when it isn't what was asked for — normalise, report, never fail.
/// The returned `bool` is "this was substituted", which callers push into
/// `MediaResult::ignored`.
pub fn nearest_supported(want: &str, supported: &[String]) -> Option<(String, bool)> {
    if supported.is_empty() {
        return None;
    }
    if supported.iter().any(|s| s == want) {
        return Some((want.to_string(), false));
    }
    let ratio_of = |r: &str| -> Option<f64> {
        let (w, h) = r.split_once(':')?;
        Some(w.trim().parse::<f64>().ok()? / h.trim().parse::<f64>().ok()?)
    };
    let target = ratio_of(want)?;
    supported
        .iter()
        .filter_map(|s| ratio_of(s).map(|v| (s.clone(), (v - target).abs())))
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(s, _)| (s, true))
}

/// Width/height straight out of the file header — PNG's IHDR, JPEG's SOFn, or
/// WebP's VP8X/VP8L/VP8 — so a caption can say "1024×1024" without pulling a
/// whole decode through for two numbers.
pub fn probe_dimensions(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    // PNG: 8-byte signature, then IHDR with width/height at offsets 16 and 20.
    if bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return (Some(w), Some(h));
    }
    // JPEG: walk the segment chain to the first start-of-frame marker.
    if bytes.len() > 4 && bytes.starts_with(b"\xff\xd8") {
        let mut i = 2usize;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xff {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            // SOF0..SOF15, minus the non-frame markers that share the range.
            if (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
                let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
                return (Some(w), Some(h));
            }
            let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            i += 2 + len;
        }
        return (None, None);
    }
    // WebP: only the extended (VP8X) form carries dimensions cheaply.
    if bytes.len() >= 30 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" && &bytes[12..16] == b"VP8X" {
        let w = 1 + u32::from_le_bytes([bytes[24], bytes[25], bytes[26], 0]);
        let h = 1 + u32::from_le_bytes([bytes[27], bytes[28], bytes[29], 0]);
        return (Some(w), Some(h));
    }
    (None, None)
}

/// Write generated bytes into the media directory under a fresh name. Every
/// hosted backend must land its result here before returning: provider URLs
/// expire, and Library has to outlive them.
pub fn materialize(out_dir: &Path, prefix: &str, ext: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("couldn't create the output directory: {e}"))?;
    let path = out_dir.join(format!("{prefix}-{}.{ext}", uuid::Uuid::new_v4().simple()));
    std::fs::write(&path, bytes).map_err(|e| format!("couldn't save the generated file: {e}"))?;
    Ok(path)
}

/// A `data:` URI for a reference image, the shape every hosted image API wants
/// its `input_references` in.
pub fn data_uri_for(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("couldn't read a reference image: {e}"))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{};base64,{b64}", mime_for_path(path)))
}

pub fn mime_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "image/jpeg",
    }
}

/// How an async backend waits on a submitted job. `poll` returns `Ok(Some(v))`
/// when the job is done, `Ok(None)` to keep waiting, and `Err` to give up.
///
/// The interval widens after `slow_after` because the first minute is where a
/// fast job finishes and the tenth is where nothing is gained by asking often.
pub async fn poll_until_done<T, F, Fut>(
    ceiling: Duration,
    slow_after: Duration,
    cancel: &CancelFlag,
    mut poll: F,
) -> Result<T, String>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Option<T>, String>>,
{
    let start = Instant::now();
    loop {
        if cancel.is_cancelled() {
            return Err(CANCELLED.to_string());
        }
        if start.elapsed() > ceiling {
            return Err(format!(
                "This took longer than {} minutes and was given up on.",
                ceiling.as_secs() / 60
            ));
        }
        let interval = if start.elapsed() > slow_after { 3 } else { 2 };
        tokio::time::sleep(Duration::from_secs(interval)).await;
        if let Some(done) = poll().await? {
            return Ok(done);
        }
    }
}

/// The error a backend returns when it noticed the cancel flag. The job layer
/// recognises it and reports the job cancelled rather than failed, so a
/// deliberate stop never shows the user an error.
pub const CANCELLED: &str = "cancelled";

/// Truncate on a `char` boundary, never a byte index — a `&str[..n]` slice
/// panics the instant the cut lands inside a multi-byte character (`FIX-1`).
pub fn ellipsize(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ellipsize_leaves_short_strings_alone() {
        assert_eq!(ellipsize("a fox", 40), "a fox");
    }

    #[test]
    fn ellipsize_cuts_on_char_boundaries_not_bytes() {
        // Every char here is multi-byte in UTF-8; a byte-index slice at 5
        // would panic. A char-index cut must not.
        let prompt = "Zeichne eine Straße bei Nacht mit Laternen und Nebel 🦊🌫️";
        let short = ellipsize(prompt, 5);
        assert_eq!(short, "Zeich…");
    }

    #[test]
    fn ellipsize_handles_emoji_at_the_boundary() {
        let prompt = "fox🦊fox🦊fox";
        let short = ellipsize(prompt, 4);
        assert_eq!(short.chars().count(), 5); // 4 chars + the ellipsis char
    }

    #[test]
    fn normalizes_ratios_written_any_of_the_usual_ways() {
        assert_eq!(normalize_aspect_ratio(" 16:9 "), Some("16:9"));
        assert_eq!(normalize_aspect_ratio("16x9"), Some("16:9"));
        assert_eq!(normalize_aspect_ratio("1920:1080"), Some("16:9"));
        assert_eq!(normalize_aspect_ratio("1.0"), Some("1:1"));
        assert_eq!(normalize_aspect_ratio("banana"), None);
    }

    #[test]
    fn nearest_supported_substitutes_and_says_so() {
        let supported = vec!["1:1".to_string(), "16:9".to_string()];
        assert_eq!(nearest_supported("16:9", &supported), Some(("16:9".into(), false)));
        // 21:9 isn't offered; 16:9 is the closest thing that is, and the flag
        // is what puts "21:9 wasn't available" in front of the user.
        assert_eq!(nearest_supported("21:9", &supported), Some(("16:9".into(), true)));
    }

    #[test]
    fn probes_png_dimensions_from_the_header() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&[0, 0, 0, 13]);
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&1024u32.to_be_bytes());
        png.extend_from_slice(&768u32.to_be_bytes());
        assert_eq!(probe_dimensions(&png), (Some(1024), Some(768)));
        assert_eq!(probe_dimensions(b"not an image"), (None, None));
    }

    // ---- `BKD-2`'s acceptance test ----
    //
    // The seam's promise is that a new provider costs one file and one line in
    // `Registry::new()`. This registers a backend the same way a real one is
    // registered and asserts it is discoverable, selectable and able to
    // generate — with nothing outside `media/` touched. If this test ever needs
    // an edit somewhere else to pass, the seam has regressed.

    struct TestBackend;

    static TEST_DESCRIPTOR: BackendDescriptor = BackendDescriptor {
        id: "test",
        label: "Test",
        modalities: &[Modality::Image],
        credential: Credential::Local,
        supports_references: false,
        supports_edit: false,
        is_async: false,
        console_url: None,
    };

    #[async_trait]
    impl MediaBackend for TestBackend {
        fn descriptor(&self) -> &'static BackendDescriptor {
            &TEST_DESCRIPTOR
        }

        async fn list_models(&self, _db: &Db) -> Result<Vec<MediaModel>, String> {
            Ok(vec![MediaModel {
                id: "media:test/swatch".to_string(),
                name: "Swatch".to_string(),
                backend_id: TEST_DESCRIPTOR.id.to_string(),
                backend_label: TEST_DESCRIPTOR.label.to_string(),
                modality: Modality::Image,
                price_label: None,
                supports_edit: false,
                supports_streaming: false,
                supported_aspect_ratios: vec!["1:1".to_string()],
                supported_resolutions: vec![],
                max_duration_secs: None,
            }])
        }

        async fn generate(
            &self,
            _db: &Db,
            _req: &MediaRequest,
            out_dir: &Path,
            _cancel: &CancelFlag,
        ) -> Result<MediaResult, String> {
            let path = materialize(out_dir, "test", "png", b"\x89PNG\r\n\x1a\n")?;
            Ok(MediaResult {
                path,
                mime: "image/png".to_string(),
                width: None,
                height: None,
                duration_secs: None,
                model_id: "test:swatch".to_string(),
                provider_label: "Swatch".to_string(),
                seed: None,
                cost_usd: None,
                ignored: Vec::new(),
            })
        }
    }

    /// The registry as `Registry::new()` builds it, plus the one line a new
    /// provider costs.
    fn registry_with_test_backend() -> Registry {
        Registry { backends: vec![Box::new(TestBackend)] }
    }

    #[tokio::test]
    async fn a_backend_registered_with_one_line_is_listed_selectable_and_generates() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let registry = registry_with_test_backend();

        // Listed. (`all_models` is cached process-wide, so go through the
        // backend the way `all_models` does rather than fighting the cache.)
        let models = registry.available(&db)[0].list_models(&db).await.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "media:test/swatch");

        // Selectable, by both routes.
        let (by_id, slug) = resolve_backend_for_model(&registry, "media:test/swatch").unwrap();
        assert_eq!(by_id.descriptor().id, "test");
        assert_eq!(slug, "swatch");
        let inferred = resolve_backend(&registry, &db, Modality::Image).unwrap();
        assert_eq!(inferred.descriptor().id, "test");

        // Generates, and the result records as a real artifact.
        let req = MediaRequest { prompt: "a swatch".to_string(), ..Default::default() };
        let res = inferred.generate(&db, &req, dir.path(), &CancelFlag::new()).await.unwrap();
        assert!(res.path.exists());
        let artifact = record(&db, None, &req, &res, None, Modality::Image, None).unwrap();
        assert_eq!(artifact.kind, "image");
        assert_eq!(artifact.title, "a swatch");
    }

    #[tokio::test]
    async fn an_unusable_backend_never_shadows_a_usable_one() {
        struct NotReady;
        static NR: BackendDescriptor = BackendDescriptor {
            id: "notready",
            label: "Not ready",
            modalities: &[Modality::Image],
            credential: Credential::Local,
            supports_references: false,
            supports_edit: false,
            is_async: false,
            console_url: None,
        };
        #[async_trait]
        impl MediaBackend for NotReady {
            fn descriptor(&self) -> &'static BackendDescriptor {
                &NR
            }
            async fn list_models(&self, _db: &Db) -> Result<Vec<MediaModel>, String> {
                Ok(vec![])
            }
            async fn generate(
                &self,
                _db: &Db,
                _r: &MediaRequest,
                _o: &Path,
                _c: &CancelFlag,
            ) -> Result<MediaResult, String> {
                Err("should never be selected".into())
            }
            fn is_ready(&self, _db: &Db) -> bool {
                false
            }
        }

        let _dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        // The unusable one is registered *first*, exactly as the local backend
        // is — this is the bug that made the tool path unable to reach a cloud
        // backend that was sitting right there with a valid key.
        let registry = Registry { backends: vec![Box::new(NotReady), Box::new(TestBackend)] };

        assert_eq!(registry.available(&db).len(), 1);
        let picked = resolve_backend(&registry, &db, Modality::Image).unwrap();
        assert_eq!(picked.descriptor().id, "test");
    }

    #[tokio::test]
    async fn the_primary_setting_outranks_registration_order() {
        struct Other;
        static OTHER: BackendDescriptor = BackendDescriptor {
            id: "other",
            label: "Other",
            modalities: &[Modality::Image],
            credential: Credential::Local,
            supports_references: false,
            supports_edit: false,
            is_async: false,
            console_url: None,
        };
        #[async_trait]
        impl MediaBackend for Other {
            fn descriptor(&self) -> &'static BackendDescriptor {
                &OTHER
            }
            async fn list_models(&self, _db: &Db) -> Result<Vec<MediaModel>, String> {
                Ok(vec![])
            }
            async fn generate(
                &self,
                _db: &Db,
                _r: &MediaRequest,
                _o: &Path,
                _c: &CancelFlag,
            ) -> Result<MediaResult, String> {
                Err("unused".into())
            }
        }

        let _dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let registry = Registry { backends: vec![Box::new(TestBackend), Box::new(Other)] };

        // Registration order alone would pick `test`.
        assert_eq!(resolve_backend(&registry, &db, Modality::Image).unwrap().descriptor().id, "test");

        db.set_setting(PRIMARY_IMAGE_KEY, "other").unwrap();
        assert_eq!(resolve_backend(&registry, &db, Modality::Image).unwrap().descriptor().id, "other");

        // A preference naming something unavailable falls through rather than
        // failing — that is the whole point of having a fallback list.
        db.set_setting(PRIMARY_IMAGE_KEY, "gone").unwrap();
        db.set_setting(FALLBACKS_KEY, "also-gone, other").unwrap();
        assert_eq!(resolve_backend(&registry, &db, Modality::Image).unwrap().descriptor().id, "other");
    }
}
