use futures::{
    FutureExt,
    task::{LocalFutureObj, LocalSpawn, SpawnError},
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
use swarmie_agent_loop::ToolDecision;
use swarmie_model_provider_genai::{
    Configuration, Connection, ConnectionId, Credentials, ModelId, ModelKey, Provider,
};
use swarmie_models::{AgentContext, Message, ModelRequest, ToolCall, ToolResult, ToolSpec};
use tembra::{Context, Host, Reference};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

#[derive(Clone, Copy)]
pub struct LocalExecutor;
impl LocalSpawn for LocalExecutor {
    fn spawn_local_obj(&self, future: LocalFutureObj<'static, ()>) -> Result<(), SpawnError> {
        tokio::task::spawn_local(future);
        Ok(())
    }
}

pub struct Secrets;
impl Credentials for Secrets {
    fn resolve(&self, key: &str) -> Option<String> {
        (key == "fixture").then(|| "fixture-secret".into())
    }
}

#[derive(Clone)]
pub struct Reply {
    pub status: u16,
    pub body: Value,
    pub delay: Duration,
}
impl Reply {
    pub fn ok(body: Value) -> Self {
        Self {
            status: 200,
            body,
            delay: Duration::ZERO,
        }
    }
}

#[derive(Clone)]
pub struct Captured {
    pub path: String,
    pub headers: String,
    pub body: Value,
}

pub struct StreamReply {
    pub parts: Vec<String>,
    pub interval: Duration,
    pub content_type: &'static str,
}

enum Body {
    Json(Reply),
    Stream(StreamReply),
}

pub struct Server {
    pub endpoint: String,
    pub calls: Rc<RefCell<Vec<Captured>>>,
    pub sent: Rc<Cell<usize>>,
    worker: Option<JoinHandle<()>>,
}
impl Server {
    pub async fn new(replies: Vec<Reply>) -> Self {
        Self::script(replies.into_iter().map(Body::Json).collect()).await
    }

    pub async fn streaming(replies: Vec<StreamReply>) -> Self {
        Self::script(replies.into_iter().map(Body::Stream).collect()).await
    }

    async fn script(replies: Vec<Body>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1/", listener.local_addr().unwrap());
        let calls = Rc::new(RefCell::new(Vec::new()));
        let captured = calls.clone();
        let sent = Rc::new(Cell::new(0));
        let sent_frames = sent.clone();
        let worker = tokio::task::spawn_local(async move {
            for reply in replies {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut data = Vec::new();
                let header_end;
                loop {
                    let mut buf = [0; 4096];
                    let n = stream.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    data.extend_from_slice(&buf[..n]);
                    if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = i + 4;
                        break;
                    }
                }
                let headers = String::from_utf8(data[..header_end].to_vec()).unwrap();
                let length: usize = headers
                    .lines()
                    .find_map(|l| {
                        l.split_once(':')
                            .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                            .map(|(_, v)| v.trim().parse().unwrap())
                    })
                    .unwrap();
                while data.len() < header_end + length {
                    let mut buf = [0; 4096];
                    let n = stream.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    data.extend_from_slice(&buf[..n]);
                }
                let body = serde_json::from_slice(&data[header_end..header_end + length]).unwrap();
                let path = headers
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .into();
                captured.borrow_mut().push(Captured {
                    path,
                    headers,
                    body,
                });
                match reply {
                    Body::Json(reply) => {
                        tokio::time::sleep(reply.delay).await;
                        let body = reply.body.to_string();
                        let output = format!(
                            "HTTP/1.1 {} Fixture\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                            reply.status,
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(output.as_bytes()).await;
                    }
                    Body::Stream(reply) => {
                        let length: usize = reply.parts.iter().map(String::len).sum();
                        let header = format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: {}\r\ncontent-length: {length}\r\nconnection: close\r\n\r\n",
                            reply.content_type
                        );
                        let _ = stream.write_all(header.as_bytes()).await;
                        for (index, part) in reply.parts.into_iter().enumerate() {
                            if index > 0 {
                                tokio::time::sleep(reply.interval).await;
                            }
                            if stream.write_all(part.as_bytes()).await.is_err() {
                                break;
                            }
                            sent_frames.set(sent_frames.get() + 1);
                        }
                    }
                }
            }
        });
        Self {
            endpoint,
            calls,
            sent,
            worker: Some(worker),
        }
    }
    pub async fn finish(mut self) {
        tokio::time::timeout(Duration::from_secs(3), self.worker.take().unwrap())
            .await
            .unwrap()
            .unwrap();
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(worker) = &self.worker {
            worker.abort();
        }
    }
}

pub fn configuration(provider: Provider, endpoint: &str) -> Configuration {
    let mut connection = Connection::new(
        ConnectionId::new("primary").unwrap(),
        provider,
        [ModelId::new("fixture-model").unwrap()],
    );
    connection.endpoint = Some(endpoint.into());
    if provider != Provider::Ollama {
        connection.credential_key = Some("fixture".into());
    }
    Configuration {
        connections: vec![connection],
    }
}
pub fn key() -> ModelKey {
    ModelKey {
        connection: ConnectionId::new("primary").unwrap(),
        model: ModelId::new("fixture-model").unwrap(),
    }
}

pub fn input(tool: bool) -> ModelRequest {
    ModelRequest {
        context: AgentContext {
            instructions: "Be concise.".into(),
            messages: vec![Message::User {
                content: "Add one.".into(),
            }],
        },
        tools: if tool {
            vec![ToolSpec {
                name: swarmie_models::ToolName::new("counter_add").unwrap(),
                description: "Add one".into(),
                input_schema: json!({"type":"object","properties":{"amount":{"type":"integer"}},"required":["amount"]}),
            }]
        } else {
            vec![]
        },
    }
}

pub fn wire(provider: Provider, tool: bool) -> Value {
    match provider {
        Provider::OpenAi | Provider::DeepSeek => {
            json!({"id":"chat_fixture","object":"chat.completion","model":"fixture-model","choices":[{"index":0,"message":if tool{json!({"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"counter_add","arguments":"{\"amount\":1}"}}]})}else{json!({"role":"assistant","content":"Done"})},"finish_reason":if tool{"tool_calls"}else{"stop"}}],"usage":{"prompt_tokens":7,"completion_tokens":3,"total_tokens":10}})
        }
        Provider::OpenAiResponses => {
            json!({"id":"resp_fixture","object":"response","model":"fixture-model","status":"completed","output":if tool{json!([{"type":"function_call","id":"fc_fixture","call_id":"call_1","name":"counter_add","arguments":"{\"amount\":1}","status":"completed"}])}else{json!([{"type":"message","id":"msg_fixture","status":"completed","role":"assistant","content":[{"type":"output_text","text":"Done","annotations":[]}]}])},"usage":{"input_tokens":7,"output_tokens":3,"total_tokens":10}})
        }
        Provider::Anthropic => {
            json!({"id":"msg_fixture","type":"message","role":"assistant","model":"fixture-model","content":if tool{json!([{"type":"tool_use","id":"call_1","name":"counter_add","input":{"amount":1}}])}else{json!([{"type":"text","text":"Done"}])},"stop_reason":if tool{"tool_use"}else{"end_turn"},"usage":{"input_tokens":7,"output_tokens":3}})
        }
        Provider::Gemini => {
            json!({"candidates":[{"content":{"role":"model","parts":if tool{json!([{"functionCall":{"name":"counter_add","args":{"amount":1}}}])}else{json!([{"text":"Done"}])}},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":7,"candidatesTokenCount":3,"totalTokenCount":10},"modelVersion":"fixture-model"})
        }
        Provider::Ollama => {
            json!({"model":"fixture-model","message":if tool{json!({"role":"assistant","content":"","tool_calls":[{"function":{"name":"counter_add","arguments":{"amount":1}}}]})}else{json!({"role":"assistant","content":"Done"})},"done":true,"done_reason":if tool{"tool_calls"}else{"stop"},"prompt_eval_count":7,"eval_count":3})
        }
    }
}
pub const PROVIDERS: [Provider; 6] = [
    Provider::OpenAi,
    Provider::OpenAiResponses,
    Provider::Anthropic,
    Provider::Gemini,
    Provider::Ollama,
    Provider::DeepSeek,
];

pub async fn call<A: 'static, R: 'static>(
    host: &Host,
    reference: Rc<Reference>,
    principal: &str,
    method: &str,
    args: A,
) -> tembra::Result<R> {
    let method = method.to_string();
    host.root(principal, move |ctx| {
        async move { reference.call(&ctx, &method, args).await }.boxed_local()
    })
    .await
}

pub async fn close(host: &Host) {
    host.close().await.unwrap();
    let diagnostics = host.kernel().diagnostics();
    assert_eq!(diagnostics.roots, 0);
    assert_eq!(diagnostics.calls, 0);
    assert_eq!(diagnostics.resources, 0);
}

#[derive(Default)]
pub struct Counter(pub Cell<usize>);
impl swarmie_agent_loop::Tools for Counter {
    fn validate<'a>(
        &'a self,
        context: &'a Context,
        _: &'a ToolSpec,
        call: &'a ToolCall,
    ) -> futures::future::LocalBoxFuture<'a, tembra::Result<ToolDecision>> {
        async move {
            context.check()?;
            assert_eq!(call.arguments, json!({"amount":1}));
            Ok(ToolDecision::Allow)
        }
        .boxed_local()
    }
    fn execute<'a>(
        &'a self,
        context: &'a Context,
        _: swarmie_agent_loop::ToolExecution<'a>,
    ) -> futures::future::LocalBoxFuture<'a, tembra::Result<ToolResult>> {
        async move {
            context.check()?;
            self.0.set(self.0.get() + 1);
            Ok(ToolResult::Success(json!(1)))
        }
        .boxed_local()
    }
}
