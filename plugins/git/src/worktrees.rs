//! Detached worktrees and explicit preparation, using the host's process executor.

use crate::{
    Cancellation, CommandOutput, CommandSpec, Error, Git, Intent, RepositoryTarget, Result,
    settings::SetupScript,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorktreeRequest {
    pub managed_root: PathBuf,
    pub destination: PathBuf,
    pub commit: String,
    /// Explicit files to include; ignored files are refused by this profile.
    pub include_paths: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreatedWorktree {
    pub target: RepositoryTarget,
    pub source_head: String,
    /// Failure after creation retains the destination for inspection and recovery.
    pub preparation_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SetupOutcome {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct IndexEntry {
    pub path: String,
    pub mode: String,
    pub oid: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct FileEntry {
    pub path: String,
    pub bytes: Vec<u8>,
    pub executable: bool,
    pub symlink: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IncludedFile {
    path: String,
    index: Option<IndexEntry>,
    working: Option<FileEntry>,
}

impl Git {
    pub fn create_worktree(
        &self,
        principal: &str,
        source: &RepositoryTarget,
        request: &WorktreeRequest,
        cancel: &Cancellation,
    ) -> Result<CreatedWorktree> {
        let operation = format!(
            "create_worktree:{}",
            serde_json::to_string(request).map_err(|error| Error::Invalid(error.to_string()))?
        );
        self.authority.authorize(
            principal,
            &Intent {
                target: source.clone(),
                operation: operation.clone(),
            },
        )?;
        self.verify(source, cancel)?;
        let root = checkout_root(source)?;
        validate_destination(&request.managed_root, &request.destination)?;
        let resolved = self.run_checked(
            root,
            &strings(&[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{}^{{commit}}", request.commit),
            ]),
            false,
            cancel,
        )?;
        let commit = String::from_utf8(resolved)
            .map_err(|_| Error::Invalid("non-UTF-8 commit identity".into()))?
            .trim()
            .to_owned();
        let source_head = source.head.clone().ok_or_else(|| {
            Error::Invalid("an unborn checkout cannot create a managed worktree".into())
        })?;
        if !request.include_paths.is_empty() && source_head != commit {
            return Err(Error::Invalid(
                "including local changes requires the captured source HEAD as the starting commit"
                    .into(),
            ));
        }
        let included = capture_selection(self, root, &request.include_paths, cancel)?;
        self.verify(source, cancel)?;
        if included != capture_selection(self, root, &request.include_paths, cancel)? {
            return Err(Error::Stale);
        }
        self.authority.authorize(
            principal,
            &Intent {
                target: source.clone(),
                operation,
            },
        )?;
        validate_destination(&request.managed_root, &request.destination)?;
        let result = self.run(
            root,
            &[
                "worktree".into(),
                "add".into(),
                "--detach".into(),
                "--".into(),
                path_string(&request.destination)?,
                commit.clone(),
            ],
            true,
            cancel,
        );
        // Git can create a checkout before a hook fails. Preserve that authoritative fact.
        if !request.destination.exists() {
            result?;
            return Err(Error::Invalid(
                "worktree destination was not created".into(),
            ));
        }
        let discovered = self.discover(&request.destination, &Cancellation::default())?;
        let target = match discovered {
            Some(target)
                if target.root.as_deref()
                    == Some(path_string(&fs::canonicalize(&request.destination)?)?.as_str())
                    && target.common_dir == source.common_dir
                    && target.head.as_deref() == Some(commit.as_str())
                    && target.branch.is_none() =>
            {
                target
            }
            _ => {
                result?;
                return Err(Error::Invalid(
                    "worktree creation did not produce the reserved checkout".into(),
                ));
            }
        };
        let mut created = CreatedWorktree {
            target,
            source_head,
            preparation_error: None,
        };
        match result {
            Ok(output) if output.status == 0 => {}
            Ok(output) => {
                created.preparation_error =
                    Some(String::from_utf8_lossy(&output.stderr).into_owned())
            }
            Err(error) => created.preparation_error = Some(error.to_string()),
        }
        if created.preparation_error.is_none()
            && let Err(error) = apply_selection(self, &request.destination, &included, cancel)
        {
            created.preparation_error = Some(error.to_string());
        }
        created.target = self
            .discover(&request.destination, &Cancellation::default())?
            .ok_or(Error::Stale)?;
        Ok(created)
    }

    /// Setup is a separately authorized effect. Failures never remove the checkout.
    /// Callers must persist admission before calling and must not automatically replay.
    pub fn run_setup(
        &self,
        principal: &str,
        target: &RepositoryTarget,
        script: &SetupScript,
        cancel: &Cancellation,
    ) -> Result<SetupOutcome> {
        if !script.enabled {
            return Err(Error::Invalid("setup is disabled".into()));
        }
        self.authority.authorize(
            principal,
            &Intent {
                target: target.clone(),
                operation: format!(
                    "setup:{}",
                    serde_json::to_string(script)
                        .map_err(|error| Error::Invalid(error.to_string()))?
                ),
            },
        )?;
        self.verify(target, cancel)?;
        validate_relative(&script.working_directory)?;
        let root = fs::canonicalize(checkout_root(target)?)?;
        let cwd = fs::canonicalize(root.join(&script.working_directory))?;
        if !cwd.starts_with(&root) {
            return Err(Error::Invalid(
                "setup working directory leaves the checkout".into(),
            ));
        }
        let mut args = script.arguments.clone();
        args.push(script.script.clone());
        let CommandOutput {
            status,
            stdout,
            stderr,
            truncated,
        } = self.executor.run(
            &CommandSpec {
                program: path_string(&script.shell)?,
                cwd,
                args,
                env: BTreeMap::new(),
                timeout: Duration::from_secs(300),
                output_limit: 12 * 1024,
            },
            cancel,
        )?;
        Ok(SetupOutcome {
            status,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            truncated,
        })
    }
}

pub(crate) fn checkout_root(target: &RepositoryTarget) -> Result<&Path> {
    target
        .root
        .as_deref()
        .map(Path::new)
        .ok_or_else(|| Error::Invalid("this operation requires a checkout".into()))
}

pub(crate) fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

pub(crate) fn path_string(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::Invalid("this profile requires UTF-8 paths".into()))
}

pub(crate) fn validate_relative(path: &Path) -> Result<()> {
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
        || path.components().any(|part| {
            part.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(".git")
        })
    {
        return Err(Error::Invalid(
            "path must stay inside the checkout and outside Git administration".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_destination(managed_root: &Path, destination: &Path) -> Result<()> {
    if !managed_root.is_absolute() || !destination.is_absolute() {
        return Err(Error::Invalid(
            "managed root and destination must be absolute node paths".into(),
        ));
    }
    let managed_root = fs::canonicalize(managed_root)?;
    if destination.exists() || fs::symlink_metadata(destination).is_ok() {
        return Err(Error::Invalid("destination already exists".into()));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| Error::Invalid("destination requires a parent".into()))?;
    let parent = fs::canonicalize(parent)?;
    if !parent.starts_with(managed_root) {
        return Err(Error::Invalid(
            "destination is outside the authorized managed root".into(),
        ));
    }
    if destination.file_name().is_none() {
        return Err(Error::Invalid(
            "destination requires a directory name".into(),
        ));
    }
    Ok(())
}

pub(crate) fn logical_index(
    git: &Git,
    root: &Path,
    cancel: &Cancellation,
) -> Result<Vec<IndexEntry>> {
    let flags = git.run_checked(root, &strings(&["ls-files", "-v", "-z"]), false, cancel)?;
    if flags
        .split(|byte| *byte == 0)
        .filter(|item| !item.is_empty())
        .any(|item| item.first() != Some(&b'H'))
    {
        return Err(Error::Invalid(
            "sparse, unmerged or special index flags are unsupported by this preservation profile"
                .into(),
        ));
    }
    let data = git.run_checked(
        root,
        &strings(&["ls-files", "--stage", "-z"]),
        false,
        cancel,
    )?;
    data.split(|byte| *byte == 0)
        .filter(|item| !item.is_empty())
        .map(|entry| {
            let entry = std::str::from_utf8(entry)
                .map_err(|_| Error::Invalid("non-UTF-8 index paths are unsupported".into()))?;
            let (metadata, path) = entry
                .split_once('\t')
                .ok_or_else(|| Error::Invalid("invalid logical index".into()))?;
            let values: Vec<_> = metadata.split(' ').collect();
            if values.len() != 3
                || values[2] != "0"
                || !matches!(values[0], "100644" | "100755" | "120000")
            {
                return Err(Error::Invalid(
                    "conflicted indexes and submodules require another preservation profile".into(),
                ));
            }
            validate_relative(Path::new(path))?;
            Ok(IndexEntry {
                path: path.into(),
                mode: values[0].into(),
                oid: values[1].into(),
            })
        })
        .collect()
}

fn capture_selection(
    git: &Git,
    root: &Path,
    paths: &[String],
    cancel: &Cancellation,
) -> Result<Vec<IncludedFile>> {
    let index = logical_index(git, root, cancel)?;
    paths
        .iter()
        .map(|path| {
            validate_relative(Path::new(path))?;
            if path.is_empty() {
                return Err(Error::Invalid("select individual files".into()));
            }
            let ignored = git.run(
                root,
                &strings(&["check-ignore", "--quiet", "--", path]),
                false,
                cancel,
            )?;
            if ignored.status == 0 {
                return Err(Error::Invalid(
                    "ignored files require an explicit protected-content policy".into(),
                ));
            }
            let working = read_file(root, path)?;
            let index = index.iter().find(|entry| &entry.path == path).cloned();
            if working.is_none()
                && index.is_none()
                && git
                    .run_checked(
                        root,
                        &strings(&["ls-tree", "--name-only", "HEAD", "--", path]),
                        false,
                        cancel,
                    )?
                    .is_empty()
            {
                return Err(Error::Invalid(
                    "selected path is absent from HEAD, the index and checkout".into(),
                ));
            }
            Ok(IncludedFile {
                path: path.clone(),
                index,
                working,
            })
        })
        .collect()
}

fn apply_selection(
    git: &Git,
    root: &Path,
    included: &[IncludedFile],
    cancel: &Cancellation,
) -> Result<()> {
    for file in included {
        match &file.index {
            Some(index) => {
                git.run_checked(
                    root,
                    &strings(&[
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        &index.mode,
                        &index.oid,
                        &index.path,
                    ]),
                    true,
                    cancel,
                )?;
            }
            None => {
                git.run_checked(
                    root,
                    &strings(&["update-index", "--force-remove", "--", &file.path]),
                    true,
                    cancel,
                )?;
            }
        }
        match &file.working {
            Some(entry) => write_file(root, entry)?,
            None => {
                let path = root.join(&file.path);
                if fs::symlink_metadata(&path).is_ok() {
                    fs::remove_file(path)?;
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn read_file(root: &Path, relative: &str) -> Result<Option<FileEntry>> {
    validate_relative(Path::new(relative))?;
    let path = root.join(relative);
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("file has no parent".into()))?;
    if parent.exists() && !fs::canonicalize(parent)?.starts_with(fs::canonicalize(root)?) {
        return Err(Error::Invalid(
            "file traverses a symlink outside the checkout".into(),
        ));
    }
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.is_symlink() {
        return Ok(Some(FileEntry {
            path: relative.into(),
            bytes: path_string(&fs::read_link(&path)?)?.into_bytes(),
            executable: false,
            symlink: true,
        }));
    }
    if !metadata.is_file() {
        return Err(Error::Invalid(
            "only regular files and symlinks are supported".into(),
        ));
    }
    if metadata.len() > 64 * 1024 * 1024 {
        return Err(Error::Invalid(
            "file exceeds the preservation size limit".into(),
        ));
    }
    let file = crate::repository::open_regular_file(&path)?;
    #[cfg(unix)]
    let executable = {
        use std::os::unix::fs::PermissionsExt;
        file.metadata()?.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let executable = false;
    use std::io::Read;
    let mut bytes = Vec::new();
    file.take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(Error::Invalid(
            "file exceeds the preservation size limit".into(),
        ));
    }
    Ok(Some(FileEntry {
        path: relative.into(),
        bytes,
        executable,
        symlink: false,
    }))
}

pub(crate) fn write_file(root: &Path, entry: &FileEntry) -> Result<()> {
    validate_relative(Path::new(&entry.path))?;
    let path = root.join(&entry.path);
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("file has no parent".into()))?;
    validate_parent_chain(root, parent)?;
    fs::create_dir_all(parent)?;
    if !fs::canonicalize(parent)?.starts_with(fs::canonicalize(root)?) {
        return Err(Error::Invalid(
            "destination traverses a symlink outside the checkout".into(),
        ));
    }
    if fs::symlink_metadata(&path).is_ok() {
        fs::remove_file(&path)?;
    }
    if entry.symlink {
        let target = std::str::from_utf8(&entry.bytes)
            .map_err(|_| Error::Invalid("non-UTF-8 symlink target".into()))?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, &path)?;
        #[cfg(not(unix))]
        {
            let _ = target;
            return Err(Error::Invalid(
                "symlink restoration is not qualified on this platform".into(),
            ));
        }
    } else {
        fs::write(&path, &entry.bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                &path,
                fs::Permissions::from_mode(if entry.executable { 0o755 } else { 0o644 }),
            )?;
        }
    }
    Ok(())
}

fn validate_parent_chain(root: &Path, parent: &Path) -> Result<()> {
    let relative = parent
        .strip_prefix(root)
        .map_err(|_| Error::Invalid("destination leaves checkout".into()))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_symlink() => {
                return Err(Error::Invalid(
                    "preservation does not traverse symlink ancestors".into(),
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(Error::Invalid("file ancestor is not a directory".into()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
