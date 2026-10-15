//! Private pinned provider packages and SDK support, independent of a source checkout.
use super::*;

const AUTH_FILES: &[(&str, &str)] = &[
    (
        "package.json",
        include_str!("../../../../../packages/agent-auth-worker/package.json"),
    ),
    (
        "package-lock.json",
        include_str!("../../../../../packages/agent-auth-worker/package-lock.json"),
    ),
    (
        "worker.mjs",
        include_str!("../../../../../packages/agent-auth-worker/worker.mjs"),
    ),
];
const CLAUDE_FILES: &[(&str, &str)] = &[
    (
        "package.json",
        include_str!("../../../../../packages/claude-agent-worker/package.json"),
    ),
    (
        "package-lock.json",
        include_str!("../../../../../packages/claude-agent-worker/package-lock.json"),
    ),
    (
        "worker.mjs",
        include_str!("../../../../../packages/claude-agent-worker/worker.mjs"),
    ),
    (
        "session.mjs",
        include_str!("../../../../../packages/claude-agent-worker/session.mjs"),
    ),
    (
        "tasks.mjs",
        include_str!("../../../../../packages/claude-agent-worker/tasks.mjs"),
    ),
    (
        "input-queue.mjs",
        include_str!("../../../../../packages/claude-agent-worker/input-queue.mjs"),
    ),
    (
        "diagnostics.mjs",
        include_str!("../../../../../packages/claude-agent-worker/diagnostics.mjs"),
    ),
    (
        "transport.mjs",
        include_str!("../../../../../packages/claude-agent-worker/transport.mjs"),
    ),
];
const NPM_RUNNER: &str = r#"import {spawn} from 'node:child_process';
import {writeFileSync} from 'node:fs';
const [npm, receipt, ...args] = process.argv.slice(2);
const child = spawn(npm, args, {stdio:'ignore', shell:false});
child.once('error', () => { writeFileSync(receipt, 'failed', {mode:0o600}); process.exitCode=1; });
child.once('exit', code => { writeFileSync(receipt, code === 0 ? 'completed' : 'failed', {mode:0o600}); process.exitCode=code ?? 1; });
"#;

pub(super) fn cursor_release() -> Option<(&'static str, &'static str)> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some((
            "darwin-arm64",
            "2d0f69ba7086d32bce1173740a6b89ec5d5cbe377f3a4072a119a4e94bfcb71e",
        )),
        ("macos", "x86_64") => Some((
            "darwin-x64",
            "731e2b521eb80316aba37e1d057f4daefebbcbd8ffb173dcb54409210be749ca",
        )),
        ("linux", "aarch64") => Some((
            "linux-arm64",
            "d12acaa7fc56c4007802c52b7ad1f7b68904faddd147c716647a3cd620dd841d",
        )),
        ("linux", "x86_64") => Some((
            "linux-x64",
            "6076187d12b9e37dc68ad9736bb08424a78cde5acb3c2cd1fa706fa43a20ed5a",
        )),
        // npm's Windows command shim is not a directly executable child process.
        // Advertise managed SDK support only once its npm launcher is supported.
        _ => None,
    }
}
fn pinned_package(harness: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match harness {
        "codex" => Some(("@openai/codex", "0.162.1", "codex")),
        "claude" => Some(("@anthropic-ai/claude-code", "2.1.300", "claude")),
        "opencode" => Some(("opencode-ai", "1.19.0", "opencode")),
        "pi" => Some(("@earendil-works/pi-coding-agent", "1.2.0", "pi")),
        _ => None,
    }
}
async fn prerequisites() -> Result<(PathBuf, PathBuf)> {
    let requested_node = std::env::var("SWARMIE_NODE_EXECUTABLE").unwrap_or_else(|_| "node".into());
    let node = process::find_executable(&requested_node).ok_or_else(|| {
        Error::Provider(
            "Install Node.js 22.19 or newer on this runtime node before installing this agent tool"
                .into(),
        )
    })?;
    let requested_npm = std::env::var("SWARMIE_NPM_EXECUTABLE").unwrap_or_else(|_| "npm".into());
    let npm = process::find_executable(&requested_npm).ok_or_else(|| {
        Error::Provider("Install npm on this runtime node before installing this agent tool".into())
    })?;
    let output = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::process::Command::new(&node)
            .arg("--version")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| Error::Timeout("Checking Node.js".into()))??;
    let version = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<_> = version.trim().trim_start_matches('v').split('.').collect();
    let major = parts.first().and_then(|value| value.parse::<u32>().ok());
    let minor = parts.get(1).and_then(|value| value.parse::<u32>().ok());
    if !output.status.success()
        || !matches!((major,minor), (Some(major),Some(minor)) if major > 22 || major == 22 && minor >= 19)
    {
        return Err(Error::Provider(
            "Managed agent tool installation requires Node.js 22.19 or newer".into(),
        ));
    }
    Ok((node, npm))
}
async fn npm_command(
    node: &Path,
    npm: &Path,
    directory: &Path,
    args: &[String],
    cancel: &AtomicBool,
) -> Result<()> {
    cancelled(cancel)?;
    let script = directory.join("swarmie-install.mjs");
    let receipt = directory.join("swarmie-install-result");
    std::fs::write(&script, NPM_RUNNER)?;
    std::fs::write(directory.join(".npmrc"), "")?;
    let mut command = vec![
        script.to_string_lossy().into_owned(),
        npm.to_string_lossy().into_owned(),
        receipt.to_string_lossy().into_owned(),
    ];
    command.extend_from_slice(args);
    command.extend([
        "--global=false".into(),
        "--prefix".into(),
        directory.to_string_lossy().into_owned(),
    ]);
    // A private cache avoids modifying the user's npm cache or native installation.
    let environment = vec![
        (
            "npm_config_cache".into(),
            directory.join(".npm-cache").to_string_lossy().into_owned(),
        ),
        ("npm_config_update_notifier".into(), "false".into()),
        (
            "npm_config_userconfig".into(),
            directory.join(".npmrc").to_string_lossy().into_owned(),
        ),
        (
            "npm_config_registry".into(),
            "https://registry.npmjs.org/".into(),
        ),
    ];
    let child = process::ManagedProcess::spawn(node, &command, directory, &environment)
        .await
        .map_err(|_| Error::Provider("Unable to start the private npm installer".into()))?;
    let start = tokio::time::Instant::now();
    while !child.exited() {
        if cancel.load(Ordering::Acquire) || start.elapsed() > Duration::from_secs(600) {
            let _ = child.kill().await;
            cancelled(cancel)?;
            return Err(Error::Timeout("Installing provider dependencies".into()));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    cancelled(cancel)?;
    let success = std::fs::read(&receipt).ok().as_deref() == Some(b"completed");
    let _ = std::fs::remove_file(&receipt);
    let _ = std::fs::remove_file(&script);
    let _ = std::fs::remove_dir_all(directory.join(".npm-cache"));
    if !success {
        return Err(Error::Provider("Provider package installation failed; check the runtime node's network connection and available storage".into()));
    }
    Ok(())
}
fn write_support(directory: &Path, files: &[(&str, &str)]) -> Result<()> {
    private_directory(directory)?;
    for (name, contents) in files {
        std::fs::write(directory.join(name), contents)?;
    }
    Ok(())
}
async fn install_support(
    node: &Path,
    npm: &Path,
    directory: &Path,
    files: &[(&str, &str)],
    cancel: &AtomicBool,
) -> Result<()> {
    write_support(directory, files)?;
    npm_command(
        node,
        npm,
        directory,
        &[
            "ci".into(),
            "--omit=dev".into(),
            "--omit=optional".into(),
            "--ignore-scripts".into(),
            "--no-audit".into(),
            "--no-fund".into(),
        ],
        cancel,
    )
    .await
}
fn relative_executable(directory: &Path, candidate: &Path) -> Result<PathBuf> {
    let absolute = std::fs::canonicalize(candidate).map_err(|_| {
        Error::Provider("The installed provider package did not contain its executable".into())
    })?;
    let base = std::fs::canonicalize(directory)?;
    let relative = absolute
        .strip_prefix(base)
        .map_err(|_| invalid())?
        .to_path_buf();
    if !absolute.is_file() || std::fs::metadata(&absolute)?.len() == 0 {
        return Err(invalid());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&absolute, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(relative)
}
fn extract_cursor(archive: &Path, destination: &Path, cancel: &AtomicBool) -> Result<()> {
    use std::io::{Read, Write};
    let mut archive =
        tar::Archive::new(flate2::read::GzDecoder::new(std::fs::File::open(archive)?));
    let mut total = 0u64;
    let mut paths = std::collections::BTreeSet::new();
    for (index, entry) in archive.entries()?.enumerate() {
        cancelled(cancel)?;
        if index > 1024 {
            return Err(invalid());
        }
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        if path.is_absolute()
            || path
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
            || path.to_string_lossy().contains(['\\', ':'])
            || !paths.insert(path.clone())
        {
            return Err(invalid());
        }
        total = total.checked_add(entry.size()).ok_or_else(invalid)?;
        if total > MAX_EXECUTABLE {
            return Err(invalid());
        }
        let target = destination.join(path);
        if entry.header().entry_type().is_dir() {
            private_directory(&target)?;
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err(invalid());
        }
        private_directory(target.parent().ok_or_else(invalid)?)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(if entry.header().mode()? & 0o111 != 0 {
                0o700
            } else {
                0o600
            });
        }
        let mut output = options.open(target)?;
        let mut buffer = [0u8; 65536];
        loop {
            cancelled(cancel)?;
            let count = entry.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            output.write_all(&buffer[..count])?;
        }
        output.sync_all()?;
    }
    Ok(())
}
async fn grok_download(
    client: &reqwest::Client,
    staging: &Path,
    cancel: &AtomicBool,
    progress: &mpsc::Sender<Value>,
) -> Result<(String, PathBuf)> {
    let response = client
        .get("https://storage.googleapis.com/grok-build-public-artifacts/cli/stable")
        .send()
        .await
        .map_err(|_| Error::Provider("Unable to fetch the official Grok release".into()))?
        .error_for_status()
        .map_err(|_| invalid())?;
    let mut stream = response.bytes_stream();
    let mut bytes = vec![];
    while let Some(chunk) = stream.next().await {
        cancelled(cancel)?;
        let chunk = chunk.map_err(|_| invalid())?;
        if bytes.len() + chunk.len() > 128 {
            return Err(invalid());
        }
        bytes.extend_from_slice(&chunk);
    }
    let version = String::from_utf8(bytes)
        .map_err(|_| invalid())?
        .trim()
        .to_string();
    if !valid_version(&version) {
        return Err(invalid());
    }
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        _ => "linux",
    };
    let arch = if std::env::consts::ARCH == "aarch64" {
        "aarch64"
    } else {
        "x86_64"
    };
    let extension = if cfg!(windows) { ".exe" } else { "" };
    let file = format!("grok-{version}-{os}-{arch}{extension}");
    let path = staging.join(if cfg!(windows) { "grok.exe" } else { "grok" });
    // The official distributor serves authenticated HTTPS artifacts without a published digest.
    let url = reqwest::Url::parse(&format!(
        "https://storage.googleapis.com/grok-build-public-artifacts/cli/{file}"
    ))
    .map_err(|_| invalid())?;
    bounded_download(client, url, &path, cancel, progress, "").await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
    }
    verify_native_version(&path, staging, &version).await?;
    Ok((version, path))
}
async fn verify_native_version(executable: &Path, cwd: &Path, expected: &str) -> Result<()> {
    use tokio::io::AsyncReadExt;
    let (child, stdin, stdout, stderr) =
        process::ManagedProcess::spawn_with_pipes(executable, &["--version".into()], cwd, &[])
            .await
            .map_err(|_| {
                Error::Provider(
                    "The installed provider binary cannot run on this runtime node".into(),
                )
            })?;
    drop(stdin);
    let operation = async {
        let mut output = Vec::new();
        let mut errors = Vec::new();
        let mut stdout = stdout.take(4097);
        let mut stderr = stderr.take(4097);
        tokio::try_join!(
            stdout.read_to_end(&mut output),
            stderr.read_to_end(&mut errors)
        )?;
        if output.len() > 4096 || errors.len() > 4096 {
            return Err(invalid());
        }
        output.extend(errors);
        let text = String::from_utf8_lossy(&output);
        if !text
            .split_whitespace()
            .any(|part| part.trim_start_matches('v') == expected.trim_start_matches('v'))
        {
            return Err(invalid());
        }
        // A printed version is insufficient: require the supervised child to
        // exit naturally and successfully before publishing its managed release.
        while !child.exited() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if !child.exit_status().is_some_and(|status| status.success()) {
            return Err(Error::Provider(
                "The installed provider binary failed its startup check".into(),
            ));
        }
        Ok(())
    };
    let result = tokio::time::timeout(Duration::from_secs(10), operation)
        .await
        .map_err(|_| Error::Timeout("Verifying the native provider version".into()))?;
    let _ = child.kill().await;
    result
}
pub(super) async fn install(
    root: &Path,
    harness: &str,
    cancel: Arc<AtomicBool>,
    progress: mpsc::Sender<Value>,
) -> Result<PathBuf> {
    let cancellation = async {
        while !cancel.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    tokio::select! { biased; result = install_inner(root,harness,cancel.clone(),progress) => result, _ = cancellation => Err(Error::Provider("Installation cancelled".into())) }
}
async fn install_inner(
    root: &Path,
    harness: &str,
    cancel: Arc<AtomicBool>,
    progress: mpsc::Sender<Value>,
) -> Result<PathBuf> {
    let directory = root.join("managed").join(harness);
    private_directory(&directory)?;
    let staging = directory.join(format!("staging-{}", uuid::Uuid::new_v4()));
    private_directory(&staging)?;
    let mut cleanup = InstallationFiles {
        staging: staging.clone(),
        release: None,
        metadata: None,
        activated: false,
    };
    cancelled(&cancel)?;
    let _ = progress.try_send(json!({"message":"Preparing the official provider package"}));
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|_| invalid())?;
    let mut sdk_worker = None;
    let mut auth_worker = None;
    let (version, executable) = if harness == "grok" {
        let (version, executable) = grok_download(&client, &staging, &cancel, &progress).await?;
        (version, relative_executable(&staging, &executable)?)
    } else {
        let (node, npm) = prerequisites().await?;
        if harness == "cursor" {
            let (platform, hash) = cursor_release().ok_or_else(invalid)?;
            let archive = staging.join("bridge.tar.gz");
            let url = reqwest::Url::parse(&format!("https://github.com/cursor/sdk-bridge/releases/download/v1.0.37/cursor-sdk-bridge-standalone-{platform}.tar.gz")).map_err(|_| invalid())?;
            // GitHub's artifact redirect is resolved only to its official asset host.
            let response = client
                .get(url.clone())
                .send()
                .await
                .map_err(|_| invalid())?;
            let download = if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(invalid)?;
                let location = reqwest::Url::parse(location).map_err(|_| invalid())?;
                if location.scheme() != "https"
                    || location.host_str() != Some("release-assets.githubusercontent.com")
                    || !location.username().is_empty()
                    || location.password().is_some()
                    || location.port().is_some()
                {
                    return Err(invalid());
                }
                location
            } else {
                url
            };
            bounded_download(&client, download, &archive, &cancel, &progress, hash).await?;
            extract_cursor(&archive, &staging.join("cursor"), &cancel)?;
            std::fs::remove_file(archive)?;
            let support = staging.join("auth");
            let _ = progress
                .try_send(json!({"message":"Installing locked native authentication support"}));
            install_support(&node, &npm, &support, AUTH_FILES, &cancel).await?;
            auth_worker = Some(PathBuf::from("auth/worker.mjs"));
            let mut options = swarmie::SpawnOptions::new("cursor", staging.clone());
            options.config = json!({"executable":staging.join("cursor/bin/cursor-sdk-bridge")});
            options
                .execution
                .env
                .insert("CURSOR_API_KEY".into(), String::new());
            let readiness = crate::cursor::probe_options(&options).await.map_err(|_| {
                Error::Provider(
                    "The installed Cursor Bridge could not complete its local startup check".into(),
                )
            })?;
            if !readiness.available || readiness.version.as_deref() != Some("1.0.37") {
                return Err(Error::Provider("The official Cursor Bridge cannot run on this runtime node; check its platform and CPU requirements".into()));
            }
            (
                "1.0.37".into(),
                relative_executable(&staging, &staging.join("cursor/bin/cursor-sdk-bridge"))?,
            )
        } else {
            let (package, version, binary) = pinned_package(harness).ok_or_else(invalid)?;
            std::fs::write(staging.join("package.json"), "{\"private\":true}")?;
            let _ = progress
                .try_send(json!({"message":"Installing the pinned official provider package"}));
            npm_command(
                &node,
                &npm,
                &staging,
                &[
                    "install".into(),
                    format!(
                        "{}@{version}",
                        native_package(harness).unwrap_or_else(|| package.into())
                    ),
                    "--ignore-scripts".into(),
                    "--no-audit".into(),
                    "--no-fund".into(),
                ],
                &cancel,
            )
            .await?;
            let executable = provider_binary(&staging, harness, binary)?;
            if harness == "claude" {
                let _ = progress
                    .try_send(json!({"message":"Installing locked Claude Agent SDK support"}));
                install_support(
                    &node,
                    &npm,
                    &staging.join("claude-sdk"),
                    CLAUDE_FILES,
                    &cancel,
                )
                .await?;
                sdk_worker = Some(PathBuf::from("claude-sdk/worker.mjs"));
            }
            if harness == "pi" {
                install_support(&node, &npm, &staging.join("auth"), AUTH_FILES, &cancel).await?;
                auth_worker = Some(PathBuf::from("auth/worker.mjs"));
            }
            verify_native_version(&staging.join(&executable), &staging, version).await?;
            (version.into(), executable)
        }
    };
    cancelled(&cancel)?;
    let release = format!("{version}-{}", uuid::Uuid::new_v4());
    let executable = PathBuf::from(&release).join(executable);
    let active = Active {
        version,
        executable: executable.clone(),
        sdk_worker: sdk_worker.map(|path| PathBuf::from(&release).join(path)),
        auth_worker: auth_worker.map(|path| PathBuf::from(&release).join(path)),
    };
    cleanup.release = Some(directory.join(&release));
    std::fs::rename(&staging, directory.join(&release))?;
    let temporary = directory.join(format!("active-{}.tmp", uuid::Uuid::new_v4()));
    cleanup.metadata = Some(temporary.clone());
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    let mut output = options.open(&temporary)?;
    output.write_all(&serde_json::to_vec(&active)?)?;
    output.sync_all()?;
    drop(output);
    cancelled(&cancel)?;
    std::fs::rename(&temporary, directory.join("active.json"))?;
    cleanup.activated = true;
    Ok(directory.join(executable))
}
fn musl() -> bool {
    cfg!(target_env = "musl")
        || std::fs::read_dir("/lib").ok().is_some_and(|entries| {
            entries
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().starts_with("ld-musl-"))
        })
}
fn native_package(harness: &str) -> Option<String> {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    let arch = if std::env::consts::ARCH == "aarch64" {
        "arm64"
    } else {
        "x64"
    };
    let musl = if os == "linux" && musl() { "-musl" } else { "" };
    match harness {
        "claude" => Some(format!("@anthropic-ai/claude-code-{os}-{arch}{musl}")),
        // The baseline build is supported on modern and older x64 CPUs.
        "opencode" => Some(format!(
            "opencode-{os}-{arch}{}{musl}",
            if arch == "x64" { "-baseline" } else { "" }
        )),
        _ => None,
    }
}
fn provider_binary(staging: &Path, harness: &str, binary: &str) -> Result<PathBuf> {
    let path = if let Some(package) = native_package(harness) {
        staging
            .join("node_modules")
            .join(package)
            .join(if harness == "claude" {
                "claude"
            } else {
                "bin/opencode"
            })
    } else {
        staging.join("node_modules/.bin").join(binary)
    };
    relative_executable(staging, &path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[cfg(unix)]
    async fn packaged_claude_session_imports_without_source_checkout() {
        let root = temporary("claude-worker-import");
        write_support(&root, CLAUDE_FILES).unwrap();
        let node = process::find_executable("node").expect("Node.js is required for worker tests");
        let output = tokio::process::Command::new(node)
            .current_dir(&root)
            .args([
                "--input-type=module",
                "-e",
                "await import('./session.mjs');",
            ])
            .output()
            .await
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn temporary(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("swarmie-{label}-{}", uuid::Uuid::new_v4()));
        private_directory(&root).unwrap();
        root
    }
    #[cfg(unix)]
    fn fixture_npm(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[tokio::test]
    #[cfg(unix)]
    async fn private_package_runner_requires_success_and_cancels_owned_tree() {
        let root = temporary("npm-fixture");
        let node = root.join("node");
        fixture_npm(
            &node,
            "shift\nnpm=$1; shift\nreceipt=$1; shift\nif \"$npm\" \"$@\"; then printf completed > \"$receipt\"; else printf failed > \"$receipt\"; fi",
        );
        let npm = root.join("npm");
        fixture_npm(&npm, "exit 1");
        assert!(
            npm_command(&node, &npm, &root, &[], &AtomicBool::new(false))
                .await
                .is_err()
        );
        fixture_npm(&npm, "exit 0");
        npm_command(&node, &npm, &root, &[], &AtomicBool::new(false))
            .await
            .unwrap();
        fixture_npm(&npm, "sleep 30 &\nwait");
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            signal.store(true, Ordering::Release);
        });
        let result = tokio::time::timeout(
            Duration::from_secs(4),
            npm_command(&node, &npm, &root, &[], &cancel),
        )
        .await
        .unwrap();
        assert!(result.is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    #[cfg(unix)]
    async fn native_version_check_requires_expected_release_and_success() {
        let root = temporary("version-fixture");
        let executable = root.join("tool");
        fixture_npm(&executable, "echo 'grok 1.2.3'");
        verify_native_version(&executable, &root, "1.2.3")
            .await
            .unwrap();
        assert!(
            verify_native_version(&executable, &root, "1.2.4")
                .await
                .is_err()
        );
        fixture_npm(&executable, "echo 'grok 1.2.3'\nsleep 0.1\nexit 1");
        assert!(
            verify_native_version(&executable, &root, "1.2.3")
                .await
                .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    #[ignore = "explicit network staging smoke; writes only to the selected external artifact directory"]
    async fn pinned_support_imports_without_source_checkout() {
        let root = PathBuf::from(
            std::env::var("SWARMIE_TEST_MANAGED_INSTALL_ROOT")
                .expect("set external staging directory"),
        );
        assert!(!root.exists());
        private_directory(&root).unwrap();
        let (node, npm) = prerequisites().await.unwrap();
        let cancel = AtomicBool::new(false);
        for (name, files, module) in [
            ("auth", AUTH_FILES, "@cursor/sdk"),
            ("claude", CLAUDE_FILES, "@anthropic-ai/claude-agent-sdk"),
        ] {
            let directory = root.join(name);
            install_support(&node, &npm, &directory, files, &cancel)
                .await
                .unwrap();
            let result = tokio::process::Command::new(&node)
                .current_dir(&directory)
                .args([
                    "--input-type=module",
                    "-e",
                    &format!(
                        "await import('{module}');{}",
                        if name == "auth" {
                            "await import('@earendil-works/pi-ai/oauth');"
                        } else {
                            ""
                        }
                    ),
                ])
                .output()
                .await
                .unwrap();
            assert!(result.status.success(), "pinned SDK import failed");
        }
    }
}
