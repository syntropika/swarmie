use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use swarmie::RuntimeExtension;
use swarmie_workspace::WorkspaceExtension;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

#[tokio::test]
async fn explicit_asset_copy_preserves_bytes_and_refuses_overwrite_or_workspace_escape() {
    let project = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let extension = WorkspaceExtension::new().with_storage_root(state.path());
    let opened = extension
        .invoke("project.open", json!({"path":project.path()}))
        .await
        .unwrap();
    let bytes = [0, 255, 128, 13, 10, 0, 42];
    let args =
        json!({"projectId":opened["id"], "path":"saved-copy.bin", "data":STANDARD.encode(bytes)});
    let copy = extension
        .invoke("assets.export", args.clone())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(copy["path"].as_str().unwrap()).unwrap(),
        bytes
    );
    assert!(extension.invoke("assets.export", args).await.is_err());
    for path in ["../outside.bin", "/tmp/outside.bin"] {
        assert!(
            extension
                .invoke(
                    "assets.export",
                    json!({"projectId":opened["id"], "path":path,"data":STANDARD.encode(bytes)})
                )
                .await
                .is_err()
        );
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.path(), project.path().join("outside")).unwrap();
        assert!(extension.invoke("assets.export", json!({"projectId":opened["id"], "path":"outside/escaped.bin","data":STANDARD.encode(bytes)})).await.is_err());
        std::os::unix::fs::symlink(
            outside.path().join("untouched.bin"),
            project.path().join("link.bin"),
        )
        .unwrap();
        assert!(extension.invoke("assets.export", json!({"projectId":opened["id"], "path":"link.bin","data":STANDARD.encode(bytes)})).await.is_err());
        assert!(!outside.path().join("untouched.bin").exists());
    }
}
fn git(cwd: &Path, args: &[&str]) {
    let result = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
async fn request(url: &str, path: &str) -> Vec<u8> {
    let address = url.strip_prefix("http://").unwrap();
    let mut socket = TcpStream::connect(address).await.unwrap();
    socket
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut bytes = vec![];
    tokio::time::timeout(Duration::from_secs(4), socket.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    bytes
}
#[tokio::test]
async fn projects_create_clone_scratch_and_bounded_search_are_real() {
    let folders = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let extension = WorkspaceExtension::new().with_storage_root(state.path());
    let project = extension
        .invoke(
            "project.create",
            json!({"parent":folders.path(),"name":"new project","initializeGit":true}),
        )
        .await
        .unwrap();
    let path = Path::new(project["path"].as_str().unwrap());
    assert!(path.join(".git").exists());
    git(path, &["config", "user.email", "fixture@example.test"]);
    git(path, &["config", "user.name", "Fixture"]);
    std::fs::write(path.join("hello.txt"), "source").unwrap();
    git(path, &["add", "."]);
    git(path, &["commit", "-m", "Initialize fixture"]);
    let clone = extension
        .invoke(
            "project.clone",
            json!({"parent":folders.path(),"name":"clone","url":path}),
        )
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(Path::new(clone["path"].as_str().unwrap()).join("hello.txt"))
            .unwrap(),
        "source"
    );
    assert!(
        extension
            .invoke(
                "project.create",
                json!({"parent":folders.path(),"name":"new project"})
            )
            .await
            .is_err()
    );
    assert!(
        extension
            .invoke(
                "project.create",
                json!({"parent":folders.path(),"name":"../escape"})
            )
            .await
            .is_err()
    );
    let scratch = extension
        .invoke("project.scratch", json!({}))
        .await
        .unwrap();
    assert_eq!(scratch["scratch"], true);
    assert!(
        Path::new(scratch["path"].as_str().unwrap())
            .starts_with(std::fs::canonicalize(state.path()).unwrap())
    );
    std::fs::create_dir(path.join("node_modules")).unwrap();
    std::fs::write(path.join("node_modules/hidden.txt"), "ignored").unwrap();
    let found = extension
        .invoke(
            "files.find",
            json!({"projectId":project["id"],"query":"txt","limit":20}),
        )
        .await
        .unwrap();
    assert_eq!(found["entries"].as_array().unwrap().len(), 1);
    assert_eq!(found["entries"][0]["path"], "hello.txt");
    assert_eq!(found["paths"], json!(["hello.txt"]));
}
#[tokio::test]
async fn binary_assets_are_private_bounded_and_rehydrate_after_extension_restart() {
    let folder = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let extension = WorkspaceExtension::new().with_storage_root(state.path());
    let project = extension
        .invoke("project.open", json!({"path":folder.path()}))
        .await
        .unwrap();
    let bytes = vec![7u8; 8 * 1024 * 1024];
    let asset = extension
        .invoke(
            "assets.write",
            json!({"projectId":project["id"],"name":"binary.bin","data":STANDARD.encode(&bytes)}),
        )
        .await
        .unwrap();
    assert_eq!(asset["size"], 8 * 1024 * 1024);
    assert!(!folder.path().join("binary.bin").exists());
    assert!(Path::new(asset["path"].as_str().unwrap()).starts_with(state.path()));
    let restarted = WorkspaceExtension::new().with_storage_root(state.path());
    let restored = restarted
        .invoke("assets.read", json!({"assetId":asset["assetId"]}))
        .await
        .unwrap();
    assert_eq!(
        STANDARD.decode(restored["data"].as_str().unwrap()).unwrap(),
        bytes
    );
    assert!(
        extension
            .invoke(
                "assets.write",
                json!({"projectId":project["id"],"name":"../bad","data":"YQ=="})
            )
            .await
            .is_err()
    );
    assert!(extension.invoke("assets.write",json!({"projectId":project["id"],"name":"large.bin","data":STANDARD.encode(vec![0u8;8*1024*1024+1])})).await.is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(asset["path"].as_str().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
#[tokio::test]
async fn preview_serves_actual_content_rejects_private_paths_and_stops() {
    let folder = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("index.html"), "<h1>Actual preview</h1>").unwrap();
    std::fs::write(folder.path().join(".private"), "secret").unwrap();
    let extension = WorkspaceExtension::new().with_storage_root(state.path());
    let project = extension
        .invoke("project.open", json!({"path":folder.path()}))
        .await
        .unwrap();
    let preview = extension
        .invoke("preview.start", json!({"projectId":project["id"]}))
        .await
        .unwrap();
    let url = preview["url"].as_str().unwrap();
    assert!(
        String::from_utf8(request(url, "/").await)
            .unwrap()
            .contains("Actual preview")
    );
    assert!(
        String::from_utf8(request(url, "/.private").await)
            .unwrap()
            .starts_with("HTTP/1.1 403")
    );
    assert!(
        String::from_utf8(request(url, "/%2e%2e/secret").await)
            .unwrap()
            .starts_with("HTTP/1.1 403")
    );
    extension
        .invoke("preview.stop", json!({"previewId":preview["previewId"]}))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(
        TcpStream::connect(url.strip_prefix("http://").unwrap())
            .await
            .is_err()
    );
}
#[tokio::test]
async fn native_adb_peer_capture_and_live_png_stream_use_actual_binary_wire() {
    let state = tempfile::tempdir().unwrap();
    let executable = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/device.py");
    let extension = WorkspaceExtension::new()
        .with_storage_root(state.path())
        .with_integration_executable("adb", executable)
        .with_integration_executable("emulator", state.path().join("missing"));
    let devices = extension.invoke("devices.list", json!({})).await.unwrap();
    assert_eq!(devices["devices"][0]["id"], "emulator-5554");
    let parameters = json!({"platform":"android","id":"emulator-5554"});
    let asset = extension
        .invoke("devices.capture", parameters.clone())
        .await
        .unwrap();
    assert_eq!(asset["mime"], "image/png");
    assert_eq!(asset["width"], 1);
    extension
        .invoke("devices.open", parameters.clone())
        .await
        .unwrap();
    let stream = extension
        .invoke("devices.stream", parameters)
        .await
        .unwrap();
    let address = stream["url"]
        .as_str()
        .unwrap()
        .strip_prefix("http://")
        .unwrap();
    let mut socket = TcpStream::connect(address).await.unwrap();
    socket.write_all(b"GET / HTTP/1.1\r\n").await.unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    socket
        .write_all(format!("Host: {address}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut bytes = vec![];
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let mut buffer = [0u8; 4096];
            let n = socket.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buffer[..n]);
            if bytes.windows(8).any(|b| b == b"\x89PNG\r\n\x1a\n") {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert!(bytes.starts_with(b"HTTP/1.1 200"));
    let frame = extension
        .invoke(
            "devices.stream.frame",
            json!({"streamId":stream["streamId"]}),
        )
        .await
        .unwrap();
    assert_eq!(frame["mime"], "image/png");
    let frame_bytes = STANDARD.decode(frame["data"].as_str().unwrap()).unwrap();
    let captured = extension
        .invoke("assets.read", json!({"assetId":asset["assetId"]}))
        .await
        .unwrap();
    assert_eq!(
        frame_bytes,
        STANDARD.decode(captured["data"].as_str().unwrap()).unwrap()
    );
    extension
        .invoke(
            "devices.stream.stop",
            json!({"streamId":stream["streamId"]}),
        )
        .await
        .unwrap();
    assert!(
        extension
            .invoke(
                "devices.stream.frame",
                json!({"streamId":stream["streamId"]})
            )
            .await
            .is_err()
    );
    if !cfg!(target_os = "macos") {
        assert!(matches!(
            extension
                .invoke(
                    "devices.capture",
                    json!({"platform":"ios","id":"not-a-device"})
                )
                .await,
            Err(swarmie::Error::Unsupported(_))
        ));
    }
    let capabilities: Value = extension
        .invoke("platform.capabilities", json!({}))
        .await
        .unwrap();
    assert_eq!(capabilities["os"], std::env::consts::OS);
}

#[tokio::test]
async fn publish_uses_explicit_private_repository_and_structured_arguments_without_live_network() {
    let folder = tempfile::tempdir().unwrap();
    let peer = tempfile::tempdir().unwrap();
    let executable = peer.path().join("project_gh.py");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project_gh.py"),
        &executable,
    )
    .unwrap();
    let extension = WorkspaceExtension::new()
        .with_storage_root(peer.path())
        .with_integration_executable("gh", &executable);
    let project = extension
        .invoke("project.open", json!({"path":folder.path()}))
        .await
        .unwrap();
    let published=extension.invoke("project.publish",json!({"projectId":project["id"],"name":"fixture/project","description":"Literal $(text)"})).await.unwrap();
    assert_eq!(published["published"], true);
    let args: Value = serde_json::from_str(
        std::fs::read_to_string(executable.with_extension("jsonl"))
            .unwrap()
            .trim(),
    )
    .unwrap();
    assert!(args.as_array().unwrap().contains(&json!("--private")));
    assert!(args.as_array().unwrap().contains(&json!("--push")));
    assert!(args.as_array().unwrap().contains(&json!("Literal $(text)")));
    assert!(!args.as_array().unwrap().contains(&json!("merge")));
    assert!(
        extension
            .invoke(
                "project.publish",
                json!({"projectId":project["id"],"name":"--public"})
            )
            .await
            .is_err()
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn selected_x11_window_capture_and_accessibility_use_exact_native_identifiers() {
    let state = tempfile::tempdir().unwrap();
    let peer = state.path().join("window.py");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/window.py"),
        &peer,
    )
    .unwrap();
    let mut extension = WorkspaceExtension::new().with_storage_root(state.path());
    for name in ["wmctrl", "xprop", "import", "python3"] {
        let executable = state.path().join(name);
        std::os::unix::fs::symlink(&peer, &executable).unwrap();
        extension = extension.with_integration_executable(name, executable);
    }
    let windows = extension.invoke("windows.list", json!({})).await.unwrap();
    assert_eq!(windows["windows"][0]["id"], "0x01234567");
    assert_eq!(windows["windows"][0]["title"], "Selected fixture window");
    let parameters = json!({"windowId":"0x01234567"});
    let capture = extension
        .invoke("windows.capture", parameters.clone())
        .await
        .unwrap();
    assert_eq!(capture["mime"], "image/png");
    assert_eq!(capture["width"], 1);
    let context = extension
        .invoke("windows.context", parameters)
        .await
        .unwrap();
    assert!(
        context["accessibility"]
            .as_str()
            .unwrap()
            .contains("Selected fixture element")
    );
    assert!(
        extension
            .invoke("windows.capture", json!({"windowId":"-root"}))
            .await
            .is_err()
    );
    let ledger = std::fs::read_to_string(peer.with_extension("jsonl")).unwrap();
    assert!(ledger.contains("4242"));
    assert!(!ledger.contains("-root"));
}
