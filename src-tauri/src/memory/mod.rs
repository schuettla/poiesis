//! The durable self (MEM-1): plain markdown files on disk that outlive any one
//! conversation.
//!
//! Two commitments shape everything here. **The user owns this** — it is a
//! folder of readable markdown they can edit in Notepad, not an opaque blob in
//! a database. And **nothing is destroyed** — "forgetting" moves a file to
//! `.trash/`, consolidation snapshots first, unparseable files are set aside
//! rather than dropped. The model never rewrites files wholesale; it calls
//! narrow verbs and this module owns the layout and the index.
//!
//! ```text
//! memory/
//! ├─ MEMORY.md      generated index — never hand-edited, never model-edited
//! ├─ SOUL.md        standing instructions; user-edited, agent only proposes
//! ├─ facts/         durable facts about the user
//! ├─ lessons/       reflection output (11A) — same format, kind: lesson
//! ├─ recipes/       approved procedures (11C)
//! ├─ .trash/        forgotten entries (recoverable)
//! ├─ .quarantine/   unparseable files moved aside (recoverable)
//! └─ .snapshots/    pre-consolidation copies, timestamped
//! ```

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::db::Db;

/// Collections that hold entry files. Facts land in v1; lessons and recipes
/// are written by Part IV but share this module's parser, slugger, and trash.
pub const FACTS: &str = "facts";
pub const LESSONS: &str = "lessons";
pub const RECIPES: &str = "recipes";
const COLLECTIONS: [&str; 3] = [FACTS, LESSONS, RECIPES];

/// A fact must stay short enough to sit in every prompt without crowding out
/// the conversation. Long content belongs in a file or artifact.
const BODY_CAP: usize = 1500;
/// Standing instructions ride in every prompt, so they stay short. Public so a
/// soul *proposal* can be rejected before it's stored, not only at accept time.
pub const SOUL_CAP: usize = 1500;
/// Per-section caps on the always-injected index.
const INDEX_CAP_FACTS: usize = 2000;
const INDEX_CAP_LESSONS: usize = 1000;
const INDEX_CAP_RECIPES: usize = 800;
const SLUG_MAX: usize = 64;
/// A lesson is one behavioural correction, not an essay (REF-1).
const LESSON_BODY_CAP: usize = 600;
/// How many lessons are kept live. Reflection writes these unprompted, so the
/// collection needs a ceiling the user never has to enforce by hand.
const LESSON_CAP: usize = 40;
/// A recipe is a procedure, not a manual (RCP-1).
pub const RECIPE_STEPS_CAP: usize = 2000;

/// One durable entry — a fact, a lesson, or a recipe. Backed 1:1 by a markdown
/// file with a frontmatter header.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    /// Slug; also the file stem.
    pub name: String,
    /// One line, shown in the always-injected index.
    pub description: String,
    /// preference | fact | decision | project — or "lesson" / "recipe".
    pub kind: String,
    /// YYYY-MM-DD.
    pub created: String,
    pub source_conversation: Option<String>,
    pub body: String,
}

pub struct MemoryStore {
    dir: PathBuf,
    /// Serializes mutations so two concurrent writes can't race on the same
    /// file (e.g. a save and a forget of one slug). The index is rebuilt from
    /// disk after the guard drops, so it always converges on the final state.
    lock: Mutex<()>,
}

/// Lowercase, non-alphanumeric runs collapse to `-`, trimmed, capped.
pub fn slugify(input: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in input.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch);
            if out.chars().count() >= SLUG_MAX {
                break;
            }
        } else {
            pending_dash = true;
        }
    }
    if out.is_empty() {
        return Err("that name has no letters or digits in it".into());
    }
    Ok(out)
}

/// Today as YYYY-MM-DD, from the system clock.
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    civil_date(secs / 86_400)
}

/// Days since the Unix epoch → `YYYY-MM-DD` (Howard Hinnant's civil_from_days).
fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Parse a `---` frontmatter header plus body. Deliberately forgiving: no YAML
/// crate, unknown keys ignored, CRLF tolerated, missing description falls back
/// to the first body line. A file a human hand-edited should still load.
fn parse_entry(name: &str, text: &str) -> Fact {
    let text = text.replace("\r\n", "\n");
    let mut header = String::new();
    let mut body = text.clone();

    // A leading `---` line opens frontmatter; the closer is the next line that
    // is exactly `---`. Splitting on lines handles an empty header (`---\n---`)
    // that a byte-offset `find("\n---")` misses.
    if let Some(rest) = text.strip_prefix("---\n") {
        let lines: Vec<&str> = rest.split('\n').collect();
        if let Some(close) = lines.iter().position(|l| *l == "---") {
            header = lines[..close].join("\n");
            body = lines[close + 1..].join("\n");
        }
    }

    let mut description = String::new();
    let mut kind = String::new();
    let mut created = String::new();
    let mut source_conversation = None;
    let mut file_name = name.to_string();

    for line in header.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim().to_string();
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "name" => file_name = value,
            "description" => description = value,
            // `type` is the on-disk spelling; `kind` accepted for tolerance.
            "type" | "kind" => kind = value,
            "created" => created = value,
            "source_conversation" => source_conversation = Some(value),
            _ => {}
        }
    }

    let body = body.trim().to_string();
    if description.is_empty() {
        description = body.lines().next().unwrap_or("").trim().to_string();
    }

    Fact {
        name: file_name,
        description,
        kind: if kind.is_empty() { "fact".into() } else { kind },
        created,
        source_conversation,
        body,
    }
}

/// Collapse a header value to one line. A frontmatter field is line-delimited,
/// so a stray newline in a model-supplied `description` would corrupt the file
/// on the next read; flatten it here rather than trust the caller.
fn one_line(s: &str) -> String {
    s.replace(['\r', '\n'], " ").trim().to_string()
}

/// A procedure the agent authored and the user approved (RCP-1): markdown
/// steps, plus an optional workspace-surface template a new conversation can
/// start from. Same folder, same slug rules, richer frontmatter than a fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    pub name: String,
    pub description: String,
    /// One line: when this recipe applies.
    pub trigger: String,
    /// YYYY-MM-DD.
    pub created: String,
    /// How often it has actually been used — a recipe earns its place.
    pub used: u32,
    pub last_used: Option<String>,
    /// The numbered steps (the body, minus any surface fence).
    pub steps: String,
    /// The `render_ui` tree the workspace starts from, if the recipe has one.
    pub surface_json: Option<String>,
}

/// The fence that carries a recipe's workspace template. Found by a plain
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

/// Read a recipe out of its file text. Public so a stored *proposal* — which
/// holds the exact future file content — can be turned back into a `Recipe` on
/// accept, using the same parser that reads it from disk afterwards.
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

pub fn render_recipe(r: &Recipe) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("name: {}\n", one_line(&r.name)));
    out.push_str(&format!("description: {}\n", one_line(&r.description)));
    out.push_str("type: recipe\n");
    out.push_str(&format!("trigger: {}\n", one_line(&r.trigger)));
    out.push_str(&format!("created: {}\n", one_line(&r.created)));
    out.push_str(&format!("used: {}\n", r.used));
    if let Some(last) = &r.last_used {
        out.push_str(&format!("last_used: {}\n", one_line(last)));
    }
    out.push_str("---\n");
    out.push_str(r.steps.trim());
    out.push('\n');
    if let Some(json) = &r.surface_json {
        if !json.trim().is_empty() {
            out.push_str("\n```surface\n");
            out.push_str(json.trim());
            out.push_str("\n```\n");
        }
    }
    out
}

/// Read back a `<timestamp>-<collection>-<name>.md` filename, as written into
/// `.trash/` and `.quarantine/`. Refuses anything that could climb out of those
/// folders: the name is ours, but it arrives from the frontend.
fn parse_aside_name(file: &str) -> Result<(String, String), String> {
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return Err("that isn't a set-aside entry".into());
    }
    let stem = file.strip_suffix(".md").unwrap_or(file);
    let mut parts = stem.splitn(3, '-');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(_ts), Some(c), Some(n)) if COLLECTIONS.contains(&c) => {
            Ok((c.to_string(), n.to_string()))
        }
        _ => Err("that isn't a set-aside entry".into()),
    }
}

fn render_entry(f: &Fact) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("name: {}\n", one_line(&f.name)));
    out.push_str(&format!("description: {}\n", one_line(&f.description)));
    out.push_str(&format!("type: {}\n", one_line(&f.kind)));
    out.push_str(&format!("created: {}\n", one_line(&f.created)));
    if let Some(src) = &f.source_conversation {
        out.push_str(&format!("source_conversation: {}\n", one_line(src)));
    }
    out.push_str("---\n");
    out.push_str(f.body.trim());
    out.push('\n');
    out
}

impl MemoryStore {
    pub fn new(app_data: &Path) -> std::io::Result<Self> {
        let dir = app_data.join("memory");
        for sub in COLLECTIONS.iter().chain([".trash", ".quarantine", ".snapshots"].iter()) {
            std::fs::create_dir_all(dir.join(sub))?;
        }
        let store = MemoryStore {
            dir,
            lock: Mutex::new(()),
        };
        // A fresh install still gets an index file, so the folder explains itself.
        store.write_index();
        Ok(store)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn dir_for(&self, collection: &str) -> PathBuf {
        self.dir.join(collection)
    }

    fn path_for(&self, collection: &str, name: &str) -> PathBuf {
        self.dir_for(collection).join(format!("{name}.md"))
    }

    /// Every entry in a collection, newest first.
    ///
    /// A file this module cannot make sense of — unreadable bytes, or nothing
    /// usable after parsing — is **moved aside** to `.quarantine/` (HEAL-3)
    /// rather than silently skipped or deleted. One damaged file must never
    /// break the whole self, and must never vanish either.
    pub fn list_in(&self, collection: &str) -> Vec<Fact> {
        let Ok(entries) = std::fs::read_dir(self.dir_for(collection)) else {
            return Vec::new();
        };
        let mut out: Vec<Fact> = Vec::new();
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.extension().is_some_and(|x| x == "md") {
                continue;
            }
            let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string()) else {
                continue;
            };
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let fact = parse_entry(&stem, &text);
                    // Nothing to show and nothing to inject: not an entry.
                    if fact.body.trim().is_empty() && fact.description.trim().is_empty() {
                        self.quarantine(&path, collection, &stem);
                        continue;
                    }
                    out.push(fact);
                }
                Err(_) => self.quarantine(&path, collection, &stem),
            }
        }
        // `created` is YYYY-MM-DD, so lexicographic order is chronological.
        out.sort_by(|a, b| b.created.cmp(&a.created).then(a.name.cmp(&b.name)));
        out
    }

    /// Move one damaged file into `.quarantine/`. Best-effort and silent: the
    /// user-visible record is made by `quarantine_scan`, which has a Db.
    fn quarantine(&self, path: &Path, collection: &str, stem: &str) {
        let dest = self
            .dir
            .join(".quarantine")
            .join(format!("{}-{collection}-{stem}.md", timestamp()));
        let _ = std::fs::rename(path, dest);
    }

    /// Files currently set aside as unreadable, newest first (HEAL-3 / ORG-1).
    pub fn quarantined(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.dir.join(".quarantine")) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort_by(|a, b| b.cmp(a));
        names
    }

    /// Walk every collection so damaged files are set aside, and log whatever
    /// moved. Run at startup and whenever the Health tab is read.
    pub fn quarantine_scan(&self, db: &Db) {
        let before = self.quarantined();
        for collection in COLLECTIONS {
            let _ = self.list_in(collection);
        }
        for file in self.quarantined() {
            if !before.contains(&file) {
                let _ = db.log_activity(None, "heal", &format!("quarantined {file}"));
            }
        }
    }

    /// Put a quarantined file back where it came from — the user presumably
    /// fixed it in an editor. The filename encodes its collection and slug.
    pub fn restore_quarantined(&self, db: &Db, file: &str) -> Result<(), String> {
        let (collection, name) = parse_aside_name(file)?;
        let _guard = self.lock.lock().unwrap();
        let dest = self.path_for(&collection, &name);
        if dest.exists() {
            return Err(format!("a memory named {name} exists again — rename it first"));
        }
        std::fs::rename(self.dir.join(".quarantine").join(file), dest).map_err(|e| e.to_string())?;
        drop(_guard);
        self.write_index();
        self.sync_fts(db);
        Ok(())
    }

    /// Discard a quarantined file for good — the one place in this module that
    /// really deletes, and only ever on an explicit user action.
    pub fn delete_quarantined(&self, file: &str) -> Result<(), String> {
        parse_aside_name(file)?;
        std::fs::remove_file(self.dir.join(".quarantine").join(file)).map_err(|e| e.to_string())
    }

    pub fn list(&self) -> Vec<Fact> {
        self.list_in(FACTS)
    }

    pub fn read_in(&self, collection: &str, name: &str) -> Option<Fact> {
        let name = slugify(name).ok()?;
        let text = std::fs::read_to_string(self.path_for(collection, &name)).ok()?;
        Some(parse_entry(&name, &text))
    }

    pub fn read(&self, name: &str) -> Option<Fact> {
        self.read_in(FACTS, name)
    }

    /// Write a new entry. Refuses to overwrite: an existing slug means the
    /// model should be updating, not silently replacing what's there.
    pub fn save_in(&self, db: &Db, collection: &str, f: &Fact) -> Result<String, String> {
        if f.body.chars().count() > BODY_CAP {
            return Err("keep facts short; put long content in a file or artifact".into());
        }
        let name = slugify(&f.name)?;
        let _guard = self.lock.lock().unwrap();
        let path = self.path_for(collection, &name);
        if path.exists() {
            return Err(format!("a fact named {name} exists; use op:update"));
        }
        let entry = Fact {
            name: name.clone(),
            created: if f.created.is_empty() { today() } else { f.created.clone() },
            ..f.clone()
        };
        std::fs::write(&path, render_entry(&entry)).map_err(|e| e.to_string())?;
        drop(_guard);
        self.write_index();
        self.sync_fts(db);
        Ok(name)
    }

    pub fn save(&self, db: &Db, f: &Fact) -> Result<String, String> {
        self.save_in(db, FACTS, f)
    }

    /// Rewrite an entry's body, and optionally its one-line description.
    /// Everything else (kind, created, source) is preserved.
    pub fn update_in(
        &self,
        db: &Db,
        collection: &str,
        name: &str,
        description: Option<&str>,
        body: &str,
    ) -> Result<(), String> {
        if body.chars().count() > BODY_CAP {
            return Err("keep facts short; put long content in a file or artifact".into());
        }
        let name = slugify(name)?;
        let _guard = self.lock.lock().unwrap();
        let path = self.path_for(collection, &name);
        let text = std::fs::read_to_string(&path).map_err(|_| format!("no memory named {name}"))?;
        let mut entry = parse_entry(&name, &text);
        entry.body = body.trim().to_string();
        if let Some(d) = description {
            if !d.trim().is_empty() {
                entry.description = d.trim().to_string();
            }
        }
        std::fs::write(&path, render_entry(&entry)).map_err(|e| e.to_string())?;
        drop(_guard);
        self.write_index();
        self.sync_fts(db);
        Ok(())
    }

    pub fn update(
        &self,
        db: &Db,
        name: &str,
        description: Option<&str>,
        body: &str,
    ) -> Result<(), String> {
        self.update_in(db, FACTS, name, description, body)
    }

    /// Move an entry to `.trash/` — recoverable, never deleted outright.
    /// Returns the trash filename, which `restore_trash` takes back.
    pub fn forget_in(&self, db: &Db, collection: &str, name: &str) -> Result<String, String> {
        let name = slugify(name)?;
        let _guard = self.lock.lock().unwrap();
        let path = self.path_for(collection, &name);
        if !path.exists() {
            return Err(format!("no memory named {name}"));
        }
        let filename = format!("{}-{}-{}.md", timestamp(), collection, name);
        std::fs::rename(&path, self.dir.join(".trash").join(&filename))
            .map_err(|e| e.to_string())?;
        drop(_guard);
        self.write_index();
        self.sync_fts(db);
        Ok(filename)
    }

    pub fn forget(&self, db: &Db, name: &str) -> Result<String, String> {
        self.forget_in(db, FACTS, name)
    }

    /// Undo a `forget`, using the filename it returned.
    pub fn restore_trash(&self, db: &Db, file: &str) -> Result<(), String> {
        let (collection, name) = parse_aside_name(file)?;
        let _guard = self.lock.lock().unwrap();
        let src = self.dir.join(".trash").join(file);
        // Never clobber a live entry: since the forget, the agent may have saved
        // a new entry under this slug. Restoring must not destroy it.
        let dest = self.path_for(&collection, &name);
        if dest.exists() {
            return Err(format!(
                "a memory named {name} exists again — remove or rename it before restoring"
            ));
        }
        std::fs::rename(&src, dest).map_err(|e| e.to_string())?;
        drop(_guard);
        self.write_index();
        self.sync_fts(db);
        Ok(())
    }

    // ---- lessons (REF-1): what the agent learned about its own working ----

    pub fn list_lessons(&self) -> Vec<Fact> {
        self.list_in(LESSONS)
    }

    /// Save a lesson, then prune the collection back to `LESSON_CAP`.
    ///
    /// Lessons accumulate on their own (reflection writes them without being
    /// asked), so unlike facts they need a ceiling. Pruning moves the oldest to
    /// `.trash/` rather than deleting: they stay recoverable, and their text is
    /// still findable through the conversation they came from.
    pub fn save_lesson(&self, db: &Db, f: &Fact) -> Result<String, String> {
        if f.body.chars().count() > LESSON_BODY_CAP {
            return Err(format!("a lesson must stay under {LESSON_BODY_CAP} characters"));
        }
        let name = self.save_in(
            db,
            LESSONS,
            &Fact {
                kind: "lesson".to_string(),
                ..f.clone()
            },
        )?;
        self.prune_lessons(db);
        Ok(name)
    }

    pub fn forget_lesson(&self, db: &Db, name: &str) -> Result<String, String> {
        self.forget_in(db, LESSONS, name)
    }

    /// Trash everything past `LESSON_CAP`, oldest first. Best-effort: a failed
    /// prune leaves an over-full collection, which is harmless.
    fn prune_lessons(&self, db: &Db) {
        let lessons = self.list_in(LESSONS);
        if lessons.len() <= LESSON_CAP {
            return;
        }
        // `list_in` is newest-first, so the tail is the oldest.
        for old in &lessons[LESSON_CAP..] {
            if self.forget_in(db, LESSONS, &old.name).is_ok() {
                let _ = db.log_activity(
                    None,
                    "reflect",
                    &format!("pruned an old lesson: {}", old.name),
                );
            }
        }
    }

    // ---- recipes (RCP-1): procedures the agent developed with the user ----

    /// Every recipe, newest first. Goes through `list_in` so a damaged recipe
    /// file is quarantined on the same path as any other entry.
    pub fn list_recipes(&self) -> Vec<Recipe> {
        self.list_in(RECIPES)
            .iter()
            .filter_map(|f| self.read_recipe(&f.name))
            .collect()
    }

    pub fn read_recipe(&self, name: &str) -> Option<Recipe> {
        let name = slugify(name).ok()?;
        let text = std::fs::read_to_string(self.path_for(RECIPES, &name)).ok()?;
        Some(parse_recipe(&name, &text))
    }

    /// Write a recipe. Unlike facts, a recipe may be re-saved: accepting a
    /// proposal for a slug that exists is an intentional revision, so this
    /// overwrites rather than refusing — the previous version is superseded,
    /// and `created`/`used` carry over from what was on disk.
    pub fn save_recipe(&self, db: &Db, r: &Recipe) -> Result<String, String> {
        if r.steps.chars().count() > RECIPE_STEPS_CAP {
            return Err(format!("keep the steps under {RECIPE_STEPS_CAP} characters"));
        }
        if let Some(json) = &r.surface_json {
            if !json.trim().is_empty() {
                serde_json::from_str::<serde_json::Value>(json)
                    .map_err(|e| format!("the surface template isn't valid JSON: {e}"))?;
            }
        }
        let name = slugify(&r.name)?;
        let existing = self.read_recipe(&name);
        let entry = Recipe {
            name: name.clone(),
            created: match (&existing, r.created.is_empty()) {
                (Some(prev), _) => prev.created.clone(),
                (None, true) => today(),
                (None, false) => r.created.clone(),
            },
            used: existing.as_ref().map(|p| p.used).unwrap_or(r.used),
            last_used: existing.and_then(|p| p.last_used).or_else(|| r.last_used.clone()),
            ..r.clone()
        };

        let _guard = self.lock.lock().unwrap();
        std::fs::write(self.path_for(RECIPES, &name), render_recipe(&entry))
            .map_err(|e| e.to_string())?;
        drop(_guard);
        self.write_index();
        self.sync_fts(db);
        Ok(name)
    }

    /// Record one use. Silent by design (rung R0): counting how often a
    /// procedure earns its keep is not a change the user needs to consent to.
    pub fn touch_recipe(&self, db: &Db, name: &str) -> Result<(), String> {
        let name = slugify(name)?;
        let mut recipe = self
            .read_recipe(&name)
            .ok_or_else(|| format!("no recipe named {name}"))?;
        recipe.used += 1;
        recipe.last_used = Some(today());
        let _guard = self.lock.lock().unwrap();
        std::fs::write(self.path_for(RECIPES, &name), render_recipe(&recipe))
            .map_err(|e| e.to_string())?;
        drop(_guard);
        // The index carries `(used N×)` into every system prompt, so a use that
        // doesn't refresh it leaves the model reading a stale count.
        self.write_index();
        self.sync_fts(db);
        Ok(())
    }

    pub fn forget_recipe(&self, db: &Db, name: &str) -> Result<String, String> {
        self.forget_in(db, RECIPES, name)
    }

    /// One index line per entry: `- [name] (type) description`.
    fn index_section(entries: &[Fact], cap: usize) -> String {
        let mut lines = Vec::new();
        let mut used = 0usize;
        let mut dropped = 0usize;
        for f in entries {
            let line = format!("- [{}] ({}) {}\n", f.name, f.kind, f.description);
            // Count in chars, like BODY_CAP/SOUL_CAP — the caps are a budget on
            // prompt length, not on UTF-8 byte size.
            let cost = line.chars().count();
            if used + cost > cap {
                dropped += 1;
                continue;
            }
            used += cost;
            lines.push(line);
        }
        let mut out: String = lines.concat();
        if dropped > 0 {
            out.push_str(&format!(
                "- …and {dropped} older entries (search_history finds them)\n"
            ));
        }
        out
    }

    /// Regenerate the always-injected index. Sections for lessons and recipes
    /// appear on their own once Part IV starts writing those collections.
    pub fn index_markdown(&self) -> String {
        let mut out = String::new();

        let facts = self.list_in(FACTS);
        if !facts.is_empty() {
            out.push_str(&Self::index_section(&facts, INDEX_CAP_FACTS));
        }

        let lessons = self.list_in(LESSONS);
        if !lessons.is_empty() {
            out.push_str("\n## Lessons (things you learned from your own mistakes)\n");
            out.push_str(&Self::index_section(&lessons, INDEX_CAP_LESSONS));
        }

        // Recipes index on their trigger, not their description: the model's
        // question is "does this situation call for one?", and `use_recipe`
        // fetches the steps once the answer is yes.
        let recipes = self.list_recipes();
        if !recipes.is_empty() {
            out.push_str("\n## Recipes (procedures you may reuse — read with use_recipe first)\n");
            let as_entries: Vec<Fact> = recipes
                .iter()
                .map(|r| Fact {
                    name: r.name.clone(),
                    description: format!("when: {}", r.trigger),
                    kind: format!("used {}×", r.used),
                    created: r.created.clone(),
                    source_conversation: None,
                    body: String::new(),
                })
                .collect();
            out.push_str(&Self::index_section(&as_entries, INDEX_CAP_RECIPES));
        }

        out
    }

    pub fn write_index(&self) {
        let body = self.index_markdown();
        let text = if body.trim().is_empty() {
            "# Memory Index\n\nNothing saved yet.\n".to_string()
        } else {
            format!("# Memory Index\n\n{body}")
        };
        let _ = std::fs::write(self.dir.join("MEMORY.md"), text);
    }

    // ---- SOUL.md: standing instructions the user owns ----

    pub fn soul(&self) -> String {
        std::fs::read_to_string(self.dir.join("SOUL.md")).unwrap_or_default()
    }

    pub fn set_soul(&self, text: &str) -> Result<(), String> {
        if text.chars().count() > SOUL_CAP {
            return Err(format!("standing instructions must stay under {SOUL_CAP} characters"));
        }
        let _guard = self.lock.lock().unwrap();
        std::fs::write(self.dir.join("SOUL.md"), text).map_err(|e| e.to_string())
    }

    /// Copy the whole self aside before a bulk change (consolidation). Returns
    /// the snapshot directory name.
    pub fn snapshot(&self) -> Result<String, String> {
        let name = timestamp().to_string();
        let dest = self.dir.join(".snapshots").join(&name);
        std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        for collection in COLLECTIONS {
            let target = dest.join(collection);
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            if let Ok(entries) = std::fs::read_dir(self.dir_for(collection)) {
                for entry in entries.filter_map(|e| e.ok()) {
                    if let Some(file) = entry.path().file_name() {
                        let _ = std::fs::copy(entry.path(), target.join(file));
                    }
                }
            }
        }
        for file in ["MEMORY.md", "SOUL.md"] {
            let _ = std::fs::copy(self.dir.join(file), dest.join(file));
        }
        Ok(name)
    }

    /// Rebuild the memory FTS index wholesale. At entry-count scale (tens, not
    /// thousands) this is cheaper than maintaining incremental correctness.
    pub fn sync_fts(&self, db: &Db) {
        let mut rows = Vec::new();
        for collection in COLLECTIONS {
            for f in self.list_in(collection) {
                rows.push((f.name, f.description, f.body, f.kind));
            }
        }
        let _ = db.replace_memory_fts(&rows);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (MemoryStore, Db, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(tmp.path()).unwrap();
        (store, Db::open_in_memory().unwrap(), tmp)
    }

    fn fact(name: &str, body: &str) -> Fact {
        Fact {
            name: name.into(),
            description: "a description".into(),
            kind: "preference".into(),
            created: "2026-07-16".into(),
            source_conversation: Some("conv-1".into()),
            body: body.into(),
        }
    }

    #[test]
    fn slug_rule() {
        assert_eq!(slugify("Prefers Metric Units").unwrap(), "prefers-metric-units");
        assert_eq!(slugify("  hello___world!! ").unwrap(), "hello-world");
        assert_eq!(slugify("Über").unwrap(), "ber", "non-ascii is dropped, not kept");
        assert_eq!(slugify("a".repeat(200).as_str()).unwrap().len(), SLUG_MAX);
        assert!(slugify("!!!").is_err(), "a name with nothing in it is refused");
        assert!(slugify("").is_err());
    }

    #[test]
    fn save_update_forget_round_trip() {
        let (s, db, _tmp) = store();

        let name = s.save(&db, &fact("Prefers Metric", "Always metric.")).unwrap();
        assert_eq!(name, "prefers-metric");
        let got = s.read("prefers-metric").unwrap();
        assert_eq!(got.body, "Always metric.");
        assert_eq!(got.kind, "preference");
        assert_eq!(got.source_conversation.as_deref(), Some("conv-1"));

        // A second save under the same slug must not clobber the first.
        let err = s.save(&db, &fact("prefers-metric", "different")).unwrap_err();
        assert!(err.contains("use op:update"), "got {err}");
        assert_eq!(s.read("prefers-metric").unwrap().body, "Always metric.");

        s.update(&db, "prefers-metric", Some("new description"), "Metric, confirmed twice.")
            .unwrap();
        let got = s.read("prefers-metric").unwrap();
        assert_eq!(got.body, "Metric, confirmed twice.");
        assert_eq!(got.description, "new description");
        assert_eq!(got.created, "2026-07-16", "update preserves provenance");

        // Forgetting is recoverable.
        let trashed = s.forget(&db, "prefers-metric").unwrap();
        assert!(s.read("prefers-metric").is_none());
        assert!(s.list().is_empty());
        s.restore_trash(&db, &trashed).unwrap();
        assert_eq!(s.read("prefers-metric").unwrap().body, "Metric, confirmed twice.");

        assert!(s.update(&db, "nope", None, "x").is_err());
        assert!(s.forget(&db, "nope").is_err());
        assert!(s.restore_trash(&db, "../../etc/passwd").is_err());
    }

    #[test]
    fn rejects_oversized_bodies() {
        let (s, db, _tmp) = store();
        let err = s.save(&db, &fact("big", &"x".repeat(BODY_CAP + 1))).unwrap_err();
        assert!(err.contains("keep facts short"), "got {err}");
        assert!(s.save(&db, &fact("ok", &"x".repeat(BODY_CAP))).is_ok());
    }

    #[test]
    fn index_is_capped_and_reports_the_remainder() {
        let (s, db, _tmp) = store();
        for i in 0..120 {
            let mut f = fact(&format!("fact-{i:03}"), "body");
            f.description = "d".repeat(60);
            s.save(&db, &f).unwrap();
        }
        let index = s.index_markdown();
        assert!(index.len() <= INDEX_CAP_FACTS + 80, "index length {}", index.len());
        assert!(index.contains("older entries (search_history finds them)"));
    }

    #[test]
    fn frontmatter_is_forgiving() {
        let (s, _db, _tmp) = store();
        // Extra keys, CRLF line endings, and out-of-order fields all survive.
        std::fs::write(
            s.dir().join("facts").join("hand-written.md"),
            "---\r\nname: hand-written\r\ntype: decision\r\nmystery: 42\r\ncreated: 2026-01-02\r\n---\r\nWe chose SQLite.\r\n",
        )
        .unwrap();
        let got = s.read("hand-written").unwrap();
        assert_eq!(got.kind, "decision");
        assert_eq!(got.body, "We chose SQLite.");
        assert_eq!(got.description, "We chose SQLite.", "falls back to the first body line");

        // No frontmatter at all is still readable, not lost.
        std::fs::write(s.dir().join("facts").join("bare.md"), "just a note").unwrap();
        let bare = s.read("bare").unwrap();
        assert_eq!(bare.body, "just a note");
        assert_eq!(bare.kind, "fact");

        // An empty frontmatter block must not swallow the whole file as body.
        std::fs::write(s.dir().join("facts").join("empty-fm.md"), "---\n---\nthe body\n").unwrap();
        assert_eq!(s.read("empty-fm").unwrap().body, "the body");
    }

    #[test]
    fn multiline_description_cannot_corrupt_the_header() {
        let (s, db, _tmp) = store();
        let mut f = fact("notes", "body");
        f.description = "line one\nname: injected\nline two".into();
        s.save(&db, &f).unwrap();
        // Round-trips as one field; the smuggled `name:` doesn't become the slug.
        let got = s.read("notes").unwrap();
        assert_eq!(got.name, "notes");
        assert!(!got.description.contains('\n'));
        assert!(got.description.starts_with("line one"));
    }

    #[test]
    fn restore_wont_clobber_a_reused_slug() {
        let (s, db, _tmp) = store();
        let trashed = {
            s.save(&db, &fact("greeting", "first")).unwrap();
            s.forget(&db, "greeting").unwrap()
        };
        // The slug is live again with different content.
        s.save(&db, &fact("greeting", "second")).unwrap();
        // Restoring must refuse rather than destroy the live "second".
        assert!(s.restore_trash(&db, &trashed).is_err());
        assert_eq!(s.read("greeting").unwrap().body, "second");
    }

    #[test]
    fn soul_round_trips_and_is_capped() {
        let (s, _db, _tmp) = store();
        assert_eq!(s.soul(), "");
        s.set_soul("Always answer in metric.").unwrap();
        assert_eq!(s.soul(), "Always answer in metric.");
        assert!(s.set_soul(&"x".repeat(SOUL_CAP + 1)).is_err());
    }

    #[test]
    fn sync_fts_makes_entries_searchable() {
        let (s, db, _tmp) = store();
        s.save(&db, &fact("prefers-metric", "Always give measurements in metric."))
            .unwrap();
        let hits = db.search_memory_fts("metric", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "prefers-metric");

        // Forgetting removes it from search too.
        s.forget(&db, "prefers-metric").unwrap();
        assert!(db.search_memory_fts("metric", 5).unwrap().is_empty());
    }

    #[test]
    fn lessons_are_capped_and_the_oldest_are_trashed_not_deleted() {
        let (s, db, _tmp) = store();
        // 45 lessons, oldest first by `created` — day 01 is the oldest.
        for i in 0..45 {
            let mut f = fact(&format!("lesson-{i:02}"), "check the path exists first");
            f.created = format!("2026-01-{:02}", (i % 28) + 1);
            s.save_lesson(&db, &f).unwrap();
        }
        let live = s.list_lessons();
        assert_eq!(live.len(), LESSON_CAP, "the collection is held at its ceiling");
        assert!(live.iter().all(|l| l.kind == "lesson"), "kind is forced, not trusted");
        // The pruned ones are recoverable, not gone.
        let trashed = std::fs::read_dir(s.dir().join(".trash")).unwrap().count();
        assert_eq!(trashed, 45 - LESSON_CAP);

        let mut long = fact("too-long", &"x".repeat(LESSON_BODY_CAP + 1));
        long.created = "2026-06-01".into();
        assert!(s.save_lesson(&db, &long).is_err());
    }

    #[test]
    fn unreadable_files_are_quarantined_not_dropped() {
        let (s, db, _tmp) = store();
        s.save(&db, &fact("good", "a real fact")).unwrap();
        // Empty file: parses to nothing usable.
        std::fs::write(s.dir().join("facts").join("hollow.md"), "").unwrap();

        let live = s.list();
        assert_eq!(live.len(), 1, "the good fact still loads");
        assert_eq!(live[0].name, "good");

        let aside = s.quarantined();
        assert_eq!(aside.len(), 1);
        assert!(aside[0].ends_with("-facts-hollow.md"), "got {}", aside[0]);
        assert!(!s.dir().join("facts").join("hollow.md").exists());

        // The user fixes it in an editor and puts it back.
        let file = aside[0].clone();
        std::fs::write(
            s.dir().join(".quarantine").join(&file),
            "---\nname: hollow\n---\nnow it has content\n",
        )
        .unwrap();
        s.restore_quarantined(&db, &file).unwrap();
        assert_eq!(s.read("hollow").unwrap().body, "now it has content");
        assert!(s.quarantined().is_empty());

        assert!(s.restore_quarantined(&db, "../../evil.md").is_err());
        assert!(s.delete_quarantined("not-a-set-aside-name").is_err());
    }

    fn recipe(name: &str, surface: Option<&str>) -> Recipe {
        Recipe {
            name: name.into(),
            description: "Compile the weekly status report".into(),
            trigger: "user asks for a weekly report".into(),
            created: String::new(),
            used: 0,
            last_used: None,
            steps: "1. Ask which week.\n2. search_history for decisions.".into(),
            surface_json: surface.map(str::to_string),
        }
    }

    #[test]
    fn recipe_round_trips_including_the_surface_fence() {
        let (s, db, _tmp) = store();
        let tree = r#"{"kind":"stack","children":[]}"#;
        s.save_recipe(&db, &recipe("Weekly Report", Some(tree))).unwrap();

        let got = s.read_recipe("weekly-report").unwrap();
        assert_eq!(got.trigger, "user asks for a weekly report");
        assert_eq!(got.used, 0);
        assert_eq!(got.surface_json.as_deref(), Some(tree));
        assert!(got.steps.starts_with("1. Ask which week."));
        assert!(!got.steps.contains("```"), "the fence is not part of the steps");

        // Usage counting is silent and durable.
        s.touch_recipe(&db, "weekly-report").unwrap();
        s.touch_recipe(&db, "weekly-report").unwrap();
        let got = s.read_recipe("weekly-report").unwrap();
        assert_eq!(got.used, 2);
        assert!(got.last_used.is_some());
        // …and the count a use produced is the one the prompt will carry.
        assert!(
            s.index_markdown().contains("(used 2×)"),
            "using a recipe must refresh the index it is advertised in"
        );

        // Re-saving is a revision: provenance and usage survive it.
        let mut revised = recipe("weekly-report", None);
        revised.steps = "1. Different steps entirely.".into();
        s.save_recipe(&db, &revised).unwrap();
        let got = s.read_recipe("weekly-report").unwrap();
        assert_eq!(got.used, 2, "a revision doesn't reset the usage count");
        assert_eq!(got.steps, "1. Different steps entirely.");
        assert_eq!(got.surface_json, None);

        // The index advertises the trigger and the usage count.
        let index = s.index_markdown();
        assert!(index.contains("[weekly-report] (used 2×) when: user asks for a weekly report"), "got:\n{index}");

        assert!(s.list_recipes().len() == 1);
        s.forget_recipe(&db, "weekly-report").unwrap();
        assert!(s.list_recipes().is_empty());
        assert!(s.touch_recipe(&db, "weekly-report").is_err());
    }

    #[test]
    fn recipe_rejects_bad_templates_and_overlong_steps() {
        let (s, db, _tmp) = store();
        let err = s.save_recipe(&db, &recipe("bad", Some("not json"))).unwrap_err();
        assert!(err.contains("isn't valid JSON"), "got {err}");

        let mut long = recipe("long", None);
        long.steps = "x".repeat(RECIPE_STEPS_CAP + 1);
        assert!(s.save_recipe(&db, &long).is_err());
    }

    #[test]
    fn an_unterminated_fence_stays_readable_as_steps() {
        let (s, _db, _tmp) = store();
        std::fs::write(
            s.dir().join("recipes").join("broken.md"),
            "---\nname: broken\ntrigger: t\n---\n1. Do it.\n```surface\n{\"kind\":\"stack\"}\n",
        )
        .unwrap();
        let got = s.read_recipe("broken").unwrap();
        assert_eq!(got.surface_json, None);
        assert!(got.steps.contains("1. Do it."), "the steps aren't swallowed");
    }

    #[test]
    fn snapshot_copies_the_whole_self() {
        let (s, db, _tmp) = store();
        s.save(&db, &fact("kept", "body")).unwrap();
        s.set_soul("standing instruction").unwrap();
        let name = s.snapshot().unwrap();
        let dir = s.dir().join(".snapshots").join(&name);
        assert!(dir.join("facts").join("kept.md").exists());
        assert!(dir.join("SOUL.md").exists());
        assert!(dir.join("MEMORY.md").exists());
    }
}
