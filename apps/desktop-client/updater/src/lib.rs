//! Desktop-owned installation lifecycle, statically linked into the Native SDK host.
//!
//! Checks run off the UI thread. Downloading and replacement require explicit consent;
//! Velopack owns package verification, platform replacement and relaunch.

mod engine;
mod ffi;

pub use engine::{Phase, Snapshot, Updater};
