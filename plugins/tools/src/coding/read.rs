//! The `read` tool.

use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use crate::{
    ContentHash, FileRead, FilesRead, KeptEnd, LinesShown, OutputCut, Scope, Tool, ToolContext,
    ToolExecutionError, ToolOutput, Touches, args_schema, content_hash,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::coding::workspace::{Local, Workspace, text_error};
use crate::coding::{
    CUT_MARK, MAX_BYTES, MAX_FILE_BYTES, MAX_LINE_CHARS, MAX_LINES, blocking, clip, shell_path,
};

/// Reads a text file's lines as they are. Publishes the file it
/// read ([`FilesRead`]), by its [resolved](Workspace) path, with the hash
/// of the bytes it read and, for a text file, the lines it showed, and,
/// when lines after those were left out, [`OutputCut`].
#[derive(Clone, Debug)]
pub struct Read {
    /// Where the file is read.
    pub workspace: Arc<dyn Workspace>,
    /// The directory a relative path is read from; `None` is the
    /// workspace's working directory.
    pub base: Option<PathBuf>,
    /// Whether a path that resolves outside `base` is refused, before
    /// anything is read.
    pub confine: bool,
    /// Where each file read is noted, for an [`Edit`](crate::coding::Edit) given
    /// the same to tell whether a file changed since.
    pub seen: Seen,
}

impl Default for Read {
    /// Reads this machine's files, from this process's working directory.
    fn default() -> Self {
        Self {
            workspace: Arc::new(Local),
            base: None,
            confine: false,
            seen: Seen::default(),
        }
    }
}

/// Most bytes of file text a [`Seen`] keeps, for the files seen last.
const SEEN_TEXT_BYTES: usize = 8 * 1024 * 1024;

/// The hash ([`content_hash`]) of each file's text as [`Read`] last showed
/// it or [`Edit`](crate::coding::Edit) or [`Write`](crate::coding::Write) last left it, by
/// resolved path; shared by its clones. With it, an edit whose `old_text`
/// is not found says whether the file changed since, and so needs reading
/// again.
///
/// It also keeps the whole text of the files last seen whole, up to 8 MB
/// of them, so an edit starts from it and only writes, if the file still
/// holds that text ([`Workspace::write_if`]).
#[derive(Clone, Debug, Default)]
pub struct Seen(Arc<Mutex<Files>>);

/// What a [`Seen`] holds.
#[derive(Debug, Default)]
struct Files {
    hashes: HashMap<PathBuf, ContentHash>,
    /// The texts kept, oldest first, each with its hash.
    texts: VecDeque<(PathBuf, ContentHash, Arc<str>)>,
    /// The bytes of `texts`.
    bytes: usize,
}

impl Files {
    /// Drops the text kept of `path`, if one is.
    fn drop_text(&mut self, path: &Path) {
        let Self { texts, bytes, .. } = self;
        texts.retain(|(kept, _, text)| {
            let same = kept == path;
            if same {
                *bytes -= text.len();
            }
            !same
        });
    }
}

impl Seen {
    /// Notes that the file `path` held the text with `hash`.
    pub fn saw(&self, path: &Path, hash: ContentHash) {
        let mut seen = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        seen.hashes.insert(path.to_path_buf(), hash);
    }

    /// Notes that the file `path` held `text`, all of it: its hash, and the
    /// text itself, which a text seen longer ago may make room for.
    pub fn saw_whole(&self, path: &Path, text: &str) {
        let hash = content_hash(text.as_bytes());
        let mut seen = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        seen.hashes.insert(path.to_path_buf(), hash.clone());
        seen.drop_text(path);
        let Files { texts, bytes, .. } = &mut *seen;
        if text.len() > SEEN_TEXT_BYTES {
            return;
        }
        while *bytes + text.len() > SEEN_TEXT_BYTES
            && let Some((_, _, old)) = texts.pop_front()
        {
            *bytes -= old.len();
        }
        *bytes += text.len();
        texts.push_back((path.to_path_buf(), hash, Arc::from(text)));
    }

    /// Forgets what `path` held, as after a write of it that may or may
    /// not have happened, so the next edit starts from the file itself.
    pub fn forget(&self, path: &Path) {
        let mut seen = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        seen.hashes.remove(path);
        seen.drop_text(path);
    }

    /// The hash of the text `path` last held when it was seen, if it was.
    pub fn hash(&self, path: &Path) -> Option<ContentHash> {
        let seen = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        seen.hashes.get(path).cloned()
    }

    /// The whole text `path` held when it was last seen, and its hash, if
    /// it was seen whole then and the text is still kept.
    pub fn text(&self, path: &Path) -> Option<(ContentHash, Arc<str>)> {
        let seen = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let last = seen.hashes.get(path)?;
        seen.texts
            .iter()
            .find(|(kept, hash, _)| kept == path && hash == last)
            .map(|(_, hash, text)| (hash.clone(), text.clone()))
    }
}

/// Arguments of [`Read`].
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadArgs {
    /// The file to read.
    pub path: String,
    /// The first line to read, from 1.
    pub offset: Option<usize>,
    /// How many lines to read.
    pub limit: Option<usize>,
}

impl ReadArgs {
    /// Read `path` from its start.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            offset: None,
            limit: None,
        }
    }

    /// The same read, of at most `limit` lines from line `offset` (from 1).
    pub fn lines(mut self, offset: usize, limit: usize) -> Self {
        self.offset = Some(offset);
        self.limit = Some(limit);
        self
    }

    /// What [`Read`] shows of `text`, the file's whole text, for this read:
    /// its lines from `offset` (default 1), at most `limit` of them (default
    /// and most [`MAX_LINES`]) and [`MAX_BYTES`], as [`show_lines`] shows them.
    ///
    /// ```
    /// use swarmie_tools::coding::ReadArgs;
    ///
    /// let shown = ReadArgs::new("a.txt").lines(2, 1).show("one\ntwo\nthree\n");
    /// assert_eq!(shown, "two\n[1 more lines; read on with offset 3]\n");
    /// ```
    pub fn show(&self, text: &str) -> String {
        self.shown(text).0
    }

    /// [`Self::show`], and which lines it shows: what [`Read`] publishes
    /// in its [`FileRead`], for code that shows a file as `read` would and
    /// records what it showed.
    ///
    /// ```
    /// use swarmie_tools::coding::ReadArgs;
    ///
    /// let (_, lines) = ReadArgs::new("a.txt").lines(2, 1).shown("one\ntwo\nthree\n");
    /// assert_eq!((lines.first, lines.count, lines.total), (2, 1, 3));
    /// ```
    pub fn shown(&self, text: &str) -> (String, LinesShown) {
        let (first, limit) = self.span();
        lines_from(text, first, limit, MAX_BYTES)
    }

    /// The first line this read shows, from 1, and how many lines at most.
    fn span(&self) -> (usize, usize) {
        let first = self.offset.unwrap_or(1).max(1);
        (first, self.limit.unwrap_or(MAX_LINES).min(MAX_LINES))
    }
}

impl Tool for Read {
    const NAME: &'static str = "read";
    type Args = ReadArgs;
    type Output = ToolOutput;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        format!(
            "Read a regular text file of at most {} MB. Returns its lines as they are, \
             at most {MAX_LINES} lines or {} KB (lines too long for that are cut shorter, and a \
             note says so and how to read on), then the offset to read on from if it stopped \
             early; use offset (from 1) and limit for part of a file, and `rg -n` in `shell` to \
             find a line. Use `read` to look at a file, not `cat`, `head` or `sed` in `shell` \
             (but for the command a cut line's note gives), and read several files at once by \
             calling `read` several times in one reply.",
            MAX_FILE_BYTES / (1024 * 1024),
            MAX_BYTES / 1024
        )
    }

    fn parameters(&self) -> serde_json::Value {
        args_schema::<ReadArgs>()
    }

    fn annotations(&self) -> crate::ToolAnnotations {
        crate::ToolAnnotations::default()
            .with_read_only(true)
            .with_open_world(false)
    }

    fn footprint(&self, args: &ReadArgs) -> Touches {
        Touches::Reads(Scope::Paths(vec![
            self.workspace.resolve(self.base.as_deref(), &args.path),
        ]))
    }

    async fn call(
        &self,
        context: &mut ToolContext,
        args: ReadArgs,
    ) -> Result<ToolOutput, ToolExecutionError> {
        let path = self
            .workspace
            .place(self.base.as_deref(), self.confine, &args.path)?;
        let shown = path.to_string_lossy().into_owned();
        let workspace = self.workspace.clone();
        let seen = self.seen.clone();
        let (output, read) = blocking(move || {
            let (first, limit) = args.span();
            let excerpt = workspace
                .excerpt(&path, first, limit)
                .map_err(|error| text_error(&shown, error))?;
            let asked = excerpt.text.lines().take(limit).collect();
            let (output, lines) = show(asked, first, excerpt.total, MAX_BYTES, Some(&shown));
            let hash = excerpt.hash;
            // A read that got the whole file leaves its text for an edit.
            let whole = excerpt
                .whole
                .as_deref()
                .or((first == 1 && excerpt.total <= limit).then_some(excerpt.text.as_str()))
                .filter(|whole| content_hash(whole.as_bytes()) == hash);
            match whole {
                Some(whole) => seen.saw_whole(&path, whole),
                None => seen.saw(&path, hash.clone()),
            }
            let read = FileRead::new(shown, hash).with_lines(lines);
            Ok::<_, ToolExecutionError>((ToolOutput::text(output), read))
        })
        .await?;
        let cut = read.lines.and_then(cut_of);
        context.insert_result(FilesRead(vec![read]))?;
        if let Some(cut) = cut {
            context.insert_result(cut)?;
        }
        Ok(output)
    }
}

/// How a read that showed `lines` was cut: from its first line on, the
/// lines after those it showed, which its last line names.
fn cut_of(lines: LinesShown) -> Option<OutputCut> {
    let from_first = lines.total.checked_sub(lines.first.saturating_sub(1))?;
    (lines.count < from_first).then_some(OutputCut {
        kept: KeptEnd::Start,
        kept_lines: lines.count,
        total_lines: from_first,
        spill: None,
    })
}

/// `text`'s lines from line `first` (counted from 1) as [`Read`] shows
/// them: as they are, at most `limit` lines and about `max_bytes` bytes,
/// and a last line saying how many lines were left out and the offset to
/// read on from, or that `first` is past the end. Lines longer than a
/// width are cut, marked `[…]`, and counted in a note: 2000 characters, or
/// less when that lets the asked lines fit `max_bytes`, as in a file of
/// long lines, but never below 100 characters.
pub fn show_lines(text: &str, first: usize, limit: usize, max_bytes: usize) -> String {
    lines_from(text, first, limit, max_bytes).0
}

/// The narrowest a line is cut to so that more lines fit.
const MIN_LINE_CHARS: usize = 100;
/// The bytes of a cut line the command in a read's note shows at once:
/// a fifth of what `shell` returns, in lines it keeps whole.
const CHUNK_BYTES: usize = 10_000;

/// [`show_lines`], and which lines it shows.
fn lines_from(text: &str, first: usize, limit: usize, max_bytes: usize) -> (String, LinesShown) {
    let first = first.max(1);
    let asked = text.lines().skip(first - 1).take(limit).collect();
    show(asked, first, text.lines().count(), max_bytes, None)
}

/// [`lines_from`] given the lines `asked`, from line `first` (from 1) of
/// a file of `total` lines. With the file's `path`, the note on the lines
/// cut says how to read on in the first of them.
fn show(
    asked: Vec<&str>,
    first: usize,
    total: usize,
    max_bytes: usize,
    path: Option<&str>,
) -> (String, LinesShown) {
    let width = width(&asked, max_bytes);
    let mut out = String::new();
    let mut shown = 0;
    let mut cut = 0;
    // The first line cut, its number, and the bytes shown of it.
    let mut first_cut = None;
    for (at, line) in asked.iter().enumerate() {
        let kept = clip(line, width);
        let clipped = kept.len() < line.len();
        let mark = if clipped { CUT_MARK.len() } else { 0 };
        // The first line always shows, so a read always makes progress.
        if shown > 0 && out.len() + kept.len() + mark + 1 > max_bytes {
            break;
        }
        out.push_str(kept);
        if clipped {
            out.push_str(CUT_MARK);
            cut += 1;
            first_cut = first_cut.or(Some((first + at, kept.len(), line.len())));
        }
        out.push('\n');
        shown += 1;
    }
    let last = first - 1 + shown;
    if cut > 0 {
        let _ = write!(
            out,
            "[{cut} {} cut at {width} characters{}",
            if cut == 1 { "line was" } else { "lines were" },
            if width < MAX_LINE_CHARS {
                format!(
                    " to fit {} KB; a smaller limit shows more of each",
                    max_bytes / 1024
                )
            } else {
                String::new()
            }
        );
        if let (Some(path), Some((number, kept, bytes))) = (path.and_then(shell_path), first_cut) {
            out.push_str(&read_on(&path, cut, number, kept, bytes));
        }
        out.push_str("]\n");
    }
    if last < total {
        let full = shown < asked.len();
        let _ = writeln!(
            out,
            "[{} more lines; read on with offset {}{}]",
            total - last,
            last + 1,
            if full {
                format!(", as this read reached its {} KB", max_bytes / 1024)
            } else {
                String::new()
            }
        );
    }
    if total == 0 {
        out.push_str("[empty file]\n");
    } else if first > total {
        out.push_str(&format!(
            "[offset {first} is past the end: the file has {total} lines]\n"
        ));
    }
    (
        out,
        LinesShown {
            first,
            count: shown,
            total,
        },
    )
}

/// How to read on in line `number` of the file at `path`, quoted for
/// `sh`, cut after its first `kept` of `bytes` bytes, the first of the
/// `cut` lines cut: a command that prints the next bytes of it in lines
/// `shell` keeps whole.
fn read_on(path: &str, cut: usize, number: usize, kept: usize, bytes: usize) -> String {
    let which = match cut {
        1 => format!("line {number}"),
        _ => format!("the first, line {number},"),
    };
    let from = kept + 1;
    let to = bytes.min(kept + CHUNK_BYTES);
    let next = match to < bytes {
        true => format!(
            "the next {CHUNK_BYTES} bytes, and `cut -b {}-{}` the ones after",
            to + 1,
            bytes.min(to + CHUNK_BYTES)
        ),
        false => format!("the remaining {} bytes", to - kept),
    };
    format!(
        "; {which} shows {kept} of its {bytes} bytes: in `shell`, `sed -n '{number}p;{number}q' \
         {path} | cut -b {from}-{to} | LC_ALL=C fold -b -w {MAX_LINE_CHARS}` shows {next}"
    )
}

/// The width the lines of `asked` are cut at to be shown in `max_bytes`:
/// [`MAX_LINE_CHARS`] when they fit so, else the widest of at least
/// [`MIN_LINE_CHARS`] at which they do. When even that is too wide, the
/// lines are cut at [`MIN_LINE_CHARS`] only if most of them are longer, so
/// a file of ordinary lines with a few long ones keeps its lines whole and
/// is read on in pieces.
fn width(asked: &[&str], max_bytes: usize) -> usize {
    let size = |width: usize| -> usize {
        asked
            .iter()
            .map(|line| {
                let kept = clip(line, width);
                let mark = if kept.len() < line.len() {
                    CUT_MARK.len()
                } else {
                    0
                };
                kept.len() + mark + 1
            })
            .sum()
    };
    if size(MAX_LINE_CHARS) <= max_bytes {
        return MAX_LINE_CHARS;
    }
    if size(MIN_LINE_CHARS) > max_bytes {
        let long = asked
            .iter()
            .filter(|line| line.chars().nth(MIN_LINE_CHARS).is_some())
            .count();
        return if long * 2 > asked.len() {
            MIN_LINE_CHARS
        } else {
            MAX_LINE_CHARS
        };
    }
    // The widest that fits: `low` always does, `high` never.
    let (mut low, mut high) = (MIN_LINE_CHARS, MAX_LINE_CHARS);
    while high - low > 1 {
        let middle = low + (high - low) / 2;
        if size(middle) <= max_bytes {
            low = middle;
        } else {
            high = middle;
        }
    }
    low
}

#[cfg(test)]
mod tests {
    use crate::{
        FileRead, FilesRead, KeptEnd, LinesShown, OutputCut, Tool, ToolContext, ToolExecutionError,
        ToolOutput, content_hash,
    };

    use super::{Read, ReadArgs};
    use crate::coding::fs::resolve;

    /// What `read` answers for `args`, and the context it published to.
    fn call(args: ReadArgs) -> (Result<ToolOutput, ToolExecutionError>, ToolContext) {
        let mut context = ToolContext::new();
        let read = futures::executor::block_on(Tool::call(&Read::default(), &mut context, args));
        (read, context)
    }

    /// The text `read` shows for `args`, empty when it fails.
    #[cfg(unix)]
    fn text(args: ReadArgs) -> String {
        let (read, _) = call(args);
        read.ok()
            .and_then(|shown| shown.as_text().map(str::to_owned))
            .unwrap_or_default()
    }

    /// Reading a file publishes it by its resolved path, with the hash of the
    /// bytes read and the lines shown, and a cut when lines after those were
    /// left out; a failed read publishes nothing.
    #[test]
    fn a_read_publishes_the_file_it_read() {
        let path =
            std::env::temp_dir().join(format!("swarmie-tools-read-{}.txt", std::process::id()));
        std::fs::write(&path, "one\ntwo\n").ok();
        let path = path.to_string_lossy().into_owned();

        let (shown, context) = call(ReadArgs::new(&path).lines(2, 1));
        let shown = shown
            .ok()
            .and_then(|shown| shown.as_text().map(str::to_owned));
        assert!(shown.is_some_and(|shown| shown.starts_with("two\n")));
        let published = context.result::<FilesRead>().ok().flatten();
        assert_eq!(
            published,
            Some(FilesRead(vec![
                FileRead::new(
                    resolve(None, &path).to_string_lossy(),
                    content_hash(b"one\ntwo\n")
                )
                .with_lines(LinesShown {
                    first: 2,
                    count: 1,
                    total: 2
                })
            ]))
        );
        assert_eq!(context.result::<OutputCut>().ok().flatten(), None);

        let (shown, context) = call(ReadArgs::new(&path).lines(1, 1));
        assert_eq!(
            shown.ok().as_ref().and_then(ToolOutput::as_text),
            Some("one\n[1 more lines; read on with offset 2]\n")
        );
        assert_eq!(
            context.result::<OutputCut>().ok().flatten(),
            Some(OutputCut {
                kept: KeptEnd::Start,
                kept_lines: 1,
                total_lines: 2,
                spill: None
            })
        );

        let (missing, context) = call(ReadArgs::new(format!("{path}.gone")));
        assert!(missing.is_err());
        assert_eq!(context.result::<FilesRead>().ok().flatten(), None);
        std::fs::remove_file(path).ok();
    }

    /// A read from past a file's last line says so and how long the file is,
    /// rather than coming back empty.
    #[test]
    fn an_offset_past_the_end_says_so() {
        let shown = ReadArgs::new("a.txt")
            .lines(5, 10)
            .show("one\ntwo\nthree\n");
        assert_eq!(shown, "[offset 5 is past the end: the file has 3 lines]\n");
    }

    /// A file of long lines reads as far as one of short lines: its lines are
    /// cut to fit the bytes a read shows, and a note says so and how to see
    /// more of each, while a read of a few of them shows them whole.
    #[test]
    fn long_lines_are_cut_to_fit_rather_than_stopping_early() {
        let long = "x".repeat(640);
        let text: String = (0..475).map(|n| format!("{n} {long}\n")).collect();
        let shown = ReadArgs::new("data.txt").show(&text);
        let lines: Vec<&str> = shown.lines().collect();
        assert_eq!(lines.len(), 476, "{:?}", lines.last());
        assert!(
            lines
                .first()
                .is_some_and(|line| line.starts_with("0 x") && line.ends_with(" […]"))
        );
        assert!(lines.get(474).is_some_and(|line| line.starts_with("474 x")));
        let note = lines.last().copied().unwrap_or_default();
        assert!(
            note.starts_with("[475 lines were cut at ")
                && note.ends_with(" characters to fit 50 KB; a smaller limit shows more of each]"),
            "{note}"
        );
        assert!(shown.len() <= crate::coding::MAX_BYTES, "{}", shown.len());
        let few = ReadArgs::new("data.txt").lines(1, 20).show(&text);
        assert!(
            few.starts_with(&format!("0 {long}\n1 {long}\n")),
            "{few:.80}"
        );
        assert!(
            few.ends_with("[455 more lines; read on with offset 21]\n"),
            "{few}"
        );
    }

    /// A file of ordinary lines that does not fit keeps its lines whole and
    /// says the read stopped at its bytes.
    #[test]
    fn a_read_that_stops_at_its_bytes_says_so() {
        let text: String = (0..3000)
            .map(|n| format!("let line_{n:04} = {n:030};\n"))
            .collect();
        let shown = ReadArgs::new("big.rs").show(&text);
        let last = shown.lines().last().unwrap_or_default();
        assert!(
            last.ends_with(", as this read reached its 50 KB]")
                && last.contains("more lines; read on"),
            "{last}"
        );
        assert!(!shown.contains("[…]"));
    }

    #[test]
    fn a_binary_file_is_refused_as_not_text() {
        let path =
            std::env::temp_dir().join(format!("swarmie-tools-read-{}.bin", std::process::id()));
        std::fs::write(&path, b"\xff\xfe\0\x01").ok();
        let path = resolve(None, &path.to_string_lossy());
        let path = path.to_string_lossy().into_owned();
        let (refused, _) = call(ReadArgs::new(&path));
        let refused = refused
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            refused.contains(&format!("{path} is not a text file (not UTF-8)")),
            "{refused}"
        );
        #[cfg(unix)]
        assert!(
            refused.ends_with(&format!(
                "(not UTF-8); `file '{path}'` in `shell` shows what it is"
            )),
            "{refused}"
        );
        std::fs::remove_file(path).ok();
    }

    /// The note on a line cut short gives a command only for a path `shell`
    /// reaches as written, also when lines are cut narrower to fit; the
    /// refusal of a file that is not text likewise.
    #[cfg(unix)]
    #[test]
    fn a_command_is_given_only_for_a_path_shell_reaches() {
        let long = "x".repeat(640);
        let lines: Vec<String> = (0..475).map(|n| format!("{n:03} {long}")).collect();
        let asked: Vec<&str> = lines.iter().map(String::as_str).collect();
        let (shown, _) = super::show(
            asked.clone(),
            1,
            475,
            crate::coding::MAX_BYTES,
            Some("/w/data.txt"),
        );
        let note = shown.lines().last().unwrap_or_default();
        let width: usize = note
            .split_once("cut at ")
            .and_then(|(_, rest)| rest.split(' ').next())
            .and_then(|width| width.parse().ok())
            .unwrap_or_default();
        assert!((100..2000).contains(&width), "{note}");
        assert!(
        note.ends_with(&format!(
            " to fit 50 KB; a smaller limit shows more of each; the first, line 1, shows {width} \
             of its 644 bytes: in `shell`, `sed -n '1p;1q' '/w/data.txt' | cut -b {}-644 | LC_ALL=C \
             fold -b -w 2000` shows the remaining {} bytes]",
            width + 1,
            644 - width
        )),
        "{note}"
    );
        let (remote, _) = super::show(
            asked,
            1,
            475,
            crate::coding::MAX_BYTES,
            Some("bench:~/data.txt"),
        );
        let note = remote.lines().last().unwrap_or_default();
        assert!(
            note.ends_with("a smaller limit shows more of each]"),
            "{note}"
        );
        let refused = crate::coding::workspace::text_error(
            "bench:~/data.bin",
            std::io::Error::from(std::io::ErrorKind::InvalidData),
        );
        assert!(
            refused
                .to_string()
                .ends_with("bench:~/data.bin is not a text file (not UTF-8)"),
            "{refused}"
        );
    }

    /// The note on a line cut short names it and the byte its rest starts at,
    /// and gives a command that shows the rest in lines `shell` keeps whole,
    /// starting exactly where the read stopped.
    #[cfg(unix)]
    #[test]
    fn a_cut_line_can_be_read_on_in_shell() {
        let dir =
            std::env::temp_dir().join(format!("swarmie-tools-read-it's-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        let dir = resolve(None, &dir.to_string_lossy());
        let path = dir.join("long.txt");
        let ascii = "x".repeat(30_000);
        let wide = "é".repeat(2_500);
        let three = "日".repeat(10_000);
        std::fs::write(&path, format!("short\n{ascii}\n{wide}\n{three}\n")).ok();
        let path = path.to_string_lossy().into_owned();
        let quoted = format!("'{}'", path.replace('\'', "'\\''"));

        let shown = text(ReadArgs::new(&path).lines(2, 1));
        let note = shown.lines().nth(1).unwrap_or_default();
        assert_eq!(
            note,
            format!(
                "[1 line was cut at 2000 characters; line 2 shows 2000 of its 30000 bytes: in \
             `shell`, `sed -n '2p;2q' {quoted} | cut -b 2001-12000 | LC_ALL=C fold -b -w 2000` shows the \
             next 10000 bytes, and `cut -b 12001-22000` the ones after]"
            )
        );
        let shown = text(ReadArgs::new(&path).lines(3, 2));
        let note = shown.lines().nth(2).unwrap_or_default();
        assert!(
        note.starts_with(
            "[2 lines were cut at 2000 characters; the first, line 3, shows 4000 of its 5000 bytes"
        ) && note.ends_with(
            "| cut -b 4001-5000 | LC_ALL=C fold -b -w 2000` shows the remaining 1000 bytes]"
        ),
        "{note}"
    );

        let rest = |note: &str| {
            let command = note.split('`').nth(3).unwrap_or_default();
            let ran = futures::executor::block_on(
                crate::coding::Shell::default().run(crate::coding::ShellArgs::new(command)),
            );
            let output = ran.map(|ran| ran.output).unwrap_or_default();
            crate::coding::split_status(&output).0.to_owned()
        };
        let next = rest(note);
        assert_eq!(next, format!("{}\n", "é".repeat(500)));
        let ascii_note = text(ReadArgs::new(&path).lines(2, 1));
        let next = rest(ascii_note.lines().nth(1).unwrap_or_default());
        assert!(next.lines().all(|line| line.len() == 2000), "{next:.100}");
        assert_eq!(next.lines().collect::<String>(), "x".repeat(10_000));
        // Chunks of 10000 bytes start and end inside three-byte characters,
        // as do the folds, which show as U+FFFD; every chunk still comes through.
        let three_note = text(ReadArgs::new(&path).lines(4, 1));
        let three_note = three_note.lines().nth(1).unwrap_or_default();
        assert!(three_note.contains("| cut -b 6001-16000 |"), "{three_note}");
        for chunk in [
            rest(three_note),
            rest(&three_note.replace("6001-16000", "16001-26000")),
        ] {
            let characters = chunk.chars().filter(|c| *c == '日').count();
            assert!(characters >= 3320, "{characters} {chunk:.60}");
            assert!(chunk.lines().all(|line| line.len() <= 2000 * 3));
        }
        std::fs::remove_dir_all(dir).ok();
    }
}
