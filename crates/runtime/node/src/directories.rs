use futures::future::LocalBoxFuture;
use std::path::{Path, PathBuf};
use swarmie_runtime::{DirectoryEntry, DirectoryPage, DirectoryRequest, DirectorySource};
use tembra::{Code, Error};

const MAX_ENTRIES: usize = 4096;
const PAGE_ENTRIES: usize = 64;
const PAGE_BYTES: usize = 24 * 1024;

/// The native embedding exposes directories only below its configured user root.
pub(crate) struct NativeDirectories {
    root: PathBuf,
    principal: String,
}

impl NativeDirectories {
    pub(crate) fn new(root: PathBuf, principal: String) -> Self {
        Self { root, principal }
    }
}

fn failure(code: Code, message: &str) -> Error {
    Error::new(code, "directories.read", message)
}

fn portable(path: &Path) -> tembra::Result<String> {
    path.to_str()
        .filter(|value| value.len() <= 4096)
        .map(str::to_owned)
        .ok_or_else(|| failure(Code::Invalid, "the directory path is not supported"))
}

fn read(root: PathBuf, request: DirectoryRequest) -> tembra::Result<DirectoryPage> {
    let root = root
        .canonicalize()
        .map_err(|_| failure(Code::Unavailable, "the folder root is unavailable"))?;
    let path = request.path.map_or_else(|| root.clone(), PathBuf::from);
    if !path.is_absolute() {
        return Err(failure(
            Code::Invalid,
            "use an absolute node directory path",
        ));
    }
    let path = path
        .canonicalize()
        .map_err(|_| failure(Code::Unavailable, "this directory cannot be opened"))?;
    if !path.starts_with(&root) {
        return Err(failure(
            Code::Forbidden,
            "this directory is outside the permitted root",
        ));
    }
    let mut entries = Vec::new();
    let source = std::fs::read_dir(&path)
        .map_err(|_| failure(Code::Unavailable, "this directory cannot be read"))?;
    for (index, entry) in source.enumerate() {
        if index >= MAX_ENTRIES {
            return Err(failure(
                Code::Exhausted,
                "this directory exceeds the listing limit",
            ));
        }
        let entry =
            entry.map_err(|_| failure(Code::Unavailable, "a directory entry cannot be read"))?;
        // Symlinks cannot make a directory outside the configured root selectable.
        let candidate = entry.path().canonicalize();
        let Ok(candidate) = candidate else { continue };
        if !candidate.starts_with(&root) || !candidate.is_dir() {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| failure(Code::Invalid, "a directory name is not supported"))?;
        entries.push(DirectoryEntry {
            name,
            path: portable(&candidate)?,
        });
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    let mut page = DirectoryPage {
        path: portable(&path)?,
        parent: if path == root {
            None
        } else {
            path.parent().map(portable).transpose()?
        },
        directories: Vec::new(),
        next: None,
    };
    let cursor = request.cursor;
    for entry in entries
        .into_iter()
        .filter(|entry| cursor.as_ref().is_none_or(|cursor| entry.name > *cursor))
    {
        page.directories.push(entry);
        let bytes = serde_json::to_vec(&page)
            .map_err(|_| failure(Code::Unavailable, "the directory page cannot be encoded"))?
            .len();
        if page.directories.len() > PAGE_ENTRIES || bytes > PAGE_BYTES {
            page.directories.pop();
            page.next = page.directories.last().map(|entry| entry.name.clone());
            if page.next.is_none() {
                return Err(failure(
                    Code::Exhausted,
                    "the directory entry exceeds the page limit",
                ));
            }
            break;
        }
    }
    Ok(page)
}

impl DirectorySource for NativeDirectories {
    fn list(
        &self,
        principal: String,
        request: DirectoryRequest,
    ) -> LocalBoxFuture<'static, tembra::Result<DirectoryPage>> {
        let root = self.root.clone();
        let allowed = principal == self.principal;
        Box::pin(async move {
            if !allowed {
                return Err(failure(
                    Code::Forbidden,
                    "the node user cannot browse this root",
                ));
            }
            tokio::task::spawn_blocking(move || read(root, request))
                .await
                .map_err(|_| failure(Code::Unavailable, "directory reading failed"))?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(path: Option<String>, cursor: Option<String>) -> DirectoryRequest {
        DirectoryRequest { path, cursor }
    }

    #[test]
    fn browsing_returns_only_directories_and_a_bounded_parent() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("project")).unwrap();
        std::fs::write(root.path().join("file.txt"), "content").unwrap();
        let first = read(root.path().into(), request(None, None)).unwrap();
        assert!(first.parent.is_none());
        assert_eq!(first.directories.len(), 1);
        assert_eq!(first.directories[0].name, "project");
        let child = read(
            root.path().into(),
            request(Some(first.directories[0].path.clone()), None),
        )
        .unwrap();
        assert_eq!(child.parent.as_deref(), Some(first.path.as_str()));
        assert!(child.directories.is_empty());
    }

    #[test]
    fn pagination_does_not_drop_or_duplicate_directories() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..70 {
            std::fs::create_dir(root.path().join(format!("folder-{index:02}"))).unwrap();
        }
        let first = read(root.path().into(), request(None, None)).unwrap();
        assert_eq!(first.directories.len(), 64);
        let next = read(root.path().into(), request(Some(first.path), first.next)).unwrap();
        assert_eq!(next.directories.len(), 6);
        assert_eq!(next.directories[0].name, "folder-64");
        assert!(next.next.is_none());
    }

    #[test]
    fn paths_outside_the_root_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        assert!(
            read(
                root.path().into(),
                request(Some(outside.path().to_str().unwrap().into()), None)
            )
            .is_err()
        );
    }

    #[test]
    fn another_principal_cannot_list_the_native_root() {
        let root = tempfile::tempdir().unwrap();
        let source = NativeDirectories::new(root.path().into(), "owner".into());
        let result =
            futures::executor::block_on(source.list("another-user".into(), request(None, None)));
        assert_eq!(result.unwrap_err().code, Code::Forbidden);
    }

    #[cfg(unix)]
    #[test]
    fn outside_symlinks_are_not_exposed() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("outside")).unwrap();
        assert!(
            read(root.path().into(), request(None, None))
                .unwrap()
                .directories
                .is_empty()
        );
    }
}
