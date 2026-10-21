//! Deterministic runtime diagnostic. Results are measurements, not acceptance-test verdicts.

mod diagnostic;

fn main() -> std::process::ExitCode {
    match diagnostic::run(std::env::args_os().skip(1)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("runtime diagnostic failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
