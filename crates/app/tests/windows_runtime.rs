#![cfg(windows)]
//! Native qualification of private local attachment, retained identity and startup policy.
use serde_json::Value;
use std::{path::PathBuf, time::Duration};
use tokio::process::Command;
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
        let mut command = Command::new(env!("CARGO_BIN_EXE_swarmie"));
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
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::process::Command::new(env!("CARGO_BIN_EXE_swarmie"))
            .arg("--state-dir")
            .arg(&self.root)
            .args(["runtime", "stop"])
            .output();
        // Each temporary configuration has a distinct scheduled task name.
        let _ = std::process::Command::new(env!("CARGO_BIN_EXE_swarmie"))
            .arg("--state-dir")
            .arg(&self.root)
            .args(["service", "uninstall"])
            .env("LOCALAPPDATA", &self.config)
            .output();
    }
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
