use tembra::prelude::*;

/// The plugin metadata consumed by administration and generated browser bindings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct PluginManifest {
    pub schema_version: u32,
    pub id: String,
    pub version: String,
    pub compatibility: Compatibility,
    pub dependencies: Vec<PluginDependency>,
    pub facets: Vec<PluginFacet>,
    pub files: Vec<ManifestFile>,
    pub distribution: Distribution,
    pub ui: Vec<UiContribution>,
}

/// Runtime and dependency requirements, checked before admitting plugin artifacts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct Compatibility {
    pub swarmie: String,
    pub tembra_revision: String,
}

/// A required plugin version range.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct PluginDependency {
    pub id: String,
    pub version: String,
}

/// Supported JavaScript execution locations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum FacetTarget {
    Server,
    Browser,
}

/// Supported Tembra loading mechanisms; native browser ESM is not embedded server code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum FacetFormat {
    #[default]
    Bundled,
    NativeEsm,
}

impl FacetFormat {
    fn is_bundled(&self) -> bool {
        *self == Self::Bundled
    }
}

/// A verified Tembra artifact and its selected module export.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct PluginFacet {
    pub id: String,
    pub target: FacetTarget,
    #[serde(default, skip_serializing_if = "FacetFormat::is_bundled")]
    pub format: FacetFormat,
    pub artifact_path: String,
    pub sha256: String,
    pub selected_export: String,
}

/// An integrity-pinned file relative to the acquired package root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ManifestFile {
    pub path: String,
    pub sha256: String,
}

/// Human-readable distribution metadata; this does not select a manager implementation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct Distribution {
    pub provider: String,
    pub description: String,
}

/// Supported browser contribution categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum UiContributionKind {
    Slot,
    Widget,
    Command,
    Panel,
    Screen,
    Renderer,
    Configuration,
}

/// A lifecycle-owned browser module export, resolved by identity rather than serialized code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct UiContribution {
    pub id: String,
    pub plugin_id: String,
    pub kind: UiContributionKind,
    pub label: String,
    pub slot_id: String,
    pub order: i32,
    pub module_path: String,
    pub export_name: String,
}

/// Metadata attached to each live UI registry entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct UiRegistryMetadata {
    pub plugin_id: String,
    pub contribution: UiContribution,
}
