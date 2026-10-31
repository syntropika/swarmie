//! One native owner for the client engine, private cards and vault tool dispatch.
use super::*;
use std::{collections::BTreeMap, rc::Weak};
use swarmie_conversation_runtime::{ConversationExecution, ExecutionOwner, ExecutionScope};

pub(crate) struct ClientExecution {
    node: NativeNode,
    principal: String,
    agents: RefCell<BTreeMap<Uuid, Weak<NativeAgent>>>,
    // Conversation approvals belong to the application session. Tembra still
    // owns service objects, observers and every physical task invocation.
    logical: RefCell<BTreeMap<Uuid, Rc<NativeAgent>>>,
    plugins: swarmie_plugin_manager::PluginIndex,
}

impl ClientExecution {
    pub fn new(
        node: NativeNode,
        principal: String,
        plugins: swarmie_plugin_manager::PluginIndex,
    ) -> Rc<Self> {
        Rc::new(Self {
            node,
            principal,
            agents: RefCell::new(BTreeMap::new()),
            logical: RefCell::new(BTreeMap::new()),
            plugins,
        })
    }

    pub fn tools(self: &Rc<Self>) -> Orb {
        let owner = self.clone();
        swarmie_vault::tools::orb_with_owner(move |context| {
            if !owner.plugins.required_active() {
                return Err(unavailable());
            }
            let binding = owner.node.executions.resolve(context)?;
            owner
                .agents
                .borrow()
                .get(&binding.identity().execution)
                .and_then(Weak::upgrade)
                .filter(|agent| agent.current())
                .and_then(|agent| agent.tools())
                .ok_or_else(unavailable)
        })
    }
}

impl ExecutionOwner for ClientExecution {
    fn conversation(
        self: Rc<Self>,
        principal: &str,
        conversation: Uuid,
    ) -> tembra::Result<Rc<dyn ConversationExecution>> {
        if principal != self.principal {
            return Err(unavailable());
        }
        self.logical.borrow_mut().retain(|_, agent| agent.current());
        Ok(Rc::new(ClientConversation {
            owner: self.clone(),
            conversation,
        }))
    }

    fn tools(&self) -> tembra::Result<Vec<swarmie_tools::ToolDefinition>> {
        swarmie_vault::tool_definitions()
    }
}

struct ClientConversation {
    owner: Rc<ClientExecution>,
    conversation: Uuid,
}

impl ConversationExecution for ClientConversation {
    fn enter(&self, context: &Context) -> tembra::Result<ExecutionScope> {
        if !self.owner.plugins.required_active() {
            return Err(unavailable());
        }
        if context.principal()? != self.owner.principal {
            return Err(unavailable());
        }
        let existing = self
            .owner
            .logical
            .borrow()
            .get(&self.conversation)
            .cloned()
            .filter(|agent| agent.current());
        let agent = if let Some(agent) = existing {
            agent
        } else {
            let agent = Rc::new(
                self.owner
                    .node
                    .main_agent(None, None, Some(self.conversation))?,
            );
            let mut agents = self.owner.agents.borrow_mut();
            agents.retain(|_, agent| agent.strong_count() > 0);
            agents.insert(agent.identity().execution, Rc::downgrade(&agent));
            drop(agents);
            self.owner
                .logical
                .borrow_mut()
                .retain(|_, agent| agent.current());
            self.owner
                .logical
                .borrow_mut()
                .insert(self.conversation, agent.clone());
            let owner = Rc::downgrade(&self.owner);
            let conversation = self.conversation;
            let execution = agent.identity().execution;
            agent.on_owner_close(Box::new(move || {
                if let Some(owner) = owner.upgrade() {
                    let mut logical = owner.logical.borrow_mut();
                    if logical
                        .get(&conversation)
                        .is_some_and(|agent| agent.identity().execution == execution)
                    {
                        logical.remove(&conversation);
                    }
                    owner.agents.borrow_mut().remove(&execution);
                }
            }));
            agent
        };
        let physical = agent.enter(context)?;
        Ok(ExecutionScope::new(
            physical.context().clone(),
            agent.protection(),
            (agent, physical),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;

    #[test]
    fn presentation_owner_drop_preserves_logical_conversation_authority_until_application_close() {
        let directory = tempfile::tempdir().unwrap();
        let node = NativeNode::open(directory.path().into()).unwrap();
        let owner = ClientExecution::new(node.clone(), "fixture".into(), Default::default());
        let conversation = Uuid::new_v4();
        let mut pool = futures::executor::LocalPool::new();
        let host = tembra::Host::new(pool.spawner());
        let first_owner = owner.clone();
        let first = pool
            .run_until(host.root("fixture", move |context| {
                async move {
                    let observer = first_owner.clone().conversation("fixture", conversation)?;
                    let scope = observer.enter(&context)?;
                    let identity = first_owner
                        .node
                        .executions
                        .resolve(scope.context())?
                        .identity()
                        .clone();
                    drop(scope);
                    drop(observer);
                    Ok(identity)
                }
                .boxed_local()
            }))
            .unwrap();
        let second_owner = owner.clone();
        let second = pool
            .run_until(host.root("fixture", move |context| {
                async move {
                    let observer = second_owner.clone().conversation("fixture", conversation)?;
                    let scope = observer.enter(&context)?;
                    Ok(second_owner
                        .node
                        .executions
                        .resolve(scope.context())?
                        .identity()
                        .clone())
                }
                .boxed_local()
            }))
            .unwrap();
        assert_eq!(first, second);
        let retained = owner.clone().conversation("fixture", conversation).unwrap();
        let observer = retained.clone();
        pool.run_until(host.root("fixture", move |context| {
            async move {
                let _scope = observer.enter(&context)?;
                Ok(())
            }
            .boxed_local()
        }))
        .unwrap();
        let retired = Rc::downgrade(&owner.logical.borrow()[&conversation]);
        node.close();
        assert!(!owner.logical.borrow().contains_key(&conversation));
        assert!(retired.upgrade().is_none());
        drop(retained);
        pool.run_until(host.close()).unwrap();
    }
}
