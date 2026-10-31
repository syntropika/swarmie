//! The [`Workspace`] the tools act on: the disk they read and write and
//! where their commands run. [`Local`] is this machine; another, such as
//! another backend supplied by the host, is a type of its
//! own that every tool and the shell then go through.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read as _};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use crate::{ContentHash, ToolExecutionError, content_hash};
use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;

use crate::coding::fs::{Previous, io_error, write_atomic};
use crate::coding::process::{Launcher, Process, Spawn};
use crate::coding::{MAX_BYTES, MAX_FILE_BYTES, shell_path};

/// The disk and the commands the tools act on: this machine's ([`Local`]),
/// or another, such as one reached over SSH. Paths are the workspace's own:
/// a tool's base, the paths it resolves and the facts it publishes
/// ([`FilesRead`](crate::FilesRead),
/// [`FilesChanged`](crate::FilesChanged)) name files in it.
///
/// Synchronous, as the rest of swarmie-tools: the tools call it inside
/// [`blocking`](crate::coding::blocking). Code on a thread that must not wait, such
/// as an app's main thread, calls it inline only when [`host`](Self::host)
/// is `None`, and otherwise off that thread.
///
/// A workspace of your own implements every method; [`Local`] is the
/// reference for what each does.
pub trait Workspace: Send + Sync + fmt::Debug + 'static {
    /// Where it is when it is not this machine, such as `ssh://dev@box`;
    /// `None` for this machine.
    fn host(&self) -> Option<&str> {
        None
    }

    /// What `path` is, its links followed: a file (its size, whether it is
    /// read-only, when it was last changed), a directory, something else, or
    /// nothing.
    fn stat(&self, path: &Path) -> io::Result<Stat>;

    /// Up to `limit` bytes of the regular file `path`, and its whole size.
    /// Anything but a regular file is refused without being opened: opening
    /// a FIFO or a device can block forever.
    fn read(&self, path: &Path, limit: u64) -> io::Result<Contents>;

    /// The whole content of each of `paths` that is a regular file of at
    /// most `limit` bytes, in order: `None` for any other path, and for one
    /// that could not be read. What a search reads; a workspace where each
    /// call costs a round trip, such as a remote one, reads them in one.
    fn read_files(&self, paths: &[PathBuf], limit: u64) -> Vec<Option<Vec<u8>>> {
        paths
            .iter()
            .map(|path| read_file(self, path, limit))
            .collect()
    }

    /// Lines `first` (from 1) on, at most `count` of them, of the regular
    /// text file `path`, with how many lines it has and the hash of all of
    /// it. Refused, as `read_text` refuses,
    /// when it is no regular file, larger than [`MAX_FILE_BYTES`] (a
    /// [`FileTooLarge`](ErrorKind::FileTooLarge) error) or not UTF-8 (an
    /// [`InvalidData`](ErrorKind::InvalidData) error); a workspace that
    /// sends only the lines asked of a file larger than
    /// [`MAX_BYTES`], such as a remote one, checks only those for UTF-8.
    ///
    /// By default the whole file is [`read`](Self::read) and cut here.
    fn excerpt(&self, path: &Path, first: usize, count: usize) -> io::Result<Excerpt> {
        excerpt_of(self, path, first, count)
    }

    /// Replaces the file `path` with `bytes` in one step, creating its
    /// directory, keeping its permissions, and refusing a read-only file or
    /// anything but a regular file (see [`write_atomic`]).
    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;

    /// [`write`](Self::write)s `bytes` to `path` and says what the file
    /// held before, as [`read`](Self::read) with `limit` reads it: a
    /// [`NotFound`](ErrorKind::NotFound) error when there was no file, the
    /// file's own when it could not be read. What a tool that writes a
    /// file diffs against; a workspace where each call costs a round trip,
    /// such as a remote one, does both in one.
    ///
    /// By default it reads, then writes.
    fn replace(&self, path: &Path, bytes: &[u8], limit: u64) -> io::Result<io::Result<Contents>> {
        let before = self.read(path, limit);
        self.write(path, bytes)?;
        Ok(before)
    }

    /// [`write`](Self::write)s `bytes` to the regular file `path` only
    /// while it holds the text whose [`content_hash`] is `held`: `None`
    /// when it did. Otherwise nothing is written and it says what the file
    /// holds instead, as [`read`](Self::read) with a limit of
    /// [`MAX_FILE_BYTES`] reads it. What an edit made from text seen
    /// before writes with; a workspace where each call costs a round trip,
    /// such as a remote one, checks and writes in one.
    ///
    /// By default it reads, then writes.
    fn write_if(
        &self,
        path: &Path,
        bytes: &[u8],
        held: &ContentHash,
    ) -> io::Result<Option<Contents>> {
        let now = self.read(path, MAX_FILE_BYTES)?;
        if now.size > MAX_FILE_BYTES || content_hash(&now.bytes) != *held {
            return Ok(Some(now));
        }
        self.write(path, bytes)?;
        Ok(None)
    }

    /// Removes the file `path`; a directory is refused.
    fn remove(&self, path: &Path) -> io::Result<()>;

    /// Moves the file `from` to `to`, creating `to`'s directory and
    /// replacing a file there.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;

    /// `path` made absolute, with the links of its longest existing part
    /// followed (the file's own too, when it exists) and `..` folded, so two
    /// spellings of one file come out alike. A footprint is made with it,
    /// which may be on a thread that must not wait: a remote workspace may
    /// answer from a cache, or fold `..` only.
    fn canonical(&self, path: &Path) -> PathBuf;

    /// `path` relative to `base`, as `resolve` joins them:
    /// `base.join(path)`, so an absolute `path` stays as it is. A workspace
    /// whose paths can name another machine, such as
    /// a workspace adapter, keeps such a path whole too. A
    /// workspace with a home directory reads a first component `~` as that
    /// home: [`Local`] does for `~`, `~/notes` and, on Windows, `~\notes`,
    /// but not for `~bob` or `./~`.
    fn join(&self, base: &Path, path: &Path) -> PathBuf {
        base.join(path)
    }

    /// The regular files under `root` (or `root` itself when it is a file),
    /// in walk order, as `root` joined with their path below it: those
    /// `listing` keeps ([`Listing`]), and only those matching `glob`, such
    /// as `*.rs`, when it is given. A malformed glob is an
    /// [`InvalidInput`](ErrorKind::InvalidInput) error that says so.
    fn files(&self, root: &Path, glob: Option<&str>, listing: Listing) -> io::Result<Vec<PathBuf>>;

    /// Starts `program` with `args` in `dir` (`None`: the launcher's own
    /// directory) under `launcher`'s policy: its wrapper and variables.
    fn spawn(
        &self,
        launcher: &Launcher,
        dir: Option<&Path>,
        program: &OsStr,
        args: &[OsString],
        how: Spawn,
    ) -> io::Result<Process>;
}

/// Which files [`Workspace::files`] lists, and a watch reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Listing {
    /// What a person browsing the project sees, as ripgrep lists it: no
    /// file the ignore rules (`.gitignore`, `.ignore`, the git excludes)
    /// leave out, no hidden file or directory (named with a leading `.`),
    /// no `target` directory.
    #[default]
    Visible,
    /// Every file the ignore rules keep, hidden ones such as `.github/` and
    /// `.env.example` included; `.git` left out.
    Hidden,
    /// Every regular file.
    All,
}

impl Listing {
    /// The listing of a tool call's `hidden` and `glob` arguments.
    pub(crate) fn from_options(hidden: bool, glob: Option<&str>) -> Self {
        match hidden {
            true => Self::Hidden,
            false => Self::Visible,
        }
        .for_glob(glob)
    }

    /// The listing a search or listing for `glob` uses: `Visible` becomes
    /// `Hidden` when the glob names a hidden file or directory, such as
    /// `.github/**` or `**/.env*`, so asking for one finds it.
    pub fn for_glob(self, glob: Option<&str>) -> Self {
        let names_hidden = glob.is_some_and(|glob| {
            glob.split(['/', '\\'])
                .any(|part| part.starts_with('.') && part != "." && part != "..")
        });
        match self {
            Self::Visible if names_hidden => Self::Hidden,
            other => other,
        }
    }

    /// Whether this listing keeps `below`, a path below the listed root, by
    /// its names alone (the ignore rules aside): `Visible` none of whose
    /// parts is hidden or `target`, `Hidden` none of whose parts is `.git`.
    pub fn keeps(self, below: &Path) -> bool {
        let mut parts = below.iter();
        match self {
            Self::Visible => !parts.any(|part| part == "target" || is_hidden(Path::new(part))),
            Self::Hidden => !parts.any(|part| part == ".git"),
            Self::All => true,
        }
    }
}

/// What [`Workspace::read`] read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Contents {
    /// At most the asked number of bytes, from the file's start.
    pub bytes: Vec<u8>,
    /// The file's whole size, more than `bytes` holds when it was cut.
    pub size: u64,
}

/// What [`Workspace::excerpt`] read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Excerpt {
    /// The lines asked, each with its line end as in the file.
    pub text: String,
    /// How many lines the file has, as [`str::lines`] counts them.
    pub total: usize,
    /// The hash of the whole file.
    pub hash: ContentHash,
    /// The whole file's text, when it is at most [`MAX_BYTES`]: sending
    /// that much costs about what a call does, and an edit after the read
    /// starts from it instead of fetching the file.
    pub whole: Option<String>,
}

impl Excerpt {
    /// Lines `first` (from 1) on, at most `count` of them, of `text`, the
    /// whole file, kept whole too when it is small.
    pub fn of(text: &str, first: usize, count: usize) -> Self {
        let start = text
            .split_inclusive('\n')
            .take(first.saturating_sub(1))
            .map(str::len)
            .sum();
        let rest = text.get(start..).unwrap_or_default();
        let len = rest.split_inclusive('\n').take(count).map(str::len).sum();
        Self {
            text: rest.get(..len).unwrap_or_default().to_owned(),
            total: text.lines().count(),
            hash: content_hash(text.as_bytes()),
            whole: (text.len() <= MAX_BYTES).then(|| text.to_owned()),
        }
    }
}

/// What a path is, as [`Workspace::stat`] says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stat {
    /// A regular file.
    File {
        /// Its size in bytes.
        size: u64,
        /// Whether it may not be written.
        read_only: bool,
        /// When its content last changed, when the workspace can tell:
        /// how a scan or a polling watch sees an edit that kept the size.
        modified: Option<SystemTime>,
    },
    /// A directory.
    Dir,
    /// Something else, such as a FIFO or a device: the tools neither read
    /// nor write it.
    Other,
    /// Nothing is there.
    Missing,
}

impl dyn Workspace {
    /// `path` as the file tools name it: [`join`](Workspace::join)ed onto
    /// `base` (`None`: the working directory), then made
    /// [`canonical`](Workspace::canonical).
    pub fn resolve(&self, base: Option<&Path>, path: &str) -> PathBuf {
        self.canonical(&self.join(base.unwrap_or(Path::new("")), path.as_ref()))
    }

    /// `path` `resolve`d against `base`, as a file tool
    /// opens it; with `confine`, refused before any other I/O when it lies
    /// outside `base` (`None`: the working directory).
    ///
    /// ```
    /// use swarmie_tools::coding::workspace::{Local, Workspace};
    ///
    /// let local: &dyn Workspace = &Local;
    /// let base = std::fs::canonicalize(".")?;
    /// assert_eq!(local.place(Some(&base), true, "src/../a.rs")?, base.join("a.rs"));
    /// assert!(local.place(Some(&base), true, "../outside.rs").is_err());
    /// assert!(local.place(Some(&base), false, "../outside.rs").is_ok());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn place(
        &self,
        base: Option<&Path>,
        confine: bool,
        path: &str,
    ) -> Result<PathBuf, ToolExecutionError> {
        let resolved = self.resolve(base, path);
        if confine {
            let root = self.resolve(base, ".");
            if !resolved.starts_with(&root) {
                return Err(ToolExecutionError::permission_denied(format!(
                    "{path} is outside {}, the only directory this tool works in",
                    root.display()
                )));
            }
        }
        Ok(resolved)
    }

    /// The regular file `path` as text, in one [`read`](Workspace::read):
    /// refused when it is no regular file or larger than
    /// [`MAX_FILE_BYTES`].
    pub fn read_text(&self, path: &Path) -> Result<String, ToolExecutionError> {
        let shown = path.to_string_lossy();
        self.read(path, MAX_FILE_BYTES)
            .and_then(text_of)
            .map_err(|error| text_error(&shown, error))
    }

    /// What the file `path` holds before a tool changes it, in one
    /// [`read`](Workspace::read): `None` when there is none; empty, with
    /// only its size, when it is too large to read; empty, with no state,
    /// when it is no regular file or could not be read.
    pub(crate) fn previous(&self, path: &Path) -> Option<Previous> {
        Previous::read(self.read(path, MAX_FILE_BYTES))
    }
}

/// The whole text of a file read with a limit of [`MAX_FILE_BYTES`]:
/// refused when it is larger or not UTF-8.
pub(crate) fn text_of(contents: Contents) -> io::Result<String> {
    if contents.size > MAX_FILE_BYTES {
        return Err(too_large(contents.size));
    }
    String::from_utf8(contents.bytes).map_err(|_| not_text())
}

/// The error for a file of `size` bytes, more than [`MAX_FILE_BYTES`].
pub(crate) fn too_large(size: u64) -> io::Error {
    io::Error::new(
        ErrorKind::FileTooLarge,
        format!(
            "{} MB; the tools open files up to {} MB",
            size / (1024 * 1024),
            MAX_FILE_BYTES / (1024 * 1024)
        ),
    )
}

/// The error for bytes that are not UTF-8 text.
pub(crate) fn not_text() -> io::Error {
    io::Error::new(ErrorKind::InvalidData, "stream did not contain valid UTF-8")
}

/// A model-visible error for a failed text read of `shown`
/// (`read_text`, [`Workspace::excerpt`]).
pub(crate) fn text_error(shown: &str, error: io::Error) -> ToolExecutionError {
    match error.kind() {
        ErrorKind::NotFound => ToolExecutionError::not_found(format!("{shown}: no such file")),
        ErrorKind::InvalidInput | ErrorKind::IsADirectory => {
            ToolExecutionError::invalid_args(format!("{shown} is not a regular file"))
        }
        ErrorKind::FileTooLarge => ToolExecutionError::invalid_args(format!("{shown} is {error}")),
        ErrorKind::InvalidData => ToolExecutionError::invalid_args(match shell_path(shown) {
            Some(path) => format!(
                "{shown} is not a text file (not UTF-8); `file {path}` in `shell` shows what it is"
            ),
            None => format!("{shown} is not a text file (not UTF-8)"),
        }),
        _ => io_error(shown, error),
    }
}

/// [`Workspace::excerpt`] as it is by default: the whole file read and cut
/// here.
pub(crate) fn excerpt_of<W: Workspace + ?Sized>(
    workspace: &W,
    path: &Path,
    first: usize,
    count: usize,
) -> io::Result<Excerpt> {
    let text = text_of(workspace.read(path, MAX_FILE_BYTES)?)?;
    Ok(Excerpt::of(&text, first, count))
}

/// `path`'s whole content when it is a regular file of at most `limit`
/// bytes, as [`Workspace::read_files`] reads each by default.
pub(crate) fn read_file<W: Workspace + ?Sized>(
    workspace: &W,
    path: &Path,
    limit: u64,
) -> Option<Vec<u8>> {
    let small = matches!(workspace.stat(path), Ok(Stat::File { size, .. }) if size <= limit);
    small
        .then(|| workspace.read(path, limit).ok())
        .flatten()
        .map(|contents| contents.bytes)
}

/// `path` in this machine's home directory when its first component is
/// `~`: `None` for any other path, and when the home is unknown.
pub(crate) fn home_path(path: &Path) -> Option<PathBuf> {
    let rest = path.strip_prefix("~").ok()?;
    let home = std::env::home_dir()?;
    Some(match rest.as_os_str().is_empty() {
        true => home,
        false => home.join(rest),
    })
}

/// This machine: its file system and its processes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Local;

impl Workspace for Local {
    fn stat(&self, path: &Path) -> io::Result<Stat> {
        match fs::metadata(path) {
            Ok(meta) if meta.is_file() => Ok(Stat::File {
                size: meta.len(),
                read_only: meta.permissions().readonly(),
                modified: meta.modified().ok(),
            }),
            Ok(meta) if meta.is_dir() => Ok(Stat::Dir),
            Ok(_) => Ok(Stat::Other),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Stat::Missing),
            Err(error) => Err(error),
        }
    }

    fn read(&self, path: &Path, limit: u64) -> io::Result<Contents> {
        if !fs::metadata(path)?.is_file() {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "not a regular file",
            ));
        }
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        let mut bytes = Vec::new();
        file.take(limit).read_to_end(&mut bytes)?;
        Ok(Contents { bytes, size })
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        write_atomic(path, bytes)
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        if let Some(parent) = to.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        match fs::rename(from, to) {
            Err(error) if error.kind() == ErrorKind::CrossesDevices => {
                fs::copy(from, to)?;
                fs::remove_file(from)
            }
            moved => moved,
        }
    }

    /// `~` is this machine's home directory, when it is known.
    fn join(&self, base: &Path, path: &Path) -> PathBuf {
        home_path(path).unwrap_or_else(|| base.join(path))
    }

    fn canonical(&self, path: &Path) -> PathBuf {
        let joined = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let real = joined.ancestors().find_map(|ancestor| {
            let real = fs::canonicalize(ancestor).ok()?;
            Some((real, joined.strip_prefix(ancestor).ok()?))
        });
        let (mut out, rest) = real.unwrap_or((PathBuf::new(), &joined));
        for component in rest.components() {
            match component {
                Component::ParentDir => {
                    out.pop();
                }
                Component::CurDir => {}
                other => out.push(other),
            }
        }
        out
    }

    fn files(&self, root: &Path, glob: Option<&str>, listing: Listing) -> io::Result<Vec<PathBuf>> {
        let mut walk = WalkBuilder::new(root);
        match listing {
            Listing::All => walk.standard_filters(false),
            Listing::Visible | Listing::Hidden => walk.hidden(false).filter_entry(move |entry| {
                entry.depth() == 0 || listing.keeps(Path::new(entry.file_name()))
            }),
        };
        if let Some(glob) = glob {
            let overrides = OverrideBuilder::new(root)
                .add(glob)
                .and_then(|builder| builder.build())
                .map_err(|error| {
                    io::Error::new(ErrorKind::InvalidInput, format!("bad glob: {error}"))
                })?;
            walk.overrides(overrides);
        }
        // Only regular files: symbolic links are not followed.
        Ok(walk
            .build()
            .flatten()
            .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
            .map(ignore::DirEntry::into_path)
            .collect())
    }

    fn spawn(
        &self,
        launcher: &Launcher,
        dir: Option<&Path>,
        program: &OsStr,
        args: &[OsString],
        how: Spawn,
    ) -> io::Result<Process> {
        match dir {
            Some(dir) => launcher.clone().in_dir(dir).spawn(program, args, how),
            None => launcher.spawn(program, args, how),
        }
    }
}

/// Whether `path` is hidden: its name starts with a `.`, such as `.env` or
/// `.github/`.
pub(crate) fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.as_encoded_bytes().starts_with(b"."))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;
    use std::ffi::{OsStr, OsString};
    use std::io::{self, ErrorKind};
    use std::path::{Component, Path, PathBuf};
    use std::sync::{Arc, Mutex, PoisonError};

    use crate::{
        ChangeKind, ContentHash, FilesChanged, Tool, ToolContext, ToolOutput, content_hash,
    };

    use super::{Contents, Listing, Stat, Workspace};
    use crate::coding::process::{Launcher, Process, Spawn};
    use crate::coding::{
        Delete, DeleteArgs, Edit, EditArgs, Move, MoveArgs, Read, ReadArgs, Replacement, Search,
        SearchArgs, Write, WriteArgs,
    };

    /// Files held in memory, by absolute path: a workspace that is not this
    /// machine's disk.
    #[derive(Debug, Default)]
    pub(crate) struct Memory(Mutex<BTreeMap<PathBuf, Vec<u8>>>);

    impl Memory {
        pub(crate) fn files(&self) -> std::sync::MutexGuard<'_, BTreeMap<PathBuf, Vec<u8>>> {
            self.0.lock().unwrap_or_else(PoisonError::into_inner)
        }
    }

    impl Workspace for Memory {
        fn host(&self) -> Option<&str> {
            Some("memory://")
        }

        fn stat(&self, path: &Path) -> io::Result<Stat> {
            let files = self.files();
            Ok(match files.get(path) {
                Some(bytes) => Stat::File {
                    size: bytes.len() as u64,
                    read_only: false,
                    modified: None,
                },
                None if files.keys().any(|file| file.starts_with(path)) => Stat::Dir,
                None => Stat::Missing,
            })
        }

        fn read(&self, path: &Path, limit: u64) -> io::Result<Contents> {
            let files = self.files();
            let bytes = files.get(path).ok_or(ErrorKind::NotFound)?;
            Ok(Contents {
                bytes: bytes.iter().copied().take(limit as usize).collect(),
                size: bytes.len() as u64,
            })
        }

        fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
            self.files().insert(path.to_path_buf(), bytes.to_vec());
            Ok(())
        }

        fn remove(&self, path: &Path) -> io::Result<()> {
            self.files()
                .remove(path)
                .map(drop)
                .ok_or(ErrorKind::NotFound.into())
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            let mut files = self.files();
            let bytes = files.remove(from).ok_or(ErrorKind::NotFound)?;
            files.insert(to.to_path_buf(), bytes);
            Ok(())
        }

        fn canonical(&self, path: &Path) -> PathBuf {
            let mut out = PathBuf::from("/");
            for component in path.components() {
                match component {
                    Component::ParentDir => {
                        out.pop();
                    }
                    Component::Normal(part) => out.push(part),
                    _ => {}
                }
            }
            out
        }

        fn files(&self, root: &Path, glob: Option<&str>, _: Listing) -> io::Result<Vec<PathBuf>> {
            let suffix = glob.map(|glob| glob.trim_start_matches('*'));
            Ok(self
                .files()
                .keys()
                .filter(|path| path.starts_with(root))
                .filter(|path| suffix.is_none_or(|suffix| path.to_string_lossy().ends_with(suffix)))
                .cloned()
                .collect())
        }

        fn spawn(
            &self,
            _: &Launcher,
            _: Option<&Path>,
            _: &OsStr,
            _: &[OsString],
            _: Spawn,
        ) -> io::Result<Process> {
            Err(ErrorKind::Unsupported.into())
        }
    }

    fn call<T: Tool<Error = crate::ToolExecutionError>>(
        tool: &T,
        args: T::Args,
    ) -> (Result<T::Output, crate::ToolExecutionError>, ToolContext) {
        let mut context = ToolContext::new();
        let output = futures::executor::block_on(tool.call(&mut context, args));
        (output, context)
    }

    fn changes(context: &ToolContext) -> Vec<(String, ChangeKind)> {
        let changed: FilesChanged = context.result().ok().flatten().unwrap_or_default();
        changed
            .0
            .into_iter()
            .map(|change| (change.path, change.kind))
            .collect()
    }

    /// The content each changed file held before the call, by its hash.
    fn befores(context: &ToolContext) -> Vec<Option<ContentHash>> {
        let changed: FilesChanged = context.result().ok().flatten().unwrap_or_default();
        changed
            .0
            .into_iter()
            .map(|change| change.before.and_then(|it| it.hash))
            .collect()
    }

    /// Every file tool acts on its workspace, here files in memory: what one
    /// writes the next reads, edits, finds, moves and deletes, each change
    /// naming the content it was made to, and nothing reaches this machine's
    /// disk.
    #[test]
    fn every_file_tool_goes_through_its_workspace() {
        let memory = Arc::new(Memory::default());
        let workspace: Arc<dyn Workspace> = memory.clone();
        let base = Some(PathBuf::from("/project"));
        let (read, write, edit, search, delete, moved) = (
            Read {
                workspace: workspace.clone(),
                base: base.clone(),
                confine: true,
                seen: crate::coding::Seen::default(),
            },
            Write {
                workspace: workspace.clone(),
                base: base.clone(),
                confine: true,
                seen: Default::default(),
            },
            Edit {
                workspace: workspace.clone(),
                base: base.clone(),
                confine: true,
                seen: crate::coding::Seen::default(),
            },
            Search {
                workspace: workspace.clone(),
                base: base.clone(),
                confine: true,
            },
            Delete {
                workspace: workspace.clone(),
                base: base.clone(),
                confine: true,
            },
            Move {
                workspace,
                base,
                confine: true,
            },
        );

        let (written, context) = call(&write, WriteArgs::new("src/a.rs", "fn one() {}\n"));
        assert!(written.is_ok(), "{written:?}");
        assert_eq!(
            changes(&context),
            [(
                Path::new("/project")
                    .join("src")
                    .join("a.rs")
                    .display()
                    .to_string(),
                ChangeKind::Created
            )]
        );
        assert!(!Path::new("/project/src/a.rs").exists());

        let (edited, context) = call(
            &edit,
            EditArgs::new("src/a.rs", [Replacement::new("one", "two")]),
        );
        assert!(edited.is_ok(), "{edited:?}");
        assert_eq!(befores(&context), [Some(content_hash(b"fn one() {}\n"))]);
        let (shown, _) = call(&read, ReadArgs::new("src/../src/a.rs"));
        assert_eq!(
            shown.ok().as_ref().and_then(ToolOutput::as_text),
            Some("fn two() {}\n")
        );
        let (found, _) = call(&search, SearchArgs::new("two").glob("*.rs"));
        assert_eq!(
            found.ok(),
            Some(format!(
                "{}:1: fn two() {{}}",
                Path::new("src").join("a.rs").display()
            ))
        );

        let (refused, _) = call(&moved, MoveArgs::new("src/a.rs", "../b.rs"));
        assert!(refused.is_err());
        let (renamed, context) = call(&moved, MoveArgs::new("src/a.rs", "src/b.rs"));
        assert!(renamed.is_ok(), "{renamed:?}");
        assert_eq!(
            changes(&context),
            [
                (
                    Path::new("/project")
                        .join("src")
                        .join("a.rs")
                        .display()
                        .to_string(),
                    ChangeKind::Deleted
                ),
                (
                    Path::new("/project")
                        .join("src")
                        .join("b.rs")
                        .display()
                        .to_string(),
                    ChangeKind::Created
                )
            ]
        );
        assert_eq!(
            befores(&context),
            [Some(content_hash(b"fn two() {}\n")), None]
        );
        let (deleted, context) = call(&delete, DeleteArgs::new("src/b.rs"));
        assert!(deleted.is_ok(), "{deleted:?}");
        assert_eq!(befores(&context), [Some(content_hash(b"fn two() {}\n"))]);
        assert_eq!(
            changes(&context),
            [(
                Path::new("/project")
                    .join("src")
                    .join("b.rs")
                    .display()
                    .to_string(),
                ChangeKind::Deleted
            )]
        );
        assert!(memory.files().is_empty());
    }

    /// Only a first component `~` is the home directory: `~bob`, a file whose
    /// name starts with `~` and a folder named `~` reached as `./~` or below
    /// another stay where they are.
    #[test]
    fn only_a_leading_tilde_is_the_home_directory() {
        let base = Path::new("/project");
        for path in ["~bob/x", "~$report.docx", "./~/x", "a/~/x"] {
            assert_eq!(
                super::Local.join(base, Path::new(path)),
                base.join(path),
                "{path}"
            );
        }
    }

    /// Tilde expansion uses the current home without changing the environment
    /// or reading or writing user files. Confined paths still stay in the base.
    #[test]
    fn tilde_is_the_home_directory() {
        let local: &dyn Workspace = &super::Local;
        let home = std::env::home_dir().expect("test process has a home directory");
        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(dir.path()).unwrap();
        assert_eq!(local.join(&base, Path::new("~/notes")), home.join("notes"));
        assert!(local.place(Some(&base), true, "~/notes").is_err());
        assert_eq!(
            local.place(Some(&base), false, "~/notes").unwrap(),
            local.canonical(&home.join("notes"))
        );
        std::fs::create_dir(base.join("~")).unwrap();
        assert_eq!(
            local.place(Some(&base), true, "./~/notes").unwrap(),
            base.join("~/notes")
        );
    }
}
