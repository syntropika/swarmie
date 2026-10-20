use std::fmt;

use tembra::prelude::*;

use crate::{ActivePackage, BrowserObservation, BrowserSessionRecord, ReadPackageFileResult};

/// Opaque, revocable browser capability issued by the trusted transport embedding.
///
/// A grant is bound to one supervisor incarnation, admitted session and exact approved
/// activations. It never authorizes administrative mutation. The embedding must authenticate
/// its session before issuing a grant; a caller-supplied session label is not authentication.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", transparent)]
#[schemars(crate = "tembra::schemars")]
pub struct BrowserGrant(String);

impl BrowserGrant {
    /// Wraps transported capability material; construction grants no authority.
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// Borrows capability material for verification by its issuer.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for BrowserGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BrowserGrant([redacted])")
    }
}

/// Inventory restricted to this grant's approved packages and original browser session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct BrowserInventory {
    pub session_id: String,
    pub packages: Vec<ActivePackage>,
    pub observations: Vec<BrowserSessionRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct BrowserInventoryRequest {
    pub grant: BrowserGrant,
    pub session_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct BrowserActivePackageRequest {
    pub grant: BrowserGrant,
    pub session_id: String,
    pub plugin_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct BrowserReadPackageFileRequest {
    pub grant: BrowserGrant,
    pub session_id: String,
    pub plugin_id: String,
    pub activation_id: String,
    pub path: String,
    #[schemars(range(max = 8_388_608))]
    pub offset: u32,
    #[schemars(range(min = 1, max = 32_768))]
    pub length: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct BrowserReportRequest {
    pub grant: BrowserGrant,
    pub session_id: String,
    pub plugin_id: String,
    pub version: String,
    pub activation_id: String,
    pub observed: BrowserObservation,
    pub error: String,
}

/// Narrow approved-code delivery and browser lifecycle reporting; never administration.
#[tembra::contract(local, name = "swarmie.browser-access", version = 1)]
pub trait BrowserAccess {
    #[portable]
    async fn inventory(&self, request: BrowserInventoryRequest) -> Result<BrowserInventory>;

    #[portable]
    async fn active_package(&self, request: BrowserActivePackageRequest) -> Result<ActivePackage>;

    #[portable]
    async fn read_package_file(
        &self,
        request: BrowserReadPackageFileRequest,
    ) -> Result<ReadPackageFileResult>;

    #[portable]
    async fn report_browser(&self, request: BrowserReportRequest) -> Result<BrowserInventory>;
}

/// Narrow remote browser ingress; the embedding issues grants after transport authentication.
#[tembra::contract(
    remote,
    name = "swarmie.remote-browser-access",
    version = 1,
    host = "swarmie-runtime"
)]
pub trait RemoteBrowserAccess {
    async fn inventory(&self, request: BrowserInventoryRequest) -> Result<BrowserInventory>;

    async fn active_package(&self, request: BrowserActivePackageRequest) -> Result<ActivePackage>;

    async fn read_package_file(
        &self,
        request: BrowserReadPackageFileRequest,
    ) -> Result<ReadPackageFileResult>;

    async fn report_browser(&self, request: BrowserReportRequest) -> Result<BrowserInventory>;
}
