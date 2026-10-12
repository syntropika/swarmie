use super::*;
use serde_json::Value;

fn catalog(connection: &Connection, identity: LocalStoreIdentity) -> Result<LocalCatalog> {
    let mut catalog = LocalCatalog {
        identity,
        ..Default::default()
    };
    let mut directories = connection
        .prepare("SELECT record_json FROM directories")
        .map_err(database_error)?;
    for row in directories
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(database_error)?
    {
        let directory: DirectoryResource = serde_json::from_str(&row.map_err(database_error)?)?;
        catalog.directories.insert(directory.id.clone(), directory);
    }
    let mut projects = connection.prepare("SELECT r.record_json FROM project_revisions r JOIN project_heads h USING(project_id, revision)").map_err(database_error)?;
    for row in projects
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(database_error)?
    {
        let project: ProjectRecord = serde_json::from_str(&row.map_err(database_error)?)?;
        catalog.projects.insert(project.id.clone(), project);
    }
    let selected: Option<String> = connection.query_row("SELECT r.record_json FROM default_directory_revisions r JOIN default_directory_head h USING(revision)", [], |r| r.get(0)).optional().map_err(database_error)?;
    catalog.default_directory = selected
        .map(|json| serde_json::from_str(&json).map_err(Error::from))
        .transpose()?;
    Ok(catalog)
}
impl SqliteSessionStore {
    pub(crate) async fn catalog(&self) -> Result<LocalCatalog> {
        let identity = self.identity.clone();
        self.database(move |connection| {
            let transaction = connection.transaction().map_err(database_error)?;
            catalog(&transaction, identity)
        })
        .await
    }
    pub(crate) async fn mutate_catalog(&self, update: CatalogUpdate) -> Result<CatalogRecord> {
        let identity = self.identity.clone();
        self.database(move |connection| {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(database_error)?;
            let mut current = catalog(&transaction, identity)?;
            let record = current.apply(update)?;
            match &record {
                CatalogRecord::Directory(d) => {
                    transaction.execute("INSERT INTO directories VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
                        params![d.id, integer(d.revision)?, d.environment_id, serde_json::to_string(d)?]).map_err(database_error)?;
                }
                CatalogRecord::Project(p) => {
                    transaction.execute("INSERT INTO project_revisions VALUES (?1, ?2, ?3, ?4, ?5)", params![p.id, integer(p.revision)?,
                        p.directory.as_ref().map(|d| &d.id), p.directory.as_ref().map(|d| integer(d.revision)).transpose()?, serde_json::to_string(p)?]).map_err(database_error)?;
                    transaction.execute("INSERT INTO project_heads VALUES (?1, ?2) ON CONFLICT(project_id) DO UPDATE SET revision=excluded.revision", params![p.id, integer(p.revision)?]).map_err(database_error)?;
                }
                CatalogRecord::Default(d) => {
                    transaction.execute("INSERT INTO default_directory_revisions VALUES (?1, ?2, ?3, ?4, ?5)", params![integer(d.revision)?,d.environment_id,d.directory.id,integer(d.directory.revision)?,serde_json::to_string(d)?]).map_err(database_error)?;
                    transaction.execute("INSERT INTO default_directory_head VALUES (1, ?1) ON CONFLICT(singleton) DO UPDATE SET revision=excluded.revision", [integer(d.revision)?]).map_err(database_error)?;
                }
            }
            transaction.commit().map_err(database_error)?;
            Ok(record)
        }).await
    }
    pub(crate) async fn prepare(
        &self,
        session_id: &str,
        binding_id: &str,
        selection: ContextSelection,
    ) -> Result<WorkingContext> {
        validate_id(session_id)?;
        validate_id(binding_id)?;
        let session_id = session_id.to_owned();
        let binding_id = binding_id.to_owned();
        let identity = self.identity.clone();
        self.database(move |connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(database_error)?;
            let current = catalog(&transaction, identity)?;
            let context = if let ContextSelection::Inherited { context_id } = &selection {
                let json: String = transaction
                    .query_row(
                        "SELECT record_json FROM working_contexts WHERE context_id=?1",
                        [context_id],
                        |r| r.get(0),
                    )
                    .map_err(database_error)?;
                let parent: WorkingContext = serde_json::from_str(&json)?;
                WorkingContext {
                    id: uuid::Uuid::new_v4().to_string(),
                    session_id: session_id.clone(),
                    binding_id,
                    ..parent
                }
            } else {
                current.prepare(session_id.clone(), binding_id, &selection)?
            };
            let (project_id, project_revision, default_revision) = match &context.source {
                ContextSource::ProjectFolder {
                    project_id,
                    revision,
                } => (Some(project_id), Some(integer(*revision)?), None),
                ContextSource::ProjectFreeDefault { revision } => {
                    (None, None, Some(integer(*revision)?))
                }
                _ => (None, None, None),
            };
            transaction
                .execute(
                    "INSERT INTO sessions(session_id) VALUES (?1) ON CONFLICT DO NOTHING",
                    [&session_id],
                )
                .map_err(database_error)?;
            transaction
                .execute(
                    "INSERT INTO working_contexts VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        context.id,
                        context.session_id,
                        context.binding_id,
                        context.environment_id,
                        context.directory.id,
                        integer(context.directory.revision)?,
                        project_id,
                        project_revision,
                        default_revision,
                        serde_json::to_string(&context)?
                    ],
                )
                .map_err(database_error)?;
            transaction.commit().map_err(database_error)?;
            Ok(context)
        })
        .await
    }
    pub(crate) async fn context(&self, id: &str) -> Result<Option<WorkingContext>> {
        let id = id.to_owned();
        self.database(move |connection| {
            let json: Option<String> = connection
                .query_row(
                    "SELECT record_json FROM working_contexts WHERE context_id=?1",
                    [id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(database_error)?;
            json.map(|json| serde_json::from_str(&json).map_err(Error::from))
                .transpose()
        })
        .await
    }
    pub(crate) async fn read_extension(&self, id: &str) -> Result<Option<Value>> {
        let id = id.to_owned();
        self.database(move |connection| {
            let json: Option<String> = connection
                .query_row(
                    "SELECT record_json FROM extension_state WHERE extension_id=?1",
                    [id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(database_error)?;
            json.map(|json| serde_json::from_str(&json).map_err(Error::from))
                .transpose()
        })
        .await
    }
    pub(crate) async fn write_extension(&self, id: &str, value: Value) -> Result<()> {
        if id.is_empty() || id.len() > 128 {
            return Err(Error::Invalid("invalid extension namespace".into()));
        }
        let id = id.to_owned();
        let json = serde_json::to_string(&value)?;
        if json.len() > 64 * 1024 * 1024 {
            return Err(Error::Invalid("extension state exceeds 64 MiB".into()));
        }
        self.database(move |connection| {
            connection.execute("INSERT INTO extension_state VALUES (?1, ?2) ON CONFLICT(extension_id) DO UPDATE SET record_json=excluded.record_json", params![id,json]).map_err(database_error)?;
            Ok(())
        }).await
    }
}
