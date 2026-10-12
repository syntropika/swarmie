use mpai::{Error, Result, Usage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub const MAX_TEXT: usize = 64 * 1024;
pub const MAX_ITEMS: usize = 128;
const MAX_STATE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Template {
    pub id: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub name: String,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptHistory {
    pub session_id: String,
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub cwd: PathBuf,
    pub text: String,
    pub truncated: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: String,
    pub harness: String,
    pub cwd: PathBuf,
    pub model: Option<String>,
    #[serde(default)]
    pub native_id: String,
    #[serde(default)]
    pub forked: bool,
    #[serde(default)]
    pub inherited_sequence: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecord {
    pub session_id: String,
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub model: Option<String>,
    pub usage: Usage,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub id: String,
    pub session_id: String,
    pub text: String,
    pub next_run_at_ms: u64,
    pub interval_seconds: Option<u64>,
    pub enabled: bool,
    pub status: String,
    pub last_run_at_ms: Option<u64>,
    pub last_result: Option<Value>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetResult {
    pub harness: Option<String>,
    pub model: Option<String>,
    pub session_id: Option<String>,
    pub status: String,
    pub text: String,
    pub truncated: bool,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FanoutJob {
    pub id: String,
    pub status: String,
    pub created_at_ms: u64,
    pub targets: Vec<TargetResult>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct State {
    pub templates: BTreeMap<String, Template>,
    pub schedules: BTreeMap<String, Schedule>,
    pub jobs: BTreeMap<String, FanoutJob>,
    pub history: Vec<PromptHistory>,
    pub sessions: BTreeMap<String, SessionRecord>,
    pub usage: Vec<UsageRecord>,
    pub history_truncated: bool,
    pub usage_truncated: bool,
    pub scanned: BTreeMap<String, u64>,
}

pub fn load<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(Error::Invalid(
            "resource state must be a regular file".into(),
        ));
    }
    if meta.len() > MAX_STATE_BYTES {
        return Err(Error::Invalid("resource state exceeds 64 MiB".into()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(Error::Invalid("resource state exceeds 64 MiB".into()));
    }
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<()> {
        let mut file = options.open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        #[cfg(unix)]
        if let Some(parent) = path.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
pub fn private_root(path: &Path) -> Result<PathBuf> {
    if path.exists() && fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(Error::Invalid("resource root must not be a symlink".into()));
    }
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(fs::canonicalize(path)?)
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
pub fn bounded(text: &str, limit: usize) -> (String, bool) {
    if text.len() <= limit {
        return (text.to_owned(), false);
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}
pub fn text<'a>(params: &'a Value, key: &str) -> Result<&'a str> {
    params[key]
        .as_str()
        .filter(|v| !v.trim().is_empty() && v.len() <= MAX_TEXT)
        .ok_or_else(|| Error::Invalid(format!("missing or oversized string {key}")))
}
pub fn identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return Err(Error::Invalid(
            "use an identifier of at most 128 letters, digits, dots, hyphens or underscores".into(),
        ));
    }
    Ok(())
}
pub fn capacity(existing: bool, len: usize) -> Result<()> {
    if !existing && len >= MAX_ITEMS {
        Err(Error::Invalid("resource collection limit reached".into()))
    } else {
        Ok(())
    }
}
