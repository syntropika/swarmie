use super::{
    History,
    checkpoint::{Checkpoint, Phase, Record},
    writer::Progress,
};
use crate::{AgentContext, Message, State, model::invalid};
use swarmie_conversations::{EntryId, Head, Method, Read};
use tembra::Context;

struct Loaded {
    messages: Vec<Message>,
    initial: usize,
    assistant: Option<EntryId>,
    prepared: Option<crate::ModelRequest>,
    prepared_tool: Option<(crate::ToolCall, Option<crate::ToolProvider>)>,
}

impl History {
    /// Resolves only model messages at the selected immutable prefix. Stored
    /// prepared requests remain separate evidence and do not enter AgentContext.
    pub async fn context(
        &self,
        context: &Context,
        at: Head,
        instructions: String,
    ) -> tembra::Result<AgentContext> {
        let loaded = self.load(context, at, at, None, None).await?;
        Ok(AgentContext {
            instructions,
            messages: loaded.messages,
        })
    }

    async fn load(
        &self,
        context: &Context,
        at: Head,
        initial: Head,
        prepared: Option<(crate::OperationId, EntryId)>,
        prepared_tool: Option<(crate::OperationId, Option<EntryId>)>,
    ) -> tembra::Result<Loaded> {
        context.check()?;
        if at.conversation != initial.conversation {
            return Err(invalid("checkpoint changed conversation"));
        }
        if initial.entry.is_none() {
            let conversation: Option<swarmie_conversations::Conversation> = self
                .reference()?
                .call(context, Method::Get.as_str(), (initial.conversation,))
                .await?;
            if conversation
                .ok_or_else(|| invalid("initial conversation is missing"))?
                .origin
                .is_some()
            {
                return Err(invalid("a branch has no empty inherited prefix"));
            }
        }
        let mut read = Read::new(at);
        let mut loaded = Loaded {
            messages: vec![],
            initial: 0,
            assistant: None,
            prepared: None,
            prepared_tool: None,
        };
        let mut found_initial = initial.entry.is_none();
        loop {
            let page: swarmie_conversations::Page = self
                .reference()?
                .call(context, Method::Read.as_str(), read.clone())
                .await?;
            for entry in &page.entries {
                let record: Record = entry.decode()?;
                match record {
                    Record::Message(message) => {
                        if matches!(message, Message::Assistant(_)) {
                            loaded.assistant = Some(entry.id)
                        }
                        loaded.messages.push(message);
                    }
                    Record::PreparedRequest { operation, request } => {
                        if prepared == Some((operation, entry.id)) {
                            loaded.prepared = Some(request)
                        }
                    }
                    Record::PreparedTool {
                        operation,
                        call,
                        provider,
                    } => {
                        if let Some((pending, selected)) = prepared_tool
                            && operation == pending
                        {
                            if selected != Some(entry.id) {
                                return Err(invalid(
                                    "missing or ambiguous prepared tool reference",
                                ));
                            }
                            loaded.prepared_tool = Some((call, provider))
                        }
                    }
                }
                if Some(entry.id) == initial.entry {
                    loaded.initial = loaded.messages.len();
                    found_initial = true;
                }
            }
            let Some(next) = page.next else { break };
            if Some(next) == read.after || page.entries.last().map(|entry| entry.id) != Some(next) {
                return Err(invalid("history provider returned an invalid cursor"));
            }
            read.after = Some(next);
        }
        if !found_initial {
            return Err(invalid("initial history point is not visible"));
        }
        if prepared.is_some() && loaded.prepared.is_none() {
            return Err(invalid(
                "prepared request is missing or belongs to another operation",
            ));
        }
        if prepared_tool.is_some_and(|(_, entry)| entry.is_some()) && loaded.prepared_tool.is_none()
        {
            return Err(invalid(
                "prepared tool is missing or belongs to another operation",
            ));
        }
        Ok(loaded)
    }

    pub(super) async fn restore(
        &self,
        context: &Context,
        checkpoint: Checkpoint,
    ) -> tembra::Result<(State, Progress)> {
        let selected = match checkpoint.phase {
            Phase::ModelPending { operation, request } => Some((operation, request)),
            _ => None,
        };
        let selected_tool = match checkpoint.phase {
            Phase::ToolPending {
                operation,
                prepared,
                ..
            } => Some((operation, prepared)),
            _ => None,
        };
        let loaded = self
            .load(
                context,
                checkpoint.head,
                checkpoint.input.at,
                selected,
                selected_tool,
            )
            .await?;
        let phase = match &checkpoint.phase {
            Phase::PrepareTurn => crate::Phase::PrepareTurn,
            Phase::ModelPending { operation, .. } => crate::Phase::ModelPending {
                operation: *operation,
                request: loaded
                    .prepared
                    .ok_or_else(|| invalid("missing model request"))?,
            },
            Phase::PrepareTools { assistant, next }
            | Phase::ToolPending {
                assistant, next, ..
            } => {
                if loaded.assistant != Some(*assistant) {
                    return Err(invalid("pending tools reference a different assistant"));
                }
                let calls = loaded
                    .messages
                    .iter()
                    .rev()
                    .find_map(|message| match message {
                        Message::Assistant(message) => Some(message.tool_calls.clone()),
                        _ => None,
                    })
                    .ok_or_else(|| invalid("pending tools have no assistant"))?;
                match checkpoint.phase {
                    Phase::ToolPending { operation, .. } => {
                        let (call, provider) = loaded
                            .prepared_tool
                            .or_else(|| calls.get(*next).cloned().map(|call| (call, None)))
                            .ok_or_else(|| invalid("invalid pending tool position"))?;
                        crate::Phase::ToolPending {
                            operation,
                            call,
                            provider,
                            calls,
                            next: *next,
                        }
                    }
                    _ => crate::Phase::PrepareTools { calls, next: *next },
                }
            }
            Phase::Finished { reason } => crate::Phase::Finished { reason: *reason },
        };
        let count = loaded.messages.len();
        let state = State {
            request: crate::Request {
                context: AgentContext {
                    instructions: checkpoint.input.instructions.clone(),
                    messages: loaded.messages,
                },
                tools: checkpoint.input.tools.clone(),
                max_turns: checkpoint.input.max_turns,
            },
            initial_messages: loaded.initial,
            turn: checkpoint.turn,
            usage: checkpoint.usage,
            available_tools: checkpoint.available_tools.clone(),
            phase,
        };
        state.validate()?;
        Ok((
            state,
            Progress {
                checkpoint,
                messages: count,
                assistant: loaded.assistant,
            },
        ))
    }
}
