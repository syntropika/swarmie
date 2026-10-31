pub const PROVIDER_NAME: &str = "cohere";
pub(crate) fn reads_images(model: &str) -> bool {
    model.contains("vision")
}
pub(crate) fn thinks(facts: &crate::codecs::catalog::ModelFacts, model: &str) -> Option<bool> {
    facts
        .for_model(PROVIDER_NAME, model)
        .map(|spec| spec.reasoning.supported())
}
