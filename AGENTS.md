# Agent instructions

## Contribution and preservation

Never merge a pull request. Merging belongs exclusively to the user. Do not replace remote
history, force push, delete remote branches or change repository visibility without explicit
authorization. Local history resets require verified preservation of the original tree,
history and pending user work. Preserve user-owned files and working directories.

Every commit message must follow Conventional Commits and be entirely lowercase, including
its type, scope, subject, body and footer. Use `type(scope): description` or
`type: description`, with a breaking-change marker when appropriate.

This project is in active development. Backward compatibility with previous development
clients, internal APIs, schemas and persisted formats is not required unless explicitly
requested. Update affected consumers and tests together; remove obsolete structures instead
of adding compatibility shims. This does not authorize data deletion or dropping explicitly
agreed external integration requirements.

Write persisted project prose, code comments and UI copy in English unless the user explicitly
requests a different language for that artifact. Existing localization catalogs keep their
supported languages. Present explanations to the user in Spanish. Keep device-specific
operations, deployment history and temporary reports outside the repository. Design reusable
interfaces and configurable adapters rather than hardcoding individual machines.

## Architecture and ownership

Tembra is the composition runtime for objects, facets, Contracts, Registries, References,
reactive State, subscriptions, hooks and lifecycle. Use its actual public APIs and artifacts;
do not implement parallel object protocols, registries, dependency containers or reload engines.
Pin and qualify the dependency revision before relying on an API.

The bootstrap initializes Tembra, registers compiled Core Plugins and starts plugin
administration and recovery. Keep it minimal. The Rust Plugin Manager is a Core Plugin and
administrative control plane, outside ordinary plugin invocation paths. Consumers use Tembra
References, Contracts and Registries directly. Provider connections belong to their objects
and use Tembra Resources when supported. SQLite holds durable administrative state; reactive
State is not durable storage.

Core Plugins are Rust compiled with Swarmie. They are not downloadable and do not hot reload
native code. Dynamic Plugins are trusted TypeScript or JavaScript artifacts loaded and
executed through Tembra. Compatible JavaScript updates use its real reload machinery.
Do not add native dynamic-library plugins, arbitrary native plugin binaries or WASM Components
as a plugin distribution format. Trusted plugin execution does not imply sandbox isolation.
Preserve administrative authorization, provenance, integrity, compatibility and honest errors.

Discover Plugin Source Providers through Tembra Contracts and Registries. The Plugin Manager
must not know individual registries, marketplaces or remote vendors. Reuse Tembra packaging
and loader contracts rather than creating a competing artifact format. UI contributions use
the actual Browser Host and browser facets; React is the renderer. Resolve React components
through module and export identity, never serialized functions. Keep server JavaScript
execution distinct from browser module loading and keep contributions tied to plugin lifecycle.
Installation and capability availability belong to the connected node, not every node.

Agent Loop, subagents, model providers, tools, memory, compaction, coordination, workflows,
a general storage plugin, a hosted marketplace and a full Desktop rewrite are outside the
foundation delivery. Do not introduce premature contracts or implementations for those features.

## Repository boundaries and documentation

This public repository owns clients, the independently installable runtime, public contracts,
shared UI, extension interfaces and distribution tooling. It must build without private
packages, hosted credentials, documentation contents or another repository checkout.
The independent private `swarmie-cloud` repository owns hosted identity, enrollment, relay
routing, allocation, collaboration authorization, synchronization, billing and entitlements.
It is not a submodule or workspace member. Public code must not import private implementation.

Technical documentation and task tracking live in the verified Swarmie Space. Do not recreate
the previous documentation application or a root `docs/` tree. Keep required licenses,
legal notices, agent policy and operational build inputs with their owners.

Describe Swarmie's own behavior without comparisons or inspiration references to other
projects. Check prose, hyperlinks, citations, examples and metadata. Keep necessary technical
dependencies and supported integration references, and preserve legal attribution and exact
historical source archives. Do not remove a working dependency merely to hide its name.

- [Current foundation and task specifications](https://chatgpt.com/space/page_eae8127333ac8191a1fcdfcea3236fa0).
- [Architecture overview](https://chatgpt.com/space/page_6ad2888a451481918a45bb8e60906c5f).
- [Historical public documentation](https://chatgpt.com/space/page_f906cd304fb48191b4c05bd472bdf04d).
- [Shared engineering and design references](https://chatgpt.com/space/page_2ca4e9eeb8f4819199773f9c68b1c204).

Refresh relevant Pages, their comments and review decisions before updates. Preserve human
edits and use current hashes and sequence guards. Review status is separate from execution.
Record owners, dependencies, blockers and acceptance evidence. A plan, open PR or passing
build alone does not complete a task. Historical notices distinguish previous designs from
current behavior; consult archived source only for deliberate reuse, without importing the
old runtime wholesale or keeping legacy copies in the active tree.

## Shared TypeScript and frontend rules

Start JavaScript and TypeScript workflows from this repository root. The root owns
`package.json`, `bun.lock`, `tsconfig.json`, `oxlint.config.ts`, `oxfmt.config.ts`,
`mise.toml` and `justfile`. Do not introduce application lockfiles, nested lint/format policy,
relative filesystem dependencies or automatic hosted-repository checkouts.

Before changing TypeScript, React, styling or frontend tooling, read the shared engineering,
design principles, visual system and brandkit references in Spaces. Their historical source
paths were `frontend.md`, `design-principles.md`, `DESIGN.md` and `brandkit.md`; apply the
current Tembra and repository ownership rules when those references describe retired components.
Runtime configuration, fonts, Tailwind setup and tokens belong in `packages/config` and
canonical brand assets in `assets/brandkit`. Builds must not depend on documentation metadata.

Use the repository-local [HeroUI React skill](.agents/skills/heroui-react/SKILL.md) for shared
DOM interfaces and the [HeroUI Native skill](.agents/skills/heroui-native/SKILL.md) for the
reversible native trial. HeroUI React is the web primitive base; the native choice remains
provisional. Reuse public `@swarmie/ui-web` and `@swarmie/ui-native` exports before adding
components. Keep platform dependencies, compiler projects and Storybooks separate.

The website belongs in `apps/web`; Mobile belongs in `apps/mobile`. Bun shared packages
belong in `packages/`. The retired language SDKs, native bindings and WIT boundary have been
removed. The source-built Tembra SDK is a required implementation dependency. The standalone
TypeScript contracts and plugin UI packages and the browser acceptance application have been
retired. Rust contracts remain authoritative; generated TypeScript bindings are reproducible,
ignored build outputs under `target/generated/contracts/`, consumed by the named plugin examples
and acceptance fixtures. Mobile retains account onboarding
and native UI, with old runtime-dependent views removed and connectivity unavailable;
do not restore the retired transport or advertise native builds or live connectivity before
a new client integration is qualified. Do not recreate Tembra's transport engine in each language.

Use precise types, validated external boundaries, discriminated workflow states, explicit
failures and narrow adapters. Preserve accessibility, localization, reduced motion and
approved shared control geometry. Do not suppress rejected promises or broad compiler/lint
warnings. Keep tokens and neutral primitives independent of transports and hosted authority.
Run root formatting, typechecks, lint and affected behavior tests; qualify retained builds
and both Storybooks when shared frontend contracts change. Browser checks do not qualify
native device behavior.

## Rust and verification

Use the root toolchain in `mise.toml`, formatting in `rustfmt.toml`, recipes in `justfile`
and changelog policy in `cliff.toml`. Keep Cargo manifests with their owners. The Rust workspace
contains new foundation crates; align recipes with the qualified Tembra toolchain and actual members.
Do not copy configuration into application or package directories.

Write Rust documentation in English and describe meaningful invariants and failure conditions.
Keep `lib.rs` focused on public entries and separate adjacent declarations with a blank line.
Keep unit tests inline and integration tests in each crate's `tests/` directory. Verify changed
Rust with formatting, Clippy, relevant behavior tests and Rustdoc link checks. Report genuine
exceptions instead of suppressing warnings across a crate. Run macOS-specific operations on
the configured macOS host through SSH. Do not add a `.vscode/` directory.
