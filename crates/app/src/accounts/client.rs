// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
//! Headless attachment to the independently running per-user owner.
use super::{Configuration, Endpoint, files};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use swarmie::{Error, Result};
async fn send(endpoint: &Endpoint, request: Value) -> Result<Value> {
    let url = reqwest::Url::parse(&endpoint.url)
        .map_err(|_| Error::Invalid("invalid account endpoint".into()))?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none()
        || url.path() != "/"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Invalid("invalid account endpoint".into()));
    }
    let id = request["id"].clone();
    let mut response = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .build()
        .map_err(|_| Error::Provider("account transport unavailable".into()))?
        .post(format!("{}/rpc", endpoint.url))
        .bearer_auth(&endpoint.token)
        .json(&request)
        .send()
        .await
        .map_err(|_| {
            Error::Provider("account owner unavailable; request was not replayed".into())
        })?;
    if !response.status().is_success() {
        return Err(Error::Provider("account owner is unavailable".into()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| Error::Provider("invalid account owner response".into()))?
    {
        if bytes.len() + chunk.len() > 256 * 1024 {
            return Err(Error::Provider(
                "account owner response exceeds its limit".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| Error::Provider("invalid account owner response".into()))?;
    if value["jsonrpc"] != "2.0" || value["id"] != id {
        return Err(Error::Provider("invalid account owner response".into()));
    }
    Ok(value)
}
async fn retained(directory: &Path) -> Result<Option<Endpoint>> {
    let Some(endpoint) = files::read::<Endpoint>(&directory.join("endpoint.json"))? else {
        return Ok(None);
    };
    if files::read::<Configuration>(&directory.join("configuration.json"))?
        .is_none_or(|config| config.installation != endpoint.installation)
    {
        return Err(Error::Invalid(
            "account endpoint belongs to a different installation".into(),
        ));
    }
    match tokio::time::timeout(
        Duration::from_secs(1),
        send(
            &endpoint,
            json!({"jsonrpc":"2.0","id":1,"method":"account/owner/read","params":{}}),
        ),
    )
    .await
    {
        Ok(Ok(value)) if value["result"]["installation"] == json!(endpoint.installation) => {
            Ok(Some(endpoint))
        }
        _ => Ok(None),
    }
}
#[cfg(windows)]
type OwnerChild = crate::runtime::windows_spawn::Child;
#[cfg(not(windows))]
type OwnerChild = tokio::process::Child;
struct StartingOwner {
    child: OwnerChild,
    armed: bool,
}
impl Drop for StartingOwner {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.child.start_kill();
        }
    }
}
pub async fn call(root: &Path, request: Value) -> Result<Value> {
    let directory = root.join("identity");
    files::directory(&directory)?;
    if let Some(endpoint) = retained(&directory).await? {
        return send(&endpoint, request).await;
    }
    // Serialize launchers independently from the lifetime lock held by the owner process.
    let mut launch = None;
    for _ in 0..100 {
        match files::lock(&directory.join("launch.lock")) {
            Ok(lock) => {
                launch = Some(lock);
                break;
            }
            Err(Error::Busy) => tokio::time::sleep(Duration::from_millis(100)).await,
            Err(error) => return Err(error),
        }
    }
    let _launch = launch.ok_or(Error::Busy)?;
    if let Some(endpoint) = retained(&directory).await? {
        return send(&endpoint, request).await;
    }
    let output = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("owner.log"))?;
    let executable = std::env::current_exe()?;
    let args: Vec<std::ffi::OsString> = vec![
        "--state-dir".into(),
        root.as_os_str().to_owned(),
        "account".into(),
        "serve".into(),
    ];
    #[cfg(windows)]
    let child = crate::runtime::windows_spawn::owner(&executable, &args, &output)?;
    #[cfg(not(windows))]
    let child = {
        let mut command = tokio::process::Command::new(&executable);
        command
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(output.try_clone()?)
            .stderr(output);
        #[cfg(unix)]
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command.spawn()?
    };
    let mut child = StartingOwner { child, armed: true };
    for _ in 0..150 {
        if let Some(endpoint) = retained(&directory).await? {
            child.armed = false;
            return send(&endpoint, request).await;
        }
        if child.child.try_wait()?.is_some() {
            return Err(Error::Provider(
                "account owner could not start; inspect its local log".into(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(Error::Timeout("account owner startup".into()))
}
