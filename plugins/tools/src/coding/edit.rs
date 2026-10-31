//! The `edit` tool: several replacements in one file per call, applied in
//! order, each to the text the ones before it left, and all of them or
//! none. A byte-order mark and CRLF line endings are kept. Text that does
//! not match exactly may still match line by line once trailing
//! whitespace and typographic quotes, dashes and spaces are evened out, but
//! only where exactly one place in the file matches. The result names the
//! lines that now hold the changed text and shows them, with a few lines
//! around each, as the file now holds them; a failure says what to send
//! instead, with the closest text to an `old_text` that was not found.

use std::borrow::Cow;
use std::fmt::Write as _;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ToolAnnotations;
use crate::{
    FileChange, FileState, FilesChanged, HunkLine, Scope, Tool, ToolContext, ToolExecutionError,
    Touches, args_schema, content_hash,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::coding::fs::{diff, io_error};
use crate::coding::workspace::{Local, Workspace, text_error, text_of};
use crate::coding::{Seen, blocking, clip};

/// Most matches an ambiguity error lists by line, and most changed places
/// a reply lists.
const MAX_LISTED: usize = 8;
/// Most lines of the closest text a not-found error shows, and the most
/// characters of each.
const MAX_CLOSEST: usize = 20;
const MAX_CLOSEST_CHARS: usize = 300;

/// Replaces text in a file. Publishes the file it changed
/// ([`FilesChanged`]): its [resolved](Workspace) path, and the
/// whole diff.
#[derive(Clone, Debug)]
pub struct Edit {
    /// Where the file is edited.
    pub workspace: Arc<dyn Workspace>,
    /// The directory a relative path is edited in; `None` is the
    /// workspace's working directory.
    pub base: Option<PathBuf>,
    /// Whether a path that resolves outside `base` is refused, before
    /// anything is read.
    pub confine: bool,
    /// The files [`Read`](crate::coding::Read) showed and edits left, shared with
    /// it: an `old_text` not found asks for a new read only of a file that
    /// changed since, and an edit of a file seen whole is made from that
    /// text, in one [`write_if`](Workspace::write_if).
    pub seen: Seen,
}

impl Default for Edit {
    /// Edits this machine's files, from this process's working directory.
    fn default() -> Self {
        Self {
            workspace: Arc::new(Local),
            base: None,
            confine: false,
            seen: Seen::default(),
        }
    }
}

/// Arguments of [`Edit`].
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditArgs {
    /// The file to edit.
    pub path: String,
    /// The replacements, in order: each matches the text the ones before
    /// it left.
    #[schemars(length(min = 1))]
    pub edits: Vec<Replacement>,
}

impl EditArgs {
    /// Make `edits` to `path`.
    pub fn new(path: impl Into<String>, edits: impl IntoIterator<Item = Replacement>) -> Self {
        Self {
            path: path.into(),
            edits: edits.into_iter().collect(),
        }
    }
}

// One replacement of an `edit` call; not a doc comment, which the model would see.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Replacement {
    /// The text to replace, copied from the file.
    pub old_text: String,
    /// The replacement.
    pub new_text: String,
    /// Replace every match instead of exactly one.
    #[serde(default)]
    pub replace_all: bool,
}

impl Replacement {
    /// Replace the one match of `old_text` with `new_text`.
    pub fn new(old_text: impl Into<String>, new_text: impl Into<String>) -> Self {
        Self {
            old_text: old_text.into(),
            new_text: new_text.into(),
            replace_all: false,
        }
    }

    /// The same replacement, of every match.
    pub fn all(mut self) -> Self {
        self.replace_all = true;
        self
    }
}

impl Tool for Edit {
    const NAME: &'static str = "edit";
    type Args = EditArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Replace text in one file. Each entry of `edits` replaces `old_text` with `new_text`. \
         Copy each `old_text` from the file as `read` showed it, with just enough lines around \
         the change to match one place, or set `replace_all` to replace every exact match. \
         Text with no exact match still matches line by line once trailing whitespace and \
         typographic quotes, dashes and spaces are evened out, but only where exactly one place \
         matches that way, and the result says so. Make all changes to one file in one call, \
         one entry per change: the entries apply in order, each to the text the ones before it \
         left, and all of them or none. Returns the lines that now hold the changed text, and \
         those lines and a few around each as the file now holds them, so there is no need to \
         read it again."
            .to_owned()
    }

    fn parameters(&self) -> serde_json::Value {
        args_schema::<EditArgs>()
    }

    fn annotations(&self) -> ToolAnnotations {
        ToolAnnotations::default()
            .with_destructive(true)
            .with_idempotent(false)
            .with_open_world(false)
    }

    fn footprint(&self, args: &EditArgs) -> Touches {
        Touches::Writes(Scope::Paths(vec![
            self.workspace.resolve(self.base.as_deref(), &args.path),
        ]))
    }

    async fn call(
        &self,
        context: &mut ToolContext,
        args: EditArgs,
    ) -> Result<String, ToolExecutionError> {
        let path = self
            .workspace
            .place(self.base.as_deref(), self.confine, &args.path)?;
        let workspace = self.workspace.clone();
        let seen = self.seen.clone();
        let (output, change) = blocking(move || edit(&*workspace, &seen, path, args)).await?;
        context.insert_result(FilesChanged(vec![change]))?;
        Ok(output)
    }
}

fn edit(
    workspace: &dyn Workspace,
    seen: &Seen,
    resolved: PathBuf,
    args: EditArgs,
) -> Result<(String, FileChange), ToolExecutionError> {
    let path = resolved.to_string_lossy();
    // From the text seen last, the edit is one write, made only while the
    // file still holds that text; else it is made from what the file holds.
    let mut held = None;
    if let Some((hash, text)) = seen.text(&resolved)
        && let Ok(applied) = apply_to(&path, &text, &args.edits, false)
    {
        let now = workspace
            .write_if(&resolved, applied.text.as_bytes(), &hash)
            .map_err(|error| {
                seen.forget(&resolved);
                text_error(&path, error)
            })?;
        match now {
            None => return Ok(reply(seen, &resolved, &args, applied)),
            Some(now) => held = Some(text_of(now).map_err(|error| text_error(&path, error))?),
        }
    }
    let raw = match held {
        Some(raw) => raw,
        None => workspace.read_text(&resolved)?,
    };
    let changed = seen
        .hash(&resolved)
        .is_some_and(|hash| hash != content_hash(raw.as_bytes()));
    let applied = apply_to(&path, &raw, &args.edits, changed)?;
    workspace
        .write(&resolved, applied.text.as_bytes())
        .map_err(|error| {
            seen.forget(&resolved);
            io_error(&path, error)
        })?;
    Ok(reply(seen, &resolved, &args, applied))
}

/// What an edit that wrote `applied` to `resolved` answers and publishes,
/// noting the text it left as seen.
fn reply(seen: &Seen, resolved: &Path, args: &EditArgs, applied: Applied) -> (String, FileChange) {
    seen.saw_whole(resolved, &applied.text);
    let places = places(&applied.change);
    let at = changed_at(&places, &applied.change, applied.replaced, args.edits.len());
    let mut out = format!("Edited {}: {at}.", args.path);
    for note in applied.notes {
        let _ = write!(out, "\nNote: {note}");
    }
    out.push_str(&around(&applied.text, &places));
    (out, applied.change)
}

/// Lines shown before and after each changed place in an edit's reply.
const CONTEXT: usize = 3;
/// Most lines of the file an edit's reply shows, and most of one place;
/// a longer place shows its first and last lines.
const MAX_AROUND: usize = 60;
const MAX_PLACE: usize = 30;

/// The lines of `text`, the file after an edit, at and around each of
/// `places`, as they are now: each run of them under a `[lines A-B]` line,
/// with long lines cut, so the reply shows the result without a new read.
fn around(text: &str, places: &[Place]) -> String {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for place in places {
        let (first, last) = match *place {
            Place::Lines(first, last) => (first, last),
            Place::Removed(at) => (at, at.saturating_sub(1)),
        };
        let first = first.saturating_sub(CONTEXT).max(1);
        let last = (last + CONTEXT).min(lines.len());
        match runs.last_mut() {
            Some((_, end)) if first <= *end + 1 => *end = (*end).max(last),
            _ if first <= last => runs.push((first, last)),
            _ => {}
        }
    }
    let mut out = String::new();
    let mut shown = 0;
    for (index, &(first, last)) in runs.iter().enumerate() {
        if shown >= MAX_AROUND {
            let _ = write!(
                out,
                "\n[{} more changed places not shown; read them to see them]",
                runs.len() - index
            );
            break;
        }
        let _ = write!(out, "\n[lines {first}-{last}]");
        let count = last - first + 1;
        let half = MAX_PLACE / 2;
        for (number, line) in (first..=last).zip(lines.iter().skip(first - 1)) {
            if count > MAX_PLACE && number == first + half {
                let _ = write!(out, "\n[… {} lines]", count - 2 * half);
            }
            if count > MAX_PLACE && number >= first + half && number <= last - half {
                continue;
            }
            out.push('\n');
            out.push_str(clip(line, MAX_CLOSEST_CHARS));
            shown += 1;
        }
    }
    out
}

/// Where `change` left the changed text, its `places`, as an `edit` reply
/// says it: how many places were replaced when that is not one per edit
/// (`replace_all`), each place by its lines in the file after the change
/// (a place that only removed lines by the line now where they were), and
/// how many lines the file grew or shrank by.
fn changed_at(places: &[Place], change: &FileChange, replaced: usize, edits: usize) -> String {
    let mut out = String::new();
    if replaced != edits {
        let _ = write!(out, "{replaced} replacements, ");
    }
    out.push_str(match places {
        [Place::Removed(_)] => "line ",
        [Place::Lines(first, last)] if first == last => "line ",
        // Each removal names its own line.
        [Place::Removed(_), ..] => "",
        _ => "lines ",
    });
    let listed: Vec<String> = places
        .iter()
        .take(MAX_LISTED)
        .map(|place| match *place {
            Place::Lines(first, last) if first == last => first.to_string(),
            Place::Lines(first, last) => format!("{first}-{last}"),
            Place::Removed(at) if places.len() == 1 => at.to_string(),
            Place::Removed(at) => format!("line {at}"),
        })
        .collect();
    out.push_str(&listed.join(", "));
    let more = places.len().saturating_sub(MAX_LISTED);
    if more > 0 {
        let _ = write!(out, ", and {more} more");
    }
    let shift = change.added as i64 - change.removed as i64;
    if shift != 0 {
        let noun = if shift.abs() == 1 { "line" } else { "lines" };
        let _ = write!(out, " ({shift:+} {noun})");
    }
    out
}

/// A place a change changed, by its lines in the file after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// It added or rewrote these lines, both ends included.
    Lines(usize, usize),
    /// It only removed lines, where this line now is.
    Removed(usize),
}

/// Each run of changed lines in `change`'s hunks, in order.
fn places(change: &FileChange) -> Vec<Place> {
    let mut places = Vec::new();
    for hunk in &change.hunks {
        let kept = |line: &HunkLine| matches!(line, HunkLine::Same(_));
        let mut runs = hunk.lines.chunk_by(|a, b| kept(a) == kept(b)).peekable();
        // The line of the file after the change that the next run starts at.
        let mut at = hunk.new.start;
        while let Some(run) = runs.next() {
            if run.first().is_some_and(kept) {
                at += run.len();
                continue;
            }
            let added = run.iter().filter(|line| line.sign() == '+').count();
            places.push(match added {
                // Hunks keep the lines after a change, so a run that ends
                // its hunk ends the file: lines removed there are at its last.
                0 if runs.peek().is_none() => Place::Removed(at.saturating_sub(1).max(1)),
                0 => Place::Removed(at),
                added => Place::Lines(at, at + added - 1),
            });
            at += added;
        }
    }
    places
}

/// What [`apply`] makes of a file's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    /// The edited text, with the byte-order mark and line endings the file
    /// had: what [`Edit`] writes.
    pub text: String,
    /// What the edit does to the file, with its hunks, the hash of the text
    /// it was made to and that of [`text`](Self::text).
    pub change: FileChange,
    /// How many places were replaced.
    pub replaced: usize,
    /// How each edit that matched only loosely was matched, for the model.
    pub notes: Vec<String>,
}

/// Makes `edits` to `text`, the whole text of the file at `path`, exactly
/// as [`Edit`] does, without touching the file: each edit is matched,
/// exactly or loosely, against the text the edits before it left, and all
/// are applied or none. A byte-order mark and CRLF line endings are kept.
/// `path` names the file in the [`FileChange`] and in an error.
///
/// ```
/// use swarmie_tools::coding::{Replacement, edit::apply};
///
/// let applied = apply("a.txt", "one\r\ntwo\r\n", &[Replacement::new("two", "2")])?;
/// assert_eq!(applied.text, "one\r\n2\r\n");
/// assert_eq!((applied.change.added, applied.change.removed), (1, 1));
/// let missed = apply("a.txt", "one\n", &[Replacement::new("three", "3")]);
/// assert!(missed.is_err_and(|error| error.to_string().contains("`a.txt` was not changed")));
/// # Ok::<(), swarmie_tools::ToolExecutionError>(())
/// ```
///
/// # Errors
///
/// When an edit matches nowhere or more than once, or the edits change
/// nothing: the error says what to send instead.
pub fn apply(path: &str, text: &str, edits: &[Replacement]) -> Result<Applied, ToolExecutionError> {
    apply_to(path, text, edits, false)
}

/// [`apply`], `changed` when the file changed since it was last read: an
/// `old_text` not found then asks for a new read.
fn apply_to(
    path: &str,
    text: &str,
    edits: &[Replacement],
    changed: bool,
) -> Result<Applied, ToolExecutionError> {
    if edits.is_empty() {
        return Err(ToolExecutionError::invalid_args(format!(
            "`edits` is empty, so `{path}` was not changed; give at least one \
             {{\"old_text\", \"new_text\"}}"
        )));
    }
    let whole = text;
    let file = Decoded::new(text);
    let text: &str = &file.text;
    let count = edits.len();
    let many = count > 1;
    // The text as the edits applied so far left it.
    let mut edited = text.to_owned();
    let mut applied: Vec<usize> = Vec::new();
    let mut replaced = 0;
    let mut notes = Vec::new();
    // Every edit is checked, so one reply names all that need fixing.
    let mut failed: Vec<String> = Vec::new();
    // The closest text to each `old_text` not found, shown after them.
    let mut closest: Vec<String> = Vec::new();
    for (index, replacement) in edits.iter().enumerate() {
        let label = if many {
            format!("edits[{index}]: ")
        } else {
            String::new()
        };
        let old = lf(&replacement.old_text);
        let new = lf(&replacement.new_text);
        if old.is_empty() {
            failed.push(format!(
                "{label}`old_text` is empty; to add text, replace a nearby line with itself plus the new text"
            ));
            continue;
        }
        if old == new {
            failed.push(format!(
                "{label}`old_text` and `new_text` are the same, so it changes nothing; drop it or fix `new_text`"
            ));
            continue;
        }
        match locate(&edited, &old, replacement.replace_all) {
            Err(Miss::NotFound) => {
                if let Some(earlier) = changed_by(text, edits, &applied, &old, replacement) {
                    failed.push(format!(
                        "{label}`old_text` was in the file, but edits[{earlier}], applied before \
                         it, changed that text; match the text as edits[{earlier}] leaves it, or \
                         make this change in edits[{earlier}]"
                    ));
                    continue;
                }
                let near = (!changed).then(|| nearest(&edited, &old)).flatten();
                failed.push(format!(
                    "{label}{}",
                    not_found(path, changed, near.is_some())
                ));
                if let Some(near) = near {
                    let whose = if many {
                        format!("of edits[{index}] ")
                    } else {
                        String::new()
                    };
                    closest.push(format!(
                        "The closest text to the `old_text` {whose}is {}:\n{}",
                        near.lines, near.text
                    ));
                }
            }
            Err(miss) => failed.push(format!("{label}{}", miss.explain())),
            Ok(found) => {
                if let Some(note) = found.note {
                    notes.push(format!("{label}{note}"));
                }
                replaced += found.spans.len();
                edited = splice(&edited, &found.spans, &new);
                applied.push(index);
            }
        }
    }
    if !failed.is_empty() {
        let mut why = batch_failure(path, count, &failed);
        for near in closest {
            let _ = write!(why, "\n{near}");
        }
        return Err(ToolExecutionError::invalid_args(why));
    }
    if edited == text {
        return Err(refused(
            path,
            "the edits leave the file as it was; check `new_text`".to_owned(),
        ));
    }
    let change = diff(path, Some(text), Some(&edited))
        .with_before_text(text)
        .with_before(FileState::of(whole.as_bytes()));
    let encoded = file.encode(edited);
    Ok(Applied {
        change: change.with_after(FileState::of(encoded.as_bytes())),
        text: encoded,
        replaced,
        notes,
    })
}

/// `text` with each of `spans`, in order and apart, replaced by `new`.
fn splice(text: &str, spans: &[Range<usize>], new: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for span in spans {
        out.push_str(text.get(at..span.start).unwrap_or_default());
        out.push_str(new);
        at = span.end;
    }
    out.push_str(text.get(at..).unwrap_or_default());
    out
}

/// Which of the edits `applied` before it, made in order to `text`, took
/// away the text `old` of `replacement` matched there; `None` when it
/// matched nowhere to begin with. Only worked out for a refusal.
fn changed_by(
    text: &str,
    edits: &[Replacement],
    applied: &[usize],
    old: &str,
    replacement: &Replacement,
) -> Option<usize> {
    let mut before = text.to_owned();
    for &index in applied {
        if locate(&before, old, replacement.replace_all).is_err() {
            return None;
        }
        let earlier = edits.get(index)?;
        let found = locate(&before, &lf(&earlier.old_text), earlier.replace_all).ok()?;
        before = splice(&before, &found.spans, &lf(&earlier.new_text));
        if locate(&before, old, replacement.replace_all).is_err() {
            return Some(index);
        }
    }
    None
}

/// The file's text with LF line endings and no byte-order mark, and how to
/// put both back.
struct Decoded<'a> {
    text: Cow<'a, str>,
    bom: bool,
    crlf: bool,
}

impl<'a> Decoded<'a> {
    fn new(raw: &'a str) -> Self {
        let (bom, body) = match raw.strip_prefix('\u{feff}') {
            Some(body) => (true, body),
            None => (false, raw),
        };
        // Only a file whose every line ends in CRLF is converted; a mixed
        // one is edited as it is, and loose matching ignores its `\r`s.
        let newlines = body.matches('\n').count();
        let crlf = newlines > 0 && body.matches("\r\n").count() == newlines;
        let text = if crlf {
            Cow::Owned(body.replace("\r\n", "\n"))
        } else {
            Cow::Borrowed(body)
        };
        Self { text, bom, crlf }
    }

    /// `text` with the file's line endings and byte-order mark.
    fn encode(&self, text: String) -> String {
        let body = if self.crlf {
            text.replace('\n', "\r\n")
        } else {
            text
        };
        if self.bom {
            format!("\u{feff}{body}")
        } else {
            body
        }
    }
}

/// `text` with CRLF line endings made LF, as the file is matched.
fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// A model-visible refusal: nothing was written to `path`.
fn refused(path: &str, why: String) -> ToolExecutionError {
    ToolExecutionError::invalid_args(format!(
        "{why}. `{path}` was not changed; fix the call and send all of its edits again."
    ))
}

/// The refusal for edits of `path` that failed: each one's reason, and, in
/// a batch, that the edits that matched were not applied either and must
/// be sent again with the fixed ones.
fn batch_failure(path: &str, count: usize, failed: &[String]) -> String {
    let [why] = failed else {
        return format!(
            "{} of the {count} edits failed:\n{}.\nNo edit was applied, so `{path}` was not \
             changed. Send all {count} edits again, with these fixed.",
            failed.len(),
            failed.join(".\n"),
        );
    };
    if count == 1 {
        return format!("{why}. `{path}` was not changed; fix the edit and send it again.");
    }
    format!(
        "{why}. No edit was applied, so `{path}` was not changed. Send all {count} edits again, \
         with this one fixed."
    )
}

/// The 1-based line holding byte `at` of `text`.
fn line_of(text: &str, at: usize) -> usize {
    text.get(..at)
        .map_or(0, |before| before.matches('\n').count())
        + 1
}

/// Where an edit's `old_text` matched.
struct Found {
    spans: Vec<Range<usize>>,
    /// How the match was loose, if it was.
    note: Option<String>,
}

/// Why an edit's `old_text` did not match once.
enum Miss {
    /// It matches nowhere.
    NotFound,
    /// It matches at these 1-based lines, `loose` when only after evening
    /// out whitespace or punctuation.
    Ambiguous { lines: Vec<usize>, loose: bool },
}

impl Miss {
    /// What went wrong and what to send instead.
    fn explain(&self) -> String {
        match self {
            Miss::Ambiguous { lines, loose } => {
                let listed: Vec<String> = lines
                    .iter()
                    .take(MAX_LISTED)
                    .map(ToString::to_string)
                    .collect();
                let more = lines.len().saturating_sub(MAX_LISTED);
                let more = if more > 0 {
                    format!(" and {more} more")
                } else {
                    String::new()
                };
                format!(
                    "`old_text` {}matches {} places, starting at lines {}{more}; add the lines \
                     around the one to change so it matches once, or set `replace_all` to \
                     change every one",
                    if *loose {
                        "does not match exactly, and loosely "
                    } else {
                        ""
                    },
                    lines.len(),
                    listed.join(", ")
                )
            }
            Miss::NotFound => not_found("the file", false, false),
        }
    }
}

/// Why an `old_text` matched nowhere in `path`, which `changed` since it
/// was last read, and what to do: read it again when it changed, else copy
/// the text from the closest place when there is one (`near`).
fn not_found(path: &str, changed: bool, near: bool) -> String {
    let what = "`old_text` was not found, not even ignoring trailing whitespace";
    if changed {
        format!(
            "{what}: `{path}` changed since it was last read, so read it again and copy \
             `old_text` from what it holds now"
        )
    } else if near {
        format!("{what}; copy it from the closest text in the file, shown below")
    } else {
        format!("{what}, and nothing in the file is close to it; check the path and the text")
    }
}

/// The place in a text closest to an `old_text` that matched nowhere.
struct Near {
    /// Its lines, as `line 3` or `lines 3-5`.
    lines: String,
    /// Its text, as the file holds it, with long lines cut.
    text: String,
}

/// The run of `text`'s lines most like `old`'s, as many as `old` has: each
/// line compared by how much of its start and end match once evened out as
/// a loose match is. `None` when no run is even half alike.
fn nearest(text: &str, old: &str) -> Option<Near> {
    let file = Lines::new(text);
    let count = file.spans.len();
    let wanted: Vec<String> = old
        .strip_suffix('\n')
        .unwrap_or(old)
        .split('\n')
        .map(loose_key)
        .collect();
    let size = wanted.len().min(count).max(1);
    let keys: Vec<String> = (0..count)
        .map(|index| loose_key(file.line(index)))
        .collect();
    // Only the first lines of a long `old_text` are compared, so a large
    // file is searched quickly.
    let compared = size.min(MAX_CLOSEST);
    let mut best: Option<(f64, usize)> = None;
    for start in 0..=count.saturating_sub(size) {
        let score: f64 = wanted
            .iter()
            .zip(keys.iter().skip(start))
            .take(compared)
            .map(|(want, have)| alike(want, have))
            .sum::<f64>()
            / compared as f64;
        if best.is_none_or(|(most, _)| score > most) {
            best = Some((score, start));
        }
    }
    let (score, start) = best?;
    if score < 0.5 {
        return None;
    }
    let shown: Vec<String> = (start..start + size)
        .take(MAX_CLOSEST)
        .map(|index| clip(file.line(index), MAX_CLOSEST_CHARS).to_owned())
        .collect();
    let mut text = shown.join("\n");
    if size > MAX_CLOSEST {
        let _ = write!(text, "\n[… {} more lines]", size - MAX_CLOSEST);
    }
    let lines = match size {
        1 => format!("line {}", start + 1),
        _ => format!("lines {}-{}", start + 1, start + size),
    };
    Some(Near { lines, text })
}

/// How alike two lines are, from 0 to 1: the characters their starts and
/// ends share, over the longer one's.
fn alike(one: &str, other: &str) -> f64 {
    let (one, other): (Vec<char>, Vec<char>) = (one.chars().collect(), other.chars().collect());
    let longest = one.len().max(other.len());
    if longest == 0 {
        return 1.0;
    }
    let start = one.iter().zip(&other).take_while(|(a, b)| a == b).count();
    let end = one
        .iter()
        .rev()
        .zip(other.iter().rev())
        .take(one.len().min(other.len()) - start)
        .take_while(|(a, b)| a == b)
        .count();
    (start + end) as f64 / longest as f64
}

/// Finds `old` in `text`: exactly, else line by line with trailing
/// whitespace and typographic punctuation evened out. A loose match must be
/// the only one; `replace_all` takes exact matches only.
fn locate(text: &str, old: &str, replace_all: bool) -> Result<Found, Miss> {
    let exact: Vec<Range<usize>> = text
        .match_indices(old)
        .map(|(start, matched)| start..start + matched.len())
        .collect();
    match exact.len() {
        0 => {}
        count if count == 1 || replace_all => {
            return Ok(Found {
                spans: exact,
                note: None,
            });
        }
        _ => {
            return Err(Miss::Ambiguous {
                lines: exact.iter().map(|span| line_of(text, span.start)).collect(),
                loose: false,
            });
        }
    }
    if replace_all {
        return Err(Miss::NotFound);
    }
    let lines = Lines::new(text);
    let wanted: Vec<&str> = old.strip_suffix('\n').unwrap_or(old).split('\n').collect();
    let start = match lines.windows(&wanted).as_slice() {
        [] => return Err(Miss::NotFound),
        [start] => *start,
        starts => {
            return Err(Miss::Ambiguous {
                lines: starts.iter().map(|start| start + 1).collect(),
                loose: true,
            });
        }
    };
    Ok(Found {
        spans: vec![lines.span(start, wanted.len(), old.ends_with('\n'))],
        note: Some(
            "`old_text` matched only after evening out trailing whitespace and quote, dash or \
             space characters; copy text exactly next time"
                .to_owned(),
        ),
    })
}

/// `line` as loosely compared: without trailing whitespace, and with
/// typographic quotes, dashes and spaces made ASCII.
fn loose_key(line: &str) -> String {
    line.trim_end().chars().map(plain).collect()
}

/// The ASCII form of a typographic quote, dash or space.
fn plain(c: char) -> char {
    match c {
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
        '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
        '\u{00A0}' | '\u{2002}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => ' ',
        c => c,
    }
}

/// The lines of a text, with where each starts and ends.
struct Lines<'a> {
    text: &'a str,
    /// For each line, its byte range without and with its `\n`.
    spans: Vec<(Range<usize>, usize)>,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        let mut spans = Vec::new();
        let mut start = 0;
        for line in text.split_inclusive('\n') {
            let end = start + line.len();
            let content = end - usize::from(line.ends_with('\n'));
            spans.push((start..content, end));
            start = end;
        }
        Self { text, spans }
    }

    fn line(&self, index: usize) -> &'a str {
        self.spans
            .get(index)
            .and_then(|(content, _)| self.text.get(content.clone()))
            .unwrap_or_default()
    }

    /// The 0-based first lines of each run of lines that loosely equals
    /// `wanted`.
    fn windows(&self, wanted: &[&str]) -> Vec<usize> {
        let keys: Vec<String> = (0..self.spans.len())
            .map(|index| loose_key(self.line(index)))
            .collect();
        let wanted: Vec<String> = wanted.iter().map(|line| loose_key(line)).collect();
        if wanted.iter().all(String::is_empty) {
            return Vec::new();
        }
        keys.windows(wanted.len().max(1))
            .enumerate()
            .filter(|(_, window)| *window == wanted.as_slice())
            .map(|(start, _)| start)
            .collect()
    }

    /// The bytes of `count` lines from `start`, with the last line's `\n`
    /// when `whole_lines`.
    fn span(&self, start: usize, count: usize, whole_lines: bool) -> Range<usize> {
        let first = self
            .spans
            .get(start)
            .map_or(self.text.len(), |(content, _)| content.start);
        let last = self.spans.get(start + count - 1);
        let end = match last {
            Some((_, with_newline)) if whole_lines => *with_newline,
            Some((content, _)) => content.end,
            None => self.text.len(),
        };
        first..end
    }
}

#[cfg(test)]
mod tests {
    use crate::{FilesChanged, HunkLine, Tool, ToolContext};

    use super::{Edit, EditArgs, Replacement, apply, batch_failure};

    /// A misspelled key inside an edit, such as `replaceAll`, is refused
    /// rather than ignored, which would replace one match instead of all.
    #[test]
    fn a_misspelled_key_inside_an_edit_is_refused() {
        let edit = serde_json::json!({"old_text": "a", "new_text": "b", "replaceAll": true});
        let args = serde_json::json!({"path": "a.rs", "edits": [edit]});
        let parsed = serde_json::from_value::<EditArgs>(args).map(|_| ());
        assert!(parsed.is_err_and(|error| error.to_string().contains("replaceAll")));
    }

    /// A failed edit is sent again; in a batch, every failed edit is named and
    /// the whole batch is sent again.
    #[test]
    fn a_failure_names_every_failed_edit() {
        let single = batch_failure("a.rs", 1, &["`old_text` was not found".to_owned()]);
        assert_eq!(
            single,
            "`old_text` was not found. `a.rs` was not changed; fix the edit and send it again."
        );
        let one = batch_failure(
            "a.rs",
            3,
            &["edits[1]: `old_text` was not found".to_owned()],
        );
        assert_eq!(
            one,
            "edits[1]: `old_text` was not found. No edit was applied, so `a.rs` was not changed. \
         Send all 3 edits again, with this one fixed."
        );
        let failed = ["edits[0]: one".to_owned(), "edits[2]: two".to_owned()];
        let two = batch_failure("a.rs", 3, &failed);
        assert_eq!(
            two,
            "2 of the 3 edits failed:\nedits[0]: one.\nedits[2]: two.\nNo edit was applied, so \
         `a.rs` was not changed. Send all 3 edits again, with these fixed."
        );
    }

    /// `apply` makes of a file's text what the tool writes to it: here a CRLF
    /// file with a byte-order mark, edited by a loose match.
    #[test]
    fn apply_gives_the_text_the_tool_writes() {
        let edit_tool = Edit::default();
        let path =
            std::env::temp_dir().join(format!("swarmie-tools-apply-{}.txt", std::process::id()));
        let before = "\u{feff}let a = \u{201c}x\u{201d};  \r\nlet b = 2;\r\n";
        std::fs::write(&path, before).ok();
        let path = path.to_string_lossy().into_owned();
        let edits = [Replacement::new("let a = \"x\";\n", "let a = 1;\n")];

        let applied = apply(&path, before, &edits);
        let mut context = ToolContext::new();
        let edited = Tool::call(&edit_tool, &mut context, EditArgs::new(&path, edits));
        let output = futures::executor::block_on(edited).unwrap_or_default();
        let written = std::fs::read_to_string(&path).ok();
        std::fs::remove_file(&path).ok();

        let applied = applied.ok();
        assert_eq!(
            applied.as_ref().map(|applied| applied.text.as_str()),
            Some("\u{feff}let a = 1;\r\nlet b = 2;\r\n")
        );
        assert_eq!(
            applied.as_ref().map(|applied| applied.text.clone()),
            written
        );
        assert_eq!(applied.as_ref().map(|applied| applied.notes.len()), Some(1));
        assert!(
            output.contains("Note: `old_text` matched only after"),
            "{output}"
        );
    }

    /// An edit's reply starts with the lines that now hold the changed text
    /// and how far the file grew or shrank, not with the change itself, which
    /// it still publishes whole for the terminal view.
    #[test]
    fn an_edit_answers_where_it_changed_the_file() {
        let edit_tool = Edit::default();
        let path =
            std::env::temp_dir().join(format!("swarmie-tools-where-{}.txt", std::process::id()));
        std::fs::write(&path, "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n").ok();
        let path = path.to_string_lossy().into_owned();
        let call = |edits: Vec<Replacement>| {
            let mut context = ToolContext::new();
            let edited = Tool::call(&edit_tool, &mut context, EditArgs::new(&path, edits));
            let reply = futures::executor::block_on(edited).unwrap_or_default();
            let said = reply.lines().next().unwrap_or_default().to_owned();
            (said, context.result::<FilesChanged>().ok().flatten())
        };

        // b becomes three lines; h goes.
        let (reply, published) = call(vec![
            Replacement::new("b\n", "b1\nb2\nb3\n"),
            Replacement::new("h\n", ""),
        ]);
        assert_eq!(
            reply,
            format!("Edited {path}: lines 2-4, line 10 (+1 line).")
        );
        assert!(!reply.contains("@@"), "{reply}");
        let hunks = published
            .map(|FilesChanged(changes)| changes)
            .unwrap_or_default();
        let lines = hunks
            .iter()
            .flat_map(|change| &change.hunks)
            .flat_map(|hunk| &hunk.lines);
        assert!(
            lines
                .clone()
                .any(|line| matches!(line, HunkLine::Same(text) if text == "a\n")),
            "the published change keeps its context: {hunks:?}"
        );
        assert!(
            lines
                .clone()
                .any(|line| matches!(line, HunkLine::Removed(text) if text == "h\n"))
        );

        let (reply, _) = call(vec![Replacement::new("j", "J").all()]);
        assert_eq!(reply, format!("Edited {path}: line 11."));
        std::fs::write(&path, "x\ny\nx\ny\nx\n").ok();
        let (reply, _) = call(vec![Replacement::new("x", "z").all()]);
        assert_eq!(
            reply,
            format!("Edited {path}: 3 replacements, lines 1, 3, 5.")
        );
        // The first place only removed a line, and the file shrank.
        std::fs::write(&path, "a\nb\nc\nd\ne\nf\n").ok();
        let (reply, _) = call(vec![
            Replacement::new("b\n", ""),
            Replacement::new("e\n", "E\n"),
        ]);
        std::fs::remove_file(&path).ok();
        assert_eq!(reply, format!("Edited {path}: line 2, 4 (-1 line)."));
    }

    /// After where it changed the file, an edit's reply shows those lines and
    /// three around each as the file now holds them, places near each other in
    /// one run, a long place by its ends, and lines that only went by the lines
    /// around where they were.
    #[test]
    fn an_edit_shows_the_lines_around_each_change() {
        let edit_tool = Edit::default();
        let path =
            std::env::temp_dir().join(format!("swarmie-tools-around-{}.txt", std::process::id()));
        let numbered = |lines: std::ops::RangeInclusive<usize>| -> String {
            lines.map(|n| format!("{n}\n")).collect()
        };
        std::fs::write(&path, numbered(1..=100)).ok();
        let path = path.to_string_lossy().into_owned();
        let call = |edits: Vec<Replacement>| {
            let mut context = ToolContext::new();
            let edited = Tool::call(&edit_tool, &mut context, EditArgs::new(&path, edits));
            futures::executor::block_on(edited).unwrap_or_default()
        };

        let long: String = (0..40).map(|n| format!("new {n}\n")).collect();
        let reply = call(vec![
            Replacement::new("\n10\n", "\nten\n"),
            Replacement::new("\n12\n", "\ntwelve\n"),
            Replacement::new("\n50\n", "\n"),
            Replacement::new("\n80\n", format!("\n{long}")),
        ]);
        let middle: String = (0..12).map(|n| format!("new {n}\n")).collect();
        let end: String = (28..40).map(|n| format!("new {n}\n")).collect();
        let expected = format!(
            "Edited {path}: lines 10, 12, line 50, 79-118 (+38 lines).\n\
         [lines 7-15]\n7\n8\n9\nten\n11\ntwelve\n13\n14\n15\n\
         [lines 47-52]\n47\n48\n49\n51\n52\n53\n\
         [lines 76-121]\n77\n78\n79\n{middle}[… 16 lines]\n{end}81\n82\n83"
        );
        std::fs::remove_file(&path).ok();
        assert_eq!(reply, expected);
    }

    /// An `old_text` that is simply wrong is answered with the closest text in
    /// the file, and no new read; only a file that changed since it was read
    /// is to be read again.
    #[test]
    fn a_missed_old_text_shows_the_closest_text_and_asks_for_a_read_only_after_a_change() {
        let path =
            std::env::temp_dir().join(format!("swarmie-tools-missed-{}.rs", std::process::id()));
        let text = "fn main() {\n    let total = add(1, 2);\n    println!(\"{total}\");\n}\n";
        std::fs::write(&path, text).ok();
        let path = path.to_string_lossy().into_owned();
        let seen = crate::coding::Seen::default();
        let read = crate::coding::Read {
            seen: seen.clone(),
            ..crate::coding::Read::default()
        };
        let edit = Edit {
            seen,
            ..Edit::default()
        };
        let call = |edits: Vec<Replacement>| {
            let args = EditArgs::new(&path, edits);
            let mut context = ToolContext::new();
            futures::executor::block_on(Tool::call(&edit, &mut context, args))
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default()
        };
        let read_it = || {
            let args = crate::coding::ReadArgs::new(&path);
            let mut context = ToolContext::new();
            futures::executor::block_on(Tool::call(&read, &mut context, args)).ok()
        };
        assert!(read_it().is_some());

        let missed = call(vec![
            Replacement::new("fn main() {", "fn main() -> () {"),
            Replacement::new(
                "    let total = add(1, 3);\n",
                "    let total = add(2, 3);\n",
            ),
        ]);
        assert!(
        missed.contains("edits[1]: `old_text` was not found")
            && missed.contains("copy it from the closest text in the file, shown below")
            && missed.contains(
                "The closest text to the `old_text` of edits[1] is line 2:\n    let total = add(1, 2);"
            )
            && !missed.contains("read it again"),
        "{missed}"
    );

        std::fs::write(&path, text.replace("add(1, 2)", "sum(1, 2)")).ok();
        let changed = call(vec![Replacement::new("add(1, 2)", "add(2, 3)")]);
        assert!(
            changed.contains("changed since it was last read, so read it again"),
            "{changed}"
        );
        assert!(read_it().is_some());
        let unrelated = call(vec![Replacement::new("nothing like it at all", "x")]);
        std::fs::remove_file(&path).ok();
        assert!(
            unrelated.contains("nothing in the file is close to it")
                && !unrelated.contains("read it again"),
            "{unrelated}"
        );
    }

    /// Edits apply in order, each to the text the ones before it left, so two
    /// that touch the same lines, or a `replace_all` and an edit of one of its
    /// matches, both apply. An edit whose text an earlier one took away is
    /// refused naming that one.
    #[test]
    fn edits_apply_in_order_even_where_they_overlap() {
        let text = "fn area(w: u32, h: u32) -> u32 {\n    w * h\n}\n";
        let same_lines = [
            Replacement::new(
                "fn area(w: u32, h: u32) -> u32 {\n    w * h",
                "fn area(w: u64, h: u64) -> u64 {\n    w * h",
            ),
            Replacement::new("    w * h", "    w.saturating_mul(h)"),
        ];
        let applied = apply("a.rs", text, &same_lines).map(|applied| applied.text);
        assert_eq!(
            applied.ok().as_deref(),
            Some("fn area(w: u64, h: u64) -> u64 {\n    w.saturating_mul(h)\n}\n")
        );
        let all_then_one = [
            Replacement::new("u32", "u64").all(),
            Replacement::new("-> u64 {", "-> Option<u64> {"),
        ];
        let applied = apply("a.rs", text, &all_then_one).map(|applied| applied.replaced);
        assert_eq!(applied.ok(), Some(4));

        let taken = [
            Replacement::new("u32", "u64").all(),
            Replacement::new("-> u32 {", "-> Option<u32> {"),
        ];
        let refused = apply("a.rs", text, &taken)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            refused.starts_with(
                "edits[1]: `old_text` was in the file, but edits[0], applied before it, changed \
             that text; match the text as edits[0] leaves it, or make this change in edits[0]. \
             No edit was applied"
            ),
            "{refused}"
        );
    }
}
