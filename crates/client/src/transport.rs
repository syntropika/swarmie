use std::{
    process::Stdio,
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
};
use tokio_util::sync::CancellationToken;

use crate::{
    boundary::{ClientError, ErrorKind, HttpConfig, Request, StdioConfig},
    decode,
};

pub(crate) struct Client {
    transport: Transport,
    timeout: Duration,
    limit: usize,
    prefix: String,
    sequence: AtomicU64,
    closed: CancellationToken,
}

enum Transport {
    Http {
        client: reqwest::Client,
        url: reqwest::Url,
        token: Option<String>,
    },
    Stdio(Arc<Mutex<Option<Process>>>),
}

struct Process {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

pub(crate) fn duration(ms: u64) -> Result<Duration, ClientError> {
    // Bound deadlines to a portable range, including platforms with narrower Instants.
    if ms == 0 || ms > 365 * 24 * 60 * 60 * 1000 {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "Deadline must be between 1 ms and one year",
        ));
    }
    Ok(Duration::from_millis(ms))
}

fn limit(bytes: u64) -> Result<usize, ClientError> {
    if bytes == 0 || bytes > 256 * 1024 * 1024 {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "Byte limit must be between 1 and 256 MiB",
        ));
    }
    Ok(bytes as usize)
}

impl Client {
    pub(crate) fn http(config: HttpConfig) -> Result<Self, ClientError> {
        let mut url = reqwest::Url::parse(&config.url).map_err(|_| {
            ClientError::new(ErrorKind::InvalidInput, "Use an HTTP or HTTPS endpoint")
        })?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "Use an HTTP or HTTPS endpoint without URL credentials",
            ));
        }
        if url.path() == "/" {
            url.set_path("/rpc");
        }
        url.set_fragment(None);
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ClientError::internal())?;
        Ok(Self::new(
            Transport::Http {
                client,
                url,
                token: config.token,
            },
            duration(config.timeout_ms)?,
            limit(config.max_response_bytes)?,
        ))
    }

    pub(crate) fn stdio(config: StdioConfig) -> Result<Self, ClientError> {
        let timeout = duration(config.timeout_ms)?;
        let limit = limit(config.max_response_bytes)?;
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| ClientError::protocol("Unable to start runtime process"))?;
        let stdin = child.stdin.take().ok_or_else(ClientError::internal)?;
        let stdout = BufReader::new(child.stdout.take().ok_or_else(ClientError::internal)?);
        Ok(Self::new(
            Transport::Stdio(Arc::new(Mutex::new(Some(Process {
                child,
                stdin,
                stdout,
            })))),
            timeout,
            limit,
        ))
    }

    fn new(transport: Transport, timeout: Duration, limit: usize) -> Self {
        Self {
            transport,
            timeout,
            limit,
            prefix: uuid::Uuid::new_v4().to_string(),
            sequence: AtomicU64::new(0),
            closed: CancellationToken::new(),
        }
    }

    pub(crate) fn closed_token(&self) -> CancellationToken {
        self.closed.clone()
    }

    pub(crate) fn result_limit(&self) -> usize {
        self.limit
    }

    pub(crate) fn cancel(&self) {
        self.closed.cancel();
    }

    pub(crate) async fn close(&self) {
        self.cancel();
        if let Transport::Stdio(state) = &self.transport
            && let Some(mut process) = state.lock().await.take()
        {
            let _ = process.child.kill().await;
        }
    }

    pub(crate) async fn request(
        &self,
        request: Request,
        cancel: &CancellationToken,
    ) -> Result<Value, ClientError> {
        if self.closed.is_cancelled() {
            return Err(ClientError::new(
                ErrorKind::Closed,
                "Runtime client is closed",
            ));
        }
        let timeout = request
            .timeout_ms
            .map(duration)
            .transpose()?
            .unwrap_or(self.timeout)
            .min(self.timeout);
        let params: Value = serde_json::from_str(&request.params_json).map_err(|_| {
            ClientError::new(ErrorKind::InvalidInput, "Request params are not JSON")
        })?;
        let id = format!(
            "{}-{}",
            self.prefix,
            self.sequence.fetch_add(1, Ordering::Relaxed)
        );
        let frame = json!({"jsonrpc":"2.0", "id":id, "method":request.method, "params":params});
        let mut guard = match &self.transport {
            Transport::Stdio(state) => Some(AbortProcess {
                state: state.clone(),
                closed: self.closed.clone(),
                armed: true,
            }),
            Transport::Http { .. } => None,
        };
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(ClientError::new(ErrorKind::Cancelled, "Runtime request cancelled")),
            _ = self.closed.cancelled() => Err(ClientError::new(ErrorKind::Closed, "Runtime client is closed")),
            value = tokio::time::timeout(timeout, self.exchange(frame, &id)) => value.unwrap_or_else(|_| Err(ClientError::new(ErrorKind::Timeout, "Runtime request timed out"))),
        };
        // A cancelled owned stream cannot safely correlate a later response with another call.
        if result
            .as_ref()
            .err()
            .is_some_and(|error| !matches!(error.kind, ErrorKind::Rpc | ErrorKind::InvalidInput))
            && matches!(self.transport, Transport::Stdio(_))
        {
            self.close().await;
        }
        if let Some(guard) = &mut guard {
            guard.armed = false;
        }
        result
    }

    async fn exchange(&self, frame: Value, id: &str) -> Result<Value, ClientError> {
        let bytes = match &self.transport {
            Transport::Http { client, url, token } => {
                let mut request = client.post(url.clone()).json(&frame);
                if let Some(token) = token {
                    request = request.bearer_auth(token);
                }
                let response = request
                    .send()
                    .await
                    .map_err(|_| ClientError::protocol("Runtime HTTP transport failed"))?;
                if !response.status().is_success() {
                    return Err(ClientError::protocol(format!(
                        "Runtime HTTP status {}",
                        response.status().as_u16()
                    )));
                }
                let mut bytes = Vec::new();
                let mut stream = response.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    let chunk =
                        chunk.map_err(|_| ClientError::protocol("Runtime response read failed"))?;
                    if bytes.len().saturating_add(chunk.len()) > self.limit {
                        return Err(ClientError::protocol(
                            "Runtime response exceeded the configured byte limit",
                        ));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                bytes
            }
            Transport::Stdio(state) => {
                let mut guard = state.lock().await;
                let process = guard.as_mut().ok_or_else(|| {
                    ClientError::new(ErrorKind::Closed, "Runtime process is closed")
                })?;
                let mut bytes = serde_json::to_vec(&frame).map_err(|_| ClientError::internal())?;
                bytes.push(b'\n');
                process
                    .stdin
                    .write_all(&bytes)
                    .await
                    .map_err(|_| ClientError::protocol("Runtime process write failed"))?;
                process
                    .stdin
                    .flush()
                    .await
                    .map_err(|_| ClientError::protocol("Runtime process write failed"))?;
                loop {
                    let mut frame = Vec::new();
                    loop {
                        let chunk =
                            process.stdout.fill_buf().await.map_err(|_| {
                                ClientError::protocol("Runtime process read failed")
                            })?;
                        if chunk.is_empty() {
                            return Err(ClientError::protocol("Runtime process exited"));
                        }
                        let end = chunk.iter().position(|byte| *byte == b'\n');
                        let count = end.map_or(chunk.len(), |index| index + 1);
                        if frame.len().saturating_add(count) > self.limit {
                            return Err(ClientError::protocol(
                                "Runtime stdio frame exceeded the configured byte limit",
                            ));
                        }
                        frame.extend_from_slice(&chunk[..count]);
                        process.stdout.consume(count);
                        if end.is_some() {
                            break;
                        }
                    }
                    if frame.iter().all(u8::is_ascii_whitespace) {
                        continue;
                    }
                    let value: Value = serde_json::from_slice(&frame)
                        .map_err(|_| ClientError::protocol("Runtime response is not JSON"))?;
                    // Notifications have no response identity and do not consume a request.
                    if value.as_object().is_some_and(|object| {
                        !object.contains_key("id")
                            && object.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
                            && object.get("method").and_then(Value::as_str).is_some()
                    }) {
                        continue;
                    }
                    return decode(value, id);
                }
            }
        };
        decode(
            serde_json::from_slice(&bytes)
                .map_err(|_| ClientError::protocol("Runtime response is not JSON"))?,
            id,
        )
    }
}

// Dropping an in-flight event read (including an outer deadline) also poisons the
// owned stream. Reap on the executor rather than blocking an FFI destructor.
struct AbortProcess {
    state: Arc<Mutex<Option<Process>>>,
    closed: CancellationToken,
    armed: bool,
}

impl Drop for AbortProcess {
    fn drop(&mut self) {
        if self.armed {
            self.closed.cancel();
            let state = self.state.clone();
            tokio::spawn(async move {
                if let Some(mut process) = state.lock().await.take() {
                    let _ = process.child.kill().await;
                }
            });
        }
    }
}
