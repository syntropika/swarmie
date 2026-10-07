//! Bounded direct process execution for optional local integrations.
use mpai::{Error, Result};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};
pub async fn binary(program: &Path, cwd: &Path, args: &[String]) -> Result<Vec<u8>> {
    let mut command = Command::new(program);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            Error::Provider(format!("{} is unavailable: {error}", program.display()))
        })?;
    let _group = ProcessGroup(child.id());
    let mut out = child.stdout.take().unwrap().take(8 * 1024 * 1024 + 1);
    let mut err = child.stderr.take().unwrap().take(1024 * 1024 + 1);
    let result = tokio::time::timeout(Duration::from_secs(120), async {
        let mut stdout = vec![];
        let mut stderr = vec![];
        let (a, b) = tokio::join!(out.read_to_end(&mut stdout), err.read_to_end(&mut stderr));
        a?;
        b?;
        if stdout.len() > 8 * 1024 * 1024 || stderr.len() > 1024 * 1024 {
            return Err(Error::Invalid(
                "integration output exceeded its limit".into(),
            ));
        }
        let status = child.wait().await?;
        if !status.success() {
            return Err(Error::Provider(format!(
                "{} exited {status}: {}",
                program.display(),
                String::from_utf8_lossy(&stderr).trim()
            )));
        }
        Ok(stdout)
    })
    .await
    .map_err(|_| Error::Timeout(format!("{} operation", program.display())))?;
    if result.is_err() {
        let _ = child.kill().await;
    }
    result
}
pub fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).into()).collect()
}
pub fn available(program: &str) -> bool {
    let path = Path::new(program);
    if path.is_absolute() {
        return path.is_file();
    }
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|path| {
            #[cfg(windows)]
            {
                return ["", ".exe", ".cmd", ".bat"]
                    .iter()
                    .any(|suffix| path.join(format!("{program}{suffix}")).is_file());
            }
            #[cfg(not(windows))]
            {
                path.join(program).is_file()
            }
        })
    })
}

pub async fn run(program: &Path, cwd: &Path, args: &[String]) -> Result<String> {
    String::from_utf8(binary(program, cwd, args).await?)
        .map_err(|_| Error::Provider("integration output is not UTF-8".into()))
}

struct ProcessGroup(Option<u32>);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0 {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}
