//! A synthetic provider implementing the exact neutral Model Contract method schemas.

use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    rc::Rc,
};

use futures::FutureExt;
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
    OpaqueTool,
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
    pub reject_terminal: Rc<std::cell::Cell<bool>>,
    pub suppress_publication_error: Rc<std::cell::Cell<bool>>,
    pub publication_failures: Rc<std::cell::Cell<u32>>,
    pub recovery_calls: Rc<std::cell::Cell<u32>>,
    pub cancel_calls: Rc<std::cell::Cell<u32>>,
    pub physical_gate: Rc<RefCell<Option<crate::gate::AgentGateRef>>>,
    pub dropped: Rc<std::cell::Cell<u32>>,
    pub premature_drop: Rc<std::cell::Cell<u32>>,
    pub backend_closes: Rc<std::cell::Cell<u32>>,
    pub stream_events: Rc<std::cell::Cell<u32>>,
    pub close_barrier: Rc<RefCell<Option<futures::channel::oneshot::Receiver<()>>>>,
    pub close_publication_failures: Rc<std::cell::Cell<u32>>,
}

/// Provides scripted replies through an ordinary Tembra-owned provider Facet.
pub fn facet(replies: Vec<Reply>, probe: ModelProbe) -> Result<NativeFacet> {
    scripted_facet(Scripts::Ordered(VecDeque::from(replies)), probe)
}

/// Independent turn scripts selected by the actual request's model identity.
/// Each turn must use a unique model name; unknown names and exhausted scripts fail closed.
pub fn routed_facet(
    replies: BTreeMap<String, Vec<Reply>>,
    probe: ModelProbe,
) -> Result<NativeFacet> {
    scripted_facet(
        Scripts::Routed(
            replies
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect(),
        ),
        probe,
    )
}

enum Scripts {
    Ordered(VecDeque<Reply>),
    Routed(BTreeMap<String, VecDeque<Reply>>),
}

impl Scripts {
    fn next(&mut self, request: &ModelRequest) -> Result<Reply> {
        let queue = match self {
            Self::Ordered(queue) => Some(queue),
            Self::Routed(scripts) => scripts.get_mut(&request.model.model),
        };
        queue.and_then(VecDeque::pop_front).ok_or_else(|| {
            RuntimeError::invalid(
                "fixture_model_exhausted",
                "no response for this request's script",
            )
        })
    }
}

fn scripted_facet(scripts: Scripts, probe: ModelProbe) -> Result<NativeFacet> {
    let replies = Rc::new(RefCell::new(scripts));
    NativeFacet::new("fixture.agent-model", vec![], move |scope, setup| {
        let replies = replies.clone();
        let probe = probe.clone();
        let generation_replies = replies.clone();
        let generation_probe = probe.clone();
        let generation_scope = scope.clone();
        let progress_definition = setup.state("progress")?;
        let terminal_publication_guard_definition = setup.state("terminal-publication-guard")?;
        setup.serve(
            FixtureGeneration,
            move |input: GenerationInput, _, declaration| {
                let request = input.request;
                let state = declaration.state(progress_definition.value(GenerationState {
                    request_id: request.request_id.clone(),
                    phase: GenerationPhase::Prepared,
                    acknowledged: 0,
                    last_sequence: 0,
                    events: vec![],
                    response: None,
                    error: None,
                    physically_settled: false,
                })?)?;
                let rejection = generation_probe.clone();
                let _projection = declaration.derive(
                    &terminal_publication_guard_definition,
                    (state.readable(),),
                    move |(candidate,)| {
                        if rejection.reject_terminal.get() && candidate.physically_settled {
                            rejection
                                .publication_failures
                                .set(rejection.publication_failures.get() + 1);
                            return Err(RuntimeError::invalid(
                                "fixture_terminal_publication",
                                "the owning graph refused terminal State",
                            ));
                        }
                        Ok(true)
                    },
                )?;
                let completion = Rc::new(RefCell::new(None::<GenerationState>));
                let closed_state = state.clone();
                let closed_completion = completion.clone();
                let closed_probe = generation_probe.clone();
                let closing_scope = generation_scope.clone();
                let lifetime = declaration.core_mut().resource(
                    "backend-lifetime",
                    core::ResourceDeclaration::new(|_| async { Ok(()) }.boxed_local()).with_close(
                        move |_| {
                            let state = closed_state.clone();
                            let completion = closed_completion.clone();
                            let probe = closed_probe.clone();
                            let scope = closing_scope.clone();
                            async move {
                                // The managed closer owns the backend after Host retirement has
                                // invalidated public References. It settles before the provider drops.
                                probe.backend_closes.set(probe.backend_closes.get() + 1);
                                if completion.borrow().is_none() {
                                    let barrier = probe.close_barrier.borrow_mut().take();
                                    if let Some(barrier) = barrier {
                                        barrier.await.map_err(|_| {
                                            RuntimeError::invalid(
                                                "fixture_close_barrier",
                                                "physical worker barrier was abandoned",
                                            )
                                        })?;
                                    }
                                    let mut terminal = state.readable().value()?;
                                    terminal.phase = GenerationPhase::Settled;
                                    terminal.physically_settled = true;
                                    terminal.error = Some(ProviderError {
                                        kind: ProviderErrorKind::Cancelled,
                                        submission: Submission::MayHaveBeenSent,
                                        http_status: None,
                                        retry_after_ms: None,
                                    });
                                    *completion.borrow_mut() = Some(terminal.clone());
                                    if scope
                                        .root(move |context| {
                                            async move {
                                                state
                                                    .change(&Context::from_core(context), |state| {
                                                        *state = terminal;
                                                        Ok(())
                                                    })
                                                    .map(|_| ())
                                            }
                                            .boxed_local()
                                        })
                                        .await
                                        .is_err()
                                    {
                                        probe
                                            .close_publication_failures
                                            .set(probe.close_publication_failures.get() + 1);
                                    }
                                }
                                Ok(())
                            }
                            .boxed_local()
                        },
                    ),
                )?;
                Ok(Generation {
                    state,
                    request,
                    replies: generation_replies.clone(),
                    probe: generation_probe.clone(),
                    pending: RefCell::new(None),
                    completion,
                    _lifetime: lifetime,
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
    replies: Rc<RefCell<Scripts>>,
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
        let reply = self.replies.borrow_mut().next(&request)?;
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
    async fn recover(&self) -> Result<GenerationState>;

    #[portable]
    async fn cancel(&self) -> Result<()>;
}

struct Generation {
    request: ModelRequest,
    state: Cell<GenerationState>,
    replies: Rc<RefCell<Scripts>>,
    probe: ModelProbe,
    pending: RefCell<Option<GenerateResult>>,
    completion: Rc<RefCell<Option<GenerationState>>>,
    _lifetime: core::Resource<()>,
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
        let reply = self.replies.borrow_mut().next(&self.request)?;
        if matches!(reply, Reply::Hang) {
            return self
                .state
                .change(&context, |state| {
                    state.phase = GenerationPhase::Running;
                    state.last_sequence = 1;
                    state.events.push(SequencedEvent {
                        sequence: 1,
                        event: ModelEvent::TextDelta {
                            text: "partial streamed answer".into(),
                        },
                    });
                    Ok(())
                })
                .map(|_| ());
        }
        let result = scripted_response(self.request.clone(), reply);
        let events = match &result {
            GenerateResult::Response { response } => response
                .message
                .content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text { text } => {
                        Some(ModelEvent::TextDelta { text: text.clone() })
                    }
                    ContentPart::Reasoning { text } => {
                        Some(ModelEvent::ReasoningDelta { text: text.clone() })
                    }
                    ContentPart::ToolCall { call } => Some(ModelEvent::ToolCallSnapshot {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        arguments_json: call.arguments.to_string(),
                    }),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            GenerateResult::Error { .. } => vec![],
        };
        let events = if events.len() == 1 && self.probe.stream_events.get() > 0 {
            vec![events[0].clone(); self.probe.stream_events.get().min(256) as usize]
        } else {
            events
        };
        *self.pending.borrow_mut() = Some(result);
        self.state.change(&context, |state| {
            state.phase = GenerationPhase::Running;
            for event in events {
                state.last_sequence += 1;
                state.events.push(SequencedEvent {
                    sequence: state.last_sequence,
                    event,
                });
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

    async fn recover(&self, _context: Context) -> Result<GenerationState> {
        self.probe
            .recovery_calls
            .set(self.probe.recovery_calls.get() + 1);
        let mut current = self.state.readable().value()?;
        if let Some(completion) = self.completion.borrow().as_ref() {
            current.phase = completion.phase;
            current.physically_settled = completion.physically_settled;
            current.response.clone_from(&completion.response);
            current.error.clone_from(&completion.error);
        }
        Ok(current)
    }

    async fn cancel(&self, context: Context) -> Result<()> {
        self.probe
            .cancel_calls
            .set(self.probe.cancel_calls.get() + 1);
        let gate = self.probe.physical_gate.borrow().clone();
        if let Some(gate) = gate {
            gate.await_gate().await?;
        }
        if self.completion.borrow().is_some() {
            return Ok(());
        }
        self.pending.borrow_mut().take();
        self.complete(
            GenerateResult::Error {
                error: ProviderError {
                    kind: ProviderErrorKind::Cancelled,
                    submission: Submission::MayHaveBeenSent,
                    http_status: None,
                    retry_after_ms: None,
                },
            },
            &context,
        )
    }
}

impl Generation {
    fn settle(&self, context: &Context) -> Result<()> {
        let Some(result) = self.pending.borrow_mut().take() else {
            return Ok(());
        };
        self.complete(result, context)
    }

    fn complete(&self, result: GenerateResult, context: &Context) -> Result<()> {
        let mut terminal = self.state.readable().value()?;
        terminal.phase = GenerationPhase::Settled;
        terminal.physically_settled = true;
        match result {
            GenerateResult::Response { response } => terminal.response = Some(*response),
            GenerateResult::Error { error } => terminal.error = Some(error),
        }
        *self.completion.borrow_mut() = Some(terminal.clone());
        let publication = self
            .state
            .change(context, |state| {
                *state = terminal;
                Ok(())
            })
            .map(|_| ());
        if self.probe.suppress_publication_error.get() {
            Ok(())
        } else {
            publication
        }
    }
}

impl Drop for Generation {
    fn drop(&mut self) {
        self.probe.dropped.set(self.probe.dropped.get() + 1);
        if !self
            .completion
            .borrow()
            .as_ref()
            .is_some_and(|state| state.physically_settled)
        {
            self.probe
                .premature_drop
                .set(self.probe.premature_drop.get() + 1);
        }
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
        Reply::OpaqueTool => (
            Message {
                role: Role::Assistant,
                content: vec![
                    ContentPart::Reasoning {
                        text: "private thought prose".into(),
                    },
                    ContentPart::Extension {
                        extension: Extension {
                            namespace: "swarmie.continuation.anthropic".into(),
                            version: 2,
                            value: serde_json::json!({"kind":"provider_content","value":{"type":"thinking","thinking":"private thought prose","signature":"signed-opaque-value"}}),
                        },
                    },
                    ContentPart::Text {
                        text: "before tool".into(),
                    },
                    ContentPart::ToolCall {
                        call: ToolCall {
                            id: format!("{}:call", request.request_id),
                            name: "fixture.double".into(),
                            arguments: serde_json::json!({"value":21}),
                            extensions: vec![Extension {
                                namespace: "swarmie.continuation.fixture".into(),
                                version: 1,
                                value: serde_json::json!({"kind":"tool_thought_signatures","value":["opaque-tool-signature"]}),
                            }],
                        },
                    },
                    ContentPart::Extension {
                        extension: Extension {
                            namespace: "swarmie.continuation.anthropic".into(),
                            version: 2,
                            value: serde_json::json!({"kind":"provider_content","value":{"type":"redacted_thinking","data":"redacted-opaque-value"}}),
                        },
                    },
                ],
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

#[cfg(test)]
mod routing_tests {
    use super::*;

    fn request(model: &str, id: &str) -> ModelRequest {
        ModelRequest {
            request_id: id.into(),
            model: ModelIdentity {
                connection: "fixture".into(),
                model: model.into(),
            },
            messages: vec![],
            tools: vec![],
            options: Default::default(),
        }
    }

    #[test]
    fn independent_scripts_survive_interleaving_and_refuse_unknown_or_exhausted_routes() {
        let mut scripts = Scripts::Routed(BTreeMap::from([
            (
                "a".into(),
                VecDeque::from([Reply::double(21), Reply::text("42")]),
            ),
            (
                "b".into(),
                VecDeque::from([Reply::double(7), Reply::text("14")]),
            ),
        ]));
        assert!(
            matches!(scripts.next(&request("a", "a1")).unwrap(), Reply::Tool { arguments, .. } if arguments["value"] == 21)
        );
        assert!(
            matches!(scripts.next(&request("b", "b1")).unwrap(), Reply::Tool { arguments, .. } if arguments["value"] == 7)
        );
        assert!(scripts.next(&request("unknown", "x")).is_err());
        assert!(
            matches!(scripts.next(&request("b", "b2")).unwrap(), Reply::Text(text) if text == "14")
        );
        assert!(
            matches!(scripts.next(&request("a", "a2")).unwrap(), Reply::Text(text) if text == "42")
        );
        assert!(scripts.next(&request("a", "a3")).is_err());
    }
}
