//! The `shell` tool.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::ToolAnnotations;
use crate::coding::redact;
use crate::{ExitStatus, KeptEnd, OutputCut, Tool, ToolContext, ToolExecutionError, args_schema};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::coding::process::{Ended, KillOnDrop, Launcher, OutputLine, Process, Spawn};
use crate::coding::workspace::{Local, Workspace};
use crate::coding::{CUT_MARK, Live, MAX_LINE_CHARS, OutputLimit, Spill, blocking, clip};

/// Output past which the command is killed. While it runs, only what the
/// [`OutputLimit`] keeps, and each stream's unfinished line, is held in
/// memory.
const OUTPUT_LIMIT: u64 = 10 * 1024 * 1024;

/// Runs a command with `sh -c` on Unix or Windows PowerShell on Windows, in its
/// own process group or job, in its
/// [`Workspace`], as its [`Launcher`] starts processes. Its stdout and
/// stderr come back interleaved line by line, as each line ends, without
/// terminal escape sequences such as colours; output past the
/// [`OutputLimit`] keeps its first and last lines and the first and last
/// errors and failures from between. Publishes how the
/// command ended ([`ExitStatus`]) beside its output, and how the output was
/// cut ([`OutputCut`]) when it was, and reports the output
/// as it comes to its [`Live`], if it has one.
#[derive(Clone, Debug)]
pub struct Shell {
    /// Where the command runs.
    pub workspace: Arc<dyn Workspace>,
    /// How the command is started: a wrapper, a directory, its variables.
    pub launcher: Launcher,
    /// Where each piece of output is reported as it comes, before the
    /// command ends.
    pub live: Option<Live>,
    /// Where output that is cut is kept whole, named in the cut output.
    pub spill: Option<Spill>,
    /// How much of the output the result keeps; `None` keeps all of it, up
    /// to the 10 MB the command is killed at, for a host that cuts output
    /// itself.
    pub cut: Option<OutputLimit>,
    /// Whether a `cwd` that resolves outside the launcher's directory is
    /// refused. What the command itself does is not confined: it may still
    /// `cd` anywhere.
    pub confine: bool,
}

impl Default for Shell {
    /// Started on this machine as this process starts it, nothing spilled
    /// or reported live, output cut as [`OutputLimit::default`] cuts it.
    fn default() -> Self {
        Self {
            workspace: Arc::new(Local),
            launcher: Launcher::default(),
            live: None,
            spill: None,
            cut: Some(OutputLimit::default()),
            confine: false,
        }
    }
}

/// Arguments of [`Shell`].
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShellArgs {
    /// The command line.
    pub command: String,
    /// Seconds before the command is stopped; none waits until it ends.
    pub timeout_secs: Option<u64>,
    /// The directory to run it in, relative to the working directory;
    /// default the working directory.
    pub cwd: Option<String>,
}

impl ShellArgs {
    /// Run `command` until it ends, in the working directory.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            timeout_secs: None,
            cwd: None,
        }
    }

    /// The same arguments, stopping the command after `secs` seconds.
    pub fn timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }

    /// The same arguments, running the command in `dir`.
    pub fn in_dir(mut self, dir: impl Into<String>) -> Self {
        self.cwd = Some(dir.into());
        self
    }
}

/// What a command printed and how it ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellRun {
    /// Its output, stdout and stderr interleaved line by line in the order
    /// the lines came, as the [`OutputLimit`] keeps it, ending in a line
    /// saying how it ended.
    pub output: String,
    /// How it ended.
    pub status: ExitStatus,
}

impl Shell {
    /// Runs `args` as the tool does, waiting on a thread of its own, and
    /// returns the output and how the command ended. Dropping the future
    /// stops the command tree: `SIGTERM`, then `SIGKILL` after
    /// [`STOP_GRACE`](crate::coding::process::STOP_GRACE) on Unix; immediate
    /// job termination on Windows. Cancellation cannot undo external effects.
    pub async fn run(&self, args: ShellArgs) -> Result<ShellRun, ToolExecutionError> {
        self.execute(args, None).await.map(|(ran, _)| ran)
    }

    /// [`run`](Self::run), and how its output was cut.
    async fn execute(
        &self,
        args: ShellArgs,
        cancellation: Option<tembra::Cancellation>,
    ) -> Result<(ShellRun, Option<OutputCut>), ToolExecutionError> {
        let timeout = args
            .timeout_secs
            .map(|secs| Duration::from_secs(secs.max(1)));
        let process = self.start(&args)?;
        let stop = KillOnDrop(process.group());
        let shell = self.clone();
        let physical = Box::pin(blocking(move || shell.finish(process, timeout)));
        if let Some(cancellation) = cancellation {
            match futures::future::select(physical, Box::pin(cancellation.cancelled())).await {
                futures::future::Either::Left((outcome, _)) => outcome,
                futures::future::Either::Right(((), physical)) => {
                    // Stop the group, then retain physical ownership through
                    // reaping. Cancellation cannot undo effects it already made.
                    drop(stop);
                    let _ = physical.await;
                    Err(ToolExecutionError::other(
                        "shell execution was cancelled; its effects require reconciliation",
                    ))
                }
            }
        } else {
            physical.await
        }
    }
}

impl Tool for Shell {
    const NAME: &'static str = "shell";
    type Args = ShellArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        let kept = match self.cut {
            Some(cut) => format!(
                "; past {} lines or {} KB, its first and last lines and the lines with errors \
                 or failures from between",
                cut.lines,
                cut.bytes / 1024
            ),
            None => String::new(),
        };
        let interpreter = if cfg!(windows) {
            "Windows PowerShell (PowerShell syntax)"
        } else {
            "`sh -c` (POSIX syntax)"
        };
        format!(
            "Run a shell command with {interpreter} in the current directory, or in cwd, and wait \
             until it ends, however long it takes; set timeout_secs only for a command that may \
             hang. Returns the exit code and stdout and stderr interleaved line by line{kept}. Each call starts a fresh shell with an \
             empty stdin. Do not run what never ends, such as a server, in the foreground, and \
             do not assume a git repository."
        )
    }

    fn parameters(&self) -> serde_json::Value {
        args_schema::<ShellArgs>()
    }

    fn annotations(&self) -> ToolAnnotations {
        ToolAnnotations::default().with_title("Shell")
    }

    async fn call(
        &self,
        context: &mut ToolContext,
        args: ShellArgs,
    ) -> Result<String, ToolExecutionError> {
        let cancellation = context.execution().ok().map(tembra::Context::signal);
        let (ran, cut) = self.execute(args, cancellation).await?;
        context.insert_result(ran.status)?;
        if let Some(cut) = cut {
            context.insert_result(cut)?;
        }
        Ok(ran.output)
    }
}

impl Shell {
    /// Starts the command `args` names in the workspace, as this shell's
    /// launcher starts processes, its stdout and stderr in pipes of their
    /// own.
    fn start(&self, args: &ShellArgs) -> Result<Process, ToolExecutionError> {
        if !cfg!(any(unix, windows)) {
            return Err(ToolExecutionError::refused(
                "shell requires a Unix process-group or Windows job backend",
            ));
        }
        let base = self.launcher.cwd.as_deref();
        let dir = match &args.cwd {
            Some(dir) => {
                if self.confine {
                    self.workspace.place(base, true, dir)?;
                }
                Some(
                    self.workspace
                        .join(base.unwrap_or(Path::new("")), Path::new(dir)),
                )
            }
            None => base.map(Path::to_path_buf),
        };
        let how = Spawn {
            stdin: None,
            merge_stderr: false,
        };
        let (program, line) = command_line(&args.command);
        self.workspace
            .spawn(
                &self.launcher,
                dir.as_deref(),
                OsStr::new(program),
                &line,
                how,
            )
            .map_err(|error| ToolExecutionError::other(format!("could not run {program}: {error}")))
    }

    /// Waits for `process` to end, at most `timeout`, keeping its output as
    /// the cut says.
    fn finish(
        &self,
        mut process: Process,
        timeout: Option<Duration>,
    ) -> Result<(ShellRun, Option<OutputCut>), ToolExecutionError> {
        let mut kept = Kept {
            spill: self.spill.clone(),
            cut: self.cut,
            live: self.live.clone(),
            ..Kept::default()
        };
        let deadline = timeout.and_then(|timeout| Instant::now().checked_add(timeout));
        let end = process
            .run(deadline, |line| {
                kept.push(&line);
                kept.written <= OUTPUT_LIMIT
            })
            .map_err(|error| {
                ToolExecutionError::other(format!("waiting for the command failed: {error}"))
            })?;
        let silent = kept.written == 0;
        let (mut output, cut) = kept.text();
        let status = match end {
            Ended::Exited(status) => match status.code() {
                Some(code) => {
                    output.push_str(&format!("{EXIT_PREFIX}{code}]"));
                    ExitStatus::Exited(code)
                }
                None => {
                    output.push_str(&killed(status, process.remote()));
                    ExitStatus::Signaled
                }
            },
            Ended::TimedOut => {
                let secs = timeout.unwrap_or_default().as_secs();
                output.push_str(&format!(
                    "{TIMEOUT_PREFIX}{secs}s and stopped{}",
                    there(&process)
                ));
                if silent {
                    output.push_str(SILENT);
                }
                output.push(']');
                ExitStatus::TimedOut
            }
            Ended::Stopped => {
                output.push_str(&format!(
                    "{LIMIT_PREFIX}{} MB{}]",
                    OUTPUT_LIMIT / 1024 / 1024,
                    there(&process)
                ));
                ExitStatus::Killed
            }
        };
        Ok((ShellRun { output, status }, cut))
    }
}

/// Select the local platform's interpreter. PowerShell receives one command
/// argument, with UTF-8 output enabled; user code never goes through cmd.exe.
fn command_line(command: &str) -> (&'static str, Vec<OsString>) {
    if cfg!(windows) {
        ("powershell.exe", vec![
            "-NoLogo".into(), "-NoProfile".into(), "-NonInteractive".into(),
            "-Command".into(),
            format!("[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); $OutputEncoding = [Console]::OutputEncoding; {command}").into(),
        ])
    } else {
        ("sh", vec!["-c".into(), command.into()])
    }
}

/// What a status line adds when the command printed nothing at all.
const SILENT: &str = ". It printed nothing. If it should have, it may hold its output back when \
                      that is not a terminal (`python3 -u` or `stdbuf -oL` make it print as it \
                      goes); if it needs longer, give a larger `timeout_secs`";

/// Where `process` was stopped besides this machine: the machine it ran
/// the command on, whose end of the transport stops the command when this
/// end is stopped (see [`remote`](crate::coding::remote)).
fn there(process: &Process) -> String {
    match process.remote() {
        Some(host) => format!(", on {host} too"),
        None => String::new(),
    }
}

/// How the status line of a command that exited begins.
const EXIT_PREFIX: &str = "[exit code ";
/// How the status line of a command killed by a signal begins.
const KILLED_PREFIX: &str = "[killed by ";
/// How the status line of a command stopped at its timeout begins.
const TIMEOUT_PREFIX: &str = "[timed out after ";
/// How the status line of a command stopped at the output limit begins.
const LIMIT_PREFIX: &str = "[stopped after printing more than ";

/// How each line [`Shell`] ends a command's output with begins, one per
/// way the command can end.
const STATUS_STARTS: [&str; 4] = [EXIT_PREFIX, KILLED_PREFIX, TIMEOUT_PREFIX, LIMIT_PREFIX];

/// The status line of a command that ended by a signal this tool did not
/// send (its timeout and output limit say so themselves): which signal,
/// what likely sent it, and what to do. With `remote`, the process was the
/// transport to that machine, and its end there stopped the command.
fn killed(status: std::process::ExitStatus, remote: Option<&str>) -> String {
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        let (name, why) = sent(signal);
        if let Some(host) = remote {
            return format!(
                "{KILLED_PREFIX}{name} (signal {signal}), which this tool did not send: {why}. It \
                 was the connection to {host}, and the command there was stopped with it]"
            );
        }
        return format!(
            "{KILLED_PREFIX}{name} (signal {signal}), which this tool did not send: {why}. It was \
             the process this call started here, `sh` or the last program it ran; when that \
             program was a client of another machine, such as a remote shell, the command \
             there may still be running or may have finished, so check there before running \
             it again]"
        );
    }
    #[cfg(not(unix))]
    let _ = (status, remote);
    format!("{KILLED_PREFIX}a signal]")
}

/// The name of `signal` and what likely sent it.
#[cfg(unix)]
pub(crate) fn sent(signal: i32) -> (&'static str, &'static str) {
    match signal {
        libc::SIGHUP => ("SIGHUP", "its terminal or session went away"),
        libc::SIGINT => ("SIGINT", "something on this machine interrupted it"),
        libc::SIGQUIT => ("SIGQUIT", "something on this machine asked it to quit"),
        libc::SIGILL => ("SIGILL", "it crashed"),
        libc::SIGTRAP => ("SIGTRAP", "it crashed, or a debugger stopped it"),
        libc::SIGABRT => ("SIGABRT", "it aborted itself, as on a failed assertion"),
        libc::SIGBUS => ("SIGBUS", "it crashed"),
        libc::SIGFPE => ("SIGFPE", "it crashed on an arithmetic error"),
        libc::SIGKILL => (
            "SIGKILL",
            "something on this machine killed it, such as the out-of-memory killer or a `kill`",
        ),
        libc::SIGSEGV => ("SIGSEGV", "it crashed"),
        libc::SIGPIPE => ("SIGPIPE", "it wrote to a pipe that nothing read any more"),
        libc::SIGTERM => ("SIGTERM", "something on this machine asked it to stop"),
        _ => ("a signal", "something on this machine sent it"),
    }
}

/// Splits a `shell` call's output ([`ShellRun::output`]) into what the
/// command printed, each line with its `\n`, and the line after it saying
/// how the command ended; `text` whole and `None` when it does not end in
/// such a line. A command's own output never ends the text, so a line it
/// printed that reads like one is kept as printed.
///
/// ```
/// use swarmie_tools::coding::split_status;
///
/// assert_eq!(split_status("built\n[exit code 0]"), ("built\n", Some("[exit code 0]")));
/// assert_eq!(split_status("no status\n"), ("no status\n", None));
/// ```
pub fn split_status(text: &str) -> (&str, Option<&str>) {
    let start = text.rfind('\n').map_or(0, |newline| newline + 1);
    match text.split_at_checked(start) {
        Some((printed, last))
            if last.ends_with(']') && STATUS_STARTS.iter().any(|at| last.starts_with(at)) =>
        {
            (printed, Some(last))
        }
        _ => (text, None),
    }
}

/// The longest line [`Kept`] waits for the end of; a longer one is broken
/// into lines of this length.
const MAX_HELD: usize = 1024 * 1024;
/// How many lines after a line that [`matters`] are kept with it, such as
/// an error's place in the code or a failed assertion's values.
const CONTEXT: usize = 3;
/// The longest a line kept from the cut middle is shown.
const MAX_PICKED: usize = 300;

/// Whether `line` from the cut middle of an output is kept: an error, a
/// failure or a panic, as compilers, test runners and scripts print them.
/// Warnings are not, so a flood of them never crowds out an error.
fn matters(line: &str) -> bool {
    let start = line.trim_start();
    start.starts_with("error")
        || start.starts_with("Error")
        || [
            "FAILED",
            "FAIL:",
            "failed",
            "panicked",
            "Traceback",
            "Error:",
        ]
        .iter()
        .any(|word| line.contains(word))
}

/// `line` without the terminal escape sequences in it, such as the colours
/// a program prints even when its output is no terminal, or a link's or a
/// window title's markup: the text between them stays.
fn plain(line: &str) -> Cow<'_, str> {
    if !line.contains('\x1b') {
        return Cow::Borrowed(line);
    }
    let mut text = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            text.push(c);
            continue;
        }
        match chars.next() {
            // A control sequence: parameters, then one final character.
            Some('[') => {
                let _ = chars.by_ref().find(|c| ('\x40'..='\x7e').contains(c));
            }
            // A string, such as a link or a title, up to BEL or `ESC \`.
            Some(']' | 'P' | 'X' | '^' | '_') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' || (c == '\x1b' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            // Intermediate characters, then one final character.
            Some(' '..='/') => {
                let _ = chars.by_ref().find(|c| !(' '..='/').contains(c));
            }
            _ => {}
        }
    }
    Cow::Owned(text)
}

/// A line of the output [`Kept`] holds whole, or cut short once the
/// output is cut.
struct Line {
    /// Its number, from 0.
    at: usize,
    /// Its text, ending in `\n`.
    text: String,
    /// Whether the whole line [`matters`].
    matters: bool,
    /// Whether it was cut short.
    long: bool,
}

impl Line {
    /// Cuts the line at `chars` characters, marked, when it is longer.
    fn shorten(&mut self, chars: usize) {
        let body = self.text.trim_end_matches(['\n', '\r']);
        let kept = clip(body, chars);
        if kept.len() < body.len() {
            self.text = format!("{kept}{CUT_MARK}\n");
            self.long = true;
        }
    }
}

/// What [`Shell`] keeps of a command's output while it runs, line by line:
/// all of it while it fits the [`OutputLimit`]; past it, the first lines
/// (half of the limit), the lines that [`matter`](matters) from the
/// middle with a few after each (at most a quarter: the first of them in
/// one eighth, the last in the other) and the last lines (the rest), each
/// cut at [`MAX_LINE_CHARS`]. The whole output goes to the spill file from
/// the first cut on.
#[derive(Default)]
struct Kept {
    /// Every byte received.
    written: u64,
    /// How much is kept; `None` keeps everything.
    cut: Option<OutputLimit>,
    /// The start of each stream's line still being written: stdout's,
    /// then stderr's.
    held: [String; 2],
    /// How many lines came.
    lines: usize,
    /// The first lines, once the output was cut.
    head: Vec<Line>,
    /// The bytes of `head`.
    head_bytes: usize,
    /// The first lines that matter from the middle.
    picked: Vec<Line>,
    /// The bytes of `picked`.
    picked_bytes: usize,
    /// The last lines that matter from the middle once `picked` is full,
    /// with the lines after them.
    later: VecDeque<Line>,
    /// The bytes of `later`.
    later_bytes: usize,
    /// Whether some lines that matter, or lines after them, were cut.
    missed: bool,
    /// How many lines after the last one that matters are kept too.
    context: usize,
    /// The last lines: all of them before the first cut.
    tail: VecDeque<Line>,
    /// The bytes of `tail`.
    tail_bytes: usize,
    /// Whether some output was cut, and `head` is final.
    cutting: bool,
    /// Where the whole output goes once some is cut.
    spill: Option<Spill>,
    /// The file it goes to, from the first cut on, and its path.
    spilled: Option<(PathBuf, File)>,
    /// Where each line is reported as it comes.
    live: Option<Live>,
    /// Whether the output is inside a private key block, whose lines are
    /// left out until its end line.
    in_key: bool,
}

impl Kept {
    /// Takes `piece` of the output: a whole line goes in at once, one
    /// still being written waits for its end, so a line of one stream is
    /// never split by the other's.
    fn push(&mut self, piece: &OutputLine) {
        self.written += piece.text().len() as u64;
        let held = match piece {
            OutputLine::Out(_) => 0,
            OutputLine::Err(_) => 1,
        };
        let Some(line) = self.held.get_mut(held) else {
            return;
        };
        line.push_str(piece.text());
        if line.ends_with('\n') || line.len() >= MAX_HELD {
            let line = std::mem::take(line);
            self.line(line);
        }
    }

    /// Takes the lines both streams were still writing, once the output
    /// ended.
    fn flush(&mut self) {
        for held in 0..self.held.len() {
            if let Some(line) = self.held.get_mut(held).map(std::mem::take)
                && !line.is_empty()
            {
                self.line(line);
            }
        }
    }

    /// Takes the next whole `line`, its terminal escape sequences dropped
    /// ([`plain`]) and its secrets redacted, reporting it live and spilling
    /// it, and cuts what no longer fits: no key it printed is shown,
    /// spilled or returned.
    fn line(&mut self, line: String) {
        let line = plain(&line);
        let mut line = self.redact(&line).into_owned();
        if line.is_empty() {
            return;
        }
        if !line.ends_with('\n') {
            line.push('\n');
        }
        if let Some(live) = &self.live {
            live.line(&line);
        }
        if let Some((_, file)) = &mut self.spilled
            && file.write_all(line.as_bytes()).is_err()
        {
            self.spilled = None;
        }
        let mut line = Line {
            at: self.lines,
            matters: self.cut.is_some() && matters(&line),
            text: line,
            long: false,
        };
        if self.cutting {
            line.shorten(MAX_LINE_CHARS);
        }
        self.tail_bytes += line.text.len();
        self.tail.push_back(line);
        self.lines += 1;
        let Some(limit) = self.cut else {
            return;
        };
        if !self.cutting {
            if self.tail.len() <= limit.lines && self.tail_bytes <= limit.bytes {
                return;
            }
            self.cutting = true;
            self.spill();
            for line in &mut self.tail {
                line.shorten(MAX_LINE_CHARS);
            }
            self.tail_bytes = self.tail.iter().map(|line| line.text.len()).sum();
            while self.head.len() < limit.lines / 2
                && let Some(line) = self.tail.pop_front()
            {
                if self.head_bytes + line.text.len() > limit.bytes / 2 {
                    self.tail.push_front(line);
                    break;
                }
                self.head_bytes += line.text.len();
                self.tail_bytes -= line.text.len();
                self.head.push(line);
            }
        }
        // The last lines take what the first and the picked ones leave.
        while self.tail.len()
            > limit
                .lines
                .saturating_sub(self.head.len() + self.picked.len() + self.later.len())
            || self.tail_bytes
                > limit
                    .bytes
                    .saturating_sub(self.head_bytes + self.picked_bytes + self.later_bytes)
        {
            let Some(mut line) = self.tail.pop_front() else {
                break;
            };
            self.tail_bytes -= line.text.len();
            if line.matters {
                self.context = CONTEXT + 1;
            }
            if self.context > 0 {
                self.context -= 1;
                line.shorten(MAX_PICKED);
                self.pick(limit, line);
            }
        }
    }

    /// Keeps `line` from the middle: in `picked` while it has room, then
    /// in `later`, which drops its oldest lines that matter, with the lines
    /// after them, to make room for new ones.
    fn pick(&mut self, limit: OutputLimit, line: Line) {
        let (lines, bytes) = (limit.lines / 8, limit.bytes / 8);
        if self.later.is_empty()
            && self.picked.len() < lines
            && self.picked_bytes + line.text.len() <= bytes
        {
            self.picked_bytes += line.text.len();
            self.picked.push(line);
            return;
        }
        self.later_bytes += line.text.len();
        self.later.push_back(line);
        while self.later.len() > lines || self.later_bytes > bytes {
            self.missed = true;
            while let Some(line) = self.later.pop_front() {
                self.later_bytes -= line.text.len();
                if self.later.front().is_none_or(|line| line.matters) {
                    break;
                }
            }
        }
    }

    /// `piece`, a line, with its secrets redacted ([`redact::text`]). A
    /// private key block spans lines: its first is redacted to its end, and
    /// the lines after it, through its end line, are left out.
    fn redact<'a>(&mut self, piece: &'a str) -> Cow<'a, str> {
        if self.in_key {
            self.in_key = !piece.contains("-----END ");
            return Cow::Borrowed("");
        }
        let mut redacted = redact::text(piece);
        if let Cow::Owned(line) = &mut redacted {
            self.in_key = redact::find(piece).any(|secret| {
                secret.kind == redact::Kind::PrivateKey
                    && piece
                        .get(secret.range)
                        .is_some_and(|key| !key.contains("-----END "))
            });
            // A key redacted to the line's end took its newline.
            if piece.ends_with('\n') && !line.ends_with('\n') {
                line.push('\n');
            }
        }
        redacted
    }

    /// Starts keeping the whole output, from the lines kept so far, which
    /// are all of it before the first cut. Tried once.
    fn spill(&mut self) {
        let Some(spill) = self.spill.take() else {
            return;
        };
        self.spilled = spill.create("shell").ok().filter(|(_, file)| {
            let mut file = file;
            self.tail
                .iter()
                .try_for_each(|line| file.write_all(line.text.as_bytes()))
                .is_ok()
        });
    }

    /// The kept output: after a cut, a line saying how much was cut and
    /// how to read it in the file that has it all, then the kept lines with
    /// a `[… n lines cut (lines a-b) …]` line where lines are missing,
    /// numbered as in that file; and that cut as a fact.
    fn text(mut self) -> (String, Option<OutputCut>) {
        self.flush();
        let kept_lines = self.head.len() + self.picked.len() + self.later.len() + self.tail.len();
        let errors = match self.missed {
            true => "the first and last lines with errors or failures",
            false => "lines with errors or failures",
        };
        let cut = self.lines - kept_lines;
        let kept: Vec<&Line> = (self.head.iter().chain(&self.picked))
            .chain(&self.later)
            .chain(&self.tail)
            .collect();
        let long: Vec<usize> = kept
            .iter()
            .filter(|line| line.long)
            .map(|line| line.at)
            .collect();
        let mut text = String::new();
        let mut fact = None;
        if cut > 0 || !long.is_empty() {
            let mut what = Vec::new();
            if cut > 0 {
                what.push(format!(
                    "{cut} of {} lines cut, but for {errors}",
                    self.lines
                ));
            }
            if !long.is_empty() {
                what.push(format!(
                    "{} long {} cut short",
                    long.len(),
                    if long.len() == 1 { "line" } else { "lines" }
                ));
            }
            let what = what.join(", and ");
            match &self.spilled {
                Some((path, _)) => {
                    // The first line cut, from 1, and how many follow it.
                    let mut next = 0;
                    let (first, gap) = kept
                        .iter()
                        .find_map(|line| {
                            let gap = (line.at > next).then(|| (next + 1, line.at - next));
                            next = line.at + 1;
                            gap
                        })
                        .unwrap_or((next + 1, cut));
                    let read = match long.first() {
                        Some(at) if cut == 0 => format!(
                            "with offset {} and limit 1 for the first long line, whose note says \
                             how to read on in it",
                            at + 1
                        ),
                        _ => format!(
                            "with offset {first} and limit {} for the first lines cut",
                            gap.min(crate::coding::MAX_LINES)
                        ),
                    };
                    text.push_str(&format!(
                        "[{what}; all of the output is in {}: `read` it there {read}, rather \
                         than running the command again]\n",
                        path.display(),
                    ));
                }
                None => text.push_str(&format!("[{what}]\n")),
            }
            fact = Some(OutputCut {
                kept: KeptEnd::Ends,
                kept_lines,
                total_lines: self.lines,
                spill: self
                    .spilled
                    .as_ref()
                    .map(|(path, _)| path.to_string_lossy().into_owned()),
            });
        }
        let mut next = 0;
        for line in kept {
            if line.at > next {
                text.push_str(&format!(
                    "[… {} lines cut (lines {}-{}) …]\n",
                    line.at - next,
                    next + 1,
                    line.at
                ));
            }
            text.push_str(&line.text);
            next = line.at + 1;
        }
        (text, fact)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use crate::{ExitStatus, KeptEnd, OutputCut, Tool, ToolContext};

    use super::{Shell, ShellArgs, split_status};
    use crate::coding::OutputLimit;
    use crate::coding::process::Launcher;

    fn shell(command: &str, spill: Option<crate::coding::Spill>) -> String {
        let shell = Shell {
            spill,
            ..Shell::default()
        };
        futures::executor::block_on(shell.run(ShellArgs::new(command)))
            .map(|ran| ran.output)
            .unwrap_or_default()
    }

    /// What `read` shows for `args`, empty when it fails.
    fn read_text(args: crate::coding::ReadArgs) -> String {
        let (read, mut context) = (crate::coding::Read::default(), ToolContext::new());
        futures::executor::block_on(Tool::call(&read, &mut context, args))
            .ok()
            .and_then(|shown| shown.as_text().map(str::to_owned))
            .unwrap_or_default()
    }

    /// The command runs as the launcher starts it (under its wrapper, with its
    /// variables) in the directory the call names, and the call publishes how
    /// it ended beside its output.
    #[cfg(unix)]
    #[test]
    fn a_call_runs_as_its_launcher_says_and_publishes_how_it_ended() {
        let dir = std::env::temp_dir().join(format!("swarmie-tools-launch-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).ok();
        let shell = Shell {
            launcher: Launcher::default()
                .wrapped(["env", "WRAPPED=yes"])
                .in_dir(&dir)
                .with_env("SET", "set")
                .without_env(["SWARMIE_TEST_UNSET"]),
            ..Shell::default()
        };
        let mut context = ToolContext::new();
        let args = ShellArgs::new(
            "echo \"$WRAPPED $SET ${SWARMIE_TEST_UNSET:-gone} $(basename \"$PWD\")\"; exit 3",
        )
        .in_dir("sub")
        .timeout_secs(5);
        let output = futures::executor::block_on(Tool::call(&shell, &mut context, args));
        assert_eq!(
            output.as_deref().ok(),
            Some("yes set gone sub\n[exit code 3]")
        );
        let status = context.result::<ExitStatus>().ok().flatten();
        assert_eq!(status, Some(ExitStatus::Exited(3)));
        std::fs::remove_dir_all(dir).ok();
    }

    /// Without a cut, the whole output comes back, for a host that cuts it.
    #[cfg(unix)]
    #[test]
    fn without_a_cut_the_whole_output_comes_back() {
        let shell = Shell {
            cut: None,
            ..Shell::default()
        };
        let ran = futures::executor::block_on(shell.run(ShellArgs::new("seq 1 5000")));
        let ran = ran.ok();
        assert!(
            ran.as_ref()
                .is_some_and(|ran| ran.output.starts_with("1\n2\n")
                    && ran.output.ends_with("5000\n[exit code 0]"))
        );
        assert_eq!(ran.map(|ran| ran.status), Some(ExitStatus::Exited(0)));
    }

    /// A process that leaves the command's process group keeps the output pipe
    /// open after the kill; the call must still return.
    #[cfg(target_os = "linux")]
    #[test]
    fn returns_when_a_process_outside_the_group_holds_the_pipe() {
        use std::time::{Duration, Instant};

        let started = Instant::now();
        let output = shell("setsid sleep 5 & echo started", None);
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(output.contains("started"), "{output}");
        assert!(output.ends_with("[exit code 0]"), "{output}");
    }

    /// Output cut by lines or by bytes is kept whole, and `read` reads it back
    /// by the path the cut output names.
    #[cfg(unix)]
    #[test]
    fn cut_output_is_read_back_by_its_handle() {
        let dir = std::env::temp_dir().join(format!("swarmie-tools-spill-{}", std::process::id()));
        let spill = Some(crate::coding::Spill(dir.clone()));
        for (command, lines) in [("seq 1 3000", 3000), ("seq 1 300000", 300000)] {
            let output = shell(command, spill.clone());
            let handle = output
                .lines()
                .next()
                .and_then(|line| line.split_once(" is in "))
                .and_then(|(_, rest)| rest.split_once(':'))
                .map(|(path, _)| path.to_owned())
                .unwrap_or_default();
            let args = crate::coding::ReadArgs::new(handle.clone()).lines(lines / 2, 1);
            let read = read_text(args);
            let wanted = format!("{}\n[", lines / 2);
            assert!(read.starts_with(&wanted), "{output:.200} {read:?}");
            // The note's own offset and limit read the first lines it cut.
            let number = |after: &str| {
                let first = output.lines().next().unwrap_or_default();
                let rest = first.split_once(after).map_or("", |(_, rest)| rest);
                rest.split(' ').next().and_then(|n| n.parse::<usize>().ok())
            };
            let (offset, limit) = (number("with offset ").unwrap_or(0), number(" and limit "));
            let args = crate::coding::ReadArgs::new(handle).lines(offset, limit.unwrap_or(0));
            let read = read_text(args);
            let gap = format!("lines cut (lines {offset}-");
            assert!(output.contains(&gap), "{output:.300}");
            assert!(read.starts_with(&format!("{offset}\n")), "{read:.100}");
        }
        assert!(shell("seq 1 3", spill).starts_with("1\n2\n3\n"));
        assert_eq!(std::fs::read_dir(&dir).map(Iterator::count).ok(), Some(2));
        std::fs::remove_dir_all(dir).ok();
    }

    /// Each piece of output reaches the shell's `Live` while the command still
    /// runs: here the command waits for a file the sink makes on its first line.
    #[cfg(unix)]
    #[test]
    fn live_output_arrives_before_the_command_ends() {
        use std::sync::{Arc, Mutex, PoisonError};

        use crate::coding::Live;

        let flag = std::env::temp_dir().join(format!("swarmie-tools-live-{}", std::process::id()));
        std::fs::remove_file(&flag).ok();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let live = Live::new({
            let (seen, flag) = (seen.clone(), flag.clone());
            move |text| {
                std::fs::write(&flag, "").ok();
                seen.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(text.to_owned());
            }
        });
        let shell = Shell {
            live: Some(live),
            ..Shell::default()
        };
        let command = format!(
            "echo one; while [ ! -e '{}' ]; do sleep 0.05; done; echo two",
            flag.display()
        );
        let ran = futures::executor::block_on(shell.run(ShellArgs::new(command).timeout_secs(5)));
        assert_eq!(ran.map(|ran| ran.status).ok(), Some(ExitStatus::Exited(0)));
        assert_eq!(
            *seen.lock().unwrap_or_else(PoisonError::into_inner),
            ["one\n", "two\n"]
        );
        std::fs::remove_file(flag).ok();
    }

    /// The command starts through the shell's workspace, in the directory the
    /// call names below the launcher's.
    #[cfg(unix)]
    #[test]
    fn the_command_starts_through_the_workspace_in_the_calls_directory() {
        use std::ffi::{OsStr, OsString};
        use std::path::{Path, PathBuf};
        use std::sync::{Arc, Mutex, PoisonError};

        use crate::coding::process::{Process, Spawn};
        use crate::coding::workspace::{Contents, Local, Stat, Workspace};

        /// This machine, noting the directory of every command it starts.
        #[derive(Debug, Default)]
        struct Recording(Mutex<Vec<Option<PathBuf>>>);

        impl Workspace for Recording {
            fn stat(&self, path: &Path) -> std::io::Result<Stat> {
                Local.stat(path)
            }
            fn read(&self, path: &Path, limit: u64) -> std::io::Result<Contents> {
                Local.read(path, limit)
            }
            fn write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
                Local.write(path, bytes)
            }
            fn remove(&self, path: &Path) -> std::io::Result<()> {
                Local.remove(path)
            }
            fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
                Local.rename(from, to)
            }
            fn canonical(&self, path: &Path) -> PathBuf {
                Local.canonical(path)
            }
            fn files(
                &self,
                root: &Path,
                glob: Option<&str>,
                listing: crate::coding::workspace::Listing,
            ) -> std::io::Result<Vec<PathBuf>> {
                Local.files(root, glob, listing)
            }
            fn spawn(
                &self,
                launcher: &Launcher,
                dir: Option<&Path>,
                program: &OsStr,
                args: &[OsString],
                how: Spawn,
            ) -> std::io::Result<Process> {
                self.0
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(dir.map(Path::to_path_buf));
                Local.spawn(launcher, dir, program, args, how)
            }
        }

        let dir = std::env::temp_dir().join(format!("swarmie-tools-spawn-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).ok();
        let recording = Arc::new(Recording::default());
        let shell = Shell {
            workspace: recording.clone(),
            launcher: Launcher::default().in_dir(&dir),
            confine: true,
            ..Shell::default()
        };
        let ran = futures::executor::block_on(shell.run(ShellArgs::new("pwd").in_dir("sub")));
        assert!(ran.is_ok_and(|ran| ran.output.contains("sub\n")));
        let refused = futures::executor::block_on(shell.run(ShellArgs::new("pwd").in_dir("..")));
        assert!(refused.is_err());
        assert_eq!(
            *recording.0.lock().unwrap_or_else(PoisonError::into_inner),
            [Some(dir.join("sub"))]
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// The status line splits off a run's output, also after a line the
    /// command printed that reads like one.
    #[cfg(unix)]
    #[test]
    fn the_status_line_splits_off_what_the_command_printed() {
        let output = shell("echo one; echo '[exit code 0]'; exit 4", None);
        assert_eq!(
            split_status(&output),
            ("one\n[exit code 0]\n", Some("[exit code 4]"))
        );
        let output = shell("kill -9 $$", None);
        let (printed, status) = split_status(&output);
        assert_eq!(printed, "");
        let status = status.unwrap_or_default();
        assert!(
            status.starts_with("[killed by SIGKILL (signal 9), which this tool did not send"),
            "{status}"
        );
    }

    /// A call with no `timeout_secs` waits until the command ends, however
    /// long it is quiet; one with `timeout_secs` is stopped then, and says so.
    #[cfg(unix)]
    #[test]
    fn a_command_runs_to_its_end_unless_its_timeout_stops_it() {
        let shell = Shell::default();
        let ran = futures::executor::block_on(shell.run(ShellArgs::new("sleep 2; echo built")));
        let output = ran.map(|ran| ran.output).unwrap_or_default();
        assert_eq!(output, "built\n[exit code 0]");
        let args = ShellArgs::new("echo started; sleep 10").timeout_secs(1);
        let ran = futures::executor::block_on(shell.run(args)).ok();
        assert_eq!(
            ran.as_ref().map(|ran| ran.status),
            Some(ExitStatus::TimedOut)
        );
        let output = ran.map(|ran| ran.output).unwrap_or_default();
        assert_eq!(
            split_status(&output),
            ("started\n", Some("[timed out after 1s and stopped]"))
        );
        let args = ShellArgs::new("sleep 10").timeout_secs(1);
        let output = futures::executor::block_on(shell.run(args)).map(|ran| ran.output);
        let output = output.unwrap_or_default();
        assert!(
            output.contains("It printed nothing. If it should have,")
                && output.ends_with("if it needs longer, give a larger `timeout_secs`]"),
            "{output}"
        );
    }

    /// stdout and stderr interleave only where a line ends: a line one stream
    /// writes in pieces is never split by the other's, and a last line without
    /// a newline gets one.
    #[cfg(unix)]
    #[test]
    fn the_streams_interleave_only_at_line_ends() {
        let output = shell(
            "printf 'test result:'; sleep 0.2; echo '   Doc-tests' >&2; sleep 0.2; echo ' ok.'; \
         printf out; printf err >&2",
            None,
        );
        assert_eq!(
            output,
            "   Doc-tests\ntest result: ok.\nout\nerr\n[exit code 0]"
        );
    }

    /// Past its limit, the output keeps its first lines (half of the limit),
    /// the lines with errors from the middle with a few after each, and its
    /// last lines in the rest, with a line naming the lines missing.
    #[cfg(unix)]
    #[test]
    fn a_cut_keeps_both_ends_and_the_errors_between() {
        let shell = Shell {
            cut: Some(OutputLimit {
                lines: 40,
                bytes: 100 * 1024,
            }),
            ..Shell::default()
        };
        let mut context = ToolContext::new();
        let command = "seq 1 1000; echo 'error[E0308]: mismatched types'; \
                   echo '  --> src/lib.rs:3:5'; seq 1001 3000";
        let output =
            futures::executor::block_on(Tool::call(&shell, &mut context, ShellArgs::new(command)));
        let output = output.unwrap_or_default();
        let mut lines = output.lines();
        let marker = "[2962 of 3002 lines cut, but for lines with errors or failures]";
        assert_eq!(lines.next(), Some(marker));
        let head: Vec<String> = (1..=20).map(|n| n.to_string()).collect();
        let tail: Vec<String> = (2985..=3000).map(|n| n.to_string()).collect();
        let mut wanted = head;
        wanted.extend(
            [
                "[… 980 lines cut (lines 21-1000) …]",
                "error[E0308]: mismatched types",
                "  --> src/lib.rs:3:5",
                "1001",
                "1002",
                "[… 1982 lines cut (lines 1005-2986) …]",
            ]
            .map(str::to_owned),
        );
        wanted.extend(tail);
        wanted.push("[exit code 0]".to_owned());
        assert_eq!(lines.collect::<Vec<_>>(), wanted);
        assert_eq!(
            context.result::<OutputCut>().ok().flatten(),
            Some(OutputCut {
                kept: KeptEnd::Ends,
                kept_lines: 40,
                total_lines: 3002,
                spill: None,
            })
        );
    }

    /// Once the output is cut, every line kept for the model is cut at 2000
    /// characters, on a character boundary, while the spill file keeps it
    /// whole: a first line of 300,000 characters keeps its start, and one
    /// among short lines too. An error word past the cut still keeps a line
    /// from the middle.
    #[cfg(unix)]
    #[test]
    fn a_long_line_keeps_its_start_and_the_spill_keeps_it_whole() {
        let dir = std::env::temp_dir().join(format!("swarmie-tools-long-{}", std::process::id()));
        let spilling = Shell {
            spill: Some(crate::coding::Spill(dir.clone())),
            ..Shell::default()
        };
        let x = |n: usize| format!("head -c {n} /dev/zero | tr '\\0' x");
        let mut context = ToolContext::new();
        let args = ShellArgs::new(format!("{}; echo", x(300_000)));
        let output = futures::executor::block_on(Tool::call(&spilling, &mut context, args));
        let output = output.unwrap_or_default();
        let mut lines = output.lines();
        let marker = lines.next().unwrap_or_default();
        assert!(
            marker.starts_with("[1 long line cut short; all of the output is in ")
                && marker.ends_with(
                    ": `read` it there with offset 1 and limit 1 for the first long line, whose \
                 note says how to read on in it, rather than running the command again]"
                ),
            "{marker}"
        );
        assert_eq!(
            lines.next(),
            Some(format!("{} […]", "x".repeat(2000)).as_str())
        );
        assert_eq!(lines.next(), Some("[exit code 0]"));
        let cut = context.result::<OutputCut>().ok().flatten();
        assert_eq!(
            cut.as_ref().map(|cut| (cut.kept_lines, cut.total_lines)),
            Some((1, 1))
        );
        let spilled = cut.and_then(|cut| cut.spill).unwrap_or_default();
        assert_eq!(
            std::fs::read_to_string(&spilled).ok(),
            Some(format!("{}\n", "x".repeat(300_000)))
        );

        let fits = shell(&format!("{}; echo", x(5000)), None);
        assert_eq!(fits, format!("{}\n[exit code 0]", "x".repeat(5000)));

        let command = format!(
            "seq 1 3000; {}; echo ' test FAILED'; seq 1 3000; yes é | head -n 3000 | tr -d '\\n'; \
         echo",
            x(5000)
        );
        let output = shell(&command, Some(crate::coding::Spill(dir.clone())));
        assert!(
            output.starts_with(
                "[4002 of 6002 lines cut, but for lines with errors or failures, and 2 long lines \
             cut short; all of the output is in "
            ),
            "{output:.300}"
        );
        assert!(
            output.contains(&format!("\n{} […]\n", "x".repeat(300))),
            "{output:.300}"
        );
        assert!(output.contains(&format!("\n{} […]\n", "é".repeat(2000))));
        std::fs::remove_dir_all(dir).ok();
    }

    /// The cut note's offset and limit name the first lines missing, past an
    /// error kept right after the first lines.
    #[cfg(unix)]
    #[test]
    fn the_cut_note_names_the_first_lines_missing() {
        let dir = std::env::temp_dir().join(format!("swarmie-tools-gap-{}", std::process::id()));
        let shell = Shell {
            spill: Some(crate::coding::Spill(dir.clone())),
            cut: Some(OutputLimit {
                lines: 40,
                bytes: 100 * 1024,
            }),
            ..Shell::default()
        };
        let ran = shell.run(ShellArgs::new("seq 1 20; echo 'test FAILED'; seq 21 3000"));
        let output = futures::executor::block_on(ran)
            .map(|ran| ran.output)
            .unwrap_or_default();
        let first = output.lines().next().unwrap_or_default();
        assert!(
            first.contains("with offset 25 and limit 2000 for the first lines cut"),
            "{first}"
        );
        assert!(
            output.contains("\ntest FAILED\n21\n22\n23\n[… 2961 lines cut (lines 25-2985) …]\n"),
            "{output:.600}"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// Past what the cut keeps of the lines that matter from the middle, it
    /// keeps the first and the last of them, each with the lines after it,
    /// and says so: the output is cut to the limit however many there are.
    #[cfg(unix)]
    #[test]
    fn a_cut_keeps_the_first_and_last_errors_between() {
        let shell = Shell {
            cut: Some(OutputLimit {
                lines: 80,
                bytes: 100 * 1024,
            }),
            ..Shell::default()
        };
        let mut context = ToolContext::new();
        let command = "seq 1 100; for i in $(seq 1 40); do echo \"error: e$i\"; seq 5; done; \
                   seq 101 400";
        let output =
            futures::executor::block_on(Tool::call(&shell, &mut context, ShellArgs::new(command)));
        let output = output.unwrap_or_default();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(
            lines.first().copied(),
            Some(
                "[560 of 640 lines cut, but for the first and last lines with errors or failures]"
            )
        );
        for kept in ["error: e1", "error: e2", "error: e39", "error: e40"] {
            assert!(lines.contains(&kept), "{kept} in {output}");
        }
        assert!(!lines.contains(&"error: e20"), "{output}");
        let cut = context.result::<OutputCut>().ok().flatten();
        assert_eq!(cut.map(|cut| cut.kept_lines), Some(80));
    }

    /// Terminal escape sequences a program prints, such as colours or a
    /// link's markup, are dropped from its output; their text stays.
    #[cfg(unix)]
    #[test]
    fn terminal_escape_sequences_are_dropped_from_the_output() {
        let command = "printf '\\033[1;31merror\\033[0m: \\033[4mbad\\033(B\\033[m\\n'; \
                   printf '\\033]8;;https://example.com\\033\\\\a link\\033]8;;\\a\\n'";
        assert_eq!(shell(command, None), "error: bad\na link\n[exit code 0]");
    }

    /// A call whose output was cut publishes the cut beside it: the ends kept,
    /// how many lines of how many, and the file holding them all.
    #[cfg(unix)]
    #[test]
    fn a_cut_output_publishes_the_cut() {
        let dir = std::env::temp_dir().join(format!("swarmie-tools-cut-{}", std::process::id()));
        let shell = Shell {
            spill: Some(crate::coding::Spill(dir.clone())),
            cut: Some(OutputLimit {
                lines: 2,
                bytes: 1024,
            }),
            ..Shell::default()
        };
        let mut context = ToolContext::new();
        let output = futures::executor::block_on(Tool::call(
            &shell,
            &mut context,
            ShellArgs::new("seq 1 5"),
        ));
        let cut = context.result::<OutputCut>().ok().flatten();
        let spill = cut
            .as_ref()
            .and_then(|cut| cut.spill.clone())
            .unwrap_or_default();
        assert!(output.is_ok_and(
            |output| output.starts_with("[3 of 5 lines cut,") && output.contains(&spill)
        ));
        assert_eq!(
            cut,
            Some(OutputCut {
                kept: KeptEnd::Ends,
                kept_lines: 2,
                total_lines: 5,
                spill: Some(spill.clone())
            })
        );
        assert_eq!(
            std::fs::read_to_string(&spill).ok().as_deref(),
            Some("1\n2\n3\n4\n5\n")
        );
        let mut context = ToolContext::new();
        futures::executor::block_on(Tool::call(&shell, &mut context, ShellArgs::new("seq 1 2")))
            .ok();
        assert_eq!(context.result::<OutputCut>().ok().flatten(), None);
        std::fs::remove_dir_all(dir).ok();
    }

    /// A key the command prints is redacted before it is kept, spilled or
    /// returned, and a private key block is left out through its end line.
    #[cfg(unix)]
    #[test]
    fn printed_keys_are_neither_returned_nor_spilled() {
        let dir = std::env::temp_dir().join(format!("swarmie-tools-keys-{}", std::process::id()));
        let printed = "printf 'OPENAI_API_KEY=sk-test-probe-1234567890abcdef\\n\
        -----BEGIN RSA PRIVATE KEY-----\\nMIIEsecretbody\\n-----END RSA PRIVATE KEY-----\\n\
        after\\n'";
        let output = shell(printed, None);
        assert_eq!(
            output,
            "OPENAI_API_KEY=[REDACTED]\n[REDACTED]\nafter\n[exit code 0]"
        );
        let spilled = shell(
            &format!("{printed}; seq 1 3000"),
            Some(crate::coding::Spill(dir.clone())),
        );
        let spill = std::fs::read_dir(&dir)
            .ok()
            .and_then(|mut files| files.next())
            .and_then(Result::ok)
            .and_then(|file| std::fs::read_to_string(file.path()).ok())
            .unwrap_or_default();
        std::fs::remove_dir_all(dir).ok();
        assert!(
            spill.starts_with("OPENAI_API_KEY=[REDACTED]\n[REDACTED]\nafter\n1\n"),
            "{spill:.200}"
        );
        assert!(!spilled.contains("secret"));
    }
}
