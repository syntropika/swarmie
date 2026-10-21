//! Opt-in network qualification. Never selected by ordinary test or benchmark recipes.
use std::time::Duration;
use swarmie_model_contracts::*;
use swarmie_model_provider_genai::{
    Configuration, Connection, GENERATION_CONTRACT, Limits, PROVIDER_CONTRACT, facet,
};
use tembra::{Host, core};
use tembra_native::TokioDriver;

#[test]
#[ignore = "requires explicitly configured live model; performs two potentially billable inferences"]
fn live_chat_and_streaming() {
    let provider = std::env::var("SWARMIE_LIVE_PROVIDER").expect("set SWARMIE_LIVE_PROVIDER");
    let model = std::env::var("SWARMIE_LIVE_MODEL").expect("set SWARMIE_LIVE_MODEL");
    let config = Configuration {
        connections: vec![Connection {
            id: "live".into(),
            provider,
            models: vec![model.clone()],
            base_url: std::env::var("SWARMIE_LIVE_BASE_URL").ok(),
            credential_env: std::env::var("SWARMIE_LIVE_CREDENTIAL_ENV").ok(),
            timeout_ms: 120_000,
            max_concurrent: 1,
            defaults: GenerationOptions {
                max_output_tokens: Some(128),
                ..Default::default()
            },
        }],
        limits: Limits::default(),
    };
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let host = Host::with_driver("live-models", "test", driver.clone()).unwrap();
            host.install(
                facet(&config, driver.clone()).unwrap(),
                core::LoadOptions::default(),
            )
            .unwrap()
            .await
            .unwrap();
            let make = |id: &str| ModelRequest {
                request_id: id.into(),
                model: ModelIdentity {
                    connection: "live".into(),
                    model: model.clone(),
                },
                messages: vec![Message::text(Role::User, "Reply with a short greeting.")],
                tools: vec![],
                options: Default::default(),
            };
            let p = host
                .get(
                    ModelEndpoint::new(PROVIDER_CONTRACT).unwrap(),
                    ModelProviderSchemaInput {},
                )
                .await
                .unwrap();
            assert!(matches!(
                p.generate(make("live-chat")).await.unwrap(),
                GenerateResult::Response { .. }
            ));
            let g = host
                .get(
                    GenerationEndpoint::new(GENERATION_CONTRACT).unwrap(),
                    GenerationInput {
                        request: make("live-stream"),
                    },
                )
                .await
                .unwrap();
            g.start().await.unwrap();
            loop {
                let state = g.progress().value().unwrap();
                g.acknowledge(state.last_sequence).await.unwrap();
                if state.physically_settled {
                    assert!(state.error.is_none());
                    assert!(state.response.is_some());
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            host.close().await.unwrap();
        }));
}
