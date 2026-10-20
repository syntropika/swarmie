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
