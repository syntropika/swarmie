#![cfg(target_os = "macos")]
use serde_json::json;
use std::time::Duration;
use swarmie::RuntimeExtension;
use swarmie_workspace::WorkspaceExtension;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

struct Shutdown(String);
impl Drop for Shutdown {
    fn drop(&mut self) {
        let _ = std::process::Command::new("xcrun")
            .args(["simctl", "shutdown", &self.0])
            .output();
    }
}

#[tokio::test]
#[ignore = "Requires an explicitly selected installed macOS simulator and opens Simulator"]
async fn native_ios_capture_and_http_stream_use_actual_simulator_pixels() {
    let id = std::env::var("SWARMIE_NATIVE_SIMULATOR_UDID")
        .expect("select an installed simulator explicitly");
    let root = tempfile::tempdir().unwrap();
    let extension = WorkspaceExtension::new().with_storage_root(root.path());
    let devices = extension.invoke("devices.list", json!({})).await.unwrap();
    let device = devices["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|device| device["id"] == id && device["platform"] == "ios")
        .expect("selected native simulator must exist");
    let _shutdown = (device["state"] != "Booted").then(|| Shutdown(id.clone()));
    extension
        .invoke("devices.open", json!({"platform":"ios","id":id}))
        .await
        .unwrap();
    let boot = tokio::process::Command::new("xcrun")
        .args(["simctl", "bootstatus", &id, "-b"])
        .kill_on_drop(true)
        .output();
    assert!(
        tokio::time::timeout(Duration::from_secs(120), boot)
            .await
            .unwrap()
            .unwrap()
            .status
            .success()
    );
    let capture = extension
        .invoke("devices.capture", json!({"platform":"ios","id":id}))
        .await
        .unwrap();
    assert_eq!(capture["mime"], "image/png");
    assert!(capture["width"].as_u64().unwrap() > 100);
    let stream = extension
        .invoke(
            "devices.stream",
            json!({"platform":"ios","id":id,"intervalMs":500}),
        )
        .await
        .unwrap();
    let address = stream["url"]
        .as_str()
        .unwrap()
        .strip_prefix("http://")
        .unwrap();
    let mut socket = TcpStream::connect(address).await.unwrap();
    socket
        .write_all(format!("GET / HTTP/1.1\r\nHost: {address}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut bytes = vec![];
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let mut buffer = [0u8; 4096];
            let n = socket.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buffer[..n]);
            if bytes.windows(8).any(|bytes| bytes == b"\x89PNG\r\n\x1a\n") {
                break;
            }
        }
    })
    .await
    .unwrap();
    extension
        .invoke(
            "devices.stream.stop",
            json!({"streamId":stream["streamId"]}),
        )
        .await
        .unwrap();
    let windows = extension.invoke("windows.list", json!({})).await.unwrap();
    assert!(windows["windows"].is_array());
}
