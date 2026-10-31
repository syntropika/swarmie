# Agent instructions

## Contribution and preservation

Use task branches and worktrees. Reuse suitable existing work before creating another
checkout, and preserve unrelated changes and user-owned files. Never merge a pull request.
Create a pull request only when the user explicitly requests one. Do not rewrite remote
history, force push, remove branches or change repository visibility without authorization.
This repository is public. The independent `swarmie-cloud` repository is private.

Every commit message must use Conventional Commits and be entirely lowercase, including
its type, scope, subject, body and footer. Use `type(scope): description` or `type: description`.

This project is in active development. Compatibility with previous development clients,
internal APIs, schemas and formats is not required unless explicitly requested. Update
consumers and tests together and remove obsolete structures instead of adding compatibility
shims. This does not authorize deleting user data or dropping agreed integration support.

Write persisted project prose, comments and UI copy in English unless the user explicitly
requests another language for that artifact. Existing localization catalogs retain their
languages. Keep research, measurements, temporary reports and local deployment scripts
outside the repository. Avoid persisting device-specific paths, addresses, hostnames or
operational history in reusable project code and documentation.

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
Model contracts, authentication, submission protection and native AI integration belong in
`plugins/providers`. Keep their responsibilities in internal modules. Keep test mocks behind
explicit features.

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

Keep technical project documentation under root `docs/`: runtime composition in
`docs/runtime.md`, capability guides in `docs/plugins/` and client guides in
`docs/client-runtime/`. Keep the guide index in `docs/index.md`. Do not create package-local
technical READMEs or documentation trees.
Keep root README focused on purpose and capabilities, and setup/commands in DEVELOPMENT.md.

### Documentation content and language

Explain how Swarmie works in all project documentation. Describe the current behavior,
public interfaces, configuration, procedures, ownership and limits. Check these descriptions
against the current source code. Keep decision history, design rationale, alternatives,
product comparisons and future plans outside the source repository, in research or planning
artifacts. Do not present proposed behavior as implemented behavior.

Use [ASD-STE100 Simplified Technical English](https://www.asd-ste100.org/about_STE.html)
for all English documentation. Follow its writing rules and approved dictionary. Use the
permitted project technical nouns and verbs consistently. Keep API names, identifiers and
code examples exact. Write short, complete sentences. Use one instruction in each procedural
sentence. Check the language and terminology before you finish a documentation change.

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
do not duplicate formatting, toolchain or changelog configuration. General Rust libraries belong in `crates/`, with short directory names. The embeddable
runtime lives in `crates/runtime/`, storage in `crates/storage/`, and neutral storage
contracts in `crates/storage/core/`. The model catalog, source generation and operational
cache belong inside `crates/ai/`. Capability libraries stay in `plugins/`, and synthetic
fixtures stay in `tests/fixtures/`.
Do not add top-level example applications or an editor-specific `.vscode/` directory.

Keep lib.rs focused on public exports. Use typed domain/control values and accurate public
contracts; document meaningful invariants, cancellation and failure conditions in English.
Separate adjacent declarations. Keep unit tests inline and integration tests in each crate's
`tests/` directory; do not create source directories solely to hold tests. Run root formatting,
strict Clippy, relevant behavior tests and Rustdoc link checks. Report real exceptions rather
than suppressing new warnings. Run platform-specific checks on a configured compatible host.

### Investigation and changes

Read the owning implementation and its manifest before changing a public interface.
Check actual signatures and feature flags before writing examples. Search the owning
module first and read complete relevant constructs. Reuse unchanged source already in
context. Expand the search when a dependency or caller requires it. Keep commands on
the intended worktree. An empty search outside the indexed workspace does not prove
that an interface has no callers.

Reuse existing domain types, errors and extension points. Do not add placeholder APIs,
speculative abstractions or silent fallback. Refuse unsupported behavior before an
external effect. Preserve confirmed results when observation or projection fails.

### Rust errors and control flow

Use typed errors for new fallible APIs. Do not use `String` as the public error type.
Use `?` and meaningful error conversions. New production paths must handle absent or
invalid data without `unwrap`, `expect`, `panic!` or `unreachable!`. Use checked indexing
and conversions for external values. Express required invariants in validated types.
Do not replace an error with a guessed default or a different account or provider.

Release lock and RefCell guards before `.await`. Keep the root Clippy warning gate
strict for all targets. Do not enable an entire restriction group or add a broad
allowance to make a check pass. Explain a necessary exception on the smallest item.
Do not add a crate-level `cfg(test)` allowance that exempts production declarations.
Keep existing unit tests inline. Tests that return `Result` must propagate failures;
use ordinary test functions when assertions describe the expected result.

The root enables async lock restrictions and the existing placeholder/debug lints.
The unpublished `xtask` also denies unwrap, expect, indexing and panic-related lints.
Other crates do not yet enforce that whole set. Follow these production rules when
changing them, without claiming that historical code has passed a full migration.

### Provider codecs

Provider codecs under `crates/ai/src/codecs/providers/` are synchronous data
transformations. Keep transport, task spawning, filesystem access, credential lookup
and renewal outside that boundary. Explicit credentials and physical execution retain
their existing owners. Compile-time platform constants are allowed. Qualified dialects
share their existing codecs and use extension hooks for actual profile differences.
Preserve native fields and reject an option that the selected route cannot enforce.

### Verification tools

Use `just rust-test -p <package> <filter>` for focused iteration. Before completing a
change, run the relevant package and feature checks, strict Clippy, root formatting and
Rustdoc link checks. Check affected consumers when a shared contract changes. Distinguish
local fixtures from real account, platform and composed-runtime qualification.

Use `just rust-source-check` for workspace lint inheritance and provider boundary checks.
These checks parse Rust source, including non-test platform branches and macro tokens.
They permit explicit `#[cfg(test)]` items, including inline tests. They are syntactic
guards; review indirect calls, aliases and dependencies separately. Do not treat them as
an effect sandbox. Add source-policy tooling to the unpublished `xtask`, with behavior
tests, and invoke it through the root `justfile`. Keep one toolchain and command owner.
