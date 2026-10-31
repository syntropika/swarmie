use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};
/// Most lines a tool returns.
pub const MAX_LINES: usize = 2000;
/// Most bytes a tool returns.
pub const MAX_BYTES: usize = 50 * 1024;
/// The largest file `read` and `edit` load whole, and `search` scans.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// Characters of one line `read` and `shell` show; the rest of a longer
/// line is cut.
pub(super) const MAX_LINE_CHARS: usize = 2000;
/// What marks a line that was cut.
pub(super) const CUT_MARK: &str = " […]";

/// How much of a long output a tool keeps: at most `lines` lines and
/// `bytes` bytes, of whole lines. [`tail`](Self::tail) keeps the last ones;
/// [`Shell`](crate::coding::Shell) keeps a quarter for the first lines, a quarter for the errors
/// and failures from between, and the rest for the last lines.
///
/// ```
/// use swarmie_tools::coding::OutputLimit;
///
/// let cut = OutputLimit { lines: 2, bytes: 1024 };
/// assert_eq!(cut.tail("one\ntwo\nthree\n"), ("two\nthree\n", 1));
/// let narrow = OutputLimit { lines: 10, bytes: 7 };
/// assert_eq!(narrow.tail("one\ntwo\nthree\n"), ("three\n", 2));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputLimit {
    /// Most lines kept.
    pub lines: usize,
    /// Most bytes kept.
    pub bytes: usize,
}

impl Default for OutputLimit {
    /// [`MAX_LINES`] lines or [`MAX_BYTES`] bytes.
    fn default() -> Self {
        Self {
            lines: MAX_LINES,
            bytes: MAX_BYTES,
        }
    }
}

impl OutputLimit {
    /// The end of `text` this keeps, starting at a line, and how many lines
    /// before it were left out. When even the last line is longer than
    /// [`bytes`](Self::bytes), nothing is kept.
    pub fn tail<'t>(&self, text: &'t str) -> (&'t str, usize) {
        let starts: Vec<usize> = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(at, _)| at + 1))
            .filter(|start| *start < text.len())
            .collect();
        let first = starts.len().saturating_sub(self.lines);
        let kept = starts
            .iter()
            .enumerate()
            .skip(first)
            .find(|(_, start)| text.len() - **start <= self.bytes);
        match kept {
            Some((dropped, start)) => (text.get(*start..).unwrap_or_default(), dropped),
            None => ("", starts.len()),
        }
    }
}

/// `line` cut to its first `limit` characters.
pub fn clip(line: &str, limit: usize) -> &str {
    match line.char_indices().nth(limit) {
        Some((end, _)) => line.get(..end).unwrap_or(line),
        None => line,
    }
}

/// `text` quoted for `sh`.
pub(crate) fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// `path`, a path a file tool resolved, quoted for a command in `shell`:
/// `None` when it is not absolute for `sh`, such as a remote `~/` path, a
/// `HOST:PATH` or a Windows path, which `shell` would not find as written.
pub(crate) fn shell_path(path: &str) -> Option<String> {
    (cfg!(unix) && path.starts_with('/')).then(|| quote(path))
}

/// `text` cut to fit in `width` terminal columns, ending in `…` when cut:
/// a wide character, such as most CJK ones, takes two columns.
///
/// ```
/// use swarmie_tools::coding::shorten;
///
/// assert_eq!(shorten("a short label", 20), "a short label");
/// assert_eq!(shorten("a longer label", 9), "a longer…");
/// assert_eq!(shorten("日本語のラベル", 7), "日本語…");
/// ```
pub fn shorten(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let room = width.saturating_sub(1);
    let mut used = 0;
    let end = text
        .char_indices()
        .find(|(_, character)| {
            used += character.width().unwrap_or(0);
            used > room
        })
        .map_or(text.len(), |(at, _)| at);
    match width {
        0 => String::new(),
        _ => format!("{}…", text.get(..end).unwrap_or_default()),
    }
}

/// How long ago `elapsed` was, roughly, for a list a person reads:
/// `just now`, `3m ago`, `5h ago`, `2d ago`.
///
/// ```
/// use std::time::Duration;
/// use swarmie_tools::coding::ago;
///
/// assert_eq!(ago(Duration::from_secs(30)), "just now");
/// assert_eq!(ago(Duration::from_secs(5 * 3600 + 59)), "5h ago");
/// ```
pub fn ago(elapsed: std::time::Duration) -> String {
    let seconds = elapsed.as_secs();
    match seconds {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{}m ago", seconds / 60),
        3600..86_400 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr as _;

    use crate::{Scope, Tool, Touches};

    use crate::coding::{
        Edit, EditArgs, OutputLimit, Read, ReadArgs, Search, SearchArgs, Shell, ShellArgs, Write,
        WriteArgs, shorten,
    };

    /// A cut keeps whole lines from the end, within both limits, and says how
    /// many it left out; a last line longer than the byte limit leaves nothing.
    #[test]
    fn a_cut_keeps_whole_lines_from_the_end() {
        let text = "one\ntwo\nthree";
        let roomy = OutputLimit {
            lines: 10,
            bytes: 100,
        };
        assert_eq!(roomy.tail(text), (text, 0));
        assert_eq!(roomy.tail(""), ("", 0));
        let two = OutputLimit {
            lines: 2,
            bytes: 100,
        };
        assert_eq!(two.tail(text), ("two\nthree", 1));
        let narrow = OutputLimit {
            lines: 10,
            bytes: 3,
        };
        assert_eq!(narrow.tail(text), ("", 3));
        assert_eq!(OutputLimit::default().tail("a\n"), ("a\n", 0));
    }

    /// A shortened text fits its width in terminal columns, the ellipsis
    /// included, however wide its characters are.
    #[test]
    fn shorten_fits_the_width() {
        for text in [
            "plain ascii text",
            "日本語のテキストです",
            "mixed 漢字 and ascii",
        ] {
            for width in 0..24 {
                let short = shorten(text, width);
                assert!(short.width() <= width, "{short:?} in {width}");
                assert!(text.starts_with(short.trim_end_matches('…')));
            }
        }
        assert_eq!(shorten("日本語", 5), "日本…");
    }

    /// Each call's footprint names the path it reads or writes, resolved
    /// against the tool's base; a search without a path reads the base, and a
    /// shell call may change anything.
    #[test]
    fn each_call_names_what_it_touches() {
        let scratch = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(scratch.path()).unwrap();
        let at = |path: &str| Scope::Paths(vec![base.join(path)]);
        let read = Read {
            base: Some(base.clone()),
            confine: false,
            seen: crate::coding::Seen::default(),
            ..Default::default()
        };
        let search = Search {
            base: Some(base.clone()),
            confine: false,
            ..Default::default()
        };
        let edit = Edit {
            base: Some(base.clone()),
            confine: false,
            seen: crate::coding::Seen::default(),
            ..Default::default()
        };
        let write = Write {
            base: Some(base.clone()),
            confine: false,
            ..Default::default()
        };
        assert_eq!(
            read.footprint(&ReadArgs::new("src/../a.rs")),
            Touches::Reads(at("a.rs"))
        );
        assert_eq!(
            search.footprint(&SearchArgs::new("x").in_path("src")),
            Touches::Reads(at("src"))
        );
        assert_eq!(
            search.footprint(&SearchArgs::new("x")),
            Touches::Reads(Scope::Paths(vec![base.clone()]))
        );
        assert_eq!(
            edit.footprint(&EditArgs::new("a.rs", [])),
            Touches::Writes(at("a.rs"))
        );
        assert_eq!(
            write.footprint(&WriteArgs::new(base.join("b.rs").to_string_lossy(), "")),
            Touches::Writes(at("b.rs"))
        );
        assert_eq!(
            Shell::default().footprint(&ShellArgs::new("ls")),
            Touches::Writes(Scope::Anything)
        );
    }
}
