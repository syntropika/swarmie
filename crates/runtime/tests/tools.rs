use futures::{FutureExt, executor::LocalPool};
use std::{num::NonZeroU32, rc::Rc};
use swarmie_plugin_manager::{Catalog, CorePlugin, InstalledPlugin, PluginId};
use swarmie_runtime::Runtime;
use swarmie_tools::{
    self as tools, CatalogMethod, Selection, ToolDefinition, ToolName, ToolProvider, ToolSpec,
};
use tembra::Orb;

fn id(name: &str) -> PluginId {
    PluginId::new(name).unwrap()
}
fn version() -> NonZeroU32 {
    NonZeroU32::new(1).unwrap()
}
fn desired(name: &str, enabled: bool) -> InstalledPlugin {
    InstalledPlugin {
        id: id(name),
        version: version(),
        enabled,
    }
}
fn tool(name: &str) -> ToolDefinition {
    ToolDefinition::new(
        ToolSpec {
            name: ToolName::new(name).unwrap(),
            description: "Fixture".into(),
            input_schema: serde_json::json!({}),
        },
        ToolProvider::new("fixture.provider", vec!["main".into()]).unwrap(),
    )
    .unwrap()
}

#[test]
fn plugin_lifecycle_changes_tool_availability_without_unloading_native_orbs() {
    let directory = tempfile::tempdir().unwrap();
    let mut pool = LocalPool::new();
    let plugins = Catalog::new([
        CorePlugin::new(id("files"), version(), |_| {
            Ok(vec![Orb::new("fixture.files", 1)])
        })
        .contributes(tool("files.read")),
        CorePlugin::new(id("dependent"), version(), |_| {
            Ok(vec![Orb::new("fixture.dependent", 1)])
        })
        .requires([id("files")])
        .contributes(tool("files.extra")),
        CorePlugin::new(id("broken"), version(), |_| {
            Err(tembra::Error::invalid("fixture.failed", "mount failed"))
        })
        .contributes(tool("broken.read")),
        CorePlugin::new(id("duplicate"), version(), |_| {
            Ok(vec![Orb::new("fixture.duplicate", 1)])
        })
        .contributes(tool("files.read")),
        CorePlugin::new(id("tools"), version(), |ctx| {
            Ok(vec![tools::orb(
                ctx.host,
                tools::Catalog::from_plugins(ctx.index),
            )])
        }),
    ])
    .unwrap();
    let index = plugins.index();
    let executor = pool.spawner();
    let runtime = pool
        .run_until(Runtime::open(
            executor,
            swarmie_storage::sqlite::factory(directory.path().join("runtime.sqlite")),
            plugins,
        ))
        .unwrap();
    for name in ["files", "dependent", "broken", "tools"] {
        pool.run_until(runtime.configure(desired(name, true)))
            .unwrap();
    }
    let host = runtime.host().clone();
    let catalog = Rc::new(
        host.get(tools::SERVICE, vec![tools::INSTANCE_KEY.into()], "alice")
            .unwrap(),
    );
    let read = |pool: &mut LocalPool| -> Selection {
        let catalog = catalog.clone();
        pool.run_until(host.root("alice", move |ctx| {
            async move { catalog.call(&ctx, CatalogMethod::List.as_str(), ()).await }.boxed_local()
        }))
        .unwrap()
    };
    assert_eq!(read(&mut pool).tools.len(), 2);
    assert_eq!(index.contributions::<ToolDefinition>().len(), 2);
    let changed = pool
        .run_until(runtime.configure(desired("files", false)))
        .unwrap();
    assert!(
        changed
            .plugins
            .iter()
            .find(|p| p.id == id("files"))
            .unwrap()
            .restart_required
    );
    assert!(read(&mut pool).tools.is_empty());
    // Physical services and the observed metadata remain until host restart.
    assert_eq!(index.contributions::<ToolDefinition>().len(), 2);
    assert!(index.active_contributions::<ToolDefinition>().is_empty());
    let consumer = catalog.clone();
    pool.run_until(host.root("alice", move |ctx| {
        async move {
            let definition = tool("files.read");
            let call = tools::ToolCall {
                id: tools::ToolCallId::new("disabled-call")?,
                name: definition.spec.name.clone(),
                arguments: serde_json::json!({}),
            };
            let prepared: tools::ToolPreparation = consumer
                .call(
                    &ctx,
                    CatalogMethod::Prepare.as_str(),
                    (definition.spec, call.clone()),
                )
                .await?;
            assert!(matches!(prepared.decision, tools::ToolDecision::Block(_)));
            let result: tools::ToolResult = consumer
                .call(
                    &ctx,
                    CatalogMethod::Execute.as_str(),
                    (tools::OperationId::new(), call, definition.provider),
                )
                .await?;
            assert!(matches!(result, tools::ToolResult::Blocked(_)));
            Ok(())
        }
        .boxed_local()
    }))
    .unwrap();
    pool.run_until(runtime.configure(desired("files", true)))
        .unwrap();
    assert_eq!(read(&mut pool).tools.len(), 2);
    let mut changed = desired("files", true);
    changed.version = NonZeroU32::new(2).unwrap();
    pool.run_until(runtime.configure(changed)).unwrap();
    assert!(read(&mut pool).tools.is_empty());
    pool.run_until(runtime.configure(desired("files", true)))
        .unwrap();
    pool.run_until(runtime.configure(desired("duplicate", true)))
        .unwrap();
    let consumer = catalog.clone();
    pool.run_until(host.root("alice", move |ctx| {
        async move {
            let definition = tool("files.read");
            let call = tools::ToolCall {
                id: tools::ToolCallId::new("conflicting-call")?,
                name: definition.spec.name,
                arguments: serde_json::json!({}),
            };
            let result: tools::ToolResult = consumer
                .call(
                    &ctx,
                    CatalogMethod::Execute.as_str(),
                    (tools::OperationId::new(), call, definition.provider),
                )
                .await?;
            assert!(matches!(result, tools::ToolResult::Blocked(_)));
            Ok(())
        }
        .boxed_local()
    }))
    .unwrap();
    pool.run_until(runtime.close()).unwrap();
    let d = host.kernel().diagnostics();
    assert_eq!((d.calls, d.resources), (0, 0));
}
