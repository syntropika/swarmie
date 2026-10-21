//! Qualifies optional model-provider selection through the production compiled catalog.
use serde_json::json;
use swarmie_composition::Selection;
use swarmie_model_contracts::{
    ModelEndpoint, ModelIdentity, ModelProviderSchemaInput, ProviderRegistration, Support,
    model_providers,
};
use swarmie_runtime::{Application, Configuration, start};
use tembra_native::TokioDriver;

#[test]
fn application_profile_selects_the_compiled_provider_with_private_initialization() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(driver.run_until(async {
        let directory=tempfile::tempdir().unwrap();let source=directory.path().join("sources");std::fs::create_dir(&source).unwrap();
        let mut application=Application::default();
        for bundle in application.composition.bundles.values_mut() {
            for selection in &mut bundle.plugins {
                if selection.plugin==swarmie_memory_store::PLUGIN_ID {selection.config=json!({"database":directory.path().join("memory.sqlite")});}
            }
        }
        application.composition.profile.plugins.push(Selection {
            id:"models".into(),plugin:swarmie_model_provider_genai::PLUGIN_ID.into(),version:env!("CARGO_PKG_VERSION").into(),provider:None,
            config:json!({"connections":[{"id":"local","provider":"ollama","models":["configured-model"],"base_url":null,"credential_env":null}]}),
        });
        let runtime=start(driver.clone(),Configuration {database:directory.path().join("administration.sqlite"),source,application:Some(application)}).await.unwrap();
        assert!(runtime.deployment_errors.is_empty(),"{:?}",runtime.deployment_errors);
        let registry=runtime.host.core().registry(&model_providers().unwrap()).unwrap();
        let table=registry.table(None).unwrap();let registration:ProviderRegistration=serde_json::from_value(table.get(swarmie_model_provider_genai::PLUGIN_ID).unwrap().data().clone()).unwrap();
        let provider=runtime.host.get(ModelEndpoint::new(&registration.provider_contract).unwrap(),ModelProviderSchemaInput{}).await.unwrap();
        assert_eq!(provider.capabilities(ModelIdentity{connection:"local".into(),model:"configured-model".into()}).await.unwrap().streaming,Support::Supported);
        runtime.host.close().await.unwrap();assert!(registry.table(None).is_err());
    }));
}
