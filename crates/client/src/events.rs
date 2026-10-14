use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    boundary::{ClientError, ErrorKind, EventOptions, Request},
    transport::{Client, duration},
};

pub(crate) struct Events {
    client: Arc<Client>,
    session: String,
    sequence: u64,
    deadline: Instant,
    interval: Duration,
    pending: VecDeque<Value>,
    wait: bool,
}

impl Events {
    pub(crate) fn new(
        client: Arc<Client>,
        session: String,
        options: EventOptions,
    ) -> Result<Self, ClientError> {
        Ok(Self {
            client,
            session,
            sequence: options.after_sequence,
            deadline: Instant::now() + duration(options.timeout_ms)?,
            interval: duration(options.poll_interval_ms)?,
            pending: VecDeque::new(),
            wait: false,
        })
    }

    pub(crate) async fn next(&mut self, cancel: &CancellationToken) -> Result<Value, ClientError> {
        if Instant::now() >= self.deadline {
            return Err(ClientError::new(
                ErrorKind::Timeout,
                "Event polling deadline exceeded",
            ));
        }
        let deadline = self.deadline.into();
        let closed = self.client.closed_token();
        tokio::select! {
            biased;
            _ = closed.cancelled() => Err(ClientError::new(ErrorKind::Closed, "Runtime client is closed")),
            _ = cancel.cancelled() => Err(ClientError::new(ErrorKind::Cancelled, "Event polling cancelled")),
            result = tokio::time::timeout_at(deadline, self.advance(cancel)) => result.unwrap_or_else(|_| Err(ClientError::new(ErrorKind::Timeout, "Event polling deadline exceeded"))),
        }
    }

    async fn advance(&mut self, cancel: &CancellationToken) -> Result<Value, ClientError> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                let sequence = event
                    .get("sequence")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| ClientError::protocol("Runtime event has no sequence"))?;
                if event.get("sessionId").and_then(Value::as_str) != Some(&self.session)
                    || sequence <= self.sequence
                {
                    return Err(ClientError::protocol(
                        "Runtime event ordering or session identity mismatch",
                    ));
                }
                self.sequence = sequence;
                return Ok(event);
            }
            if self.wait {
                tokio::time::sleep(self.interval).await;
            }
            let result = self.client.request(Request { method: "session/transcript".into(), params_json: json!({"sessionId":self.session,"afterSequence":self.sequence,"limit":256}).to_string(), timeout_ms: None }, cancel).await?;
            let batch = result
                .as_array()
                .ok_or_else(|| ClientError::protocol("Runtime transcript is not an array"))?;
            if batch.len() > 256 {
                return Err(ClientError::protocol(
                    "Runtime transcript exceeded the requested page limit",
                ));
            }
            self.wait = batch.len() < 256;
            self.pending.extend(batch.iter().cloned());
        }
    }
}
