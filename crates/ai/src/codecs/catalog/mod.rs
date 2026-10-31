//! Captured encoder facts. Operational resolution remains in the shared catalog.
mod details;
mod spec;
pub use details::*;
pub use spec::*;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelFacts {
    spec: Option<std::sync::Arc<ModelSpec>>,
}

impl ModelFacts {
    pub fn new(spec: ModelSpec) -> Self {
        Self {
            spec: Some(std::sync::Arc::new(spec)),
        }
    }
    pub fn spec(&self) -> Option<&ModelSpec> {
        self.spec.as_deref()
    }
    pub fn for_model(&self, provider: &str, model: &str) -> Option<&ModelSpec> {
        self.spec()
            .filter(|spec| spec.id == model && spec.provider.vendor() == provider)
    }
    pub(crate) fn builtin() -> &'static Self {
        static EMPTY: ModelFacts = ModelFacts { spec: None };
        &EMPTY
    }
    pub(crate) fn reads_images_or(
        &self,
        provider: &str,
        model: &str,
        _rule: impl FnOnce(&str) -> bool,
    ) -> bool {
        self.for_model(provider, model)
            .is_some_and(|spec| spec.input.image)
    }
}
