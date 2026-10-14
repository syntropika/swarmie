use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::json;
use swarmie::*;
use tokio::sync::mpsc;

struct EchoHarness;
struct EchoBackend(mpsc::Sender<BackendEvent>);
#[async_trait]
impl Harness for EchoHarness {
    fn id(&self) -> &str {
        "external-echo"
    }
    async fn detect(&self) -> Result<Detection> {
        Ok(Detection {
            harness: self.id().into(),
            available: true,
            executable: None,
            capabilities: Capabilities::default(),
        })
    }
    async fn connect(
        &self,
        _: &SpawnOptions,
        saved: Option<&NativeSessionRef>,
        fork: bool,
    ) -> Result<BackendConnection> {
        if saved.is_some() || fork {
            return Err(Error::Unsupported("echo resume/fork".into()));
        }
        let (tx, events) = mpsc::channel(16);
        Ok(BackendConnection {
            backend: Box::new(EchoBackend(tx)),
            native: NativeSessionRef {
                provider: self.id().into(),
                id: "demo".into(),
            },
            capabilities: Capabilities::default(),
            events,
        })
    }
}
#[async_trait]
impl AgentBackend for EchoBackend {
    async fn prompt(&self, prompt: &Prompt) -> Result<()> {
        self.0
            .send(BackendEvent::content(
                EventKind::TextDelta {
                    text: prompt.text()?,
                    item_id: None,
                },
                json!({"echo":true}),
            ))
            .await
            .map_err(|_| Error::Closed)?;
        self.0
            .send(BackendEvent::TurnFinished {
                outcome: TurnOutcome::Completed,

                native_reason: None,
                raw: None,
            })
            .await
            .map_err(|_| Error::Closed)?;
        Ok(())
    }
    async fn cancel(&self) -> Result<()> {
        Err(Error::Unsupported("echo cancellation".into()))
    }
    async fn respond_permission(&self, _: &PermissionResponse) -> Result<()> {
        Err(Error::Unsupported("echo permissions".into()))
    }
    async fn close(&self) -> Result<()> {
        Ok(())
    }
    async fn kill(&self) -> Result<()> {
        Ok(())
    }
}
struct Prefix;
#[async_trait]
impl RuntimeExtension for Prefix {
    fn id(&self) -> &str {
        "prompt-prefix"
    }
    async fn before_prompt(&self, _: &SessionSnapshot, prompt: &mut Prompt) -> Result<()> {
        *prompt = format!("External extension: {}", prompt.text()?)
            .as_str()
            .into();
        Ok(())
    }
}
#[tokio::main]
async fn main() -> Result<()> {
    let runtime = Runtime::builder()
        .harness(EchoHarness)
        .extension(Prefix)
        .build();
    let agent = runtime
        .spawn(SpawnOptions::new("external-echo", "."))
        .await?;
    let mut events = agent.events();
    agent.prompt("Hello runtime").await?;
    while let Some(event) = events.next().await {
        let event = event?;
        println!("{}", serde_json::to_string(&event)?);
        if matches!(event.kind, EventKind::TurnEnded { .. }) {
            break;
        }
    }
    agent.close().await?;
    while let Some(event) = events.next().await {
        println!("{}", serde_json::to_string(&event?)?);
    }
    Ok(())
}
