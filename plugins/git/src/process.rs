use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[cfg(unix)]
use std::{
    io::Read,
    process::{Command, Stdio},
    thread,
    time::Instant,
};

use crate::{Error, Result};

/// Cancellation is cooperative before spawn and kills and reaps a running child.
#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub program: String,
    pub cwd: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub timeout: Duration,
    pub output_limit: usize,
}

#[derive(Clone, Debug)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub truncated: bool,
}

/// A host may replace native execution; it must settle the child before returning.
/// Return `Error::NotStarted` only when no process or external effect was dispatched.
pub trait ProcessExecutor: Send + Sync {
    fn run(&self, command: &CommandSpec, cancellation: &Cancellation) -> Result<CommandOutput>;
}

#[derive(Clone, Default)]
pub struct NativeProcess;

#[cfg(unix)]
fn collect(
    mut reader: impl Read + std::os::fd::AsRawFd,
    limit: usize,
    settled: Arc<AtomicBool>,
) -> std::io::Result<(Vec<u8>, bool)> {
    let descriptor = reader.as_raw_fd();
    // SAFETY: this live pipe descriptor is owned by the reader thread. Nonblocking
    // reads let settlement close inherited pipes even if a detached helper escaped.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let mut bytes = Vec::new();
    let mut truncated = false;
    let mut buffer = [0; 8192];
    let mut settled_bytes = 0usize;
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if settled.load(Ordering::Acquire) {
                    break;
                }
                thread::sleep(Duration::from_millis(2));
                continue;
            }
            Err(error) => return Err(error),
        };
        let keep = count.min(limit.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..keep]);
        truncated |= keep < count;
        if settled.load(Ordering::Acquire) {
            settled_bytes += count;
            if settled_bytes > 1024 * 1024 {
                truncated = true;
                break;
            }
        }
    }
    Ok((bytes, truncated))
}

impl ProcessExecutor for NativeProcess {
    fn run(&self, command: &CommandSpec, cancellation: &Cancellation) -> Result<CommandOutput> {
        #[cfg(unix)]
        {
            run_native(command, cancellation)
        }
        #[cfg(not(unix))]
        {
            let _ = (command, cancellation);
            Err(Error::Invalid(
                "native process cancellation requires a host executor on this platform".into(),
            ))
        }
    }
}

#[cfg(unix)]
fn run_native(command: &CommandSpec, cancellation: &Cancellation) -> Result<CommandOutput> {
    if cancellation.is_cancelled() {
        return Err(Error::NotStarted(
            "operation cancelled before process dispatch".into(),
        ));
    }
    let mut process = Command::new(&command.program);
    process
        .args(&command.args)
        .current_dir(&command.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        process.process_group(0);
    }
    if command.program == "git" {
        for key in [
            "GIT_DIR",
            "GIT_COMMON_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_NAMESPACE",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
            "GIT_CONFIG",
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_SYSTEM",
            "GIT_CONFIG_NOSYSTEM",
            "GIT_CEILING_DIRECTORIES",
            "GIT_DISCOVERY_ACROSS_FILESYSTEM",
            "GIT_ATTR_SOURCE",
            "GIT_SHALLOW_FILE",
            "GIT_REPLACE_REF_BASE",
        ] {
            process.env_remove(key);
        }
    }
    // Explicit host-provided overrides are applied after inherited Git routing is removed.
    process.envs(&command.env);
    let mut child = process
        .spawn()
        .map_err(|error| Error::NotStarted(error.to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Invalid("missing child stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::Invalid("missing child stderr".into()))?;
    let limit = command.output_limit;
    let settled = Arc::new(AtomicBool::new(false));
    let output_settled = settled.clone();
    let error_settled = settled.clone();
    let output_thread = thread::spawn(move || collect(stdout, limit, output_settled));
    let error_thread = thread::spawn(move || collect(stderr, limit, error_settled));
    let started = Instant::now();
    let mut interrupted = None;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if cancellation.is_cancelled() || started.elapsed() > command.timeout {
            interrupted = Some(if cancellation.is_cancelled() {
                Error::Cancelled
            } else {
                Error::Timeout
            });
            // A racing natural exit may make kill fail; wait still establishes settlement.
            #[cfg(unix)]
            // SAFETY: the child owns a new process group created immediately before spawn.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            break child.wait()?;
        }
        thread::sleep(Duration::from_millis(10));
    };
    #[cfg(unix)]
    // Hooks must settle with their owning command; detached pipe owners cannot hang readers.
    // SAFETY: only this command's dedicated process group is addressed.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    settled.store(true, Ordering::Release);
    let (stdout, out_truncated) = output_thread
        .join()
        .map_err(|_| Error::Invalid("stdout reader panicked".into()))??;
    let (stderr, err_truncated) = error_thread
        .join()
        .map_err(|_| Error::Invalid("stderr reader panicked".into()))??;
    if let Some(error) = interrupted {
        return Err(error);
    }
    Ok(CommandOutput {
        status: status.code().unwrap_or(-1),
        stdout,
        stderr,
        truncated: out_truncated || err_truncated,
    })
}
