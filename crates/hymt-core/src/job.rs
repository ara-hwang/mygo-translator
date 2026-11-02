//! Kernel-side cleanup for the managed `llama-server` child.
//!
//! The supervisor thread normally stops the child itself, but if the application ever
//! dies abruptly (a crash, `taskkill /f`, an abort) it has no chance to run any Rust
//! code. On Windows the child is therefore assigned to a job object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: the kernel terminates every process in the job
//! the moment the last job handle disappears, which happens automatically when this
//! process exits, no matter how.
//!
//! Only `std` is used; the few Win32 calls are declared inline so the crate keeps its
//! zero-dependency rule. On other platforms [`JobGuard`] is a no-op.

use std::io;
use std::process::Child;

#[cfg(windows)]
type Handle = *mut std::ffi::c_void;

#[cfg(windows)]
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
#[cfg(windows)]
const JOBOBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;

/// The subset of `JOBOBJECT_EXTENDED_LIMIT_INFORMATION` we need, with the exact Win64
/// layout. Every limit stays at zero; only the kill-on-close flag is set.
#[cfg(windows)]
#[repr(C)]
struct ExtendedLimitInformation {
    basic: BasicLimitInformation,
    io_counters: [u64; 6],
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}

#[cfg(windows)]
#[repr(C)]
struct BasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[cfg(windows)]
extern "system" {
    fn CreateJobObjectW(attrs: *mut std::ffi::c_void, name: *const u16) -> Handle;
    fn SetInformationJobObject(
        job: Handle,
        class: i32,
        info: *mut std::ffi::c_void,
        size: u32,
    ) -> i32;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
}

/// Keeps a job object alive for as long as the managed child should live.
///
/// Dropping the guard closes the job handle, which terminates the processes inside it.
/// A guard created by [`JobGuard::detached`] does nothing; it exists so callers can
/// keep running when job assignment fails in an unusual host environment.
pub struct JobGuard {
    #[cfg(windows)]
    handle: Handle,
}

#[cfg(windows)]
impl JobGuard {
    /// Creates a job object that kills its processes when it is closed and puts
    /// `child` into it.
    pub fn assign(child: &Child) -> io::Result<JobGuard> {
        use std::os::windows::io::AsRawHandle;

        let handle = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }

        let result = (|| {
            // `std::mem::zeroed` is fine: the struct is plain data and every limit is
            // meant to stay at zero.
            let mut info: ExtendedLimitInformation = unsafe { std::mem::zeroed() };
            info.basic.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = unsafe {
                SetInformationJobObject(
                    handle,
                    JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                    &mut info as *mut ExtendedLimitInformation as *mut std::ffi::c_void,
                    std::mem::size_of::<ExtendedLimitInformation>() as u32,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            // Windows 8 and later allow nested jobs, so this succeeds even when the
            // application itself already runs inside a job.
            let ok = unsafe { AssignProcessToJobObject(handle, child.as_raw_handle()) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        })();

        if let Err(err) = result {
            unsafe { CloseHandle(handle) };
            return Err(err);
        }
        Ok(JobGuard { handle })
    }

    /// A guard that manages nothing; used when job assignment is not possible.
    pub fn detached() -> JobGuard {
        JobGuard {
            handle: std::ptr::null_mut(),
        }
    }
}

#[cfg(windows)]
impl Drop for JobGuard {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // KILL_ON_JOB_CLOSE does the termination; the result only reports whether
            // the handle existed, so there is nothing to handle here.
            unsafe { CloseHandle(self.handle) };
        }
    }
}

#[cfg(not(windows))]
impl JobGuard {
    /// No-op on platforms without job objects.
    pub fn assign(_child: &Child) -> io::Result<JobGuard> {
        Ok(JobGuard {})
    }

    /// No-op on platforms without job objects.
    pub fn detached() -> JobGuard {
        JobGuard {}
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::Duration;

    fn long_lived_child() -> Child {
        Command::new("cmd")
            .args(["/c", "ping", "-n", "60", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn a long-lived test child")
    }

    #[test]
    fn the_child_survives_while_the_job_is_alive() {
        let mut child = long_lived_child();
        let _job = JobGuard::assign(&child).expect("assign child to a job");
        thread::sleep(Duration::from_millis(500));
        assert!(
            child.try_wait().expect("poll child").is_none(),
            "a living job must not kill its child"
        );
        drop(_job);
        let _ = child.wait();
    }

    #[test]
    fn closing_the_job_kills_the_child() {
        let mut child = long_lived_child();
        let job = JobGuard::assign(&child).expect("assign child to a job");
        drop(job);
        let mut reaped = false;
        for _ in 0..50 {
            if child.try_wait().expect("poll child").is_some() {
                reaped = true;
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        assert!(reaped, "child survived the job object being closed");
        let _ = child.wait();
    }
}
