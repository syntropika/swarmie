//! Source policy checks, invoked through the root Just recipes.

#![deny(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unreachable,
    clippy::unwrap_used
)]

mod manifests;
mod providers;

use std::path::Path;
use std::process::ExitCode;

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("{0}")]
    Policy(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Toml(#[from] toml::de::Error),
    #[error(transparent)]
    Syntax(#[from] syn::Error),
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Error> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| Error::Policy("xtask must be inside the workspace root".into()))?;
    let mut args = std::env::args().skip(1);
    let command = args.next();
    if args.next().is_some() {
        return Err(Error::Policy("expected one source-check command".into()));
    }
    match command.as_deref() {
        Some("check") => {
            manifests::check(root)?;
            providers::check(root)?;
        }
        Some("check-manifests") => manifests::check(root)?,
        Some("check-provider-boundary") => providers::check(root)?,
        None | Some("--help" | "-h") => {
            println!(
                "just rust-xtask <command>\n\n\
                 check                    run all source-policy checks\n\
                 check-manifests          require workspace lint inheritance\n\
                 check-provider-boundary  keep provider codecs independent of I/O"
            );
        }
        Some(other) => {
            return Err(Error::Policy(format!(
                "unknown source-check command: {other}"
            )));
        }
    }
    Ok(())
}
