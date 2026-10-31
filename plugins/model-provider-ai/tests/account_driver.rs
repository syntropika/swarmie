use std::time::Duration;
use swarmie_model_provider_ai::{AiAccountDriver, AiModelFactory, ModelProtection};
use swarmie_provider_auth::{
    ChatgptBrowserOptions, ChatgptRegistration, Credential, Provider, VerifiedIdentity,
};
use swarmie_providers::{
    AccountRecord, CredentialRequirement, ModelFactory, ModelSelection,
    accounts::{AccountCandidate, AccountDriver},
    settings::ConfigureProvider,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;
use zeroize::Zeroizing;

fn issued(scope: Uuid) -> Credential {
    Credential {
        provider: Provider::Chatgpt,
        access_token: "fixture-access-token".to_owned().into(),
        refresh_token: "fixture-refresh-token".to_owned().into(),
        expires_at_ms: u64::MAX,
        endpoint: "https://api.openai.com/v1/".into(),
        chatgpt: Some(Box::new(ChatgptRegistration {
            client_id: "issued-client".into(),
            identity: VerifiedIdentity {
                issuer: "https://auth.openai.com".into(),
                subject: "fixture-subject".into(),
                email: Some("person@example.test".into()),
            },
            host_id: format!("urn:uuid:{scope}"),
            id_token: "fixture-id-token".to_owned().into(),
            scopes: vec!["chatgpt.tokens.use.direct".into()],
            nonce: "fixture-nonce".to_owned().into(),
        })),
    }
}

#[test]
fn protocol_binding_tracks_every_injected_option_without_starting_io() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let scope = Uuid::new_v4();
    let mut options = ChatgptBrowserOptions::default();
    options.browser.authorization_url = format!("{endpoint}/public-authorization");
    options.browser.token_url = format!("{endpoint}/token");
    options.jwks_url = format!("{endpoint}/jwks");
    options.browser.callback_port = 0;
    let catalog = Some(format!("{endpoint}/models"));
    let binding = |scope, options, catalog| {
        AiAccountDriver::with_options(scope, options, catalog)
            .unwrap()
            .protocol_binding()
            .unwrap()
    };
    let original = binding(scope, options.clone(), catalog.clone());
    assert!(binding(scope, options.clone(), catalog.clone()) == original);
    assert!(binding(Uuid::new_v4(), options.clone(), catalog.clone()) != original);
    assert!(binding(scope, options.clone(), None) != original);
    assert!(
        binding(
            scope,
            options.clone(),
            Some(format!("{endpoint}/other-models"))
        ) != original
    );
    for changed in 0..6 {
        let mut changed_options = options.clone();
        match changed {
            0 => changed_options.browser.authorization_url.push_str("/other"),
            1 => changed_options.browser.token_url.push_str("/other"),
            2 => changed_options.jwks_url.push_str("/other"),
            3 => changed_options.browser.callback_port = 1,
            4 => changed_options.browser.timeout += Duration::from_secs(1),
            5 => changed_options.browser.timeout += Duration::from_nanos(1),
            _ => unreachable!(),
        }
        assert!(binding(scope, changed_options, catalog.clone()) != original);
    }
    let wire = serde_json::to_string(&original).unwrap();
    assert!(!wire.contains(&endpoint));
    assert!(!wire.contains(&scope.to_string()));
    let restored: swarmie_providers::ProtocolBinding = serde_json::from_str(&wire).unwrap();
    assert!(restored == original);
    assert_eq!(original.as_bytes().len(), 32);
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

async fn server(body: serde_json::Value, status: u16) -> (String, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let size = tokio::time::timeout(Duration::from_secs(3), socket.read(&mut buffer))
                .await
                .unwrap()
                .unwrap();
            assert_ne!(size, 0);
            bytes.extend_from_slice(&buffer[..size]);
            assert!(bytes.len() < 32 * 1024);
            if let Some(end) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
                let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if bytes.len() >= end + 4 + length {
                    break;
                }
            }
        }
        let body = body.to_string();
        socket.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        String::from_utf8(bytes).unwrap()
    });
    (endpoint, task)
}

async fn authorized(
    scope: Uuid,
    credential: &Zeroizing<String>,
) -> (AiAccountDriver, AccountCandidate) {
    let (endpoint,request)=server(serde_json::json!({"models":[{"slug":"fixture-model","visibility":"list","display_name":"Fixture model","supported_reasoning_levels":[{"effort":"medium"}]}]}),200).await;
    let driver = AiAccountDriver::with_options(
        scope,
        ChatgptBrowserOptions::default(),
        Some(endpoint.clone()),
    )
    .unwrap();
    let candidate = driver
        .accept_authorization("openai", credential)
        .await
        .unwrap();
    let request = request.await.unwrap();
    assert!(request.starts_with("GET /models HTTP/1.1"));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer fixture-access-token")
    );
    assert_eq!(
        candidate.record.private_configuration()["discovered_limits"]["endpoint"],
        endpoint
    );
    (driver, candidate)
}

#[tokio::test(flavor = "current_thread")]
async fn renewal_decision_uses_the_validated_capture_and_supplied_time_without_an_exchange() {
    let scope = Uuid::new_v4();
    let encoded = swarmie_provider_auth::encode(&issued(scope)).unwrap();
    let (_, candidate) = authorized(scope, &encoded).await;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let mut options = ChatgptBrowserOptions::default();
    options.browser.token_url = format!("{endpoint}/token");
    options.jwks_url = format!("{endpoint}/jwks");
    let driver = AiAccountDriver::with_options(scope, options, None).unwrap();
    for (expiry, now, expected) in [
        (0, 0, true),
        (60_000, 0, true),
        (60_001, 0, false),
        (u64::MAX, u64::MAX - 60_001, false),
        (u64::MAX, u64::MAX - 60_000, true),
        (u64::MAX, u64::MAX, true),
    ] {
        let mut credential = issued(scope);
        credential.expires_at_ms = expiry;
        let encoded = swarmie_provider_auth::encode(&credential).unwrap();
        assert_eq!(
            driver
                .needs_renewal(&candidate.record, &encoded, now)
                .unwrap(),
            expected
        );
    }
    let foreign = swarmie_provider_auth::encode(&issued(Uuid::new_v4())).unwrap();
    assert!(
        driver
            .needs_renewal(&candidate.record, &foreign, 0)
            .is_err()
    );
    let mut foreign = issued(scope);
    foreign.chatgpt.as_mut().unwrap().identity.subject = "unrelated-subject".into();
    let foreign = swarmie_provider_auth::encode(&foreign).unwrap();
    assert!(
        driver
            .needs_renewal(&candidate.record, &foreign, 0)
            .is_err()
    );
    let (mut public, private) = candidate.record.clone().into_parts();
    public.provider_id = "anthropic".into();
    let foreign = AccountRecord::new(public, private).unwrap();
    assert!(driver.needs_renewal(&foreign, &encoded, 0).is_err());
    let local: AccountRecord = serde_json::from_value(serde_json::json!({
        "account":{"id":"local","providerId":"ollama","providerLabel":"Local","label":"Local","models":[],"available":true},
        "provider":"ollama","endpoint":"http://localhost:11434","oauth":false
    })).unwrap();
    assert!(driver.needs_renewal(&local, &encoded, 0).is_err());
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[tokio::test(flavor = "current_thread")]
async fn local_configuration_discovers_without_custody_and_does_not_allocate_an_account_id() {
    let (endpoint,request)=server(serde_json::json!({"models":[{"name":"fixture-local","display_name":"Local model","unrelated":"private catalog data"}]}),200).await;
    let driver = AiAccountDriver::new(Uuid::new_v4()).unwrap();
    let candidate = driver
        .configure(ConfigureProvider {
            provider_id: "ollama".into(),
            method: "local".into(),
            credential: Some("unused-local-fixture-key".into()),
            url: Some(endpoint.clone()),
        })
        .await
        .unwrap();
    let request = request.await.unwrap();
    assert!(request.starts_with("GET /api/tags HTTP/1.1"));
    assert!(!request.to_ascii_lowercase().contains("authorization:"));
    assert!(!request.contains("unused-local-fixture-key"));
    assert!(candidate.credential.is_none());
    assert!(candidate.association_key.is_none());
    assert!(candidate.record.account.id.is_empty());
    assert_eq!(candidate.record.account.models[0].id, "fixture-local");
    assert_eq!(
        driver.credential_requirement(&candidate.record).unwrap(),
        CredentialRequirement::None
    );
    let encoded = serde_json::to_string(&candidate.record).unwrap();
    assert!(!encoded.contains("unused-local-fixture-key"));
    assert!(!encoded.contains("private catalog data"));
}

#[tokio::test(flavor = "current_thread")]
async fn authorized_candidate_keeps_registration_binding_private_and_does_not_qualify_inference() {
    let scope = Uuid::new_v4();
    let credential = issued(scope);
    let encoded = swarmie_provider_auth::encode(&credential).unwrap();
    let (driver, candidate) = authorized(scope, &encoded).await;
    assert!(candidate.record.account.id.is_empty());
    assert_eq!(candidate.record.account.models[0].efforts, ["medium"]);
    assert_eq!(
        driver.credential_requirement(&candidate.record).unwrap(),
        CredentialRequirement::Account
    );
    assert_eq!(
        driver.association_key(&candidate.record).unwrap(),
        candidate.association_key
    );
    driver
        .validate_credential(&candidate.record, &encoded)
        .unwrap();
    let private = serde_json::to_string(&candidate.record).unwrap();
    for secret in [
        "fixture-access-token",
        "fixture-refresh-token",
        "fixture-id-token",
    ] {
        assert!(!private.contains(secret));
    }
    assert!(
        candidate.record.private_configuration()["discovered_limits"]["models"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    let mut foreign = credential.clone();
    foreign.chatgpt.as_mut().unwrap().identity.subject = "another-subject".into();
    let foreign = swarmie_provider_auth::encode(&foreign).unwrap();
    let error = driver
        .validate_credential(&candidate.record, &foreign)
        .unwrap_err();
    assert_eq!(error.code, tembra::Code::Invalid);
    assert_eq!(error.reason, "providers.credential_invalid");
    let other = AiAccountDriver::new(Uuid::new_v4()).unwrap();
    assert!(
        other
            .validate_credential(&candidate.record, &encoded)
            .is_err()
    );
    assert!(
        driver
            .renew(&candidate.record, &encoded, 1)
            .await
            .unwrap()
            .is_none()
    );
    let mut record = candidate.record;
    record.account.id = "committed-account".into();
    let selection = ModelSelection {
        account_id: record.account.id.clone(),
        model_id: "fixture-model".into(),
        reasoning: None,
    };
    let error = AiModelFactory::new(ModelProtection::Unprotected)
        .capture(&record, &selection)
        .err()
        .unwrap();
    assert_eq!(error.code, tembra::Code::Unavailable);
    assert_eq!(error.reason, "providers.model_profile");
}

#[tokio::test(flavor = "current_thread")]
async fn api_keys_are_not_encoded_authorization_credentials_and_need_no_refresh() {
    let record:AccountRecord=serde_json::from_value(serde_json::json!({
        "account":{"id":"account","providerId":"openai","providerLabel":"Fixture","label":"Account","models":[],"available":true},
        "provider":"open_ai","endpoint":"https://api.openai.com/v1","oauth":false,"oauth_identity":null
    })).unwrap();
    let scope = Uuid::new_v4();
    let driver = AiAccountDriver::new(scope).unwrap();
    let key = Zeroizing::<String>::new("fixture-key".into());
    driver.validate_credential(&record, &key).unwrap();
    assert!(!driver.needs_renewal(&record, &key, u64::MAX).unwrap());
    assert_eq!(
        driver.credential_requirement(&record).unwrap(),
        CredentialRequirement::Account
    );
    assert!(
        driver
            .renew(&record, &key, u64::MAX)
            .await
            .unwrap()
            .is_none()
    );
    let encoded = swarmie_provider_auth::encode(&issued(scope)).unwrap();
    assert!(driver.validate_credential(&record, &encoded).is_err());
    for value in ["", "bad\nkey", "swarmie-oauth-v1:invalid"] {
        assert!(driver.validate_credential(&record, value).is_err());
    }
    assert!(AiAccountDriver::new(Uuid::nil()).is_err());
    assert!(
        AiAccountDriver::with_options(
            scope,
            ChatgptBrowserOptions::default(),
            Some("https://unrelated.example/".into())
        )
        .is_err()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn existing_browser_flow_validates_callbacks_and_closes_the_physical_listener() {
    let scope = Uuid::new_v4();
    let mut options = ChatgptBrowserOptions::default();
    options.browser.callback_port = 0;
    let driver = AiAccountDriver::with_options(scope, options, None).unwrap();
    let returning = swarmie_provider_auth::encode(&issued(scope)).unwrap();
    let flow = driver
        .begin_authorization("openai", Some(&returning))
        .await
        .unwrap();
    let instructions = flow.instructions();
    let authorization = reqwest::Url::parse(&instructions.authorization_url).unwrap();
    assert!(
        authorization
            .query_pairs()
            .any(|(name, value)| name == "client_id" && value == "issued-client")
    );
    let callback = instructions.callback_uri.unwrap();
    let error = flow
        .submit_redirect(&format!("{callback}?code=fixture-code&state=incorrect"))
        .await
        .err()
        .unwrap();
    assert!(!error.message.contains("fixture-id-token"));
    assert!(flow.poll().await.unwrap().is_none());
    flow.cancel();
    flow.close().await;
    assert!(flow.poll().await.is_err());
    let callback = reqwest::Url::parse(&callback).unwrap();
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", callback.port().unwrap()))
            .await
            .is_err()
    );
    assert!(driver.begin_authorization("kimi", None).await.is_err());
    let foreign = swarmie_provider_auth::encode(&issued(Uuid::new_v4())).unwrap();
    assert!(
        driver
            .begin_authorization("openai", Some(&foreign))
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn expired_refresh_uses_the_existing_protocol_once_and_redacts_its_error() {
    let scope = Uuid::new_v4();
    let mut credential = issued(scope);
    credential.expires_at_ms = 0;
    let encoded = swarmie_provider_auth::encode(&credential).unwrap();
    let (_, candidate) = authorized(scope, &encoded).await;
    let (endpoint, request) = server(
        serde_json::json!({"error":"private-provider-diagnostic"}),
        401,
    )
    .await;
    let mut options = ChatgptBrowserOptions::default();
    options.browser.token_url = format!("{endpoint}/token");
    let driver = AiAccountDriver::with_options(scope, options, None).unwrap();
    let foreign = AiAccountDriver::new(Uuid::new_v4()).unwrap();
    assert!(
        driver
            .needs_renewal(&candidate.record, &encoded, 1)
            .unwrap()
    );
    assert!(foreign.renew(&candidate.record, &encoded, 1).await.is_err());
    let error = driver
        .renew(&candidate.record, &encoded, 1)
        .await
        .err()
        .unwrap();
    assert!(error.message.contains("sign in again"));
    assert!(!error.message.contains("private-provider-diagnostic"));
    let request = request.await.unwrap();
    assert!(request.starts_with("POST /token HTTP/1.1"));
    assert!(request.contains("grant_type=refresh_token"));
    assert!(request.contains("client_id=issued-client"));
}

#[tokio::test(flavor = "current_thread")]
async fn common_accounts_uses_the_concrete_driver_and_factory_without_local_credential_io() {
    use futures::{FutureExt, future::LocalBoxFuture};
    use std::{cell::RefCell, rc::Rc};
    use swarmie_providers::{
        AgentContext, Message, ModelRequest, OperationId,
        accounts::{
            AccountStore, Accounts, CredentialStore, ModelChoice, SelectModel, StoreWriteError,
        },
        settings::ProviderSettings,
    };

    use std::collections::BTreeMap;
    use swarmie_providers::accounts::recovery::{
        CredentialRead, CredentialReference, LegacyPredecessor, ProviderOwner, ProviderState,
        ProvisioningPlan, RecoveryLimits, StateLoad,
    };

    struct Store(RefCell<ProviderState>);

    impl AccountStore for Store {
        fn load(&self) -> LocalBoxFuture<'_, tembra::Result<StateLoad>> {
            Box::pin(async { Ok(StateLoad::Initialized(self.0.borrow().clone())) })
        }

        fn initialize(
            &self,
            _plan: ProvisioningPlan,
            _initial: ProviderState,
        ) -> LocalBoxFuture<'_, Result<(), StoreWriteError>> {
            Box::pin(async { panic!("fixture is already initialized") })
        }

        fn adopt(
            &self,
            _expected: LegacyPredecessor,
            _initial: ProviderState,
        ) -> LocalBoxFuture<'_, Result<(), StoreWriteError>> {
            Box::pin(async { panic!("fixture has no legacy state") })
        }

        fn save(
            &self,
            expected: Uuid,
            next: ProviderState,
        ) -> LocalBoxFuture<'_, Result<(), StoreWriteError>> {
            Box::pin(async move {
                assert_eq!(self.0.borrow().revision(), expected);
                *self.0.borrow_mut() = next;
                Ok(())
            })
        }
    }

    struct NoCustody;

    impl CredentialStore for NoCustody {
        fn check_reference(&self, _reference: &CredentialReference) -> tembra::Result<()> {
            panic!("a local account must not plan credentials")
        }

        fn read(
            &self,
            _reference: CredentialReference,
        ) -> LocalBoxFuture<'_, tembra::Result<CredentialRead>> {
            Box::pin(async { panic!("a local account must not read credentials") })
        }

        fn stage(
            &self,
            _reference: CredentialReference,
            _value: Zeroizing<String>,
        ) -> LocalBoxFuture<'_, Result<(), StoreWriteError>> {
            Box::pin(async { panic!("a local account must not stage credentials") })
        }

        fn retire(
            &self,
            _reference: CredentialReference,
        ) -> LocalBoxFuture<'_, Result<(), StoreWriteError>> {
            Box::pin(async { panic!("a local account must not retire credentials") })
        }
    }

    let (endpoint, request) = server(
        serde_json::json!({"models":[{
            "name":"fixture-local","limit":{"context":32000,"output":4000}
        }]}),
        200,
    )
    .await;
    let owner = ProviderOwner::new("fixture".into(), Uuid::new_v4()).unwrap();
    let store = Rc::new(Store(RefCell::new(
        ProviderState::new(
            owner.clone(),
            Uuid::new_v4(),
            vec![],
            BTreeMap::new(),
            None,
            RecoveryLimits::default(),
        )
        .unwrap(),
    )));
    let accounts = Accounts::new(
        owner,
        store.clone(),
        Rc::new(NoCustody),
        Rc::new(AiAccountDriver::new(Uuid::new_v4()).unwrap()),
        Rc::new(swarmie_provider_auth::now_ms),
        64,
        RecoveryLimits::default(),
    )
    .unwrap();
    let pool = futures::executor::LocalPool::new();
    let host = tembra::Host::new(pool.spawner());
    host.root("fixture", move |context| {
        async move {
            let account = accounts
                .configure(
                    context.principal()?,
                    ConfigureProvider {
                        provider_id: "ollama".into(),
                        method: "local".into(),
                        credential: None,
                        url: Some(endpoint),
                    },
                )
                .await?;
            assert!(!account.id.is_empty());
            let factory = AiModelFactory::new(ModelProtection::Unprotected);
            let model = accounts
                .select(
                    &context,
                    SelectModel {
                        account_id: account.id,
                        model: ModelChoice::Default,
                        reasoning: None,
                    },
                    &factory,
                )
                .await?;
            let prepared = model
                .prepare(
                    &context,
                    OperationId::new(),
                    ModelRequest {
                        context: AgentContext {
                            sources: None,
                            instructions: "Current instruction".into(),
                            messages: vec![Message::User {
                                content: "Pending input".into(),
                            }],
                        },
                        tools: vec![],
                    },
                    None,
                )
                .await?;
            assert_eq!(prepared.description().model.identity.provider, "ollama");
            assert_eq!(prepared.description().model.identity.model, "fixture-local");
            assert_eq!(prepared.description().output_reserve, 4000);
            drop(prepared);
            assert_eq!(store.0.borrow().accounts().len(), 1);
            Ok(())
        }
        .boxed_local()
    })
    .await
    .unwrap();
    host.close().await.unwrap();
    assert!(request.await.unwrap().starts_with("GET /api/tags HTTP/1.1"));
}
