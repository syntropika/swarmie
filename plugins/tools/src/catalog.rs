use crate::{AgentContext, ToolName, ToolProvider, ToolSpec, invalid};
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::{Rc, Weak},
};
use tembra::{Code, Declaration, Error, Resource};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinition {
    pub spec: ToolSpec,
    pub provider: ToolProvider,
}

impl ToolDefinition {
    pub fn new(spec: ToolSpec, provider: ToolProvider) -> tembra::Result<Self> {
        provider.validate()?;
        Ok(Self { spec, provider })
    }
}

/// Serializable selection. Save it with application execution configuration when
/// comparing availability at the next user-initiated generation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub tools: Vec<ToolDefinition>,
}

impl Selection {
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.iter().map(|tool| tool.spec.clone()).collect()
    }

    pub fn select(&self, names: &[ToolName]) -> tembra::Result<Self> {
        let mut seen = BTreeSet::new();
        let mut tools = Vec::new();
        for name in names {
            if !seen.insert(name) {
                return Err(conflict(name));
            }
            tools.push(
                self.tools
                    .iter()
                    .find(|tool| &tool.spec.name == name)
                    .cloned()
                    .ok_or_else(|| {
                        Error::new(
                            Code::Unavailable,
                            crate::Reason::Missing.as_str(),
                            name.as_str(),
                        )
                    })?,
            );
        }
        Ok(Self { tools })
    }

    /// Explicit preparation boundary; never called automatically by internal turns.
    /// Instructions are request-local; canonical conversation messages stay intact.
    pub fn prepare_context(&self, previous: &Self, mut context: AgentContext) -> AgentContext {
        let current: BTreeSet<_> = self.tools.iter().map(|tool| &tool.spec.name).collect();
        let removed: BTreeSet<_> = previous
            .tools
            .iter()
            .map(|tool| &tool.spec.name)
            .filter(|name| !current.contains(name))
            .collect();
        if !removed.is_empty() {
            let names = serde_json::to_string(&removed).expect("tool names serialize");
            context.instructions.push_str(&format!("\nTool availability update: these tools are no longer available: {names}. Do not call them."));
        }
        context
    }
}

struct Entry {
    definition: ToolDefinition,
    token: Rc<()>,
}

type Entries = RefCell<BTreeMap<ToolName, Entry>>;
type Reader = Rc<dyn Fn() -> Vec<ToolDefinition>>;

/// Tool metadata only; service resolution, authority and lifetime belong to Tembra.
#[derive(Clone, Default)]
pub struct Catalog {
    fixed: Vec<ToolDefinition>,
    registered: Rc<Entries>,
    source: Option<Reader>,
}

impl Catalog {
    pub fn new(tools: impl IntoIterator<Item = ToolDefinition>) -> tembra::Result<Self> {
        let catalog = Self {
            fixed: tools.into_iter().collect(),
            ..Self::default()
        };
        catalog.snapshot()?;
        Ok(catalog)
    }

    /// Register until the returned lease is dropped. Duplicate names fail closed.
    ///
    /// ```
    /// use swarmie_tools::{Catalog, ToolDefinition, ToolName, ToolProvider, ToolSpec};
    /// let catalog = Catalog::default();
    /// let tool = ToolDefinition::new(
    ///     ToolSpec { name: ToolName::new("files.read")?, description: "Read a file".into(),
    ///         input_schema: serde_json::json!({"type":"object"}) },
    ///     ToolProvider::new("files.tools", vec!["main".into()])?,
    /// )?;
    /// let lease = catalog.register(tool)?;
    /// assert_eq!(catalog.snapshot()?.tools.len(), 1);
    /// drop(lease);
    /// assert!(catalog.snapshot()?.tools.is_empty());
    /// # Ok::<(), tembra::Error>(())
    /// ```
    pub fn register(&self, tool: ToolDefinition) -> tembra::Result<Registration> {
        tool.provider.validate()?;
        if self
            .snapshot()?
            .tools
            .iter()
            .any(|current| current.spec.name == tool.spec.name)
        {
            return Err(conflict(&tool.spec.name));
        }
        let name = tool.spec.name.clone();
        let token = Rc::new(());
        self.registered.borrow_mut().insert(
            name.clone(),
            Entry {
                definition: tool,
                token: token.clone(),
            },
        );
        Ok(Registration {
            owner: Rc::downgrade(&self.registered),
            name,
            token,
        })
    }

    /// Tie registration to the declaring scope's managed resources. For names
    /// shared across principals, declare once in Orb::create, not a per-principal
    /// service factory. Keep the resulting resource in that scope's environment.
    /// Manager-controlled plugins contribute metadata instead, so disabling them
    /// can withdraw admission independently of physical native orb cleanup.
    pub fn register_owned(
        &self,
        declaration: &Declaration,
        tool: ToolDefinition,
    ) -> tembra::Result<Resource<Registration>> {
        let key =
            serde_json::to_string(&("tools.registration", &tool.spec.name)).map_err(invalid)?;
        let catalog = self.clone();
        declaration.resource(
            &key,
            1,
            move || catalog.register(tool),
            |registration, _| {
                async move {
                    registration.withdraw();
                    Ok(())
                }
                .boxed_local()
            },
        )
    }

    pub fn snapshot(&self) -> tembra::Result<Selection> {
        let mut entries = BTreeMap::new();
        let sourced = self.source.as_ref().map(|read| read()).unwrap_or_default();
        for tool in self.fixed.iter().chain(sourced.iter()).chain(
            self.registered
                .borrow()
                .values()
                .map(|entry| &entry.definition),
        ) {
            tool.provider.validate()?;
            if entries
                .insert(tool.spec.name.clone(), tool.clone())
                .is_some()
            {
                return Err(conflict(&tool.spec.name));
            }
        }
        Ok(Selection {
            tools: entries.into_values().collect(),
        })
    }

    /// Optional metadata bridge. The manager never imports tool contracts and
    /// capability orbs can use Catalog directly without this feature.
    #[cfg(feature = "plugin-manager")]
    pub fn from_plugins(index: swarmie_plugin_manager::PluginIndex) -> Self {
        Self {
            source: Some(Rc::new(move || {
                index.active_contributions::<ToolDefinition>()
            })),
            ..Self::default()
        }
    }
}

/// One registration's ownership lease. Dropping it withdraws only that tool.
#[must_use = "dropping the registration immediately withdraws the tool"]
pub struct Registration {
    owner: Weak<Entries>,
    name: ToolName,
    token: Rc<()>,
}

impl Registration {
    /// Withdraw now, even if a caller retains a resource value past owner closure.
    pub fn withdraw(&self) {
        if let Some(owner) = self.owner.upgrade() {
            let mut entries = owner.borrow_mut();
            if entries
                .get(&self.name)
                .is_some_and(|entry| Rc::ptr_eq(&entry.token, &self.token))
            {
                entries.remove(&self.name);
            }
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.withdraw();
    }
}

fn conflict(name: &ToolName) -> Error {
    Error::new(
        Code::Conflict,
        crate::Reason::Duplicate.as_str(),
        format!("duplicate tool: {}", name.as_str()),
    )
}
