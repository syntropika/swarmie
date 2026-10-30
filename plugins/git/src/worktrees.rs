//! Detached worktrees and explicit preparation, using the host's process executor.

use crate::{
    Cancellation, CommandOutput, CommandSpec, Error, Git, Intent, IntentOperation,
    RepositoryTarget, Result, settings::SetupScript,
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
    pub mode: crate::permissions::Permissions,
    pub symlink: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct DirectoryEntry {
    pub path: String,
    pub mode: crate::permissions::Permissions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IncludedFile {
    path: String,
    index: Option<IndexEntry>,
    working: Option<FileEntry>,
    directories: Vec<DirectoryEntry>,
}

impl Git {
    pub fn create_worktree(
        &self,
        principal: &str,
        source: &RepositoryTarget,
        request: &WorktreeRequest,
        cancel: &Cancellation,
    ) -> Result<CreatedWorktree> {
        let (root, commit, source_head, included) = (|| -> Result<_> {
            let operation = IntentOperation::CreateWorktree {
                request: request.clone(),
            };
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
            Ok((root, commit, source_head, included))
        })()
        .map_err(|error| Error::NotStarted(error.to_string()))?;
        // Restrict the reserved root before Git materializes any tracked content.
        create_private_directory(&request.destination)?;
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
        // Compare native canonical identities, independently of Git's argv spelling.
        let destination_identity = fs::canonicalize(&request.destination)?;
        let target = match discovered {
            Some(target)
                if target.root.as_deref().map(Path::new)
                    == Some(destination_identity.as_path())
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
        let (cwd, args) = (|| -> Result<_> {
            if !script.enabled {
                return Err(Error::Invalid("setup is disabled".into()));
            }
            self.authority.authorize(
                principal,
                &Intent {
                    target: target.clone(),
                    operation: IntentOperation::Setup {
                        script: script.clone(),
                    },
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
            Ok((cwd, args))
        })()
        .map_err(|error| Error::NotStarted(error.to_string()))?;
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
    let value = path
        .to_str()
        .ok_or_else(|| Error::Invalid("this profile requires UTF-8 paths".into()))?;
    #[cfg(windows)]
    {
        use std::path::Prefix;
        // Git for Windows cannot use Rust's verbatim namespace as a bundle path.
        // Relative inventory paths also use Git's slash convention, so nested
        // working files and index entries retain the same identity.
        let value = match path.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::VerbatimDisk(_) => value
                    .get(4..)
                    .ok_or_else(|| Error::Invalid("invalid verbatim drive path".into()))?
                    .to_owned(),
                Prefix::VerbatimUNC(_, _) => format!(
                    "//{}",
                    value
                        .get(8..)
                        .ok_or_else(|| Error::Invalid("invalid verbatim UNC path".into()))?
                ),
                Prefix::Verbatim(_) | Prefix::DeviceNS(_) => {
                    return Err(Error::Invalid(
                        "Git does not support this device namespace".into(),
                    ));
                }
                _ => value.to_owned(),
            },
            _ => value.to_owned(),
        };
        Ok(value.replace('\\', "/"))
    }
    #[cfg(not(windows))]
    Ok(value.to_owned())
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
    if paths.is_empty() {
        return Ok(Vec::new());
    }
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
                directories: capture_parents(root, Path::new(path))?,
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
    let mut directories = BTreeMap::new();
    for file in included {
        for directory in &file.directories {
            create_private_directory(&root.join(&directory.path))?;
            directories.insert(directory.path.clone(), directory.mode.clone());
        }
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
    #[cfg(not(windows))]
    for (path, mode) in directories.iter().rev() {
        restore_mode(&root.join(path), mode)?;
    }
    #[cfg(windows)]
    {
        // Protected temporary children prevent parent inheritance from changing
        // their contents' access while each captured ACL is restored in order.
        for (path, mode) in &directories {
            restore_mode(&root.join(path), mode)?;
        }
        for file in included {
            if let Some(entry) = &file.working {
                restore_mode(&root.join(&entry.path), &entry.mode)?;
            }
        }
    }
    let paths = included
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    if !included.is_empty() && capture_selection(git, root, &paths, cancel)? != included {
        return Err(Error::Invalid(
            "selected content or permissions could not be restored exactly; checkout retained"
                .into(),
        ));
    }
    Ok(())
}

pub(crate) fn read_file(root: &Path, relative: &str) -> Result<Option<FileEntry>> {
    read_file_for(root, relative, FilePurpose::Preservation)
}

/// Git records content and an executable flag, not filesystem ownership or ACLs.
pub(crate) fn read_git_file(root: &Path, relative: &str) -> Result<Option<FileEntry>> {
    read_file_for(root, relative, FilePurpose::GitSnapshot)
}

#[derive(Clone, Copy)]
enum FilePurpose {
    Preservation,
    GitSnapshot,
}

impl FilePurpose {
    fn mode(self, path: &Path, metadata: &fs::Metadata) -> Result<crate::permissions::Permissions> {
        if matches!(self, Self::Preservation) {
            return preservation_mode(path, metadata);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            Ok(crate::permissions::Permissions::Unix(
                if metadata.permissions().mode() & 0o111 != 0 {
                    0o755
                } else {
                    0o644
                },
            ))
        }
        #[cfg(not(unix))]
        Ok(crate::permissions::Permissions::Unix(0o644))
    }

    fn restore(self, path: &Path, mode: &crate::permissions::Permissions) -> Result<()> {
        if matches!(self, Self::Preservation) {
            #[cfg(not(windows))]
            return restore_mode(path, mode);
            // Windows callers restore the completed hierarchy from parents to
            // children after all payload files have a protected temporary ACL.
            #[cfg(windows)]
            return Ok(());
        }
        #[cfg(windows)]
        let _ = (path, mode);
        #[cfg(unix)]
        restore_mode(path, mode)?;
        // A Windows Git snapshot is a new private file, not an archive restore.
        // Its inherited ACL is never claimed to preserve the source metadata.
        Ok(())
    }
}

fn read_file_for(root: &Path, relative: &str, purpose: FilePurpose) -> Result<Option<FileEntry>> {
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
        #[cfg(windows)]
        if matches!(purpose, FilePurpose::Preservation) {
            return Err(Error::Invalid(
                "reparse points require another preservation profile".into(),
            ));
        }
        #[cfg(unix)]
        if matches!(purpose, FilePurpose::Preservation) {
            reject_extended_metadata(&path)?;
        }
        return Ok(Some(FileEntry {
            path: relative.into(),
            bytes: path_string(&fs::read_link(&path)?)?.into_bytes(),
            mode: crate::permissions::Permissions::Unix(0),
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
    let mode = purpose.mode(&path, &file.metadata()?)?;
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
        mode,
        symlink: false,
    }))
}

pub(crate) fn write_file(root: &Path, entry: &FileEntry) -> Result<()> {
    write_file_for(root, entry, FilePurpose::Preservation)
}

pub(crate) fn write_git_file(root: &Path, entry: &FileEntry) -> Result<()> {
    write_file_for(root, entry, FilePurpose::GitSnapshot)
}

fn write_file_for(root: &Path, entry: &FileEntry, purpose: FilePurpose) -> Result<()> {
    validate_relative(Path::new(&entry.path))?;
    let path = root.join(&entry.path);
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("file has no parent".into()))?;
    validate_parent_chain(root, parent)?;
    create_private_parents(root, parent)?;
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
        use std::io::Write;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        #[cfg(windows)]
        crate::permissions::windows::protect_file(&path)?;
        file.write_all(&entry.bytes)?;
        file.sync_all()?;
        purpose.restore(&path, &entry.mode)?;
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

pub(crate) fn create_private_directory(path: &Path) -> Result<()> {
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        create_directory(builder, path)
    }
    #[cfg(windows)]
    {
        let _ = builder;
        crate::permissions::windows::create_private_directory(path)
    }
    #[cfg(not(any(unix, windows)))]
    create_directory(builder, path)
}

#[cfg(not(windows))]
fn create_directory(builder: fs::DirBuilder, path: &Path) -> Result<()> {
    match builder.create(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(path)?;
            if metadata.is_dir() && !metadata.is_symlink() {
                Ok(())
            } else {
                Err(Error::Invalid(
                    "preservation directory is not a physical directory".into(),
                ))
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn create_private_parents(root: &Path, parent: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    for component in parent
        .strip_prefix(root)
        .map_err(|_| Error::Invalid("destination leaves checkout".into()))?
        .components()
    {
        current.push(component);
        create_private_directory(&current)?;
    }
    Ok(())
}

fn capture_parents(root: &Path, path: &Path) -> Result<Vec<DirectoryEntry>> {
    let mut directories = Vec::new();
    let mut current = PathBuf::new();
    if let Some(parent) = path.parent() {
        for component in parent.components() {
            current.push(component);
            let absolute = root.join(&current);
            let metadata = match fs::symlink_metadata(&absolute) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => return Err(error.into()),
            };
            if !metadata.is_dir() || metadata.is_symlink() {
                return Err(Error::Invalid(
                    "preservation does not traverse symlink ancestors".into(),
                ));
            }
            directories.push(DirectoryEntry {
                path: path_string(&current)?,
                mode: preservation_mode(&absolute, &metadata)?,
            });
        }
    }
    Ok(directories)
}

pub(crate) fn preservation_mode(
    path: &Path,
    metadata: &fs::Metadata,
) -> Result<crate::permissions::Permissions> {
    crate::permissions::Permissions::capture(path, metadata)
}

#[cfg(unix)]
pub(crate) fn reject_extended_metadata(path: &Path) -> Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| Error::Invalid("invalid filesystem path".into()))?;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let count = unsafe { libc::llistxattr(path.as_ptr(), std::ptr::null_mut(), 0) };
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let count =
        unsafe { libc::listxattr(path.as_ptr(), std::ptr::null_mut(), 0, libc::XATTR_NOFOLLOW) };
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    )))]
    let count = -1;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        unsafe extern "C" {
            fn acl_get_link_np(path: *const libc::c_char, kind: libc::c_int) -> *mut libc::c_void;
            fn acl_get_entry(
                acl: *mut libc::c_void,
                entry_id: libc::c_int,
                entry: *mut *mut libc::c_void,
            ) -> libc::c_int;
            fn acl_free(object: *mut libc::c_void) -> libc::c_int;
        }
        // Darwin stores extended ACL entries separately from ordinary xattrs.
        let acl = unsafe { acl_get_link_np(path.as_ptr(), 0x100) };
        if acl.is_null() {
            let error = std::io::Error::last_os_error();
            // Darwin reports an absent FILESEC_ACL property as ENOENT.
            if error.raw_os_error() != Some(libc::ENOENT) {
                return Err(error.into());
            }
        } else {
            let mut entry = std::ptr::null_mut();
            let entries = unsafe { acl_get_entry(acl, 0, &mut entry) };
            let entry_error = std::io::Error::last_os_error();
            unsafe {
                acl_free(acl);
            }
            if entries == 0 {
                return Err(Error::Invalid(
                    "extended ACLs require another preservation profile".into(),
                ));
            }
            if entry_error.raw_os_error() != Some(libc::EINVAL) {
                return Err(entry_error.into());
            }
        }
    }
    if count > 0 {
        return Err(Error::Invalid(
            "extended attributes and ACLs require another preservation profile".into(),
        ));
    }
    if count < 0 {
        let error = std::io::Error::last_os_error();
        if !matches!(error.raw_os_error(), Some(libc::ENOTSUP)) {
            return Err(error.into());
        }
    }
    Ok(())
}

pub(crate) fn restore_mode(path: &Path, mode: &crate::permissions::Permissions) -> Result<()> {
    mode.restore(path)
}

#[cfg(all(test, windows))]
mod path_tests {
    use super::path_string;
    use std::path::Path;

    #[test]
    fn git_paths_preserve_drive_unc_and_nested_identity_without_device_namespaces() {
        for (native, git) in [
            (r"C:\checkout\file", "C:/checkout/file"),
            (r"\\?\C:\checkout\file", "C:/checkout/file"),
            (r"\\server\share\file", "//server/share/file"),
            (r"\\?\UNC\server\share\file", "//server/share/file"),
            (r"nested\file", "nested/file"),
        ] {
            assert_eq!(path_string(Path::new(native)).unwrap(), git);
        }
        assert!(path_string(Path::new(r"\\.\PhysicalDrive0")).is_err());
    }
}
