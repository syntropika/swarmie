mod acp;
mod antigravity;
mod claude;
mod codex;
mod history;
mod opencode;
mod pi;
mod readiness;
mod settings;
pub use acp::AcpHarness;
pub use antigravity::AntigravityHarness;
pub use claude::ClaudeHarness;
pub use codex::CodexHarness;
use mpai::*;
pub use opencode::OpenCodeHarness;
pub use pi::PiHarness;
use process::{JsonProcess, WireStyle, find_executable};
use serde_json::{Value, json};
use std::path::PathBuf;
use tokio::sync::mpsc;

pub fn builtin_builder() -> RuntimeBuilder {
    Runtime::builder()
        .harness(CodexHarness)
        .harness(PiHarness)
        .harness(ClaudeHarness)
        .harness(OpenCodeHarness)
        .harness(AntigravityHarness)
        .harness(AcpHarness::new(
            "gemini",
            "gemini",
            vec!["--experimental-acp".into()],
        ))
        .harness(AcpHarness::new("qwen", "qwen", vec!["--acp".into()]))
        .harness(AcpHarness::new(
            "cursor",
            "cursor-agent",
            vec!["acp".into()],
        ))
        .harness(AcpHarness::new(
            "grok",
            "grok",
            vec![
                "--no-auto-update".into(),
                "agent".into(),
                "--no-leader".into(),
                "stdio".into(),
            ],
        ))
        .harness(AcpHarness::new("hermes", "hermes", vec!["acp".into()]))
        .harness(AcpHarness::new("kiro", "kiro-cli", vec!["acp".into()]))
}
fn launch(options: &SpawnOptions, default: &str, args: Vec<String>) -> (PathBuf, Vec<String>) {
    let executable = options.config["executable"]
        .as_str()
        .map(PathBuf::from)
        .or_else(|| configured_executable(&options.harness))
        .or_else(|| configured_executable(default))
        .unwrap_or_else(|| default.into());
    let args = options.config["args"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or(args);
    (executable, args)
}
fn execution_env(
    options: &SpawnOptions,
    config_variable: Option<&str>,
) -> Result<std::collections::BTreeMap<String, String>> {
    let mut env = options.execution.env.clone();
    if let Some(home) = &options.execution.config_home {
        let name = config_variable
            .ok_or_else(|| Error::Unsupported("configuration home for this harness".into()))?;
        std::fs::create_dir_all(home)?;
        let home = std::fs::canonicalize(home)?;
        if let Some(previous) = env.get(name)
            && previous != &home.to_string_lossy()
        {
            return Err(Error::Invalid(format!(
                "conflicting {name} and configuration home"
            )));
        }
        env.insert(name.into(), home.to_string_lossy().into_owned());
    }
    Ok(env)
}
async fn spawn_json(
    options: &SpawnOptions,
    exe: &std::path::Path,
    args: &[String],
    style: WireStyle,
    env: std::collections::BTreeMap<String, String>,
) -> Result<(JsonProcess, mpsc::Receiver<Value>)> {
    let diagnostic = &options.config["diagnostics"];
    let recording = if let Some(path) = diagnostic["recording"]["path"].as_str() {
        let bytes = diagnostic["recording"]["maxBytes"]
            .as_u64()
            .unwrap_or(8 * 1024 * 1024);
        if bytes == 0 || bytes > 64 * 1024 * 1024 {
            return Err(Error::Invalid(
                "wire recording budget must be between 1 byte and 64 MiB".into(),
            ));
        }
        let requested = PathBuf::from(path);
        let path = if requested.exists() {
            let stem = requested
                .file_stem()
                .and_then(|v| v.to_str())
                .unwrap_or("trace");
            requested.with_file_name(format!("{stem}-{}.jsonl", uuid::Uuid::new_v4()))
        } else {
            requested
        };
        Some(process::WireRecording {
            path,
            max_bytes: bytes as usize,
        })
    } else {
        None
    };
    let stall_timeout = diagnostic["stallTimeoutMs"]
        .as_u64()
        .map(std::time::Duration::from_millis);
    JsonProcess::spawn_with_options(
        exe,
        args,
        &options.cwd,
        style,
        &process::ProcessOptions {
            environment: env,
            recording,
            stall_timeout,
        },
    )
    .await
}
fn detection(id: &str, executable: &str, capabilities: Capabilities) -> Detection {
    let path = if let Some(path) = configured_executable(id) {
        find_executable(&path.to_string_lossy())
    } else {
        find_executable(executable)
    };
    Detection {
        harness: id.into(),
        available: path.is_some(),
        executable: path,
        capabilities,
    }
}
fn configured_executable(id: &str) -> Option<PathBuf> {
    std::env::var_os(format!(
        "MPAI_{}_EXECUTABLE",
        id.to_uppercase().replace('-', "_")
    ))
    .map(PathBuf::from)
}
async fn terminal_frame(frame: &Value, tx: &mpsc::Sender<BackendEvent>) -> bool {
    if let Some(message) = frame
        .get("_runtime_exit")
        .or_else(|| frame.get("_runtime_error"))
        .and_then(Value::as_str)
    {
        let _ = tx
            .send(BackendEvent::Exited {
                message: message.into(),
            })
            .await;
        true
    } else {
        false
    }
}
fn string(value: &Value, name: &str) -> String {
    value[name].as_str().unwrap_or_default().into()
}
fn option(id: &str, label: &str, kind: &str) -> PermissionOption {
    PermissionOption {
        id: id.into(),
        label: label.into(),
        kind: kind.into(),
    }
}
fn native(provider: &str, id: String) -> NativeSessionRef {
    NativeSessionRef {
        provider: provider.into(),
        id,
    }
}
fn input(prompt: &Prompt) -> Result<Value> {
    Ok(Value::Array(
        prompt
            .content
            .iter()
            .map(|c| match c {
                PromptContent::Text(text) => {
                    Ok(json!({"type":"text","text":text,"text_elements":[]}))
                }
                PromptContent::Image { data, mime_type } => {
                    Ok(json!({"type":"image","url":format!("data:{mime_type};base64,{data}")}))
                }
                PromptContent::Resource { uri } => {
                    Err(Error::Unsupported(format!("resource input {uri}")))
                }
                PromptContent::Skill { name, path } => {
                    if name.trim().is_empty() || path.trim().is_empty() || path.contains('\0') {
                        return Err(Error::Invalid(
                            "native skill requires a name and path".into(),
                        ));
                    }
                    Ok(json!({"type":"skill","name":name,"path":path}))
                }
            })
            .collect::<Result<Vec<_>>>()?,
    ))
}

/// Common client-facing tool fields; every provider frame remains in the event's raw payload.
fn tool(value: &Value, status: &str) -> ToolEvent {
    fn first(v: &Value, keys: &[&str]) -> Value {
        keys.iter()
            .find_map(|k| v.get(*k).filter(|v| !v.is_null()))
            .cloned()
            .unwrap_or(Value::Null)
    }
    let state = value.get("state").unwrap_or(value);
    let id = first(
        value,
        &["id", "itemId", "toolCallId", "callID", "tool_use_id"],
    );
    let input = first(value, &["input", "args", "rawInput", "command"]);
    let input = if input.is_null() {
        first(state, &["input", "args"])
    } else {
        input
    };
    let output = first(
        value,
        &[
            "output",
            "result",
            "rawOutput",
            "content",
            "aggregatedOutput",
            "delta",
        ],
    );
    let output = if output.is_null() {
        first(state, &["output", "error"])
    } else {
        output
    };
    let mut event = ToolEvent {
        details: None,
        id: id.as_str().unwrap_or_default().into(),
        name: first(value, &["name", "toolName", "tool", "title", "type"])
            .as_str()
            .map(String::from),
        status: if value["isError"] == true
            || value["is_error"] == true
            || state["status"] == "error"
            || state["status"] == "failed"
            || value["exitCode"].as_i64().is_some_and(|code| code != 0)
        {
            "failed"
        } else {
            match state["status"].as_str() {
                Some("declined" | "denied" | "rejected") => "denied",
                Some("cancelled" | "canceled") => "cancelled",
                Some("inProgress" | "running" | "pending") => "running",
                Some("completed") => "completed",
                _ => status,
            }
        }
        .into(),
        input,
        output,
        metadata: json!({
            "kind":first(value,&["kind","type"]),
            "locations":first(value,&["locations","paths","fileLocations"]),
            "diff":first(value,&["diff","changes","patch"]),
            "progress":first(value,&["progress","progressMessage"]),
            "outputDelta":value.get("delta"),
        }),
    };
    event.details = Some(tool_details(value, &event));
    event
}
/// Normalize only fields carried by the native frame or already retained metadata.
fn canonical_permission(mut request: PermissionRequest) -> PermissionRequest {
    // Invalid legacy questions remain visible for the session actor to reject.
    if let Ok(questions) = request.canonical_questions() {
        request.questions = questions;
    }
    request
}

fn tool_details(value: &Value, event: &ToolEvent) -> ToolDetails {
    fn first_text(values: &[&Value]) -> Option<String> {
        values.iter().find_map(|v| v.as_str().map(str::to_owned))
    }
    fn location(value: &Value) -> Option<FileLocation> {
        if let Some(path) = value.as_str() {
            return Some(FileLocation {
                path: path.into(),
                ..Default::default()
            });
        }
        let path = value["path"].as_str().or(value["filePath"].as_str())?;
        Some(FileLocation {
            path: path.into(),
            line: value["line"].as_u64(),
            column: value["column"].as_u64(),
            end_line: value["endLine"].as_u64(),
            end_column: value["endColumn"].as_u64(),
            extra: value
                .as_object()
                .map(|map| {
                    map.iter()
                        .filter(|(key, _)| {
                            !matches!(
                                key.as_str(),
                                "path" | "filePath" | "line" | "column" | "endLine" | "endColumn"
                            )
                        })
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
    fn diff(value: &Value, fallback: Option<&str>) -> Option<FileDiff> {
        let path = value["path"]
            .as_str()
            .or(value["filePath"].as_str())
            .or(fallback)?;
        let mut extra = value
            .as_object()
            .map(|map| {
                map.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<std::collections::BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        for field in [
            "path", "filePath", "oldText", "newText", "diff", "unified", "kind",
        ] {
            extra.remove(field);
        }
        Some(FileDiff {
            path: path.into(),
            old_text: value["oldText"].as_str().map(str::to_owned),
            new_text: value["newText"].as_str().map(str::to_owned),
            unified: value
                .as_str()
                .or(value["unified"].as_str())
                .or(value["diff"].as_str())
                .map(str::to_owned),
            change: value["kind"]
                .as_str()
                .or(value["kind"]["type"].as_str())
                .map(str::to_owned),
            extra,
        })
    }
    let metadata = &event.metadata;
    let input = &event.input;
    let reported = first_text(&[&value["kind"], &metadata["kind"], &value["type"]]);
    let kind = reported.map(|kind| match kind.as_str() {
        "commandExecution" => "execute".into(),
        "fileChange" => "edit".into(),
        "mcpToolCall" => "mcp".into(),
        "collabAgentToolCall" | "subAgentActivity" => "subagent".into(),
        other => other.to_owned(),
    });
    let mut locations = Vec::new();
    if let Some(source) = [&value["locations"], &metadata["locations"], &value["paths"]]
        .into_iter()
        .find(|v| !v.is_null())
    {
        if let Some(array) = source.as_array() {
            locations.extend(array.iter().filter_map(location));
        } else if let Some(item) = location(source) {
            locations.push(item);
        }
    }
    let path = first_text(&[
        &input["path"],
        &input["file_path"],
        &input["filePath"],
        &input["TargetFile"],
    ]);
    if let Some(path) = &path
        && !locations.iter().any(|l| &l.path == path)
    {
        locations.push(FileLocation {
            path: path.clone(),
            ..Default::default()
        });
    }
    let mut diffs = Vec::new();
    for source in [&value["changes"], &value["diffs"]] {
        if let Some(array) = source.as_array() {
            diffs.extend(array.iter().filter_map(|v| diff(v, path.as_deref())));
        }
    }
    if let Some(source) = [&value["content"], &metadata["content"]]
        .into_iter()
        .find(|v| !v.is_null())
        && let Some(array) = source.as_array()
    {
        diffs.extend(
            array
                .iter()
                .filter(|v| v["type"] == "diff")
                .filter_map(|v| diff(v, path.as_deref())),
        );
    }
    if diffs.is_empty()
        && let Some(item) = diff(&metadata["diff"], path.as_deref())
        && (item.unified.is_some() || item.old_text.is_some() || item.new_text.is_some())
    {
        diffs.push(item);
    }
    if diffs.is_empty()
        && let Some(path) = &path
    {
        let old_text = input["old_string"].as_str().map(str::to_owned);
        let new_text = input["new_string"].as_str().map(str::to_owned).or_else(|| {
            (event.name.as_deref() == Some("Write"))
                .then(|| input["content"].as_str())
                .flatten()
                .map(str::to_owned)
        });
        if old_text.is_some() || new_text.is_some() {
            diffs.push(FileDiff {
                path: path.clone(),
                old_text,
                new_text,
                ..Default::default()
            });
        }
    }
    for change in &diffs {
        if !locations.iter().any(|l| l.path == change.path) {
            locations.push(FileLocation {
                path: change.path.clone(),
                ..Default::default()
            });
        }
    }
    let progress = &metadata["progress"];
    let message = first_text(&[progress, &progress["message"], &value["progressMessage"]]);
    let elapsed_seconds = progress["elapsedSeconds"]
        .as_f64()
        .or(value["elapsed_time_seconds"].as_f64())
        .or_else(|| progress["elapsedMs"].as_f64().map(|ms| ms / 1000.0));
    let percent = progress["percent"].as_f64();
    let progress = if message.is_some() || elapsed_seconds.is_some() || percent.is_some() {
        Some(ToolProgress {
            message,
            elapsed_seconds,
            percent,
            extra: Default::default(),
        })
    } else {
        None
    };
    let mut extra: std::collections::BTreeMap<String, Value> = metadata
        .as_object()
        .map(|map| map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    for field in [
        "kind",
        "title",
        "command",
        "cwd",
        "locations",
        "diffs",
        "progress",
        "subagent",
    ] {
        extra.remove(field);
    }
    // Unclassified metadata remains available alongside the stable projections.
    let subagent = metadata.get("subagent").filter(|v|v.is_object()).cloned().map(SubagentInfo::from)
        .or_else(|| matches!(event.name.as_deref(),Some("Agent"|"Task")).then(|| SubagentInfo::from(json!({
            "id":metadata["taskId"],"parentToolId":event.id,"role":input["subagent_type"],"model":input["model"],"status":event.status,"summary":metadata["summary"],"background":metadata["background"]
        }))));
    ToolDetails {
        kind,
        title: first_text(&[&value["title"]]),
        command: first_text(&[&input["command"], &value["command"]]),
        cwd: first_text(&[&input["cwd"], &value["cwd"]]),
        locations,
        diffs,
        progress,
        subagent,
        extra: std::mem::take(&mut extra),
    }
}

fn usage(value: &Value) -> Usage {
    let scope = if value.get("tokenUsage").is_some() {
        "session"
    } else {
        "turn"
    };
    let v = if let Some(t) = value.get("tokenUsage") {
        &t["total"]
    } else {
        value
            .get("usage")
            .or_else(|| value.get("tokens"))
            .unwrap_or(value)
    };
    let number = |names: &[&str]| names.iter().find_map(|k| v.get(*k).and_then(Value::as_u64));
    Usage {
        scope: scope.into(),
        input_tokens: number(&["inputTokens", "input_tokens", "input"]),
        output_tokens: number(&["outputTokens", "output_tokens", "output"]),
        cached_input_tokens: number(&[
            "cachedInputTokens",
            "cache_read_input_tokens",
            "cacheRead",
            "cachedReadTokens",
        ])
        .or_else(|| v["cache"]["read"].as_u64()),
        cache_write_tokens: number(&[
            "cacheWriteInputTokens",
            "cache_creation_input_tokens",
            "cacheWrite",
        ])
        .or_else(|| v["cache"]["write"].as_u64()),
        total_tokens: number(&["totalTokens", "total_tokens", "total"]).or_else(|| {
            match (
                number(&["inputTokens", "input_tokens", "input"]),
                number(&["outputTokens", "output_tokens", "output"]),
            ) {
                (Some(i), Some(o)) => i.checked_add(o),
                _ => None,
            }
        }),
        cost_usd: value["costUsd"]
            .as_f64()
            .or_else(|| v["cost"]["total"].as_f64())
            .or_else(|| value["cost"].as_f64())
            .or_else(|| {
                value["cost"]["amount"]
                    .as_f64()
                    .filter(|_| value["cost"]["currency"] == "USD")
            }),
    }
}

#[cfg(test)]
mod semantic_tool_contracts {
    use super::*;

    #[test]
    fn native_file_change_exposes_real_paths_and_unified_patch() {
        let native = json!({"id":"change","type":"fileChange","changes":[{"path":"src/app.rs","kind":"update","diff":"@@ -1 +1 @@\n-old\n+new","vendor":{"checkpoint":"native"}}]});
        let event = tool(&native, "completed");
        let details = event.details.unwrap();
        assert_eq!(details.kind.as_deref(), Some("edit"));
        assert_eq!(details.locations[0].path, "src/app.rs");
        assert_eq!(
            details.diffs[0].unified.as_deref(),
            Some("@@ -1 +1 @@\n-old\n+new")
        );
        assert_eq!(details.diffs[0].extra["vendor"]["checkpoint"], "native");
        assert!(details.diffs[0].old_text.is_none());
    }

    #[test]
    fn native_command_and_progress_keep_details_without_guessing_missing_fields() {
        let mut event = tool(
            &json!({"id":"command","type":"commandExecution","command":"cargo check","cwd":"/workspace"}),
            "running",
        );
        event.metadata["progress"] = json!({"elapsedMs":1500,"message":"Checking"});
        event.metadata["vendor"] = json!({"receipt":"native"});
        event.details = Some(tool_details(
            &json!({"type":"commandExecution","command":"cargo check","cwd":"/workspace"}),
            &event,
        ));
        let details = event.details.unwrap();
        assert_eq!(details.command.as_deref(), Some("cargo check"));
        assert_eq!(details.cwd.as_deref(), Some("/workspace"));
        assert_eq!(details.progress.unwrap().elapsed_seconds, Some(1.5));
        assert_eq!(details.extra["vendor"]["receipt"], "native");
        assert!(details.diffs.is_empty());
        let unknown = tool(
            &json!({"id":"native-unknown","name":"vendorTool","input":{"opaque":true}}),
            "running",
        );
        assert!(unknown.details.unwrap().kind.is_none());
    }
    #[test]
    fn native_edit_and_agent_inputs_expose_content_and_child_parameters() {
        let edit = tool(
            &json!({"id":"edit","name":"Edit","input":{"file_path":"src/app.rs","old_string":"old","new_string":"new"}}),
            "running",
        );
        let details = edit.details.unwrap();
        assert_eq!(details.diffs[0].path, "src/app.rs");
        assert_eq!(details.diffs[0].old_text.as_deref(), Some("old"));
        assert_eq!(details.diffs[0].new_text.as_deref(), Some("new"));
        let write = tool(
            &json!({"id":"write","name":"Write","input":{"file_path":"new.rs","content":"new file"}}),
            "running",
        );
        assert!(write.details.as_ref().unwrap().diffs[0].old_text.is_none());
        assert_eq!(
            write.details.unwrap().diffs[0].new_text.as_deref(),
            Some("new file")
        );
        let child = tool(
            &json!({"id":"agent-tool","name":"Agent","input":{"subagent_type":"researcher","model":"native-model"}}),
            "running",
        );
        let child = child.details.unwrap().subagent.unwrap();
        assert_eq!(child.parent_tool_id.as_deref(), Some("agent-tool"));
        assert_eq!(child.role.as_deref(), Some("researcher"));
        assert_eq!(child.model.as_deref(), Some("native-model"));
        assert!(child.id.is_none());
    }
}
