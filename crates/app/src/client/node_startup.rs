//! Resolve startup control from the native registry, never from a browser-supplied SSH command.
use crate::node_gateway::{Local, NodeGateway};
use serde_json::{Value, json};
use std::time::Duration;
use storage::NodeTransport;
use swarmie::{Error, Result};

impl NodeGateway {
    pub(super) async fn startup(&self, params: &Value) -> Result<Value> {
        let node = params["environmentId"]
            .as_str()
            .ok_or_else(|| Error::Invalid("environmentId is required".into()))?;
        let enabled = match params.get("enabled") {
            Some(value) => Some(
                value
                    .as_bool()
                    .ok_or_else(|| Error::Invalid("enabled must be a boolean".into()))?,
            ),
            None => None,
        };
        let boot = match params.get("boot") {
            Some(value) => value
                .as_bool()
                .ok_or_else(|| Error::Invalid("boot must be a boolean".into()))?,
            None => false,
        };
        if node == self.local_id {
            match &self.local {
                Local::Endpoint { root, identity, .. } => {
                    if crate::runtime_host::read(root)?.is_none_or(|endpoint| endpoint.runtime_identity.identity != *identity) {
                        return Err(Error::Invalid("local node changed; check it again before configuring startup".into()));
                    }
                    let mut result = match enabled {
                        Some(enabled) => crate::service::configure_startup(root, enabled, boot).await?,
                        None => crate::service::inspect_startup(root).await?,
                    };
                    result["environmentId"] = node.into();
                    return Ok(result);
                }
                Local::Runtime(_) => return Err(Error::Unsupported("Configure startup through the independently installed native client or service CLI.".into())),
            }
        }
        let name = params["name"]
            .as_str()
            .ok_or_else(|| Error::Invalid("saved SSH connection name is required".into()))?;
        let revision = params["revision"]
            .as_u64()
            .ok_or_else(|| Error::Invalid("connection revision is required".into()))?;
        let route =
            self.manager.find(name).await?.ok_or_else(|| {
                Error::Invalid("connection was removed; check the node again".into())
            })?;
        if route.revision != revision || route.identity.environment_id != node {
            return Err(Error::Invalid(
                "connection changed; check the node again before configuring startup".into(),
            ));
        }
        let NodeTransport::Ssh { host, socket_path } = route.transport else {
            return Err(Error::Unsupported(
                "Startup configuration requires a local node or a saved SSH connection.".into(),
            ));
        };
        if !socket_path.is_empty() {
            return Err(Error::Unsupported(
                "Configure startup for a custom runtime directory separately.".into(),
            ));
        }
        let readiness = crate::remote::inspect_ssh(&host, "", None).await?;
        let target: swarmie::RuntimeIdentity =
            serde_json::from_value(readiness["runtimeIdentity"].clone()).map_err(|_| {
                Error::Invalid("verify the running SSH node before configuring startup".into())
            })?;
        if target.identity != route.identity {
            return Err(Error::Invalid(
                "remote node changed; startup configuration was not modified".into(),
            ));
        }
        let action = match enabled {
            Some(true) => "enable",
            Some(false) => "disable",
            None => "inspect",
        };
        let shell = super::ssh_shell::discover(&host).await?;
        let mut arguments = vec!["service".into(), "startup".into(), action.into()];
        if boot {
            arguments.push("--boot".into());
        }
        arguments.extend([
            "--expected-node".into(),
            serde_json::to_string(&route.identity)?,
        ]);
        let command = shell.runtime(&arguments);
        let mut result = crate::client::ssh_preparation::execute(
            &host,
            command,
            None,
            None,
            Duration::from_secs(60),
        )
        .await?;
        result["environmentId"] = json!(node);
        Ok(result)
    }
}
