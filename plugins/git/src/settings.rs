//! Project Git preferences. Execution platform is supplied by the selected node.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLayout {
    #[default]
    Unified,
    Split,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SetupScript {
    pub enabled: bool,
    pub shell: PathBuf,
    pub arguments: Vec<String>,
    pub script: String,
    /// Relative directory inside the newly prepared checkout.
    pub working_directory: PathBuf,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct GitSettings {
    pub diff_layout: DiffLayout,
    pub include_local_changes: bool,
    pub managed_root: Option<PathBuf>,
    pub branch_prefix: String,
    pub commit_guidance: String,
    /// Default for commits prepared by the project UI; each operation captures its choice.
    #[serde(default)]
    pub run_commit_hooks: bool,
    pub cleanup_enabled: bool,
    /// No implicit retention count is selected on behalf of the user.
    pub retained_worktrees: Option<usize>,
    /// Platform keys refer to execution nodes, never the browser's platform.
    pub setup_by_platform: BTreeMap<String, SetupScript>,
}

impl GitSettings {
    /// Returns only the effective script for the captured execution platform.
    pub fn setup_for(&self, execution_platform: &str) -> Option<&SetupScript> {
        self.setup_by_platform
            .get(execution_platform)
            .filter(|script| script.enabled)
    }

    /// Saving the current node's script preserves other nodes' settings.
    pub fn set_setup(&mut self, execution_platform: String, script: SetupScript) {
        self.setup_by_platform.insert(execution_platform, script);
    }
}
