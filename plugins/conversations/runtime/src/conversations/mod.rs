//! Client admission and projection over the existing Agent Loop and Conversations libraries.
mod checkout;
mod context;
mod context_service;
mod execution;
mod index;
mod input;
mod projection;
mod sources;
pub(crate) mod tasks;
mod types;

use futures::FutureExt;
use sha2::{Digest, Sha256};
use std::{cell::RefCell, num::NonZeroU32, path::PathBuf, rc::Rc};
use swarmie_agent_loop::{
    Message as AgentMessage,
    conversation::{History, Record, Request},
};
use swarmie_conversations::{ConversationId, Conversations, EntryDraft, EntryId, Read};
use swarmie_providers::native::NativeProviders;
use swarmie_tools::{Catalog, CatalogService};
use tembra::{Context, Host, Orb, Service, State};
use types::{Metadata, Phase, Receipt, RunResult, Snapshot, Submission};

pub(crate) const SERVICE: &str = "swarmie.conversation.session";
pub(crate) const CATALOG: &str = "swarmie.conversation.catalog";

fn failure(message: &str) -> tembra::Error {
    tembra::Error::new(tembra::Code::Unavailable, "conversation.execution", message)
}

struct Session {
    id: String,
    node: String,
    principal: String,
    history_id: ConversationId,
    history: Conversations,
    agent_history: History,
    tasks: Rc<tasks::Tasks>,
    providers: Rc<NativeProviders>,
    execution: Option<Rc<dyn crate::ConversationExecution>>,
    tools: Rc<CatalogService>,
    catalog: Catalog,
    git: Option<swarmie_git::GitService>,
    environment_configuration: Option<swarmie_context::environment::Configuration>,
    agent_git_authority: Option<std::sync::Arc<dyn swarmie_git::Authority>>,
    index: Rc<index::Index>,
    state: State<Snapshot>,
    running: RefCell<Option<RunningContext>>,
    stop_epoch: std::cell::Cell<u64>,
    serial: tokio::sync::Mutex<()>,
    compaction_running: RefCell<Option<(swarmie_providers::OperationId, swarmie_ai::StopHandle)>>,
}

impl Session {
    async fn close(&self) -> tembra::Result<()> {
        let history = self.history.close().await.map_err(Into::into);
        let tasks = self
            .tasks
            .storage
            .close()
            .await
            .map_err(|error| failure(&error.to_string()));
        history.and(tasks)
    }

    fn enter_execution(&self, context: &Context) -> tembra::Result<Option<crate::ExecutionScope>> {
        self.execution
            .as_ref()
            .map(|owner| owner.enter(context))
            .transpose()
    }

    async fn transcript(&self, context: &Context) -> tembra::Result<Transcript> {
        let metadata = self.index.get(&self.id)?;
        let receipts = metadata
            .as_ref()
            .map_or_else(std::collections::BTreeMap::new, |record| {
                record
                    .receipts
                    .iter()
                    .map(|receipt| {
                        (
                            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, receipt.id.as_bytes())
                                .to_string(),
                            receipt.id.clone(),
                        )
                    })
                    .collect()
            });
        let mut result = Transcript::default();
        let Some(conversation) = self.history.get(context, self.history_id).await? else {
            return Ok(result);
        };
        result.head = Some(conversation.head);
        let mut request = Read::new(conversation.head);
        request.limit = std::num::NonZeroU16::new(32).unwrap();
        loop {
            let page = self.history.read(context, request.clone()).await?;
            for entry in page.entries {
                let record: Record = entry.decode()?;
                if let Record::Message(message) = record {
                    let (role, text) = match message {
                        AgentMessage::User { content } => {
                            if let Some(receipt) = receipts.get(&entry.id.to_string()) {
                                result.admitted.push(receipt.clone());
                            }
                            ("user", content)
                        }
                        AgentMessage::Assistant(message) => {
                            result.pending.extend(message.tool_calls);
                            ("assistant", message.content)
                        }
                        AgentMessage::Tool {
                            call,
                            result: output,
                        } => {
                            result.pending.retain(|pending| pending.id != call);
                            (
                                "tool",
                                serde_json::to_string(&output)
                                    .map_err(|_| failure("the tool result could not be encoded"))?,
                            )
                        }
                    };
                    let mut message = types::Message {
                        id: entry.id.to_string(),
                        role: role.into(),
                        text,
                        truncated: false,
                    };
                    projection::clip(&mut message, 8 * 1024);
                    result.messages.push(message);
                    if result.messages.len() > 24 {
                        result.messages.remove(0);
                        result.omitted += 1;
                    }
                }
            }
            let Some(next) = page.next else {
                break;
            };
            if request.after == Some(next) {
                return Err(failure("the history cursor did not advance"));
            }
            request.after = Some(next);
        }
        Ok(result)
    }

    async fn refresh(&self, context: &Context) -> tembra::Result<()> {
        let transcript = self.transcript(context).await?;
        let metadata = self.index.get(&self.id)?;
        let mut phase = metadata.as_ref().map_or(Phase::Idle, |record| record.phase);
        if self.running.borrow().is_none()
            && (metadata
                .as_ref()
                .is_some_and(|record| record.task.is_some())
                || matches!(phase, Phase::Working | Phase::Interrupted))
        {
            let observed = if let Some(id) = metadata.as_ref().and_then(|record| record.task) {
                self.tasks.observe(context, id).await?
            } else {
                None
            };
            let interrupted = observed.as_ref().is_some_and(|value| {
                value.observation == swarmie_durable_tasks::TaskObservation::Interrupted
            });
            let task = observed.map(|value| value.task);
            let active = !interrupted
                && task.as_ref().is_some_and(|task| {
                    matches!(
                        task.status,
                        swarmie_durable_tasks::TaskStatus::Ready
                            | swarmie_durable_tasks::TaskStatus::Waiting(_)
                    )
                });
            if active {
                // A handler can project its result before the scheduler joins it.
                // Keep that result while the durable record is still Ready.
                if matches!(phase, Phase::Idle | Phase::Working | Phase::Interrupted) {
                    phase = Phase::Working;
                }
            } else {
                phase = if interrupted {
                    Phase::Interrupted
                } else if !transcript.pending.is_empty() {
                    Phase::Unknown
                } else if let Some(task) = &task {
                    match &task.status {
                        swarmie_durable_tasks::TaskStatus::Completed(value) => {
                            match serde_json::from_value::<tasks::Outcome>(value.clone())
                                .map_err(|_| failure("invalid confirmed turn outcome"))?
                            {
                                tasks::Outcome::Settled { phase } => phase,
                                tasks::Outcome::StopRequested => Phase::Stopped,
                            }
                        }
                        swarmie_durable_tasks::TaskStatus::Failed(_) => Phase::Failed,
                        _ => Phase::Stopped,
                    }
                } else {
                    Phase::Stopped
                };
            }
            // Tool reconciliation can finish after the durable turn result. Do not
            // restore its old unknown phase or checkpoint once history is settled.
            if phase == Phase::Unknown && transcript.pending.is_empty() {
                phase = Phase::Stopped;
            }
            let checkpoint = if phase == Phase::Unknown
                && metadata
                    .as_ref()
                    .is_some_and(|record| record.checkpoint.is_none())
            {
                task.and_then(|task| task.checkpoint)
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|_| failure("invalid confirmed task checkpoint"))?
            } else {
                None
            };
            if metadata
                .as_ref()
                .is_some_and(|record| record.phase != phase)
                || checkpoint.is_some()
            {
                self.index.change(&self.id, |record| {
                    record.phase = phase;
                    if record.checkpoint.is_none() {
                        record.checkpoint = checkpoint;
                    }
                })?;
            }
        }

        if phase == Phase::Unknown && transcript.pending.is_empty() {
            phase = Phase::Stopped;
            self.index.change(&self.id, |record| {
                record.phase = phase;
                record.checkpoint = None;
            })?;
        }
        let label = match phase {
            Phase::Unknown => {
                "A tool may have run. Acknowledge its unknown outcome before sending a new message; it will not be replayed."
            }
            Phase::Stopped => "Stopped",
            Phase::Failed => "The request failed. You can send a new message.",
            Phase::Working => "Thinking…",
            Phase::Interrupted => {
                "Execution interrupted. Stop this turn before sending a new message; unfinished effects will not be replayed."
            }
            _ => "",
        };
        self.state.update(context, |current| {
            let mut context_status = current.context.clone();
            if let Some(inspection) = &mut context_status.inspection {
                inspection.stale = transcript.head != Some(inspection.at);
            }
            if phase == Phase::Working {
                context_status.can_compact = false;
            }
            let mut messages = transcript.messages;
            if phase == Phase::Working
                && let Some(preview) = current
                    .messages
                    .last()
                    .filter(|message| message.id == "preview")
            {
                messages.push(preview.clone());
            }
            Ok(projection::bound(Snapshot {
                context: context_status,
                messages,
                admitted: transcript.admitted,
                omitted_messages: transcript.omitted,
                can_acknowledge: phase == Phase::Unknown
                    && !transcript.pending.is_empty()
                    && self.running.borrow().is_none(),
                phase,
                execution_context: metadata
                    .as_ref()
                    .and_then(|record| record.execution_context.clone()),
                lifecycle_warning: metadata
                    .as_ref()
                    .and_then(|record| record.lifecycle_warning.clone())
                    .map(checkout::bounded_warning),
                label: label.into(),
                ..current.clone()
            }))
        })?;
        Ok(())
    }

    async fn settle(&self, context: &Context, acknowledge: bool) -> tembra::Result<()> {
        if self.running.borrow().is_some() {
            return Err(failure("the request is still physically running"));
        }
        self.wait_turn(context).await?;
        let Some(metadata) = self.index.get(&self.id)? else {
            return Err(failure("there is no recorded outcome to acknowledge"));
        };
        if metadata.phase != Phase::Unknown {
            return self.refresh(context).await;
        }
        let tools = checkout::ScopedTools {
            inner: self.tools.clone(),
            binding: metadata.execution_context,
            authority: self.agent_git_authority.clone(),
            project: metadata
                .checkpoint
                .as_ref()
                .and_then(|checkpoint| checkpoint.sources())
                .map(|_| {
                    sources::SkillTools::new(
                        self.tasks.host.clone(),
                        self.id.clone(),
                        self.principal.clone(),
                    )
                })
                .transpose()?,
        };
        if let Some(checkpoint) = metadata.checkpoint {
            let scope = match self.enter_execution(context) {
                Ok(scope) => scope,
                Err(_) if !acknowledge => return self.refresh(context).await,
                Err(error) => return Err(error),
            };
            let context = scope.as_ref().map_or(context, |scope| scope.context());
            let settlement = self
                .agent_history
                .settle_tools(context, &tools, checkpoint, acknowledge, None)
                .await;
            match settlement {
                Err(_) if !acknowledge && context.check().is_ok() => {
                    return self.refresh(context).await;
                }
                Err(error) => return Err(error),
                Ok(swarmie_agent_loop::conversation::ToolSettlement::Unknown(_)) => {
                    return self.refresh(context).await;
                }
                Ok(swarmie_agent_loop::conversation::ToolSettlement::Settled(_)) => {}
            }
        } else {
            // After a process interruption no dispatch receipt may remain. Only explicit
            // acknowledgement may annotate those calls; it cannot assert an effect failed.
            if !acknowledge {
                return self.refresh(context).await;
            }
            let transcript = self.transcript(context).await?;
            if !transcript.pending.is_empty() {
                let head = transcript
                    .head
                    .ok_or_else(|| failure("the conversation is missing"))?;
                let entries = transcript
                    .pending
                    .into_iter()
                    .map(|call| {
                        EntryDraft::encode(Record::Message(AgentMessage::Tool {
                            call: call.id,
                            result: swarmie_agent_loop::ToolResult::Failed(
                                "outcome unknown; not replayed".into(),
                            ),
                        }))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let receipt = self.history.append(context, head, entries).await?;
                if receipt.confirmation != swarmie_conversations::WriteConfirmation::Committed {
                    return Err(failure("tool outcome acknowledgement is not confirmed"));
                }
            }
        }
        self.index.change(&self.id, |record| {
            record.phase = Phase::Stopped;
            record.checkpoint = None;
        })?;
        self.refresh(context).await
    }

    async fn run(&self, context: Context, input: Submission) -> tembra::Result<RunResult> {
        let may_be_admitted = std::cell::Cell::new(false);
        let result = self
            .run_inner(context.clone(), input, &may_be_admitted)
            .await;
        if self.running.borrow().is_none() {
            let _ = self.refresh(&context).await;
            checkout::sync_hold(&self.index, self.git.as_ref(), &self.principal, &self.id).await;
            let _ = self.refresh(&context).await;
        }
        match result {
            Ok(()) => Ok(RunResult::Settled),
            Err(error) if !may_be_admitted.get() => Ok(RunResult::Refused {
                reason: error.message,
            }),
            Err(error) => Err(error),
        }
    }

    async fn observe_existing(
        &self,
        context: &Context,
        input: &Submission,
        digest: &str,
        may_be_admitted: &std::cell::Cell<bool>,
    ) -> tembra::Result<bool> {
        let records = self.index.read()?;
        let Some(existing) =
            records
                .iter()
                .find(|record| record.id == self.id)
                .and_then(|record| {
                    record
                        .receipts
                        .iter()
                        .find(|receipt| receipt.id == input.submission_id)
                })
        else {
            return Ok(false);
        };
        if existing.digest != digest {
            return Err(failure("this submission identity has different arguments"));
        }
        // A stored preparation is never proof of non-admission. Consult history before
        // provider lookup, cancellation or new execution; a duplicate is observation only.
        may_be_admitted.set(true);
        if !self
            .transcript(context)
            .await?
            .admitted
            .contains(&input.submission_id)
        {
            return Err(failure(
                "this preparation has an unconfirmed outcome; it cannot be replayed",
            ));
        }
        let _ = self.state.update(context, |current| {
            let mut next = current.clone();
            next.admitted.retain(|id| id != &input.submission_id);
            next.admitted.push(input.submission_id.clone());
            Ok(projection::bound(next))
        });
        Ok(true)
    }

    async fn wait_turn(&self, context: &Context) -> tembra::Result<()> {
        if let Some(metadata) = self.index.get(&self.id)?
            && let Some(task) = metadata.task
            && self
                .tasks
                .storage
                .get(task)
                .await
                .map_err(|error| failure(&error.to_string()))?
                .is_some()
        {
            let mut observed = self.tasks.wait(context, task).await?;
            if observed == swarmie_durable_tasks::TaskObservation::Interrupted
                && metadata.stop_requested
            {
                // One explicit Stop can readmit this held task once. The handler
                // consumes the persisted intent before model or tool dispatch.
                self.tasks.wake(context, task).await?;
                observed = self.tasks.wait(context, task).await?;
            }
            if observed != swarmie_durable_tasks::TaskObservation::Settled {
                return Err(tembra::Error::new(
                    tembra::Code::Unavailable,
                    "conversation.recovery_required",
                    "the previous turn was interrupted; stop it before continuing",
                ));
            }
        }
        Ok(())
    }

    async fn run_inner(
        &self,
        context: Context,
        input: Submission,
        may_be_admitted: &std::cell::Cell<bool>,
    ) -> tembra::Result<()> {
        let stop_epoch = self.stop_epoch.get();
        if context.principal()? != self.principal
            || input.conversation_id != self.id
            || input.draft.node != self.node
            || input.submission_id.is_empty()
            || input.submission_id.len() > 255
            || !input.draft.attachments.is_empty()
            || !matches!(input.mode.as_str(), "queue" | "steer")
        {
            return Err(failure(
                "the conversation submission is invalid or includes unsupported attachments",
            ));
        }
        input::validate_text(&input.draft.text)?;
        if input.execution_context.is_some()
            && input.draft.execution_context.is_some()
            && input.execution_context != input.draft.execution_context
        {
            return Err(failure("the submission has conflicting checkout bindings"));
        }
        let binding = input
            .execution_context
            .clone()
            .or_else(|| input.draft.execution_context.clone());
        if let Some(existing) = self.index.get(&self.id)?
            && existing.execution_context != binding
        {
            return Err(failure(
                "the conversation checkout binding cannot be changed",
            ));
        }
        let encoded = serde_json::to_vec(&input)
            .map_err(|_| failure("the submission could not be encoded"))?;
        let digest = format!("{:x}", Sha256::digest(encoded));
        if self
            .observe_existing(&context, &input, &digest, may_be_admitted)
            .await?
        {
            return Ok(());
        }
        let previous = self.running.borrow().clone();
        if input.mode == "steer" {
            // Preparation can hold the serial owner before durable admission exists.
            // A different pending checkout must not cancel that physical invocation.
            if previous.is_some_and(|running| running.binding != binding) {
                return Err(failure(
                    "the running conversation uses another checkout binding",
                ));
            }
            if self.compaction_running.borrow().is_none() {
                self.request_stop(&context).await?;
            }
        }
        let _serial = self.serial.lock().await;
        context.check()?;
        self.context_wait(&context).await?;
        self.wait_turn(&context).await?;
        // Another first submission may have bound this conversation while we waited.
        if let Some(existing) = self.index.get(&self.id)?
            && existing.execution_context != binding
        {
            return Err(failure(
                "the conversation checkout binding cannot be changed",
            ));
        }
        self.refresh(&context).await?;
        if self.state.value()?.phase == Phase::Unknown {
            self.settle(&context, false).await?;
        }
        if self.stop_epoch.get() != stop_epoch {
            return Err(failure("the request was stopped before admission"));
        }
        *self.running.borrow_mut() = Some(RunningContext {
            context: context.clone(),
            binding: binding.clone(),
            execution: None,
            sources: None,
        });
        let _running = Running(&self.running, context.clone());
        if self
            .observe_existing(&context, &input, &digest, may_be_admitted)
            .await?
        {
            return Ok(());
        }

        if self
            .index
            .read()?
            .iter()
            .any(|record| record.id == self.id && record.phase == Phase::Unknown)
        {
            return Err(failure(
                "the previous request has an uncertain outcome; observe its history without replaying it",
            ));
        }
        if let Some(binding) = &binding {
            checkout::validate(
                self.git.as_ref(),
                &context,
                &self.node,
                &self.principal,
                binding,
            )
            .await?;
        }
        let sources = self
            .capture_sources(&context, binding.as_ref(), &input.draft.text)
            .await?;
        let tools = self.context_tools(binding.as_ref(), sources.as_ref())?;
        let account = input
            .draft
            .account
            .as_deref()
            .ok_or_else(|| failure("select a provider account"))?;
        let selected_model = self
            .providers
            .model_with_protection(
                &self.principal,
                account,
                &input.draft.model,
                &input.draft.effort,
                swarmie_providers::ai::ModelProtection::Unprotected,
            )
            .await?;
        context.check()?;
        let task = swarmie_durable_tasks::TaskId::new();
        let mut records = self.index.read()?;
        if !records.iter().any(|record| record.id == self.id) {
            records.push(Metadata {
                context: Some(context::Ledger::new()?),
                id: self.id.clone(),
                title: input.draft.text.trim().chars().take(64).collect(),
                principal: self.principal.clone(),
                node: self.node.clone(),
                account: account.into(),
                model: input.draft.model.clone(),
                effort: input.draft.effort.clone(),
                archived: false,
                pinned: false,
                receipts: vec![],
                phase: Phase::Idle,
                checkpoint: None,
                task: None,
                stop_requested: false,
                execution_context: binding.clone(),
                lifecycle_warning: None,
            });
        }
        let record = records
            .iter_mut()
            .find(|record| record.id == self.id)
            .ok_or_else(|| failure("the conversation index is unavailable"))?;
        record.account = account.into();
        record.model = input.draft.model.clone();
        record.effort = input.draft.effort.clone();
        record.checkpoint = None;
        record.task = Some(task);
        record.stop_requested = false;
        record.phase = Phase::Working;
        record.receipts.push(Receipt {
            id: input.submission_id.clone(),
            digest,
        });
        if record.receipts.len() > 1024 {
            return Err(failure("the conversation submission limit was reached"));
        }
        self.index.write(&records)?;
        let conversation = match self.history.get(&context, self.history_id).await? {
            Some(value) => value,
            None => {
                self.history.create(&context, self.history_id).await?;
                self.history
                    .get(&context, self.history_id)
                    .await?
                    .ok_or_else(|| failure("the conversation could not be created"))?
            }
        };
        let mut entry = EntryDraft::encode(Record::Message(AgentMessage::User {
            content: input.draft.text,
        }))?;
        entry.id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, input.submission_id.as_bytes())
            .to_string()
            .parse::<EntryId>()
            .map_err(|_| failure("the submission identity is invalid"))?;
        let instructions = context::instructions(binding.as_ref());
        let mut request = sources::with_sources(
            Request::new(
                conversation.head,
                instructions,
                tools,
                NonZeroU32::new(16).ok_or_else(|| failure("turn bound is invalid"))?,
            ),
            sources,
        )?;
        let configuration =
            context::configuration(&selected_model, &request.instructions, &request.tools)?;
        request.projection = self
            .context_ledger()?
            .checkpoint
            .active_for(configuration)
            .cloned();
        may_be_admitted.set(true);
        self.tasks
            .admit(
                &context,
                &self.history,
                task,
                entry,
                tasks::Input {
                    session: self.id.clone(),
                    account: account.into(),
                    model: input.draft.model,
                    effort: input.draft.effort,
                    binding: binding.clone(),
                    context_fence: self.context_ledger()?.fence,
                    request,
                },
            )
            .await?;
        checkout::sync_hold(&self.index, self.git.as_ref(), &self.principal, &self.id).await;
        self.tasks.wait(&context, task).await?;
        let _ = self.refresh(&context).await;
        Ok(())
    }
}

#[derive(Default)]
struct Transcript {
    head: Option<swarmie_conversations::Head>,
    messages: Vec<types::Message>,
    admitted: Vec<String>,
    omitted: usize,
    pending: Vec<swarmie_agent_loop::ToolCall>,
}

#[derive(Clone)]
struct RunningContext {
    context: Context,
    binding: Option<types::ExecutionContext>,
    execution: Option<swarmie_durable_tasks::Execution>,
    sources: Option<swarmie_providers::ContextSources>,
}

struct Running<'a>(&'a RefCell<Option<RunningContext>>, Context);
impl Drop for Running<'_> {
    fn drop(&mut self) {
        if self
            .0
            .borrow()
            .as_ref()
            .is_some_and(|running| running.context.root == self.1.root)
        {
            self.0.borrow_mut().take();
        }
    }
}

pub struct ExecutionPolicy {
    pub service: Option<swarmie_git::GitService>,
    pub authority: Option<std::sync::Arc<dyn swarmie_git::Authority>>,
    pub owner: Option<Rc<dyn crate::ExecutionOwner>>,
    pub environment: Option<swarmie_context::environment::Configuration>,
}

pub fn orbs(
    host: Host,
    path: PathBuf,
    node: String,
    principal: String,
    providers: Rc<NativeProviders>,
    catalog: Catalog,
    policy: ExecutionPolicy,
) -> tembra::Result<Vec<Orb>> {
    let ExecutionPolicy {
        service: git,
        authority: agent_git_authority,
        owner: execution,
        environment,
    } = policy;
    let owner = swarmie_storage::sqlite::SqliteOwner::new(path.join("conversations.sqlite"));
    let options = swarmie_durable_tasks::Options::new(
        tasks::Executor,
        std::num::NonZeroU64::new(5_000).unwrap(),
        Rc::new(|duration| async move { tokio::time::sleep(duration).await }.boxed_local()),
    );
    let scheduler = swarmie_durable_tasks::orb(
        host.clone(),
        owner.task_factory(),
        swarmie_durable_tasks::Definitions::new([
            tasks::definition(),
            swarmie_context::durable::definition(),
        ])
        .map_err(|error| failure(&error.to_string()))?,
        options,
    );
    let handler = tasks::handler(host.clone());
    let context_handler = context::handler(host.clone());
    let context_orb = context_service::orb(host.clone(), principal.clone());
    let commands = context_service::commands(host.clone(), principal.clone());
    let history_orb = swarmie_conversations::orb(owner.conversation_factory());
    let index = Rc::new(index::Index {
        path: path.join("conversations.json"),
        lifecycle: tokio::sync::Mutex::new(()),
    });
    let public_index = index.clone();
    let public_principal = principal.clone();
    let public_git = git.clone();
    let listing = Orb::new(CATALOG, 1).service(
        Service::new(CATALOG, move |declaration| {
            if declaration.address()?.principal != public_principal
                || declaration.address()?.key != ["main"]
            {
                return Err(tembra::Error::new(tembra::Code::Forbidden, "conversation.principal", "conversation access refused"));
            }
            Ok(public_index.clone())
        })
        .method(
            "list",
            |index, context, (cursor,): (Option<String>,)| async move {
                context.check()?;
                index.page(cursor)
            },
        )
        .method(
            "update",
            move |index, context, (id, title, pinned, archived): (String, String, bool, bool)| {
                let git = public_git.clone();
                async move {
                    context.check()?;
                    if title.len() > 512 {
                        return Err(failure("the conversation title is too long"));
                    }
                    let principal = context.principal()?;
                    let mut confirmed = index.get(&id)?.ok_or_else(|| failure("the conversation is not indexed"))?;
                    confirmed.title = title.clone();
                    confirmed.pinned = pinned;
                    confirmed.archived = archived;
                    index.change(&id, |record| {
                        record.title = title;
                        record.pinned = pinned;
                        record.archived = archived;
                    })?;
                    checkout::sync_hold(&index, git.as_ref(), &principal, &id).await;
                    match index.get(&id) {
                        Ok(Some(current)) => confirmed = current,
                        _ => confirmed.lifecycle_warning = Some("Conversation metadata was saved. Git retention observation is unavailable; the checkout is retained.".into()),
                    }
                    Ok(types::CatalogEntry::from(confirmed))
                }
            },
        ),
    );
    let sessions = Orb::new(SERVICE, 1).service(
        Service::new(SERVICE, move |declaration| {
            let address = declaration.address()?;
            let [id] = address.key.as_slice() else {
                return Err(tembra::Error::invalid(
                    "conversation.key",
                    "invalid conversation key",
                ));
            };
            if address.principal != principal || id.is_empty() || id.len() > 255 {
                return Err(tembra::Error::new(
                    tembra::Code::Forbidden,
                    "conversation.principal",
                    "conversation access refused",
                ));
            }
            let history_id = uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_URL,
                format!("{node}/{principal}/{id}").as_bytes(),
            )
            .to_string()
            .parse::<ConversationId>()
            .map_err(|_| failure("invalid conversation identity"))?;
            let owner_conversation_id = execution.as_ref().map(|_| {
                uuid::Uuid::new_v5(
                    &uuid::Uuid::NAMESPACE_URL,
                    format!("{node}/{principal}/{id}").as_bytes(),
                )
            });
            let state = declaration.state(
                Snapshot {
                    context: swarmie_context::Snapshot::default(),
                    conversation_id: id.clone(),
                    owner_conversation_id,
                    messages: vec![],
                    phase: Phase::Idle,
                    label: String::new(),
                    admitted: vec![],
                    omitted_messages: 0,
                    can_acknowledge: false,
                    execution_context: index
                        .get(id)?
                        .as_ref()
                        .and_then(|record| record.execution_context.clone()),
                    lifecycle_warning: index
                        .get(id)?
                        .and_then(|record| record.lifecycle_warning)
                        .map(checkout::bounded_warning),
                },
                1,
                |value| {
                    if serde_json::to_vec(value)
                        .map_err(|_| failure("conversation projection could not be encoded"))?
                        .len()
                        > 48 * 1024
                    {
                        return Err(failure(
                            "the conversation preview exceeds the current transport limit",
                        ));
                    }
                    Ok(())
                },
            )?;
            let agent_history = History::bind(
                &declaration,
                host.clone(),
                swarmie_conversations::SERVICE,
                vec!["main".into()],
            )?;
            let execution = execution
                .as_ref()
                .map(|owner| {
                    owner.clone().conversation(
                        &principal,
                        owner_conversation_id.ok_or_else(|| {
                            failure("native conversation identity is unavailable")
                        })?,
                    )
                })
                .transpose()?;
            let tools = Rc::new(CatalogService::bind(
                &declaration,
                host.clone(),
                swarmie_tools::SERVICE,
                vec!["main".into()],
            )?);
            let history_factory = owner.conversation_factory();
            let task_factory = owner.task_factory();
            let (id, node, principal) = (id.clone(), node.clone(), principal.clone());
            let (host, providers, catalog) = (host.clone(), providers.clone(), catalog.clone());
            let environment = environment.clone();
            let (git, agent_git_authority, index) =
                (git.clone(), agent_git_authority.clone(), index.clone());
            declaration.resource_async(
                "conversation.session",
                1,
                &[],
                move |signal| {
                    async move {
                        signal.check()?;
                        let history = Conversations::new(history_factory().await?);
                        let storage = task_factory()
                            .await
                            .map_err(|error| failure(&error.to_string()))?;
                        signal.check()?;
                        Ok(Session {
                            id: id.clone(),
                            node,
                            principal,
                            history_id,
                            history,
                            tasks: Rc::new(tasks::Tasks { storage, host }),
                            agent_history,
                            providers,
                            execution,
                            tools,
                            catalog,
                            git,
                            environment_configuration: environment,
                            agent_git_authority,
                            index,
                            state,
                            running: RefCell::new(None),
                            stop_epoch: std::cell::Cell::new(0),
                            serial: tokio::sync::Mutex::new(()),
                            compaction_running: RefCell::new(None),
                        })
                    }
                    .boxed_local()
                },
                |session, _| async move { session.close().await }.boxed_local(),
            )
        })
        .lifetime(tembra::core::Lifetime::Referenced, 30_000)
        .method("project_skills", |session, context, (binding,): (types::ExecutionContext,)| async move {
            session.value()?.environment_inventory(&context, &binding).await
        })
        .method("validate", |session, context, (spec, call): (swarmie_tools::ToolSpec, swarmie_tools::ToolCall)| async move {
            session.value()?.validate_skill(&context, &spec, &call)
        })
        .method("execute", |session, context, (_operation, call): (swarmie_providers::OperationId, swarmie_tools::ToolCall)| async move {
            session.value()?.read_skill(&context, &call).await
        })
        .method("recover", |session, context, (_operation, call): (swarmie_providers::OperationId, swarmie_tools::ToolCall)| async move {
            let session = session.value()?;
            context.check()?;
            if context.principal()? != session.principal || call.name.as_str() != sources::TOOL { return Err(failure("skill recovery is not authorized")); }
            Ok(swarmie_tools::Recovery::<swarmie_tools::ToolResult>::Unknown)
        })
        .method("context_inspect", |session, context, (): ()| async move {
            session.value()?.context_inspect(&context).await
        })
        .method(
            "context_compact",
            |session, context, (request,): (swarmie_context::CompactionRequest,)| async move {
                session.value()?.context_compact(&context, request).await
            },
        )
        .method(
            "context_stop",
            |session, context, (operation,): (swarmie_providers::OperationId,)| async move {
                session.value()?.context_stop(&context, operation).await
            },
        )
        .method(
            "execute_compaction",
            |session, context, execution: swarmie_durable_tasks::Execution| async move {
                session
                    .value()?
                    .execute_compaction(&context, execution)
                    .await
            },
        )
        .method("snapshot", |session, context, (): ()| async move {
            let session = session.value()?;
            session.refresh(&context).await?;
            if session.running.borrow().is_none() && session.state.value()?.phase == Phase::Unknown
            {
                let _serial = session.serial.lock().await;
                if session.state.value()?.phase == Phase::Unknown {
                    session.settle(&context, false).await?;
                }
            }
            checkout::sync_hold(
                &session.index,
                session.git.as_ref(),
                &session.principal,
                &session.id,
            )
            .await;
            session.refresh(&context).await?;
            Ok(())
        })
        .method(
            "run",
            |session, context, (input,): (Submission,)| async move {
                session.value()?.run(context, input).await
            },
        )
        .method(
            "execute_task",
            |session, context, execution: swarmie_durable_tasks::Execution| async move {
                session.value()?.execute_task(context, execution).await
            },
        )
        .method("acknowledge", |session, context, (): ()| async move {
            let session = session.value()?;
            if session.running.borrow().is_some() {
                return Err(failure("the request is still physically running"));
            }
            let _serial = session.serial.lock().await;
            session.refresh(&context).await?;
            session.settle(&context, true).await?;
            checkout::sync_hold(
                &session.index,
                session.git.as_ref(),
                &session.principal,
                &session.id,
            )
            .await;
            session.refresh(&context).await
        })
        .method("stop", |session, context, (): ()| async move {
            let session = session.value()?;
            session.stop_epoch.set(
                session
                    .stop_epoch
                    .get()
                    .checked_add(1)
                    .ok_or_else(|| failure("the stop sequence is exhausted"))?,
            );
            if session.compaction_running.borrow().is_none()
                && session.index.get(&session.id)?.is_some()
            {
                let mut ledger = session.context_ledger()?;
                ledger.fence = uuid::Uuid::new_v4().try_into()?;
                session.save_context_ledger(ledger)?;
            }
            session.request_stop(&context).await?;
            let _serial = session.serial.lock().await;
            // The task retains physical ownership even after its observer is gone.
            session.wait_turn(&context).await?;
            session.refresh(&context).await?;
            if session.state.value()?.phase == Phase::Unknown {
                session.settle(&context, false).await?;
            }
            checkout::sync_hold(
                &session.index,
                session.git.as_ref(),
                &session.principal,
                &session.id,
            )
            .await;
            session.refresh(&context).await?;
            Ok(())
        }),
    );
    Ok(vec![
        history_orb,
        scheduler,
        handler,
        context_handler,
        context_orb,
        commands,
        listing,
        sessions,
    ])
}
