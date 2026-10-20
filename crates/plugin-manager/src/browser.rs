use swarmie_plugin_contracts::*;

use crate::{
    ManagerError, PluginManager, Result,
    store::CachedPlugin,
    validation::{safe_path, validate_package},
};

/// Maximum bytes returned by one portable package-file read.
pub const MAX_PACKAGE_CHUNK_BYTES: u32 = 32_768;

impl PluginManager {
    /// Returns approved metadata and a receipt without transferring package-file bytes.
    ///
    /// # Errors
    /// Refuses invalid proofs, unknown plugins, invalid cached bytes and inactive intent.
    /// Metadata does not assert that a browser has loaded or activated the package.
    pub fn active_package(&self, proof: &AdminProof, id: &str) -> Result<ActivePackage> {
        let plugin = self.active_selection(proof, id)?;
        Ok(ActivePackage {
            manifest: plugin.package.manifest,
            provenance: plugin.package.provenance,
            activation_id: plugin.activation_id,
        })
    }

    fn active_selection(&self, proof: &AdminProof, id: &str) -> Result<CachedPlugin> {
        self.authorize(proof)?;
        if self.uncertain.borrow().contains_key(id) {
            return Err(ManagerError::RecoveryRequired);
        }
        let plugin = self.dynamic(id)?;
        if plugin.desired != DesiredState::Active || plugin.activation_id.is_empty() {
            return Err(ManagerError::InvalidPackage(
                "plugin has no admitted active intent on this node".into(),
            ));
        }
        validate_package(&plugin.package)?;
        Ok(plugin)
    }

    /// Reads an exact bounded range from a currently approved cached package file.
    ///
    /// Every call verifies authorization, current activation receipt, manifest membership and
    /// cached integrity. Length must be 1 through 32,768 bytes; offset may equal the total file
    /// length to read an empty end-of-file range. No provider or network acquisition occurs.
    /// # Errors
    /// Refuses stale/inactive/uncertain selections, unsafe or absent paths, invalid bounds,
    /// unauthorized callers and corrupted cache bytes. Refusal does not mutate package state.
    pub fn read_package_file(
        &self,
        request: ReadPackageFileRequest,
    ) -> Result<ReadPackageFileResult> {
        let plugin = self.active_selection(&request.proof, &request.plugin_id)?;
        if plugin.activation_id != request.activation_id {
            return Err(ManagerError::InvalidPackage(
                "stale package activation receipt".into(),
            ));
        }
        if !safe_path(&request.path)
            || !plugin
                .package
                .manifest
                .files
                .iter()
                .any(|file| file.path == request.path)
        {
            return Err(ManagerError::InvalidPackage(
                "package file is not an exact approved manifest path".into(),
            ));
        }
        if request.length == 0 || request.length > MAX_PACKAGE_CHUNK_BYTES {
            return Err(ManagerError::InvalidPackage(
                "package chunk length must be 1 through 32768 bytes".into(),
            ));
        }
        let file = plugin
            .package
            .files
            .iter()
            .find(|file| file.path == request.path)
            .ok_or_else(|| ManagerError::Integrity("approved package file is absent".into()))?;
        let total_length = u32::try_from(file.bytes.len()).map_err(|_| {
            ManagerError::InvalidPackage("package file length exceeds the portable limit".into())
        })?;
        if request.offset > total_length {
            return Err(ManagerError::InvalidPackage(
                "package chunk offset exceeds the approved file length".into(),
            ));
        }
        let start = usize::try_from(request.offset).map_err(|_| {
            ManagerError::InvalidPackage(
                "package chunk offset is unsupported on this platform".into(),
            )
        })?;
        let end = start
            .saturating_add(request.length as usize)
            .min(file.bytes.len());
        Ok(ReadPackageFileResult {
            bytes: file.bytes[start..end].to_vec(),
            total_length,
        })
    }

    /// Records a browser Host's observed result separately from native runtime availability.
    ///
    /// Active/failure observations require the current release and activation receipt. A
    /// retiring session may acknowledge actual removal using its previously recorded receipt,
    /// including after update or uninstall. Receipt history is retained independently for every
    /// activation in a browser session, so a failed newer candidate cannot erase an older
    /// physical generation's pending removal acknowledgement. Native deactivation alone does not establish that
    /// a browser stopped running old code; its last observation remains until acknowledged.
    /// # Errors
    /// Refuses stale receipts, unknown sessions, invalid proofs and SQLite failures.
    pub fn report_browser(&self, request: ReportBrowserRequest) -> Result<Inventory> {
        self.authorize(&request.proof)?;
        let _guard = self.guard()?;
        if request.session_id.is_empty()
            || request.session_id.len() > 128
            || request.error.len() > 4096
        {
            return Err(ManagerError::InvalidPackage(
                "browser session identity or diagnostic length is invalid".into(),
            ));
        }
        let previous = self
            .storage()?
            .browser_sessions()?
            .into_iter()
            .find(|record| {
                record.plugin_id == request.plugin_id
                    && record.session_id == request.session_id
                    && record.activation_id == request.activation_id
                    && record.version == request.version
            });
        let known_retirement = request.observed == BrowserObservation::Inactive
            && previous.as_ref().is_some_and(|record| {
                record.version == request.version && record.activation_id == request.activation_id
            });
        if !known_retirement {
            let plugin = self.dynamic(&request.plugin_id)?;
            if plugin.desired != DesiredState::Active
                || plugin.package.manifest.version != request.version
                || plugin.activation_id != request.activation_id
                || request.activation_id.is_empty()
            {
                return Err(ManagerError::InvalidPackage(
                    "stale browser activation receipt".into(),
                ));
            }
            if !plugin
                .package
                .manifest
                .facets
                .iter()
                .any(|facet| facet.target == FacetTarget::Browser)
            {
                return Err(ManagerError::InvalidPackage(
                    "plugin has no browser facet".into(),
                ));
            }
        }
        self.storage()?.browser_session(&BrowserSessionRecord {
            plugin_id: request.plugin_id,
            version: request.version,
            activation_id: request.activation_id,
            session_id: request.session_id,
            observed: request.observed,
            error: request.error.replace(self.proof.as_str(), "[redacted]"),
        })?;
        self.snapshot()
    }
}
