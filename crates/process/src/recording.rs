use mpai::{Error, Result};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Mutex,
};

#[derive(Clone, Debug)]
pub struct WireRecording {
    pub path: PathBuf,
    pub max_bytes: usize,
}
pub struct Recorder {
    state: Mutex<State>,
    secrets: Vec<String>,
    max_bytes: usize,
}
struct State {
    file: File,
    bytes: usize,
    disabled: bool,
}
impl Recorder {
    pub fn open(options: &WireRecording, env: &BTreeMap<String, String>) -> Result<Self> {
        if options.max_bytes == 0 || options.max_bytes > 128 * 1024 * 1024 {
            return Err(Error::Invalid(
                "wire recording budget must be between 1 byte and 128 MiB".into(),
            ));
        }
        let mut open = OpenOptions::new();
        open.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            open.mode(0o600);
        }
        let file = open.open(&options.path)?;
        let mut secrets = env
            .values()
            .filter(|v| !v.is_empty())
            .cloned()
            .collect::<Vec<_>>();
        secrets.extend(
            std::env::vars_os()
                .filter_map(|(key, value)| {
                    Some((key.to_str()?.to_owned(), value.to_str()?.to_owned()))
                })
                .filter(|(key, _)| secret_field(key))
                .map(|(_, value)| value)
                .filter(|v| !v.is_empty()),
        );
        secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
        secrets.dedup();
        Ok(Self {
            state: Mutex::new(State {
                file,
                bytes: 0,
                disabled: false,
            }),
            secrets,
            max_bytes: options.max_bytes,
        })
    }
    pub fn record(&self, direction: &str, frame: &Value) -> Option<Value> {
        let mut state = self.state.lock().unwrap();
        if state.disabled {
            return None;
        }
        let frame = redact(frame, &self.secrets);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut bytes = serde_json::to_vec(
            &json!({"direction":direction,"timestampMs":timestamp,"frame":frame}),
        )
        .expect("JSON values serialize");
        bytes.push(b'\n');
        if bytes.len() > self.max_bytes.saturating_sub(state.bytes) {
            state.disabled = true;
            return Some(
                json!({"level":"warning","code":"wire_recording_full","message":"Wire recording reached its byte budget; recording stopped while the provider connection continues."}),
            );
        }
        if let Err(error) = state
            .file
            .write_all(&bytes)
            .and_then(|_| state.file.flush())
        {
            state.disabled = true;
            return Some(
                json!({"level":"warning","code":"wire_recording_failed","message":format!("Wire recording failed: {error}; the provider connection continues.")}),
            );
        }
        state.bytes += bytes.len();
        None
    }
}
fn secret_field(key: &str) -> bool {
    let key = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    if [
        "inputtokens",
        "outputtokens",
        "cachedinputtokens",
        "cachewritetokens",
        "totaltokens",
        "maxoutputtokens",
        "usedtokens",
        "windowtokens",
        "contextwindowtokens",
        "remainingtokens",
    ]
    .contains(&key.as_str())
    {
        return false;
    }
    key.contains("password")
        || key.contains("secret")
        || key.contains("token")
        || key.contains("apikey")
        || key.contains("authorization")
        || key.contains("credential")
        || key.contains("privatekey")
        || key.contains("accesskey")
        || [
            "headers",
            "env",
            "environment",
            "authentication",
            "auth",
            "config",
            "cookie",
            "cookies",
            "setcookie",
        ]
        .contains(&key.as_str())
}
fn redact(value: &Value, secrets: &[String]) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        if secret_field(key) {
                            json!("[REDACTED]")
                        } else {
                            redact(value, secrets)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(|v| redact(v, secrets)).collect()),
        Value::String(value) => {
            let mut value = value.clone();
            for secret in secrets {
                if value == *secret {
                    value = "[REDACTED]".into();
                    break;
                }
                if secret.len() >= 4 {
                    value = value.replace(secret, "[REDACTED]");
                }
            }
            if value.starts_with("Bearer ") || value.starts_with("Basic ") {
                value = "[REDACTED]".into();
            }
            Value::String(value)
        }
        other => other.clone(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    fn write_failure_disables_recording_with_visible_warning() {
        let folder = tempfile::tempdir().unwrap();
        let recorder = Recorder::open(
            &WireRecording {
                path: folder.path().join("wire"),
                max_bytes: 1024,
            },
            &BTreeMap::new(),
        )
        .unwrap();
        recorder.state.lock().unwrap().file =
            OpenOptions::new().write(true).open("/dev/full").unwrap();
        let diagnostic = recorder.record("send", &json!({"method":"read"})).unwrap();
        assert_eq!(diagnostic["code"], "wire_recording_failed");
        assert!(recorder.record("send", &json!({"method":"read"})).is_none());
    }
    #[test]
    fn redacts_nested_credentials_and_echoed_environment_without_touching_numbers() {
        let value = redact(
            &json!({"auth":{"api_key":"secret"},"headers":{"X-Custom":"secret"},"input":"echo sensitive-value","environment":{"NORMAL":"hello"},"config":{"custom":"config-sensitive"},"cookie":"cookie-sensitive","privateKey":"key-sensitive","usage":{"inputTokens":20},"result":"Bearer abc"}),
            &["sensitive-value".into()],
        );
        let encoded = value.to_string();
        assert!(
            !encoded.contains("secret")
                && !encoded.contains("sensitive-value")
                && !encoded.contains("hello")
                && !encoded.contains("Bearer")
                && !encoded.contains("config-sensitive")
                && !encoded.contains("cookie-sensitive")
                && !encoded.contains("key-sensitive")
        );
        assert_eq!(value["usage"]["inputTokens"], 20);
    }
}
