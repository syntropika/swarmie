use crate::*;
use futures_util::StreamExt;
use std::time::Duration;

impl ResourcesExtension {
    pub(crate) fn start_scheduler(&self) {
        if tokio::runtime::Handle::try_current().is_err()
            || self.0.started.swap(true, Ordering::SeqCst)
        {
            return;
        }
        let weak = Arc::downgrade(&self.0);
        let flusher = tokio::spawn(async move {
            let period = Duration::from_millis(250);
            let mut clock = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
            clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                clock.tick().await;
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                let extension = ResourcesExtension(inner);
                if extension.runtime().is_err() {
                    break;
                }
                if extension.0.text_dirty.load(Ordering::SeqCst) {
                    let state = extension.0.state.lock().await;
                    if let Err(error) = extension.persist(&state) {
                        extension.record_error(error);
                    }
                }
            }
        });
        *self.0.flusher.lock().unwrap() = Some(flusher);
        let weak = Arc::downgrade(&self.0);
        let task = tokio::spawn(async move {
            let mut clock = tokio::time::interval(Duration::from_millis(250));
            clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                clock.tick().await;
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                let extension = ResourcesExtension(inner);
                if extension.0.stopped.load(Ordering::SeqCst) {
                    break;
                }
                if extension.runtime().is_err() {
                    let agents = std::mem::take(&mut *extension.0.agents.lock().await);
                    for agent in agents.values() {
                        let _ = agent.kill().await;
                    }
                    break;
                }
                let due = {
                    let state = extension.0.state.lock().await;
                    state
                        .schedules
                        .values()
                        .filter(|s| s.enabled && s.status != "running" && s.next_run_at_ms <= now())
                        .map(|s| s.id.clone())
                        .collect::<Vec<_>>()
                };
                for id in due {
                    if let Err(error) = extension.run_schedule(&id).await {
                        extension.record_error(error);
                    }
                }
            }
        });
        *self.0.scheduler.lock().unwrap() = Some(task);
    }
    pub(crate) async fn workflow_command(&self, command: &str, p: &Value) -> Result<Value> {
        self.start_scheduler();
        match command {
            "account.quotas" => {
                let options = self.account_options(p).await?;
                let quotas = self
                    .runtime()?
                    .account_quotas(AccountQuery {
                        harness: options.harness,
                        cwd: options.cwd,
                        config: options.config,
                        execution: AccountExecutionOptions {
                            env: options.execution.env,
                            config_home: options.execution.config_home,
                        },
                    })
                    .await?;
                Ok(quotas)
            }
            "session.resume" => {
                let agent = self.peer(text(p, "sessionId")?).await?;
                Ok(serde_json::to_value(agent.snapshot().await)?)
            }
            "text.generate" => {
                let options = self.launch_options(p).await?;
                let seconds = p["timeoutSeconds"].as_u64().unwrap_or(120);
                if !(1..=3600).contains(&seconds) {
                    return Err(Error::Invalid("timeoutSeconds must be 1-3600".into()));
                }
                let text = self
                    .runtime()?
                    .generate_with_timeout(
                        options,
                        text(p, "text")?.into(),
                        Duration::from_secs(seconds),
                    )
                    .await?;
                Ok(json!({"text":text}))
            }
            "session.create" | "peers.spawn" => {
                let options = self.launch_options(p).await?;
                let agent = self.runtime()?.spawn(options).await?;
                let snapshot = agent.snapshot().await;
                self.0
                    .agents
                    .lock()
                    .await
                    .insert(snapshot.id.clone(), agent.clone());
                if command == "session.create" {
                    return Ok(serde_json::to_value(snapshot)?);
                }
                let delivery = if p.get("text").is_some() {
                    Some(self.deliver(&agent, text(p, "text")?).await?)
                } else {
                    None
                };
                Ok(json!({"session":snapshot,"delivery":delivery}))
            }
            "peers.list" => Ok(json!({"sessions":self.runtime()?.sessions().await?})),
            "peers.read" => {
                let id = text(p, "sessionId")?;
                let after = p["afterSequence"].as_u64().unwrap_or(0);
                let limit = p["limit"].as_u64().unwrap_or(100);
                if !(1..=4096).contains(&limit) {
                    return Err(Error::Invalid("peer read limit must be 1-4096".into()));
                }
                let runtime = self.runtime()?;
                match runtime.transcript(id, after, limit as usize).await {
                    Ok(events) => Ok(json!({"events":events,"durable":true})),
                    Err(Error::Unsupported(_)) => {
                        let agent = runtime.agent(id).await.ok_or(Error::Closed)?;
                        let mut events = agent.replay(after).await?;
                        events.truncate(limit as usize);
                        Ok(json!({"events":events,"durable":false}))
                    }
                    Err(error) => Err(error),
                }
            }
            "peers.send" => {
                let id = text(p, "sessionId")?;
                let prompt = text(p, "text")?;
                let agent = self.peer(id).await?;
                self.deliver(&agent, prompt).await
            }
            "schedules.list" => Ok(
                json!({"schedules":self.0.state.lock().await.schedules.values().collect::<Vec<_>>(),"lastError":self.0.last_error.lock().unwrap().clone()}),
            ),
            "schedules.put" => {
                let id = p["id"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                identifier(&id)?;
                let session_id = text(p, "sessionId")?.to_owned();
                self.runtime()?.snapshot(&session_id).await?;
                let interval = p
                    .get("intervalSeconds")
                    .filter(|v| !v.is_null())
                    .map(|v| {
                        v.as_u64()
                            .filter(|s| (1..=31_536_000).contains(s))
                            .ok_or_else(|| {
                                Error::Invalid("intervalSeconds must be 1-31536000".into())
                            })
                    })
                    .transpose()?;
                let next = p["nextRunAtMs"]
                    .as_u64()
                    .ok_or_else(|| Error::Invalid("nextRunAtMs is required".into()))?;
                let schedule = Schedule {
                    id: id.clone(),
                    session_id,
                    text: text(p, "text")?.into(),
                    next_run_at_ms: next,
                    interval_seconds: interval,
                    enabled: p["enabled"].as_bool().unwrap_or(true),
                    status: "scheduled".into(),
                    last_run_at_ms: None,
                    last_result: None,
                    error: None,
                };
                let mut state = self.0.state.lock().await;
                if state
                    .schedules
                    .get(&id)
                    .is_some_and(|s| s.status == "running")
                {
                    return Err(Error::Busy);
                }
                capacity(state.schedules.contains_key(&id), state.schedules.len())?;
                state.schedules.insert(id, schedule.clone());
                self.persist(&state)?;
                Ok(serde_json::to_value(schedule)?)
            }
            "schedules.remove" => {
                let id = text(p, "id")?;
                let mut state = self.0.state.lock().await;
                if state
                    .schedules
                    .get(id)
                    .is_some_and(|s| s.status == "running")
                {
                    return Err(Error::Busy);
                }
                state.schedules.remove(id);
                self.persist(&state)?;
                Ok(json!({"removed":true}))
            }
            "schedules.run" => self.run_schedule(text(p, "id")?).await,
            "fanout.start" => self.start_fanout(p).await,
            "fanout.status" => {
                let state = self.0.state.lock().await;
                Ok(serde_json::to_value(
                    state
                        .jobs
                        .get(text(p, "id")?)
                        .ok_or_else(|| Error::Invalid("unknown comparison".into()))?,
                )?)
            }
            "fanout.list" => {
                let limit = p["limit"].as_u64().unwrap_or(128);
                if !(1..=128).contains(&limit) {
                    return Err(Error::Invalid("comparison list limit must be 1-128".into()));
                }
                let state = self.0.state.lock().await;
                let mut jobs = state
                    .jobs
                    .values()
                    .filter(|job| {
                        p["status"]
                            .as_str()
                            .is_none_or(|status| job.status == status)
                    })
                    .collect::<Vec<_>>();
                jobs.sort_by(|a, b| {
                    b.created_at_ms
                        .cmp(&a.created_at_ms)
                        .then_with(|| b.id.cmp(&a.id))
                });
                Ok(
                    json!({"jobs":jobs.into_iter().take(limit as usize).map(|job| json!({"id":job.id,"status":job.status,"createdAtMs":job.created_at_ms,"targets":job.targets.iter().map(|target|json!({"harness":target.harness,"model":target.model,"sessionId":target.session_id,"status":target.status})).collect::<Vec<_>>()})).collect::<Vec<_>>()}),
                )
            }
            "fanout.remove" => {
                let id = text(p, "id")?;
                if self
                    .0
                    .jobs
                    .lock()
                    .unwrap()
                    .get(id)
                    .is_some_and(|task| !task.is_finished())
                {
                    return Err(Error::Busy);
                }
                let sessions = {
                    let state = self.0.state.lock().await;
                    let Some(job) = state.jobs.get(id) else {
                        return Ok(json!({"removed":true}));
                    };
                    if matches!(job.status.as_str(), "running" | "cancelling") {
                        return Err(Error::Busy);
                    }
                    job.targets
                        .iter()
                        .filter_map(|target| target.session_id.clone())
                        .collect::<Vec<_>>()
                };
                let runtime = self.runtime()?;
                for id in sessions {
                    if let Some(agent) = runtime.agent(&id).await
                        && matches!(
                            agent.snapshot().await.state,
                            SessionState::Running
                                | SessionState::NeedsInput
                                | SessionState::Cancelling
                        )
                    {
                        return Err(Error::Busy);
                    }
                }
                let mut state = self.0.state.lock().await;
                state.jobs.remove(id);
                self.persist(&state)?;
                self.0.jobs.lock().unwrap().remove(id);
                Ok(json!({"removed":true}))
            }
            "fanout.cancel" => self.cancel_fanout(text(p, "id")?).await,
            _ => Err(Error::Unsupported(command.into())),
        }
    }
    async fn peer(&self, id: &str) -> Result<Agent> {
        let runtime = self.runtime()?;
        if let Some(agent) = runtime.agent(id).await {
            return Ok(agent);
        }
        let agent = runtime.resume(id).await?;
        self.0.agents.lock().await.insert(id.into(), agent.clone());
        Ok(agent)
    }
    async fn deliver(&self, agent: &Agent, prompt: &str) -> Result<Value> {
        let snapshot = agent.snapshot().await;
        if snapshot.state == SessionState::Idle {
            match agent.prompt(prompt).await {
                Ok(()) => return Ok(json!({"sessionId":snapshot.id,"delivery":"started"})),
                Err(Error::Busy) => {}
                Err(error) => return Err(error),
            }
        }
        let queued = agent.enqueue(prompt).await?;
        Ok(json!({"sessionId":snapshot.id,"delivery":"queued","queueId":queued.id}))
    }
    async fn run_schedule(&self, id: &str) -> Result<Value> {
        let schedule = {
            let mut state = self.0.state.lock().await;
            let schedule = state
                .schedules
                .get_mut(id)
                .ok_or_else(|| Error::Invalid("unknown schedule".into()))?;
            if schedule.status == "running" {
                return Err(Error::Busy);
            }
            schedule.status = "running".into();
            schedule.error = None;
            let schedule = schedule.clone();
            self.persist(&state)?;
            schedule
        };
        let result = async {
            let agent = self.peer(&schedule.session_id).await?;
            self.deliver(&agent, &schedule.text).await
        }
        .await;
        let mut state = self.0.state.lock().await;
        let current = state
            .schedules
            .get_mut(id)
            .expect("running schedule cannot be removed");
        current.last_run_at_ms = Some(now());
        current.status = if result.is_ok() { "delivered" } else { "error" }.into();
        match &result {
            Ok(value) => current.last_result = Some(value.clone()),
            Err(error) => current.error = Some(error.to_string()),
        }
        if let Some(seconds) = current.interval_seconds {
            current.next_run_at_ms = now().saturating_add(seconds.saturating_mul(1000));
        } else {
            current.enabled = false;
        }
        self.persist(&state)?;
        result.map(|mut value| {
            value["id"] = json!(id);
            value
        })
    }
    async fn start_fanout(&self, p: &Value) -> Result<Value> {
        let targets = p["targets"]
            .as_array()
            .filter(|v| !v.is_empty() && v.len() <= 8)
            .ok_or_else(|| Error::Invalid("comparison requires 1-8 targets".into()))?;
        let prompt = text(p, "text")?.to_owned();
        let timeout = p["timeoutSeconds"].as_u64().unwrap_or(300);
        if !(1..=3600).contains(&timeout) {
            return Err(Error::Invalid("timeoutSeconds must be 1-3600".into()));
        }
        let mut options = Vec::new();
        let mut results = Vec::new();
        for target in targets {
            let fields = target
                .as_object()
                .ok_or_else(|| Error::Invalid("target must be an object".into()))?;
            let mut args = json!({"cwd":text(p,"cwd")?});
            for (key, value) in fields {
                if !matches!(key.as_str(), "harness" | "model" | "cwd") {
                    return Err(Error::Invalid("unknown comparison target field".into()));
                }
                args[key] = value.clone();
            }
            let launch = self.launch_options(&args).await?;
            results.push(TargetResult {
                harness: Some(launch.harness.clone()),
                model: launch.model.clone(),
                session_id: None,
                status: "pending".into(),
                text: String::new(),
                truncated: false,
                error: None,
            });
            options.push(launch);
        }
        self.runtime()?;
        let id = uuid::Uuid::new_v4().to_string();
        let job = FanoutJob {
            id: id.clone(),
            status: "running".into(),
            created_at_ms: now(),
            targets: results,
        };
        {
            let mut state = self.0.state.lock().await;
            capacity(false, state.jobs.len())?;
            if state
                .jobs
                .values()
                .filter(|j| matches!(j.status.as_str(), "running" | "cancelling"))
                .count()
                >= 4
            {
                return Err(Error::Invalid(
                    "at most four model comparisons may run concurrently".into(),
                ));
            }
            state.jobs.insert(id.clone(), job.clone());
            self.persist(&state)?;
        }
        let weak = Arc::downgrade(&self.0);
        let task_id = id.clone();
        let task = tokio::spawn(async move {
            let tasks = options.into_iter().enumerate().map(|(index, options)| {
                let weak = weak.clone();
                let id = task_id.clone();
                let prompt = prompt.clone();
                async move {
                    if let Some(inner) = weak.upgrade() {
                        let extension = ResourcesExtension(inner);
                        extension
                            .run_target(&id, index, options, &prompt, timeout)
                            .await;
                    }
                }
            });
            futures_util::future::join_all(tasks).await;
            if let Some(inner) = weak.upgrade() {
                let extension = ResourcesExtension(inner);
                let mut state = extension.0.state.lock().await;
                if let Some(job) = state.jobs.get_mut(&task_id)
                    && job.status == "running"
                {
                    job.status = if job.targets.iter().any(|t| t.status != "completed") {
                        "completed_with_errors"
                    } else {
                        "completed"
                    }
                    .into();
                }
                if let Err(error) = extension.persist(&state) {
                    extension.record_error(error);
                }
            }
        });
        let mut jobs = self.0.jobs.lock().unwrap();
        jobs.retain(|_, task| !task.is_finished());
        jobs.insert(id, task);
        Ok(serde_json::to_value(job)?)
    }
    async fn target_update(
        &self,
        id: &str,
        index: usize,
        update: impl FnOnce(&mut TargetResult),
    ) -> Result<bool> {
        let mut state = self.0.state.lock().await;
        let job = state
            .jobs
            .get_mut(id)
            .ok_or_else(|| Error::Invalid("unknown comparison".into()))?;
        if job.status != "running" {
            return Ok(false);
        }
        update(&mut job.targets[index]);
        self.persist(&state)?;
        Ok(true)
    }
    async fn target_text(&self, id: &str, index: usize, delta: &str) -> Result<()> {
        let mut state = self.0.state.lock().await;
        let job = state
            .jobs
            .get_mut(id)
            .ok_or_else(|| Error::Invalid("unknown comparison".into()))?;
        if job.status != "running" {
            return Ok(());
        }
        let target = &mut job.targets[index];
        let combined = format!("{}{}", target.text, delta);
        let (text, truncated) = bounded(&combined, 16 * 1024);
        target.text = text;
        target.truncated |= truncated;
        self.0.text_dirty.store(true, Ordering::SeqCst);
        Ok(())
    }
    async fn run_target(
        &self,
        id: &str,
        index: usize,
        options: SpawnOptions,
        prompt: &str,
        seconds: u64,
    ) {
        let result = async {
            let agent = self.runtime()?.spawn(options).await?;
            let snapshot = agent.snapshot().await;
            self.0
                .agents
                .lock()
                .await
                .insert(snapshot.id.clone(), agent.clone());
            if !self
                .target_update(id, index, |target| {
                    target.session_id = Some(snapshot.id);
                    target.status = "running".into();
                })
                .await?
            {
                agent.kill().await?;
                return Ok(());
            }
            let mut events = agent.events();
            agent.prompt(prompt).await?;
            let outcome = tokio::time::timeout(Duration::from_secs(seconds), async {
                while let Some(event) = events.next().await {
                    let event = event?;
                    match event.kind {
                        EventKind::TextDelta { text, .. } => {
                            self.target_text(id, index, &text).await?;
                        }
                        EventKind::PermissionRequested(_) => {
                            self.target_update(id, index, |target| {
                                target.status = "needs_input".into()
                            })
                            .await?;
                        }
                        EventKind::PermissionResolved { .. } => {
                            self.target_update(id, index, |target| {
                                target.status = "running".into()
                            })
                            .await?;
                        }
                        EventKind::TurnEnded { reason } => {
                            self.target_update(id, index, |target| {
                                target.status = if reason == "completed"
                                    || reason == "end_turn"
                                    || reason == "stop"
                                {
                                    "completed"
                                } else {
                                    reason.as_str()
                                }
                                .into();
                            })
                            .await?;
                            return Ok(());
                        }
                        EventKind::SessionEnded => return Err(Error::Closed),
                        _ => {}
                    }
                }
                Err(Error::Closed)
            })
            .await;
            match outcome {
                Ok(result) => result,
                Err(_) => {
                    let cancel = agent.cancel().await;
                    self.target_update(id, index, |target| {
                        target.status = "timeout".into();
                        target.error = Some(match cancel {
                            Ok(()) => "Comparison timed out and cancellation was requested".into(),
                            Err(error) => {
                                format!("Comparison timed out; cancellation failed: {error}")
                            }
                        });
                    })
                    .await?;
                    Err(Error::Timeout("model comparison".into()))
                }
            }
        }
        .await;
        if let Err(error) = result
            && let Err(persistence) = self
                .target_update(id, index, |target| {
                    if target.status != "timeout" {
                        target.status = "error".into();
                        target.error = Some(error.to_string());
                    }
                })
                .await
        {
            self.record_error(persistence);
        }
    }
    async fn cancel_fanout(&self, id: &str) -> Result<Value> {
        let ids = {
            let mut state = self.0.state.lock().await;
            let job = state
                .jobs
                .get_mut(id)
                .ok_or_else(|| Error::Invalid("unknown comparison".into()))?;
            if job.status != "running" {
                return Ok(json!({"job":job,"errors":[]}));
            }
            let ids = job
                .targets
                .iter()
                .filter(|target| {
                    matches!(
                        target.status.as_str(),
                        "running" | "needs_input" | "pending"
                    )
                })
                .filter_map(|target| target.session_id.clone())
                .collect::<Vec<_>>();
            // Claim cancellation atomically with collecting known native sessions. A concurrent spawn closes itself when its target update is rejected.
            job.status = "cancelling".into();
            self.persist(&state)?;
            ids
        };
        let mut errors = Vec::new();
        for session_id in ids {
            if let Some(agent) = self.runtime()?.agent(&session_id).await
                && let Err(error) = agent.cancel().await
            {
                errors.push(format!("{session_id}: {error}"));
            }
        }
        let mut state = self.0.state.lock().await;
        let job = state.jobs.get_mut(id).unwrap();
        job.status = if errors.is_empty() {
            "cancelled"
        } else {
            "cancellation_failed"
        }
        .into();
        for target in &mut job.targets {
            if matches!(
                target.status.as_str(),
                "running" | "needs_input" | "pending"
            ) {
                target.status = job.status.clone();
            }
        }
        let output = json!({"job":job,"errors":errors});
        self.persist(&state)?;
        Ok(output)
    }
}
