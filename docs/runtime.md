# Runtime

The runtime is an embeddable Rust library. It creates a Tembra host and starts
PluginManager. The embedding supplies its local executor, storage and compiled
plugin catalog.

## Storage selection

The runtime lives in `crates/runtime/`. Storage is an independent package in
`crates/storage/`; it enables its `sqlite` feature by default and exposes the backend as
`swarmie_storage::sqlite`. The embedding supplies the file path and factory. Enabling
the backend does not open a database or install more Orbs.

Disable Storage's default features to use only its backend-neutral API. Capabilities
share the small internal `crates/storage/core/` package, so they can be used without the
runtime or concrete backend. Run `just storage-check` to qualify that boundary.

## Open and use

```rust
use futures::executor::LocalPool;
use swarmie_plugin_manager::Catalog;
use swarmie_runtime::Runtime;
use swarmie_storage::sqlite;

let directory = tempfile::tempdir().unwrap();
let mut pool = LocalPool::new();
let spawner = pool.spawner();
pool.run_until(async {
    let runtime = Runtime::open(
        spawner,
        sqlite::factory(directory.path().join("runtime.sqlite")),
        Catalog::default(),
    ).await.unwrap();

    assert!(runtime.plugins().await.unwrap().plugins.is_empty());
    runtime.close().await.unwrap();
});
```

The empty catalog and inventory start only PluginManager. `Runtime::configure`
stores desired plugin state before reconciliation. `Runtime::refresh` reloads it.
`Runtime::plugins` returns the last confirmed inventory and observed mount state.
See [PluginManager](plugins/plugin-manager.md).

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

A failed manager acquisition or startup closes the partial host. An optional plugin
mount failure is reported in its snapshot. A required plugin mount failure aborts
startup and closes the partial host.

## Product composition

Swarmie agent executions use the durable conversation adapter. Independent Agent
Loop library consumers may use ordinary execution without storage. The generic
runtime takes the embedding's catalog; it does not install product capabilities
on its own.

[Durable Tasks](plugins/durable-tasks.md) owns assignments and checkpoints.
[Conversations](plugins/conversations.md) owns persistent branches and history.
[Agent Loop](plugins/agent-loop.md) coordinates model and tool work through these optional
adapters. Compatible history writes and checkpoints share one storage owner commit.
Final task completion can be confirmed later by the scheduler.

External model and tool providers own effect recognition and reconciliation.
Unknown outcomes must not trigger automatic replay. Cancellation does not undo
an external effect.

## Optional node access

The `rpc` feature re-exports `tembra::rpc` directly. The embedding owns transport,
authentication and byte queues. `node_plugin` registers the read-only identity
service with PluginManager. The embedding supplies the stable `NodeId`.

```rust
use swarmie_plugin_manager::{Catalog, PluginId};
use swarmie_runtime::{NodeId, NODE_SERVICE, node_plugin};

let catalog = Catalog::new([
    node_plugin(NodeId::new("configured-node").unwrap()),
]).unwrap().require([PluginId::new(NODE_SERVICE).unwrap()]).unwrap();
```

`Catalog::require` selects mandatory plugins and their dependencies. Configure
this policy before opening the runtime or obtaining its contribution index.
Only a confirmed revision-zero empty inventory is initialized automatically.
An existing missing, disabled or incompatible required record fails startup
without replacing that inventory. An uncertain initial write requires recovery.

The native host uses `Runtime::open` for the same startup path. Its catalog contains
the node identity plugin and a required native services plugin. The latter declares
the private operator, tool, provider and conversation services on the runtime's
single Host. It does not create another runtime or change provider protocols.
Plugin inventory uses `runtime.sqlite` in the host-selected profile. The embedding
retains its profile lock until runtime resources and physical work have closed.
Standalone hosting uses the same native host library.

Native factories declare storage without opening files. Scheduler, history and
session resources acquire leases during managed activation. Conversation and task
leases share one SQLite owner, so joint admission remains atomic. Session cleanup
releases both leases after its physical invocations settle. Other active leases
keep the shared connection open until the final managed consumer closes.

`node_orb` remains available for independently composed Tembra hosts. See
[Node connections](client-runtime/node-connections.md).

Rust dependencies and their exact source are recorded in the root Cargo manifest
and lockfile. Browser dependencies have their own root Bun resolution. Changing
one does not upgrade the other.

## Current limits

The native runtime installs compiled Orbs. It does not load JavaScript plugins.
The current hook and task adapters are native Rust interfaces. Cross-language
authoring is outside their qualified scope. No checkpoint migration system is
provided. Process-crash recovery is tested with synthetic providers; arbitrary
production effects still require provider-specific reconciliation.


## Directory navigation

`directories_orb` composes the optional `swarmie.directories` service with key `main`.
The bare runtime does not install it. The embedding supplies a `DirectorySource` for I/O
and checks the authenticated principal on each read. Tembra owns the service lifetime.

The `list` method accepts `DirectoryRequest`. A null path opens the configured root.
A null cursor requests the first page. The result is a `DirectoryPage` with the current
path, permitted parent, directory entries and an optional next cursor. Clients pass the
returned paths and cursors back to the same node. They must not infer file permissions
from a directory result. The service checks cancellation before and after the read.
Selecting a directory does not grant access to its files or change project membership.
