#![cfg(unix)]
use std::{
    os::unix::fs::PermissionsExt,
    process::Command,
    time::{Duration, Instant},
};

#[test]
fn native_open_hands_encoded_project_and_session_to_the_host() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("project with spaces & query");
    std::fs::create_dir(&project).unwrap();
    let host = directory.path().join("native-host");
    let receipt = directory.path().join("receipt");
    std::fs::write(&host, "#!/bin/sh\nprintf '%s\\n' \"$SWARMIE_DESKTOP_URL\" \"$SWARMIE_DESKTOP_MANAGED\" > \"$SWARMIE_OPEN_RECEIPT\"\n").unwrap();
    std::fs::set_permissions(&host, std::fs::Permissions::from_mode(0o700)).unwrap();
    let session = "12345678-1234-1234-1234-123456789abc";
    let output = Command::new(env!("CARGO_BIN_EXE_swarmie"))
        .args([
            "open",
            "--url",
            "http://127.0.0.1:7480/?existing=value&project=old&session=old",
            "--host",
        ])
        .arg(&host)
        .arg("--project")
        .arg(&project)
        .args(["--session", session])
        .env("SWARMIE_OPEN_RECEIPT", &receipt)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let deadline = Instant::now() + Duration::from_secs(3);
    while !receipt.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let content = std::fs::read_to_string(receipt).unwrap();
    let mut lines = content.lines();
    let url = reqwest::Url::parse(lines.next().unwrap()).unwrap();
    let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
    assert_eq!(
        url.query_pairs()
            .filter(|(key, _)| key == "project")
            .count(),
        1
    );
    assert_eq!(
        url.query_pairs()
            .filter(|(key, _)| key == "session")
            .count(),
        1
    );
    assert_eq!(query.get("project").unwrap(), &project.to_string_lossy());
    assert_eq!(query.get("session").unwrap(), session);
    assert_eq!(query.get("existing").unwrap(), "value");
    assert_eq!(lines.next(), Some("1"));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        url.as_str()
    );
}

#[test]
fn explicit_missing_host_fails_but_print_never_launches() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing-host");
    let failed = Command::new(env!("CARGO_BIN_EXE_swarmie"))
        .args(["open", "--host"])
        .arg(&missing)
        .output()
        .unwrap();
    assert!(!failed.status.success());
    let printed = Command::new(env!("CARGO_BIN_EXE_swarmie"))
        .args(["open", "--host"])
        .arg(&missing)
        .arg("--print")
        .output()
        .unwrap();
    assert!(printed.status.success());
    assert_eq!(
        String::from_utf8(printed.stdout).unwrap().trim(),
        "http://127.0.0.1:7480/"
    );
}

#[test]
fn explicit_native_host_rejects_remote_origin() {
    let failed = Command::new(env!("CARGO_BIN_EXE_swarmie"))
        .args([
            "open",
            "--url",
            "https://example.com",
            "--host",
            "/unused/host",
        ])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(
        String::from_utf8(failed.stderr)
            .unwrap()
            .contains("requires an HTTP URL")
    );
}
