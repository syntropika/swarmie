//! Version-qualified Grok ACP extensions, separate from portable ACP behavior.
use super::*;

const REVIEWED_VERSION: &str = "1.0.52";
const SOURCE_COMMIT: &str = "2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8";

pub(super) struct Extensions;
pub(super) fn unqualified(capabilities: &mut Capabilities) {
    let unknown = || Capability {
        source: CapabilitySource::Extension,
        ..Capability::unknown(
            "Grok extensions require the reviewed native initialize identity and version",
        )
    };
    capabilities.steering = unknown();
    capabilities.rollback = unknown();
    capabilities.background_tasks = unknown();
}
impl Extensions {
    pub(super) fn negotiate(harness: &str, initialize: &Value) -> Option<Self> {
        (harness == "grok"
            && initialize["_meta"]["grokShell"] == true
            && initialize["_meta"]["agentVersion"] == REVIEWED_VERSION)
            .then_some(Self)
    }
    pub(super) fn capabilities(&self, capabilities: &mut Capabilities) {
        let extension = |method: &str| Capability {
            source: CapabilitySource::Extension,
            source_id: Some(method.into()),
            ..Capability::negotiated(true)
        };
        capabilities.steering = extension("_x.ai/interject");
        capabilities.rollback = extension("_x.ai/rewind/execute");
        capabilities.background_tasks = extension("_x.ai/task/list");
        capabilities.metadata.insert(
            "grokExtensions".into(),
            json!({
                "version":REVIEWED_VERSION,"sourceCommit":SOURCE_COMMIT,
                "rollbackScopes":["conversation_only","conversation_and_files"],
                "taskOperations":["list","stop"],"taskOutputFiles":false
            }),
        );
    }
    pub(super) async fn steer(
        &self,
        wire: &JsonProcess,
        session: &str,
        prompt: &Prompt,
    ) -> Result<()> {
        let text = prompt
            .content
            .iter()
            .map(|content| match content {
                PromptContent::Text(text) => Ok(text.as_str()),
                _ => Err(Error::Unsupported(
                    "Grok steering supports text input only".into(),
                )),
            })
            .collect::<Result<Vec<_>>>()?
            .join("\n");
        if text.trim().is_empty() {
            return Err(Error::Invalid("Grok steering requires text".into()));
        }
        let receipt = envelope(wire.request("_x.ai/interject", json!({
            "sessionId":session,"text":text,"interjectionId":uuid::Uuid::new_v4().to_string()
        })).await?)?;
        if receipt["status"] != "queued" {
            return Err(invalid_receipt());
        }
        Ok(())
    }
    pub(super) async fn rollback(
        &self,
        wire: &JsonProcess,
        session: &str,
        turns: u64,
        scope: RollbackScope,
    ) -> Result<()> {
        if turns == 0 {
            return Err(Error::Invalid("Rollback requires at least one turn".into()));
        }
        // The native list contains every prompt checkpoint, in the current timeline.
        let response = wire
            .request("_x.ai/rewind/points", json!({"sessionId":session}))
            .await?;
        let points = response["rewind_points"]
            .as_array()
            .ok_or_else(invalid_receipt)?;
        let turns = usize::try_from(turns)
            .map_err(|_| Error::Invalid("Rollback exceeds native history".into()))?;
        let index = points
            .len()
            .checked_sub(turns)
            .ok_or_else(|| Error::Invalid("Rollback exceeds native history".into()))?;
        if !points
            .iter()
            .enumerate()
            .all(|(i, p)| p["prompt_index"].as_u64() == Some(i as u64))
        {
            return Err(invalid_receipt());
        }
        let mode = match scope {
            RollbackScope::ConversationOnly => "conversation_only",
            RollbackScope::ConversationAndFiles => "all",
        };
        let receipt = wire
            .request(
                "_x.ai/rewind/execute",
                json!({
                    "sessionId":session,"targetPromptIndex":index,"force":false,"mode":mode
                }),
            )
            .await?;
        if receipt["success"] != true
            || receipt["target_prompt_index"] != index
            || receipt["mode"] != mode
        {
            return Err(Error::Provider("Grok did not confirm the requested rollback; native conflicts or unavailable history may prevent it".into()));
        }
        Ok(())
    }
    pub(super) async fn tasks(&self, wire: &JsonProcess, session: &str) -> Result<Vec<AgentTask>> {
        let receipt = envelope(
            wire.request("_x.ai/task/list", json!({"sessionId":session}))
                .await?,
        )?;
        let rows = receipt["tasks"].as_array().ok_or_else(invalid_receipt)?;
        if rows.len() > 1024 {
            return Err(invalid_receipt());
        }
        Ok(rows
            .iter()
            .filter(|row| row["is_backgrounded"] == true)
            .filter(|row| {
                row["owner_session_id"]
                    .as_str()
                    .is_none_or(|owner| owner == session)
            })
            .filter_map(task)
            .collect())
    }
    pub(super) async fn stop_task(
        &self,
        wire: &JsonProcess,
        session: &str,
        id: &str,
    ) -> Result<Option<AgentTask>> {
        if id.is_empty() || id.len() > 512 || id.chars().any(char::is_control) {
            return Err(Error::Invalid("Invalid Grok background task ID".into()));
        }
        let mut selected = self
            .tasks(wire, session)
            .await?
            .into_iter()
            .find(|task| task.id == id)
            .ok_or_else(|| {
                Error::Invalid("Unknown background task for this Grok session".into())
            })?;
        let receipt = envelope(
            wire.request(
                "_x.ai/task/kill",
                json!({
                    "sessionId":session,"taskId":id,"source":"clientUi"
                }),
            )
            .await?,
        )?;
        if receipt["taskId"] != id {
            return Err(invalid_receipt());
        }
        match receipt["outcome"].as_str() {
            Some("killed") => {
                selected.status = AgentTaskStatus::Stopped;
                Ok(Some(selected))
            }
            Some("already_exited") => Ok(self
                .tasks(wire, session)
                .await?
                .into_iter()
                .find(|task| task.id == id)),
            Some("not_found") => Err(Error::Invalid(
                "The native Grok task no longer exists".into(),
            )),
            _ => Err(invalid_receipt()),
        }
    }
}
fn invalid_receipt() -> Error {
    Error::Provider("Grok returned an unrecognized extension receipt".into())
}
fn envelope(value: Value) -> Result<Value> {
    if value.get("error").is_some_and(|error| !error.is_null()) || value["result"].is_null() {
        return Err(Error::Provider(
            "The native Grok extension could not complete the requested operation".into(),
        ));
    }
    Ok(value["result"].clone())
}
fn task(row: &Value) -> Option<AgentTask> {
    let id = row["task_id"].as_str()?.to_owned();
    if id.is_empty() || id.len() > 512 || id.chars().any(char::is_control) {
        return None;
    }
    let status = if row["explicitly_killed"] == true {
        AgentTaskStatus::Stopped
    } else if let Some(status) = row["status"].as_str() {
        match status {
            "running" => AgentTaskStatus::Running,
            "completed" => AgentTaskStatus::Completed,
            "failed" => AgentTaskStatus::Failed,
            _ => AgentTaskStatus::Unknown,
        }
    } else if row["completed"] == false {
        AgentTaskStatus::Running
    } else if row["completed"] == true
        && (row["exit_code"] == 0 || row["exit_code"].is_null() && row["signal"].is_null())
    {
        AgentTaskStatus::Completed
    } else if row["completed"] == true {
        AgentTaskStatus::Failed
    } else {
        AgentTaskStatus::Unknown
    };
    let description = row["description"]
        .as_str()
        .or(row["display_command"].as_str())
        .or(row["command"].as_str())
        .unwrap_or("Background task")
        .chars()
        .take(1024)
        .collect();
    Some(AgentTask {
        id,
        description,
        status,
        kind: row["kind"].as_str().map(String::from),
        summary: row["output"]
            .as_str()
            .filter(|text| !text.is_empty())
            .map(|text| text.chars().take(2048).collect()),
    })
}
fn prompt_usage(value: &Value) -> Usage {
    let mut reported = usage(value);
    reported.scope = "turn".into();
    reported.cache_write_tokens = value["cacheCreationTokens"].as_u64();
    reported.cost_usd = if value["usageIsIncomplete"] == true || value["costIsPartial"] == true {
        None
    } else {
        value["costUsdTicks"]
            .as_i64()
            .filter(|ticks| *ticks >= 0)
            .map(|ticks| ticks as f64 / 10_000_000_000.0)
    };
    reported
}
pub(super) fn events(params: &Value, session: &str, active: bool) -> Vec<EventKind> {
    if params["sessionId"] != session {
        return vec![];
    }
    let update = &params["update"];
    match update["sessionUpdate"].as_str() {
        Some("background_tasks") => update["tasks"]
            .as_array()
            .into_iter()
            .flatten()
            .take(1024)
            .filter_map(task)
            .map(EventKind::TaskUpdated)
            .collect(),
        Some("task_backgrounded") => {
            let mut row = update.clone();
            row["status"] = json!("running");
            task(&row).map(EventKind::TaskUpdated).into_iter().collect()
        }
        Some("task_completed") => {
            let row = &update["task_snapshot"];
            if row["is_backgrounded"] != true
                || row["owner_session_id"]
                    .as_str()
                    .is_some_and(|owner| owner != session)
            {
                return vec![];
            }
            task(row).map(EventKind::TaskUpdated).into_iter().collect()
        }
        Some("subagent_spawned" | "subagent_progress" | "subagent_finished")
            if update["subagent_id"].is_string() =>
        {
            if update["parent_session_id"]
                .as_str()
                .is_some_and(|parent| parent != session)
            {
                return vec![];
            }
            vec![EventKind::SubagentUpdated(json!({
                "id":update["subagent_id"],"parentSessionId":session,
                "role":update["subagent_type"],"model":update["model"],
                "title":update["description"],"summary":update["output"],
                "status":update["status"].as_str().unwrap_or("running"),"tokens":update["tokens_used"],
                "durationMs":update["duration_ms"],"childSessionId":update["child_session_id"]
            }).into())]
        }
        Some("turn_completed") if active && update["usage"].is_object() => {
            vec![EventKind::UsageUpdated(prompt_usage(&update["usage"]))]
        }
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_native_notifications_keep_parent_child_identity_and_task_status() {
        let params = json!({"sessionId":"parent","update":{
            "sessionUpdate":"subagent_spawned","subagent_id":"child",
            "parent_session_id":"parent","child_session_id":"native-child",
            "subagent_type":"explore","model":"native-model","description":"Inspect parser"
        }});
        let events = events(&params, "parent", false);
        assert!(
            matches!(&events[0],EventKind::SubagentUpdated(info) if info.id.as_deref()==Some("child") && info.parent_session_id.as_deref()==Some("parent") && info.role.as_deref()==Some("explore") && info.status.as_deref()==Some("running"))
        );
        assert!(super::events(&params, "other", false).is_empty());
        let completed = json!({"sessionId":"parent","update":{
            "sessionUpdate":"task_completed","task_snapshot":{
                "task_id":"background","completed":true,"is_backgrounded":true,
                "owner_session_id":"parent","description":"Build","kind":"bash","exit_code":3
            }
        }});
        assert!(
            matches!(&super::events(&completed,"parent",false)[0],EventKind::TaskUpdated(task) if task.status == AgentTaskStatus::Failed)
        );
        let mut sibling = completed;
        sibling["update"]["task_snapshot"]["owner_session_id"] = json!("sibling");
        assert!(super::events(&sibling, "parent", false).is_empty());
    }
    #[test]
    fn replayed_terminal_usage_does_not_double_count_and_incomplete_cost_stays_unknown() {
        let params = json!({"sessionId":"native","update":{
            "sessionUpdate":"turn_completed","usage":{"inputTokens":4,"outputTokens":2,
            "costUsdTicks":5000000000i64,"usageIsIncomplete":true}
        }});
        assert!(events(&params, "native", false).is_empty());
        assert!(
            matches!(&events(&params,"native",true)[0],EventKind::UsageUpdated(usage) if usage.total_tokens==Some(6) && usage.cost_usd.is_none())
        );
    }
}
