//! OpenRouter media backend (`ORI-1`, `VID-1`). Reuses the OpenRouter chat
//! provider's existing key (`Credential::Cloud`) — no new auth surface, which
//! is the entire reason this backend goes first in the build order.
//!
//! Image generation is one blocking POST. Video is polled: submit, then poll
//! `polling_url` until `status == "completed"`.

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;

use crate::cloud::{self, Provider};
use crate::db::Db;
use crate::runtime::proxy::CancelFlag;
use crate::media::{
    self, BackendDescriptor, Credential, MediaBackend, MediaModel, MediaRequest, MediaResult, Modality,
};

static DESCRIPTOR: BackendDescriptor = BackendDescriptor {
    id: "openrouter",
    label: "OpenRouter",
    modalities: &[Modality::Image, Modality::Video],
    credential: Credential::Cloud(Provider::OpenRouter),
    supports_references: true,
    supports_edit: true,
    is_async: true,
    console_url: Some("https://openrouter.ai/keys"),
};

/// Curated fallback, used when the live catalog can't be reached (rate
/// limited, offline, or the endpoint shape has moved since this was written —
/// `ORI-2`'s discovery is "verify at implementation time" territory for a
/// newer API). A curated list beats an empty picker group with a key present.
const CURATED_IMAGE_MODELS: &[(&str, &str)] = &[
    ("google/gemini-2.5-flash-image", "Nano Banana Pro"),
    ("black-forest-labs/flux-1.1-pro", "FLUX 1.1 Pro"),
    ("openai/gpt-image-1", "GPT Image 1"),
];
const CURATED_VIDEO_MODELS: &[(&str, &str)] = &[("google/veo-3.1", "Veo 3.1")];

pub struct OpenRouterBackend {
    client: reqwest::Client,
}

impl OpenRouterBackend {
    pub fn new() -> Self {
        Self { client: reqwest::Client::new() }
    }
}

impl Default for OpenRouterBackend {
    fn default() -> Self {
        Self::new()
    }
}

fn curated_models(modality: Modality) -> Vec<MediaModel> {
    let (list, ratios, max_dur): (&[(&str, &str)], Vec<String>, Option<u32>) = match modality {
        Modality::Image => (
            CURATED_IMAGE_MODELS,
            vec!["1:1".into(), "16:9".into(), "9:16".into(), "4:3".into(), "3:4".into(), "21:9".into()],
            None,
        ),
        Modality::Video => (CURATED_VIDEO_MODELS, vec!["16:9".into(), "9:16".into()], Some(10)),
    };
    list.iter()
        .map(|(slug, name)| MediaModel {
            id: format!("media:openrouter/{slug}"),
            name: (*name).to_string(),
            backend_id: DESCRIPTOR.id.to_string(),
            backend_label: DESCRIPTOR.label.to_string(),
            modality,
            price_label: None,
            supports_edit: matches!(modality, Modality::Image),
            supports_streaming: false,
            supported_aspect_ratios: ratios.clone(),
            supported_resolutions: vec!["1K".into(), "2K".into()],
            max_duration_secs: max_dur,
        })
        .collect()
}

fn key() -> Result<String, String> {
    cloud::get_key(Provider::OpenRouter).ok_or_else(|| "No OpenRouter key is set. Add one in Settings → Cloud.".to_string())
}

async fn provider_error(resp: reqwest::Response) -> String {
    let status = resp.status();
    match resp.json::<serde_json::Value>().await {
        Ok(body) => body
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .map(|m| m.to_string())
            .unwrap_or_else(|| format!("OpenRouter returned {status}")),
        Err(_) => format!("OpenRouter returned {status}"),
    }
}

#[async_trait]
impl MediaBackend for OpenRouterBackend {
    fn descriptor(&self) -> &'static BackendDescriptor {
        &DESCRIPTOR
    }

    async fn list_models(&self, _db: &Db) -> Result<Vec<MediaModel>, String> {
        let Ok(k) = key() else { return Ok(vec![]) };
        let resp = self
            .client
            .get("https://openrouter.ai/api/v1/images/models")
            .bearer_auth(&k)
            .timeout(Duration::from_secs(10))
            .send()
            .await;
        let Ok(resp) = resp else { return Ok(curated_models(Modality::Image)) };
        if !resp.status().is_success() {
            return Ok(curated_models(Modality::Image));
        }
        let Ok(body) = resp.json::<serde_json::Value>().await else {
            return Ok(curated_models(Modality::Image));
        };
        let data = body.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default();
        if data.is_empty() {
            return Ok(curated_models(Modality::Image));
        }
        Ok(data
            .iter()
            .filter_map(|m| {
                let id = m.get("id")?.as_str()?.to_string();
                let name = m.get("name").and_then(|n| n.as_str()).unwrap_or(&id).to_string();
                let price = m
                    .get("pricing")
                    .and_then(|p| p.get("image"))
                    .and_then(|v| v.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| v.as_f64()));
                Some(MediaModel {
                    id: format!("media:openrouter/{id}"),
                    name,
                    backend_id: DESCRIPTOR.id.to_string(),
                    backend_label: DESCRIPTOR.label.to_string(),
                    modality: Modality::Image,
                    price_label: price.map(|p| format!("${p:.2}")),
                    supports_edit: true,
                    // Only what the provider itself claims. Guessing here
                    // would mean sending `stream: true` to a model that
                    // rejects it, breaking generation for a nicety (`STR-4`).
                    supports_streaming: m
                        .get("supports_streaming")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    supported_aspect_ratios: vec![
                        "1:1".into(), "16:9".into(), "9:16".into(), "4:3".into(), "3:4".into(), "21:9".into(),
                    ],
                    supported_resolutions: vec!["1K".into(), "2K".into(), "4K".into()],
                    max_duration_secs: None,
                })
            })
            .collect())
    }

    async fn generate(
        &self,
        _db: &Db,
        req: &MediaRequest,
        out_dir: &Path,
        cancel: &CancelFlag,
    ) -> Result<MediaResult, String> {
        if cancel.is_cancelled() {
            return Err(media::CANCELLED.to_string());
        }
        if req.modality == Some(Modality::Video) {
            return self.generate_video(req, out_dir, cancel).await;
        }
        let k = key()?;
        if req.references.len() > 8 {
            return Err("Up to 8 reference images are supported.".into());
        }

        let model = req
            .model
            .clone()
            .unwrap_or_else(|| CURATED_IMAGE_MODELS[0].0.to_string());

        let mut input_references = Vec::new();
        for r in &req.references {
            input_references.push(serde_json::json!({
                "type": "image_url",
                "image_url": { "url": media::data_uri_for(&r.path)? }
            }));
        }

        let mut body = serde_json::json!({
            "model": model,
            "prompt": req.effective_prompt(),
            "n": 1,
            "output_format": "png",
        });
        if let Some(ar) = &req.aspect_ratio {
            body["aspect_ratio"] = serde_json::json!(ar);
        }
        if let Some(res) = &req.resolution {
            body["resolution"] = serde_json::json!(res);
        }
        if let Some(seed) = req.seed {
            body["seed"] = serde_json::json!(seed);
        }
        if !input_references.is_empty() {
            body["input_references"] = serde_json::json!(input_references);
        }

        // `STR-4`: stream only when there is a job listening *and* the
        // provider's own catalog says this model emits partials. A cold cache
        // reports nothing, which correctly means "don't". The fallback below
        // makes a wrong guess survivable, but the guard is what makes it rare.
        let streaming = req.job_id.is_some()
            && media::cached_model(&format!("media:openrouter/{model}"))
                .map(|m| m.supports_streaming)
                .unwrap_or(false);

        let payload = if streaming {
            match self.stream_image(&k, &body, req.job_id.as_deref()).await {
                Ok(payload) => payload,
                // The stream shape is newer than the plain endpoint and this
                // was written without an account to check it against. Falling
                // back costs one extra request; not falling back would cost
                // the user their picture.
                Err(_) => self.post_image(&k, &body).await?,
            }
        } else {
            self.post_image(&k, &body).await?
        };
        let entry = payload
            .get("data")
            .and_then(|d| d.as_array())
            .and_then(|a| a.first())
            .ok_or("OpenRouter didn't return an image")?;
        let b64 = entry
            .get("b64_json")
            .and_then(|v| v.as_str())
            .ok_or("OpenRouter's response had no image data")?;
        let media_type = entry.get("media_type").and_then(|v| v.as_str()).unwrap_or("image/png");
        let cost_usd = payload.get("usage").and_then(|u| u.get("cost")).and_then(|c| c.as_f64());

        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| format!("couldn't decode the returned image: {e}"))?;
        let ext = if media_type.contains("webp") { "webp" } else if media_type.contains("jpeg") { "jpg" } else { "png" };
        let out_path = media::materialize(out_dir, "img", ext, &bytes)?;
        let (width, height) = media::probe_dimensions(&bytes);

        let mut ignored = Vec::new();
        if req.width.is_some() || req.height.is_some() {
            ignored.push("width/height (this model uses aspect ratio)".to_string());
        }

        Ok(MediaResult {
            path: out_path,
            mime: media_type.to_string(),
            width,
            height,
            duration_secs: None,
            model_id: format!("openrouter:{model}"),
            provider_label: CURATED_IMAGE_MODELS
                .iter()
                .find(|(slug, _)| *slug == model)
                .map(|(_, name)| name.to_string())
                .unwrap_or(model),
            seed: req.seed,
            cost_usd,
            ignored,
        })
    }
}

impl OpenRouterBackend {
    /// One plain, non-streaming POST — the path everything used before
    /// `STR-4`, and the fallback when streaming doesn't work out.
    async fn post_image(&self, key: &str, body: &serde_json::Value) -> Result<serde_json::Value, String> {
        let resp = self
            .client
            .post("https://openrouter.ai/api/v1/images")
            .bearer_auth(key)
            .json(body)
            .timeout(Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| format!("couldn't reach OpenRouter: {e}"))?;
        if !resp.status().is_success() {
            return Err(provider_error(resp).await);
        }
        resp.json().await.map_err(|e| format!("bad response from OpenRouter: {e}"))
    }

    /// `STR-4`: the same request with `stream: true`, forwarding each
    /// `image_generation.partial_image` to the placeholder as it arrives and
    /// returning the final payload in the shape `post_image` would have.
    ///
    /// Every partial is best-effort: one that doesn't parse is skipped rather
    /// than failing the generation, because a missed frame of a progress
    /// animation is not worth losing a picture over.
    async fn stream_image(
        &self,
        key: &str,
        body: &serde_json::Value,
        job_id: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        use futures_util::StreamExt;

        let mut body = body.clone();
        body["stream"] = serde_json::json!(true);

        let resp = self
            .client
            .post("https://openrouter.ai/api/v1/images")
            .bearer_auth(key)
            .json(&body)
            .timeout(Duration::from_secs(180))
            .send()
            .await
            .map_err(|e| format!("couldn't reach OpenRouter: {e}"))?;
        if !resp.status().is_success() {
            return Err(provider_error(resp).await);
        }

        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        let mut final_payload: Option<serde_json::Value> = None;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("the image stream broke: {e}"))?;
            buf.push_str(&String::from_utf8_lossy(&chunk));

            // SSE frames are separated by a blank line; hold anything after
            // the last complete one until more bytes arrive.
            while let Some(split) = buf.find("\n\n") {
                let frame = buf[..split].to_string();
                buf.drain(..split + 2);

                let Some(data) = frame.lines().find_map(|l| l.strip_prefix("data:")) else { continue };
                let data = data.trim();
                if data == "[DONE]" {
                    continue;
                }
                let Ok(event) = serde_json::from_str::<serde_json::Value>(data) else { continue };

                match event.get("type").and_then(|t| t.as_str()) {
                    Some("image_generation.partial_image") => {
                        let (Some(job_id), Some(b64)) =
                            (job_id, event.get("b64_json").and_then(|v| v.as_str()))
                        else {
                            continue;
                        };
                        let mime = event.get("media_type").and_then(|v| v.as_str()).unwrap_or("image/png");
                        super::super::jobs::partial(job_id, format!("data:{mime};base64,{b64}"));
                    }
                    // The completed event carries the same `data[]` shape the
                    // non-streaming response does, so downstream parsing is
                    // identical either way.
                    Some("image_generation.completed") | None => {
                        if event.get("data").is_some() {
                            final_payload = Some(event);
                        }
                    }
                    _ => {}
                }
            }
        }

        final_payload.ok_or_else(|| "the image stream ended without an image".to_string())
    }

    /// Submit + poll — `VID-1`. Blocks this one call for up to the ceiling
    /// below rather than the agent turn: the full non-blocking job queue
    /// (`JOB-1`, with restart safety and a live elapsed-time UI) is real work
    /// this pass doesn't include. A Tauri command already runs off the UI
    /// thread, so the app stays responsive; what's missing is the ability to
    /// send another message while this one is still running, and a Cancel
    /// button that means anything before the ceiling.
    async fn generate_video(
        &self,
        req: &MediaRequest,
        out_dir: &Path,
        cancel: &CancelFlag,
    ) -> Result<MediaResult, String> {
        let k = key()?;
        let model = req.model.clone().unwrap_or_else(|| CURATED_VIDEO_MODELS[0].0.to_string());

        let mut body = serde_json::json!({ "model": model, "prompt": req.effective_prompt() });
        if let Some(ar) = &req.aspect_ratio {
            body["aspect_ratio"] = serde_json::json!(ar);
        }
        if let Some(secs) = req.duration_secs {
            body["duration"] = serde_json::json!(secs);
        }
        if !req.references.is_empty() {
            let mut frame_images = Vec::new();
            for r in &req.references {
                frame_images.push(serde_json::json!({
                    "type": "image_url",
                    "image_url": { "url": media::data_uri_for(&r.path)? }
                }));
            }
            body["frame_images"] = serde_json::json!(frame_images);
        }

        let submitted = self
            .client
            .post("https://openrouter.ai/api/v1/videos")
            .bearer_auth(&k)
            .json(&body)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("couldn't reach OpenRouter: {e}"))?;
        if !submitted.status().is_success() {
            return Err(provider_error(submitted).await);
        }
        let submission: serde_json::Value = submitted.json().await.map_err(|e| format!("bad response from OpenRouter: {e}"))?;
        let polling_url = submission
            .get("polling_url")
            .and_then(|v| v.as_str())
            .ok_or("OpenRouter didn't return a polling URL")?
            .to_string();

        // 2s interval, 3s after the first minute, hard ceiling 10 minutes —
        // the plan's own numbers, verified against nothing beyond its own
        // spec since a live account was never in reach while writing this.
        let payload = media::poll_until_done(Duration::from_secs(600), Duration::from_secs(60), cancel, || async {
            let poll = self
                .client
                .get(&polling_url)
                .bearer_auth(&k)
                .timeout(Duration::from_secs(15))
                .send()
                .await
                .map_err(|e| format!("couldn't reach OpenRouter: {e}"))?;
            if !poll.status().is_success() {
                return Err(provider_error(poll).await);
            }
            let status: serde_json::Value = poll.json().await.map_err(|e| format!("bad response from OpenRouter: {e}"))?;
            match status.get("status").and_then(|s| s.as_str()) {
                Some("completed") => Ok(Some(status)),
                Some("failed") => {
                    let msg = status.get("error").and_then(|e| e.as_str()).unwrap_or("the provider reported a failure");
                    Err(format!("Video generation failed: {msg}"))
                }
                _ => Ok(None),
            }
        })
        .await?;

        let video_url = payload
            .get("unsigned_urls")
            .and_then(|u| u.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
            .ok_or("OpenRouter's response had no video URL")?;
        let cost_usd = payload.get("usage").and_then(|u| u.get("cost")).and_then(|c| c.as_f64());

        let bytes = self
            .client
            .get(video_url)
            .timeout(Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| format!("couldn't fetch the finished video: {e}"))?
            .bytes()
            .await
            .map_err(|e| format!("couldn't read the finished video: {e}"))?;

        let out_path = media::materialize(out_dir, "vid", "mp4", &bytes)?;

        Ok(MediaResult {
            path: out_path,
            mime: "video/mp4".to_string(),
            width: None,
            height: None,
            duration_secs: req.duration_secs.map(|s| s as f32),
            model_id: format!("openrouter:{model}"),
            provider_label: CURATED_VIDEO_MODELS
                .iter()
                .find(|(slug, _)| *slug == model)
                .map(|(_, name)| name.to_string())
                .unwrap_or(model),
            seed: None,
            cost_usd,
            ignored: Vec::new(),
        })
    }
}

