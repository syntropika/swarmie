//! Coding tools for Swarmie agents: [`Read`], [`Write`], [`Edit`], [`Search`], [`List`],
//! [`Shell`], [`Delete`] and [`Move`].
//!
//! - Each says in its description when to pick it, and what it may touch
//!   ([`footprint`](crate::Tool::footprint)): `read`, `search` and `list` read the
//!   path they name, the other file tools write theirs, and `shell` may change anything, so a
//!   host runs calls on different files side by side.
//! - Each publishes what it did beside its text: files read
//!   ([`FilesRead`](crate::FilesRead)), [`SearchHits`], [`FilesListed`], files changed
//!   with their diffs ([`FilesChanged`](crate::FilesChanged)), how a command ended
//!   ([`ExitStatus`](crate::ExitStatus)).
//! - Each acts on a [`Workspace`](workspace::Workspace): this machine
//!   ([`Local`](workspace::Local), the default) or another. The file tools resolve paths from an
//!   optional base directory they can be confined to. A path whose first component is `~` is in
//!   the home directory; `./~` is a folder named `~`.
//! - `shell` reports its output as it comes to a [`Live`]. Arguments are plain data with public
//!   fields.
//!
//! Hosts select the workspace, process launcher and admission policy explicitly.
//! Path confinement is not a process sandbox.
//!
//! Native only: the tools use the file system and processes.

mod blocking;
mod delete;
pub mod edit;
pub mod fs;
mod install;
mod list;
mod live;
mod move_file;
pub mod process;
mod read;
pub mod redact;
mod search;
mod shell;
mod spill;
pub mod workspace;
mod write;

pub use blocking::blocking;
pub use delete::{Delete, DeleteArgs};
pub use edit::{Edit, EditArgs, Replacement};
pub use install::{CodingConfig, CodingTools};
pub use list::{FilesListed, List, ListArgs};
pub use live::Live;
pub use move_file::{Move, MoveArgs};
pub use read::{Read, ReadArgs, Seen, show_lines};
pub use search::{
    Found, Hit, MAX_FILES, MAX_MATCHES, Search, SearchArgs, SearchHits, find, render,
};
pub use shell::{Shell, ShellArgs, ShellRun, split_status};
pub use spill::Spill;
pub use write::{Write, WriteArgs};

mod presentation;
use presentation::{CUT_MARK, MAX_LINE_CHARS, shell_path};
pub use presentation::{MAX_BYTES, MAX_FILE_BYTES, MAX_LINES, OutputLimit, ago, clip, shorten};
