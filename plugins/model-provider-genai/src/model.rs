mod streaming;

use crate::{
    Connection, Credentials, ModelId, conversion,
    error::{Reason, invalid, upstream},
};
use futures::{FutureExt, future::LocalBoxFuture};
use genai::{
    Client,
    resolver::{AuthData, Endpoint, ProviderConfig},
};
use std::time::Duration;
use swarmie_providers::{Model, ModelRequest, ModelResponse, OperationId, Progress, Recovery};
use tembra::{Code, Context, Error};

/// One configured model client. The caller owns direct futures; a native orb
/// delegates physical ownership and resource cleanup to Tembra. No HTTP retry,
/// operation receipt store or independent executor is created here.
pub struct GenaiModel {
    client: Client,
    stream_client: Client,
    connection: Connection,
    model: ModelId,
    reasoning: Option<genai::chat::ReasoningEffort>,
}
impl GenaiModel {
    /// Apply a reasoning value admitted by the embedding's model catalog.
    pub fn with_reasoning(mut self, effort: &str) -> tembra::Result<Self> {
        self.reasoning = Some(
            genai::chat::ReasoningEffort::from_keyword(effort)
                .ok_or_else(|| invalid(Reason::Configuration, "unsupported reasoning effort"))?,
        );
        Ok(self)
    }
    fn request(&self, request: &ModelRequest) -> tembra::Result<genai::chat::ChatRequest> {
        self.budget(
            serde_json::to_vec(request)
                .map_err(|_| invalid(Reason::Request, "request cannot be encoded"))?
                .len(),
            true,
        )?;
        conversion::request(request)
    }

    fn budget(&self, bytes: usize, request: bool) -> tembra::Result<()> {
        let (limit, reason) = if request {
            (self.connection.request_bytes.get(), Reason::Request)
        } else {
            (self.connection.response_bytes.get(), Reason::Response)
        };
        if bytes > limit {
            return Err(Error::new(
                Code::Exhausted,
                reason.as_str(),
                "model payload exceeds its configured decoded limit",
            ));
        }
        Ok(())
    }

    fn response(
        &self,
        response: genai::chat::ChatResponse,
        request: &ModelRequest,
    ) -> tembra::Result<ModelResponse> {
        let output = conversion::response(response, request)?;
        self.budget(
            serde_json::to_vec(&output)
                .map_err(|_| invalid(Reason::Response, "response cannot be encoded"))?
                .len(),
            false,
        )?;
        Ok(output)
    }
    /// Call within the embedding application's Tokio I/O runtime.
    pub fn new(
        connection: Connection,
        model: ModelId,
        credentials: &dyn Credentials,
    ) -> tembra::Result<Self> {
        connection.validate()?;
        if !connection.models.contains(&model) {
            return Err(invalid(
                Reason::Configuration,
                "model is not admitted by this connection",
            ));
        }
        tokio::runtime::Handle::try_current().map_err(|_| {
            Error::new(
                Code::Unavailable,
                Reason::Runtime.as_str(),
                "model HTTP requires the host's Tokio I/O runtime",
            )
        })?;
        let auth = match &connection.credential_key {
            Some(key) => AuthData::from_single(
                credentials
                    .resolve(key)
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| {
                        Error::new(
                            Code::Unavailable,
                            Reason::Credentials.as_str(),
                            "model credential is unavailable",
                        )
                    })?,
            ),
            None => AuthData::None,
        };
        let mut config = ProviderConfig::from_auth(auth);
        if let Some(url) = &connection.endpoint {
            config = config.with_endpoint(Endpoint::from_owned(format!(
                "{}/",
                url.trim_end_matches('/')
            )));
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_millis(
                connection.timeout_ms.get().min(30_000),
            ))
            .build()
            .map_err(|_| {
                Error::new(
                    Code::Unavailable,
                    Reason::Runtime.as_str(),
                    "model HTTP client initialization failed",
                )
            })?;
        let build_client = |http, config| {
            Client::builder()
                .with_reqwest(http)
                .with_adapter_kind(connection.provider.adapter())
                .append_provider_config(connection.provider.adapter(), config)
                .build()
                .map_err(|_| {
                    Error::new(
                        Code::Unavailable,
                        Reason::Runtime.as_str(),
                        "model client initialization failed",
                    )
                })
        };
        let stream_client = build_client(http, config.clone())?;
        let complete_http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_millis(
                connection.timeout_ms.get().min(30_000),
            ))
            .timeout(Duration::from_millis(connection.timeout_ms.get()))
            .build()
            .map_err(|_| invalid(Reason::Runtime, "model HTTP client initialization failed"))?;
        let client = build_client(complete_http, config)?;
        Ok(Self {
            client,
            stream_client,
            connection,
            model,
            reasoning: None,
        })
    }
}
impl Model for GenaiModel {
    fn stream<'a>(
        &'a self,
        context: &'a Context,
        operation: OperationId,
        request: &'a ModelRequest,
        progress: &'a Progress,
    ) -> LocalBoxFuture<'a, tembra::Result<ModelResponse>> {
        async move {
            let physical = streaming::stream(self, context, operation, request, progress);
            let signal = context.signal();
            let cancelled = signal.cancelled();
            futures::pin_mut!(physical, cancelled);
            match futures::future::select(physical, cancelled).await {
                futures::future::Either::Left((output, _)) => output,
                futures::future::Either::Right(((), physical)) => {
                    // Retain the physical future after logical cancellation. A
                    // continuing heartbeat must not extend cleanup indefinitely.
                    let _ = tokio::time::timeout(
                        Duration::from_millis(self.connection.timeout_ms.get()),
                        physical,
                    )
                    .await;
                    context.check()?;
                    Err(Error::new(
                        Code::Cancelled,
                        Reason::Transport.as_str(),
                        "cancelled model stream has no confirmed response",
                    ))
                }
            }
        }
        .boxed_local()
    }

    fn complete<'a>(
        &'a self,
        context: &'a Context,
        _operation: OperationId,
        request: &'a ModelRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<ModelResponse>> {
        async move {
            context.check()?;
            let input = self.request(request)?;
            context.check()?;
            // Await physical HTTP settlement even if the logical waiter cancels.
            // The configured timeout bounds transport waiting; cancellation cannot
            // prove that remote computation or billing stopped.
            let mut options =
                genai::chat::ChatOptions::default().with_infer_reasoning_from_model_name(false);
            if let Some(effort) = self.reasoning.clone() {
                options = options.with_reasoning_effort(effort);
            }
            let response = self
                .client
                .exec_chat(self.model.as_str(), input, Some(&options))
                .await
                .map_err(upstream)?;
            let output = self.response(response, request)?;
            context.check()?;
            Ok(output)
        }
        .boxed_local()
    }
    fn recover<'a>(
        &'a self,
        context: &'a Context,
        _operation: OperationId,
        _request: &'a ModelRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<Recovery<ModelResponse>>> {
        // A local operation ID does not establish remote provider idempotency.
        // genai exposes no portable operation lookup; never infer safe replay.
        async move {
            context.check()?;
            Ok(Recovery::Unknown)
        }
        .boxed_local()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConnectionId, Provider};
    use futures::task::{LocalFutureObj, LocalSpawn, SpawnError};
    use std::{
        io::{Read, Write},
        net::TcpListener,
        rc::Rc,
    };
    use swarmie_providers::{AgentContext, Message};

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
            Some("fixture-secret".into())
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn official_openai_requests_do_not_infer_cache_options_from_catalog_model_names() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                let address = listener.local_addr().unwrap();
                let (sent, received) = std::sync::mpsc::channel();
                let server = std::thread::spawn(move || {
                    for _ in 0..2 {
                        let (mut socket, _) = listener.accept().unwrap();
                        socket
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        let mut bytes = Vec::new();
                        let mut buffer = [0_u8; 4096];
                        let body = loop {
                            let count = socket.read(&mut buffer).unwrap();
                            assert_ne!(count, 0);
                            bytes.extend_from_slice(&buffer[..count]);
                            if let Some(header) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                                let headers = String::from_utf8_lossy(&bytes[..header]);
                                let length: usize = headers
                                    .lines()
                                    .find_map(|line| {
                                        let (key, value) = line.split_once(':')?;
                                        key.eq_ignore_ascii_case("content-length")
                                            .then(|| value.trim().parse().unwrap())
                                    })
                                    .unwrap();
                                if bytes.len() >= header + 4 + length {
                                    break serde_json::from_slice::<serde_json::Value>(
                                        &bytes[header + 4..header + 4 + length],
                                    )
                                    .unwrap();
                                }
                            }
                        };
                        sent.send(body).unwrap();
                        let body = r#"{"error":{"message":"private-secret provider diagnostic"}}"#;
                        write!(
                            socket,
                            "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len(),
                        )
                        .unwrap();
                    }
                });
                let http = reqwest::Client::builder()
                    .proxy(reqwest::Proxy::http(format!("http://{address}")).unwrap())
                    .retry(reqwest::retry::never())
                    .timeout(Duration::from_secs(5))
                    .build()
                    .unwrap();
                let model_id = ModelId::new("gpt-6.1-terra").unwrap();
                let mut connection = Connection::new(
                    ConnectionId::new("fixture").unwrap(),
                    Provider::OpenAiResponses,
                    [model_id.clone()],
                );
                connection.endpoint = Some("http://api.openai.com/v1/".into());
                connection.credential_key = Some("fixture".into());
                let mut model = GenaiModel::new(connection, model_id, &Secrets).unwrap();
                let client = || {
                    Client::builder()
                        .with_reqwest(http.clone())
                        .with_adapter_kind(genai::adapter::AdapterKind::OpenAIResp)
                        .append_provider_config(
                            genai::adapter::AdapterKind::OpenAIResp,
                            ProviderConfig::from_auth(AuthData::from_single("fixture-secret"))
                                .with_endpoint(Endpoint::from_static("http://api.openai.com/v1/")),
                        )
                        .build()
                        .unwrap()
                };
                model.client = client();
                model.stream_client = client();
                let model = Rc::new(model);
                let host = tembra::Host::new(Executor);
                for streaming in [true, false] {
                    let model = model.clone();
                    let result = host
                        .root("fixture", move |ctx| {
                            async move {
                                let input = ModelRequest {
                                    context: AgentContext {
                                        instructions: String::new(),
                                        messages: vec![Message::User {
                                            content: "Reply with OK.".into(),
                                        }],
                                    },
                                    tools: vec![],
                                };
                                if streaming {
                                    model
                                        .stream(
                                            &ctx,
                                            OperationId::new(),
                                            &input,
                                            &Progress::new(|_, _| {}),
                                        )
                                        .await
                                } else {
                                    model.complete(&ctx, OperationId::new(), &input).await
                                }
                            }
                            .boxed_local()
                        })
                        .await;
                    let body = received.recv_timeout(Duration::from_secs(5)).unwrap();
                    assert!(
                        body.get("prompt_cache_options").is_none(),
                        "unsolicited cache options: {body}"
                    );
                    assert!(!body.to_string().contains("prompt_cache_breakpoint"));
                    let error = result.unwrap_err();
                    assert_eq!(error.code, Code::Invalid);
                    assert!(!format!("{error:?}").contains("private-secret"));
                }
                host.close().await.unwrap();
                server.join().unwrap();
            })
            .await;
    }
}
