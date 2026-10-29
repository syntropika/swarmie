use crate::node::PRINCIPAL;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use swarmie_git::{Authority, Error, Intent};

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

    fn authorize_destination(&self, operation: &str) -> swarmie_git::Result<()> {
        let raw = operation
            .strip_prefix("create_worktree:")
            .or_else(|| operation.strip_prefix("restore_archive:"));
        let parsed = serde_json::from_str::<serde_json::Value>(raw.unwrap_or(operation));
        let Ok(value) = parsed else {
            return Ok(());
        };
        let pair = if raw.is_some() {
            value["managed_root"]
                .as_str()
                .zip(value["destination"].as_str())
        } else if value["kind"] == "extension" && value["name"] == "worktree_create" {
            value["arguments"][1]["managed_root"]
                .as_str()
                .zip(value["arguments"][1]["destination"].as_str())
        } else if value["kind"] == "extension" && value["name"] == "archive_restore" {
            value["arguments"][1]
                .as_str()
                .zip(value["arguments"][2].as_str())
        } else {
            None
        };
        if let Some((root, destination)) = pair {
            self.managed_destination(root, destination)?;
        }
        Ok(())
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
        self.authorize_destination(&intent.operation)
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
        let intent = |root: &Path, destination: &Path| {
            format!(
                "create_worktree:{}",
                serde_json::json!({"managed_root":root,"destination":destination})
            )
        };
        assert!(
            grants
                .authorize_destination(&intent(managed.path(), &managed.path().join("new")))
                .is_ok()
        );
        assert!(
            grants
                .authorize_destination(&intent(selected.path(), &selected.path().join("new")))
                .is_err()
        );
        assert!(
            grants
                .authorize_destination(&intent(managed.path(), &outside.path().join("new")))
                .is_err()
        );
    }
}
