# Development

Start commands from the repository root. Tool versions are pinned in `mise.toml`;
the root owns Cargo/Bun lockfiles, formatting, linting and compiler configuration.

## Native libraries

Install the pinned Rust toolchain with Clippy and rustfmt, and the pinned Just.
The Rust workspace builds without frontend dependencies or documentation contents.

On Linux, the client node uses the Secret Service credential store. Install the
D-Bus development files and `pkg-config` before you build the workspace.
On Debian and Ubuntu, use:

```sh
sudo apt-get update
sudo apt-get install -y libdbus-1-dev pkg-config
```

```sh
just rust-check
just rust-source-check
just foundation-check
just storage-check
just agent-loop-check
just model-provider-check
just tools-check
```

The foundation recipe also qualifies the optional Agent Loop adapters and AI
provider feature combinations, plus Storage with and without its default SQLite backend. Individual cases can run through `just rust-test`.
Use `just rust-lock-workspace` when changing workspace membership while retaining
unrelated dependency versions.

Source policy checks run in the unpublished `xtask` package through the root Just
recipes. `just rust-source-check` checks workspace lint inheritance and the synchronous
provider-codec boundary. `just rust-check` includes these checks before Clippy and tests.
Use `just rust-xtask --help` to list the individual source checks. Run their behavior
tests with `just rust-test -p xtask`. The checks require no provider credentials or
external service. See [Rust source checks](docs/rust-source-checks.md) for their scope.

## TypeScript SDK and clients

Cargo resolves Tembra from the exact Git revision in the root manifest. Bun consumes
the pinned SDK and native/browser engines from npm. Root lockfiles fix their resolved artifacts;
no upstream checkout, local package link or SDK source build is needed.

```sh
mise install
mise exec -- bun install --frozen-lockfile
mise exec -- just sdk-check
mise exec -- bun run check
```

`sdk-check` builds synthetic orbs through the published SDK tooling and executes one
through its installed native engine. `just tembra-browser` exports the installed
browser engine under `target/browser-wasm`; it does not compile an upstream checkout.

Use `just dev` for the website, `just dev-mobile` for mobile development, and
`just storybook` or `bun run storybook:native` for the separate component catalogs.
Root `just build`, `just mobile-export` and `just build-storybook` build those clients
and catalogs. These checks do not qualify device-specific native runtime behavior.

## Repository layout

- `crates/runtime/`: the embeddable runtime.
- `crates/storage/`: the shared storage API and feature-gated `src/sqlite/` implementation.
- `crates/storage/core/`: internal contracts used by independent capabilities.
- `crates/model-catalog/`: independent model metadata and preparation.
- `plugins/providers/`: shared model/provider types and its browser presentation.
- `plugins/`: independent capability libraries and their optional Tembra orb composition.
- `tests/fixtures/`: synthetic behavior and SDK authoring fixtures.
- `apps/web-client`: the independently composed node client.
- `apps/mobile-client`: the mobile client.
- `apps/desktop-client`: the Native SDK desktop host for the shared web client.
- `apps/web`: the website.
- `packages/config`, `packages/ui/web` and `packages/ui/native`: shared configuration
  and separate platform component libraries.
- `vendor/`: the licensed provider dependency snapshot and its recorded patch.
- `docs/runtime.md`: runtime composition and lifecycle.
- `docs/plugins/`: independent capability guides.
- `docs/client-runtime/`: client and frontend guides.

## Capability guides

Start with the [concept index](docs/index.md).

- [Runtime and orb administration](docs/runtime.md)
- [Storage and durable execution](docs/plugins/durable-tasks.md)
- [Independent Agent Loop and lifecycle hooks](docs/plugins/agent-loop.md)
- [Tool registration and provider routing](docs/plugins/tools.md)
- [MCP connections and tool execution](docs/plugins/mcp.md)
- [Providers and the AI adapter](docs/plugins/providers.md)
- [Desktop development and packaging](docs/client-runtime/desktop.md)

Durable Tasks and Conversations remain optional Agent Loop dependencies. The
normal library does not require PluginManager, a task scheduler, a conversation
store or a concrete model provider. External model/tool effects need provider-owned
reconciliation; an atomic local commit does not imply exactly-once external effects.
