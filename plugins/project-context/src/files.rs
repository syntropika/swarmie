use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

/// Technical read limits, not model limits or user storage quotas.
#[derive(Clone, Debug)]
pub struct Limits {
    pub max_skills: usize,
    pub max_files: usize,
    pub max_depth: usize,
    pub max_package_bytes: usize,
    pub max_library_bytes: usize,
    pub max_text_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_skills: 256,
            max_files: 2048,
            max_depth: 16,
            max_package_bytes: 16 * 1024 * 1024,
            max_library_bytes: 32 * 1024 * 1024,
            max_text_bytes: 64 * 1024,
        }
    }
}

impl Limits {
    pub(crate) fn validate(&self) -> Result<()> {
        if [
            self.max_skills,
            self.max_files,
            self.max_depth,
            self.max_package_bytes,
            self.max_library_bytes,
            self.max_text_bytes,
        ]
        .contains(&0)
            || self.max_text_bytes > self.max_package_bytes
            || self.max_package_bytes > self.max_library_bytes
        {
            return Err(Error::Invalid("invalid filesystem read limits".into()));
        }
        Ok(())
    }
}

/// An explicit host-authorized traversal boundary. Construction grants no permission.
#[derive(Clone, Debug)]
pub struct Scope {
    root: PathBuf,
    directory: PathBuf,
}

impl Scope {
    pub fn new(root: impl AsRef<Path>, directory: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        let directory = directory.as_ref().canonicalize()?;
        if !root.is_dir() || !directory.is_dir() || !directory.starts_with(&root) {
            return Err(Error::Invalid(
                "working directory is outside the project root".into(),
            ));
        }
        Ok(Self { root, directory })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub(crate) fn ancestors(&self) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for directory in self.directory.ancestors() {
            paths.push(directory.to_path_buf());
            if directory == self.root {
                paths.reverse();
                return Ok(paths);
            }
        }
        Err(Error::Changed)
    }
}

pub(crate) fn path_text(path: &Path) -> Result<&str> {
    path.to_str()
        .filter(|s| !s.chars().any(char::is_control))
        .ok_or_else(|| Error::Invalid("source paths must be UTF-8 without controls".into()))
}

pub(crate) fn read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    if !fs::metadata(path)?.is_file() {
        return Err(Error::Invalid("source is not a regular file".into()));
    }
    let file = fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(Error::Invalid("source is not a regular file".into()));
    }
    let mut bytes = Vec::new();
    file.take(
        u64::try_from(limit)
            .map_err(|_| Error::Limit)?
            .checked_add(1)
            .ok_or(Error::Limit)?,
    )
    .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(Error::Limit);
    }
    Ok(bytes)
}

pub(crate) fn text(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes).map_err(|_| Error::Invalid("source is not UTF-8 text".into()))
}

#[derive(Clone)]
pub(crate) struct Tree {
    pub revision: String,
    pub bytes: usize,
    pub files: BTreeMap<String, Vec<u8>>,
}

impl Tree {
    pub fn capture(root: &Path, limits: &Limits) -> Result<Self> {
        limits.validate()?;
        let mut scan = Scan {
            root,
            limits,
            active: BTreeSet::new(),
            files: BTreeMap::new(),
            bytes: 0,
            entries: 0,
            digest: Sha256::new(),
        };
        scan.walk(root, Path::new(""), 0)?;
        Ok(Self {
            revision: format!("{:x}", scan.digest.finalize()),
            bytes: scan.bytes,
            files: scan.files,
        })
    }
}

struct Scan<'a> {
    root: &'a Path,
    limits: &'a Limits,
    active: BTreeSet<PathBuf>,
    files: BTreeMap<String, Vec<u8>>,
    bytes: usize,
    entries: usize,
    digest: Sha256,
}

impl Scan<'_> {
    fn field(&mut self, value: &[u8]) {
        self.digest.update((value.len() as u128).to_le_bytes());
        self.digest.update(value);
    }
    fn walk(&mut self, path: &Path, relative: &Path, depth: usize) -> Result<()> {
        self.entries = self.entries.checked_add(1).ok_or(Error::Limit)?;
        if depth > self.limits.max_depth || self.entries > self.limits.max_files {
            return Err(Error::Limit);
        }
        let real = path.canonicalize()?;
        if !real.starts_with(self.root) {
            return Err(Error::Invalid("package link escapes its root".into()));
        }
        let metadata = fs::metadata(&real)?;
        let name = relative
            .components()
            .map(|component| {
                let Component::Normal(part) = component else {
                    return Err(Error::Invalid("invalid package path".into()));
                };
                let name = path_text(Path::new(part))?;
                if name.contains('\\') {
                    return Err(Error::Invalid(
                        "package names cannot contain backslashes".into(),
                    ));
                }
                Ok(name)
            })
            .collect::<Result<Vec<_>>>()?
            .join("/");
        self.field(name.as_bytes());
        if fs::symlink_metadata(path)?.file_type().is_symlink() {
            self.field(b"link");
            self.field(path_text(&fs::read_link(path)?)?.as_bytes());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            self.field(&(metadata.permissions().mode() & 0o111).to_le_bytes());
        }
        if metadata.is_dir() {
            if !self.active.insert(real.clone()) {
                return Err(Error::Invalid("cyclic package link".into()));
            }
            self.field(b"directory");
            let mut entries = fs::read_dir(&real)?
                .take(self.limits.max_files.saturating_add(1))
                .collect::<std::io::Result<Vec<_>>>()?;
            if entries.len() > self.limits.max_files {
                return Err(Error::Limit);
            }
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                self.walk(&entry.path(), &relative.join(entry.file_name()), depth + 1)?;
            }
            self.active.remove(&real);
        } else if metadata.is_file() {
            self.field(b"file");
            let remaining = self
                .limits
                .max_package_bytes
                .checked_sub(self.bytes)
                .ok_or(Error::Limit)?;
            let bytes = read(&real, remaining)?;
            self.bytes = self.bytes.checked_add(bytes.len()).ok_or(Error::Limit)?;
            self.field(&bytes);
            self.files.insert(name, bytes);
        } else {
            return Err(Error::Invalid(
                "package contains a non-regular resource".into(),
            ));
        }
        if path.canonicalize()? != real {
            return Err(Error::Changed);
        }
        Ok(())
    }
}

pub(crate) fn resource_name(path: &str) -> Result<String> {
    let value = Path::new(path);
    if path.is_empty()
        || path.contains('\\')
        || value
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(Error::Invalid(
            "resource must be a package-relative path".into(),
        ));
    }
    Ok(path.into())
}
