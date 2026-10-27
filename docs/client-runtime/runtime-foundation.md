# Runtime

The runtime is an embeddable Rust library. It creates a Tembra host and starts
PluginManager. The embedding supplies its local executor, storage and compiled
plugin catalog.

## Open and use

```rust
use futures::executor::LocalPool;
use swarmie_plugin_manager::Catalog;
use swarmie_runtime::Runtime;

let directory = tempfile::tempdir().unwrap();
let mut pool = LocalPool::new();
let spawner = pool.spawner();
pool.run_until(async {
    let runtime = Runtime::open(
        spawner,
        swarmie_storage_sqlite::factory(directory.path().join("runtime.sqlite")),
        Catalog::default(),
    ).await.unwrap();

    assert!(runtime.plugins().await.unwrap().plugins.is_empty());
    runtime.close().await.unwrap();
});
```

The empty catalog and inventory start only PluginManager. `Runtime::configure`
stores desired plugin state before reconciliation. `Runtime::refresh` reloads it.
`Runtime::plugins` returns the last confirmed inventory and observed mount state.
See [PluginManager](plugin-manager.md).

`Runtime::host` exposes the normal Tembra host. Capability calls use Tembra
references directly. PluginManager is outside their invocation path. An independent
capability can also be composed in a plain Tembra host without this runtime.

## Ownership and shutdown

Tembra owns service instances, resources, calls, subscriptions and physical
cleanup. The runtime does not create a CLI, listener or executor thread. It does
not choose storage paths or delete existing files.

Keep singleton references alive across calls. Releasing the last reference can
start disposal; immediate reacquisition may fail while disposal is in progress.
`Runtime::close` releases its manager reference and closes the host. Keep driving
the executor until it returns. Dropping the close future does not prove shutdown.

A failed manager acquisition or startup closes the partial host. A plugin mount
failure is reported in its snapshot while independent plugins can still mount.

## Product composition

Swarmie agent executions use the durable conversation adapter. Independent Agent
Loop library consumers may use ordinary execution without storage. The generic
runtime takes the embedding's catalog; it does not install product capabilities
on its own.

[Durable Tasks](durable-tasks.md) owns assignments and checkpoints.
[Conversations](conversations.md) owns persistent branches and history.
[Agent Loop](agent-loop.md) coordinates model and tool work through these optional
adapters. Compatible history writes and checkpoints share one storage owner commit.
Final task completion can be confirmed later by the scheduler.

External model and tool providers own effect recognition and reconciliation.
Unknown outcomes must not trigger automatic replay. Cancellation does not undo
an external effect.

## Optional node access

The `rpc` feature re-exports `tembra::rpc` directly. The embedding owns transport,
authentication and byte queues. `node_orb` supplies read-only node identity when
explicitly composed. See [Node connections](node-connections.md).

Rust dependencies and their exact source are recorded in the root Cargo manifest
and lockfile. Browser dependencies have their own root Bun resolution. Changing
one does not upgrade the other.

## Current limits

The native runtime installs compiled Orbs. It does not load JavaScript plugins.
The current hook and task adapters are native Rust interfaces. Cross-language
authoring is outside their qualified scope. No checkpoint migration system is
provided. Process-crash recovery is tested with synthetic providers; arbitrary
production effects still require provider-specific reconciliation.
