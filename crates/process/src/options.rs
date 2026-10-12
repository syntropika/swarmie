use crate::WireRecording;
use swarmie::{Error, Result};
use std::{collections::BTreeMap, time::Duration};

#[derive(Clone, Debug, Default)]
pub struct ProcessOptions {
    pub environment: BTreeMap<String, String>,
    pub recording: Option<WireRecording>,
    pub stall_timeout: Option<Duration>,
}
pub fn validate_environment(env: &BTreeMap<String, String>) -> Result<()> {
    for (key, value) in env {
        if key.is_empty() || key.contains(['=', '\0']) || value.contains('\0') {
            return Err(Error::Invalid("environment names must be nonempty and contain neither '=' nor NUL; values cannot contain NUL".into()));
        }
    }
    Ok(())
}
