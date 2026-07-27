//! Commands over the durable self (MEM-3, MEM-5, SOUL-3, MEM-UI).
//!
//! The user-facing half of the memory system: read the index for injection,
//! browse and edit facts by hand, and review what the agent proposed. Nothing
//! here lets the agent apply a change on its own — `consolidate` and
//! `propose_soul_edit` both stop at a proposal the user answers.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::cloud::{drive_turn, ChatEndpoint};
use crate::commands::agent::{build_cloud_endpoint, ChatTarget};
use crate::db::{ChangeProposal, Db};
use crate::memory::{Fact, MemoryStore, LESSONS};
use crate::runtime::proxy::{CancelFlag, TurnOutcome};
use crate::runtime::RuntimeManager;
use crate::NexusError;

type Cmd<T> = Result<T, NexusError>;

/// Settings key holding a consolidation the user hasn't answered yet.
const PENDING_CONSOLIDATION: &str = "memory.pending_consolidation";

fn err<E: std::fmt::Display>(e: E) -> NexusError {
    NexusError::Message(e.to_string())
}

/// What gets prepended to every conversation (MEM-3).
#[derive(Debug, Serialize)]
pub struct MemoryContext {
    /// The generated MEMORY.md body — one line per entry.
    pub index: String,
    /// Standing instructions the user approved (SOUL.md).
    pub soul: String,
    pub fact_count: usize,
}

#[tauri::command]
pub fn get_memory_context_cmd(mem: State<'_, MemoryStore>) -> MemoryContext {
    MemoryContext {
        index: mem.index_markdown(),
        soul: mem.soul(),
        fact_count: mem.list().len(),
    }
}

#[tauri::command]
pub fn list_memory_facts_cmd(mem: State<'_, MemoryStore>) -> Vec<Fact> {
    mem.list()
}

#[tauri::command]
pub fn update_memory_fact_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    name: String,
    description: Option<String>,
    body: String,
) -> Cmd<()> {
    mem.update(&db, &name, description.as_deref(), &body)
        .map_err(NexusError::Message)?;
    let _ = db.log_activity(None, "memory", &format!("edited {name}"));
    Ok(())
}

/// Move a fact to `.trash/`. Returns the trash filename, which the undo strip
/// hands back to `restore_memory_fact_cmd`.
#[tauri::command]
pub fn forget_memory_fact_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    name: String,
) -> Cmd<String> {
    let file = mem.forget(&db, &name).map_err(NexusError::Message)?;
    let _ = db.log_activity(None, "memory", &format!("forgot {name}"));
    Ok(file)
}

#[tauri::command]
pub fn restore_memory_fact_cmd(
    mem: State<'_, MemoryStore>,
    db: State<'_, Db>,
    file: String,
) -> Cmd<()> {
    mem.restore_trash(&db, &file).map_err(NexusError::Message)?;
    let _ = db.log_activity(None, "memory", "restored a forgotten memory");
    Ok(())
}

#[tauri::command]
pub fn set_soul_cmd(mem: State<'_, MemoryStore>, db: State<'_, Db>, text: String) -> Cmd<()> {
    mem.set_soul(&text).map_err(NexusError::Message)?;
    let _ = db.log_activity(None, "memory", "edited standing instructions");
    Ok(())
}

/// Reveal the memory folder in the OS file manager — the point of storing the
/// self as markdown is that the user can open it.
#[tauri::command]
pub fn open_memory_dir_cmd(app: tauri::AppHandle, mem: State<'_, MemoryStore>) -> Cmd<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(mem.dir().to_string_lossy().to_string(), None::<&str>)
        .map_err(err)
}

/// Zip the whole self into the data directory and reveal it (MEM-UI-1).
///
/// Deliberately not a save dialog: the export lands beside the memory folder
/// the user already knows, and the file manager opens on it. `.snapshots/` is
/// skipped — those are copies of what's already in the archive.
#[tauri::command]
pub fn export_memory_zip_cmd(app: tauri::AppHandle, mem: State<'_, MemoryStore>) -> Cmd<String> {
    use std::io::Write;
    use tauri_plugin_opener::OpenerExt;

    let dir = mem.dir();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let out = dir
        .parent()
        .unwrap_or(dir)
        .join(format!("poiesis-memory-{stamp}.zip"));

    let file = std::fs::File::create(&out).map_err(err)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();

    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((from, prefix)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&from) else { continue };
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".snapshots" {
                continue;
            }
            let inner = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            if entry.path().is_dir() {
                stack.push((entry.path(), inner));
            } else if let Ok(bytes) = std::fs::read(entry.path()) {
                zip.start_file(&inner, opts).map_err(err)?;
                zip.write_all(&bytes).map_err(err)?;
            }
        }
    }
    zip.finish().map_err(err)?;

    let path = out.to_string_lossy().to_string();
    let _ = app.opener().reveal_item_in_dir(&path);
    Ok(path)
}

// ---- proposals (SOUL-3) ----

#[tauri::command]
pub fn list_change_proposals_cmd(db: State<'_, Db>) -> Cmd<Vec<ChangeProposal>> {
    db.list_change_proposals().map_err(err)
}

/// Answer a proposal. Accepting a `soul` proposal is the only path by which
/// standing instructions ever change on the agent's initiative — and it runs
/// here, on an explicit user action, never inside the agent loop.
///
/// `persona` proposals are deliberately unhandled: the table's `target` and
/// `persona_id` columns future-proof per-persona prompt edits, which are out of
/// scope for v1. Recipe proposals arrive with RCP-2 in Part IV.
#[tauri::command]
pub fn resolve_change_proposal_cmd(
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    id: String,
    accept: bool,
) -> Cmd<()> {
    let proposal = db
        .get_change_proposal(&id)
        .map_err(err)?
        .ok_or_else(|| NexusError::Message("That proposal no longer exists.".into()))?;

    // Already answered (double-click, stale card): don't apply a second time.
    if proposal.status != "pending" {
        return Err(NexusError::Message("That proposal was already answered.".into()));
    }

    if !accept {
        db.resolve_change_proposal(&id, "dismissed").map_err(err)?;
        let _ = db.log_activity(None, "memory", "dismissed a proposed change");
        return Ok(());
    }

    match proposal.target.as_str() {
        "soul" => {
            mem.set_soul(&proposal.proposed_text).map_err(NexusError::Message)?;
            db.resolve_change_proposal(&id, "applied").map_err(err)?;
            let _ = db.log_activity(None, "memory", "accepted a standing instruction");
            Ok(())
        }
        "recipe" => {
            // The proposal holds the complete future file, so accepting is a
            // parse and a write — the same text the user reviewed, nothing re-derived.
            let slug = proposal.slug.as_deref().unwrap_or("recipe");
            let recipe = crate::memory::parse_recipe(slug, &proposal.proposed_text);
            let name = mem.save_recipe(&db, &recipe).map_err(NexusError::Message)?;
            db.resolve_change_proposal(&id, "applied").map_err(err)?;
            let _ = db.log_activity(None, "memory", &format!("kept the recipe {name}"));
            Ok(())
        }
        "lesson" => {
            // Reflection at rung `ask`: the body is the lesson, the rationale
            // is its one-line description.
            let slug = proposal
                .slug
                .clone()
                .ok_or_else(|| NexusError::Message("That lesson has no name.".into()))?;
            mem.save_lesson(
                &db,
                &Fact {
                    name: slug.clone(),
                    description: proposal.rationale.clone(),
                    kind: "lesson".into(),
                    created: String::new(),
                    source_conversation: None,
                    body: proposal.proposed_text.clone(),
                },
            )
            .map_err(NexusError::Message)?;
            db.resolve_change_proposal(&id, "applied").map_err(err)?;
            let _ = db.log_activity(None, "reflect", &format!("learned {slug}"));
            Ok(())
        }
        other => Err(NexusError::Message(format!(
            "Proposals for '{other}' can't be applied in this version."
        ))),
    }
}

// ---- consolidation (MEM-5) ----

/// A cleanup the model proposes over the whole fact set. Never applied without
/// the user pressing Apply.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Consolidation {
    #[serde(default)]
    pub deletes: Vec<String>,
    #[serde(default)]
    pub edits: Vec<ConsolidationEdit>,
    #[serde(default)]
    pub merges: Vec<ConsolidationMerge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationEdit {
    pub name: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationMerge {
    /// The entry that survives, rewritten with `text`.
    pub keep: String,
    /// Entries folded into `keep` and moved to trash.
    pub drop: Vec<String>,
    pub text: String,
}

/// Ask the model to propose a tidy-up of the memory set. Returns the proposal
/// and stores it as a pending setting; **nothing is changed on disk here.**
#[tauri::command]
pub async fn consolidate_memory_cmd(
    mgr: State<'_, RuntimeManager>,
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    target: Option<ChatTarget>,
) -> Cmd<Consolidation> {
    // AUT-1: tidying up rewrites and drops entries wholesale, so `off` has to
    // stop it at the source — not merely hide the button.
    if crate::autonomy::autonomy_gate(&db, "consolidate") == crate::autonomy::Rung::Off {
        return Err(NexusError::Message(
            "Tidying up my memory is turned off in my Self panel.".into(),
        ));
    }
    let target = target.unwrap_or_default();
    let endpoint = if target.provenance.as_deref() == Some("cloud") {
        build_cloud_endpoint(&target).map_err(NexusError::Message)?
    } else {
        let Some((base_url, token)) = mgr.engine_endpoint().await else {
            return Err(NexusError::Message(
                "No model is loaded, so memory can't be tidied up right now.".into(),
            ));
        };
        ChatEndpoint::OpenAi {
            base_url,
            api_key: Some(token),
            model: None,
        }
    };

    let mut entries = mem.list();
    entries.extend(mem.list_in(LESSONS));
    if entries.is_empty() {
        return Ok(Consolidation::default());
    }
    let listing = entries
        .iter()
        .map(|f| format!("- {} ({}): {}", f.name, f.kind, f.body))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "You maintain a personal memory file set. Propose a cleanup as JSON only:\n\
         {{\"deletes\":[\"name\"],\"edits\":[{{\"name\":\"...\",\"text\":\"...\"}}],\
         \"merges\":[{{\"keep\":\"name\",\"drop\":[\"name\"],\"text\":\"merged body\"}}]}}\n\
         Merge duplicates, drop facts superseded by newer ones, tighten wording.\n\
         Propose nothing you are unsure about.\nFacts:\n{listing}"
    );
    let msgs = vec![
        serde_json::json!({
            "role": "system",
            "content": "You output only JSON. No preamble, no code fences.",
        }),
        serde_json::json!({ "role": "user", "content": prompt }),
    ];

    let outcome = drive_turn(&mgr.client, &endpoint, &msgs, &[], 0.2, &CancelFlag::new(), |_| {})
        .await
        .map_err(err)?;
    let raw = match outcome {
        TurnOutcome::Final { content } => content,
        _ => return Ok(Consolidation::default()),
    };

    // Parse strictly. A model that returns something we can't read proposes
    // nothing — guessing at its intent is exactly the wrong move here.
    let proposal: Consolidation =
        serde_json::from_str(strip_fence(&raw)).unwrap_or_default();

    let json = serde_json::to_string(&proposal).map_err(err)?;
    db.set_setting(PENDING_CONSOLIDATION, &json).map_err(err)?;
    Ok(proposal)
}

/// Strip a ```json fence if the model wrapped its answer in one.
fn strip_fence(s: &str) -> &str {
    let s = s.trim();
    let Some(rest) = s.strip_prefix("```") else { return s };
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    rest.trim_start_matches('\n').trim_end_matches('`').trim()
}

#[tauri::command]
pub fn get_pending_consolidation_cmd(db: State<'_, Db>) -> Cmd<Option<Consolidation>> {
    let Some(raw) = db.get_setting(PENDING_CONSOLIDATION).map_err(err)? else {
        return Ok(None);
    };
    Ok(serde_json::from_str(&raw).ok())
}

/// Apply or discard the pending consolidation. Applying snapshots the whole
/// memory folder first, so a bad tidy-up is always recoverable.
#[tauri::command]
pub fn apply_consolidation_cmd(
    db: State<'_, Db>,
    mem: State<'_, MemoryStore>,
    accept: bool,
) -> Cmd<()> {
    let Some(raw) = db.get_setting(PENDING_CONSOLIDATION).map_err(err)? else {
        return Ok(());
    };
    if !accept {
        db.set_setting(PENDING_CONSOLIDATION, "").map_err(err)?;
        return Ok(());
    }

    // Turning consolidation off between proposing and applying withdraws
    // consent for the whole batch; the pending proposal is dropped with it.
    if crate::autonomy::autonomy_gate(&db, "consolidate") == crate::autonomy::Rung::Off {
        db.set_setting(PENDING_CONSOLIDATION, "").map_err(err)?;
        return Err(NexusError::Message(
            "Tidying up my memory is turned off in my Self panel.".into(),
        ));
    }

    let proposal: Consolidation = serde_json::from_str(&raw).map_err(err)?;
    mem.snapshot().map_err(NexusError::Message)?;

    for edit in &proposal.edits {
        if mem.update(&db, &edit.name, None, &edit.text).is_ok() {
            let _ = db.log_activity(None, "memory", &format!("tidied {}", edit.name));
        }
    }
    for merge in &proposal.merges {
        // Only drop the sources once the target has actually absorbed them.
        // A model that names a keep that doesn't exist must not delete facts
        // into a merge that never happened.
        if mem.update(&db, &merge.keep, None, &merge.text).is_ok() {
            let _ = db.log_activity(None, "memory", &format!("merged into {}", merge.keep));
            for name in &merge.drop {
                if name == &merge.keep {
                    continue; // never trash the entry we just merged into
                }
                if mem.forget(&db, name).is_ok() {
                    let _ = db.log_activity(None, "memory", &format!("merged away {name}"));
                }
            }
        }
    }
    for name in &proposal.deletes {
        if mem.forget(&db, name).is_ok() {
            let _ = db.log_activity(None, "memory", &format!("dropped {name}"));
        }
    }

    db.set_setting(PENDING_CONSOLIDATION, "").map_err(err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_fenced_proposal() {
        let raw = "```json\n{\"deletes\":[\"a\"],\"edits\":[{\"name\":\"b\",\"text\":\"t\"}]}\n```";
        let p: Consolidation = serde_json::from_str(strip_fence(raw)).unwrap();
        assert_eq!(p.deletes, vec!["a"]);
        assert_eq!(p.edits[0].name, "b");
        assert!(p.merges.is_empty(), "missing keys default to empty");
    }

    #[test]
    fn unparseable_output_proposes_nothing() {
        let p: Consolidation = serde_json::from_str(strip_fence("sure! here you go")).unwrap_or_default();
        assert!(p.deletes.is_empty() && p.edits.is_empty() && p.merges.is_empty());
    }
}
