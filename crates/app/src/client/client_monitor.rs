//! Native node health observation and bounded reconnect scheduling.
use crate::{client_submissions::ClientSubmissions, node_gateway::NodeGateway};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub fn spawn(
    nodes: NodeGateway,
    watched: Arc<Mutex<HashSet<String>>>,
    submissions: ClientSubmissions,
    monitor_stop: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(Duration::from_secs(1));
        let mut retries: HashMap<String, (tokio::time::Instant, u64)> = HashMap::new();
        loop {
            tokio::select! { _ = monitor_stop.cancelled() => break, _ = timer.tick() => {} }
            let mut destinations = watched.lock().await.clone();
            destinations.insert(nodes.local_id().to_owned());
            retries.retain(|node, _| destinations.contains(node));
            let mut checks = tokio::task::JoinSet::new();
            for node in destinations {
                if retries
                    .get(&node)
                    .is_some_and(|(at, _)| *at > tokio::time::Instant::now())
                {
                    continue;
                }
                let nodes = nodes.clone();
                checks.spawn(async move { let response = nodes.forward(&node, json!({"jsonrpc":"2.0","id":"health","method":"runtime/initialize","params":{}})).await; (node, response) });
            }
            loop {
                tokio::select! {
                    _ = monitor_stop.cancelled() => { checks.abort_all(); return; },
                    checked = checks.join_next() => {
                        let Some(Ok((node, response))) = checked else { break; };
                        if node != nodes.local_id() && !watched.lock().await.contains(&node) { nodes.disconnect(&node).await; continue; }
                        let connected = response.get("result").is_some();
                        if let Ok(target) = serde_json::from_value::<swarmie::RuntimeIdentity>(response["result"]["runtimeIdentity"].clone())
                            && let Ok(Err(error)) = tokio::time::timeout(Duration::from_secs(1), submissions.observe(&node, &target)).await { tracing::debug!("native submission observation: {error}"); }
                        let delay = if connected { 2 } else { retries.get(&node).map_or(1, |(_, delay)| *delay).saturating_mul(2).min(30) };
                        retries.insert(node.clone(), (tokio::time::Instant::now() + Duration::from_secs(delay), delay));
                        nodes.publish(json!({"environmentId":node,"method":"client/status","params":{"connected":connected,"runtimeIdentity":response["result"]["runtimeIdentity"],"error":response["error"]["message"]}}));
                    }
                }
            }
        }
    })
}
