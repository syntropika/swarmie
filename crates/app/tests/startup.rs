#![cfg(target_os = "linux")]
//! Startup configuration preserves the current owner and only changes future service policy.
use serde_json::Value;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};
use tokio::process::Command;

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    tools: PathBuf,
    ledger: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let tools = temp.path().join("tools");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&tools).unwrap();
        for (name, script) in [
            (
                "systemctl",
                r#"#!/bin/sh
printf '%s\n' "$*" >> "$SWARMIE_STARTUP_LEDGER"
case "$2" in
  show) printf 'fixture-version\n' ;;
  is-enabled) if test -f "$SWARMIE_STARTUP_LEDGER.enabled"; then printf 'enabled\n'; else printf 'disabled\n'; exit 1; fi ;;
  is-active) printf 'inactive\n'; exit 3 ;;
  enable) touch "$SWARMIE_STARTUP_LEDGER.enabled" ;;
  disable) rm -f "$SWARMIE_STARTUP_LEDGER.enabled" ;;
  daemon-reload) ;;
  *) exit 1 ;;
esac
"#,
            ),
            (
                "loginctl",
                r#"#!/bin/sh
printf '%s\n' "$*" >> "$SWARMIE_STARTUP_LEDGER"
if test "$1" = show-user; then
  if test -f "$SWARMIE_STARTUP_LEDGER.linger"; then printf 'yes\n'; else printf 'no\n'; fi
elif test "$1" = --no-ask-password && test "$2" = enable-linger; then
  if test -n "$SWARMIE_LINGER_REFUSE"; then exit 1; fi
  touch "$SWARMIE_STARTUP_LEDGER.linger"
else exit 1; fi
"#,
            ),
        ] {
            let path = tools.join(name);
            std::fs::write(&path, script).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let root = temp.path().join("state");
        let ledger = temp.path().join("manager.log");
        Self {
            _temp: temp,
            root,
            home,
            tools,
            ledger,
        }
    }
    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_swarmie"));
        command
            .arg("--state-dir")
            .arg(&self.root)
            .args(arguments)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.tools.display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("SWARMIE_STARTUP_LEDGER", &self.ledger)
            .env_remove("SWARMIE_TOKEN");
        command
    }
    async fn call(&self, arguments: &[&str]) -> Value {
        let output = self.command(arguments).output().await.unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn unit(&self) -> PathBuf {
        self.home.join(".config/systemd/user/swarmie.service")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::process::Command::new(env!("CARGO_BIN_EXE_swarmie"))
            .arg("--state-dir")
            .arg(&self.root)
            .args(["runtime", "stop"])
            .output();
    }
}
#[tokio::test]
async fn future_startup_and_explicit_lingering_preserve_live_owner_and_retained_identity() {
    let fixture = Fixture::new();
    let initial = fixture.call(&["service", "startup", "inspect"]).await;
    assert_eq!(initial["installed"], false);
    assert_eq!(initial["enabled"], false);
    assert!(!fixture.root.exists());
    assert!(!fixture.home.join(".config").exists());
    let live = fixture.call(&["runtime", "start"]).await;
    let before = fixture.call(&["runtime", "status"]).await;
    let enabled = fixture.call(&["service", "startup", "enable"]).await;
    assert_eq!(enabled["enabled"], true);
    assert_eq!(enabled["trigger"], "login");
    assert_eq!(enabled["executionChanged"], false);
    assert_eq!(enabled["supervisionPendingRestart"], true);
    let unit = std::fs::read_to_string(fixture.unit()).unwrap();
    assert!(unit.contains("127.0.0.1:0"));
    assert!(unit.contains("--git-worktrees"));
    assert!(unit.contains("--resources-root"));
    let boot = fixture
        .call(&["service", "startup", "enable", "--boot"])
        .await;
    assert_eq!(boot["bootReady"], true);
    assert_eq!(boot["trigger"], "boot");
    let after = fixture.call(&["runtime", "status"]).await;
    assert_eq!(before["running"], true);
    assert_eq!(after["running"], true);
    assert_eq!(
        before["endpoint"]["runtimeIdentity"],
        after["endpoint"]["runtimeIdentity"]
    );
    assert_eq!(
        live["runtimeIdentity"],
        after["endpoint"]["runtimeIdentity"]
    );
    let disabled = fixture.call(&["service", "startup", "disable"]).await;
    assert_eq!(disabled["enabled"], false);
    assert!(fixture.ledger.with_extension("log.linger").exists());
    assert_eq!(
        fixture.call(&["runtime", "status"]).await["endpoint"]["runtimeIdentity"],
        before["endpoint"]["runtimeIdentity"]
    );
    let actions = std::fs::read_to_string(&fixture.ledger).unwrap();
    assert!(
        !actions
            .lines()
            .any(|line| line.starts_with("--user start ") || line.starts_with("--user stop "))
    );
}
#[tokio::test]
async fn modified_and_foreign_units_are_preserved_without_manager_mutation() {
    let fixture = Fixture::new();
    fixture.call(&["service", "startup", "enable"]).await;
    std::fs::write(fixture.unit(), "user-owned modified unit").unwrap();
    let before = std::fs::read_to_string(&fixture.ledger).unwrap();
    let output = fixture
        .command(&["service", "startup", "enable"])
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        std::fs::read_to_string(fixture.unit()).unwrap(),
        "user-owned modified unit"
    );
    let after = std::fs::read_to_string(&fixture.ledger).unwrap();
    assert!(!after[before.len()..].contains("--user enable "));
    assert_eq!(
        fixture.call(&["service", "startup", "inspect"]).await["available"],
        false
    );
}
#[tokio::test]
async fn denied_lingering_and_replaced_node_do_not_install_or_enable_a_service() {
    let fixture = Fixture::new();
    let denied = fixture
        .command(&["service", "startup", "enable", "--boot"])
        .env("SWARMIE_LINGER_REFUSE", "1")
        .output()
        .await
        .unwrap();
    assert!(!denied.status.success());
    assert!(!fixture.unit().exists());
    let output = fixture
        .command(&[
            "service",
            "startup",
            "enable",
            "--expected-node",
            r#"{"environmentId":"foreign","storeId":"store","ownerScopeId":"owner"}"#,
        ])
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert!(!fixture.unit().exists());
}
