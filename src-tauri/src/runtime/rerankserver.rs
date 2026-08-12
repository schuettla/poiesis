//! The reranking engine (Perception, `RRK`): a **third** `llama-server`
//! process, CPU-only, dedicated to cross-encoder scoring. `EMB`'s embedder is
//! a bi-encoder — query and chunk are scored independently, which is fast but
//! blind to whether a passage actually *answers* the query, not just shares
//! its topic. A cross-encoder reads the pair together and is far more
//! accurate, at the cost of one model pass per candidate — too slow to run on
//! every result, which is why `retrieval.rs` only calls this when the
//! embedding-only ranking is already in doubt (`RRK-4`).
//!
//! Lifecycle mirrors `embedserver.rs` exactly (lazy start, idle stop, one
//! instance for the app) but this cannot share the embedder's process: one
//! model per `llama-server`, and the two are launched with different flags.
//!
//! `RRK-1` — **which path was taken, verified against the pinned build itself**
//! (see `PINNED_BUILD_TAG` in `manifest.rs`), not assumed:
//!
//! * The native `/rerank` endpoint exists at that tag, so no manifest bump and
//!   no embedding-endpoint workaround: this reuses the same shared binary the
//!   chat and embedding engines already download.
//! * The flag is `--reranking` (alias `--rerank`). `rerank_extra_args` passes it
//!   alone, unlike `embedserver`'s `--embeddings --pooling mean`: earlier builds
//!   rejected the two together outright (`common/arg.cpp`: "either --embedding
//!   or --reranking can be specified, but not both"), and while the pinned build
//!   no longer refuses, a reranker has no business claiming the embedding path.
//! * `relevance_score` is the model's **raw classifier logit**, not a
//!   normalised score: `format_response_rerank` in `examples/server/utils.hpp`
//!   emits `json_value(rank, "score", 0.0)` with no transform, and bge-style
//!   rerankers routinely score non-matches negative. It is squashed through a
//!   sigmoid here — see `rerank` — which is the same normalisation `RRK-1`
//!   describes for the fallback path, so the two remain comparable in scale.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};

use tauri::Manager;
use tokio::sync::Mutex;

use super::process::{spawn_engine, wait_until_healthy, EngineConfig, EngineError, EngineStatus, RunningEngine};

/// Idle timeout before the engine is stopped to free RAM (`RRK-2`, mirrors
/// `EMB-2`).
pub const IDLE_STOP: Duration = Duration::from_secs(5 * 60);
/// A query plus one candidate passage, comfortably under this even at
/// `retrieval.rs`'s `EXCERPT_CAP`.
const RERANK_CTX_SIZE: u32 = 2048;

fn rerank_extra_args() -> Vec<String> {
    vec!["--reranking".into()]
}

/// A curated reranker model (`RRK-3`). Same `note` discipline as
/// `EmbedCatalogEntry` (`SMP-8a`): no *embedding*, *vector*, *reranker*,
/// *bi-encoder* or *cross-encoder* in user-facing copy.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RerankCatalogEntry {
    pub name: String,
    pub note: String,
    pub size_label: String,
    pub url: String,
    pub filename: String,
}

/// The default (bge-reranker-base) plus one larger, more accurate alternate
/// (bge-reranker-v2-m3), same shape as `embedserver::embed_catalog`.
///
/// `BAAI` publishes these as PyTorch weights only, so — exactly as with the
/// embedding catalog — both entries point at a community GGUF conversion of
/// the upstream model. Both URLs and both byte sizes were checked against the
/// Hugging Face API rather than inferred from the repo naming pattern; the
/// sizes below are the real `F16` blobs (`RRK-3`'s "~280 MB / ~600 MB" was
/// written for quantised weights, but full precision is the same policy the
/// recall models follow, and reranking is the one pass where precision
/// decides the ordering).
pub fn rerank_catalog() -> Vec<RerankCatalogEntry> {
    vec![
        RerankCatalogEntry {
            name: "bge-reranker-base".into(),
            note: "The default — re-reads the closest matches before answering, without much extra wait."
                .into(),
            size_label: "~540 MB".into(),
            url: "https://huggingface.co/sinjab/bge-reranker-base-F16-GGUF/resolve/main/bge-reranker-base-F16.gguf"
                .into(),
            filename: "bge-reranker-base-F16.gguf".into(),
        },
        RerankCatalogEntry {
            name: "bge-reranker-v2-m3".into(),
            note: "Slower to load and to run, but noticeably better at telling close matches apart.".into(),
            size_label: "~1.1 GB".into(),
            url: "https://huggingface.co/gpustack/bge-reranker-v2-m3-GGUF/resolve/main/bge-reranker-v2-m3-FP16.gguf"
                .into(),
            filename: "bge-reranker-v2-m3-FP16.gguf".into(),
        },
    ]
}

#[derive(Debug, thiserror::Error)]
pub enum RerankError {
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error("rerank request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("rerank engine returned an unexpected response: {0}")]
    BadResponse(String),
}

/// Monotonic squash of a raw cross-encoder logit into `(0, 1)`.
///
/// Monotonic is the load-bearing word: it cannot reorder the candidates, it
/// only puts them on a bounded scale so a reranked score can sit in the same
/// `Scored.relevance` field as an embedding score. Clamping instead would be
/// destructive — a batch where every candidate scores negative (exactly the
/// ambiguous case `RRK-4` sends here) would collapse to a single value and
/// throw the ranking away.
fn squash(logit: f32) -> f32 {
    1.0 / (1.0 + (-logit).exp())
}

/// Post one query against `documents` to the running engine's `/rerank`,
/// returning one relevance score per input, in the same order (`RRK-1`).
///
/// `relevance_score` arrives as a raw, unbounded logit (see the module doc),
/// so each is normalised through `squash` before it leaves this function —
/// callers never see the raw scale.
pub async fn rerank(
    client: &reqwest::Client,
    base_url: &str,
    token: &str,
    query: &str,
    documents: &[String],
) -> Result<Vec<f32>, RerankError> {
    #[derive(serde::Deserialize)]
    struct RerankRow {
        index: usize,
        relevance_score: f32,
    }
    #[derive(serde::Deserialize)]
    struct RerankResponse {
        results: Vec<RerankRow>,
    }

    if documents.is_empty() {
        return Ok(Vec::new());
    }

    let resp: RerankResponse = client
        .post(format!("{base_url}/rerank"))
        .bearer_auth(token)
        .json(&serde_json::json!({ "query": query, "documents": documents }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut out: Vec<Option<f32>> = vec![None; documents.len()];
    for row in resp.results {
        if row.index >= documents.len() {
            return Err(RerankError::BadResponse(format!(
                "rerank index {} is outside a batch of {}",
                row.index,
                documents.len()
            )));
        }
        out[row.index] = Some(squash(row.relevance_score));
    }
    out.into_iter()
        .enumerate()
        .map(|(i, slot)| slot.ok_or_else(|| RerankError::BadResponse(format!("no score for document {i}"))))
        .collect()
}

/// Lifecycle for the reranking engine: lazy start, idle stop, one instance for
/// the whole app (`RRK-2`). Structurally identical to `EmbedManager` — kept as
/// its own type rather than a generic because the two are launched with
/// different flags and can never share a process.
pub struct RerankManager {
    engine: Mutex<Option<RunningEngine>>,
    last_used: StdMutex<Instant>,
    running: AtomicBool,
}

impl RerankManager {
    pub fn new() -> Self {
        Self {
            engine: Mutex::new(None),
            last_used: StdMutex::new(Instant::now()),
            running: AtomicBool::new(false),
        }
    }

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

    /// Start the engine if it isn't already running against this exact model.
    /// Any failure here means "reranking unavailable" to the caller (`RRK-5`)
    /// — never a hard error surfaced to a search.
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
            *guard = None;
            self.running.store(false, Ordering::Relaxed);
        }
        let config = EngineConfig {
            server_binary,
            model_path,
            ctx_size: RERANK_CTX_SIZE,
            n_gpu_layers: 0, // CPU-only, deliberate (RRK-2): never competes with the chat model for VRAM
            extra_args: rerank_extra_args(),
        };
        let mut engine = spawn_engine(&config)?;
        wait_until_healthy(client, &mut engine, Duration::from_secs(60)).await?;
        let result = (engine.base_url(), engine.token.clone());
        *guard = Some(engine);
        self.running.store(true, Ordering::Relaxed);
        Ok(result)
    }

    /// Start the engine if needed and score `documents` against `query`,
    /// keeping the idle timer alive around the call. **This is what callers
    /// should use** — same reasoning as `EmbedManager::embed_texts`.
    pub async fn rerank_documents(
        &self,
        client: &reqwest::Client,
        server_binary: PathBuf,
        model_path: PathBuf,
        query: &str,
        documents: &[String],
    ) -> Result<Vec<f32>, RerankError> {
        let (base_url, token) = self.ensure_started(client, server_binary, model_path).await?;
        let result = rerank(client, &base_url, &token, query, documents).await;
        self.touch();
        result
    }

    pub async fn stop(&self) {
        *self.engine.lock().await = None;
        self.running.store(false, Ordering::Relaxed);
    }

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

impl Default for RerankManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Background idle-stop loop (`RRK-2`), identical in shape to
/// `embedserver::spawn_idle_stop`.
pub fn spawn_idle_stop(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let Some(mgr) = app.try_state::<RerankManager>() else {
                return;
            };
            let idle = mgr.idle_for() >= IDLE_STOP;
            let mut guard = mgr.engine.lock().await;
            let died = guard.as_mut().map(|e| !e.still_running()).unwrap_or(false);
            let stopping = guard.is_some() && (idle || died);
            if stopping {
                *guard = None;
                mgr.running.store(false, Ordering::Relaxed);
            }
            drop(guard);
            if stopping {
                if let Some(db) = app.try_state::<crate::db::Db>() {
                    let note = if died {
                        "the re-read engine exited on its own"
                    } else {
                        "stopped the re-read engine (idle)"
                    };
                    let _ = db.log_activity(None, "rerank", note);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_the_default_and_one_alternate() {
        let catalog = rerank_catalog();
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog[0].name, "bge-reranker-base");
        assert_eq!(catalog[1].name, "bge-reranker-v2-m3");
    }

    /// The download lands at `models/rerank/<filename>`, so a `filename` that
    /// disagrees with the URL's last segment would re-download every time the
    /// existence check runs.
    #[test]
    fn every_catalog_url_ends_in_its_own_filename() {
        for entry in rerank_catalog() {
            assert!(
                entry.url.ends_with(&entry.filename),
                "{} points at {} but claims the file is {}",
                entry.name,
                entry.url,
                entry.filename
            );
        }
    }

    /// The property the whole rerank pass rests on: normalising must not be
    /// able to reorder candidates, and negative logits — the common case for a
    /// weak batch — must stay distinct rather than collapsing to one value.
    #[test]
    fn squashing_preserves_order_and_keeps_negatives_apart() {
        let logits = [-11.0_f32, -4.2, -0.5, 0.0, 2.7, 9.4];
        let scored: Vec<f32> = logits.iter().map(|&l| squash(l)).collect();
        for pair in scored.windows(2) {
            assert!(pair[0] < pair[1], "squash must be strictly increasing: {pair:?}");
        }
        assert!(scored.iter().all(|s| *s > 0.0 && *s < 1.0), "scores must land inside (0,1)");
        assert!(
            scored[0] < scored[1] && scored[1] < scored[2],
            "an all-negative batch must still rank, not tie at the floor"
        );
    }
}
