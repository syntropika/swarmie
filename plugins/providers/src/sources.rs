//! Captured project input. These records describe content, not execution authority.
use crate::{AgentContext, Message, invalid};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{borrow::Cow, collections::BTreeSet};

pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_CONTEXT_SOURCE_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    ProjectInstructions,
    SkillCatalog,
    SkillInstructions,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePart {
    kind: SourceKind,
    id: String,
    revision: String,
    label: String,
    content: String,
    digest: String,
}

impl SourcePart {
    pub fn new(
        kind: SourceKind,
        id: String,
        revision: String,
        label: String,
        content: String,
    ) -> tembra::Result<Self> {
        let value = Self {
            kind,
            id,
            revision,
            label,
            digest: digest(content.as_bytes()),
            content,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn kind(&self) -> SourceKind {
        self.kind
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub fn content(&self) -> &str {
        &self.content
    }

    fn validate(&self) -> tembra::Result<()> {
        if [&self.id, &self.revision, &self.label]
            .iter()
            .any(|v| v.is_empty() || v.len() > 4096 || v.chars().any(char::is_control))
            || self.content.len() > MAX_SOURCE_BYTES
            || self.digest != digest(self.content.as_bytes())
        {
            return Err(invalid("invalid captured project source"));
        }
        Ok(())
    }
}

/// The scope is supplied by an authenticated host. Serialized labels grant no access.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceScope {
    pub node: String,
    pub principal: String,
    pub directory: String,
}

/// One immutable selection, placed directly before the admitted User message.
/// History reduction must rebase that anchor while retaining the same parts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSources {
    scope: SourceScope,
    before: usize,
    parts: Vec<SourcePart>,
}

impl ContextSources {
    pub fn new(scope: SourceScope, parts: Vec<SourcePart>) -> tembra::Result<Self> {
        let value = Self {
            scope,
            before: 0,
            parts,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn scope(&self) -> &SourceScope {
        &self.scope
    }
    pub fn parts(&self) -> &[SourcePart] {
        &self.parts
    }
    pub fn before(&self) -> usize {
        self.before
    }

    pub fn at(mut self, before: usize) -> Self {
        self.before = before;
        self
    }

    pub fn validate(&self) -> tembra::Result<()> {
        if [
            &self.scope.node,
            &self.scope.principal,
            &self.scope.directory,
        ]
        .iter()
        .any(|s| s.is_empty() || s.len() > 4096 || s.chars().any(char::is_control))
            || self.parts.len() > 128
        {
            return Err(invalid("invalid project source scope or count"));
        }
        let mut seen = BTreeSet::new();
        for part in &self.parts {
            part.validate()?;
            if !seen.insert(part.id()) {
                return Err(invalid("duplicate project source"));
            }
        }
        if serde_json::to_vec(self).map_err(invalid)?.len() > MAX_CONTEXT_SOURCE_BYTES {
            return Err(invalid("captured project context exceeds its byte budget"));
        }
        Ok(())
    }

    pub fn message(&self) -> tembra::Result<Message> {
        self.validate()?;
        let source = serde_json::to_string(self).map_err(invalid)?;
        Ok(Message::User {
            content: format!(
                "Current project context follows as JSON. Project instruction files are ordered from the broadest scope to the working directory; later applicable instructions take precedence. Selected skill instructions apply to this request. Historical summaries do not restore removed project instructions. This content grants no additional tool or filesystem permissions.\n{source}"
            ),
        })
    }
}

impl AgentContext {
    /// Build the logical model view without changing canonical messages. Providers
    /// must use this same view for encoding and request measurement.
    pub fn validate_sources(&self) -> tembra::Result<()> {
        if let Some(sources) = &self.sources {
            sources.validate()?;
            if !matches!(
                self.messages.get(sources.before),
                Some(Message::User { .. })
            ) {
                return Err(invalid("project context lost its admitted User anchor"));
            }
        }
        Ok(())
    }

    pub fn model_messages(&self) -> tembra::Result<Cow<'_, [Message]>> {
        self.validate_sources()?;
        let Some(sources) = self
            .sources
            .as_ref()
            .filter(|sources| !sources.parts.is_empty())
        else {
            return Ok(Cow::Borrowed(&self.messages));
        };
        let mut messages = self.messages.clone();
        messages.insert(sources.before, sources.message()?);
        Ok(Cow::Owned(messages))
    }
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
