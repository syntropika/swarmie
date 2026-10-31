use super::ExecutionAttempt;
use crate::{Error, OutputMask, Result, SecretReference};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command, time::Instant};
use zeroize::Zeroizing;

const LIMIT: usize = 1024 * 1024;

/// Only environment slots are substituted as data. Shell source is passed unchanged to Bash.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BashRequest {
    pub source: String,
    pub cwd: PathBuf,
    pub environment: BTreeMap<String, SecretReference>,
    pub timeout_ms: u64,
}

/// This receipt has only sanitized output. Unknown effects never imply safe command replay.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BashReceipt {
    pub exit: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub truncated: bool,
}

/// Owns the binary selection; callers cannot turn a credential into executable shell source.
pub struct BashExecutor {
    program: PathBuf,
}

impl BashExecutor {
    pub fn new(program: PathBuf) -> Self {
        Self { program }
    }

    pub async fn execute(
        &self,
        attempt: &ExecutionAttempt<'_>,
        request: BashRequest,
    ) -> Result<BashReceipt> {
        match self.run(attempt, request, false, None).await? {
            ProcessResult::Visible(receipt) => Ok(receipt),
            ProcessResult::Captured(_) => Err(Error::Integrity),
        }
    }

    pub(super) async fn run(
        &self,
        attempt: &ExecutionAttempt<'_>,
        request: BashRequest,
        capture: bool,
        approved: Option<Vec<crate::UseIntent>>,
    ) -> Result<ProcessResult> {
        if !cfg!(unix) {
            return Err(Error::Unsupported);
        }
        if !self.program.is_absolute() {
            return Err(Error::Invalid);
        }
        validate(&request)?;
        let destination = format!("bash:{}:{}", request.cwd.display(), request.source);
        let references: Vec<_> = request.environment.values().copied().collect();
        let intents = if let Some(approved) = approved {
            approved
        } else {
            attempt.approvals(&references, &destination).await?
        };
        let leases = attempt.leases(&intents)?;
        let mut command = Command::new(&self.program);
        command
            .arg("--noprofile")
            .arg("--norc")
            .arg("-c")
            .arg(&request.source)
            .current_dir(&request.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        for (name, reference) in &request.environment {
            let lease = leases
                .iter()
                .find(|l| l.reference == *reference)
                .ok_or(Error::Invalid)?;
            lease.deliver(|value| {
                let value = std::str::from_utf8(value).map_err(|_| Error::Invalid)?;
                if value.contains('\0') {
                    return Err(Error::Unsupported);
                }
                command.env(name, value);
                Ok(())
            })?;
        }
        let mut out_mask = OutputMask::new(leases)?;
        let mut err_mask = out_mask.fork();
        // No await between final lease admission above and physical process creation.
        let mut child = command.spawn().map_err(|_| Error::Unavailable)?;
        drop(command);
        let pid = child.id().ok_or(Error::Unknown)?;
        let mut group = ProcessGroup(Some(pid));
        let mut stdout = child.stdout.take().ok_or(Error::Unknown)?;
        let mut stderr = child.stderr.take().ok_or(Error::Unknown)?;
        let deadline = Instant::now() + Duration::from_millis(request.timeout_ms);
        let mut out = Zeroizing::new(vec![]);
        let mut err = Zeroizing::new(vec![]);
        let mut observed = 0_usize;
        let mut io_failed = false;
        let mut out_bytes = Zeroizing::new([0; 8192]);
        let mut err_bytes = Zeroizing::new([0; 8192]);
        let mut out_open = true;
        let mut err_open = true;
        let mut timed_out = false;
        let mut truncated = false;
        // Do not reap the child before final group cleanup: retaining its PID prevents group-ID reuse.
        while out_open || err_open {
            tokio::select! {
                n = stdout.read(out_bytes.as_mut_slice()), if out_open => {
                    let Ok(n) = n else { io_failed = true; break; };
                    observed = observed.saturating_add(n);
                    if n == 0 { out_open = false; } else if capture { out.extend_from_slice(&out_bytes[..n]); } else { out.extend(out_mask.push(&out_bytes[..n])); }
                }
                n = stderr.read(err_bytes.as_mut_slice()), if err_open => {
                    let Ok(n) = n else { io_failed = true; break; };
                    observed = observed.saturating_add(n);
                    if n == 0 { err_open = false; } else if !capture { err.extend(err_mask.push(&err_bytes[..n])); }
                }
                _ = tokio::time::sleep_until(deadline) => { timed_out = true; break; }
            }
            if observed > LIMIT {
                truncated = true;
                break;
            }
        }
        drop(stdout);
        drop(stderr);
        if timed_out || truncated || io_failed {
            kill_group(pid);
            let _ = child.start_kill();
        }
        if !timed_out && !truncated && !io_failed {
            match tokio::time::timeout_at(deadline, wait_unreaped(pid)).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => {
                    group.0 = None;
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    return Err(Error::Unknown);
                }
                Err(_) => {
                    timed_out = true;
                }
            }
        }
        // The unreaped direct child still owns the group ID. Kill remaining descendants before
        // reaping it, including descendants that deliberately closed their inherited output pipes.
        kill_group(pid);
        let status = child.wait().await.map_err(|_| Error::Unknown)?;
        group.0 = None;
        if capture {
            if timed_out || truncated || io_failed || !status.success() {
                return Err(Error::Unknown);
            }
            return Ok(ProcessResult::Captured(out));
        }
        if io_failed {
            return Err(Error::Unknown);
        }
        if (timed_out || truncated) && out_mask.has_secrets() {
            out.clear();
            err.clear();
            out.extend_from_slice(b"[output withheld: execution or output limit reached]");
        } else {
            out.extend(out_mask.finish());
            err.extend(err_mask.finish());
        }
        Ok(ProcessResult::Visible(BashReceipt {
            exit: status.code(),
            stdout: String::from_utf8_lossy(&out).into_owned(),
            stderr: String::from_utf8_lossy(&err).into_owned(),
            timed_out,
            truncated,
        }))
    }
}

pub(super) enum ProcessResult {
    Visible(BashReceipt),
    Captured(Zeroizing<Vec<u8>>),
}

pub(super) fn validate(request: &BashRequest) -> Result<()> {
    if request.source.is_empty()
        || request.source.len() > 65_536
        || request.source.contains("{{secret:")
        || request.environment.len() > 16
        || !(1..=300_000).contains(&request.timeout_ms)
    {
        return Err(Error::Invalid);
    }
    for name in request.environment.keys() {
        if !name.starts_with("SWARMIE_SECRET_")
            || name.len() > 128
            || !name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(Error::Invalid);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn kill_group(pid: u32) {
    if let Ok(pid) = i32::try_from(pid) {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
}
#[cfg(not(unix))]
fn kill_group(_: u32) {}

// Physical ownership survives a dropped observer; cancelling the native runner also kills
// its still-owned process group before Tokio drops/reaps the direct child.
struct ProcessGroup(Option<u32>);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            kill_group(pid);
        }
    }
}

#[cfg(unix)]
async fn wait_unreaped(pid: u32) -> std::result::Result<(), nix::errno::Errno> {
    use nix::libc;
    loop {
        // SAFETY: zero is a valid initialized siginfo_t representation. waitid writes only this
        // local object; WNOWAIT deliberately leaves the owned direct child available to Tokio.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: pid belongs to this process's still-owned Child; info is writable and aligned.
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
            )
        };
        if result != 0 {
            let error = nix::errno::Errno::last();
            if error == nix::errno::Errno::EINTR {
                continue;
            }
            return Err(error);
        }
        // SAFETY: a successful waitid initialized info; a zero PID means no exit is available.
        if unsafe { info.si_pid() } != 0 {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}
#[cfg(not(unix))]
async fn wait_unreaped(_: u32) -> Result<()> {
    Err(Error::Unsupported)
}
