use async_trait::async_trait;
use swarmie::{
    mock::{MockHarness, MockScript},
    *,
};
use serde_json::json;
use std::path::Path;
use storage::SqliteSessionStore;

struct Resumable(MockHarness);
#[async_trait]
impl Harness for Resumable {
    fn id(&self) -> &str {
        "mock"
    }
    async fn detect(&self) -> Result<Detection> {
        self.0.detect().await
    }
    async fn connect(
        &self,
        options: &SpawnOptions,
        native: Option<&NativeSessionRef>,
        fork: bool,
    ) -> Result<BackendConnection> {
        let mut connection = self.0.connect(options, None, false).await?;
        connection.capabilities.resume = true.into();
        connection.capabilities.fork = true.into();
        if !fork && let Some(native) = native {
            connection.native = native.clone();
        }
        Ok(connection)
    }
}
async fn host(root: &Path, mock: &MockHarness) -> Runtime {
    Runtime::builder()
        .store(SqliteSessionStore::new(root).await.unwrap())
        .harness(Resumable(mock.clone()))
        .build()
}
async fn project(runtime: &Runtime, path: &Path) -> ProjectRecord {
    let directory = runtime.register_directory(path).await.unwrap();
    runtime
        .create_project(
            "Example".into(),
            Some(directory.reference()),
            ProjectLaunchDefaults::default(),
        )
        .await
        .unwrap()
}
fn launch(project: &ProjectRecord) -> SpawnOptions {
    let mut options = SpawnOptions::new("mock", "");
    options.project_id = Some(project.id.clone());
    options.project_revision = Some(project.revision);
    options
}
fn update(project: &ProjectRecord) -> ProjectUpdate {
    ProjectUpdate {
        project_id: project.id.clone(),
        expected_revision: project.revision,
        name: None,
        archived: None,
        directory: None,
        unlink_directory: false,
        launch: None,
    }
}

#[tokio::test]
async fn sessions_share_a_folder_and_retain_selections_across_relink_restart_and_fork() {
    let root = tempfile::tempdir().unwrap();
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let mock = MockHarness::new(MockScript::default());
    let runtime = host(root.path(), &mock).await;
    let p = project(&runtime, first.path()).await;
    let a = runtime.spawn(launch(&p)).await.unwrap();
    let b = runtime.spawn(launch(&p)).await.unwrap();
    let saved = a.snapshot().await;
    let original = saved.working_context.clone().unwrap();
    let other = b.snapshot().await.working_context.unwrap();
    assert_eq!(original.directory, other.directory);
    assert_ne!(original.id, other.id);
    assert_ne!(original.binding_id, other.binding_id);
    let directory = runtime.register_directory(second.path()).await.unwrap();
    let mut change = update(&p);
    change.directory = Some(directory.reference());
    let p2 = runtime.update_project(change).await.unwrap();
    assert!(runtime.spawn(launch(&p)).await.is_err());
    assert_eq!(mock.launches().await.len(), 2);
    assert_eq!(
        runtime
            .spawn(launch(&p2))
            .await
            .unwrap()
            .snapshot()
            .await
            .options
            .cwd,
        second.path().canonicalize().unwrap()
    );
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let runtime = host(root.path(), &mock).await;
    let resumed = runtime.resume(&saved.id).await.unwrap();
    assert_eq!(
        resumed.snapshot().await.working_context.as_ref(),
        Some(&original)
    );
    let fork = runtime.fork(&saved.id).await.unwrap().snapshot().await;
    let inherited = fork.working_context.unwrap();
    assert_eq!(inherited.directory, original.directory);
    assert_eq!(inherited.source, original.source);
    assert_ne!(inherited.binding_id, original.binding_id);
    assert_eq!(fork.options.cwd, first.path().canonicalize().unwrap());
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn default_changes_only_select_new_contexts_and_stale_preparations_fail() {
    let root = tempfile::tempdir().unwrap();
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let mock = MockHarness::new(MockScript::default());
    let runtime = host(root.path(), &mock).await;
    assert!(runtime.spawn(SpawnOptions::new("mock", "")).await.is_err());
    let directory = runtime.register_directory(first.path()).await.unwrap();
    runtime
        .set_default_directory(0, directory.reference())
        .await
        .unwrap();
    let saved = runtime
        .spawn(SpawnOptions::new("mock", ""))
        .await
        .unwrap()
        .snapshot()
        .await;
    let directory = runtime.register_directory(second.path()).await.unwrap();
    runtime
        .set_default_directory(1, directory.reference())
        .await
        .unwrap();
    let store = SqliteSessionStore::new(root.path()).await.unwrap();
    assert!(
        store
            .prepare_context(
                &uuid::Uuid::new_v4().to_string(),
                &uuid::Uuid::new_v4().to_string(),
                ContextSelection::Default {
                    expected_revision: Some(1)
                }
            )
            .await
            .is_err()
    );
    assert_eq!(
        runtime
            .spawn(SpawnOptions::new("mock", ""))
            .await
            .unwrap()
            .snapshot()
            .await
            .options
            .cwd,
        second.path().canonicalize().unwrap()
    );
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let runtime = host(root.path(), &mock).await;
    assert_eq!(
        runtime
            .resume(&saved.id)
            .await
            .unwrap()
            .snapshot()
            .await
            .working_context,
        saved.working_context
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_and_replaced_folders_block_dispatch_and_resume_without_losing_history() {
    let root = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let folder = parent.path().join("folder");
    std::fs::create_dir(&folder).unwrap();
    let mock = MockHarness::new(MockScript::default());
    let runtime = host(root.path(), &mock).await;
    let p = project(&runtime, &folder).await;
    let agent = runtime.spawn(launch(&p)).await.unwrap();
    let saved = agent.snapshot().await;
    std::fs::rename(&folder, parent.path().join("original")).unwrap();
    assert!(!runtime.session_context(&saved.id).await.unwrap().available);
    assert!(agent.prompt("blocked").await.is_err());
    assert!(runtime.spawn(launch(&p)).await.is_err());
    assert!(mock.prompts().await.is_empty());
    assert_eq!(mock.launches().await.len(), 1);
    std::fs::create_dir(&folder).unwrap();
    assert!(!runtime.session_context(&saved.id).await.unwrap().available);
    assert!(agent.prompt("still blocked").await.is_err());
    assert_eq!(
        runtime.snapshot(&saved.id).await.unwrap().working_context,
        saved.working_context
    );
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let runtime = host(root.path(), &mock).await;
    assert!(runtime.resume(&saved.id).await.is_err());
    assert_eq!(mock.launches().await.len(), 1);
    assert_eq!(runtime.sessions().await.unwrap().len(), 1);
    runtime.transcript(&saved.id, 0, 100).await.unwrap();
    assert!(!folder.join(".swarmie").exists());
    runtime.shutdown().await.unwrap();
}

struct Redirect(std::path::PathBuf);
#[async_trait]
impl RuntimeExtension for Redirect {
    fn id(&self) -> &str {
        "redirect"
    }
    async fn before_spawn(&self, options: &mut SpawnOptions) -> Result<()> {
        options.cwd = self.0.clone();
        Ok(())
    }
}
#[tokio::test]
async fn extension_redirection_cross_node_refs_and_invalid_defaults_fail_before_connect() {
    let root = tempfile::tempdir().unwrap();
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let mock = MockHarness::new(MockScript::default());
    let runtime = Runtime::builder()
        .store(SqliteSessionStore::new(root.path()).await.unwrap())
        .harness(mock.clone())
        .extension(Redirect(second.path().into()))
        .build();
    let p = project(&runtime, first.path()).await;
    assert!(runtime.spawn(launch(&p)).await.is_err());
    assert!(runtime.preview_configuration(launch(&p)).await.is_err());
    let foreign = Runtime::default()
        .register_directory(second.path())
        .await
        .unwrap();
    assert!(
        runtime
            .set_default_directory(0, foreign.reference())
            .await
            .is_err()
    );
    assert!(
        runtime
            .create_project(
                "Invalid".into(),
                None,
                ProjectLaunchDefaults {
                    config: json!("scalar"),
                    ..Default::default()
                }
            )
            .await
            .is_err()
    );
    assert!(mock.launches().await.is_empty());
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn unlink_and_archive_preserve_user_files_and_existing_execution_selection() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("owned.txt"), "user content").unwrap();
    let mock = MockHarness::new(MockScript::default());
    let runtime = host(root.path(), &mock).await;
    let p = project(&runtime, folder.path()).await;
    let saved = runtime.spawn(launch(&p)).await.unwrap().snapshot().await;
    let mut change = update(&p);
    change.unlink_directory = true;
    change.archived = Some(true);
    let p2 = runtime.update_project(change).await.unwrap();
    assert!(runtime.spawn(launch(&p2)).await.is_err());
    assert_eq!(
        std::fs::read_to_string(folder.path().join("owned.txt")).unwrap(),
        "user content"
    );
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let runtime = host(root.path(), &mock).await;
    assert_eq!(
        runtime
            .resume(&saved.id)
            .await
            .unwrap()
            .snapshot()
            .await
            .working_context,
        saved.working_context
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn retained_context_cannot_be_removed_or_rewritten_by_snapshot_updates() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let mock = MockHarness::new(MockScript::default());
    let runtime = host(root.path(), &mock).await;
    let saved = runtime
        .spawn(SpawnOptions::new("mock", folder.path()))
        .await
        .unwrap()
        .snapshot()
        .await;
    let store = SqliteSessionStore::new(root.path()).await.unwrap();
    let mut changed = saved.clone();
    changed.working_context = None;
    assert!(store.save(&changed).await.is_err());
    changed = saved.clone();
    changed.working_context.as_mut().unwrap().binding_id = uuid::Uuid::new_v4().to_string();
    assert!(store.save(&changed).await.is_err());
    assert_eq!(
        store
            .load(&saved.id)
            .await
            .unwrap()
            .unwrap()
            .working_context,
        saved.working_context
    );
    runtime.shutdown().await.unwrap();
}
