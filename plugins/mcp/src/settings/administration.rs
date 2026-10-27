use super::{Definition, DefinitionStore, SaveDefinition, ServerAction, ServerStatus};
use crate::{McpMethod, Observation, error::busy, invalid};
use futures::lock::Mutex;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use tembra::{Context, Host, Reference};

pub(crate) const MANAGED_SERVICE: &str = "swarmie.mcp.configured";

/// Keeps administrative references. Tembra owns the referenced resources and calls.
pub(crate) struct Administration {
    pub host: Host,
    pub store: Rc<dyn DefinitionStore>,
    pub principal: String,
    references: RefCell<BTreeMap<Vec<String>, Rc<Reference>>>,
    changing: Mutex<()>,
}

impl Administration {
    pub fn new(host: Host, store: Rc<dyn DefinitionStore>, principal: String) -> Self {
        Self {
            host,
            store,
            principal,
            references: RefCell::new(BTreeMap::new()),
            changing: Mutex::new(()),
        }
    }

    fn reference(&self, definition: &Definition) -> tembra::Result<Rc<Reference>> {
        let key = definition.key();
        if let Some(reference) = self.references.borrow().get(&key) {
            return Ok(reference.clone());
        }
        let reference = Rc::new(
            self.host
                .get(MANAGED_SERVICE, key.clone(), &self.principal)?,
        );
        self.references.borrow_mut().insert(key, reference.clone());
        Ok(reference)
    }

    fn inventory(&self) -> tembra::Result<Vec<Definition>> {
        let definitions = self.store.load(&self.principal)?;
        super::store::validate_inventory(&definitions)?;
        Ok(definitions)
    }

    fn selected(&self, input: &ServerAction) -> tembra::Result<Definition> {
        self.inventory()?
            .into_iter()
            .find(|entry| {
                entry.id.as_str() == input.id.as_str() && entry.revision == input.revision
            })
            .ok_or_else(|| invalid("MCP settings changed; refresh before this action"))
    }

    pub async fn list(&self, context: &Context) -> tembra::Result<Vec<ServerStatus>> {
        context.check()?;
        let mut result = Vec::new();
        for definition in self.inventory()? {
            let reference = self.reference(&definition)?;
            let observation = reference
                .call(context, McpMethod::Describe.as_str(), ())
                .await?;
            result.push(ServerStatus {
                definition,
                observation,
            });
        }
        Ok(result)
    }

    async fn disable_reference(
        &self,
        context: &Context,
        definition: &Definition,
    ) -> tembra::Result<()> {
        self.reference(definition)?
            .call(context, McpMethod::Disable.as_str(), ())
            .await
    }

    pub async fn save(
        &self,
        context: &Context,
        input: SaveDefinition,
    ) -> tembra::Result<Definition> {
        context.check()?;
        let _changing = self.changing.try_lock().ok_or_else(busy)?;
        let before = self.inventory()?;
        let previous = before
            .iter()
            .find(|entry| entry.id.as_str() == input.definition.id.as_str());
        if previous.map(|entry| entry.revision) != input.expected_revision {
            return Err(invalid("MCP settings changed; refresh before saving"));
        }
        if previous.is_some_and(|entry| {
            std::mem::discriminant(&entry.endpoint)
                != std::mem::discriminant(&input.definition.endpoint)
        }) {
            return Err(invalid(
                "remove the server before changing its connection type",
            ));
        }
        let mut definition = input.definition;
        definition.revision = input
            .expected_revision
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| invalid("MCP revision limit reached"))?;
        definition.enabled = false;
        definition.validate()?;
        if let Some(previous) = previous {
            self.disable_reference(context, previous).await?;
        }
        let mut next = before
            .iter()
            .filter(|entry| entry.id.as_str() != definition.id.as_str())
            .cloned()
            .collect::<Vec<_>>();
        next.push(definition.clone());
        context.check()?;
        self.store.replace(&self.principal, &before, &next)?;
        self.references
            .borrow_mut()
            .retain(|key, _| key[0] != definition.id.as_str());
        Ok(definition)
    }

    pub async fn set_enabled(
        &self,
        context: &Context,
        input: ServerAction,
        enabled: bool,
    ) -> tembra::Result<ServerStatus> {
        context.check()?;
        let _changing = self.changing.try_lock().ok_or_else(busy)?;
        let mut definition = self.selected(&input)?;
        let before = self.inventory()?;
        if !enabled {
            self.disable_reference(context, &definition).await?;
        }
        definition.enabled = enabled;
        let next = before
            .iter()
            .map(|entry| {
                if entry.id.as_str() == definition.id.as_str() {
                    definition.clone()
                } else {
                    entry.clone()
                }
            })
            .collect::<Vec<_>>();
        context.check()?;
        self.store.replace(&self.principal, &before, &next)?;
        let method = if enabled {
            McpMethod::Connect
        } else {
            McpMethod::Describe
        };
        let observation: Observation = self
            .reference(&definition)?
            .call(context, method.as_str(), ())
            .await?;
        Ok(ServerStatus {
            definition,
            observation,
        })
    }

    pub async fn remove(&self, context: &Context, input: ServerAction) -> tembra::Result<()> {
        context.check()?;
        let _changing = self.changing.try_lock().ok_or_else(busy)?;
        let definition = self.selected(&input)?;
        let before = self.inventory()?;
        self.disable_reference(context, &definition).await?;
        let next = before
            .iter()
            .filter(|entry| entry.id.as_str() != definition.id.as_str())
            .cloned()
            .collect::<Vec<_>>();
        context.check()?;
        self.store.replace(&self.principal, &before, &next)?;
        self.references
            .borrow_mut()
            .retain(|key, _| key[0] != definition.id.as_str());
        Ok(())
    }
}
