use axum::{Json, extract::State, http::header, response::IntoResponse, routing::post};
use futures::{
    FutureExt,
    task::{LocalFutureObj, LocalSpawn, SpawnError},
};
use serde_json::{Value, json};
use std::{
    num::NonZeroU32,
    rc::Rc,
    sync::{Arc, Mutex},
};
use swarmie_agent_loop::{self as agent, Agent, ExecutionResult};
use swarmie_fixture_mcp_server::{Counter, serve_http};
use swarmie_mcp::{
    self as mcp, Configuration, Endpoint, Limits, Observation, Protocol, ServerBinding, ServerId,
};
use swarmie_model_provider_genai::{
    Connection, ConnectionId, Credentials, GenaiModel, ModelId, Provider,
};
use swarmie_providers::{AgentContext, Message, Progress, ToolResult};
use swarmie_tools::{self as tools, Catalog, CatalogService};
use tembra::{Host, Reference};

#[derive(Clone, Copy)]
struct Executor;
impl LocalSpawn for Executor {
    fn spawn_local_obj(&self, future: LocalFutureObj<'static, ()>) -> Result<(), SpawnError> {
        tokio::task::spawn_local(future);
        Ok(())
    }
}
struct Secrets;
impl Credentials for Secrets {
    fn resolve(&self, _: &str) -> Option<String> {
        Some("synthetic-secret".into())
    }
}
async fn call<A: 'static, R: 'static>(
    host: &Host,
    reference: &Rc<Reference>,
    method: &str,
    args: A,
) -> tembra::Result<R> {
    let reference = reference.clone();
    let method = method.to_owned();
    host.root("alice", move |ctx| {
        async move { reference.call(&ctx, &method, args).await }.boxed_local()
    })
    .await
}
fn configuration(endpoint: Endpoint) -> Configuration {
    Configuration {
        id: ServerId::new("fixture").unwrap(),
        endpoint,
        protocol: Protocol::Current,
        limits: Limits::default(),
    }
}
fn mount(configuration: Configuration, catalog: &Catalog) -> (Host, Rc<Reference>) {
    let host = Host::new(Executor);
    host.install(vec![
        mcp::orb(vec![ServerBinding {
            configuration,
            principal: "alice".into(),
            catalog: catalog.clone(),
        }])
        .unwrap(),
        tools::orb(host.clone(), catalog.clone()),
    ])
    .unwrap();
    let reference = Rc::new(
        host.get(mcp::SERVICE, vec!["fixture".into()], "alice")
            .unwrap(),
    );
    (host, reference)
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_schema_on_a_later_discovery_page_never_publishes_a_partial_catalog() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let counter = Counter::default().with_invalid_input_schema();
            let (url, stop, serving) = serve_http(counter.clone()).await;
            let catalog = Catalog::default();
            let (host, reference) = mount(
                configuration(Endpoint::Http {
                    url,
                    headers: Default::default(),
                }),
                &catalog,
            );
            let error =
                call::<_, Observation>(&host, &reference, mcp::McpMethod::Connect.as_str(), ())
                    .await
                    .expect_err("invalid remote schemas must fail before publication");
            assert_eq!(error.code, tembra::Code::Invalid);
            assert_eq!(counter.catalog_reads(), 2);
            assert!(catalog.snapshot().unwrap().tools.is_empty());
            assert_eq!(counter.effects(), 0);
            host.close().await.unwrap();
            stop.cancel();
            serving.await.unwrap();
        })
        .await;
}

type Captured = Arc<Mutex<Vec<Value>>>;
async fn inference(
    State(captured): State<Captured>,
    Json(request): Json<Value>,
) -> impl IntoResponse {
    let mut calls = captured.lock().unwrap();
    let first = calls.is_empty();
    assert_eq!(request["store"], false);
    assert_eq!(request["stream"], true);
    assert!(!request.to_string().contains("mcp.fixture."));
    let name = request["tools"][0]["name"].as_str().unwrap().to_owned();
    assert_eq!(name, "mcp__fixture__counter__add");
    calls.push(request);
    let output = if first {
        json!([{"type":"function_call","id":"fc_fixture","call_id":"call_fixture","name":name,"arguments":"{\"amount\":7}","status":"completed"}])
    } else {
        json!([{"type":"message","id":"msg_fixture","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Done","annotations":[]}]}])
    };
    let response = json!({"type":"response.completed","response":{"id":"resp_fixture","object":"response","model":"fixture-model","status":"completed","output":output,"usage":{"input_tokens":7,"output_tokens":3,"total_tokens":10}}});
    let mut stream = String::new();
    if first {
        let item = json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc_fixture","call_id":"call_fixture","name":name,"arguments":""}});
        stream.push_str(&format!(
            "event: response.output_item.added\ndata: {item}\n\n"
        ));
        let arguments = json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_fixture","delta":"{\"amount\":7}"});
        stream.push_str(&format!(
            "event: response.function_call_arguments.delta\ndata: {arguments}\n\n"
        ));
    } else {
        let text = json!({"type":"response.output_text.delta","output_index":0,"item_id":"msg_fixture","content_index":0,"delta":"Done"});
        stream.push_str(&format!(
            "event: response.output_text.delta\ndata: {text}\n\n"
        ));
    }
    stream.push_str(&format!("event: response.completed\ndata: {response}\n\n"));
    ([(header::CONTENT_TYPE, "text/event-stream")], stream)
}

#[tokio::test(flavor = "current_thread")]
async fn stdio_mcp_catalog_routes_a_real_genai_tool_call_once_and_preserves_canonical_history() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let captured: Captured = Arc::default();
            let router = axum::Router::new()
                .route("/v1/responses", post(inference))
                .with_state(captured.clone());
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}/v1/", listener.local_addr().unwrap());
            let stop = tokio_util::sync::CancellationToken::new();
            let shutdown = stop.clone();
            let serving = tokio::spawn(async move {
                axum::serve(listener, router)
                    .with_graceful_shutdown(shutdown.cancelled_owned())
                    .await
                    .unwrap();
            });
            let catalog = Catalog::default();
            let (host, reference) = mount(
                configuration(Endpoint::Local {
                    program: env!("CARGO_BIN_EXE_swarmie-fixture-mcp-server").into(),
                    arguments: vec![],
                    directory: None,
                }),
                &catalog,
            );
            let connected: Observation =
                call(&host, &reference, mcp::McpMethod::Connect.as_str(), ())
                    .await
                    .unwrap();
            assert_eq!(connected.tools.len(), 3);
            let selection = catalog.snapshot().unwrap();
            let id = ModelId::new("fixture-model").unwrap();
            let mut connection = Connection::new(
                ConnectionId::new("fixture").unwrap(),
                Provider::OpenAiResponses,
                [id.clone()],
            );
            connection.endpoint = Some(endpoint);
            connection.credential_key = Some("fixture".into());
            let model = Rc::new(GenaiModel::new(connection, id, &Secrets).unwrap());
            let binding = host.clone();
            host.install(vec![agent::orb(move |declaration| {
                Ok(Agent::new(
                    model.clone(),
                    Rc::new(CatalogService::bind(
                        &declaration,
                        binding.clone(),
                        tools::SERVICE,
                        vec![tools::INSTANCE_KEY.into()],
                    )?),
                )
                .with_progress(Progress::new(|_, _| {})))
            })])
            .unwrap();
            let execution = Rc::new(
                host.get(agent::SERVICE, vec!["main".into()], "alice")
                    .unwrap(),
            );
            let output: ExecutionResult = call(
                &host,
                &execution,
                agent::Method::Execute.as_str(),
                agent::Request {
                    context: AgentContext {
                        instructions: "Use the counter tool.".into(),
                        messages: vec![Message::User {
                            content: "Add seven.".into(),
                        }],
                    },
                    tools: selection.specs(),
                    max_turns: NonZeroU32::new(3).unwrap(),
                },
            )
            .await
            .unwrap();
            let output = output.unwrap();
            assert!(matches!(
                &output.messages[2], Message::Assistant(message) if message.content == "Done"
            ));
            assert!(matches!(
                &output.messages[0], Message::Assistant(message)
                    if message.tool_calls[0].name.as_str() == "mcp.fixture.counter.add"
            ));
            let Message::Tool {
                result: ToolResult::Success(result),
                ..
            } = &output.messages[1]
            else {
                panic!("expected the real MCP result");
            };
            assert_eq!(result["structuredContent"]["effect_count"], 1);
            assert_eq!(result["structuredContent"]["amount"], 7);
            let pid = result["structuredContent"]["pid"].as_u64().unwrap();
            assert_ne!(pid, u64::from(std::process::id()));
            {
                let calls = captured.lock().unwrap();
                assert_eq!(calls.len(), 2);
                assert!(calls[1]["input"].to_string().contains("effect_count"));
            }
            host.close().await.unwrap();
            assert!(catalog.snapshot().unwrap().tools.is_empty());
            stop.cancel();
            serving.await.unwrap();
        })
        .await;
}
