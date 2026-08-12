//! `JOB-1` — media generation as a background job rather than a blocked turn.
//!
//! Before this, `generate_image` held the agent loop for up to 300s and video
//! for up to ten minutes. Both are rude, and the second is untenable: nothing
//! else in the conversation could happen while a clip rendered.
//!
//! A submit writes one `media_jobs` row and spawns a worker. The caller gets a
//! job id back immediately. When the worker finishes it records the artifact,
//! closes the row, and announces itself on an app-level event — deliberately
//! *not* through the agent-run channel, because by then the run that asked for
//! it has usually ended. The row carries `message_id` so the result still lands
//! in the turn that wanted it, live or after a reload.
//!
//! Cancellation is honoured at the boundaries a subprocess or an HTTP request
//! can actually be interrupted at: before the call starts, and between polls of
//! an async provider. A local `stable-diffusion.cpp` run already in flight, or
//! a single blocking image POST, finishes its work and is discarded — the user
//! sees the turn end at once either way, which is the part that matters.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Emitter, Manager};

use crate::db::{Db, MediaJob};
use crate::runtime::manager::RuntimeManager;
use crate::runtime::proxy::CancelFlag;

use super::{MediaRequest, Modality, Registry};

/// The app handle, set once at startup. A worker outlives the command that
/// spawned it, so it can't borrow state from one — and threading a handle
/// through `run_agent`'s twenty parameters into every `ToolContext` to reach
/// one toolset would be a worse trade than this.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Cancel flags for jobs currently in flight, by job id.
static LIVE: OnceLock<Mutex<HashMap<String, CancelFlag>>> = OnceLock::new();

fn live() -> &'static Mutex<HashMap<String, CancelFlag>> {
    LIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Called once from `lib.rs` setup. Also sweeps jobs orphaned by a restart —
/// doing it here rather than at first submit means a placeholder left over
/// from a crash is corrected on launch, not whenever the user next generates.
pub fn init(app: AppHandle) {
    let _ = APP.set(app.clone());
    let db = app.state::<Db>();
    match db.fail_interrupted_media_jobs() {
        Ok(n) if n > 0 => eprintln!("media: marked {n} interrupted job(s) as failed"),
        _ => {}
    }
}

/// What the frontend receives when a job changes state.
#[derive(Debug, Clone, serde::Serialize)]
pub struct JobEvent {
    pub job_id: String,
    pub conversation_id: Option<String>,
    pub message_id: Option<String>,
    pub status: String,
    /// Present on `done` — the whole artifact, so the stream can render the
    /// media block without a second round trip.
    pub artifact: Option<crate::db::Artifact>,
    pub error: Option<String>,
}

fn announce(event: JobEvent) {
    if let Some(app) = APP.get() {
        let _ = app.emit("poiesis-media-job", event);
    }
}

/// `STR-4`: a partial image, mid-generation. Deliberately fire-and-forget —
/// a dropped partial costs nothing, and the final result arrives on the job
/// event regardless of whether any of these ever landed.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PartialEvent {
    pub job_id: String,
    /// A complete `data:` URI, so the UI can drop it straight into an `<img>`.
    pub data_uri: String,
}

pub fn partial(job_id: &str, data_uri: String) {
    if let Some(app) = APP.get() {
        let _ = app.emit(
            "poiesis-media-partial",
            PartialEvent { job_id: job_id.to_string(), data_uri },
        );
    }
}

/// Everything a worker needs that isn't reachable from the `AppHandle`.
pub struct SubmitArgs {
    pub conversation_id: Option<String>,
    pub message_id: Option<String>,
    pub modality: Modality,
    /// `media:<backend>/<slug>` when the user declared a model (Path E), or
    /// `None` to let `resolve_backend` walk the precedence chain.
    pub model_id: Option<String>,
    pub request: MediaRequest,
    pub parent_artifact_id: Option<String>,
}

/// Write the row, start the worker, return immediately.
///
/// Returns an error only when the job could not be *started* — a bad model id
/// or no usable backend. Anything that goes wrong after that is reported
/// through the job's own completion event, because by then the caller has
/// already been told the work began.
pub fn submit(db: &Db, args: SubmitArgs) -> Result<MediaJob, String> {
    // Resolve up front: "no image backend is set up" is an answer the caller
    // should get synchronously, not thirty seconds later as a failed job.
    {
        let registry = Registry::new();
        match &args.model_id {
            Some(id) => {
                super::resolve_backend_for_model(&registry, id)?;
            }
            None => {
                super::resolve_backend(&registry, db, args.modality)?;
            }
        }
    }

    let job = db
        .add_media_job(
            args.conversation_id.as_deref(),
            args.message_id.as_deref(),
            args.modality.as_kind(),
            &args.request.prompt,
            args.model_id.as_deref(),
            args.request.aspect_ratio.as_deref(),
        )
        .map_err(|e| format!("couldn't record the generation job: {e}"))?;

    let cancel = CancelFlag::new();
    live().lock().unwrap().insert(job.id.clone(), cancel.clone());

    let job_id = job.id.clone();
    tauri::async_runtime::spawn(async move {
        run_job(job_id, args, cancel).await;
    });

    Ok(job)
}

/// Ask a running job to stop. Returns false if it already finished — the UI
/// treats that as "you were too late", not as an error.
pub fn cancel(db: &Db, job_id: &str) -> bool {
    let flag = live().lock().unwrap().get(job_id).cloned();
    let Some(flag) = flag else { return false };
    flag.cancel();
    // Close the row now rather than waiting for the worker to notice: the user
    // asked for this turn to be over, and a worker mid-HTTP-call may take a
    // while to reach its next cancellation point.
    let closed = db.finish_media_job(job_id, "cancelled", None, None).unwrap_or(false);
    if closed {
        let job = db.get_media_job(job_id).ok().flatten();
        announce(JobEvent {
            job_id: job_id.to_string(),
            conversation_id: job.as_ref().and_then(|j| j.conversation_id.clone()),
            message_id: job.as_ref().and_then(|j| j.message_id.clone()),
            status: "cancelled".to_string(),
            artifact: None,
            error: None,
        });
    }
    closed
}

async fn run_job(job_id: String, mut args: SubmitArgs, cancel: CancelFlag) {
    // `STR-4`: a backend can only report progress if it knows where to send it.
    args.request.job_id = Some(job_id.clone());
    let outcome = generate(&args, &cancel).await;
    live().lock().unwrap().remove(&job_id);

    let Some(app) = APP.get() else { return };
    let db = app.state::<Db>();

    // A cancel already closed the row and told the UI; whatever the worker
    // produced in the meantime is discarded rather than resurrecting a turn
    // the user ended.
    if cancel.is_cancelled() {
        if let Ok(Some(res)) = outcome {
            let _ = std::fs::remove_file(&res.path);
        }
        return;
    }

    match outcome {
        Ok(Some(result)) => {
            let artifact = super::record(
                &db,
                args.conversation_id.as_deref(),
                &args.request,
                &result,
                args.parent_artifact_id.as_deref(),
                args.modality,
                args.message_id.as_deref(),
            );
            match artifact {
                Ok(artifact) => {
                    // The live event paints it into the open transcript; the
                    // attachment row is what makes it still be there, with its
                    // actions, after a restart (`ART-2`). Both, or the picture
                    // is either invisible now or gone later.
                    if let Some(message_id) = args.message_id.as_deref() {
                        let name = std::path::Path::new(&artifact.content)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("image.png");
                        let _ = db.add_attachment(
                            message_id,
                            args.modality.as_kind(),
                            name,
                            &artifact.content,
                            Some(&artifact.id),
                        );
                    }
                    let _ = db.log_activity(
                        args.conversation_id.as_deref(),
                        args.modality.as_kind(),
                        &format!("generated: {}", artifact.title),
                    );
                    let ok = db
                        .finish_media_job(&job_id, "done", Some(&artifact.id), None)
                        .unwrap_or(false);
                    if ok {
                        announce(JobEvent {
                            job_id,
                            conversation_id: args.conversation_id.clone(),
                            message_id: args.message_id.clone(),
                            status: "done".to_string(),
                            artifact: Some(artifact),
                            error: None,
                        });
                    }
                }
                Err(e) => fail(&db, job_id, &args, format!("couldn't save the artifact: {e}")),
            }
        }
        Ok(None) => {} // cancelled before the work started; nothing to report
        // A backend that noticed the flag itself reports the sentinel rather
        // than a message — that is a stop, not a failure, and must never reach
        // the user as an error.
        Err(e) if e == super::CANCELLED => {}
        Err(e) => fail(&db, job_id, &args, e),
    }
}

fn fail(db: &Db, job_id: String, args: &SubmitArgs, error: String) {
    if db.finish_media_job(&job_id, "failed", None, Some(&error)).unwrap_or(false) {
        announce(JobEvent {
            job_id,
            conversation_id: args.conversation_id.clone(),
            message_id: args.message_id.clone(),
            status: "failed".to_string(),
            artifact: None,
            error: Some(error),
        });
    }
}

/// `Ok(None)` means "cancelled before the work started".
async fn generate(args: &SubmitArgs, cancel: &CancelFlag) -> Result<Option<super::MediaResult>, String> {
    let Some(app) = APP.get() else {
        return Err("the app isn't ready".to_string());
    };
    if cancel.is_cancelled() {
        return Ok(None);
    }

    let db = app.state::<Db>();
    let mgr = app.state::<RuntimeManager>();
    let out_dir = mgr.generated_media_dir();

    let registry = Registry::new();
    let mut request = args.request.clone();
    let backend = match &args.model_id {
        Some(id) => {
            let (backend, slug) = super::resolve_backend_for_model(&registry, id)?;
            request.model = Some(slug);
            backend
        }
        None => super::resolve_backend(&registry, &db, args.modality)?,
    };
    request.modality = Some(args.modality);

    if !backend.descriptor().modalities.contains(&args.modality) {
        return Err(format!(
            "{} can't make {}s.",
            backend.descriptor().label,
            args.modality.as_kind()
        ));
    }
    if !request.references.is_empty() && !backend.descriptor().supports_references {
        return Err(format!(
            "{} can't edit images — pick a cloud image model to refine this one.",
            backend.descriptor().label
        ));
    }

    backend.generate(&db, &request, &out_dir, cancel).await.map(Some)
}

#[cfg(test)]
mod tests {
    use crate::db::Db;

    /// The restart-safety half of `JOB-1`, which is the part a user actually
    /// notices: a job whose worker died with the process must not come back as
    /// a placeholder that spins forever.
    #[test]
    fn a_job_interrupted_by_a_restart_is_failed_not_left_running() {
        let db = Db::open_in_memory().unwrap();
        let conv = db.create_conversation("test", None, false).unwrap();
        let job = db
            .add_media_job(Some(&conv.id), None, "video", "a fox in snow", None, Some("16:9"))
            .unwrap();
        assert_eq!(job.status, "running");
        assert_eq!(db.list_running_media_jobs(&conv.id).unwrap().len(), 1);

        assert_eq!(db.fail_interrupted_media_jobs().unwrap(), 1);

        let after = db.get_media_job(&job.id).unwrap().unwrap();
        assert_eq!(after.status, "failed");
        assert_eq!(after.error.as_deref(), Some("interrupted by restart"));
        assert!(after.finished_at.is_some());
        assert!(db.list_running_media_jobs(&conv.id).unwrap().is_empty());
    }

    /// A cancel arriving at the same moment as a completion must not produce
    /// two outcomes — whichever lands first is the one that sticks.
    #[test]
    fn a_finished_job_cannot_be_finished_twice() {
        let db = Db::open_in_memory().unwrap();
        let job = db.add_media_job(None, None, "image", "a fox", None, None).unwrap();

        assert!(db.finish_media_job(&job.id, "cancelled", None, None).unwrap());
        assert!(!db.finish_media_job(&job.id, "done", Some("art-1"), None).unwrap());

        let after = db.get_media_job(&job.id).unwrap().unwrap();
        assert_eq!(after.status, "cancelled");
        assert_eq!(after.artifact_id, None);
    }
}
