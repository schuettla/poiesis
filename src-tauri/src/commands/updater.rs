//! Self-update (`UPD-4`): look for a newer signed build on GitHub Releases,
//! download it, and hand over to the installer.
//!
//! These are our own commands rather than the updater plugin's JS bindings
//! because the shutdown hook has to be set on the `UpdaterBuilder` that
//! performs the install, and the bindings build theirs inside the plugin with
//! no way in. On Windows the install ends with the installer launching and this
//! process exiting through `std::process::exit`, so that hook is the only
//! chance to stop the engines (`UPD-3`).

use std::sync::Mutex;

use serde::Serialize;
use tauri::{ipc::Channel, AppHandle, Manager, State};
use tauri_plugin_updater::{Error as UpdaterError, Update, UpdaterExt};

/// The update found by the last check, kept so the install step can run
/// without checking again. It isn't serialisable, so the UI only ever sees
/// [`UpdateMeta`].
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

#[derive(Debug, Clone, Serialize)]
pub struct UpdateMeta {
    pub version: String,
    /// The release notes, exactly as written on the GitHub release.
    pub notes: String,
}

/// Download progress. `contentLength` is `None` when the server doesn't say.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DownloadEvent {
    Progress {
        chunk_length: usize,
        content_length: Option<u64>,
    },
    /// All bytes are in and verified; the installer starts next.
    Finished,
}

/// The reason an update step failed, as a code the UI turns into words. A raw
/// error string never reaches the user (AUTOUPDATE_PLAN §5).
fn failure_code(e: &UpdaterError) -> &'static str {
    match e {
        // The download arrived but isn't ours: the one that must never be
        // waved through, and the one the negative test looks for.
        UpdaterError::Minisign(_) | UpdaterError::Base64(_) | UpdaterError::SignatureUtf8(_) => {
            "signature"
        }
        UpdaterError::Reqwest(_)
        | UpdaterError::Network(_)
        | UpdaterError::ReleaseNotFound
        | UpdaterError::TargetNotFound(_)
        | UpdaterError::TargetsNotFound(_) => "network",
        _ => "other",
    }
}

fn fail(e: UpdaterError) -> String {
    eprintln!("updater: {e}");
    failure_code(&e).to_string()
}

/// Ask GitHub whether a newer version exists. `None` means this is the newest.
/// Fails with `"network"`, `"signature"` or `"other"`.
#[tauri::command]
pub async fn update_check_cmd(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<Option<UpdateMeta>, String> {
    let hook_app = app.clone();
    let updater = app
        .updater_builder()
        // Replaces the plugin's own hook, so it has to do what that one does
        // (tear down windows and tray icons) as well as stop the engines.
        .on_before_exit(move || {
            hook_app.cleanup_before_exit();
            shutdown_off_runtime(&hook_app);
        })
        .build()
        .map_err(fail)?;
    let found = updater.check().await.map_err(fail)?;
    let meta = found.as_ref().map(|u| UpdateMeta {
        version: u.version.clone(),
        notes: u.body.clone().unwrap_or_default(),
    });
    *pending.0.lock().unwrap() = found;
    Ok(meta)
}

/// Download the update found by the last check, verify its signature against
/// the public key baked into this build, and install it. On Windows the
/// installer takes over and the app exits before this returns.
#[tauri::command]
pub async fn update_install_cmd(
    pending: State<'_, PendingUpdate>,
    on_event: Channel<DownloadEvent>,
) -> Result<(), String> {
    let update = pending
        .0
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "other".to_string())?;
    let progress = on_event.clone();
    update
        .download_and_install(
            move |chunk_length, content_length| {
                let _ = progress.send(DownloadEvent::Progress {
                    chunk_length,
                    content_length,
                });
            },
            move || {
                let _ = on_event.send(DownloadEvent::Finished);
            },
        )
        .await
        .map_err(fail)
}

/// Run [`crate::shutdown_engines`] on a thread of its own. The hook fires from
/// inside the async install, and `block_on` panics if it is entered from a
/// thread that is already running the async runtime.
fn shutdown_off_runtime(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::spawn(move || crate::shutdown_engines(&app)).join();
}

/// The running version, from `tauri.conf.json` — the same one the updater
/// compares against, so About can never disagree with it.
pub fn running_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

pub fn manage(app: &AppHandle) {
    app.manage(PendingUpdate::default());
}
