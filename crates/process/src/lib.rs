mod discovery;
mod options;
mod recording;
#[cfg(windows)]
mod windows_job;
pub use discovery::find_executable;
pub use options::{ProcessOptions, validate_environment};
pub use recording::{Recorder, WireRecording};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use swarmie::{Error, Result};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, Command},
    sync::{mpsc, oneshot},
};

#[derive(Clone, Copy)]
pub enum WireStyle {
    JsonRpc,
    Pi,
    Claude,
}
struct Inner {
    tree: ProcessTree,
    stdin: tokio::sync::Mutex<Option<ChildStdin>>,
    pending: Arc<Mutex<BTreeMap<String, oneshot::Sender<Result<Value>>>>>,
    next: AtomicU64,
    kill: mpsc::UnboundedSender<()>,
    exited: Arc<AtomicBool>,
    closing: Arc<AtomicBool>,
    stderr: Arc<Mutex<VecDeque<String>>>,
    style: WireStyle,
    recording: Option<Arc<recording::Recorder>>,
    events: mpsc::Sender<Value>,
    activity: Arc<Mutex<WireActivity>>,
}
struct WireActivity {
    last: std::time::Instant,
    warned: bool,
    active: bool,
}
impl WireActivity {
    fn touch(&mut self) {
        self.last = std::time::Instant::now();
        self.warned = false;
    }
}
fn diagnostic(events: &mpsc::Sender<Value>, value: Value) {
    if let Err(error) = events.try_send(json!({"_runtime_diagnostic":value})) {
        eprintln!("process diagnostic could not be delivered: {error}");
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        if !self.exited.load(Ordering::SeqCst) {
            self.tree.terminate();
        }
        let _ = self.kill.send(());
    }
}
#[derive(Clone)]
pub struct JsonProcess(Arc<Inner>);
impl JsonProcess {
    pub async fn spawn(
        executable: &Path,
        args: &[String],
        cwd: &Path,
        style: WireStyle,
    ) -> Result<(Self, mpsc::Receiver<Value>)> {
        Self::spawn_with_env(executable, args, cwd, style, &BTreeMap::new()).await
    }
    pub async fn spawn_with_env(
        executable: &Path,
        args: &[String],
        cwd: &Path,
        style: WireStyle,
        environment: &BTreeMap<String, String>,
    ) -> Result<(Self, mpsc::Receiver<Value>)> {
        Self::spawn_with_options(
            executable,
            args,
            cwd,
            style,
            &ProcessOptions {
                environment: environment.clone(),
                ..Default::default()
            },
        )
        .await
    }
    pub async fn spawn_with_options(
        executable: &Path,
        args: &[String],
        cwd: &Path,
        style: WireStyle,
        options: &ProcessOptions,
    ) -> Result<(Self, mpsc::Receiver<Value>)> {
        validate_environment(&options.environment)?;
        if options
            .stall_timeout
            .is_some_and(|duration| duration < Duration::from_millis(10))
        {
            return Err(Error::Invalid(
                "stall timeout must be at least 10 ms".into(),
            ));
        }
        let recording = options
            .recording
            .as_ref()
            .map(|settings| recording::Recorder::open(settings, &options.environment).map(Arc::new))
            .transpose()?;
        let activity = Arc::new(Mutex::new(WireActivity {
            last: std::time::Instant::now(),
            warned: false,
            active: false,
        }));
        let mut command = Command::new(executable);
        command
            .args(args)
            .envs(&options.environment)
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn()?;
        let tree = ProcessTree::attach(&mut child)?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Provider("stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Provider("stdout unavailable".into()))?;
        let stderr_pipe = child
            .stderr
            .take()
            .ok_or_else(|| Error::Provider("stderr unavailable".into()))?;
        let pending: Arc<Mutex<BTreeMap<String, oneshot::Sender<Result<Value>>>>> = Arc::default();
        let exited = Arc::new(AtomicBool::new(false));
        let closing = Arc::new(AtomicBool::new(false));
        let stderr = Arc::new(Mutex::new(VecDeque::new()));
        let (kill, mut kill_rx) = mpsc::unbounded_channel();
        let (events, rx) = mpsc::channel(1024);
        if let Some(settings) = &options.recording {
            diagnostic(
                &events,
                json!({"level":"info","code":"wire_recording_started","message":"Wire recording started","path":settings.path}),
            );
        }
        let pending_reader = pending.clone();
        let killer = kill.clone();
        let events_reader = events.clone();
        let recording_reader = recording.clone();
        let activity_reader = activity.clone();
        if let Some(timeout) = options.stall_timeout {
            let activity = activity.clone();
            let pending = pending.clone();
            let exited = exited.clone();
            let events = events.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(timeout.min(Duration::from_millis(250)));
                while !exited.load(Ordering::SeqCst) {
                    interval.tick().await;
                    let requests = !pending.lock().unwrap().is_empty();
                    let stalled = {
                        let mut state = activity.lock().unwrap();
                        if (requests || state.active)
                            && !state.warned
                            && state.last.elapsed() >= timeout
                        {
                            state.warned = true;
                            true
                        } else {
                            false
                        }
                    };
                    if stalled {
                        diagnostic(
                            &events,
                            json!({"level":"warning","code":"wire_stalled","message":"The provider wire is inactive while an operation is outstanding; it has not been cancelled.","inactiveMs":timeout.as_millis() as u64}),
                        );
                    }
                }
            });
        }
        let reader = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = Vec::new();
                // Taking a bounded view caps allocation even for an unterminated hostile record.
                let read = (&mut reader)
                    .take(8 * 1024 * 1024 + 1)
                    .read_until(b'\n', &mut line)
                    .await;
                match read {
                    Ok(0) => break,
                    Ok(_) if line.len() > 8 * 1024 * 1024 => {
                        let _ = events_reader
                            .try_send(json!({"_runtime_error":"provider record exceeds 8 MiB"}));
                        let _ = killer.send(());
                        break;
                    }
                    Err(e) => {
                        let _ = events_reader.try_send(json!({"_runtime_error":e.to_string()}));
                        let _ = killer.send(());
                        break;
                    }
                    _ => (),
                }
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let frame: Value = match serde_json::from_slice(&line) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = events_reader.try_send(
                            json!({"_runtime_error":format!("malformed provider JSON: {e}")}),
                        );
                        let _ = killer.send(());
                        break;
                    }
                };
                activity_reader.lock().unwrap().touch();
                if let Some(recorder) = &recording_reader
                    && let Some(warning) = recorder.record("receive", &frame)
                {
                    diagnostic(&events_reader, warning);
                }
                if frame["type"] == "control_response" {
                    let r = &frame["response"];
                    if let Some(tx) = pending_reader
                        .lock()
                        .unwrap()
                        .remove(&r["request_id"].to_string())
                    {
                        let result = if r["subtype"] == "error" {
                            Err(Error::Provider(r["error"].to_string()))
                        } else {
                            Ok(r["response"].clone())
                        };
                        let _ = tx.send(result);
                    }
                    continue;
                }
                let response = frame.get("id").is_some()
                    && (frame.get("result").is_some()
                        || frame.get("error").is_some()
                        || frame["type"] == "response");
                if response
                    && let Some(tx) = pending_reader
                        .lock()
                        .unwrap()
                        .remove(&frame["id"].to_string())
                {
                    let result = if frame["success"] == false
                        || frame.get("error").is_some_and(|v| !v.is_null())
                    {
                        Err(Error::Provider(frame["error"].to_string()))
                    } else {
                        Ok(if frame["type"] == "response" {
                            frame["data"].clone()
                        } else {
                            frame["result"].clone()
                        })
                    };
                    let _ = tx.send(result);
                } else if events_reader.try_send(frame).is_err() {
                    // Explicit fatal overflow instead of losing approvals or turn completion.
                    let _ = killer.send(());
                    break;
                }
            }
        });
        let stderr_log = stderr.clone();
        let stderr_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr_pipe);
            loop {
                let mut line = Vec::new();
                match (&mut reader).take(8193).read_until(b'\n', &mut line).await {
                    Ok(0) | Err(_) => break,
                    _ => (),
                }
                let line = String::from_utf8_lossy(&line).into_owned();
                let mut log = stderr_log.lock().unwrap();
                if log.len() == 32 {
                    log.pop_front();
                }
                log.push_back(line);
            }
        });
        let exited_task = exited.clone();
        let closing_task = closing.clone();
        let stderr_task_log = stderr.clone();
        let pending_task = pending.clone();
        let retained_events = events.clone();
        let supervised_tree = tree.clone();
        tokio::spawn(async move {
            let status = tokio::select! { s = child.wait() => s, _ = kill_rx.recv() => { supervised_tree.terminate(); let _ = child.kill().await; child.wait().await } };
            supervised_tree.terminate();
            exited_task.store(true, Ordering::SeqCst);
            // Process exit does not imply its pipe is empty. Drain through EOF before
            // failing pending responses, including the final native completion.
            let mut reader = reader;
            if tokio::time::timeout(Duration::from_millis(250), &mut reader)
                .await
                .is_err()
            {
                reader.abort();
            }
            let mut stderr_task = stderr_task;
            if tokio::time::timeout(Duration::from_millis(100), &mut stderr_task)
                .await
                .is_err()
            {
                stderr_task.abort();
            }
            let log = stderr_task_log
                .lock()
                .unwrap()
                .iter()
                .cloned()
                .collect::<String>();
            let message = format!("provider exited ({status:?}); {log}");
            for (_, tx) in std::mem::take(&mut *pending_task.lock().unwrap()) {
                let _ = tx.send(Err(Error::Provider(message.clone())));
            }
            // A full queue is drained before this final failure; close can interrupt a blocked send.
            let _ = events
                .send(
                    json!({"_runtime_exit":message,"expected":closing_task.load(Ordering::SeqCst)}),
                )
                .await;
        });
        Ok((
            Self(Arc::new(Inner {
                tree,
                stdin: tokio::sync::Mutex::new(Some(stdin)),
                pending,
                next: AtomicU64::new(1),
                kill,
                exited,
                closing,
                stderr,
                style,
                recording,
                events: retained_events,
                activity,
            })),
            rx,
        ))
    }
    pub async fn send(&self, frame: Value) -> Result<()> {
        if self.0.exited.load(Ordering::SeqCst) {
            return Err(Error::Closed);
        }
        let data = serde_json::to_vec(&frame)?;
        if data.len() > 16 * 1024 * 1024 {
            return Err(Error::Invalid("provider request exceeds 16 MiB".into()));
        }
        let mut guard = self.0.stdin.lock().await;
        let stdin = guard.as_mut().ok_or(Error::Closed)?;
        self.0.activity.lock().unwrap().touch();
        if let Some(recorder) = &self.0.recording
            && let Some(warning) = recorder.record("send", &frame)
        {
            diagnostic(&self.0.events, warning);
        }
        tokio::time::timeout(Duration::from_secs(10), async {
            stdin.write_all(&data).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await
        })
        .await
        .map_err(|_| Error::Timeout("provider write".into()))??;
        Ok(())
    }
    /// Mark a streaming operation active after its native request has been accepted.
    pub fn set_active(&self, active: bool) {
        let mut state = self.0.activity.lock().unwrap();
        if state.active != active {
            state.active = active;
            state.touch();
        }
    }
    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.request_timeout(method, params, Duration::from_secs(30))
            .await
    }
    pub async fn request_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value> {
        let n = self.0.next.fetch_add(1, Ordering::Relaxed);
        let id = match self.0.style {
            WireStyle::JsonRpc => json!(n),
            WireStyle::Pi | WireStyle::Claude => json!(format!("r{n}")),
        };
        let frame = match self.0.style {
            WireStyle::Claude => {
                let mut p = params.as_object().cloned().unwrap_or_default();
                p.insert("subtype".into(), json!(method));
                json!({"type":"control_request","request_id":id,"request":p})
            }
            WireStyle::JsonRpc => json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
            WireStyle::Pi => {
                let mut p = params.as_object().cloned().unwrap_or_default();
                p.insert("id".into(), id.clone());
                p.insert("type".into(), json!(method));
                Value::Object(p)
            }
        };
        let (tx, rx) = oneshot::channel();
        let key = id.to_string();
        self.0.pending.lock().unwrap().insert(key.clone(), tx);
        let _guard = PendingGuard {
            pending: self.0.pending.clone(),
            key,
        };
        self.send(frame).await?;
        tokio::time::timeout(timeout, rx)
            .await
            .map_err(|_| Error::Timeout(method.into()))?
            .map_err(|_| Error::Closed)?
    }
    pub async fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.send(json!({"jsonrpc":"2.0","method":method,"params":params}))
            .await
    }
    pub async fn respond(&self, id: Value, result: Value) -> Result<()> {
        self.send(json!({"jsonrpc":"2.0","id":id,"result":result}))
            .await
    }
    pub async fn reject(&self, id: Value, message: &str) -> Result<()> {
        self.send(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":message}}))
            .await
    }
    pub async fn close(&self) -> Result<()> {
        self.0.closing.store(true, Ordering::SeqCst);
        self.0.stdin.lock().await.take();
        for _ in 0..20 {
            if self.0.exited.load(Ordering::SeqCst) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        self.kill().await
    }
    pub async fn kill(&self) -> Result<()> {
        self.0.closing.store(true, Ordering::SeqCst);
        let _ = self.0.kill.send(());
        for _ in 0..80 {
            if self.0.exited.load(Ordering::SeqCst) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Err(Error::Timeout("process termination".into()))
    }
    pub fn stderr(&self) -> String {
        self.0.stderr.lock().unwrap().iter().cloned().collect()
    }
}
struct PendingGuard {
    pending: Arc<Mutex<BTreeMap<String, oneshot::Sender<Result<Value>>>>>,
    key: String,
}
impl Drop for PendingGuard {
    fn drop(&mut self) {
        self.pending.lock().unwrap().remove(&self.key);
    }
}
#[derive(Clone)]
struct ProcessTree {
    #[cfg(unix)]
    pid: Option<u32>,
    #[cfg(windows)]
    job: Arc<windows_job::Job>,
}
impl ProcessTree {
    fn attach(child: &mut tokio::process::Child) -> Result<Self> {
        Ok(Self {
            #[cfg(unix)]
            pid: child.id(),
            #[cfg(windows)]
            job: Arc::new(windows_job::Job::attach(child)?),
        })
    }
    fn terminate(&self) {
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        self.job.terminate();
    }
}
/// Supervised process for servers whose stdout is diagnostic rather than JSONL.
pub struct ManagedProcess {
    tree: ProcessTree,
    kill: mpsc::UnboundedSender<()>,
    exited: Arc<AtomicBool>,
    status: Arc<Mutex<Option<std::process::ExitStatus>>>,
}
type ManagedPipes = (
    ManagedProcess,
    Option<tokio::process::ChildStdin>,
    Option<tokio::process::ChildStdout>,
    Option<tokio::process::ChildStderr>,
);
impl ManagedProcess {
    /// A private native authentication terminal with owned process-group cleanup.
    #[cfg(unix)]
    pub async fn spawn_with_terminal(
        executable: &Path,
        args: &[String],
        cwd: &Path,
        env: &[(String, String)],
    ) -> Result<(Self, tokio::fs::File, tokio::fs::File)> {
        use std::os::fd::{AsRawFd, FromRawFd};
        validate_environment(&env.iter().cloned().collect())?;
        let mut master = -1;
        let mut slave = -1;
        let mut size = libc::winsize {
            ws_row: 40,
            ws_col: 120,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // openpty transfers two fresh descriptors, immediately owned by Files.
        if unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::addr_of_mut!(size),
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let master = unsafe { std::fs::File::from_raw_fd(master) };
        let slave = unsafe { std::fs::File::from_raw_fd(slave) };
        for file in [&master, &slave] {
            if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        let mut command = Command::new(executable);
        command
            .args(args)
            .current_dir(cwd)
            .envs(env.iter().cloned())
            .stdin(slave.try_clone()?)
            .stdout(slave.try_clone()?)
            .stderr(slave)
            .kill_on_drop(true);
        // setsid establishes the same owned process group used by termination.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0
                    || libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY as _, 0) < 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                let mut terminal = std::mem::zeroed();
                if libc::tcgetattr(libc::STDIN_FILENO, &mut terminal) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                terminal.c_lflag &= !(libc::ECHO | libc::ECHONL);
                if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &terminal) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        let input = tokio::fs::File::from_std(master.try_clone()?);
        let output = tokio::fs::File::from_std(master);
        Ok((Self::supervise(child)?, input, output))
    }
    pub async fn spawn(
        executable: &Path,
        args: &[String],
        cwd: &Path,
        env: &[(String, String)],
    ) -> Result<Self> {
        Self::spawn_inner(executable, args, cwd, env, false, false)
            .await
            .map(|(process, _, _, _)| process)
    }
    /// Capture a server's ready-line handshake while retaining process-group supervision.
    pub async fn spawn_with_stderr(
        executable: &Path,
        args: &[String],
        cwd: &Path,
        env: &[(String, String)],
    ) -> Result<(Self, tokio::process::ChildStderr)> {
        let (process, _, _, stderr) =
            Self::spawn_inner(executable, args, cwd, env, true, false).await?;
        Ok((process, stderr.expect("captured server stderr")))
    }
    /// Private login IPC with the same bounded process-tree ownership as servers.
    pub async fn spawn_with_pipes(
        executable: &Path,
        args: &[String],
        cwd: &Path,
        env: &[(String, String)],
    ) -> Result<(
        Self,
        tokio::process::ChildStdin,
        tokio::process::ChildStdout,
        tokio::process::ChildStderr,
    )> {
        let (process, stdin, stdout, stderr) =
            Self::spawn_inner(executable, args, cwd, env, true, true).await?;
        Ok((
            process,
            stdin.expect("captured login stdin"),
            stdout.expect("captured login stdout"),
            stderr.expect("captured login stderr"),
        ))
    }
    async fn spawn_inner(
        executable: &Path,
        args: &[String],
        cwd: &Path,
        env: &[(String, String)],
        capture_stderr: bool,
        capture_pipes: bool,
    ) -> Result<ManagedPipes> {
        validate_environment(&env.iter().cloned().collect())?;
        let mut cmd = Command::new(executable);
        cmd.args(args)
            .current_dir(cwd)
            .envs(env.iter().cloned())
            .stdin(if capture_pipes {
                std::process::Stdio::piped()
            } else {
                std::process::Stdio::null()
            })
            .stdout(if capture_pipes {
                std::process::Stdio::piped()
            } else {
                std::process::Stdio::null()
            })
            .stderr(if capture_stderr {
                std::process::Stdio::piped()
            } else {
                std::process::Stdio::inherit()
            })
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        Ok((Self::supervise(child)?, stdin, stdout, stderr))
    }
    fn supervise(mut child: tokio::process::Child) -> Result<Self> {
        let tree = ProcessTree::attach(&mut child)?;
        let (kill, mut rx) = mpsc::unbounded_channel();
        let exited = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(None));
        let completion = status.clone();
        let flag = exited.clone();
        let supervised_tree = tree.clone();
        tokio::spawn(async move {
            let result = tokio::select! {
                result = child.wait() => result,
                _ = rx.recv() => { supervised_tree.terminate(); let _ = child.start_kill(); child.wait().await }
            };
            *completion.lock().unwrap() = result.ok();
            supervised_tree.terminate();
            flag.store(true, Ordering::SeqCst);
        });
        Ok(Self {
            tree,
            kill,
            exited,
            status,
        })
    }
    pub fn exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }
    pub fn exit_status(&self) -> Option<std::process::ExitStatus> {
        *self.status.lock().unwrap()
    }
    pub async fn kill(&self) -> Result<()> {
        let _ = self.kill.send(());
        for _ in 0..80 {
            if self.exited() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Err(Error::Timeout("server termination".into()))
    }
}
impl Drop for ManagedProcess {
    fn drop(&mut self) {
        if !self.exited() {
            self.tree.terminate();
        }
        let _ = self.kill.send(());
    }
}
