//! Windows process trees. Assign a suspended child before it can execute.

use super::Group;
use std::{
    io,
    mem::size_of,
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    process::{Child, Command},
    sync::Arc,
    thread,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        },
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
            JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList,
            JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
            TerminateJobObject,
        },
        Threading::{
            CREATE_NO_WINDOW, CREATE_SUSPENDED, INFINITE, OpenProcess, OpenThread,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, ResumeThread,
            THREAD_SUSPEND_RESUME, WaitForSingleObject,
        },
    },
};

#[derive(Debug)]
pub(super) struct Job(OwnedHandle);

impl Job {
    fn new() -> io::Result<Self> {
        // SAFETY: null attributes create a non-inherited, unnamed job handle.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the successful call transfers this unique handle to its owner.
        let job = Self(unsafe { OwnedHandle::from_raw_handle(handle) });
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the owned job and correctly sized information value remain live.
        if unsafe {
            SetInformationJobObject(
                job.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    fn members(&self) -> io::Result<Vec<OwnedHandle>> {
        let mut buffer = vec![0usize; 128];
        loop {
            // SAFETY: usize storage aligns the variable-length process ID list.
            // The header and all reported identifiers fit the supplied byte length.
            let success = unsafe {
                QueryInformationJobObject(
                    self.0.as_raw_handle(),
                    JobObjectBasicProcessIdList,
                    buffer.as_mut_ptr().cast(),
                    (buffer.len() * size_of::<usize>()) as u32,
                    std::ptr::null_mut(),
                )
            };
            if success == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_MORE_DATA as i32) && buffer.len() < 65536 {
                    buffer.resize(buffer.len() * 2, 0);
                    continue;
                }
                return Err(error);
            }
            // SAFETY: successful QueryInformationJobObject initialized the list.
            let list = unsafe { &*buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
            let count = list.NumberOfProcessIdsInList as usize;
            let header_words = std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList)
                / size_of::<usize>();
            if count > buffer.len().saturating_sub(header_words) {
                return Err(io::Error::other("invalid job process list"));
            }
            // SAFETY: the checked count lies within the aligned backing allocation.
            let ids = unsafe {
                std::slice::from_raw_parts(
                    std::ptr::addr_of!(list.ProcessIdList).cast::<usize>(),
                    count,
                )
            };
            let mut handles = Vec::new();
            for &id in ids {
                // SAFETY: only read/query and synchronization access are requested.
                let raw = unsafe {
                    OpenProcess(
                        PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                        0,
                        id as u32,
                    )
                };
                if raw.is_null() {
                    let error = io::Error::last_os_error();
                    if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                        continue;
                    }
                    return Err(error);
                }
                // SAFETY: the successful OpenProcess transfers a unique owned handle.
                let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
                let mut member = 0;
                // SAFETY: query the captured handle, not an identifier that can be reused.
                if unsafe {
                    IsProcessInJob(handle.as_raw_handle(), self.0.as_raw_handle(), &mut member)
                } == 0
                {
                    return Err(io::Error::last_os_error());
                }
                if member != 0 {
                    handles.push(handle);
                }
            }
            return Ok(handles);
        }
    }

    pub(super) fn terminate(&self) -> io::Result<()> {
        // SAFETY: this owned job contains only this invocation's process tree.
        if unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn settle(&self) -> io::Result<()> {
        // Retain exact process handles before requesting termination. An accounting
        // count alone is not a wait on each observed process's signaled state.
        let members = self.members();
        // SAFETY: this job exclusively owns the command and its inherited descendants.
        if unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let (members, capture_error) = match members {
            Ok(members) => (members, None),
            Err(error) => (Vec::new(), Some(error)),
        };
        for member in members {
            // SAFETY: the captured handle belongs to this job and remains live.
            if unsafe { WaitForSingleObject(member.as_raw_handle(), INFINITE) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        loop {
            let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            // SAFETY: the output buffer is writable and has the documented size.
            if unsafe {
                QueryInformationJobObject(
                    self.0.as_raw_handle(),
                    JobObjectBasicAccountingInformation,
                    (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if accounting.ActiveProcesses == 0 {
                return capture_error.map_or(Ok(()), Err);
            }
            thread::sleep(Duration::from_millis(2));
        }
    }
}

fn resume(child: &Child) -> io::Result<()> {
    // The primary thread is still suspended. It cannot create threads or children
    // before assignment to the job. The live Child handle keeps its identity pinned.
    // SAFETY: snapshot flags require no additional pointers.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the successful call transfers a unique snapshot handle.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    // SAFETY: entry is writable and initialized to the required structure size.
    let mut available = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
    while available != 0 {
        if entry.th32OwnerProcessID == child.id() {
            // SAFETY: the suspended child's thread identifier comes from this snapshot.
            let handle = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: OpenThread transfers its successful, uniquely owned handle.
            let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
            // SAFETY: this thread belongs to the suspended child already inside our job.
            if unsafe { ResumeThread(handle.as_raw_handle()) } == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            return Ok(());
        }
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        // SAFETY: the snapshot and output entry remain live.
        available = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
    }
    Err(io::Error::other(
        "the suspended command thread was not found",
    ))
}

/// No command code runs until the child belongs to the owned job. On a
/// setup failure the still-suspended child is killed and reaped.
pub(super) fn spawn(command: &mut Command) -> io::Result<(Child, Group)> {
    let job = Arc::new(Job::new()?);
    let mut child = command
        .creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW)
        .spawn()?;
    // SAFETY: both handles are live and the primary thread is suspended.
    let assigned =
        unsafe { AssignProcessToJobObject(job.0.as_raw_handle(), child.as_raw_handle()) };
    let ready = if assigned == 0 {
        Err(io::Error::last_os_error())
    } else {
        resume(&child)
    };
    if let Err(error) = ready {
        let _ = job.terminate();
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    Ok((child, Group(job)))
}
