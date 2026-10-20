use std::fmt;

use tembra::prelude::*;

use crate::{PackageProvenance, PluginManifest, SourceOperation};

/// Opaque administrative capability material, verified by the receiving supervisor.
///
/// Construction only wraps a value. It does not authenticate a caller or grant permission.
/// Debug output is redacted; serialization intentionally carries the proof to its verifier.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", transparent)]
#[schemars(crate = "tembra::schemars")]
pub struct AdminProof(String);

impl AdminProof {
    /// Wraps proof material issued by the supervisor.
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// Borrows the proof for verification at an administrative ingress.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AdminProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AdminProof([redacted])")
    }
}

/// An explicitly confirmed installation candidate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct InstallRequest {
    pub proof: AdminProof,
    pub provider_id: String,
    pub plugin_id: String,
    pub version: String,
    pub confirmed: bool,
}

/// An installed plugin selected for activation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ActivateRequest {
    pub proof: AdminProof,
    pub plugin_id: String,
}

/// An installed plugin selected for deactivation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct DeactivateRequest {
    pub proof: AdminProof,
    pub plugin_id: String,
}

/// An explicitly confirmed replacement for an installed plugin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct UpdateRequest {
    pub proof: AdminProof,
    pub provider_id: String,
    pub plugin_id: String,
    pub version: String,
    pub confirmed: bool,
}

/// An installed plugin selected for removal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct UninstallRequest {
    pub proof: AdminProof,
    pub plugin_id: String,
}

/// Durable source-provider settings; interpretation belongs to the provider.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ConfigureProviderRequest {
    pub proof: AdminProof,
    pub provider_id: String,
    pub configuration: tembra::serde_json::Value,
}

/// An authenticated inventory query.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct InventoryRequest {
    pub proof: AdminProof,
}

/// An authenticated request for the currently admitted browser package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ActivePackageRequest {
    pub proof: AdminProof,
    pub plugin_id: String,
}

/// Approved package metadata and the activation receipt required for bounded file reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ActivePackage {
    pub manifest: PluginManifest,
    pub provenance: PackageProvenance,
    pub activation_id: String,
}

/// A receipt-bound request for at most 32 KiB of an approved package file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ReadPackageFileRequest {
    pub proof: AdminProof,
    pub plugin_id: String,
    pub activation_id: String,
    pub path: String,
    #[schemars(range(max = 8_388_608))]
    pub offset: u32,
    #[schemars(range(min = 1, max = 32_768))]
    pub length: u32,
}

/// A bounded exact byte range and the stable approved file length.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ReadPackageFileResult {
    #[schemars(length(max = 32_768))]
    pub bytes: Vec<u8>,
    #[schemars(range(max = 8_388_608))]
    pub total_length: u32,
}

/// Browser-local execution observation, distinct from server plugin availability.
/// Unknown means the supervisor has no current confirmation of the session's execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum BrowserObservation {
    Active,
    Inactive,
    Failed,
    Unknown,
}

/// An authenticated browser acknowledgement bound to a specific activation and session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ReportBrowserRequest {
    pub proof: AdminProof,
    pub plugin_id: String,
    pub version: String,
    pub activation_id: String,
    pub session_id: String,
    pub observed: BrowserObservation,
    pub error: String,
}

/// Persisted browser session evidence; an empty error denotes no reported error.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct BrowserSessionRecord {
    pub plugin_id: String,
    pub version: String,
    pub activation_id: String,
    pub session_id: String,
    pub observed: BrowserObservation,
    pub error: String,
}

/// How a plugin enters the runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum PluginKind {
    Core,
    Dynamic,
}

/// Durable administrative intent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum DesiredState {
    Installed,
    Active,
    Absent,
}

/// Sanitized server and administrative availability on the connected node.
/// Browser execution is reported separately for each browser session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum ObservedState {
    Installed,
    Active,
    Inactive,
    Failed,
    Unavailable,
}

/// Installed release and its independently tracked intent and observation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct PluginRecord {
    pub plugin_id: String,
    pub version: String,
    pub provider_id: String,
    pub kind: PluginKind,
    pub desired: DesiredState,
    pub observed: ObservedState,
    pub errors: Vec<String>,
}

/// A discovered source provider and its administrative settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ProviderRecord {
    pub provider_id: String,
    pub configuration: tembra::serde_json::Value,
    pub operations: Vec<SourceOperation>,
}

/// Administrative operation categories persisted for reconciliation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum AdminOperation {
    Install,
    Activate,
    Deactivate,
    Update,
    Uninstall,
    ConfigureProvider,
    Recover,
}

/// Administrative progress, distinct from plugin availability.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum OperationStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Interrupted,
}

/// Durable operation evidence; an empty error denotes no recorded error.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct OperationRecord {
    pub id: String,
    pub plugin_id: String,
    pub operation: AdminOperation,
    pub status: OperationStatus,
    pub error: String,
}

/// Sanitized administrative snapshot rebuilt from durable records and live discovery.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct Inventory {
    pub plugins: Vec<PluginRecord>,
    pub providers: Vec<ProviderRecord>,
    pub operations: Vec<OperationRecord>,
    pub browser_sessions: Vec<BrowserSessionRecord>,
}

/// Local administrative control plane; ordinary plugin invocation bypasses this contract.
#[tembra::contract(local, name = "swarmie.plugin-administration", version = 1)]
pub trait PluginAdministration {
    #[portable]
    async fn install(&self, request: InstallRequest) -> Result<Inventory>;

    #[portable]
    async fn activate(&self, request: ActivateRequest) -> Result<Inventory>;

    #[portable]
    async fn deactivate(&self, request: DeactivateRequest) -> Result<Inventory>;

    #[portable]
    async fn update(&self, request: UpdateRequest) -> Result<Inventory>;

    #[portable]
    async fn uninstall(&self, request: UninstallRequest) -> Result<Inventory>;

    #[portable]
    async fn configure_provider(&self, request: ConfigureProviderRequest) -> Result<Inventory>;

    #[portable]
    async fn inventory(&self, request: InventoryRequest) -> Result<Inventory>;

    #[portable]
    async fn active_package(&self, request: ActivePackageRequest) -> Result<ActivePackage>;

    #[portable]
    async fn read_package_file(
        &self,
        request: ReadPackageFileRequest,
    ) -> Result<ReadPackageFileResult>;

    #[portable]
    async fn report_browser(&self, request: ReportBrowserRequest) -> Result<Inventory>;
}

/// Remote administrative ingress; implementations must verify proofs before effects.
#[tembra::contract(
    remote,
    name = "swarmie.remote-plugin-administration",
    version = 1,
    host = "swarmie-runtime"
)]
pub trait RemotePluginAdministration {
    async fn install(&self, request: InstallRequest) -> Result<Inventory>;

    async fn activate(&self, request: ActivateRequest) -> Result<Inventory>;

    async fn deactivate(&self, request: DeactivateRequest) -> Result<Inventory>;

    async fn update(&self, request: UpdateRequest) -> Result<Inventory>;

    async fn uninstall(&self, request: UninstallRequest) -> Result<Inventory>;

    async fn configure_provider(&self, request: ConfigureProviderRequest) -> Result<Inventory>;

    async fn inventory(&self, request: InventoryRequest) -> Result<Inventory>;

    async fn active_package(&self, request: ActivePackageRequest) -> Result<ActivePackage>;

    async fn read_package_file(
        &self,
        request: ReadPackageFileRequest,
    ) -> Result<ReadPackageFileResult>;

    async fn report_browser(&self, request: ReportBrowserRequest) -> Result<Inventory>;
}
