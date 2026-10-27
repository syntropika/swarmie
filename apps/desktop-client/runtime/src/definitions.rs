use std::{collections::BTreeMap, io::Write, path::PathBuf};
use swarmie_mcp::settings::{Definition, DefinitionStore};

/// One native host owns writes to this file. The path belongs to the embedding.
pub(crate) struct Definitions(pub PathBuf);

impl Definitions {
    fn all(&self) -> tembra::Result<BTreeMap<String, Vec<Definition>>> {
        match std::fs::read(&self.0) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| failure()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(_) => Err(failure()),
        }
    }
}

fn failure() -> tembra::Error {
    tembra::Error::new(
        tembra::Code::Unavailable,
        "mcp.settings_store",
        "MCP settings could not be read or saved. Existing data has been preserved.",
    )
}

impl DefinitionStore for Definitions {
    fn load(&self, principal: &str) -> tembra::Result<Vec<Definition>> {
        Ok(self.all()?.remove(principal).unwrap_or_default())
    }

    fn replace(
        &self,
        principal: &str,
        expected: &[Definition],
        next: &[Definition],
    ) -> tembra::Result<()> {
        let mut data = self.all()?;
        let current = data.get(principal).cloned().unwrap_or_default();
        if serde_json::to_value(current).map_err(|_| failure())?
            != serde_json::to_value(expected).map_err(|_| failure())?
        {
            return Err(failure());
        }
        for definition in next {
            definition.validate()?;
        }
        data.insert(principal.into(), next.to_vec());
        let parent = self.0.parent().ok_or_else(failure)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|_| failure())?;
        serde_json::to_writer(&mut temporary, &data).map_err(|_| failure())?;
        temporary.flush().map_err(|_| failure())?;
        temporary.as_file().sync_all().map_err(|_| failure())?;
        temporary.persist(&self.0).map_err(|_| failure())?;
        Ok(())
    }
}
