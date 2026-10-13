use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalStoreIdentity {
    pub store_id: String,
    pub environment_id: String,
    pub owner_scope_id: String,
}
impl Default for LocalStoreIdentity {
    fn default() -> Self {
        Self {
            store_id: uuid::Uuid::new_v4().to_string(),
            environment_id: uuid::Uuid::new_v4().to_string(),
            owner_scope_id: uuid::Uuid::new_v4().to_string(),
        }
    }
}
/// A retained Environment identity and one live execution-owner incarnation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeIdentity {
    pub identity: LocalStoreIdentity,
    pub incarnation_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryRef {
    pub id: String,
    pub revision: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryOwnership {
    User,
    ExternalOwner,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryResource {
    pub id: String,
    pub revision: u64,
    pub environment_id: String,
    pub path: PathBuf,
    pub ownership: DirectoryOwnership,
    /// Node-local continuity evidence, not a filesystem lease or permission grant.
    pub fingerprint: Option<String>,
}
impl DirectoryResource {
    pub fn reference(&self) -> DirectoryRef {
        DirectoryRef {
            id: self.id.clone(),
            revision: self.revision,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectLaunchDefaults {
    pub harness: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub config: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRecord {
    pub id: String,
    pub revision: u64,
    pub name: String,
    pub archived: bool,
    pub directory: Option<DirectoryRef>,
    pub launch: ProjectLaunchDefaults,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DefaultDirectory {
    pub revision: u64,
    pub environment_id: String,
    pub owner_scope_id: String,
    pub directory: DirectoryRef,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LocalCatalog {
    pub identity: LocalStoreIdentity,
    pub directories: BTreeMap<String, DirectoryResource>,
    pub projects: BTreeMap<String, ProjectRecord>,
    pub default_directory: Option<DefaultDirectory>,
}
#[derive(Clone, Debug)]
pub enum CatalogUpdate {
    RegisterDirectory {
        path: PathBuf,
        fingerprint: Option<String>,
        ownership: DirectoryOwnership,
    },
    CreateProject {
        name: String,
        directory: Option<DirectoryRef>,
        launch: ProjectLaunchDefaults,
    },
    UpdateProject(ProjectUpdate),
    SetDefault {
        expected_revision: u64,
        directory: DirectoryRef,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectCreate {
    pub name: String,
    pub directory: Option<DirectoryRef>,
    #[serde(default)]
    pub launch: ProjectLaunchDefaults,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectoryRegistration {
    pub path: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefaultDirectoryUpdate {
    pub expected_revision: u64,
    pub directory: DirectoryRef,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectUpdate {
    pub project_id: String,
    pub expected_revision: u64,
    pub name: Option<String>,
    pub archived: Option<bool>,
    /// Omission retains the selection; explicit unlink is a separate flag.
    pub directory: Option<DirectoryRef>,
    #[serde(default)]
    pub unlink_directory: bool,
    pub launch: Option<ProjectLaunchDefaults>,
}
#[derive(Clone, Debug)]
pub enum CatalogRecord {
    Directory(DirectoryResource),
    Project(ProjectRecord),
    Default(DefaultDirectory),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContextSource {
    ProjectFolder {
        #[serde(rename = "projectId")]
        project_id: String,
        revision: u64,
    },
    ProjectFreeDefault {
        revision: u64,
    },
    /// An explicit host-directory selection by a CLI/library launcher.
    ExplicitDirectory,
    NativeExisting,
}
#[derive(Clone, Debug)]
pub enum ContextSelection {
    Project {
        id: String,
        expected_revision: Option<u64>,
    },
    Default {
        expected_revision: Option<u64>,
    },
    Inherited {
        context_id: String,
    },
    Directory {
        directory: DirectoryRef,
        native_existing: bool,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkingContext {
    pub id: String,
    pub revision: u64,
    pub session_id: String,
    pub binding_id: String,
    pub environment_id: String,
    pub directory: DirectoryResource,
    pub source: ContextSource,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContextStatus {
    pub context: WorkingContext,
    pub available: bool,
    pub reason: Option<String>,
}
impl LocalCatalog {
    pub fn directory(&self, reference: &DirectoryRef) -> Result<&DirectoryResource> {
        self.directories
            .get(&reference.id)
            .filter(|d| {
                d.revision == reference.revision && d.environment_id == self.identity.environment_id
            })
            .ok_or_else(|| {
                Error::Invalid("directory reference is stale or belongs to another node".into())
            })
    }
    pub fn apply(&mut self, update: CatalogUpdate) -> Result<CatalogRecord> {
        match update {
            CatalogUpdate::RegisterDirectory {
                path,
                fingerprint,
                ownership,
            } => {
                if !path.is_absolute() {
                    return Err(Error::Invalid("directory locator must be absolute".into()));
                }
                if let Some(existing) = self.directories.values().find(|d| {
                    d.path == path && d.fingerprint == fingerprint && d.ownership == ownership
                }) {
                    return Ok(CatalogRecord::Directory(existing.clone()));
                }
                let directory = DirectoryResource {
                    id: uuid::Uuid::new_v4().to_string(),
                    revision: 1,
                    environment_id: self.identity.environment_id.clone(),
                    path,
                    fingerprint,
                    ownership,
                };
                self.directories
                    .insert(directory.id.clone(), directory.clone());
                Ok(CatalogRecord::Directory(directory))
            }
            CatalogUpdate::CreateProject {
                name,
                directory,
                launch,
            } => {
                validate_name(&name)?;
                validate_project_config(&launch.config)?;
                if let Some(d) = &directory {
                    self.user_directory(d)?;
                }
                let project = ProjectRecord {
                    id: uuid::Uuid::new_v4().to_string(),
                    revision: 1,
                    name,
                    archived: false,
                    directory,
                    launch,
                };
                self.projects.insert(project.id.clone(), project.clone());
                Ok(CatalogRecord::Project(project))
            }
            CatalogUpdate::UpdateProject(update) => {
                let mut project = self
                    .projects
                    .get(&update.project_id)
                    .cloned()
                    .ok_or_else(|| Error::Invalid("project not found".into()))?;
                if project.revision != update.expected_revision {
                    return Err(Error::Invalid(
                        "project revision changed; reload before editing".into(),
                    ));
                }
                if update.unlink_directory && update.directory.is_some() {
                    return Err(Error::Invalid(
                        "cannot link and unlink a directory together".into(),
                    ));
                }
                if let Some(name) = update.name {
                    validate_name(&name)?;
                    project.name = name;
                }
                if let Some(launch) = update.launch {
                    validate_project_config(&launch.config)?;
                    project.launch = launch;
                }
                if let Some(archived) = update.archived {
                    project.archived = archived;
                }
                if let Some(directory) = update.directory {
                    self.user_directory(&directory)?;
                    project.directory = Some(directory);
                }
                if update.unlink_directory {
                    project.directory = None;
                }
                project.revision = project
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| Error::Storage("project revision overflow".into()))?;
                self.projects.insert(project.id.clone(), project.clone());
                Ok(CatalogRecord::Project(project))
            }
            CatalogUpdate::SetDefault {
                expected_revision,
                directory,
            } => {
                self.user_directory(&directory)?;
                let current = self.default_directory.as_ref().map_or(0, |d| d.revision);
                if expected_revision != current {
                    return Err(Error::Invalid(
                        "default directory revision changed; reload before editing".into(),
                    ));
                }
                let selected = DefaultDirectory {
                    revision: current.checked_add(1).ok_or_else(|| {
                        Error::Storage("default directory revision overflow".into())
                    })?,
                    environment_id: self.identity.environment_id.clone(),
                    owner_scope_id: self.identity.owner_scope_id.clone(),
                    directory,
                };
                self.default_directory = Some(selected.clone());
                Ok(CatalogRecord::Default(selected))
            }
        }
    }
    fn user_directory(&self, reference: &DirectoryRef) -> Result<()> {
        let directory = self.directory(reference)?;
        if directory.ownership != DirectoryOwnership::User || directory.fingerprint.is_none() {
            return Err(Error::Invalid(
                "folder selection requires a verified user directory".into(),
            ));
        }
        Ok(())
    }
    pub fn select(
        &self,
        selection: &ContextSelection,
    ) -> Result<(DirectoryResource, ContextSource)> {
        let (reference, source) = match selection {
            ContextSelection::Project {
                id,
                expected_revision,
            } => {
                let project = self
                    .projects
                    .get(id)
                    .filter(|p| !p.archived)
                    .ok_or_else(|| Error::Invalid("project is missing or archived".into()))?;
                if expected_revision.is_some_and(|r| r != project.revision) {
                    return Err(Error::Invalid(
                        "project revision changed; reload before launching".into(),
                    ));
                }
                (
                    project
                        .directory
                        .as_ref()
                        .ok_or_else(|| Error::Invalid("project has no linked directory".into()))?,
                    ContextSource::ProjectFolder {
                        project_id: id.clone(),
                        revision: project.revision,
                    },
                )
            }
            ContextSelection::Default { expected_revision } => {
                let selected = self.default_directory.as_ref().ok_or_else(|| {
                    Error::Invalid("choose a project-free working directory in settings".into())
                })?;
                if expected_revision.is_some_and(|r| r != selected.revision) {
                    return Err(Error::Invalid(
                        "default directory revision changed; reload before launching".into(),
                    ));
                }
                (
                    &selected.directory,
                    ContextSource::ProjectFreeDefault {
                        revision: selected.revision,
                    },
                )
            }
            ContextSelection::Inherited { .. } => {
                return Err(Error::Invalid(
                    "inherited context requires a retained parent".into(),
                ));
            }
            ContextSelection::Directory {
                directory,
                native_existing,
            } => (
                directory,
                if *native_existing {
                    ContextSource::NativeExisting
                } else {
                    ContextSource::ExplicitDirectory
                },
            ),
        };
        Ok((self.directory(reference)?.clone(), source))
    }
    pub fn prepare(
        &self,
        session_id: String,
        binding_id: String,
        selection: &ContextSelection,
    ) -> Result<WorkingContext> {
        let (directory, source) = self.select(selection)?;
        Ok(WorkingContext {
            id: uuid::Uuid::new_v4().to_string(),
            revision: 1,
            session_id,
            binding_id,
            environment_id: self.identity.environment_id.clone(),
            directory,
            source,
        })
    }
}
fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
        return Err(Error::Invalid(
            "project name must contain 1 to 256 bytes without control characters".into(),
        ));
    }
    Ok(())
}
pub fn validate_project_config(value: &Value) -> Result<()> {
    if !value.is_null() && !value.is_object() {
        return Err(Error::Invalid(
            "project configuration must be an object or null".into(),
        ));
    }
    validate_config_fields(value)
}
fn validate_config_fields(value: &Value) -> Result<()> {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                let lower = key.to_ascii_lowercase();
                let sensitive = lower == "env"
                    || lower == "headers"
                    || lower == "authorization"
                    || ["secret", "password", "apikey", "api_key", "api-key"]
                        .iter()
                        .any(|k| lower.contains(k))
                    || (lower.contains("token") && !(value.is_number() || value.is_boolean())
                        || lower.contains("token")
                            && ["auth", "access", "refresh", "bearer"]
                                .iter()
                                .any(|k| lower.contains(k)));
                if sensitive && !value.is_null() {
                    return Err(Error::Invalid(
                        "project configuration contains a sensitive field; use native credentials"
                            .into(),
                    ));
                }
                validate_config_fields(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_config_fields(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Verify a retained node resource without creating a directory or selecting a fallback.
pub async fn observe_directory(
    path: &Path,
    allow_missing: bool,
) -> Result<(PathBuf, Option<String>)> {
    let canonical = match tokio::fs::canonicalize(path).await {
        Ok(path) => path,
        Err(e)
            if allow_missing
                && matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
                ) =>
        {
            return Ok((std::path::absolute(path)?, None));
        }
        Err(e) => {
            return Err(Error::Invalid(format!(
                "working directory unavailable: {e}"
            )));
        }
    };
    let metadata = tokio::fs::metadata(&canonical).await?;
    if !metadata.is_dir() {
        return Err(Error::Invalid(
            "working directory must be a directory".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let created = metadata
            .created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos());
        Ok((
            canonical,
            Some(format!(
                "unix:{}:{}:{created:?}",
                metadata.dev(),
                metadata.ino()
            )),
        ))
    }
    #[cfg(windows)]
    {
        Ok((
            canonical.clone(),
            Some(windows::directory_identity(&canonical)?),
        ))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = metadata;
        Err(Error::Unsupported(
            "stable directory identity on this platform".into(),
        ))
    }
}
#[cfg(windows)]
mod windows;
pub async fn verify_context(context: &WorkingContext) -> Result<()> {
    let (path, fingerprint) = observe_directory(&context.directory.path, false).await?;
    if context.environment_id != context.directory.environment_id
        || path != context.directory.path
        || fingerprint.is_none()
        || context.directory.fingerprint != fingerprint
    {
        return Err(Error::Invalid(
            "working directory identity changed; restore or explicitly relink it before execution"
                .into(),
        ));
    }
    Ok(())
}
