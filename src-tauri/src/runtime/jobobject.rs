//! Lifecycle safety (§7.4): bind the engine child process to a Win32 Job Object
//! configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. The job handle is held
//! open for the whole app lifetime; if the parent dies *ungracefully* (crash,
//! taskkill) the OS closes the handle, the job closes, and every assigned child
//! is terminated — so no orphaned `llama-server` keeps holding VRAM.
//!
//! On non-Windows targets this is a no-op (v1 ships Windows only).

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
}

#[cfg(not(windows))]
mod imp {
    pub fn assign(_child: &std::process::Child) {}
}

pub use imp::assign;
