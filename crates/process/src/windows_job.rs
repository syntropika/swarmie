use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};
use tokio::process::Child;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};

/// Owns the process tree independently of the worker's reusable numeric PID.
pub(super) struct Job(OwnedHandle);

impl Job {
    pub(super) fn attach(child: &mut Child) -> io::Result<Self> {
        let result = Self::create_and_assign(child);
        if result.is_err() {
            // Fail before exposing provider pipes or sending a request when the
            // host cannot assign this process to an owned job.
            let _ = child.start_kill();
        }
        result
    }

    fn create_and_assign(child: &Child) -> io::Result<Self> {
        // No security attributes means this handle is not inherited by workers.
        let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateJobObjectW returned a new valid handle owned by this call.
        let owned = unsafe { OwnedHandle::from_raw_handle(handle) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: The structure remains valid for the synchronous API call.
        if unsafe {
            SetInformationJobObject(
                owned.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let process = child.raw_handle().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Worker exited before job assignment",
            )
        })?;
        // SAFETY: Child retains its live process handle throughout assignment.
        if unsafe { AssignProcessToJobObject(owned.as_raw_handle(), process) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(owned))
    }

    pub(super) fn terminate(&self) {
        // The owned handle refers only to this job even after its original worker
        // exits. Closing the last handle also terminates remaining descendants.
        unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{process::Stdio, time::Duration};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use windows_sys::Win32::{
        Foundation::WAIT_OBJECT_0,
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };

    async fn tree() -> (Job, Child, OwnedHandle) {
        // The parent waits for input before creating its child, proving that
        // descendants inherit the job assigned before provider communication.
        let script = "$null=[Console]::ReadLine(); $child=Start-Process powershell.exe -ArgumentList '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 60' -NoNewWindow -PassThru; [Console]::Out.WriteLine($child.Id); $action=[Console]::ReadLine(); if ($action -eq 'exit') { exit 0 }; Start-Sleep -Seconds 60";
        let mut parent = tokio::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let job = Job::attach(&mut parent).unwrap();
        parent
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"start\n")
            .await
            .unwrap();
        let mut line = String::new();
        tokio::time::timeout(
            Duration::from_secs(10),
            BufReader::new(parent.stdout.take().unwrap()).read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        let pid = line.trim().parse().unwrap();
        // Keep a real child process handle so assertions never follow a reused PID.
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        assert!(!handle.is_null());
        let descendant = unsafe { OwnedHandle::from_raw_handle(handle) };
        (job, parent, descendant)
    }

    fn assert_terminated(process: &OwnedHandle) {
        assert_eq!(
            unsafe { WaitForSingleObject(process.as_raw_handle(), 3000) },
            WAIT_OBJECT_0
        );
    }

    #[tokio::test]
    async fn forced_shutdown_terminates_owned_descendants() {
        let (job, mut parent, child) = tree().await;
        job.terminate();
        assert_terminated(&child);
        parent.wait().await.unwrap();
    }

    #[tokio::test]
    async fn closing_last_job_handle_terminates_owned_descendants() {
        let (job, mut parent, child) = tree().await;
        drop(job);
        assert_terminated(&child);
        parent.wait().await.unwrap();
    }

    #[tokio::test]
    async fn cleanup_after_parent_exit_uses_the_owned_job() {
        let (job, mut parent, child) = tree().await;
        parent
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"exit\n")
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), parent.wait())
            .await
            .unwrap()
            .unwrap();
        job.terminate();
        assert_terminated(&child);
    }
}
