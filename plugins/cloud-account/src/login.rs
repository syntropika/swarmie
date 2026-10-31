use crate::{AccountSession, AccountTransport, Error, Result, SessionStorage, session::Grant};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use reqwest::Method;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{rc::Rc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use uuid::Uuid;
use zeroize::Zeroizing;

/// Memory-only native PKCE flow. The UI may open the authorization URL but never sees tokens.
pub struct LoginFlow {
    transport: AccountTransport,
    storage: Rc<dyn SessionStorage>,
    listener: TcpListener,
    verifier: Zeroizing<String>,
    state: Zeroizing<String>,
    callback: String,
    installation: Uuid,
    authorization: String,
}
impl LoginFlow {
    pub async fn begin(
        transport: AccountTransport,
        storage: Rc<dyn SessionStorage>,
        installation: Uuid,
    ) -> Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| Error::Unavailable)?;
        let callback = format!(
            "http://127.0.0.1:{}/account/callback",
            listener
                .local_addr()
                .map_err(|_| Error::Unavailable)?
                .port()
        );
        let verifier = random();
        let state = random();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let body=Zeroizing::new(serde_json::to_vec(&serde_json::json!({"installationId":installation,"callback":callback,"state":state.as_str(),"challenge":challenge})).map_err(|_|Error::Invalid)?);
        let response = transport
            .exchange("/auth/handoff/request", Method::POST, Some(&body), None)
            .await?;
        response.success()?;
        let value: Value = response.decode()?;
        let authorization = value
            .get("authorizationUrl")
            .and_then(Value::as_str)
            .ok_or(Error::Integrity)?
            .to_owned();
        let url = reqwest::Url::parse(&authorization).map_err(|_| Error::Integrity)?;
        let origin =
            reqwest::Url::parse(&transport.endpoints.authentication).map_err(|_| Error::Invalid)?;
        if url.origin() != origin.origin() || !url.path().starts_with(origin.path()) {
            return Err(Error::Integrity);
        }
        Ok(Self {
            transport,
            storage,
            listener,
            verifier,
            state,
            callback,
            installation,
            authorization,
        })
    }
    pub fn authorization_url(&self) -> &str {
        &self.authorization
    }
    pub async fn finish(self) -> Result<AccountSession> {
        self.finish_if(|| true).await
    }
    /// The native owner fences late login completion before protected session installation.
    pub async fn finish_if(self, permitted: impl Fn() -> bool) -> Result<AccountSession> {
        tokio::time::timeout(Duration::from_secs(300), self.receive(permitted))
            .await
            .map_err(|_| Error::SignInRequired)?
    }
    async fn receive(self, permitted: impl Fn() -> bool) -> Result<AccountSession> {
        for _ in 0..16 {
            let (mut socket, peer) = self
                .listener
                .accept()
                .await
                .map_err(|_| Error::Unavailable)?;
            if !peer.ip().is_loopback() {
                continue;
            }
            let mut request = Zeroizing::new(vec![]);
            let mut chunk = [0; 1024];
            let read = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let n = socket
                        .read(&mut chunk)
                        .await
                        .map_err(|_| Error::Unavailable)?;
                    if n == 0 {
                        return Err(Error::Invalid);
                    }
                    request.extend_from_slice(&chunk[..n]);
                    if request.len() > 8192 {
                        return Err(Error::Invalid);
                    }
                    if request.windows(4).any(|w| w == b"\r\n\r\n") {
                        return Ok(());
                    }
                }
            })
            .await;
            if !matches!(read, Ok(Ok(()))) {
                continue;
            }
            let Ok(request) = std::str::from_utf8(&request) else {
                continue;
            };
            let host = format!(
                "127.0.0.1:{}",
                self.listener
                    .local_addr()
                    .map_err(|_| Error::Unavailable)?
                    .port()
            );
            let hosts: Vec<_> = request
                .lines()
                .skip(1)
                .filter_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("host").then(|| value.trim())
                })
                .collect();
            if hosts != [host.as_str()] {
                continue;
            }
            let Some(first) = request.lines().next() else {
                continue;
            };
            let mut parts = first.split(' ');
            if parts.next() != Some("GET") {
                continue;
            }
            let Some(path) = parts.next() else {
                continue;
            };
            if !path.starts_with("/account/callback?")
                || parts.next() != Some("HTTP/1.1")
                || parts.next().is_some()
            {
                continue;
            }
            let Ok(url) = reqwest::Url::parse(&format!("http://127.0.0.1{path}")) else {
                continue;
            };
            let pairs: Vec<_> = url.query_pairs().collect();
            let states: Vec<_> = pairs
                .iter()
                .filter(|(k, _)| k == "state")
                .map(|(_, v)| v.as_ref())
                .collect();
            let codes: Vec<_> = pairs
                .iter()
                .filter(|(k, _)| k == "code")
                .map(|(_, v)| v.as_ref())
                .collect();
            if url.path() != "/account/callback"
                || states != [self.state.as_str()]
                || codes.len() != 1
            {
                let _=socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                continue;
            }
            if !permitted() {
                return Err(Error::SignInRequired);
            }
            let body=Zeroizing::new(serde_json::to_vec(&serde_json::json!({"code":codes[0],"verifier":self.verifier.as_str(),"callback":self.callback,"installationId":self.installation})).map_err(|_|Error::Invalid)?);
            let reply = self
                .transport
                .exchange("/auth/handoff/redeem", Method::POST, Some(&body), None)
                .await?;
            reply.success()?;
            let grant: Grant = reply.decode()?;
            if !permitted() {
                // No protected store mutation occurs for a superseded native owner.
                grant.revoke(&self.transport).await;
                return Err(Error::SignInRequired);
            }
            let message = b"You can return to Swarmie.";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                message.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.write_all(message).await;
            return AccountSession::accept_if(
                self.transport,
                self.storage,
                self.installation,
                grant,
                permitted,
            )
            .await;
        }
        Err(Error::SignInRequired)
    }
}
fn random() -> Zeroizing<String> {
    let mut bytes = Zeroizing::new([0; 32]);
    OsRng.fill_bytes(bytes.as_mut());
    Zeroizing::new(URL_SAFE_NO_PAD.encode(bytes.as_ref()))
}
