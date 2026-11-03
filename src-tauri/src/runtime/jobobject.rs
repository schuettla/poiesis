//! Lifecycle safety (§7.4): bind the engine child process to a Win32 Job Object
//! configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. The job handle is held
//! open for the whole app lifetime; if the parent dies *ungracefully* (crash,
//! taskkill) the OS closes the handle, the job closes, and every assigned child
//! is terminated — so no orphaned `llama-server` keeps holding VRAM.
//!
//! Unix has no job object. The closest equivalent (CROSS_PLATFORM_PLAN XP-4)
//! is [`bind`] before the spawn: the child leads its own process group, so
//! [`kill_group`] / [`Group`] end the whole tree the way `taskkill /T` and job
//! close do, and on Linux the kernel SIGKILLs it if Poiesis dies. macOS has no
//! parent-death signal; a normal exit and the update restart still stop
//! engines through `shutdown_engines`.

#[cfg(windows)]
mod imp {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    // The job handle, kept alive for the process lifetime (never closed by us).
    static JOB: OnceLock<usize> = OnceLock::new();

    fn job_handle() -> HANDLE {
        let raw = *JOB.get_or_init(|| unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if !job.is_null() {
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
            }
            job as usize
        });
        raw as HANDLE
    }

    /// Assign `child` to the kill-on-close job. Best-effort; a failure here is not
    /// fatal (the `Drop`/exit-handler paths still terminate the child normally).
    pub fn assign(child: &std::process::Child) {
        let job = job_handle();
        if job.is_null() {
            return;
        }
        unsafe {
            AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE);
        }
    }

    /// Same as `assign`, for a child whose `std::process::Child` is not at hand
    /// (a `tokio::process::Child` only gives its raw handle). External agent
    /// apps (`CLI-3`) are bound to the job too: everything below the process
    /// inherits it, so a crash of Poiesis cannot leave an agent's node child
    /// running.
    pub fn assign_raw(handle: std::os::windows::io::RawHandle) {
        let job = job_handle();
        if job.is_null() {
            return;
        }
        unsafe {
            AssignProcessToJobObject(job, handle as HANDLE);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn assign(_child: &std::process::Child) {}
}

pub use imp::assign;
#[cfg(windows)]
pub use imp::assign_raw;

/// Make `cmd`'s child the leader of a new process group and, on Linux, have
/// the kernel kill it when the parent goes away.
///
/// `PR_SET_PDEATHSIG` fires when the *thread* that spawned the child exits,
/// not the process. Every spawn site runs on a tokio worker thread, which
/// lives as long as the app; spawning from a `spawn_blocking` thread (which
/// exits after ten idle seconds) would kill a healthy child.
#[cfg(unix)]
pub fn bind(cmd: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
    #[cfg(target_os = "linux")]
    // SAFETY: runs between fork and exec; `prctl` is async-signal-safe and
    // nothing here allocates.
    unsafe {
        cmd.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// SIGKILL the process group led by `pid` (a child spawned through [`bind`]).
#[cfg(unix)]
pub fn kill_group(pid: u32) {
    // SAFETY: plain syscall; a group that is already gone returns ESRCH.
    unsafe {
        libc::killpg(pid as libc::pid_t, libc::SIGKILL);
    }
}

/// Kills the child's process group when dropped: the Unix stand-in for the
/// sandbox's per-run kill-on-close job, so nothing a snippet started in the
/// background outlives the run.
#[cfg(unix)]
pub struct Group(pub u32);

#[cfg(unix)]
impl Drop for Group {
    fn drop(&mut self) {
        kill_group(self.0);
    }
}
