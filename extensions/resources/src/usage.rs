use crate::settings::cwd;
use crate::*;

impl ResourcesExtension {
    pub(crate) fn observe(&self, state: &mut State, event: &RuntimeEvent) -> bool {
        match &event.kind {
            EventKind::SessionStarted(snapshot) => {
                let record = state
                    .sessions
                    .entry(snapshot.id.clone())
                    .or_insert_with(|| SessionRecord {
                        id: snapshot.id.clone(),
                        harness: snapshot.options.harness.clone(),
                        cwd: snapshot.options.cwd.clone(),
                        model: None,
                        native_id: snapshot.native.id.clone(),
                        forked: snapshot.metadata.fork_origin.is_some(),
                        inherited_sequence: None,
                    });
                record.model = snapshot.options.model.clone();
                if record.forked
                    && record.native_id == snapshot.native.id
                    && record.inherited_sequence.is_none()
                {
                    record.inherited_sequence = Some(event.sequence.saturating_sub(1));
                }
                let inherited = record.inherited_sequence;
                if let Some(sequence) = inherited {
                    state.history.retain(|entry| {
                        entry.session_id != event.session_id || entry.sequence > sequence
                    });
                    state.usage.retain(|entry| {
                        entry.session_id != event.session_id || entry.sequence > sequence
                    });
                }
                true
            }
            EventKind::ConfigurationUpdated(value) => {
                if let Some(model) = value["model"].as_str()
                    && let Some(session) = state.sessions.get_mut(&event.session_id)
                {
                    session.model = Some(model.into());
                    return true;
                }
                false
            }
            EventKind::ModelRerouted(value) => {
                if let Some(session) = state.sessions.get_mut(&event.session_id) {
                    session.model = Some(value.to_model.clone());
                    return true;
                }
                false
            }
            EventKind::UserMessage(prompt) => {
                if state
                    .sessions
                    .get(&event.session_id)
                    .and_then(|record| record.inherited_sequence)
                    .is_some_and(|sequence| event.sequence <= sequence)
                {
                    return false;
                }
                if state
                    .history
                    .iter()
                    .any(|v| v.session_id == event.session_id && v.sequence == event.sequence)
                {
                    return false;
                }
                let Some(session) = state.sessions.get(&event.session_id) else {
                    return false;
                };
                let prompt = prompt
                    .content
                    .iter()
                    .filter_map(|content| {
                        if let PromptContent::Text(value) = content {
                            Some(value.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let (text, truncated) = bounded(&prompt, MAX_TEXT);
                state.history.push(PromptHistory {
                    session_id: event.session_id.clone(),
                    sequence: event.sequence,
                    timestamp_ms: event.timestamp_ms,
                    cwd: session.cwd.clone(),
                    text,
                    truncated,
                });
                state
                    .history
                    .sort_by_key(|entry| (entry.timestamp_ms, entry.sequence));
                if state.history.len() > 1000 {
                    state.history.remove(0);
                    state.history_truncated = true;
                }
                true
            }
            EventKind::UsageUpdated(usage) => {
                if state
                    .sessions
                    .get(&event.session_id)
                    .and_then(|record| record.inherited_sequence)
                    .is_some_and(|sequence| event.sequence <= sequence)
                {
                    return false;
                }
                if state
                    .usage
                    .iter()
                    .any(|v| v.session_id == event.session_id && v.sequence == event.sequence)
                {
                    return false;
                }
                state.usage.push(UsageRecord {
                    session_id: event.session_id.clone(),
                    sequence: event.sequence,
                    timestamp_ms: event.timestamp_ms,
                    model: state
                        .sessions
                        .get(&event.session_id)
                        .and_then(|s| s.model.clone()),
                    usage: usage.clone(),
                });
                state
                    .usage
                    .sort_by_key(|entry| (entry.timestamp_ms, entry.sequence));
                if state.usage.len() > 10000 {
                    state.usage.remove(0);
                    state.usage_truncated = true;
                }
                true
            }
            _ => false,
        }
    }
    pub(crate) async fn backfill(&self) -> Result<()> {
        let runtime = self.runtime()?;
        let sessions = runtime.sessions().await?;
        let mut remaining = 100_000usize;
        let mut capped = false;
        for snapshot in sessions {
            let record = SessionRecord {
                id: snapshot.id.clone(),
                harness: snapshot.options.harness.clone(),
                cwd: snapshot.options.cwd.clone(),
                model: snapshot.options.model.clone(),
                native_id: snapshot.native.id.clone(),
                forked: snapshot.metadata.fork_origin.is_some(),
                inherited_sequence: None,
            };
            self.0
                .state
                .lock()
                .await
                .sessions
                .entry(snapshot.id.clone())
                .or_insert(record);
            let mut after = self
                .0
                .state
                .lock()
                .await
                .scanned
                .get(&snapshot.id)
                .copied()
                .unwrap_or(0);
            loop {
                if remaining == 0 {
                    capped = true;
                    break;
                }
                let events = match runtime
                    .transcript(&snapshot.id, after, remaining.min(4096))
                    .await
                {
                    Ok(events) => events,
                    Err(Error::Unsupported(_)) => {
                        if let Some(agent) = runtime.agent(&snapshot.id).await {
                            agent.replay(after).await?
                        } else {
                            break;
                        }
                    }
                    Err(error) => return Err(error),
                };
                if events.is_empty() {
                    break;
                }
                remaining = remaining.saturating_sub(events.len());
                after = events.last().unwrap().sequence;
                let mut state = self.0.state.lock().await;
                for event in &events {
                    self.observe(&mut state, event);
                }
                state.scanned.insert(snapshot.id.clone(), after);
                self.persist(&state).await?;
                if runtime.transcript_durability().is_none() {
                    break;
                }
            }
            if capped {
                break;
            }
        }
        if capped {
            let mut state = self.0.state.lock().await;
            state.history_truncated = true;
            state.usage_truncated = true;
            self.persist(&state).await?;
        }
        Ok(())
    }
    pub(crate) async fn dashboard(&self, p: &Value) -> Result<Value> {
        self.backfill().await?;
        let path = if p.get("cwd").is_some() {
            Some(cwd(p)?)
        } else {
            None
        };
        let since = p["sinceMs"].as_u64().unwrap_or(0);
        let until = p["untilMs"].as_u64().unwrap_or(u64::MAX);
        if since > until {
            return Err(Error::Invalid("sinceMs exceeds untilMs".into()));
        }
        let state = self.0.state.lock().await;
        let mut totals = Totals::default();
        let mut by_day = BTreeMap::<String, Totals>::new();
        let mut by_harness = BTreeMap::<String, Totals>::new();
        let mut by_model = BTreeMap::<String, Totals>::new();
        let mut session_totals = BTreeMap::<String, Totals>::new();
        let mut previous = BTreeMap::<String, Usage>::new();
        let mut records = state.usage.iter().collect::<Vec<_>>();
        records.sort_by_key(|entry| (&entry.session_id, entry.sequence));
        for record in records {
            let Some(session) = state.sessions.get(&record.session_id) else {
                continue;
            };
            if session
                .inherited_sequence
                .is_some_and(|sequence| record.sequence <= sequence)
            {
                continue;
            }
            let usage = if record.usage.scope == "session" {
                let delta = increment(&record.usage, previous.get(&record.session_id));
                previous.insert(
                    record.session_id.clone(),
                    baseline(&record.usage, previous.get(&record.session_id)),
                );
                delta
            } else {
                record.usage.clone()
            };
            if record.timestamp_ms < since
                || record.timestamp_ms > until
                || path.as_ref().is_some_and(|path| &session.cwd != path)
                || p["harness"].as_str().is_some_and(|h| h != session.harness)
            {
                continue;
            }
            totals.add(&usage);
            by_day
                .entry(day(record.timestamp_ms))
                .or_default()
                .add(&usage);
            by_harness
                .entry(session.harness.clone())
                .or_default()
                .add(&usage);
            by_model
                .entry(record.model.clone().unwrap_or_else(|| "unknown".into()))
                .or_default()
                .add(&usage);
            session_totals
                .entry(record.session_id.clone())
                .or_default()
                .add(&usage);
        }
        Ok(
            json!({"totals":totals,"byDay":by_day,"byHarness":by_harness,"byModel":by_model,"sessions":session_totals,"unknownCosts":totals.unknown_costs,"truncated":state.usage_truncated,"scope":"Observed native usage events; missing costs and token fields remain unknown"}),
        )
    }
}
#[derive(Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Totals {
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    cache_write_tokens: u64,
    total_tokens: u64,
    known_cost_usd: f64,
    unknown_costs: u64,
    unknown_token_fields: u64,
    observations: u64,
}
impl Totals {
    fn add(&mut self, value: &Usage) {
        self.observations += 1;
        for (field, value) in [
            (&mut self.input_tokens, value.input_tokens),
            (&mut self.output_tokens, value.output_tokens),
            (&mut self.cached_input_tokens, value.cached_input_tokens),
            (&mut self.cache_write_tokens, value.cache_write_tokens),
            (&mut self.total_tokens, value.total_tokens),
        ] {
            if let Some(value) = value {
                *field = field.saturating_add(value);
            } else {
                self.unknown_token_fields += 1;
            }
        }
        if let Some(cost) = value
            .cost_usd
            .filter(|cost| cost.is_finite() && *cost >= 0.0)
        {
            self.known_cost_usd += cost;
        } else {
            self.unknown_costs += 1;
        }
    }
}
fn increment(value: &Usage, previous: Option<&Usage>) -> Usage {
    fn delta(value: Option<u64>, previous: Option<u64>) -> Option<u64> {
        value.map(|value| {
            previous
                .filter(|old| *old <= value)
                .map_or(value, |old| value - old)
        })
    }
    Usage {
        scope: "increment".into(),
        input_tokens: delta(value.input_tokens, previous.and_then(|p| p.input_tokens)),
        output_tokens: delta(value.output_tokens, previous.and_then(|p| p.output_tokens)),
        cached_input_tokens: delta(
            value.cached_input_tokens,
            previous.and_then(|p| p.cached_input_tokens),
        ),
        cache_write_tokens: delta(
            value.cache_write_tokens,
            previous.and_then(|p| p.cache_write_tokens),
        ),
        total_tokens: delta(value.total_tokens, previous.and_then(|p| p.total_tokens)),
        cost_usd: value.cost_usd.map(|value| {
            previous
                .and_then(|p| p.cost_usd)
                .filter(|old| *old <= value)
                .map_or(value, |old| value - old)
        }),
    }
}
fn baseline(value: &Usage, previous: Option<&Usage>) -> Usage {
    Usage {
        scope: value.scope.clone(),
        input_tokens: value
            .input_tokens
            .or_else(|| previous.and_then(|p| p.input_tokens)),
        output_tokens: value
            .output_tokens
            .or_else(|| previous.and_then(|p| p.output_tokens)),
        cached_input_tokens: value
            .cached_input_tokens
            .or_else(|| previous.and_then(|p| p.cached_input_tokens)),
        cache_write_tokens: value
            .cache_write_tokens
            .or_else(|| previous.and_then(|p| p.cache_write_tokens)),
        total_tokens: value
            .total_tokens
            .or_else(|| previous.and_then(|p| p.total_tokens)),
        cost_usd: value.cost_usd.or_else(|| previous.and_then(|p| p.cost_usd)),
    }
}
// UTC calendar dates without a platform timezone dependency.
fn day(milliseconds: u64) -> String {
    let days = (milliseconds / 86_400_000) as i64 + 719468;
    let era = days / 146097;
    let doe = days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}
