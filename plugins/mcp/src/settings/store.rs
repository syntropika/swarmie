use super::Definition;
use crate::{invalid, unavailable};
use std::{cell::RefCell, collections::BTreeMap};

/// The embedding owns persistence and serializes writes to this store.
/// Replace must compare the expected inventory before it commits the new inventory.
pub trait DefinitionStore {
    fn load(&self, principal: &str) -> tembra::Result<Vec<Definition>>;
    fn replace(
        &self,
        principal: &str,
        expected: &[Definition],
        definitions: &[Definition],
    ) -> tembra::Result<()>;
}

/// In-memory configuration for embeddings that do not require restart persistence.
#[derive(Default)]
pub struct MemoryDefinitions(RefCell<BTreeMap<String, Vec<Definition>>>);

impl DefinitionStore for MemoryDefinitions {
    fn load(&self, principal: &str) -> tembra::Result<Vec<Definition>> {
        Ok(self.0.borrow().get(principal).cloned().unwrap_or_default())
    }

    fn replace(
        &self,
        principal: &str,
        expected: &[Definition],
        next: &[Definition],
    ) -> tembra::Result<()> {
        let current = self.load(principal)?;
        compare(&current, expected)?;
        validate_inventory(next)?;
        self.0.borrow_mut().insert(principal.into(), next.to_vec());
        Ok(())
    }
}

pub(crate) fn compare(a: &[Definition], b: &[Definition]) -> tembra::Result<()> {
    if serde_json::to_value(a).map_err(invalid)? != serde_json::to_value(b).map_err(invalid)? {
        return Err(unavailable("MCP settings changed; refresh before saving"));
    }
    Ok(())
}

pub(crate) fn validate_inventory(definitions: &[Definition]) -> tembra::Result<()> {
    let mut ids = std::collections::BTreeSet::new();
    for definition in definitions {
        definition.validate()?;
        if !ids.insert(definition.id.as_str()) {
            return Err(invalid("duplicate MCP server identity"));
        }
    }
    Ok(())
}
