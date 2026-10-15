//! Local SDK checkpoint history through the pinned public Bridge read APIs.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashSet},
    path::Path,
};

const PAGE: usize = 100;
const MAX_STORES: usize = 128;
const MAX_AGENTS: usize = 10_000;
const MAX_MESSAGES: usize = 20_000;
const MAX_BYTES: usize = 64 * 1024 * 1024;

fn validate(query: &NativeHistoryQuery) -> Result<()> {
    if query.harness != "cursor" || !(1..=1000).contains(&query.limit) {
        return Err(Error::Invalid(
            "Cursor history requires a limit between 1 and 1000".into(),
        ));
    }
    Ok(())
}
fn absolute(path: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(path)
        .or_else(|_| {
            if path.is_absolute() {
                Ok(path.to_path_buf())
            } else {
                std::env::current_dir().map(|cwd| cwd.join(path))
            }
        })
        .map_err(Into::into)
}
fn matches_cwd(query: &NativeHistoryQuery, cwd: &Path) -> Result<bool> {
    Ok(match &query.cwd {
        Some(scope) => absolute(scope)? == absolute(cwd)?,
        None => true,
    })
}
fn regular_index(root: &Path) -> bool {
    std::fs::symlink_metadata(root)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        && std::fs::symlink_metadata(root.join("index.db"))
            .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
}
fn roots(query: &NativeHistoryQuery, overlay: &AgentToolExecutionOverlay) -> Result<Vec<PathBuf>> {
    if let Some(root) = query.config_home.as_ref().or(overlay.config_home.as_ref()) {
        return Ok(if regular_index(root) {
            vec![absolute(root)?]
        } else {
            vec![]
        });
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| Error::Invalid("The local SDK home directory is unavailable".into()))?;
    let projects = PathBuf::from(home).join(".cursor/projects");
    if let Some(cwd) = &query.cwd {
        let workspace = absolute(cwd)?.to_string_lossy().into_owned();
        let slug = workspace
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '-'
                }
            })
            .collect::<String>();
        let slug = slug
            .split('-')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let directory = projects.join(slug).join("sdk-agent-store");
        let expected = directory.join(format!("{:x}", Sha256::digest(workspace.as_bytes())));
        if regular_index(&expected) {
            return Ok(vec![expected]);
        }
        // Passing an existing legacy root explicitly avoids the SDK's automatic
        // md5-to-sha256 directory migration. Do not move native files ourselves.
        return child_roots(&directory);
    }
    let entries = match std::fs::read_dir(projects) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(error.into()),
    };
    let mut found = vec![];
    for (index, entry) in entries.enumerate() {
        if index > 4096 {
            return Err(Error::Invalid(
                "Cursor SDK project discovery exceeds its read budget".into(),
            ));
        }
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            found.extend(child_roots(&entry.path().join("sdk-agent-store"))?);
        }
        if found.len() > MAX_STORES {
            return Err(Error::Invalid(
                "Cursor SDK store discovery exceeds its read budget".into(),
            ));
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
}
fn child_roots(directory: &Path) -> Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(error.into()),
    };
    let mut found = vec![];
    for (index, entry) in entries.enumerate() {
        if index > MAX_STORES {
            return Err(Error::Invalid(
                "Cursor SDK store discovery exceeds its read budget".into(),
            ));
        }
        let entry = entry?;
        if regular_index(&entry.path()) {
            found.push(entry.path());
        }
    }
    found.sort();
    Ok(found)
}
fn options(
    query: &NativeHistoryQuery,
    root: &Path,
    overlay: &AgentToolExecutionOverlay,
) -> Result<SpawnOptions> {
    let mut options = SpawnOptions::new("cursor", std::env::current_dir()?);
    if let Some(cwd) = &query.cwd
        && cwd.is_dir()
    {
        options.cwd = absolute(cwd)?;
    }
    options.config = overlay.config.clone();
    options.execution.config_home = Some(root.into());
    // History reads authenticate only to the local Bridge. Suppress account-key
    // fallback, and never copy secret execution overlays into returned options.
    options
        .execution
        .env
        .insert("CURSOR_API_KEY".into(), String::new());
    Ok(options)
}
fn local_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && !value.starts_with("bc-")
        && !value.chars().any(char::is_control)
}
fn modified(value: &Value) -> u64 {
    if let Some(milliseconds) = value.as_u64() {
        return milliseconds;
    }
    value
        .as_str()
        .and_then(|text| {
            time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339).ok()
        })
        .map(|time| (time.unix_timestamp_nanos() / 1_000_000).max(0) as u64)
        .unwrap_or(0)
}
fn session(
    query: &NativeHistoryQuery,
    root: &Path,
    info: &Value,
) -> Result<Option<NativeHistorySession>> {
    let id = info["agentId"].as_str().filter(|id| local_id(id));
    let cwd = info["local"]["cwd"].as_str();
    let (Some(id), Some(cwd)) = (id, cwd) else {
        return Ok(None);
    };
    if info.get("cloud").is_some() || !matches_cwd(query, Path::new(cwd))? {
        return Ok(None);
    }
    let title = info["name"]
        .as_str()
        .filter(|name| !name.trim().is_empty())
        .or_else(|| {
            info["summary"]
                .as_str()
                .filter(|name| !name.trim().is_empty())
        })
        .unwrap_or(id)
        .chars()
        .take(200)
        .collect();
    Ok(Some(NativeHistorySession {
        native: NativeSessionRef {
            provider: "cursor".into(),
            id: id.into(),
        },
        cwd: PathBuf::from(cwd),
        title,
        path: root.join("index.db"),
        updated_at_ms: modified(&info["lastModified"]),
        metadata: json!({"nativeHistory":true,"transport":"cursor_sdk_bridge","sdkVersion":bridge::SDK_VERSION,"runtime":"local","resumable":true,"archived":info["archived"] == true,"status":info["status"],"historySource":"sdk_checkpoint_store"}),
    }))
}
async fn discover_root(
    query: &NativeHistoryQuery,
    root: &Path,
    overlay: &AgentToolExecutionOverlay,
) -> Result<Vec<NativeHistorySession>> {
    let bridge = Bridge::spawn(&options(query, root, overlay)?).await?;
    let operation = async {
        let mut found = vec![];
        let mut cursor = String::new();
        let mut cursors = HashSet::new();
        let mut identities = HashSet::new();
        let mut scanned = 0;
        loop {
            let mut opts = json!({"runtime":"RUNTIME_LOCAL","limit":PAGE,"cursor":cursor});
            if let Some(cwd) = &query.cwd {
                opts["cwd"] = json!(absolute(cwd)?);
            }
            let page = bridge
                .call("SdkAgentService", "ListAgents", json!({"options":opts}))
                .await?;
            let entries = page["items"].as_array().ok_or_else(|| {
                Error::Provider("Cursor SDK returned an invalid history page".into())
            })?;
            scanned += entries.len();
            if scanned > MAX_AGENTS {
                return Err(Error::Invalid(
                    "Cursor SDK agent history exceeds its read budget".into(),
                ));
            }
            for info in entries {
                if let Some(entry) = session(query, root, info)?
                    && identities.insert(entry.native.id.clone())
                {
                    found.push(entry);
                }
            }
            let next = page["nextCursor"].as_str().unwrap_or_default();
            if next.is_empty() {
                break;
            }
            if !cursors.insert(next.to_string()) || cursors.len() > 100 {
                return Err(Error::Provider(
                    "Cursor SDK history pagination did not advance".into(),
                ));
            }
            cursor = next.into();
        }
        Ok(found)
    }
    .await;
    let _ = bridge.shutdown().await;
    operation
}
pub(super) async fn discover(
    query: &NativeHistoryQuery,
    overlay: &AgentToolExecutionOverlay,
) -> Result<Vec<NativeHistorySession>> {
    validate(query)?;
    let mut found = vec![];
    let mut identities = BTreeSet::new();
    for root in roots(query, overlay)? {
        for entry in discover_root(query, &root, overlay).await? {
            if identities.insert(entry.native.id.clone()) {
                found.push(entry);
            }
            if found.len() > MAX_AGENTS {
                return Err(Error::Invalid(
                    "Cursor SDK agent history exceeds its read budget".into(),
                ));
            }
        }
    }
    found.sort_by(|a, b| {
        b.updated_at_ms
            .cmp(&a.updated_at_ms)
            .then_with(|| a.native.id.cmp(&b.native.id))
    });
    found.truncate(query.limit);
    Ok(found)
}
pub(super) async fn read(
    query: &NativeHistoryQuery,
    id: &str,
    overlay: &AgentToolExecutionOverlay,
) -> Result<NativeHistoryDocument> {
    validate(query)?;
    if !local_id(id) {
        return Err(Error::Invalid(
            "Cursor history requires a local SDK agent ID".into(),
        ));
    }
    // A requested identity must first belong to this local SDK store and cwd
    // query; a CLI UUID or a cloud ID never becomes an SDK history conversion.
    let mut discovery = query.clone();
    discovery.limit = 1000;
    let mut selected = None;
    for root in roots(query, overlay)? {
        if let Some(found) = discover_root(&discovery, &root, overlay)
            .await?
            .into_iter()
            .find(|entry| entry.native.id == id)
        {
            selected = Some((root, found));
            break;
        }
    }
    let (root, selected) = selected.ok_or_else(|| {
        Error::Invalid("Cursor SDK history was not found within this query".into())
    })?;
    let bridge = Bridge::spawn(&options(query, &root, overlay)?).await?;
    let operation=async {
        let info=bridge.call("SdkAgentService","GetAgent",json!({"agentId":id,"options":{"cwd":selected.cwd}})).await?;
        let verified=session(query,&root,&info["agent"] )?.filter(|entry|entry.native.id==id && entry.cwd==selected.cwd).ok_or_else(||Error::Invalid("Cursor SDK history identity or workspace changed during discovery".into()))?;
        let mut messages=vec![];let mut bytes=0;let mut message_ids=HashSet::new();
        loop {
            let page=bridge.call("SdkAgentService","ListAgentMessages",json!({"agentId":id,"options":{"runtime":"RUNTIME_LOCAL","cwd":verified.cwd,"limit":PAGE,"offset":messages.len()}})).await?;
            let entries=page["messages"].as_array().ok_or_else(||Error::Provider("Cursor SDK returned invalid history messages".into()))?;
            if entries.len()>PAGE {return Err(Error::Provider("Cursor SDK history exceeded its requested page size".into()));}
            bytes+=serde_json::to_vec(entries)?.len();if bytes>MAX_BYTES || messages.len()+entries.len()>MAX_MESSAGES {return Err(Error::Invalid("Cursor SDK conversation exceeds its read budget".into()));}
            for message in entries {
                if message["agentId"].as_str().or_else(||message["agent_id"].as_str()) != Some(id) {return Err(Error::Invalid("Cursor SDK returned history for a different agent".into()));}
                let uuid=message["uuid"].as_str().filter(|id|!id.is_empty()).ok_or_else(||Error::Provider("Cursor SDK returned a history message without an identity".into()))?;
                if !message_ids.insert(uuid.to_string()) {return Err(Error::Provider("Cursor SDK history message pagination did not advance".into()));}
                messages.push(message.clone());
            }
            if entries.len()<PAGE {break;}
        }
        document(verified,&root,messages)
    }.await;
    let _ = bridge.shutdown().await;
    operation
}
fn text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.into();
    }
    if let Some(text) = value["text"].as_str() {
        return text.into();
    }
    if let Some(parts) = value.as_array() {
        return parts.iter().map(text).collect::<Vec<_>>().join("");
    }
    if let Some(content) = value.get("content") {
        return text(content);
    }
    String::new()
}
fn tagged<'a>(value: &'a Value, tag: &str) -> Option<&'a Value> {
    value.get(tag).or_else(|| {
        if value["case"] == tag {
            value.get("value")
        } else {
            None
        }
    })
}
fn conversation(value: &Value) -> Option<(&str, &Value)> {
    for tag in ["agentConversationTurn", "shellConversationTurn"] {
        if let Some(turn) =
            tagged(value, tag).or_else(|| value.get("turn").and_then(|turn| tagged(turn, tag)))
        {
            return Some((tag, turn));
        }
        if value["type"] == tag {
            return value.get("turn").map(|turn| (tag, turn));
        }
    }
    None
}
fn step(value: &Value) -> Option<(&str, &Value)> {
    for tag in ["assistantMessage", "thinkingMessage", "toolCall"] {
        if let Some(message) = tagged(value, tag).or_else(|| {
            value
                .get("message")
                .and_then(|message| tagged(message, tag))
        }) {
            return Some((tag, message));
        }
        if value["type"] == tag {
            return value.get("message").map(|message| (tag, message));
        }
    }
    None
}
fn tool_value(message: &Value, fallback_id: &str) -> (Value, bool) {
    let native = message
        .get("tool")
        .and_then(|tool| {
            tool["case"]
                .as_str()
                .and_then(|case| tool.get("value").map(|value| (case, value)))
        })
        .or_else(|| {
            message.as_object().and_then(|fields| {
                fields
                    .iter()
                    .find(|(name, _)| name.ends_with("ToolCall"))
                    .map(|(name, value)| (name.as_str(), value))
            })
        });
    let (name, call) = native
        .map(|(name, call)| (Some(name.trim_end_matches("ToolCall")), call))
        .unwrap_or((
            message["type"]
                .as_str()
                .or_else(|| message["name"].as_str()),
            message,
        ));
    let id = message["toolCallId"]
        .as_str()
        .or_else(|| message["tool_call_id"].as_str())
        .or_else(|| message["id"].as_str())
        .or_else(|| message["callId"].as_str())
        .unwrap_or(fallback_id);
    let result = &call["result"];
    let native_result = result
        .get("result")
        .and_then(|union| {
            union["case"]
                .as_str()
                .and_then(|case| union.get("value").map(|value| (case, value)))
        })
        .or_else(|| {
            result.as_object().and_then(|fields| {
                fields
                    .iter()
                    .find(|(name, _)| {
                        matches!(
                            name.as_str(),
                            "success" | "error" | "permissionDenied" | "declined"
                        )
                    })
                    .map(|(name, value)| (name.as_str(), value))
            })
        });
    let output = native_result
        .map(|(status, value)| json!({"status":status,"value":value}))
        .unwrap_or_else(|| result.clone());
    let complete = native_result.is_some() || result["status"].is_string();
    let failure = matches!(
        output["status"].as_str(),
        Some("error" | "permissionDenied" | "declined")
    );
    (
        json!({"id":id,"name":name,"args":call["args"],"result":output,"isError":failure}),
        complete,
    )
}
fn document(
    mut session: NativeHistorySession,
    root: &Path,
    messages: Vec<Value>,
) -> Result<NativeHistoryDocument> {
    let mut options = SpawnOptions::new("cursor", session.cwd.clone());
    options.execution.config_home = Some(root.into());
    let mut records = vec![];
    let mut tree = vec![];
    let mut previous = None;
    let mut ids = HashSet::new();
    for entry in &messages {
        let uuid = entry["uuid"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                Error::Provider("Cursor SDK returned a history message without an identity".into())
            })?;
        if !ids.insert(uuid.to_string()) {
            return Err(Error::Provider(
                "Cursor SDK history message pagination did not advance".into(),
            ));
        }
        let turn_id = Some(format!("cursor-checkpoint-{uuid}"));
        records.push(NativeHistoryRecord {
            turn_id: None,
            kind: EventKind::Raw,
            raw: Some(entry.clone()),
        });
        records.push(NativeHistoryRecord {
            turn_id: turn_id.clone(),
            kind: EventKind::TurnStarted,
            raw: None,
        });
        let mut append_node = |role: &str, suffix: &str, text: String, metadata: Value| {
            let id = format!("{uuid}:{suffix}");
            tree.push(NativeMessageNode {
                id: id.clone(),
                parent_id: previous.take(),
                role: role.into(),
                text,
                active: true,
                metadata,
            });
            previous = Some(id.clone());
            id
        };
        if let Some((kind, turn)) = conversation(&entry["message"]) {
            if kind == "shellConversationTurn" {
                let value = json!({"id":format!("{uuid}:shell"),"name":"shell","args":turn["shellCommand"],"result":turn["shellOutput"],"isError":turn["shellOutput"]["exitCode"].as_i64().is_some_and(|value|value!=0)});
                let mut event = crate::tool(&value, "completed");
                event.metadata = json!({"nativeHistory":true,"nativeMessageId":uuid});
                records.push(NativeHistoryRecord {
                    turn_id: turn_id.clone(),
                    kind: EventKind::ToolCompleted(event),
                    raw: Some(entry.clone()),
                });
                append_node(
                    "tool",
                    "shell",
                    text(&turn["shellOutput"]["stdout"]),
                    json!({"nativeAnchor":false,"shell":turn}),
                );
            } else {
                if let Some(user) = turn.get("userMessage").or_else(|| turn.get("user_message")) {
                    let message = text(user);
                    let id = append_node(
                        "user",
                        "user",
                        message.clone(),
                        json!({"nativeAnchor":false,"nativeMessageId":uuid,"message":user}),
                    );
                    records.push(NativeHistoryRecord {
                        turn_id: turn_id.clone(),
                        kind: EventKind::UserMessage(Prompt::from(message.as_str())),
                        raw: Some(json!({"nativeMessageId":id,"source":entry})),
                    });
                }
                for (step_index, value) in
                    turn["steps"].as_array().into_iter().flatten().enumerate()
                {
                    match step(value) {
                        Some(("assistantMessage", message)) => {
                            let content = text(message);
                            let id = append_node(
                                "assistant",
                                &format!("assistant-{step_index}"),
                                content.clone(),
                                json!({"nativeAnchor":false,"nativeMessageId":uuid,"step":value}),
                            );
                            records.push(NativeHistoryRecord{turn_id:turn_id.clone(),kind:EventKind::AssistantMessage(json!({"id":id,"role":"assistant","content":[{"type":"text","text":content}]})),raw:Some(value.clone())});
                            records.push(NativeHistoryRecord {
                                turn_id: turn_id.clone(),
                                kind: EventKind::TextDelta {
                                    text: content,
                                    item_id: Some(id.clone()),
                                },
                                raw: None,
                            });
                            records.push(NativeHistoryRecord {
                                turn_id: turn_id.clone(),
                                kind: EventKind::MessageEnded { item_id: Some(id) },
                                raw: None,
                            });
                        }
                        Some(("thinkingMessage", message)) => records.push(NativeHistoryRecord {
                            turn_id: turn_id.clone(),
                            kind: EventKind::ReasoningDelta {
                                text: text(message),
                            },
                            raw: Some(value.clone()),
                        }),
                        Some(("toolCall", message)) => {
                            let (normalized, complete) =
                                tool_value(message, &format!("{uuid}:tool-{step_index}"));
                            let mut event = crate::tool(
                                &normalized,
                                if complete { "completed" } else { "unknown" },
                            );
                            event.metadata =
                                json!({"nativeHistory":true,"nativeMessageId":uuid,"step":value});
                            records.push(NativeHistoryRecord {
                                turn_id: turn_id.clone(),
                                kind: if complete {
                                    EventKind::ToolCompleted(event)
                                } else {
                                    EventKind::ToolStarted(event)
                                },
                                raw: Some(value.clone()),
                            });
                            append_node(
                                "tool",
                                &format!("tool-{step_index}"),
                                message["type"].as_str().unwrap_or("Tool call").into(),
                                json!({"nativeAnchor":false,"step":value}),
                            );
                        }
                        _ => (),
                    }
                }
            }
        } else {
            // Keep unsupported native checkpoint shapes inspectable without
            // guessing a tool schema or inventing execution outcomes.
            let content = text(&entry["message"]);
            let role = entry["type"].as_str().unwrap_or("unknown");
            let id = append_node(
                role,
                "message",
                content.clone(),
                json!({"nativeAnchor":false,"nativeMessageId":uuid,"message":entry["message"]}),
            );
            if role == "user" {
                records.push(NativeHistoryRecord {
                    turn_id: turn_id.clone(),
                    kind: EventKind::UserMessage(Prompt::from(content.as_str())),
                    raw: Some(entry.clone()),
                });
            } else if role == "assistant" {
                records.push(NativeHistoryRecord {
                    turn_id: turn_id.clone(),
                    kind: EventKind::TextDelta {
                        text: content,
                        item_id: Some(id),
                    },
                    raw: Some(entry.clone()),
                });
            }
        }
        records.push(NativeHistoryRecord {
            turn_id: turn_id.clone(),
            kind: EventKind::TurnEnded {
                outcome: TurnOutcome::Unknown,
                native_reason: Some("imported_sdk_checkpoint".into()),
            },
            raw: None,
        });
    }
    session.metadata["recordCount"] = json!(messages.len());
    session.metadata["historySemantics"] = json!(
        "Latest local SDK checkpoint turns; run boundaries and outcomes are not inferred, and CLI/editor databases are not imported"
    );
    Ok(NativeHistoryDocument {
        session,
        options,
        capabilities: capabilities(),
        records,
        tree,
    })
}
