//! Media models as a picker category (`PIK-1`), and the declared-route
//! generation command (`PIK-2`). One command each, because the registry
//! already knows every backend it has credentials for — adding a provider
//! must never mean touching this file.

use std::path::Path;

use tauri::State;

use crate::commands::files::{assert_ui_readable_raw, DialogGrants};
use crate::db::{Db, MediaJob, MediaSpend};
use crate::media::{self, MediaModel, MediaRef, Modality, Registry};
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

/// Every image/video model the user can reach right now, across all backends
/// whose credential is satisfied. Empty on a fresh install with no engine and
/// no key — the picker then omits the group entirely rather than showing an
/// empty heading.
#[tauri::command]
pub async fn list_media_models_cmd(db: State<'_, Db>, modality: Option<String>) -> Cmd<Vec<MediaModel>> {
    let want = match modality.as_deref() {
        Some("image") => Some(Modality::Image),
        Some("video") => Some(Modality::Video),
        _ => None,
    };
    let registry = Registry::new();
    Ok(registry.all_models(&db, want).await)
}

/// The declared route (`PIK-2` / Path E): the user picked an exact model in
/// the chooser, so generation goes straight to that backend rather than
/// through availability precedence. Also the entry point for references
/// (`EDT-1`) and implicit refinement (`EDT-2`) — both just pass `references`.
///
/// Returns as soon as the job is *accepted* (`JOB-1`), not when the picture is
/// ready. Everything that can be checked up front — an empty prompt, too many
/// references, a backend that can't do this modality — is still checked here,
/// so a mistake is a synchronous error rather than a job that fails a minute
/// later. `message_id` is the assistant turn the result belongs to.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn generate_media_cmd(
    db: State<'_, Db>,
    grants: State<'_, DialogGrants>,
    conversation_id: Option<String>,
    message_id: Option<String>,
    model_id: String,
    modality: String,
    prompt: String,
    aspect_ratio: Option<String>,
    resolution: Option<String>,
    seed: Option<i64>,
    steps: Option<i64>,
    negative: Option<String>,
    duration_secs: Option<u32>,
    references: Option<Vec<String>>,
    parent_artifact_id: Option<String>,
) -> Cmd<MediaJob> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err(PoiesisError::Message("Describe what to make.".into()));
    }
    // The picker already tags every model with its modality — trusting that
    // instead of guessing from the backend descriptor is what keeps a video
    // request from silently landing on the image endpoint of a backend (like
    // OpenRouter) that serves both.
    let modality = match modality.as_str() {
        "video" => Modality::Video,
        _ => Modality::Image,
    };

    let refs = references.unwrap_or_default();
    if refs.len() > 8 {
        return Err(PoiesisError::Message("Up to 8 reference images are supported.".into()));
    }
    let mut media_refs = Vec::with_capacity(refs.len());
    for path in refs {
        let p = Path::new(&path);
        assert_ui_readable_raw(&db, &grants, conversation_id.as_deref(), p, Some(&path))?;
        media_refs.push(MediaRef { path: p.to_path_buf(), role: media::RefRole::Source });
    }

    let registry = Registry::new();
    let (backend, slug) = media::resolve_backend_for_model(&registry, &model_id).map_err(PoiesisError::Message)?;
    if !backend.descriptor().modalities.contains(&modality) {
        return Err(PoiesisError::Message(format!("{} doesn't offer {}.", backend.descriptor().label, modality.as_kind())));
    }
    if !media_refs.is_empty() && !backend.descriptor().supports_references {
        return Err(PoiesisError::Message(format!(
            "{} can't take reference images — pick a cloud image model to refine this one.",
            backend.descriptor().label
        )));
    }

    // `PIK-4`'s advanced knobs ride through untouched — a backend that can't
    // honour one reports it in `MediaResult::ignored` rather than failing.
    let req = media::MediaRequest {
        model: Some(slug),
        modality: Some(modality),
        prompt: prompt.to_string(),
        negative: negative.filter(|n| !n.trim().is_empty()),
        aspect_ratio,
        resolution,
        seed,
        steps,
        duration_secs,
        references: media_refs,
        ..Default::default()
    };

    media::jobs::submit(
        &db,
        media::jobs::SubmitArgs {
            conversation_id,
            message_id,
            modality,
            model_id: Some(model_id),
            request: req,
            parent_artifact_id,
        },
    )
    .map_err(PoiesisError::Message)
}

/// `CST-2`: what media has cost this calendar month, and all time. Derived
/// from the artifacts themselves, so it can't drift from what was actually
/// made. No enforcement — this phase shows the number and stops there.
#[tauri::command]
pub fn media_spend_cmd(db: State<'_, Db>) -> Cmd<MediaSpendReport> {
    Ok(MediaSpendReport {
        month: db.media_spend(start_of_month_ms()).map_err(err)?,
        all_time: db.media_spend(0).map_err(err)?,
    })
}

#[derive(serde::Serialize)]
pub struct MediaSpendReport {
    pub month: MediaSpend,
    pub all_time: MediaSpend,
}

/// Midnight on the 1st, local time. Done by hand rather than with a date crate
/// because this is the only place in the app that needs a calendar boundary,
/// and `chrono` is not currently a dependency.
fn start_of_month_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let days = now / 86_400;
    // Civil-from-days (Howard Hinnant's algorithm), shifted to a March-based
    // year so leap days land at the end and need no special case.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    // Days elapsed since the 1st is all we actually need.
    let _ = (era, yoe, mp);
    ((days - (d - 1)) * 86_400) * 1_000
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same civil-from-days maths as `start_of_month_ms`, but for an arbitrary
    /// day rather than today — the only way to check a calendar boundary
    /// without waiting for one.
    fn day_of_month(days_since_epoch: i64) -> i64 {
        let z = days_since_epoch + 719_468;
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        doy - (153 * mp + 2) / 5 + 1
    }

    #[test]
    fn day_of_month_handles_leap_days_and_month_ends() {
        // 1970-01-01 is day 0.
        assert_eq!(day_of_month(0), 1);
        assert_eq!(day_of_month(30), 31); // 1970-01-31
        assert_eq!(day_of_month(31), 1); // 1970-02-01
        // 2024-02-29 — the case a naive 30-day assumption gets wrong.
        assert_eq!(day_of_month(19_782), 29);
        assert_eq!(day_of_month(19_783), 1); // 2024-03-01
    }

    #[test]
    fn the_month_window_starts_at_or_before_now_and_within_31_days() {
        let start = start_of_month_ms();
        let now = crate::db::now_ms();
        assert!(start <= now);
        assert!(now - start < 31 * 86_400_000);
        // Midnight exactly — a window that started mid-day would silently drop
        // whatever was generated earlier on the 1st.
        assert_eq!(start % 86_400_000, 0);
    }

    #[test]
    fn spend_sums_cost_and_counts_by_kind_ignoring_non_media() {
        let db = Db::open_in_memory().unwrap();
        let with_cost = |cost: f64| format!(r#"{{"cost_usd":{cost}}}"#);
        db.add_artifact_with(None, "a", "image", "/a.png", Some(&with_cost(0.04)), None).unwrap();
        db.add_artifact_with(None, "b", "image", "/b.png", Some(&with_cost(0.06)), None).unwrap();
        // A local generation records no cost at all — it must count as an
        // image without adding to the money.
        db.add_artifact_with(None, "c", "image", "/c.png", Some(r#"{"seed":1}"#), None).unwrap();
        db.add_artifact_with(None, "d", "video", "/d.mp4", Some(&with_cost(0.25)), None).unwrap();
        db.add_artifact_with(None, "e", "markdown", "notes", None, None).unwrap();

        let spend = db.media_spend(0).unwrap();
        assert_eq!(spend.images, 3);
        assert_eq!(spend.videos, 1);
        assert!((spend.usd - 0.35).abs() < 1e-9);
    }
}

/// Stop a generation the user no longer wants. `false` means it had already
/// finished — the UI treats that as "too late", not as a failure.
#[tauri::command]
pub fn cancel_media_job_cmd(db: State<'_, Db>, job_id: String) -> Cmd<bool> {
    Ok(media::jobs::cancel(&db, &job_id))
}

/// Jobs still running for a conversation, so a reload re-attaches to a
/// generation in flight instead of showing a turn that looks abandoned.
#[tauri::command]
pub fn list_running_media_jobs_cmd(db: State<'_, Db>, conversation_id: String) -> Cmd<Vec<MediaJob>> {
    db.list_running_media_jobs(&conversation_id).map_err(err)
}
