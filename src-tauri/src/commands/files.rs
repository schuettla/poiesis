//! Working-folder commands backing the Workbench panel.
//!
//! These serve the *user's* browsing, not the agent's tool calls, but they share
//! the same scope rules: the UI may only reach into the conversation's attached
//! folder, a persisted grant, or somewhere the user just picked in a native
//! dialog. Without that, the panel would be a hole around the consent system it
//! sits next to.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::State;

use crate::agent::filesystem::IGNORED_DIRS;
use crate::agent::trash;
use crate::db::{Db, TrashEntry};
use crate::permissions::{canonicalize_lenient, path_within_root, refuse_as_working_folder};
use crate::NexusError;

type Cmd<T> = Result<T, NexusError>;

fn err<E: std::fmt::Display>(e: E) -> NexusError {
    NexusError::Message(e.to_string())
}

/// Paths the user chose in a native dialog this session. A dialog *is* consent —
/// but only for what was actually picked, so we remember rather than trust any
/// path the frontend hands back.
#[derive(Default)]
pub struct DialogGrants(Mutex<Vec<PathBuf>>);

impl DialogGrants {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn remember(&self, path: &Path) {
        let mut g = self.0.lock().unwrap();
        let canon = canonicalize_lenient(path);
        if !g.contains(&canon) {
            g.push(canon);
        }
    }
    /// Everything Poiesis itself produced — generated images, exports — is
    /// readable without asking; the user never picked it, but they did ask for it.
    pub fn allow_app_data(&self, dir: &Path) {
        self.remember(dir);
    }
    fn covers(&self, path: &Path) -> bool {
        self.0
            .lock()
            .unwrap()
            .iter()
            .any(|root| path == root || path_within_root(path, root))
    }
}

/// May the UI read this path? Allowed inside the conversation's working folder,
/// inside any persisted grant, or under something picked from a dialog.
pub fn assert_ui_readable(
    db: &Db,
    grants: &DialogGrants,
    conversation_id: Option<&str>,
    path: &Path,
) -> Result<(), NexusError> {
    assert_ui_readable_raw(db, grants, conversation_id, path, None)
}

/// As above, but also checks `raw` — the path exactly as the caller gave it —
/// against stored attachments, since rows written before paths were canonicalised
/// hold the picker's original spelling.
pub fn assert_ui_readable_raw(
    db: &Db,
    grants: &DialogGrants,
    conversation_id: Option<&str>,
    path: &Path,
    raw: Option<&str>,
) -> Result<(), NexusError> {
    if let Some(cid) = conversation_id {
        if let Ok((Some(folder), _)) = db.conversation_folder(cid) {
            if path_within_root(path, Path::new(&folder)) {
                return Ok(());
            }
        }
    }
    if let Ok(list) = db.list_permissions() {
        if list.iter().any(|g| path_within_root(path, Path::new(&g.path))) {
            return Ok(());
        }
    }
    if grants.covers(path) {
        return Ok(());
    }
    // A file the user once attached to a message stays readable across restarts —
    // otherwise reopening an old chat couldn't show its own images.
    let canonical = path.to_string_lossy().to_string();
    for candidate in [Some(canonical.as_str()), raw].into_iter().flatten() {
        if db.is_known_attachment(candidate).unwrap_or(false) {
            return Ok(());
        }
    }
    Err(NexusError::Message(format!(
        "{} isn't in a folder Poiesis has access to.",
        path.display()
    )))
}

/// One row in the Workbench tree.
#[derive(Debug, Clone, Serialize)]
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: i64,
}

// ---- attaching a folder ----

/// Open the folder picker and validate the choice. Returns `None` if cancelled.
#[tauri::command]
pub async fn pick_folder_cmd(
    app: tauri::AppHandle,
    grants: State<'_, DialogGrants>,
) -> Cmd<Option<String>> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(picked) = rx.await.map_err(err)? else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|e| NexusError::Message(format!("that folder can't be used: {e}")))?;

    let app_data = app_data_dir(&app);
    if let Some(reason) = refuse_as_working_folder(&path, app_data.as_deref()) {
        return Err(NexusError::Message(format!(
            "Poiesis can't work in {} — {reason}.",
            path.display()
        )));
    }
    let canon = canonicalize_lenient(&path);
    grants.remember(&canon);
    Ok(Some(canon.to_string_lossy().to_string()))
}

/// Pick files to attach to a message. Routed through Rust (rather than the
/// frontend calling the dialog plugin directly) so the picked paths are recorded
/// as consent — otherwise reading them back would have to trust an arbitrary
/// path from the webview.
#[tauri::command]
pub async fn pick_files_cmd(
    app: tauri::AppHandle,
    grants: State<'_, DialogGrants>,
) -> Cmd<Vec<String>> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Images and PDFs", &["png", "jpg", "jpeg", "gif", "webp", "bmp", "pdf"])
        .pick_files(move |picked| {
            let _ = tx.send(picked);
        });
    let Some(picked) = rx.await.map_err(err)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for p in picked {
        if let Ok(path) = p.into_path() {
            grants.remember(&path);
            out.push(canonicalize_lenient(&path).to_string_lossy().to_string());
        }
    }
    Ok(out)
}

fn app_data_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    app.path().app_data_dir().ok()
}

/// Attach (or detach, with `None`) the conversation's working folder.
#[tauri::command]
pub fn set_conversation_folder_cmd(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    grants: State<'_, DialogGrants>,
    id: String,
    path: Option<String>,
) -> Cmd<()> {
    let Some(path) = path else {
        // Detaching touches nothing on disk — it only forgets the path.
        return db.set_conversation_folder(&id, None).map_err(err);
    };
    let p = canonicalize_lenient(Path::new(&path));
    if let Some(reason) = refuse_as_working_folder(&p, app_data_dir(&app).as_deref()) {
        return Err(NexusError::Message(format!(
            "Poiesis can't work in {} — {reason}.",
            p.display()
        )));
    }
    grants.remember(&p);
    db.set_conversation_folder(&id, Some(&p.to_string_lossy())).map_err(err)
}

/// Set how much the agent may change inside the attached folder.
#[tauri::command]
pub fn set_conversation_trust_cmd(db: State<'_, Db>, id: String, trust: String) -> Cmd<()> {
    // Round-trip through the enum so an unknown string can't land in the DB and
    // silently read back as something more permissive than intended.
    let trust = crate::permissions::Trust::parse(&trust);
    db.set_conversation_trust(&id, trust.as_str()).map_err(err)
}

// ---- browsing ----

/// List one directory: every entry, files included, minus build noise.
///
/// Split out from the command so it can be tested directly — a listing that
/// silently drops files is the kind of bug that looks like a filter and isn't.
pub fn list_dir_nodes(dir: &Path, show_hidden: bool) -> std::io::Result<Vec<FileNode>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir)?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        // `file_type` is cheap (it comes off the directory entry) and, unlike
        // `metadata`, doesn't follow symlinks or fail on locked files — a
        // failure there must never silently reclassify a file as a folder.
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir && IGNORED_DIRS.contains(&name.as_str()) {
            continue;
        }
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let meta = e.metadata().ok();
        out.push(FileNode {
            name,
            path: e.path().to_string_lossy().to_string(),
            is_dir,
            size: meta.as_ref().filter(|m| m.is_file()).map(|m| m.len()).unwrap_or(0),
            modified: meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
        });
    }
    // Folders first, then files, each alphabetical — how people scan a tree.
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

/// One level of the tree. The panel expands lazily, so this is always one level.
#[tauri::command]
pub fn read_dir_tree_cmd(
    db: State<'_, Db>,
    grants: State<'_, DialogGrants>,
    conversation_id: Option<String>,
    path: String,
    show_hidden: Option<bool>,
) -> Cmd<Vec<FileNode>> {
    let dir = canonicalize_lenient(Path::new(&path));
    assert_ui_readable(&db, &grants, conversation_id.as_deref(), &dir)?;
    list_dir_nodes(&dir, show_hidden.unwrap_or(false)).map_err(err)
}

/// Read a file for the Workbench viewer. Bounded, because the viewer is a
/// preview — anything genuinely huge should be opened in a real editor.
#[tauri::command]
pub fn read_text_file_cmd(
    db: State<'_, Db>,
    grants: State<'_, DialogGrants>,
    conversation_id: Option<String>,
    path: String,
    max_bytes: Option<u64>,
) -> Cmd<String> {
    let p = canonicalize_lenient(Path::new(&path));
    assert_ui_readable(&db, &grants, conversation_id.as_deref(), &p)?;

    let cap = max_bytes.unwrap_or(512 * 1024);
    let meta = std::fs::metadata(&p).map_err(err)?;
    let text = std::fs::read_to_string(&p)
        .map_err(|_| NexusError::Message("that file isn't readable as text".into()))?;
    if meta.len() > cap {
        let clipped: String = text.chars().take(cap as usize).collect();
        return Ok(format!("{clipped}\n\n… truncated — open the file to see the rest"));
    }
    Ok(text)
}

/// Open a file with the system default application.
#[tauri::command]
pub fn open_path_cmd(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    grants: State<'_, DialogGrants>,
    conversation_id: Option<String>,
    path: String,
) -> Cmd<()> {
    use tauri_plugin_opener::OpenerExt;
    let p = canonicalize_lenient(Path::new(&path));
    assert_ui_readable(&db, &grants, conversation_id.as_deref(), &p)?;
    app.opener()
        .open_path(p.to_string_lossy().to_string(), None::<&str>)
        .map_err(err)
}

/// Show a file in the OS file manager.
#[tauri::command]
pub fn reveal_path_cmd(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    grants: State<'_, DialogGrants>,
    conversation_id: Option<String>,
    path: String,
) -> Cmd<()> {
    use tauri_plugin_opener::OpenerExt;
    let p = canonicalize_lenient(Path::new(&path));
    assert_ui_readable(&db, &grants, conversation_id.as_deref(), &p)?;
    app.opener().reveal_item_in_dir(&p).map_err(err)
}

// ---- artifact promotion ----

/// Materialise an artifact into the working folder: the moment something the
/// agent made becomes something the user keeps. `dest` may be relative to the
/// folder. Records a trash entry so the write is undoable like any other.
#[tauri::command]
pub fn save_artifact_to_folder_cmd(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    grants: State<'_, DialogGrants>,
    conversation_id: String,
    artifact_id: String,
    dest: String,
) -> Cmd<String> {
    let artifact = db
        .get_artifact(&artifact_id)
        .map_err(err)?
        .ok_or_else(|| NexusError::Message("that artifact no longer exists".into()))?;

    let (folder, _) = db.conversation_folder(&conversation_id).map_err(err)?;
    let folder = folder
        .ok_or_else(|| NexusError::Message("attach a folder first to save this into it".into()))?;
    let root = canonicalize_lenient(Path::new(&folder));

    let dest_path = Path::new(&dest);
    let target = canonicalize_lenient(&if dest_path.is_relative() {
        root.join(dest_path)
    } else {
        dest_path.to_path_buf()
    });
    if !path_within_root(&target, &root) {
        return Err(NexusError::Message(
            "pick a location inside the working folder".into(),
        ));
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(err)?;
    }
    let data_dir = app_data_dir(&app).unwrap_or_else(|| root.clone());
    let entry = trash::record(&db, &data_dir, &conversation_id, "save", &target, None);

    if artifact.kind == "image" {
        // Image artifacts hold a path to the generated file, not its bytes.
        std::fs::copy(&artifact.content, &target).map_err(err)?;
    } else {
        std::fs::write(&target, &artifact.content).map_err(err)?;
    }

    let shown = target.to_string_lossy().to_string();
    db.set_artifact_saved_path(&artifact_id, &shown).map_err(err)?;
    grants.remember(&target);
    let _ = db.log_activity(
        Some(&conversation_id),
        "file",
        &format!("saved artifact to {shown}"),
    );
    let _ = entry;
    Ok(shown)
}

// ---- recent changes / undo ----

#[tauri::command]
pub fn list_trash_cmd(
    db: State<'_, Db>,
    conversation_id: String,
    limit: Option<i64>,
) -> Cmd<Vec<TrashEntry>> {
    db.list_trash(&conversation_id, limit.unwrap_or(20)).map_err(err)
}

/// Reverse one recorded file operation.
#[tauri::command]
pub fn undo_file_op_cmd(db: State<'_, Db>, id: String) -> Cmd<()> {
    let entry = db
        .get_trash_entry(&id)
        .map_err(err)?
        .ok_or_else(|| NexusError::Message("that change is no longer undoable".into()))?;
    trash::undo(&db, &entry).map_err(NexusError::Message)?;
    let _ = db.log_activity(
        Some(&entry.conversation_id),
        "file",
        &format!("undid {} on {}", entry.op, entry.path),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact flow the Workbench performs: attach a folder, then ask to list
    /// it. If the scope check rejects the folder's own root, the panel shows an
    /// empty tree and the whole feature is dead.
    #[test]
    fn the_attached_folder_is_readable_including_its_own_root() {
        let dir = std::env::temp_dir().join(format!("poiesis_scope_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("README.md"), "hi").unwrap();
        let dir = canonicalize_lenient(&dir);

        let db = Db::open_in_memory().unwrap();
        let c = db.create_conversation("Scope", None, false).unwrap();
        db.set_conversation_folder(&c.id, Some(&dir.to_string_lossy())).unwrap();
        let grants = DialogGrants::new();

        assert!(
            assert_ui_readable(&db, &grants, Some(&c.id), &dir).is_ok(),
            "the folder's own root must be readable"
        );
        assert!(
            assert_ui_readable(&db, &grants, Some(&c.id), &dir.join("README.md")).is_ok(),
            "files inside it must be readable"
        );
        assert!(
            assert_ui_readable(&db, &grants, Some(&c.id), &dir.join("src")).is_ok(),
            "subfolders must be readable"
        );
        assert!(
            assert_ui_readable(&db, &grants, Some(&c.id), Path::new(r"C:\Windows\notepad.exe"))
                .is_err(),
            "…and nothing outside it"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn listing_returns_files_not_just_folders() {
        let dir = std::env::temp_dir().join(format!("poiesis_nodes_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        std::fs::write(dir.join("package.json"), "{}").unwrap();
        std::fs::write(dir.join("README.md"), "hi").unwrap();
        std::fs::write(dir.join(".env"), "SECRET=1").unwrap();
        // A *file* sharing a name with an ignored build folder must still list —
        // the ignore list is about directories, not names.
        std::fs::write(dir.join("build"), "#!/bin/sh").unwrap();

        let nodes = list_dir_nodes(&dir, false).unwrap();
        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();

        assert!(names.contains(&"package.json"), "files must list: {names:?}");
        assert!(names.contains(&"README.md"), "files must list: {names:?}");
        assert!(names.contains(&"build"), "a file named like a build dir still lists");
        assert!(names.contains(&"src"));
        assert!(!names.contains(&"node_modules"), "build folders stay out");
        assert!(!names.contains(&".env"), "dotfiles are hidden by default");

        // Folders first, then files, each alphabetical.
        assert_eq!(names.first(), Some(&"src"));
        assert!(nodes.iter().filter(|n| !n.is_dir).count() == 3);

        let with_hidden = list_dir_nodes(&dir, true).unwrap();
        assert!(with_hidden.iter().any(|n| n.name == ".env"), "show_hidden reveals them");

        // Sizes and timestamps come through, so the UI can show them.
        let readme = nodes.iter().find(|n| n.name == "README.md").unwrap();
        assert_eq!(readme.size, 2);
        assert!(readme.modified > 0);

        std::fs::remove_dir_all(&dir).ok();
    }
}
