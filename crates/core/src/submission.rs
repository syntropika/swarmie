use crate::*;
use serde::{Deserialize, Serialize};

/// Knowledge of native delivery, independent of execution outcome.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryKnowledge {
    NotSent,
    InFlight,
    Accepted,
    Rejected,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubmissionReceipt {
    pub id: String,
    pub session_id: String,
    pub idempotency_key: String,
    pub turn_id: String,
    pub delivery: DeliveryKnowledge,
    pub recovery_required: bool,
    pub outcome: Option<TurnOutcome>,
    pub error: Option<String>,
}

/// Correlated facts read from one native submission. Absence is never a rejection.
#[derive(Clone, Debug)]
pub struct NativeSubmissionEvidence {
    pub submission_id: String,
    pub native_turn_id: String,
    pub outcome: Option<TurnOutcome>,
    pub native_reason: Option<String>,
    pub raw: serde_json::Value,
    pub messages: Vec<serde_json::Value>,
}

/// Authoritative delivery ledger. It is retained independently of event history.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmissionRecord {
    pub receipt: SubmissionReceipt,
    pub authored: Prompt,
    pub projected: Prompt,
    pub terminal_native_reason: Option<String>,
    pub terminal_raw: Option<serde_json::Value>,
    pub terminal_sequence: Option<u64>,
    pub recovered_messages: Vec<serde_json::Value>,
    pub reconciliation_raw: Option<serde_json::Value>,
}
impl SubmissionRecord {
    pub(crate) fn resolve_not_sent(&mut self) -> bool {
        if self.receipt.delivery != DeliveryKnowledge::NotSent {
            return false;
        }
        self.receipt.delivery = DeliveryKnowledge::Rejected;
        self.receipt.recovery_required = false;
        self.receipt.error = Some(
            "The runtime did not dispatch this submission. Use a new key for an intentional retry."
                .into(),
        );
        true
    }
    pub(crate) fn reconcile(&mut self, evidence: NativeSubmissionEvidence) -> Result<()> {
        if evidence.submission_id != self.receipt.id || evidence.native_turn_id.is_empty() {
            return Err(Error::Provider(
                "native submission evidence has a different identity".into(),
            ));
        }
        // Terminal evidence cannot be weakened by later partial native views.
        if self.receipt.outcome.is_some() || self.receipt.delivery == DeliveryKnowledge::Rejected {
            return Ok(());
        }
        let mut message_ids = std::collections::BTreeSet::new();
        for message in &evidence.messages {
            let id = message["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| Error::Provider("recovered message omitted its identity".into()))?;
            if !message_ids.insert(id) {
                return Err(Error::Provider(
                    "recovered message identity is ambiguous".into(),
                ));
            }
        }
        self.receipt.delivery = DeliveryKnowledge::Accepted;
        self.reconciliation_raw = Some(evidence.raw.clone());
        if let Some(outcome) = evidence.outcome {
            self.receipt.outcome = Some(outcome);
            self.receipt.recovery_required = false;
            self.receipt.error = None;
            self.terminal_native_reason = evidence.native_reason;
            self.terminal_raw = Some(evidence.raw);
            self.recovered_messages = evidence.messages;
        } else {
            self.receipt.recovery_required = true;
            self.receipt.error = Some("Native receipt is confirmed; completion is not yet confirmed. Check again before sending more work.".into());
        }
        Ok(())
    }

    pub(crate) fn duplicate(&self, prompt: &Prompt) -> Result<SubmissionReceipt> {
        if serde_json::to_value(&self.authored)? != serde_json::to_value(prompt)? {
            return Err(Error::IdempotencyConflict);
        }
        Ok(self.receipt.clone())
    }
    pub(crate) fn blocks_dispatch(&self) -> bool {
        self.receipt.recovery_required
            || !matches!(
                self.receipt.delivery,
                DeliveryKnowledge::Accepted | DeliveryKnowledge::Rejected
            )
    }
    pub(crate) fn recover(&mut self) {
        if self.receipt.delivery == DeliveryKnowledge::InFlight {
            self.receipt.delivery = DeliveryKnowledge::Unknown;
        }
        if self.receipt.delivery != DeliveryKnowledge::Rejected && self.receipt.outcome.is_none() {
            self.receipt.recovery_required = true;
        }
    }
}
pub(crate) fn validate_client_submission_key(key: &str) -> Result<()> {
    validate_submission_key(key)?;
    if key.starts_with("queue:") {
        return Err(Error::Invalid(
            "queue: is a reserved submission key prefix".into(),
        ));
    }
    Ok(())
}
pub(crate) fn validate_submission_key(key: &str) -> Result<()> {
    if key.is_empty() || key.len() > 128 || key.chars().any(char::is_control) {
        return Err(Error::Invalid(
            "idempotencyKey must contain 1 to 128 bytes without control characters".into(),
        ));
    }
    Ok(())
}

impl crate::session::Engine {
    pub(crate) fn retained_submission(
        &self,
        key: &str,
        prompt: &Prompt,
    ) -> Result<Option<SubmissionReceipt>> {
        self.submissions
            .get(key)
            .map(|record| record.duplicate(prompt))
            .transpose()
    }
    pub(crate) async fn retain_submission(&mut self, record: SubmissionRecord) -> Result<()> {
        // Keep in-process evidence even if the durable write fails after native acceptance.
        self.submissions
            .insert(record.receipt.idempotency_key.clone(), record.clone());
        self.store
            .commit(SessionWrite {
                snapshot: Some(&self.snapshot),
                submission: Some(&record),
                ..Default::default()
            })
            .await
    }
    pub(crate) fn dispatch_blocked(&self) -> bool {
        self.snapshot.execution_policy.dispatch_blocked
            || self
                .submissions
                .values()
                .any(SubmissionRecord::blocks_dispatch)
    }
    pub(crate) async fn submit_prompt(
        &mut self,
        key: String,
        authored: Prompt,
    ) -> Result<SubmissionReceipt> {
        validate_submission_key(&key)?;
        if let Some(receipt) = self.retained_submission(&key, &authored)? {
            return Ok(receipt);
        }
        if self.mcp_restart.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(Error::Rejected("MCP configuration changed; close and reopen this native session before sending another message".into()));
        }
        if self.dispatch_blocked() || self.snapshot.turn.is_some() || !self.permissions.is_empty() {
            return Err(Error::Busy);
        }
        if authored.content.is_empty() {
            return Err(Error::Invalid("prompt content must not be empty".into()));
        }
        let context = self
            .snapshot
            .working_context
            .as_ref()
            .ok_or_else(|| Error::Storage("missing working context".into()))?;
        verify_context(context).await?;
        self.store.save(&self.snapshot).await?;
        let mut projected = authored.clone();
        for extension in &self.extensions {
            extension
                .before_prompt(&self.snapshot, &mut projected)
                .await?;
        }
        let mut record = SubmissionRecord {
            receipt: SubmissionReceipt {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: self.snapshot.id.clone(),
                idempotency_key: key,
                turn_id: uuid::Uuid::new_v4().to_string(),
                delivery: DeliveryKnowledge::NotSent,
                recovery_required: true,
                outcome: None,
                error: None,
            },
            authored,
            projected,
            terminal_native_reason: None,
            terminal_raw: None,
            terminal_sequence: None,
            recovered_messages: vec![],
            reconciliation_raw: None,
        };
        self.retain_submission(record.clone()).await?;
        record.receipt.delivery = DeliveryKnowledge::InFlight;
        if let Err(error) = self.retain_submission(record.clone()).await {
            // No native call has occurred. The durable marker may nevertheless be uncertain.
            record.receipt.delivery = DeliveryKnowledge::NotSent;
            record.receipt.error = Some(error.to_string());
            self.submissions
                .insert(record.receipt.idempotency_key.clone(), record);
            return Err(error);
        }
        if let Some(tools) = &self.host_tools {
            tools.begin_turn(&record.receipt.turn_id);
        }
        let backend = self.backend.clone();
        let submitted = record.projected.clone();
        let submission_id = record.receipt.id.clone();
        let result = self
            .controlled(
                Box::pin(async move { backend.submit(&submission_id, &submitted).await }),
                true,
            )
            .await;
        match result {
            Ok(()) => {
                record.receipt.delivery = DeliveryKnowledge::Accepted;
                record.receipt.recovery_required = false;
            }
            Err(Error::Rejected(error)) => {
                record.receipt.delivery = DeliveryKnowledge::Rejected;
                record.receipt.recovery_required = false;
                record.receipt.error = Some(error);
            }
            Err(error) => {
                // Native errors do not certify absence of effects in the common interface.
                record.receipt.delivery = DeliveryKnowledge::Unknown;
                record.receipt.error = Some(error.to_string());
            }
        }
        if self.closed.load(std::sync::atomic::Ordering::SeqCst)
            && record.receipt.delivery == DeliveryKnowledge::Accepted
        {
            record.receipt.error = Some("session closed after native acceptance".into());
            record.receipt.recovery_required = true;
        }
        let persistence = self.retain_submission(record.clone()).await;
        if !self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            if record.receipt.delivery == DeliveryKnowledge::Accepted {
                self.snapshot.turn = Some(record.receipt.turn_id.clone());
                if let Err(error) = self.refresh_execution_policy().await {
                    self.snapshot.execution_policy.dispatch_blocked = true;
                    self.snapshot.execution_policy.status = PolicyStatus::Unknown;
                    self.snapshot.execution_policy.reason = Some(error.to_string());
                }
                let state = if persistence.is_ok() {
                    SessionState::Running
                } else {
                    self.snapshot.queue.paused = true;
                    SessionState::NeedsInput
                };
                self.state(state).await;
                self.emit(EventKind::TurnStarted, None).await;
                self.emit(EventKind::UserMessage(record.projected), None)
                    .await;
            } else if record.receipt.delivery == DeliveryKnowledge::Unknown {
                self.snapshot.queue.paused = true;
                self.state(SessionState::NeedsInput).await;
            }
        }
        if record.receipt.delivery != DeliveryKnowledge::Accepted
            && let Some(tools) = &self.host_tools
        {
            tools.cancel_turn();
        }
        persistence?;
        if let Some(error) = &self.storage_error {
            return Err(Error::Storage(error.clone()));
        }
        Ok(record.receipt)
    }
    pub(crate) async fn reconcile_retained_submission(
        &mut self,
        key: &str,
    ) -> Result<Option<SubmissionReceipt>> {
        let Some(mut record) = self.submissions.get(key).cloned() else {
            return Ok(None);
        };
        if !record.receipt.recovery_required && record.reconciliation_raw.is_none() {
            return Ok(Some(record.receipt));
        }
        if self.snapshot.turn.is_some() || !self.permissions.is_empty() {
            return Err(Error::Busy);
        }
        if record.resolve_not_sent() {
            self.retain_submission(record.clone()).await?;
            self.state(if self.dispatch_blocked() {
                SessionState::NeedsInput
            } else {
                SessionState::Idle
            })
            .await;
            self.persist().await;
            return Ok(Some(record.receipt));
        }
        if record.receipt.recovery_required {
            let evidence = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                self.backend.reconcile_submission(&record.receipt.id),
            )
            .await
            .map_err(|_| Error::Timeout("native submission reconciliation".into()))??;
            let Some(evidence) = evidence else {
                return Ok(Some(record.receipt));
            };
            record.reconcile(evidence)?;
        }
        let mut history = SubmissionHistory::default();
        if self.store.transcript_durability().is_some() {
            let mut after = 0;
            loop {
                let events = self.store.events(&self.snapshot.id, after, 4096).await?;
                if events.is_empty() {
                    break;
                }
                for event in events {
                    after = event.sequence;
                    history.observe(&event);
                }
            }
        } else {
            for event in self.history.read().await.iter() {
                history.observe(event);
            }
        }
        let missing = history.missing(&record);
        if missing
            .iter()
            .any(|(kind, _)| matches!(kind, EventKind::TurnEnded { .. }))
        {
            record.terminal_sequence = Some(self.snapshot.sequence + missing.len() as u64);
        }
        self.retain_submission(record.clone()).await?;
        self.snapshot.turn = Some(record.receipt.turn_id.clone());
        for (kind, raw) in missing {
            if matches!(kind, EventKind::TurnEnded { .. }) {
                self.snapshot.turns_completed += 1;
            }
            self.emit(kind, raw).await;
        }
        self.snapshot.turn = None;
        if self.storage_error.is_none() {
            self.state(if self.dispatch_blocked() || !self.permissions.is_empty() {
                SessionState::NeedsInput
            } else {
                SessionState::Idle
            })
            .await;
            self.persist().await;
        }
        if let Some(error) = &self.storage_error {
            return Err(Error::Storage(error.clone()));
        }
        Ok(Some(record.receipt))
    }
    pub(crate) async fn settle_submission(
        &mut self,
        outcome: TurnOutcome,
        native_reason: Option<String>,
        raw: Option<serde_json::Value>,
    ) {
        let Some(turn) = self.snapshot.turn.as_ref() else {
            return;
        };
        let Some(mut record) = self
            .submissions
            .values()
            .find(|r| &r.receipt.turn_id == turn)
            .cloned()
        else {
            return;
        };
        record.receipt.delivery = DeliveryKnowledge::Accepted;
        record.receipt.recovery_required = false;
        record.receipt.outcome = Some(outcome);
        record.terminal_native_reason = native_reason;
        record.terminal_raw = raw;
        record.terminal_sequence = Some(self.snapshot.sequence + self.permissions.len() as u64 + 1);
        if let Err(error) = self.retain_submission(record).await {
            self.snapshot.queue.paused = true;
            self.storage_error = Some(error.to_string());
            self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
            self.snapshot.state = SessionState::Failed;
            *self.shared.write().await = self.snapshot.clone();
        }
    }
}

/// Repair the journal from durable delivery evidence without retrying provider work.
#[derive(Default)]
pub(crate) struct SubmissionHistory {
    started: std::collections::BTreeSet<String>,
    authored: std::collections::BTreeSet<String>,
    ended: std::collections::BTreeSet<String>,
    messages: std::collections::BTreeSet<(String, String)>,
}
impl SubmissionHistory {
    pub(crate) fn observe(&mut self, event: &RuntimeEvent) {
        let Some(turn) = &event.turn_id else { return };
        match &event.kind {
            EventKind::TurnStarted => {
                self.started.insert(turn.clone());
            }
            EventKind::UserMessage(_) => {
                self.authored.insert(turn.clone());
            }
            EventKind::AssistantMessage(message) => {
                if let Some(id) = message["id"].as_str() {
                    self.messages.insert((turn.clone(), id.into()));
                }
            }
            EventKind::TurnEnded { .. } => {
                self.ended.insert(turn.clone());
            }
            _ => {}
        }
    }
    pub(crate) fn open_turn(&self, record: &SubmissionRecord) -> bool {
        record.receipt.delivery == DeliveryKnowledge::Accepted
            && record.receipt.outcome.is_none()
            && !self.ended.contains(&record.receipt.turn_id)
    }
    pub(crate) fn missing(
        &self,
        record: &SubmissionRecord,
    ) -> Vec<(EventKind, Option<serde_json::Value>)> {
        if record.receipt.delivery != DeliveryKnowledge::Accepted {
            return vec![];
        }
        let turn = &record.receipt.turn_id;
        let mut missing = vec![];
        if !self.started.contains(turn) {
            missing.push((EventKind::TurnStarted, None));
        }
        if !self.authored.contains(turn) {
            missing.push((EventKind::UserMessage(record.projected.clone()), None));
        }
        for message in &record.recovered_messages {
            if let Some(id) = message["id"].as_str()
                && !self.messages.contains(&(turn.clone(), id.into()))
            {
                missing.push((
                    EventKind::AssistantMessage(message.clone()),
                    Some(message.clone()),
                ));
            }
        }
        if !self.ended.contains(turn)
            && let Some(outcome) = &record.receipt.outcome
        {
            missing.push((
                EventKind::TurnEnded {
                    outcome: *outcome,
                    native_reason: record.terminal_native_reason.clone(),
                },
                record.terminal_raw.clone(),
            ));
        }
        missing
    }
}
