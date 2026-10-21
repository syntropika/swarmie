//! A synthetic provider implementing the exact neutral Model Contract method schemas.

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use swarmie_model_contracts::*;
use tembra::{core, prelude::*};

#[tembra::contract(local, name = "swarmie.acceptance-agent-model", version = 1)]
trait FixtureModel {
    #[portable]
    async fn capabilities(&self, model: ModelIdentity) -> Result<Capabilities>;

    #[portable]
    async fn models(&self, connection: String) -> Result<ModelList>;

    #[portable]
    async fn generate(&self, request: ModelRequest) -> Result<GenerateResult>;
}

/// Deterministic responses exercise agent control flow without network credentials.
#[derive(Clone)]
pub enum Reply {
    Text(String),
    Tool {
        name: String,
        arguments: serde_json::Value,
    },
    Error,
    Hang,
}

impl Reply {
    pub fn text(text: &str) -> Self {
        Self::Text(text.into())
    }

    pub fn double(value: i64) -> Self {
        Self::Tool {
            name: "fixture.double".into(),
            arguments: serde_json::json!({"value":value}),
        }
    }
}

/// Captures requests at the actual managed provider boundary.
#[derive(Clone, Default)]
pub struct ModelProbe {
    pub requests: Rc<RefCell<Vec<ModelRequest>>>,
    pub admission_log: Rc<RefCell<Option<std::path::PathBuf>>>,
}

/// Provides scripted replies through an ordinary Tembra-owned provider Facet.
pub fn facet(replies: Vec<Reply>, probe: ModelProbe) -> Result<NativeFacet> {
    let replies = Rc::new(RefCell::new(VecDeque::from(replies)));
    NativeFacet::new("fixture.agent-model", vec![], move |_, setup| {
        let replies = replies.clone();
        let probe = probe.clone();
        let generation_replies = replies.clone();
        let generation_probe = probe.clone();
        setup.serve(
            FixtureGeneration,
            move |input: GenerationInput, _, declaration| {
                let request = input.request;
                Ok(Generation {
                    state: declaration.state(
                        "progress",
                        GenerationState {
                            request_id: request.request_id.clone(),
                            phase: GenerationPhase::Prepared,
                            acknowledged: 0,
                            last_sequence: 0,
                            events: vec![],
                            response: None,
                            error: None,
                            physically_settled: false,
                        },
                    )?,
                    request,
                    replies: generation_replies.clone(),
                    probe: generation_probe.clone(),
                    pending: RefCell::new(None),
                })
            },
        )?;
        setup.serve_with_options(
            FixtureModel,
            move |_, _, declaration| {
                declaration.core_mut().register_method(
                    &model_providers()?,
                    "fixture",
                    serde_json::to_value(ProviderRegistration {
                        provider: "fixture".into(),
                        version: 1,
                        provider_contract: "swarmie.acceptance-agent-model".into(),
                        generation_contract: "swarmie.acceptance-agent-generation".into(),
                        connections: vec!["fixture".into()],
                    })
                    .map_err(|error| {
                        RuntimeError::invalid("fixture_model_metadata", error.to_string())
                    })?,
                    "generate",
                )?;
                Ok(Model {
                    replies: replies.clone(),
                    probe: probe.clone(),
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

struct Model {
    replies: Rc<RefCell<VecDeque<Reply>>>,
    probe: ModelProbe,
}

impl FixtureModelProvider for Model {
    async fn capabilities(&self, _model: ModelIdentity, _context: Context) -> Result<Capabilities> {
        Ok(Capabilities {
            streaming: Support::Supported,
            tools: Support::Supported,
            json: Support::Supported,
            json_schema: Support::Supported,
            reasoning: Support::Unsupported,
            images: Support::Unsupported,
            developer_role: Support::Supported,
            model_discovery: Support::Supported,
        })
    }

    async fn models(&self, connection: String, _context: Context) -> Result<ModelList> {
        Ok(ModelList::Models {
            models: vec![ModelIdentity {
                connection,
                model: "scripted".into(),
            }],
        })
    }

    async fn generate(&self, request: ModelRequest, _context: Context) -> Result<GenerateResult> {
        self.probe.requests.borrow_mut().push(request.clone());
        let reply = self.replies.borrow_mut().pop_front().ok_or_else(|| {
            RuntimeError::invalid(
                "fixture_model_exhausted",
                "the model script has no remaining response",
            )
        })?;
        if matches!(reply, Reply::Hang) {
            return futures::future::pending().await;
        }
        Ok(scripted_response(request, reply))
    }
}

#[tembra::contract(local, name = "swarmie.acceptance-agent-generation", version = 1, input = GenerationInput)]
trait FixtureGeneration {
    #[state]
    fn progress(&self) -> GenerationState;

    #[portable]
    async fn start(&self) -> Result<()>;

    #[portable]
    async fn acknowledge(&self, through: u64) -> Result<()>;

    #[portable]
    async fn cancel(&self) -> Result<()>;
}

struct Generation {
    request: ModelRequest,
    state: Cell<GenerationState>,
    replies: Rc<RefCell<VecDeque<Reply>>>,
    probe: ModelProbe,
    pending: RefCell<Option<GenerateResult>>,
}

impl FixtureGenerationProvider for Generation {
    fn progress(&self) -> Readable<GenerationState> {
        self.state.readable()
    }

    async fn start(&self, context: Context) -> Result<()> {
        if self.state.readable().value()?.phase != GenerationPhase::Prepared {
            return Ok(());
        }
        self.probe.requests.borrow_mut().push(self.request.clone());
        if let Some(path) = self.probe.admission_log.borrow().as_ref() {
            use std::io::Write;
            let mut file = std::fs::File::create(path)
                .map_err(|error| RuntimeError::invalid("fixture_model_log", error.to_string()))?;
            writeln!(file, "{}", self.request.request_id)
                .and_then(|()| file.sync_all())
                .map_err(|error| RuntimeError::invalid("fixture_model_log", error.to_string()))?;
        }
        let reply = self.replies.borrow_mut().pop_front().ok_or_else(|| {
            RuntimeError::invalid(
                "fixture_model_exhausted",
                "the model script has no remaining response",
            )
        })?;
        if matches!(reply, Reply::Hang) {
            return self
                .state
                .change(&context, |state| {
                    state.phase = GenerationPhase::Running;
                    Ok(())
                })
                .map(|_| ());
        }
        let result = scripted_response(self.request.clone(), reply);
        let event = match &result {
            GenerateResult::Response { response } => match response.message.content.first() {
                Some(ContentPart::Text { text }) => {
                    Some(ModelEvent::TextDelta { text: text.clone() })
                }
                Some(ContentPart::ToolCall { call }) => Some(ModelEvent::ToolCallSnapshot {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments_json: call.arguments.to_string(),
                }),
                _ => None,
            },
            GenerateResult::Error { .. } => None,
        };
        *self.pending.borrow_mut() = Some(result);
        self.state.change(&context, |state| {
            state.phase = GenerationPhase::Running;
            if let Some(event) = event {
                state.last_sequence = 1;
                state.events.push(SequencedEvent { sequence: 1, event });
            }
            Ok(())
        })?;
        // A generation without deltas can settle immediately. Otherwise acknowledgement
        // drives the next publication, exercising actual state consumption and backpressure.
        if self.state.readable().value()?.events.is_empty() {
            self.settle(&context)?;
        }
        Ok(())
    }

    async fn acknowledge(&self, through: u64, context: Context) -> Result<()> {
        let current = self.state.readable().value()?;
        if through < current.acknowledged || through > current.last_sequence {
            return Err(RuntimeError::invalid(
                "fixture_ack",
                "invalid streaming acknowledgement",
            ));
        }
        self.state.change(&context, |state| {
            state.acknowledged = through;
            state.events.retain(|event| event.sequence > through);
            Ok(())
        })?;
        self.settle(&context)
    }

    async fn cancel(&self, context: Context) -> Result<()> {
        self.pending.borrow_mut().take();
        self.state
            .change(&context, |state| {
                state.phase = GenerationPhase::Settled;
                state.physically_settled = true;
                state.error = Some(ProviderError {
                    kind: ProviderErrorKind::Cancelled,
                    submission: Submission::MayHaveBeenSent,
                    http_status: None,
                    retry_after_ms: None,
                });
                Ok(())
            })
            .map(|_| ())
    }
}

impl Generation {
    fn settle(&self, context: &Context) -> Result<()> {
        let Some(result) = self.pending.borrow_mut().take() else {
            return Ok(());
        };
        self.state
            .change(context, |state| {
                state.phase = GenerationPhase::Settled;
                state.physically_settled = true;
                match result {
                    GenerateResult::Response { response } => state.response = Some(*response),
                    GenerateResult::Error { error } => state.error = Some(error),
                }
                Ok(())
            })
            .map(|_| ())
    }
}

fn scripted_response(request: ModelRequest, reply: Reply) -> GenerateResult {
    let (message, finish_reason) = match reply {
        Reply::Text(text) => (Message::text(Role::Assistant, text), FinishReason::Stop),
        Reply::Tool { name, arguments } => (
            Message {
                role: Role::Assistant,
                content: vec![ContentPart::ToolCall {
                    call: ToolCall {
                        id: format!("{}:call", request.request_id),
                        name,
                        arguments,
                        extensions: vec![],
                    },
                }],
            },
            FinishReason::ToolCalls,
        ),
        Reply::Error | Reply::Hang => {
            return GenerateResult::Error {
                error: ProviderError {
                    kind: ProviderErrorKind::Transport,
                    submission: Submission::MayHaveBeenSent,
                    http_status: None,
                    retry_after_ms: None,
                },
            };
        }
    };
    GenerateResult::Response {
        response: Box::new(ModelResponse {
            request_id: request.request_id,
            model: request.model,
            message,
            finish_reason,
            raw_finish_reason: None,
            usage: TokenUsage {
                input_tokens: Some(2),
                output_tokens: Some(1),
                total_tokens: Some(3),
                ..Default::default()
            },
            provider_response_id: None,
            extensions: vec![],
        }),
    }
}
