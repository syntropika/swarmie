use tembra::prelude::*;

use crate::{
    ActivePackage, ActivePackageRequest, BrowserSessionRecord, ReadPackageFileRequest,
    ReadPackageFileResult, ReportBrowserRequest,
};

/// Current approved browser packages and observations for one locally admitted session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct BrowserInventory {
    pub session_id: String,
    pub packages: Vec<ActivePackage>,
    pub observations: Vec<BrowserSessionRecord>,
}

/// The trusted embedding supplies the identity of its admitted browser session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct BrowserInventoryRequest {
    pub session_id: String,
}

/// Local browser lifecycle coordination for trusted in-Host callers.
#[tembra::contract(local, name = "swarmie.browser-access", version = 1)]
pub trait BrowserAccess {
    #[portable]
    async fn inventory(&self, request: BrowserInventoryRequest) -> Result<BrowserInventory>;

    #[portable]
    async fn active_package(&self, request: ActivePackageRequest) -> Result<ActivePackage>;

    #[portable]
    async fn read_package_file(
        &self,
        request: ReadPackageFileRequest,
    ) -> Result<ReadPackageFileResult>;

    #[portable]
    async fn report_browser(&self, request: ReportBrowserRequest) -> Result<BrowserInventory>;
}

/// Public approved-code delivery for the development transport. It exposes no mutation,
/// administrative inventory or browser-session reporting. Connection authentication belongs
/// to the transport embedding; activation identities only reject stale package versions.
#[tembra::contract(
    remote,
    name = "swarmie.remote-browser-access",
    version = 1,
    host = "swarmie-runtime"
)]
pub trait RemoteBrowserAccess {
    async fn inventory(&self) -> Result<Vec<ActivePackage>>;

    async fn active_package(&self, request: ActivePackageRequest) -> Result<ActivePackage>;

    async fn read_package_file(
        &self,
        request: ReadPackageFileRequest,
    ) -> Result<ReadPackageFileResult>;
}
