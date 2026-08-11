//! OpenAI BYOK image backend (`OAI-1`). Exists so a user with only an OpenAI
//! key, and no OpenRouter key, isn't locked out of Path A. `POST
//! /v1/images/generations`, or `/v1/images/edits` (multipart) when references
//! are present. No useful discovery endpoint for image models, so the
//! catalog is curated, the same tradeoff `ORI-1` documents for its fallback.

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;

use crate::cloud::{self, Provider};
use crate::db::Db;
use crate::media::{
    self, BackendDescriptor, Credential, MediaBackend, MediaModel, MediaRequest, MediaResult, Modality,
};

static DESCRIPTOR: BackendDescriptor = BackendDescriptor {
    id: "openai",
    label: "OpenAI",
    modalities: &[Modality::Image],
    credential: Credential::Cloud(Provider::OpenAi),
    supports_references: true,
    supports_edit: true,
    is_async: false,
    console_url: Some("https://platform.openai.com/api-keys"),
};

const CURATED_MODELS: &[(&str, &str)] = &[("gpt-image-1", "GPT Image 1")];

pub struct OpenAiBackend {
    client: reqwest::Client,
}

impl OpenAiBackend {
    pub fn new() -> Self {
        Self { client: reqwest::Client::new() }
    }
}

impl Default for OpenAiBackend {
    fn default() -> Self {
        Self::new()
    }
}

fn key() -> Result<String, String> {
    cloud::get_key(Provider::OpenAi).ok_or_else(|| "No OpenAI key is set. Add one in Settings → Cloud.".to_string())
}

/// The three shapes this model offers, as ratios, so the shared
/// `nearest_supported` can do the choosing.
const SUPPORTED_RATIOS: &[&str] = &["1:1", "16:9", "9:16"];

/// OpenAI's image endpoints take a fixed `WxH` string, not an aspect ratio.
/// Returns the size and, when the request asked for something this model
/// doesn't offer, the substitution to report back (`MediaResult::ignored`).
fn nearest_size(width: Option<i64>, height: Option<i64>, aspect_ratio: Option<&str>) -> (&'static str, Option<String>) {
    if let (Some(w), Some(h)) = (width, height) {
        let size = if w > h { "1536x1024" } else if h > w { "1024x1536" } else { "1024x1024" };
        return (size, None);
    }
    let Some(want) = aspect_ratio.and_then(media::normalize_aspect_ratio) else {
        return ("1024x1024", None);
    };
    let supported: Vec<String> = SUPPORTED_RATIOS.iter().map(|s| s.to_string()).collect();
    let (chosen, substituted) = media::nearest_supported(want, &supported).unwrap_or(("1:1".to_string(), false));
    let size = match chosen.as_str() {
        "16:9" => "1536x1024",
        "9:16" => "1024x1536",
        _ => "1024x1024",
    };
    (size, substituted.then(|| format!("{want} — made {chosen} instead")))
}

#[async_trait]
impl MediaBackend for OpenAiBackend {
    fn descriptor(&self) -> &'static BackendDescriptor {
        &DESCRIPTOR
    }

    async fn list_models(&self, _db: &Db) -> Result<Vec<MediaModel>, String> {
        if key().is_err() {
            return Ok(vec![]);
        }
        Ok(CURATED_MODELS
            .iter()
            .map(|(slug, name)| MediaModel {
                id: format!("media:openai/{slug}"),
                name: (*name).to_string(),
                backend_id: DESCRIPTOR.id.to_string(),
                backend_label: DESCRIPTOR.label.to_string(),
                modality: Modality::Image,
                price_label: None,
                supports_edit: true,
                supports_streaming: false,
                supported_aspect_ratios: vec!["1:1".into(), "16:9".into(), "9:16".into()],
                supported_resolutions: vec!["1K".into()],
                max_duration_secs: None,
            })
            .collect())
    }

    /// One blocking POST, so the only honest cancellation point is before it.
    async fn generate(
        &self,
        _db: &Db,
        req: &MediaRequest,
        out_dir: &Path,
        cancel: &crate::runtime::proxy::CancelFlag,
    ) -> Result<MediaResult, String> {
        if cancel.is_cancelled() {
            return Err(media::CANCELLED.to_string());
        }
        let k = key()?;
        let model = req.model.clone().unwrap_or_else(|| CURATED_MODELS[0].0.to_string());
        let (size, substitution) = nearest_size(req.width, req.height, req.aspect_ratio.as_deref());

        let (resp, edited) = if req.references.is_empty() {
            let body = serde_json::json!({
                "model": model,
                "prompt": req.effective_prompt(),
                "size": size,
                "n": 1,
                "output_format": "png",
            });
            let r = self
                .client
                .post("https://api.openai.com/v1/images/generations")
                .bearer_auth(&k)
                .json(&body)
                .timeout(Duration::from_secs(120))
                .send()
                .await
                .map_err(|e| format!("couldn't reach OpenAI: {e}"))?;
            (r, false)
        } else {
            if req.references.len() > 8 {
                return Err("Up to 8 reference images are supported.".into());
            }
            let mut form = reqwest::multipart::Form::new()
                .text("model", model.clone())
                .text("prompt", req.effective_prompt())
                .text("size", size.to_string())
                .text("n", "1");
            for r in &req.references {
                let bytes = std::fs::read(&r.path).map_err(|e| format!("couldn't read a reference image: {e}"))?;
                let name = r.path.file_name().and_then(|n| n.to_str()).unwrap_or("ref.png").to_string();
                form = form.part("image[]", reqwest::multipart::Part::bytes(bytes).file_name(name));
            }
            let r = self
                .client
                .post("https://api.openai.com/v1/images/edits")
                .bearer_auth(&k)
                .multipart(form)
                .timeout(Duration::from_secs(120))
                .send()
                .await
                .map_err(|e| format!("couldn't reach OpenAI: {e}"))?;
            (r, true)
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let msg = resp
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|b| b.get("error")?.get("message")?.as_str().map(str::to_string))
                .unwrap_or_else(|| format!("OpenAI returned {status}"));
            return Err(msg);
        }
        let payload: serde_json::Value = resp.json().await.map_err(|e| format!("bad response from OpenAI: {e}"))?;
        let entry = payload
            .get("data")
            .and_then(|d| d.as_array())
            .and_then(|a| a.first())
            .ok_or("OpenAI didn't return an image")?;
        let b64 = entry
            .get("b64_json")
            .and_then(|v| v.as_str())
            .ok_or("OpenAI's response had no image data")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| format!("couldn't decode the returned image: {e}"))?;

        let out_path = media::materialize(out_dir, "img", "png", &bytes)?;

        let (width, height) = size
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
            .unzip();

        Ok(MediaResult {
            path: out_path,
            mime: "image/png".to_string(),
            width,
            height,
            duration_secs: None,
            model_id: format!("openai:{model}{}", if edited { "/edit" } else { "" }),
            provider_label: CURATED_MODELS
                .iter()
                .find(|(slug, _)| *slug == model)
                .map(|(_, name)| name.to_string())
                .unwrap_or(model),
            seed: None,
            cost_usd: None,
            ignored: substitution.into_iter().collect(),
        })
    }
}
