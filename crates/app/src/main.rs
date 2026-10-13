mod client;
mod runtime;
use client::{
    client_host, client_monitor, client_operations, client_projection, client_submissions, desktop, node_channel,
    node_connection, node_credentials, node_gateway, open, remote,
};
use runtime::{mcp_bridge, runtime_host, service, transport};
mod update;
use clap::{Parser, Subcommand, ValueEnum};
use futures_util::StreamExt;
use protocol::RuntimeService;
use serde_json::Value;
use std::{collections::VecDeque, path::PathBuf, time::Duration};
use storage::SqliteSessionStore;
use swarmie::*;
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Parser)]
#[command(
    name = "swarmie",
    version,
    about = "Headless multi-harness agent runtime"
)]
struct Cli {
    /// Application data root. Defaults to ~/.swarmie on the runtime node.
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    /// Inspect the client-owned node registry; retained observations do not imply live connectivity.
    Node {
        #[command(subcommand)]
        command: NodeCommand,
    },
    /// Attach a desktop client to its persistent Environment runtime, starting it if needed.
    Desktop {
        #[arg(long)]
        assets: PathBuf,
        #[arg(long)]
        host: PathBuf,
    },
    /// Start, inspect, stop, or proxy a connection to this Environment's runtime.
    Runtime {
        #[command(subcommand)]
        command: runtime_host::RuntimeCommand,
    },
    /// Check a configured HTTPS release manifest or stage a verified update archive.
    Update {
        #[command(subcommand)]
        command: update::UpdateCommand,
    },
    /// Discover, inspect, or import sessions created by native CLIs.
    History {
        harness: String,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        config_home: Option<PathBuf>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
        #[arg(long)]
        native_id: Option<String>,
        #[arg(long, requires = "native_id")]
        import: bool,
    },
    /// Pair and connect to local or remote authenticated daemons.
    Remote {
        #[command(subcommand)]
        command: remote::RemoteCommand,
    },
    /// Manage the user daemon service.
    Service {
        #[command(subcommand)]
        command: service::ServiceCommand,
    },
    /// Open the GUI with an optional project or runtime session.
    Open {
        #[arg(long, default_value = "http://127.0.0.1:7480/")]
        url: String,
        #[arg(long)]
        project: Option<PathBuf>,
        #[arg(long)]
        session: Option<String>,
        /// Native desktop executable for a loopback HTTP URL; packaged installations detect it automatically.
        #[arg(long)]
        host: Option<PathBuf>,
        #[arg(long)]
        print: bool,
    },
    /// Expose opt-in extension commands as model-callable MCP tools over stdio.
    Tools {
        url: String,
        #[arg(long, default_value = "resources")]
        extension: String,
        #[arg(long, env = "SWARMIE_TOKEN")]
        token: Option<String>,
        #[arg(long, env = "SWARMIE_TOOL_CHANNEL")]
        host_channel: Option<String>,
    },
    /// List harness installations without launching them.
    Discover,
    /// Check installed version and provider authentication without starting a conversation.
    Probe { harness: String },
    /// Read native account quotas without opening a conversation.
    Quotas {
        harness: String,
        #[arg(default_value = ".")]
        cwd: PathBuf,
        #[arg(long)]
        config_home: Option<PathBuf>,
    },
    /// List durable session references.
    Sessions,
    /// Run a session. Omit --prompt for an interactive conversation.
    Run {
        harness: String,
        #[arg(default_value = ".")]
        cwd: PathBuf,
        #[arg(short, long)]
        prompt: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        resume: Option<String>,
        #[arg(long)]
        fork: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, value_enum, default_value = "ask")]
        permissions: Permissions,
        /// Opaque provider configuration, encoded as a JSON object.
        #[arg(long, default_value = "{}")]
        config: String,
        /// ACP executable for a custom harness name.
        #[arg(long)]
        executable: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        agent_arg: Vec<String>,
        #[arg(long, default_value_t = 600)]
        timeout: u64,
    },
    /// Serve JSON-RPC over stdio (default) or WebSocket.
    Serve {
        #[arg(long, conflicts_with = "http")]
        websocket: Option<std::net::SocketAddr>,
        /// Serve JSON-RPC POST requests at /rpc and persistent WebSocket channels at /ws.
        #[arg(long, conflicts_with = "websocket")]
        http: Option<std::net::SocketAddr>,
        /// Serve the desktop frontend and allow its own HTTP origin. Loopback only.
        #[arg(long, requires = "http")]
        desktop_assets: Option<PathBuf>,
        /// Explicit authenticated frontend origin: HTTPS proxy or loopback HTTP SSH tunnel.
        #[arg(long, requires = "desktop_assets")]
        public_origin: Option<String>,
        /// Enable the Git extension; created worktrees live in this directory.
        #[arg(long)]
        git_worktrees: Option<PathBuf>,
        /// Enable optional folder, file, terminal and project-action commands.
        #[arg(long)]
        workbench: bool,
        /// Enable optional resource and orchestration commands in this directory.
        #[arg(long)]
        resources_root: Option<PathBuf>,
        /// Register an in-process scripted test harness from a JSON file.
        #[arg(long)]
        mock_script: Option<PathBuf>,
        /// Optional policy for surfaced permission requests: ask, accept-edits, auto-approve, deny.
        #[arg(long)]
        permission_policy: Option<swarmie_permissions::PolicyMode>,
        /// Bearer token required by WebSocket clients. Mandatory for non-loopback listeners.
        #[arg(long, env = "SWARMIE_TOKEN")]
        token: Option<String>,
        /// Read a bearer token from a private file.
        #[arg(long, conflicts_with = "token")]
        token_file: Option<PathBuf>,
        /// Register a generic ACP harness: --acp name=executable. Pass launch arguments with --acp-arg.
        #[arg(long)]
        acp: Vec<String>,
        #[arg(long, allow_hyphen_values = true)]
        acp_arg: Vec<String>,
    },
    /// Send one JSON-RPC request to a WebSocket daemon.
    Call {
        url: String,
        method: String,
        #[arg(long, default_value = "{}")]
        params: String,
        #[arg(long, env = "SWARMIE_TOKEN")]
        token: Option<String>,
    },
}
#[derive(Subcommand)]
enum NodeCommand {
    List,
}
#[derive(Clone, Copy, ValueEnum)]
enum Permissions {
    Ask,
    Approve,
    Deny,
}
fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("swarmie: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(entry());
    // Entry drains accepted work and native cleanup. Tokio's stdin reader can still be
    // blocked in an OS read after explicit shutdown; it must not require client EOF to exit.
    runtime.shutdown_timeout(Duration::from_secs(1));
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("swarmie: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
async fn entry() -> Result<()> {
    let cli = Cli::parse();
    if let Commands::Tools {
        url,
        extension,
        token,
        host_channel,
    } = cli.command
    {
        return mcp_bridge::serve(url, extension, token, host_channel).await;
    }
    let root = match cli.state_dir {
        Some(root) => root,
        None => storage::default_data_root()?,
    };
    match cli.command {
        Commands::Node {
            command: NodeCommand::List,
        } => {
            let manager = node_connection::ConnectionManager::open(&root).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&manager.catalog().await?)?
            );
            return Ok(());
        }
        Commands::Desktop { assets, host } => return desktop::run(&root, assets, host).await,
        Commands::Runtime { command } => return runtime_host::run(&root, command).await,
        Commands::Update { command } => return update::run(&root, command).await,
        Commands::Remote { command } => return remote::run(&root, command).await,
        Commands::Service { command } => return service::run(&root, command).await,
        Commands::Open {
            url,
            project,
            session,
            host,
            print,
        } => return open::run(url, project, session, host, print).await,
        Commands::Tools { .. } => unreachable!(),
        Commands::Call {
            url,
            method,
            params,
            token,
        } => return transport::call(&url, method, serde_json::from_str(&params)?, token).await,
        command => {
            let store = SqliteSessionStore::new(&root).await?;
            // Native writers share one state owner; OS locks are released on process exit.
            let _owner = if matches!(
                &command,
                Commands::Serve { .. }
                    | Commands::Run { .. }
                    | Commands::History { import: true, .. }
            ) {
                let mut options = std::fs::OpenOptions::new();
                options.read(true).write(true).create(true).truncate(false);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let file = options.open(root.join("runtime/runtime-owner.lock"))?;
                fs2::FileExt::try_lock_exclusive(&file).map_err(|_| Error::Storage("another runtime owns this state directory; connect to that runtime or choose a separate state directory".into()))?;
                Some(file)
            } else {
                None
            };
            let mut builder = adapters::builtin_builder().store(store);
            match &command {
                Commands::Serve {
                    acp,
                    acp_arg,
                    git_worktrees,
                    desktop_assets,
                    workbench,
                    resources_root,
                    mock_script,
                    permission_policy,
                    ..
                } => {
                    if let Some(path) = resources_root {
                        builder =
                            builder.extension(swarmie_resources::ResourcesExtension::open(path)?);
                    }
                    if let Some(mode) = permission_policy {
                        builder = builder
                            .extension(swarmie_permissions::PermissionPolicyExtension::new(*mode));
                    }
                    if let Some(path) = mock_script {
                        use tokio::io::AsyncReadExt;
                        let mut bytes = Vec::new();
                        tokio::fs::File::open(path)
                            .await?
                            .take(1024 * 1024 + 1)
                            .read_to_end(&mut bytes)
                            .await?;
                        if bytes.len() > 1024 * 1024 {
                            return Err(Error::Invalid("mock script exceeds 1 MiB".into()));
                        }
                        let script: swarmie::mock::MockScript = serde_json::from_slice(&bytes)?;
                        builder = builder.harness(swarmie::mock::MockHarness::new(script));
                    }
                    if let Some(path) = git_worktrees {
                        builder = builder.extension(swarmie_git::GitExtension::new(path).await?);
                    }
                    if *workbench || desktop_assets.is_some() {
                        builder = builder.extension(
                            swarmie_workspace::WorkspaceExtension::new()
                                .with_storage_root(root.join("desktop")),
                        );
                    }
                    for spec in acp {
                        let (id, exe) = spec.split_once('=').ok_or_else(|| {
                            Error::Invalid("--acp requires name=executable".into())
                        })?;
                        builder =
                            builder.harness(adapters::AcpHarness::new(id, exe, acp_arg.clone()));
                    }
                }
                Commands::Run {
                    harness,
                    executable: Some(exe),
                    agent_arg,
                    ..
                } => {
                    builder =
                        builder.harness(adapters::AcpHarness::new(harness, exe, agent_arg.clone()));
                }
                _ => (),
            }
            let runtime = builder.build();
            match command {
                Commands::History {
                    harness,
                    cwd,
                    config_home,
                    limit,
                    native_id,
                    import,
                } => {
                    let query = NativeHistoryQuery {
                        harness,
                        cwd,
                        config_home,
                        limit,
                    };
                    let result = match native_id {
                        Some(id) if import => {
                            serde_json::to_value(runtime.import_native_history(query, &id).await?)?
                        }
                        Some(id) => {
                            serde_json::to_value(runtime.read_native_history(query, &id).await?)?
                        }
                        None => serde_json::to_value(runtime.native_history(query).await?)?,
                    };
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                Commands::Discover => println!(
                    "{}",
                    serde_json::to_string_pretty(&runtime.discover().await?)?
                ),
                Commands::Sessions => println!(
                    "{}",
                    serde_json::to_string_pretty(&runtime.sessions().await?)?
                ),
                Commands::Probe { harness } => println!(
                    "{}",
                    serde_json::to_string_pretty(&runtime.probe(&harness).await?)?
                ),
                Commands::Quotas {
                    harness,
                    cwd,
                    config_home,
                } => {
                    let mut query = AccountQuery::new(harness, std::fs::canonicalize(cwd)?);
                    query.execution.config_home = config_home;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&runtime.account_quotas(query).await?)?
                    );
                }
                Commands::Serve {
                    websocket,
                    http,
                    desktop_assets,
                    mut token,
                    token_file,
                    public_origin,
                    ..
                } => {
                    if let Some(path) = token_file {
                        let value = tokio::fs::read_to_string(path).await?;
                        let value = value.trim();
                        if value.is_empty() || value.chars().any(char::is_control) {
                            return Err(Error::Invalid("invalid bearer token file".into()));
                        }
                        token = Some(value.into());
                    }
                    let service = RuntimeService::new(runtime);
                    let result = if let Some(addr) = websocket {
                        transport::websocket(service.clone(), addr, token).await
                    } else if let Some(addr) = http {
                        transport::http(
                            service.clone(),
                            addr,
                            token,
                            desktop_assets,
                            public_origin,
                            Some(root.clone()),
                        )
                        .await
                    } else {
                        transport::stdio(service.clone()).await
                    };
                    let cleanup = service.shutdown().await;
                    result?;
                    cleanup?;
                }
                Commands::Run {
                    harness,
                    cwd,
                    prompt,
                    model,
                    resume,
                    fork,
                    json,
                    permissions,
                    config,
                    timeout,
                    ..
                } => {
                    let config: Value = serde_json::from_str(&config)?;
                    let agent = if let Some(id) = resume {
                        runtime.resume(&id).await?
                    } else if let Some(id) = fork {
                        runtime.fork(&id).await?
                    } else {
                        runtime
                            .spawn(SpawnOptions {
                                project_id: None,
                                project_revision: None,
                                execution: Default::default(),
                                harness,
                                cwd,
                                model,
                                config,
                            })
                            .await?
                    };
                    let result = run(&agent, prompt, json, permissions, timeout).await;
                    let cleanup = agent.close().await;
                    result?;
                    cleanup?;
                }
                Commands::Call { .. }
                | Commands::Node { .. }
                | Commands::Desktop { .. }
                | Commands::Update { .. }
                | Commands::Runtime { .. }
                | Commands::Remote { .. }
                | Commands::Service { .. }
                | Commands::Open { .. }
                | Commands::Tools { .. } => unreachable!(),
            }
        }
    }
    Ok(())
}
async fn run(
    agent: &Agent,
    initial: Option<String>,
    json_output: bool,
    policy: Permissions,
    timeout: u64,
) -> Result<()> {
    let interactive = initial.is_none();
    let mut events = agent.events();
    let id = agent.snapshot().await.id;
    if !json_output {
        eprintln!("Session {id}. Commands: /steer TEXT, /cancel, /quit, /answer OPTION [JSON].");
    }
    let mut stdin = BufReader::new(tokio::io::stdin()).lines();
    let mut pending: VecDeque<PermissionRequest> = VecDeque::new();
    if let Some(p) = initial {
        agent.prompt(p.as_str()).await?;
    }
    let deadline = tokio::time::sleep(Duration::from_secs(timeout));
    tokio::pin!(deadline);
    let mut eof = false;
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            event = events.next() => {
                let Some(event) = event else { return Err(Error::Closed); }; let event = event?;
                if json_output { println!("{}",serde_json::to_string(&event)?); }
                match &event.kind {
                    EventKind::TextDelta { text,.. } if !json_output => { use std::io::Write; print!("{text}"); std::io::stdout().flush()?; }
                    EventKind::PermissionRequested(p) => {
                        let selected = match policy {
                            Permissions::Approve => p.options.iter().find(|o| matches!(o.kind.as_str(),"approve"|"approve_once")),
                            Permissions::Deny => p.options.iter().find(|o| matches!(o.kind.as_str(),"deny"|"deny_once"|"deny_session"|"cancel")),
                            Permissions::Ask if !interactive => p.options.iter().find(|o| matches!(o.kind.as_str(),"deny"|"deny_once"|"deny_session"|"cancel")),
                            _ => None,
                        };
                        if let Some(option) = selected { agent.respond_permission(PermissionResponse { answers: Default::default(), id:p.id.clone(),option:option.id.clone(),data:Value::Null }).await?; }
                        else if interactive {
                            eprintln!("\n{}: {}",p.title,p.options.iter().map(|o| format!("{} ({})",o.id,o.label)).collect::<Vec<_>>().join(", ")); pending.push_back(p.clone());
                        } else { return Err(Error::Invalid("permission needs an explicit answer; use an interactive session or the daemon".into())); }
                    }
                    EventKind::PermissionResolved { id,.. } => pending.retain(|p| &p.id != id),
                    EventKind::Error { message } => { eprintln!("\n{message}"); }
                    EventKind::TurnEnded { outcome, .. } => {
                        if !json_output { println!(); eprintln!("Turn ended: {outcome}"); }
                        let failed = !matches!(outcome, TurnOutcome::Completed | TurnOutcome::Handled);
                        if !interactive || eof { return if failed { Err(Error::Provider("turn failed".into())) } else { Ok(()) }; }
                    }
                    EventKind::StateChanged(SessionState::Failed | SessionState::Closed) => return Err(Error::Closed),
                    _ => (),
                }
            }
            line = stdin.next_line(), if interactive && !eof => {
                let Some(line) = line? else { eof = true; if agent.snapshot().await.turn.is_none() { return Ok(()); } continue; };
                let result = if line == "/quit" { return Ok(()); }
                else if line == "/cancel" { agent.cancel().await }
                else if let Some(text) = line.strip_prefix("/steer ") { agent.steer(text).await }
                else if let Some(request) = pending.front() {
                    let answer = line.strip_prefix("/answer ").unwrap_or(&line);
                    let (option,data) = answer.split_once(' ').unwrap_or((answer,"null"));
                    match serde_json::from_str(data) { Ok(data) => agent.respond_permission(PermissionResponse { answers: Default::default(), id:request.id.clone(),option:option.into(),data }).await,Err(e) => Err(e.into()) }
                } else if line.trim().is_empty() { Ok(()) } else { agent.prompt(line.as_str()).await };
                if let Err(e) = result { eprintln!("{e}"); }
            }
            _ = &mut shutdown => { let _ = agent.cancel().await; return Err(Error::Invalid("interrupted".into())); }
            _ = &mut deadline, if !interactive => { let _ = agent.cancel().await; return Err(Error::Timeout("turn completion".into())); }
        }
    }
}
