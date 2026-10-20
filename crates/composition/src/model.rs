use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use swarmie_plugin_contracts::{FacetTarget, PluginKind};

/// Versioned application input. Bundle order and explicit overrides determine selection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    pub schema_version: u32,
    pub profile: Profile,
    #[serde(default)]
    pub bundles: BTreeMap<String, Bundle>,
}

/// One named application selection. Overrides replace a complete existing selection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    #[serde(default)]
    pub bundles: Vec<String>,
    #[serde(default)]
    pub plugins: Vec<Selection>,
    #[serde(default)]
    pub overrides: Vec<Selection>,
    #[serde(default)]
    pub disable: Vec<String>,
}

/// Reusable deployment metadata. A bundle does not own an execution scope.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    #[serde(default)]
    pub bundles: Vec<String>,
    #[serde(default)]
    pub plugins: Vec<Selection>,
}

fn empty_config() -> Value {
    json!({})
}

/// Configured instance with an exact package version and explicit source selection.
///
/// Configuration remains caller-owned data; the implementation adapter validates its typed
/// schema before runtime preparation. Dynamic selections name a source provider; Core
/// selections omit it. Fixed artifact Facet identities cannot be remapped by composition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub id: String,
    pub plugin: String,
    pub version: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default = "empty_config")]
    pub config: Value,
}

/// Fixed artifact identity within one execution target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FacetIdentity {
    pub id: String,
    pub target: FacetTarget,
}

/// Trusted release facts extracted by compiled adapters or verified artifact admission.
///
/// These are catalog records, not another package manifest. The caller must establish their
/// provenance and agreement with the executable artifact before calling the resolver.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub plugin: String,
    pub version: String,
    pub kind: PluginKind,
    pub facets: Vec<FacetIdentity>,
}

/// Winning selection layer. Bundle identity remains visible after flattening.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Origin {
    Bundle { id: String },
    Profile,
    Override,
}

/// Desired selection eligibility only. Tembra owns actual compatibility and readiness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SelectionState {
    Enabled,
    Disabled,
    UnavailableRelease { plugin: String, version: String },
}

/// Inspectable resolved instance with its original configuration and trusted release facts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveInstance {
    pub selection: Selection,
    pub origin: Origin,
    pub release: Option<Release>,
    pub state: SelectionState,
}

/// Reproducible desired deployment in declaration order, without runtime dependency claims.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveComposition {
    pub profile: String,
    pub instances: BTreeMap<String, EffectiveInstance>,
    pub deployment_order: Vec<String>,
}
