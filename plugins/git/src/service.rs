//! Node-owned persistence and typed product operations over the independent core.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    ArchiveDirectory, ArchiveReceipt, ArchiveRestoreRequest, Cancellation, CleanupEligibility,
    CreatedWorktree, Error, Git, GitSettings, Intent, Outcome, Receipt, RepositoryTarget, Request,
    Response, Result, SetupOutcome, WorktreeRequest,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WireRequest {
    Core(Request),
    Managed(ManagedRequest),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ManagedRequest {
    SettingsGet {
        target: RepositoryTarget,
        project_id: Option<String>,
    },
    SettingsSet {
        operation_id: String,
        target: RepositoryTarget,
        project_id: Option<String>,
        settings: GitSettings,
    },
    WorktreeCreate {
        operation_id: String,
        target: RepositoryTarget,
        project_id: Option<String>,
        request: WorktreeRequest,
    },
    SetupTest {
        operation_id: String,
        target: RepositoryTarget,
        project_id: Option<String>,
    },
    ArchiveCreate {
        operation_id: String,
        target: RepositoryTarget,
        archive_id: String,
    },
    ArchiveRestore {
        operation_id: String,
        target: RepositoryTarget,
        archive_id: String,
        managed_root: PathBuf,
        destination: PathBuf,
    },
    ArchiveList {
        target: RepositoryTarget,
    },
    CleanupCheck {
        target: RepositoryTarget,
        archive_id: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WireResponse {
    Core(Box<Response>),
    Managed(ManagedResponse),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ManagedResponse {
    Settings {
        settings: GitSettings,
        execution_platform: String,
    },
    Worktree {
        worktree: Box<CreatedWorktree>,
        receipt: Box<Receipt>,
    },
    Setup {
        outcome: SetupOutcome,
        receipt: Box<Receipt>,
    },
    Archive {
        archive: Box<ArchiveReceipt>,
        receipt: Box<Receipt>,
    },
    Archives {
        archives: Vec<ArchiveReceipt>,
    },
    Cleanup {
        eligibility: CleanupEligibility,
    },
}

/// Host-owned directories are private and must remain outside user checkouts.
#[derive(Clone)]
pub struct GitService {
    git: Git,
    directory: Arc<PathBuf>,
    archives: Arc<ArchiveDirectory>,
}

impl GitService {
    pub fn new(git: Git, directory: impl Into<PathBuf>) -> Result<Self> {
        let directory = directory.into();
        fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        let directory = fs::canonicalize(directory)?;
        fs::create_dir_all(directory.join("settings"))?;
        fs::create_dir_all(directory.join("worktrees"))?;
        let archives = Arc::new(ArchiveDirectory::open(directory.join("archives"))?);
        Ok(Self {
            git,
            directory: Arc::new(directory),
            archives,
        })
    }

    pub fn git(&self) -> &Git {
        &self.git
    }

    /// Synchronous dispatch: embedding hosts must offload and settle this call.
    pub fn execute(
        &self,
        principal: &str,
        request: WireRequest,
        cancel: &Cancellation,
    ) -> Result<WireResponse> {
        match request {
            WireRequest::Core(request) => self
                .git
                .execute(principal, request, cancel)
                .map(|response| WireResponse::Core(Box::new(response))),
            WireRequest::Managed(request) => {
                let result = self.managed(principal, request.clone(), cancel);
                match result {
                    Err(error) => {
                        let binding = match &request {
                            ManagedRequest::SettingsSet {
                                operation_id,
                                target,
                                ..
                            }
                            | ManagedRequest::WorktreeCreate {
                                operation_id,
                                target,
                                ..
                            }
                            | ManagedRequest::SetupTest {
                                operation_id,
                                target,
                                ..
                            }
                            | ManagedRequest::ArchiveCreate {
                                operation_id,
                                target,
                                ..
                            }
                            | ManagedRequest::ArchiveRestore {
                                operation_id,
                                target,
                                ..
                            } => Some((operation_id, target)),
                            _ => None,
                        };
                        if let Some((operation_id, target)) = binding {
                            let operation = crate::Operation::Extension {
                                name: "managed_rejected".into(),
                                arguments: json(&request)?,
                            };
                            let receipt = self.git.reject_before_admission(
                                principal,
                                operation_id.clone(),
                                target.clone(),
                                operation,
                                error,
                            )?;
                            Ok(WireResponse::Core(Box::new(Response::Receipt {
                                receipt: Box::new(receipt),
                            })))
                        } else {
                            Err(error)
                        }
                    }
                    result => result,
                }
            }
        }
    }

    fn authorize(&self, principal: &str, target: &RepositoryTarget, operation: &str) -> Result<()> {
        if principal.is_empty() {
            return Err(Error::Denied(
                "an authenticated principal is required".into(),
            ));
        }
        if Path::new(&target.common_dir).starts_with(self.directory.as_ref())
            || self
                .directory
                .starts_with(target.root.as_deref().unwrap_or(&target.git_dir))
        {
            return Err(Error::Denied(
                "capability persistence must remain outside the checkout".into(),
            ));
        }
        self.git.authority.authorize(
            principal,
            &Intent {
                target: target.clone(),
                operation: operation.into(),
            },
        )
    }

    fn managed(
        &self,
        principal: &str,
        request: ManagedRequest,
        cancel: &Cancellation,
    ) -> Result<WireResponse> {
        let platform = std::env::consts::OS.to_owned();
        let target = match &request {
            ManagedRequest::SettingsGet { target, .. }
            | ManagedRequest::SettingsSet { target, .. }
            | ManagedRequest::WorktreeCreate { target, .. }
            | ManagedRequest::SetupTest { target, .. }
            | ManagedRequest::ArchiveCreate { target, .. }
            | ManagedRequest::ArchiveRestore { target, .. }
            | ManagedRequest::ArchiveList { target }
            | ManagedRequest::CleanupCheck { target, .. } => target,
        };
        self.authorize(
            principal,
            target,
            &serde_json::to_string(&request).map_err(invalid_json)?,
        )?;
        if matches!(&request, ManagedRequest::SettingsGet { .. }) {
            self.git.verify_identity(target, cancel)?;
        } else {
            self.git.verify(target, cancel)?;
        }
        match request {
            ManagedRequest::SettingsGet { target, project_id } => {
                self.authorize(principal, &target, "settings_read")?;
                let mut settings = self.settings(principal, &target, project_id.as_deref())?;
                settings.setup_by_platform.retain(|key, _| key == &platform);
                Ok(WireResponse::Managed(ManagedResponse::Settings {
                    settings,
                    execution_platform: platform,
                }))
            }
            ManagedRequest::SettingsSet {
                operation_id,
                target,
                project_id,
                settings,
            } => {
                self.authorize(principal, &target, "settings_write")?;
                validate_settings(&settings, &platform)?;
                let arguments = json(&(project_id.clone(), &settings))?;
                let receipt = self.git.execute_extension(
                    principal,
                    crate::ExtensionOperation {
                        operation_id,
                        target: target.clone(),
                        name: "settings_set".into(),
                        arguments,
                    },
                    cancel,
                    || {
                        let mut settings = settings;
                        let saved = self.settings(principal, &target, project_id.as_deref())?;
                        let current = settings.setup_by_platform.remove(&platform);
                        settings.setup_by_platform = saved.setup_by_platform;
                        if let Some(script) = current {
                            settings.setup_by_platform.insert(platform.clone(), script);
                        }
                        self.save_settings(principal, &target, project_id.as_deref(), &settings)?;
                        settings.setup_by_platform.retain(|key, _| key == &platform);
                        json(&ManagedResponse::Settings {
                            settings,
                            execution_platform: platform,
                        })
                    },
                )?;
                if let Some(value) = completed(&receipt) {
                    return serde_json::from_value(value)
                        .map(WireResponse::Managed)
                        .map_err(invalid_json);
                }
                Ok(WireResponse::Core(Box::new(Response::Receipt {
                    receipt: Box::new(receipt),
                })))
            }
            ManagedRequest::WorktreeCreate {
                operation_id,
                target,
                project_id,
                request,
            } => {
                self.authorize(principal, &target, "create_worktree")?;
                if self.git.journal.load(&operation_id)?.is_none() {
                    crate::worktrees::validate_destination(
                        &request.managed_root,
                        &request.destination,
                    )?;
                }
                if request.commit.is_empty()
                    || request.commit.starts_with('-')
                    || request.commit.contains('\0')
                {
                    return Err(Error::Invalid("choose a valid starting commit".into()));
                }
                for path in &request.include_paths {
                    crate::repository::relative_path(path)?;
                }
                let settings = self.settings(principal, &target, project_id.as_deref())?;
                let script = settings.setup_for(&platform).cloned();
                let arguments = json(&(project_id, &request, &script))?;
                let receipt = self.git.execute_extension(
                    principal,
                    crate::ExtensionOperation {
                        operation_id,
                        target: target.clone(),
                        name: "worktree_create".into(),
                        arguments,
                    },
                    cancel,
                    || {
                        let mut worktree = self
                            .git
                            .create_worktree(principal, &target, &request, cancel)?;
                        if worktree.preparation_error.is_none()
                            && let Some(script) = script
                        {
                            match self
                                .git
                                .run_setup(principal, &worktree.target, &script, cancel)
                            {
                                Ok(outcome) if outcome.status == 0 => {}
                                Ok(outcome) => {
                                    worktree.preparation_error = Some(format!(
                                        "Setup failed ({}): {}",
                                        outcome.status, outcome.stderr
                                    ))
                                }
                                Err(error) => worktree.preparation_error = Some(error.to_string()),
                            }
                            if let Some(current) = self.git.discover(
                                Path::new(
                                    worktree
                                        .target
                                        .root
                                        .as_deref()
                                        .unwrap_or(&worktree.target.git_dir),
                                ),
                                &Cancellation::default(),
                            )? {
                                worktree.target = current;
                            }
                        }
                        json(&worktree)
                    },
                )?;
                match completed(&receipt) {
                    Some(value) => Ok(WireResponse::Managed(ManagedResponse::Worktree {
                        worktree: serde_json::from_value(value).map_err(invalid_json)?,
                        receipt: Box::new(receipt),
                    })),
                    None => Ok(WireResponse::Core(Box::new(Response::Receipt {
                        receipt: Box::new(receipt),
                    }))),
                }
            }
            ManagedRequest::SetupTest {
                operation_id,
                target,
                project_id,
            } => {
                self.authorize(principal, &target, "setup")?;
                let settings = self.settings(principal, &target, project_id.as_deref())?;
                let script = settings.setup_for(&platform).cloned().ok_or_else(|| {
                    Error::Invalid("save and enable this node's setup script before testing".into())
                })?;
                let receipt = self.git.execute_extension(
                    principal,
                    crate::ExtensionOperation {
                        operation_id,
                        target: target.clone(),
                        name: "setup_test".into(),
                        arguments: json(&script)?,
                    },
                    cancel,
                    || json(&self.git.run_setup(principal, &target, &script, cancel)?),
                )?;
                match completed(&receipt) {
                    Some(value) => Ok(WireResponse::Managed(ManagedResponse::Setup {
                        outcome: serde_json::from_value(value).map_err(invalid_json)?,
                        receipt: Box::new(receipt),
                    })),
                    None => Ok(WireResponse::Core(Box::new(Response::Receipt {
                        receipt: Box::new(receipt),
                    }))),
                }
            }
            ManagedRequest::ArchiveCreate {
                operation_id,
                target,
                archive_id,
            } => {
                self.authorize(principal, &target, "archive_protected_content")?;
                let receipt = self.git.execute_extension(
                    principal,
                    crate::ExtensionOperation {
                        operation_id,
                        target: target.clone(),
                        name: "archive_create".into(),
                        arguments: json(&archive_id)?,
                    },
                    cancel,
                    || {
                        json(&self.archives.capture(
                            &self.git,
                            principal,
                            &archive_id,
                            &target,
                            cancel,
                        )?)
                    },
                )?;
                archive_response(receipt)
            }
            ManagedRequest::ArchiveRestore {
                operation_id,
                target,
                archive_id,
                managed_root,
                destination,
            } => {
                self.authorize(principal, &target, "restore_archive")?;
                let arguments = json(&(&archive_id, &managed_root, &destination))?;
                let receipt = self.git.execute_extension(
                    principal,
                    crate::ExtensionOperation {
                        operation_id,
                        target: target.clone(),
                        name: "archive_restore".into(),
                        arguments,
                    },
                    cancel,
                    || {
                        json(&self.archives.restore(
                            &self.git,
                            principal,
                            &target,
                            &ArchiveRestoreRequest {
                                archive_id,
                                managed_root,
                                destination,
                            },
                            cancel,
                        )?)
                    },
                )?;
                archive_response(receipt)
            }
            ManagedRequest::ArchiveList { target } => {
                self.authorize(principal, &target, "archive_read")?;
                let mut archives = self.archives.list(&self.git, principal, &target, cancel)?;
                archives.truncate(32);
                Ok(WireResponse::Managed(ManagedResponse::Archives {
                    archives,
                }))
            }
            ManagedRequest::CleanupCheck { target, archive_id } => {
                self.authorize(principal, &target, "inspect_cleanup")?;
                let eligibility = self.archives.cleanup_eligibility(
                    &self.git,
                    principal,
                    &archive_id,
                    &target,
                    cancel,
                )?;
                Ok(WireResponse::Managed(ManagedResponse::Cleanup {
                    eligibility,
                }))
            }
        }
    }

    fn settings_path(
        &self,
        principal: &str,
        target: &RepositoryTarget,
        project: Option<&str>,
    ) -> PathBuf {
        let binding = serde_json::to_vec(&(principal, &target.common_dir, project))
            .expect("string binding serialization");
        self.directory
            .join("settings")
            .join(format!("{:x}.json", Sha256::digest(binding)))
    }

    fn settings(
        &self,
        principal: &str,
        target: &RepositoryTarget,
        project: Option<&str>,
    ) -> Result<GitSettings> {
        match fs::read(self.settings_path(principal, target, project)) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(invalid_json),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(GitSettings {
                managed_root: Some(self.directory.join("worktrees")),
                ..GitSettings::default()
            }),
            Err(error) => Err(error.into()),
        }
    }

    fn save_settings(
        &self,
        principal: &str,
        target: &RepositoryTarget,
        project: Option<&str>,
        settings: &GitSettings,
    ) -> Result<()> {
        let path = self.settings_path(principal, target, project);
        static SETTINGS_SEQUENCE: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(0);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| Error::Journal(error.to_string()))?
            .as_nanos();
        let sequence = SETTINGS_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temporary =
            path.with_extension(format!("{nonce}-{}-{sequence}.pending", std::process::id()));
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec(settings).map_err(invalid_json)?)?;
        file.sync_all()?;
        fs::rename(&temporary, &path)?;
        #[cfg(unix)]
        fs::File::open(
            path.parent()
                .ok_or_else(|| Error::Invalid("missing settings directory".into()))?,
        )?
        .sync_all()?;
        Ok(())
    }
}

fn validate_settings(settings: &GitSettings, platform: &str) -> Result<()> {
    if settings.setup_by_platform.keys().any(|key| key != platform) {
        return Err(Error::Invalid(
            "only the current execution system can be configured".into(),
        ));
    }
    if settings.commit_guidance.len() > 4096 || settings.branch_prefix.len() > 128 {
        return Err(Error::Invalid(
            "Git preferences exceed their length limits".into(),
        ));
    }
    if let Some(script) = settings.setup_by_platform.get(platform) {
        crate::worktrees::validate_relative(&script.working_directory)?;
        if script.script.len() > 16_384
            || script.arguments.len() > 16
            || script.shell.as_os_str().is_empty()
        {
            return Err(Error::Invalid("invalid setup script configuration".into()));
        }
    }
    Ok(())
}

fn completed(receipt: &Receipt) -> Option<Value> {
    if receipt.state == Outcome::Completed {
        receipt.result.clone()
    } else {
        None
    }
}

fn archive_response(receipt: Receipt) -> Result<WireResponse> {
    match completed(&receipt) {
        Some(value) => Ok(WireResponse::Managed(ManagedResponse::Archive {
            archive: serde_json::from_value(value).map_err(invalid_json)?,
            receipt: Box::new(receipt),
        })),
        None => Ok(WireResponse::Core(Box::new(Response::Receipt {
            receipt: Box::new(receipt),
        }))),
    }
}

fn json(value: &impl Serialize) -> Result<Value> {
    serde_json::to_value(value).map_err(invalid_json)
}
fn invalid_json(error: serde_json::Error) -> Error {
    Error::Invalid(error.to_string())
}
