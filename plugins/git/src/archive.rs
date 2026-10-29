//! Bounded checkout preservation with verified restoration. Cleanup remains blocked
//! until an embedding host qualifies external-writer quiescence and removal.

use crate::{
    Cancellation, Error, Git, Intent, IntentOperation, RepositoryTarget, Result, WorktreeRequest,
    worktrees::{
        DirectoryEntry, FileEntry, IndexEntry, checkout_root, create_private_directory,
        logical_index, path_string, preservation_mode, read_file, restore_mode, strings,
        validate_destination, write_file,
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PreservedIndex {
    entry: IndexEntry,
    blob: String,
    size: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ContentManifest {
    head: String,
    root_mode: crate::permissions::Permissions,
    index: Vec<PreservedIndex>,
    files: Vec<PreservedFile>,
    directories: Vec<DirectoryEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PreservedFile {
    path: String,
    blob: String,
    size: usize,
    mode: crate::permissions::Permissions,
    symlink: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ArchiveUnavailable {
    pub entry_id: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ArchiveListing {
    pub archives: Vec<ArchiveReceipt>,
    pub unavailable: Vec<ArchiveUnavailable>,
}

const MAX_CONTENT_BYTES: usize = 256 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const MAX_BUNDLE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ArchiveManifest {
    version: u32,
    principal: String,
    archive_id: String,
    target: RepositoryTarget,
    content: ContentManifest,
    bundle_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ArchiveReceipt {
    pub archive_id: String,
    pub target: RepositoryTarget,
    pub head: String,
    pub files: usize,
    pub restore_verified: bool,
    pub restored_target: Option<RepositoryTarget>,
    pub cleanup_blocked_reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CleanupEligibility {
    pub eligible: bool,
    pub changed_since_archive: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ArchiveRestoreRequest {
    pub archive_id: String,
    pub managed_root: PathBuf,
    pub destination: PathBuf,
}

/// A node-local archive store. The embedding host must qualify its durability and
/// protection policy before exposing archive operations. Ignored files are retained;
/// this directory must never be placed inside a checkout or synchronized implicitly.
#[derive(Clone, Debug)]
pub struct ArchiveDirectory {
    directory: PathBuf,
}

impl ArchiveDirectory {
    pub fn open(directory: impl Into<PathBuf>) -> Result<Self> {
        let directory = directory.into();
        fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        #[cfg(windows)]
        crate::permissions::windows::protect_directory(&directory)?;
        Ok(Self {
            directory: fs::canonicalize(directory)?,
        })
    }

    fn location(&self, archive_id: &str) -> Result<PathBuf> {
        if archive_id.is_empty() || archive_id.len() > 256 {
            return Err(Error::Invalid("invalid archive ID".into()));
        }
        Ok(self
            .directory
            .join(format!("{:x}", Sha256::digest(archive_id.as_bytes()))))
    }

    /// Captures tracked, untracked and ignored content without modifying the source.
    /// No successful archive receipt promises permission to delete a checkout.
    pub fn capture(
        &self,
        git: &Git,
        principal: &str,
        archive_id: &str,
        target: &RepositoryTarget,
        cancel: &Cancellation,
    ) -> Result<ArchiveReceipt> {
        let (root, directory) = (|| -> Result<_> {
            git.authority.authorize(
                principal,
                &Intent {
                    target: target.clone(),
                    operation: IntentOperation::ArchiveCapture {
                        archive_id: archive_id.into(),
                    },
                },
            )?;
            git.verify(target, cancel)?;
            if target.head.is_none() {
                return Err(Error::Invalid("archive requires a committed HEAD".into()));
            }
            let root = fs::canonicalize(checkout_root(target)?)?;
            if self.directory.starts_with(&root) {
                return Err(Error::Invalid(
                    "archive storage must be outside the source checkout".into(),
                ));
            }
            let directory = self.location(archive_id)?;
            if directory.exists() {
                return Err(Error::Invalid("archive ID already exists".into()));
            }
            Ok((root, directory))
        })()
        .map_err(|error| Error::NotStarted(error.to_string()))?;
        create_private_directory(&directory)?;
        create_private_directory(&directory.join("blobs"))?;
        let content = capture_content(git, &root, cancel, Some(&directory))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        let bundle = directory.join("commits.bundle");
        git.run_checked(
            &root,
            &strings(&["bundle", "create", &path_string(&bundle)?, "HEAD"]),
            false,
            cancel,
        )?;
        git.run_checked(
            &root,
            &strings(&["bundle", "verify", &path_string(&bundle)?]),
            false,
            cancel,
        )?;
        if content != capture_content(git, &root, cancel, None)? {
            return Err(Error::Stale);
        }
        git.verify(target, cancel)?;
        fs::OpenOptions::new()
            .write(true)
            .open(&bundle)?
            .sync_all()?;
        let manifest = ArchiveManifest {
            version: 2,
            principal: principal.into(),
            archive_id: archive_id.into(),
            target: target.clone(),
            content,
            bundle_sha256: hash_file(&bundle, MAX_BUNDLE_BYTES)?,
        };
        let manifest_bytes =
            serde_json::to_vec(&manifest).map_err(|error| Error::Invalid(error.to_string()))?;
        if manifest_bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(Error::Invalid(
                "archive manifest exceeds its size limit".into(),
            ));
        }
        durable_write(&directory.join("manifest.json"), &manifest_bytes)?;
        durable_write(
            &directory.join("manifest.sha256"),
            format!("{:x}", Sha256::digest(&manifest_bytes)).as_bytes(),
        )?;
        crate::persistence::sync_directory(&directory)?;
        crate::persistence::sync_directory(&self.directory)?;
        Ok(receipt(archive_id, &manifest, false))
    }

    /// Restoration imports archived objects and creates a newly reserved detached
    /// worktree. It never overlays an existing directory or runs setup scripts.
    pub fn restore(
        &self,
        git: &Git,
        principal: &str,
        repository: &RepositoryTarget,
        request: &ArchiveRestoreRequest,
        cancel: &Cancellation,
    ) -> Result<ArchiveReceipt> {
        let archive_id = request.archive_id.as_str();
        let managed_root = request.managed_root.as_path();
        let destination = request.destination.as_path();
        let (directory, manifest, bundle, root) = (|| -> Result<_> {
            git.authority.authorize(
                principal,
                &Intent {
                    target: repository.clone(),
                    operation: IntentOperation::ArchiveRestore {
                        archive_id: archive_id.into(),
                        managed_root: managed_root.into(),
                        destination: destination.into(),
                    },
                },
            )?;
            git.verify(repository, cancel)?;
            validate_destination(managed_root, destination)?;
            let directory = self.location(archive_id)?;
            let manifest = self.load_owned(archive_id, principal)?;
            verify_content_blobs(&directory, &manifest.content)?;
            let bundle = directory.join("commits.bundle");
            if hash_file(&bundle, MAX_BUNDLE_BYTES)? != manifest.bundle_sha256 {
                return Err(Error::Invalid("archive bundle integrity mismatch".into()));
            }
            let root = checkout_root(repository)?;
            git.run_checked(
                root,
                &strings(&["bundle", "verify", &path_string(&bundle)?]),
                false,
                cancel,
            )?;
            Ok((directory, manifest, bundle, root))
        })()
        .map_err(|error| Error::NotStarted(error.to_string()))?;
        git.run_checked(
            root,
            &strings(&[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                &path_string(&bundle)?,
                "HEAD",
            ]),
            true,
            cancel,
        )?;
        let refreshed = git.discover(root, cancel)?.ok_or(Error::Stale)?;
        let created = git
            .create_worktree(
                principal,
                &refreshed,
                &WorktreeRequest {
                    managed_root: managed_root.into(),
                    destination: destination.into(),
                    commit: manifest.content.head.clone(),
                    include_paths: Vec::new(),
                },
                cancel,
            )
            .map_err(|error| {
                Error::Invalid(format!(
                    "archive objects were imported; checkout preparation failed: {error}"
                ))
            })?;
        if let Some(error) = created.preparation_error {
            return Err(Error::Invalid(format!(
                "restore checkout retained after materialization failed: {error}"
            )));
        }
        // Remove only tracked baseline files absent from the exact archived inventory.
        // Refuse unexpected files: never recursively delete an unreviewed directory.
        let baseline = capture_content(git, destination, cancel, None)?;
        for file in &baseline.files {
            if !manifest
                .content
                .files
                .iter()
                .any(|entry| entry.path == file.path)
            {
                if !baseline
                    .index
                    .iter()
                    .any(|entry| entry.entry.path == file.path)
                {
                    return Err(Error::Invalid(
                        "unexpected materialized content blocks restoration".into(),
                    ));
                }
                fs::remove_file(destination.join(&file.path))?;
            }
        }
        let mut baseline_directories = baseline.directories.clone();
        baseline_directories.sort_by_key(|entry| std::cmp::Reverse(entry.path.len()));
        for directory in baseline_directories {
            if !manifest
                .content
                .directories
                .iter()
                .any(|entry| entry.path == directory.path)
            {
                // remove_dir refuses nonempty directories; there is no recursive fallback.
                fs::remove_dir(destination.join(&directory.path))?;
            }
        }
        for entry in &baseline.index {
            git.run_checked(
                destination,
                &strings(&["update-index", "--force-remove", "--", &entry.entry.path]),
                true,
                cancel,
            )?;
        }
        for (number, preserved) in manifest.content.index.iter().enumerate() {
            let temporary = directory.join(format!("blob-{number}.import"));
            durable_write(
                &temporary,
                &load_blob(&directory, &preserved.blob, preserved.size)?,
            )?;
            let oid = git.run_checked(
                destination,
                &strings(&[
                    "hash-object",
                    "-w",
                    "--no-filters",
                    &path_string(&temporary)?,
                ]),
                true,
                cancel,
            )?;
            fs::remove_file(&temporary)?;
            if String::from_utf8_lossy(&oid).trim() != preserved.entry.oid {
                return Err(Error::Invalid(
                    "archive staged content integrity mismatch".into(),
                ));
            }
            git.run_checked(
                destination,
                &strings(&[
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &preserved.entry.mode,
                    &preserved.entry.oid,
                    &preserved.entry.path,
                ]),
                true,
                cancel,
            )?;
        }
        for directory in &manifest.content.directories {
            crate::worktrees::validate_relative(Path::new(&directory.path))?;
            create_private_directory(&destination.join(&directory.path))?;
        }
        // Files are sorted so a symlink can never become an ancestor of a later file.
        for file in &manifest.content.files {
            write_file(
                destination,
                &FileEntry {
                    path: file.path.clone(),
                    bytes: load_blob(&directory, &file.blob, file.size)?,
                    mode: file.mode.clone(),
                    symlink: file.symlink,
                },
            )?;
        }
        #[cfg(not(windows))]
        {
            for entry in manifest.content.directories.iter().rev() {
                restore_mode(&destination.join(&entry.path), &entry.mode)?;
            }
            restore_mode(destination, &manifest.content.root_mode)?;
        }
        #[cfg(windows)]
        {
            // Files and directories remain protected until their captured parent
            // ACLs are in place. Final leaf ACLs cannot inherit from temporary parents.
            restore_mode(destination, &manifest.content.root_mode)?;
            for entry in &manifest.content.directories {
                restore_mode(&destination.join(&entry.path), &entry.mode)?;
            }
            for file in &manifest.content.files {
                restore_mode(&destination.join(&file.path), &file.mode)?;
            }
        }
        let observed = capture_content(git, destination, cancel, None)?;
        if observed != manifest.content {
            return Err(Error::Invalid(
                "restoration verification failed; destination retained for inspection".into(),
            ));
        }
        let mut result = receipt(archive_id, &manifest, true);
        let restored_target = git.discover(destination, cancel)?.ok_or(Error::Stale)?;
        let proof_identity = format!("{:x}", Sha256::digest(restored_target.git_dir.as_bytes()));
        result.restored_target = Some(restored_target);
        let proof_bytes =
            serde_json::to_vec(&result).map_err(|error| Error::Invalid(error.to_string()))?;
        let mut generation = 0_u64;
        loop {
            let proof = directory.join(format!(
                "verified-restoration-{proof_identity}-{generation}.json"
            ));
            match durable_write(&proof, &proof_bytes) {
                // A later explicit restore may reuse a manually released destination.
                // Retain earlier proofs rather than overwriting their evidence.
                Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    generation = generation.checked_add(1).ok_or_else(|| {
                        Error::Invalid("restoration proof generation exhausted".into())
                    })?;
                }
                outcome => {
                    outcome?;
                    break;
                }
            }
        }
        crate::persistence::sync_directory(&directory)?;
        Ok(result)
    }

    /// A freshness check is useful evidence but cannot freeze an external editor.
    /// Consequently this native profile deliberately exposes no delete operation.
    pub fn cleanup_eligibility(
        &self,
        git: &Git,
        principal: &str,
        archive_id: &str,
        target: &RepositoryTarget,
        cancel: &Cancellation,
    ) -> Result<CleanupEligibility> {
        git.authority.authorize(
            principal,
            &Intent {
                target: target.clone(),
                operation: IntentOperation::CleanupCheck {
                    archive_id: archive_id.into(),
                    pinned: false,
                    active_writers: 0,
                },
            },
        )?;
        let manifest = self.load_owned(archive_id, principal)?;
        let changed = target.common_dir != manifest.target.common_dir
            || target.git_dir != manifest.target.git_dir
            || capture_content(git, checkout_root(target)?, cancel, None)? != manifest.content;
        Ok(CleanupEligibility { eligible: false, changed_since_archive: changed, reason: if changed { "Checkout changed since capture; retain it and create a new archive." } else { "The host has not qualified external-writer quiescence and safe physical removal; checkout retained." }.into() })
    }

    fn load(&self, archive_id: &str) -> Result<ArchiveManifest> {
        let location = self.location(archive_id)?;
        let bytes = bounded_read(&location.join("manifest.json"), MAX_MANIFEST_BYTES)?;
        if format!("{:x}", Sha256::digest(&bytes))
            != String::from_utf8(bounded_read(&location.join("manifest.sha256"), 64)?)
                .map_err(|_| Error::Invalid("invalid archive checksum".into()))?
        {
            return Err(Error::Invalid("archive manifest integrity mismatch".into()));
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|error| Error::Invalid(error.to_string()))?;
        if value.get("version").and_then(serde_json::Value::as_u64) != Some(2) {
            return Err(Error::Invalid(
                "unsupported archive manifest version; original archive retained".into(),
            ));
        }
        let manifest: ArchiveManifest =
            serde_json::from_value(value).map_err(|error| Error::Invalid(error.to_string()))?;
        if manifest.version != 2 {
            return Err(Error::Invalid(
                "unsupported archive manifest version".into(),
            ));
        }
        validate_manifest(&manifest)?;
        Ok(manifest)
    }

    fn load_owned(&self, archive_id: &str, principal: &str) -> Result<ArchiveManifest> {
        let manifest = self.load(archive_id)?;
        if manifest.principal != principal {
            return Err(Error::Denied("archive belongs to another principal".into()));
        }
        Ok(manifest)
    }

    pub fn list(
        &self,
        git: &Git,
        principal: &str,
        target: &RepositoryTarget,
        cancel: &Cancellation,
    ) -> Result<ArchiveListing> {
        git.authority.authorize(
            principal,
            &Intent {
                target: target.clone(),
                operation: IntentOperation::ArchiveList,
            },
        )?;
        git.verify(target, cancel)?;
        let mut archives = Vec::new();
        let mut unavailable = Vec::new();
        for entry in fs::read_dir(&self.directory)? {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let path = entry.path();
            let entry_id = entry.file_name().to_string_lossy().into_owned();
            let result = (|| -> Result<Option<ArchiveReceipt>> {
                let bytes = bounded_read(&path.join("manifest.json"), MAX_MANIFEST_BYTES)?;
                let manifest: ArchiveManifest = serde_json::from_slice(&bytes)
                    .map_err(|error| Error::Invalid(error.to_string()))?;
                // Do not expose another principal's recognizable archive metadata.
                if manifest.principal != principal
                    || manifest.target.common_dir != target.common_dir
                {
                    return Ok(None);
                }
                let verified = self.load_owned(&manifest.archive_id, principal)?;
                if self.location(&manifest.archive_id)? != path {
                    return Err(Error::Invalid("archive directory identity mismatch".into()));
                }
                Ok(Some(receipt(
                    &verified.archive_id,
                    &verified,
                    has_restoration_proof(&path, &verified)?,
                )))
            })();
            match result {
                Ok(Some(receipt)) => archives.push(receipt),
                Ok(None) => {},
                Err(_) => unavailable.push(ArchiveUnavailable { entry_id, message: "Archive unavailable: metadata is incomplete, damaged, unsupported, or exceeds this profile's limits. Original files were retained.".into() }),
            }
        }
        archives.sort_by(|left, right| left.archive_id.cmp(&right.archive_id));
        unavailable.sort_by(|left, right| left.entry_id.cmp(&right.entry_id));
        Ok(ArchiveListing {
            archives,
            unavailable,
        })
    }
}

fn has_restoration_proof(directory: &Path, manifest: &ArchiveManifest) -> Result<bool> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_str().is_some_and(|name| {
            name.starts_with("verified-restoration-") && name.ends_with(".json")
        }) && entry.file_type()?.is_file()
        {
            let bytes = match bounded_read(&entry.path(), 64 * 1024) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            // A crash can leave an incomplete proof file. Its existence alone does
            // not establish a confirmed restoration.
            if let Ok(proof) = serde_json::from_slice::<ArchiveReceipt>(&bytes)
                && proof.restore_verified
                && proof.restored_target.is_some()
                && proof.archive_id == manifest.archive_id
                && proof.target == manifest.target
                && proof.head == manifest.content.head
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn receipt(id: &str, manifest: &ArchiveManifest, verified: bool) -> ArchiveReceipt {
    ArchiveReceipt {
        archive_id: id.into(),
        target: manifest.target.clone(),
        head: manifest.content.head.clone(),
        files: manifest.content.files.len(),
        restore_verified: verified,
        restored_target: None,
        cleanup_blocked_reason:
            "Checkout retained: external-writer quiescence and physical removal are not qualified."
                .into(),
    }
}

fn durable_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("archive file requires a parent".into()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    // NamedTempFile creates Unix files as 0600 before any content is written.
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    if !crate::persistence::publish_new(temporary.path(), path)? {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "archive file already exists",
        )
        .into());
    }
    Ok(())
}

fn capture_content(
    git: &Git,
    root: &Path,
    cancel: &Cancellation,
    store: Option<&Path>,
) -> Result<ContentManifest> {
    let target = git.discover(root, cancel)?.ok_or(Error::Stale)?;
    let head = target
        .head
        .ok_or_else(|| Error::Invalid("archive requires a committed HEAD".into()))?;
    let mut index = Vec::new();
    let mut budget = 0_usize;
    for entry in logical_index(git, root, cancel)? {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if index.len() >= MAX_ENTRIES {
            return Err(Error::Invalid(
                "archive index exceeds its inventory limit".into(),
            ));
        }
        let bytes = git.run_checked(
            root,
            &strings(&["cat-file", "blob", &entry.oid]),
            false,
            cancel,
        )?;
        account_bytes(&mut budget, bytes.len())?;
        index.push(PreservedIndex {
            entry,
            blob: preserve_blob(store, &bytes)?,
            size: bytes.len(),
        });
    }
    let mut files = Vec::new();
    let mut directories = Vec::new();
    collect_files(
        root,
        root,
        &mut files,
        &mut directories,
        &mut budget,
        store,
        cancel,
    )?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    directories.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(ContentManifest {
        head,
        root_mode: preservation_mode(root, &fs::metadata(root)?)?,
        index,
        files,
        directories,
    })
}

fn collect_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<PreservedFile>,
    directories: &mut Vec<DirectoryEntry>,
    budget: &mut usize,
    store: Option<&Path>,
    cancel: &Cancellation,
) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let entry = entry?;
        if directory == root && entry.file_name() == ".git" {
            continue;
        }
        if files.len() + directories.len() >= MAX_ENTRIES {
            return Err(Error::Invalid(
                "checkout exceeds this archive profile's inventory limit".into(),
            ));
        }
        let path = entry.path();
        let relative = path_string(
            path.strip_prefix(root)
                .map_err(|_| Error::Invalid("inventory left checkout".into()))?,
        )?;
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            if path.join(".git").exists() {
                return Err(Error::Invalid(
                    "nested repositories require a separately qualified archive profile".into(),
                ));
            }
            directories.push(DirectoryEntry {
                path: relative,
                mode: preservation_mode(&path, &metadata)?,
            });
            collect_files(root, &path, files, directories, budget, store, cancel)?;
        } else {
            let file = read_file(root, &relative)?.ok_or(Error::Stale)?;
            account_bytes(budget, file.bytes.len())?;
            files.push(PreservedFile {
                path: file.path,
                blob: preserve_blob(store, &file.bytes)?,
                size: file.bytes.len(),
                mode: file.mode,
                symlink: file.symlink,
            });
        }
    }
    Ok(())
}

fn account_bytes(total: &mut usize, bytes: usize) -> Result<()> {
    *total = total
        .checked_add(bytes)
        .ok_or_else(|| Error::Invalid("archive size overflow".into()))?;
    if *total > MAX_CONTENT_BYTES {
        return Err(Error::Invalid(
            "archive staged and working content exceeds its combined size limit".into(),
        ));
    }
    Ok(())
}

fn preserve_blob(store: Option<&Path>, bytes: &[u8]) -> Result<String> {
    let digest = format!("{:x}", Sha256::digest(bytes));
    if let Some(store) = store {
        let path = store.join("blobs").join(&digest);
        match durable_write(&path, bytes) {
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if hash_file(&path, MAX_CONTENT_BYTES as u64)? != digest {
                    return Err(Error::Invalid(
                        "archive content collision or corruption".into(),
                    ));
                }
            }
            result => result?,
        }
    }
    Ok(digest)
}

fn load_blob(directory: &Path, digest: &str, size: usize) -> Result<Vec<u8>> {
    if digest.len() != 64
        || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        || size > 64 * 1024 * 1024
    {
        return Err(Error::Invalid("invalid archive content descriptor".into()));
    }
    let bytes = bounded_read(&directory.join("blobs").join(digest), size as u64)?;
    if bytes.len() != size || format!("{:x}", Sha256::digest(&bytes)) != digest {
        return Err(Error::Invalid("archive content integrity mismatch".into()));
    }
    Ok(bytes)
}

fn bounded_read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = crate::repository::open_regular_file(path)?;
    if file.metadata()?.len() > limit {
        return Err(Error::Invalid("archive file exceeds its size limit".into()));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(Error::Invalid("archive file exceeds its size limit".into()));
    }
    Ok(bytes)
}

fn hash_file(path: &Path, limit: u64) -> Result<String> {
    let mut file = crate::repository::open_regular_file(path)?;
    if file.metadata()?.len() > limit {
        return Err(Error::Invalid(
            "archive bundle exceeds its size limit".into(),
        ));
    }
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let length = file.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        total += length as u64;
        if total > limit {
            return Err(Error::Invalid("archive file exceeds its size limit".into()));
        }
        digest.update(&buffer[..length]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn validate_manifest(manifest: &ArchiveManifest) -> Result<()> {
    if manifest.content.index.len() > MAX_ENTRIES
        || manifest.content.files.len() + manifest.content.directories.len() > MAX_ENTRIES
    {
        return Err(Error::Invalid("archive inventory exceeds its limit".into()));
    }
    manifest.content.root_mode.validate()?;
    for directory in &manifest.content.directories {
        crate::worktrees::validate_relative(Path::new(&directory.path))?;
        directory.mode.validate()?;
    }
    for file in &manifest.content.files {
        crate::worktrees::validate_relative(Path::new(&file.path))?;
        file.mode.validate()?;
    }
    for entry in &manifest.content.index {
        crate::worktrees::validate_relative(Path::new(&entry.entry.path))?;
        if !matches!(entry.entry.mode.as_str(), "100644" | "100755" | "120000")
            || !matches!(entry.entry.oid.len(), 40 | 64)
            || !entry.entry.oid.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(Error::Invalid("invalid preserved index entry".into()));
        }
    }
    let mut total = 0;
    for size in manifest
        .content
        .index
        .iter()
        .map(|entry| entry.size)
        .chain(manifest.content.files.iter().map(|entry| entry.size))
    {
        account_bytes(&mut total, size)?;
    }
    Ok(())
}

fn verify_content_blobs(directory: &Path, content: &ContentManifest) -> Result<()> {
    let mut expected = BTreeMap::new();
    for (digest, size) in content
        .index
        .iter()
        .map(|entry| (&entry.blob, entry.size))
        .chain(content.files.iter().map(|entry| (&entry.blob, entry.size)))
    {
        if digest.len() != 64
            || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            || size > 64 * 1024 * 1024
        {
            return Err(Error::Invalid("invalid archive content descriptor".into()));
        }
        if let Some(previous) = expected.insert(digest, size)
            && previous != size
        {
            return Err(Error::Invalid("inconsistent archive content sizes".into()));
        }
    }
    for (digest, size) in expected {
        let path = directory.join("blobs").join(digest);
        if fs::symlink_metadata(&path)?.len() != size as u64
            || hash_file(&path, size as u64)? != *digest
        {
            return Err(Error::Invalid("archive content integrity mismatch".into()));
        }
    }
    Ok(())
}
