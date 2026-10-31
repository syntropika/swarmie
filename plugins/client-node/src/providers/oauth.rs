//! The independent OAuth protocol owns PKCE and loopback I/O; Tembra owns each session.
use super::{NativeProviders, failure};
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use swarmie_providers::auth::BrowserLogin;
use swarmie_providers::settings::Account;
use tembra::{Context, Orb, Service, State};

pub(crate) const SERVICE: &str = "swarmie.providers.authorization";

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum AuthorizationState {
    Waiting {
        #[serde(rename = "authorizationUrl")]
        authorization_url: String,
        #[serde(rename = "userCode")]
        user_code: Option<String>,
        #[serde(rename = "callbackUri")]
        callback_uri: Option<String>,
    },
    Connected {
        account: Account,
    },
    Failed {
        error: String,
        retryable: bool,
    },
}

struct Authorization {
    login: BrowserLogin,
    source: Rc<NativeProviders>,
    principal: String,
    busy: Cell<bool>,
    cancelled: Cell<bool>,
    pending: RefCell<Option<swarmie_providers::auth::Credential>>,
    operation: RefCell<Option<Context>>,
}

struct Busy<'a>(&'a Cell<bool>);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl Authorization {
    fn waiting(&self) -> AuthorizationState {
        let instructions = self.login.instructions();
        AuthorizationState::Waiting {
            authorization_url: instructions.authorization_url.clone(),
            user_code: None,
            callback_uri: Some(instructions.callback_url.clone()),
        }
    }
    async fn advance(
        &self,
        context: &Context,
        state: &State<Option<AuthorizationState>>,
        callback: Option<String>,
        retry: bool,
    ) -> tembra::Result<()> {
        context.check()?;
        self.source.authorize(&context.principal()?)?;
        if self.cancelled.get() {
            return Ok(());
        }
        if state.value()?.as_ref().as_ref().is_some_and(|value| {
            matches!(value, AuthorizationState::Connected { .. })
                || matches!(
                    value,
                    AuthorizationState::Failed { retryable, .. } if !retry || !retryable
                )
        }) {
            return Ok(());
        }
        if self.busy.replace(true) {
            return Ok(());
        }
        let _busy = Busy(&self.busy);
        *self.operation.borrow_mut() = Some(context.clone());
        let manual = callback.is_some();
        let result = if self.pending.borrow().is_some() {
            Ok(None)
        } else {
            match callback {
                Some(callback) => self.login.submit_redirect(&callback).await.map(Some),
                None => self.login.poll().await,
            }
        };
        let next = match result {
            Ok(credential) => {
                if let Some(credential) = credential {
                    self.pending.replace(Some(credential));
                }
                let credential = self.pending.borrow().clone();
                if let Some(credential) = credential {
                    match self
                        .source
                        .save_oauth(context, &self.principal, &credential, &self.cancelled)
                        .await
                    {
                        Ok(account) => {
                            self.pending.take();
                            AuthorizationState::Connected { account }
                        }
                        Err(error) => AuthorizationState::Failed {
                            error: error.message,
                            retryable: true,
                        },
                    }
                } else {
                    self.waiting()
                }
            }
            Err(error) if manual => {
                self.operation.take();
                return Err(failure(&error.to_string()));
            }
            Err(error) => AuthorizationState::Failed {
                error: error.to_string(),
                retryable: false,
            },
        };
        self.operation.take();
        state.update(context, |_| Ok(Some(next)))?;
        Ok(())
    }
}

pub(crate) fn orb(source: Rc<NativeProviders>) -> Orb {
    Orb::new(SERVICE, 1).service(
        Service::new(SERVICE, move |declaration| {
            let address = declaration.address()?;
            source.authorize(&address.principal)?;
            let (provider, id, returning) = match address.key.as_slice() {
                [provider, id] => (provider, id, None),
                [provider, id, account] => (provider, id, Some(account.clone())),
                _ => return Err(failure("invalid authorization session key")),
            };
            if provider != "openai" || uuid::Uuid::parse_str(id).is_err() {
                return Err(failure("unsupported authorization session"));
            }
            let principal = address.principal;
            let source = source.clone();
            let state = declaration.state(None::<AuthorizationState>, 1, |state| {
                if serde_json::to_vec(state)
                    .map_err(|_| failure("authorization state could not be encoded"))?
                    .len()
                    > 24 * 1024
                {
                    return Err(failure(
                        "authorization state exceeds its presentation budget",
                    ));
                }
                Ok(())
            })?;
            let resource = declaration.resource_async(
                "provider.authorization",
                1,
                &[],
                move |_| {
                    let source = source.clone();
                    let principal = principal.clone();
                    let returning = returning.clone();
                    async move {
                        let login = source.begin_authorization(returning.as_deref()).await?;
                        Ok(Authorization {
                            login,
                            source,
                            principal,
                            busy: Cell::new(false),
                            cancelled: Cell::new(false),
                            pending: RefCell::new(None),
                            operation: RefCell::new(None),
                        })
                    }
                    .boxed_local()
                },
                |authorization, _| {
                    async move {
                        authorization.login.close().await;
                        Ok(())
                    }
                    .boxed_local()
                },
            )?;
            Ok((resource, state))
        })
        .lifetime(tembra::core::Lifetime::Referenced, 0)
        .method("poll", |env, context, (): ()| async move {
            env.0.value()?.advance(&context, &env.1, None, false).await
        })
        .method("retry", |env, context, (): ()| async move {
            env.0.value()?.advance(&context, &env.1, None, true).await
        })
        .method(
            "complete",
            |env, context, (callback,): (String,)| async move {
                env.0
                    .value()?
                    .advance(&context, &env.1, Some(callback), false)
                    .await
            },
        )
        .method("cancel", |env, context, (): ()| async move {
            let authorization = env.0.value()?;
            if env
                .1
                .value()?
                .as_ref()
                .as_ref()
                .is_some_and(|state| matches!(state, AuthorizationState::Connected { .. }))
            {
                return Ok(());
            }
            authorization.cancelled.set(true);
            authorization.login.cancel();
            if let Some(operation) = authorization.operation.take() {
                let _ = operation.cancel();
            }
            authorization.pending.take();
            env.1.update(&context, |_| {
                Ok(Some(AuthorizationState::Failed {
                    error: "Provider authorization was cancelled.".into(),
                    retryable: false,
                }))
            })?;
            Ok(())
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use futures::task::{LocalFutureObj, LocalSpawn, SpawnError};
    use jsonwebtoken::{EncodingKey, Header};
    use rsa::{
        RsaPrivateKey, pkcs1::EncodeRsaPrivateKey, rand_core::OsRng, traits::PublicKeyParts,
    };
    use serde_json::{Value, json};
    use std::{
        collections::BTreeMap,
        sync::{
            Arc, Mutex, OnceLock,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Duration,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        sync::Notify,
    };
    use zeroize::Zeroizing;

    #[derive(Clone, Copy)]
    struct Executor;
    impl LocalSpawn for Executor {
        fn spawn_local_obj(&self, future: LocalFutureObj<'static, ()>) -> Result<(), SpawnError> {
            tokio::task::spawn_local(future);
            Ok(())
        }
    }
    #[derive(Default)]
    struct Secrets {
        values: Mutex<BTreeMap<String, Zeroizing<String>>>,
        writes: AtomicUsize,
        manifest_writes: AtomicUsize,
        refuse_read: AtomicBool,
        fail_next_read: AtomicBool,
        block_failed_read: AtomicBool,
        failed_read_entered: Notify,
        block: AtomicBool,
        block_write: AtomicBool,
        refuse_write: AtomicBool,
        write_entered: Notify,
    }
    impl super::super::secrets::SecretStore for Secrets {
        fn get(&self, key: &str) -> tembra::Result<Zeroizing<String>> {
            if self.fail_next_read.swap(false, Ordering::SeqCst) {
                self.failed_read_entered.notify_one();
                while self.block_failed_read.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(1));
                }
                return Err(failure("fixture transient credential access failure"));
            }
            if self.refuse_read.load(Ordering::SeqCst) {
                return Err(failure("fixture credential read refused"));
            }
            while self.block.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
            self.values
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .ok_or_else(super::super::secrets::missing)
        }
        fn set(&self, key: &str, value: &str) -> tembra::Result<()> {
            self.write_entered.notify_one();
            if self.refuse_write.load(Ordering::SeqCst) {
                return Err(failure("fixture credential publication refused"));
            }
            while self.block_write.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
            self.writes.fetch_add(1, Ordering::SeqCst);
            if value.starts_with("swarmie.parts.v1:") {
                self.manifest_writes.fetch_add(1, Ordering::SeqCst);
            }
            self.values
                .lock()
                .unwrap()
                .insert(key.into(), Zeroizing::new(value.into()));
            Ok(())
        }
        fn delete(&self, key: &str) -> tembra::Result<()> {
            self.values.lock().unwrap().remove(key);
            Ok(())
        }
    }
    struct Keys {
        signer: EncodingKey,
        jwks: Value,
    }
    fn keys() -> &'static Keys {
        static KEYS: OnceLock<Keys> = OnceLock::new();
        KEYS.get_or_init(|| {
            let key = RsaPrivateKey::new(&mut OsRng, 2048).unwrap();
            Keys { signer: EncodingKey::from_rsa_der(key.to_pkcs1_der().unwrap().as_bytes()), jwks: json!({"keys":[{"kty":"RSA","kid":"native-fixture","use":"sig","alg":"RS256","n":URL_SAFE_NO_PAD.encode(key.n().to_bytes_be()),"e":URL_SAFE_NO_PAD.encode(key.e().to_bytes_be())}]}) }
        })
    }
    struct Fixture {
        origin: String,
        token: Arc<Mutex<String>>,
        catalogs: Arc<AtomicUsize>,
        exchanges: Arc<AtomicUsize>,
        fail: Arc<AtomicBool>,
        block: Arc<AtomicBool>,
        entered: Arc<Notify>,
        release: Arc<Notify>,
        task: tokio::task::AbortHandle,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    impl Fixture {
        async fn new() -> Self {
            let _ = keys();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let token = Arc::new(Mutex::new(String::new()));
            let catalogs = Arc::new(AtomicUsize::new(0));
            let exchanges = Arc::new(AtomicUsize::new(0));
            let fail = Arc::new(AtomicBool::new(false));
            let block = Arc::new(AtomicBool::new(false));
            let entered = Arc::new(Notify::new());
            let release = Arc::new(Notify::new());
            let (t, c, e, f, b, n, r) = (
                token.clone(),
                catalogs.clone(),
                exchanges.clone(),
                fail.clone(),
                block.clone(),
                entered.clone(),
                release.clone(),
            );
            let task = tokio::spawn(async move {
                loop {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        return;
                    };
                    let (t, c, e, f, b, n, r) = (
                        t.clone(),
                        c.clone(),
                        e.clone(),
                        f.clone(),
                        b.clone(),
                        n.clone(),
                        r.clone(),
                    );
                    tokio::spawn(async move {
                        let mut bytes = Vec::new();
                        loop {
                            let mut buffer = [0; 4096];
                            let count = stream.read(&mut buffer).await.unwrap();
                            if count == 0 {
                                return;
                            }
                            bytes.extend_from_slice(&buffer[..count]);
                            if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                                let headers = String::from_utf8_lossy(&bytes[..end]);
                                let length = headers
                                    .lines()
                                    .find_map(|line| {
                                        line.split_once(':')
                                            .filter(|(key, _)| {
                                                key.eq_ignore_ascii_case("content-length")
                                            })
                                            .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                                    })
                                    .unwrap_or(0);
                                if bytes.len() >= end + 4 + length {
                                    break;
                                }
                            }
                        }
                        let first = String::from_utf8_lossy(&bytes)
                            .lines()
                            .next()
                            .unwrap()
                            .to_owned();
                        let (status, body) = if first.starts_with("GET /jwks ") {
                            (200, keys().jwks.to_string())
                        } else if first.starts_with("POST /token ") {
                            e.fetch_add(1, Ordering::SeqCst);
                            (200, t.lock().unwrap().clone())
                        } else {
                            c.fetch_add(1, Ordering::SeqCst);
                            n.notify_one();
                            if b.load(Ordering::SeqCst) {
                                r.notified().await;
                            }
                            if f.load(Ordering::SeqCst) {
                                (503, "{}".into())
                            } else {
                                (200, json!({"models":[{"slug":"fixture-model","visibility":"list","display_name":"  ","limit":{"context":128000,"output":8192},"swarmie_capabilities":{"profile":"swarmie.text-tools.v1","tools":true,"streaming":true},"supported_reasoning_levels":[{"effort":"medium"}]},{"slug":"fixture-model","visibility":"list"}]}).to_string())
                            }
                        };
                        let _ = stream.write_all(format!("HTTP/1.1 {status} Fixture\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
                    });
                }
            });
            Self {
                origin,
                token,
                catalogs,
                exchanges,
                fail,
                block,
                entered,
                release,
                task: task.abort_handle(),
            }
        }
        fn options(&self) -> swarmie_providers::auth::ChatgptBrowserOptions {
            swarmie_providers::auth::ChatgptBrowserOptions {
                browser: swarmie_providers::auth::BrowserOptions {
                    authorization_url: format!("{}/authorize", self.origin),
                    token_url: format!("{}/token", self.origin),
                    callback_port: 0,
                    timeout: Duration::from_secs(10),
                },
                jwks_url: format!("{}/jwks", self.origin),
            }
        }
        fn callback(&self, reference: &tembra::Reference) -> String {
            self.callback_with_subject(reference, "native-fixture-person")
        }
        fn callback_with_subject(&self, reference: &tembra::Reference, subject: &str) -> String {
            let state = reference
                .state::<Option<AuthorizationState>>()
                .unwrap()
                .value()
                .unwrap();
            let Some(AuthorizationState::Waiting {
                authorization_url,
                callback_uri: Some(callback),
                ..
            }) = state.as_ref()
            else {
                panic!("authorization is not waiting");
            };
            let fields: BTreeMap<String, String> = reqwest::Url::parse(authorization_url)
                .unwrap()
                .query_pairs()
                .into_owned()
                .collect();
            let claims = json!({"iss":"https://auth.openai.com","sub":subject,"aud":"oaiapp_fixture","nonce":fields["nonce"],"exp":swarmie_providers::auth::now_ms()/1000+3600,"iat":swarmie_providers::auth::now_ms()/1000,"email":format!("{}@example.test","a".repeat(520)),"email_verified":true});
            let mut header = Header::new(jsonwebtoken::Algorithm::RS256);
            header.kid = Some("native-fixture".into());
            let signed = jsonwebtoken::encode(&header, &claims, &keys().signer).unwrap();
            *self.token.lock().unwrap() = json!({"access_token":"native-fixture-access","refresh_token":"native-fixture-refresh","expires_in":3600,"token_type":"Bearer","scope":"openid profile email offline_access resource.invoke chatgpt.tokens.use.direct","id_token":signed}).to_string();
            let mut callback = reqwest::Url::parse(callback).unwrap();
            callback
                .query_pairs_mut()
                .append_pair("state", &fields["state"])
                .append_pair("code", "single-use-fixture")
                .append_pair("client_id", "oaiapp_fixture");
            callback.into()
        }
    }
    async fn call(
        host: &tembra::Host,
        reference: Rc<tembra::Reference>,
        method: &'static str,
        args: Vec<Value>,
    ) -> tembra::Result<()> {
        host.root("fixture", move |context| {
            async move {
                if args.is_empty() {
                    reference.call(&context, method, ()).await
                } else {
                    let callback = args[0]
                        .as_str()
                        .expect("fixture callback string")
                        .to_owned();
                    reference.call(&context, method, (callback,)).await
                }
            }
            .boxed_local()
        })
        .await
    }
    async fn setup(
        fixture: &Fixture,
        path: std::path::PathBuf,
        secrets: Arc<dyn super::super::secrets::SecretStore>,
    ) -> (tembra::Host, Rc<NativeProviders>, Rc<tembra::Reference>) {
        let mut source = NativeProviders::new(
            path,
            "embedded-fixture".into(),
            "fixture".into(),
            uuid::Uuid::new_v4(),
        );
        source.secrets = secrets;
        source.oauth_options = Some((fixture.options(), fixture.origin.clone()));
        let source = Rc::new(source);
        let host = tembra::Host::new(Executor);
        host.install(vec![orb(source.clone())]).unwrap();
        let reference = Rc::new(
            host.get(
                SERVICE,
                vec!["openai".into(), uuid::Uuid::new_v4().to_string()],
                "fixture",
            )
            .unwrap(),
        );
        call(&host, reference.clone(), "poll", vec![])
            .await
            .unwrap();
        (host, source, reference)
    }

    #[tokio::test]
    async fn native_oauth_retry_keeps_single_delivered_credential_and_saves_once() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let fixture = Fixture::new().await;
                fixture.fail.store(true, Ordering::SeqCst);
                let directory = tempfile::tempdir().unwrap();
                let secrets = Arc::new(Secrets::default());
                let (host, source, reference) = setup(
                    &fixture,
                    directory.path().join("providers.json"),
                    secrets.clone(),
                )
                .await;
                call(
                    &host,
                    reference.clone(),
                    "complete",
                    vec![json!(fixture.callback(&reference))],
                )
                .await
                .unwrap();
                assert!(matches!(
                    reference
                        .state::<Option<AuthorizationState>>()
                        .unwrap()
                        .value()
                        .unwrap()
                        .as_ref(),
                    Some(AuthorizationState::Failed {
                        retryable: true,
                        ..
                    })
                ));
                assert_eq!(secrets.writes.load(Ordering::SeqCst), 0);
                assert!(!source.path.exists());
                call(&host, reference.clone(), "poll", vec![])
                    .await
                    .unwrap();
                assert_eq!(
                    fixture.catalogs.load(Ordering::SeqCst),
                    1,
                    "timer poll must not retry"
                );
                fixture.fail.store(false, Ordering::SeqCst);
                call(&host, reference.clone(), "retry", vec![])
                    .await
                    .unwrap();
                let state = reference
                    .state::<Option<AuthorizationState>>()
                    .unwrap()
                    .value()
                    .unwrap();
                let Some(AuthorizationState::Connected { account }) = state.as_ref() else {
                    panic!("retry did not connect");
                };
                let credential = source
                    .returning_credential(&account.id)
                    .await
                    .unwrap()
                    .unwrap();
                let stored = swarmie_providers::auth::encode(&credential).unwrap();
                assert!(source.decode_oauth(&stored, None).is_some());
                let foreign = NativeProviders::new(
                    source.path.clone(),
                    source.node.clone(),
                    "fixture".into(),
                    uuid::Uuid::new_v4(),
                );
                assert!(foreign.decode_oauth(&stored, None).is_none());
                assert_eq!(account.models.len(), 1);
                assert_eq!(account.models[0].label, "fixture-model");
                assert!(account.label.len() <= 512);
                assert_eq!(secrets.writes.load(Ordering::SeqCst), 1);
                assert_eq!(
                    fixture.exchanges.load(Ordering::SeqCst),
                    1,
                    "authorization code must not be exchanged twice"
                );
                call(&host, reference.clone(), "retry", vec![])
                    .await
                    .unwrap();
                call(&host, reference.clone(), "cancel", vec![])
                    .await
                    .unwrap();
                assert!(matches!(
                    reference
                        .state::<Option<AuthorizationState>>()
                        .unwrap()
                        .value()
                        .unwrap()
                        .as_ref(),
                    Some(AuthorizationState::Connected { .. })
                ));
                assert_eq!(source.read().await.unwrap().len(), 1);
                assert_eq!(secrets.writes.load(Ordering::SeqCst), 1);
                // A targeted authorization reuses the verified account's issued registration.
                let second = Rc::new(
                    host.get(
                        SERVICE,
                        vec![
                            "openai".into(),
                            uuid::Uuid::new_v4().to_string(),
                            account.id.clone(),
                        ],
                        "fixture",
                    )
                    .unwrap(),
                );
                call(&host, second.clone(), "poll", vec![]).await.unwrap();
                let waiting = second
                    .state::<Option<AuthorizationState>>()
                    .unwrap()
                    .value()
                    .unwrap();
                let Some(AuthorizationState::Waiting {
                    authorization_url, ..
                }) = waiting.as_ref()
                else {
                    panic!("reauthorization not waiting");
                };
                assert!(
                    reqwest::Url::parse(authorization_url)
                        .unwrap()
                        .query_pairs()
                        .any(|(key, value)| key == "client_id" && value == "oaiapp_fixture")
                );
                call(
                    &host,
                    second.clone(),
                    "complete",
                    vec![json!(fixture.callback(&second))],
                )
                .await
                .unwrap();
                assert_eq!(
                    source.read().await.unwrap().len(),
                    1,
                    "the same verified account must not create duplicate rows"
                );
                // Selecting an unqualified inference profile must not renew or
                // read credentials. Authorization remains independently usable.
                let exchanged = fixture.exchanges.load(Ordering::SeqCst);
                let writes = secrets.writes.load(Ordering::SeqCst);
                let catalogs = fixture.catalogs.load(Ordering::SeqCst);
                let (first, second_model) = futures::join!(
                    source.model("fixture", &account.id, "fixture-model", "medium"),
                    source.model("fixture", &account.id, "fixture-model", "medium")
                );
                assert!(first.is_err() && second_model.is_err());
                assert_eq!(fixture.exchanges.load(Ordering::SeqCst), exchanged);
                assert_eq!(fixture.catalogs.load(Ordering::SeqCst), catalogs);
                assert_eq!(secrets.writes.load(Ordering::SeqCst), writes);
                drop(second);
                drop(reference);
                host.close().await.unwrap();
            })
            .await;
    }

    #[tokio::test]
    async fn cancellation_during_native_catalog_request_writes_no_secrets_or_metadata() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let fixture = Fixture::new().await;
                fixture.block.store(true, Ordering::SeqCst);
                let directory = tempfile::tempdir().unwrap();
                let secrets = Arc::new(Secrets::default());
                let (host, source, reference) = setup(
                    &fixture,
                    directory.path().join("providers.json"),
                    secrets.clone(),
                )
                .await;
                let pending = call(
                    &host,
                    reference.clone(),
                    "complete",
                    vec![json!(fixture.callback(&reference))],
                );
                let cancel = async {
                    fixture.entered.notified().await;
                    call(&host, reference.clone(), "cancel", vec![])
                        .await
                        .unwrap();
                    fixture.release.notify_one();
                };
                let (result, ()) = futures::join!(pending, cancel);
                assert!(result.is_err());
                assert_eq!(secrets.writes.load(Ordering::SeqCst), 0);
                assert!(!source.path.exists());
                assert!(matches!(
                    reference
                        .state::<Option<AuthorizationState>>()
                        .unwrap()
                        .value()
                        .unwrap()
                        .as_ref(),
                    Some(AuthorizationState::Failed {
                        retryable: false,
                        ..
                    })
                ));
                drop(reference);
                host.close().await.unwrap();
            })
            .await;
    }

    #[tokio::test]
    async fn cancellation_after_a_credential_write_starts_settles_metadata_without_publishing_connected()
     {
        tokio::task::LocalSet::new()
            .run_until(async {
                let fixture = Fixture::new().await;
                let directory = tempfile::tempdir().unwrap();
                let secrets = Arc::new(Secrets::default());
                secrets.block_write.store(true, Ordering::SeqCst);
                let (host, source, reference) = setup(
                    &fixture,
                    directory.path().join("providers.json"),
                    secrets.clone(),
                )
                .await;
                let pending = call(
                    &host,
                    reference.clone(),
                    "complete",
                    vec![json!(fixture.callback(&reference))],
                );
                let cancel = async {
                    secrets.write_entered.notified().await;
                    call(&host, reference.clone(), "cancel", vec![])
                        .await
                        .unwrap();
                    secrets.block_write.store(false, Ordering::SeqCst);
                };
                let (result, ()) = futures::join!(pending, cancel);
                assert!(result.is_err());
                assert_eq!(secrets.writes.load(Ordering::SeqCst), 1);
                assert_eq!(
                    source.read().await.unwrap().len(),
                    1,
                    "a physical credential write must finish its metadata commit"
                );
                assert!(matches!(
                    reference
                        .state::<Option<AuthorizationState>>()
                        .unwrap()
                        .value()
                        .unwrap()
                        .as_ref(),
                    Some(AuthorizationState::Failed {
                        retryable: false,
                        ..
                    })
                ));
                drop(reference);
                host.close().await.unwrap();
            })
            .await;
    }

    fn connected_account(reference: &tembra::Reference) -> Account {
        let state = reference
            .state::<Option<AuthorizationState>>()
            .unwrap()
            .value()
            .unwrap();
        let Some(AuthorizationState::Connected { account }) = state.as_ref() else {
            panic!("the native authorization did not connect");
        };
        account.clone()
    }
    #[tokio::test]
    async fn fresh_native_sign_in_replaces_a_corrupt_or_older_credential_after_one_catalog_retry() {
        tokio::task::LocalSet::new()
            .run_until(async {
                for corrupt in [
                    "swarmie.parts.v1:corrupt",
                    "older-single-entry",
                    "invalid digest",
                ] {
                    let fixture = Fixture::new().await;
                    let directory = tempfile::tempdir().unwrap();
                    let raw = Arc::new(Secrets::default());
                    let store = Arc::new(super::super::secrets::ChunkedStore::new(raw.clone()));
                    let (host, source, first) =
                        setup(&fixture, directory.path().join("providers.json"), store).await;
                    call(
                        &host,
                        first.clone(),
                        "complete",
                        vec![json!(fixture.callback(&first))],
                    )
                    .await
                    .unwrap();
                    let original = connected_account(&first);
                    let corrupt = if corrupt == "invalid digest" {
                        let previous = raw.values.lock().unwrap()[&original.id].clone();
                        format!(
                            "{}:{}",
                            previous.rsplit_once(':').unwrap().0,
                            "0".repeat(64)
                        )
                    } else {
                        corrupt.to_owned()
                    };
                    raw.values
                        .lock()
                        .unwrap()
                        .insert(original.id.clone(), Zeroizing::new(corrupt));
                    raw.values.lock().unwrap().insert(
                        "unrelated".into(),
                        Zeroizing::new("unrelated synthetic value".into()),
                    );
                    let fresh = Rc::new(
                        host.get(
                            SERVICE,
                            vec!["openai".into(), uuid::Uuid::new_v4().to_string()],
                            "fixture",
                        )
                        .unwrap(),
                    );
                    call(&host, fresh.clone(), "poll", vec![]).await.unwrap();
                    fixture.fail.store(true, Ordering::SeqCst);
                    call(
                        &host,
                        fresh.clone(),
                        "complete",
                        vec![json!(fixture.callback(&fresh))],
                    )
                    .await
                    .unwrap();
                    assert_eq!(raw.manifest_writes.load(Ordering::SeqCst), 1);
                    call(&host, fresh.clone(), "poll", vec![]).await.unwrap();
                    fixture.fail.store(false, Ordering::SeqCst);
                    call(&host, fresh.clone(), "retry", vec![]).await.unwrap();
                    let repaired = connected_account(&fresh);
                    assert_eq!(repaired.id, original.id);
                    assert_eq!(source.read().await.unwrap().len(), 1);
                    assert_eq!(
                        raw.manifest_writes.load(Ordering::SeqCst),
                        2,
                        "the replacement must publish once"
                    );
                    assert_eq!(
                        fixture.exchanges.load(Ordering::SeqCst),
                        2,
                        "retry must retain its delivered credential"
                    );
                    call(&host, fresh.clone(), "retry", vec![]).await.unwrap();
                    assert_eq!(raw.manifest_writes.load(Ordering::SeqCst), 2);
                    let restored = super::super::secrets::get(source.secrets.clone(), repaired.id)
                        .await
                        .unwrap();
                    assert!(
                        swarmie_providers::auth::decode(&restored)
                            .unwrap()
                            .is_some()
                    );
                    assert_eq!(
                        raw.values.lock().unwrap()["unrelated"].as_str(),
                        "unrelated synthetic value"
                    );
                    drop(first);
                    drop(fresh);
                    host.close().await.unwrap();
                }
            })
            .await;
    }
    #[tokio::test]
    async fn native_reconnect_uses_fresh_authorization_when_the_stored_credential_is_unavailable() {
        tokio::task::LocalSet::new()
            .run_until(async {
                for unavailable in ["missing", "corrupt", "read refused"] {
                    let fixture = Fixture::new().await;
                    let directory = tempfile::tempdir().unwrap();
                    let raw = Arc::new(Secrets::default());
                    let store = Arc::new(super::super::secrets::ChunkedStore::new(raw.clone()));
                    let (host, source, first) =
                        setup(&fixture, directory.path().join("providers.json"), store).await;
                    call(
                        &host,
                        first.clone(),
                        "complete",
                        vec![json!(fixture.callback(&first))],
                    )
                    .await
                    .unwrap();
                    let original = connected_account(&first);
                    match unavailable {
                        "missing" => {
                            raw.values.lock().unwrap().remove(&original.id);
                        }
                        "corrupt" => {
                            raw.values.lock().unwrap().insert(
                                original.id.clone(),
                                Zeroizing::new("swarmie.parts.v1:corrupt".into()),
                            );
                        }
                        _ => {
                            raw.refuse_read.store(true, Ordering::SeqCst);
                        }
                    }
                    let reconnect = Rc::new(
                        host.get(
                            SERVICE,
                            vec![
                                "openai".into(),
                                uuid::Uuid::new_v4().to_string(),
                                original.id.clone(),
                            ],
                            "fixture",
                        )
                        .unwrap(),
                    );
                    call(&host, reconnect.clone(), "poll", vec![])
                        .await
                        .unwrap();
                    let waiting = reconnect
                        .state::<Option<AuthorizationState>>()
                        .unwrap()
                        .value()
                        .unwrap();
                    let Some(AuthorizationState::Waiting {
                        authorization_url, ..
                    }) = waiting.as_ref()
                    else {
                        panic!("reconnect did not offer fresh authorization");
                    };
                    assert!(
                        reqwest::Url::parse(authorization_url)
                            .unwrap()
                            .query_pairs()
                            .any(|(key, value)| key == "client_id"
                                && value == "dynamic_agent_client")
                    );
                    call(
                        &host,
                        reconnect.clone(),
                        "complete",
                        vec![json!(fixture.callback(&reconnect))],
                    )
                    .await
                    .unwrap();
                    raw.refuse_read.store(false, Ordering::SeqCst);
                    if unavailable == "read refused" {
                        assert!(matches!(
                            reconnect
                                .state::<Option<AuthorizationState>>()
                                .unwrap()
                                .value()
                                .unwrap()
                                .as_ref(),
                            Some(AuthorizationState::Failed {
                                retryable: true,
                                ..
                            })
                        ));
                        assert_eq!(raw.manifest_writes.load(Ordering::SeqCst), 1);
                        call(&host, reconnect.clone(), "retry", vec![])
                            .await
                            .unwrap();
                    }
                    assert_eq!(connected_account(&reconnect).id, original.id);
                    assert_eq!(source.read().await.unwrap().len(), 1);
                    assert_eq!(raw.manifest_writes.load(Ordering::SeqCst), 2);
                    drop(first);
                    drop(reconnect);
                    host.close().await.unwrap();
                }
            })
            .await;
    }

    #[tokio::test]
    async fn readable_native_reconnect_keeps_the_verified_identity_bound_to_the_registration() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let fixture = Fixture::new().await;
                let directory = tempfile::tempdir().unwrap();
                let raw = Arc::new(Secrets::default());
                let store = Arc::new(super::super::secrets::ChunkedStore::new(raw.clone()));
                let (host, source, first) =
                    setup(&fixture, directory.path().join("providers.json"), store).await;
                call(
                    &host,
                    first.clone(),
                    "complete",
                    vec![json!(fixture.callback(&first))],
                )
                .await
                .unwrap();
                let original = connected_account(&first);
                let previous =
                    super::super::secrets::get(source.secrets.clone(), original.id.clone())
                        .await
                        .unwrap();
                let reconnect = Rc::new(
                    host.get(
                        SERVICE,
                        vec![
                            "openai".into(),
                            uuid::Uuid::new_v4().to_string(),
                            original.id.clone(),
                        ],
                        "fixture",
                    )
                    .unwrap(),
                );
                call(&host, reconnect.clone(), "poll", vec![])
                    .await
                    .unwrap();
                assert!(
                    call(
                        &host,
                        reconnect.clone(),
                        "complete",
                        vec![json!(fixture.callback_with_subject(
                            &reconnect,
                            "different-fixture-person"
                        ))]
                    )
                    .await
                    .is_err()
                );
                assert_eq!(source.read().await.unwrap().len(), 1);
                let unchanged = super::super::secrets::get(source.secrets.clone(), original.id)
                    .await
                    .unwrap();
                assert!(
                    previous.as_str() == unchanged.as_str(),
                    "the stored credential changed"
                );
                assert_eq!(raw.manifest_writes.load(Ordering::SeqCst), 1);
                drop(first);
                drop(reconnect);
                host.close().await.unwrap();
            })
            .await;
    }

    #[tokio::test]
    async fn failed_metadata_commit_restores_only_a_readable_previous_oauth_credential() {
        tokio::task::LocalSet::new()
            .run_until(async {
                for corrupt in [false, true] {
                    let fixture = Fixture::new().await;
                    let directory = tempfile::tempdir().unwrap();
                    let raw = Arc::new(Secrets::default());
                    let store = Arc::new(super::super::secrets::ChunkedStore::new(raw.clone()));
                    let (host, source, first) =
                        setup(&fixture, directory.path().join("providers.json"), store).await;
                    call(
                        &host,
                        first.clone(),
                        "complete",
                        vec![json!(fixture.callback(&first))],
                    )
                    .await
                    .unwrap();
                    let original = connected_account(&first);
                    let previous =
                        super::super::secrets::get(source.secrets.clone(), original.id.clone())
                            .await
                            .unwrap();
                    if corrupt {
                        raw.values.lock().unwrap().insert(
                            original.id.clone(),
                            Zeroizing::new("swarmie.parts.v1:corrupt".into()),
                        );
                    }
                    raw.values.lock().unwrap().insert(
                        "unrelated".into(),
                        Zeroizing::new("unrelated synthetic value".into()),
                    );
                    let fresh = Rc::new(
                        host.get(
                            SERVICE,
                            vec!["openai".into(), uuid::Uuid::new_v4().to_string()],
                            "fixture",
                        )
                        .unwrap(),
                    );
                    call(&host, fresh.clone(), "poll", vec![]).await.unwrap();
                    raw.write_entered.notified().await;
                    raw.block_write.store(true, Ordering::SeqCst);
                    let pending = call(
                        &host,
                        fresh.clone(),
                        "complete",
                        vec![json!(fixture.callback(&fresh))],
                    );
                    let backup = directory.path().join("metadata-backup.json");
                    let fail_metadata = async {
                        raw.write_entered.notified().await;
                        std::fs::rename(&source.path, &backup).unwrap();
                        std::fs::create_dir(&source.path).unwrap();
                        raw.block_write.store(false, Ordering::SeqCst);
                    };
                    let (result, ()) = futures::join!(pending, fail_metadata);
                    result.unwrap();
                    assert!(matches!(
                        fresh
                            .state::<Option<AuthorizationState>>()
                            .unwrap()
                            .value()
                            .unwrap()
                            .as_ref(),
                        Some(AuthorizationState::Failed {
                            retryable: true,
                            ..
                        })
                    ));
                    let after =
                        super::super::secrets::get(source.secrets.clone(), original.id.clone())
                            .await;
                    if corrupt {
                        assert!(after.is_err(), "an unreadable value is not a restore point");
                    } else {
                        assert!(
                            after.unwrap().as_str() == previous.as_str(),
                            "rollback must restore the readable credential"
                        );
                    }
                    assert_eq!(
                        raw.values.lock().unwrap()["unrelated"].as_str(),
                        "unrelated synthetic value"
                    );
                    std::fs::remove_dir(&source.path).unwrap();
                    std::fs::rename(&backup, &source.path).unwrap();
                    call(&host, fresh.clone(), "retry", vec![]).await.unwrap();
                    assert_eq!(connected_account(&fresh).id, original.id);
                    assert_eq!(
                        fixture.exchanges.load(Ordering::SeqCst),
                        2,
                        "metadata retry must retain its delivered authorization"
                    );
                    drop(first);
                    drop(fresh);
                    host.close().await.unwrap();
                }
            })
            .await;
    }

    #[tokio::test]
    async fn transient_store_read_failure_before_a_metadata_failure_preserves_the_old_credential() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let fixture = Fixture::new().await;
                let directory = tempfile::tempdir().unwrap();
                let raw = Arc::new(Secrets::default());
                let store = Arc::new(super::super::secrets::ChunkedStore::new(raw.clone()));
                let (host, source, first) =
                    setup(&fixture, directory.path().join("providers.json"), store).await;
                call(
                    &host,
                    first.clone(),
                    "complete",
                    vec![json!(fixture.callback(&first))],
                )
                .await
                .unwrap();
                let original = connected_account(&first);
                let previous = raw.values.lock().unwrap()[&original.id].clone();
                let keys = raw
                    .values
                    .lock()
                    .unwrap()
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>();
                let fresh = Rc::new(
                    host.get(
                        SERVICE,
                        vec!["openai".into(), uuid::Uuid::new_v4().to_string()],
                        "fixture",
                    )
                    .unwrap(),
                );
                call(&host, fresh.clone(), "poll", vec![]).await.unwrap();
                raw.fail_next_read.store(true, Ordering::SeqCst);
                raw.block_failed_read.store(true, Ordering::SeqCst);
                let pending = call(
                    &host,
                    fresh.clone(),
                    "complete",
                    vec![json!(fixture.callback(&fresh))],
                );
                let backup = directory.path().join("metadata-backup.json");
                let fail_metadata = async {
                    raw.failed_read_entered.notified().await;
                    std::fs::rename(&source.path, &backup).unwrap();
                    std::fs::create_dir(&source.path).unwrap();
                    raw.block_failed_read.store(false, Ordering::SeqCst);
                };
                let (result, ()) = futures::join!(pending, fail_metadata);
                result.unwrap();
                assert!(matches!(
                    fresh
                        .state::<Option<AuthorizationState>>()
                        .unwrap()
                        .value()
                        .unwrap()
                        .as_ref(),
                    Some(AuthorizationState::Failed {
                        retryable: true,
                        ..
                    })
                ));
                assert!(
                    raw.values
                        .lock()
                        .unwrap()
                        .get(&original.id)
                        .is_some_and(|value| value.as_str() == previous.as_str()),
                    "a transient access failure must not replace or delete the old manifest"
                );
                assert_eq!(
                    raw.values
                        .lock()
                        .unwrap()
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>(),
                    keys,
                    "the old parts must be retained"
                );
                assert_eq!(
                    raw.manifest_writes.load(Ordering::SeqCst),
                    1,
                    "no new credential can be published without a known restore point"
                );
                std::fs::remove_dir(&source.path).unwrap();
                std::fs::rename(&backup, &source.path).unwrap();
                call(&host, fresh.clone(), "retry", vec![]).await.unwrap();
                assert_eq!(connected_account(&fresh).id, original.id);
                assert_eq!(
                    fixture.exchanges.load(Ordering::SeqCst),
                    2,
                    "retry must retain the delivered authorization"
                );
                assert_eq!(raw.manifest_writes.load(Ordering::SeqCst), 2);
                drop(first);
                drop(fresh);
                host.close().await.unwrap();
            })
            .await;
    }

    #[tokio::test]
    async fn a_blocked_secret_store_does_not_block_the_node_executor() {
        let store = Arc::new(Secrets::default());
        store.block.store(true, Ordering::SeqCst);
        let blocked = super::super::secrets::get(store.clone(), "fixture".into());
        let unblock = async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            store.block.store(false, Ordering::SeqCst);
        };
        tokio::time::timeout(Duration::from_secs(1), async {
            let (_, ()) = futures::join!(blocked, unblock);
        })
        .await
        .unwrap();
    }
}
