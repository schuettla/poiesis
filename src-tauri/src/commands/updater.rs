//! Self-update (`UPD-4`): look for a newer signed build on GitHub Releases,
//! download it, and hand over to the installer.
//!
//! These are our own commands rather than the updater plugin's JS bindings
//! because the shutdown hook has to be set on the `UpdaterBuilder` that
//! performs the install, and the bindings build theirs inside the plugin with
//! no way in. On Windows the install ends with the installer launching and this
//! process exiting through `std::process::exit`, so that hook is the only
//! chance to stop the engines (`UPD-3`).
//!
//! The portable Windows exe is the exception: nothing is installed, so it
//! downloads the new exe, checks its signature, and swaps its own file. The
//! new version starts on the next restart, as on macOS and Linux.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{ipc::Channel, AppHandle, Manager, State};
use tauri_plugin_updater::{Error as UpdaterError, Update, UpdaterExt};

/// The update-list entry the portable exe looks for. The release workflow adds
/// it next to the installer entries.
const PORTABLE_TARGET: &str = "windows-x86_64-portable";

/// The portable Windows exe: a release build the bundler never touched, so it
/// carries no installer type. The installed copy is marked NSIS or MSI.
fn is_portable() -> bool {
    cfg!(windows) && !cfg!(debug_assertions) && tauri::utils::platform::bundle_type().is_none()
}

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
    /// A deb or rpm install: the updater runs `dpkg`/`rpm` through `pkexec`,
    /// so the system asks for the user's password (CROSS_PLATFORM_PLAN XP-6).
    #[serde(rename = "asksPassword")]
    pub asks_password: bool,
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
    let mut builder = app.updater_builder();
    if is_portable() {
        builder = builder.target(PORTABLE_TARGET);
    }
    let updater = builder
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
        asks_password: matches!(
            tauri::utils::platform::bundle_type(),
            Some(tauri::utils::config::BundleType::Deb | tauri::utils::config::BundleType::Rpm)
        ),
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
    if is_portable() {
        // `download` checks the signature before handing the bytes over.
        let bytes = update
            .download(
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
            .map_err(fail)?;
        let exe = std::env::current_exe().map_err(|_| "other".to_string())?;
        return replace_exe(&exe, &bytes).map_err(|e| {
            eprintln!("updater: {e}");
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                "readonly".to_string()
            } else {
                "other".to_string()
            }
        });
    }
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

/// Put `new_exe` where `exe` is. Windows won't overwrite a running exe but will
/// rename it, so the running one moves aside to `<name>.exe.old` and the new one
/// takes its name. The new file is written first, so a read-only folder fails
/// before anything is touched; a failed swap puts the old exe back.
fn replace_exe(exe: &std::path::Path, new_exe: &[u8]) -> std::io::Result<()> {
    let new = exe.with_extension("exe.new");
    let old = exe.with_extension("exe.old");
    std::fs::write(&new, new_exe)?;
    let _ = std::fs::remove_file(&old);
    if let Err(e) = std::fs::rename(exe, &old) {
        let _ = std::fs::remove_file(&new);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&new, exe) {
        let _ = std::fs::rename(&old, exe);
        let _ = std::fs::remove_file(&new);
        return Err(e);
    }
    Ok(())
}

/// Delete the exe a portable update moved aside. The old process may still be
/// exiting right after a restart, so this waits a few seconds first.
fn remove_replaced_exe() {
    let Ok(exe) = std::env::current_exe() else { return };
    let old = exe.with_extension("exe.old");
    if !old.exists() {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(5));
        let _ = std::fs::remove_file(&old);
    });
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
    if is_portable() {
        remove_replaced_exe();
    }
}

#[cfg(test)]
mod tests {
    use super::replace_exe;

    #[test]
    fn replace_exe_swaps_the_file_and_keeps_the_old_one_aside() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("poiesis.exe");
        std::fs::write(&exe, b"old").unwrap();
        replace_exe(&exe, b"new").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert_eq!(std::fs::read(dir.path().join("poiesis.exe.old")).unwrap(), b"old");
        assert!(!dir.path().join("poiesis.exe.new").exists());
        // A second update replaces the leftover from the first.
        replace_exe(&exe, b"newer").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"newer");
        assert_eq!(std::fs::read(dir.path().join("poiesis.exe.old")).unwrap(), b"new");
    }

    #[test]
    fn replace_exe_leaves_everything_alone_when_the_exe_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("poiesis.exe");
        assert!(replace_exe(&exe, b"new").is_err());
        assert!(!dir.path().join("poiesis.exe.new").exists());
        assert!(!exe.exists());
    }
}
