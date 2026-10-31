use crate::account::{AccountTransport, Error, Result, SessionStorage, StoredSession};
use futures::lock::Mutex;
use reqwest::Method;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Grant {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub profile: Value,
}
impl Grant {
    pub(crate) async fn revoke(&self, transport: &AccountTransport) {
        let Ok(body) = serde_json::to_vec(&serde_json::json!({"refreshToken":self.refresh_token}))
        else {
            return;
        };
        let body = Zeroizing::new(body);
        // A late or rejected handoff must not install credentials; revocation is best effort.
        let _ = transport
            .exchange("/auth/session/revoke", Method::POST, Some(&body), None)
            .await;
    }
}
impl Drop for Grant {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}

/// Actual native credential family, obtained from authenticated account endpoints.
#[derive(Clone, Debug, Serialize)]
pub struct AccountIdentity {
    pub account: Uuid,
    pub installation: Uuid,
    pub family: Uuid,
}

/// One native owner serializes renewal; ordinary callers never receive bearer credentials.
#[derive(Clone)]
pub struct AccountSession(Rc<Session>);
struct Session {
    transport: AccountTransport,
    storage: Rc<dyn SessionStorage>,
    tokens: Mutex<StoredSession>,
    identity: AccountIdentity,
    live: Cell<bool>,
    ended: RefCell<std::collections::BTreeMap<Uuid, Box<dyn Fn()>>>,
}

impl AccountSession {
    pub fn reopen(
        transport: AccountTransport,
        storage: Rc<dyn SessionStorage>,
        account: Uuid,
        installation: Uuid,
    ) -> Result<Self> {
        let tokens = storage.load(account, installation)?;
        if tokens.endpoints != transport.endpoints {
            return Err(Error::Integrity);
        }
        let identity = AccountIdentity {
            account,
            installation,
            family: tokens.family,
        };
        Ok(Self(Rc::new(Session {
            transport,
            storage,
            tokens: Mutex::new(tokens),
            identity,
            live: Cell::new(true),
            ended: RefCell::new(std::collections::BTreeMap::new()),
        })))
    }
    pub(crate) async fn accept_if(
        transport: AccountTransport,
        storage: Rc<dyn SessionStorage>,
        installation: Uuid,
        grant: Grant,
        permitted: impl Fn() -> bool,
    ) -> Result<Self> {
        let result = async {
            let account: Uuid = grant
                .profile
                .get("id")
                .and_then(Value::as_str)
                .ok_or(Error::Integrity)?
                .parse()
                .map_err(|_| Error::Integrity)?;
            let response = transport
                .exchange(
                    "/api/account/sessions",
                    Method::GET,
                    None,
                    Some(&grant.access_token),
                )
                .await?;
            response.success()?;
            let page: Value = response.decode()?;
            let family: Uuid = page
                .pointer("/current/id")
                .and_then(Value::as_str)
                .ok_or(Error::Integrity)?
                .parse()
                .map_err(|_| Error::Integrity)?;
            let declared: Uuid = page
                .pointer("/current/installationId")
                .and_then(Value::as_str)
                .ok_or(Error::Integrity)?
                .parse()
                .map_err(|_| Error::Integrity)?;
            if declared != installation {
                return Err(Error::Integrity);
            }
            let tokens = StoredSession {
                version: 1,
                account,
                installation,
                family,
                endpoints: transport.endpoints.clone(),
                access_token: grant.access_token.clone(),
                refresh_token: grant.refresh_token.clone(),
                expires_at: grant.expires_at,
                renewal_unknown: false,
            };
            if !permitted() {
                return Err(Error::SignInRequired);
            }
            storage.save(&tokens)?;
            Ok(Self(Rc::new(Session {
                transport: transport.clone(),
                storage,
                tokens: Mutex::new(tokens),
                identity: AccountIdentity {
                    account,
                    installation,
                    family,
                },
                live: Cell::new(true),
                ended: RefCell::new(std::collections::BTreeMap::new()),
            })))
        }
        .await;
        if result.is_err() {
            grant.revoke(&transport).await;
        }
        result
    }
    /// Replace one embedding owner's cleanup registration rather than retaining every unlock.
    pub fn on_end(&self, owner: Uuid, callback: Box<dyn Fn()>) {
        if self.is_live() {
            self.0.ended.borrow_mut().insert(owner, callback);
        } else {
            callback();
        }
    }
    fn end(&self) {
        if self.0.live.replace(false) {
            let callbacks = std::mem::take(&mut *self.0.ended.borrow_mut());
            for callback in callbacks.into_values() {
                callback();
            }
        }
    }
    pub fn identity(&self) -> &AccountIdentity {
        &self.0.identity
    }
    pub fn is_live(&self) -> bool {
        self.0.live.get()
    }
    pub async fn needs_sign_in(&self) -> bool {
        self.0.tokens.lock().await.renewal_unknown
    }
    pub fn endpoints(&self) -> &crate::account::Endpoints {
        &self.0.transport.endpoints
    }

    async fn access(&self, rejected: Option<&str>) -> Result<Zeroizing<String>> {
        if !self.is_live() {
            return Err(Error::Revoked);
        }
        let mut tokens = self.0.tokens.lock().await;
        if !self.is_live() {
            return Err(Error::Revoked);
        }
        if tokens.renewal_unknown {
            return Err(Error::RefreshUnknown);
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Unavailable)?
            .as_secs();
        if rejected.is_some_and(|rejected| rejected == tokens.access_token)
            || tokens.expires_at
                <= i64::try_from(now.saturating_add(30)).map_err(|_| Error::Unavailable)?
        {
            // Rotating renewal has no recognition contract: never replay after a lost response.
            tokens.renewal_unknown = true;
            self.0.storage.save(&tokens)?;
            let body = Zeroizing::new(
                serde_json::to_vec(&serde_json::json!({"refreshToken":tokens.refresh_token}))
                    .map_err(|_| Error::Invalid)?,
            );
            let response = self
                .0
                .transport
                .exchange("/auth/session/refresh", Method::POST, Some(&body), None)
                .await?;
            if response.status == 401 {
                self.end();
                return Err(Error::Revoked);
            }
            response.success()?;
            let mut grant: Grant = response.decode()?;
            let account: Uuid = grant
                .profile
                .get("id")
                .and_then(Value::as_str)
                .ok_or(Error::Integrity)?
                .parse()
                .map_err(|_| Error::Integrity)?;
            if account != tokens.account {
                return Err(Error::Integrity);
            }
            tokens.access_token.zeroize();
            tokens.refresh_token.zeroize();
            tokens.access_token = std::mem::take(&mut grant.access_token);
            tokens.refresh_token = std::mem::take(&mut grant.refresh_token);
            tokens.expires_at = grant.expires_at;
            tokens.renewal_unknown = false;
            self.0.storage.save(&tokens)?;
        }
        if !self.is_live() {
            return Err(Error::Revoked);
        }
        Ok(Zeroizing::new(tokens.access_token.clone()))
    }
    /// No automatic retry of a mutating API call; the consuming capability owns recognition.
    pub async fn post<T: DeserializeOwned>(&self, path: &str, value: &impl Serialize) -> Result<T> {
        let token = self.access(None).await?;
        let body = Zeroizing::new(serde_json::to_vec(value).map_err(|_| Error::Invalid)?);
        let reply = self
            .0
            .transport
            .exchange(path, Method::POST, Some(&body), Some(&token))
            .await?;
        // A bare access-token 401 can mean expiration; it is not a durable enrollment denial.
        if reply.status == 401 {
            // The original mutation is never replayed. Renewal adjudicates denial; a successful
            // replacement token is for a subsequent explicit operation/recognition request.
            self.access(Some(&token)).await?;
            return Err(Error::SignInRequired);
        }
        reply.success()?;
        reply.decode()
    }
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let token = self.access(None).await?;
        let reply = self
            .0
            .transport
            .exchange(path, Method::GET, None, Some(&token))
            .await?;
        if reply.status == 401 {
            // The original mutation is never replayed. Renewal adjudicates denial; a successful
            // replacement token is for a subsequent explicit operation/recognition request.
            self.access(Some(&token)).await?;
            return Err(Error::SignInRequired);
        }
        reply.success()?;
        reply.decode()
    }
    /// End local authority immediately. Failure to revoke remotely cannot revive the local owner.
    pub async fn logout(&self) -> Result<()> {
        self.end();
        let mut tokens = self.0.tokens.lock().await;
        let forgotten = self.0.storage.forget(tokens.account, tokens.installation);
        let body = Zeroizing::new(
            serde_json::to_vec(&serde_json::json!({"refreshToken":tokens.refresh_token}))
                .map_err(|_| Error::Invalid)?,
        );
        tokens.access_token.zeroize();
        tokens.refresh_token.zeroize();
        let revoked = self
            .0
            .transport
            .exchange("/auth/session/revoke", Method::POST, Some(&body), None)
            .await
            .and_then(|reply| reply.success());
        forgotten.and(revoked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::{Endpoints, https};
    use std::{
        collections::BTreeMap,
        sync::{
            Arc, Mutex as StdMutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    #[derive(Default)]
    struct Store(StdMutex<Entries>);
    type Entries = BTreeMap<(Uuid, Uuid), Zeroizing<Vec<u8>>>;
    impl SessionStorage for Arc<Store> {
        fn load(&self, account: Uuid, installation: Uuid) -> Result<StoredSession> {
            StoredSession::decode(
                self.0
                    .lock()
                    .unwrap()
                    .get(&(account, installation))
                    .ok_or(Error::SignInRequired)?,
            )
        }
        fn save(&self, session: &StoredSession) -> Result<()> {
            session.write_private(|bytes| {
                self.0.lock().unwrap().insert(
                    (session.account, session.installation),
                    Zeroizing::new(bytes.to_vec()),
                );
                Ok(())
            })
        }
        fn forget(&self, account: Uuid, installation: Uuid) -> Result<()> {
            self.0.lock().unwrap().remove(&(account, installation));
            Ok(())
        }
    }
    fn transport(endpoint: &str) -> AccountTransport {
        AccountTransport::new(
            Endpoints {
                api: endpoint.into(),
                authentication: endpoint.into(),
            },
            Some(https::certificate()),
        )
        .unwrap()
    }
    fn stored(endpoint: &str, account: Uuid, installation: Uuid, expires_at: i64) -> StoredSession {
        StoredSession {
            version: 1,
            account,
            installation,
            family: Uuid::new_v4(),
            endpoints: Endpoints {
                api: endpoint.into(),
                authentication: endpoint.into(),
            },
            access_token: "synthetic-access".into(),
            refresh_token: "synthetic-refresh".into(),
            expires_at,
            renewal_unknown: false,
        }
    }
    fn reply(status: u16, value: Value) -> https::Reply {
        https::Reply {
            status,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&value).unwrap(),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uncertain_rotating_renewal_is_durably_fenced_and_never_repeated() {
        let account = Uuid::new_v4();
        let installation = Uuid::new_v4();
        let store = Arc::new(Store::default());
        let observed = store.clone();
        let (endpoint, server) = https::serve(1, move |request| {
            assert_eq!(request.method, "POST");
            assert_eq!(request.path, "/auth/session/refresh");
            assert!(!request.headers.contains_key("authorization"));
            assert_eq!(
                serde_json::from_slice::<Value>(&request.body).unwrap()["refreshToken"],
                "synthetic-refresh"
            );
            assert!(
                observed
                    .load(account, installation)
                    .unwrap()
                    .renewal_unknown
            );
            reply(503, serde_json::json!({"error":{"code":"unavailable"}}))
        })
        .await;
        store
            .save(&stored(&endpoint, account, installation, 0))
            .unwrap();
        let session = AccountSession::reopen(
            transport(&endpoint),
            Rc::new(store.clone()),
            account,
            installation,
        )
        .unwrap();
        assert_eq!(
            session
                .post::<Value>("/api/vault/commands", &serde_json::json!({}))
                .await,
            Err(Error::Unavailable)
        );
        assert_eq!(
            session
                .post::<Value>("/api/vault/commands", &serde_json::json!({}))
                .await,
            Err(Error::RefreshUnknown)
        );
        assert!(session.is_live());
        assert!(store.load(account, installation).unwrap().renewal_unknown);
        server.await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn successful_renewal_updates_private_custody_and_next_attempt_uses_new_access() {
        let account = Uuid::new_v4();
        let installation = Uuid::new_v4();
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let (endpoint,server)=https::serve(2,move|request|{
            assert_eq!(request.method,"POST");let body:Value=serde_json::from_slice(&request.body).unwrap();
            if seen.fetch_add(1,Ordering::SeqCst)==0{
                assert_eq!(request.path,"/auth/session/refresh");assert!(!request.headers.contains_key("authorization"));assert_eq!(body["refreshToken"],"synthetic-refresh");
                reply(200,serde_json::json!({"accessToken":"renewed-access","refreshToken":"renewed-refresh","expiresAt":2_000_000_000_i64,"profile":{"id":account}}))
            }else{assert_eq!(request.path,"/api/vault/commands");assert_eq!(request.headers["authorization"],"Bearer renewed-access");assert_eq!(body["synthetic"],true);reply(200,serde_json::json!({"ok":true}))}
        }).await;
        let store = Arc::new(Store::default());
        store
            .save(&stored(&endpoint, account, installation, 0))
            .unwrap();
        let session = AccountSession::reopen(
            transport(&endpoint),
            Rc::new(store.clone()),
            account,
            installation,
        )
        .unwrap();
        let response: Value = session
            .post(
                "/api/vault/commands",
                &serde_json::json!({"synthetic":true}),
            )
            .await
            .unwrap();
        assert_eq!(response["ok"], true);
        let persisted = store.load(account, installation).unwrap();
        assert!(!persisted.renewal_unknown);
        assert_eq!(persisted.refresh_token, "renewed-refresh");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        server.await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_pkce_callback_binds_state_verifier_installation_and_actual_family() {
        let account = Uuid::new_v4();
        let installation = Uuid::new_v4();
        let family = Uuid::new_v4();
        let state = Arc::new(StdMutex::new(None::<Value>));
        let observed = state.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let endpoint_cell = Arc::new(StdMutex::new(String::new()));
        let server_endpoint = endpoint_cell.clone();
        let (endpoint,server)=https::serve(3,move|request|{
            let step=seen.fetch_add(1,Ordering::SeqCst);
            if step==0{
                assert_eq!(request.method,"POST");assert_eq!(request.path,"/auth/handoff/request");assert!(!request.headers.contains_key("authorization"));
                let body:Value=serde_json::from_slice(&request.body).unwrap();assert!(body.get("verifier").is_none());*observed.lock().unwrap()=Some(body);
                reply(200,serde_json::json!({"authorizationUrl":format!("{}auth/handoff/authorize?request=synthetic",server_endpoint.lock().unwrap())}))
            }else if step==1{
                assert_eq!(request.method,"POST");assert_eq!(request.path,"/auth/handoff/redeem");assert!(!request.headers.contains_key("authorization"));let body:Value=serde_json::from_slice(&request.body).unwrap();let prior=observed.lock().unwrap();let prior=prior.as_ref().unwrap();
                use base64::Engine;use sha2::Digest;
                let challenge=base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(body["verifier"].as_str().unwrap().as_bytes()));assert_eq!(challenge,prior["challenge"]);assert_eq!(body["callback"],prior["callback"]);assert_eq!(body["installationId"],prior["installationId"]);assert_eq!(body["code"],"synthetic-code");
                reply(200,serde_json::json!({"accessToken":"native-access","refreshToken":"native-refresh","expiresAt":2_000_000_000_i64,"profile":{"id":account}}))
            }else{assert_eq!(request.method,"GET");assert_eq!(request.path,"/api/account/sessions");assert!(request.body.is_empty());assert_eq!(request.headers["authorization"],"Bearer native-access");reply(200,serde_json::json!({"current":{"id":family,"installationId":installation},"others":[]}))}
        }).await;
        *endpoint_cell.lock().unwrap() = endpoint.clone();
        let store = Arc::new(Store::default());
        let flow = crate::account::LoginFlow::begin(
            transport(&endpoint),
            Rc::new(store.clone()),
            installation,
        )
        .await
        .unwrap();
        assert!(flow.authorization_url().contains("request=synthetic"));
        let callback = state.lock().unwrap().as_ref().unwrap().clone();
        let callback_url = reqwest::Url::parse(callback["callback"].as_str().unwrap()).unwrap();
        let browser = async {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut socket = tokio::net::TcpStream::connect((
                std::net::Ipv4Addr::LOCALHOST,
                callback_url.port().unwrap(),
            ))
            .await
            .unwrap();
            let request = format!(
                "GET /account/callback?code=synthetic-code&state={} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
                callback["state"].as_str().unwrap(),
                callback_url.port().unwrap()
            );
            socket.write_all(request.as_bytes()).await.unwrap();
            let mut response = vec![];
            socket.read_to_end(&mut response).await.unwrap();
            assert!(response.starts_with(b"HTTP/1.1 200"));
        };
        let (session, ()) = tokio::join!(flow.finish(), browser);
        let session = session.unwrap();
        assert_eq!(session.identity().account, account);
        assert_eq!(session.identity().family, family);
        assert_eq!(session.identity().installation, installation);
        assert_eq!(
            store.load(account, installation).unwrap().access_token,
            "native-access"
        );
        server.await.unwrap();
    }
    #[tokio::test(flavor = "current_thread")]
    async fn real_https_definitive_refusals_are_distinct_from_availability_failures() {
        for (status, expected) in [
            (302, Error::Refused),
            (403, Error::Forbidden),
            (404, Error::Refused),
            (409, Error::Refused),
            (422, Error::Refused),
            (408, Error::Unavailable),
            (429, Error::Unavailable),
            (500, Error::Unavailable),
            (503, Error::Unavailable),
        ] {
            let account = Uuid::new_v4();
            let installation = Uuid::new_v4();
            let store = Arc::new(Store::default());
            let (endpoint, server) = https::serve(1, move |request| {
                assert_eq!(request.path, "/api/vault/check");
                reply(status, serde_json::json!({"error":"synthetic"}))
            })
            .await;
            store
                .save(&stored(&endpoint, account, installation, i64::MAX))
                .unwrap();
            let session =
                AccountSession::reopen(transport(&endpoint), Rc::new(store), account, installation)
                    .unwrap();
            assert_eq!(
                session.post::<Value>("/api/vault/check", &()).await,
                Err(expected)
            );
            server.await.unwrap();
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn superseded_handoff_revokes_new_family_without_installing_tokens() {
        let account = Uuid::new_v4();
        let installation = Uuid::new_v4();
        let family = Uuid::new_v4();
        let count = Arc::new(AtomicUsize::new(0));
        let observed = count.clone();
        let (endpoint, server) = https::serve(2, move |request| {
            let index = observed.fetch_add(1, Ordering::SeqCst);
            if index == 0 {
                assert_eq!(request.path, "/api/account/sessions");
                assert_eq!(request.headers["authorization"], "Bearer new-access");
                reply(200, serde_json::json!({"current":{"id":family,"installationId":installation},"others":[]}))
            } else {
                assert_eq!(request.path, "/auth/session/revoke");
                assert!(!request.headers.contains_key("authorization"));
                let body:Value=serde_json::from_slice(&request.body).unwrap();
                assert_eq!(body["refreshToken"], "new-refresh");
                reply(200, serde_json::json!({}))
            }
        }).await;
        let transport = AccountTransport::new(
            Endpoints {
                api: endpoint,
                authentication: "https://browser-login.example/".into(),
            },
            Some(https::certificate()),
        )
        .unwrap();
        let store = Arc::new(Store::default());
        let grant = Grant {
            access_token: "new-access".into(),
            refresh_token: "new-refresh".into(),
            expires_at: i64::MAX,
            profile: serde_json::json!({"id":account}),
        };
        let result = AccountSession::accept_if(
            transport,
            Rc::new(store.clone()),
            installation,
            grant,
            || false,
        )
        .await;
        assert!(matches!(result, Err(Error::SignInRequired)));
        assert!(store.0.lock().unwrap().is_empty());
        server.await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
}
