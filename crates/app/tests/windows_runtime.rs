#![cfg(windows)]
//! Native qualification of private local attachment, retained identity and startup policy.
use serde_json::Value;
use std::{path::PathBuf, time::Duration};
use tokio::process::Command;
fn runtime_binary() -> PathBuf {
    std::env::var_os("SWARMIE_TEST_RUNTIME_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_swarmie")))
}
struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    config: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        Self {
            root: temp.path().join("state with spaces é"),
            config: temp.path().join("local app data"),
            _temp: temp,
        }
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(runtime_binary());
        command
            .arg("--state-dir")
            .arg(&self.root)
            .args(args)
            .env("LOCALAPPDATA", &self.config)
            .env_remove("SWARMIE_TOKEN");
        command
    }
    async fn call(&self, args: &[&str]) -> Value {
        let output = tokio::time::timeout(Duration::from_secs(40), self.command(args).output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::process::Command::new(runtime_binary())
            .arg("--state-dir")
            .arg(&self.root)
            .args(["runtime", "stop"])
            .output();
        // Each temporary configuration has a distinct scheduled task name.
        let _ = std::process::Command::new(runtime_binary())
            .arg("--state-dir")
            .arg(&self.root)
            .args(["service", "uninstall"])
            .env("LOCALAPPDATA", &self.config)
            .output();
    }
}
#[tokio::test]
async fn detached_owner_does_not_inherit_unrelated_controller_handles() {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
    let fixture = Fixture::new();
    let path = fixture._temp.path().join("controller-only-file");
    let file = std::fs::OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .share_mode(3)
        .open(&path)
        .unwrap();
    assert_ne!(
        unsafe {
            SetHandleInformation(
                file.as_raw_handle(),
                HANDLE_FLAG_INHERIT,
                HANDLE_FLAG_INHERIT,
            )
        },
        0
    );
    fixture.call(&["runtime", "start"]).await;
    drop(file);
    std::fs::remove_file(path).expect("the owner must not retain unrelated inherited handles");
    assert_eq!(fixture.call(&["runtime", "status"]).await["running"], true);
}
#[tokio::test]
async fn detached_start_closes_captured_stdio_and_preserves_the_live_owner() {
    let fixture = Fixture::new();
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        fixture
            .command(&["runtime", "start"])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("the launcher must close captured streams independently of its detached owner")
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let started: Value = serde_json::from_slice(&output.stdout).unwrap();
    let observed = fixture.call(&["runtime", "status"]).await;
    assert_eq!(observed["running"], true);
    assert_eq!(
        observed["endpoint"]["runtimeIdentity"],
        started["runtimeIdentity"]
    );
}
#[tokio::test]
async fn private_pipe_attach_preserves_identity_and_requires_bearer_over_http() {
    let fixture = Fixture::new();
    let started = fixture.call(&["runtime", "start"]).await;
    let before = fixture.call(&["runtime", "status"]).await;
    assert_eq!(before["running"], true);
    assert!(
        before["endpoint"]["localSocket"]
            .as_str()
            .unwrap()
            .starts_with(r"\\.\pipe\swarmie-")
    );
    let reattached = fixture.call(&["runtime", "start"]).await;
    assert_eq!(started["runtimeIdentity"], reattached["runtimeIdentity"]);
    let response = reqwest::Client::new()
        .post(format!(
            "{}/rpc",
            before["endpoint"]["url"].as_str().unwrap()
        ))
        .json(
            &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"runtime/initialize","params":{}}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    let stopped = fixture.call(&["runtime", "stop"]).await;
    assert_eq!(stopped["stopped"], true);
    let restarted = fixture.call(&["runtime", "start"]).await;
    assert_eq!(
        started["runtimeIdentity"]["identity"],
        restarted["runtimeIdentity"]["identity"]
    );
    assert_ne!(
        started["runtimeIdentity"]["incarnationId"],
        restarted["runtimeIdentity"]["incarnationId"]
    );
}
#[tokio::test]
async fn logon_startup_changes_future_policy_without_interrupting_the_owner() {
    let fixture = Fixture::new();
    let before = fixture.call(&["runtime", "start"]).await;
    let enabled = fixture.call(&["service", "startup", "enable"]).await;
    assert_eq!(enabled["enabled"], true);
    assert_eq!(enabled["executionChanged"], false);
    assert_eq!(enabled["trigger"], "login");
    assert_eq!(enabled["bootReady"], false);
    assert_eq!(
        fixture.call(&["runtime", "start"]).await["runtimeIdentity"],
        before["runtimeIdentity"]
    );
    let disabled = fixture.call(&["service", "startup", "disable"]).await;
    assert_eq!(disabled["enabled"], false);
    let unit = fixture.config.join("swarmie/runtime-task.xml");
    let original = std::fs::read(&unit).unwrap();
    std::fs::write(&unit, "user modified task").unwrap();
    let refused = fixture
        .command(&["service", "startup", "enable"])
        .output()
        .await
        .unwrap();
    assert!(!refused.status.success());
    assert_eq!(
        std::fs::read_to_string(&unit).unwrap(),
        "user modified task"
    );
    std::fs::write(&unit, original).unwrap();
}
