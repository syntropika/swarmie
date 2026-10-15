use super::*;
use serde::{Deserialize, Serialize};

const STATE: &str = "swarmie.mcp";
const PREFIX: &str = "swarmie_managed_";

#[derive(Default, Serialize, Deserialize)]
struct Registry {
    revision: u64,
    entries: Vec<McpEntry>,
}

impl Runtime {
    async fn mcp_registry(&self) -> Result<Registry> {
        let value = match self.0.store.extension_state(STATE).await {
            Ok(value) => value,
            Err(Error::Unsupported(_)) => None,
            Err(error) => return Err(error),
        };
        value
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| Error::Storage("Invalid MCP registry".into()))
            .map(Option::unwrap_or_default)
    }
    async fn mcp_context(&self, mut query: McpQuery) -> Result<McpQuery> {
        if let Some(id) = &query.session_id {
            let snapshot = self.snapshot(id).await?;
            if query
                .harness
                .as_ref()
                .is_some_and(|h| *h != snapshot.options.harness)
                || query
                    .cwd
                    .as_ref()
                    .is_some_and(|p| *p != snapshot.options.cwd)
                || query.config_home.as_ref().is_some_and(|path| {
                    snapshot.options.execution.config_home.as_ref() != Some(path)
                })
            {
                return Err(Error::Invalid(
                    "MCP query belongs to a different native session".into(),
                ));
            }
            if let Some(harness) = self.0.harnesses.get(&snapshot.options.harness) {
                query.externally_managed = harness
                    .mcp_capabilities(&snapshot.options)
                    .await?
                    .metadata
                    .get("mcpConfigurationScope")
                    .is_some_and(|scope| scope == "external");
            }
            query.harness = Some(snapshot.options.harness);
            query.cwd = Some(snapshot.options.cwd);
            query.config_home = snapshot.options.execution.config_home;
            if let Some(agent) = self.agent(id).await {
                query.owner_home = agent.execution_home();
                query.owner_env = agent.mcp_environment();
            } else {
                query.owner_home = None;
                query.owner_env.clear();
            }
        }
        if query
            .harness
            .as_ref()
            .is_some_and(|harness| !self.0.harnesses.contains_key(harness))
        {
            return Err(Error::Invalid("Unknown MCP harness".into()));
        }
        if query
            .config_home
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        {
            return Err(Error::Invalid(
                "Native configuration home must be absolute".into(),
            ));
        }
        if let Some(path) = &query.cwd {
            if !path.is_absolute() || !path.is_dir() {
                return Err(Error::Invalid(
                    "Select a working directory on this execution node".into(),
                ));
            }
            query.cwd = Some(std::fs::canonicalize(path)?);
        }
        Ok(query)
    }
    pub async fn mcp_catalog(&self, query: McpQuery) -> Result<McpCatalog> {
        let query = self.mcp_context(query).await?;
        let mut result = McpCatalog::default();
        let registry = self.mcp_registry().await?;
        let agents: Vec<_> = self
            .0
            .agents
            .read()
            .await
            .values()
            .filter_map(std::sync::Weak::upgrade)
            .map(Agent)
            .filter(|agent| !agent.0.closed.load(std::sync::atomic::Ordering::SeqCst))
            .collect();
        for agent in agents {
            if agent
                .0
                .mcp_restart
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                let snapshot = agent.snapshot().await;
                if query
                    .harness
                    .as_ref()
                    .is_none_or(|h| *h == snapshot.options.harness)
                {
                    result.restart_sessions.push(snapshot.id);
                }
            }
        }
        result.entries = registry
            .entries
            .into_iter()
            .filter(|entry| {
                query
                    .harness
                    .as_ref()
                    .is_none_or(|h| entry.harness.as_ref().is_none_or(|e| e == h))
                    && query.cwd.as_ref().is_none_or(|cwd| {
                        entry
                            .project_path
                            .as_ref()
                            .is_none_or(|p| cwd.starts_with(p))
                    })
            })
            .collect();
        if query.externally_managed {
            for entry in &mut result.entries {
                entry.runtime_status = "unsupported".into();
            }
        }
        let live = if let Some(id) = &query.session_id {
            self.agent(id).await
        } else {
            None
        };
        let native_context_available =
            !query.externally_managed && (query.session_id.is_none() || live.is_some());
        if query.externally_managed {
            result.limitations.push("This harness server owns its MCP configuration; node-local definitions are not applied to this session".into());
        } else if !native_context_available {
            result.limitations.push(
                "Open this session to discover its native MCP configuration and connections".into(),
            );
        }
        for (id, harness) in &self.0.harnesses {
            if !native_context_available {
                continue;
            }
            if query.harness.as_ref().is_some_and(|h| h != id) {
                continue;
            }
            match harness.mcp_catalog(&query).await {
                Ok(entries) => result.entries.extend(entries),
                Err(Error::Unsupported(_)) => { if query.harness.is_some() { result.limitations.push(format!("{id}: native MCP management is not supported")); } }
                Err(_) => result.limitations.push(format!("{id}: native configuration could not be read; fix its permissions or format and refresh")),
            }
        }
        if let Some(id) = &query.session_id
            && let Some(agent) = live
        {
            match agent
                .mcp_control(serde_json::json!({"action":"inspect"}))
                .await
            {
                Ok(rows) => {
                    let mut names = std::collections::BTreeMap::new();
                    for entry in &result.entries {
                        *names.entry(entry.native_name.clone()).or_insert(0) += 1;
                    }
                    for row in rows.as_array().into_iter().flatten() {
                        let Some(name) = row["name"].as_str() else {
                            continue;
                        };
                        if query.externally_managed
                            || names.get(name).is_none_or(|count| *count > 1)
                                && name != "swarmie_host"
                                && !name.starts_with(PREFIX)
                        {
                            result.entries.push(McpEntry {
                                    id: format!("native-live:{id}:{name}"), revision: "live".into(), name: name.into(), native_name: name.into(),
                                    origin: "native".into(), harness: query.harness.clone(), scope: "session".into(), source: None, project_path: query.cwd.clone(),
                                    enabled: true, definition: McpDefinition { transport: "unknown".into(), ..Default::default() }, actions: vec![], credential_fields: vec![],
                                    read_only_reason: Some("Live native server; this adapter does not expose its configuration management".into()),
                                    runtime_status: "unknown".into(), auth_status: "unknown".into(), tools: vec![], requires_restart: false,
                                });
                            names.entry(name.into()).or_insert(1);
                        }
                    }
                    for entry in &mut result.entries {
                        if query.externally_managed && entry.origin == "swarmie" {
                            continue;
                        }
                        if result.restart_sessions.contains(id) {
                            entry.requires_restart = true;
                        }
                        if names[&entry.native_name] > 1 && entry.scope != "session" {
                            continue;
                        }
                        if let Some(row) = rows
                            .as_array()
                            .into_iter()
                            .flatten()
                            .find(|r| r["name"].as_str() == Some(&entry.native_name))
                        {
                            entry.runtime_status =
                                row["runtimeStatus"].as_str().unwrap_or("unknown").into();
                            entry.auth_status =
                                row["authStatus"].as_str().unwrap_or("unknown").into();
                            entry.tools = row["tools"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect();
                            if row["canAuthenticate"] == true
                                && row["runtimeStatus"] != "disabled"
                                && entry.enabled
                            {
                                entry.actions.push("authenticate".into());
                            }
                        }
                    }
                }
                Err(_) => result.limitations.push(
                    "Live MCP tools and authentication status are unavailable for this session"
                        .into(),
                ),
            }
        }
        Ok(result)
    }
    pub async fn mcp_mutate(&self, mut mutation: McpMutation) -> Result<McpReceipt> {
        let _open = self.0.open_lock.lock().await;
        let _lock = self.0.mcp_lock.lock().await;
        if mutation
            .origin
            .as_ref()
            .is_some_and(|origin| !matches!(origin.as_str(), "swarmie" | "native"))
        {
            return Err(Error::Invalid("Unknown MCP configuration owner".into()));
        }
        mutation.query = self.mcp_context(mutation.query).await?;
        if let Some(id) = &mutation.query.harness
            && !self.0.harnesses.contains_key(id)
        {
            return Err(Error::Invalid("Unknown MCP harness".into()));
        }
        let mut registry = self.mcp_registry().await?;
        let selected = if let Some(id) = &mutation.id {
            let catalog = self.mcp_catalog(mutation.query.clone()).await?;
            let entry = catalog
                .entries
                .into_iter()
                .find(|e| &e.id == id)
                .ok_or_else(|| {
                    Error::Rejected("MCP entry no longer exists; refresh the catalog".into())
                })?;
            if mutation.expected_revision.as_deref() != Some(&entry.revision) {
                return Err(Error::Rejected(
                    "MCP settings changed; refresh before trying again".into(),
                ));
            }
            if !entry.actions.contains(&mutation.action) {
                return Err(Error::Unsupported(
                    entry
                        .read_only_reason
                        .clone()
                        .unwrap_or_else(|| "This MCP operation is unavailable".into()),
                ));
            }
            Some(entry)
        } else {
            None
        };
        if mutation.action == "authenticate" {
            let entry = selected.ok_or_else(|| Error::Invalid("Choose an MCP server".into()))?;
            let id = mutation.query.session_id.as_deref().ok_or_else(|| {
                Error::Invalid("Open a native session before authentication".into())
            })?;
            let agent = self.agent(id).await.ok_or(Error::Closed)?;
            let receipt = agent
                .mcp_control(serde_json::json!({"action":"authenticate","name":entry.native_name}))
                .await?;
            let authorization_url = receipt["authorizationUrl"].as_str().ok_or_else(|| {
                Error::Provider("Native authentication did not return a browser URL".into())
            })?;
            let url = url::Url::parse(authorization_url).map_err(|_| {
                Error::Provider("Native authentication returned an invalid URL".into())
            })?;
            if !matches!(url.scheme(), "https" | "http")
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(Error::Provider(
                    "Native authentication returned an invalid URL".into(),
                ));
            }
            return Ok(McpReceipt {
                action: mutation.action,
                entry: Some(entry),
                restart_sessions: vec![],
                authorization_url: Some(authorization_url.into()),
            });
        }
        let native = selected.as_ref().is_some_and(|e| e.origin == "native")
            || mutation.origin.as_deref() == Some("native");
        if native && mutation.query.externally_managed {
            return Err(Error::Unsupported(
                "Native configuration writes on external harness servers are not supported".into(),
            ));
        }
        if native
            && let Some(id) = &mutation.query.session_id
            && self.agent(id).await.is_none()
        {
            return Err(Error::Invalid(
                "Open this session before managing its native MCP configuration".into(),
            ));
        }
        let changed;
        let result = if native {
            let id = selected
                .as_ref()
                .and_then(|e| e.harness.as_ref())
                .or(mutation.query.harness.as_ref())
                .ok_or_else(|| {
                    Error::Invalid("Choose a harness for native MCP management".into())
                })?;
            let harness = self
                .0
                .harnesses
                .get(id)
                .ok_or_else(|| Error::Invalid("Unknown MCP harness".into()))?;
            let result = harness.mcp_mutate(&mutation).await?;
            changed = selected.clone().or_else(|| result.clone());
            result
        } else {
            registry.revision += 1;
            let entry = match mutation.action.as_str() {
                "install" => {
                    if selected.is_some() {
                        return Err(Error::Invalid("This server is already installed".into()));
                    }
                    let name = mutation.name.as_deref().unwrap_or_default();
                    validate_mcp_name(name)?;
                    let definition = mutation
                        .definition
                        .clone()
                        .ok_or_else(|| Error::Invalid("MCP settings are required".into()))?;
                    definition.validate()?;
                    let scope = mutation.scope.as_deref().unwrap_or("user");
                    if !matches!(scope, "user" | "project") {
                        return Err(Error::Invalid("Choose user or project scope".into()));
                    }
                    let project =
                        if scope == "project" {
                            Some(mutation.query.cwd.clone().ok_or_else(|| {
                                Error::Invalid("Choose a project directory".into())
                            })?)
                        } else {
                            None
                        };
                    if registry.entries.len() >= 500
                        || registry.entries.iter().any(|e| {
                            e.name == name
                                && e.project_path == project
                                && e.harness == mutation.query.harness
                        })
                    {
                        return Err(Error::Invalid(
                            "MCP name is already used in this scope or the registry is full".into(),
                        ));
                    }
                    let id = uuid::Uuid::new_v4().simple().to_string();
                    let entry = McpEntry {
                        id: format!("swarmie:{id}"),
                        revision: registry.revision.to_string(),
                        name: name.into(),
                        native_name: format!("{PREFIX}{id}"),
                        origin: "swarmie".into(),
                        harness: mutation.query.harness.clone(),
                        scope: scope.into(),
                        source: None,
                        project_path: project,
                        enabled: true,
                        definition,
                        actions: vec![
                            "update".into(),
                            "enable".into(),
                            "disable".into(),
                            "uninstall".into(),
                        ],
                        credential_fields: vec![],
                        read_only_reason: None,
                        runtime_status: "unknown".into(),
                        auth_status: "unknown".into(),
                        tools: vec![],
                        requires_restart: false,
                    };
                    registry.entries.push(entry.clone());
                    Some(entry)
                }
                "enable" | "disable" | "update" | "uninstall" => {
                    let entry = selected
                        .as_ref()
                        .ok_or_else(|| Error::Invalid("Choose an MCP server".into()))?;
                    let index = registry
                        .entries
                        .iter()
                        .position(|e| e.id == entry.id)
                        .ok_or_else(|| Error::Rejected("MCP registry changed".into()))?;
                    if mutation.action == "uninstall" {
                        registry.entries.remove(index);
                        None
                    } else {
                        let entry = &mut registry.entries[index];
                        if mutation.action == "update" {
                            let definition = mutation.definition.clone().ok_or_else(|| {
                                Error::Invalid("MCP settings are required".into())
                            })?;
                            definition.validate()?;
                            entry.definition = definition;
                        } else {
                            entry.enabled = mutation.action == "enable";
                        }
                        entry.revision = registry.revision.to_string();
                        Some(entry.clone())
                    }
                }
                _ => return Err(Error::Invalid("Unknown MCP operation".into())),
            };
            changed = selected.or_else(|| entry.clone());
            self.0
                .store
                .save_extension_state(STATE, serde_json::to_value(registry)?)
                .await?;
            entry
        };
        let mut restart_sessions = vec![];
        let agents: Vec<_> = self
            .0
            .agents
            .read()
            .await
            .values()
            .filter_map(std::sync::Weak::upgrade)
            .map(Agent)
            .filter(|agent| !agent.0.closed.load(std::sync::atomic::Ordering::SeqCst))
            .collect();
        for agent in agents {
            let snapshot = agent.snapshot().await;
            let mut affected = changed.as_ref().is_none_or(|e| {
                e.harness
                    .as_ref()
                    .is_none_or(|h| h == &snapshot.options.harness)
                    && e.project_path
                        .as_ref()
                        .is_none_or(|p| snapshot.options.cwd.starts_with(p))
            });
            if let Some(entry) = changed.as_ref().filter(|entry| entry.origin == "native") {
                let query = McpQuery {
                    harness: Some(snapshot.options.harness.clone()),
                    cwd: Some(snapshot.options.cwd.clone()),
                    config_home: snapshot.options.execution.config_home.clone(),
                    owner_home: agent.execution_home(),
                    owner_env: agent.mcp_environment(),
                    ..Default::default()
                };
                affected = snapshot
                    .capabilities
                    .metadata
                    .get("mcpConfigurationScope")
                    .is_none_or(|scope| scope != "external")
                    && self
                        .0
                        .harnesses
                        .get(&snapshot.options.harness)
                        .is_some_and(|harness| {
                            harness.mcp_source_applies(&query, entry).unwrap_or(true)
                        });
            }
            let supported = changed.as_ref().is_none_or(|entry| {
                entry.origin == "native"
                    || snapshot
                        .capabilities
                        .metadata
                        .get("mcpTransports")
                        .and_then(Value::as_array)
                        .is_some_and(|values| {
                            values
                                .iter()
                                .any(|v| v.as_str() == Some(&entry.definition.transport))
                        })
            });
            // Only require reopening when the native session can actually be resumed.
            if affected
                && supported
                && snapshot.options.execution.tools != ToolPolicy::Disabled
                && snapshot.capabilities.resume.is_available()
            {
                agent
                    .0
                    .mcp_restart
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                restart_sessions.push(snapshot.id);
            }
        }
        Ok(McpReceipt {
            action: mutation.action,
            entry: result,
            restart_sessions,
            authorization_url: None,
        })
    }
    /// Reconnect an idle native session with current MCP definitions and its live credentials.
    pub async fn mcp_reopen(&self, id: &str) -> Result<Agent> {
        let agent = self.agent(id).await.ok_or(Error::Closed)?;
        let snapshot = agent.snapshot().await;
        if !snapshot.capabilities.resume.is_available() {
            return Err(Error::Unsupported(
                "This native session cannot be reopened".into(),
            ));
        }
        let execution = agent
            .0
            .execution
            .lock()
            .unwrap()
            .clone()
            .ok_or(Error::Closed)?;
        agent.close_idle().await?;
        self.resume_with(id, execution).await
    }
    pub(super) async fn compile_mcp(&self, options: &mut SpawnOptions) -> Result<()> {
        let registry = self.mcp_registry().await?;
        options
            .execution
            .mcp_servers
            .retain(|server| !server.name.starts_with(PREFIX));
        if options.execution.tools == ToolPolicy::Disabled || registry.entries.is_empty() {
            return Ok(());
        }
        let harness = self
            .0
            .harnesses
            .get(&options.harness)
            .ok_or_else(|| Error::Invalid("Unknown harness".into()))?;
        let capabilities = harness.mcp_capabilities(options).await?;
        if capabilities
            .metadata
            .get("mcpConfigurationScope")
            .is_some_and(|scope| scope == "external")
        {
            return Ok(());
        }
        let transports = capabilities
            .metadata
            .get("mcpTransports")
            .cloned()
            .unwrap_or_default();
        for entry in registry.entries.into_iter().filter(|e| {
            e.enabled
                && e.harness.as_ref().is_none_or(|h| h == &options.harness)
                && e.project_path
                    .as_ref()
                    .is_none_or(|p| options.cwd.starts_with(p))
        }) {
            if !transports.as_array().is_some_and(|v| {
                v.iter()
                    .any(|v| v.as_str() == Some(&entry.definition.transport))
            }) {
                if entry.harness.as_ref() == Some(&options.harness) {
                    return Err(Error::Unsupported(format!(
                        "{} does not support this MCP transport",
                        options.harness
                    )));
                }
                continue;
            }
            options.execution.mcp_servers.push(
                entry
                    .definition
                    .resolve(entry.native_name, &options.execution)?,
            );
        }
        Ok(())
    }
}
