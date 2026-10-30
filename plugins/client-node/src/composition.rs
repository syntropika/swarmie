use crate::{definitions::Definitions, directories::NativeDirectories, providers::NativeProviders};
use std::{path::PathBuf, rc::Rc, sync::Arc};
use swarmie_runtime::{NodeId, node_orb};
use swarmie_tools::Catalog;
use tembra::Host;

/// Configuration supplied by the embedding, independent of a desktop installation.
pub struct NodeConfiguration {
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

/// Install identity, provider administration, Tools, MCP and conversation services.
///
/// The caller owns the host and its shutdown. Calls require a Tokio local executor with
/// I/O, time and blocking-task support. Credential custody uses the OS credential store;
/// public account metadata and conversation history use the supplied profile directory.
/// Task scheduling starts paused. Call [`crate::start`] after installation to enable
/// admitted work and recovery through the host's complete composition.
///
/// # Errors
/// Returns the capability installation or profile-storage error. On failure the caller
/// must close the host before it releases the profile's exclusive writer lock.
pub fn install(host: &Host, configuration: NodeConfiguration) -> tembra::Result<()> {
    let NodeConfiguration {
        state,
        node,
        principal,
        oauth_host,
        directory_root,
        git,
        agent_git_authority,
    } = configuration;
    if oauth_host.is_nil() {
        return Err(tembra::Error::invalid(
            "client-node.oauth_host",
            "OAuth host identity is required",
        ));
    }
    let catalog = Catalog::new(if git.is_some() {
        swarmie_git::tool_definitions()?
            .into_iter()
            .filter(|tool| tool.spec.name.as_str() != "git_mutate" || agent_git_authority.is_some())
            .collect()
    } else {
        vec![]
    })?;
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
    let mut definitions = vec![
        node_orb(node),
        swarmie_providers::settings::settings_orb(providers.clone()),
        crate::providers::oauth::orb(providers.clone()),
        swarmie_tools::orb_with_catalog(host.clone(), move |caller| {
            if caller == tools_principal {
                Ok(catalog.clone())
            } else {
                Err(tembra::Error::invalid(
                    "client-node.principal",
                    "unbound user",
                ))
            }
        }),
        swarmie_mcp::settings::settings_orb(
            host.clone(),
            Rc::new(Definitions(state.join("mcp.json"))),
            move |caller| {
                if caller == settings_principal {
                    Ok(settings_catalog.clone())
                } else {
                    Err(tembra::Error::invalid(
                        "client-node.principal",
                        "unbound user",
                    ))
                }
            },
        ),
    ];
    if let Some(service) = &git {
        definitions.push(swarmie_git::orb(service.clone()));
    }
    if let Some(root) = directory_root {
        definitions.push(swarmie_runtime::directories_orb(Rc::new(
            NativeDirectories::new(root, principal.clone()),
        )));
    }
    definitions.extend(crate::conversations::orbs(
        host.clone(),
        state,
        providers.node_id(),
        principal,
        providers,
        conversation_catalog,
        crate::conversations::GitExecution {
            service: git,
            authority: agent_git_authority,
        },
    )?);
    host.install(definitions)?;
    Ok(())
}
