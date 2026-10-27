# Development

Start commands from the repository root. Tool versions are pinned in `mise.toml`;
the root owns Cargo/Bun lockfiles, formatting, linting and compiler configuration.

## Native libraries

Install the pinned Rust toolchain with Clippy and rustfmt, and the pinned Just.
The Rust workspace builds without frontend dependencies or documentation contents.

```sh
just rust-check
just foundation-check
just storage-check
just agent-loop-check
just model-provider-check
just tools-check
```

The foundation recipe also qualifies the optional Agent Loop adapters and genai
provider feature combinations, plus Storage with and without its default SQLite backend. Individual cases can run through `just rust-test`.
Use `just rust-lock-workspace` when changing workspace membership while retaining
unrelated dependency versions.

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

- `runtime/`: the embeddable runtime.
- `storage/`: the shared storage API and feature-gated `src/sqlite/` implementation.
- `storage/core/`: internal contracts used by independent capabilities.
- `plugins/providers/`: shared model/provider types and its browser presentation.
- `plugins/`: independent capability libraries and their optional Tembra orb composition.
- `tests/fixtures/`: synthetic behavior and SDK authoring fixtures.
- `apps/web-client`: the independently composed node client.
- `apps/mobile-client`: the mobile client.
- `apps/web`: the website.
- `packages/config`, `packages/ui/web` and `packages/ui/native`: shared configuration
  and separate platform component libraries.
- `vendor/`: the licensed provider dependency snapshot and its recorded patch.
- `docs/client-runtime/`: technical capability guides.

## Capability guides

Start with the [concept index](docs/client-runtime/index.md).

- [Runtime and orb administration](docs/client-runtime/runtime-foundation.md)
- [Storage and durable execution](docs/client-runtime/durable-tasks.md)
- [Independent Agent Loop and lifecycle hooks](docs/client-runtime/agent-loop.md)
- [Tool registration and provider routing](docs/client-runtime/tools.md)
- [Providers and the genai adapter](docs/client-runtime/providers.md)

Durable Tasks and Conversations remain optional Agent Loop dependencies. The
normal library does not require PluginManager, a task scheduler, a conversation
store or a concrete model provider. External model/tool effects need provider-owned
reconciliation; an atomic local commit does not imply exactly-once external effects.
