//! Native context admission and execution on the existing conversation task owner.
mod admission;
mod execution;

use super::{Session, failure};
use serde::{Deserialize, Serialize};
use swarmie_context::{Checkpoint, Configuration, WorkFence};
use swarmie_conversations::{Head, HistoryPoint};
use swarmie_durable_tasks::TaskId;
use swarmie_providers::{ModelRequest, ToolSpec};
use tembra::Context;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Ledger {
    pub checkpoint: Checkpoint,
    pub fence: WorkFence,
    pub task: Option<TaskId>,
    pub automatic: bool,
    pub operation_fence: Option<WorkFence>,
}

impl Ledger {
    pub fn new() -> tembra::Result<Self> {
        Ok(Self {
            checkpoint: Checkpoint::default(),
            fence: uuid::Uuid::new_v4().try_into()?,
            task: None,
            automatic: false,
            operation_fence: None,
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Work {
    session: String,
    account: String,
    model: String,
    effort: String,
    instructions: String,
    sources: Option<swarmie_providers::ContextSources>,
    tools: Vec<ToolSpec>,
    head: Head,
    initial: Checkpoint,
    required: bool,
    request: swarmie_context::durable::Input,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Progress {
    head: Head,
    checkpoint: Checkpoint,
}

pub(super) use execution::{handler, instructions};

fn source_view(
    mut view: swarmie_providers::AgentContext,
    sources: Option<swarmie_providers::ContextSources>,
) -> tembra::Result<swarmie_providers::AgentContext> {
    if let Some(sources) = sources {
        let before = view
            .messages
            .iter()
            .rposition(|message| matches!(message, swarmie_providers::Message::User { .. }))
            .ok_or_else(|| failure("captured project context has no admitted User"))?;
        view.sources = Some(sources.at(before));
        view.validate_sources()?;
    }
    Ok(view)
}

pub(super) fn configuration(
    model: &swarmie_providers::ai::AiModel,
    instructions: &str,
    tools: &[ToolSpec],
) -> tembra::Result<Configuration> {
    use sha2::{Digest, Sha256};
    let metadata = model
        .metadata()
        .ok_or_else(|| failure("qualified model metadata is unavailable"))?;
    let bytes = serde_json::to_vec(&(metadata.generation(), instructions, tools))
        .map_err(|_| failure("context configuration cannot be encoded"))?;
    Ok(Configuration(Sha256::digest(bytes).into()))
}

impl Session {
    pub(super) fn context_ledger(&self) -> tembra::Result<Ledger> {
        let record = self
            .index
            .get(&self.id)?
            .ok_or_else(|| failure("conversation metadata is missing"))?;
        if record.principal != self.principal || record.node != self.node {
            return Err(failure("conversation context belongs to another owner"));
        }
        if let Some(ledger) = record.context {
            return Ok(ledger);
        }
        let ledger = Ledger::new()?;
        self.save_context_ledger(ledger.clone())?;
        Ok(ledger)
    }

    pub(super) fn save_context_ledger(&self, ledger: Ledger) -> tembra::Result<()> {
        self.index
            .change(&self.id, |record| record.context = Some(ledger))
    }

    async fn context_prefix(&self, context: &Context, at: Head) -> tembra::Result<HistoryPoint> {
        use swarmie_agent_loop::conversation::Record;
        let mut read = swarmie_conversations::Read::new(at);
        let mut previous = None;
        let mut selected = None;
        loop {
            let page = self.history.read(context, read.clone()).await?;
            for entry in page.entries {
                if let Record::Message(message) = entry.decode::<Record>()? {
                    if matches!(message, swarmie_providers::Message::User { .. }) {
                        selected = previous;
                    }
                    previous = Some(entry.id);
                }
            }
            match page.next {
                Some(next) => read.after = Some(next),
                None => break,
            }
        }
        Ok(HistoryPoint {
            conversation: at.conversation,
            entry: selected.ok_or_else(|| {
                failure("an earlier complete conversation prefix is required for compaction")
            })?,
        })
    }

    async fn projected_context(
        &self,
        context: &Context,
        at: Head,
        instructions: String,
        checkpoint: &Checkpoint,
        configuration: Configuration,
    ) -> tembra::Result<swarmie_providers::AgentContext> {
        if let Some(projection) = checkpoint.active_for(configuration) {
            self.agent_history
                .context_with_projection(context, at, instructions, projection.clone())
                .await
        } else {
            self.agent_history.context(context, at, instructions).await
        }
    }

    async fn context_request(
        &self,
        context: &Context,
        at: Head,
        instructions: String,
        tools: Vec<ToolSpec>,
        checkpoint: &Checkpoint,
        configuration: Configuration,
    ) -> tembra::Result<ModelRequest> {
        Ok(ModelRequest {
            context: self
                .projected_context(context, at, instructions, checkpoint, configuration)
                .await?,
            tools,
        })
    }
}
