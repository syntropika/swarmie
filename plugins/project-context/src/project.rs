use crate::{Error, Inventory, Limits, Result, Scope, Selection, files, skills};
use std::{collections::BTreeSet, path::PathBuf, sync::OnceLock};
use swarmie_providers::{ContextSources, SourceKind, SourcePart, SourceScope, source_digest};

/// Paths and policy are selected by the embedding, not by a model or renderer.
#[derive(Clone, Debug, Default)]
pub struct Configuration {
    pub skill_roots: Vec<PathBuf>,
    pub personal_instructions: Vec<PathBuf>,
    pub limits: Limits,
}

/// A concrete native source owner. It never executes model, tool or package code.
#[derive(Clone)]
pub struct ProjectContext {
    scope: Scope,
    configuration: Configuration,
    inventory: OnceLock<Inventory>,
}

impl ProjectContext {
    pub fn new(scope: Scope, configuration: Configuration) -> Result<Self> {
        configuration.limits.validate()?;
        Ok(Self {
            scope,
            configuration,
            inventory: OnceLock::new(),
        })
    }

    pub fn inspect(&self) -> Result<Inventory> {
        if let Some(inventory) = self.inventory.get() {
            return Ok(inventory.clone());
        }
        let mut roots = self
            .configuration
            .skill_roots
            .iter()
            .map(|path| (path.clone(), "user".into()))
            .collect::<Vec<_>>();
        roots.extend(
            self.scope
                .ancestors()?
                .into_iter()
                .map(|path| (path.join(".agents/skills"), "project".into())),
        );
        let inventory = skills::discover(&roots, &self.configuration.limits)?;
        let _ = self.inventory.set(inventory.clone());
        Ok(inventory)
    }

    pub fn read(&self, selection: &Selection, relative: &str) -> Result<String> {
        let inventory = self.inspect()?;
        let skill = inventory
            .skills
            .iter()
            .find(|skill| skill.selection.id == selection.id)
            .ok_or(Error::Changed)?;
        skills::load(skill, selection, relative, &self.configuration.limits)
    }

    /// Capture ordered instructions and optional explicit skill bodies. All bytes
    /// are loaded before model intent. A later request must capture independently.
    pub fn capture(
        &self,
        node: &str,
        principal: &str,
        selected: &[Selection],
    ) -> Result<ContextSources> {
        let mut parts = Vec::new();
        let mut seen = BTreeSet::new();
        let mut instruction_files = self
            .configuration
            .personal_instructions
            .iter()
            .cloned()
            .map(|file| (file, false))
            .collect::<Vec<_>>();
        instruction_files.extend(
            self.scope
                .ancestors()?
                .into_iter()
                .map(|path| (path.join("AGENTS.md"), true)),
        );
        let mut resolved = Vec::new();
        for (file, optional) in instruction_files {
            match std::fs::symlink_metadata(&file) {
                Err(error) if optional && error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
                Ok(_) => {}
            }
            let real = file.canonicalize()?;
            if file.starts_with(self.scope.root()) && !real.starts_with(self.scope.root()) {
                return Err(Error::Invalid(
                    "project instruction link escapes the project".into(),
                ));
            }
            resolved.push((file, real));
        }
        // Keep the most specific occurrence when two paths name the same file.
        resolved.reverse();
        resolved.retain(|(_, real)| seen.insert(real.clone()));
        resolved.reverse();
        let mut captured = Vec::new();
        for (file, real) in resolved {
            let bytes = files::read(&real, self.configuration.limits.max_text_bytes)?;
            let content = files::text(&bytes)?.to_owned();
            let id = source_digest(files::path_text(&real)?.as_bytes());
            let revision = source_digest(&bytes);
            if file.canonicalize()? != real {
                return Err(Error::Changed);
            }
            captured.push((file.clone(), real.clone(), bytes));
            parts.push(
                SourcePart::new(
                    SourceKind::ProjectInstructions,
                    id,
                    revision,
                    files::path_text(&file)?.into(),
                    content,
                )
                .map_err(|_| Error::Limit)?,
            );
        }
        let inventory = self.inspect()?;
        let catalog = inventory.listing()?;
        if inventory
            .skills
            .iter()
            .any(|skill| skill.model_invocable && skill.unsupported.is_empty())
        {
            parts.push(
                SourcePart::new(
                    SourceKind::SkillCatalog,
                    "swarmie.skills.catalog".into(),
                    source_digest(catalog.as_bytes()),
                    "Available skills".into(),
                    catalog,
                )
                .map_err(|_| Error::Limit)?,
            );
        }
        let mut selected_ids = BTreeSet::new();
        for selection in selected {
            if !selected_ids.insert(&selection.id) {
                return Err(Error::Invalid("a skill was selected more than once".into()));
            }
            let skill = inventory
                .skills
                .iter()
                .find(|skill| skill.selection.id == selection.id)
                .ok_or(Error::Changed)?;
            if !skill.user_invocable {
                return Err(Error::Unsupported);
            }
            let body = skills::load(skill, selection, "SKILL.md", &self.configuration.limits)?;
            parts.push(
                SourcePart::new(
                    SourceKind::SkillInstructions,
                    selection.id.clone(),
                    selection.revision.clone(),
                    skill.name.clone(),
                    body,
                )
                .map_err(|_| Error::Limit)?,
            );
        }
        for (file, real, bytes) in captured {
            if file.canonicalize()? != real
                || files::read(&real, self.configuration.limits.max_text_bytes)? != bytes
            {
                return Err(Error::Changed);
            }
        }
        ContextSources::new(
            SourceScope {
                node: node.into(),
                principal: principal.into(),
                directory: files::path_text(self.scope.directory())?.into(),
            },
            parts,
        )
        .map_err(|_| Error::Limit)
    }
}
