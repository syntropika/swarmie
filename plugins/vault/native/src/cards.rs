//! Pending private human interactions. Tembra State receives only the safe card projection.
use crate::control::Control;
use futures::{FutureExt, channel::oneshot, future::LocalBoxFuture};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::{Rc, Weak},
};
use swarmie_permissions::{ExecutionBinding, HumanDecision, OwnerBinding};
use swarmie_vault::{
    executors::{CaptureSource, UseDecisions},
    tools::{InputIntent, InputReceipt, PrivateInputs},
    *,
};
use uuid::Uuid;

pub enum Pending {
    Use(UseIntent, oneshot::Sender<Result<(HumanDecision, bool)>>),
    Input(
        InputIntent,
        ExecutionBinding,
        oneshot::Sender<Result<InputReceipt>>,
    ),
    Capture(
        Rc<CaptureHandle>,
        CaptureSource,
        oneshot::Sender<Result<CapturedSecretReceipt>>,
    ),
}
impl Pending {
    fn is_live(&self) -> bool {
        match self {
            Self::Use(_, send) => !send.is_canceled(),
            Self::Input(_, binding, send) => binding.is_live() && !send.is_canceled(),
            Self::Capture(handle, _, send) => handle.execution().is_live() && !send.is_canceled(),
        }
    }
}
pub struct Card {
    order: u64,
    pub owner: OwnerBinding,
    execution: ExecutionBinding,
    pub view: Value,
    pub pending: Pending,
}
#[derive(Default)]
pub struct Cards(RefCell<BTreeMap<Uuid, Card>>, Cell<u64>);
impl Cards {
    pub fn views(&self) -> Vec<Value> {
        self.0.borrow_mut().retain(|_, card| {
            card.execution.is_live() && card.owner.is_live() && card.pending.is_live()
        });
        let cards = self.0.borrow();
        let mut ordered = cards.values().collect::<Vec<_>>();
        ordered.sort_by_key(|card| card.order);
        ordered.into_iter().map(|card| card.view.clone()).collect()
    }
    pub fn binding(&self, id: Uuid) -> Result<OwnerBinding> {
        self.0
            .borrow()
            .get(&id)
            .map(|card| card.owner.clone())
            .ok_or(Error::NotFound)
    }
    pub fn take_owner(&self, id: Uuid, owner: &OwnerBinding) -> Result<Card> {
        let mut cards = self.0.borrow_mut();
        if !cards.get(&id).is_some_and(|card| {
            card.owner.same_owner(owner)
                && card.execution.is_live()
                && card.owner.is_live()
                && card.pending.is_live()
        }) {
            return Err(Error::Revoked);
        }
        cards.remove(&id).ok_or(Error::NotFound)
    }
    pub fn cancel(&self, id: Uuid) {
        self.0.borrow_mut().remove(&id);
    }
    pub fn take_for_account(&self, id: Uuid, account: Option<Uuid>) -> Result<Card> {
        let mut cards = self.0.borrow_mut();
        if !cards.get(&id).is_some_and(|card| {
            card.owner.is_live()
                && card.pending.is_live()
                && card.owner.identity().account == account
        }) {
            return Err(Error::Revoked);
        }
        cards.remove(&id).ok_or(Error::NotFound)
    }
    fn push(&self, execution: ExecutionBinding, mut view: Value, pending: Pending) -> Result<()> {
        let owner = execution.shell().clone();
        if !execution.is_live() || !owner.is_live() {
            return Err(Error::Revoked);
        }
        let mut cards = self.0.borrow_mut();
        cards.retain(|_, card| {
            card.execution.is_live() && card.owner.is_live() && card.pending.is_live()
        });
        if cards.len() >= 64 {
            return Err(Error::Exhausted);
        }
        let id = Uuid::new_v4();
        let order = self.1.get().checked_add(1).ok_or(Error::Exhausted)?;
        self.1.set(order);
        view["id"] = json!(id);
        cards.insert(
            id,
            Card {
                order,
                owner,
                execution,
                view,
                pending,
            },
        );
        Ok(())
    }
    pub fn cancel_all(&self) {
        self.0.borrow_mut().clear();
    }
}
pub struct CardPort(pub Weak<Control>);
impl CardPort {
    fn control(&self) -> Result<Rc<Control>> {
        self.0
            .upgrade()
            .filter(|control| control.private_decisions_available())
            .ok_or(Error::Revoked)
    }
}
impl UseDecisions for CardPort {
    fn request<'a>(
        &'a self,
        execution: &'a ExecutionBinding,
        intent: &'a UseIntent,
    ) -> LocalBoxFuture<'a, Result<(HumanDecision, bool)>> {
        async move {
            let control=self.control()?;control.owner_for(execution)?;
            let (send,receive)=oneshot::channel();
            control.cards.push(execution.clone(),json!({"kind":"use","conversation":intent.execution.conversation,"title":intent.metadata.name,"purpose":intent.metadata.purpose,"metadata":intent.metadata,"reference":intent.reference.to_string(),"provider":intent.provider,"project":intent.execution.project,"agent":intent.execution.agent,"mainAgent":intent.execution.main_agent,"destination":intent.destination,"conversationAllowed":control.conversation_allowed(intent)}),Pending::Use(intent.clone(),send))?;
            receive.await.map_err(|_|Error::Revoked)?
        }.boxed_local()
    }
}
impl PrivateInputs for CardPort {
    fn request<'a>(
        &'a self,
        execution: ExecutionBinding,
        intent: InputIntent,
    ) -> LocalBoxFuture<'a, Result<InputReceipt>> {
        async move {
            let control=self.control()?;control.owner_for(&execution)?;
            let (send,receive)=oneshot::channel();
            control.cards.push(execution.clone(),json!({"kind":"entry","conversation":intent.execution.conversation,"title":intent.metadata.name,"purpose":intent.metadata.purpose,"metadata":intent.metadata,"replacing":intent.expected.is_some()}),Pending::Input(intent,execution,send))?;
            receive.await.map_err(|_|Error::Revoked)?
        }.boxed_local()
    }
    fn capture<'a>(
        &'a self,
        handle: Rc<CaptureHandle>,
        source: CaptureSource,
    ) -> LocalBoxFuture<'a, Result<CapturedSecretReceipt>> {
        async move {
            let control=self.control()?;let record=handle.record()?;control.owner_for(handle.execution())?;
            let (send,receive)=oneshot::channel();
            control.cards.push(handle.execution().clone(),json!({"kind":"capture","conversation":record.intent.execution.conversation,"title":record.intent.metadata.name,"purpose":record.intent.metadata.purpose,"destination":record.intent.destination,"profile":record.intent.profile,"metadata":record.intent.metadata,"replacing":record.intent.expected.is_some()}),Pending::Capture(handle,source,send))?;
            receive.await.map_err(|_|Error::Revoked)?
        }.boxed_local()
    }
}
