use std::{ffi::OsString, path::PathBuf, time::Duration};

use tembra::{Result, RuntimeError};

pub const HELP: &str = "runtime-diagnostic --output <fresh-external-directory> [--repetitions <1..10000>] [--observer wait|store] [--diagnostic-timeout-secs <1..3600>]\n\nDefault: one turn, actual DurableTasks.wait, 30-second cooperative diagnostic cap per phase.\nScheduler is fixed at 2ms. The optional store observer reads immediately, then every nominal 250ms.\nOutput parent must exist; output must be fresh and outside the source checkout.\nNDJSON phase/result records are flushed to stdout. Raw databases and JSON results are retained.\nLinux reports CLOCK_THREAD_CPUTIME_ID and native TID; other platforms report null.\nDiagnostic caps do not replace acceptance-test deadlines or a wrapper process watchdog.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observer {
    Wait,
    Store,
}

impl Observer {
    pub fn name(self) -> &'static str {
        match self {
            Self::Wait => "wait",
            Self::Store => "store",
        }
    }
}

pub struct Options {
    pub output: PathBuf,
    pub repetitions: u32,
    pub observer: Observer,
    pub timeout: Duration,
}

impl Options {
    pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Option<Self>> {
        let mut output = None;
        let mut repetitions = None;
        let mut observer = None;
        let mut timeout = None;
        let mut arguments = arguments.into_iter();
        while let Some(flag) = arguments.next() {
            if flag == "--help" || flag == "-h" {
                return Ok(None);
            }
            let value = arguments
                .next()
                .ok_or_else(|| invalid("missing flag value"))?;
            match flag.to_str() {
                Some("--output") if output.is_none() => output = Some(PathBuf::from(value)),
                Some("--repetitions") if repetitions.is_none() => {
                    repetitions = Some(number(value, 10_000)? as u32);
                }
                Some("--observer") if observer.is_none() => {
                    observer = Some(match value.to_str() {
                        Some("wait") => Observer::Wait,
                        Some("store") => Observer::Store,
                        _ => return Err(invalid("observer must be wait or store")),
                    });
                }
                Some("--diagnostic-timeout-secs") if timeout.is_none() => {
                    timeout = Some(Duration::from_secs(number(value, 3600)?));
                }
                _ => return Err(invalid("unknown or repeated flag")),
            }
        }
        Ok(Some(Self {
            output: output.ok_or_else(|| invalid("--output is required"))?,
            repetitions: repetitions.unwrap_or(1),
            observer: observer.unwrap_or(Observer::Wait),
            timeout: timeout.unwrap_or(Duration::from_secs(30)),
        }))
    }
}

fn number(value: OsString, maximum: u64) -> Result<u64> {
    value
        .to_str()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (1..=maximum).contains(value))
        .ok_or_else(|| invalid("numeric flag is outside its documented bounds"))
}

fn invalid(message: &str) -> RuntimeError {
    RuntimeError::invalid("diagnostic_arguments", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Result<Option<Options>> {
        Options::parse(arguments.iter().map(OsString::from))
    }

    #[test]
    fn refuses_unbounded_or_ambiguous_work() {
        for arguments in [
            vec!["--output", "result", "--repetitions", "0"],
            vec!["--output", "result", "--repetitions", "10001"],
            vec!["--output", "result", "--diagnostic-timeout-secs", "3601"],
            vec!["--output", "result", "--observer", "unknown"],
            vec!["--output", "one", "--output", "two"],
            vec!["--output"],
        ] {
            assert!(parse(&arguments).is_err(), "{arguments:?}");
        }
        assert!(parse(&[]).is_err());
        assert!(parse(&["--help"]).unwrap().is_none());
        let options = parse(&["--output", "result"]).unwrap().unwrap();
        assert_eq!(options.observer, Observer::Wait);
        assert_eq!(options.repetitions, 1);
        assert_eq!(options.timeout, Duration::from_secs(30));
    }
}
