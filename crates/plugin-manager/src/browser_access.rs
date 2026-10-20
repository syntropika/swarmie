use swarmie_plugin_contracts::*;

use crate::{ManagerError, PluginManager, Result};

impl PluginManager {
    /// Discovers currently admitted browser packages without exposing administrative state.
    pub fn active_browser_packages(&self) -> Result<Vec<ActivePackage>> {
        let mut packages = vec![];
        for plugin in self.storage()?.all()? {
            if plugin.desired == DesiredState::Active
                && !plugin.activation_id.is_empty()
                && !self
                    .uncertain
                    .borrow()
                    .contains_key(&plugin.package.manifest.id)
                && plugin
                    .package
                    .manifest
                    .facets
                    .iter()
                    .any(|facet| facet.target == FacetTarget::Browser)
            {
                packages.push(self.active_package(&plugin.package.manifest.id)?);
            }
        }
        Ok(packages)
    }

    /// Returns approved metadata only for a package with an applicable Browser Facet.
    pub fn active_browser_package(&self, id: &str) -> Result<ActivePackage> {
        let package = self.active_package(id)?;
        if !package
            .manifest
            .facets
            .iter()
            .any(|facet| facet.target == FacetTarget::Browser)
        {
            return Err(ManagerError::InvalidPackage(
                "package has no browser facet".into(),
            ));
        }
        Ok(package)
    }

    /// Delivers exact approved package bytes only for an applicable browser package.
    pub fn read_browser_package_file(
        &self,
        request: ReadPackageFileRequest,
    ) -> Result<ReadPackageFileResult> {
        self.active_browser_package(&request.plugin_id)?;
        self.read_package_file(request)
    }

    /// Reads browser coordination state for a session identified by the trusted embedding.
    /// The session label is lifecycle metadata, not caller authentication.
    pub fn browser_inventory(&self, session_id: &str) -> Result<BrowserInventory> {
        if session_id.is_empty() || session_id.len() > 128 {
            return Err(ManagerError::InvalidPackage(
                "invalid admitted browser session".into(),
            ));
        }
        Ok(BrowserInventory {
            session_id: session_id.into(),
            packages: self.active_browser_packages()?,
            observations: self
                .storage()?
                .browser_sessions()?
                .into_iter()
                .filter(|record| record.session_id == session_id)
                .collect(),
        })
    }

    /// Records a trusted local observation; no anonymous remote report ingress is installed.
    pub fn browser_report(&self, request: ReportBrowserRequest) -> Result<BrowserInventory> {
        let session = request.session_id.clone();
        self.report_browser(request)?;
        self.browser_inventory(&session)
    }
}
