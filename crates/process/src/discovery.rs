use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

/// Executable discovery never launches a harness. Login-shell PATH is a bounded fallback.
pub fn find_executable(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.components().count() > 1 {
        return executable(path).then(|| path.to_path_buf());
    }
    let mut paths: Vec<_> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        paths.extend([
            home.join(".local/bin"),
            home.join(".bun/bin"),
            home.join(".npm/bin"),
            home.join(".cargo/bin"),
        ]);
    }
    paths.extend([
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
    ]);
    if let Some(path) = paths
        .into_iter()
        .map(|p| p.join(name))
        .find(|p| executable(p))
    {
        return Some(path);
    }
    let shell = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())?;
    login_paths(&shell, std::env::var_os("HOME").unwrap_or_default())
        .into_iter()
        .map(|p| p.join(name))
        .find(|p| executable(p))
}
fn executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        true
    }
}
type Cache = BTreeMap<(PathBuf, std::ffi::OsString), (Instant, Vec<PathBuf>)>;
fn login_paths(shell: &Path, home: std::ffi::OsString) -> Vec<PathBuf> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Mutex::default).lock().unwrap();
    let key = (shell.to_path_buf(), home);
    if let Some((when, paths)) = cache.get(&key)
        && when.elapsed() < Duration::from_secs(300)
    {
        return paths.clone();
    }
    let paths = query_login_paths(shell).unwrap_or_default();
    if cache.len() >= 16 {
        cache.clear();
    }
    cache.insert(key, (Instant::now(), paths.clone()));
    paths
}
fn query_login_paths(shell: &Path) -> Option<Vec<PathBuf>> {
    if !executable(shell) {
        return None;
    }
    let mut command = Command::new(shell);
    command
        .args(["-lc", "printf '%s' \"$PATH\""])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(read_login_output(stdout));
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Err(_) => break None,
            _ => {}
        }
        if started.elapsed() >= Duration::from_secs(1) {
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    if status.is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    // Use the remaining lookup budget so reader scheduling cannot truncate a quick shell.
    let remaining = Duration::from_millis(1100).saturating_sub(started.elapsed());
    let bytes = receive.recv_timeout(remaining).ok()??;
    if !status?.success() || bytes.len() > 65536 {
        return None;
    }
    let path = std::str::from_utf8(&bytes).ok()?;
    Some(
        std::env::split_paths(path)
            .filter(|p| p.is_absolute())
            .collect(),
    )
}
fn read_login_output(mut stdout: std::process::ChildStdout) -> Option<Vec<u8>> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let fd = stdout.as_raw_fd();
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags == -1 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) == -1 {
                return None;
            }
        }
        let deadline = Instant::now() + Duration::from_millis(1100);
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            match stdout.read(&mut buffer) {
                Ok(0) => return Some(bytes),
                Ok(count) => {
                    bytes.extend_from_slice(&buffer[..count]);
                    if bytes.len() > 65536 {
                        return None;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return None,
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[cfg(not(unix))]
    {
        let mut bytes = Vec::new();
        stdout.by_ref().take(65537).read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn login_path_is_bounded_cached_and_never_interpolates_executable_names() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let shell = root.path().join("shell");
        let counter = root.path().join("count");
        std::fs::write(
            &shell,
            format!(
                "#!/bin/sh\necho called >> '{}'\nprintf '/extra/bin:/another/bin'\n",
                counter.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o700)).unwrap();
        let home = std::ffi::OsString::from("fixture");
        assert_eq!(
            login_paths(&shell, home.clone()),
            vec![PathBuf::from("/extra/bin"), PathBuf::from("/another/bin")]
        );
        assert_eq!(login_paths(&shell, home).len(), 2);
        assert_eq!(std::fs::read_to_string(counter).unwrap().lines().count(), 1);
        let slow = root.path().join("slow");
        std::fs::write(&slow, "#!/bin/sh\nsleep 20\n").unwrap();
        std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o700)).unwrap();
        let started = Instant::now();
        assert!(query_login_paths(&slow).is_none());
        assert!(started.elapsed() < Duration::from_secs(3));
    }
    #[cfg(unix)]
    #[test]
    fn detached_profile_child_cannot_hold_the_lookup_output_open() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let shell = root.path().join("shell");
        let pid_file = root.path().join("pid");
        std::fs::write(
            &shell,
            format!(
                "#!/bin/sh\npython3 -c 'import os,time\npid=os.fork()\nif pid:\n open(\"{}\",\"w\").write(str(pid))\n print(\"/extra/bin\",flush=True)\nelse:\n os.setsid()\n time.sleep(20)'\n",
                pid_file.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o700)).unwrap();
        let started = Instant::now();
        let result = query_login_paths(&shell);
        let pid: i32 = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
        assert!(result.is_none());
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
