use crate::{Configuration, invalid, unavailable};
use rmcp::{
    RoleClient,
    model::{ClientRequest, ListToolsRequest, PaginatedRequestParams, ServerResult, Tool},
    service::Peer,
};
use std::collections::{BTreeMap, BTreeSet};
use swarmie_tools::{ToolName, ToolSpec};
use tembra::Cancellation;
use tokio::time::Instant;

pub(crate) struct DiscoveredTool {
    pub spec: ToolSpec,
    pub remote: Tool,
}

/// Read pages without the SDK's stale-on-error cache. Partial catalogs never publish.
pub(crate) async fn discover(
    peer: &Peer<RoleClient>,
    config: &Configuration,
    signal: &Cancellation,
    deadline: Instant,
) -> tembra::Result<BTreeMap<ToolName, DiscoveredTool>> {
    let mut tools = BTreeMap::new();
    let mut cursor = None;
    let mut seen = BTreeSet::new();
    for _ in 0..config.limits.max_catalog_pages {
        signal.check()?;
        let params = cursor
            .take()
            .map(|cursor| PaginatedRequestParams::default().with_cursor(Some(cursor)));
        let request = ListToolsRequest {
            params,
            ..Default::default()
        };
        let request = tokio::time::timeout(
            config.limits.request_timeout,
            peer.send_request(ClientRequest::ListToolsRequest(request)),
        );
        let response = tokio::select! {
            biased;
            () = signal.cancelled() => return Err(crate::error::cancelled()),
            () = tokio::time::sleep_until(deadline) => return Err(unavailable("MCP connection and discovery deadline exceeded")),
            response = request => response
                .map_err(|_| unavailable("MCP tool discovery timed out"))?
                .map_err(|_| unavailable("MCP tool discovery failed"))?,
        };
        let ServerResult::ListToolsResult(page) = response else {
            return Err(invalid("unexpected MCP tool discovery response"));
        };
        for remote in page.tools {
            if tools.len() >= config.limits.max_tools {
                return Err(invalid("MCP tool catalog exceeds its configured limit"));
            }
            let name = ToolName::new(format!("mcp.{}.{}", config.id.as_str(), remote.name))?;
            let tool = DiscoveredTool {
                spec: ToolSpec {
                    name: name.clone(),
                    description: remote.description.as_deref().unwrap_or_default().to_owned(),
                    input_schema: serde_json::Value::Object((*remote.input_schema).clone()),
                },
                remote,
            };
            if tools.insert(name, tool).is_some() {
                return Err(invalid("MCP server returned a duplicate tool name"));
            }
        }
        match page.next_cursor {
            None => return Ok(tools),
            Some(next) if !seen.insert(next.clone()) => {
                return Err(invalid("MCP server repeated a pagination cursor"));
            }
            Some(next) => cursor = Some(next),
        }
    }
    Err(invalid(
        "MCP tool catalog exceeds its configured page limit",
    ))
}
