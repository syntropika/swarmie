use super::*;
use std::collections::BTreeMap;

/// Use Pi's session registration API; native Pi owns transport, discovery and teardown.
pub(super) async fn prepare(
    options: &SpawnOptions,
    args: &mut Vec<String>,
    env: &mut BTreeMap<String, String>,
) -> Result<Option<tempfile::TempDir>> {
    if options.execution.tools == ToolPolicy::Disabled {
        return Ok(None);
    }
    let query = McpQuery {
        harness: Some("pi".into()),
        cwd: Some(options.cwd.clone()),
        config_home: options.execution.config_home.clone(),
        owner_home: options
            .execution
            .env
            .get("HOME")
            .or_else(|| options.execution.env.get("USERPROFILE"))
            .map(PathBuf::from),
        owner_env: options
            .execution
            .env
            .iter()
            .filter(|(key, _)| key.as_str() == "PI_CODING_AGENT_DIR")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        ..Default::default()
    };
    let native = super::native_mcp::catalog("pi", &query).await?;
    let requested_names: Vec<_> = options
        .execution
        .mcp_servers
        .iter()
        .map(|server| server.name.clone())
        .collect();
    super::native_mcp::registration_conflicts("pi", &query, &requested_names)?;
    if native.is_empty() && options.execution.mcp_servers.is_empty() {
        return Ok(None);
    }
    if args.iter().any(|arg| arg == "--no-extensions") {
        return Err(Error::Invalid(
            "Pi MCP injection requires extensions".into(),
        ));
    }
    let directory = tempfile::Builder::new()
        .prefix("swarmie-pi-mcp-")
        .tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let mut servers = vec![];
    let mut names: Vec<_> = native
        .iter()
        .map(|entry| json!({"name":entry.native_name,"enabled":entry.enabled}))
        .collect();
    let mut counter = 0;
    let mut reference = |value: &str| {
        counter += 1;
        let key = format!(
            "SWARMIE_PI_MCP_{}_{}",
            uuid::Uuid::new_v4().simple(),
            counter
        );
        env.insert(key.clone(), value.into());
        format!("${{{key}}}")
    };
    for server in &options.execution.mcp_servers {
        let config = match &server.transport {
            McpTransport::Stdio { command, args, env } => {
                let environment: BTreeMap<_, _> = env
                    .iter()
                    .map(|(key, value)| (key.clone(), reference(value)))
                    .collect();
                json!({"command":command,"args":args,"env":environment,"exposure":"direct"})
            }
            McpTransport::Http { url, headers } => {
                let headers: BTreeMap<_, _> = headers
                    .iter()
                    .map(|(key, value)| (key.clone(), reference(value)))
                    .collect();
                json!({"url":url,"headers":headers,"exposure":"direct"})
            }
            McpTransport::Sse { .. } => {
                return Err(Error::Unsupported("Pi legacy SSE transport".into()));
            }
        };
        names.push(json!({"name":server.name,"enabled":true}));
        servers.push(json!({"name":server.name,"config":config}));
    }
    let manifest = directory.path().join("servers.json");
    std::fs::write(
        &manifest,
        serde_json::to_vec(&json!({"servers":servers,"names":names}))?,
    )?;
    let extension = directory.path().join("extension.mjs");
    std::fs::write(&extension, include_str!("pi-mcp.mjs"))?;
    env.insert(
        "SWARMIE_PI_MCP_STATUS".into(),
        directory
            .path()
            .join("status.json")
            .to_string_lossy()
            .into_owned(),
    );
    env.insert(
        "SWARMIE_PI_MCP_MANIFEST".into(),
        manifest.to_string_lossy().into_owned(),
    );
    args.extend([
        "--extension".into(),
        extension.to_string_lossy().into_owned(),
    ]);
    Ok(Some(directory))
}

pub(super) fn inspect(directory: &Option<tempfile::TempDir>) -> Result<Value> {
    let Some(directory) = directory else {
        return Ok(json!([]));
    };
    let path = directory.path().join("status.json");
    if !path.is_file() {
        return Err(Error::Unsupported(
            "Pi MCP registration was not acknowledged by this version".into(),
        ));
    }
    if std::fs::metadata(&path)?.len() > 4 * 1024 * 1024 {
        return Err(Error::Provider(
            "Pi MCP inventory exceeds the supported size".into(),
        ));
    }
    serde_json::from_str(&std::fs::read_to_string(path)?)
        .map_err(|_| Error::Provider("Pi MCP inventory is unavailable".into()))
}
