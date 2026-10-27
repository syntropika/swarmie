# Agent instructions

## Contribution and preservation

Use task branches and worktrees. Reuse suitable existing work before creating another
checkout, and preserve unrelated changes and user-owned files. Never merge a pull request.
Create a pull request only when the user explicitly requests one. Do not rewrite remote
history, force push, remove branches or change repository visibility without authorization.
Both this repository and the independent hosted-service repository are private.

Every commit message must use Conventional Commits and be entirely lowercase, including
its type, scope, subject, body and footer. Use `type(scope): description` or `type: description`.

This project is in active development. Compatibility with previous development clients,
internal APIs, schemas and formats is not required unless explicitly requested. Update
consumers and tests together and remove obsolete structures instead of adding compatibility
shims. This does not authorize deleting user data or dropping agreed integration support.

Write persisted project prose, comments and UI copy in English unless the user explicitly
requests another language for that artifact. Existing localization catalogs retain their
languages. Explain work to the user in Spanish. Keep research, measurements, temporary
reports and local deployment scripts outside the repository. Avoid persisting device-specific
paths, addresses, hostnames or operational history in reusable project code and documentation.

## Architecture and ownership

Tembra owns composition, objects, Orbs, References, State, hooks, subscriptions and physical
lifecycle. Use its actual public APIs at the pinned qualified revision. Do not build parallel
object protocols, containers, registries, hook dispatchers, reload engines or cleanup runtimes.
Use Orb and orb/orbs consistently; the source folder for capability crates is `plugins/`.
PluginManager is the administrative capability, outside ordinary invocation paths.

The runtime is an embeddable Rust library. Its host supplies execution and I/O services;
there is no standalone CLI or mandatory Desktop shell. Keep bootstrap minimal. Core Orbs
are Rust compiled with the application; do not introduce native dynamic-library plugins.
Trusted execution does not imply sandbox isolation or automatic remote availability.

Capability libraries are independently usable. Their Tembra orb composition must not force
consumers to use PluginManager. Agent Loop has one engine, with optional Durable Tasks and
Conversations adapters. Normal execution does not require a scheduler or history store.
Tools owns tool metadata and provider routing; Tembra owns the actual services and
references. Catalog membership never grants provider authority. Confirm routed provider
identity with pending tool arguments, and reconcile before permitting a retry.
Shared model values and provider interfaces belong in `plugins/providers`; concrete model
adapters belong in their own provider crate. Keep test mocks behind explicit features.

Storage is a backend-neutral resource. The storage package enables its `sqlite` feature by default;
disable default features to supply another backend. Keep the neutral contracts independent
of the concrete adapter and consuming capabilities. The SQLite adapter owns its connections and sets
WAL plus synchronous=NORMAL on every connection. Only compatible writes issued by the
same storage owner can share an atomic commit. Do not infer cross-store atomicity or
power-loss durability. Durable Tasks owns assignment, attempts and checkpoints; the resource
consumer owns recognition, receipts and reconciliation of external effects. Unknown outcomes
must not permit automatic replay. Cancellation cannot undo an external effect.

Hooks use typed values and Tembra contribution plans. Confirm effective model requests and
tool arguments before dispatch. Request projections do not silently rewrite canonical history.
Terminal observation uses the physically owned source and must preserve original cancellation,
identity and versioning without reviving storage scopes. Observation errors cannot replace
confirmed outcomes or grant replay authority. Do not claim exactly-once hook effects.

Keep subagents, collaboration, compaction, memory, hosted synchronization and Desktop embedding
within their explicitly agreed scopes. Extension points do not imply those features are already
implemented or qualified. Do not restore retired interfaces as speculative compatibility layers.

## Repository and documentation boundaries

This repository owns clients, reusable runtime capabilities, extension interfaces, local
persistence, shared UI and distribution tools. It must build without the hosted-service
repository, hosted credentials or documentation contents. The independent `swarmie-cloud`
repository owns hosted identity, enrollment, relay routing, allocations, collaboration authority,
hosted synchronization, billing and entitlements. It is not a submodule or workspace member.
Do not add relative cross-repository dependencies or automatic hosted checkouts.

Keep technical project documentation under root `docs/`, with client/runtime guides in
`docs/client-runtime/`. Do not create package-local technical READMEs or documentation trees.
Keep root README focused on purpose and capabilities, and setup/commands in DEVELOPMENT.md.
Describe accepted behavior and open limits without external comparison products. Preserve
licenses, third-party notices and source provenance with the dependencies they describe.

Architecture task Pages in the verified Swarmie Space remain authoritative for scope, human
review, execution state and evidence. Refresh Pages and comments before guarded changes,
preserve concurrent edits and synchronize the external local tracking mirror. Keep private
tracking and operational evidence outside source control. Do not mark work done merely from
a plan, open PR or passing build; distinguish human review from implementation qualification.

## TypeScript and clients

Start workflows from the repository root. The root owns package.json, bun.lock, tsconfig.json,
oxlint.config.ts, oxfmt.config.ts, mise.toml and justfile. Do not add package-local lockfiles,
lint/format policy or duplicate toolchains. Use the pinned Bun and frozen installation.

Shared compiler settings, fonts, Tailwind setup and tokens belong in `packages/config`.
Keep `packages/ui/web` and `packages/ui/native` dependencies, projects and Storybooks separate.
Reuse public shared components. Native vendor selection remains provisional. Preserve
accessibility, localization and reduced motion.
The website belongs in `apps/web`, the web client in `apps/web-client`, and the mobile client
in `apps/mobile-client`. Keep hosted authority outside clients.
The desktop host belongs in `apps/desktop-client` and reuses `apps/web-client`. Keep its
identity and icon in the Native SDK manifest; do not duplicate presentation or runtime state.

Use strict TypeScript, precise discriminated values, type-only imports and owning-boundary
validation. Do not replace rejected promises or unsafe casts with lint suppressions. Check
formatting, types, lint and affected behavior; qualify clients and catalogs when their shared
contracts change. A browser-rendered native story does not qualify native device behavior.

Consume Tembra from pinned crates.io and npm releases, with the root lockfiles owning
resolution and integrity. Do not restore local SDK links, upstream source builds or a
copied/generated SDK package as installation workarounds.
Keep synthetic authoring fixtures under `tests/fixtures` and generated outputs under target.
Do not recreate a generic Contract generator or packaging layer without a concrete requirement.

## Rust and verification

Use the root pinned toolchain, rustfmt.toml and justfile. Keep manifests with their owners;
do not duplicate formatting, toolchain or changelog configuration. Infrastructure belongs in
`runtime/`; storage lives in root `storage/`, with neutral contracts in `storage/core/`.
Capability libraries belong in `plugins/`, and synthetic fixtures in `tests/fixtures/`.
Do not add top-level example applications or an editor-specific `.vscode/` directory.

Keep lib.rs focused on public exports. Use typed domain/control values and accurate public
contracts; document meaningful invariants, cancellation and failure conditions in English.
Separate adjacent declarations. Keep unit tests inline and integration tests in each crate's
`tests/` directory; do not create source directories solely to hold tests. Run root formatting,
strict Clippy, relevant behavior tests and Rustdoc link checks. Report real exceptions rather
than suppressing new warnings. Run platform-specific checks on a configured compatible host.
