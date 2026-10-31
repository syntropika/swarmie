use crate::node::PRINCIPAL;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use swarmie_git::{Authority, Error, Intent, IntentOperation, ManagedRequest, Operation};

#[derive(Default)]
pub(crate) struct DirectoryGrants(Mutex<Vec<PathBuf>>, Mutex<Option<PathBuf>>);

impl DirectoryGrants {
    pub(crate) fn grant(&self, path: &Path) -> std::io::Result<()> {
        let path = path.canonicalize()?;
        if !path.is_dir() {
            return Err(std::io::Error::other("select a directory"));
        }
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("directory grants unavailable"))?
            .push(path);
        Ok(())
    }

    fn permits(&self, path: &Path) -> bool {
        let Ok(path) = path.canonicalize() else {
            return false;
        };
        self.0
            .lock()
            .is_ok_and(|roots| roots.iter().any(|root| path.starts_with(root)))
    }

    fn managed_destination(&self, root: &str, destination: &str) -> swarmie_git::Result<()> {
        let expected = self
            .1
            .lock()
            .map_err(|_| Error::Denied("managed directory unavailable".into()))?;
        let requested = Path::new(root).canonicalize()?;
        let parent = Path::new(destination)
            .parent()
            .ok_or_else(|| Error::Denied("missing destination directory".into()))?
            .canonicalize()?;
        if expected.as_ref() == Some(&requested) && parent.starts_with(&requested) {
            Ok(())
        } else {
            Err(Error::Denied(
                "managed worktrees require the host-owned worktree directory".into(),
            ))
        }
    }

    fn authorize_operation(&self, operation: &IntentOperation) -> swarmie_git::Result<()> {
        match operation {
            IntentOperation::CreateWorktree { request } => self.managed_destination(
                &request.managed_root.to_string_lossy(),
                &request.destination.to_string_lossy(),
            ),
            IntentOperation::ArchiveRestore {
                managed_root,
                destination,
                ..
            } => self.managed_destination(
                &managed_root.to_string_lossy(),
                &destination.to_string_lossy(),
            ),
            IntentOperation::Managed { request } => match request.as_ref() {
                ManagedRequest::WorktreeCreate { request, .. } => self.managed_destination(
                    &request.managed_root.to_string_lossy(),
                    &request.destination.to_string_lossy(),
                ),
                ManagedRequest::ArchiveRestore {
                    managed_root,
                    destination,
                    ..
                } => self.managed_destination(
                    &managed_root.to_string_lossy(),
                    &destination.to_string_lossy(),
                ),
                ManagedRequest::CapabilityInfo
                | ManagedRequest::SettingsGet { .. }
                | ManagedRequest::SettingsSet { .. }
                | ManagedRequest::ArchiveCreate { .. }
                | ManagedRequest::ArchiveList { .. }
                | ManagedRequest::CleanupCheck { .. } => Ok(()),
            },
            IntentOperation::Mutate { operation } => match operation {
                Operation::Abandoned | Operation::Extension { .. } => Err(Error::Denied(
                    "extension writes require their typed facade".into(),
                )),
                Operation::Stage { .. }
                | Operation::Unstage { .. }
                | Operation::Commit { .. }
                | Operation::CreateBranch { .. }
                | Operation::Fetch { .. }
                | Operation::Push { .. } => Ok(()),
            },
            IntentOperation::Discover
            | IntentOperation::Identify
            | IntentOperation::Reconcile { .. }
            | IntentOperation::CatalogEntry { .. }
            | IntentOperation::Status
            | IntentOperation::Diff { .. }
            | IntentOperation::History { .. }
            | IntentOperation::Remotes
            | IntentOperation::Files
            | IntentOperation::File { .. }
            | IntentOperation::Recover { .. }
            | IntentOperation::Operations
            | IntentOperation::Setup { .. }
            | IntentOperation::ArchiveCapture { .. }
            | IntentOperation::CleanupCheck { .. }
            | IntentOperation::ArchiveList
            | IntentOperation::ConversationHold { .. } => Ok(()),
        }
    }
}

impl Authority for DirectoryGrants {
    fn authorize_discovery(&self, principal: &str, path: &Path) -> swarmie_git::Result<()> {
        if principal == PRINCIPAL && self.permits(path) {
            Ok(())
        } else {
            Err(Error::Denied(
                "select this repository with the native directory picker first".into(),
            ))
        }
    }

    fn authorize(&self, principal: &str, intent: &Intent) -> swarmie_git::Result<()> {
        let path = intent
            .target
            .root
            .as_deref()
            .unwrap_or(&intent.target.git_dir);
        self.authorize_discovery(principal, Path::new(path))?;
        self.authorize_operation(&intent.operation)
    }
}

pub(crate) fn service(
    state: &Path,
    grants: Arc<DirectoryGrants>,
) -> Result<swarmie_git::GitService, String> {
    let journal = Arc::new(
        swarmie_git::DirectoryJournal::open(state.join("git-operations"))
            .map_err(|error| error.to_string())?,
    );
    let git = swarmie_git::Git::new(
        Arc::new(swarmie_git::NativeProcess),
        journal,
        grants.clone(),
    );
    let service =
        swarmie_git::GitService::new(git, state.join("git")).map_err(|error| error.to_string())?;
    let managed = state.join("git").join("worktrees");
    std::fs::create_dir_all(&managed).map_err(|error| error.to_string())?;
    grants.grant(&managed).map_err(|error| error.to_string())?;
    *grants
        .1
        .lock()
        .map_err(|_| "managed directory unavailable")? =
        Some(managed.canonicalize().map_err(|error| error.to_string())?);
    Ok(service)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_selection_grants_only_its_canonical_directory_tree() {
        let selected = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let grants = DirectoryGrants::default();
        grants.grant(selected.path()).unwrap();
        assert!(
            grants
                .authorize_discovery(PRINCIPAL, selected.path())
                .is_ok()
        );
        assert!(
            grants
                .authorize_discovery("other", selected.path())
                .is_err()
        );
        assert!(
            grants
                .authorize_discovery(PRINCIPAL, outside.path())
                .is_err()
        );
        #[cfg(unix)]
        {
            let link = selected.path().join("outside");
            std::os::unix::fs::symlink(outside.path(), &link).unwrap();
            assert!(grants.authorize_discovery(PRINCIPAL, &link).is_err());
        }
    }

    #[test]
    fn managed_worktree_destination_cannot_expand_a_selected_source_grant() {
        let selected = tempfile::tempdir().unwrap();
        let managed = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let grants = DirectoryGrants::default();
        grants.grant(selected.path()).unwrap();
        *grants.1.lock().unwrap() = Some(managed.path().canonicalize().unwrap());
        let intent = |root: &Path, destination: &Path| IntentOperation::ArchiveRestore {
            archive_id: "fixture".into(),
            managed_root: root.into(),
            destination: destination.into(),
        };
        assert!(
            grants
                .authorize_operation(&intent(managed.path(), &managed.path().join("new")))
                .is_ok()
        );
        assert!(
            grants
                .authorize_operation(&intent(selected.path(), &selected.path().join("new")))
                .is_err()
        );
        assert!(
            grants
                .authorize_operation(&intent(managed.path(), &outside.path().join("new")))
                .is_err()
        );
    }
}
