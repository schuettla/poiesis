//! Built-in Code Execution toolset (TOOL-5). Runs short Python or Node snippets in
//! an isolated **Job-Object-confined subprocess** (reusing `runtime/jobobject`):
//! no network, a scratch working directory, and a hard kill-on-close so a runaway
//! script can't outlive the run. Defaults off; opt-in per chat.
//!
//! `DAT`: the sandbox generalises past a bespoke `query_csv`-style tool — it's
//! made *reachable* for spreadsheet/data questions (`DAT-1`), given read access
//! to the attached working folder (`DAT-2`), and can hand a result back as a
//! rendered table instead of only prose (`DAT-3`).
//!
//! `DAT-2` says reads go "through the same permission gate as every other file
//! access", and the honest position is that a subprocess cannot be held to
//! that gate the way `filesystem.rs` can — see `sandbox.rs`. Two things
//! narrow the gap: a folder attached **read-only** is never handed over at
//! all (that trust level makes `permissions::gate` refuse writes, and a
//! promise we can't keep is worse than a missing capability), and any file the
//! snippet did change is named in the activity log afterwards, so an
//! unattended write is visible rather than silent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::permissions::Trust;

use super::toolsets::{render_block, ToolContext};
use super::sandbox;

/// How many entries the working folder may hold before the after-the-fact
/// change check (`DAT-2`) gives up. Walking a huge tree twice around a
/// 10-second run would cost more than the run itself; a folder that big gets
/// no check rather than a slow one.
const WATCH_ENTRY_CAP: usize = 4000;
/// How many changed filenames to name in the record before summarising.
const WATCH_NAMES_SHOWN: usize = 5;

/// Every file under `root`, as `relative path -> (size, mtime-ms)`.
///
/// `None` means "couldn't take a reliable snapshot" — the tree is larger than
/// `WATCH_ENTRY_CAP`, or unreadable — which the caller treats as "no check",
/// never as "nothing changed".
fn folder_snapshot(root: &Path) -> Option<BTreeMap<PathBuf, (u64, i64)>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).ok()?.flatten() {
            if out.len() + stack.len() > WATCH_ENTRY_CAP {
                return None;
            }
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                out.insert(rel, (meta.len(), mtime));
            }
        }
    }
    Some(out)
}

/// Files created, modified or removed between two snapshots, as display paths.
fn folder_changes(
    before: &BTreeMap<PathBuf, (u64, i64)>,
    after: &BTreeMap<PathBuf, (u64, i64)>,
) -> Vec<String> {
    let mut changed: Vec<String> = after
        .iter()
        .filter(|(path, stat)| before.get(*path).map(|old| old != *stat).unwrap_or(true))
        .map(|(path, _)| path.to_string_lossy().replace('\\', "/"))
        .collect();
    changed.extend(
        before
            .keys()
            .filter(|path| !after.contains_key(*path))
            .map(|path| format!("{} (removed)", path.to_string_lossy().replace('\\', "/"))),
    );
    changed.sort();
    changed
}

/// The OpenAI tool schema advertised to the model for this toolset.
pub fn tool_specs() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "run_code",
                "description": "Run a short Python or Node.js snippet in an isolated, time- and memory-limited sandbox and return its output. This is the tool for calculation, data wrangling, spreadsheet/CSV questions, and comparing numbers — reach for it instead of estimating by eye. If a folder is attached to this conversation, its path is available to the snippet in the POIESIS_FOLDER environment variable (os.environ[\"POIESIS_FOLDER\"] in Python, process.env.POIESIS_FOLDER in Node) so it can read files there directly; treat it as read-only, and write any output files to the current directory instead. POIESIS_FOLDER is absent when no folder is attached or when the folder is attached read-only — check for it rather than assuming it, and fall back to read_file. To show the user a table (not just describe it in prose), print exactly one line of JSON as the LAST line of stdout shaped like {\"table\": {\"columns\": [\"Name\", \"Total\"], \"rows\": [[\"a\", 1], [\"b\", 2]]}} — it renders directly instead of being read back by you.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "language": {
                            "type": "string",
                            "enum": ["python", "node"],
                            "description": "Which runtime to use"
                        },
                        "code": { "type": "string", "description": "The source code to execute" },
                        "skill": { "type": "string", "description": "OPTIONAL: name of an active skill whose scripts/ this snippet runs from (SKL-3) — gives more time and a POIESIS_SKILL_DIR pointing at the skill's own folder, for running a bundled scripts/*.py or scripts/*.js." }
                    },
                    "required": ["language", "code"]
                }
            }
        }
    ])
}

/// Is this a Code Execution tool name?
pub fn handles(name: &str) -> bool {
    name == "run_code"
}

/// Human-readable (verb, target) for the timeline (§5.6 plain past-tense).
///
/// `DAT-UI-1`: reframed as "worked out the numbers" rather than "ran python" —
/// the sandbox is presented as the data tool, with the snippet itself tucked
/// behind the step's `⌄` disclosure rather than named up front.
pub fn describe(name: &str, _args: &serde_json::Value) -> (String, String) {
    match name {
        "run_code" => ("worked out".into(), "the numbers".into()),
        other => (other.into(), String::new()),
    }
}

/// `DAT-3`: if the snippet's last line of stdout is a single JSON object shaped
/// `{"table": {"columns": [...], "rows": [...]}}`, pull the inner value out so
/// it can render as a `table` block instead of only living in text the model
/// then has to describe.
fn extract_table(stdout: &str) -> Option<serde_json::Value> {
    let last = stdout.lines().rev().find(|l| !l.trim().is_empty())?;
    let value: serde_json::Value = serde_json::from_str(last.trim()).ok()?;
    let table = value.get("table")?;
    let columns = table.get("columns")?.as_array()?;
    let rows = table.get("rows")?.as_array()?;
    if columns.is_empty() || rows.is_empty() {
        return None;
    }
    Some(table.clone())
}

/// Execute a Code Execution tool call: write the snippet to a throwaway scratch
/// directory, run it in the confined sandbox, and return its captured output.
pub async fn execute(
    ctx: &ToolContext<'_>,
    _name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    let language = args
        .get("language")
        .and_then(|l| l.as_str())
        .unwrap_or("python");
    let code = args
        .get("code")
        .and_then(|c| c.as_str())
        .ok_or("missing 'code' argument")?;

    let (program, filename) = match language {
        "python" => ("python", "main.py"),
        "node" => ("node", "main.js"),
        other => return Err(format!("Unsupported language '{other}'. Use 'python' or 'node'.")),
    };

    // `DAT-UI-1`: the snippet itself lives behind the timeline step's `⌄`
    // disclosure, not in the answer — same event shape as `Recall`.
    ctx.sink.emit(super::AgentEvent::Code {
        id: ctx.call_id.to_string(),
        language: language.to_string(),
        code: code.to_string(),
    });

    // Isolated scratch directory, removed after the run.
    let dir = std::env::temp_dir().join(format!("poiesis-exec-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't create the sandbox dir: {e}"))?;
    let script = dir.join(filename);
    std::fs::write(&script, code).map_err(|e| format!("couldn't write the snippet: {e}"))?;

    // `DAT-2`: the attached working folder is handed to the snippet as a
    // readable path. Reads inside the folder never prompt at any trust level
    // (`permissions::gate` returns `Ok(false)` for `Impact::Read`), so an
    // ordinary attachment owes no extra confirmation here.
    //
    // Read-only is the exception, and it is not a nuance: that trust level
    // makes `gate` *refuse* writes outright, and the sandbox cannot enforce
    // read-only on a child process — it can only ask. Rather than hand a
    // path we can't defend, the folder is withheld entirely; `read_file` and
    // the other gated tools still work, so nothing the user allowed is lost.
    let (folder, trust_raw) = ctx
        .db
        .conversation_folder(ctx.conversation_id)
        .unwrap_or((None, "confirm".to_string()));
    let trust = Trust::parse(&trust_raw);
    // SCH-3: the same enforcement gap applies to an unattended job — nobody is
    // watching to notice an unintended write, so a headless run gets exactly
    // the read-only treatment regardless of the folder's actual trust level.
    let withheld = folder.is_some() && (trust == Trust::ReadOnly || ctx.headless);
    let folder_path = if withheld {
        None
    } else {
        folder.as_deref().map(Path::new)
    };

    // The folder is exposed but not walled off, so what the snippet did to it
    // is recorded instead: one snapshot either side of the run, so a write
    // lands in the activity log by name rather than happening invisibly.
    let before = folder_path.and_then(folder_snapshot);

    // `SKL-3`: an explicit `skill` argument swaps in the longer-timeout,
    // skill-directory profile so a bundled `scripts/*` has room to run and
    // knows where its own folder is. Falls back to the ordinary ad-hoc
    // profile when the name is missing, disabled, or not found — the
    // snippet still runs, just without the extended profile.
    let (run_dir, profile) = match args.get("skill").and_then(|s| s.as_str()) {
        Some(name) if !name.trim().is_empty() => {
            let working_folder = ctx
                .db
                .conversation_folder(ctx.conversation_id)
                .ok()
                .and_then(|(f, _)| f)
                .map(std::path::PathBuf::from);
            let packs = super::skillpack::discover(ctx.mgr.app_data_dir(), working_folder.as_deref());
            match packs.iter().find(|p| p.name == name && super::skillpack::is_enabled(ctx.db, p)) {
                Some(pack) => (pack.dir.clone(), sandbox::Profile::skill(&pack.dir)),
                None => (dir.clone(), sandbox::Profile::ad_hoc()),
            }
        }
        _ => (dir.clone(), sandbox::Profile::ad_hoc()),
    };

    let result = sandbox::run(
        program,
        &[script.to_string_lossy().into_owned()],
        &run_dir,
        folder_path,
        &profile,
    )
    .await;
    let _ = std::fs::remove_dir_all(&dir);

    let _ = ctx
        .db
        .log_activity(Some(ctx.conversation_id), "code", &format!("ran {language}"));

    let changes = match (before, folder_path.and_then(folder_snapshot)) {
        (Some(before), Some(after)) => folder_changes(&before, &after),
        _ => Vec::new(),
    };
    if !changes.is_empty() {
        let named: Vec<&str> = changes.iter().take(WATCH_NAMES_SHOWN).map(String::as_str).collect();
        let rest = changes.len().saturating_sub(named.len());
        let tail = if rest > 0 { format!(" and {rest} more") } else { String::new() };
        let _ = ctx.db.log_activity(
            Some(ctx.conversation_id),
            "file",
            &format!("the snippet changed {}{tail}", named.join(", ")),
        );
    }

    let out = result?;
    if out.timed_out {
        return Ok("The code ran longer than the 10-second limit and was stopped.".to_string());
    }

    // `DAT-3`: a table the snippet printed renders directly instead of only
    // being described back — one render per call, same guard rails as `RND-3`.
    if let Some(table) = extract_table(&out.stdout) {
        let rows = table.get("rows").and_then(|r| r.as_array()).map(|a| a.len()).unwrap_or(0);
        let title = format!("{rows} row{}", if rows == 1 { "" } else { "s" });
        render_block(ctx, "table", &title, &table);
    }

    let mut report = String::new();
    if !out.stdout.is_empty() {
        report.push_str(&format!("stdout:\n{}\n", out.stdout.trim_end()));
    }
    if !out.stderr.is_empty() {
        report.push_str(&format!("stderr:\n{}\n", out.stderr.trim_end()));
    }
    match out.exit_code {
        Some(0) | None => {}
        Some(code) => report.push_str(&format!("(exited with code {code})\n")),
    }
    // Say why POIESIS_FOLDER was absent, so a snippet that needed it can be
    // rewritten to use `read_file` instead of retried blindly.
    if withheld && ctx.headless {
        report.push_str(
            "(POIESIS_FOLDER was not set: this is an unattended run and the sandbox can't \
             guarantee it won't be written to. Use read_file to read from it.)\n",
        );
    } else if withheld {
        report.push_str(
            "(POIESIS_FOLDER was not set: this folder is attached read-only, and the sandbox \
             can't guarantee it won't be written to. Use read_file to read from it.)\n",
        );
    }
    if !changes.is_empty() {
        report.push_str(&format!(
            "(the snippet changed {} file{} in the working folder — this was recorded)\n",
            changes.len(),
            if changes.len() == 1 { "" } else { "s" }
        ));
    }
    if report.is_empty() {
        report.push_str("(the snippet produced no output)");
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trailing_table_line_is_extracted() {
        let stdout = "computing totals…\n{\"table\": {\"columns\": [\"Name\", \"Total\"], \"rows\": [[\"a\", 1], [\"b\", 2]]}}\n";
        let table = extract_table(stdout).expect("a valid trailing table line should parse");
        assert_eq!(table["columns"].as_array().unwrap().len(), 2);
        assert_eq!(table["rows"].as_array().unwrap().len(), 2);
    }

    fn snap(entries: &[(&str, u64, i64)]) -> BTreeMap<PathBuf, (u64, i64)> {
        entries
            .iter()
            .map(|(p, size, mtime)| (PathBuf::from(p), (*size, *mtime)))
            .collect()
    }

    #[test]
    fn an_untouched_folder_reports_no_changes() {
        let before = snap(&[("a.csv", 10, 1), ("sub/b.txt", 20, 2)]);
        assert!(folder_changes(&before, &before.clone()).is_empty());
    }

    #[test]
    fn writes_creations_and_deletions_are_all_named() {
        let before = snap(&[("a.csv", 10, 1), ("gone.txt", 5, 1), ("same.md", 7, 3)]);
        let after = snap(&[("a.csv", 99, 4), ("same.md", 7, 3), ("new.json", 1, 9)]);
        let changed = folder_changes(&before, &after);
        assert_eq!(changed, vec!["a.csv", "gone.txt (removed)", "new.json"]);
    }

    /// A file rewritten to the same length still has a newer mtime, so size
    /// alone would miss it.
    #[test]
    fn an_in_place_rewrite_of_the_same_length_is_still_a_change() {
        let before = snap(&[("data.csv", 128, 1)]);
        let after = snap(&[("data.csv", 128, 2)]);
        assert_eq!(folder_changes(&before, &after), vec!["data.csv"]);
    }

    #[test]
    fn plain_output_with_no_table_extracts_nothing() {
        assert!(extract_table("42\n").is_none());
        assert!(extract_table("{\"not_a_table\": true}\n").is_none());
        assert!(extract_table("{\"table\": {\"columns\": [], \"rows\": []}}\n").is_none());
    }
}
