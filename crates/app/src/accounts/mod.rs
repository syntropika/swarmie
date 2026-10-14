// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
//! Independently hosted per-user account owner, shared by native windows and headless clients.
mod cli;
mod client;
mod files;
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
pub use cli::{AccountCommand, run};
pub use client::call;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use swarmie::{Error, Result};
use swarmie_accounts::{
    AccountManager,
    vault::{PlatformVault, Vault},
};
use tokio::sync::Mutex;

// The non-secret marker invalidates retained credentials even when the OS vault is locked.
struct GuardedVault {
    vault: PlatformVault,
    marker: PathBuf,
}
#[async_trait::async_trait]
impl Vault for GuardedVault {
    async fn check(&self) -> swarmie_accounts::Result<()> {
        self.vault.check().await
    }
    async fn read(&self) -> swarmie_accounts::Result<Option<String>> {
        if !files::read::<bool>(&self.marker)
            .map_err(|_| swarmie_accounts::Error::Vault)?
            .unwrap_or(false)
        {
            return Ok(None);
        }
        self.vault.read().await
    }
    async fn write(&self, secret: String) -> swarmie_accounts::Result<()> {
        files::write(&self.marker, &false).map_err(|_| swarmie_accounts::Error::Vault)?;
        self.vault.write(secret).await?;
        files::write(&self.marker, &true).map_err(|_| swarmie_accounts::Error::Vault)
    }
    async fn delete(&self) -> swarmie_accounts::Result<()> {
        files::write(&self.marker, &false).map_err(|_| swarmie_accounts::Error::Vault)?;
        // Credentials no longer restore locally; a locked vault can defer physical deletion.
        let _ = self.vault.delete().await;
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Configuration {
    installation: uuid::Uuid,
    origin: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Endpoint {
    url: String,
    token: String,
    installation: uuid::Uuid,
}
struct Owner {
    directory: PathBuf,
    endpoint: Endpoint,
    config: Mutex<Configuration>,
    manager: Mutex<Option<Arc<AccountManager>>>,
    stop: tokio_util::sync::CancellationToken,
}
fn account_error(error: swarmie_accounts::Error) -> Error {
    Error::Provider(error.to_string())
}
fn unconfigured() -> Value {
    json!({"status":"signedOut","generation":0,"profile":null,"persistence":"secureVault","loginId":null,"configured":false})
}
impl Owner {
    async fn invoke(&self, method: &str, params: Value) -> Result<Value> {
        if method == "account/owner/read" {
            return Ok(json!({"installation":self.endpoint.installation}));
        }
        if method == "account/owner/stop" {
            self.stop.cancel();
            return Ok(Value::Null);
        }
        // Hold the manager slot across commands so reconfiguration cannot race with a login or callback.
        let mut slot = self.manager.lock().await;
        if method == "account/configure" {
            if let Some(manager) = slot.as_ref()
                && manager.read().await.map_err(account_error)?.status != "signedOut"
            {
                return Err(Error::Busy);
            }
            let origin = params["origin"]
                .as_str()
                .ok_or_else(|| Error::Invalid("account origin is required".into()))?;
            let origin = swarmie_accounts::validate_origin(origin)
                .map_err(account_error)?
                .to_string();
            let mut config = self.config.lock().await;
            let manager = Arc::new(
                AccountManager::new(
                    &origin,
                    config.installation,
                    Arc::new(GuardedVault {
                        vault: PlatformVault::new(config.installation),
                        marker: self.directory.join("vault-enabled.json"),
                    }),
                )
                .await
                .map_err(account_error)?,
            );
            config.origin = Some(origin);
            files::write(&self.directory.join("configuration.json"), &*config)?;
            *slot = Some(manager);
            return Ok(unconfigured_with(true));
        }
        let Some(manager) = slot.as_ref() else {
            if method == "account/read" || method == "account/profile/read" {
                return Ok(unconfigured());
            }
            return Err(Error::Invalid(
                "configure an account service origin before signing in".into(),
            ));
        };
        match method {
            "account/read" | "account/profile/read" => Ok(serde_json::to_value(
                manager.read().await.map_err(account_error)?,
            )?),
            "account/login/start" => {
                let value = manager
                    .start(
                        format!("{}/account/callback", self.endpoint.url),
                        params["sessionOnly"].as_bool().unwrap_or(false),
                    )
                    .await
                    .map_err(account_error)?;
                if params["openBrowser"].as_bool().unwrap_or(true) {
                    let url = value["authorizationUrl"].as_str().unwrap().to_owned();
                    // Launching a browser does not own the pending login's lifetime.
                    if crate::open::browser(&url).await.is_err() {
                        return Ok(
                            json!({"loginId":value["loginId"],"authorizationUrl":value["authorizationUrl"],"browserOpened":false}),
                        );
                    }
                }
                Ok(value)
            }
            "account/login/cancel" => {
                let id = serde_json::from_value(params["loginId"].clone())?;
                Ok(serde_json::to_value(
                    manager.cancel(id).await.map_err(account_error)?,
                )?)
            }
            "account/logout" => {
                let expected = params
                    .get("expectedAccountId")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()?;
                manager.logout_for(expected).await.map_err(account_error)
            }
            "account/profile/avatar/update" => Ok(serde_json::to_value(
                manager.avatar(params).await.map_err(account_error)?,
            )?),
            "account/profile/update" => Ok(serde_json::to_value(
                manager.update(params).await.map_err(account_error)?,
            )?),
            _ => Err(Error::Unsupported("account method".into())),
        }
    }
}
fn unconfigured_with(configured: bool) -> Value {
    let mut value = unconfigured();
    value["configured"] = json!(configured);
    value
}
async fn rpc(
    State(owner): State<Arc<Owner>>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    if headers.get("authorization").and_then(|v| v.to_str().ok())
        != Some(format!("Bearer {}", owner.endpoint.token).as_str())
        || headers.contains_key("origin")
        || headers.get("host").and_then(|v| v.to_str().ok())
            != owner.endpoint.url.strip_prefix("http://")
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if request["jsonrpc"] != "2.0"
        || !request["method"].is_string()
        || !(request["id"].is_number() || request["id"].is_string())
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let result = owner
        .invoke(
            request["method"].as_str().unwrap(),
            request["params"].clone(),
        )
        .await;
    Json(match result {Ok(value)=>json!({"jsonrpc":"2.0","id":request["id"],"result":value}),Err(error)=>json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32000,"message":error.to_string()}})}).into_response()
}
#[derive(Deserialize)]
struct Callback {
    state: String,
    code: String,
}
async fn callback(
    State(owner): State<Arc<Owner>>,
    headers: HeaderMap,
    Query(query): Query<Callback>,
) -> Response {
    if headers.get("host").and_then(|v| v.to_str().ok())
        != owner.endpoint.url.strip_prefix("http://")
        || query.state.len() > 128
        || query.code.len() > 128
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let slot = owner.manager.lock().await;
    let result = if let Some(manager) = slot.as_ref() {
        manager.complete(&query.state, &query.code).await
    } else {
        Err(swarmie_accounts::Error::Unauthenticated)
    };
    let mut response=match result {Ok(())=>Html("<!doctype html><html lang=\"en\"><title>Swarmie connected</title><h1>You are signed in</h1><p>You can close this browser tab and return to Swarmie.</p></html>").into_response(),Err(_)=>(StatusCode::BAD_REQUEST,"Sign-in could not complete. Start a new sign-in from Swarmie.").into_response()};
    for (name, value) in [
        ("cache-control", "no-store"),
        ("referrer-policy", "no-referrer"),
        (
            "content-security-policy",
            "default-src 'none'; frame-ancestors 'none'",
        ),
    ] {
        response
            .headers_mut()
            .insert(name, axum::http::HeaderValue::from_static(value));
    }
    response
}
pub async fn serve(root: &Path) -> Result<()> {
    let directory = root.join("identity");
    files::directory(&directory)?;
    let _owner = files::lock(&directory.join("owner.lock"))?;
    let config = files::read::<Configuration>(&directory.join("configuration.json"))?.unwrap_or(
        Configuration {
            installation: uuid::Uuid::new_v4(),
            origin: None,
        },
    );
    files::write(&directory.join("configuration.json"), &config)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = Endpoint {
        url: format!("http://{}", listener.local_addr()?),
        token: format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4()),
        installation: config.installation,
    };
    let manager = match config.origin.as_ref() {
        Some(origin) => Some(Arc::new(
            AccountManager::new(
                origin,
                config.installation,
                Arc::new(GuardedVault {
                    vault: PlatformVault::new(config.installation),
                    marker: directory.join("vault-enabled.json"),
                }),
            )
            .await
            .map_err(account_error)?,
        )),
        None => None,
    };
    let stop = tokio_util::sync::CancellationToken::new();
    let owner = Arc::new(Owner {
        directory: directory.clone(),
        endpoint: endpoint.clone(),
        config: Mutex::new(config),
        manager: Mutex::new(manager),
        stop: stop.clone(),
    });
    let router = axum::Router::new()
        .route("/rpc", post(rpc))
        .route("/account/callback", get(callback))
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(owner.clone());
    files::write(&directory.join("endpoint.json"), &endpoint)?;
    let tick_owner = owner.clone();
    let tick_stop = stop.clone();
    let tick = tokio::spawn(async move {
        loop {
            tokio::select! {_ = tick_stop.cancelled()=>break,_ = tokio::time::sleep(Duration::from_secs(30))=>{let slot=tick_owner.manager.lock().await;if let Some(manager)=slot.as_ref(){manager.tick().await;}}}
        }
    });
    let signal = stop.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        signal.cancel();
    });
    let result = axum::serve(listener, router)
        .with_graceful_shutdown(stop.clone().cancelled_owned())
        .await;
    stop.cancel();
    tick.abort();
    let _ = std::fs::remove_file(directory.join("endpoint.json"));
    result.map_err(Into::into)
}
