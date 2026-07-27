//! Built-in File System skill: a real toolkit over the user's real disk.
//!
//! There is no sandbox. When a conversation has a working folder attached, the
//! agent reads and writes the actual files in it. Three things keep that safe:
//!
//! 1. **Scope.** Every path is canonicalised (collapsing `..`, following
//!    symlinks) before any check, and must resolve inside the working folder, a
//!    persisted grant, or a folder the user approves interactively.
//! 2. **Trust.** The conversation carries a level — read-only, ask-first, full —
//!    that decides whether a change goes through silently, raises a prompt, or is
//!    refused. Deletes and moves ask at *every* level.
//! 3. **Undo.** Anything that destroys bytes snapshots them first (see `trash`),
//!    so "Recent changes" in the Workbench can put them back.
//!
//! Paths may be relative to the working folder — that's what makes the agent
//! fluent rather than making it guess absolute Windows paths.

use std::path::{Path, PathBuf};

use crate::db::Db;
use crate::permissions::{
    canonicalize_lenient, gate, path_within_root, Decision, Impact, Mode, PermissionManager,
    PermissionRequest, Trust,
};

use super::run::AgentEventSink;
use super::skills::SkillContext;
use super::trash;

// ---- limits ----
//
// Context is the scarce resource. A tool that quietly returns four megabytes of
// text costs the user a whole conversation, so every read is bounded and says so
// when it truncates.

/// Largest whole-file read without an explicit window.
const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;
/// Files larger than this are skipped when searching contents.
const MAX_SEARCH_BYTES: u64 = 1024 * 1024;
const MAX_DIR_ENTRIES: usize = 500;
const MAX_SEARCH_RESULTS: usize = 100;
const MAX_LINE_CHARS: usize = 400;
/// Bytes inspected when deciding whether a file is text.
const SNIFF_BYTES: usize = 8192;

/// Directories that are noise in every project. Skipped when listing, walking
/// and searching — the agent should see the user's work, not their build output.
pub const IGNORED_DIRS: [&str; 12] = [
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    ".cache",
    ".idea",
    ".vscode",
];

pub fn is_ignored(name: &str, show_hidden: bool) -> bool {
    if IGNORED_DIRS.contains(&name) {
        return true;
    }
    !show_hidden && name.starts_with('.')
}

/// The OpenAI tool schemas advertised to the model for this skill.
pub fn tool_specs() -> serde_json::Value {
    // Repeated in every description because models read parameter docs far more
    // reliably than they remember the system prompt.
    const REL: &str = "Absolute, or relative to the attached working folder.";
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a text file from the user's computer. Large files must be read in windows using offset/limit.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": REL },
                        "offset": { "type": "integer", "description": "First line to read, 1-based. Omit to start at the top." },
                        "limit": { "type": "integer", "description": "How many lines to read from `offset`." }
                    },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "list_directory",
                "description": "List the entries in a directory. Build output and version-control folders are omitted.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": REL },
                        "recursive": { "type": "boolean", "description": "Descend up to three levels instead of one." }
                    },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "search_files",
                "description": "Find files by name pattern and/or search their contents. Use this to orient yourself in a folder before reading anything.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Folder to search in. " },
                        "glob": { "type": "string", "description": "Filename pattern, e.g. \"*.md\" or \"test_*.py\"." },
                        "query": { "type": "string", "description": "Text to find inside matching files. Case-insensitive." },
                        "max_results": { "type": "integer", "description": "Cap on results (default 40)." }
                    },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Write a text file, creating it and any missing parent folders. Overwrites the whole file — prefer edit_file for changes to an existing one.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": REL },
                        "content": { "type": "string", "description": "The full file contents." }
                    },
                    "required": ["path", "content"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "edit_file",
                "description": "Replace an exact snippet in a file, leaving the rest untouched. `old_string` must appear exactly once unless replace_all is set. Read the file first so the snippet matches byte for byte.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": REL },
                        "old_string": { "type": "string", "description": "Exact text to replace, including indentation." },
                        "new_string": { "type": "string", "description": "Replacement text." },
                        "replace_all": { "type": "boolean", "description": "Replace every occurrence instead of requiring a unique match." }
                    },
                    "required": ["path", "old_string", "new_string"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "create_dir",
                "description": "Create a folder, including any missing parents.",
                "parameters": {
                    "type": "object",
                    "properties": { "path": { "type": "string", "description": REL } },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "move_file",
                "description": "Move or rename a file or folder.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "from": { "type": "string", "description": REL },
                        "to": { "type": "string", "description": REL },
                        "overwrite": { "type": "boolean", "description": "Allow replacing an existing file at the destination." }
                    },
                    "required": ["from", "to"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "delete_file",
                "description": "Delete a file, or a folder with `recursive`. Always asks the user first.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": REL },
                        "recursive": { "type": "boolean", "description": "Required to delete a non-empty folder." }
                    },
                    "required": ["path"]
                }
            }
        }
    ])
}

/// Is this a File System tool name?
pub fn handles(name: &str) -> bool {
    matches!(
        name,
        "read_file"
            | "list_directory"
            | "search_files"
            | "write_file"
            | "edit_file"
            | "create_dir"
            | "move_file"
            | "delete_file"
    )
}

/// Human-readable (verb, target) for the timeline (§5.6 plain past-tense).
pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    let arg = |k: &str| args.get(k).and_then(|p| p.as_str()).unwrap_or("");
    let path = if arg("path").is_empty() { arg("from") } else { arg("path") };
    let short = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_string();

    match name {
        "read_file" => ("read".into(), short),
        "list_directory" => ("listed".into(), short),
        "search_files" => {
            let what = if !arg("query").is_empty() {
                format!("\u{201c}{}\u{201d}", arg("query"))
            } else if !arg("glob").is_empty() {
                arg("glob").to_string()
            } else {
                short
            };
            ("searched".into(), what)
        }
        "write_file" => ("wrote".into(), short),
        "edit_file" => ("edited".into(), short),
        "create_dir" => ("created folder".into(), short),
        "move_file" => {
            let to = Path::new(arg("to"))
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            ("moved".into(), format!("{short} \u{2192} {to}"))
        }
        "delete_file" => ("deleted".into(), short),
        other => (other.into(), short),
    }
}

/// What each tool does to the disk, and therefore what trust it needs.
fn impact(name: &str) -> Impact {
    match name {
        "read_file" | "list_directory" | "search_files" => Impact::Read,
        "write_file" | "edit_file" | "create_dir" => Impact::Modify,
        _ => Impact::Destroy,
    }
}

fn required_mode(name: &str) -> Mode {
    if impact(name) == Impact::Read {
        Mode::Read
    } else {
        Mode::ReadWrite
    }
}

/// The folder a scope grant should cover for a given target: its parent
/// directory, so granting one file opens its folder (§5.4.4 copy).
fn grant_root(path: &Path) -> PathBuf {
    path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| path.to_path_buf())
}

/// Resolve a model-supplied path. Relative paths hang off the working folder —
/// the single change that lets the model say `docs/plan.md` instead of guessing
/// `C:\Users\...`. Canonicalisation happens here, once, before any check.
fn resolve(raw: &str, folder: Option<&Path>) -> PathBuf {
    let p = Path::new(raw);
    let joined = match folder {
        Some(root) if p.is_relative() => root.join(p),
        _ => p.to_path_buf(),
    };
    canonicalize_lenient(&joined)
}

/// Show a path the way the user thinks of it: relative to the working folder
/// when it lives there, absolute otherwise.
fn display(path: &Path, folder: Option<&Path>) -> String {
    if let Some(root) = folder {
        if let Ok(rel) = path.strip_prefix(root) {
            let s = rel.to_string_lossy();
            if !s.is_empty() {
                return s.replace('\\', "/");
            }
        }
    }
    path.display().to_string()
}

/// The outcome of asking whether an operation may proceed.
struct Clearance {
    /// True when the path sits inside the attached working folder.
    in_folder: bool,
}

/// Decide whether this operation may run, prompting if it must.
///
/// Order matters: the working folder is consulted first (that is what makes it
/// feel like a workspace rather than a permission dialog), then the persisted
/// allowlist, then per-chat grants, and only then do we interrupt the user.
#[allow(clippy::too_many_arguments)]
async fn authorize(
    db: &Db,
    perms: &PermissionManager,
    sink: &AgentEventSink,
    conversation_id: &str,
    folder: Option<&Path>,
    trust: Trust,
    path: &Path,
    name: &str,
    detail: Option<String>,
) -> Result<Clearance, String> {
    let op_impact = impact(name);

    // ---- inside the working folder: trust decides ----
    if let Some(root) = folder {
        if path_within_root(path, root) {
            let must_ask = gate(trust, op_impact)?;
            if !must_ask {
                return Ok(Clearance { in_folder: true });
            }
            let (verb, target) = describe(name, &serde_json::json!({ "path": path.to_string_lossy() }));
            let summary = match &detail {
                Some(d) => format!("{} {target} — {d}?", capitalize(&verb)),
                None => format!("{} {target}?", capitalize(&verb)),
            };
            let id = format!("perm_{}", uuid::Uuid::new_v4());
            let rx = perms.open_request(&id);
            sink.send_permission(PermissionRequest::operation(
                id,
                summary,
                path.to_string_lossy().to_string(),
                required_mode(name),
                detail,
            ));
            return match rx.await.unwrap_or(Decision::Deny) {
                Decision::Deny => Err(format!(
                    "The user declined that change to {}.",
                    display(path, folder)
                )),
                // "Don't ask again in this folder" arrives as Forever and raises
                // the trust level, so subsequent changes go through silently.
                Decision::Forever => {
                    let _ = db.set_conversation_trust(conversation_id, Trust::Auto.as_str());
                    Ok(Clearance { in_folder: true })
                }
                _ => Ok(Clearance { in_folder: true }),
            };
        }
    }

    // ---- outside it: the original scope-grant flow, unchanged ----
    let mode = required_mode(name);
    if let Ok(grants) = db.list_permissions() {
        for g in &grants {
            let gmode = if g.mode == "read-write" { Mode::ReadWrite } else { Mode::Read };
            if gmode.satisfies(mode) && path_within_root(path, Path::new(&g.path)) {
                return Ok(Clearance { in_folder: false });
            }
        }
    }
    if perms.chat_allows(conversation_id, path, mode) {
        return Ok(Clearance { in_folder: false });
    }

    let root = grant_root(path);
    let id = format!("perm_{}", uuid::Uuid::new_v4());
    let verb = if mode == Mode::ReadWrite { "write files in" } else { "read files in" };
    let summary = format!("Poiesis wants to {verb} {} to answer this.", root.display());
    let rx = perms.open_request(&id);
    sink.send_permission(PermissionRequest::scope(
        id,
        summary,
        root.to_string_lossy().to_string(),
        mode,
    ));

    match rx.await.unwrap_or(Decision::Deny) {
        Decision::Deny => Err("You declined access to that folder.".to_string()),
        Decision::Once => Ok(Clearance { in_folder: false }),
        Decision::Chat => {
            perms.add_chat_grant(conversation_id, root, mode);
            Ok(Clearance { in_folder: false })
        }
        Decision::Forever => {
            let _ = db.add_permission(&root.to_string_lossy(), mode.as_str());
            Ok(Clearance { in_folder: false })
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

// ---- reading helpers ----

/// Is this file text? A NUL byte in the first few kilobytes means no. Returning
/// a descriptor beats spraying binary into the model's context.
fn looks_binary(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; SNIFF_BYTES];
    let n = f.read(&mut buf).unwrap_or(0);
    buf[..n].contains(&0)
}

fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

fn read_windowed(path: &Path, offset: Option<usize>, limit: Option<usize>) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("couldn't open {}: {e}", path.display()))?;
    if meta.is_dir() {
        return Err(format!("{} is a folder — use list_directory", path.display()));
    }
    if looks_binary(path) {
        return Ok(format!(
            "<binary file, {}> — not readable as text. Tell the user what it is rather than guessing at its contents.",
            human_bytes(meta.len())
        ));
    }
    if meta.len() > MAX_READ_BYTES && offset.is_none() && limit.is_none() {
        return Err(format!(
            "{} is {} — too large to read at once. Call read_file again with `offset` and `limit` to read part of it, or use search_files to find the part you need.",
            path.display(),
            human_bytes(meta.len())
        ));
    }

    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
    if offset.is_none() && limit.is_none() {
        return Ok(text);
    }

    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();
    let start = offset.unwrap_or(1).saturating_sub(1).min(total);
    let end = limit.map(|l| (start + l).min(total)).unwrap_or(total);
    let body = lines[start..end].join("\n");
    Ok(format!(
        "[lines {}\u{2013}{} of {}]\n{}",
        start + 1,
        end,
        total,
        body
    ))
}

fn list_dir(path: &Path, recursive: bool) -> Result<String, String> {
    let mut out = Vec::new();
    walk(path, path, if recursive { 3 } else { 1 }, &mut out)?;
    out.sort();
    let truncated = out.len() > MAX_DIR_ENTRIES;
    out.truncate(MAX_DIR_ENTRIES);
    if out.is_empty() {
        return Ok("(empty)".to_string());
    }
    let mut text = out.join("\n");
    if truncated {
        text.push_str(&format!(
            "\n… more than {MAX_DIR_ENTRIES} entries; narrow with search_files"
        ));
    }
    Ok(text)
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) -> Result<(), String> {
    if depth == 0 || out.len() > MAX_DIR_ENTRIES {
        return Ok(());
    }
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("couldn't list {}: {e}", dir.display()))?;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if is_ignored(&name, false) {
            continue;
        }
        let p = e.path();
        let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
        if p.is_dir() {
            out.push(format!("{rel}/"));
            walk(root, &p, depth - 1, out)?;
        } else {
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            out.push(format!("{rel}  ({})", human_bytes(size)));
        }
    }
    Ok(())
}

/// Shell-style glob over a single filename: `*` and `?` only, which is what
/// models reach for and all that is useful without a pattern crate.
fn glob_match(pattern: &str, name: &str) -> bool {
    fn inner(p: &[char], n: &[char]) -> bool {
        match p.first() {
            None => n.is_empty(),
            Some('*') => inner(&p[1..], n) || (!n.is_empty() && inner(p, &n[1..])),
            Some('?') => !n.is_empty() && inner(&p[1..], &n[1..]),
            Some(c) => {
                !n.is_empty() && n[0].eq_ignore_ascii_case(c) && inner(&p[1..], &n[1..])
            }
        }
    }
    inner(&pattern.chars().collect::<Vec<_>>(), &name.chars().collect::<Vec<_>>())
}

fn search(
    root: &Path,
    glob: Option<&str>,
    query: Option<&str>,
    max: usize,
) -> Result<String, String> {
    let mut hits: Vec<String> = Vec::new();
    let needle = query.map(|q| q.to_lowercase());
    search_dir(root, root, glob, needle.as_deref(), max, &mut hits, 8)?;
    if hits.is_empty() {
        return Ok("No matches.".to_string());
    }
    let truncated = hits.len() >= max;
    let mut text = hits.join("\n");
    if truncated {
        text.push_str("\n… more matches; narrow the pattern or query");
    }
    Ok(text)
}

#[allow(clippy::too_many_arguments)]
fn search_dir(
    root: &Path,
    dir: &Path,
    glob: Option<&str>,
    needle: Option<&str>,
    max: usize,
    hits: &mut Vec<String>,
    depth: usize,
) -> Result<(), String> {
    if depth == 0 || hits.len() >= max {
        return Ok(());
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for e in entries.flatten() {
        if hits.len() >= max {
            return Ok(());
        }
        let name = e.file_name().to_string_lossy().to_string();
        if is_ignored(&name, false) {
            continue;
        }
        let p = e.path();
        if p.is_dir() {
            search_dir(root, &p, glob, needle, max, hits, depth - 1)?;
            continue;
        }
        if let Some(g) = glob {
            if !glob_match(g, &name) {
                continue;
            }
        }
        let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
        let Some(needle) = needle else {
            // Name-only search: the match is the file itself.
            hits.push(rel);
            continue;
        };
        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
        if size > MAX_SEARCH_BYTES || looks_binary(&p) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            if hits.len() >= max {
                return Ok(());
            }
            if line.to_lowercase().contains(needle) {
                let mut snippet = line.trim().to_string();
                if snippet.chars().count() > MAX_LINE_CHARS {
                    snippet = snippet.chars().take(MAX_LINE_CHARS).collect::<String>() + "…";
                }
                hits.push(format!("{rel}:{}: {snippet}", i + 1));
            }
        }
    }
    Ok(())
}

/// Apply an exact-snippet edit. Fails loudly rather than guessing: a silent
/// wrong edit on someone's real file is far worse than a retry.
fn apply_edit(
    original: &str,
    old: &str,
    new: &str,
    replace_all: bool,
) -> Result<(String, usize), String> {
    if old.is_empty() {
        return Err("`old_string` is empty — use write_file to create a file.".to_string());
    }
    if old == new {
        return Err("`old_string` and `new_string` are identical — nothing to do.".to_string());
    }
    let count = original.matches(old).count();
    match count {
        0 => Err(
            "That exact text isn't in the file. Read it again and copy the snippet verbatim, including indentation."
                .to_string(),
        ),
        _ if count > 1 && !replace_all => Err(format!(
            "That text appears {count} times. Include more surrounding context so the snippet is unique, or set replace_all."
        )),
        _ => {
            let updated = if replace_all {
                original.replace(old, new)
            } else {
                original.replacen(old, new, 1)
            };
            Ok((updated, count))
        }
    }
}

/// A short before/after excerpt for the consent panel, so approving an edit is
/// reviewing a change rather than trusting a sentence about it.
fn edit_excerpt(old: &str, new: &str) -> String {
    let clip = |s: &str| {
        let lines: Vec<&str> = s.lines().take(6).collect();
        let mut out = lines.join("\n");
        if s.lines().count() > 6 {
            out.push_str("\n…");
        }
        out
    };
    format!("- {}\n+ {}", clip(old).replace('\n', "\n- "), clip(new).replace('\n', "\n+ "))
}

/// The system-prompt block describing the attached folder, if there is one.
///
/// This is what turns "there is a permission grant somewhere" into "we are
/// working in this folder" — without it the model has no anchor for a relative
/// path and falls back to guessing absolute ones.
pub fn working_folder_brief(db: &Db, conversation_id: &str) -> Option<String> {
    let (folder, trust) = db.conversation_folder(conversation_id).ok()?;
    let folder = folder?;
    let trust = Trust::parse(&trust);

    let mut brief = format!(
        "Working folder: {folder}  (access: {})\n\
         File paths may be given relative to this folder — prefer `docs/plan.md` over a full path.\n\
         Use search_files to orient yourself before reading anything.\n\
         Prefer edit_file over rewriting a whole file with write_file.",
        trust.label()
    );
    match trust {
        Trust::ReadOnly => brief.push_str(
            "\nThis folder is read-only: you cannot change anything in it. If the user asks for a \
             change, tell them to switch access in the Workbench panel.",
        ),
        Trust::Confirm => brief.push_str(
            "\nEvery change you make is shown to the user for approval first, so propose the edit \
             by making it rather than asking permission in prose.",
        ),
        Trust::Auto => brief.push_str(
            "\nChanges apply without a prompt (deletions still ask), so be deliberate — say what \
             you changed after you change it.",
        ),
    }
    brief.push_str(
        "\nUse create_artifact for something the user should look at; use write_file when they \
         want it kept on disk.",
    );
    Some(brief)
}

/// Execute a File System tool call. Returns the result text fed back to the model.
pub async fn execute(
    ctx: &SkillContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let db = ctx.db;
    let conversation_id = ctx.conversation_id;

    let (folder_raw, trust_raw) = db
        .conversation_folder(conversation_id)
        .map_err(|e| e.to_string())?;
    let folder = folder_raw.as_ref().map(|f| canonicalize_lenient(Path::new(f)));
    let folder_ref = folder.as_deref();
    let trust = Trust::parse(&trust_raw);

    let str_arg = |k: &str| args.get(k).and_then(|v| v.as_str());
    let usize_arg = |k: &str| args.get(k).and_then(|v| v.as_u64()).map(|v| v as usize);
    let bool_arg = |k: &str| args.get(k).and_then(|v| v.as_bool()).unwrap_or(false);

    let raw_path = str_arg("path")
        .or_else(|| str_arg("from"))
        .ok_or("missing 'path' argument")?;
    let path = resolve(raw_path, folder_ref);
    let shown = display(&path, folder_ref);

    // Prepare the review detail before asking, so the prompt shows the change.
    let mut pending_edit: Option<(String, usize)> = None;
    let detail: Option<String> = match name {
        "edit_file" => {
            let old = str_arg("old_string").ok_or("missing 'old_string'")?;
            let new = str_arg("new_string").ok_or("missing 'new_string'")?;
            let original = std::fs::read_to_string(&path)
                .map_err(|e| format!("couldn't read {shown}: {e}"))?;
            let (updated, count) = apply_edit(&original, old, new, bool_arg("replace_all"))?;
            pending_edit = Some((updated, count));
            Some(edit_excerpt(old, new))
        }
        "write_file" => {
            let content = str_arg("content").unwrap_or("");
            Some(if path.exists() {
                format!("replace {} with {}", human_bytes(std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)), human_bytes(content.len() as u64))
            } else {
                format!("create, {}", human_bytes(content.len() as u64))
            })
        }
        "delete_file" => Some(if path.is_dir() { "delete this folder and everything in it".into() } else { "permanently delete".into() }),
        "move_file" => str_arg("to").map(|t| format!("move to {}", display(&resolve(t, folder_ref), folder_ref))),
        _ => None,
    };

    let clearance = authorize(
        db,
        ctx.perms,
        ctx.sink,
        conversation_id,
        folder_ref,
        trust,
        &path,
        name,
        detail,
    )
    .await?;

    // A move touches two places; the destination needs clearing too.
    if name == "move_file" {
        let to = resolve(str_arg("to").ok_or("missing 'to' argument")?, folder_ref);
        authorize(
            db,
            ctx.perms,
            ctx.sink,
            conversation_id,
            folder_ref,
            trust,
            &to,
            "write_file",
            None,
        )
        .await?;
    }

    let data_dir = ctx.data_dir;
    let mut undo_token: Option<String> = None;
    let mut changed_path: Option<String> = None;

    let result = match name {
        "read_file" => read_windowed(&path, usize_arg("offset"), usize_arg("limit"))?,

        "list_directory" => list_dir(&path, bool_arg("recursive"))?,

        "search_files" => search(
            &path,
            str_arg("glob"),
            str_arg("query"),
            usize_arg("max_results").unwrap_or(40).min(MAX_SEARCH_RESULTS),
        )?,

        "write_file" => {
            let content = str_arg("content").unwrap_or("");
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("couldn't create {}: {e}", parent.display()))?;
            }
            let entry = trash::record(db, data_dir, conversation_id, "write", &path, None);
            std::fs::write(&path, content)
                .map_err(|e| format!("couldn't write {shown}: {e}"))?;
            undo_token = entry.map(|e| e.id);
            changed_path = Some(path.to_string_lossy().to_string());
            format!("Wrote {} to {shown}", human_bytes(content.len() as u64))
        }

        "edit_file" => {
            let (updated, count) = pending_edit.ok_or("edit was not prepared")?;
            let entry = trash::record(db, data_dir, conversation_id, "edit", &path, None);
            std::fs::write(&path, &updated)
                .map_err(|e| format!("couldn't write {shown}: {e}"))?;
            undo_token = entry.map(|e| e.id);
            changed_path = Some(path.to_string_lossy().to_string());
            let n = if count == 1 { "1 occurrence".to_string() } else { format!("{count} occurrences") };
            format!("Edited {shown} ({n} replaced)")
        }

        "create_dir" => {
            std::fs::create_dir_all(&path)
                .map_err(|e| format!("couldn't create {shown}: {e}"))?;
            let entry = trash::record(db, data_dir, conversation_id, "write", &path, None);
            undo_token = entry.map(|e| e.id);
            changed_path = Some(path.to_string_lossy().to_string());
            format!("Created folder {shown}")
        }

        "move_file" => {
            let to = resolve(str_arg("to").ok_or("missing 'to' argument")?, folder_ref);
            if to.exists() && !bool_arg("overwrite") {
                return Err(format!(
                    "{} already exists. Set overwrite to replace it, or pick another name.",
                    display(&to, folder_ref)
                ));
            }
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let entry = trash::record(db, data_dir, conversation_id, "move", &to, Some(&path));
            std::fs::rename(&path, &to)
                .map_err(|e| format!("couldn't move {shown}: {e}"))?;
            undo_token = entry.map(|e| e.id);
            changed_path = Some(to.to_string_lossy().to_string());
            format!("Moved {shown} to {}", display(&to, folder_ref))
        }

        "delete_file" => {
            let entry = trash::record(db, data_dir, conversation_id, "delete", &path, None);
            if path.is_dir() {
                if !bool_arg("recursive") {
                    return Err(format!(
                        "{shown} is a folder — set recursive to delete it and its contents."
                    ));
                }
                std::fs::remove_dir_all(&path)
                    .map_err(|e| format!("couldn't delete {shown}: {e}"))?;
            } else {
                std::fs::remove_file(&path)
                    .map_err(|e| format!("couldn't delete {shown}: {e}"))?;
            }
            undo_token = entry.map(|e| e.id);
            changed_path = Some(path.to_string_lossy().to_string());
            format!("Deleted {shown}")
        }

        other => return Err(format!("unknown file tool '{other}'")),
    };

    // Tell the Workbench what moved, so the tree and Recent changes stay honest.
    if let Some(changed) = changed_path {
        let (verb, _) = describe(name, args);
        ctx.sink.file_changed(&verb, &changed, undo_token.as_deref());
    }

    let (verb, _) = describe(name, args);
    let scope = if clearance.in_folder { "" } else { " (outside the working folder)" };
    let _ = db.log_activity(
        Some(conversation_id),
        "file",
        &format!("{verb} {}{scope}", path.display()),
    );

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_resolve_inside_the_working_folder() {
        let root = canonicalize_lenient(&std::env::temp_dir());
        let resolved = resolve("docs/plan.md", Some(&root));
        assert!(path_within_root(&resolved, &root));
        assert!(resolved.ends_with("plan.md"));
    }

    #[test]
    fn relative_traversal_cannot_escape_the_working_folder() {
        let root = canonicalize_lenient(&std::env::temp_dir());
        let escaped = resolve("../secrets.txt", Some(&root));
        assert!(
            !path_within_root(&escaped, &root),
            "`..` must resolve outside the folder so the scope check rejects it"
        );
    }

    #[test]
    fn absolute_paths_are_left_alone() {
        let root = canonicalize_lenient(&std::env::temp_dir());
        let other = root.join("elsewhere.txt");
        assert_eq!(resolve(&other.to_string_lossy(), Some(&root)), canonicalize_lenient(&other));
    }

    #[test]
    fn paths_display_relative_to_the_folder() {
        let root = canonicalize_lenient(&std::env::temp_dir());
        assert_eq!(display(&root.join("docs").join("a.md"), Some(&root)), "docs/a.md");
    }

    #[test]
    fn edit_requires_a_unique_match() {
        let text = "alpha\nbeta\nalpha\n";
        assert!(apply_edit(text, "alpha", "gamma", false).is_err(), "ambiguous edits refuse");
        let (all, count) = apply_edit(text, "alpha", "gamma", true).unwrap();
        assert_eq!(count, 2);
        assert_eq!(all, "gamma\nbeta\ngamma\n");
        let (one, _) = apply_edit(text, "beta", "delta", false).unwrap();
        assert_eq!(one, "alpha\ndelta\nalpha\n");
    }

    #[test]
    fn edit_refuses_a_snippet_that_isnt_there() {
        assert!(apply_edit("hello", "goodbye", "hi", false).is_err());
        assert!(apply_edit("hello", "", "hi", false).is_err());
        assert!(apply_edit("hello", "hello", "hello", false).is_err());
    }

    #[test]
    fn glob_matches_the_patterns_models_actually_write() {
        assert!(glob_match("*.md", "plan.md"));
        assert!(glob_match("*.md", "PLAN.MD"), "case-insensitive, like Windows");
        assert!(!glob_match("*.md", "plan.txt"));
        assert!(glob_match("test_*.py", "test_thing.py"));
        assert!(glob_match("a?c.txt", "abc.txt"));
        assert!(!glob_match("a?c.txt", "ac.txt"));
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn large_files_refuse_a_whole_read_but_allow_a_window() {
        let f = std::env::temp_dir().join(format!("poiesis_big_{}.txt", uuid::Uuid::new_v4()));
        let line = "x".repeat(1023);
        let body: String = std::iter::repeat(line.as_str()).take(3000).collect::<Vec<_>>().join("\n");
        std::fs::write(&f, &body).unwrap();
        assert!(std::fs::metadata(&f).unwrap().len() > MAX_READ_BYTES);

        assert!(read_windowed(&f, None, None).is_err(), "whole read of a big file refuses");
        let windowed = read_windowed(&f, Some(1), Some(3)).unwrap();
        assert!(windowed.starts_with("[lines 1\u{2013}3 of 3000]"));
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn binary_files_return_a_descriptor_not_bytes() {
        let f = std::env::temp_dir().join(format!("poiesis_bin_{}.dat", uuid::Uuid::new_v4()));
        std::fs::write(&f, [0x89, 0x50, 0x00, 0x4e, 0x47]).unwrap();
        let out = read_windowed(&f, None, None).unwrap();
        assert!(out.starts_with("<binary file,"), "got: {out}");
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn search_finds_by_name_and_by_content() {
        let dir = std::env::temp_dir().join(format!("poiesis_search_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        std::fs::write(dir.join("a.md"), "the needle is here\nnot here").unwrap();
        std::fs::write(dir.join("b.txt"), "needle too").unwrap();
        std::fs::write(dir.join("node_modules").join("c.md"), "needle").unwrap();

        let by_name = search(&dir, Some("*.md"), None, 40).unwrap();
        assert!(by_name.contains("a.md"));
        assert!(!by_name.contains("node_modules"), "build folders stay out of results");

        let by_content = search(&dir, None, Some("NEEDLE"), 40).unwrap();
        assert!(by_content.contains("a.md:1:"), "hits carry line numbers: {by_content}");
        assert!(by_content.contains("b.txt:1:"));

        assert_eq!(search(&dir, Some("*.rs"), None, 40).unwrap(), "No matches.");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn listing_omits_noise_directories() {
        let dir = std::env::temp_dir().join(format!("poiesis_list_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src").join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.join("README.md"), "hi").unwrap();

        let flat = list_dir(&dir, false).unwrap();
        assert!(flat.contains("README.md"));
        assert!(flat.contains("src/"));
        assert!(!flat.contains(".git"));
        assert!(!flat.contains("main.rs"), "non-recursive stays at one level");

        let deep = list_dir(&dir, true).unwrap();
        assert!(deep.contains("src/main.rs"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_tool_is_classified_and_handled() {
        for spec in tool_specs().as_array().unwrap() {
            let name = spec["function"]["name"].as_str().unwrap();
            assert!(handles(name), "{name} advertised but not handled");
        }
        assert_eq!(impact("read_file"), Impact::Read);
        assert_eq!(impact("search_files"), Impact::Read);
        assert_eq!(impact("edit_file"), Impact::Modify);
        assert_eq!(impact("delete_file"), Impact::Destroy);
        assert_eq!(impact("move_file"), Impact::Destroy);
        assert_eq!(required_mode("read_file"), Mode::Read);
        assert_eq!(required_mode("write_file"), Mode::ReadWrite);
    }
}
