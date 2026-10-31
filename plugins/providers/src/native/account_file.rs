//! Native provider-state file mechanics. Domain authority, qualified recovery
//! observation and a bound AccountStore implementation are separate requirements.

mod state;

use std::path::{Path, PathBuf};

fn failure(message: &str) -> tembra::Error {
    tembra::Error::new(
        tembra::Code::Unavailable,
        "providers.account_store",
        message,
    )
}

fn validate_path(path: &Path) -> tembra::Result<()> {
    if path.file_name().is_none() {
        return Err(tembra::Error::invalid(
            "providers.account_store",
            "an account file path is required",
        ));
    }
    Ok(())
}

/// An unbound provider-state file helper. Construction performs no I/O and
/// grants no profile, initialization or recovery authority. The inherent typed
/// methods require an embedding-supplied physical owner and trusted bindings.
/// This helper intentionally does not implement the recovery AccountStore port.
pub struct NativeAccountFile {
    path: PathBuf,
}

impl NativeAccountFile {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}
