use std::{cell::Cell, rc::Rc};

use swarmie_plugin_contracts::{
    CAPABILITY_REGISTRY, Counter, CounterInput, RemoteCounter, RemoteCounterInput,
};
use tembra::{core, prelude::*};

#[test]
fn native_and_registry_calls_publish_the_same_state_and_refuse_overflow() -> Result<()> {
    let driver = core::ManualDriver::new();
    let host = Host::with_driver("swarmie-runtime", "supervisor", driver.clone())?;
    driver.run_until(async move {
        let registry = core::Registry::new(CAPABILITY_REGISTRY, core::ConflictPolicy::Error)?;
        let registry = host.core().registry(&registry)?;
        host.install(
            swarmie_counter::facet_with_initial(u32::MAX - 2)?,
            core::LoadOptions::default(),
        )?
        .await?;
        let counter = host.get(Counter, CounterInput {}).await?;
        let latest = Rc::new(Cell::new(0));
        let observer = latest.clone();
        let _subscription = counter.count().subscribe(move |snapshot| {
            observer.set(snapshot.value()?);
            Ok(())
        })?;
        assert_eq!(latest.get(), u32::MAX - 2);
        assert_eq!(counter.increment(0).await?, u32::MAX - 2);
        assert_eq!(counter.increment(1).await?, u32::MAX - 1);

        let operation = registry
            .table(None)?
            .get(swarmie_counter::PLUGIN_ID)
            .expect("host-lifetime registry contribution")
            .operation()
            .unwrap()
            .clone();
        host.root(move |context| async move {
            assert_eq!(
                operation.call_json(context.core(), Some(1.into())).await?,
                Some(u32::MAX.into())
            );
            Ok(())
        })
        .await?;
        assert_eq!(latest.get(), u32::MAX);
        let failure = counter.increment(1).await.unwrap_err();
        assert_eq!(failure.reason, "counter_overflow");
        assert_eq!(counter.read().await?, u32::MAX);
        assert_eq!(latest.get(), u32::MAX);

        host.close().await?;
        assert!(counter.read().await.is_err());
        assert!(registry.table(None).is_err());
        Ok(())
    })
}

#[test]
fn remote_facade_forwards_native_reference_and_state() -> Result<()> {
    use core::rpc::*;

    let driver = core::ManualDriver::new();
    let server = Host::with_driver("swarmie-runtime", "supervisor", driver.clone())?;
    let client = Host::with_driver("counter-client", "browser-principal", driver.clone())?;
    let transport_driver = driver.clone();
    driver.run_until(async move {
        server
            .install(swarmie_counter::facet()?, core::LoadOptions::default())?
            .await?;
        let local = server.get(Counter, CounterInput {}).await?;
        server.core().set_rpc_identity(RootOrigin::new(
            "swarmie-runtime",
            "11111111111111111111111111111111",
        )?)?;
        client.core().set_rpc_identity(RootOrigin::new(
            "counter-client",
            "22222222222222222222222222222222",
        )?)?;
        let (left, right, _) = memory_link(transport_driver, MemoryLinkOptions::default())?;
        let outgoing =
            client
                .core()
                .connect_client(left, "swarmie-runtime", ConnectionOptions::default())?;
        let _incoming = server.core().accept_client(
            right,
            "authenticated-client".into(),
            ConnectionOptions::default(),
        )?;
        client.core().set_route("swarmie-runtime", outgoing)?;

        let remote = client.get(RemoteCounter, RemoteCounterInput {}).await?;
        assert_eq!(local.increment(2).await?, 2);
        assert_eq!(remote.read().await?, 2);
        let latest = Rc::new(Cell::new(0));
        let observer = latest.clone();
        let _subscription = remote.count().subscribe(move |snapshot| {
            observer.set(snapshot.value()?);
            Ok(())
        })?;
        assert_eq!(latest.get(), 2);
        assert_eq!(remote.increment(3).await?, 5);
        assert_eq!(remote.count().value()?, 5);
        assert_eq!(local.count().value()?, 5);
        assert_eq!(latest.get(), 5);
        client.close().await?;
        server.close().await?;
        Ok(())
    })
}
