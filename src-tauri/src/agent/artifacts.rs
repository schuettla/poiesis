//! Built-in Artifacts toolset (CHT-6). The model calls `create_artifact` to emit a
//! titled, self-contained piece of content — HTML, SVG, markdown, or code — which
//! Poiesis persists and renders in the Canvas side panel (HTML/SVG in a sandboxed
//! iframe). The tool result fed back to the model is only a short confirmation, so
//! a large artifact doesn't bloat the context window.
//!
//! `ART-4`: creating was the only thing the model could do here, which made
//! "fix the bug in that game" impossible to answer well. It could emit a whole
//! second artifact from memory, or go hunting for a file that was never on
//! disk — both of which happened. `read_artifact` and `update_artifact` close
//! that loop: the thing in the Canvas is now readable and editable in place,
//! with no file anywhere in the story.
//!
//! `ART-5`: an HTML artifact runs in the preview iframe, and that iframe reports
//! its console and its errors back (see `record_console`). `read_artifact`
//! returns them alongside the content, so the model debugging its own page
//! sees the same red text the user sees rather than guessing from the source.
//!
//! `ART-6`: that console is only ever what the *user's* open panel happened to
//! print — empty when nobody is looking, and empty reads exactly like "no
//! errors". `check_preview` closes that gap by running the page itself, in the
//! browser, at the URL `preview.rs` serves it from. Text only: what it drew,
//! what it logged, what it failed to load. A screenshot would need tool results
//! that can carry images and a model with eyes, and neither is true yet.

use std::collections::HashMap;
use std::sync::Mutex;

use super::toolsets::ToolContext;

const KINDS: [&str; 4] = ["html", "svg", "markdown", "code"];

/// `COD-18`: how long a code artifact may run. Longer than a snippet's ten
/// seconds, since the user pressed Run and is watching; short enough that a
/// program waiting on input is stopped rather than hanging the panel.
const CODE_RUN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// `COD-18`: which runtime a `code` artifact is for, judged from its title's
/// extension first and its content second. `None` when it is neither Python
/// nor JavaScript, or cannot be told apart.
pub fn code_language(title: &str, content: &str) -> Option<&'static str> {
    let t = title.to_ascii_lowercase();
    if t.ends_with(".py") {
        return Some("python");
    }
    if t.ends_with(".js") || t.ends_with(".mjs") || t.ends_with(".cjs") {
        return Some("node");
    }
    if t.ends_with(".ts") || t.ends_with(".rs") || t.ends_with(".go") || t.ends_with(".html") {
        return None;
    }
    let py = ["def ", "import ", "print(", "elif ", "self.", "if __name__"]
        .iter()
        .filter(|m| content.contains(*m))
        .count();
    let js = ["console.log", "function ", "const ", "let ", "=>", "require(", "process."]
        .iter()
        .filter(|m| content.contains(*m))
        .count();
    match py.cmp(&js) {
        std::cmp::Ordering::Greater => Some("python"),
        std::cmp::Ordering::Less => Some("node"),
        std::cmp::Ordering::Equal => None,
    }
}

/// What running a code artifact produced.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CodeRun {
    pub language: String,
    pub output: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub outcome: String,
    pub diagnostics: Vec<super::diagnostics::Diagnostic>,
    pub duration_ms: u64,
}

/// `COD-18`: run a `code` artifact in a throwaway folder, with the snippet
/// sandbox's scrubbed environment and the task runner's streaming and
/// diagnostics, so a traceback reads the same here as in a project.
pub async fn run_code(artifact: &crate::db::Artifact) -> Result<CodeRun, String> {
    if artifact.kind != "code" {
        return Err(format!("\"{}\" is a {} artifact, not code.", artifact.title, artifact.kind));
    }
    let language = code_language(&artifact.title, &artifact.content).ok_or_else(|| {
        format!("I can only run Python or JavaScript, and \"{}\" is not clearly either.", artifact.title)
    })?;
    let (program, file) = if language == "python" { ("python", "main.py") } else { ("node", "main.js") };
    let dir = std::env::temp_dir().join(format!("poiesis-artifact-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't create a folder to run it in: {e}"))?;
    std::fs::write(dir.join(file), &artifact.content).map_err(|e| format!("couldn't write the code: {e}"))?;
    let profile = super::sandbox::Profile { timeout: CODE_RUN_TIMEOUT, ..super::sandbox::Profile::ad_hoc() };
    let result =
        super::sandbox::run_streaming(program, &[file.to_string()], &dir, &profile, None, |_| {}).await;
    let _ = std::fs::remove_dir_all(&dir);
    let out = result?;
    let report = super::diagnostics::parse(&out.output, out.exit_code);
    Ok(CodeRun {
        language: language.to_string(),
        outcome: if out.timed_out {
            format!("stopped after {}s", CODE_RUN_TIMEOUT.as_secs())
        } else {
            report.outcome.clone()
        },
        diagnostics: report
            .items
            .into_iter()
            // The file is ours, not the user's: name it after the artifact.
            .map(|mut d| {
                if d.file.ends_with(file) {
                    d.file = artifact.title.clone();
                }
                d
            })
            .collect(),
        output: out.output,
        exit_code: out.exit_code,
        timed_out: out.timed_out,
        duration_ms: out.duration_ms,
    })
}

/// Cap on the artifact body handed back by `read_artifact`. A page past this is
/// being read to be edited, and the model can window into it by asking for the
/// part it needs; dumping half a megabyte costs the rest of the conversation.
const MAX_READ_CHARS: usize = 60_000;

/// Console entries kept per artifact. The preview only ever re-runs from the
/// top, so the interesting lines are the newest ones.
const MAX_CONSOLE_ENTRIES: usize = 50;

/// One line the preview iframe reported (`ART-5`). `level` is a console method
/// name (`log`, `warn`, `error`) or `uncaught` for a thrown error.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConsoleEntry {
    pub level: String,
    pub text: String,
    /// `file:line:col` where the webview places it, when it says.
    pub source: Option<String>,
}

/// Process-wide, in-memory console buffers keyed by artifact id.
///
/// Deliberately not in the database and not on `ToolContext`: these are the
/// transient output of a preview that is open right now, they are worthless
/// after a restart, and threading them through every `run_agent` caller to
/// reach one toolset would be a lot of signature churn for a ring buffer.
static CONSOLE: Mutex<Option<HashMap<String, Vec<ConsoleEntry>>>> = Mutex::new(None);

/// Record what an artifact's preview printed. Called by the frontend bridge
/// each time the iframe reports; oldest entries fall off the front.
pub fn record_console(artifact_id: &str, entries: Vec<ConsoleEntry>) {
    let mut guard = CONSOLE.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    let buf = map.entry(artifact_id.to_string()).or_default();
    buf.extend(entries);
    if buf.len() > MAX_CONSOLE_ENTRIES {
        let drop = buf.len() - MAX_CONSOLE_ENTRIES;
        buf.drain(..drop);
    }
}

/// Drop an artifact's buffer — the preview reloaded, so what came before it is
/// from a version that no longer exists and would only mislead.
pub fn clear_console(artifact_id: &str) {
    if let Some(map) = CONSOLE.lock().unwrap().as_mut() {
        map.remove(artifact_id);
    }
}

/// What the preview has printed for this artifact, oldest first.
pub fn console_for(artifact_id: &str) -> Vec<ConsoleEntry> {
    CONSOLE
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(artifact_id).cloned())
        .unwrap_or_default()
}

/// Render an artifact's console for the model, or `None` when it printed
/// nothing. Errors are called out first: a page that threw is a page that
/// never finished running, and that is the fact the model needs before it
/// starts reasoning about the source.
fn console_brief(artifact_id: &str) -> Option<String> {
    let entries = console_for(artifact_id);
    if entries.is_empty() {
        return None;
    }
    let errors = entries.iter().filter(|e| e.level == "error" || e.level == "uncaught").count();
    let mut out = if errors > 0 {
        format!("\n\n--- preview console ({errors} error(s)) ---\n")
    } else {
        "\n\n--- preview console ---\n".to_string()
    };
    for e in &entries {
        out.push_str(&format!("[{}] {}", e.level, e.text));
        if let Some(src) = &e.source {
            out.push_str(&format!("  ({src})"));
        }
        out.push('\n');
    }
    Some(out)
}

/// The OpenAI tool schema advertised to the model for this toolset.
pub fn tool_specs() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "create_artifact",
                "description": "Render a NEW self-contained artifact in the side panel: a web page (html), vector graphic (svg), rich document (markdown), or code file. Use for anything the user should see rendered rather than described. To change something you already made, use update_artifact instead — do not create a second copy of it.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "description": "Short title for the artifact" },
                        "kind": {
                            "type": "string",
                            "enum": ["html", "svg", "markdown", "code"],
                            "description": "How to render the content"
                        },
                        "content": { "type": "string", "description": "The full artifact content" }
                    },
                    "required": ["title", "kind", "content"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "read_artifact",
                "description": "Read back an artifact you or an earlier turn already made, exactly as it currently stands in the Canvas panel. For an html artifact this also returns whatever its live preview printed to the console, including uncaught errors — read it before guessing why a page misbehaves. Artifacts are not files: never try to open one with read_file.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "The artifact's id, as listed in the conversation's artifact summary or returned by create_artifact" }
                    },
                    "required": ["id"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "check_preview",
                "description": "Actually run an artifact and report back what happened. An html artifact runs in a real browser: its console, anything it failed to load, and whether it drew anything at all. A Python or JavaScript code artifact runs in the sandbox: its output and any error, located. Use this after making or fixing one, and before telling the user it works — read_artifact only shows what the open Canvas panel happened to print, and shows nothing when nobody is looking at it.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "The artifact's id" },
                        "wait_ms": { "type": "number", "description": "How long to let the page run before reporting, in milliseconds. Default 800; raise it for a page that draws on a timer." }
                    },
                    "required": ["id"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "update_artifact",
                "description": "Replace an existing artifact's content in place, keeping its id and its spot in the Canvas panel. This is how you fix or revise something you already made. Pass the complete new content, not a patch or a diff. Read the artifact first so you are editing what is actually there.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "The artifact's id" },
                        "content": { "type": "string", "description": "The full replacement content" },
                        "title": { "type": "string", "description": "New title, if it should change. Omit to keep the current one." }
                    },
                    "required": ["id", "content"]
                }
            }
        }
    ])
}

/// Is this an Artifacts tool name?
pub fn handles(name: &str) -> bool {
    matches!(
        name,
        "create_artifact" | "read_artifact" | "update_artifact" | "check_preview"
    )
}

/// Human-readable (verb, target) for the timeline (§5.6 plain past-tense).
pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    let title = args.get("title").and_then(|t| t.as_str()).unwrap_or("artifact");
    let id = args.get("id").and_then(|t| t.as_str()).unwrap_or("artifact");
    match name {
        "create_artifact" => ("created".into(), title.to_string()),
        // The id is not what the user calls it, but `execute` has the row and
        // the timeline step is rewritten from its result note anyway.
        "read_artifact" => ("read".into(), id.to_string()),
        "update_artifact" => ("updated".into(), id.to_string()),
        "check_preview" => ("ran".into(), id.to_string()),
        other => (other.into(), title.to_string()),
    }
}

/// The system-prompt block listing what this conversation has in the Canvas.
///
/// Without it `read_artifact`/`update_artifact` are unusable: the model has no
/// way to learn an id it never saw. Only the newest few, and no content — this
/// is an index, not the artifacts themselves.
pub fn artifacts_brief(db: &crate::db::Db, conversation_id: &str) -> Option<String> {
    let mut rows = db.list_artifacts(conversation_id).ok()?;
    // Media is made and refined through its own path (`ART-1`), not by editing
    // a body of text, so listing it here would only invite a wrong tool call.
    rows.retain(|a| a.kind != "image" && a.kind != "video");
    if rows.is_empty() {
        return None;
    }
    let recent: Vec<_> = rows.iter().rev().take(10).collect();
    let mut brief = String::from(
        "Artifacts already in this conversation's Canvas panel. They live in the app, \
         not on disk — reach them with read_artifact/update_artifact, never with the file tools:\n",
    );
    for a in recent.into_iter().rev() {
        brief.push_str(&format!("- {} ({}) — id: {}\n", a.title, a.kind, a.id));
    }
    brief.push_str(
        "When the user asks you to change one of these, update_artifact it in place. \
         Creating a second artifact instead leaves them looking at the broken one.",
    );
    Some(brief)
}

/// Persist the artifact, emit it to the Canvas panel, and return a short receipt.
pub async fn execute(
    ctx: &ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    match name {
        "read_artifact" => read(ctx, args),
        "update_artifact" => update(ctx, args),
        "check_preview" => check_preview(ctx, args).await,
        _ => create(ctx, args),
    }
}

fn create(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let title = args
        .get("title")
        .and_then(|t| t.as_str())
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or("Untitled");
    let kind = args
        .get("kind")
        .and_then(|k| k.as_str())
        .filter(|k| KINDS.contains(k))
        .unwrap_or("markdown");
    let content = args
        .get("content")
        .and_then(|c| c.as_str())
        .ok_or("missing 'content' argument")?;

    let artifact = ctx
        .db
        .add_artifact(Some(ctx.conversation_id), title, kind, content, ctx.assistant_message_id)
        .map_err(|e| format!("couldn't save the artifact: {e}"))?;

    ctx.sink.artifact(&artifact.id, title, kind, content);
    let _ = ctx
        .db
        .log_activity(Some(ctx.conversation_id), "artifact", &format!("created {title}"));

    Ok(format!(
        "Created the {kind} artifact \"{title}\" and opened it in the Canvas panel. \
         Its id is {} — to change it later, call update_artifact with that id rather than \
         making another one.",
        artifact.id
    ))
}

/// Look up the artifact this call names, keeping the error useful: a model that
/// guessed an id needs to be told what the real ones are, not just "not found".
fn lookup(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<crate::db::Artifact, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("missing 'id' argument")?;

    if let Some(a) = ctx.db.get_artifact(id).map_err(|e| e.to_string())? {
        return Ok(a);
    }

    // A model that lost the id often still has the title right, so try that
    // before giving up. Newest match wins.
    let rows = ctx.db.list_artifacts(ctx.conversation_id).map_err(|e| e.to_string())?;
    if let Some(a) = rows.iter().rev().find(|a| a.title.eq_ignore_ascii_case(id)) {
        return Ok(a.clone());
    }

    let known: Vec<String> = rows
        .iter()
        .rev()
        .take(10)
        .map(|a| format!("{} (id: {})", a.title, a.id))
        .collect();
    if known.is_empty() {
        Err("There is no artifact by that id, and this conversation has none yet.".to_string())
    } else {
        Err(format!(
            "There is no artifact \"{id}\". This conversation has: {}.",
            known.join(", ")
        ))
    }
}

fn read(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let a = lookup(ctx, args)?;

    let mut body = a.content.clone();
    let truncated = body.chars().count() > MAX_READ_CHARS;
    if truncated {
        body = body.chars().take(MAX_READ_CHARS).collect();
    }

    super::toolsets::set_step_note(ctx, format!("read {}", a.title));

    let mut out = format!("Artifact \"{}\" ({}), id {}:\n\n{}", a.title, a.kind, a.id, body);
    if truncated {
        out.push_str("\n\n[truncated — the artifact is longer than this]");
    }
    if let Some(console) = console_brief(&a.id) {
        out.push_str(&console);
    } else if a.kind == "html" {
        // `ART-6`: an empty buffer is not evidence of a working page — it is
        // just as likely that nobody has the Canvas open. Say which, or the
        // model reads silence as success.
        out.push_str(
            "\n\n[No console output recorded. That means nothing was captured, not that \
             nothing went wrong — the Canvas panel may not be open. Call check_preview to \
             actually run the page.]",
        );
    }
    Ok(out)
}

fn update(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let a = lookup(ctx, args)?;
    let content = args
        .get("content")
        .and_then(|c| c.as_str())
        .ok_or("missing 'content' argument")?;
    let title = args
        .get("title")
        .and_then(|t| t.as_str())
        .map(str::trim)
        .filter(|t| !t.is_empty());

    ctx.db
        .update_artifact(&a.id, title, content)
        .map_err(|e| format!("couldn't save the change: {e}"))?;

    let new_title = title.unwrap_or(&a.title);
    // The old console belongs to the old code. Clearing it here means the next
    // `read_artifact` shows what *this* version did, not a mix of both.
    clear_console(&a.id);
    ctx.sink.artifact(&a.id, new_title, &a.kind, content);
    let _ = ctx
        .db
        .log_activity(Some(ctx.conversation_id), "artifact", &format!("updated {new_title}"));

    super::toolsets::set_step_note(ctx, format!("updated {new_title}"));
    Ok(format!(
        "Updated \"{new_title}\" in place — the Canvas panel is showing the new version."
    ))
}

/// `ART-6`: run the artifact and say what happened.
///
/// This exists because `read_artifact`'s console (`ART-5`) is whatever the
/// *user's* open Canvas panel happened to print. If the panel was never opened,
/// if the run is unattended, or if the model wants to check a fix it just made
/// without waiting for someone to look — there is nothing there, and "no
/// console output" reads exactly like "no errors". Here the page is actually
/// run, so silence means silence.
async fn check_preview(
    ctx: &ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<String, String> {
    let a = lookup(ctx, args)?;
    if a.kind == "code" {
        return check_code(ctx, &a).await;
    }
    if a.kind != "html" {
        return Err(format!(
            "\"{}\" is a {} artifact — only an html page or a code artifact can be run.",
            a.title, a.kind
        ));
    }
    let url = super::preview::url_for(&a.id)
        .ok_or("the preview server isn't running, so there's nothing to open")?;
    let pool = ctx
        .browser_pool
        .ok_or("running a preview needs the browser, which isn't available right now")?;

    let wait = args
        .get("wait_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(800)
        .clamp(0, 10_000);

    let report = super::browser::inspect_local(
        pool,
        ctx.conversation_id,
        ctx.data_dir,
        &url,
        std::time::Duration::from_millis(wait),
    )
    .await?;

    let errors = report.errors();
    super::toolsets::set_step_note(
        ctx,
        if errors > 0 {
            format!("ran {} — {errors} error(s)", a.title)
        } else {
            format!("ran {}", a.title)
        },
    );
    let _ = ctx.db.log_activity(
        Some(ctx.conversation_id),
        "artifact",
        &format!("ran {} ({errors} error(s))", a.title),
    );

    Ok(render_report(&a.title, &url, &report))
}

/// `COD-18`: `check_preview` for a code artifact. Running code is the Code
/// Execution toolset's to allow, so it is refused here when that is off rather
/// than let in through the Canvas.
async fn check_code(ctx: &ToolContext<'_>, a: &crate::db::Artifact) -> Result<String, String> {
    if !super::toolsets::Toolset::CodeExec.is_enabled(ctx.db) {
        return Err("Running code is switched off (Settings → Tools → Code execution), so I can't run this artifact. Say that it has not been run.".into());
    }
    let run = run_code(a).await?;
    super::toolsets::set_step_note(ctx, format!("ran {} \u{2014} {}", a.title, run.outcome));
    let _ = ctx.db.log_activity(Some(ctx.conversation_id), "artifact", &format!("ran {}: {}", a.title, run.outcome));
    let report = super::diagnostics::Report {
        errors: 0,
        warnings: 0,
        failed: 0,
        passed: None,
        outcome: run.outcome.clone(),
        tail: run.output.lines().rev().take(30).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"),
        items: run.diagnostics.clone(),
    };
    Ok(super::diagnostics::render_for_model(&a.title, &report, run.exit_code))
}

/// The report as the model reads it. Facts first, in the order they decide what
/// to do next: did it leave, did it draw, what did it say.
fn render_report(
    title: &str,
    url: &str,
    report: &crate::agent::browser::PageReport,
) -> String {
    let mut out = format!("Ran \"{title}\" in a real browser.\n");

    // A page that navigated itself away is describing somewhere else entirely,
    // and every other line below would be about the wrong document.
    if !report.final_url.starts_with(url) {
        out.push_str(&format!(
            "\nThe page navigated itself away, to {}. Nothing below describes your artifact.\n",
            report.final_url
        ));
    }

    out.push_str(&format!("\nTitle: {}\n", if report.title.is_empty() { "(none)" } else { &report.title }));
    if report.elements <= 3 && report.text.is_empty() {
        out.push_str(
            "The page rendered nothing at all — an empty body. Whatever should have built \
             the DOM never ran.\n",
        );
    } else if report.text.is_empty() {
        out.push_str(&format!(
            "{} elements, but no visible text. It may be drawing to a canvas, or everything \
             may be hidden or positioned off-screen.\n",
            report.elements
        ));
    } else {
        out.push_str(&format!(
            "{} elements. Visible text begins: \"{}\"\n",
            report.elements, report.text
        ));
    }

    if report.console.is_empty() {
        out.push_str("\nThe console stayed silent — nothing logged, nothing thrown, nothing failed to load.\n");
        return out;
    }

    let errors = report.errors();
    out.push_str(&if errors > 0 {
        format!("\n--- console ({errors} error(s)) ---\n")
    } else {
        "\n--- console ---\n".to_string()
    });
    for e in &report.console {
        out.push_str(&format!("[{}] {}", e.level, e.text));
        if let Some(src) = &e.source {
            out.push_str(&format!("  ({src})"));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_artifacts_language_comes_from_its_title_then_its_content() {
        assert_eq!(code_language("fib.py", ""), Some("python"));
        assert_eq!(code_language("server.js", ""), Some("node"));
        assert_eq!(code_language("main.rs", "print(1)"), None);
        assert_eq!(code_language("Fibonacci", "def fib(n):\n    return n\nprint(fib(3))"), Some("python"));
        assert_eq!(code_language("Counter", "const n = 3;\nconsole.log(n)"), Some("node"));
        assert_eq!(code_language("Notes", "just words"), None);
    }
    use crate::agent::browser::PageReport;

    fn report(elements: u64, text: &str, console: Vec<ConsoleEntry>) -> PageReport {
        PageReport {
            title: "Pac-Man".into(),
            final_url: "http://127.0.0.1:1/tok/a".into(),
            text: text.into(),
            elements,
            console,
        }
    }

    /// The report the whole feature exists for: "black screen, just the header
    /// shows" has to arrive as a fact the model can act on.
    #[test]
    fn an_empty_page_is_reported_as_empty() {
        let out = render_report("Pac-Man", "http://127.0.0.1:1/tok/a", &report(2, "", vec![]));
        assert!(out.contains("rendered nothing at all"));
        assert!(out.contains("The console stayed silent"));
    }

    #[test]
    fn a_canvas_game_is_not_mistaken_for_a_blank_page() {
        let out = render_report("Pac-Man", "http://127.0.0.1:1/tok/a", &report(40, "", vec![]));
        assert!(!out.contains("rendered nothing at all"));
        assert!(out.contains("drawing to a canvas"));
    }

    #[test]
    fn errors_are_counted_and_quoted_verbatim() {
        let out = render_report(
            "Pac-Man",
            "http://127.0.0.1:1/tok/a",
            &report(
                40,
                "Score: 0",
                vec![
                    ConsoleEntry { level: "log".into(), text: "booting".into(), source: None },
                    ConsoleEntry {
                        level: "uncaught".into(),
                        text: "ctx is not defined".into(),
                        source: Some("line 12:5".into()),
                    },
                ],
            ),
        );
        assert!(out.contains("1 error(s)"));
        assert!(out.contains("ctx is not defined"));
        assert!(out.contains("(line 12:5)"));
    }

    /// A page that navigates itself somewhere else is describing another
    /// document — saying so first stops the model debugging the wrong thing.
    #[test]
    fn a_page_that_navigates_away_says_so_before_anything_else() {
        let mut r = report(10, "Sign in", vec![]);
        r.final_url = "https://example.com/login".into();
        let out = render_report("Pac-Man", "http://127.0.0.1:1/tok/a", &r);
        let away = out.find("navigated itself away").unwrap();
        assert!(away < out.find("Title:").unwrap());
        assert!(out.contains("https://example.com/login"));
    }

    #[test]
    fn console_keeps_only_the_newest_entries() {
        let id = "art-ring-test";
        clear_console(id);
        for i in 0..(MAX_CONSOLE_ENTRIES + 10) {
            record_console(
                id,
                vec![ConsoleEntry { level: "log".into(), text: format!("{i}"), source: None }],
            );
        }
        let got = console_for(id);
        assert_eq!(got.len(), MAX_CONSOLE_ENTRIES);
        assert_eq!(got[0].text, "10");
        clear_console(id);
    }

    #[test]
    fn the_brief_calls_out_errors_first() {
        let id = "art-brief-test";
        clear_console(id);
        record_console(
            id,
            vec![ConsoleEntry {
                level: "uncaught".into(),
                text: "x is not defined".into(),
                source: Some("artifact:12:5".into()),
            }],
        );
        let brief = console_brief(id).unwrap();
        assert!(brief.contains("1 error(s)"));
        assert!(brief.contains("x is not defined"));
        clear_console(id);
    }
}
