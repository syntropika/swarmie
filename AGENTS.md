# Agent instructions

## Development stage and compatibility

This project is in active development. Backward compatibility with earlier versions of the application's clients, internal APIs, database schemas, configuration, and persisted formats is not required unless the user explicitly requests it for a specific change.

Prefer updating the current design directly and adjusting all affected consumers, documentation, and tests together. Remove obsolete interfaces and structures rather than adding compatibility shims, dual implementations, or historical-version branches.

Database schema changes may target a fresh development installation. Historical database migrations and importers are not required solely to preserve earlier development versions.

Keep support for external Harnesses and integrations within the explicitly agreed support scope. This development policy does not remove those integration requirements.

Existing user files and working directories retain their ownership and lifecycle rules. A fresh-schema design does not authorize deleting existing data.

## Repository boundaries and documentation

The parent repository contains clients, the independently installable runtime, Harness adapters,
local persistence, extension interfaces, public wire contracts, tests, and distribution tools.
It must build and run without the hosted-service repository, documentation contents, or hosted credentials.

This repository owns the TypeScript website, desktop and mobile clients, shared UI,
and Storybooks. The independent `swarmie-cloud` repository contains the Rust hosted services.
It is not a submodule or workspace member of this repository. Account identity,
account-linked enrollment, hosted relay routing, cloud allocation, collaboration authorization,
hosted synchronization, billing, and commercial entitlements belong to that repository.
A private implementation may consume public contracts; public code must not depend on private packages.

Keep all project documentation in the root `docs/` folder, tracked directly in this repository.
The root `AGENTS.md` is the canonical agent policy; `docs/AGENTS.md` links to it.
There is no separate documentation repository, checkout, or submodule. The independent
hosted-service repository does not mount another copy. Shared design and engineering guides
live directly in `docs/`. Runtime, desktop, mobile, SDK, extension and testing references
use direct category folders; hosted-service documentation is in `docs/cloud/`. Keep technical
README files, specifications, plans, and generated API documentation under `docs/`, outside source
package directories and distribution packages. Source checkouts must build without documentation
contents.

Documentation uses English. Describe accepted behavior and open decisions without naming
external comparison products. Preserve user-owned files and working directories.

Both source repositories are private. Do not change visibility without explicit user authorization.
Never merge a pull request. Every commit message must follow Conventional Commits and be entirely lowercase.

## Shared TypeScript and frontend rules

Start JavaScript and TypeScript workflows from the parent repository root. The root owns
`package.json`, `bun.lock`, `tsconfig.json`, `oxlint.config.ts`, and `oxfmt.config.ts`.
Read [frontend engineering rules](docs/frontend.md) and [Swarmie design principles](docs/design-principles.md)
from `docs/` at the source repository root before changing TypeScript, React,
shared UI, styling, or frontend tooling. Also read the canonical [shared visual system](docs/DESIGN.md)
and [brandkit guide](docs/brandkit.md). Design metadata belongs in
`docs/.impeccable/design.json`, not in root or application-local folders.
Impeccable's automatic discovery may not follow this documentation layout: read the explicit
paths instead of generating duplicate DESIGN files or root `.impeccable` metadata.

Use the repository-local [HeroUI Native skill](.agents/skills/heroui-native/SKILL.md)
for the wired, reversible native UI trial. Use the separate [HeroUI React skill](.agents/skills/heroui-react/SKILL.md)
for shared web and desktop work. Keep each skill scoped to its platform: HeroUI React
is the shared DOM primitive base, while the native library choice remains provisional.
Apply them alongside the frontend rules and design principles above.

Language clients and SDK generation/packaging tools belong in `sdk/`. Bun-oriented shared
packages and the executable runtime package belong in `packages/`. The TypeScript SDK
participates in the root Bun workspace from `sdk/typescript`.

Native SDK transport, RPC validation, deadlines, cancellation, and event behavior belong
to `crates/client`. Node-API and the C ABI are thin bindings. Keep the native boundary in
`sdk/wit/client.wit`; its generated Rust types and dispatch must match implementation.
Do not recreate transport engines in Node, Python, Go, or Swift. Browser-only HTTP stays
in the explicit browser export. Build and verify native SDKs through the root `justfile`;
build Apple artifacts and run Swift qualification on a configured macOS host.

Shared compiler settings, fonts, Tailwind setup, and theme tokens belong in `packages/config`.
Neutral web components and stories belong in `packages/ui/web` (`@swarmie/ui-web`);
native components and stories belong in `packages/ui/native` (`@swarmie/ui-native`).
Keep their dependencies, compiler projects, and Storybooks separate. Reuse their public
exports before adding another implementation. The website belongs in `apps/web`. Native
account transport and credential ownership belong to the Rust account capability; clients
render its sanitized observations. Hosted authority and server implementation remain in
the independent hosted-service repository. Do not retain unused browser account packages
or introduce a parallel account transport without an actual consumer.
Do not recreate package-local lint or formatting configurations or application lockfiles.
Each source repository owns its dependency graph and root checks. Do not add relative
filesystem dependencies, parent-root wrappers, or automatic hosted-repository checkouts.

## Shared Rust and development configuration

Use the parent root `rustfmt.toml` with the toolchain pinned in `mise.toml`. Run development,
formatting, checking, testing, packaging, and changelog recipes through the root `justfile`.
Do not duplicate `rustfmt.toml`, `mise.toml`, `justfile`, or `cliff.toml`
inside applications or packages. Keep Cargo manifests and application-specific build inputs
beside their owners. Hosted runtime profiles, Docker, Compose, deployment policy, and secret
configuration belong to the independent hosted repository.

Write Rust documentation in English, explain meaningful invariants and failure conditions,
keep `lib.rs` focused on public entries, and separate adjacent declarations with a blank line.
Keep unit tests inline and integration tests in the crate's `tests/` directory. Do not create
source directories solely to hold test files. Verify changed Rust with formatting, Clippy,
relevant behavior tests, and Rustdoc link checks; document exceptions rather than suppressing
new warnings across a crate.

Do not add a `.vscode/` directory to this repository.
