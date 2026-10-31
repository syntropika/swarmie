use futures::FutureExt;
use std::{net::TcpListener, num::NonZeroU64};
use swarmie_model_catalog::{Catalog, Evidence, Layer, LimitClaim, Limits, Precision};
use swarmie_model_provider_ai::{AiModelFactory, ModelProtection};
use swarmie_providers::{AccountRecord, CredentialRequirement, ModelFactory, ModelSelection};
use zeroize::Zeroizing;

fn account(product: &str, provider: &str, endpoint: &str, model: &str) -> AccountRecord {
    let evidence = Evidence {
        source: "fixture-discovery".into(),
        revision: "one".into(),
        layer: Layer::Provider,
        observed_at_ms: 0,
        expires_at_ms: None,
    };
    let claim = |tokens| LimitClaim {
        tokens: NonZeroU64::new(tokens).unwrap(),
        precision: Precision::Reported,
        evidence: evidence.clone(),
    };
    serde_json::from_value(serde_json::json!({
        "account": {
            "id":"account", "providerId":product,"providerLabel":"Fixture",
            "label":"Account", "available":true,
            "models":[{"id":model,"label":"Model","efforts":["high"]}],
            "authorizationMethod":if product=="ollama" {"local"} else {"api-key"}
        },
        "provider":provider, "endpoint":endpoint, "oauth":false,
        "oauth_identity":null, "retained_private_field":{"revision":7},
        "discovered_limits":{
            "endpoint":endpoint,
            "models":{model:Limits {context:vec![claim(32_000)],input:vec![],output:vec![claim(4_000)]}},
            "capabilities":{}
        }
    })).unwrap()
}

fn selection(model: &str) -> ModelSelection {
    ModelSelection {
        account_id: "account".into(),
        model_id: model.into(),
        reasoning: None,
    }
}

fn resolve(
    capture: &swarmie_providers::CapturedModel,
) -> std::sync::Arc<swarmie_model_catalog::Snapshot> {
    let input = capture.metadata();
    let mut catalog = Catalog::new(4);
    catalog.replace_sources(input.records.clone()).unwrap();
    catalog
        .prepare(input.preparation.clone(), swarmie_provider_auth::now_ms())
        .unwrap()
}

#[test]
fn exact_product_binding_is_adapter_owned_and_cannot_cross_an_endpoint() {
    let factory = AiModelFactory::new(ModelProtection::Unprotected);
    let record = account("kimi", "kimi", "https://api.moonshot.ai/v1", "fixture");
    let capture = factory.capture(&record, &selection("fixture")).unwrap();
    assert_eq!(
        capture.metadata().embedded.as_ref().unwrap().provider,
        "moonshotai"
    );
    assert_eq!(
        capture.credential_requirement(),
        CredentialRequirement::Account
    );
    assert_eq!(capture.metadata().preparation.identity.provider, "kimi");
    let (saved_account, mut private) = record.into_parts();
    private.insert(
        "endpoint".into(),
        "https://api.moonshot.ai.attacker.example/v1".into(),
    );
    let changed = AccountRecord::new(saved_account, private).unwrap();
    let capture = factory.capture(&changed, &selection("fixture")).unwrap();
    assert!(capture.metadata().embedded.is_none());
    assert!(
        capture
            .metadata()
            .records
            .iter()
            .all(|record| record.limits == Limits::default())
    );
}

#[test]
fn unavailable_profiles_and_foreign_selections_fail_without_a_credential() {
    let factory = AiModelFactory::new(ModelProtection::Unprotected);
    let record = account(
        "kimi",
        "kimi",
        "https://api.moonshot.ai/v1",
        "kimi-k3.1-pro",
    );
    let error = factory
        .capture(&record, &selection("kimi-k3.1-pro"))
        .err()
        .unwrap();
    assert_eq!(error.code, tembra::Code::Unavailable);
    assert_eq!(error.reason, "providers.model_profile");
    let record = account(
        "openai",
        "open_ai_responses",
        "https://api.openai.com/v1",
        "fixture",
    );
    let (saved_account, mut private) = record.into_parts();
    private.insert("oauth".into(), true.into());
    let record = AccountRecord::new(saved_account, private).unwrap();
    let error = factory
        .capture(&record, &selection("fixture"))
        .err()
        .unwrap();
    assert_eq!(error.code, tembra::Code::Unavailable);
    assert!(error.message.contains("has not been qualified"));
    let record = account(
        "openai",
        "open_ai",
        "https://provider.example/v1",
        "fixture",
    );
    let mut wrong = selection("fixture");
    wrong.account_id = "another".into();
    assert!(factory.capture(&record, &wrong).is_err());
    let mut wrong = selection("fixture");
    wrong.reasoning = Some("unsupported".into());
    assert!(factory.capture(&record, &wrong).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn factory_freezes_configuration_and_retains_the_snapshot_without_http() {
    use swarmie_providers::{AgentContext, Message, ModelRequest, OperationId};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let factory = AiModelFactory::new(ModelProtection::Unprotected);
    let record = account("openai", "open_ai", &endpoint, "fixture");
    let mut choice = selection("fixture");
    choice.reasoning = Some("HIGH".into());
    let capture = factory.capture(&record, &choice).unwrap();
    let snapshot = resolve(&capture);
    let (saved_account, mut private) = record.into_parts();
    private.insert("endpoint".into(), "https://changed.example/v1".into());
    let changed = AccountRecord::new(saved_account, private).unwrap();
    assert_ne!(
        factory
            .capture(&changed, &choice)
            .unwrap()
            .metadata()
            .preparation,
        *snapshot.preparation()
    );
    let model = capture
        .construct(snapshot.clone(), Some(Zeroizing::new("fixture-key".into())))
        .unwrap();
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    let pool = futures::executor::LocalPool::new();
    let host = tembra::Host::new(pool.spawner());
    host.root("fixture", move |context| {
        async move {
            let request = ModelRequest {
                context: AgentContext {
                    sources: None,
                    instructions: "Current instruction".into(),
                    messages: vec![Message::User {
                        content: "Pending input".into(),
                    }],
                },
                tools: vec![],
            };
            let prepared = model
                .prepare(&context, OperationId::new(), request, None)
                .await?;
            assert_eq!(
                prepared.description().model.generation,
                snapshot.generation()
            );
            assert_eq!(
                prepared.description().model.identity.endpoint,
                format!("{endpoint}/chat/completions")
            );
            assert_eq!(
                listener.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
            drop(prepared);
            Ok(())
        }
        .boxed_local()
    })
    .await
    .unwrap();
    host.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn local_configuration_requires_no_account_credential() {
    let factory = AiModelFactory::new(ModelProtection::Unprotected);
    let record = account("ollama", "ollama", "https://provider.example/v1", "fixture");
    let capture = factory.capture(&record, &selection("fixture")).unwrap();
    assert_eq!(
        capture.credential_requirement(),
        CredentialRequirement::None
    );
    let snapshot = resolve(&capture);
    let _model = capture.construct(snapshot, None).unwrap();
    let record = account(
        "openai",
        "open_ai",
        "https://provider.example/v1",
        "fixture",
    );
    let capture = factory.capture(&record, &selection("fixture")).unwrap();
    assert_eq!(
        capture.credential_requirement(),
        CredentialRequirement::Account
    );
    let snapshot = resolve(&capture);
    assert!(capture.construct(snapshot, None).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn scoped_factory_preserves_mandatory_protection_before_http() {
    use std::rc::Rc;
    use swarmie_model_provider_ai::ModelDecisions;
    use swarmie_model_submission_protection::{DecisionIntent, ModelGuard};
    use swarmie_permissions::{
        Authority, CorePlugin, ExecutionContexts, HumanDecision, PluginGrant,
    };
    use swarmie_providers::{AgentContext, Message, ModelRequest, OperationId};

    struct NoDecision;

    impl ModelDecisions for NoDecision {
        fn request<'a>(
            &'a self,
            _execution: &'a swarmie_permissions::ExecutionBinding,
            _intent: &'a DecisionIntent,
        ) -> futures::future::LocalBoxFuture<'a, tembra::Result<HumanDecision>> {
            Box::pin(async { panic!("an unbound execution cannot request a model decision") })
        }
    }

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let factory = AiModelFactory::new(ModelProtection::Guarded {
        guard: ModelGuard::new(
            "factory-fixture".into(),
            PluginGrant::bundled(CorePlugin::ModelProtection, "factory-fixture".into()),
            None,
        ),
        executions: ExecutionContexts::new(Authority::new()),
        decisions: Rc::new(NoDecision),
    });
    let record = account("openai", "open_ai", &endpoint, "fixture");
    let capture = factory.capture(&record, &selection("fixture")).unwrap();
    let snapshot = resolve(&capture);
    let model = capture
        .construct(snapshot, Some(Zeroizing::new("fixture-key".into())))
        .unwrap();
    let pool = futures::executor::LocalPool::new();
    let host = tembra::Host::new(pool.spawner());
    host.root("fixture", move |context| {
        async move {
            let request = ModelRequest {
                context: AgentContext {
                    sources: None,
                    instructions: "Current instruction".into(),
                    messages: vec![Message::User {
                        content: "Pending input".into(),
                    }],
                },
                tools: vec![],
            };
            let prepared = model
                .prepare(&context, OperationId::new(), request, None)
                .await?;
            let error = prepared
                .execute()
                .now_or_never()
                .expect("an unbound execution must be refused before asynchronous HTTP")
                .err()
                .unwrap();
            assert_eq!(error.code, tembra::Code::Forbidden);
            assert_eq!(
                listener.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
            Ok(())
        }
        .boxed_local()
    })
    .await
    .unwrap();
    host.close().await.unwrap();
}
