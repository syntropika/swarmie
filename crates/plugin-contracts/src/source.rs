use tembra::prelude::*;

use crate::PluginManifest;

/// Source-provider operation names used in registry metadata and checked schemas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum SourceOperation {
    Search,
    Resolve,
    Inspect,
    Acquire,
}

/// Metadata identifying an operation without selecting a vendor-specific implementation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct SourceRegistryMetadata {
    pub provider_id: String,
    pub operation: SourceOperation,
}

/// A provider search query.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct SearchRequest {
    pub query: String,
}

/// Search results without transferred artifacts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct SearchResult {
    pub plugins: Vec<PluginSummary>,
}

/// One discoverable plugin release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct PluginSummary {
    pub id: String,
    pub version: String,
    pub description: String,
}

/// An exact release lookup or provider-supported version requirement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ResolveRequest {
    pub plugin_id: String,
    pub version: String,
}

/// A release resolved to a concrete immutable manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ResolvedPlugin {
    pub manifest: PluginManifest,
}

/// An exact release whose manifest is requested without artifact transfer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct InspectRequest {
    pub plugin_id: String,
    pub version: String,
}

/// An exact release to acquire from its owning source provider.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct AcquireRequest {
    pub plugin_id: String,
    pub version: String,
}

/// A transferred artifact; its path and digest still require admission checks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct PackageFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// Provider-issued acquisition evidence, retained with durable administrative records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct PackageProvenance {
    pub provider_id: String,
    pub source: String,
    pub integrity: String,
}

/// Complete acquisition result before the manager validates and stages any file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct AcquiredPackage {
    pub manifest: PluginManifest,
    pub files: Vec<PackageFile>,
    pub provenance: PackageProvenance,
}

/// A canonical schema source. Concrete providers declare their own unique contract names.
#[tembra::contract(local, name = "swarmie.plugin-source-schema", version = 1)]
pub trait PluginSourceSchema {
    #[portable]
    async fn search(&self, request: SearchRequest) -> Result<SearchResult>;

    #[portable]
    async fn resolve(&self, request: ResolveRequest) -> Result<ResolvedPlugin>;

    #[portable]
    async fn inspect(&self, request: InspectRequest) -> Result<PluginManifest>;

    #[portable]
    async fn acquire(&self, request: AcquireRequest) -> Result<AcquiredPackage>;
}
