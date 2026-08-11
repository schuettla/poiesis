//! Agent Skills commands (`SKL-UI-1`/`SKL-4`): discover, enable, install,
//! write, and forget skills.

use tauri::State;

use crate::agent::skillpack::{self, SkillPack, SkillSource};
use crate::db::Db;
use crate::runtime::RuntimeManager;
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

/// One skill as shown in the Skills tab (`SKL-UI-1`).
#[derive(serde::Serialize)]
pub struct SkillView {
    pub name: String,
    pub description: String,
    pub when_to_use: Option<String>,
    pub source: String,
    pub dir: String,
    pub enabled: bool,
    pub unsupported: Vec<String>,
    /// `used {n}× · {m} rough` (`OUT-2`) — every activation ever, and how many
    /// had at least one tool failure afterwards.
    pub used: i64,
    pub rough: i64,
    /// `SKL-4`: `TRU-1`'s reading of the body, so the row can say what the
    /// skill contains before the user enables it.
    pub risk: u8,
    pub risk_flags: Vec<String>,
}

fn to_view(db: &Db, pack: SkillPack) -> SkillView {
    let enabled = skillpack::is_enabled(db, &pack);
    let (used, rough) = db.skill_run_totals(&pack.name).unwrap_or((0, 0));
    SkillView {
        name: pack.name,
        description: pack.description,
        when_to_use: pack.when_to_use,
        source: pack.source.id().to_string(),
        dir: pack.dir.display().to_string(),
        enabled,
        unsupported: pack.unsupported,
        used,
        rough,
        risk: pack.risk,
        risk_flags: pack.risk_flags,
    }
}

#[tauri::command]
pub fn list_skills_cmd(db: State<'_, Db>, mgr: State<'_, RuntimeManager>, working_folder: Option<String>) -> Vec<SkillView> {
    let folder = working_folder.map(std::path::PathBuf::from);
    skillpack::discover(mgr.app_data_dir(), folder.as_deref())
        .into_iter()
        .map(|p| to_view(&db, p))
        .collect()
}

#[tauri::command]
pub async fn set_skill_enabled_cmd(
    db: State<'_, Db>,
    mem: State<'_, crate::memory::MemoryStore>,
    mgr: State<'_, RuntimeManager>,
    app: tauri::AppHandle,
    source: String,
    name: String,
    enabled: bool,
    target: Option<crate::commands::agent::ChatTarget>,
) -> Cmd<()> {
    let source = match source.as_str() {
        "personal" => SkillSource::Personal,
        "project" => SkillSource::Project,
        "app" => SkillSource::App,
        other => return Err(PoiesisError::Message(format!("Unknown skill source '{other}'."))),
    };

    // `GLD-2`: enabling a skill injects new instructions into every prompt —
    // check it against the golden set right after, and switch it back off on
    // a confirmed regression. Turning one *off* only narrows what the model
    // can do, so it isn't worth the round trip.
    if !enabled {
        skillpack::set_enabled(&db, source, &name, false);
        return Ok(());
    }

    use tauri::Emitter;
    let regressed = crate::agent::golden::guard_self_change(
        &mgr.client,
        &mgr,
        &db,
        &mem,
        target.as_ref(),
        || {
            skillpack::set_enabled(&db, source, &name, true);
            Ok(())
        },
        || {
            skillpack::set_enabled(&db, source, &name, false);
            Ok(())
        },
    )
    .await
    .map_err(PoiesisError::Message)?;

    if let Some(n) = regressed {
        let _ = db.log_activity(
            None,
            "memory",
            &format!("the {name} skill made me worse at {n} thing(s) — I turned it back off"),
        );
        let _ = app.emit("poiesis-golden-reverted", serde_json::json!({ "count": n }));
    }
    Ok(())
}

/// Write a new skill directly (`SKL-5`: "the user gets a pen") — no proposal,
/// since the user is authoring it themselves.
#[tauri::command]
pub fn create_skill_cmd(
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    name: String,
    description: String,
    when_to_use: String,
    body: String,
) -> Cmd<SkillView> {
    let slug = crate::memory::slugify(&name).map_err(PoiesisError::Message)?;
    let dir = mgr.skills_dir().join(&slug);
    std::fs::create_dir_all(&dir).map_err(err)?;
    let text = skillpack::render_skill_md(&slug, &description, &when_to_use, &body);
    std::fs::write(dir.join("SKILL.md"), &text).map_err(err)?;
    skillpack::set_enabled(&db, SkillSource::App, &slug, true);
    let _ = db.log_activity(None, "memory", &format!("wrote the skill {slug}"));
    let pack = skillpack::parse_pack(&dir, SkillSource::App).ok_or_else(|| PoiesisError::Message("couldn't read back the skill just written".into()))?;
    Ok(to_view(&db, pack))
}

/// Overwrite an existing App-sourced skill's body (`SKL-5`). Personal/Project
/// skills are never rewritten in place — those live in a directory the app
/// doesn't own.
#[tauri::command]
pub fn update_skill_cmd(
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    name: String,
    description: String,
    when_to_use: String,
    body: String,
) -> Cmd<SkillView> {
    // Slugified, not trusted: `name` arrives from the renderer, and a `..` in
    // it would otherwise write outside the skills folder entirely.
    let name = crate::memory::slugify(&name).map_err(PoiesisError::Message)?;
    let dir = mgr.skills_dir().join(&name);
    if !dir.join("SKILL.md").exists() {
        return Err(PoiesisError::Message(format!("No skill named {name} in my own folder.")));
    }
    let text = skillpack::render_skill_md(&name, &description, &when_to_use, &body);
    std::fs::write(dir.join("SKILL.md"), &text).map_err(err)?;
    let _ = db.log_activity(None, "memory", &format!("updated the skill {name}"));
    let pack = skillpack::parse_pack(&dir, SkillSource::App).ok_or_else(|| PoiesisError::Message("couldn't read back the updated skill".into()))?;
    Ok(to_view(&db, pack))
}

/// Install a skill folder already on disk (`Add from folder…`, `SKL-4`): copy
/// it into `<app-data>/skills/<name>/`, refusing symlinks and anything past a
/// modest size cap so a hostile "skill" can't smuggle in something enormous.
const INSTALL_FILE_CAP_BYTES: u64 = 5 * 1024 * 1024;
const INSTALL_TOTAL_CAP_BYTES: u64 = 50 * 1024 * 1024;

#[tauri::command]
pub fn install_skill_cmd(db: State<'_, Db>, mgr: State<'_, RuntimeManager>, source_dir: String) -> Cmd<SkillView> {
    install_from_dir(&db, &mgr, std::path::Path::new(&source_dir))
}

/// Install a skill from a `.zip` archive (`Add from zip…`, `SKL-4`): extract
/// to a scratch directory under the same traversal/symlink/size-cap refusals
/// as a folder install, then hand off to the same install path so both
/// sources are gated identically.
#[tauri::command]
pub fn install_skill_zip_cmd(db: State<'_, Db>, mgr: State<'_, RuntimeManager>, archive_path: String) -> Cmd<SkillView> {
    let file = std::fs::File::open(&archive_path).map_err(err)?;
    let mut zip = zip::ZipArchive::new(file).map_err(err)?;

    let scratch = mgr.app_data_dir().join("skills-scratch").join(uuid::Uuid::new_v4().to_string());
    let result = (|| -> Result<SkillView, PoiesisError> {
        std::fs::create_dir_all(&scratch).map_err(err)?;
        extract_zip_checked(&mut zip, &scratch).map_err(err)?;
        let root = find_skill_root(&scratch)
            .ok_or_else(|| PoiesisError::Message("That archive doesn't have a SKILL.md in it.".into()))?;
        install_from_dir(&db, &mgr, &root)
    })();
    std::fs::remove_dir_all(&scratch).ok();
    result
}

fn install_from_dir(db: &Db, mgr: &RuntimeManager, src: &std::path::Path) -> Cmd<SkillView> {
    if !src.join("SKILL.md").exists() {
        return Err(PoiesisError::Message("That folder doesn't have a SKILL.md in it.".into()));
    }
    let pack = skillpack::parse_pack(src, SkillSource::App)
        .ok_or_else(|| PoiesisError::Message("Couldn't read that skill.".into()))?;
    let slug = crate::memory::slugify(&pack.name).map_err(PoiesisError::Message)?;
    let dest = mgr.skills_dir().join(&slug);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(err)?;
    }
    let mut total = 0u64;
    copy_dir_checked(src, &dest, &mut total).map_err(err)?;
    skillpack::set_enabled(db, SkillSource::App, &slug, true);
    let _ = db.log_activity(None, "memory", &format!("installed the skill {slug}"));
    let installed = skillpack::parse_pack(&dest, SkillSource::App)
        .ok_or_else(|| PoiesisError::Message("installed, but couldn't read it back".into()))?;
    Ok(to_view(db, installed))
}

fn copy_dir_checked(src: &std::path::Path, dest: &std::path::Path, total: &mut u64) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        let target = dest.join(entry.file_name());
        if meta.is_symlink() {
            continue; // refuse: a skill directory must be plain files
        }
        if meta.is_dir() {
            copy_dir_checked(&entry.path(), &target, total)?;
        } else if meta.is_file() {
            if meta.len() > INSTALL_FILE_CAP_BYTES {
                continue;
            }
            *total += meta.len();
            if *total > INSTALL_TOTAL_CAP_BYTES {
                return Err(std::io::Error::other("that skill is too large to install"));
            }
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Unix "is a symlink" bits within the packed `st_mode` a zip entry may carry
/// in its external attributes. Windows-built archives never set this, so a
/// symlink from a unix machine is the only case this needs to catch.
const SYMLINK_MODE_MASK: u32 = 0o170000;
const SYMLINK_MODE: u32 = 0o120000;

/// Extract a zip archive under the same refusals as `copy_dir_checked`:
/// path-traversal entries (`enclosed_name` already refuses these), symlinks,
/// and anything past the per-file/total size caps.
fn extract_zip_checked(zip: &mut zip::ZipArchive<std::fs::File>, dest: &std::path::Path) -> std::io::Result<()> {
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(std::io::Error::other)?;
        let Some(rel) = entry.enclosed_name() else { continue };
        if let Some(mode) = entry.unix_mode() {
            if mode & SYMLINK_MODE_MASK == SYMLINK_MODE {
                continue; // refuse: a skill directory must be plain files
            }
        }
        let out_path = dest.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
            continue;
        }
        if entry.size() > INSTALL_FILE_CAP_BYTES {
            continue;
        }
        total += entry.size();
        if total > INSTALL_TOTAL_CAP_BYTES {
            return Err(std::io::Error::other("that archive is too large to install"));
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}

/// The archive may wrap everything in one top-level folder (the common "zip
/// a folder" shape). Check the extraction root first, then exactly one level
/// down if it holds a single directory entry — never deeper.
fn find_skill_root(scratch: &std::path::Path) -> Option<std::path::PathBuf> {
    if scratch.join("SKILL.md").exists() {
        return Some(scratch.to_path_buf());
    }
    let mut entries = std::fs::read_dir(scratch).ok()?.filter_map(|e| e.ok());
    let only = entries.next()?;
    if entries.next().is_some() {
        return None; // more than one top-level entry — not a wrapper folder
    }
    let path = only.path();
    (path.is_dir() && path.join("SKILL.md").exists()).then_some(path)
}

/// A skill's full body markdown, for the Skills tab's `View`/`Edit` (`SKL-UI-1`).
#[tauri::command]
pub fn skill_body_cmd(mgr: State<'_, RuntimeManager>, working_folder: Option<String>, name: String) -> Cmd<String> {
    let folder = working_folder.map(std::path::PathBuf::from);
    let pack = skillpack::discover(mgr.app_data_dir(), folder.as_deref())
        .into_iter()
        .find(|p| p.name == name)
        .ok_or_else(|| PoiesisError::Message(format!("no skill named {name}")))?;
    skillpack::load_body(&pack).map_err(PoiesisError::Message)
}

/// `SKL-4`: skills sitting in other agents' folders, offered for import.
///
/// Listing is not reading. Poiesis never loads these into a prompt — it shows
/// what's there so the user can choose to copy it in, which is the deliberate
/// act that replaced automatically scanning `~/.claude/`.
#[tauri::command]
pub fn discoverable_skill_imports_cmd(
    mgr: State<'_, RuntimeManager>,
    extra_roots: Option<Vec<String>>,
) -> Vec<skillpack::ImportableSkill> {
    let extra: Vec<std::path::PathBuf> =
        extra_roots.unwrap_or_default().into_iter().map(std::path::PathBuf::from).collect();
    skillpack::discoverable_imports(mgr.app_data_dir(), &extra)
}

/// Copy chosen skills in. Each lands in `<app_data>/skills/` through exactly
/// the same path as `Add from folder…`, so the traversal, symlink and size
/// refusals apply identically — an import is an install, not a shortcut.
///
/// Partial success is reported rather than rolled back: eight of ten skills
/// copied is a better outcome than none, and the two that failed are named.
#[tauri::command]
pub fn import_skills_cmd(
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    dirs: Vec<String>,
) -> Cmd<Vec<String>> {
    let mut failed = Vec::new();
    for dir in &dirs {
        if let Err(e) = install_from_dir(&db, &mgr, std::path::Path::new(dir)) {
            let name =
                std::path::Path::new(dir).file_name().and_then(|n| n.to_str()).unwrap_or(dir);
            failed.push(format!("{name} ({e})"));
        }
    }
    Ok(failed)
}

/// Where the user's own skills live, created on demand.
///
/// The Skills tab shows this path and can open it. On a fresh install the
/// folder doesn't exist yet, and "drop a folder in `~/.poiesis/skills/`" is
/// useless advice if the place isn't there — so asking for it makes it.
#[tauri::command]
pub fn personal_skills_dir_cmd() -> Cmd<String> {
    let dir = skillpack::personal_skills_dir()
        .ok_or_else(|| PoiesisError::Message("I couldn't work out your home folder.".into()))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| PoiesisError::Message(format!("I couldn't make {}: {e}", dir.display())))?;
    Ok(dir.display().to_string())
}

/// `SKL-UI-2`: score a not-yet-installed `SKILL.md` so the install card can
/// carry its `TRU-1` risk line. The card holds the proposed text and nothing
/// on disk yet, so it can't go through `list_skills_cmd`'s scan — but the
/// judgement must be the same one, hence `untrusted::scan` rather than
/// anything card-specific.
#[tauri::command]
pub fn scan_skill_text_cmd(text: String) -> crate::agent::untrusted::Scan {
    crate::agent::untrusted::scan(&text)
}

/// A skill's bundled workspace template, if it has one (`SKL-5`, carrying
/// `RCP-UI-2` forward): `assets/surface.json` beside its `SKILL.md`. `None`
/// when the skill has no template — the common case, and not an error.
#[tauri::command]
pub fn skill_surface_cmd(
    mgr: State<'_, RuntimeManager>,
    working_folder: Option<String>,
    name: String,
) -> Option<String> {
    let folder = working_folder.map(std::path::PathBuf::from);
    let pack = skillpack::discover(mgr.app_data_dir(), folder.as_deref())
        .into_iter()
        .find(|p| p.name == name)?;
    let text = std::fs::read_to_string(pack.dir.join("assets").join("surface.json")).ok()?;
    // A template that isn't valid JSON is the same as not having one: the
    // caller would only hand it to `set_surface_cmd`, which refuses it anyway.
    serde_json::from_str::<serde_json::Value>(&text).ok()?;
    Some(text)
}

/// Remove an App-sourced skill entirely. Personal/Project skills aren't
/// "forgotten" here — turn them off instead (`set_skill_enabled_cmd`); the
/// app doesn't delete files it doesn't own.
#[tauri::command]
pub fn forget_skill_cmd(db: State<'_, Db>, mgr: State<'_, RuntimeManager>, name: String) -> Cmd<()> {
    // Same reasoning as `update_skill_cmd`: this one ends in `remove_dir_all`,
    // so an unvalidated `..` would be considerably worse than a stray write.
    let name = crate::memory::slugify(&name).map_err(PoiesisError::Message)?;
    let dir = mgr.skills_dir().join(&name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(err)?;
    }
    let _ = db.set_setting(&skillpack::setting_key(SkillSource::App, &name), "false");
    let _ = db.log_activity(None, "memory", &format!("forgot the skill {name}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("poiesis_zip_{label}_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn open_zip(path: &std::path::Path) -> zip::ZipArchive<std::fs::File> {
        zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap()
    }

    /// `SKL-4`: a traversal entry's target lands outside `dest` unless
    /// refused. `enclosed_name()` (relied on by `extract_zip_checked`, same
    /// as `runtime::download::unpack_zip`) is what does the refusing.
    #[test]
    fn zip_extraction_refuses_path_traversal_entries() {
        let root = scratch_dir("traversal");
        let archive = root.join("evil.zip");
        {
            let file = std::fs::File::create(&archive).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("SKILL.md", opts).unwrap();
            zip.write_all(b"---\nname: x\n---\nbody").unwrap();
            zip.start_file("../../escaped.txt", opts).unwrap();
            zip.write_all(b"should never land outside dest").unwrap();
            zip.finish().unwrap();
        }

        let dest = root.join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        let mut zip = open_zip(&archive);
        extract_zip_checked(&mut zip, &dest).unwrap();

        assert!(dest.join("SKILL.md").exists());
        assert!(!root.join("escaped.txt").exists(), "traversal entry must not escape dest");
        std::fs::remove_dir_all(&root).ok();
    }

    /// `SKL-4`: a symlink packed into the archive must not be recreated —
    /// same refusal `copy_dir_checked` applies to a folder install.
    #[test]
    fn zip_extraction_refuses_symlinks() {
        let root = scratch_dir("symlink");
        let archive = root.join("evil.zip");
        {
            let file = std::fs::File::create(&archive).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("SKILL.md", opts).unwrap();
            zip.write_all(b"---\nname: x\n---\nbody").unwrap();
            zip.add_symlink("link.txt", "/etc/passwd", opts).unwrap();
            zip.finish().unwrap();
        }

        let dest = root.join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        let mut zip = open_zip(&archive);
        extract_zip_checked(&mut zip, &dest).unwrap();

        assert!(dest.join("SKILL.md").exists());
        assert!(!dest.join("link.txt").exists(), "a symlink entry must not be recreated");
        std::fs::remove_dir_all(&root).ok();
    }

    /// `SKL-4`: an oversized entry is skipped, not merely truncated — the
    /// same per-file cap `copy_dir_checked` enforces on a folder install.
    #[test]
    fn zip_extraction_refuses_oversized_files() {
        let root = scratch_dir("oversize");
        let archive = root.join("big.zip");
        {
            let file = std::fs::File::create(&archive).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("SKILL.md", opts).unwrap();
            zip.write_all(b"---\nname: x\n---\nbody").unwrap();
            zip.start_file("huge.bin", opts).unwrap();
            // Well over `INSTALL_FILE_CAP_BYTES` (5 MB); zeros compress to
            // nearly nothing so this stays a fast test.
            zip.write_all(&vec![0u8; 6 * 1024 * 1024]).unwrap();
            zip.finish().unwrap();
        }

        let dest = root.join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        let mut zip = open_zip(&archive);
        extract_zip_checked(&mut zip, &dest).unwrap();

        assert!(dest.join("SKILL.md").exists());
        assert!(!dest.join("huge.bin").exists(), "an oversized entry must be skipped");
        std::fs::remove_dir_all(&root).ok();
    }

    /// `find_skill_root`: a zip of a folder commonly wraps everything in one
    /// top-level directory — the installer must look one level down for it,
    /// but no deeper.
    #[test]
    fn find_skill_root_looks_one_level_into_a_wrapper_folder() {
        let root = scratch_dir("wrapper");
        std::fs::create_dir_all(root.join("my-skill")).unwrap();
        std::fs::write(root.join("my-skill").join("SKILL.md"), "---\nname: x\n---\nbody").unwrap();

        let found = find_skill_root(&root).expect("should find the wrapped skill");
        assert_eq!(found, root.join("my-skill"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn find_skill_root_is_none_for_multiple_top_level_entries_with_no_skill_md() {
        let root = scratch_dir("ambiguous");
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::create_dir_all(root.join("b")).unwrap();
        assert!(find_skill_root(&root).is_none());
        std::fs::remove_dir_all(&root).ok();
    }
}
