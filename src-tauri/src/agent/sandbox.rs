//! A confined subprocess for the Code Execution toolset (TOOL-5) and for
//! project tasks (`COD-6`).
//!
//! What the confinement actually is, stated plainly (`COD-9`), because a
//! sandbox that claims more than it does is worse than none:
//!
//! **Enforced on Windows**, by a dedicated Win32 Job Object per run (separate
//! from the engine's global job):
//! - a per-process **memory cap**,
//! - an **active-process cap**, so a fork loop stops at the limit,
//! - **kill-on-close lifetime**: when the run overruns, is stopped, or its
//!   future is dropped, the job handle closes and every process in the tree
//!   dies with it, children included,
//! - a **wall-clock timeout**.
//!
//! **Arranged, not enforced:**
//! - the **environment**: a snippet gets a scrubbed minimum; a project task
//!   inherits the user's environment minus secret-shaped variables, because a
//!   build needs `PATH`, toolchains and `HOME`,
//! - the **working directory**: a snippet starts in a throwaway scratch
//!   folder, a task in the project folder. Nothing stops either from opening a
//!   path elsewhere.
//!
//! **Not confined at all on Windows today:** the **filesystem** beyond the
//! working directory, and the **network**. Blocking those needs an
//! AppContainer profile, which is its own piece of work. Until then both
//! toolsets are opt-in, a read-only folder is never handed to a snippet, a
//! task never runs in a read-only project, and every run is logged.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

/// Per-process memory cap (bytes) for a snippet.
const MEM_LIMIT_BYTES: usize = 512 * 1024 * 1024;
/// Cap on captured stdout/stderr so a noisy script can't blow the context.
const OUTPUT_CAP: usize = 16 * 1024;
/// `COD-6`: what a task's output is cut to, keeping the end, where a build
/// prints the error that matters.
pub const TASK_OUTPUT_CAP: usize = 64 * 1024;

/// How a child's environment is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvMode {
    /// Only what an interpreter needs to start.
    Minimal,
    /// Everything the app inherited, minus secret-shaped names (`COD-6`).
    InheritWithoutSecrets,
}

/// What varies between an ad-hoc `run_code` call, a skill's bundled script
/// (`SKL-3`) and a project task (`COD-6`): the clock, the limits and the
/// environment. The Job Object and kill-on-drop are identical between them.
pub struct Profile {
    pub timeout: Duration,
    pub extra_env: Vec<(String, String)>,
    pub mem_limit: usize,
    pub process_limit: u32,
    pub env: EnvMode,
}

impl Profile {
    /// Today's behaviour, unchanged: 10s, a throwaway scratch cwd, no extra env.
    pub fn ad_hoc() -> Profile {
        Profile {
            timeout: Duration::from_secs(10),
            extra_env: Vec::new(),
            mem_limit: MEM_LIMIT_BYTES,
            process_limit: 16,
            env: EnvMode::Minimal,
        }
    }

    /// `SKL-3`: a skill's `scripts/*.py` gets more wall-clock than an ad-hoc
    /// snippet, and `POIESIS_SKILL_DIR` so it can resolve sibling
    /// `references/`/`assets/` files without the model guessing an absolute
    /// path.
    pub fn skill(skill_dir: &Path) -> Profile {
        Profile {
            timeout: Duration::from_secs(120),
            extra_env: vec![("POIESIS_SKILL_DIR".to_string(), skill_dir.display().to_string())],
            ..Profile::ad_hoc()
        }
    }

    /// `COD-6`: a project's own build or tests. Five minutes, room for a
    /// compiler's memory, room for the many children a build spawns, and the
    /// user's toolchain environment rather than a scrubbed one.
    pub fn task() -> Profile {
        Profile {
            timeout: Duration::from_secs(300),
            extra_env: Vec::new(),
            mem_limit: 4 * 1024 * 1024 * 1024,
            process_limit: 512,
            env: EnvMode::InheritWithoutSecrets,
        }
    }
}

/// `COD-6`: does this variable name look like it holds a credential? A task
/// inherits the environment, and a build script has no business reading the
/// user's API keys on the way past.
pub fn is_secret_name(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    const PARTS: [&str; 10] = [
        "TOKEN", "SECRET", "PASSWORD", "PASSWD", "API_KEY", "APIKEY", "PRIVATE_KEY", "ACCESS_KEY",
        "CREDENTIAL", "SESSION_KEY",
    ];
    PARTS.iter().any(|p| n.contains(p))
        || n.ends_with("_AUTH")
        || n.starts_with("AUTH_")
        || n.ends_with("_PAT")
        || n.starts_with("POIESIS_")
}

/// Find a program on `PATH` the way a shell would, including Windows' `.cmd`
/// and `.bat` shims: `npm`, `pnpm` and `yarn` are all `.cmd` files there, and
/// `Command::new("npm")` looks only for an `.exe`. The program is still run
/// directly with an argument vector, never through a shell string.
pub fn resolve_program(program: &str) -> std::path::PathBuf {
    let as_given = Path::new(program);
    if as_given.components().count() > 1 || !cfg!(windows) {
        return as_given.to_path_buf();
    }
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let has_ext = as_given.extension().is_some();
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            if has_ext && dir.join(program).is_file() {
                return dir.join(program);
            }
            for ext in exts.split(';').filter(|e| !e.is_empty()) {
                let candidate = dir.join(format!("{program}{}", ext.to_ascii_lowercase()));
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }
    as_given.to_path_buf()
}

pub struct SandboxOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

/// Run `program args…` in `workdir`, confined and time-limited. `readable_folder`
/// is the conversation's attached working folder, if any (`DAT-2`) — passed in
/// as the `POIESIS_FOLDER` env var so a snippet can open files there directly,
/// rather than the model having to guess a path. Returns captured output, or a
/// friendly error if the interpreter is missing.
pub async fn run(
    program: &str,
    args: &[String],
    workdir: &Path,
    readable_folder: Option<&Path>,
    profile: &Profile,
) -> Result<SandboxOutput, String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env_clear();

    // Preserve only the minimal environment the interpreter needs to start, so a
    // snippet can't read the app's inherited secrets/tokens from the environment.
    for key in ["SystemRoot", "PATH", "Path", "TEMP", "TMP", "WINDIR"] {
        if let Ok(val) = std::env::var(key) {
            cmd.env(key, val);
        }
    }
    for (key, val) in &profile.extra_env {
        cmd.env(key, val);
    }
    // `DAT-2` reads only: this is advisory, not OS-enforced — same category of
    // limitation as the network isolation noted above. Confining a Windows
    // child process's filesystem view needs an AppContainer profile; until
    // that lands, "read (not write)" is a contract stated in the tool
    // description, not a wall the sandbox itself builds. Because of that,
    // `codeexec` withholds this path entirely for a read-only folder and
    // records any file the snippet did change — see the comments there.
    if let Some(folder) = readable_folder {
        cmd.env("POIESIS_FOLDER", folder);
    }

    let child = cmd.spawn().map_err(|e| spawn_error(program, &e))?;

    // Confine to a dedicated kill-on-close job with memory + process limits.
    #[cfg(windows)]
    let _job = {
        let guard = job::Job::new(profile.mem_limit, profile.process_limit);
        if let (Some(g), Some(handle)) = (guard.as_ref(), child.raw_handle()) {
            g.assign(handle as isize);
        }
        guard
    };

    match tokio::time::timeout(profile.timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => Ok(SandboxOutput {
            stdout: cap(String::from_utf8_lossy(&output.stdout).into_owned()),
            stderr: cap(String::from_utf8_lossy(&output.stderr).into_owned()),
            exit_code: output.status.code(),
            timed_out: false,
        }),
        Ok(Err(e)) => Err(format!("the sandbox process failed: {e}")),
        // Timed out: the dropped future (kill_on_drop) and job kill-on-close both
        // terminate the process tree.
        Err(_) => Ok(SandboxOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            timed_out: true,
        }),
    }
}

/// What a streamed run left behind.
pub struct StreamOutput {
    /// stdout and stderr interleaved in arrival order, cut from the front to
    /// `TASK_OUTPUT_CAP`.
    pub output: String,
    pub truncated: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
    pub duration_ms: u64,
}

/// `COD-6`: run `program args…` in `workdir` under `profile`, handing each
/// output line to `on_line` as it arrives, so a four-minute build is four
/// minutes of visible progress rather than silence.
///
/// Stops early when `cancel` is set (Stop) or the profile's clock runs out.
/// Either way the child is dropped, and kill-on-drop plus the job's
/// kill-on-close take the whole tree down with it.
pub async fn run_streaming(
    program: &str,
    args: &[String],
    workdir: &Path,
    profile: &Profile,
    cancel: Option<&crate::runtime::proxy::CancelFlag>,
    mut on_line: impl FnMut(&str),
) -> Result<StreamOutput, String> {
    use std::collections::VecDeque;
    use tokio::io::{AsyncBufReadExt, BufReader};

    let started = std::time::Instant::now();
    let resolved = resolve_program(program);
    let mut cmd = Command::new(&resolved);
    cmd.args(args)
        .current_dir(workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    match profile.env {
        EnvMode::Minimal => {
            cmd.env_clear();
            for key in ["SystemRoot", "PATH", "Path", "TEMP", "TMP", "WINDIR"] {
                if let Ok(val) = std::env::var(key) {
                    cmd.env(key, val);
                }
            }
        }
        EnvMode::InheritWithoutSecrets => {
            for (key, _) in std::env::vars_os() {
                if is_secret_name(&key.to_string_lossy()) {
                    cmd.env_remove(key);
                }
            }
        }
    }
    // Non-interactive and uncoloured: a test runner that would otherwise sit
    // in watch mode exits, and the log reads as text rather than escape codes.
    cmd.env("CI", "1").env("NO_COLOR", "1").env("FORCE_COLOR", "0");
    for (key, val) in &profile.extra_env {
        cmd.env(key, val);
    }

    let mut child = cmd.spawn().map_err(|e| spawn_error(program, &e))?;

    #[cfg(windows)]
    let _job = {
        let guard = job::Job::new(profile.mem_limit, profile.process_limit);
        if let (Some(g), Some(handle)) = (guard.as_ref(), child.raw_handle()) {
            g.assign(handle as isize);
        }
        guard
    };

    let mut out = BufReader::new(child.stdout.take().ok_or("no stdout")?);
    let mut err = BufReader::new(child.stderr.take().ok_or("no stderr")?);
    let (mut out_open, mut err_open) = (true, true);
    let (mut out_buf, mut err_buf) = (Vec::new(), Vec::new());
    let mut kept: VecDeque<String> = VecDeque::new();
    let mut kept_bytes = 0usize;
    let mut truncated = false;
    let deadline = tokio::time::Instant::now() + profile.timeout;
    let mut keep = |line: &[u8], on_line: &mut dyn FnMut(&str)| {
        let text = String::from_utf8_lossy(line);
        let text = text.trim_end_matches(['\r', '\n']);
        on_line(text);
        kept_bytes += text.len() + 1;
        kept.push_back(text.to_string());
        while kept_bytes > TASK_OUTPUT_CAP && kept.len() > 1 {
            if let Some(dropped) = kept.pop_front() {
                kept_bytes -= dropped.len() + 1;
                truncated = true;
            }
        }
    };

    let mut timed_out = false;
    let mut cancelled = false;
    // One interval for the whole run: a sleep made fresh each time round the
    // loop would never fire while output keeps arriving, and Stop would wait
    // for the build to go quiet.
    let mut tick = tokio::time::interval(Duration::from_millis(150));
    while out_open || err_open {
        tokio::select! {
            n = out.read_until(b'\n', &mut out_buf), if out_open => {
                match n {
                    Ok(0) | Err(_) => out_open = false,
                    Ok(_) => { keep(&out_buf, &mut on_line); out_buf.clear(); }
                }
            }
            n = err.read_until(b'\n', &mut err_buf), if err_open => {
                match n {
                    Ok(0) | Err(_) => err_open = false,
                    Ok(_) => { keep(&err_buf, &mut on_line); err_buf.clear(); }
                }
            }
            _ = tokio::time::sleep_until(deadline) => { timed_out = true; break; }
            _ = tick.tick() => {
                if cancel.is_some_and(|c| c.is_cancelled()) { cancelled = true; break; }
            }
        }
    }

    let exit_code = if timed_out || cancelled {
        let _ = child.start_kill();
        None
    } else {
        match tokio::time::timeout_at(deadline, child.wait()).await {
            Ok(Ok(status)) => status.code(),
            Ok(Err(e)) => return Err(format!("the task process failed: {e}")),
            Err(_) => {
                timed_out = true;
                let _ = child.start_kill();
                None
            }
        }
    };
    drop(keep);
    Ok(StreamOutput {
        output: kept.into_iter().collect::<Vec<_>>().join("\n"),
        truncated,
        exit_code,
        timed_out,
        cancelled,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

fn cap(mut s: String) -> String {
    if s.len() > OUTPUT_CAP {
        let mut end = OUTPUT_CAP;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push_str("\n…(output truncated)");
    }
    s
}

fn spawn_error(program: &str, e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        let hint = match program {
            "python" => "Python isn't installed or isn't on your PATH.",
            "node" => "Node.js isn't installed or isn't on your PATH.",
            other => return format!("Couldn't start '{other}': {e}"),
        };
        hint.to_string()
    } else {
        format!("Couldn't start '{program}': {e}")
    }
}

#[cfg(windows)]
mod job {
    //! A dedicated, ephemeral kill-on-close Job Object with resource limits. On
    //! drop the handle is closed, which (via KILL_ON_JOB_CLOSE) terminates every
    //! assigned process — including any children the snippet spawned.

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    };

    // The handle is stored as `isize` (not the raw `*mut c_void`) so the guard is
    // `Send` — it is held across the `.await` in `run`, and the Tauri command
    // future must be `Send`.
    pub struct Job(isize);

    impl Job {
        pub fn new(mem_limit: usize, process_limit: u32) -> Option<Job> {
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return None;
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                    | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                    | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
                info.BasicLimitInformation.ActiveProcessLimit = process_limit;
                info.ProcessMemoryLimit = mem_limit;
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                Some(Job(job as isize))
            }
        }

        pub fn assign(&self, child: isize) {
            unsafe {
                AssignProcessToJobObject(self.0 as HANDLE, child as HANDLE);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // Closing the only handle triggers KILL_ON_JOB_CLOSE on the tree.
            unsafe {
                CloseHandle(self.0 as HANDLE);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_shaped_names_are_withheld_and_ordinary_ones_are_not() {
        for name in ["OPENAI_API_KEY", "GITHUB_TOKEN", "AWS_SECRET_ACCESS_KEY", "NPM_AUTH", "db_password", "POIESIS_RUN_TOKEN"] {
            assert!(is_secret_name(name), "{name}");
        }
        for name in ["PATH", "HOME", "CARGO_HOME", "AUTHOR", "JAVA_HOME", "NODE_OPTIONS"] {
            assert!(!is_secret_name(name), "{name}");
        }
    }

    fn shell(script: &str) -> (&'static str, Vec<String>) {
        if cfg!(windows) {
            ("cmd", vec!["/C".to_string(), script.to_string()])
        } else {
            ("sh", vec!["-c".to_string(), script.to_string()])
        }
    }

    /// `COD-6-T`: a non-zero exit surfaces its code and the end of stderr.
    #[tokio::test]
    async fn a_failing_command_reports_its_exit_code_and_its_last_lines() {
        let (program, args) = shell("echo first && echo boom 1>&2 && exit 3");
        let mut seen = Vec::new();
        let out = run_streaming(program, &args, &std::env::temp_dir(), &Profile::task(), None, |l| seen.push(l.to_string()))
            .await
            .unwrap();
        assert_eq!(out.exit_code, Some(3));
        assert!(!out.timed_out);
        assert!(out.output.contains("boom"), "{}", out.output);
        assert!(seen.iter().any(|l| l.trim() == "first"), "lines arrive as they are printed: {seen:?}");
    }

    /// `COD-6-T2`: an overrun is killed, and the result says so.
    #[tokio::test]
    async fn an_overrun_is_killed_and_says_so() {
        let (program, args) = if cfg!(windows) {
            ("ping", vec!["-n".to_string(), "30".to_string(), "127.0.0.1".to_string()])
        } else {
            ("sleep", vec!["30".to_string()])
        };
        let profile = Profile { timeout: Duration::from_millis(700), ..Profile::task() };
        let started = std::time::Instant::now();
        let out = run_streaming(program, &args, &std::env::temp_dir(), &profile, None, |_| {}).await.unwrap();
        assert!(out.timed_out);
        assert_eq!(out.exit_code, None);
        assert!(started.elapsed() < Duration::from_secs(10), "it did not wait for the command");
    }

    #[tokio::test]
    async fn stop_ends_a_task_while_it_is_still_printing() {
        let (program, args) = if cfg!(windows) {
            ("ping", vec!["-n".to_string(), "30".to_string(), "127.0.0.1".to_string()])
        } else {
            ("sh", vec!["-c".to_string(), "while true; do echo tick; sleep 0.05; done".to_string()])
        };
        let flag = crate::runtime::proxy::CancelFlag::new();
        let stopper = flag.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(400)).await;
            stopper.cancel();
        });
        let out = run_streaming(program, &args, &std::env::temp_dir(), &Profile::task(), Some(&flag), |_| {}).await.unwrap();
        assert!(out.cancelled);
    }
}
