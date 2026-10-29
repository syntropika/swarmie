use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::{Cancellation, EntryKind, Error, Git, RepositoryTarget, Result, StatusEntry};

pub(crate) const TEXT_LIMIT: usize = 16 * 1024;

/// One owned, verified read. No process-wide cache or retained observation registry.
pub(crate) struct RepositoryObservation {
    pub target: RepositoryTarget,
    pub index: Vec<u8>,
    pub status: Vec<u8>,
}

struct ContentObservation {
    fingerprint: String,
    index_fingerprint: String,
    index: Vec<u8>,
    status: Vec<u8>,
}

pub(crate) fn open_regular_file(path: &Path) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Validate the opened descriptor without waiting for a replacement FIFO.
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(Error::Invalid(
                "file observation does not follow reparse points".into(),
            ));
        }
    }
    if !file.metadata()?.is_file() {
        return Err(Error::Invalid("only regular files can be read".into()));
    }
    Ok(file)
}

pub(crate) fn preview(bytes: &[u8]) -> Result<(String, bool)> {
    let truncated = bytes.len() > TEXT_LIMIT;
    let limited = &bytes[..bytes.len().min(TEXT_LIMIT)];
    match std::str::from_utf8(limited) {
        Ok(text) => Ok((text.to_owned(), truncated)),
        Err(error) if truncated && error.error_len().is_none() => {
            Ok((utf8(&limited[..error.valid_up_to()])?.to_owned(), true))
        }
        Err(_) => Err(Error::Invalid(
            "non-UTF-8 text preview is unsupported".into(),
        )),
    }
}

/// Strip URL credentials and query/fragment data from user-visible transport output.
pub(crate) fn redact(value: &str) -> String {
    let mut result = String::new();
    for (number, token) in value.split_inclusive(char::is_whitespace).enumerate() {
        if number > TEXT_LIMIT {
            break;
        }
        let token = if let Some(scheme) = token.find("://") {
            let authority = scheme + 3;
            let end = token[authority..]
                .find('/')
                .map_or(token.len(), |end| authority + end);
            let mut cleaned = token.to_owned();
            if let Some(at) = token[authority..end].rfind('@') {
                cleaned.replace_range(authority..authority + at + 1, "[redacted]@");
            }
            if let Some(query) = cleaned.find(['?', '#']) {
                cleaned.truncate(query);
            }
            cleaned
        } else {
            token.to_owned()
        };
        result.push_str(&token);
        if result.len() >= TEXT_LIMIT {
            break;
        }
    }
    while result.len() > TEXT_LIMIT {
        result.pop();
    }
    result
}

pub(crate) fn utf8(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes)
        .map_err(|_| Error::Invalid("non-UTF-8 repository paths or output are unsupported".into()))
}

pub(crate) fn text(bytes: &[u8]) -> Result<String> {
    Ok(utf8(bytes)?.trim_end_matches('\n').to_owned())
}

pub(crate) fn path_text(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::Invalid("non-UTF-8 path is unsupported".into()))
}

pub(crate) fn relative_path(value: &str) -> Result<&Path> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains('\0')
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || path.components().any(|part| {
            part.as_os_str()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(".git"))
        })
    {
        return Err(Error::Invalid(
            "path must be a repository-relative file path outside .git".into(),
        ));
    }
    Ok(path)
}

pub(crate) fn cwd(target: &RepositoryTarget) -> &Path {
    Path::new(target.root.as_deref().unwrap_or(&target.git_dir))
}

/// Parse Git's NUL-delimited porcelain v2 format, including rename source records.
pub fn parse_status(bytes: &[u8]) -> Result<Vec<StatusEntry>> {
    let mut records = bytes.split(|byte| *byte == 0);
    let mut entries = Vec::new();
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        let line = utf8(record)?;
        let marker = line.as_bytes()[0];
        if marker == b'#' {
            continue;
        }
        if marker == b'?' || marker == b'!' {
            let path = line
                .get(2..)
                .ok_or_else(|| Error::Invalid("malformed status path".into()))?;
            entries.push(StatusEntry {
                path: path.to_owned(),
                original_path: None,
                index: String::new(),
                worktree: String::new(),
                kind: if marker == b'?' {
                    EntryKind::Untracked
                } else {
                    EntryKind::Ignored
                },
            });
            continue;
        }
        let count = match marker {
            b'1' => 9,
            b'2' => 10,
            b'u' => 11,
            _ => return Err(Error::Invalid("unknown status record".into())),
        };
        let fields = line.splitn(count, ' ').collect::<Vec<_>>();
        if fields.len() != count || fields[1].len() != 2 {
            return Err(Error::Invalid("malformed status record".into()));
        }
        let xy = fields[1].as_bytes();
        let original_path = if marker == b'2' {
            Some(
                utf8(
                    records
                        .next()
                        .ok_or_else(|| Error::Invalid("missing rename source".into()))?,
                )?
                .to_owned(),
            )
        } else {
            None
        };
        entries.push(StatusEntry {
            path: fields[count - 1].to_owned(),
            original_path,
            index: (xy[0] as char).to_string(),
            worktree: (xy[1] as char).to_string(),
            kind: if marker == b'u' {
                EntryKind::Unmerged
            } else {
                EntryKind::Tracked
            },
        });
    }
    Ok(entries)
}

impl Git {
    pub(crate) fn discover(
        &self,
        path: &Path,
        cancellation: &Cancellation,
    ) -> Result<Option<RepositoryTarget>> {
        Ok(self.observe(path, cancellation)?.map(|value| value.target))
    }

    fn observe(
        &self,
        path: &Path,
        cancellation: &Cancellation,
    ) -> Result<Option<RepositoryObservation>> {
        let Some(mut target) = self.discover_identity(path, cancellation)? else {
            return Ok(None);
        };
        let mut index = Vec::new();
        let mut status = Vec::new();
        match self.fingerprint(&target, cancellation) {
            Ok(content) => {
                target.fingerprint = content.fingerprint;
                target.index_fingerprint = content.index_fingerprint;
                index = content.index;
                status = content.status;
            }
            Err(Error::OutputLimit) => {
                target.observation_error = Some(crate::ObservationError::OutputLimit)
            }
            Err(error) => return Err(error),
        }
        Ok(Some(RepositoryObservation {
            target,
            index,
            status,
        }))
    }

    pub(crate) fn observe_target(
        &self,
        target: &RepositoryTarget,
        cancellation: &Cancellation,
    ) -> Result<RepositoryObservation> {
        let observed = self
            .observe(cwd(target), cancellation)?
            .ok_or(Error::Stale)?;
        if observed.target.root != target.root
            || observed.target.git_dir != target.git_dir
            || observed.target.common_dir != target.common_dir
            || observed.target.bare != target.bare
        {
            return Err(Error::Stale);
        }
        if observed.target.observation_error.is_some() {
            return Err(Error::OutputLimit);
        }
        Ok(observed)
    }

    pub(crate) fn discover_identity(
        &self,
        path: &Path,
        cancellation: &Cancellation,
    ) -> Result<Option<RepositoryTarget>> {
        let path = fs::canonicalize(path)?;
        let path = if path.is_file() {
            path.parent()
                .ok_or_else(|| Error::Invalid("file has no parent".into()))?
                .to_owned()
        } else {
            path
        };
        let probe = self.run(
            &path,
            &["rev-parse".into(), "--absolute-git-dir".into()],
            false,
            cancellation,
        )?;
        if probe.status != 0 {
            let error = String::from_utf8_lossy(&probe.stderr);
            if error.contains("not a git repository") {
                return Ok(None);
            }
            return Err(Error::Command {
                status: probe.status,
                message: redact(&error),
            });
        }
        let git_dir = fs::canonicalize(text(&probe.stdout)?)?;
        let bare =
            self.value(&path, &["rev-parse", "--is-bare-repository"], cancellation)? == "true";
        let root = if bare {
            None
        } else {
            Some(path_text(&fs::canonicalize(self.value(
                &path,
                &["rev-parse", "--show-toplevel"],
                cancellation,
            )?)?)?)
        };
        let common_dir = PathBuf::from(self.value(
            &path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            cancellation,
        )?);
        let head = self.optional_value(&path, &["rev-parse", "--verify", "HEAD"], cancellation)?;
        let branch = self.optional_value(
            &path,
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
            cancellation,
        )?;
        let target = RepositoryTarget {
            root,
            git_dir: path_text(&git_dir)?,
            common_dir: path_text(&fs::canonicalize(common_dir)?)?,
            bare,
            head,
            branch,
            fingerprint: String::new(),
            index_fingerprint: String::new(),
            observation_error: None,
        };
        Ok(Some(target))
    }

    pub(crate) fn verify_identity(
        &self,
        target: &RepositoryTarget,
        cancellation: &Cancellation,
    ) -> Result<()> {
        let current = self
            .discover_identity(cwd(target), cancellation)?
            .ok_or(Error::Stale)?;
        if current.root != target.root
            || current.git_dir != target.git_dir
            || current.common_dir != target.common_dir
            || current.bare != target.bare
        {
            return Err(Error::Stale);
        }
        Ok(())
    }

    pub(crate) fn verify(
        &self,
        target: &RepositoryTarget,
        cancellation: &Cancellation,
    ) -> Result<RepositoryTarget> {
        if target.observation_error.is_some()
            || target.fingerprint.is_empty()
            || target.index_fingerprint.is_empty()
        {
            return Err(Error::OutputLimit);
        }
        let current = self
            .discover(cwd(target), cancellation)?
            .ok_or(Error::Stale)?;
        if current.observation_error.is_some() {
            return Err(Error::OutputLimit);
        }
        if &current != target {
            return Err(Error::Stale);
        }
        Ok(current)
    }

    pub(crate) fn value(
        &self,
        path: &Path,
        args: &[&str],
        cancellation: &Cancellation,
    ) -> Result<String> {
        text(&self.run_checked(
            path,
            &args.iter().map(|value| (*value).into()).collect::<Vec<_>>(),
            false,
            cancellation,
        )?)
    }

    pub(crate) fn optional_value(
        &self,
        path: &Path,
        args: &[&str],
        cancellation: &Cancellation,
    ) -> Result<Option<String>> {
        let output = self.run(
            path,
            &args.iter().map(|value| (*value).into()).collect::<Vec<_>>(),
            false,
            cancellation,
        )?;
        if output.truncated {
            return Err(Error::OutputLimit);
        }
        if output.status == 0 {
            Ok(Some(text(&output.stdout)?))
        } else if output.status == 1 || output.status == 128 {
            Ok(None)
        } else {
            Err(Error::Command {
                status: output.status,
                message: redact(&String::from_utf8_lossy(&output.stderr)),
            })
        }
    }

    pub(crate) fn status_bytes(
        &self,
        target: &RepositoryTarget,
        cancellation: &Cancellation,
    ) -> Result<Vec<u8>> {
        if target.bare {
            return Ok(Vec::new());
        }
        self.run_checked(
            cwd(target),
            &[
                "status",
                "--porcelain=v2",
                "-z",
                "--untracked-files=all",
                "--ignored=matching",
            ]
            .map(str::to_owned),
            false,
            cancellation,
        )
    }

    fn fingerprint(
        &self,
        target: &RepositoryTarget,
        cancellation: &Cancellation,
    ) -> Result<ContentObservation> {
        let mut hash = Sha256::new();
        hash.update(target.git_dir.as_bytes());
        hash.update(target.common_dir.as_bytes());
        hash.update(target.head.as_deref().unwrap_or("").as_bytes());
        hash.update(target.branch.as_deref().unwrap_or("").as_bytes());
        let index = self.run_checked(
            cwd(target),
            &["ls-files", "--stage", "-z"].map(str::to_owned),
            false,
            cancellation,
        )?;
        let index_fingerprint = format!("{:x}", Sha256::digest(&index));
        hash.update(&index);
        // Remote routing and native behavior configuration are part of the captured intent.
        let config = self.run_checked(
            cwd(target),
            &["config", "--null", "--list"].map(str::to_owned),
            false,
            cancellation,
        )?;
        hash.update(config);
        let status = self.status_bytes(target, cancellation)?;
        hash.update(&status);
        if let Some(root) = &target.root {
            for entry in parse_status(&status)? {
                if matches!(entry.kind, EntryKind::Ignored) {
                    continue;
                }
                relative_path(&entry.path)?;
                hash.update(entry.path.as_bytes());
                let path = Path::new(root).join(&entry.path);
                let metadata = match fs::symlink_metadata(&path) {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error.into()),
                };
                if metadata.file_type().is_symlink() {
                    hash.update(path_text(&fs::read_link(path)?)?.as_bytes());
                } else if metadata.is_file() {
                    if cancellation.is_cancelled() {
                        return Err(Error::Cancelled);
                    }
                    let mut file = open_regular_file(&path)?;
                    let mut buffer = [0; 65536];
                    loop {
                        if cancellation.is_cancelled() {
                            return Err(Error::Cancelled);
                        }
                        let size = file.read(&mut buffer)?;
                        if size == 0 {
                            break;
                        }
                        hash.update(&buffer[..size]);
                    }
                }
            }
        }
        // A raced index or status observation must not yield an apparently stable token.
        let second_index = self.run_checked(
            cwd(target),
            &["ls-files", "--stage", "-z"].map(str::to_owned),
            false,
            cancellation,
        )?;
        if index != second_index || status != self.status_bytes(target, cancellation)? {
            return Err(Error::Stale);
        }
        Ok(ContentObservation {
            fingerprint: format!("{:x}", hash.finalize()),
            index_fingerprint,
            index,
            status,
        })
    }
}

#[cfg(all(test, unix))]
mod file_tests {
    #[test]
    fn descriptor_validation_never_waits_on_a_replacement_fifo() {
        use std::{
            fs::OpenOptions, os::unix::fs::OpenOptionsExt, process::Command, sync::mpsc,
            time::Duration,
        };
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("replacement");
        assert!(
            Command::new("mkfifo")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        let captured = path.clone();
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            sender.send(super::open_regular_file(&captured)).unwrap();
        });
        let result = receiver.recv_timeout(Duration::from_secs(2));
        if result.is_err() {
            let _writer = OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(path);
        }
        worker.join().unwrap();
        assert!(matches!(
            result.expect("open must settle without a writer"),
            Err(crate::Error::Invalid(_))
        ));
    }
}
