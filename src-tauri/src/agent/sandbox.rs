//! A confined subprocess sandbox for the Code Execution toolset (TOOL-5).
//!
//! Each run gets a dedicated Win32 Job Object (separate from the engine's global
//! job) configured to **kill-on-close** with a memory cap and an active-process
//! limit, plus a hard wall-clock timeout and a scrubbed environment. The child
//! runs in a throwaway scratch directory. If the run overruns or the future is
//! dropped, both `kill_on_drop` and the job's kill-on-close terminate the whole
//! process tree so nothing escapes the turn.
//!
//! Note: this confines CPU/memory/lifetime and isolates the filesystem to a
//! scratch dir, but does **not** yet block outbound network on Windows (that
//! needs an AppContainer profile — tracked as follow-up hardening). The toolset is
//! opt-in and every run is logged.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

/// Per-job memory cap (bytes) — the same for every profile below.
const MEM_LIMIT_BYTES: usize = 512 * 1024 * 1024;
/// Cap on captured stdout/stderr so a noisy script can't blow the context.
const OUTPUT_CAP: usize = 16 * 1024;

/// What varies between an ad-hoc `run_code` call and a skill's bundled script
/// (`SKL-3`) — the clock and the environment. Everything else (Job Object,
/// memory cap, kill-on-drop) is identical between profiles.
pub struct Profile {
    pub timeout: Duration,
    pub extra_env: Vec<(String, String)>,
}

impl Profile {
    /// Today's behaviour, unchanged: 10s, a throwaway scratch cwd, no extra env.
    pub fn ad_hoc() -> Profile {
        Profile {
            timeout: Duration::from_secs(10),
            extra_env: Vec::new(),
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
        }
    }
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
        let guard = job::Job::new(MEM_LIMIT_BYTES);
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
        pub fn new(mem_limit: usize) -> Option<Job> {
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return None;
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                    | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                    | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
                info.BasicLimitInformation.ActiveProcessLimit = 16;
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
