use crate::directories::NativeDirectories;
use std::{path::PathBuf, rc::Rc, sync::Arc};
use swarmie_conversation_runtime::ExecutionOwner;
use swarmie_providers::native::NativeProviders;
use swarmie_runtime::NodeId;
use swarmie_tools::Catalog;
use tembra::Host;

/// Configuration supplied by the embedding, independent of a desktop installation.
#[derive(Clone)]
pub struct ServicesConfiguration {
    /// Optional native project content. The host chooses all additional roots.
    pub environment: Option<swarmie_context::environment::Configuration>,
    /// Existing profile directory. The embedding must hold its exclusive writer lock.
    pub state: PathBuf,
    /// Stable identity persisted by the embedding.
    pub node: NodeId,
    /// Stable, non-nil OAuth host identity persisted by the embedding.
    pub oauth_host: uuid::Uuid,
    /// Principal authenticated by the embedding's transport.
    pub principal: String,
    /// Optional permitted directory root. No directory service is installed when absent.
    pub directory_root: Option<PathBuf>,
    /// Optional host-authorized Git capability. Selection does not grant execution access.
    pub git: Option<swarmie_git::GitService>,
    /// Explicit agent mutation policy. Without it the agent receives Git reads only.
    pub agent_git_authority: Option<Arc<dyn swarmie_git::Authority>>,
}

/// Declare provider administration, Tools, MCP and conversation services.
///
/// The caller owns the host and its shutdown. Calls require a Tokio local executor with
/// I/O, time and blocking-task support. Credential custody uses the OS credential store;
/// public account metadata and conversation history use the supplied profile directory.
/// Conversation and task storage opens during managed resource activation.
/// Task scheduling starts paused. Call [`swarmie_conversation_runtime::start`] after the runtime has
/// installed the complete composition to enable admitted work and recovery.
/// The embedding supplies node identity through the runtime's node plugin.
///
/// # Errors
/// Returns a configuration or storage-declaration error. On failure the caller
/// must close the host before it releases the profile's exclusive writer lock.
pub fn orbs(
    host: &Host,
    configuration: ServicesConfiguration,
    execution: Option<Rc<dyn ExecutionOwner>>,
) -> tembra::Result<Vec<tembra::Orb>> {
    let ServicesConfiguration {
        state,
        node,
        principal,
        oauth_host,
        directory_root,
        git,
        agent_git_authority,
        environment,
    } = configuration;
    if principal.is_empty() || principal == swarmie_durable_tasks::HOST_PRINCIPAL {
        return Err(tembra::Error::invalid(
            "node.principal",
            "a non-host user principal is required",
        ));
    }
    if oauth_host.is_nil() {
        return Err(tembra::Error::invalid(
            "node.oauth_host",
            "OAuth host identity is required",
        ));
    }
    let mut tools = if git.is_some() {
        swarmie_git::tool_definitions()?
            .into_iter()
            .filter(|tool| tool.spec.name.as_str() != "git_mutate" || agent_git_authority.is_some())
            .collect::<Vec<_>>()
    } else {
        vec![]
    };
    if let Some(owner) = &execution {
        tools.extend(owner.tools()?);
    }
    let catalog = Catalog::new(tools)?;
    let conversation_catalog = catalog.clone();
    let settings_catalog = catalog.clone();
    let providers = Rc::new(NativeProviders::new(
        state.join("providers.json"),
        node.clone().into(),
        principal.clone(),
        oauth_host,
    ));
    let tools_principal = principal.clone();
    let settings_principal = principal.clone();
    let mut definitions = providers.clone().orbs();
    definitions.extend([
        swarmie_tools::orb_with_catalog(host.clone(), move |caller| {
            if caller == tools_principal {
                Ok(catalog.clone())
            } else {
                Err(tembra::Error::invalid("node.principal", "unbound user"))
            }
        }),
        swarmie_mcp::settings::settings_orb(
            host.clone(),
            Rc::new(swarmie_mcp::settings::FileDefinitions(
                state.join("mcp.json"),
            )),
            move |caller| {
                if caller == settings_principal {
                    Ok(settings_catalog.clone())
                } else {
                    Err(tembra::Error::invalid("node.principal", "unbound user"))
                }
            },
        ),
    ]);
    if let Some(service) = &git {
        definitions.push(swarmie_git::orb(service.clone()));
    }
    if let Some(root) = directory_root {
        definitions.push(swarmie_runtime::directories_orb(Rc::new(
            NativeDirectories::new(root, principal.clone()),
        )));
    }
    definitions.extend(swarmie_conversation_runtime::orbs(
        host.clone(),
        state,
        providers.node_id(),
        principal,
        providers,
        conversation_catalog,
        swarmie_conversation_runtime::ExecutionPolicy {
            service: git,
            authority: agent_git_authority,
            owner: execution,
            environment,
        },
    )?);
    Ok(definitions)
}
