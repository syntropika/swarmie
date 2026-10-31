use std::{num::NonZeroU64, rc::Rc, sync::Arc};
use swarmie_model_catalog::{
    Capability, Catalog, Evidence, Layer, LimitClaim, Limits, Precision, Preparation, Requirements,
    Snapshot, SourceRecord,
};
use swarmie_model_provider_ai::{
    AiModel, Connection, ConnectionId, EnvironmentCredentials, ModelId, ModelProtection, Provider,
};

fn connection(id: &str) -> (Connection, ModelId) {
    let model = ModelId::new("fixture-model").unwrap();
    let mut connection = Connection::new(
        ConnectionId::new(id).unwrap(),
        Provider::OpenAi,
        [model.clone()],
    );
    connection.endpoint = Some("https://provider.example/v1".into());
    connection.credential_key = Some("fixture".into());
    (connection, model)
}

fn snapshot(
    connection: &Connection,
    model: &ModelId,
    effort: Option<&str>,
    limits: bool,
) -> Arc<Snapshot> {
    let evidence = Evidence {
        source: "fixture".into(),
        revision: "1".into(),
        layer: Layer::Provider,
        observed_at_ms: 0,
        expires_at_ms: None,
    };
    let claim = |tokens| LimitClaim {
        tokens: NonZeroU64::new(tokens).unwrap(),
        precision: Precision::Reported,
        evidence: evidence.clone(),
    };
    let identity = connection.model_identity(model).unwrap();
    let record = SourceRecord {
        identity: identity.clone(),
        limits: if limits {
            Limits {
                context: vec![claim(32_000)],
                input: vec![],
                output: vec![claim(4_000)],
            }
        } else {
            Limits::default()
        },
        tools: Capability::default(),
        streaming: Capability::default(),
        native_output_maximum: Capability::default(),
        native_compaction: Capability::Unsupported {
            evidence: Evidence {
                layer: Layer::Adapter,
                ..evidence
            },
            reason: "Message-only adapter".into(),
        },
    };
    let mut catalog = Catalog::new(4);
    catalog.replace_sources(vec![record]).unwrap();
    catalog
        .prepare(
            Preparation {
                identity,
                configuration_revision: connection.configuration_revision(model, effort).unwrap(),
                requirements: Requirements {
                    context: limits,
                    input: limits,
                    output: limits,
                    ..Requirements::default()
                },
                allow_stale_for_ms: 0,
            },
            0,
        )
        .unwrap()
}

#[test]
fn descriptor_uses_explicit_route_deployment_and_reasoning_without_model_name_heuristics() {
    let (a, model) = connection("one");
    let mut b = a.clone();
    b.id = ConnectionId::new("two").unwrap();
    assert_ne!(
        a.model_identity(&model).unwrap(),
        b.model_identity(&model).unwrap()
    );
    let identity = a.model_identity(&model).unwrap();
    assert_eq!(identity.provider, "openai");
    assert_eq!(identity.adapter, "swarmie-ai.chat.openai.api-key.v1");
    assert_eq!(
        identity.endpoint,
        "https://provider.example/v1/chat/completions"
    );
    assert_eq!(identity.deployment.as_deref(), Some("one"));
    assert_ne!(
        a.configuration_revision(&model, None).unwrap(),
        a.configuration_revision(&model, Some("high")).unwrap()
    );
    assert_eq!(
        a.configuration_revision(&model, Some("zero")).unwrap(),
        a.configuration_revision(&model, Some("none")).unwrap()
    );
    b.provider = Provider::OpenAiResponses;
    assert_eq!(
        b.model_identity(&model).unwrap().provider,
        "openai-responses"
    );
    b.endpoint = None;
    assert_eq!(
        b.model_identity(&model).unwrap().endpoint,
        "https://api.openai.com/v1/responses"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn attachment_keeps_the_exact_snapshot_and_refuses_mismatched_configuration() {
    let (connection, model) = connection("one");
    let metadata = snapshot(&connection, &model, Some("high"), true);
    let make = || {
        AiModel::new(
            connection.clone(),
            model.clone(),
            Rc::new(EnvironmentCredentials),
            ModelProtection::Unprotected,
        )
        .unwrap()
    };
    let bound = make()
        .with_reasoning("high")
        .unwrap()
        .with_metadata(metadata.clone())
        .unwrap();
    assert!(Arc::ptr_eq(bound.metadata().unwrap(), &metadata));
    assert!(make().with_metadata(metadata).is_err());
    assert!(bound.with_reasoning("low").is_err());
    let mut foreign = connection.clone();
    foreign.endpoint = Some("https://another.example/v1".into());
    assert!(
        make()
            .with_metadata(snapshot(&foreign, &model, None, true))
            .is_err()
    );
    assert!(
        make()
            .with_metadata(snapshot(&connection, &model, None, false))
            .is_err()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn final_preparation_is_private_and_the_exact_measured_bytes_reach_http_once() {
    use futures::FutureExt;
    use sha2::{Digest, Sha256};
    use std::{
        io::{Read, Write},
        net::TcpListener,
        time::{Duration, Instant},
    };
    use swarmie_providers::{
        AgentContext, CountPrecision, Message, Model, ModelRequest, OperationId,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let (mut connection, model_id) = connection("http-fixture");
    connection.endpoint = Some(format!("http://{address}/v1"));
    connection.credential_key = Some("fixture".into());
    connection.timeout_ms = NonZeroU64::new(3_000).unwrap();
    struct FixtureCredentials;
    impl swarmie_model_provider_ai::Credentials for FixtureCredentials {
        fn acquire<'a>(
            &'a self,
            context: &'a tembra::Context,
            _admitted: &'a swarmie_model_provider_ai::AdmittedAttempt,
            _key: &'a str,
        ) -> futures::future::LocalBoxFuture<
            'a,
            tembra::Result<swarmie_model_provider_ai::ModelCredential>,
        > {
            async move {
                context.check()?;
                swarmie_model_provider_ai::ModelCredential::new(
                    "synthetic-fixture".into(),
                    "[credential]".into(),
                )
            }
            .boxed_local()
        }
    }
    let metadata = snapshot(&connection, &model_id, None, true);
    let model = AiModel::new(
        connection,
        model_id,
        Rc::new(FixtureCredentials),
        ModelProtection::Unprotected,
    )
    .unwrap()
    .with_metadata(metadata.clone())
    .unwrap();
    let pool = futures::executor::LocalPool::new();
    let host = tembra::Host::new(pool.spawner());
    host.root("fixture",move |context|async move {
        let request=ModelRequest {context:AgentContext {sources: None,instructions:"Current instruction".into(),messages:vec![Message::User {content:"synthetic-private-payload🙂".into()}]},tools:vec![]};
        let unavailable=model.prepare(&context,OperationId::new(),request.clone(),Some(swarmie_providers::Progress::new(|_,_|{}))).await.err().unwrap();
        assert_eq!(unavailable.code,tembra::Code::Unavailable);
        assert_eq!(unavailable.reason,"swarmie.models.capability");
        let prepared=model.prepare(&context,OperationId::new(),request,None).await?;
        let description=prepared.description().clone();
        assert_eq!(description.measurement.precision,CountPrecision::Estimated);
        assert!(!serde_json::to_string(&description).unwrap().contains("synthetic-private-payload"));
        assert_eq!(listener.accept().unwrap_err().kind(),std::io::ErrorKind::WouldBlock);
        let server=std::thread::spawn(move || {
            let deadline=Instant::now()+Duration::from_secs(5);
            let (mut socket,_)=loop {match listener.accept() {Ok(value)=>break value,Err(error) if error.kind()==std::io::ErrorKind::WouldBlock=>{assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(5));},Err(error)=>panic!("{error}")}};
            socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut received=Vec::new();let mut buffer=[0_u8;8192];
            let (path,body)=loop {
                let count=socket.read(&mut buffer).unwrap();assert_ne!(count,0);received.extend_from_slice(&buffer[..count]);
                if let Some(end)=received.windows(4).position(|value|value==b"\r\n\r\n") {
                    let headers=std::str::from_utf8(&received[..end]).unwrap();
                    let length:usize=headers.lines().find_map(|line|{let (key,value)=line.split_once(':')?;key.eq_ignore_ascii_case("content-length").then(||value.trim().parse().unwrap())}).unwrap();
                    if received.len()>=end+4+length {break (headers.lines().next().unwrap().split_whitespace().nth(1).unwrap().to_owned(),received[end+4..end+4+length].to_vec());}
                }
            };
            let response=r#"{"id":"fixture","object":"chat.completion","model":"fixture-model","choices":[{"index":0,"message":{"role":"assistant","content":"done"},"finish_reason":"stop"}],"usage":{"prompt_tokens":8,"completion_tokens":2,"total_tokens":10}}"#;
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
            (path,body)
        });
        let response=prepared.execute().await?;
        assert_eq!(response.message.content,"done");
        let (path,body)=server.join().unwrap();
        assert_eq!(description.measurement.final_payload_bytes,body.len() as u64);
        let mut hash=Sha256::new();hash.update(metadata.generation().as_bytes());hash.update([0]);hash.update(format!("http://{address}{path}").as_bytes());hash.update([0]);hash.update(&body);
        let actual:[u8;32]=hash.finalize().into();assert_eq!(description.fingerprint,actual);
        let body:serde_json::Value=serde_json::from_slice(&body).unwrap();
        assert_eq!(body["max_tokens"],4_000);
        assert_eq!(description.output_reserve,4_000);
        Ok(())
    }.boxed_local()).await.unwrap();
    host.close().await.unwrap();
}
