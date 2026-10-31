//! Qualified native human actions for pending cards. Started sources retain narrow settlement.
use super::*;
impl Control {
    pub(super) async fn decide(self: Rc<Self>, id: Uuid, choice: String) -> tembra::Result<()> {
        if choice == "cancel" {
            self.cards.cancel(id);
            return Ok(());
        }
        if choice != "allow-once" && choice != "conversation" {
            return Err(unavailable());
        }
        let human = self.shell.borrow().clone();
        let owner = self.cards.binding(id).map_err(failure)?;
        self.authority.check_binding(&owner).map_err(failure)?;
        if owner.identity().installation != self.configuration.borrow().installation
            || owner.identity().account != human.identity().account
        {
            return Err(unavailable());
        }
        let card = self.cards.take_owner(id, &owner).map_err(failure)?;
        if choice == "cancel" {
            drop(card);
            return Ok(());
        }
        match card.pending {
            crate::cards::Pending::Use(intent, send) => {
                if choice != "allow-once" && choice != "conversation" {
                    return Err(unavailable());
                }
                let conversation = choice == "conversation";
                if conversation && !self.conversation_allowed(&intent) {
                    return Err(unavailable());
                }
                let decision = self
                    .authority
                    .decide_for(&card.owner, &("vault-use/v1", &intent, conversation))
                    .map_err(failure)?;
                let _ = send.send(Ok((decision, conversation)));
            }
            crate::cards::Pending::Capture(handle, source, send) => {
                if choice != "allow-once" {
                    return Err(unavailable());
                }
                let record = handle.record().map_err(failure)?;
                let decision = human
                    .decide(&("vault-blind-save/v1", record.id, &record.intent))
                    .map_err(failure)?;
                let active = self.active()?;
                let producers = Rc::new(swarmie_vault::executors::CaptureProducers {
                    node: active.node.clone(),
                    session: active.session.clone(),
                    execution: handle.execution().clone(),
                    plugin: self.tool_grant.clone(),
                    decisions: Rc::new(crate::cards::CardPort(Rc::downgrade(&self))),
                    bash: Rc::new(swarmie_vault::executors::BashExecutor::new(native_bash())),
                    http: Rc::new(
                        swarmie_vault::executors::HttpExecutor::new(None).map_err(failure)?,
                    ),
                });
                let job = producers
                    .start(handle.clone(), source, decision)
                    .await
                    .map_err(failure)?;
                let mut jobs = self.jobs.borrow_mut();
                jobs.retain(|job| !job.is_settled());
                jobs.push(job.clone());
                drop(jobs);
                tokio::task::spawn_local(async move {
                    let result = job
                        .finished()
                        .await
                        .and_then(|()| handle.record().map(|record| record.receipt()));
                    let _ = send.send(result);
                });
            }
            crate::cards::Pending::Input(..) => return Err(unavailable()),
        }
        Ok(())
    }
    pub(super) async fn submit_entry(&self, id: Uuid, mut text: PrivateText) -> tembra::Result<()> {
        let human = self.shell.borrow().clone();
        let card = self
            .cards
            .take_for_account(id, human.identity().account)
            .map_err(failure)?;
        let crate::cards::Pending::Input(intent, execution, send) = card.pending else {
            return Err(unavailable());
        };
        self.owner_for(&execution).map_err(failure)?;
        let active = self.active()?;
        active
            .node
            .check_agent_mutation(
                &active.session,
                &execution,
                intent.reference,
                intent.expected,
                &intent.metadata,
            )
            .map_err(failure)?;
        let value = SecretValue::new(std::mem::take(&mut text.0).into_bytes()).map_err(failure)?;
        let prepared = active
            .node
            .prepare_entry(
                &active.session,
                intent.reference,
                intent.expected,
                intent.metadata,
                &value,
            )
            .map_err(failure)?;
        drop(value);
        let decision = human
            .decide(&("vault-mutation/v1", prepared.intent()))
            .map_err(failure)?;
        let result = active
            .node
            .write_entry(&active.session, prepared, decision, &self.entries)
            .await;
        let reply = result.map(|(mutation, outcome)| swarmie_vault::tools::InputReceipt {
            mutation,
            outcome,
            reference: (outcome == CommitOutcome::Accepted).then_some(intent.reference),
        });
        let _ = send.send(reply.clone());
        reply.map(|_| ()).map_err(failure)
    }
}
