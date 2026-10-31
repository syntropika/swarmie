# Plugin Manager

Plugin Manager applies a durable desired inventory to compiled orb factories.
Tembra owns service lookup, object activation, references, and resource cleanup.
The manager does not participate in ordinary service calls.

Capability crates remain usable without Plugin Manager. An application can install
their orbs directly, or register factories for administrative control.

## Three views

| View | Contents |
| --- | --- |
| `Catalog` | Compiled factories, versions, dependencies, and contribution metadata available to this binary. |
| `Inventory` | Desired plugin versions and enabled flags, with a storage revision. |
| `PluginSnapshot` | Observed installation status and whether a host restart is required. |

A catalog entry does not grant access to a service. Tembra still checks the
principal, address, and service contract when callers acquire references.

## Composition

```rust
use std::num::NonZeroU32;
use swarmie_plugin_manager::{Catalog, CorePlugin, PluginId};

fn catalog(counter: fn() -> tembra::Orb) -> Result<Catalog, Box<dyn std::error::Error>> {
    let plugin = CorePlugin::new(
        PluginId::new("example.counter")?,
        NonZeroU32::new(1).unwrap(),
        move |_context| Ok(vec![counter()]),
    );
    Ok(Catalog::new([plugin])?)
}
```

`counter` is the application's independent Orb factory. `CorePlugin::requires`
sets plugin startup dependencies. `CorePlugin::contributes` adds typed metadata.
The factory receives the existing Tembra host and the shared contribution index.
It declares orbs; managed resources perform I/O during activation.

`Catalog::require` selects mandatory plugins and their dependency closure. It
rejects missing compiled definitions. Configure this policy before obtaining the
index or opening a runtime. A catalog and its index belong to one host composition.
The manager atomically seeds these records only when the authoritative inventory
has revision zero and no records. Existing invalid, missing, disabled or incompatible
required records fail startup without repair. An uncertain seed freezes the storage
owner. Required mount failure aborts startup; the runtime closes the partial host.

Install the manager orb with this catalog and a `StorageFactory`. Acquiring its
fixed host instance activates inventory storage and then startup. Inventory read
failure fails startup. It must not be treated as an empty inventory.

## Reconciliation

The manager validates unique inventory identities. It then installs enabled,
available plugins whose dependencies are mounted and enabled. It repeats this
step until no pending plugin can start.

A missing definition, version mismatch, empty factory result, or installation
failure is recorded for that plugin. Other eligible plugins can still mount.
Unresolved dependencies remain blocked and name the missing prerequisites.

Mounted means that orb declarations are installed. It does not mean every service
object has been activated. Tembra owns later activation and resource failures.

Native orbs remain installed for the host lifetime. Disabling, replacing, or
removing an installed plugin marks it for restart. The snapshot reports both the
desired state and what remains physically installed.

## Administration

The service is `swarmie.plugin_manager`. Its principal is `host` and its instance
key is `main`. Each method checks the calling context and principal.

| `ManagerMethod` | Effect |
| --- | --- |
| `List` | Returns an owned snapshot. |
| `Configure` | Commits one desired plugin record, then reconciles installation. |
| `Refresh` | Reloads authoritative inventory, then reconciles installation. |

Inventory commits use the last observed revision. A conflict requires a refresh
before retry and invalidates cached eligibility until confirmed reconciliation.
An unknown commit outcome freezes the storage owner and requires
recovery. Concurrent manager mutations are refused while a mutation is settling.
Mount failure never overwrites the desired inventory record.
Configuration cannot disable or change the version of a required plugin or its
required dependencies. Refresh refuses invalid required state and invalidates
cached eligibility. It does not repair the external inventory.

## Contribution index

`Catalog::index` returns the shared `PluginIndex`.

- `contributions<T>` returns metadata from all physically installed plugins.
- `active_contributions<T>` returns metadata eligible for new work. Disabled or
  version-mismatched plugins, and dependents that require them, are excluded.
- `changes` returns local membership invalidations. Consumers read the index again
  after a notification.
- `required_active` returns cached required-plugin eligibility without inventory
  I/O. Native physical execution and tool admission check this value. It does not
  prove that an individual service object has activated successfully.

The manager subscribes to its storage owner's health changes. Recovery-required
or closed state invalidates eligibility synchronously, including changes caused
by another capability sharing that owner. The manager detaches the subscription
before storage cleanup. A failed owner check cannot leave new work eligible.

The index contains metadata, not service references or authority. Consumers must
still acquire and validate providers through Tembra. Installation and index
publication do not introduce another service registry.
