use mpai::{Error, Result};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::Path,
    sync::{Arc, Mutex},
};

const MAX_OUTPUT: usize = 1024 * 1024;
#[derive(Default)]
struct Output {
    bytes: Vec<u8>,
    first: u64,
    exited: bool,
    exit_code: Option<u32>,
}
struct Terminal {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    output: Arc<Mutex<Output>>,
}
#[derive(Default)]
pub struct Terminals(Mutex<BTreeMap<String, Terminal>>);
fn provider(e: impl std::fmt::Display) -> Error {
    Error::Provider(format!("terminal: {e}"))
}
impl Terminals {
    pub fn start(
        &self,
        cwd: &Path,
        program: &str,
        args: &[String],
        cols: u16,
        rows: u16,
    ) -> Result<Value> {
        let mut terminals = self.0.lock().unwrap();
        if terminals.len() >= 8 {
            return Err(Error::Invalid(
                "close an existing terminal before opening another".into(),
            ));
        }
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(provider)?;
        let mut command = CommandBuilder::new(program);
        command.args(args);
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        let mut reader = pair.master.try_clone_reader().map_err(provider)?;
        let writer = pair.master.take_writer().map_err(provider)?;
        let child = pair.slave.spawn_command(command).map_err(provider)?;
        drop(pair.slave);
        let output = Arc::new(Mutex::new(Output::default()));
        let sink = output.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut out = sink.lock().unwrap();
                        out.bytes.extend_from_slice(&buffer[..n]);
                        if out.bytes.len() > MAX_OUTPUT {
                            let excess = out.bytes.len() - MAX_OUTPUT;
                            out.bytes.drain(..excess);
                            out.first += excess as u64;
                        }
                    }
                }
            }
            sink.lock().unwrap().exited = true;
        });
        let id = uuid::Uuid::new_v4().to_string();
        terminals.insert(
            id.clone(),
            Terminal {
                master: pair.master,
                writer,
                child,
                output,
            },
        );
        Ok(json!({"terminalId":id}))
    }
    pub fn read(&self, id: &str, after: u64) -> Result<Value> {
        let mut map = self.0.lock().unwrap();
        let terminal = map
            .get_mut(id)
            .ok_or_else(|| Error::Invalid("unknown terminal".into()))?;
        let status = terminal.child.try_wait().map_err(provider)?;
        let mut out = terminal.output.lock().unwrap();
        if let Some(status) = status {
            out.exit_code = Some(status.exit_code());
        }
        let start = after.saturating_sub(out.first).min(out.bytes.len() as u64) as usize;
        let mut end = (start + 64 * 1024).min(out.bytes.len());
        // Keep incomplete UTF-8 suffixes for the next poll; terminal output is byte addressed.
        if let Err(error) = std::str::from_utf8(&out.bytes[start..end])
            && error.error_len().is_none()
            && !out.exited
        {
            end = start + error.valid_up_to();
        }
        Ok(
            json!({"text":String::from_utf8_lossy(&out.bytes[start..end]),"next":out.first + end as u64,"first":out.first,"truncated":after < out.first,"availableEnd":out.first + out.bytes.len() as u64,"exited":out.exited && out.exit_code.is_some() && end==out.bytes.len(),"exitCode":out.exit_code}),
        )
    }
    pub fn write(&self, id: &str, data: &str) -> Result<Value> {
        if data.len() > 64 * 1024 {
            return Err(Error::Invalid("terminal input exceeds 64 KiB".into()));
        }
        let mut map = self.0.lock().unwrap();
        let terminal = map
            .get_mut(id)
            .ok_or_else(|| Error::Invalid("unknown terminal".into()))?;
        terminal.writer.write_all(data.as_bytes())?;
        terminal.writer.flush()?;
        Ok(json!({"written":true}))
    }
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<Value> {
        let map = self.0.lock().unwrap();
        let terminal = map
            .get(id)
            .ok_or_else(|| Error::Invalid("unknown terminal".into()))?;
        terminal
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(provider)?;
        Ok(json!({"resized":true}))
    }
    pub fn close(&self, id: &str) -> Result<Value> {
        let mut terminal = self
            .0
            .lock()
            .unwrap()
            .remove(id)
            .ok_or_else(|| Error::Invalid("unknown terminal".into()))?;
        terminate(&mut terminal);
        Ok(json!({"closed":true}))
    }
}
fn terminate(terminal: &mut Terminal) {
    #[cfg(target_os = "linux")]
    if let Some(session) = terminal.child.process_id() {
        // Job-control shells give background jobs separate process groups, but keep
        // them in the PTY session. Enumerate that owned session before reaping its leader.
        if let Ok(entries) = std::fs::read_dir("/proc") {
            for entry in entries.flatten() {
                if let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>()
                    && unsafe { libc::getsid(pid) } == session as i32
                {
                    unsafe {
                        libc::kill(pid, libc::SIGKILL);
                    }
                }
            }
        }
    }
    #[cfg(unix)]
    {
        // The foreground job may have a different process group from its shell.
        if let Some(group) = terminal.master.process_group_leader() {
            unsafe {
                libc::killpg(group, libc::SIGKILL);
            }
        }
        if let Some(pid) = terminal.child.process_id() {
            unsafe {
                libc::killpg(pid as i32, libc::SIGKILL);
            }
        }
    }
    let _ = terminal.child.kill();
    let _ = terminal.child.wait();
}
impl Drop for Terminals {
    fn drop(&mut self) {
        for terminal in self.0.get_mut().unwrap().values_mut() {
            terminate(terminal);
        }
    }
}
