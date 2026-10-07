//! A deterministic, in-process harness for runtime clients and conformance tests.
use crate::*;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{Mutex, mpsc};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct MockScript {
    pub turns: Vec<MockTurn>,
    pub models: Vec<Value>,
    pub configuration: Vec<Value>,
}
impl MockScript {
    pub fn turn(mut self, steps: Vec<MockStep>) -> Self {
        self.turns.push(MockTurn {
            expected_text: None,
            steps,
        });
        self
    }
    pub fn expect(mut self, text: impl Into<String>, steps: Vec<MockStep>) -> Self {
        self.turns.push(MockTurn {
            expected_text: Some(text.into()),
            steps,
        });
        self
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockTurn {
    #[serde(default)]
    pub expected_text: Option<String>,
    #[serde(default)]
    pub steps: Vec<MockStep>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MockStep {
    Emit {
        kind: Box<EventKind>,
        #[serde(default)]
        raw: Option<Value>,
    },
    Text {
        text: String,
        #[serde(default, rename = "itemId")]
        item_id: Option<String>,
    },
    Reasoning {
        text: String,
    },
    ToolStarted {
        tool: ToolEvent,
    },
    ToolUpdated {
        tool: ToolEvent,
    },
    ToolCompleted {
        tool: ToolEvent,
    },
    Request {
        request: PermissionRequest,
    },
    AwaitAnswer {
        id: String,
        #[serde(default)]
        option: Option<String>,
    },
    Sleep {
        milliseconds: u64,
    },
    End {
        reason: String,
    },
    Die {
        message: String,
    },
}
impl MockStep {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            item_id: Some("mock-message".into()),
        }
    }
}
#[derive(Default)]
struct Observations {
    prompts: Mutex<Vec<Prompt>>,
    answers: Mutex<Vec<PermissionResponse>>,
    launches: Mutex<Vec<SpawnOptions>>,
}
#[derive(Clone)]
pub struct MockHarness {
    id: String,
    script: MockScript,
    observations: Arc<Observations>,
}
impl MockHarness {
    pub fn new(script: MockScript) -> Self {
        Self {
            id: "mock".into(),
            script,
            observations: Arc::default(),
        }
    }
    pub fn named(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }
    pub async fn prompts(&self) -> Vec<Prompt> {
        self.observations.prompts.lock().await.clone()
    }
    pub async fn answers(&self) -> Vec<PermissionResponse> {
        self.observations.answers.lock().await.clone()
    }
    /// Explicit test inspection; credentials are not included in serialized snapshots.
    pub async fn launches(&self) -> Vec<SpawnOptions> {
        self.observations.launches.lock().await.clone()
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            cancel: true,
            permissions: true,
            reasoning: true,
            usage: true,
            images: true,
            models: true,
            configure: true,
            metadata: std::collections::BTreeMap::from([
                ("toolsDisabled".into(), json!(true)),
                ("ephemeral".into(), json!(true)),
                ("outputSchema".into(), json!(true)),
            ]),
            ..Default::default()
        }
    }
}
#[async_trait]
impl Harness for MockHarness {
    fn id(&self) -> &str {
        &self.id
    }
    async fn detect(&self) -> Result<Detection> {
        Ok(Detection {
            harness: self.id.clone(),
            available: true,
            executable: None,
            capabilities: self.capabilities(),
        })
    }
    async fn probe(&self) -> Result<HarnessProbe> {
        Ok(HarnessProbe {
            harness: self.id.clone(),
            available: true,
            executable: None,
            version: Some("in-process".into()),
            auth_state: "unsupported".into(),
            message: Some("Scripted local harness".into()),
            capabilities: self.capabilities(),
        })
    }
    async fn connect(
        &self,
        options: &SpawnOptions,
        native: Option<&NativeSessionRef>,
        fork: bool,
    ) -> Result<BackendConnection> {
        if native.is_some() || fork {
            return Err(Error::Unsupported("mock native resume/fork".into()));
        }
        self.observations
            .launches
            .lock()
            .await
            .push(options.clone());
        let (events, receiver) = mpsc::channel(64);
        Ok(BackendConnection {
            backend: Box::new(MockBackend {
                turns: Mutex::new(self.script.turns.clone().into()),
                active: Mutex::new(None),
                events,
                observations: self.observations.clone(),
                closed: AtomicBool::new(false),
                models: self.script.models.clone(),
                configuration: Mutex::new(self.script.configuration.clone()),
            }),
            native: NativeSessionRef {
                provider: self.id.clone(),
                id: uuid::Uuid::new_v4().to_string(),
            },
            capabilities: self.capabilities(),
            events: receiver,
        })
    }
}
struct Active {
    task: tokio::task::JoinHandle<()>,
    answers: mpsc::Sender<PermissionResponse>,
    running: Arc<AtomicBool>,
}
struct MockBackend {
    turns: Mutex<VecDeque<MockTurn>>,
    active: Mutex<Option<Active>>,
    events: mpsc::Sender<BackendEvent>,
    observations: Arc<Observations>,
    closed: AtomicBool,
    models: Vec<Value>,
    configuration: Mutex<Vec<Value>>,
}
impl Drop for MockBackend {
    fn drop(&mut self) {
        if let Some(active) = self.active.get_mut().take() {
            active.task.abort();
        }
    }
}
#[async_trait]
impl AgentBackend for MockBackend {
    async fn prompt(&self, prompt: &Prompt) -> Result<()> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(Error::Closed);
        }
        let mut active = self.active.lock().await;
        if active
            .as_ref()
            .is_some_and(|active| active.running.load(Ordering::SeqCst))
        {
            return Err(Error::Busy);
        }
        let mut turns = self.turns.lock().await;
        let turn = turns
            .front()
            .ok_or_else(|| Error::Invalid("mock script has no remaining turns".into()))?;
        if let Some(expected) = &turn.expected_text
            && prompt.text()? != *expected
        {
            return Err(Error::Invalid("prompt did not match mock script".into()));
        }
        let turn = turns.pop_front().expect("checked nonempty script");
        self.observations.prompts.lock().await.push(prompt.clone());
        let (answers, receiver) = mpsc::channel(16);
        let events = self.events.clone();
        let running = Arc::new(AtomicBool::new(true));
        let playing = running.clone();
        let task = tokio::spawn(async move {
            if let Err(error) = play(turn.steps, events.clone(), receiver, playing.clone()).await {
                playing.store(false, Ordering::SeqCst);
                let _ = events
                    .send(BackendEvent::content(
                        EventKind::Error {
                            message: error.to_string(),
                        },
                        json!({"mock":true}),
                    ))
                    .await;
                let _ = events
                    .send(BackendEvent::TurnFinished {
                        reason: "failed".into(),
                        raw: None,
                    })
                    .await;
            }
        });
        *active = Some(Active {
            task,
            answers,
            running,
        });
        Ok(())
    }
    async fn cancel(&self) -> Result<()> {
        if let Some(active) = self.active.lock().await.take() {
            active.task.abort();
            let events = self.events.clone();
            // Acceptance never waits behind a full provider event buffer.
            tokio::spawn(async move {
                let _ = events
                    .send(BackendEvent::TurnFinished {
                        reason: "cancelled".into(),
                        raw: None,
                    })
                    .await;
            });
        }
        Ok(())
    }
    async fn respond_permission(&self, response: &PermissionResponse) -> Result<()> {
        let active = self.active.lock().await;
        let active = active
            .as_ref()
            .ok_or_else(|| Error::Invalid("mock has no active question".into()))?;
        active
            .answers
            .send(response.clone())
            .await
            .map_err(|_| Error::Closed)?;
        self.observations
            .answers
            .lock()
            .await
            .push(response.clone());
        Ok(())
    }
    async fn models(&self) -> Result<Value> {
        Ok(json!(self.models))
    }
    async fn configuration(&self) -> Result<Value> {
        Ok(json!(*self.configuration.lock().await))
    }
    async fn configure(&self, config: Value) -> Result<Value> {
        let object = config
            .as_object()
            .ok_or_else(|| Error::Invalid("configuration must be an object".into()))?;
        let mut descriptors = self.configuration.lock().await;
        for (id, value) in object {
            let descriptor = descriptors
                .iter_mut()
                .find(|descriptor| descriptor["id"] == *id)
                .ok_or_else(|| Error::Unsupported(format!("configuration option {id}")))?;
            descriptor["value"] = value.clone();
        }
        Ok(json!({"configuration":*descriptors}))
    }
    async fn close(&self) -> Result<()> {
        self.closed.store(true, Ordering::SeqCst);
        if let Some(active) = self.active.lock().await.take() {
            active.task.abort();
        }
        Ok(())
    }
    async fn kill(&self) -> Result<()> {
        self.close().await
    }
}
async fn play(
    steps: Vec<MockStep>,
    events: mpsc::Sender<BackendEvent>,
    mut answers: mpsc::Receiver<PermissionResponse>,
    running: Arc<AtomicBool>,
) -> Result<()> {
    for step in steps {
        let event = match step {
            MockStep::Emit { kind, raw } => BackendEvent::Content { kind, raw },
            MockStep::Text { text, item_id } => {
                BackendEvent::content(EventKind::TextDelta { text, item_id }, json!({"mock":true}))
            }
            MockStep::Reasoning { text } => {
                BackendEvent::content(EventKind::ReasoningDelta { text }, json!({"mock":true}))
            }
            MockStep::ToolStarted { tool } => {
                BackendEvent::content(EventKind::ToolStarted(tool), json!({"mock":true}))
            }
            MockStep::ToolUpdated { tool } => {
                BackendEvent::content(EventKind::ToolUpdated(tool), json!({"mock":true}))
            }
            MockStep::ToolCompleted { tool } => {
                BackendEvent::content(EventKind::ToolCompleted(tool), json!({"mock":true}))
            }
            MockStep::Request { request } => BackendEvent::content(
                EventKind::PermissionRequested(request),
                json!({"mock":true}),
            ),
            MockStep::AwaitAnswer { id, option } => {
                let answer = answers.recv().await.ok_or(Error::Closed)?;
                if answer.id != id
                    || option
                        .as_ref()
                        .is_some_and(|expected| *expected != answer.option)
                {
                    return Err(Error::Invalid("answer did not match mock script".into()));
                }
                BackendEvent::content(EventKind::Raw, json!({"answer":answer}))
            }
            MockStep::Sleep { milliseconds } => {
                tokio::time::sleep(std::time::Duration::from_millis(milliseconds)).await;
                continue;
            }
            MockStep::End { reason } => {
                running.store(false, Ordering::SeqCst);
                events
                    .send(BackendEvent::TurnFinished { reason, raw: None })
                    .await
                    .map_err(|_| Error::Closed)?;
                return Ok(());
            }
            MockStep::Die { message } => {
                running.store(false, Ordering::SeqCst);
                events
                    .send(BackendEvent::Exited { message })
                    .await
                    .map_err(|_| Error::Closed)?;
                return Ok(());
            }
        };
        events.send(event).await.map_err(|_| Error::Closed)?;
    }
    running.store(false, Ordering::SeqCst);
    events
        .send(BackendEvent::TurnFinished {
            reason: "completed".into(),
            raw: None,
        })
        .await
        .map_err(|_| Error::Closed)
}
