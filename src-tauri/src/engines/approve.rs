//! One permission panel for every agent app (`CLI-6`).
//!
//! An agent app asks before it runs a command or edits a file. The question is
//! put on the same panel everything else uses, headed with the agent's name, and
//! the answer fails closed: a timeout, Stop, a withdrawn question and a run with
//! nobody there are all a "no".

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;

use crate::agent::decide::toolgate;
use crate::agent::run::AgentEventSink;
use crate::db::Db;
use crate::permissions::{Decision, PermissionManager, PermissionRequest, Trust};
use crate::runtime::proxy::CancelFlag;

/// How long a question waits for a person. After that it is a no, so a run whose
/// user walked away does not hold its slot until the wall clock ends it.
pub const ASK_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The longest diff or command shown on the panel.
const SHOW_MAX: usize = 6000;

/// What an agent's tool does, in the three ways that matter to a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    /// Looks and changes nothing: read, list, search.
    Read,
    /// Writes or changes a file.
    Edit,
    /// Runs a program.
    Command,
    /// Reaches the network.
    Fetch,
    /// Anything else the agent has, including its helpers.
    Other,
}

pub fn kind_of(tool: &str) -> ToolKind {
    match tool {
        "Read" | "Glob" | "Grep" | "LS" | "NotebookRead" | "TodoWrite" | "BashOutput" => ToolKind::Read,
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" | "Patch" | "Delete" => ToolKind::Edit,
        "Bash" | "PowerShell" => ToolKind::Command,
        "WebFetch" | "WebSearch" => ToolKind::Fetch,
        _ => ToolKind::Other,
    }
}

fn s<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(|v| v.as_str())
}

fn clip(text: &str, max: usize) -> String {
    let t = text.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    t.chars().take(max).collect::<String>() + "…"
}

fn file_of(input: &Value) -> Option<&str> {
    s(input, "file_path").or_else(|| s(input, "path")).or_else(|| s(input, "notebook_path"))
}

/// Is the tool's path outside the folder? A relative path is read from the folder
/// the agent works in (its own working folder), never from Poiesis's.
fn outside(input: &Value, cwd: &Path) -> bool {
    file_of(input).is_some_and(|p| !crate::permissions::path_within_root(&cwd.join(p), cwd))
}

/// The path as the user knows it: relative to the folder when it is inside it.
pub fn shown_path(p: &str, cwd: &Path) -> String {
    Path::new(p).strip_prefix(cwd).map(|r| r.to_string_lossy().to_string()).unwrap_or_else(|_| p.to_string())
}

/// `(verb, target)` for the timeline, in the same past tense every other step
/// uses (`StepStart { verb, target }`). A name nobody mapped still reads sensibly.
pub fn describe_tool(tool: &str, input: &Value, cwd: &Path) -> (String, String) {
    let path = |i: &Value| file_of(i).map(|p| shown_path(p, cwd)).unwrap_or_default();
    match tool {
        "Bash" | "PowerShell" => ("ran".into(), clip(s(input, "command").unwrap_or(""), 80)),
        "Write" => ("wrote".into(), path(input)),
        "Edit" | "MultiEdit" | "NotebookEdit" | "Patch" => ("edited".into(), path(input)),
        "Delete" => ("deleted".into(), path(input)),
        "Read" | "NotebookRead" => ("read".into(), path(input)),
        "Grep" | "Glob" => ("searched".into(), clip(s(input, "pattern").unwrap_or(""), 60)),
        "LS" => ("listed".into(), path(input)),
        "Task" => ("started a helper".into(), clip(s(input, "description").unwrap_or(""), 60)),
        "WebFetch" => ("fetched".into(), clip(s(input, "url").unwrap_or(""), 80)),
        "WebSearch" => ("searched the web for".into(), clip(s(input, "query").unwrap_or(""), 60)),
        "TodoWrite" => ("updated".into(), "its to-do list".into()),
        // Poiesis's own tools (`MCPS-2`), which Claude Code names `mcp__poiesis__recall`.
        other if super::tools::short_name(other).is_some() => {
            super::tools::describe(super::tools::short_name(other).unwrap_or(other), input)
        }
        other => (other.to_ascii_lowercase(), String::new()),
    }
}

/// The question's sentence: first person to the person reading it.
fn sentence(engine: &str, tool: &str, input: &Value, cwd: &Path) -> String {
    let where_ = if outside(input, cwd) { " (outside the folder it was given)" } else { "" };
    match kind_of(tool) {
        ToolKind::Command => format!("{engine} wants to run a command."),
        ToolKind::Edit => {
            let name = file_of(input).map(|p| shown_path(p, cwd)).unwrap_or_else(|| "a file".into());
            let verb = match tool {
                "Write" if !file_of(input).is_some_and(|p| cwd.join(p).exists()) => "create",
                "Delete" => "delete",
                _ => "edit",
            };
            format!("{engine} wants to {verb} {name}{where_}.")
        }
        ToolKind::Fetch => format!("{engine} wants to reach the internet."),
        ToolKind::Read => format!("{engine} wants to read {}{where_}.", file_of(input).map(|p| shown_path(p, cwd)).unwrap_or_else(|| "a file".into())),
        ToolKind::Other => format!("{engine} wants to use {tool}."),
    }
}

/// What an edit would change, as text the panel shows in a box.
fn edit_diff(tool: &str, input: &Value) -> Option<String> {
    let lines = |prefix: &str, text: &str| text.lines().map(|l| format!("{prefix}{l}")).collect::<Vec<_>>().join("\n");
    let one = |old: &str, new: &str| {
        let mut out = String::new();
        if !old.is_empty() {
            out.push_str(&lines("- ", old));
            out.push('\n');
        }
        out.push_str(&lines("+ ", new));
        out
    };
    let text = match tool {
        "Write" => lines("+ ", s(input, "content")?),
        "Edit" => one(s(input, "old_string").unwrap_or(""), s(input, "new_string")?),
        "MultiEdit" => input
            .get("edits")?
            .as_array()?
            .iter()
            .filter_map(|e| Some(one(s(e, "old_string").unwrap_or(""), s(e, "new_string")?)))
            .collect::<Vec<_>>()
            .join("\n\u{22EF}\n"),
        "NotebookEdit" => lines("+ ", s(input, "new_source")?),
        // An agent that already sends its change as a diff (Codex): shown as it is.
        "Patch" => s(input, "diff")?.to_string(),
        _ => return None,
    };
    Some(clip(&text, SHOW_MAX))
}

/// The key "Allow for this run" remembers: the tool and the exact thing.
fn remember_key(tool: &str, input: &Value) -> String {
    match kind_of(tool) {
        ToolKind::Command => format!("{tool}:{}", s(input, "command").unwrap_or("")),
        ToolKind::Edit => format!("{tool}:{}", file_of(input).unwrap_or("")),
        _ => format!("{tool}:{input}"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    /// The words the agent reads as the tool's result.
    Deny(String),
}

impl Verdict {
    /// The JSON Claude Code's permission-prompt tool contract wants.
    pub fn to_claude_json(&self, input: &Value) -> String {
        match self {
            Verdict::Allow => serde_json::json!({ "behavior": "allow", "updatedInput": input }).to_string(),
            Verdict::Deny(why) => serde_json::json!({ "behavior": "deny", "message": why }).to_string(),
        }
    }
}

/// `USE-12`: what a request is compared with, so a quick decision can say when it
/// is not part of the task. It adds one line to the question and nothing else: an
/// agent app owns its own loop.
pub struct Look<'a> {
    pub db: &'a Db,
    /// The conversation the decision is logged and counted under.
    pub conversation_id: String,
    pub run_id: String,
    /// The task the agent app was handed.
    pub task: String,
}

/// Where one agent run's questions go.
pub struct Port<'a> {
    pub perms: &'a PermissionManager,
    pub sink: &'a AgentEventSink,
    pub engine: String,
    pub cwd: PathBuf,
    pub trust: Trust,
    /// Nobody is there to answer.
    pub headless: bool,
    look: Option<Look<'a>>,
    remembered: Mutex<HashSet<String>>,
    pub asked: AtomicU32,
    pub allowed: AtomicU32,
    pub denied: AtomicU32,
}

impl<'a> Port<'a> {
    pub fn new(
        perms: &'a PermissionManager,
        sink: &'a AgentEventSink,
        engine: &str,
        cwd: &Path,
        trust: Trust,
        headless: bool,
    ) -> Self {
        Port {
            perms,
            sink,
            engine: engine.to_string(),
            cwd: cwd.to_path_buf(),
            trust,
            headless,
            look: None,
            remembered: Mutex::new(HashSet::new()),
            asked: AtomicU32::new(0),
            allowed: AtomicU32::new(0),
            denied: AtomicU32::new(0),
        }
    }

    /// Let a quick decision read each question against the task (`USE-12`).
    pub fn with_look(mut self, look: Look<'a>) -> Self {
        self.look = Some(look);
        self
    }

    /// Decide one request. `gone` is set when the question is no longer wanted
    /// (the agent hung up, or the run was stopped): the panel entry is withdrawn.
    pub async fn decide(&self, tool: &str, input: &Value, gone: &CancelFlag, stop: &CancelFlag) -> Verdict {
        self.asked.fetch_add(1, Ordering::Relaxed);
        let verdict = self.decide_inner(tool, input, gone, stop).await;
        match verdict {
            Verdict::Allow => self.allowed.fetch_add(1, Ordering::Relaxed),
            Verdict::Deny(_) => self.denied.fetch_add(1, Ordering::Relaxed),
        };
        verdict
    }

    async fn decide_inner(&self, tool: &str, input: &Value, gone: &CancelFlag, stop: &CancelFlag) -> Verdict {
        let kind = kind_of(tool);
        // Looking is allowed at every level. A read that needs asking about is
        // one outside the folder, and that goes through the checks below.
        if kind == ToolKind::Read && !outside(input, &self.cwd) {
            return Verdict::Allow;
        }
        if self.trust == Trust::ReadOnly {
            return Verdict::Deny("Not allowed: this folder is read only for you, so you can look but not change anything.".into());
        }
        if self.headless {
            return Verdict::Deny("Not allowed: nobody is here to approve it.".into());
        }
        let key = remember_key(tool, input);
        if self.remembered.lock().unwrap().contains(&key) {
            return Verdict::Allow;
        }

        let id = format!("perm_{}", uuid::Uuid::new_v4());
        let rx = self.perms.open_request(&id);
        let argv = (kind == ToolKind::Command).then(|| vec![clip(s(input, "command").unwrap_or(""), SHOW_MAX)]);
        let diff = match kind {
            ToolKind::Edit => edit_diff(tool, input),
            // What is approved is shown up to the same length as a command.
            ToolKind::Other | ToolKind::Fetch => Some(clip(&input.to_string(), SHOW_MAX)),
            _ => None,
        };
        // `USE-12`: is it part of the task the agent was handed? One line, never a
        // question added or taken away.
        let scoped = match &self.look {
            Some(look) => {
                let detail = argv.as_ref().map(|a| a.join(" ")).or_else(|| diff.clone()).unwrap_or_default();
                toolgate::scope_of(
                    look.db,
                    &look.conversation_id,
                    Some(&look.run_id),
                    false,
                    &look.task,
                    &toolgate::engine_what(&self.engine, tool, &detail),
                    false,
                )
                .await
            }
            None => None,
        };
        self.sink.send_permission(
            PermissionRequest::engine_ask(
                id.clone(),
                self.engine.clone(),
                tool.to_string(),
                sentence(&self.engine, tool, input, &self.cwd),
                self.cwd.to_string_lossy().to_string(),
                argv,
                diff,
            )
            .with_reason(scoped.as_ref().map(|s| s.flag.line()))
            .with_reason_hover(scoped.as_ref().map(|s| s.flag.hover())),
        );

        let answer = tokio::select! {
            d = rx => d.ok(),
            _ = until(gone, stop) => None,
            _ = tokio::time::sleep(ASK_TIMEOUT) => None,
        };
        match answer {
            Some(Decision::Once) => Verdict::Allow,
            Some(Decision::Chat) | Some(Decision::Forever) => {
                self.remembered.lock().unwrap().insert(key);
                Verdict::Allow
            }
            Some(Decision::Deny) => {
                // `CTL-3`: a no to a request a quick decision called off the task.
                if let (Some(look), Some(s)) = (&self.look, &scoped) {
                    let _ = look.db.add_outcome(&s.decision, "right", None, "engine-card");
                }
                Verdict::Deny("Not allowed: the user declined.".into())
            }
            None => {
                // Withdraw what is still on the panel, then say no.
                self.perms.resolve(&id, Decision::Deny);
                self.sink.emit(crate::agent::AgentEvent::PermissionGone { id });
                Verdict::Deny("Not allowed: the question was withdrawn or nobody answered in time.".into())
            }
        }
    }
}

/// Resolves when either flag is set.
pub(super) async fn until(a: &CancelFlag, b: &CancelFlag) {
    while !a.is_cancelled() && !b.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tools_are_sorted_by_what_they_do() {
        assert_eq!(kind_of("Read"), ToolKind::Read);
        assert_eq!(kind_of("Write"), ToolKind::Edit);
        assert_eq!(kind_of("Bash"), ToolKind::Command);
        assert_eq!(kind_of("PowerShell"), ToolKind::Command, "Erich's machine has this tool");
        assert_eq!(kind_of("WebFetch"), ToolKind::Fetch);
        assert_eq!(kind_of("SomethingNew"), ToolKind::Other, "an unknown tool is asked about");
    }

    #[test]
    fn steps_read_in_the_timelines_own_words() {
        let cwd = Path::new("C:\\proj");
        let step = |t: &str, i: Value| describe_tool(t, &i, cwd);
        assert_eq!(step("Bash", json!({"command":"cargo test"})), ("ran".into(), "cargo test".into()));
        assert_eq!(step("PowerShell", json!({"command":"Get-Date"})).0, "ran");
        assert_eq!(step("Edit", json!({"file_path":"C:\\proj\\src\\a.rs"})), ("edited".into(), "src\\a.rs".into()));
        assert_eq!(step("Write", json!({"file_path":"C:\\other\\b.txt"})), ("wrote".into(), "C:\\other\\b.txt".into()));
        assert_eq!(step("Read", json!({"file_path":"C:\\proj\\x.md"})).0, "read");
        assert_eq!(step("Grep", json!({"pattern":"TODO"})), ("searched".into(), "TODO".into()));
        assert_eq!(step("Task", json!({"description":"look around"})).0, "started a helper");
        assert_eq!(step("Mystery", json!({})), ("mystery".into(), String::new()));
    }

    #[test]
    fn an_edit_shows_what_it_changes_and_a_write_shows_what_it_adds() {
        let d = edit_diff("Edit", &json!({"old_string":"a\nb","new_string":"c"})).unwrap();
        assert_eq!(d, "- a\n- b\n+ c");
        let w = edit_diff("Write", &json!({"content":"hello\nworld"})).unwrap();
        assert_eq!(w, "+ hello\n+ world");
        assert!(edit_diff("Bash", &json!({"command":"ls"})).is_none());
        let long = edit_diff("Write", &json!({"content":"x".repeat(20_000)})).unwrap();
        assert!(long.chars().count() <= SHOW_MAX + 1);
    }

    #[test]
    fn the_question_says_who_wants_what_and_flags_a_path_outside_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let inside = dir.path().join("a.txt");
        let c = sentence("Claude Code", "Bash", &json!({"command":"ls"}), dir.path());
        assert_eq!(c, "Claude Code wants to run a command.");
        let create = sentence("Claude Code", "Write", &json!({"file_path": inside.to_string_lossy()}), dir.path());
        assert_eq!(create, "Claude Code wants to create a.txt.");
        std::fs::write(&inside, "x").unwrap();
        assert!(sentence("Claude Code", "Write", &json!({"file_path": inside.to_string_lossy()}), dir.path()).contains("edit a.txt"));
        let elsewhere = tempfile::tempdir().unwrap();
        let out = sentence("Claude Code", "Edit", &json!({"file_path": elsewhere.path().join("z.txt").to_string_lossy()}), dir.path());
        assert!(out.contains("outside the folder"), "{out}");
    }

    /// A relative path is read from the agent's folder: inside is inside, and `..`
    /// out of it is still outside.
    #[test]
    fn a_relative_path_is_read_from_the_agents_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("a.rs"), "x").unwrap();
        assert!(!outside(&json!({"file_path": "src/a.rs"}), dir.path()));
        assert!(!outside(&json!({"path": "."}), dir.path()));
        assert!(outside(&json!({"file_path": "../elsewhere.txt"}), dir.path()));
        assert!(!outside(&json!({"pattern": "TODO"}), dir.path()), "no path at all is not outside");
        let edit = sentence("Codex", "Write", &json!({"file_path": "src/a.rs"}), dir.path());
        assert_eq!(edit, "Codex wants to edit src/a.rs.", "the file exists in the agent's folder");
    }

    #[test]
    fn the_verdict_is_the_json_claude_code_reads() {
        let input = json!({"command":"ls"});
        let allow: Value = serde_json::from_str(&Verdict::Allow.to_claude_json(&input)).unwrap();
        assert_eq!(allow, json!({"behavior":"allow","updatedInput":{"command":"ls"}}));
        let deny: Value = serde_json::from_str(&Verdict::Deny("no".into()).to_claude_json(&input)).unwrap();
        assert_eq!(deny, json!({"behavior":"deny","message":"no"}));
    }

    #[test]
    fn what_a_run_remembers_is_the_tool_and_the_exact_thing() {
        assert_ne!(remember_key("Bash", &json!({"command":"ls"})), remember_key("Bash", &json!({"command":"ls -la"})));
        assert_eq!(remember_key("Edit", &json!({"file_path":"a","old_string":"1"})), remember_key("Edit", &json!({"file_path":"a","old_string":"2"})));
        assert_ne!(remember_key("Edit", &json!({"file_path":"a"})), remember_key("Write", &json!({"file_path":"a"})));
    }
}
