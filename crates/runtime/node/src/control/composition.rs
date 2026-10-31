//! Native composition entry. Only the real logical execution owner supplies project/agent facts.
use super::*;
use std::collections::BTreeMap;
use swarmie_permissions::{BoundExecutionContext, ExecutionContexts};
use swarmie_providers::ai::{Configuration as Models, ModelProtection};
use swarmie_vault::VaultCredentials;
use swarmie_vault::{
    executors::{BashExecutor, HttpExecutor},
    tools::VaultTools,
};

/// Reusable desktop/native owner. Opening it reads safe configuration and protected session custody;
/// the private UI Orb performs sign-in, enrollment and unlock. No hosted source is required.
#[derive(Clone)]
pub struct NativeNode {
    pub(super) control: Rc<Control>,
    pub(super) executions: ExecutionContexts,
}
impl NativeNode {
    pub fn open(root: PathBuf) -> tembra::Result<Self> {
        Self::with_private_entries(root, PrivateEntries::default())
    }
    /// Supply opaque contributions from the embedding's existing active plugin index.
    pub fn with_private_entries(root: PathBuf, entries: PrivateEntries) -> tembra::Result<Self> {
        let control = Control::open(root, entries)?;
        let executions = ExecutionContexts::new(control.authority.clone());
        Ok(Self {
            control,
            executions,
        })
    }
    /// Serve only on the origin-authenticated private shell channel, never the agent RPC catalog.
    pub fn private_ui(&self) -> Orb {
        orb(self.control.clone())
    }
    /// Main logical execution gets a fresh generation. Retain the returned owner for the logical
    /// conversation lifetime; physical attempts within it use the same authenticated binding.
    pub fn main_agent(
        &self,
        project: Option<Uuid>,
        agent: Option<Uuid>,
        conversation: Option<Uuid>,
    ) -> tembra::Result<NativeAgent> {
        self.agent(project, agent, conversation, true)
    }
    /// Always issue a distinct child identity/generation, including for the same agent definition.
    /// Parent opaque context values must not be reused as a child's authority.
    pub fn child_agent(
        &self,
        project: Option<Uuid>,
        agent: Option<Uuid>,
        conversation: Option<Uuid>,
    ) -> tembra::Result<NativeAgent> {
        self.agent(project, agent, conversation, false)
    }
    fn agent(
        &self,
        project: Option<Uuid>,
        agent: Option<Uuid>,
        conversation: Option<Uuid>,
        main_agent: bool,
    ) -> tembra::Result<NativeAgent> {
        let active = self.control.active().ok();
        let human = active
            .as_ref()
            .map(|active| active.human.clone())
            .unwrap_or_else(|| self.control.shell.borrow().clone());
        let binding = self
            .control
            .authority
            .execution(
                &human,
                ExecutionIdentity {
                    account: human.identity().account,
                    execution: Uuid::new_v4(),
                    generation: Uuid::new_v4(),
                    project,
                    agent,
                    conversation,
                    main_agent,
                },
            )
            .map_err(failure)?;
        let port = Rc::new(crate::cards::CardPort(Rc::downgrade(&self.control)));
        let tools = if let Some(active) = &active {
            Some(Rc::new(VaultTools {
                node: active.node.clone(),
                session: active.session.clone(),
                plugin: self.control.tool_grant.clone(),
                executions: self.executions.clone(),
                inputs: port.clone(),
                decisions: port.clone(),
                bash: Rc::new(BashExecutor::new(native_bash())),
                http: Rc::new(HttpExecutor::new(None).map_err(failure)?),
            }))
        } else {
            None
        };
        Ok(NativeAgent {
            node: self.clone(),
            active,
            binding,
            tools,
            port,
        })
    }
    pub fn close(&self) {
        self.control.close();
    }
}

/// Retained actual logical owner, not reconstructible from serialized execution IDs.
pub struct NativeAgent {
    node: NativeNode,
    active: Option<Rc<Active>>,
    binding: ExecutionBinding,
    tools: Option<Rc<VaultTools>>,
    port: Rc<crate::cards::CardPort>,
}
impl Drop for NativeAgent {
    fn drop(&mut self) {
        self.binding.close();
    }
}

impl NativeAgent {
    pub(super) fn on_owner_close(&self, closed: Box<dyn Fn()>) {
        let human = self.active.as_ref().map_or_else(
            || self.node.control.shell.borrow().clone(),
            |active| active.human.clone(),
        );
        human.on_close(closed);
    }

    /// Rebuild after native sign-in, vault selection or unlock changes the owner. Consuming the old
    /// logical owner fences its pending calls and approvals; stable agent/project IDs are retained.
    /// The embedding replaces its model/tool Orbs from this returned owner before the next turn.
    pub fn refresh(self) -> tembra::Result<Self> {
        let identity = self.binding.identity().clone();
        let node = self.node.clone();
        drop(self);
        node.agent(
            identity.project,
            identity.agent,
            identity.conversation,
            identity.main_agent,
        )
    }
    /// Bind each physical root while retaining the same logical generation across turns.
    /// The returned physical scope cannot outlive retirement of this logical owner.
    pub fn enter(&self, context: &Context) -> tembra::Result<NativeExecution> {
        Ok(NativeExecution {
            bound: self.node.executions.bind(context, self.binding.clone())?,
        })
    }
    pub fn identity(&self) -> &ExecutionIdentity {
        self.binding.identity()
    }
    pub fn tools(&self) -> Option<Rc<VaultTools>> {
        self.tools.clone()
    }
    pub(super) fn current(&self) -> bool {
        if !self.binding.is_live() {
            return false;
        }
        match (&self.active, self.node.control.active().ok()) {
            (Some(previous), Some(current)) => Rc::ptr_eq(previous, &current),
            (None, None) => self
                .binding
                .shell()
                .same_owner(&self.node.control.shell.borrow().binding()),
            _ => false,
        }
    }
    pub(super) fn protection(&self) -> ModelProtection {
        ModelProtection::Guarded {
            guard: self.node.control.guard.clone(),
            executions: self.node.executions.clone(),
            decisions: self.port.clone(),
        }
    }
    /// The node's shared final guard is mandatory for every configured connection in this Orb.
    pub fn models(
        &self,
        configuration: Models,
        references: BTreeMap<String, SecretReference>,
        metadata: impl Fn(
            &swarmie_providers::ai::Connection,
            &swarmie_providers::ai::ModelId,
        )
            -> tembra::Result<std::sync::Arc<swarmie_providers::ai::catalog::Snapshot>>
        + 'static,
    ) -> tembra::Result<Orb> {
        let mut plugins = BTreeMap::new();
        for connection in &configuration.connections {
            for model in &connection.models {
                let identity = format!("{}:{}", connection.id.as_str(), model.as_str());
                plugins.insert(
                    identity.clone(),
                    PluginGrant::bundled(
                        CorePlugin::ModelCredentials,
                        format!("swarmie.models.ai:{identity}"),
                    ),
                );
            }
        }
        let credentials: Rc<dyn swarmie_providers::ai::Credentials> =
            if let Some(active) = &self.active {
                Rc::new(VaultCredentials::new(
                    active.node.clone(),
                    active.session.clone(),
                    plugins,
                    self.port.clone(),
                    references,
                ))
            } else {
                if !references.is_empty() {
                    return Err(unavailable());
                }
                Rc::new(NoCredentials)
            };
        swarmie_providers::ai::orb(configuration, credentials, self.protection(), metadata)
    }
}

struct NoCredentials;
impl swarmie_providers::ai::Credentials for NoCredentials {
    fn acquire<'a>(
        &'a self,
        _: &'a Context,
        _: &'a swarmie_providers::ai::AdmittedAttempt,
        _: &'a str,
    ) -> futures::future::LocalBoxFuture<'a, tembra::Result<swarmie_providers::ai::ModelCredential>>
    {
        Box::pin(async { Err(unavailable()) })
    }
}

/// One physical context; dropping it fences pending approvals without ending the conversation.
pub struct NativeExecution {
    bound: BoundExecutionContext,
}
impl NativeExecution {
    pub fn context(&self) -> &Context {
        self.bound.context()
    }
}
