use thiserror::Error;

/// Structural refusal. Resolution never mutates input, artifacts or runtime state.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum CompositionError {
    #[error("unsupported composition schema version {0}")]
    SchemaVersion(u32),
    #[error("invalid {field}: {reason}")]
    Invalid { field: String, reason: String },
    #[error("bundle {0} does not exist")]
    UnknownBundle(String),
    #[error("bundle cycle: {0:?}")]
    BundleCycle(Vec<String>),
    #[error("instance {0} is selected more than once; use an explicit override")]
    DuplicateInstance(String),
    #[error("override targets unknown instance {0}")]
    UnknownOverride(String),
    #[error("instance {0} is overridden more than once")]
    DuplicateOverride(String),
    #[error("disable targets unknown instance {0}")]
    UnknownDisable(String),
    #[error("instance {0} is disabled more than once")]
    DuplicateDisable(String),
    #[error("release catalog repeats {plugin}@{version}")]
    DuplicateRelease { plugin: String, version: String },
    #[error("fixed package {plugin} is selected by both {first} and {second}")]
    PackageCollision {
        plugin: String,
        first: String,
        second: String,
    },
    #[error("facet {facet} on {target} is selected by both {first} and {second}")]
    FacetCollision {
        target: String,
        facet: String,
        first: String,
        second: String,
    },
}
