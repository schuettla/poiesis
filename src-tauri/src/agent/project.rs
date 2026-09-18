//! `COD-1`..`COD-3`: what the agent knows about a project's folder before it
//! reads a single file.
//!
//! The card is detection, not understanding: file extensions counted, a
//! handful of manifests parsed shallowly, the git branch read off `HEAD`, and
//! whether the project wrote instructions for agents. Nothing here runs a
//! program. It is cached on the project row (`projects.card_json`) and rebuilt
//! when the folder changes or the user asks.
//!
//! The tasks listed here are the whole of what `run_task` may run (`COD-6`).
//! A task is a program and its arguments, never a shell string, and the only
//! way one gets onto the list is by the project declaring it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::db::{Db, Project};

/// Files counted before the language tally stops. A monorepo with a million
/// files still gets an answer, just from the first part of the walk.
const LANGUAGE_FILE_CAP: usize = 5000;
/// `COD-2`: the project block's cap, near 400 tokens.
pub const PROJECT_BLOCK_CAP: usize = 1600;
/// `COD-3`: how much of `AGENTS.md` goes into the prompt.
pub const INSTRUCTIONS_CAP: usize = 8000;

/// What a task is for. Decides which one counts as "the check" (`COD-12`) and
/// what its timeline step reads (`COD-UI-2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Build,
    Check,
    Test,
    Lint,
    Other,
}

/// One task the project declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// What the model passes to `run_task`, and what the user reads: the
    /// command as typed, with the folder it runs in when that is not the root.
    pub name: String,
    /// Program and arguments. Never a shell string.
    pub argv: Vec<String>,
    /// Relative to the project root, `/`-separated. Empty for the root.
    #[serde(default)]
    pub cwd: String,
    pub kind: TaskKind,
    /// The file that declared it, e.g. `package.json`.
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCard {
    /// Most files first. Code languages only: markup and data do not say what
    /// a project is written in.
    #[serde(default)]
    pub languages: Vec<String>,
    /// Manifests found, relative to the root.
    #[serde(default)]
    pub manifests: Vec<String>,
    #[serde(default)]
    pub package_managers: Vec<String>,
    #[serde(default)]
    pub tasks: Vec<Task>,
    /// True when the root is a git work tree.
    #[serde(default)]
    pub git: bool,
    #[serde(default)]
    pub branch: Option<String>,
    /// `AGENTS.md` or `CLAUDE.md`, whichever the project has (the first wins).
    #[serde(default)]
    pub instructions_file: Option<String>,
    #[serde(default)]
    pub readme: bool,
}

impl ProjectCard {
    /// Detection found nothing worth saying. The block is then absent rather
    /// than an empty scaffold (`COD-2`).
    pub fn is_empty(&self) -> bool {
        self.languages.is_empty()
            && self.tasks.is_empty()
            && !self.git
            && self.instructions_file.is_none()
    }

    pub fn task(&self, name: &str) -> Option<&Task> {
        let wanted = name.trim();
        self.tasks.iter().find(|t| t.name == wanted)
    }

    /// `COD-12`: the task that answers "does it still work". A type check
    /// first, because it is the fastest honest answer; then tests; then a
    /// build.
    pub fn check_task(&self) -> Option<&Task> {
        [TaskKind::Check, TaskKind::Test, TaskKind::Build]
            .iter()
            .find_map(|k| self.tasks.iter().find(|t| t.kind == *k))
    }
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Build the card for a folder. Reads the root and its immediate children, so
/// a repository with its Rust crate in `src-tauri/` is still seen whole.
pub fn detect(root: &Path) -> ProjectCard {
    let mut card = ProjectCard {
        languages: languages(root),
        ..Default::default()
    };

    let mut dirs = vec![(root.to_path_buf(), String::new())];
    if let Ok(entries) = std::fs::read_dir(root) {
        let mut children: Vec<(PathBuf, String)> = entries
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                (!super::filesystem::is_ignored(&name, false)).then(|| (e.path(), name))
            })
            .collect();
        children.sort();
        dirs.extend(children);
    }
    for (dir, rel) in &dirs {
        detect_dir(dir, rel, &mut card);
    }

    card.git = root.join(".git").exists();
    card.branch = card.git.then(|| git_branch(root)).flatten();
    card.instructions_file = ["AGENTS.md", "CLAUDE.md"]
        .into_iter()
        .find(|f| root.join(f).is_file())
        .map(str::to_string);
    card.readme = ["README.md", "README", "readme.md", "README.txt"]
        .iter()
        .any(|f| root.join(f).is_file());
    card
}

fn detect_dir(dir: &Path, rel: &str, card: &mut ProjectCard) {
    let manifest = |name: &str| -> String {
        if rel.is_empty() { name.to_string() } else { format!("{rel}/{name}") }
    };
    let push = |card: &mut ProjectCard, argv: &[&str], kind: TaskKind, source: &str| {
        let command = argv.join(" ");
        let name = if rel.is_empty() { command } else { format!("{command} (in {rel})") };
        if card.tasks.iter().any(|t| t.name == name) {
            return;
        }
        card.tasks.push(Task {
            name,
            argv: argv.iter().map(|s| s.to_string()).collect(),
            cwd: rel.to_string(),
            kind,
            source: source.to_string(),
        });
    };
    let manager = |card: &mut ProjectCard, m: &str| {
        if !card.package_managers.iter().any(|x| x == m) {
            card.package_managers.push(m.to_string());
        }
    };

    // package.json: its scripts, run through the package manager its lockfile
    // names. A dev server never exits, so it is not a task.
    if let Some(json) = read_json(&dir.join("package.json")) {
        card.manifests.push(manifest("package.json"));
        let pm = if dir.join("pnpm-lock.yaml").exists() {
            "pnpm"
        } else if dir.join("yarn.lock").exists() {
            "yarn"
        } else if dir.join("bun.lockb").exists() || dir.join("bun.lock").exists() {
            "bun"
        } else {
            "npm"
        };
        manager(card, pm);
        if let Some(scripts) = json.get("scripts").and_then(|s| s.as_object()) {
            for name in scripts.keys() {
                let Some(kind) = script_kind(name) else { continue };
                let argv: Vec<&str> = match pm {
                    "yarn" => vec!["yarn", name.as_str()],
                    other => vec![other, "run", name.as_str()],
                };
                push(card, &argv, kind, &manifest("package.json"));
            }
        }
    }

    if dir.join("Cargo.toml").is_file() {
        card.manifests.push(manifest("Cargo.toml"));
        manager(card, "cargo");
        let src = manifest("Cargo.toml");
        push(card, &["cargo", "check"], TaskKind::Check, &src);
        push(card, &["cargo", "test"], TaskKind::Test, &src);
        push(card, &["cargo", "build"], TaskKind::Build, &src);
        push(card, &["cargo", "clippy"], TaskKind::Lint, &src);
    }

    if dir.join("pyproject.toml").is_file() {
        card.manifests.push(manifest("pyproject.toml"));
        let text = std::fs::read_to_string(dir.join("pyproject.toml")).unwrap_or_default();
        let pm = if dir.join("uv.lock").exists() {
            "uv"
        } else if text.contains("[tool.poetry") {
            "poetry"
        } else {
            "pip"
        };
        manager(card, pm);
        let src = manifest("pyproject.toml");
        if text.contains("pytest") || dir.join("tests").is_dir() {
            push(card, &["python", "-m", "pytest"], TaskKind::Test, &src);
        }
        if text.contains("[tool.ruff") || text.contains("ruff") {
            push(card, &["python", "-m", "ruff", "check", "."], TaskKind::Lint, &src);
        }
        if text.contains("[tool.mypy") || text.contains("mypy") {
            push(card, &["python", "-m", "mypy", "."], TaskKind::Check, &src);
        }
    }

    if dir.join("go.mod").is_file() {
        card.manifests.push(manifest("go.mod"));
        manager(card, "go");
        let src = manifest("go.mod");
        push(card, &["go", "build", "./..."], TaskKind::Build, &src);
        push(card, &["go", "vet", "./..."], TaskKind::Check, &src);
        push(card, &["go", "test", "./..."], TaskKind::Test, &src);
    }

    if let Ok(text) = std::fs::read_to_string(dir.join("Makefile")) {
        card.manifests.push(manifest("Makefile"));
        manager(card, "make");
        for target in make_targets(&text) {
            let Some(kind) = script_kind(&target) else { continue };
            push(card, &["make", target.as_str()], kind, &manifest("Makefile"));
        }
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut slns: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.to_ascii_lowercase().ends_with(".sln"))
            .collect();
        slns.sort();
        if let Some(sln) = slns.first() {
            card.manifests.push(manifest(sln));
            manager(card, "dotnet");
            let src = manifest(sln);
            push(card, &["dotnet", "build"], TaskKind::Build, &src);
            push(card, &["dotnet", "test"], TaskKind::Test, &src);
        }
    }
}

/// What a script or make target is for, judged by its name. `None` for the
/// ones that never finish (a dev server, a watcher) and so cannot be a task.
fn script_kind(name: &str) -> Option<TaskKind> {
    let n = name.to_ascii_lowercase();
    let never_ends = ["dev", "start", "serve", "watch", "preview"];
    if never_ends.iter().any(|w| n == *w || n.starts_with(&format!("{w}:")) || n.ends_with(&format!(":{w}"))) {
        return None;
    }
    if n.starts_with("pre") || n.starts_with("post") {
        // npm lifecycle hooks run on their own around the script they name.
        return None;
    }
    Some(if n.contains("test") {
        TaskKind::Test
    } else if n.contains("lint") || n.contains("clippy") || n.contains("format:check") {
        TaskKind::Lint
    } else if n.contains("typecheck") || n.contains("type-check") || n == "check" || n.contains("tsc") {
        TaskKind::Check
    } else if n.contains("build") {
        TaskKind::Build
    } else {
        TaskKind::Other
    })
}

/// The explicit targets of a Makefile: `name:` at the start of a line, not a
/// variable assignment, not a pattern rule, not a special target.
fn make_targets(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        if line.starts_with(['\t', ' ', '#', '.']) {
            continue;
        }
        let Some((head, rest)) = line.split_once(':') else { continue };
        if rest.starts_with('=') || head.contains(['%', '$', '=', ' ']) || head.is_empty() {
            continue;
        }
        if !out.iter().any(|t| t == head) {
            out.push(head.to_string());
        }
    }
    out
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// The current branch, read off `HEAD` rather than asked of git: a folder
/// with no git installed still has a branch.
fn git_branch(root: &Path) -> Option<String> {
    let dot_git = root.join(".git");
    let git_dir = if dot_git.is_file() {
        // A worktree or submodule: `.git` is a file naming the real directory.
        let text = std::fs::read_to_string(&dot_git).ok()?;
        let target = text.trim().strip_prefix("gitdir:")?.trim();
        let p = PathBuf::from(target);
        if p.is_absolute() { p } else { root.join(p) }
    } else {
        dot_git
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref:") {
        Some(r) => Some(r.trim().trim_start_matches("refs/heads/").to_string()),
        // Detached: name the commit, short.
        None => Some(format!("detached at {}", &head[..head.len().min(7)])),
    }
}

fn language_for(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "rs" => "Rust",
        "ts" | "tsx" | "mts" | "cts" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "py" => "Python",
        "go" => "Go",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "cs" => "C#",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => "C++",
        "c" | "h" => "C",
        "rb" => "Ruby",
        "php" => "PHP",
        "swift" => "Swift",
        "dart" => "Dart",
        "scala" => "Scala",
        "lua" => "Lua",
        "zig" => "Zig",
        "vue" => "Vue",
        "svelte" => "Svelte",
        _ => return None,
    })
}

/// Code languages by file count, most first. A language under 5% of the code
/// files is left off: one build script does not make a project Python.
fn languages(root: &Path) -> Vec<String> {
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut seen = 0usize;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            if seen >= LANGUAGE_FILE_CAP {
                break;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if super::filesystem::is_ignored(&name, false) {
                continue;
            }
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Some(lang) = path
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x.to_ascii_lowercase())
                .and_then(|x| language_for(&x))
            {
                seen += 1;
                *counts.entry(lang).or_default() += 1;
            }
        }
    }
    let total: usize = counts.values().sum();
    let mut ranked: Vec<(&str, usize)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    ranked
        .into_iter()
        .filter(|(_, n)| *n * 20 >= total)
        .take(4)
        .map(|(l, _)| l.to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// The cached card
// ---------------------------------------------------------------------------

/// The card for a project with a folder: cached when there is one, detected
/// and stored when there is not. `None` for a project with no folder.
pub fn card_for(db: &Db, project: &Project) -> Option<ProjectCard> {
    let root = project.root_path.as_deref()?;
    if let Some(card) = project
        .card_json
        .as_deref()
        .and_then(|j| serde_json::from_str::<ProjectCard>(j).ok())
    {
        return Some(card);
    }
    Some(refresh(db, project.id.as_str(), Path::new(root)))
}

/// Detect again and store the result (`COD-1`, "on demand").
pub fn refresh(db: &Db, project_id: &str, root: &Path) -> ProjectCard {
    let card = detect(root);
    if let Ok(json) = serde_json::to_string(&card) {
        let _ = db.set_project_card(project_id, Some(&json));
    }
    card
}

/// The project, and its card, for a conversation working in a folder.
pub fn for_conversation(db: &Db, conversation_id: &str) -> Option<(Project, ProjectCard)> {
    let project = db.conversation_project(conversation_id).ok().flatten()?;
    let card = card_for(db, &project)?;
    Some((project, card))
}

// ---------------------------------------------------------------------------
// The prompt blocks
// ---------------------------------------------------------------------------

/// Whether `run_task` can actually run anything here, and if not, why. Passed
/// in rather than looked up so the block stays a pure function of its inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verify {
    /// `run_task` is on offer.
    CanRun,
    /// Tasks exist but nothing may run them (policy off, read-only, toolset
    /// off, or an unattended run).
    CannotRun,
}

fn tasks_of(card: &ProjectCard, kind: TaskKind) -> String {
    let names: Vec<&str> = card.tasks.iter().filter(|t| t.kind == kind).map(|t| t.name.as_str()).collect();
    if names.is_empty() { "-".to_string() } else { names.join(", ") }
}

/// `COD-2`: the compact project block. Empty when detection found nothing.
///
/// It sits in the working-folder brief rather than in `compose_system_prompt`:
/// every fact in it comes off the disk, which the frontend's mirror of the
/// assembly cannot see, and the brief is already the one place that describes
/// the folder a run works in. Every way a run starts gets it.
pub fn project_block(name: &str, card: &ProjectCard, verify: Verify) -> String {
    if card.is_empty() {
        return String::new();
    }
    let mut head = format!("Project: {name}");
    let mut facts = Vec::new();
    if !card.languages.is_empty() {
        facts.push(card.languages.join(" + "));
    }
    if let Some(branch) = &card.branch {
        facts.push(format!("git branch {branch}"));
    } else if card.git {
        facts.push("git".to_string());
    }
    if !facts.is_empty() {
        head.push_str(&format!(" ({})", facts.join(", ")));
    }
    let mut lines = vec![head];
    if !card.tasks.is_empty() {
        lines.push(format!(
            "Build: {} | Check: {}",
            tasks_of(card, TaskKind::Build),
            tasks_of(card, TaskKind::Check)
        ));
        lines.push(format!(
            "Test: {} | Lint: {}",
            tasks_of(card, TaskKind::Test),
            tasks_of(card, TaskKind::Lint)
        ));
    }
    lines.push(match &card.instructions_file {
        Some(f) => format!("Conventions: {f} is present and has been read into context."),
        None => "Conventions: no AGENTS.md; follow the style of the code around your change.".to_string(),
    });
    if let Some(check) = card.check_task() {
        lines.push(match verify {
            Verify::CanRun => format!(
                "Read a file before you edit it. After changing code, run `{}` with run_task and read the result before saying it works. If you write a plan for code changes, end it with that step.",
                check.name
            ),
            Verify::CannotRun => format!(
                "Read a file before you edit it. You cannot run `{}` here, so when you change code, say plainly that you have not verified it.",
                check.name
            ),
        });
    } else {
        lines.push("Read a file before you edit it.".to_string());
    }
    let block = lines.join("\n");
    if block.chars().count() > PROJECT_BLOCK_CAP {
        let clipped: String = block.chars().take(PROJECT_BLOCK_CAP).collect();
        return format!("{clipped}…");
    }
    block
}

/// `COD-3`: the project's own instructions for agents, read from the root and
/// framed as project-authored. `AGENTS.md` is the user's file: this reads it
/// and nothing in the app ever writes it.
pub fn instructions_block(root: &Path, card: &ProjectCard) -> Option<String> {
    let file = card.instructions_file.as_deref()?;
    let text = std::fs::read_to_string(root.join(file)).ok()?;
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let body: String = if text.chars().count() > INSTRUCTIONS_CAP {
        format!("{}…(truncated)", text.chars().take(INSTRUCTIONS_CAP).collect::<String>())
    } else {
        text.to_string()
    };
    Some(format!(
        "## Project instructions ({file} in the project root, written by the project's authors; follow them for work in this project unless the user says otherwise)\n{body}"
    ))
}

/// `COD-2` and `COD-3` together, for a conversation: the project block, then
/// the project's own instructions. `None` when the conversation has no project
/// with a folder, or detection found nothing to say.
pub fn project_brief(db: &Db, conversation_id: &str, headless: bool) -> Option<String> {
    let (project, card) = for_conversation(db, conversation_id)?;
    let verify = if super::coderun::can_verify(db, conversation_id, headless) {
        Verify::CanRun
    } else {
        Verify::CannotRun
    };
    let block = project_block(&project.name, &card, verify);
    if block.is_empty() {
        return None;
    }
    let root = project.root_path.as_deref().map(Path::new)?;
    Some(match instructions_block(root, &card) {
        Some(instructions) => format!("{block}\n\n{instructions}"),
        None => block,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poiesis_card_{name}_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn names(card: &ProjectCard) -> Vec<&str> {
        card.tasks.iter().map(|t| t.name.as_str()).collect()
    }

    /// `COD-1-T`: a node project's scripts, through the manager its lockfile
    /// names, with the dev server left off.
    #[test]
    fn a_package_json_yields_its_scripts_but_not_the_dev_server() {
        let dir = scratch("node");
        std::fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"dev":"vite","build":"tsc && vite build","test":"vitest run","lint":"eslint ."}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("pnpm-lock.yaml"), "").unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.ts"), "").unwrap();

        let card = detect(&dir);
        assert_eq!(card.languages, vec!["TypeScript"]);
        assert_eq!(card.package_managers, vec!["pnpm"]);
        assert_eq!(names(&card), vec!["pnpm run build", "pnpm run lint", "pnpm run test"]);
        assert_eq!(card.task("pnpm run test").unwrap().argv, vec!["pnpm", "run", "test"]);
        assert_eq!(card.check_task().unwrap().name, "pnpm run test", "no typecheck, so tests are the check");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_cargo_crate_in_a_subfolder_is_found_and_runs_there() {
        let dir = scratch("cargo");
        std::fs::create_dir_all(dir.join("src-tauri/src")).unwrap();
        std::fs::write(dir.join("src-tauri/Cargo.toml"), "[package]\nname='x'").unwrap();
        std::fs::write(dir.join("src-tauri/src/lib.rs"), "").unwrap();

        let card = detect(&dir);
        let check = card.check_task().unwrap();
        assert_eq!(check.name, "cargo check (in src-tauri)");
        assert_eq!(check.cwd, "src-tauri");
        assert_eq!(card.manifests, vec!["src-tauri/Cargo.toml"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn python_go_make_and_dotnet_manifests_each_declare_tasks() {
        let py = scratch("py");
        std::fs::write(py.join("pyproject.toml"), "[tool.pytest.ini_options]\n[tool.ruff]\n").unwrap();
        let card = detect(&py);
        assert!(card.task("python -m pytest").is_some());
        assert!(card.task("python -m ruff check .").is_some());

        let go = scratch("go");
        std::fs::write(go.join("go.mod"), "module x").unwrap();
        assert_eq!(detect(&go).check_task().unwrap().name, "go vet ./...");

        let mk = scratch("make");
        std::fs::write(mk.join("Makefile"), ".PHONY: test\nVAR := 1\nbuild: deps\n\tcc x\ntest:\n\t./t\n%.o: %.c\n").unwrap();
        assert_eq!(names(&detect(&mk)), vec!["make build", "make test"]);

        let net = scratch("dotnet");
        std::fs::write(net.join("App.sln"), "").unwrap();
        assert!(detect(&net).task("dotnet test").is_some());

        for d in [py, go, mk, net] {
            std::fs::remove_dir_all(&d).ok();
        }
    }

    /// `COD-1-T`, the empty case: no manifest, no git, nothing to say.
    #[test]
    fn a_folder_with_no_manifest_has_an_empty_card() {
        let dir = scratch("empty");
        std::fs::write(dir.join("notes.txt"), "hello").unwrap();
        let card = detect(&dir);
        assert!(card.is_empty(), "{card:?}");
        assert_eq!(project_block("notes", &card, Verify::CanRun), "");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_branch_and_the_instructions_file_are_read_off_disk() {
        let dir = scratch("git");
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join(".git/HEAD"), "ref: refs/heads/feature/x\n").unwrap();
        std::fs::write(dir.join("CLAUDE.md"), "Use tabs.").unwrap();
        let card = detect(&dir);
        assert!(card.git);
        assert_eq!(card.branch.as_deref(), Some("feature/x"));
        assert_eq!(card.instructions_file.as_deref(), Some("CLAUDE.md"));
        let block = instructions_block(&dir, &card).unwrap();
        assert!(block.contains("written by the project's authors"));
        assert!(block.ends_with("Use tabs."));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `COD-2-T`: byte-stable for a fixture card.
    #[test]
    fn the_project_block_is_byte_stable() {
        let card = ProjectCard {
            languages: vec!["Rust".into(), "TypeScript".into()],
            manifests: vec!["package.json".into(), "src-tauri/Cargo.toml".into()],
            package_managers: vec!["npm".into(), "cargo".into()],
            tasks: vec![
                Task { name: "npm run build".into(), argv: vec![], cwd: String::new(), kind: TaskKind::Build, source: "package.json".into() },
                Task { name: "cargo check (in src-tauri)".into(), argv: vec![], cwd: "src-tauri".into(), kind: TaskKind::Check, source: "src-tauri/Cargo.toml".into() },
                Task { name: "cargo test (in src-tauri)".into(), argv: vec![], cwd: "src-tauri".into(), kind: TaskKind::Test, source: "src-tauri/Cargo.toml".into() },
            ],
            git: true,
            branch: Some("master".into()),
            instructions_file: Some("AGENTS.md".into()),
            readme: true,
        };
        assert_eq!(
            project_block("nexus", &card, Verify::CanRun),
            "Project: nexus (Rust + TypeScript, git branch master)\n\
             Build: npm run build | Check: cargo check (in src-tauri)\n\
             Test: cargo test (in src-tauri) | Lint: -\n\
             Conventions: AGENTS.md is present and has been read into context.\n\
             Read a file before you edit it. After changing code, run `cargo check (in src-tauri)` with run_task and read the result before saying it works. If you write a plan for code changes, end it with that step."
        );
        assert!(project_block("nexus", &card, Verify::CannotRun).contains("say plainly that you have not verified it"));
    }
}
