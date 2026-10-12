//! Provider-owned readers for native CLI session journals.
use swarmie::*;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

fn root(query: &NativeHistoryQuery) -> Result<PathBuf> {
    if let Some(home) = &query.config_home {
        return Ok(home.clone());
    }
    let variable = match query.harness.as_str() {
        "codex" => "CODEX_HOME",
        "claude" => "CLAUDE_CONFIG_DIR",
        "pi" => "PI_CODING_AGENT_DIR",
        _ => return Err(Error::Unsupported("native history for this harness".into())),
    };
    if let Some(home) = std::env::var_os(variable) {
        return Ok(home.into());
    }
    let home =
        std::env::var_os("HOME").ok_or_else(|| Error::Invalid("HOME is unavailable".into()))?;
    Ok(PathBuf::from(home).join(match query.harness.as_str() {
        "codex" => ".codex",
        "claude" => ".claude",
        _ => ".pi/agent",
    }))
}
async fn files(query: &NativeHistoryQuery) -> Result<Vec<PathBuf>> {
    if query.limit == 0 || query.limit > 1000 {
        return Err(Error::Invalid(
            "native history limit must be between 1 and 1000".into(),
        ));
    }
    let root = root(query)?;
    let mut directories = match query.harness.as_str() {
        "codex" => vec![root.join("sessions"), root.join("archived_sessions")],
        "claude" => vec![root.join("projects")],
        "pi" => vec![root.join("sessions")],
        _ => return Err(Error::Unsupported("native history".into())),
    };
    let mut found = vec![];
    while let Some(directory) = directories.pop() {
        let mut entries = match tokio::fs::read_dir(directory).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        while let Some(entry) = entries.next_entry().await? {
            let kind = entry.file_type().await?;
            if kind.is_dir() {
                directories.push(entry.path());
            } else if kind.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "jsonl")
            {
                found.push(entry.path());
            }
            if found.len() + directories.len() > 100_000 {
                return Err(Error::Invalid(
                    "native history scan exceeds 100000 entries".into(),
                ));
            }
        }
    }
    Ok(found)
}
async fn lines(path: &Path, max_bytes: u64) -> Result<Vec<Value>> {
    let file = tokio::fs::File::open(path).await?;
    if file.metadata().await?.len() > max_bytes {
        return Err(Error::Invalid(
            "native session journal exceeds the read budget".into(),
        ));
    }
    let mut bytes = vec![];
    file.take(max_bytes + 1).read_to_end(&mut bytes).await?;
    if bytes.len() as u64 > max_bytes {
        return Err(Error::Invalid(
            "native session journal exceeds the read budget".into(),
        ));
    }
    let mut parsed = vec![];
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match serde_json::from_slice(line) {
            Ok(value) => parsed.push(value),
            Err(_) if !line.ends_with(b"\n") => break,
            Err(error) => {
                return Err(Error::Invalid(format!(
                    "malformed native session journal: {error}"
                )));
            }
        }
    }
    Ok(parsed)
}
async fn header(path: &Path, harness: &str) -> Result<Option<(String, PathBuf)>> {
    let file = tokio::fs::File::open(path).await?;
    let mut reader = BufReader::new(file.take(256 * 1024));
    let mut line = String::new();
    for _ in 0..32 {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            break;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let value = if harness == "codex" && value["type"] == "session_meta" {
            &value["payload"]
        } else if (harness == "pi" && value["type"] == "session")
            || (harness == "claude" && value["isSidechain"] != true)
        {
            &value
        } else {
            continue;
        };
        let id = if harness == "claude" {
            value["sessionId"].as_str()
        } else {
            value["id"].as_str()
        };
        if let (Some(id), Some(cwd)) = (id, value["cwd"].as_str()) {
            return Ok(Some((id.into(), cwd.into())));
        }
    }
    Ok(None)
}
async fn discover_all(query: &NativeHistoryQuery) -> Result<Vec<NativeHistorySession>> {
    let expected_cwd = match &query.cwd {
        Some(cwd) => Some(tokio::fs::canonicalize(cwd).await?),
        None => None,
    };
    let mut sessions = vec![];
    for path in files(query).await? {
        let Some((id, cwd)) = header(&path, &query.harness).await? else {
            continue;
        };
        if let Some(expected) = &expected_cwd
            && tokio::fs::canonicalize(&cwd).await.ok().as_ref() != Some(expected)
        {
            continue;
        }
        let metadata = tokio::fs::metadata(&path).await?;
        let updated = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        sessions.push(NativeHistorySession {
            native: NativeSessionRef {
                provider: query.harness.clone(),
                id: if query.harness == "pi" {
                    tokio::fs::canonicalize(&path)
                        .await?
                        .to_string_lossy()
                        .into()
                } else {
                    id
                },
            },
            cwd,
            title: path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            path,
            updated_at_ms: updated,
            metadata: json!({"nativeHistory":true}),
        });
    }
    sessions.sort_by_key(|a| std::cmp::Reverse(a.updated_at_ms));
    Ok(sessions)
}
pub async fn discover(query: &NativeHistoryQuery) -> Result<Vec<NativeHistorySession>> {
    let mut sessions = discover_all(query).await?;
    sessions.truncate(query.limit);
    for session in &mut sessions {
        let file = tokio::fs::File::open(&session.path).await?;
        let mut reader = BufReader::new(file.take(256 * 1024));
        let mut line = String::new();
        for _ in 0..128 {
            line.clear();
            if reader.read_line(&mut line).await? == 0 {
                break;
            }
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let message = match query.harness.as_str() {
                "codex" if value["type"] == "response_item" => &value["payload"],
                "claude" if value["isSidechain"] != true => &value["message"],
                "pi" if value["type"] == "message" => &value["message"],
                _ => continue,
            };
            if message["role"] == "user" {
                let preview = text(&message["content"]);
                if !preview.trim().is_empty() {
                    session.title = preview
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .chars()
                        .take(100)
                        .collect();
                    break;
                }
            }
        }
    }
    Ok(sessions)
}
fn text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.into();
    }
    value
        .as_array()
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}
fn id(value: &Value, index: usize) -> String {
    value["id"]
        .as_str()
        .or_else(|| value["uuid"].as_str())
        .map(str::to_string)
        .unwrap_or_else(|| format!("imported-message-{index}"))
}
fn prompt(message: &Value, fallback: &str) -> Prompt {
    let mut content = vec![];
    if let Some(parts) = message["content"].as_array() {
        for part in parts {
            if let Some(text) = part["text"].as_str() {
                content.push(PromptContent::Text(text.into()));
            } else if let (Some(data), Some(mime)) = (
                part["source"]["data"].as_str().or(part["data"].as_str()),
                part["source"]["media_type"]
                    .as_str()
                    .or(part["mimeType"].as_str()),
            ) {
                content.push(PromptContent::Image {
                    data: data.into(),
                    mime_type: mime.into(),
                });
            } else if let Some(url) = part["image_url"]
                .as_str()
                .or(part["source"]["url"].as_str())
            {
                if let Some((mime, data)) = url
                    .strip_prefix("data:")
                    .and_then(|url| url.split_once(";base64,"))
                {
                    content.push(PromptContent::Image {
                        data: data.into(),
                        mime_type: mime.into(),
                    });
                } else {
                    content.push(PromptContent::Resource { uri: url.into() });
                }
            }
        }
    }
    if content.is_empty() {
        content.push(PromptContent::Text(fallback.into()));
    }
    Prompt { content }
}
fn observed_tools(
    provider: &str,
    entry: &Value,
    node: Option<&NativeMessageNode>,
) -> Vec<(bool, Value)> {
    let mut events = vec![];
    if provider == "codex" && entry["type"] == "response_item" {
        let call = &entry["payload"];
        let identifier = call["call_id"].as_str().or(call["id"].as_str());
        if let Some(identifier) = identifier {
            match call["type"].as_str() {
                Some("function_call" | "custom_tool_call") => {
                    let input = call
                        .get("arguments")
                        .or(call.get("input"))
                        .cloned()
                        .unwrap_or(Value::Null);
                    let input = input
                        .as_str()
                        .and_then(|text| serde_json::from_str(text).ok())
                        .unwrap_or(input);
                    events.push((
                        true,
                        json!({"id":identifier,"name":call["name"],"input":input}),
                    ));
                }
                Some("function_call_output" | "custom_tool_call_output") => {
                    events.push((false, json!({"id":identifier,"output":call["output"]})))
                }
                _ => (),
            }
        }
    } else if let Some(node) = node {
        let message = &node.metadata["message"];
        if node.role == "toolResult"
            && let Some(id) = message["toolCallId"].as_str()
        {
            events.push((false,json!({"id":id,"name":message["toolName"],"output":message["content"],"isError":message["isError"]})));
        }
        for part in message["content"].as_array().into_iter().flatten() {
            match part["type"].as_str() {
                Some("tool_use" | "toolCall") if node.role == "assistant" => {
                    if let Some(id) = part["id"].as_str() {
                        events.push((true,json!({"id":id,"name":part["name"],"input":part.get("input").or(part.get("arguments"))})));
                    }
                }
                Some("tool_result") => {
                    if let Some(id) = part["tool_use_id"].as_str() {
                        events.push((
                            false,
                            json!({"id":id,"output":part["content"],"isError":part["is_error"]}),
                        ));
                    }
                }
                _ => (),
            }
        }
    }
    events
}
pub async fn read(
    query: &NativeHistoryQuery,
    native_id: &str,
    mut capabilities: Capabilities,
) -> Result<NativeHistoryDocument> {
    // Lookup by discovered identity rather than accepting an arbitrary path supplied by a client.
    let mut session = discover_all(query)
        .await?
        .into_iter()
        .find(|session| session.native.id == native_id)
        .ok_or_else(|| {
            Error::Invalid("native history session was not found within this query".into())
        })?;
    let entries = lines(&session.path, 64 * 1024 * 1024).await?;
    if query.harness == "claude" {
        capabilities
            .metadata
            .insert("rollbackHistorySupported".into(), json!(false));
    }
    let mut tree = vec![];
    let mut ancestry = BTreeMap::new();
    let mut leaf = None;
    let mut native_turn = None::<String>;
    for (index, entry) in entries.iter().enumerate() {
        if query.harness == "codex"
            && entry["type"] == "event_msg"
            && entry["payload"]["type"] == "task_started"
        {
            native_turn = entry["payload"]["turn_id"].as_str().map(str::to_string);
        }
        let (message, parent) = match query.harness.as_str() {
            "codex"
                if entry["type"] == "response_item" && entry["payload"]["type"] == "message" =>
            {
                (
                    &entry["payload"],
                    tree.last().map(|node: &NativeMessageNode| node.id.clone()),
                )
            }
            "claude"
                if entry["isSidechain"] != true
                    && matches!(entry["type"].as_str(), Some("user" | "assistant")) =>
            {
                (
                    &entry["message"],
                    entry["parentUuid"].as_str().map(str::to_string),
                )
            }
            "pi" if entry["type"] == "message" => (
                &entry["message"],
                entry["parentId"].as_str().map(str::to_string),
            ),
            _ => {
                if query.harness == "pi"
                    && let Some(entry_id) = entry["id"].as_str()
                {
                    ancestry.insert(
                        entry_id.into(),
                        entry["parentId"].as_str().map(str::to_string),
                    );
                    leaf = Some(entry_id.into());
                } else if query.harness == "claude"
                    && entry["isSidechain"] != true
                    && let Some(entry_id) = entry["uuid"].as_str()
                {
                    ancestry.insert(
                        entry_id.into(),
                        entry["parentUuid"].as_str().map(str::to_string),
                    );
                    leaf = Some(entry_id.into());
                }
                continue;
            }
        };
        let role = message["role"].as_str().unwrap_or("unknown");
        if !matches!(role, "user" | "assistant" | "toolResult") {
            continue;
        }
        let node_id = if query.harness == "codex" {
            message["id"]
                .as_str()
                .or_else(|| entry["metadata"]["retained_source"]["id"]["message_id"].as_str())
                .map(str::to_string)
                .unwrap_or_else(|| id(message, index))
        } else {
            id(entry, index)
        };
        let native_anchor = if query.harness == "codex" {
            message["id"].is_string()
                || entry["metadata"]["retained_source"]["id"]["message_id"].is_string()
        } else {
            true
        };
        ancestry.insert(node_id.clone(), parent.clone());
        leaf = Some(node_id.clone());
        tree.push(NativeMessageNode{id:node_id,parent_id:parent,role:role.into(),text:text(&message["content"]),active:query.harness=="codex",metadata:json!({"nativeAnchor":native_anchor,"nativeTurnId":entry["metadata"]["retained_source"]["id"]["turn_id"].as_str().map(str::to_string).or_else(||native_turn.clone()),"entryIndex":index,"prompt":if role=="user" {Some(prompt(message,&text(&message["content"])))}else{None},"message":message,"raw":entry})});
    }
    if query.harness != "codex" {
        let mut active = BTreeSet::new();
        while let Some(current) = leaf {
            if !active.insert(current.clone()) {
                return Err(Error::Invalid(
                    "native message ancestry contains a cycle".into(),
                ));
            }
            leaf = ancestry.get(&current).cloned().flatten();
        }
        for node in &mut tree {
            node.active = active.contains(&node.id);
        }
    }
    let mut options = SpawnOptions::new(&query.harness, &session.cwd);
    options.execution.config_home = query.config_home.clone();
    let mut records = vec![];
    let mut turn = None::<String>;
    let mut first_user = true;
    let mut calls = BTreeMap::<String, ToolEvent>::new();
    let active_nodes: BTreeMap<usize, &NativeMessageNode> = tree
        .iter()
        .filter(|node| node.active)
        .map(|node| (node.metadata["entryIndex"].as_u64().unwrap() as usize, node))
        .collect();
    for (index, entry) in entries.iter().enumerate() {
        let node = active_nodes.get(&index).copied();
        let tool_result = node.is_some_and(|node| {
            node.role == "user"
                && node.metadata["message"]["content"]
                    .as_array()
                    .is_some_and(|parts| {
                        !parts.is_empty() && parts.iter().all(|part| part["type"] == "tool_result")
                    })
        });
        let group = node.map(|node| {
            node.metadata["nativeTurnId"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| format!("imported-turn-{}", node.id))
        });
        let new_turn = node.is_some_and(|node| node.role == "user")
            && !tool_result
            && (query.harness != "codex" || turn != group);
        if new_turn && let Some(previous) = turn.take() {
            records.push(NativeHistoryRecord {
                turn_id: Some(previous),
                kind: EventKind::TurnEnded {
                    outcome: TurnOutcome::Unknown,

                    native_reason: None,
                },
                raw: None,
            });
        }
        records.push(NativeHistoryRecord {
            turn_id: None,
            kind: EventKind::Raw,
            raw: Some(entry.clone()),
        });
        if turn.is_some() {
            for (started, value) in observed_tools(&query.harness, entry, node) {
                let status = if started {
                    "running"
                } else if value["isError"] == true {
                    "error"
                } else {
                    "completed"
                };
                let mut tool = super::tool(&value, status);
                if !started && let Some(previous) = calls.get(&tool.id) {
                    if tool.name.is_none() {
                        tool.name = previous.name.clone();
                    }
                    tool.input = previous.input.clone();
                    tool.details = previous.details.clone();
                }
                tool.metadata = json!({"nativeHistory":true});
                calls.insert(tool.id.clone(), tool.clone());
                records.push(NativeHistoryRecord {
                    turn_id: turn.clone(),
                    kind: if started {
                        EventKind::ToolStarted(tool)
                    } else {
                        EventKind::ToolCompleted(tool)
                    },
                    raw: Some(entry.clone()),
                });
            }
        }
        let Some(node) = node else { continue };
        if node.role == "user" && !tool_result {
            if new_turn {
                turn = group;
                records.push(NativeHistoryRecord {
                    turn_id: turn.clone(),
                    kind: EventKind::TurnStarted,
                    raw: None,
                });
            }
            records.push(NativeHistoryRecord {
                turn_id: turn.clone(),
                kind: EventKind::UserMessage(prompt(&node.metadata["message"], &node.text)),
                raw: Some(
                    json!({"nativeMessageId":node.id,"parentId":node.parent_id,"source":entry}),
                ),
            });
            if first_user && !node.text.trim().is_empty() {
                session.title = node.text.chars().take(100).collect();
                first_user = false;
            }
        } else if node.role == "assistant" && turn.is_some() {
            let mut message = node.metadata["message"].clone();
            message["id"] = json!(node.id);
            records.push(NativeHistoryRecord {
                turn_id: turn.clone(),
                kind: EventKind::AssistantMessage(message),
                raw: Some(entry.clone()),
            });
            records.push(NativeHistoryRecord {
                turn_id: turn.clone(),
                kind: EventKind::TextDelta {
                    text: node.text.clone(),
                    item_id: Some(node.id.clone()),
                },
                raw: None,
            });
        }
    }
    if let Some(turn) = turn {
        records.push(NativeHistoryRecord {
            turn_id: Some(turn),
            kind: EventKind::TurnEnded {
                outcome: TurnOutcome::Unknown,

                native_reason: None,
            },
            raw: None,
        });
    }
    for entry in &entries {
        if query.harness == "codex" && entry["type"] == "turn_context" {
            options.model = entry["payload"]["model"].as_str().map(str::to_string);
        }
        if query.harness == "pi" && entry["type"] == "model_change" {
            options.model = entry["modelId"].as_str().map(str::to_string);
        }
    }
    session.metadata = json!({"nativeHistory":true,"tree":query.harness!="codex","recordCount":entries.len(),"historySemantics":"observed journal messages; native compaction and turn boundaries remain provider-owned"});
    Ok(NativeHistoryDocument {
        session,
        options,
        capabilities,
        records,
        tree,
    })
}
