//! `COD-6`..`COD-8`: running a project's own build and tests.
//!
//! This is the execution half the coding plan was missing. `run_code` runs a
//! snippet in a throwaway folder; it structurally cannot run `cargo check`.
//! `run_task` runs a task the project itself declares (`project::detect`), in
//! the project folder, confined by the same Job Object machinery, with its
//! output streamed to the timeline and parsed into diagnostics.
//!
//! Two layers decide whether anything runs, and both must agree:
//!
//! - **what may run at all** — the project's execution policy crossed with its
//!   trust level (`gate`). A read-only project runs nothing, because a build
//!   writes to `target/` and `node_modules/` and pretending otherwise is the
//!   same dishonesty `COD-9` removed from the sandbox docs;
//! - **when it must ask** — the permission prompt, with a per-task "always
//!   allow in this project".
//!
//! `run_command` is the free-form escape hatch: expert mode only, the project
//! opted in, an argument vector and never a shell string, and it asks unless
//! that exact `command argv[0]` pair was remembered.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::db::{Db, Project};
use crate::permissions::{Decision, PermissionRequest, Trust};

use super::project::{self, ProjectCard, TaskKind};
use super::sandbox;
use super::toolsets::{set_step_note, ToolContext};

/// Settings → Tools: what a project that has not chosen does.
pub const DEFAULT_POLICY_KEY: &str = "code_run.default_policy";
/// Settings → Tools, expert only: whether `run_command` exists at all.
pub const RUN_COMMAND_KEY: &str = "code_run.run_command";
const EXPERT_KEY: &str = "ui.expert";
/// How often a running task's newest line is sent to the timeline.
const OUTPUT_THROTTLE_MS: u128 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    Off,
    Ask,
    Allow,
}

impl Policy {
    pub fn parse(s: &str) -> Option<Policy> {
        match s {
            "off" => Some(Policy::Off),
            "ask" => Some(Policy::Ask),
            "allow" => Some(Policy::Allow),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Policy::Off => "off",
            Policy::Ask => "ask",
            Policy::Allow => "allow",
        }
    }
}

/// The Settings default, `ask` when unset.
pub fn default_policy(db: &Db) -> Policy {
    db.get_setting(DEFAULT_POLICY_KEY)
        .ok()
        .flatten()
        .and_then(|v| Policy::parse(&v))
        .unwrap_or(Policy::Ask)
}

/// The project's own choice, else the default.
pub fn project_policy(db: &Db, project: &Project) -> Policy {
    Policy::parse(&project.exec_policy).unwrap_or_else(|| default_policy(db))
}

/// What the user said "always allow" to in one project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Allow {
    #[serde(default)]
    pub tasks: Vec<String>,
    /// `command argv[0]` pairs, e.g. `git status`.
    #[serde(default)]
    pub commands: Vec<String>,
    /// The project opted in to `run_command` (`COD-8`).
    #[serde(default)]
    pub run_command: bool,
}

pub fn allow_of(project: &Project) -> Allow {
    project
        .allow_json
        .as_deref()
        .and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or_default()
}

pub fn save_allow(db: &Db, project_id: &str, allow: &Allow) {
    if let Ok(json) = serde_json::to_string(allow) {
        let _ = db.set_project_allow(project_id, Some(&json));
    }
}

/// What the gate decided for one task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    Refuse(String),
    Ask,
    Run,
}

/// Everything `gate` looks at, as plain data so the rules can be tested.
pub struct GateInput {
    pub has_folder: bool,
    pub trust: Trust,
    pub policy: Policy,
    /// The project itself says `allow`, rather than inheriting it.
    pub explicit_allow: bool,
    pub headless: bool,
    pub remembered: bool,
}

/// `COD-7`: may a declared task run here, and must it ask first?
pub fn gate(input: &GateInput) -> Gate {
    if !input.has_folder {
        return Gate::Refuse(
            "This project has no folder, so there is nothing to run a task in. A project with no folder runs no tasks."
                .into(),
        );
    }
    if input.trust == Trust::ReadOnly {
        return Gate::Refuse(
            "This project's folder is attached read-only, so no task runs in it: a build writes to its folder. \
             Tell the user they can change access in the Workbench panel if they want it verified."
                .into(),
        );
    }
    if input.policy == Policy::Off {
        return Gate::Refuse(
            "Running tasks is switched off for this project. Say plainly that you have not verified the change."
                .into(),
        );
    }
    if input.headless {
        // An unattended run has nobody to ask, and "ask" must never quietly
        // become "run". Only a project the user explicitly set to allow runs.
        return if input.explicit_allow {
            Gate::Run
        } else {
            Gate::Refuse(
                "Unattended runs only run tasks in a project set to \"Allow\", and this one is not.".into(),
            )
        };
    }
    if input.policy == Policy::Allow || input.remembered {
        Gate::Run
    } else {
        Gate::Ask
    }
}

/// `COD-8`: why a free-form command may not run, if it may not.
///
/// A command is a program and an argument vector. Anything that would let the
/// model tunnel a shell string past that — a shell, an interpreter's eval
/// flag, shell metacharacters — is refused, as is anything the agent never
/// does on its own: commit, push, install globally.
pub fn command_refusal(command: &str, args: &[String]) -> Option<String> {
    const METACHARS: [char; 9] = ['|', '&', ';', '<', '>', '`', '$', '\n', '^'];
    let program = Path::new(command)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(command)
        .to_ascii_lowercase();
    const SHELLS: [&str; 12] =
        ["sh", "bash", "zsh", "fish", "dash", "cmd", "powershell", "pwsh", "wsl", "env", "sudo", "runas"];
    if SHELLS.contains(&program.as_str()) || program == "start" || program == "xargs" {
        return Some(format!("`{command}` is a shell or a launcher. Run the program you mean directly, with its arguments."));
    }
    if command.contains(METACHARS) || args.iter().any(|a| a.contains(METACHARS)) {
        return Some(
            "Shell metacharacters (| & ; < > ` $ ^) are not allowed: nothing here runs through a shell. Pass the command and its arguments separately."
                .into(),
        );
    }
    let eval_flags: &[&str] = match program.as_str() {
        "python" | "python3" | "py" => &["-c"],
        "node" | "deno" | "bun" => &["-e", "--eval", "-p", "--print"],
        "perl" | "ruby" => &["-e"],
        _ => &[],
    };
    if args.iter().any(|a| eval_flags.contains(&a.as_str())) {
        return Some(format!("`{command} {}` runs code from a string. Write the code to a file in the project, or use run_code.", args[0]));
    }
    let first = args.first().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    if program == "git" && matches!(first.as_str(), "commit" | "push") {
        return Some("I never commit or push on my own. Tell the user what to commit instead.".into());
    }
    let installs = matches!(first.as_str(), "install" | "i" | "add");
    if installs && args.iter().any(|a| a == "-g" || a == "--global") || program == "cargo" && first == "install" {
        return Some("I never install anything globally. Ask the user to do it if it is needed.".into());
    }
    None
}

/// The allowlist key for a command: `command argv[0]`, so `git status`
/// never grants `git push`.
pub fn command_key(command: &str, args: &[String]) -> String {
    match args.first() {
        Some(first) => format!("{command} {first}"),
        None => command.to_string(),
    }
}

/// What `ToolRegistry` should advertise for a conversation: `(run_task,
/// run_command)`. An `off` or read-only project is not offered a tool that can
/// only refuse; everything is re-checked when a call arrives anyway.
pub fn advertised(db: &Db, conversation_id: &str) -> (bool, bool) {
    let Some(project) = db.conversation_project(conversation_id).ok().flatten() else {
        return (false, false);
    };
    let usable = project.root_path.is_some()
        && Trust::parse(&project.trust) != Trust::ReadOnly
        && project_policy(db, &project) != Policy::Off;
    let flag = |key: &str| db.get_setting(key).ok().flatten().is_some_and(|v| v == "true");
    let commands = usable && flag(EXPERT_KEY) && flag(RUN_COMMAND_KEY) && allow_of(&project).run_command;
    (usable, commands)
}

/// Whether a run in this conversation can verify a change by running a task,
/// for the project brief (`COD-2`) and the nudge (`COD-12`).
pub fn can_verify(db: &Db, conversation_id: &str, headless: bool) -> bool {
    if !super::toolsets::Toolset::CodeRun.is_enabled(db) {
        return false;
    }
    let (run_task, _) = advertised(db, conversation_id);
    if !run_task {
        return false;
    }
    !headless
        || db
            .conversation_project(conversation_id)
            .ok()
            .flatten()
            .is_some_and(|p| p.exec_policy == "allow")
}

pub fn tool_specs() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "run_task",
                "description": "Run one of the tasks this project declares (its build, type check, tests or linter) in the project folder, and get back the outcome and the diagnostics it printed. This is how you find out whether a change actually works: run the check after editing code, read the result, fix what it reports. `task` must be one of the names listed in the project block of your instructions, exactly.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "task": { "type": "string", "description": "The task's name, e.g. \"cargo check\" or \"npm run test\"." },
                        "args": { "type": "array", "items": { "type": "string" }, "description": "OPTIONAL extra arguments appended to the task, e.g. a test name filter." }
                    },
                    "required": ["task"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "run_command",
                "description": "Run a program that is not one of the project's tasks, in the project folder. Always shown to the user first. Give the program and its arguments separately; nothing runs through a shell, so pipes, redirects and && do not work. Never use it to commit, push or install globally.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "The program, e.g. \"git\"." },
                        "args": { "type": "array", "items": { "type": "string" }, "description": "Its arguments, one per item, e.g. [\"status\", \"--short\"]." }
                    },
                    "required": ["command"]
                }
            }
        }
    ])
}

pub fn handles(name: &str) -> bool {
    matches!(name, "run_task" | "run_command")
}

/// The words a task's step reads (`COD-UI-2`): what it is for, not its name.
pub fn kind_words(kind: TaskKind, task: &str) -> (String, String) {
    match kind {
        TaskKind::Build => ("built".into(), "the project".into()),
        TaskKind::Check => ("checked".into(), "the project".into()),
        TaskKind::Test => ("ran".into(), "the tests".into()),
        TaskKind::Lint => ("linted".into(), "the project".into()),
        TaskKind::Other => ("ran".into(), task.to_string()),
    }
}

/// `describe` has no database, so the kind is read off the name the way
/// detection named it.
fn kind_from_name(task: &str) -> TaskKind {
    let t = task.to_ascii_lowercase();
    if t.contains("test") {
        TaskKind::Test
    } else if t.contains("lint") || t.contains("clippy") || t.contains("ruff") {
        TaskKind::Lint
    } else if t.contains("check") || t.contains(" vet") || t.contains("mypy") || t.contains("tsc") {
        TaskKind::Check
    } else if t.contains("build") {
        TaskKind::Build
    } else {
        TaskKind::Other
    }
}

pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    match name {
        "run_task" => {
            let task = args.get("task").and_then(|t| t.as_str()).unwrap_or("a task");
            kind_words(kind_from_name(task), task)
        }
        "run_command" => {
            let command = args.get("command").and_then(|c| c.as_str()).unwrap_or("a command");
            let first = args
                .get("args")
                .and_then(|a| a.as_array())
                .and_then(|a| a.first())
                .and_then(|a| a.as_str())
                .map(|a| format!(" {a}"))
                .unwrap_or_default();
            ("ran".into(), format!("{command}{first}"))
        }
        other => (other.into(), String::new()),
    }
}

fn string_args(args: &serde_json::Value) -> Vec<String> {
    args.get("args")
        .and_then(|a| a.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

async fn ask(
    ctx: &ToolContext<'_>,
    kind: &'static str,
    summary: String,
    cwd: &Path,
    argv: Vec<String>,
    project: &Project,
    remember: String,
) -> Decision {
    let id = format!("perm_{}", uuid::Uuid::new_v4());
    let rx = ctx.perms.open_request(&id);
    ctx.sink.send_permission(PermissionRequest::execution(
        id,
        kind,
        summary,
        cwd.to_string_lossy().to_string(),
        argv,
        project.name.clone(),
        sandbox::Profile::task().timeout.as_secs(),
        remember,
    ));
    rx.await.unwrap_or(Decision::Deny)
}

/// Run `argv` in `cwd` with the timeline wired up: the start, the newest line
/// now and then, and the parsed end. Shared by both tools.
async fn run_streamed(
    ctx: &ToolContext<'_>,
    label: &str,
    argv: &[String],
    cwd: &Path,
    display_cwd: &str,
    kind: TaskKind,
) -> Result<String, String> {
    let profile = sandbox::Profile::task();
    ctx.sink.emit(super::AgentEvent::TaskStarted {
        id: ctx.call_id.to_string(),
        task: label.to_string(),
        argv: argv.to_vec(),
        cwd: display_cwd.to_string(),
        kind,
        timeout_secs: profile.timeout.as_secs(),
    });

    let mut last_sent = std::time::Instant::now() - std::time::Duration::from_secs(1);
    let call_id = ctx.call_id.to_string();
    let sink = ctx.sink;
    let result = sandbox::run_streaming(&argv[0], &argv[1..], cwd, &profile, ctx.cancel, |line| {
        if line.trim().is_empty() || last_sent.elapsed().as_millis() < OUTPUT_THROTTLE_MS {
            return;
        }
        last_sent = std::time::Instant::now();
        sink.emit(super::AgentEvent::TaskOutput { id: call_id.clone(), line: line.to_string() });
    })
    .await;

    let out = match result {
        Ok(out) => out,
        Err(e) => {
            ctx.sink.emit(super::AgentEvent::TaskEnded {
                id: ctx.call_id.to_string(),
                outcome: "could not start".into(),
                exit_code: None,
                timed_out: false,
                cancelled: false,
                duration_ms: 0,
                diagnostics: Vec::new(),
                tail: e.clone(),
            });
            return Err(e);
        }
    };

    let report = super::diagnostics::parse(&out.output, out.exit_code);
    let outcome = if out.cancelled {
        "stopped".to_string()
    } else if out.timed_out {
        format!("stopped after {}s", profile.timeout.as_secs())
    } else {
        report.outcome.clone()
    };
    ctx.sink.emit(super::AgentEvent::TaskEnded {
        id: ctx.call_id.to_string(),
        outcome: outcome.clone(),
        exit_code: out.exit_code,
        timed_out: out.timed_out,
        cancelled: out.cancelled,
        duration_ms: out.duration_ms,
        diagnostics: report.items.clone(),
        tail: report.tail.clone(),
    });
    set_step_note(ctx, format!("\u{2014} {outcome}"));
    let _ = ctx.db.log_activity(Some(ctx.conversation_id), "task", &format!("ran {label}: {outcome}"));
    if kind != TaskKind::Other {
        ctx.ledger.record_check();
    }

    if out.cancelled {
        return Err("The user stopped this task.".into());
    }
    let mut text = if out.timed_out {
        format!(
            "`{label}` ran longer than its {}-second limit and was stopped. Output so far is below.\n",
            profile.timeout.as_secs()
        )
    } else {
        String::new()
    };
    if out.truncated {
        text.push_str("(The output was long; only its end was kept.)\n");
    }
    text.push_str(&super::diagnostics::render_for_model(label, &report, out.exit_code));
    Ok(text)
}

struct Place {
    project: Project,
    card: ProjectCard,
    root: std::path::PathBuf,
}

fn place(ctx: &ToolContext<'_>) -> Result<Place, String> {
    let project = ctx
        .db
        .conversation_project(ctx.conversation_id)
        .ok()
        .flatten()
        .ok_or("This conversation is not in a project with a folder, so there are no tasks to run.")?;
    let root = project
        .root_path
        .clone()
        .map(std::path::PathBuf::from)
        .ok_or("This project has no folder, so there is nothing to run a task in.")?;
    let card = project::card_for(ctx.db, &project).unwrap_or_default();
    Ok(Place { project, card, root })
}

pub async fn execute(ctx: &ToolContext<'_>, name: &str, args: &serde_json::Value) -> Result<String, String> {
    match name {
        "run_task" => run_task(ctx, args).await,
        "run_command" => run_command(ctx, args).await,
        other => Err(format!("unknown tool '{other}'")),
    }
}

async fn run_task(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let Place { project, card, root } = place(ctx)?;
    let wanted = args.get("task").and_then(|t| t.as_str()).ok_or("missing 'task'")?;
    let Some(task) = card.task(wanted).cloned() else {
        let names: Vec<String> = card.tasks.iter().map(|t| format!("\"{}\"", t.name)).collect();
        return Err(if names.is_empty() {
            "This project declares no tasks I can run (no package.json scripts, Cargo.toml, pyproject.toml, go.mod, Makefile or .sln was found).".into()
        } else {
            format!("\"{wanted}\" is not one of this project's tasks. Use one of: {}.", names.join(", "))
        });
    };
    let extra = string_args(args);
    if let Some(bad) = extra.iter().find(|a| a.contains(['|', '&', ';', '<', '>', '`', '$', '\n', '^'])) {
        return Err(format!("The argument {bad:?} has a shell metacharacter in it. Nothing here runs through a shell."));
    }

    let mut allow = allow_of(&project);
    let decision = gate(&GateInput {
        has_folder: true,
        trust: Trust::parse(&project.trust),
        policy: project_policy(ctx.db, &project),
        explicit_allow: project.exec_policy == "allow",
        headless: ctx.headless,
        remembered: allow.tasks.iter().any(|t| t == &task.name),
    });
    let mut argv = task.argv.clone();
    argv.extend(extra);
    let cwd = if task.cwd.is_empty() { root.clone() } else { root.join(&task.cwd) };

    match decision {
        Gate::Refuse(why) => return Err(why),
        Gate::Run => {}
        Gate::Ask => {
            let (verb, what) = kind_words(task.kind, &task.name);
            let question = match task.kind {
                TaskKind::Test => "Run the project's tests?".to_string(),
                TaskKind::Other => format!("Run {}?", task.name),
                _ => format!("{} {what}?", match verb.as_str() {
                    "built" => "Build",
                    "checked" => "Check",
                    "linted" => "Lint",
                    _ => "Run",
                }),
            };
            match ask(ctx, "task", question, &cwd, argv.clone(), &project, task.name.clone()).await {
                Decision::Deny => return Err(format!("The user chose not to run `{}`.", task.name)),
                Decision::Forever => {
                    allow.tasks.push(task.name.clone());
                    save_allow(ctx.db, &project.id, &allow);
                }
                Decision::Once | Decision::Chat => {}
            }
        }
    }

    run_streamed(ctx, &task.name, &argv, &cwd, &task.cwd, task.kind).await
}

async fn run_command(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let Place { project, root, .. } = place(ctx)?;
    let (_, offered) = advertised(ctx.db, ctx.conversation_id);
    if !offered {
        return Err("Running arbitrary commands is not enabled for this project. Use run_task with one of its tasks.".into());
    }
    if ctx.headless {
        return Err("A free-form command always asks first, and an unattended run cannot ask.".into());
    }
    let command = args.get("command").and_then(|c| c.as_str()).ok_or("missing 'command'")?.trim().to_string();
    let rest = string_args(args);
    if command.is_empty() {
        return Err("missing 'command'".into());
    }
    if let Some(why) = command_refusal(&command, &rest) {
        return Err(why);
    }
    let key = command_key(&command, &rest);
    let mut allow = allow_of(&project);
    if !allow.commands.iter().any(|c| c == &key) {
        let mut argv = vec![command.clone()];
        argv.extend(rest.iter().cloned());
        match ask(ctx, "command", format!("Run a command in {}?", project.name), &root, argv, &project, key.clone()).await {
            Decision::Deny => return Err(format!("The user chose not to run `{key}`.")),
            Decision::Forever => {
                allow.commands.push(key.clone());
                save_allow(ctx.db, &project.id, &allow);
            }
            Decision::Once | Decision::Chat => {}
        }
    }
    let mut argv = vec![command.clone()];
    argv.extend(rest);
    let label = argv.join(" ");
    run_streamed(ctx, &label, &argv, &root, "", TaskKind::Other).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> GateInput {
        GateInput {
            has_folder: true,
            trust: Trust::Confirm,
            policy: Policy::Ask,
            explicit_allow: false,
            headless: false,
            remembered: false,
        }
    }

    /// `COD-7-T`: a read-only project refuses every task, whatever else is set.
    #[test]
    fn a_read_only_project_refuses_every_task() {
        for policy in [Policy::Off, Policy::Ask, Policy::Allow] {
            for remembered in [false, true] {
                let g = gate(&GateInput { trust: Trust::ReadOnly, policy, remembered, explicit_allow: true, ..input() });
                assert!(matches!(g, Gate::Refuse(ref why) if why.contains("read-only")), "{policy:?}: {g:?}");
            }
        }
        assert!(matches!(gate(&GateInput { has_folder: false, ..input() }), Gate::Refuse(_)));
    }

    /// `COD-7-T2`: headless refuses unless the project explicitly allows.
    #[test]
    fn headless_refuses_unless_explicitly_allowed() {
        assert!(matches!(gate(&GateInput { headless: true, ..input() }), Gate::Refuse(_)));
        assert!(matches!(
            gate(&GateInput { headless: true, remembered: true, ..input() }),
            Gate::Refuse(_)
        ), "a remembered task still never runs unattended under ask");
        assert!(matches!(
            gate(&GateInput { headless: true, policy: Policy::Allow, explicit_allow: false, ..input() }),
            Gate::Refuse(_)
        ), "allow inherited from Settings is not the project saying so");
        assert_eq!(gate(&GateInput { headless: true, policy: Policy::Allow, explicit_allow: true, ..input() }), Gate::Run);
    }

    #[test]
    fn ask_asks_until_remembered_and_allow_runs() {
        assert_eq!(gate(&input()), Gate::Ask);
        assert_eq!(gate(&GateInput { remembered: true, ..input() }), Gate::Run);
        assert_eq!(gate(&GateInput { policy: Policy::Allow, ..input() }), Gate::Run);
        assert!(matches!(gate(&GateInput { policy: Policy::Off, ..input() }), Gate::Refuse(_)));
    }

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    /// `COD-8-T`: shell metacharacters and banned prefixes are refused.
    #[test]
    fn shells_metacharacters_and_eval_flags_are_refused() {
        assert!(command_refusal("cmd", &v(&["/c", "dir"])).is_some());
        assert!(command_refusal("powershell.exe", &v(&["-Command", "ls"])).is_some());
        assert!(command_refusal("bash", &v(&["-c", "ls"])).is_some());
        assert!(command_refusal("git", &v(&["log", "|", "head"])).is_some());
        assert!(command_refusal("git status && rm", &[]).is_some());
        assert!(command_refusal("python", &v(&["-c", "print(1)"])).is_some());
        assert!(command_refusal("node", &v(&["--eval", "1"])).is_some());
        assert!(command_refusal("git", &v(&["push"])).is_some());
        assert!(command_refusal("git", &v(&["commit", "-m", "x"])).is_some());
        assert!(command_refusal("npm", &v(&["install", "-g", "left-pad"])).is_some());
        assert!(command_refusal("cargo", &v(&["install", "ripgrep"])).is_some());

        assert!(command_refusal("git", &v(&["status", "--short"])).is_none());
        assert!(command_refusal("python", &v(&["scripts/gen.py"])).is_none());
        assert!(command_refusal("npm", &v(&["install"])).is_none(), "a local install is the project's own business");
    }

    /// `COD-8-T2`: the allowlist matches on `argv[0]`, so a `git status` grant
    /// does not let `git push` through.
    #[test]
    fn a_git_status_grant_does_not_cover_git_push() {
        let allow = Allow { commands: vec![command_key("git", &v(&["status"]))], ..Default::default() };
        let remembered = |cmd: &str, args: &[&str]| allow.commands.contains(&command_key(cmd, &v(args)));
        assert!(remembered("git", &["status", "--short"]));
        assert!(!remembered("git", &["push", "origin"]));
        assert!(!remembered("git", &["log"]));
    }

    #[test]
    fn a_task_step_says_what_the_task_is_for() {
        assert_eq!(describe("run_task", &serde_json::json!({"task": "cargo test (in src-tauri)"})), ("ran".into(), "the tests".into()));
        assert_eq!(describe("run_task", &serde_json::json!({"task": "npm run build"})), ("built".into(), "the project".into()));
        assert_eq!(describe("run_command", &serde_json::json!({"command": "git", "args": ["status"]})), ("ran".into(), "git status".into()));
    }

    #[test]
    fn the_allowlist_round_trips_through_the_project_row() {
        let db = Db::open_in_memory().unwrap();
        let p = db.create_project("p", Some("C:/p")).unwrap();
        assert_eq!(allow_of(&p), Allow::default());
        save_allow(&db, &p.id, &Allow { tasks: vec!["cargo check".into()], commands: vec![], run_command: true });
        let back = allow_of(&db.get_project(&p.id).unwrap().unwrap());
        assert_eq!(back.tasks, vec!["cargo check"]);
        assert!(back.run_command);
        assert_eq!(project_policy(&db, &p), Policy::Ask, "a new project inherits the default");
        db.set_setting(DEFAULT_POLICY_KEY, "off").unwrap();
        assert_eq!(project_policy(&db, &p), Policy::Off);
    }
}
