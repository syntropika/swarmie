//! Complete local preservation with verified restoration. Cleanup remains blocked
//! until an embedding host qualifies external-writer quiescence and removal.

use crate::{
    Cancellation, Error, Git, Intent, RepositoryTarget, Result, WorktreeRequest,
    worktrees::{
        FileEntry, IndexEntry, checkout_root, logical_index, path_string, read_file, strings,
        validate_destination, write_file,
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PreservedIndex {
    entry: IndexEntry,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ContentManifest {
    head: String,
    index: Vec<PreservedIndex>,
    files: Vec<FileEntry>,
    directories: Vec<String>,
}

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
        git.authority.authorize(
            principal,
            &Intent {
                target: target.clone(),
                operation: "archive_protected_content".into(),
            },
        )?;
        git.verify(target, cancel)?;
        let root = fs::canonicalize(checkout_root(target)?)?;
        if self.directory.starts_with(&root) {
            return Err(Error::Invalid(
                "archive storage must be outside the source checkout".into(),
            ));
        }
        let content = capture_content(git, &root, cancel)?;
        let directory = self.location(archive_id)?;
        fs::create_dir(&directory)?;
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
        if content != capture_content(git, &root, cancel)? {
            return Err(Error::Stale);
        }
        git.verify(target, cancel)?;
        fs::File::open(&bundle)?.sync_all()?;
        let manifest = ArchiveManifest {
            version: 1,
            principal: principal.into(),
            archive_id: archive_id.into(),
            target: target.clone(),
            content,
            bundle_sha256: format!("{:x}", Sha256::digest(fs::read(&bundle)?)),
        };
        let manifest_bytes =
            serde_json::to_vec(&manifest).map_err(|error| Error::Invalid(error.to_string()))?;
        durable_write(&directory.join("manifest.json"), &manifest_bytes)?;
        durable_write(
            &directory.join("manifest.sha256"),
            format!("{:x}", Sha256::digest(&manifest_bytes)).as_bytes(),
        )?;
        fs::File::open(&directory)?.sync_all()?;
        fs::File::open(&self.directory)?.sync_all()?;
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
        git.authority.authorize(principal, &Intent { target: repository.clone(), operation: format!("restore_archive:{}", serde_json::json!({"archive_id":archive_id,"managed_root":managed_root,"destination":destination})) })?;
        git.verify(repository, cancel)?;
        validate_destination(managed_root, destination)?;
        let directory = self.location(archive_id)?;
        let manifest = self.load_owned(archive_id, principal)?;
        let bundle = directory.join("commits.bundle");
        if format!("{:x}", Sha256::digest(fs::read(&bundle)?)) != manifest.bundle_sha256 {
            return Err(Error::Invalid("archive bundle integrity mismatch".into()));
        }
        let root = checkout_root(repository)?;
        git.run_checked(
            root,
            &strings(&["bundle", "verify", &path_string(&bundle)?]),
            false,
            cancel,
        )?;
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
        let created = git.create_worktree(
            principal,
            &refreshed,
            &WorktreeRequest {
                managed_root: managed_root.into(),
                destination: destination.into(),
                commit: manifest.content.head.clone(),
                include_paths: Vec::new(),
            },
            cancel,
        )?;
        if let Some(error) = created.preparation_error {
            return Err(Error::Invalid(format!(
                "restore checkout retained after materialization failed: {error}"
            )));
        }
        // Remove only tracked baseline files absent from the exact archived inventory.
        // Refuse unexpected files: never recursively delete an unreviewed directory.
        let baseline = capture_content(git, destination, cancel)?;
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
        baseline_directories.sort_by_key(|path| std::cmp::Reverse(path.len()));
        for directory in baseline_directories {
            if !manifest.content.directories.contains(&directory) {
                // remove_dir refuses nonempty directories; there is no recursive fallback.
                fs::remove_dir(destination.join(directory))?;
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
            durable_write(&temporary, &preserved.bytes)?;
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
            crate::worktrees::validate_relative(Path::new(directory))?;
            fs::create_dir_all(destination.join(directory))?;
        }
        // Files are sorted so a symlink can never become an ancestor of a later file.
        for file in &manifest.content.files {
            write_file(destination, file)?;
        }
        if capture_content(git, destination, cancel)? != manifest.content {
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
        fs::File::open(&directory)?.sync_all()?;
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
                operation: "inspect_cleanup".into(),
            },
        )?;
        let manifest = self.load_owned(archive_id, principal)?;
        let changed = target.common_dir != manifest.target.common_dir
            || target.git_dir != manifest.target.git_dir
            || capture_content(git, checkout_root(target)?, cancel)? != manifest.content;
        Ok(CleanupEligibility { eligible: false, changed_since_archive: changed, reason: if changed { "Checkout changed since capture; retain it and create a new archive." } else { "The host has not qualified external-writer quiescence and safe physical removal; checkout retained." }.into() })
    }

    fn load(&self, archive_id: &str) -> Result<ArchiveManifest> {
        let location = self.location(archive_id)?;
        let bytes = fs::read(location.join("manifest.json"))?;
        if format!("{:x}", Sha256::digest(&bytes))
            != fs::read_to_string(location.join("manifest.sha256"))?
        {
            return Err(Error::Invalid("archive manifest integrity mismatch".into()));
        }
        let manifest: ArchiveManifest =
            serde_json::from_slice(&bytes).map_err(|error| Error::Invalid(error.to_string()))?;
        if manifest.version != 1 {
            return Err(Error::Invalid(
                "unsupported archive manifest version".into(),
            ));
        }
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
    ) -> Result<Vec<ArchiveReceipt>> {
        git.authority.authorize(
            principal,
            &Intent {
                target: target.clone(),
                operation: "archive_read".into(),
            },
        )?;
        git.verify(target, cancel)?;
        let mut receipts = Vec::new();
        for entry in fs::read_dir(&self.directory)? {
            let path = entry?.path();
            if !path.is_dir() || !path.join("manifest.json").is_file() {
                continue;
            }
            let manifest: ArchiveManifest =
                serde_json::from_slice(&fs::read(path.join("manifest.json"))?)
                    .map_err(|error| Error::Invalid(error.to_string()))?;
            if manifest.principal == principal && manifest.target.common_dir == target.common_dir {
                let verified = self.load_owned(&manifest.archive_id, principal)?;
                receipts.push(receipt(
                    &verified.archive_id,
                    &verified,
                    has_restoration_proof(&path, &verified)?,
                ));
            }
        }
        receipts.sort_by(|left, right| left.archive_id.cmp(&right.archive_id));
        Ok(receipts)
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
            let bytes = fs::read(entry.path())?;
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
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn capture_content(git: &Git, root: &Path, cancel: &Cancellation) -> Result<ContentManifest> {
    let target = git.discover(root, cancel)?.ok_or(Error::Stale)?;
    let head = target
        .head
        .ok_or_else(|| Error::Invalid("archive requires a committed HEAD".into()))?;
    let mut index = Vec::new();
    for entry in logical_index(git, root, cancel)? {
        let bytes = git.run_checked(
            root,
            &strings(&["cat-file", "blob", &entry.oid]),
            false,
            cancel,
        )?;
        index.push(PreservedIndex { entry, bytes });
    }
    let mut files = Vec::new();
    let mut directories = Vec::new();
    collect_files(root, root, &mut files, &mut directories)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    directories.sort();
    let bytes: usize = files.iter().map(|entry| entry.bytes.len()).sum();
    if bytes > 256 * 1024 * 1024 {
        return Err(Error::Invalid(
            "checkout exceeds this archive profile's total size limit".into(),
        ));
    }
    Ok(ContentManifest {
        head,
        index,
        files,
        directories,
    })
}

fn collect_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<FileEntry>,
    directories: &mut Vec<String>,
) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if directory == root && entry.file_name() == ".git" {
            continue;
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
            directories.push(relative);
            collect_files(root, &path, files, directories)?;
        } else {
            files.push(read_file(root, &relative)?.ok_or(Error::Stale)?);
        }
        if files.len() + directories.len() > 10_000 {
            return Err(Error::Invalid(
                "checkout exceeds this archive profile's inventory limit".into(),
            ));
        }
        if files.iter().map(|entry| entry.bytes.len()).sum::<usize>() > 256 * 1024 * 1024 {
            return Err(Error::Invalid(
                "checkout exceeds this archive profile's total size limit".into(),
            ));
        }
    }
    Ok(())
}
