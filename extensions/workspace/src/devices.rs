//! Native simulator/emulator control and loopback screenshot streams.
use crate::{WorkspaceExtension, assets, command, required};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, time::Duration};
use swarmie::{Error, Result};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Mutex,
    task::{JoinHandle, JoinSet},
};
struct Stream {
    url: String,
    platform: String,
    device_id: String,
    task: JoinHandle<()>,
}
#[derive(Default)]
pub struct Streams(Mutex<BTreeMap<String, Stream>>);
impl Drop for Streams {
    fn drop(&mut self) {
        for stream in self.0.get_mut().values() {
            stream.task.abort();
        }
    }
}
fn executable(extension: &WorkspaceExtension, name: &str) -> std::path::PathBuf {
    extension
        .integrations
        .get(name)
        .cloned()
        .unwrap_or_else(|| name.into())
}
fn valid_id(p: &Value) -> Result<&str> {
    let id = required(p, "id")?;
    if id.starts_with('-') || id.contains(['\0', '\n', '\r']) || id.len() > 256 {
        return Err(Error::Invalid("invalid device identifier".into()));
    }
    Ok(id)
}
async fn capture(extension: &WorkspaceExtension, p: &Value) -> Result<Vec<u8>> {
    let id = valid_id(p)?;
    match required(p, "platform")? {
        "android" => {
            command::binary(
                &executable(extension, "adb"),
                &extension.storage_root,
                &command::args(&["-s", id, "exec-out", "screencap", "-p"]),
            )
            .await
        }
        "ios" => {
            if !cfg!(target_os = "macos") {
                return Err(Error::Unsupported(
                    "iOS simulator capture requires macOS and Xcode".into(),
                ));
            }
            let path = assets::output_path(extension, "frame.png").await?;
            let result = command::run(
                &executable(extension, "xcrun"),
                &extension.storage_root,
                &[
                    "simctl".into(),
                    "io".into(),
                    id.into(),
                    "screenshot".into(),
                    path.to_string_lossy().into(),
                ],
            )
            .await;
            if let Err(error) = result {
                let _ = tokio::fs::remove_file(&path).await;
                return Err(error);
            }
            let bytes = assets::bytes(&path).await?;
            let _ = tokio::fs::remove_dir_all(path.parent().unwrap()).await;
            if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Err(Error::Provider("native screenshot is not PNG".into()));
            }
            if bytes.len() as u64 > assets::MAX_ASSET {
                return Err(Error::Invalid("simulator screenshot exceeds 8 MiB".into()));
            }
            Ok(bytes)
        }
        _ => Err(Error::Invalid("platform must be android or ios".into())),
    }
}
pub async fn invoke(extension: &WorkspaceExtension, name: &str, p: &Value) -> Result<Value> {
    extension.ensure_storage().await?;
    match name {
        "devices.list" => {
            let mut devices = vec![];
            let mut integrations = vec![];
            let android = command::run(
                &executable(extension, "adb"),
                &extension.storage_root,
                &command::args(&["devices", "-l"]),
            )
            .await;
            match android {
                Ok(text) => {
                    for line in text.lines().skip(1) {
                        let fields = line.split_whitespace().collect::<Vec<_>>();
                        if fields.len() >= 2 {
                            devices.push(json!({"id":fields[0],"state":fields[1],"name":fields.iter().find_map(|s|s.strip_prefix("model:")).unwrap_or(fields[0]),"platform":"android","kind":if fields[0].starts_with("emulator-"){"emulator"}else{"device"}}));
                        }
                    }
                    integrations.push(json!({"platform":"android","available":true}));
                }
                Err(error) => integrations.push(
                    json!({"platform":"android","available":false,"message":error.to_string()}),
                ),
            }
            if let Ok(text) = command::run(
                &executable(extension, "emulator"),
                &extension.storage_root,
                &command::args(&["-list-avds"]),
            )
            .await
            {
                for name in text.lines().filter(|s| !s.is_empty()) {
                    devices.push(json!({"id":format!("avd:{name}"),"name":name,"state":"stopped","platform":"android","kind":"emulator"}));
                }
            }
            if cfg!(target_os = "macos") {
                match command::run(
                    &executable(extension, "xcrun"),
                    &extension.storage_root,
                    &command::args(&["simctl", "list", "devices", "available", "--json"]),
                )
                .await
                {
                    Ok(text) => {
                        let value: Value = serde_json::from_str(&text)?;
                        for device in value["devices"]
                            .as_object()
                            .into_iter()
                            .flat_map(|v| v.values())
                            .flat_map(|v| v.as_array().into_iter().flatten())
                        {
                            devices.push(json!({"id":device["udid"],"name":device["name"],"state":device["state"],"platform":"ios","kind":"simulator"}));
                        }
                        integrations.push(json!({"platform":"ios","available":true}));
                    }
                    Err(error) => integrations.push(
                        json!({"platform":"ios","available":false,"message":error.to_string()}),
                    ),
                }
            } else {
                integrations.push(json!({"platform":"ios","available":false,"message":"Requires macOS and Xcode"}));
            }
            Ok(json!({"devices":devices,"integrations":integrations}))
        }
        "devices.open" => {
            let id = valid_id(p)?;
            match required(p, "platform")? {
                "android" => {
                    if let Some(name) = id.strip_prefix("avd:") {
                        let terminals = extension.terminals.clone();
                        let cwd = extension.storage_root.clone();
                        let program = executable(extension, "emulator")
                            .to_string_lossy()
                            .to_string();
                        let args = vec!["-avd".into(), name.into()];
                        return tokio::task::spawn_blocking(move || {
                            terminals.start(&cwd, &program, &args, 80, 24)
                        })
                        .await
                        .map_err(|e| Error::Provider(e.to_string()))?;
                    }
                    command::run(
                        &executable(extension, "adb"),
                        &extension.storage_root,
                        &command::args(&[
                            "-s",
                            id,
                            "shell",
                            "am",
                            "start",
                            "-a",
                            "android.settings.SETTINGS",
                        ]),
                    )
                    .await?;
                }
                "ios" => {
                    if !cfg!(target_os = "macos") {
                        return Err(Error::Unsupported(
                            "iOS simulator opening requires macOS and Xcode".into(),
                        ));
                    }
                    let state = command::run(
                        &executable(extension, "xcrun"),
                        &extension.storage_root,
                        &command::args(&["simctl", "list", "devices", "--json"]),
                    )
                    .await?;
                    let state: Value = serde_json::from_str(&state)?;
                    let booted = state["devices"]
                        .as_object()
                        .into_iter()
                        .flat_map(|v| v.values())
                        .flat_map(|v| v.as_array().into_iter().flatten())
                        .any(|v| v["udid"] == id && v["state"] == "Booted");
                    if !booted {
                        command::run(
                            &executable(extension, "xcrun"),
                            &extension.storage_root,
                            &command::args(&["simctl", "boot", id]),
                        )
                        .await?;
                    }
                    let simulator = command::run(
                        &executable(extension, "xcrun"),
                        &extension.storage_root,
                        &command::args(&["--find", "simctl"]),
                    )
                    .await
                    .ok()
                    .and_then(|path| {
                        let path = std::path::PathBuf::from(path.trim());
                        path.parent()?
                            .parent()?
                            .parent()
                            .map(|developer| developer.join("Applications/Simulator.app"))
                    })
                    .filter(|path| path.is_dir())
                    .map(|path| path.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Simulator".into());
                    let application = command::run(
                        Path::new("open"),
                        &extension.storage_root,
                        &command::args(&["-a", &simulator, "--args", "-CurrentDeviceUDID", id]),
                    )
                    .await;
                    return Ok(
                        json!({"opened":true,"bootRequested":!booted,"applicationOpened":application.is_ok(),"message":application.err().map(|error|format!("Simulator boot succeeded, but its native application could not open: {error}. Capture and the live device view remain available."))}),
                    );
                }
                _ => return Err(Error::Invalid("platform must be android or ios".into())),
            }
            Ok(json!({"opened":true}))
        }
        "devices.capture" => {
            let bytes = capture(extension, p).await?;
            if !bytes.starts_with(b"\x89PNG") {
                return Err(Error::Provider("native screenshot is not PNG".into()));
            }
            let path = assets::output_path(extension, "device.png").await?;
            tokio::fs::write(&path, bytes).await?;
            assets::register(extension, path).await
        }
        _ => extension.streams.invoke(extension, name, p).await,
    }
}
impl Streams {
    pub async fn invoke(
        &self,
        extension: &WorkspaceExtension,
        name: &str,
        p: &Value,
    ) -> Result<Value> {
        if name == "devices.stream.stop" {
            let id = required(p, "streamId")?;
            let mut streams = self.0.lock().await;
            let stream = streams
                .remove(id)
                .ok_or_else(|| Error::Invalid("unknown device stream".into()))?;
            stream.task.abort();
            return Ok(json!({"stopped":true}));
        }
        if name == "devices.stream.status" {
            let streams = self.0.lock().await;
            let id = required(p, "streamId")?;
            let stream = streams
                .get(id)
                .ok_or_else(|| Error::Invalid("unknown device stream".into()))?;
            return Ok(
                json!({"streamId":id,"url":stream.url,"running":!stream.task.is_finished()}),
            );
        }
        if name == "devices.stream.frame" {
            let id = required(p, "streamId")?;
            let parameters = {
                let streams = self.0.lock().await;
                let stream = streams
                    .get(id)
                    .ok_or_else(|| Error::Invalid("unknown device stream".into()))?;
                json!({"platform":stream.platform,"id":stream.device_id})
            };
            let bytes = capture(extension, &parameters).await?;
            if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Err(Error::Provider("native screenshot is not PNG".into()));
            }
            return Ok(json!({"mime":"image/png","data":STANDARD.encode(bytes)}));
        }
        if name != "devices.stream" {
            return Err(Error::Unsupported(format!("device command {name}")));
        }
        let _ = capture(extension, p).await?;
        let mut streams = self.0.lock().await;
        if streams.len() >= 4 {
            return Err(Error::Invalid(
                "close an existing device stream first".into(),
            ));
        }
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let url = format!("http://{address}");
        let host = address.to_string();
        let platform = required(p, "platform")?.to_owned();
        let id = valid_id(p)?.to_owned();
        let interval = p["intervalMs"].as_u64().unwrap_or(500).clamp(250, 5000);
        let program = executable(
            extension,
            if platform == "android" {
                "adb"
            } else {
                "xcrun"
            },
        );
        let directory = extension.storage_root.clone();
        let stream_platform = platform.clone();
        let device_id = id.clone();
        let task = tokio::spawn(async move {
            let mut clients = JoinSet::new();
            loop {
                tokio::select! {connection=listener.accept()=>match connection{Ok((socket,_)) if clients.len()<2=>{let program=program.clone();let directory=directory.clone();let id=id.clone();let host=host.clone();let platform=platform.clone();clients.spawn(async move{let _=frames(socket,&host,program,directory,platform,id,interval).await;});},Ok(_)=>{},Err(_)=>break},_=clients.join_next(),if !clients.is_empty()=>{}}
            }
        });
        let stream_id = uuid::Uuid::new_v4().to_string();
        streams.insert(
            stream_id.clone(),
            Stream {
                url: url.clone(),
                platform: stream_platform,
                device_id,
                task,
            },
        );
        Ok(json!({"streamId":stream_id,"url":url,"format":"multipart/png","intervalMs":interval}))
    }
}
async fn frames(
    mut socket: TcpStream,
    host: &str,
    program: std::path::PathBuf,
    directory: std::path::PathBuf,
    platform: String,
    id: String,
    interval: u64,
) -> Result<()> {
    let mut request = vec![];
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let mut buffer = [0u8; 1024];
            let n = socket.read(&mut buffer).await?;
            if n == 0 {
                return Err(Error::Invalid("incomplete stream request".into()));
            }
            request.extend_from_slice(&buffer[..n]);
            if request.len() > 16 * 1024 {
                return Err(Error::Invalid("stream headers exceed 16 KiB".into()));
            }
            if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                return Ok(());
            }
        }
    })
    .await
    .map_err(|_| Error::Timeout("device stream request".into()))??;
    let request = std::str::from_utf8(&request)
        .map_err(|_| Error::Invalid("invalid stream request".into()))?;
    if !request.starts_with("GET /")
        || !request.lines().any(|line| {
            line.split_once(':').is_some_and(|(key, value)| {
                key.eq_ignore_ascii_case("host") && value.trim() == host
            })
        })
    {
        return Err(Error::Invalid("invalid stream host or method".into()));
    }
    socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: multipart/x-mixed-replace; boundary=frame\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n").await?;
    loop {
        let bytes = if platform == "android" {
            command::binary(
                &program,
                &directory,
                &command::args(&["-s", &id, "exec-out", "screencap", "-p"]),
            )
            .await?
        } else {
            let path = directory.join(format!("frame-{}.png", uuid::Uuid::new_v4()));
            command::run(
                &program,
                &directory,
                &[
                    "simctl".into(),
                    "io".into(),
                    id.clone(),
                    "screenshot".into(),
                    path.to_string_lossy().into(),
                ],
            )
            .await?;
            let bytes = assets::bytes(&path).await?;
            let _ = tokio::fs::remove_file(path).await;
            bytes
        };
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err(Error::Provider("native screenshot is not PNG".into()));
        }
        if bytes.len() as u64 > assets::MAX_ASSET {
            return Err(Error::Invalid("screenshot exceeds 8 MiB".into()));
        }
        tokio::time::timeout(Duration::from_secs(3), async {
            socket
                .write_all(
                    format!(
                        "--frame\r\nContent-Type: image/png\r\nContent-Length: {}\r\n\r\n",
                        bytes.len()
                    )
                    .as_bytes(),
                )
                .await?;
            socket.write_all(&bytes).await?;
            socket.write_all(b"\r\n").await?;
            Ok::<(), std::io::Error>(())
        })
        .await
        .map_err(|_| Error::Timeout("device stream consumer".into()))??;
        tokio::time::sleep(Duration::from_millis(interval)).await;
    }
}
