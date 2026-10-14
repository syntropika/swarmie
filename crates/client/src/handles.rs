use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    boundary::{
        self, ClientError, ErrorKind, EventOptions, HttpConfig, Native, Request, StdioConfig,
    },
    decode,
    events::Events,
    transport::Client,
};

#[derive(Clone)]
enum Entry {
    Client(Arc<Client>),
    Operation(CancellationToken),
    Stream {
        events: Arc<tokio::sync::Mutex<Events>>,
        cancelled: CancellationToken,
    },
}

struct Host {
    handles: Mutex<HashMap<u64, Entry>>,
    sequence: AtomicU64,
}

impl Host {
    fn insert(&self, entry: Entry) -> Result<u64, ClientError> {
        // Keep handles exact in JavaScript's Number representation; never recycle identities.
        let handle = self.sequence.fetch_add(1, Ordering::Relaxed);
        if handle >= (1 << 53) {
            return Err(ClientError::internal());
        }
        self.handles
            .lock()
            .map_err(|_| ClientError::internal())?
            .insert(handle, entry);
        Ok(handle)
    }

    fn get(&self, handle: u64) -> Result<Entry, ClientError> {
        self.handles
            .lock()
            .map_err(|_| ClientError::internal())?
            .get(&handle)
            .cloned()
            .ok_or_else(|| ClientError::new(ErrorKind::Closed, "Native handle is closed"))
    }

    fn client(&self, handle: u64) -> Result<Arc<Client>, ClientError> {
        match self.get(handle)? {
            Entry::Client(client) => Ok(client),
            _ => Err(ClientError::new(
                ErrorKind::InvalidInput,
                "Expected a client handle",
            )),
        }
    }

    fn operation(&self, handle: u64) -> Result<CancellationToken, ClientError> {
        match self.get(handle)? {
            Entry::Operation(token) => Ok(token),
            _ => Err(ClientError::new(
                ErrorKind::InvalidInput,
                "Expected an operation handle",
            )),
        }
    }
}

impl Native for Host {
    async fn create_http(&self, config: HttpConfig) -> Result<u64, ClientError> {
        self.insert(Entry::Client(Arc::new(Client::http(config)?)))
    }

    async fn create_stdio(&self, config: StdioConfig) -> Result<u64, ClientError> {
        self.insert(Entry::Client(Arc::new(Client::stdio(config)?)))
    }

    async fn prepare(&self) -> Result<u64, ClientError> {
        self.insert(Entry::Operation(CancellationToken::new()))
    }

    async fn cancel(&self, operation: u64) -> Result<(), ClientError> {
        self.operation(operation)?.cancel();
        Ok(())
    }

    async fn release(&self, handle: u64) -> Result<(), ClientError> {
        let entry = self
            .handles
            .lock()
            .map_err(|_| ClientError::internal())?
            .remove(&handle);
        match entry {
            Some(Entry::Client(client)) => client.close().await,
            Some(Entry::Operation(token)) => token.cancel(),
            Some(Entry::Stream { cancelled, .. }) => cancelled.cancel(),
            None => {}
        }
        Ok(())
    }

    async fn invoke(
        &self,
        client: u64,
        operation: u64,
        request: Request,
    ) -> Result<String, ClientError> {
        let client = self.client(client)?;
        let cancel = self.operation(operation)?;
        client
            .request(request, &cancel)
            .await
            .map(|value| value.to_string())
    }

    async fn open_events(
        &self,
        client: u64,
        session_id: String,
        options: EventOptions,
    ) -> Result<u64, ClientError> {
        let events = Events::new(self.client(client)?, session_id, options)?;
        self.insert(Entry::Stream {
            events: Arc::new(tokio::sync::Mutex::new(events)),
            cancelled: CancellationToken::new(),
        })
    }

    async fn next_event(&self, stream: u64, operation: u64) -> Result<String, ClientError> {
        let cancel = self.operation(operation)?;
        let Entry::Stream { events, cancelled } = self.get(stream)? else {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "Expected an event stream handle",
            ));
        };
        tokio::select! {
            biased;
            _ = cancelled.cancelled() => Err(ClientError::new(ErrorKind::Closed, "Event stream is closed")),
            _ = cancel.cancelled() => Err(ClientError::new(ErrorKind::Cancelled, "Event polling cancelled")),
            result = async { events.lock().await.next(&cancel).await } => result.map(|value| value.to_string()),
        }
    }

    async fn wait_turn(
        &self,
        client: u64,
        operation: u64,
        session_id: String,
        options: EventOptions,
    ) -> Result<String, ClientError> {
        let client = self.client(client)?;
        let cancel = self.operation(operation)?;
        let mut cursor = Events::new(client.clone(), session_id, options)?;
        let mut events = Vec::new();
        let mut bytes = 2usize;
        loop {
            let event = cursor.next(&cancel).await?;
            bytes = bytes.saturating_add(event.to_string().len() + 1);
            if bytes > client.result_limit() {
                return Err(ClientError::protocol(
                    "Turn events exceeded the configured byte limit; use the event stream",
                ));
            }
            let ended = event.get("type").and_then(Value::as_str) == Some("turn.ended");
            if event.get("type").and_then(Value::as_str) == Some("session.ended") {
                return Err(ClientError::protocol(
                    "Session ended before the turn completed",
                ));
            }
            events.push(event);
            if ended {
                return serde_json::to_string(&events).map_err(|_| ClientError::internal());
            }
        }
    }

    async fn decode(
        &self,
        response_json: String,
        request_id: String,
    ) -> Result<String, ClientError> {
        let value = serde_json::from_str(&response_json)
            .map_err(|_| ClientError::protocol("Runtime response is not JSON"))?;
        decode(value, &request_id).map(|value| value.to_string())
    }
}

/// Dispatch one WIT operation through the shared executor. The JSON lowering uses
/// snake_case field names and an action tag; replies contain either ok or error.
/// A prepared operation remains owned by the caller until explicitly released.
pub fn execute(command: &str) -> String {
    let result = std::panic::catch_unwind(|| -> Result<Value, ClientError> {
        static EXECUTOR: OnceLock<Result<tokio::runtime::Runtime, std::io::Error>> =
            OnceLock::new();
        static HOST: OnceLock<Host> = OnceLock::new();
        let executor = EXECUTOR
            .get_or_init(|| {
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
            })
            .as_ref()
            .map_err(|_| ClientError::internal())?;
        let host = HOST.get_or_init(|| Host {
            handles: Mutex::new(HashMap::new()),
            sequence: AtomicU64::new(1),
        });
        let command = serde_json::from_str(command)
            .map_err(|_| ClientError::new(ErrorKind::InvalidInput, "Invalid native operation"))?;
        executor.block_on(boundary::dispatch(host, command))
    })
    .unwrap_or_else(|_| Err(ClientError::internal()));
    match result {
        Ok(value) => json!({"ok":value}),
        Err(error) => json!({"error":error}),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(command: Value) -> Value {
        serde_json::from_str(&execute(&command.to_string())).unwrap()
    }

    #[test]
    fn cancellation_before_dispatch_and_released_handles_are_safe() {
        let client = call(json!({"action":"create_http","config":{"url":"http://127.0.0.1:1","token":null,"timeout_ms":100,"max_response_bytes":1024}}))["ok"].as_u64().unwrap();
        let operation = call(json!({"action":"prepare"}))["ok"].as_u64().unwrap();
        assert!(
            call(json!({"action":"cancel","operation":operation}))
                .get("ok")
                .is_some()
        );
        let result = call(
            json!({"action":"invoke","client":client,"operation":operation,"request":{"method":"noop","params_json":"{}","timeout_ms":null}}),
        );
        assert_eq!(result["error"]["kind"], "cancelled");
        for handle in [operation, client] {
            call(json!({"action":"release","handle":handle}));
        }
        assert_eq!(
            call(json!({"action":"cancel","operation":operation}))["error"]["kind"],
            "closed"
        );
        assert!(
            call(json!({"action":"release","handle":client}))
                .get("ok")
                .is_some()
        );
        assert!(
            call(json!({"action":"prepare","unexpected":true}))
                .get("error")
                .is_some()
        );
    }
}
