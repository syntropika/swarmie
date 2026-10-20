//! Actual RPC admission checks for the boundary between trusted code and external clients.

use swarmie_plugin_contracts::{
    ConfigureProviderRequest, Inventory, PluginAdministration, PluginAdministrationInput,
    RemoteBrowserAccess, RemoteBrowserAccessInput,
};
use swarmie_runtime::{Configuration, start};
use tembra::{Host, core};
use tembra_native::TokioDriver;

// An external caller can construct its own descriptors. A remote descriptor with the local
// service's name must not change the placement of the installed administrative Contract.
#[tembra::contract(
    remote,
    name = "swarmie.plugin-administration",
    version = 1,
    host = "swarmie-runtime"
)]
trait ForgedAdministration {
    async fn configure_provider(&self, request: ConfigureProviderRequest) -> Result<Inventory>;
}

#[tembra::contract(
    remote,
    name = "swarmie.remote-plugin-administration",
    version = 1,
    host = "swarmie-runtime"
)]
trait RemovedAdministration {
    async fn configure_provider(&self, request: ConfigureProviderRequest) -> Result<Inventory>;
}

#[test]
fn anonymous_rpc_cannot_acquire_administration_but_local_trusted_code_can_mutate() {
    use core::rpc::*;

    let directory = tempfile::tempdir().unwrap();
    let driver = TokioDriver::new();
    let scheduler = driver.clone();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(scheduler.run_until(async {
            let runtime = start(
                driver.clone(),
                Configuration {
                    database: directory.path().join("administration.sqlite"),
                    source: directory.path().join("source"),
                    application: None,
                },
            )
            .await
            .unwrap();
            let local = runtime
                .host
                .get(PluginAdministration, PluginAdministrationInput {})
                .await
                .unwrap();
            let configuration = serde_json::json!({"enabled":true});
            local
                .configure_provider(ConfigureProviderRequest {
                    provider_id: "local".into(),
                    configuration: configuration.clone(),
                })
                .await
                .unwrap();
            assert_eq!(
                runtime.manager.provider_configuration("local").unwrap(),
                Some(configuration.clone())
            );

            let client = Host::with_driver("external-client", "anonymous", driver.clone()).unwrap();
            runtime
                .host
                .core()
                .set_rpc_identity(
                    RootOrigin::new("swarmie-runtime", "11111111111111111111111111111111").unwrap(),
                )
                .unwrap();
            client
                .core()
                .set_rpc_identity(
                    RootOrigin::new("external-client", "22222222222222222222222222222222").unwrap(),
                )
                .unwrap();
            let (left, right, _) = memory_link(driver, MemoryLinkOptions::default()).unwrap();
            let outgoing = client
                .core()
                .connect_client(left, "swarmie-runtime", ConnectionOptions::default())
                .unwrap();
            let _incoming = runtime
                .host
                .core()
                .accept_client(right, "anonymous".into(), ConnectionOptions::default())
                .unwrap();
            client
                .core()
                .set_route("swarmie-runtime", outgoing)
                .unwrap();

            let browser = client
                .get(RemoteBrowserAccess, RemoteBrowserAccessInput {})
                .await
                .unwrap();
            assert!(browser.inventory().await.unwrap().is_empty());
            assert!(
                client
                    .get(PluginAdministration, PluginAdministrationInput {})
                    .await
                    .is_err()
            );
            assert!(
                client
                    .get(ForgedAdministration, ForgedAdministrationInput {})
                    .await
                    .is_err()
            );
            assert!(
                client
                    .get(RemovedAdministration, RemovedAdministrationInput {})
                    .await
                    .is_err()
            );
            assert_eq!(
                runtime.manager.provider_configuration("local").unwrap(),
                Some(configuration.clone())
            );
            client.close().await.unwrap();
            runtime.host.close().await.unwrap();
        }));
}
