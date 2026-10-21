# Developing Swarmie

This guide covers local setup, development commands, extension points and repository structure.
For an introduction to the project, see [README.md](README.md).

## Architecture

The runtime bootstraps Tembra and plugin administration. Application capabilities live in
independently composable plugins:

- **Plugin Manager** handles installation, activation, recovery and administrative persistence.
- **MemoryStore** provides namespaced persistent JSON storage.
- **Durable Tasks** handles scheduling, checkpoints, recovery, ownership and cancellation.
- **Task Storage SQLite** implements transactional task persistence behind the TaskStorage Contract.
- **Application plugins** contribute their own capabilities and registered task handlers.

Core Plugins are Rust Facets compiled with Swarmie. Dynamic Plugins are trusted TypeScript or
JavaScript artifacts loaded through Tembra. Tembra owns their execution, dependency readiness,
Resources and compatible JavaScript replacement.

Durable Tasks separates execution from persistence. TaskStorage commits are authoritative;
reactive State exposes committed progress. Checkpoints preserve resumable state, and atomic
transactions can update a parent while creating its children. Applications use stable operation
identities and idempotency to handle external effects that may repeat after interruption.

### Task storage and transport limits

TaskStorage bounds complete records to 768 KiB and complete portable pages, transactions,
receipts and handler invocations to 1 MiB minus 8 KiB. These are canonical UTF-8 JSON byte
limits, including envelopes and escaped strings. The record validator reserves 8 KiB for
control data and budgets mutable counters at their maximum encoded widths so checkpoint
data cannot consume the space needed for recovery diagnostics.
SQLite persists the same canonical JSON number representation used by the portable boundary;
mathematically equivalent values such as `1.0` and `1` do not change immutable input identity.

A scan page can contain fewer records than its requested limit. Follow its continuation
cursor until `next` is absent; record count alone does not indicate the end of a scan.
Cursors bind the provider, filters and committed sequence. Concurrent writes can invalidate
a continuation, requiring a fresh consistent read. The scheduler retries transient read
contention without treating it as a storage outage. Oversized values fail explicitly with
`task_storage_too_large` before storage mutation. Values rejected before method admission
retain Tembra's portable transport errors. Checkpoints and child lists are never silently
truncated. Shared validation helpers live in
[`crates/task-contracts/src/transport.rs`](crates/task-contracts/src/transport.rs).

### Recovery and retention

Terminal records are immutable and retained indefinitely, including outcomes and ownership
relationships. Automatic deletion is not part of TaskStorage. Indexed active-state and
parent queries keep unrelated terminal history outside scheduling work; applications can
inspect retained history through bounded graph queries. Plan storage capacity for retained
data and backups. Portable revision and global commit counters are finite. Exhaustion refuses
further writes explicitly; counters never wrap or reset to bypass compare-and-swap checks.

Interrupted Running reservations are recovered when a new scheduler owner starts. A known
commit conflict after a handler has settled parks the task with its latest durable checkpoint
and an explicit diagnostic. Cancellation intent takes precedence over an uncommitted handler
decision. Before retrying a parked effect, reconcile the external operation using its stable
identity; a rejected local transition does not prove that the external effect did not happen.

## Getting started

Install [mise](https://mise.jdx.dev/), Rustup, Python 3.11 or later, Git and your platform's
native build tools. Run development commands from the repository root:

```sh
mise install
mise exec -- bun install --frozen-lockfile
mise exec -- just tembra-sdk
```

Tool versions are pinned in [`mise.toml`](mise.toml). The Tembra SDK is built from the same
immutable public revision as the Rust dependencies. Public builds require no private
repository checkout or hosted credentials.

```sh
# Website
mise exec -- just dev

# Shared web components
mise exec -- just storybook

# Mobile development
mise exec -- just dev-mobile
```

Application Profiles select plugins and their configuration. The runtime can inspect an
application configuration before deployment:

```sh
mise exec -- just rust-run -p swarmie-runtime -- resolve application.json
```

See the [composition types](crates/composition/src/model.rs) and
[application API](crates/runtime/src/application.rs) for configuration and embedding.

## Extending Swarmie

Define application capabilities as Tembra Facets with explicit Contracts. Use Registries for
lifecycle-owned contributions and Resources for provider connections.

Rust Contracts are canonical. The build generates TypeScript bindings under
`target/generated/contracts/`; generated bindings and executable artifacts stay outside
version control.

- [Plugin Contracts](crates/plugin-contracts/src/lib.rs)
- [TaskStorage Contract](crates/task-contracts/src/storage.rs)
- [Durable Tasks and handler Contracts](crates/task-contracts/src/tasks.rs)
- [TypeScript task registration](plugins/durable-tasks/typescript.ts)

## Repository layout

| Location | Contents |
| --- | --- |
| `crates/` | Runtime, composition infrastructure and public Contracts. |
| `plugins/` | Application capabilities and storage providers. |
| `apps/web/` | Website. |
| `apps/mobile/` | Mobile application. |
| `packages/ui/` | Shared web and native UI packages. |
| `packages/config/` | Shared frontend configuration, fonts and design tokens. |
| `assets/brandkit/` | Brand assets. |
| `tests/plugins/` | Plugin fixtures and Host integration tests. |

## Development checks

```sh
mise exec -- just check
```

This runs formatting, typechecks, lint, frontend tests, plugin generation, Rust workspace tests,
Clippy and Rustdoc checks. Additional recipes are listed by `mise exec -- just --list`.

Model inference integration, streaming ownership and provider configuration are described in
[the model integration guide](docs/models.md). Run `just models-check` for its focused checks.
