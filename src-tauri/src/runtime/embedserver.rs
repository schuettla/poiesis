//! The embedding engine (Perception, EMB): a second `llama-server` process,
//! CPU-only, dedicated to turning text into vectors for semantic recall and
//! folder retrieval. It's the same `llama-server` binary the chat engine
//! uses — just launched with `--embeddings --pooling mean -ngl 0` against a
//! small embedding model, on its own loopback port, so it never competes with
//! the chat engine for VRAM.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};

use tauri::Manager;
use tokio::sync::Mutex;

use super::process::{spawn_engine, wait_until_healthy, EngineConfig, EngineError, EngineStatus, RunningEngine};

/// Idle timeout before the engine is stopped to free RAM (EMB-2). A call to
/// `ensure_started` resets the clock, so indexing a large folder holds it
/// open the whole time.
pub const IDLE_STOP: Duration = Duration::from_secs(5 * 60);
/// Embedding models are short-context by nature (bge/nomic top out around
/// 512 tokens); this is generous headroom without wasting RAM.
const EMBED_CTX_SIZE: u32 = 2048;
/// How many texts go in one `/v1/embeddings` request (EMB-3).
const EMBED_BATCH: usize = 32;

fn embed_extra_args() -> Vec<String> {
    vec!["--embeddings".into(), "--pooling".into(), "mean".into()]
}

/// A curated embedding model (EMB-4). Quantised embedders degrade noticeably
/// on retrieval quality, so both options are F16.
///
/// `note` is shown to the user, so it follows SMP-8a: no *embedding*,
/// *vector*, *index*, *reranker* or *chunk* in the copy.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EmbedCatalogEntry {
    pub name: String,
    pub note: String,
    pub size_label: String,
    pub url: String,
    pub filename: String,
    pub dim: u32,
}

/// The default (bge-small) plus one larger alternate (nomic-embed), same
/// shape as `imagegen::image_catalog`.
pub fn embed_catalog() -> Vec<EmbedCatalogEntry> {
    vec![
        EmbedCatalogEntry {
            name: "bge-small-en-v1.5".into(),
            note: "The default — small and quick. Plenty for recalling notes and searching a folder."
                .into(),
            size_label: "~130 MB".into(),
            url: "https://huggingface.co/CompendiumLabs/bge-small-en-v1.5-gguf/resolve/main/bge-small-en-v1.5-f16.gguf".into(),
            filename: "bge-small-en-v1.5-f16.gguf".into(),
            dim: 384,
        },
        EmbedCatalogEntry {
            name: "nomic-embed-text-v1.5".into(),
            note: "Larger and slower to load, but a little more accurate on long passages.".into(),
            size_label: "~275 MB".into(),
            url: "https://huggingface.co/nomic-ai/nomic-embed-text-v1.5-GGUF/resolve/main/nomic-embed-text-v1.5.f16.gguf".into(),
            filename: "nomic-embed-text-v1.5.f16.gguf".into(),
            dim: 768,
        },
    ]
}

/// Normalise to unit length in place (EMB-3). A zero vector (degenerate
/// input) is left as-is rather than dividing by zero.
pub fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error("embedding request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("embedding engine returned an unexpected response: {0}")]
    BadResponse(String),
}

/// Post `texts` to the running engine's `/v1/embeddings`, in batches of
/// [`EMBED_BATCH`], returning one unit-normalised vector per input, in the
/// same order (EMB-3).
///
/// `on_batch` runs after each batch completes. [`EmbedManager::embed_texts`]
/// uses it to keep the idle timer alive across a long indexing run — without
/// it, embedding a big folder in one call could outlive [`IDLE_STOP`] and have
/// the engine stopped from under it.
///
/// Every vector is validated on the way out: the response is matched to the
/// request by its `index` field, and a batch that leaves any slot unfilled or
/// disagrees with itself about dimension is an error rather than a silently
/// empty vector (which would dot-product to zero against everything).
pub async fn embed(
    client: &reqwest::Client,
    base_url: &str,
    token: &str,
    texts: &[String],
    on_batch: impl Fn(),
) -> Result<Vec<Vec<f32>>, EmbedError> {
    #[derive(serde::Deserialize)]
    struct EmbeddingRow {
        embedding: Vec<f32>,
        index: usize,
    }
    #[derive(serde::Deserialize)]
    struct EmbeddingResponse {
        data: Vec<EmbeddingRow>,
    }

    let mut out: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
    for (chunk_ix, chunk) in texts.chunks(EMBED_BATCH).enumerate() {
        let resp: EmbeddingResponse = client
            .post(format!("{base_url}/v1/embeddings"))
            .bearer_auth(token)
            .json(&serde_json::json!({ "input": chunk }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if resp.data.len() != chunk.len() {
            return Err(EmbedError::BadResponse(format!(
                "expected {} embeddings, got {}",
                chunk.len(),
                resp.data.len()
            )));
        }
        let offset = chunk_ix * EMBED_BATCH;
        for row in resp.data {
            if row.index >= chunk.len() {
                return Err(EmbedError::BadResponse(format!(
                    "embedding index {} is outside a batch of {}",
                    row.index,
                    chunk.len()
                )));
            }
            if row.embedding.is_empty() {
                return Err(EmbedError::BadResponse("an embedding came back empty".into()));
            }
            let mut v = row.embedding;
            normalize(&mut v);
            out[offset + row.index] = Some(v);
        }
        on_batch();
    }

    let mut vectors = Vec::with_capacity(out.len());
    for (i, slot) in out.into_iter().enumerate() {
        // Only reachable if the engine repeated an index and skipped another.
        let v = slot.ok_or_else(|| EmbedError::BadResponse(format!("no embedding for input {i}")))?;
        vectors.push(v);
    }
    if let Some(dim) = vectors.first().map(|v| v.len()) {
        if vectors.iter().any(|v| v.len() != dim) {
            return Err(EmbedError::BadResponse(
                "the engine returned vectors of differing dimension".into(),
            ));
        }
    }
    Ok(vectors)
}

/// Lifecycle for the embedding engine: lazy start, idle stop, one instance
/// for the whole app (EMB-2). Mirrors `RuntimeManager` but far simpler — no
/// watchdog/self-heal, since every caller already treats "unavailable" as a
/// normal, silently-degraded outcome (EMB-5) rather than a hard error.
pub struct EmbedManager {
    engine: Mutex<Option<RunningEngine>>,
    last_used: StdMutex<Instant>,
    /// Mirrors whether `engine` holds a live process. `ensure_started` keeps
    /// the engine lock for the whole cold start (up to a minute) so two
    /// callers can't race into two processes; this lets `status()` answer the
    /// Engine view during that window instead of blocking on it.
    running: AtomicBool,
}

impl EmbedManager {
    pub fn new() -> Self {
        Self {
            engine: Mutex::new(None),
            last_used: StdMutex::new(Instant::now()),
            running: AtomicBool::new(false),
        }
    }

    /// Never blocks: a start in flight reports from the `running` flag rather
    /// than waiting for the lock the cold start is holding.
    pub async fn status(&self) -> EngineStatus {
        match self.engine.try_lock() {
            Ok(guard) => EngineStatus::from_engine(guard.as_ref()),
            Err(_) => {
                let mut status = EngineStatus::from_engine(None);
                status.running = self.running.load(Ordering::Relaxed);
                status
            }
        }
    }

    /// Start the engine if it isn't already running against this exact
    /// model, and return its connection details. Any failure here means
    /// "recall engine unavailable" to the caller (EMB-5) — never a hard
    /// error surfaced to chat.
    pub async fn ensure_started(
        &self,
        client: &reqwest::Client,
        server_binary: PathBuf,
        model_path: PathBuf,
    ) -> Result<(String, String), EngineError> {
        self.touch();
        let mut guard = self.engine.lock().await;
        if let Some(e) = guard.as_mut() {
            if e.still_running() && e.model_path == model_path {
                return Ok((e.base_url(), e.token.clone()));
            }
            *guard = None; // stale or dead — fall through and respawn
            self.running.store(false, Ordering::Relaxed);
        }
        let config = EngineConfig {
            server_binary,
            model_path,
            ctx_size: EMBED_CTX_SIZE,
            n_gpu_layers: 0, // CPU-only, deliberate (EMB-1): never touches VRAM
            extra_args: embed_extra_args(),
        };
        let mut engine = spawn_engine(&config)?;
        wait_until_healthy(client, &mut engine, Duration::from_secs(60)).await?;
        let result = (engine.base_url(), engine.token.clone());
        *guard = Some(engine);
        self.running.store(true, Ordering::Relaxed);
        Ok(result)
    }

    /// Start the engine if needed and embed `texts` (EMB-3), keeping the idle
    /// timer alive for the whole run. **This is what callers should use** —
    /// the bare [`embed`] function doesn't know about the idle timer, so a
    /// long indexing pass driven through it can have the engine stopped
    /// mid-flight (EMB-2 promises the opposite).
    pub async fn embed_texts(
        &self,
        client: &reqwest::Client,
        server_binary: PathBuf,
        model_path: PathBuf,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, EmbedError> {
        let (base_url, token) = self.ensure_started(client, server_binary, model_path).await?;
        let result = embed(client, &base_url, &token, texts, || self.touch()).await;
        self.touch();
        result
    }

    pub async fn stop(&self) {
        *self.engine.lock().await = None;
        self.running.store(false, Ordering::Relaxed);
    }

    /// Reset the idle clock. Called around every use, not just at start, so
    /// work in progress always counts as activity.
    fn touch(&self) {
        if let Ok(mut last) = self.last_used.lock() {
            *last = Instant::now();
        }
    }

    fn idle_for(&self) -> Duration {
        self.last_used
            .lock()
            .map(|last| last.elapsed())
            .unwrap_or(Duration::ZERO)
    }
}

impl Default for EmbedManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Background idle-stop loop (EMB-2): checked every 30s for the app's
/// lifetime. There is only ever one embedding engine, so this needs none of
/// the chat watchdog's generation-fencing or self-heal — an idle engine is
/// just stopped, and the next request starts it again.
///
/// It also reaps a child that exited on its own, so `running` can't sit true
/// for a process that is already gone.
pub fn spawn_idle_stop(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let Some(mgr) = app.try_state::<EmbedManager>() else {
                return;
            };
            let idle = mgr.idle_for() >= IDLE_STOP;
            let mut guard = mgr.engine.lock().await;
            let died = guard.as_mut().map(|e| !e.still_running()).unwrap_or(false);
            let stopping = guard.is_some() && (idle || died);
            if stopping {
                *guard = None; // Drop kills the child and reaps it
                mgr.running.store(false, Ordering::Relaxed);
            }
            drop(guard);
            if stopping {
                if let Some(db) = app.try_state::<crate::db::Db>() {
                    let note = if died {
                        "the recall engine exited on its own"
                    } else {
                        "stopped the recall engine (idle)"
                    };
                    let _ = db.log_activity(None, "embed", note);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_scales_to_unit_length() {
        let mut v = vec![3.0_f32, 4.0];
        normalize(&mut v);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
        assert!((v[0] - 0.6).abs() < 1e-6);
        assert!((v[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn normalize_leaves_a_zero_vector_alone() {
        let mut v = vec![0.0_f32, 0.0];
        normalize(&mut v);
        assert_eq!(v, vec![0.0, 0.0]);
    }

    #[test]
    fn catalog_has_the_default_and_one_alternate() {
        let catalog = embed_catalog();
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog[0].name, "bge-small-en-v1.5");
        assert_eq!(catalog[0].dim, 384);
        assert_eq!(catalog[1].dim, 768);
    }
}
