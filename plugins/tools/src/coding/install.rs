use std::{path::PathBuf, sync::Arc};
use tembra::Orb;

use super::{
    Delete, Edit, List, Move, Read, Search, Seen, Shell, Write,
    process::Launcher,
    workspace::{Local, Workspace},
};
use crate::{Tool, ToolDefinition, ToolHandler, ToolPolicy, ToolProvider, tool_orb, tool_spec};

/// Host-selected I/O and command configuration. File paths are confined to
/// `base`. The launcher must provide any desired process sandbox separately.
#[derive(Clone)]
pub struct CodingConfig {
    pub workspace: Arc<dyn Workspace>,
    pub base: PathBuf,
    pub launcher: Launcher,
}

impl CodingConfig {
    /// Explicitly select the local filesystem and command launcher.
    pub fn local(base: impl Into<PathBuf>) -> std::io::Result<Self> {
        let base = std::fs::canonicalize(base.into())?;
        if !base.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                "coding base must be a directory",
            ));
        }
        Ok(Self {
            workspace: Arc::new(Local),
            launcher: Launcher::default().in_dir(base.clone()),
            base,
        })
    }
}

/// Declarations and metadata for the eight opt-in coding tools. Installing
/// these declarations creates no accounts, remote connections or permission
/// grants. Contribute `tools` through the host's scoped catalog lifecycle.
pub struct CodingTools {
    pub orbs: Vec<Orb>,
    pub tools: Vec<ToolDefinition>,
}

impl CodingTools {
    /// Build declarations with mandatory host admission for every final call.
    pub fn new(config: CodingConfig, policy: ToolPolicy) -> tembra::Result<Self> {
        let CodingConfig {
            workspace,
            base,
            mut launcher,
        } = config;
        // One base owns relative file paths and shell cwd; never inherit a
        // conflicting process working directory from the launcher's input.
        launcher.cwd = Some(base.clone());
        let base = Some(base);
        let seen = Seen::default();
        let mut tools = Self {
            orbs: Vec::new(),
            tools: Vec::new(),
        };
        tools.add(
            Read {
                workspace: workspace.clone(),
                base: base.clone(),
                confine: true,
                seen: seen.clone(),
            },
            &policy,
        )?;
        tools.add(
            Write {
                workspace: workspace.clone(),
                base: base.clone(),
                confine: true,
                seen: seen.clone(),
            },
            &policy,
        )?;
        tools.add(
            Edit {
                workspace: workspace.clone(),
                base: base.clone(),
                confine: true,
                seen,
            },
            &policy,
        )?;
        tools.add(
            Search {
                workspace: workspace.clone(),
                base: base.clone(),
                confine: true,
            },
            &policy,
        )?;
        tools.add(
            List {
                workspace: workspace.clone(),
                base: base.clone(),
                confine: true,
            },
            &policy,
        )?;
        tools.add(
            Delete {
                workspace: workspace.clone(),
                base: base.clone(),
                confine: true,
            },
            &policy,
        )?;
        tools.add(
            Move {
                workspace: workspace.clone(),
                base,
                confine: true,
            },
            &policy,
        )?;
        tools.add(
            Shell {
                workspace,
                launcher,
                confine: true,
                ..Shell::default()
            },
            &policy,
        )?;
        Ok(tools)
    }

    fn add<T: Tool + Clone>(&mut self, tool: T, policy: &ToolPolicy) -> tembra::Result<()> {
        let spec = tool_spec(&tool)?;
        let service = format!("swarmie.tools.coding.{}", spec.name.as_str());
        self.tools.push(ToolDefinition::new(
            spec,
            ToolProvider::new(service.clone(), vec!["main".into()])?,
        )?);
        let policy = policy.clone();
        self.orbs.push(tool_orb(service, move |declaration| {
            if declaration.address()?.key != ["main"] {
                return Err(crate::invalid("coding tool uses the main instance"));
            }
            Ok(ToolHandler::new(tool.clone())?.with_policy(policy.clone()))
        }));
        Ok(())
    }
}
