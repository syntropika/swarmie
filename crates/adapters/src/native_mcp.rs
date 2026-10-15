use super::*;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
#[path = "native_mcp_settings.rs"]
mod settings;
#[path = "native_mcp_sources.rs"]
mod sources;
use settings::{credentials, definition, env_ref, native_settings, strings};
pub(super) fn applies(harness: &str, query: &McpQuery, entry: &McpEntry) -> Result<bool> {
    sources::applies(harness, query, entry)
}
use sources::{Source, id, sources};
#[path = "native_mcp_files.rs"]
mod files;

fn nested<'a>(value: &'a Value, keys: &[String]) -> &'a Value {
    keys.iter().fold(value, |value, key| &value[key])
}
fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
fn parse(harness: &str, text: &str) -> Result<Value> {
    if harness == "codex" {
        files::toml(text)
    } else {
        files::json(text)
    }
}
fn claude_legacy(query: &McpQuery, source: &Source, name: &str) -> Result<(String, bool)> {
    if source.scope != "project" {
        return Ok((String::new(), false));
    }
    let Some(cwd) = &query.cwd else {
        return Ok((String::new(), false));
    };
    let legacy = cwd.join(".claude/settings.local.json");
    if legacy == sources::claude_settings(query)? {
        return Ok((String::new(), false));
    }
    let text = files::read(&legacy)?;
    let value = files::json(&text)?;
    let denied = strings(&value["disabledMcpjsonServers"])
        .iter()
        .any(|item| item == name);
    Ok((text, denied))
}

fn claude_policy_text(source: &Source, text: &str, legacy: &str) -> String {
    if source.scope == "project" {
        format!("{text}\nlegacy:{legacy}")
    } else {
        text.into()
    }
}

fn restrict_legacy_policy(entry: &mut McpEntry, denied: bool) {
    if denied {
        entry.enabled = false;
        entry.actions.retain(|action| action != "enable");
        entry.read_only_reason = Some("Legacy directory-local Claude settings also disable this server; remove that denial in the native settings before enabling it here".into());
    }
}

fn claude_disabled(query: &McpQuery, source: &Source, name: &str) -> Result<(bool, String)> {
    if query.cwd.is_none() {
        return Ok((false, String::new()));
    }
    let (path, keys, field) = claude_policy(query, source)?;
    let text = files::read(&path)?;
    let value = files::json(&text)?;
    let (legacy, legacy_denied) = claude_legacy(query, source, name)?;
    let disabled = legacy_denied
        || strings(&nested(&value, &keys)[field])
            .iter()
            .any(|n| n == name);
    Ok((disabled, claude_policy_text(source, &text, &legacy)))
}
fn claude_policy(
    query: &McpQuery,
    source: &Source,
) -> Result<(PathBuf, Vec<String>, &'static str)> {
    let cwd = query
        .cwd
        .as_ref()
        .ok_or_else(|| Error::Invalid("Choose a directory for Claude MCP enablement".into()))?;
    if source.scope == "project" {
        Ok((
            sources::claude_settings(query)?,
            vec![],
            "disabledMcpjsonServers",
        ))
    } else {
        let user = sources("claude", query)?.remove(0).path;
        Ok((
            user,
            vec![
                "projects".into(),
                sources::claude_project(cwd).to_string_lossy().into_owned(),
            ],
            "disabledMcpServers",
        ))
    }
}
pub(super) fn registration_conflicts(
    harness: &str,
    query: &McpQuery,
    names: &[String],
) -> Result<()> {
    for source in sources(harness, query)? {
        let text = files::read(&source.path)?;
        if text.is_empty() {
            continue;
        }
        let data = parse(harness, &text)?;
        if names
            .iter()
            .any(|name| !nested(&data, &source.keys)[name].is_null())
        {
            return Err(Error::Rejected("Native configuration overrides an injected MCP name; remove the conflicting native definition before opening this session".into()));
        }
    }
    Ok(())
}
fn configured_entry(
    harness: &str,
    query: &McpQuery,
    source: &Source,
    name: &str,
    value: &Value,
    revision: String,
    enabled: bool,
) -> McpEntry {
    let mut actions: Vec<String> = ["update", "uninstall"].map(String::from).into();
    if harness != "claude" || query.cwd.is_some() {
        actions.extend(["enable".into(), "disable".into()]);
    }
    let read_only = !cfg!(unix)
        || std::fs::metadata(&source.path)
            .map_or(true, |metadata| metadata.permissions().readonly());
    if read_only {
        actions.clear();
    }
    McpEntry {
        id: id(harness, source, name),
        revision,
        name: name.into(),
        native_name: name.into(),
        origin: "native".into(),
        harness: Some(harness.into()),
        scope: source.scope.into(),
        source: Some(source.path.clone()),
        project_path: source.project.clone(),
        enabled,
        definition: definition(harness, value),
        actions,
        credential_fields: credentials(harness, value),
        read_only_reason: read_only.then(|| {
            if cfg!(unix) {
                "Native configuration is read-only".into()
            } else {
                "Native file writes are not qualified on this platform".into()
            }
        }),
        runtime_status: "unknown".into(),
        auth_status: "unknown".into(),
        tools: vec![],
        requires_restart: false,
    }
}

pub(super) async fn catalog(harness: &str, query: &McpQuery) -> Result<Vec<McpEntry>> {
    let mut entries = vec![];
    for source in sources(harness, query)? {
        let text = files::read(&source.path)?;
        if text.is_empty() {
            continue;
        }
        let data = parse(harness, &text)?;
        for (name, value) in nested(&data, &source.keys)
            .as_object()
            .into_iter()
            .flatten()
        {
            if name == "swarmie_host" || name.starts_with("swarmie_managed_") {
                continue;
            }
            let (disabled, policy) = if harness == "claude" {
                claude_disabled(query, &source, name)?
            } else {
                (value["enabled"] == false, String::new())
            };
            let mut entry = configured_entry(
                harness,
                query,
                &source,
                name,
                value,
                digest(&format!("{text}\n{policy}")),
                !disabled,
            );
            if harness == "claude" {
                restrict_legacy_policy(&mut entry, claude_legacy(query, &source, name)?.1);
            }
            entries.push(entry);
        }
    }
    Ok(entries)
}
pub(super) async fn mutate(harness: &str, mutation: &McpMutation) -> Result<Option<McpEntry>> {
    let entries = catalog(harness, &mutation.query).await?;
    let selected = mutation
        .id
        .as_ref()
        .map(|id| {
            entries
                .iter()
                .find(|e| &e.id == id)
                .ok_or_else(|| Error::Rejected("Native MCP entry no longer exists".into()))
        })
        .transpose()?;
    if let Some(entry) = selected {
        if mutation.expected_revision.as_deref() != Some(&entry.revision) {
            return Err(Error::Rejected(
                "Native MCP configuration changed; refresh before trying again".into(),
            ));
        }
        if !entry.actions.contains(&mutation.action) {
            return Err(Error::Unsupported(
                "Native MCP entry cannot be changed".into(),
            ));
        }
    }
    let name = selected
        .map(|e| e.name.as_str())
        .or(mutation.name.as_deref())
        .unwrap_or_default();
    if selected.is_none() {
        validate_mcp_name(name)?;
    }
    let candidates = sources(harness, &mutation.query)?;
    let source = if let Some(entry) = selected {
        candidates
            .iter()
            .find(|source| id(harness, source, name) == entry.id)
            .cloned()
            .ok_or_else(|| Error::Rejected("Native source changed".into()))?
    } else {
        let scope = mutation.scope.as_deref().unwrap_or("user");
        if !matches!(scope, "user" | "project") {
            return Err(Error::Invalid("Choose user or project scope".into()));
        }
        candidates
            .iter()
            .find(|s| s.scope == scope)
            .cloned()
            .ok_or_else(|| Error::Invalid("Choose a working directory for project scope".into()))?
    };
    let text = files::read(&source.path)?;
    let data = parse(harness, &text)?;
    let (policy_disabled, policy) = if harness == "claude" {
        claude_disabled(&mutation.query, &source, name)?
    } else {
        (false, String::new())
    };
    if let Some(entry) = selected
        && digest(&format!("{text}\n{policy}")) != entry.revision
    {
        return Err(Error::Rejected(
            "Native MCP configuration changed; refresh before trying again".into(),
        ));
    }
    let existing = &nested(&data, &source.keys)[name];
    let settings = match mutation.action.as_str() {
        "install" => {
            if !existing.is_null() {
                return Err(Error::Rejected(
                    "MCP name already exists in this native source".into(),
                ));
            }
            Some(native_settings(
                harness,
                mutation
                    .definition
                    .as_ref()
                    .ok_or_else(|| Error::Invalid("MCP settings are required".into()))?,
            )?)
        }
        "update" => {
            let definition = mutation
                .definition
                .as_ref()
                .ok_or_else(|| Error::Invalid("MCP settings are required".into()))?;
            if definition.transport != definition_from_entry(selected)?.transport {
                return Err(Error::Invalid(
                    "Uninstall before changing native transport type".into(),
                ));
            }
            let mut replacement = native_settings(harness, definition)?;
            for field in ["env", "environment", "headers", "http_headers"] {
                if let Some(old) = existing[field].as_object() {
                    let mut combined: serde_json::Map<String, Value> = old
                        .iter()
                        .filter(|(key, value)| {
                            if matches!(field, "env" | "environment") {
                                return value.as_str().and_then(env_ref).as_deref()
                                    != Some(key.as_str());
                            }
                            value.as_str().and_then(env_ref).is_none()
                                && value
                                    .as_str()
                                    .and_then(|v| v.strip_prefix("Bearer "))
                                    .and_then(env_ref)
                                    .is_none()
                        })
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect();
                    if let Some(new) = replacement[field].as_object() {
                        combined.extend(new.clone());
                    }
                    replacement[field] = json!(combined);
                }
            }
            if selected
                .is_some_and(|entry| entry.credential_fields.iter().any(|field| field == "url"))
            {
                if definition.url != definition_from_entry(selected)?.url {
                    return Err(Error::Invalid(
                        "Remove native URL credentials before changing the endpoint".into(),
                    ));
                }
                replacement["url"] = existing["url"].clone();
            }
            if selected
                .is_some_and(|entry| entry.credential_fields.iter().any(|field| field == "args"))
            {
                if !definition.args.is_empty() {
                    return Err(Error::Invalid(
                        "Edit native process arguments in their source file".into(),
                    ));
                }
                if harness == "opencode" {
                    let mut command = vec![json!(definition.command)];
                    command.extend(
                        existing["command"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .skip(1)
                            .cloned(),
                    );
                    replacement["command"] = json!(command);
                } else {
                    replacement["args"] = existing["args"].clone();
                }
            }
            Some(replacement)
        }
        "uninstall" => None,
        "enable" | "disable" if harness == "claude" => {
            let (path, keys, field) = claude_policy(&mutation.query, &source)?;
            let original = files::read(&path)?;
            let (legacy, legacy_denied) = claude_legacy(&mutation.query, &source, name)?;
            if mutation.action == "enable" && legacy_denied {
                return Err(Error::Rejected(
                    "Legacy directory-local Claude settings still disable this server".into(),
                ));
            }
            let value = files::json(&original)?;
            let mut disabled = strings(&nested(&value, &keys)[field]);
            disabled.retain(|n| n != name);
            if mutation.action == "disable" {
                disabled.push(name.into());
            }
            let updated = files::edit_json(&original, &keys, field, Some(&json!(disabled)), false)?;
            files::json(&updated)?;
            let mut entry = configured_entry(
                harness,
                &mutation.query,
                &source,
                name,
                existing,
                digest(&format!(
                    "{}\n{}",
                    if path == source.path { &updated } else { &text },
                    claude_policy_text(&source, &updated, &legacy)
                )),
                mutation.action == "enable",
            );
            restrict_legacy_policy(&mut entry, legacy_denied);
            if files::read(&source.path)? != text
                || claude_legacy(&mutation.query, &source, name)?.0 != legacy
            {
                return Err(Error::Rejected(
                    "Native configuration changed; refresh before trying again".into(),
                ));
            }
            files::write(&path, &original, &updated)?;
            return Ok(Some(entry));
        }
        "enable" | "disable" => Some(json!({"enabled":mutation.action == "enable"})),
        _ => return Err(Error::Invalid("Unknown native MCP operation".into())),
    };
    let updated = if harness == "codex" {
        files::edit_toml(&text, name, settings.as_ref(), mutation.action != "install")?
    } else {
        files::edit_json(
            &text,
            &source.keys,
            name,
            settings.as_ref(),
            mutation.action != "install",
        )?
    };
    let rendered = parse(harness, &updated)?;
    let value = &nested(&rendered, &source.keys)[name];
    let same_policy_source = harness == "claude"
        && mutation.query.cwd.is_some()
        && claude_policy(&mutation.query, &source)?.0 == source.path;
    let revision = digest(&format!(
        "{updated}\n{}",
        if same_policy_source {
            &updated
        } else {
            &policy
        }
    ));
    let enabled = if harness == "claude" {
        !policy_disabled
    } else {
        value["enabled"] != false
    };
    files::write(&source.path, &text, &updated)?;
    Ok(settings.as_ref().map(|_| {
        configured_entry(
            harness,
            &mutation.query,
            &source,
            name,
            value,
            revision,
            enabled,
        )
    }))
}
fn definition_from_entry(entry: Option<&McpEntry>) -> Result<&McpDefinition> {
    entry
        .map(|e| &e.definition)
        .ok_or_else(|| Error::Invalid("Choose an existing MCP server".into()))
}
