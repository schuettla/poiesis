//! `WRK-3`: how a chat works. The presets the `/style` menu and the chip read,
//! and the choice for one chat.

use serde::Serialize;
use tauri::State;

use crate::db::Db;
use crate::memory::ways::sentence;
use crate::memory::MemoryStore;
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

/// One preset as the window draws it.
#[derive(Debug, Clone, Serialize)]
pub struct PresetView {
    pub slug: String,
    pub name: String,
    /// The second line.
    pub line: String,
    /// Which glyph draws it.
    pub glyph: String,
    pub built_in: bool,
    pub locked: bool,
    pub version: u32,
    /// What it switches on, as sentences, for the hover.
    pub rules: Vec<String>,
    /// "from Lean, Nov 7": where it comes from, when it came from somewhere.
    pub lineage: Option<String>,
    /// The preset it branched off, when evolution made it.
    pub parent: Option<String>,
}

impl PresetView {
    /// `all` is every preset, so a lineage can name its parent.
    pub fn of(p: crate::memory::ways::Preset, all: &[crate::memory::ways::Preset]) -> PresetView {
        let parent_name = all.iter().find(|o| o.slug == p.from).map(|o| o.name.clone());
        PresetView {
            rules: p.rules.iter().map(|r| sentence(r.when, r.mv)).collect(),
            lineage: p.lineage(parent_name.as_deref()),
            parent: (p.how == "evolution" && parent_name.is_some()).then(|| p.from.clone()),
            slug: p.slug,
            name: p.name,
            line: p.line,
            glyph: p.glyph,
            built_in: p.built_in,
            locked: p.locked,
            version: p.version,
        }
    }
}

/// `WRK-7`: "Not this time" on a move. The rule is skipped the next time it would
/// fire in that run, and then it fires again. `false` when the run is over: there
/// is nothing left to skip, and the window says so rather than pretend.
#[tauri::command]
pub fn skip_move_cmd(fleet: State<'_, crate::agent::fleet::Fleet>, run_id: String, rule_id: String) -> bool {
    match fleet.get(&run_id) {
        Some(run) => {
            if let Ok(mut skips) = run.skips.lock() {
                skips.push(rule_id);
            }
            true
        }
        None => false,
    }
}

/// `WRK-13`: work another way from the next item on. The loop applies it when no
/// item is in hand, and says so in one line. `false` when the run is over or the
/// preset is not one we have, and the window says so rather than pretend.
#[tauri::command]
pub fn switch_run_style_cmd(
    fleet: State<'_, crate::agent::fleet::Fleet>,
    mem: State<'_, MemoryStore>,
    run_id: String,
    preset: String,
) -> bool {
    let Some(found) = mem.find_preset(&preset) else { return false };
    match fleet.get(&run_id) {
        Some(run) => {
            if let Ok(mut slot) = run.style_switch.lock() {
                *slot = Some(found.slug);
            }
            true
        }
        None => false,
    }
}

/// `WRK-6`: both ways of an item that was tried two ways, as they were kept: what
/// each helper said, what each changed. Read-only, and read from a record, because
/// the copies are gone.
#[derive(Debug, Clone, Serialize)]
pub struct BranchView {
    pub kept: String,
    pub reason: String,
    pub text: String,
    pub ways: Vec<String>,
    pub patches: Vec<String>,
    pub files: Vec<usize>,
    /// The helpers' own chats, so a transcript can be opened.
    pub children: Vec<String>,
}

#[tauri::command]
pub fn branch_view_cmd(mgr: State<'_, crate::runtime::RuntimeManager>, id: String) -> Option<BranchView> {
    let r = crate::agent::branch::load(&mgr.generated_media_dir(), &id)?;
    Some(BranchView {
        kept: r.kept,
        reason: r.reason,
        text: r.text,
        ways: r.ways,
        patches: r.patches,
        files: r.files,
        children: r.children,
    })
}

#[tauri::command]
pub fn presets_list_cmd(mem: State<'_, MemoryStore>) -> Vec<PresetView> {
    let all = mem.presets();
    all.iter().cloned().map(|p| PresetView::of(p, &all)).collect()
}

/// How this chat works from now on. `None` gives the choice back: the chat starts
/// where its place says (`MemoryStore::default_for`). Ad hoc chosen by name is kept
/// as `ad-hoc`, so a project or global default cannot override it. Returns the slug
/// that was kept.
#[tauri::command]
pub fn set_conversation_style_cmd(
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    id: String,
    preset: Option<String>,
) -> Cmd<Option<String>> {
    let slug = match preset.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        None => None,
        Some(typed) => Some(
            mem.find_preset(typed)
                .ok_or_else(|| PoiesisError::Message(format!("I don't have a style called \"{typed}\".")))?
                .slug,
        ),
    };
    db.set_conversation_style(&id, slug.as_deref()).map_err(|e| PoiesisError::Message(e.to_string()))?;
    Ok(slug)
}
