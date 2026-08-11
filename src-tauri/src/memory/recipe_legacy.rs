//! Recipes, as they existed before Agent Skills (`RCP-1`), and the one-shot
//! migration that turns them into skills (`SKL-5`).
//!
//! Nothing in the running app writes a recipe any more: `propose_recipe` and
//! `use_recipe` are gone, and `memory/recipes/` is no longer a live collection.
//! What remains here is a *reader*, kept so an install that predates skills can
//! carry its procedures forward instead of losing them — and so a user who
//! restores an old backup still gets one clean conversion rather than a folder
//! of files nothing understands.
//!
//! The conversion is deliberately lossless in the direction that matters:
//! `trigger` becomes `when_to_use`, `steps` becomes the body, a `surface`
//! fence becomes a real bundled asset, and the original file is moved to
//! `.trash/` — never deleted — so the user can read what they had.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{parse_entry, slugify, timestamp, MemoryStore};
use crate::db::Db;

/// Where recipes lived. Not in `COLLECTIONS` any more — the folder is read
/// once by `migrate` and then left alone.
pub const RECIPES_DIR: &str = "recipes";

/// The setting that records the migration already ran, so a user who deletes
/// the trashed originals doesn't get them regenerated, and a user who keeps a
/// recipe file around on purpose isn't fought with every launch.
const MIGRATED_KEY: &str = "migrated.recipes_to_skills";

/// A procedure the agent authored and the user approved: markdown steps, plus
/// an optional workspace-surface template a new conversation could start from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    pub name: String,
    pub description: String,
    /// One line: when this recipe applies. Becomes a skill's `when_to_use`.
    pub trigger: String,
    /// YYYY-MM-DD.
    pub created: String,
    pub used: u32,
    pub last_used: Option<String>,
    /// The numbered steps (the body, minus any surface fence).
    pub steps: String,
    /// The `render_ui` tree the workspace started from, if it had one.
    pub surface_json: Option<String>,
}

/// The fence that carried a recipe's workspace template. Found by a plain
/// string scan, not a markdown parser — the contents are JSON, and the only
/// structure that matters is where the fence opens and closes.
const SURFACE_FENCE_OPEN: &str = "\n```surface\n";
const SURFACE_FENCE_CLOSE: &str = "\n```";

/// Split a recipe body into (steps, surface_json).
fn split_surface_fence(body: &str) -> (String, Option<String>) {
    // Normalize so an opening fence on the very first line is still found.
    let padded = format!("\n{body}");
    let Some(open) = padded.find(SURFACE_FENCE_OPEN) else {
        return (body.trim().to_string(), None);
    };
    let after = open + SURFACE_FENCE_OPEN.len();
    let Some(close) = padded[after..].find(SURFACE_FENCE_CLOSE) else {
        // An unterminated fence is a damaged file, not a template. Keep the
        // text as steps rather than swallowing the rest of the recipe.
        return (body.trim().to_string(), None);
    };
    let json = padded[after..after + close].trim().to_string();
    let steps = padded[1..open].trim().to_string();
    (steps, if json.is_empty() { None } else { Some(json) })
}

/// Read a recipe out of its file text.
pub fn parse_recipe(stem: &str, text: &str) -> Recipe {
    let base = parse_entry(stem, text);
    let text = text.replace("\r\n", "\n");
    // Re-scan the header for the recipe-only fields `parse_entry` doesn't know.
    let mut trigger = String::new();
    let mut used = 0u32;
    let mut last_used = None;
    if let Some(rest) = text.strip_prefix("---\n") {
        for line in rest.split('\n').take_while(|l| *l != "---") {
            let Some((key, value)) = line.split_once(':') else { continue };
            let value = value.trim();
            match key.trim() {
                "trigger" => trigger = value.to_string(),
                "used" => used = value.parse().unwrap_or(0),
                "last_used" if !value.is_empty() => last_used = Some(value.to_string()),
                _ => {}
            }
        }
    }
    let (steps, surface_json) = split_surface_fence(&base.body);
    Recipe {
        name: base.name,
        description: base.description,
        trigger,
        created: base.created,
        used,
        last_used,
        steps,
        surface_json,
    }
}

/// The line appended to a converted body when the recipe carried a surface
/// template, so the procedure still says how to open its workspace.
pub const SURFACE_HINT: &str =
    "To start the workspace for this, render `assets/surface.json`.";

/// Turn one recipe into the text of a `SKILL.md`, and the asset that goes
/// beside it. Separated from the file writing so it can be tested without a
/// skills directory.
pub fn to_skill_md(r: &Recipe) -> (String, Option<String>) {
    let mut body = r.steps.trim().to_string();
    if r.surface_json.is_some() {
        if !body.is_empty() {
            body.push_str("\n\n");
        }
        body.push_str(SURFACE_HINT);
    }
    let when = if r.trigger.trim().is_empty() {
        r.description.clone()
    } else {
        r.trigger.clone()
    };
    let text = crate::agent::skillpack::render_skill_md(&r.name, &r.description, &when, &body);
    (text, r.surface_json.clone())
}

/// `SKL-5`: convert every `memory/recipes/*.md` into
/// `<app-data>/skills/<slug>/SKILL.md`, once. Returns how many were converted.
///
/// Runs at startup, before anything reads the skills directory. Failure of a
/// single recipe never aborts the rest — a folder of procedures shouldn't be
/// held hostage by one damaged file — and the flag is only set once the pass
/// completes, so a crash mid-way retries next launch.
pub fn migrate(mem: &MemoryStore, db: &Db, skills_dir: &Path) -> usize {
    if matches!(db.get_setting(MIGRATED_KEY).ok().flatten().as_deref(), Some("true")) {
        return 0;
    }
    let recipes_dir = mem.dir().join(RECIPES_DIR);
    let Ok(entries) = std::fs::read_dir(&recipes_dir) else {
        // No recipes folder at all: a fresh install. Nothing to carry forward.
        let _ = db.set_setting(MIGRATED_KEY, "true");
        return 0;
    };

    let mut converted = 0usize;
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.extension().is_some_and(|x| x == "md") {
            continue;
        }
        let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string()) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let recipe = parse_recipe(&stem, &text);
        let Ok(slug) = slugify(&recipe.name) else { continue };

        let dest = skills_dir.join(&slug);
        if dest.join("SKILL.md").exists() {
            // A skill of that name already exists — almost certainly because a
            // previous partial run wrote it, but possibly the user's own. Never
            // overwrite, and leave the recipe file in place so nothing is lost
            // quietly; the folder is the user's to read either way.
            let _ = db.log_activity(
                None,
                "memory",
                &format!("kept the existing skill {slug} rather than overwriting it with the old recipe"),
            );
            continue;
        }

        let (skill_md, surface) = to_skill_md(&recipe);
        if std::fs::create_dir_all(&dest).is_err() {
            continue;
        }
        if std::fs::write(dest.join("SKILL.md"), &skill_md).is_err() {
            continue;
        }
        if let Some(json) = surface {
            let assets = dest.join("assets");
            if std::fs::create_dir_all(&assets).is_ok() {
                let _ = std::fs::write(assets.join("surface.json"), json);
            }
        }

        // The original moves to `.trash/`, never deleted — same convention
        // `forget_in` uses, so `restore_trash` can put it back.
        let trashed = format!("{}-{}-{}.md", timestamp(), RECIPES_DIR, slug);
        let _ = std::fs::rename(&path, mem.dir().join(".trash").join(&trashed));
        // Its vector and usage rows belong to a collection that no longer
        // exists; leaving them would let a deleted recipe keep scoring in
        // recall.
        let _ = db.delete_vectors_for_ref("memory", RECIPES_DIR, &slug);
        let _ = db.delete_memory_usage(RECIPES_DIR, &slug);
        converted += 1;
    }

    if converted > 0 {
        let _ = db.log_activity(
            None,
            "memory",
            &format!("turned {converted} saved procedures into skills"),
        );
    }
    let _ = db.set_setting(MIGRATED_KEY, "true");
    // The index still names the recipes it just moved out from under.
    mem.write_index();
    mem.sync_fts(db);
    converted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (MemoryStore, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let mem = MemoryStore::new(tmp.path()).unwrap();
        std::fs::create_dir_all(mem.dir().join(RECIPES_DIR)).unwrap();
        (mem, tmp)
    }

    fn write_recipe(mem: &MemoryStore, slug: &str, body: &str) {
        std::fs::write(mem.dir().join(RECIPES_DIR).join(format!("{slug}.md")), body).unwrap();
    }

    const SAMPLE: &str = "---\nname: weekly-report\ndescription: Write the weekly status report.\ntype: recipe\ntrigger: it's Friday afternoon\ncreated: 2026-01-02\nused: 4\n---\n1. Gather updates.\n2. Write it up.\n";

    #[test]
    fn trigger_becomes_when_to_use_and_steps_become_the_body() {
        let r = parse_recipe("weekly-report", SAMPLE);
        let (md, surface) = to_skill_md(&r);
        assert!(surface.is_none());
        assert!(md.contains("when_to_use: it's Friday afternoon"), "got:\n{md}");
        assert!(md.contains("description: Write the weekly status report."));
        assert!(md.contains("1. Gather updates."));
    }

    #[test]
    fn a_surface_fence_becomes_a_bundled_asset_and_a_closing_line() {
        let text = format!("{SAMPLE}\n```surface\n{{\"type\":\"stack\"}}\n```\n");
        let r = parse_recipe("weekly-report", &text);
        let (md, surface) = to_skill_md(&r);
        assert_eq!(surface.as_deref(), Some("{\"type\":\"stack\"}"));
        assert!(md.trim_end().ends_with(SURFACE_HINT), "got:\n{md}");
        // The fence itself must not survive into the skill body.
        assert!(!md.contains("```surface"));
    }

    /// Moved here with `split_surface_fence` itself: a damaged fence in an old
    /// file must not swallow the steps on the way into a skill.
    #[test]
    fn an_unterminated_fence_stays_readable_as_steps() {
        let r = parse_recipe(
            "broken",
            "---\nname: broken\ntrigger: t\n---\n1. Do it.\n```surface\n{\"kind\":\"stack\"}\n",
        );
        assert_eq!(r.surface_json, None);
        assert!(r.steps.contains("1. Do it."), "the steps aren't swallowed");
    }

    #[test]
    fn migrate_writes_a_skill_trashes_the_original_and_runs_once() {
        let (mem, tmp) = store();
        let db = Db::open_in_memory().unwrap();
        let skills = tmp.path().join("skills");
        write_recipe(&mem, "weekly-report", SAMPLE);

        assert_eq!(migrate(&mem, &db, &skills), 1);
        let written = std::fs::read_to_string(skills.join("weekly-report").join("SKILL.md")).unwrap();
        assert!(written.contains("when_to_use: it's Friday afternoon"));
        assert!(!mem.dir().join(RECIPES_DIR).join("weekly-report.md").exists(), "original should be moved");
        assert_eq!(
            std::fs::read_dir(mem.dir().join(".trash")).unwrap().count(),
            1,
            "the original must be recoverable, not deleted"
        );

        // Second run is a no-op: the flag, not the empty folder, is what stops it.
        write_recipe(&mem, "another", SAMPLE.replace("weekly-report", "another").as_str());
        assert_eq!(migrate(&mem, &db, &skills), 0);
    }

    #[test]
    fn an_existing_skill_of_the_same_name_is_never_overwritten() {
        let (mem, tmp) = store();
        let db = Db::open_in_memory().unwrap();
        let skills = tmp.path().join("skills");
        std::fs::create_dir_all(skills.join("weekly-report")).unwrap();
        std::fs::write(skills.join("weekly-report").join("SKILL.md"), "mine, hands off").unwrap();
        write_recipe(&mem, "weekly-report", SAMPLE);

        assert_eq!(migrate(&mem, &db, &skills), 0);
        let kept = std::fs::read_to_string(skills.join("weekly-report").join("SKILL.md")).unwrap();
        assert_eq!(kept, "mine, hands off");
        assert!(
            mem.dir().join(RECIPES_DIR).join("weekly-report.md").exists(),
            "a recipe that couldn't be converted stays where the user can see it"
        );
    }

    #[test]
    fn a_damaged_recipe_doesnt_stop_the_rest() {
        let (mem, tmp) = store();
        let db = Db::open_in_memory().unwrap();
        let skills = tmp.path().join("skills");
        write_recipe(&mem, "broken", "");
        write_recipe(&mem, "weekly-report", SAMPLE);
        // The empty file still parses (falling back to its stem), so both convert.
        assert_eq!(migrate(&mem, &db, &skills), 2);
        assert!(skills.join("weekly-report").join("SKILL.md").exists());
    }
}
