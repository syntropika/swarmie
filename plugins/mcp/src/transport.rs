use crate::{Configuration, Endpoint, unavailable};
use rmcp::{
    ClientServiceExt, RoleClient,
    model::ClientConfig,
    service::RunningService,
    transport::{
        StreamableHttpClientTransport, common::client_side_sse::NeverRetry,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use std::{process::Stdio, sync::Arc, time::Duration};
use tembra::Cancellation;
use tokio::process::Child;
use tokio::time::Instant;

/// The declared Tembra resource owns both protocol work and the child handle.
/// Keep the process handle outside the SDK handshake so failed negotiation can
/// kill and reap the process before its opener reports physical completion.
pub(crate) struct Session {
    pub running: RunningService<RoleClient, ClientConfig>,
    child: Option<Child>,
}

impl Session {
    pub async fn close(&mut self) -> tembra::Result<()> {
        let protocol = self
            .running
            .close()
            .await
            .map_err(|_| unavailable("MCP protocol cleanup failed"));
        let process = if let Some(child) = &mut self.child {
            match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
                Ok(result) => result
                    .map(|_| ())
                    .map_err(|_| unavailable("MCP process could not be reaped")),
                Err(_) => stop(child).await,
            }
        } else {
            Ok(())
        };
        protocol.and(process)
    }
}

async fn stop(child: &mut Child) -> tembra::Result<()> {
    let stopped = child.start_kill();
    let reaped = child.wait().await;
    stopped
        .and(reaped.map(|_| ()))
        .map_err(|_| unavailable("MCP process could not be stopped and reaped"))
}

pub(crate) async fn open(
    config: &Configuration,
    signal: &Cancellation,
    deadline: Instant,
) -> tembra::Result<Session> {
    signal.check()?;
    let mut client = ClientConfig::default();
    client.protocol_version = config.protocol.version();
    client.client_info.name = "swarmie".into();
    client.client_info.version = env!("CARGO_PKG_VERSION").into();
    match &config.endpoint {
        Endpoint::Local {
            program,
            arguments,
            directory,
        } => {
            let mut command = tokio::process::Command::new(program);
            command
                .args(arguments)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            if let Some(directory) = directory {
                command.current_dir(directory);
            }
            let mut child = command
                .spawn()
                .map_err(|_| unavailable("MCP process could not be started"))?;
            let handles = child.stdout.take().zip(child.stdin.take());
            let Some(transport) = handles else {
                let _cleanup = stop(&mut child).await;
                return Err(unavailable("MCP process pipes are unavailable"));
            };
            let handshake = tokio::time::timeout_at(
                deadline,
                client.serve_with_lifecycle(transport, config.protocol.lifecycle()),
            );
            let negotiated = tokio::select! {
                biased;
                () = signal.cancelled() => Err(crate::error::cancelled()),
                response = handshake => response
                    .map_err(|_| unavailable("MCP local connection deadline exceeded"))
                    .and_then(|result| result.map_err(|_| unavailable("MCP local negotiation failed"))),
            };
            match negotiated {
                Ok(running) => Ok(Session {
                    running,
                    child: Some(child),
                }),
                Err(error) => {
                    let _cleanup = stop(&mut child).await;
                    Err(error)
                }
            }
        }
        Endpoint::Http { url, headers } => {
            let http = reqwest::Client::builder()
                .default_headers(crate::configuration::validate_headers(headers)?)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| unavailable("MCP HTTP client could not be created"))?;
            let mut options = StreamableHttpClientTransportConfig::with_uri(url.clone())
                .reinit_on_expired_session(false);
            options.retry_config = Arc::new(NeverRetry::default());
            let transport = StreamableHttpClientTransport::with_client(http, options);
            let handshake = tokio::time::timeout_at(
                deadline,
                client.serve_with_lifecycle(transport, config.protocol.lifecycle()),
            );
            let running = tokio::select! {
                biased;
                () = signal.cancelled() => return Err(crate::error::cancelled()),
                response = handshake => response
                    .map_err(|_| unavailable("MCP HTTP connection deadline exceeded"))?
                    .map_err(|_| unavailable("MCP HTTP protocol negotiation failed"))?,
            };
            Ok(Session {
                running,
                child: None,
            })
        }
    }
}
