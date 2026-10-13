#![cfg(unix)]
//! Execute the real bootstrap through an isolated command fixture, without network authentication.
use serde_json::Value;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use tokio::process::Command;

struct Remote {
    _temp: tempfile::TempDir,
    home: PathBuf,
    client: PathBuf,
    packages: PathBuf,
    path: String,
}
impl Remote {
    async fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("remote home");
        std::fs::create_dir(&home).unwrap();
        let tools = temp.path().join("tools");
        std::fs::create_dir(&tools).unwrap();
        let ssh = tools.join("ssh");
        std::fs::write(&ssh, "#!/usr/bin/env python3\nimport os,sys\nos.environ['HOME']=os.environ['FIXTURE_REMOTE_HOME']\nos.execv('/bin/sh',['sh','-c',sys.argv[-1]])\n").unwrap();
        std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o700)).unwrap();
        let packages = temp.path().join("packages");
        let output = Command::new("python3")
            .args([
                "scripts/distribution/package-runtime.py",
                "--binary",
                env!("CARGO_BIN_EXE_swarmie"),
                "--output",
            ])
            .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .arg(temp.path().join("distribution"))
            .arg("--client-packages")
            .arg(&packages)
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let path = format!("{}:/usr/bin:/bin:/usr/local/bin", tools.display());
        let client = temp.path().join("client");
        Self {
            _temp: temp,
            home,
            client,
            packages,
            path,
        }
    }
    async fn call(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_swarmie"))
            .arg("--state-dir")
            .arg(&self.client)
            .args(args)
            .env("PATH", &self.path)
            .env("FIXTURE_REMOTE_HOME", &self.home)
            .env("SWARMIE_RUNTIME_PACKAGES", &self.packages)
            .env_remove("SWARMIE_RUNTIME_MANIFEST")
            .env_remove("SWARMIE_TOKEN")
            .output()
            .await
            .unwrap()
    }
    async fn inspect(&self) -> Value {
        let output = self.call(&["remote", "inspect-ssh", "fixture"]).await;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    async fn stop(&self) {
        let output = Command::new(self.home.join(".local/bin/swarmie"))
            .arg("--state-dir")
            .arg(self.home.join(".swarmie"))
            .args(["runtime", "stop"])
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
fn checksum_file(packages: &Path) -> PathBuf {
    std::fs::read_dir(packages)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().ends_with(".sha256"))
        .unwrap()
}
#[tokio::test]
async fn default_release_source_offers_installation_without_local_packages_or_remote_changes() {
    let remote = Remote::new().await;
    std::fs::remove_dir_all(&remote.packages).unwrap();
    let status = remote.inspect().await;
    assert_eq!(status["status"], "not-installed");
    assert_eq!(status["preparation"]["available"], true);
    assert_eq!(status["preparation"]["action"], "install");
    assert!(!remote.home.join(".local").exists());
    assert!(!remote.home.join(".swarmie").exists());
}

#[tokio::test]
async fn installs_without_remote_path_changes_verifies_connects_and_starts_without_replacing_identity()
 {
    let remote = Remote::new().await;
    let before = remote.inspect().await;
    assert_eq!(before["status"], "not-installed");
    assert_eq!(before["preparation"]["available"], true);
    assert!(!remote.home.join(".local").exists());
    assert!(!remote.home.join(".swarmie").exists());
    let output = remote
        .call(&["remote", "prepare-ssh", "fixture", "--action", "install"])
        .await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let prepared: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(prepared["ready"], true);
    let target = prepared["runtimeIdentity"].clone();
    // The owner remains live after the installing client and its SSH subprocess have exited.
    assert_eq!(remote.inspect().await["runtimeIdentity"], target);
    let output = remote.call(&["remote", "ssh", "retained", "fixture"]).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = remote
        .call(&["remote", "prepare-ssh", "fixture", "--action", "install"])
        .await;
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["runtimeIdentity"],
        target
    );
    let user_file = remote.home.join(".swarmie/user-owned.txt");
    std::fs::write(&user_file, "retained user data").unwrap();
    remote.stop().await;
    assert_eq!(remote.inspect().await["status"], "runtime-unavailable");
    let output = remote
        .call(&["remote", "prepare-ssh", "fixture", "--action", "start"])
        .await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let restarted: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(restarted["runtimeIdentity"]["identity"], target["identity"]);
    assert_ne!(
        restarted["runtimeIdentity"]["incarnationId"],
        target["incarnationId"]
    );
    assert_eq!(
        std::fs::read_to_string(user_file).unwrap(),
        "retained user data"
    );
    remote.stop().await;
}
#[tokio::test]
async fn refuses_bad_checksums_unmanaged_installations_and_start_before_install_without_creating_an_owner()
 {
    let remote = Remote::new().await;
    std::fs::write(checksum_file(&remote.packages), "00".repeat(32)).unwrap();
    let output = remote
        .call(&["remote", "prepare-ssh", "fixture", "--action", "install"])
        .await;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("SHA-256 mismatch"));
    assert!(!remote.home.join(".local").exists());
    assert!(!remote.home.join(".swarmie").exists());
    let output = remote
        .call(&["remote", "prepare-ssh", "fixture", "--action", "start"])
        .await;
    assert!(!output.status.success());
    assert!(!remote.home.join(".swarmie").exists());
    let unmanaged = Remote::new().await;
    let prefix = unmanaged.home.join(".local/share/swarmie/runtime");
    std::fs::create_dir_all(&prefix).unwrap();
    std::fs::write(prefix.join("user-owned.txt"), "preserve").unwrap();
    let output = unmanaged
        .call(&["remote", "prepare-ssh", "fixture", "--action", "install"])
        .await;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("installation refused"));
    assert_eq!(
        std::fs::read_to_string(prefix.join("user-owned.txt")).unwrap(),
        "preserve"
    );
    assert!(!unmanaged.home.join(".swarmie").exists());
}
