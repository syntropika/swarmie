use super::*;
use std::path::Path;

impl RuntimeBuilder {
    /// Share one host authority with consumers that retain their own handles.
    pub fn shared_store(mut self, store: SharedStore) -> Self {
        self.store = store;
        self
    }
}
impl Runtime {
    pub async fn local_catalog(&self) -> Result<LocalCatalog> {
        self.0.store.local_catalog().await
    }
    pub async fn register_directory(&self, path: &Path) -> Result<DirectoryResource> {
        self.register_observed_directory(path, false).await
    }
    pub(super) async fn register_observed_directory(
        &self,
        path: &Path,
        native: bool,
    ) -> Result<DirectoryResource> {
        let (path, fingerprint) = observe_directory(path, native).await?;
        match self
            .0
            .store
            .update_catalog(CatalogUpdate::RegisterDirectory {
                path,
                fingerprint,
                ownership: if native {
                    DirectoryOwnership::ExternalOwner
                } else {
                    DirectoryOwnership::User
                },
            })
            .await?
        {
            CatalogRecord::Directory(record) => Ok(record),
            _ => unreachable!(),
        }
    }
    pub async fn create_project(
        &self,
        name: String,
        directory: Option<DirectoryRef>,
        launch: ProjectLaunchDefaults,
    ) -> Result<ProjectRecord> {
        match self
            .0
            .store
            .update_catalog(CatalogUpdate::CreateProject {
                name,
                directory,
                launch,
            })
            .await?
        {
            CatalogRecord::Project(record) => Ok(record),
            _ => unreachable!(),
        }
    }
    pub async fn project(&self, id: &str) -> Result<ProjectRecord> {
        self.local_catalog()
            .await?
            .projects
            .remove(id)
            .ok_or_else(|| Error::Invalid("project not found".into()))
    }
    pub async fn update_project(&self, update: ProjectUpdate) -> Result<ProjectRecord> {
        match self
            .0
            .store
            .update_catalog(CatalogUpdate::UpdateProject(update))
            .await?
        {
            CatalogRecord::Project(record) => Ok(record),
            _ => unreachable!(),
        }
    }
    pub async fn set_default_directory(
        &self,
        expected_revision: u64,
        directory: DirectoryRef,
    ) -> Result<DefaultDirectory> {
        match self
            .0
            .store
            .update_catalog(CatalogUpdate::SetDefault {
                expected_revision,
                directory,
            })
            .await?
        {
            CatalogRecord::Default(record) => Ok(record),
            _ => unreachable!(),
        }
    }
    pub async fn session_context(&self, session_id: &str) -> Result<ContextStatus> {
        let snapshot = self.snapshot(session_id).await?;
        let context = self.retained_context(&snapshot).await?;
        let reason = verify_context(&context).await.err().map(|e| e.to_string());
        Ok(ContextStatus {
            context,
            available: reason.is_none(),
            reason,
        })
    }
    pub(super) async fn retained_context(
        &self,
        snapshot: &SessionSnapshot,
    ) -> Result<WorkingContext> {
        let context = snapshot
            .working_context
            .as_ref()
            .ok_or_else(|| Error::Storage("session has no retained working context".into()))?;
        if self.0.store.working_context(&context.id).await?.as_ref() != Some(context)
            || context.session_id != snapshot.id
            || snapshot.options.cwd != context.directory.path
            || context.environment_id != self.local_catalog().await?.identity.environment_id
        {
            return Err(Error::Storage(
                "session working context differs from the host authority".into(),
            ));
        }
        Ok(context.clone())
    }
    pub async fn extension_state(&self, id: &str) -> Result<Option<Value>> {
        self.0.store.extension_state(id).await
    }
    pub async fn save_extension_state(&self, id: &str, value: Value) -> Result<()> {
        self.0.store.save_extension_state(id, value).await
    }
    /// Resolve current host defaults for a sessionless native operation.
    pub async fn resolve_launch_options(&self, mut options: SpawnOptions) -> Result<SpawnOptions> {
        let selection = self.resolve_new_selection(&mut options).await?;
        let context = self.local_catalog().await?.prepare(
            uuid::Uuid::new_v4().to_string(),
            uuid::Uuid::new_v4().to_string(),
            &selection,
        )?;
        let selected_project = options.project_id.clone();
        let selected_revision = options.project_revision;
        for extension in &self.0.extensions {
            extension.before_spawn(&mut options).await?;
        }
        if options.cwd != context.directory.path
            || options.project_id != selected_project
            || options.project_revision != selected_revision
        {
            return Err(Error::Invalid(
                "extension changed a retained working selection".into(),
            ));
        }
        verify_context(&context).await?;
        self.compile_mcp(&mut options).await?;
        Ok(options)
    }
    pub(super) async fn resolve_new_selection(
        &self,
        options: &mut SpawnOptions,
    ) -> Result<ContextSelection> {
        if let Some(id) = &options.project_id {
            let project = self.project(id).await?;
            if options
                .project_revision
                .is_some_and(|r| r != project.revision)
            {
                return Err(Error::Invalid(
                    "project revision changed; reload before launching".into(),
                ));
            }
            options.project_revision = Some(project.revision);
            if options.model.is_none() {
                options.model = project.launch.model.clone();
            }
            options.config = merge_project_config(project.launch.config, options.config.clone());
            let selection = ContextSelection::Project {
                id: id.clone(),
                expected_revision: options.project_revision,
            };
            let (directory, _) = self.local_catalog().await?.select(&selection)?;
            if !options.cwd.as_os_str().is_empty() {
                let (requested, fingerprint) = observe_directory(&options.cwd, false).await?;
                if requested != directory.path || fingerprint != directory.fingerprint {
                    return Err(Error::Invalid(
                        "cwd conflicts with the selected project directory".into(),
                    ));
                }
            }
            options.cwd = directory.path;
            return Ok(selection);
        }
        if options.project_revision.is_some() {
            return Err(Error::Invalid("projectRevision requires projectId".into()));
        }
        if options.cwd.as_os_str().is_empty() {
            let catalog = self.local_catalog().await?;
            let selection = ContextSelection::Default {
                expected_revision: catalog.default_directory.as_ref().map(|d| d.revision),
            };
            let (directory, _) = catalog.select(&selection)?;
            options.cwd = directory.path;
            Ok(selection)
        } else {
            let directory = self.register_directory(&options.cwd).await?;
            options.cwd = directory.path.clone();
            Ok(ContextSelection::Directory {
                directory: directory.reference(),
                native_existing: false,
            })
        }
    }
}
fn merge_project_config(mut base: Value, patch: Value) -> Value {
    if patch.is_null() {
        return base;
    }
    if let (Value::Object(target), Value::Object(source)) = (&mut base, &patch) {
        for (key, value) in source {
            let previous = target.remove(key).unwrap_or(Value::Null);
            target.insert(key.clone(), merge_project_config(previous, value.clone()));
        }
        base
    } else {
        patch
    }
}
