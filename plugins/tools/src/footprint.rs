//! What a tool call may read and change ([`Touches`]), so a host runs the
//! calls of one reply that touch different things side by side, and holds
//! back only those that conflict.
//!
//! ```
//! use std::path::PathBuf;
//! use swarmie_tools::{Scope, Touches};
//!
//! let paths = |path: &str| Scope::Paths(vec![PathBuf::from(path)]);
//! let edit = Touches::Writes(paths("/work/src/a.rs"));
//! assert!(!edit.conflicts_with(&Touches::Reads(paths("/work/src/b.rs"))));
//! assert!(edit.conflicts_with(&Touches::Reads(paths("/work/src"))));
//! assert!(edit.conflicts_with(&Touches::Independent));
//! assert!(!Touches::Reads(Scope::Anything).conflicts_with(&Touches::Independent));
//! ```

use std::path::{Component, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ToolAnnotations;

/// What a tool call may read and change, so calls that touch different
/// things run side by side.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Touches {
    /// Reads what the scope names and changes nothing; replay still needs provider confirmation after a
    /// restart only when the provider confirms that retry is safe.
    Reads(Scope),
    /// Reads and may change what the scope names.
    Writes(Scope),
    /// Changes nothing the other calls of its reply see, such as handing
    /// work to another agent. It may read anything, so it waits behind an
    /// earlier writer; it is never run again after a restart.
    Independent,
}

/// What a footprint names: these paths (absolute; a directory covers what
/// is under it), or anything at all. A guard compares paths as they are,
/// so a host makes a footprint [`normalized`](Touches::normalized) before
/// any guard sees it; the host chooses when to normalize it.
///
/// Normalizing folds `.` and `..` by the text alone and follows no link,
/// so a tool declares canonical paths: the files it will open once links
/// are resolved, as the coding tools do through their workspace's
/// `canonical`. A guard that must not be fooled by a link a tool did not
/// resolve canonicalizes the paths it compares itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// These files and directories, canonical and absolute.
    Paths(Vec<PathBuf>),
    /// Anything at all.
    Anything,
}

impl Scope {
    /// Whether the two name something in common: a path is, or is under, a
    /// path of the other, compared component by component; `Anything`
    /// meets everything.
    pub fn overlaps(&self, other: &Scope) -> bool {
        match (self, other) {
            (Self::Paths(ours), Self::Paths(theirs)) => ours.iter().any(|ours| {
                theirs
                    .iter()
                    .any(|theirs| ours.starts_with(theirs) || theirs.starts_with(ours))
            }),
            (Self::Anything, _) | (_, Self::Anything) => true,
        }
    }

    /// The scope with each path's `.` and `..` folded away, by the path's
    /// text alone, so `/work/src/../../etc` names `/etc`, as a guard
    /// comparing paths must see it. A relative path names no place a guard
    /// can check, so a scope with one is [`Anything`](Self::Anything).
    pub fn normalized(self) -> Scope {
        match self {
            Self::Paths(paths) => paths
                .into_iter()
                .map(normal)
                .collect::<Option<Vec<_>>>()
                .map_or(Self::Anything, Self::Paths),
            Self::Anything => Self::Anything,
        }
    }
}

/// `path` with its `.` and `..` folded away, or `None` when it is relative.
/// `..` at the root stays at the root.
fn normal(path: PathBuf) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normal.pop();
            }
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normal.push(component);
            }
        }
    }
    Some(normal)
}

impl Touches {
    /// Whether a call touching `self` must not run beside one touching
    /// `other`: one of them writes what the other reads or writes.
    /// [`Independent`](Self::Independent) counts as `Reads(Anything)` here.
    pub fn conflicts_with(&self, other: &Touches) -> bool {
        let (ours, we_write) = self.parts();
        let (theirs, they_write) = other.parts();
        (we_write || they_write) && ours.overlaps(theirs)
    }

    /// Whether it only reads: [`Reads`](Self::Reads), and nothing else.
    pub fn reads_only(&self) -> bool {
        matches!(self, Self::Reads(_))
    }

    /// The footprint with its scope [`normalized`](Scope::normalized).
    pub fn normalized(self) -> Touches {
        match self {
            Self::Reads(scope) => Self::Reads(scope.normalized()),
            Self::Writes(scope) => Self::Writes(scope.normalized()),
            Self::Independent => Self::Independent,
        }
    }

    /// From the annotations a tool's own code declares, such as a
    /// [`Tool`](super::Tool)'s: `Reads(Anything)` when it says it only
    /// reads, else `Writes(Anything)`. For annotations from elsewhere, such
    /// as an MCP server's, [`from_hints`](Self::from_hints).
    pub fn from_annotations(annotations: &ToolAnnotations) -> Self {
        match annotations.read_only() {
            true => Self::Reads(Scope::Anything),
            false => Self::Writes(Scope::Anything),
        }
    }

    /// From unverified annotations, such as those an MCP server
    /// sends: a tool that says it only reads is
    /// [`Independent`](Self::Independent), so its calls still run beside
    /// readers but are never run again after a restart; any other is
    /// `Writes(Anything)`. Only a local declaration makes a call
    /// [`Reads`](Self::Reads).
    pub fn from_hints(annotations: &ToolAnnotations) -> Self {
        match annotations.read_only() {
            true => Self::Independent,
            false => Self::Writes(Scope::Anything),
        }
    }

    /// What it names, and whether it writes there.
    fn parts(&self) -> (&Scope, bool) {
        match self {
            Self::Reads(scope) => (scope, false),
            Self::Writes(scope) => (scope, true),
            Self::Independent => (&Scope::Anything, false),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Scope, Touches};
    use crate::ToolAnnotations;

    fn paths(paths: &[&str]) -> Scope {
        Scope::Paths(paths.iter().map(PathBuf::from).collect())
    }

    /// Two calls conflict when one writes what the other reads or writes:
    /// paths meet when one is the other or under it, `Anything` meets all,
    /// and `Independent` reads anything.
    #[test]
    fn the_conflict_table() {
        let read_a = Touches::Reads(paths(&["/w/a.rs"]));
        let read_b = Touches::Reads(paths(&["/w/b.rs"]));
        let write_a = Touches::Writes(paths(&["/w/a.rs"]));
        let write_b = Touches::Writes(paths(&["/w/b.rs"]));
        let read_dir = Touches::Reads(paths(&["/w"]));
        let read_sibling = Touches::Reads(paths(&["/w/a.rs.bak", "/x"]));
        let read_all = Touches::Reads(Scope::Anything);
        let write_all = Touches::Writes(Scope::Anything);
        let independent = Touches::Independent;

        let cases = [
            (&read_a, &read_a, false),
            (&read_a, &read_all, false),
            (&read_all, &read_all, false),
            (&read_a, &write_a, true),
            (&read_b, &write_a, false),
            (&write_a, &write_a, true),
            (&write_a, &write_b, false),
            (&write_a, &read_dir, true),
            (&write_a, &read_sibling, false),
            (&write_all, &read_b, true),
            (&write_all, &write_b, true),
            (&independent, &write_a, true),
            (&independent, &write_all, true),
            (&independent, &read_a, false),
            (&independent, &independent, false),
        ];
        for (one, other, conflict) in cases {
            assert_eq!(one.conflicts_with(other), conflict, "{one:?} / {other:?}");
            assert_eq!(other.conflicts_with(one), conflict, "{other:?} / {one:?}");
        }
    }

    /// Only `Reads` only reads; a tool's annotations give a coarse default.
    #[test]
    fn reads_only_and_the_default_from_annotations() {
        assert!(Touches::Reads(Scope::Anything).reads_only());
        assert!(!Touches::Writes(paths(&["/w"])).reads_only());
        assert!(!Touches::Independent.reads_only());
        let read_only = ToolAnnotations::default().with_read_only(true);
        assert_eq!(
            Touches::from_annotations(&read_only),
            Touches::Reads(Scope::Anything)
        );
        assert_eq!(
            Touches::from_annotations(&ToolAnnotations::default()),
            Touches::Writes(Scope::Anything)
        );
        // A remote server's word that a tool only reads never makes its calls
        // run again after a restart.
        assert_eq!(Touches::from_hints(&read_only), Touches::Independent);
        assert_eq!(
            Touches::from_hints(&ToolAnnotations::default()),
            Touches::Writes(Scope::Anything)
        );
    }

    /// A tool says what each call touches from its arguments, and its handler
    /// says it through erasure; arguments that do not parse say nothing.
    #[test]
    fn normalized_folds_dots_and_widens_relative_paths() {
        let root = if cfg!(windows) { "C:/" } else { "/" };
        let absolute = |p: &str| PathBuf::from(root).join(p);
        let scope = |p: &[&str]| Scope::Paths(p.iter().map(|p| absolute(p)).collect());
        let climbing = Touches::Writes(scope(&["work/src/../../etc/./passwd", "work/a.rs"]));
        assert_eq!(
            climbing.normalized(),
            Touches::Writes(scope(&["etc/passwd", "work/a.rs"]))
        );
        assert_eq!(
            Touches::Reads(scope(&["../x"])).normalized(),
            Touches::Reads(scope(&["x"]))
        );
        assert_eq!(
            Touches::Writes(Scope::Paths(vec![
                absolute("work/a.rs"),
                "notes.txt".into()
            ]))
            .normalized(),
            Touches::Writes(Scope::Anything)
        );
        assert_eq!(Touches::Independent.normalized(), Touches::Independent);
    }
}
