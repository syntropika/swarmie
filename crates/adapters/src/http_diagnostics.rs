//! Bounded wire recording and activity diagnostics for HTTP/SSE adapters.
use swarmie::{BackendEvent, Error, EventKind, Result};
use process::{Recorder, WireRecording};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinHandle};

pub(super) struct HttpDiagnostics {
    recorder: Option<Recorder>,
    events: mpsc::Sender<BackendEvent>,
    activity: Mutex<Activity>,
    monitor: Mutex<Option<JoinHandle<()>>>,
}
struct Activity {
    last: Instant,
    requests: usize,
    active: bool,
    warned: bool,
}
pub(super) struct RequestActivity(Arc<HttpDiagnostics>);
impl Drop for RequestActivity {
    fn drop(&mut self) {
        let mut state = self.0.activity.lock().unwrap();
        state.requests = state.requests.saturating_sub(1);
    }
}
impl Drop for HttpDiagnostics {
    fn drop(&mut self) {
        if let Some(task) = self.monitor.lock().unwrap().take() {
            task.abort();
        }
    }
}
impl HttpDiagnostics {
    pub fn open(
        config: &Value,
        secrets: &BTreeMap<String, String>,
        events: mpsc::Sender<BackendEvent>,
    ) -> Result<Arc<Self>> {
        let settings = &config["diagnostics"];
        if !settings.is_null() && !settings.is_object() {
            return Err(Error::Invalid("diagnostics must be an object".into()));
        }
        let timeout = settings
            .get("stallTimeoutMs")
            .filter(|value| !value.is_null())
            .map(|value| {
                let millis = value.as_u64().ok_or_else(|| {
                    Error::Invalid("stallTimeoutMs must be a positive integer".into())
                })?;
                if millis < 10 {
                    return Err(Error::Invalid(
                        "stall timeout must be at least 10 ms".into(),
                    ));
                }
                Ok(Duration::from_millis(millis))
            })
            .transpose()?;
        let mut recording_notice = None;
        let recorder = if let Some(recording) =
            settings.get("recording").filter(|value| !value.is_null())
        {
            let path = recording["path"]
                .as_str()
                .filter(|path| !path.is_empty())
                .ok_or_else(|| Error::Invalid("wire recording requires a nonempty path".into()))?;
            let max_bytes = match recording.get("maxBytes") {
                None => 8 * 1024 * 1024,
                Some(value) => value.as_u64().ok_or_else(|| {
                    Error::Invalid("wire recording maxBytes must be an integer".into())
                })?,
            };
            if max_bytes == 0 || max_bytes > 64 * 1024 * 1024 {
                return Err(Error::Invalid(
                    "wire recording budget must be between 1 byte and 64 MiB".into(),
                ));
            }
            let requested = PathBuf::from(path);
            let fresh = || {
                let stem = requested
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("trace");
                requested.with_file_name(format!("{stem}-{}.jsonl", uuid::Uuid::new_v4()))
            };
            let mut settings = WireRecording {
                path: if requested.exists() {
                    fresh()
                } else {
                    requested.clone()
                },
                max_bytes: max_bytes as usize,
            };
            let mut opened = Recorder::open(&settings, secrets);
            if matches!(&opened, Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::AlreadyExists)
            {
                settings.path = fresh();
                opened = Recorder::open(&settings, secrets);
            }
            match opened {
                Ok(recorder) => {
                    recording_notice = Some(
                        json!({"level":"info","code":"wire_recording_started","message":"Wire recording started","path":settings.path}),
                    );
                    Some(recorder)
                }
                Err(error) => {
                    recording_notice = Some(
                        json!({"level":"warning","code":"wire_recording_failed","message":format!("Wire recording could not be opened: {error}; the provider connection continues."),"path":settings.path}),
                    );
                    None
                }
            }
        } else {
            None
        };
        let diagnostics = Arc::new(Self {
            recorder,
            events,
            activity: Mutex::new(Activity {
                last: Instant::now(),
                requests: 0,
                active: false,
                warned: false,
            }),
            monitor: Mutex::new(None),
        });
        if let Some(notice) = recording_notice {
            diagnostics.notice(notice);
        }
        if let Some(timeout) = timeout {
            let weak = Arc::downgrade(&diagnostics);
            let task = tokio::spawn(async move {
                let mut interval = tokio::time::interval(timeout.min(Duration::from_millis(250)));
                loop {
                    interval.tick().await;
                    let Some(diagnostics) = weak.upgrade() else {
                        break;
                    };
                    let mut state = diagnostics.activity.lock().unwrap();
                    if (state.active || state.requests > 0)
                        && !state.warned
                        && state.last.elapsed() >= timeout
                    {
                        state.warned = true;
                        diagnostics.notice(json!({"level":"warning","code":"wire_stalled","message":"The provider wire is inactive while an operation is outstanding; it has not been cancelled.","inactiveMs":state.last.elapsed().as_millis() as u64}));
                    }
                }
            });
            *diagnostics.monitor.lock().unwrap() = Some(task);
        }
        Ok(diagnostics)
    }
    fn notice(&self, data: Value) {
        // Diagnostics do not block the provider or become provider failures.
        let _ = self.events.try_send(BackendEvent::Content {
            kind: Box::new(EventKind::Diagnostic(data)),
            raw: None,
        });
    }
    pub fn record(&self, direction: &str, frame: &Value) {
        if let Some(recorder) = &self.recorder
            && let Some(warning) = recorder.record(direction, &sanitize_urls(frame))
        {
            self.notice(warning);
        }
    }
    pub fn request(self: &Arc<Self>) -> RequestActivity {
        self.activity();
        self.activity.lock().unwrap().requests += 1;
        RequestActivity(self.clone())
    }
    pub fn activity(&self) {
        let mut state = self.activity.lock().unwrap();
        if state.warned {
            self.notice(json!({"level":"info","code":"wire_resumed","message":"Provider wire activity resumed."}));
        }
        state.last = Instant::now();
        state.warned = false;
    }
    pub fn turn(&self, active: bool) {
        let mut state = self.activity.lock().unwrap();
        state.active = active;
        state.last = Instant::now();
        state.warned = false;
    }
}
fn sanitize_urls(value: &Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), sanitize_urls(value)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(sanitize_urls).collect()),
        Value::String(value) if value.starts_with("http://") || value.starts_with("https://") => {
            if let Ok(mut url) = reqwest::Url::parse(value) {
                let _ = url.set_username("");
                let _ = url.set_password(None);
                url.set_query(None);
                url.set_fragment(None);
                json!(url.to_string())
            } else {
                json!("[REDACTED_URL]")
            }
        }
        other => other.clone(),
    }
}
