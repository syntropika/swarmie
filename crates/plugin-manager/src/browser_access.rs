use std::collections::{BTreeMap, BTreeSet};

use swarmie_plugin_contracts::*;

use crate::{ManagerError, PluginManager, Result, manager::BrowserAuthorization};

impl PluginManager {
    /// Issues least-authority access after the embedding authenticates this browser session.
    ///
    /// Only exact currently admitted browser activations are granted. The issuer must revoke
    /// the grant on transport-session termination. Grants are volatile and never survive a
    /// supervisor restart. Neither possession nor issuance grants administrative mutation.
    pub fn issue_browser_grant(
        &self,
        proof: &AdminProof,
        session_id: String,
        plugin_ids: &BTreeSet<String>,
    ) -> Result<BrowserGrant> {
        self.authorize(proof)?;
        if session_id.is_empty() || session_id.len() > 128 {
            return Err(ManagerError::InvalidPackage(
                "invalid admitted browser session".into(),
            ));
        }
        let mut activations = BTreeMap::new();
        for id in plugin_ids {
            let package = self.active_package(proof, id)?;
            if !package
                .manifest
                .facets
                .iter()
                .any(|facet| facet.target == FacetTarget::Browser)
            {
                return Err(ManagerError::InvalidPackage(
                    "browser grant requires a browser facet".into(),
                ));
            }
            activations.insert(
                id.clone(),
                (package.manifest.version, package.activation_id),
            );
        }
        let grant = BrowserGrant::new(format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ));
        self.browser_grants.borrow_mut().insert(
            grant.as_str().into(),
            BrowserAuthorization {
                session_id,
                activations,
            },
        );
        Ok(grant)
    }

    /// Revokes browser access independently of administrative authority and package intent.
    pub fn revoke_browser_grant(&self, proof: &AdminProof, grant: &BrowserGrant) -> Result<()> {
        self.authorize(proof)?;
        self.browser_grants.borrow_mut().remove(grant.as_str());
        Ok(())
    }

    fn browser_authorization(
        &self,
        grant: &BrowserGrant,
        session_id: &str,
    ) -> Result<BrowserAuthorization> {
        self.storage()?;
        self.browser_grants
            .borrow()
            .get(grant.as_str())
            .filter(|authorization| authorization.session_id == session_id)
            .cloned()
            .ok_or(ManagerError::Unauthorized)
    }

    fn granted_activation(
        &self,
        grant: &BrowserGrant,
        session_id: &str,
        id: &str,
    ) -> Result<(String, String)> {
        self.browser_authorization(grant, session_id)?
            .activations
            .get(id)
            .cloned()
            .ok_or(ManagerError::Unauthorized)
    }

    /// Returns only this capability's current activations and its own recorded observations.
    pub fn browser_inventory(&self, request: BrowserInventoryRequest) -> Result<BrowserInventory> {
        let authorization = self.browser_authorization(&request.grant, &request.session_id)?;
        let mut packages = vec![];
        for (id, (version, activation)) in &authorization.activations {
            if self.uncertain.borrow().contains_key(id) {
                continue;
            }
            if let Some(plugin) = self.storage()?.load(id)?
                && plugin.desired == DesiredState::Active
                && plugin.package.manifest.version == *version
                && plugin.activation_id == *activation
            {
                packages.push(self.active_package(&self.proof, id)?);
            }
        }
        let observations = self
            .storage()?
            .browser_sessions()?
            .into_iter()
            .filter(|record| {
                record.session_id == authorization.session_id
                    && authorization
                        .activations
                        .get(&record.plugin_id)
                        .is_some_and(|(version, activation)| {
                            *version == record.version && *activation == record.activation_id
                        })
            })
            .collect();
        Ok(BrowserInventory {
            session_id: authorization.session_id,
            packages,
            observations,
        })
    }

    /// Returns one exact approved activation without exposing the administrative inventory.
    pub fn browser_active_package(
        &self,
        request: BrowserActivePackageRequest,
    ) -> Result<ActivePackage> {
        let (version, activation) =
            self.granted_activation(&request.grant, &request.session_id, &request.plugin_id)?;
        let package = self.active_package(&self.proof, &request.plugin_id)?;
        if package.activation_id != activation || package.manifest.version != version {
            return Err(ManagerError::Unauthorized);
        }
        Ok(package)
    }

    /// Reads approved bytes using the grant's exact session and activation scope.
    pub fn browser_read_package_file(
        &self,
        request: BrowserReadPackageFileRequest,
    ) -> Result<ReadPackageFileResult> {
        let (_, activation) =
            self.granted_activation(&request.grant, &request.session_id, &request.plugin_id)?;
        if activation != request.activation_id {
            return Err(ManagerError::Unauthorized);
        }
        self.read_package_file(ReadPackageFileRequest {
            proof: self.proof.clone(),
            plugin_id: request.plugin_id,
            activation_id: request.activation_id,
            path: request.path,
            offset: request.offset,
            length: request.length,
        })
    }

    /// Records only this capability's own exact browser activation or known retirement.
    pub fn browser_report(&self, request: BrowserReportRequest) -> Result<BrowserInventory> {
        let (version, activation) =
            self.granted_activation(&request.grant, &request.session_id, &request.plugin_id)?;
        if version != request.version || activation != request.activation_id {
            return Err(ManagerError::Unauthorized);
        }
        self.report_browser(ReportBrowserRequest {
            proof: self.proof.clone(),
            plugin_id: request.plugin_id,
            version: request.version,
            activation_id: request.activation_id,
            session_id: request.session_id.clone(),
            observed: request.observed,
            error: request.error.replace(request.grant.as_str(), "[redacted]"),
        })?;
        self.browser_inventory(BrowserInventoryRequest {
            grant: request.grant,
            session_id: request.session_id,
        })
    }
}
