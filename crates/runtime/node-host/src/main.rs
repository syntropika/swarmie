//! Explicit independent hosting. It serves ordinary Tembra byte channels only.
//! The private native operator attachment is never exported over this listener.

#[cfg(unix)]
mod local;

fn main() {
    #[cfg(unix)]
    if let Err(error) = local::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
    #[cfg(not(unix))]
    {
        eprintln!("independent node hosting is not qualified on this platform");
        std::process::exit(1);
    }
}
