//! Client admission and projection over the existing Agent Loop and Conversations libraries.
mod index;
mod projection;
mod types;

use crate::providers::NativeProviders;
use futures::FutureExt;
use sha2::{Digest, Sha256};
use std::{cell::RefCell, num::NonZeroU32, path::PathBuf, rc::Rc};
use swarmie_agent_loop::{
    Agent, Message as AgentMessage,
    conversation::{History, Record, Request},
};
use swarmie_conversations::{ConversationId, Conversations, EntryDraft, EntryId, Read};
use swarmie_providers::Progress;
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
    providers: Rc<NativeProviders>,
    tools: Rc<CatalogService>,
    catalog: Catalog,
    index: Rc<index::Index>,
    state: State<Snapshot>,
    running: RefCell<Option<Context>>,
    stop_epoch: std::cell::Cell<u64>,
    serial: tokio::sync::Mutex<()>,
}

impl Session {
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
        request.limit = std::num::NonZeroU16::new(1).unwrap();
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
        if phase == Phase::Working && self.running.borrow().is_none() {
            phase = if transcript.pending.is_empty() {
                Phase::Stopped
            } else {
                Phase::Unknown
            };
            self.index.change(&self.id, |record| record.phase = phase)?;
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
            _ => "",
        };
        self.state.update(context, |current| {
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
                messages,
                admitted: transcript.admitted,
                omitted_messages: transcript.omitted,
                can_acknowledge: phase == Phase::Unknown
                    && !transcript.pending.is_empty()
                    && self.running.borrow().is_none(),
                phase,
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
        let Some(metadata) = self.index.get(&self.id)? else {
            return Err(failure("there is no recorded outcome to acknowledge"));
        };
        if metadata.phase != Phase::Unknown {
            return self.refresh(context).await;
        }
        if let Some(checkpoint) = metadata.checkpoint {
            let settlement = self
                .agent_history
                .settle_tools(context, self.tools.as_ref(), checkpoint, acknowledge, None)
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
        match self.run_inner(context, input, &may_be_admitted).await {
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
            || input.draft.text.trim().is_empty()
            || input.draft.text.len() > 16 * 1024
            || input.submission_id.is_empty()
            || input.submission_id.len() > 255
            || !input.draft.attachments.is_empty()
            || !matches!(input.mode.as_str(), "queue" | "steer")
        {
            return Err(failure(
                "the conversation submission is invalid or includes unsupported attachments",
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
        if input.mode == "steer"
            && let Some(running) = self.running.borrow().as_ref()
        {
            running.cancel()?;
        }
        let _serial = self.serial.lock().await;
        context.check()?;
        self.refresh(&context).await?;
        if self.state.value()?.phase == Phase::Unknown {
            self.settle(&context, false).await?;
        }
        if self.stop_epoch.get() != stop_epoch {
            return Err(failure("the request was stopped before admission"));
        }
        *self.running.borrow_mut() = Some(context.clone());
        let _running = Running(&self.running);
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
        let tools = self.catalog.snapshot()?.specs();
        let account = input
            .draft
            .account
            .as_deref()
            .ok_or_else(|| failure("select a provider account"))?;
        let model = self
            .providers
            .model(
                &self.principal,
                account,
                &input.draft.model,
                &input.draft.effort,
            )
            .await?;
        context.check()?;
        let mut records = self.index.read()?;
        if !records.iter().any(|record| record.id == self.id) {
            records.push(Metadata {
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
        may_be_admitted.set(true);
        let receipt = self
            .history
            .append(&context, conversation.head, vec![entry])
            .await?;
        if receipt.confirmation != swarmie_conversations::WriteConfirmation::Committed {
            return Err(failure("the message append is not confirmed"));
        }
        let at = receipt.head;
        self.index
            .change(&self.id, |record| record.phase = Phase::Working)?;
        // Admission is durable before this best-effort presentation update.
        let _ = self.refresh(&context).await;
        let state = self.state.clone();
        let progress = Progress::new(move |context, preview| {
            let _ = state.update(context, |current| {
                let mut next = current.clone();
                if next
                    .messages
                    .last()
                    .is_some_and(|message| message.id == "preview")
                {
                    next.messages.pop();
                }
                next.messages.push(types::Message {
                    id: "preview".into(),
                    role: "assistant".into(),
                    text: preview.content,
                    truncated: false,
                });
                Ok(projection::bound(next))
            });
        });
        let agent = Agent::new(Rc::new(model), self.tools.clone()).with_progress(progress);
        let result = self
            .agent_history
            .execute(
                &context,
                &agent,
                Request {
                    at,
                    instructions: String::new(),
                    tools,
                    max_turns: NonZeroU32::new(16).unwrap(),
                },
            )
            .await;
        let (mut phase, mut checkpoint) = match result {
            Ok(_) => (Phase::Completed, None),
            Err(error) => {
                let checkpoint = error.checkpoint.map(|value| *value);
                let phase = if checkpoint
                    .as_ref()
                    .is_some_and(|value| value.has_unfinished_tools())
                {
                    Phase::Unknown
                } else if context.check().is_err() {
                    Phase::Stopped
                } else {
                    Phase::Failed
                };
                (phase, checkpoint)
            }
        };
        if phase == Phase::Unknown
            && context.check().is_ok()
            && let Some(pending) = checkpoint.clone()
            && let Ok(swarmie_agent_loop::conversation::ToolSettlement::Settled(settled)) = self
                .agent_history
                .settle_tools(&context, self.tools.as_ref(), pending, false, None)
                .await
        {
            phase = Phase::Stopped;
            checkpoint = Some(settled);
        }
        // Physical settlement always records the outcome, including observer cancellation.
        self.index.change(&self.id, |record| {
            record.phase = phase;
            record.checkpoint = checkpoint;
        })?;
        drop(_running);
        // Observation cannot replace a confirmed execution outcome.
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

struct Running<'a>(&'a RefCell<Option<Context>>);
impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.borrow_mut().take();
    }
}

pub(crate) fn orbs(
    host: Host,
    path: PathBuf,
    node: String,
    principal: String,
    providers: Rc<NativeProviders>,
    catalog: Catalog,
) -> tembra::Result<Vec<Orb>> {
    let storage: Rc<dyn swarmie_conversations::ConversationStorage> =
        Rc::new(swarmie_storage::sqlite::SqliteConversationStorage::open(
            path.join("conversations.sqlite"),
        )?);
    let history = Conversations::new(storage.clone());
    let history_orb = swarmie_conversations::orb(Rc::new(move || {
        let storage = storage.clone();
        async move { Ok(storage) }.boxed_local()
    }));
    let index = Rc::new(index::Index(path.join("conversations.json")));
    let public_index = index.clone();
    let public_principal = principal.clone();
    let listing = Orb::new(CATALOG, 1).service(Service::new(CATALOG, move |declaration| {
        if declaration.address()?.principal != public_principal || declaration.address()?.key != ["main"] { return Err(failure("conversation access refused")); }
        Ok(public_index.clone())
    }).method("list", |index, context, (cursor,): (Option<String>,)| async move { context.check()?; index.page(cursor) })
      .method("update", |index, context, (id, title, pinned, archived): (String, String, bool, bool)| async move { context.check()?; if title.len() > 512 { return Err(failure("the conversation title is too long")); } index.change(&id, |record| { record.title = title; record.pinned = pinned; record.archived = archived; }) }));
    let sessions =
        Orb::new(SERVICE, 1).service(
            Service::new(SERVICE, move |declaration| {
                let address = declaration.address()?;
                let [id] = address.key.as_slice() else {
                    return Err(failure("invalid conversation key"));
                };
                if address.principal != principal || id.is_empty() || id.len() > 255 {
                    return Err(failure("conversation access refused"));
                }
                let history_id = uuid::Uuid::new_v5(
                    &uuid::Uuid::NAMESPACE_URL,
                    format!("{node}/{principal}/{id}").as_bytes(),
                )
                .to_string()
                .parse::<ConversationId>()
                .map_err(|_| failure("invalid conversation identity"))?;
                let state = declaration.state(
                    Snapshot {
                        conversation_id: id.clone(),
                        messages: vec![],
                        phase: Phase::Idle,
                        label: String::new(),
                        admitted: vec![],
                        omitted_messages: 0,
                        can_acknowledge: false,
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
                Ok(Session {
                    id: id.clone(),
                    node: node.clone(),
                    principal: principal.clone(),
                    history_id,
                    history: history.clone(),
                    agent_history: History::bind(
                        &declaration,
                        host.clone(),
                        swarmie_conversations::SERVICE,
                        vec!["main".into()],
                    )?,
                    providers: providers.clone(),
                    tools: Rc::new(CatalogService::bind(
                        &declaration,
                        host.clone(),
                        swarmie_tools::SERVICE,
                        vec!["main".into()],
                    )?),
                    catalog: catalog.clone(),
                    index: index.clone(),
                    state,
                    running: RefCell::new(None),
                    stop_epoch: std::cell::Cell::new(0),
                    serial: tokio::sync::Mutex::new(()),
                })
            })
            .lifetime(tembra::core::Lifetime::Referenced, 30_000)
            .method("snapshot", |session, context, (): ()| async move {
                session.refresh(&context).await?;
                if session.running.borrow().is_none() {
                    let _serial = session.serial.lock().await;
                    if session.state.value()?.phase == Phase::Unknown {
                        session.settle(&context, false).await?;
                    }
                }
                Ok(())
            })
            .method(
                "run",
                |session, context, (input,): (Submission,)| async move {
                    session.run(context, input).await
                },
            )
            .method("acknowledge", |session, context, (): ()| async move {
                if session.running.borrow().is_some() {
                    return Err(failure("the request is still physically running"));
                }
                let _serial = session.serial.lock().await;
                session.refresh(&context).await?;
                session.settle(&context, true).await
            })
            .method("stop", |session, context, (): ()| async move {
                session.stop_epoch.set(
                    session
                        .stop_epoch
                        .get()
                        .checked_add(1)
                        .ok_or_else(|| failure("the stop sequence is exhausted"))?,
                );
                if let Some(running) = session.running.borrow().as_ref() {
                    running.cancel()?;
                }
                let _serial = session.serial.lock().await;
                // The cancelled invocation retains physical ownership until it settles.
                session.refresh(&context).await?;
                if session.state.value()?.phase == Phase::Unknown {
                    session.settle(&context, false).await?;
                }
                Ok(())
            }),
        );
    Ok(vec![history_orb, listing, sessions])
}
